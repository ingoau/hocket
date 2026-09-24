//! The actor. Owns every subsystem; the only thing platform layers touch.
//!
//! [`Core`] is a cheap handle: commands go in on an unbounded channel,
//! queries are answered asynchronously and events stream to every registered
//! [`EventSink`]. One actor task ([`actor::Actor`]) owns the session engine,
//! the undo stack, the action registry, the Connect engine, the Subsonic
//! client, the SQLite mirror, outbox, job queue, downloads, caches, filters,
//! autoplay, settings, stats, the playback backend and the media-session
//! state, and handles one message at a time. Long work is spawned onto the
//! runtime and reports back through [`Internal`] messages, so the loop never
//! blocks. Everything time-based runs off an injected [`Clock`] and a tick.

mod actor;
mod handlers;
pub mod io;
mod queries;
mod state;
pub mod stream_proxy;
#[cfg(any(feature = "sim", test))]
pub mod test_support;

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use parking_lot::Mutex;
use tokio::sync::{mpsc, oneshot};

use crate::api::*;
use crate::audio::backend::{PlaybackBackend, ReportSink};
use crate::autoplay::{AutoplayEngine, AutoplayPick};
use crate::connect::discovery::DiscoveryEvent;
use crate::connect::wire::WireMessage;
use crate::connect::PeerId;
use crate::downloads::StorageProbe;
use crate::jobs::QueueEvent;
use crate::lyrics::LyricsHttp;
use crate::outbox::FlushReport;
use crate::subsonic::SubsonicApi;
use crate::undo::CasResult;
use crate::util::{Clock, WallClock};

pub use actor::default_audio_settings;
pub use io::{ConnectIo, Listener, RealIo};

/// Receives events on a core-owned thread. Implementations must be cheap and
/// non-blocking; hand off to the UI thread.
pub trait EventSink: Send + Sync + 'static {
    fn on_event(&self, event: Event);
}

