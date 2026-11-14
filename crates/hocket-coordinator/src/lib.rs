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
//!   POST /reports        → create a report (grant token), 201 {id, uploadKey, files}
//!   PUT /reports/{id}/files/{index}
//!                        → the next bytes of a file (upload key, Upload-Offset)
//!   GET /reports/{id}    → what has arrived (upload key), to resume
//!   DELETE /reports/{id} → drop an unfinished report (upload key), 204
//!
//! hocket-coordinator --data-dir /var/lib/hocket reports list
//! hocket-coordinator --data-dir /var/lib/hocket reports show R-20261101-7k3f
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
//!
//! Developer diagnostics: a member's `DiagnosticsRequest` gets the room's own
//! state (from the room) and a coordinator section (from here): the version
//! for anyone, plus this process's whole log and its flags for the accounts
//! named with `--diagnostics-user`. That log covers every room, so the
//! account is matched against the credential this socket was verified with,
//! never against anything the request says.
//!
//! Reports (`docs/reports.md`, [`reports`]): with a `--data-dir`, at least one
//! `--diagnostics-user` and no `--no-reports`, the answer to such an account
//! also carries a `reportGrant` on part 0. The grant lets that device save a
//! report (note, bundle, screenshots, a recording) under
//! `<data-dir>/reports/` over the HTTP routes above, together with this
//! process's log at that moment. Reports are capped by `--reports-quota-mb`
//! (oldest finished ones go first) and purged by age every hour.
//!
//! AutoMix ([`analysis`], `docs/automix.md` → *Sharing records*): unless
//! `--no-analysis-store`, the coordinator keeps the preferred analysis record
//! per (server, track) in `<data-dir>/automix.sqlite`, says so in `Welcome`,
//! and answers members' `AnalysisFetch` / takes their `AnalysisUpload`
//! itself (never through the room). The opt-in [`worker`]
//! (`--analysis-server`) is the one exception to holding no credential: it
//! logs in with its own account's password to analyse the library, and its
//! progress shows in `/health` under `analysis`.

use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::extract::connect_info::ConnectInfo;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::DefaultBodyLimit;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{get, post, put};
use axum::Router;
use clap::{Parser, Subcommand};
use futures::{SinkExt, StreamExt};
use parking_lot::Mutex;
use tokio::sync::mpsc;
use tracing::{debug, info, warn};

use hocket_core::connect::auth::ip_is_private;
use hocket_core::connect::diagnostics;
use hocket_core::connect::replica::{
    FileReplicaStore, MemoryReplicaStore, ReplicaError, ReplicaStore,
};
use hocket_core::connect::room::{Room, RoomConfig, RoomInput, RoomOutput};
use hocket_core::connect::session_adapter::RealReducer;
use hocket_core::connect::wire::{
    account_id_for_scope, scope_key, server_key, AnalysisEntry, Credential, Msg, RefuseReason,
    ReplicaState, WireMessage, ANALYSIS_BATCH_MAX, ANALYSIS_FETCH_MAX,
};
use hocket_core::connect::PeerId;
use hocket_core::util::WallClock;

pub use hocket_core::connect::transport::MAX_FRAME_BYTES;

pub mod analysis;
pub mod reports;
pub mod worker;

/// Epoch milliseconds now; injectable so tests can move time (grant expiry,
/// retention).
pub type Clock = Arc<dyn Fn() -> u64 + Send + Sync>;

/// A socket that has not sent its `Hello` within this long is dropped.
pub const HELLO_TIMEOUT: Duration = Duration::from_secs(10);
/// A member that sends nothing for this long is dropped. The room times
/// members out at 30 s, or up to 140 s for one that announced the slowest
/// keepalive cadence (`Msg::Cadence`, see
/// [`hocket_core::connect::lease::MAX_CADENCE_MS`]); this is the
/// transport-level backstop, so it sits above that.
pub const READ_TIMEOUT: Duration = Duration::from_secs(150);
/// The verification ping's whole budget.
pub const VERIFY_TIMEOUT: Duration = Duration::from_secs(10);
/// Largest ping response body read (a Subsonic ping is a few hundred bytes).
pub const VERIFY_BODY_CAP: usize = 64 * 1024;
/// Replica changes are written at most this often per room.
pub const SAVE_DEBOUNCE: Duration = Duration::from_secs(1);
/// Heartbeat-only changes are written at most this often per room.
pub const TOUCH_SAVE_INTERVAL: Duration = Duration::from_secs(30);
/// Analysis messages (fetches and uploads together) a socket may send per
/// minute; the rest are dropped.
pub const ANALYSIS_MESSAGES_PER_MINUTE: u32 = 60;
/// Pause between the parts of the coordinator's diagnostics answer.
pub const DIAGNOSTICS_PART_INTERVAL: Duration = Duration::from_millis(100);

