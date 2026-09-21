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

use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::State;
use axum::response::IntoResponse;
use axum::routing::get;
use axum::Router;
use clap::Parser;
use futures::{SinkExt, StreamExt};
use parking_lot::Mutex;
use tokio::sync::mpsc;
use tracing::{debug, info, warn};

use hocket_core::connect::replica::{FileReplicaStore, MemoryReplicaStore, ReplicaStore};
use hocket_core::connect::room::{Room, RoomConfig, RoomInput, RoomOutput};
use hocket_core::connect::session_adapter::RealReducer;
use hocket_core::connect::wire::{Credential, Msg, RefuseReason, WireMessage};
use hocket_core::connect::PeerId;
use hocket_core::util::WallClock;

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
}

/// A live room and the sockets attached to it.
struct RoomState {
    room: Room,
    /// Outgoing frame channel per socket.
    senders: HashMap<PeerId, mpsc::UnboundedSender<Message>>,
}

/// Process-wide state shared by every connection.
pub struct App {
    args: Args,
    rooms: Mutex<HashMap<String, RoomState>>,
    store: Box<dyn ReplicaStore>,
    http: reqwest::Client,
    peer_seq: AtomicU64,
    connections: AtomicU64,
}

impl App {
    pub fn new(args: Args) -> anyhow::Result<Arc<App>> {
        let store: Box<dyn ReplicaStore> = match &args.data_dir {
            Some(dir) => Box::new(FileReplicaStore::new(dir)?),
            None => Box::new(MemoryReplicaStore::new()),
        };
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .build()?;
        Ok(Arc::new(App {
            args,
            rooms: Mutex::new(HashMap::new()),
            store,
            http,
            peer_seq: AtomicU64::new(0),
            connections: AtomicU64::new(0),
        }))
    }

    fn next_peer(&self) -> PeerId {
        format!("ws-{}", self.peer_seq.fetch_add(1, Ordering::Relaxed) + 1)
    }

    /// Verify a credential by proxying a Subsonic `ping`. The credential is
    /// used for this one request and dropped.
    async fn verify(&self, credential: &Credential) -> bool {
        let base = self
            .args
            .verify_url
            .clone()
            .unwrap_or_else(|| credential.server_url.clone());
        let base = base.trim_end_matches('/');
        let url = format!("{base}/rest/ping.view");
        let params = credential.ping_params();
        match self.http.get(&url).query(&params).send().await {
            Ok(resp) => {
                let status = resp.status();
                match resp.json::<serde_json::Value>().await {
                    Ok(v) => {
                        let ok = v["subsonic-response"]["status"].as_str() == Some("ok");
                        if !ok {
                            debug!(%url, %status, "ping rejected");
                        }
                        ok
                    }
                    Err(e) => {
                        debug!(%url, error = %e, "ping response not json");
                        false
                    }
                }
            }
            Err(e) => {
                debug!(%url, error = %e, "ping failed");
                false
            }
        }
    }

    /// Register a socket's outgoing channel with its scope's room before the
    /// room hears about it, so nothing it sends is lost.
    fn attach_sender(&self, scope: &str, peer: &PeerId, tx: mpsc::UnboundedSender<Message>) {
        let mut rooms = self.rooms.lock();
        let state = rooms
            .entry(scope.to_string())
            .or_insert_with(|| self.open_room(scope));
        state.senders.insert(peer.clone(), tx);
    }

    fn open_room(&self, scope: &str) -> RoomState {
        let replica = match self.store.load(scope) {
            Ok(r) => r,
            Err(e) => {
                warn!(scope, error = %e, "could not load replica; starting fresh");
                None
            }
        };
        let mut cfg = RoomConfig::new(scope.to_string());
        cfg.verify = !self.args.no_verify;
        cfg.max_members = self.args.max_members;
        info!(scope, restored = replica.is_some(), "room opened");
        RoomState {
            room: Room::new(cfg, Arc::new(WallClock), RealReducer::shared(), replica),
            senders: HashMap::new(),
        }
    }

    /// Feed one input to a scope's room, creating it (from the store) if
    /// needed, and perform its outputs. Returns credentials to verify.
    fn drive(&self, scope: &str, input: RoomInput) -> Vec<(PeerId, Credential)> {
        let mut rooms = self.rooms.lock();
        let disconnected = match &input {
            RoomInput::Disconnected(p) => Some(p.clone()),
            _ => None,
        };
        let state = rooms
            .entry(scope.to_string())
            .or_insert_with(|| self.open_room(scope));
        let outs = state.room.handle(input);
        let mut to_verify = vec![];
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
                RoomOutput::ReplicaChanged => {
                    if let Err(e) = self.store.save(scope, state.room.replica()) {
                        warn!(scope, error = %e, "replica save failed");
                    }
                }
            }
        }
        if let Some(p) = disconnected {
            state.senders.remove(&p);
        }
        if state.senders.is_empty() {
            // No sockets left: keep the replica on disk, drop the in-memory room.
            if let Err(e) = self.store.save(scope, state.room.replica()) {
                warn!(scope, error = %e, "replica save failed");
            }
            rooms.remove(scope);
            debug!(scope, "room closed");
        }
        to_verify
    }

    pub fn room_count(&self) -> usize {
        self.rooms.lock().len()
    }

    /// Persist every open room's replica (shutdown).
    pub fn flush_all(&self) {
        let rooms = self.rooms.lock();
        for (scope, state) in rooms.iter() {
            if let Err(e) = self.store.save(scope, state.room.replica()) {
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

    /// Lease lapse and member timeouts need a clock even when nobody talks.
    fn tick_all(&self) {
        let scopes: Vec<String> = self.rooms.lock().keys().cloned().collect();
        for s in scopes {
            let _ = self.drive(&s, RoomInput::Tick);
        }
    }
}

async fn health(State(app): State<Arc<App>>) -> impl IntoResponse {
    axum::Json(serde_json::json!({
        "status": "ok",
        "rooms": app.room_count(),
        "connections": app.connections.load(Ordering::Relaxed),
    }))
}

async fn ws_upgrade(ws: WebSocketUpgrade, State(app): State<Arc<App>>) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_socket(socket, app))
}

/// One socket: the first frame must be a `Hello` naming the scope; after
/// that everything is routed to that scope's room.
async fn handle_socket(socket: WebSocket, app: Arc<App>) {
    app.connections.fetch_add(1, Ordering::Relaxed);
    let peer = app.next_peer();
    let (mut sink, mut stream) = socket.split();
    let (tx, mut rx) = mpsc::unbounded_channel::<Message>();
    let writer = tokio::spawn(async move {
        while let Some(m) = rx.recv().await {
            let close = matches!(m, Message::Close(_));
            if sink.send(m).await.is_err() || close {
                break;
            }
        }
        let _ = sink.close().await;
    });

    let mut scope: Option<String> = None;
    while let Some(frame) = stream.next().await {
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
                app.attach_sender(&s, &peer, tx.clone());
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
    let _ = writer.await;
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
            match purge_app.store.purge_expired(hocket_core::util::now_ms()) {
                Ok(n) if n > 0 => info!(dropped = n, "expired replicas purged"),
                Ok(_) => {}
                Err(e) => warn!(error = %e, "replica purge failed"),
            }
        }
    });
    let router = router(app);
    let handle = tokio::spawn(async move {
        if let Err(e) = axum::serve(listener, router)
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
