//! The actor. Owns every subsystem; the only thing platform layers touch.
//!
//! Ownership: wired by the integration pass once subsystems exist. Until then
//! it is a compiling stub that accepts commands and answers queries with empty
//! results, so bindings and apps can build against the real surface.

use std::sync::Arc;

use parking_lot::Mutex;
use tokio::sync::mpsc;

use crate::api::*;

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

/// Handle to a running core. Cheap to clone.
#[derive(Clone)]
pub struct Core {
    inner: Arc<Inner>,
}

struct Inner {
    config: CoreConfig,
    runtime: tokio::runtime::Handle,
    _owned_runtime: Option<tokio::runtime::Runtime>,
    commands: mpsc::UnboundedSender<Command>,
    sinks: Arc<Mutex<Vec<Arc<dyn EventSink>>>>,
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
        Self::with_runtime(config, handle, Some(runtime))
    }

    /// Construct a core on an existing runtime (napi, tests, coordinator).
    pub fn on_runtime(config: CoreConfig, handle: tokio::runtime::Handle) -> Result<Core, CoreError> {
        Self::with_runtime(config, handle, None)
    }

    fn with_runtime(
        config: CoreConfig,
        runtime: tokio::runtime::Handle,
        owned: Option<tokio::runtime::Runtime>,
    ) -> Result<Core, CoreError> {
        let (tx, rx) = mpsc::unbounded_channel();
        let sinks: Arc<Mutex<Vec<Arc<dyn EventSink>>>> = Arc::new(Mutex::new(Vec::new()));
        let inner = Arc::new(Inner { config, runtime, _owned_runtime: owned, commands: tx, sinks });
        let core = Core { inner };
        core.spawn_actor(rx);
        Ok(core)
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
        self.inner.commands.send(command).map_err(|_| CoreError::ShutDown)
    }

    pub fn dispatch_json(&self, json: &str) -> Result<(), CoreError> {
        self.dispatch(serde_json::from_str(json)?)
    }

    /// Async request/response.
    pub async fn query(&self, query: Query) -> Result<QueryResult, CoreError> {
        // Stub: answered by the integration pass. Keep shape stable.
        Ok(stub_answer(query))
    }

    pub async fn query_json(&self, json: &str) -> Result<String, CoreError> {
        let q: Query = serde_json::from_str(json)?;
        Ok(serde_json::to_string(&self.query(q).await?)?)
    }

    fn spawn_actor(&self, mut rx: mpsc::UnboundedReceiver<Command>) {
        let core = self.clone();
        self.inner.runtime.spawn(async move {
            while let Some(cmd) = rx.recv().await {
                match cmd {
                    Command::Shutdown => break,
                    Command::Start | Command::RequestSnapshot => {
                        core.emit(Event::Started { snapshot: empty_snapshot() });
                    }
                    _ => {}
                }
            }
        });
    }
}

pub(crate) fn empty_snapshot() -> Snapshot {
    Snapshot {
        servers: vec![],
        session: None,
        queue: QueueView::default(),
        transport: TransportState::default(),
        connection: ConnectionState::default(),
        devices: vec![],
        jobs: vec![],
        problems: vec![],
        undo: UndoState::default(),
        settings: vec![],
        audio: default_audio_settings(),
        media_session: MediaSessionState::default(),
        resume_offer: None,
        sleep_timer: None,
        network: None,
        battery_saver: false,
        sync_progress: None,
    }
}

pub fn default_audio_settings() -> AudioSettings {
    AudioSettings {
        replay_gain: ReplayGainMode::Auto,
        replay_gain_preamp_db: 0.0,
        normalisation: false,
        eq: EqSettings { enabled: false, preamp_db: 0.0, bands: vec![], preset: None },
        gapless: true,
        output_device: None,
        exclusive: false,
    }
}

fn stub_answer(query: Query) -> QueryResult {
    match query {
        Query::Snapshot => QueryResult::SnapshotResult(empty_snapshot()),
        Query::Servers => QueryResult::Servers(vec![]),
        Query::Tracks { page, .. } => QueryResult::Tracks(TrackPage { items: vec![], offset: page.offset, total: 0 }),
        Query::TrackCount { .. } | Query::AlbumCount { .. } => QueryResult::Count(0),
        Query::Track { .. } => QueryResult::TrackDetail(None),
        Query::TracksByIds { .. } | Query::AlbumTracks { .. } | Query::ArtistTopSongs { .. } => QueryResult::TrackList(vec![]),
        Query::Albums { page, .. } => QueryResult::Albums(AlbumPage { items: vec![], offset: page.offset, total: 0 }),
        Query::Album { .. } => QueryResult::AlbumDetail(None),
        Query::Artists { page, .. } => QueryResult::Artists(ArtistPage { items: vec![], offset: page.offset, total: 0 }),
        Query::Artist { .. } => QueryResult::ArtistDetail(None),
        Query::Genres { .. } => QueryResult::Genres(vec![]),
        Query::Playlists { .. } => QueryResult::Playlists(vec![]),
        Query::Playlist { .. } => QueryResult::PlaylistDetail(None),
        Query::PlaylistTracks { page, .. } => QueryResult::Tracks(TrackPage { items: vec![], offset: page.offset, total: 0 }),
        Query::Search { request_id, query, .. } => QueryResult::Search(SearchResults { request_id, query, ..Default::default() }),
        Query::Queue => QueryResult::Queue(QueueView::default()),
        Query::SavedQueues => QueryResult::SavedQueues(vec![]),
        Query::Lyrics { .. } => QueryResult::LyricsResult(None),
        Query::Related { .. } => QueryResult::Related(vec![]),
        Query::Stats { period_days } => QueryResult::Stats(ListeningStats { period_days, ..Default::default() }),
        Query::RecentlyPlayed { .. } => QueryResult::History(vec![]),
        Query::Jobs => QueryResult::Jobs(vec![]),
        Query::Problems => QueryResult::Problems(vec![]),
        Query::Pins => QueryResult::Pins(vec![]),
        Query::Storage => QueryResult::Storage(StorageSummary::default()),
        Query::Filters => QueryResult::Filters(vec![]),
        Query::FilterPreview { .. } => QueryResult::Preview(FilterPreview {
            count: 0,
            capability: FilterCapability { server_expressible: true, local_only_fields: vec![] },
            sample: vec![],
        }),
        Query::Settings => QueryResult::Settings(vec![]),
        Query::Setting { .. } => QueryResult::SettingDetail(None),
        Query::AudioSettings => QueryResult::Audio(default_audio_settings()),
        Query::OutputDevices => QueryResult::OutputDevices(vec![]),
        Query::Connection => QueryResult::Connection(ConnectionState::default()),
        Query::Devices => QueryResult::Devices(vec![]),
        Query::UndoState => QueryResult::Undo(UndoState::default()),
        Query::Actions { .. } => QueryResult::Actions(vec![]),
        Query::Shortcuts => QueryResult::Shortcuts(vec![]),
        Query::Artwork { .. } => QueryResult::Path(None),
        Query::MediaSource { .. } => QueryResult::Source(None),
        Query::Diagnostics | Query::ConfigDocument { .. } => QueryResult::Text(String::new()),
    }
}