#[derive(Parser, Debug, Clone)]
#[command(
    name = "hocket-coordinator",
    about = "Headless Hocket coordinator: relay + replica, no credentials, no audio."
)]
pub struct Args {
    /// Address to listen on.
    #[arg(long, default_value = "0.0.0.0:7373")]
    pub listen: SocketAddr,
    /// Directory for replica persistence (and reports, in `reports/`). Omit
    /// for in-memory only.
    #[arg(long, global = true)]
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
    /// An account whose devices may receive this coordinator's whole log
    /// (every room: other users' names, servers, device names and addresses;
    /// never credentials) in developer diagnostics. `USER@https://server`,
    /// or just `USER` with `--verify-url`. Repeatable. Refused with
    /// `--no-verify`. Everyone else gets the coordinator's version only.
    #[arg(long = "diagnostics-user", value_name = "USER[@URL]")]
    pub diagnostics_users: Vec<String>,
    /// Never keep reports, even with `--data-dir` and `--diagnostics-user`.
    #[arg(long, default_value_t = false)]
    pub no_reports: bool,
    /// Cap on all reports together, in MiB; the oldest finished ones are
    /// deleted to make room.
    #[arg(long, default_value_t = 20480)]
    pub reports_quota_mb: u64,
    /// Don't keep AutoMix analysis records (`<data-dir>/automix.sqlite`, in
    /// memory without a data dir) for clients to share.
    #[arg(long, default_value_t = false)]
    pub no_analysis_store: bool,
    /// Most AutoMix records kept per Navidrome server; the least recently
    /// updated go first.
    #[arg(long, default_value_t = 400_000)]
    pub analysis_records_per_server: u64,
    /// Cap on one Navidrome server's AutoMix records, in MiB (of JSON; a
    /// typical record is ~10 KB).
    #[arg(long, default_value_t = 2048)]
    pub analysis_mb_per_server: u64,
    /// Cap on all AutoMix records together, in MiB.
    #[arg(long, default_value_t = 8192)]
    pub analysis_quota_mb: u64,
    /// The opt-in analysis worker (see `--analysis-server`).
    #[command(flatten)]
    pub worker: worker::WorkerArgs,
    /// Without a subcommand the coordinator serves.
    #[command(subcommand)]
    pub command: Option<Command>,
}

/// Offline commands (no subcommand: serve).
#[derive(Subcommand, Debug, Clone)]
pub enum Command {
    /// Read the reports saved in `--data-dir`.
    Reports {
        #[command(subcommand)]
        action: ReportsCommand,
    },
}

/// `reports …`.
#[derive(Subcommand, Debug, Clone)]
pub enum ReportsCommand {
    /// One line per report, newest first.
    List,
    /// A report's metadata, note and the absolute path of every file.
    Show {
        /// `R-YYYYMMDD-xxxx`.
        id: String,
    },
}

/// One `--diagnostics-user`: a username and, unless every credential is
/// verified against `--verify-url`, the server it belongs to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiagnosticsUser {
    pub username: String,
    pub server_url: Option<String>,
}

impl DiagnosticsUser {
    /// `user@https://server` (a username may hold `@` itself: the server
    /// starts at the last `@http://` or `@https://`) or a bare `user`.
    pub fn parse(spec: &str) -> anyhow::Result<DiagnosticsUser> {
        let spec = spec.trim();
        let at = ["@https://", "@http://"]
            .iter()
            .filter_map(|p| spec.to_ascii_lowercase().rfind(p))
            .max();
        let (username, server_url) = match at {
            Some(i) => (&spec[..i], Some(spec[i + 1..].to_string())),
            None => (spec, None),
        };
        if username.is_empty() {
            anyhow::bail!("--diagnostics-user {spec:?} has no username");
        }
        Ok(DiagnosticsUser {
            username: username.to_string(),
            server_url,
        })
    }

    /// Whether a credential this coordinator verified names this account.
    /// With `--verify-url` only the username was verified (the server named
    /// in the credential is the client's word, so it is never compared; the
    /// flags are checked to name that server), otherwise the pair was.
    fn matches(&self, server_url: &str, username: &str, verify_url_set: bool) -> bool {
        if self.username != username {
            return false;
        }
        match &self.server_url {
            _ if verify_url_set => true,
            Some(url) => scope_key(url, username) == scope_key(server_url, username),
            None => false,
        }
    }
}

