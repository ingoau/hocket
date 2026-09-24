//! Headless Hocket coordinator: relay and replica for Connect sessions.
//! Library half; `main.rs` parses arguments and serves.
//!
//! The fourth build target of the shared core. It runs one
//! [`hocket_core::connect::Room`] per scope (server + user) over an axum
//! WebSocket endpoint, persists each room's replica to `--data-dir`, and
//! verifies every client by proxying a Subsonic `ping` with the credential the
//! client presented to the server the client named. It never stores a
//! credential, never plays audio, and is never the authority: it orders and
//! relays what the devices decide.
//!
//! ```text
//! hocket-coordinator --listen 0.0.0.0:7373 --data-dir /var/lib/hocket
//!   GET /health          → 200 {"status":"ok","rooms":n}
//!   GET /ws              → WebSocket, first frame must be Hello
//! ```
//!
//! Hardening, since everything before `Welcome` is unauthenticated:
//!
//! - The proxied ping goes to `--verify-url` when set; otherwise the server
//!   the client names must be plain `http(s)`, without userinfo, and resolve
//!   only to public addresses (`--allow-private-servers` lifts that for
//!   self-hosted LANs). Redirects are never followed, the response body is
//!   capped, and the request is pinned to the addresses that were checked.
//! - A room is written to disk only once it has admitted a member; refused
//!   sockets leave nothing behind, and a socket that never said `Hello`
//!   never opens a room.
//! - Frames are capped at [`MAX_FRAME_BYTES`], a socket must speak within
//!   [`HELLO_TIMEOUT`] and keep speaking within [`READ_TIMEOUT`], the room's
//!   `Close` ends the reader at once, and connections are capped in total,
//!   per client address, and per client address per minute.
//! - Replica writes happen off the rooms lock on the blocking pool, with
//!   changes coalesced within [`SAVE_DEBOUNCE`] and heartbeat-only changes
//!   (a lease `expiresAt` moving) written at most every
//!   [`TOUCH_SAVE_INTERVAL`].

use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::extract::connect_info::ConnectInfo;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::get;
use axum::Router;
use clap::Parser;
use futures::{SinkExt, StreamExt};
use parking_lot::Mutex;
use tokio::sync::mpsc;
use tracing::{debug, info, warn};

use hocket_core::connect::auth::ip_is_private;
use hocket_core::connect::replica::{
    FileReplicaStore, MemoryReplicaStore, ReplicaError, ReplicaStore,
};
use hocket_core::connect::room::{Room, RoomConfig, RoomInput, RoomOutput};
use hocket_core::connect::session_adapter::RealReducer;
use hocket_core::connect::wire::{Credential, Msg, RefuseReason, ReplicaState, WireMessage};
use hocket_core::connect::PeerId;
use hocket_core::util::WallClock;

pub use hocket_core::connect::transport::MAX_FRAME_BYTES;

/// A socket that has not sent its `Hello` within this long is dropped.
pub const HELLO_TIMEOUT: Duration = Duration::from_secs(10);
/// A member that sends nothing for this long is dropped (the room times
/// members out at 30 s; this is the transport-level backstop).
pub const READ_TIMEOUT: Duration = Duration::from_secs(60);
/// The verification ping's whole budget.
pub const VERIFY_TIMEOUT: Duration = Duration::from_secs(10);
/// Largest ping response body read (a Subsonic ping is a few hundred bytes).
pub const VERIFY_BODY_CAP: usize = 64 * 1024;
/// Replica changes are written at most this often per room.
pub const SAVE_DEBOUNCE: Duration = Duration::from_secs(1);
/// Heartbeat-only changes are written at most this often per room.
pub const TOUCH_SAVE_INTERVAL: Duration = Duration::from_secs(30);