impl<F: Fn(Event) + Send + Sync + 'static> EventSink for F {
    fn on_event(&self, event: Event) {
        self(event)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum CoreError {
    #[error("invalid json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("core is shut down")]
    ShutDown,
    #[error("{0}")]
    Other(String),
}

impl From<anyhow::Error> for CoreError {
    fn from(e: anyhow::Error) -> Self {
        CoreError::Other(format!("{e:#}"))
    }
}

/// Messages subsystems and spawned tasks post back to the actor loop.
/// Subsystems never reach into each other's state: they are called by the
/// actor with explicit arguments, or they post one of these.
pub enum Internal {
    /// The periodic tick (250 ms in production; tests drive it explicitly).
    Tick,
    /// A report from the playback backend.
    Backend(BackendReport),
    /// A command the external backend wants the platform player to run.
    BackendCommand(BackendCommand),
    /// Job queue visible-state change.
    Jobs(QueueEvent),
    /// Library sync progress.
    SyncProgress(SyncProgress),
    /// A server capability probe finished. `Err` carries the error kind
    /// (auth / network / server) and a message.
    Probed {
        server_id: ServerId,
        result: Result<ServerCapabilities, (ErrorKind, String)>,
    },
    /// Connect I/O reports.
    Connected {
        peer: PeerId,
        url: String,
        tx: mpsc::UnboundedSender<WireMessage>,
    },
    ConnectFailed {
        error: String,
    },
    WireIn {
        peer: PeerId,
        msg: WireMessage,
    },
    Disconnected {
        peer: PeerId,
    },
    ListenerStarted {
        port: u16,
    },
    ListenerFailed {
        error: String,
    },
    PeerAccepted {
        peer: PeerId,
        tx: mpsc::UnboundedSender<WireMessage>,
    },
    Discovery(DiscoveryEvent),
    CredentialVerified {
        peer: PeerId,
        ok: bool,
    },
    /// The autoplay engine comes back from a fetch with its picks.
    Autoplay {
        engine: Box<AutoplayEngine>,
        picks: Vec<AutoplayPick>,
        generation: u32,
    },
    /// The server half of a two-batch search.
    ServerSearch {
        results: SearchResults,
    },
    /// A lyrics fetch (server or external) finished.
    LyricsFetched {
        server_id: ServerId,
        track_id: TrackId,
        lyrics: Option<Lyrics>,
        /// Emit `LyricsChanged` (false for a silent prefetch).
        announce: bool,
    },
    /// Artwork resolved through the image cache.
    Artwork {
        id: String,
        size: u32,
        path: Option<String>,
    },
    /// An outbox flush finished (`None` when no server was available).
    OutboxFlushed {
        report: Option<FlushReport>,
    },
    /// One compare-and-swap step of an undo finished.
    CasResult {
        entry_id: String,
        result: CasResult,
    },
    /// An `.nsp` export was written (or failed).
    NspWritten {
        filter_id: FilterId,
        document: String,
        path: Option<String>,
        error: Option<String>,
    },
    /// A resolved playlist id for a playlist created through the outbox.
    PlaylistCreated {
        server_id: ServerId,
    },
    /// A user action failed asynchronously right after it was issued.
    Toast {
        message: String,
    },
    /// The stream proxy cached a track, or cache entries were evicted or
    /// removed after their reader let go: these tracks' offline state
    /// changed.
    StreamCacheChanged {
        tracks: Vec<crate::downloads::TrackKey>,
    },
    /// A spawned short task finished (bookkeeping for `settle`).
    TaskDone,
    /// Test barrier: answered once every message before it was handled, with
    /// whether anything is still in flight.
    Barrier(oneshot::Sender<bool>),
}

/// What the actor loop receives. One channel keeps commands, queries and
/// internal reports strictly ordered.
pub(crate) enum ActorMsg {
    Command(Command),
    Query(Query, oneshot::Sender<QueryResult>),
    Internal(Internal),
    /// Flush everything and stop; the sender is signalled after the flush.
    Shutdown(oneshot::Sender<()>),
}

/// How the actor obtains its playback backend.
pub(crate) enum BackendChoice {
    /// Pick from [`CoreConfig::audio`].
    Auto,
    /// A backend the caller built (tests). Its report sink is wired to the
    /// actor through `sink`; `poll` is called on every tick.
    #[cfg_attr(not(any(feature = "sim", test)), allow(dead_code))]
    Provided {
        backend: Arc<dyn PlaybackBackend>,
        sink: LateSink,
        poll: Option<PollFn>,
    },
}

/// A callback the actor invokes on every tick to let a virtual-time backend
/// emit the reports that fell due.
pub type PollFn = Arc<dyn Fn() -> Vec<BackendReport> + Send + Sync>;

/// A [`ReportSink`] whose destination is connected after construction, so a
/// backend can be built before the actor that will receive its reports.
#[derive(Clone, Default)]
pub struct LateSink {
    target: Arc<Mutex<Option<mpsc::UnboundedSender<ActorMsg>>>>,
    buffered: Arc<Mutex<Vec<BackendReport>>>,
}

impl LateSink {
    pub fn new() -> Self {
        Self::default()
    }

    /// The sink to hand to a backend.
    pub fn sink(&self) -> ReportSink {
        let me = self.clone();
        Arc::new(move |report: BackendReport| me.push(report))
    }

    fn push(&self, report: BackendReport) {
        let target = self.target.lock().clone();
        match target {
            Some(tx) => {
                let _ = tx.send(ActorMsg::Internal(Internal::Backend(report)));
            }
            None => self.buffered.lock().push(report),
        }
    }

    fn connect(&self, tx: mpsc::UnboundedSender<ActorMsg>) {
        *self.target.lock() = Some(tx.clone());
        for r in self.buffered.lock().drain(..) {
            let _ = tx.send(ActorMsg::Internal(Internal::Backend(r)));
        }
    }
}

/// A server the core is attached to from construction (tests): the fake
/// server plus the metadata the platform would otherwise supply through
/// `Command::AddServer`.
/// A server whose credentials are being verified before it is added.
pub(crate) struct PendingServer {
    pub info: ServerInfo,
    pub client: Arc<crate::subsonic::Client>,
    pub credential: crate::connect::wire::Credential,
    pub lan_key: crate::connect::auth::LanKey,
}

pub(crate) struct PresetServer {
    pub api: Arc<dyn SubsonicApi>,
    pub url: String,
    pub username: String,
    pub password: String,
}

/// Everything injectable. Production uses [`Deps::production`].
pub(crate) struct Deps {
    pub clock: Arc<dyn Clock>,
    pub backend: BackendChoice,
    pub io: Arc<dyn ConnectIo>,
    pub storage: Arc<dyn StorageProbe>,
    pub lyrics_http: Option<Arc<dyn LyricsHttp>>,
    pub server: Option<PresetServer>,
    /// Session entropy seed for deterministic tests (`None` = OS random).
    pub seed: Option<u64>,
    /// Drive time from the tick only (no wall-clock ticker task).
    pub manual_tick: bool,
    /// How the loopback stream proxy reaches the server (`None` = reqwest).
    pub stream_upstream: Option<Arc<dyn stream_proxy::StreamUpstream>>,
}

impl Deps {
    fn production(config: &CoreConfig) -> Deps {
        Deps {
            clock: Arc::new(WallClock),
            backend: BackendChoice::Auto,
            io: Arc::new(RealIo::new(config)),
            storage: Arc::new(crate::downloads::UnknownStorage),
            lyrics_http: None,
            server: None,
            seed: None,
            manual_tick: false,
            stream_upstream: None,
        }
    }
}

/// Handle to a running core. Cheap to clone.
#[derive(Clone)]
pub struct Core {
    inner: Arc<Inner>,
}

struct Inner {
    config: CoreConfig,
    runtime: tokio::runtime::Handle,
    _owned_runtime: Option<tokio::runtime::Runtime>,
    tx: mpsc::UnboundedSender<ActorMsg>,
    sinks: Arc<Mutex<Vec<Arc<dyn EventSink>>>>,
    /// Messages the actor has handled (test barrier bookkeeping).
    processed: Arc<AtomicU64>,
}

impl Core {
    /// Construct a core with its own multi-threaded tokio runtime.
    pub fn new(config: CoreConfig) -> Result<Core, CoreError> {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .thread_name("hocket-core")
            .enable_all()
            .build()
            .map_err(|e| CoreError::Other(e.to_string()))?;
        let handle = runtime.handle().clone();
        let deps = Deps::production(&config);
        Self::with_runtime(config, handle, Some(runtime), deps)
    }

    /// Construct a core on an existing runtime (napi, tests, coordinator).
    pub fn on_runtime(
        config: CoreConfig,
        handle: tokio::runtime::Handle,
    ) -> Result<Core, CoreError> {
        let deps = Deps::production(&config);
        Self::with_runtime(config, handle, None, deps)
    }

    pub(crate) fn with_runtime(
        config: CoreConfig,
        runtime: tokio::runtime::Handle,
        owned: Option<tokio::runtime::Runtime>,
        deps: Deps,
    ) -> Result<Core, CoreError> {
        let (tx, rx) = mpsc::unbounded_channel();
        let sinks: Arc<Mutex<Vec<Arc<dyn EventSink>>>> = Arc::new(Mutex::new(Vec::new()));
        let processed = Arc::new(AtomicU64::new(0));
        if let BackendChoice::Provided { sink, .. } = &deps.backend {
            sink.connect(tx.clone());
        }
        let manual_tick = deps.manual_tick;
        let actor = {
            let _guard = runtime.enter();
            actor::Actor::new(
                config.clone(),
                runtime.clone(),
                tx.clone(),
                sinks.clone(),
                processed.clone(),
                deps,
            )?
        };
        let inner = Arc::new(Inner {
            config,
            runtime: runtime.clone(),
            _owned_runtime: owned,
            tx: tx.clone(),
            sinks,
            processed,
        });
        runtime.spawn(actor.run(rx));
        if !manual_tick {
            let tick_tx = tx;
            runtime.spawn(async move {
                let mut interval = tokio::time::interval(std::time::Duration::from_millis(250));
                interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
                loop {
                    interval.tick().await;
                    if tick_tx.send(ActorMsg::Internal(Internal::Tick)).is_err() {
                        break;
                    }
                }
            });
        }
        Ok(Core { inner })
    }

    pub fn config(&self) -> &CoreConfig {
        &self.inner.config
    }

    pub fn runtime(&self) -> &tokio::runtime::Handle {
        &self.inner.runtime
    }

    /// Subscribe to events. Multiple sinks are allowed (UI + media session + tests).
    pub fn add_sink(&self, sink: Arc<dyn EventSink>) {
        self.inner.sinks.lock().push(sink);
    }

    pub fn emit(&self, event: Event) {
        let sinks = self.inner.sinks.lock().clone();
        for s in sinks {
            s.on_event(event.clone());
        }
    }

    /// Fire-and-forget. Never blocks the caller.
    pub fn dispatch(&self, command: Command) -> Result<(), CoreError> {
        self.inner
            .tx
            .send(ActorMsg::Command(command))
            .map_err(|_| CoreError::ShutDown)
    }

    /// Flush everything and stop the actor; resolves once the session
    /// document, position, settings, autoplay exclusion set and sync base
    /// are written and the backend is stopped. Idempotent: a second call
    /// (or a call after `Command::Shutdown`) resolves as soon as the actor
    /// is gone. Platforms await this before dropping the core or exiting.
    pub async fn shutdown(&self) {
        let (tx, rx) = oneshot::channel();
        if self.inner.tx.send(ActorMsg::Shutdown(tx)).is_err() {
            // The actor already stopped (its flush ran before the receiver
            // was dropped).
            return;
        }
        // `Err` means the actor dropped the sender: it stopped, and every
        // sender is dropped only after the flush.
        let _ = rx.await;
    }

    /// [`Core::shutdown`] for FFI callers on a plain thread: runs the
    /// shutdown on the core's own runtime and blocks the calling thread for
    /// at most `timeout`. Returns `true` when the flush completed in time.
    /// Never call this from inside an async context; use [`Core::shutdown`]
    /// there.
    pub fn shutdown_blocking(&self, timeout: std::time::Duration) -> bool {
        let (done_tx, done_rx) = std::sync::mpsc::channel::<()>();
        let core = self.clone();
        self.inner.runtime.spawn(async move {
            core.shutdown().await;
            let _ = done_tx.send(());
        });
        done_rx.recv_timeout(timeout).is_ok()
    }

    pub fn dispatch_json(&self, json: &str) -> Result<(), CoreError> {
        self.dispatch(serde_json::from_str(json)?)
    }

    /// Post an [`Internal`] message (subsystem completions, timers).
    pub fn post_internal(&self, internal: Internal) -> Result<(), CoreError> {
        self.inner
            .tx
            .send(ActorMsg::Internal(internal))
            .map_err(|_| CoreError::ShutDown)
    }

    /// Async request/response.
    pub async fn query(&self, query: Query) -> Result<QueryResult, CoreError> {
        let (tx, rx) = oneshot::channel();
        self.inner
            .tx
            .send(ActorMsg::Query(query, tx))
            .map_err(|_| CoreError::ShutDown)?;
        rx.await.map_err(|_| CoreError::ShutDown)
    }

    pub async fn query_json(&self, json: &str) -> Result<String, CoreError> {
        let q: Query = serde_json::from_str(json)?;
        Ok(serde_json::to_string(&self.query(q).await?)?)
    }

    /// Deliver one tick to the actor (tests with `manual_tick`).
    pub fn tick(&self) -> Result<(), CoreError> {
        self.post_internal(Internal::Tick)
    }

    /// Wait until the actor has handled everything sent so far and nothing
    /// it spawned is still running. Bounded: gives up after ten seconds of
    /// wall time so a stuck task fails a test instead of hanging it.
    pub async fn settle(&self) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        let mut quiet = 0;
        loop {
            let before = self.inner.processed.load(Ordering::SeqCst);
            let (tx, rx) = oneshot::channel();
            if self
                .inner
                .tx
                .send(ActorMsg::Internal(Internal::Barrier(tx)))
                .is_err()
            {
                return;
            }
            let busy = rx.await.unwrap_or(false);
            for _ in 0..4 {
                tokio::task::yield_now().await;
            }
            let after = self.inner.processed.load(Ordering::SeqCst);
            if !busy && after == before + 1 {
                quiet += 1;
                if quiet >= 2 {
                    return;
                }
            } else {
                quiet = 0;
                tokio::time::sleep(std::time::Duration::from_millis(2)).await;
            }
            if std::time::Instant::now() > deadline {
                tracing::warn!("settle: giving up after 10 s");
                return;
            }
        }
    }
}