/// The `--diagnostics-user` list, checked against the other flags.
fn diagnostics_users(args: &Args) -> anyhow::Result<Vec<DiagnosticsUser>> {
    let users = args
        .diagnostics_users
        .iter()
        .map(|s| DiagnosticsUser::parse(s))
        .collect::<anyhow::Result<Vec<_>>>()?;
    if !users.is_empty() && args.no_verify {
        anyhow::bail!(
            "--diagnostics-user needs verified credentials: it cannot be used with --no-verify"
        );
    }
    if let Some(verify) = &args.verify_url {
        // Every credential is checked against that server alone: naming another one would grant
        // the log to whoever has this username there.
        if let Some(u) = users.iter().find(|u| {
            u.server_url
                .as_deref()
                .is_some_and(|url| scope_key(url, "") != scope_key(verify, ""))
        }) {
            anyhow::bail!(
                "--diagnostics-user {:?} names a server other than --verify-url",
                u.username
            );
        }
    } else if let Some(u) = users.iter().find(|u| u.server_url.is_none()) {
        anyhow::bail!(
            "--diagnostics-user {:?} names no server: write USER@https://server, or set --verify-url",
            u.username
        );
    }
    Ok(users)
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
            diagnostics_users: vec![],
            no_reports: false,
            reports_quota_mb: 20480,
            no_analysis_store: false,
            analysis_records_per_server: 400_000,
            analysis_mb_per_server: 2048,
            analysis_quota_mb: 8192,
            worker: worker::WorkerArgs::default(),
            command: None,
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

/// Orders one scope's replica writes: every save request takes a
/// generation, and a write only happens while it is still the latest
/// request, under the scope's write lock. Two snapshots of a scope can then
/// never land out of order (an older rename overwriting a newer file).
#[derive(Default)]
struct SaveSlot {
    requested: AtomicU64,
    write: Mutex<()>,
}

impl SaveSlot {
    /// Register a new snapshot; returns its generation.
    fn begin(&self) -> u64 {
        self.requested.fetch_add(1, Ordering::SeqCst) + 1
    }

    /// Run `write` if `generation` is still the newest request (a newer one
    /// will write its own, fresher snapshot). Returns whether it ran.
    fn write_if_latest(&self, generation: u64, write: impl FnOnce()) -> bool {
        let _guard = self.write.lock();
        if self.requested.load(Ordering::SeqCst) != generation {
            return false;
        }
        write();
        true
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
    /// Write ordering per scope (see [`SaveSlot`]).
    saves: Mutex<HashMap<String, Arc<SaveSlot>>>,
    /// `--diagnostics-user`, parsed.
    diagnostics_users: Vec<DiagnosticsUser>,
    /// Report storage and grants; `None` when reports are off.
    reports: Option<reports::Reports>,
    /// AutoMix analysis records; `None` with `--no-analysis-store`.
    analysis: Option<Arc<analysis::AnalysisStore>>,
    /// The analysis worker's progress, when it runs.
    worker_status: Arc<Mutex<Option<worker::WorkerStatus>>>,
    /// Tracks clients missed in the store, for the worker to analyse first;
    /// `None` without `--analysis-server`.
    worker_priority: Option<Arc<worker::PriorityQueue>>,
    clock: Clock,
    started: Instant,
}

impl App {
    pub fn new(args: Args) -> anyhow::Result<Arc<App>> {
        App::with_clock(args, Arc::new(|| hocket_core::util::now_ms() as u64))
    }

    /// [`App::new`] with another clock (tests).
    pub fn with_clock(args: Args, clock: Clock) -> anyhow::Result<Arc<App>> {
        let store: Arc<dyn ReplicaStore> = match &args.data_dir {
            Some(dir) => Arc::new(FileReplicaStore::new(dir)?),
            None => Arc::new(MemoryReplicaStore::new()),
        };
        let http = http_client(None)?;
        let diagnostics_users = diagnostics_users(&args)?;
        let reports = match &args.data_dir {
            Some(dir) if !diagnostics_users.is_empty() && !args.no_reports && !args.no_verify => {
                Some(reports::Reports::new(
                    dir.join(reports::REPORTS_DIR),
                    args.reports_quota_mb,
                )?)
            }
            _ => None,
        };
        let limits = analysis::StoreLimits {
            records_per_server: args.analysis_records_per_server,
            bytes_per_server: args.analysis_mb_per_server.saturating_mul(1 << 20),
            bytes_total: args.analysis_quota_mb.saturating_mul(1 << 20),
        };
        let analysis = match (&args.data_dir, args.no_analysis_store) {
            (_, true) => None,
            (Some(dir), false) => Some(analysis::AnalysisStore::open(dir)?),
            (None, false) => Some(analysis::AnalysisStore::in_memory()?),
        }
        .map(|s| Arc::new(s.with_limits(limits)));
        if args.worker.analysis_server.is_some() && analysis.is_none() {
            anyhow::bail!("--analysis-server needs the analysis store: drop --no-analysis-store");
        }
        // Keyed as the worker keys its records (see `WorkerArgs::resolve`).
        let worker_priority = args.worker.analysis_server.as_deref().map(|server| {
            Arc::new(worker::PriorityQueue::new(
                args.verify_url.as_deref().unwrap_or(server),
            ))
        });
        Ok(Arc::new(App {
            args,
            rooms: Mutex::new(HashMap::new()),
            store,
            http,
            peer_seq: AtomicU64::new(0),
            connections: AtomicU64::new(0),
            admission: Mutex::new(Admission::default()),
            verifications: AtomicU64::new(0),
            saves: Mutex::new(HashMap::new()),
            diagnostics_users,
            reports,
            analysis,
            worker_status: Arc::new(Mutex::new(None)),
            worker_priority,
            clock,
            started: Instant::now(),
        }))
    }

    /// Whether a socket verified with this account may have the whole log.
    pub fn may_read_full_log(&self, server_url: &str, username: &str) -> bool {
        self.diagnostics_users
            .iter()
            .any(|u| u.matches(server_url, username, self.args.verify_url.is_some()))
    }

    /// Epoch milliseconds by this app's clock.
    pub fn now_ms(&self) -> u64 {
        (self.clock)()
    }

    /// The AutoMix analysis store, unless `--no-analysis-store`.
    pub fn analysis_store(&self) -> Option<&Arc<analysis::AnalysisStore>> {
        self.analysis.as_ref()
    }

    /// The analysis worker's progress (`None` while it isn't running).
    pub fn worker_status(&self) -> Option<worker::WorkerStatus> {
        let mut status = self.worker_status.lock().clone()?;
        status.priority_queue = self.worker_priority.as_ref().map_or(0, |q| q.len()) as u64;
        Some(status)
    }

    /// The tracks the analysis worker is to analyse first (with
    /// `--analysis-server`).
    pub fn worker_priority(&self) -> Option<Arc<worker::PriorityQueue>> {
        self.worker_priority.clone()
    }

    /// Where the analysis worker reports its progress.
    pub fn worker_status_slot(&self) -> Arc<Mutex<Option<worker::WorkerStatus>>> {
        self.worker_status.clone()
    }

    /// The server key a socket's analysis messages read and write: the
    /// server its credential was verified against (`--verify-url` when set),
    /// or with `--no-verify` the server its `Hello` named.
    fn analysis_server(
        &self,
        verified: Option<&(String, String)>,
        hello: Option<&str>,
    ) -> Option<String> {
        if let Some(v) = &self.args.verify_url {
            return (verified.is_some() || self.args.no_verify).then(|| server_key(v));
        }
        match verified {
            Some((url, _)) => Some(server_key(url)),
            None if self.args.no_verify => hello.map(server_key),
            None => None,
        }
    }

    /// Answer an `AnalysisFetch` (off the async threads).
    async fn answer_analysis_fetch(
        &self,
        tx: &mpsc::UnboundedSender<Message>,
        server: String,
        account: String,
        mut track_ids: Vec<String>,
    ) {
        let Some(store) = self.analysis.clone() else {
            return;
        };
        track_ids.truncate(ANALYSIS_FETCH_MAX);
        let mut seen = std::collections::HashSet::new();
        track_ids.retain(|id| seen.insert(id.clone()));
        let asked = self.worker_priority.is_some().then(|| track_ids.clone());
        let key = server.clone();
        let found = tokio::task::spawn_blocking(move || store.get_many(&key, &track_ids)).await;
        let records = match found {
            Ok(Ok(r)) => {
                // Misses go to the worker, if it analyses this server: no
                // record, or one it would replace (no beat grid, not its own).
                if let (Some(queue), Some(asked)) = (&self.worker_priority, asked) {
                    let missed = asked.iter().filter(|id| {
                        !r.iter()
                            .any(|e| &e.track_id.id == *id && analysis::complete(&e.record))
                    });
                    let added = queue.push(&server, missed.map(String::as_str));
                    if added > 0 {
                        debug!(
                            added,
                            queued = queue.len(),
                            "analysis misses queued for the worker"
                        );
                    }
                }
                r.into_iter()
                    .map(|mut e| {
                        e.track_id.source = account.clone();
                        e
                    })
                    .collect()
            }
            Ok(Err(e)) => {
                warn!(error = %e, "analysis store read failed");
                vec![]
            }
            Err(e) => {
                warn!(error = %e, "analysis store read task failed");
                vec![]
            }
        };
        for records in analysis_chunks(records) {
            match WireMessage::new(Msg::AnalysisRecords { records }).encode() {
                Ok(t) => {
                    let _ = tx.send(Message::Text(t.into()));
                }
                Err(e) => warn!(error = %e, "unencodable analysis records"),
            }
        }
    }

    /// Store an `AnalysisUpload`'s valid, preferred records (made honest
    /// first: see [`analysis::sanitize_upload`]).
    async fn take_analysis_upload(&self, server: String, mut records: Vec<AnalysisEntry>) {
        let Some(store) = self.analysis.clone() else {
            return;
        };
        records.truncate(ANALYSIS_BATCH_MAX);
        let now = self.now_ms();
        let done = tokio::task::spawn_blocking(move || {
            let (mut stored, mut refused) = (0, 0);
            for mut e in records {
                analysis::sanitize_upload(&mut e.record, now);
                if let Err(why) = analysis::validate(&e.track_id.id, &e.record) {
                    debug!(track = %e.track_id, why, "analysis upload refused");
                    refused += 1;
                    continue;
                }
                match store.put(&server, &e.track_id.id, e.record, now) {
                    Ok(analysis::PutOutcome::Stored) => stored += 1,
                    Ok(analysis::PutOutcome::Kept) => {}
                    Err(err) => warn!(error = %err, "analysis store write failed"),
                }
            }
            (stored, refused)
        })
        .await;
        if let Ok((stored, refused)) = done {
            debug!(stored, refused, "analysis upload");
        }
    }

    /// Whether this coordinator keeps reports.
    pub fn reports_enabled(&self) -> bool {
        self.reports.is_some()
    }

    /// Apply report retention now (also run at startup and hourly by [`serve`]).
    pub fn purge_reports(&self) -> reports::PurgeStats {
        match &self.reports {
            Some(r) => r.purge(self.now_ms()),
            None => reports::PurgeStats::default(),
        }
    }

    /// The coordinator's section of a diagnostics bundle.
    fn diagnostics_text(&self, full: bool) -> String {
        let mut out = format!(
            "hocket-coordinator {} (protocol {}..={})\n",
            env!("CARGO_PKG_VERSION"),
            hocket_core::connect::wire::PROTOCOL_MIN,
            hocket_core::connect::wire::PROTOCOL
        );
        if !full {
            out.push_str(
                "The coordinator's own log is only sent to the accounts its operator named with --diagnostics-user.\n",
            );
            return out;
        }
        let a = &self.args;
        out.push_str(&format!(
            "up {} s; {} rooms open, {} connections, {} verifications\n",
            self.started.elapsed().as_secs(),
            self.room_count(),
            self.connection_count(),
            self.verification_count()
        ));
        out.push_str(&format!(
            "listen {} data_dir {:?} verify_url {:?} no_verify {} allow_private_servers {}\n",
            a.listen, a.data_dir, a.verify_url, a.no_verify, a.allow_private_servers
        ));
        out.push_str(&format!(
            "max_members {} max_connections {} per_ip_connections {} per_ip_rate {}/min; {} diagnostics users\n",
            a.max_members,
            a.max_connections,
            a.per_ip_connections,
            a.per_ip_rate,
            self.diagnostics_users.len()
        ));
        out.push_str(&format!(
            "reports {} (no_reports {} reports_quota_mb {})\n",
            if self.reports_enabled() { "on" } else { "off" },
            a.no_reports,
            a.reports_quota_mb
        ));
        // The asking device looks for this heading to tell the full answer from the short one.
        out.push_str(&format!(
            "\n{}{} minutes)\n----------------------------\n",
            diagnostics::COORDINATOR_LOG_HEADING,
            hocket_core::devlog::WINDOW_MS / 60_000
        ));
        let log = hocket_core::devlog::recent(hocket_core::devlog::WINDOW_MS);
        if log.is_empty() {
            out.push_str("(nothing captured: the log capture is not installed in this process)\n");
        } else {
            out.push_str(&log);
        }
        out
    }

    /// Answer a member's diagnostics request with the coordinator's section:
    /// the first part at once (so it reaches the asking device ahead of the
    /// room's answer, and the device knows to wait for the rest), the rest
    /// paced on a task.
    fn answer_diagnostics(
        &self,
        tx: &mpsc::UnboundedSender<Message>,
        request_id: String,
        to: String,
        identity: Option<&(String, String)>,
    ) {
        let full = identity.is_some_and(|(url, user)| self.may_read_full_log(url, user));
        if let Some((_, user)) = identity {
            info!(user, full, "diagnostics requested");
        }
        // Report users are the diagnostics users; with --verify-url the account's server is that
        // one, never the client's word.
        let grant = match (&self.reports, identity) {
            (Some(reports), Some((url, user))) if full => Some(reports.mint_grant(
                self.args.verify_url.as_deref().unwrap_or(url),
                user,
                self.now_ms(),
            )),
            _ => None,
        };
        let parts = diagnostics::parts(&self.diagnostics_text(full));
        let count = parts.len() as u32;
        let frames: Vec<String> = parts
            .into_iter()
            .enumerate()
            .filter_map(|(i, text)| {
                WireMessage::new(Msg::DiagnosticsReply {
                    request_id: request_id.clone(),
                    from: diagnostics::COORDINATOR.into(),
                    from_name: "Coordinator".into(),
                    to: to.clone(),
                    part: i as u32,
                    parts: count,
                    text,
                    declined: None,
                    asked: vec![],
                    report_grant: if i == 0 { grant.clone() } else { None },
                })
                .encode()
                .ok()
            })
            .collect();
        let mut frames = frames.into_iter();
        if let Some(first) = frames.next() {
            let _ = tx.send(Message::Text(first.into()));
        }
        let rest: Vec<String> = frames.collect();
        if rest.is_empty() {
            return;
        }
        let tx = tx.clone();
        tokio::spawn(async move {
            for f in rest {
                tokio::time::sleep(DIAGNOSTICS_PART_INTERVAL).await;
                if tx.send(Message::Text(f.into())).is_err() {
                    return;
                }
            }
        });
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
            None => {
                match verify_target(&credential.server_url, self.args.allow_private_servers).await {
                    Ok(t) => {
                        let host = t.host.clone();
                        let addrs = t.addrs.clone();
                        (t.url, Some((host, addrs)))
                    }
                    Err(e) => {
                        debug!(error = %e, "client named an unusable server");
                        return false;
                    }
                }
            }
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
        cfg.analysis_store = self.analysis.is_some();
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

    /// The write-ordering slot for a scope.
    fn save_slot(&self, scope: &str) -> Arc<SaveSlot> {
        let mut saves = self.saves.lock();
        if saves.len() > 1024 {
            // Slots nobody is using (no write queued or running) can go.
            saves.retain(|_, s| Arc::strong_count(s) > 1);
        }
        saves.entry(scope.to_string()).or_default().clone()
    }

    /// Write a replica snapshot on the blocking pool, never behind a newer one.
    fn save_off_lock(&self, scope: &str, replica: ReplicaState) {
        let store = self.store.clone();
        let slot = self.save_slot(scope);
        let generation = slot.begin();
        let scope = scope.to_string();
        let write = move || {
            slot.write_if_latest(generation, || {
                if let Err(e) = store.save(&scope, &replica) {
                    warn!(scope, error = %e, "replica save failed");
                }
            });
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
            let slot = self.save_slot(&scope);
            let generation = slot.begin();
            slot.write_if_latest(generation, || {
                if let Err(e) = self.store.save(&scope, &replica) {
                    warn!(scope, error = %e, "final replica save failed");
                }
            });
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
    let mut body = serde_json::json!({
        "status": "ok",
        "rooms": app.room_count(),
        "connections": app.connections.load(Ordering::Relaxed),
    });
    if let Some(status) = app.worker_status() {
        body["analysis"] = serde_json::to_value(status).unwrap_or_default();
    }
    axum::Json(body)
}

async fn ws_upgrade(
    ws: WebSocketUpgrade,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(app): State<Arc<App>>,
) -> axum::response::Response {
    let ip = addr.ip();
    let admitted = app
        .admission
        .lock()
        .try_admit(ip, &app.args, Instant::now());
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
    // The account this socket's credential was verified for.
    let mut identity: Option<(String, String)> = None;
    let mut last_diagnostics: Option<Instant> = None;
    // What the Hello said: the device, and the server its credential named.
    let mut hello_device: Option<String> = None;
    let mut hello_server: Option<String> = None;
    let mut analysis_budget = AnalysisBudget::default();
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
            (
                None,
                Msg::Hello {
                    scope: s,
                    device,
                    credential,
                    ..
                },
            ) => {
                hello_device = Some(device.id.clone());
                hello_server = credential.as_ref().map(|c| c.server_url.clone());
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
        if let Msg::DiagnosticsRequest { request_id, from } = &msg.msg {
            let admitted = app.has_member(&current_scope, from);
            let too_soon = last_diagnostics.is_some_and(|t| {
                t.elapsed().as_secs_f64() * 1000.0 < diagnostics::REQUEST_INTERVAL_MS
            });
            if admitted && !too_soon {
                last_diagnostics = Some(Instant::now());
                app.answer_diagnostics(&tx, request_id.clone(), from.clone(), identity.as_ref());
            }
        }
        if matches!(
            msg.msg,
            Msg::AnalysisFetch { .. } | Msg::AnalysisUpload { .. }
        ) {
            // The coordinator's own business: never relayed. Only for an admitted member.
            let server = app.analysis_server(identity.as_ref(), hello_server.as_deref());
            let member = hello_device
                .as_deref()
                .is_some_and(|d| app.has_member(&current_scope, d));
            match (server, member, analysis_budget.take(Instant::now())) {
                (Some(server), true, true) => match msg.msg {
                    // Records are kept for the room's own account; tracks of
                    // the room's other accounts (a mixed queue) aren't stored here.
                    Msg::AnalysisFetch { track_ids } => {
                        let account = account_id_for_scope(&current_scope);
                        let ids = track_ids
                            .into_iter()
                            .filter(|t| t.source == account)
                            .map(|t| t.id)
                            .collect();
                        app.answer_analysis_fetch(&tx, server, account, ids).await
                    }
                    Msg::AnalysisUpload { mut records } => {
                        let account = account_id_for_scope(&current_scope);
                        records.retain(|e| e.track_id.source == account);
                        app.take_analysis_upload(server, records).await
                    }
                    _ => {}
                },
                _ => debug!(peer, msg = msg.msg.name(), "analysis message dropped"),
            }
            continue;
        }
        let to_verify = app.drive(&current_scope, RoomInput::Message(peer.clone(), msg));
        for (p, credential) in to_verify {
            let ok = app.verify(&credential).await;
            if ok && p == peer {
                identity = Some((credential.server_url.clone(), credential.username.clone()));
            }
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

/// The JSON a batch of records may take in one `AnalysisRecords` frame:
/// half the frame cap, leaving room for the envelope.
const ANALYSIS_FRAME_BUDGET: usize = MAX_FRAME_BYTES / 2;

/// `AnalysisRecords` batches: at most [`ANALYSIS_BATCH_MAX`] records and
/// [`ANALYSIS_FRAME_BUDGET`] bytes each, at least one (empty when nothing was
/// found). A record too large for a frame on its own is left out.
fn analysis_chunks(records: Vec<AnalysisEntry>) -> Vec<Vec<AnalysisEntry>> {
    let mut chunks: Vec<Vec<AnalysisEntry>> = vec![];
    let mut current: Vec<AnalysisEntry> = vec![];
    let mut bytes = 0;
    for e in records {
        let size = serde_json::to_vec(&e).map_or(usize::MAX, |j| j.len());
        if size > ANALYSIS_FRAME_BUDGET {
            warn!(track = %e.track_id, size, "analysis record too large to send");
            continue;
        }
        if current.len() >= ANALYSIS_BATCH_MAX || bytes + size > ANALYSIS_FRAME_BUDGET {
            chunks.push(std::mem::take(&mut current));
            bytes = 0;
        }
        bytes += size;
        current.push(e);
    }
    if !current.is_empty() || chunks.is_empty() {
        chunks.push(current);
    }
    chunks
}

/// A socket's allowance of analysis messages: a bucket of
/// [`ANALYSIS_MESSAGES_PER_MINUTE`] that refills over a minute.
struct AnalysisBudget {
    tokens: f64,
    last: Option<Instant>,
}

impl Default for AnalysisBudget {
    fn default() -> Self {
        AnalysisBudget {
            tokens: f64::from(ANALYSIS_MESSAGES_PER_MINUTE),
            last: None,
        }
    }
}

impl AnalysisBudget {
    fn take(&mut self, now: Instant) -> bool {
        let cap = f64::from(ANALYSIS_MESSAGES_PER_MINUTE);
        if let Some(last) = self.last {
            self.tokens =
                (self.tokens + now.duration_since(last).as_secs_f64() * cap / 60.0).min(cap);
        }
        self.last = Some(now);
        if self.tokens < 1.0 {
            return false;
        }
        self.tokens -= 1.0;
        true
    }
}

/// Build the router (tests mount it on an ephemeral port).
pub fn router(app: Arc<App>) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/ws", get(ws_upgrade))
        .route("/reports", post(reports::create))
        .route(
            "/reports/{id}",
            get(reports::status).delete(reports::remove),
        )
        // The upload streams its body to disk under its own caps, not axum's 2 MB default.
        .route(
            "/reports/{id}/files/{index}",
            put(reports::upload).layer(DefaultBodyLimit::disable()),
        )
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
            let purged = tokio::task::spawn_blocking(move || {
                store.purge_expired(hocket_core::util::now_ms())
            })
            .await;
            match purged {
                Ok(Ok(n)) if n > 0 => info!(dropped = n, "expired replicas purged"),
                Ok(Ok(_)) => {}
                Ok(Err(e)) => warn!(error = %e, "replica purge failed"),
                Err(e) => warn!(error = %e, "replica purge task failed"),
            }
            // The first tick is immediate: this also purges at startup.
            if purge_app.reports_enabled() {
                let reports_app = purge_app.clone();
                match tokio::task::spawn_blocking(move || reports_app.purge_reports()).await {
                    Ok(s) if s.reports + s.recordings > 0 => info!(
                        reports = s.reports,
                        recordings = s.recordings,
                        "old reports purged"
                    ),
                    Ok(_) => {}
                    Err(e) => warn!(error = %e, "report purge task failed"),
                }
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

    #[test]
    fn analysis_answers_are_chunked_by_count_and_size() {
        use hocket_core::api::{
            AnalysisProducer, AnalysisRecord, LoudnessCurve, SourceFingerprint,
        };
        let entry = |id: usize, beats: u32| AnalysisEntry {
            track_id: hocket_core::api::TrackRef::new("s", format!("t{id}")),
            record: AnalysisRecord {
                version: 1,
                analyser: "test".into(),
                produced_by: AnalysisProducer::Desktop,
                source: SourceFingerprint {
                    duration_ms: 1,
                    size_bytes: None,
                    suffix: None,
                    transcoded: false,
                },
                duration_ms: 1,
                integrated_lufs: None,
                audible_start_ms: 0,
                audible_end_ms: 0,
                head: LoudnessCurve::default(),
                tail: LoudnessCurve::default(),
                head_vocals: Default::default(),
                tail_vocals: Default::default(),
                bpm: None,
                beats_ms: (0..beats).map(|b| 10_000_000 + b).collect(),
                downbeats_ms: vec![],
                beat_confidence: 0.0,
                created_at: 0.0,
            },
        };
        assert_eq!(analysis_chunks(vec![]).len(), 1, "one empty answer");
        let small: Vec<_> = (0..40).map(|i| entry(i, 10)).collect();
        let sizes: Vec<usize> = analysis_chunks(small).iter().map(Vec::len).collect();
        assert_eq!(sizes, vec![16, 16, 8]);
        // ~750 KB each (what the old limits allowed): fewer per frame
        let big: Vec<_> = (0..16).map(|i| entry(i, 85_000)).collect();
        let chunks = analysis_chunks(big);
        assert_eq!(chunks.iter().map(Vec::len).sum::<usize>(), 16);
        for c in &chunks {
            let frame = WireMessage::new(Msg::AnalysisRecords { records: c.clone() })
                .encode()
                .unwrap();
            assert!(frame.len() < MAX_FRAME_BYTES, "{}", frame.len());
        }
        // one record over the budget is left out, the rest still go
        let chunks = analysis_chunks(vec![entry(0, 10), entry(1, 600_000), entry(2, 10)]);
        let ids: Vec<&str> = chunks
            .iter()
            .flatten()
            .map(|e| e.track_id.id.as_str())
            .collect();
        assert_eq!(ids, vec!["t0", "t2"]);
    }

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
    fn diagnostics_users_parse_and_match() {
        let u = DiagnosticsUser::parse("me@example.com@https://Music.Example/").unwrap();
        assert_eq!(u.username, "me@example.com");
        assert_eq!(u.server_url.as_deref(), Some("https://Music.Example/"));
        assert!(u.matches("https://music.example", "me@example.com", false));
        assert!(!u.matches("https://other.example", "me@example.com", false));
        assert!(!u.matches("https://music.example", "someone", false));
        // With --verify-url the credential's server is the client's word: never compared.
        assert!(u.matches("https://other.example", "me@example.com", true));
        let bare = DiagnosticsUser::parse("alice").unwrap();
        assert_eq!(bare.server_url, None);
        assert!(bare.matches("https://anything", "alice", true));
        assert!(!bare.matches("https://anything", "alice", false));
        assert!(!bare.matches("https://anything", "Alice", true));
        assert!(DiagnosticsUser::parse("@https://music.example").is_err());
    }

    #[test]
    fn an_older_snapshot_never_overwrites_a_newer_one() {
        let slot = SaveSlot::default();
        let older = slot.begin();
        let newer = slot.begin();
        let mut written = vec![];
        // the newer write happens to run first; the older one then must not
        assert!(slot.write_if_latest(newer, || written.push(newer)));
        assert!(!slot.write_if_latest(older, || written.push(older)));
        assert_eq!(written, vec![newer]);
        // a later request supersedes one that has not run yet
        let a = slot.begin();
        let b = slot.begin();
        assert!(!slot.write_if_latest(a, || written.push(a)));
        assert!(slot.write_if_latest(b, || written.push(b)));
        assert_eq!(written, vec![newer, b]);
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
        assert_eq!(
            adm.try_admit(a, &args, t0),
            Err(Refused::TooManyFromAddress)
        );
        adm.release(a);
        // the bucket held two: the third within the minute is rate limited
        assert_eq!(adm.try_admit(a, &args, t0), Err(Refused::RateLimited));
        // a minute later a token is back
        assert!(adm
            .try_admit(a, &args, t0 + Duration::from_secs(31))
            .is_ok());
        assert!(adm.try_admit(b, &args, t0).is_ok());
        assert_eq!(
            adm.try_admit(b, &args, t0),
            Err(Refused::TooManyConnections)
        );
    }
}