#[derive(Parser, Debug, Clone)]
#[command(
    name = "hocket-coordinator",
    about = "Headless Hocket coordinator: relay + replica, no credentials, no audio."
)]
pub struct Args {
    /// Address to listen on.
    #[arg(long, default_value = "0.0.0.0:7373")]
    pub listen: SocketAddr,
    /// Directory for replica persistence. Omit for in-memory only.
    #[arg(long)]
    pub data_dir: Option<PathBuf>,
    /// Verify every credential against this Subsonic base URL instead of the
    /// one the client states (single-server deployments).
    #[arg(long)]
    pub verify_url: Option<String>,
    /// Skip credential verification entirely (tests, trusted networks).
    #[arg(long, default_value_t = false)]
    pub no_verify: bool,
    /// Max members per room.
    #[arg(long, default_value_t = 32)]
    pub max_members: usize,
    /// Let clients name a Subsonic server on a private, loopback or
    /// link-local address for verification (self-hosted LANs). Without
    /// `--verify-url` the default is to refuse those: an unauthenticated
    /// client must not be able to make this host probe its own network.
    #[arg(long, default_value_t = false)]
    pub allow_private_servers: bool,
    /// Max concurrent WebSocket connections.
    #[arg(long, default_value_t = 4096)]
    pub max_connections: usize,
    /// Max concurrent WebSocket connections per client address.
    #[arg(long, default_value_t = 32)]
    pub per_ip_connections: usize,
    /// New WebSocket connections per minute per client address.
    #[arg(long, default_value_t = 60)]
    pub per_ip_rate: u32,
}

impl Args {
    /// Defaults for embedding (tests): in-memory, verified, loopback servers allowed.
    pub fn for_tests() -> Args {
        Args {
            listen: "127.0.0.1:0".parse().expect("literal"),
            data_dir: None,
            verify_url: None,
            no_verify: false,
            max_members: 8,
            allow_private_servers: true,
            max_connections: 4096,
            per_ip_connections: 1024,
            per_ip_rate: 1_000_000,
        }
    }
}

/// A live room and the sockets attached to it.
struct RoomState {
    room: Room,
    /// Outgoing frame channel per socket.
    senders: HashMap<PeerId, mpsc::UnboundedSender<Message>>,
    /// Something worth persisting happened since the last save.
    dirty: bool,
    /// Only a heartbeat moved since the last save.
    touched: bool,
    last_saved: Instant,
}

impl RoomState {
    /// Whether a write is due now, and clear the flags if so.
    fn take_save_due(&mut self, now: Instant, closing: bool) -> bool {
        if !self.room.ever_admitted() {
            self.dirty = false;
            self.touched = false;
            return false;
        }
        let due = if closing {
            self.dirty || self.touched
        } else {
            (self.dirty && now.duration_since(self.last_saved) >= SAVE_DEBOUNCE)
                || (self.touched && now.duration_since(self.last_saved) >= TOUCH_SAVE_INTERVAL)
        };
        if due {
            self.dirty = false;
            self.touched = false;
            self.last_saved = now;
        }
        due
    }
}

/// Per-client-address admission: a token bucket for new connections and a
/// count of live ones.
#[derive(Default)]
struct ClientLimits {
    live: usize,
    tokens: f64,
    last_refill: Option<Instant>,
}

#[derive(Default)]
struct Admission {
    per_ip: HashMap<IpAddr, ClientLimits>,
    live: usize,
}

/// Why a connection was not admitted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refused {
    TooManyConnections,
    TooManyFromAddress,
    RateLimited,
}

impl Admission {
    fn try_admit(&mut self, ip: IpAddr, args: &Args, now: Instant) -> Result<(), Refused> {
        if self.live >= args.max_connections {
            return Err(Refused::TooManyConnections);
        }
        let per_minute = f64::from(args.per_ip_rate.max(1));
        let entry = self.per_ip.entry(ip).or_insert_with(|| ClientLimits {
            live: 0,
            tokens: per_minute,
            last_refill: None,
        });
        if let Some(last) = entry.last_refill {
            let elapsed = now.duration_since(last).as_secs_f64();
            entry.tokens = (entry.tokens + elapsed * per_minute / 60.0).min(per_minute);
        }
        entry.last_refill = Some(now);
        if entry.live >= args.per_ip_connections {
            return Err(Refused::TooManyFromAddress);
        }
        if entry.tokens < 1.0 {
            return Err(Refused::RateLimited);
        }
        entry.tokens -= 1.0;
        entry.live += 1;
        self.live += 1;
        Ok(())
    }

    fn release(&mut self, ip: IpAddr) {
        self.live = self.live.saturating_sub(1);
        if let Some(e) = self.per_ip.get_mut(&ip) {
            e.live = e.live.saturating_sub(1);
        }
        // Forget idle addresses so the map never grows with the internet
        // (an address seen more than a minute ago has a full bucket again).
        if self.per_ip.len() > 4096 {
            self.per_ip.retain(|_, e| {
                e.live > 0
                    || e.last_refill
                        .map(|t| t.elapsed() < Duration::from_secs(60))
                        .unwrap_or(false)
            });
        }
    }
}

/// Process-wide state shared by every connection.
pub struct App {
    args: Args,
    rooms: Mutex<HashMap<String, RoomState>>,
    store: Arc<dyn ReplicaStore>,
    http: reqwest::Client,
    peer_seq: AtomicU64,
    connections: AtomicU64,
    admission: Mutex<Admission>,
    /// Verification pings performed (tests assert on it).
    verifications: AtomicU64,
}

impl App {
    pub fn new(args: Args) -> anyhow::Result<Arc<App>> {
        let store: Arc<dyn ReplicaStore> = match &args.data_dir {
            Some(dir) => Arc::new(FileReplicaStore::new(dir)?),
            None => Arc::new(MemoryReplicaStore::new()),
        };
        let http = http_client(None)?;
        Ok(Arc::new(App {
            args,
            rooms: Mutex::new(HashMap::new()),
            store,
            http,
            peer_seq: AtomicU64::new(0),
            connections: AtomicU64::new(0),
            admission: Mutex::new(Admission::default()),
            verifications: AtomicU64::new(0),
        }))
    }

    fn next_peer(&self) -> PeerId {
        format!("ws-{}", self.peer_seq.fetch_add(1, Ordering::Relaxed) + 1)
    }

    /// Verification pings performed so far.
    pub fn verification_count(&self) -> u64 {
        self.verifications.load(Ordering::Relaxed)
    }

    /// Verify a credential by proxying a Subsonic `ping`. The credential is
    /// used for this one request and dropped. See [`verify_target`] for what
    /// the target must look like.
    async fn verify(&self, credential: &Credential) -> bool {
        let (url, pinned) = match &self.args.verify_url {
            Some(base) => match verify_target(base, true).await {
                Ok(t) => (t.url, None),
                Err(e) => {
                    warn!(error = %e, "--verify-url is not usable");
                    return false;
                }
            },
            None => match verify_target(&credential.server_url, self.args.allow_private_servers)
                .await
            {
                Ok(t) => {
                    let host = t.host.clone();
                    let addrs = t.addrs.clone();
                    (t.url, Some((host, addrs)))
                }
                Err(e) => {
                    debug!(error = %e, "client named an unusable server");
                    return false;
                }
            },
        };
        // The request goes to the addresses that were checked, whatever the
        // name resolves to a moment later.
        let client = match &pinned {
            Some((host, addrs)) => match http_client(Some((host, addrs))) {
                Ok(c) => c,
                Err(e) => {
                    warn!(error = %e, "could not build a pinned client");
                    return false;
                }
            },
            None => self.http.clone(),
        };
        self.verifications.fetch_add(1, Ordering::Relaxed);
        let params = credential.ping_params();
        let host = url.host_str().unwrap_or("?").to_string();
        match client.get(url).query(&params).send().await {
            Ok(resp) => {
                let status = resp.status();
                match read_capped(resp, VERIFY_BODY_CAP).await {
                    Ok(body) => {
                        let ok = serde_json::from_slice::<serde_json::Value>(&body)
                            .map(|v| v["subsonic-response"]["status"].as_str() == Some("ok"))
                            .unwrap_or(false);
                        if !ok {
                            debug!(%host, %status, "ping rejected");
                        }
                        ok
                    }
                    Err(e) => {
                        debug!(%host, error = %e, "ping response unusable");
                        false
                    }
                }
            }
            Err(e) => {
                // Never the URL: reqwest's Display would print the query,
                // and the query is the credential.
                debug!(%host, error = %e.without_url(), "ping failed");
                false
            }
        }
    }

    /// Register a socket's outgoing channel with its scope's room before the
    /// room hears about it, so nothing it sends is lost. Loads the replica
    /// (if any) off the lock.
    async fn attach_sender(&self, scope: &str, peer: &PeerId, tx: mpsc::UnboundedSender<Message>) {
        let restored = if self.rooms.lock().contains_key(scope) {
            None
        } else {
            let store = self.store.clone();
            let s = scope.to_string();
            Some(
                tokio::task::spawn_blocking(move || store.load(&s))
                    .await
                    .unwrap_or_else(|e| Err(ReplicaError::Io(e.into()))),
            )
        };
        let mut rooms = self.rooms.lock();
        let state = rooms
            .entry(scope.to_string())
            .or_insert_with(|| self.open_room(scope, restored));
        state.senders.insert(peer.clone(), tx);
    }

    fn open_room(
        &self,
        scope: &str,
        restored: Option<Result<Option<ReplicaState>, ReplicaError>>,
    ) -> RoomState {
        let replica = match restored {
            Some(Ok(r)) => r,
            Some(Err(e)) => {
                warn!(scope, error = %e, "could not load replica; starting fresh");
                None
            }
            None => None,
        };
        let mut cfg = RoomConfig::new(scope.to_string());
        cfg.verify = !self.args.no_verify;
        cfg.max_members = self.args.max_members;
        // Debug, not info: anyone can name a scope in a Hello.
        debug!(scope, restored = replica.is_some(), "room opened");
        RoomState {
            room: Room::new(cfg, Arc::new(WallClock), RealReducer::shared(), replica),
            senders: HashMap::new(),
            dirty: false,
            touched: false,
            last_saved: Instant::now(),
        }
    }

    /// Feed one input to a scope's room and perform its outputs. A room that
    /// is not open (never opened, or already closed) ignores the input: a
    /// stray `Disconnected` must not resurrect one from disk. Returns
    /// credentials to verify.
    fn drive(&self, scope: &str, input: RoomInput) -> Vec<(PeerId, Credential)> {
        let disconnected = match &input {
            RoomInput::Disconnected(p) => Some(p.clone()),
            _ => None,
        };
        let mut to_verify = vec![];
        let mut to_save: Option<ReplicaState> = None;
        {
            let mut rooms = self.rooms.lock();
            let Some(state) = rooms.get_mut(scope) else {
                return to_verify;
            };
            let outs = state.room.handle(input);
            for o in outs {
                match o {
                    RoomOutput::Send(peer, msg) => {
                        if let Some(tx) = state.senders.get(&peer) {
                            match msg.encode() {
                                Ok(text) => {
                                    let _ = tx.send(Message::Text(text.into()));
                                }
                                Err(e) => warn!(error = %e, "unencodable frame"),
                            }
                        }
                    }
                    RoomOutput::Close(peer) => {
                        if let Some(tx) = state.senders.remove(&peer) {
                            let _ = tx.send(Message::Close(None));
                        }
                    }
                    RoomOutput::Verify { peer, credential } => to_verify.push((peer, credential)),
                    RoomOutput::ReplicaChanged => state.dirty = true,
                    RoomOutput::ReplicaTouched => state.touched = true,
                }
            }
            if let Some(p) = disconnected {
                state.senders.remove(&p);
            }
            let now = Instant::now();
            if state.senders.is_empty() {
                // No sockets left: keep the replica on disk, drop the in-memory room.
                if state.take_save_due(now, true) {
                    to_save = Some(state.room.replica().clone());
                }
                rooms.remove(scope);
                debug!(scope, "room closed");
            } else if state.take_save_due(now, false) {
                to_save = Some(state.room.replica().clone());
            }
        }
        if let Some(replica) = to_save {
            self.save_off_lock(scope, replica);
        }
        to_verify
    }

    /// Write a replica snapshot on the blocking pool.
    fn save_off_lock(&self, scope: &str, replica: ReplicaState) {
        let store = self.store.clone();
        let scope = scope.to_string();
        let write = move || {
            if let Err(e) = store.save(&scope, &replica) {
                warn!(scope, error = %e, "replica save failed");
            }
        };
        match tokio::runtime::Handle::try_current() {
            Ok(h) => {
                h.spawn_blocking(write);
            }
            Err(_) => write(),
        }
    }

    pub fn room_count(&self) -> usize {
        self.rooms.lock().len()
    }

    /// Persist every open room's replica (shutdown). Synchronous: the
    /// process is on its way out.
    pub fn flush_all(&self) {
        let snapshots: Vec<(String, ReplicaState)> = {
            let mut rooms = self.rooms.lock();
            let now = Instant::now();
            rooms
                .iter_mut()
                .filter(|(_, s)| s.room.ever_admitted())
                .map(|(scope, s)| {
                    s.dirty = false;
                    s.touched = false;
                    s.last_saved = now;
                    (scope.clone(), s.room.replica().clone())
                })
                .collect()
        };
        for (scope, replica) in snapshots {
            if let Err(e) = self.store.save(&scope, &replica) {
                warn!(scope, error = %e, "final replica save failed");
            }
        }
    }

    /// Whether a device is currently a member of the scope's room.
    pub fn has_member(&self, scope: &str, device_id: &str) -> bool {
        self.rooms
            .lock()
            .get(scope)
            .map(|r| r.room.has_member(device_id))
            .unwrap_or(false)
    }

    /// The scope's current transport lease owner, if the room is open.
    pub fn live_owner(&self, scope: &str) -> Option<String> {
        self.rooms
            .lock()
            .get(scope)
            .and_then(|r| r.room.live_owner())
    }

    pub fn store(&self) -> &dyn ReplicaStore {
        self.store.as_ref()
    }

    /// Live WebSocket connections.
    pub fn connection_count(&self) -> u64 {
        self.connections.load(Ordering::Relaxed)
    }

    /// Lease lapse and member timeouts need a clock even when nobody talks;
    /// debounced writes are flushed from here too.
    fn tick_all(&self) {
        let scopes: Vec<String> = self.rooms.lock().keys().cloned().collect();
        for s in scopes {
            let _ = self.drive(&s, RoomInput::Tick);
        }
    }
}

/// A reqwest client that never follows redirects; pinned to `addrs` for
/// `host` when given.
fn http_client(pin: Option<(&String, &Vec<SocketAddr>)>) -> anyhow::Result<reqwest::Client> {
    let mut b = reqwest::Client::builder()
        .timeout(VERIFY_TIMEOUT)
        .redirect(reqwest::redirect::Policy::none());
    if let Some((host, addrs)) = pin {
        b = b.resolve_to_addrs(host, addrs);
    }
    Ok(b.build()?)
}

/// Read a response body up to `cap` bytes; anything longer is an error.
async fn read_capped(mut resp: reqwest::Response, cap: usize) -> anyhow::Result<Vec<u8>> {
    if let Some(len) = resp.content_length() {
        if len > cap as u64 {
            anyhow::bail!("response body of {len} bytes exceeds the cap");
        }
    }
    let mut body = Vec::new();
    while let Some(chunk) = resp.chunk().await? {
        if body.len() + chunk.len() > cap {
            anyhow::bail!("response body exceeds the cap");
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

/// A checked verification target: the `ping` URL, its host and the
/// addresses that host resolved to.
#[derive(Debug, Clone)]
pub struct VerifyTarget {
    pub url: url::Url,
    pub host: String,
    pub addrs: Vec<SocketAddr>,
}

/// Turn a Subsonic base URL into the `ping` endpoint, refusing anything an
/// unauthenticated client must not be able to make this host fetch: a
/// non-`http(s)` scheme, userinfo, a missing host, or (unless
/// `allow_private`) a host that resolves to a private, loopback, link-local
/// or otherwise non-public address. The path is set structurally, so a `#`
/// or `?` in the base cannot swallow it.
pub async fn verify_target(base: &str, allow_private: bool) -> anyhow::Result<VerifyTarget> {
    let mut url = url::Url::parse(base.trim())?;
    if !matches!(url.scheme(), "http" | "https") {
        anyhow::bail!("server URL must be http or https");
    }
    if !url.username().is_empty() || url.password().is_some() {
        anyhow::bail!("server URL must not carry credentials");
    }
    let host = url
        .host_str()
        .ok_or_else(|| anyhow::anyhow!("server URL needs a host"))?
        .to_string();
    let port = url
        .port_or_known_default()
        .ok_or_else(|| anyhow::anyhow!("server URL needs a port"))?;
    url.set_fragment(None);
    url.set_query(None);
    let path = format!("{}/rest/ping.view", url.path().trim_end_matches('/'));
    url.set_path(&path);
    let addrs: Vec<SocketAddr> = tokio::net::lookup_host((host.as_str(), port))
        .await?
        .collect();
    if addrs.is_empty() {
        anyhow::bail!("server host does not resolve");
    }
    if !allow_private {
        if let Some(a) = addrs.iter().find(|a| ip_is_private(a.ip())) {
            anyhow::bail!("server host resolves to a non-public address ({})", a.ip());
        }
    }
    Ok(VerifyTarget { url, host, addrs })
}

async fn health(State(app): State<Arc<App>>) -> impl IntoResponse {
    axum::Json(serde_json::json!({
        "status": "ok",
        "rooms": app.room_count(),
        "connections": app.connections.load(Ordering::Relaxed),
    }))
}

async fn ws_upgrade(
    ws: WebSocketUpgrade,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(app): State<Arc<App>>,
) -> axum::response::Response {
    let ip = addr.ip();
    let admitted = app.admission.lock().try_admit(ip, &app.args, Instant::now());
    if let Err(why) = admitted {
        debug!(%ip, ?why, "connection refused");
        let status = match why {
            Refused::TooManyConnections => StatusCode::SERVICE_UNAVAILABLE,
            Refused::TooManyFromAddress | Refused::RateLimited => StatusCode::TOO_MANY_REQUESTS,
        };
        return status.into_response();
    }
    ws.max_message_size(MAX_FRAME_BYTES)
        .max_frame_size(MAX_FRAME_BYTES)
        .on_upgrade(move |socket| async move {
            handle_socket(socket, app.clone()).await;
            app.admission.lock().release(ip);
        })
}

/// One socket: the first frame must be a `Hello` naming the scope; after
/// that everything is routed to that scope's room.
async fn handle_socket(socket: WebSocket, app: Arc<App>) {
    app.connections.fetch_add(1, Ordering::Relaxed);
    let peer = app.next_peer();
    let (mut sink, mut stream) = socket.split();
    let (tx, mut rx) = mpsc::unbounded_channel::<Message>();
    let mut writer = tokio::spawn(async move {
        while let Some(m) = rx.recv().await {
            let close = matches!(m, Message::Close(_));
            if sink.send(m).await.is_err() || close {
                break;
            }
        }
        let _ = sink.close().await;
    });

    let mut scope: Option<String> = None;
    loop {
        let limit = if scope.is_some() {
            READ_TIMEOUT
        } else {
            HELLO_TIMEOUT
        };
        // The writer finishing means the room closed us (or the peer went
        // away): stop reading at once instead of waiting for a frame.
        let frame = tokio::select! {
            f = tokio::time::timeout(limit, stream.next()) => match f {
                Ok(Some(f)) => f,
                Ok(None) => break,
                Err(_) => {
                    debug!(peer, "read timeout");
                    break;
                }
            },
            _ = &mut writer => break,
        };
        let text = match frame {
            Ok(Message::Text(t)) => t.to_string(),
            Ok(Message::Binary(b)) => match String::from_utf8(b.to_vec()) {
                Ok(t) => t,
                Err(_) => continue,
            },
            Ok(Message::Close(_)) | Err(_) => break,
            Ok(_) => continue,
        };
        let msg = match WireMessage::decode(&text) {
            Ok(m) => m,
            Err(e) => {
                debug!(peer, error = %e, "bad frame");
                continue;
            }
        };
        let current_scope = match (&scope, &msg.msg) {
            (None, Msg::Hello { scope: s, .. }) => {
                let s = s.clone();
                scope = Some(s.clone());
                app.attach_sender(&s, &peer, tx.clone()).await;
                app.drive(&s, RoomInput::Connected(peer.clone()));
                s
            }
            (None, other) => {
                debug!(peer, msg = other.name(), "frame before hello");
                let refuse = WireMessage::new(Msg::Refuse {
                    reason: RefuseReason::Unauthorised,
                    message: "hello first".into(),
                });
                if let Ok(t) = refuse.encode() {
                    let _ = tx.send(Message::Text(t.into()));
                }
                break;
            }
            (Some(s), _) => s.clone(),
        };
        let to_verify = app.drive(&current_scope, RoomInput::Message(peer.clone(), msg));
        for (p, credential) in to_verify {
            let ok = app.verify(&credential).await;
            drop(credential);
            app.drive(&current_scope, RoomInput::Verified { peer: p, ok });
        }
        if !app
            .rooms
            .lock()
            .get(&current_scope)
            .map(|r| r.senders.contains_key(&peer))
            .unwrap_or(false)
        {
            // The room closed us.
            break;
        }
    }
    if let Some(s) = &scope {
        app.drive(s, RoomInput::Disconnected(peer.clone()));
    }
    drop(tx);
    if !writer.is_finished() {
        let _ = tokio::time::timeout(Duration::from_secs(5), &mut writer).await;
        writer.abort();
    }
    app.connections.fetch_sub(1, Ordering::Relaxed);
}

/// Build the router (tests mount it on an ephemeral port).
pub fn router(app: Arc<App>) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/ws", get(ws_upgrade))
        .layer(tower_http::trace::TraceLayer::new_for_http())
        .with_state(app)
}

/// Serve until `shutdown` resolves. Returns the bound address once listening.
pub async fn serve(
    app: Arc<App>,
    listen: SocketAddr,
    shutdown: impl std::future::Future<Output = ()> + Send + 'static,
) -> anyhow::Result<(SocketAddr, tokio::task::JoinHandle<()>)> {
    let listener = tokio::net::TcpListener::bind(listen).await?;
    let addr = listener.local_addr()?;
    let ticker_app = app.clone();
    let ticker = tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(1));
        loop {
            interval.tick().await;
            ticker_app.tick_all();
        }
    });
    let purge_app = app.clone();
    let purger = tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(3600));
        loop {
            interval.tick().await;
            let store = purge_app.store.clone();
            let purged =
                tokio::task::spawn_blocking(move || store.purge_expired(hocket_core::util::now_ms()))
                    .await;
            match purged {
                Ok(Ok(n)) if n > 0 => info!(dropped = n, "expired replicas purged"),
                Ok(Ok(_)) => {}
                Ok(Err(e)) => warn!(error = %e, "replica purge failed"),
                Err(e) => warn!(error = %e, "replica purge task failed"),
            }
        }
    });
    let router = router(app);
    let handle = tokio::spawn(async move {
        if let Err(e) = axum::serve(
            listener,
            router.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .with_graceful_shutdown(shutdown)
        .await
        {
            warn!(error = %e, "server error");
        }
        ticker.abort();
        purger.abort();
    });
    Ok((addr, handle))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn verify_target_refuses_what_an_attacker_would_name() {
        for bad in [
            "ftp://music.example/",
            "file:///etc/passwd",
            "http://user:pw@music.example/",
            "http://127.0.0.1:4533/",
            "http://localhost:4533/",
            "http://169.254.169.254/latest/meta-data",
            "http://10.0.0.1/",
            "http://[::1]:4533/",
            "not a url",
        ] {
            assert!(verify_target(bad, false).await.is_err(), "{bad}");
        }
        let t = verify_target("http://127.0.0.1:4533/nav/#frag?x=1", true)
            .await
            .unwrap();
        assert_eq!(t.url.as_str(), "http://127.0.0.1:4533/nav/rest/ping.view");
        assert_eq!(t.host, "127.0.0.1");
        assert_eq!(t.addrs, vec!["127.0.0.1:4533".parse().unwrap()]);
        // a fragment cannot swallow the path
        let t = verify_target("http://127.0.0.1:4533#", true).await.unwrap();
        assert_eq!(t.url.path(), "/rest/ping.view");
    }

    #[test]
    fn admission_limits_by_address_and_rate() {
        let mut args = Args::for_tests();
        args.max_connections = 3;
        args.per_ip_connections = 2;
        args.per_ip_rate = 2;
        let a: IpAddr = "10.0.0.1".parse().unwrap();
        let b: IpAddr = "10.0.0.2".parse().unwrap();
        let mut adm = Admission::default();
        let t0 = Instant::now();
        assert!(adm.try_admit(a, &args, t0).is_ok());
        assert!(adm.try_admit(a, &args, t0).is_ok());
        assert_eq!(adm.try_admit(a, &args, t0), Err(Refused::TooManyFromAddress));
        adm.release(a);
        // the bucket held two: the third within the minute is rate limited
        assert_eq!(adm.try_admit(a, &args, t0), Err(Refused::RateLimited));
        // a minute later a token is back
        assert!(adm
            .try_admit(a, &args, t0 + Duration::from_secs(31))
            .is_ok());
        assert!(adm.try_admit(b, &args, t0).is_ok());
        assert_eq!(adm.try_admit(b, &args, t0), Err(Refused::TooManyConnections));
    }
}
