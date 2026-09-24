//! The actor task: owns every subsystem and handles one message at a time.

use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use parking_lot::Mutex;
use tokio::sync::mpsc;

use crate::actions::ActionRegistry;
use crate::api::*;
use crate::audio::backend::{NullBackend, PlaybackBackend};
use crate::audio::external::ExternalBackend;
use crate::audio::sleep::SleepTimerMachine;
use crate::autoplay::AutoplayEngine;
use crate::cache::Caches;
use crate::connect::discovery::Discovery;
use crate::connect::engine::{Engine, Input, PersistedConnectState};
use crate::connect::session_adapter::ReducerPolicy;
use crate::connect::transport::PeerIds;
use crate::connect::wire::WireMessage;
use crate::connect::PeerId;
use crate::db::Db;
use crate::downloads::Downloads;
use crate::jobs::{JobQueue, QueueEvent};
use crate::outbox::{Outbox, Scrobbler};
use crate::session::{DeterministicEntropy, Entropy, SystemEntropy};
use crate::settings::{keys, Settings, SettingsError, SettingsStore};
use crate::undo::UndoStack;
use crate::util::Clock;

use super::io::ConnectIo;
use super::state::{PendingCas, Playback, SavedPosition, ServerState};
use super::{ActorMsg, BackendChoice, CoreError, Deps, EventSink, Internal, PollFn};

/// Session document persistence debounce.
pub(crate) const DOC_SAVE_DEBOUNCE_MS: f64 = 250.0;
/// Position heartbeat while playing.
pub(crate) const POSITION_HEARTBEAT_MS: f64 = 30_000.0;
/// Retry cadence for a non-empty outbox.
pub(crate) const OUTBOX_RETRY_MS: f64 = 30_000.0;
/// Engine tick cadence (the engine wants at least one per second).
pub(crate) const ENGINE_TICK_MS: f64 = 1_000.0;
/// Items fetched per autoplay top-up.
pub(crate) const AUTOPLAY_BATCH: u32 = 10;
/// Fatal load failures of one item before it is marked unavailable.
pub(crate) const LOAD_RETRIES: u32 = 1;
/// Consecutive unavailable items before playback stops trying.
pub(crate) const MAX_CONSECUTIVE_SKIPS: u32 = 3;
/// Artwork size for the media session (small in battery saver).
pub(crate) const MEDIA_SESSION_ART: u32 = 640;
pub(crate) const MEDIA_SESSION_ART_SMALL: u32 = 160;
/// Finished outbox entries are kept this long (housekeeping).
pub(crate) const OUTBOX_RETENTION: std::time::Duration = std::time::Duration::from_secs(7 * 86_400);
/// Diagnostics ring buffer.
const LOG_RING: usize = 200;

pub fn default_audio_settings() -> AudioSettings {
    AudioSettings {
        replay_gain: ReplayGainMode::Auto,
        replay_gain_preamp_db: 0.0,
        normalisation: false,
        eq: EqSettings {
            enabled: false,
            preamp_db: 0.0,
            bands: vec![],
            preset: None,
        },
        gapless: true,
        output_device: None,
        exclusive: false,
    }
}

/// Posts `TaskDone` when dropped: on completion, cancellation or a panic.
struct TaskDoneGuard(mpsc::UnboundedSender<ActorMsg>);

impl Drop for TaskDoneGuard {
    fn drop(&mut self) {
        let _ = self.0.send(ActorMsg::Internal(Internal::TaskDone));
    }
}

/// Settings persistence over `saved_state("settings")`.
struct DbSettingsStore {
    db: Db,
    clock: Arc<dyn Clock>,
}

impl SettingsStore for DbSettingsStore {
    fn load(&self) -> Result<Option<String>, SettingsError> {
        self.db
            .saved_state_get_raw("settings")
            .map_err(|e| SettingsError::Store(e.to_string()))
    }
    fn save(&self, json: &str) -> Result<(), SettingsError> {
        self.db
            .saved_state_set_raw("settings", json, self.clock.as_ref())
            .map_err(|e| SettingsError::Store(e.to_string()))
    }
}

pub(crate) struct Actor {
    pub cfg: CoreConfig,
    pub clock: Arc<dyn Clock>,
    pub rt: tokio::runtime::Handle,
    pub tx: mpsc::UnboundedSender<ActorMsg>,
    sinks: Arc<Mutex<Vec<Arc<dyn EventSink>>>>,
    processed: Arc<AtomicU64>,

    pub db: Db,
    pub settings: Settings,
    settings_store: DbSettingsStore,
    /// Why the stored settings document could not be used (defaults are in
    /// effect and synced settings are not broadcast).
    pub settings_corrupt: Option<String>,
    pub registry: ActionRegistry,
    pub undo: UndoStack,
    pub jobs: JobQueue,
    pub outbox: Outbox,
    pub downloads: Downloads,
    /// In-process reader players inside the process stream through
    /// (`hocket-stream://`; `None` on the coordinator).
    pub stream_reader: Option<super::stream_reader::StreamReader>,
    /// Whether the playback backend reads `hocket-stream://` sources (the
    /// native backend always; an external one once it says so with
    /// `Command::SetBackendCapabilities`). Otherwise media sources carry
    /// the server URL directly.
    pub core_stream: bool,
    /// Background prefetch of the next queue items (transport owner only).
    pub prefetch: super::handlers::prefetch::PrefetchState,
    /// The album primer (first seconds of a track the user is about to play).
    pub prime: super::handlers::prime::PrimeState,
    /// Stream-cache bookkeeping (offline skips, budget checks).
    pub cache: super::handlers::cache::CacheState,
    pub caches: Caches,
    pub scrobbler: Scrobbler,
    pub sleep: SleepTimerMachine,
    pub autoplay: Option<Box<AutoplayEngine>>,
    pub autoplay_generation: u32,
    pub autoplay_wanted: bool,
    pub reducer_policy: Arc<ReducerPolicy>,
    pub entropy: Arc<dyn Entropy>,

    pub backend: Arc<dyn PlaybackBackend>,
    pub backend_poll: Option<PollFn>,
    pub external: Option<Arc<ExternalBackend>>,
    pub audio: AudioSettings,
    pub output_devices: Vec<OutputDevice>,

    pub io: Arc<dyn ConnectIo>,
    pub peer_ids: Arc<PeerIds>,
    pub conns: HashMap<PeerId, mpsc::UnboundedSender<WireMessage>>,
    pub listener_port: Option<u16>,
    pub listener_stop: Option<tokio::sync::oneshot::Sender<()>>,
    pub discovery: Option<Box<dyn Discovery>>,
    pub lyrics_http: Option<Arc<dyn crate::lyrics::LyricsHttp>>,

    pub server: Option<ServerState>,
    /// Servers added but not yet verified by a probe.
    pub pending_servers: HashMap<ServerId, super::PendingServer>,
    pub engine: Option<Engine>,
    pub scope: Option<String>,
    pub queued_inputs: VecDeque<Input>,
    /// Effects the reducer reported for the local op being submitted.
    pub pending_effects: Option<Vec<crate::session::Effect>>,
    pub last_doc: Option<SessionDocument>,
    pub last_saved_queues: Vec<SavedQueue>,
    pub last_transport: TransportState,
    /// Plays waiting for the session's scrobble verdict (mirrors the durable
    /// [`super::state::PendingScrobble`] records of the open scope).
    pub pending_submits: HashMap<(TrackId, u64), super::state::PendingScrobble>,
    pub pending_cas: HashMap<String, PendingCas>,
    pub outbox_entries: HashMap<String, String>,

    pub playback: Playback,
    pub network: Option<NetworkState>,
    pub visible: bool,
    pub focused: bool,
    pub battery_saver: bool,
    pub selection: ActionTarget,
    pub sync_progress: Option<SyncProgress>,
    pub resume_offer: Option<ResumeOffer>,
    pub picker_open: bool,
    pub picker_targets: Vec<DeviceInfo>,
    pub media_art: Option<(String, u32, Option<String>)>,
    /// Track the last lyrics fetch was announced for (now-playing changes).
    pub lyrics_for: Option<TrackId>,
    pub media_art_pending: Option<(String, u32)>,
    pub log_ring: VecDeque<String>,

    pub started: bool,
    pub shutting_down: bool,
    pub in_flight: usize,
    pub flush_in_flight: bool,
    pub doc_dirty_since: Option<f64>,
    pub position_saved_at: f64,
    pub last_engine_tick: f64,
    pub last_outbox_retry: f64,
    pub last_sync_check: f64,
    pub last_meta_prune: f64,
    /// Local ops issued from this device (undo labels).
    pub open_report: crate::db::OpenReport,
}

impl Actor {
    pub fn new(
        cfg: CoreConfig,
        rt: tokio::runtime::Handle,
        tx: mpsc::UnboundedSender<ActorMsg>,
        sinks: Arc<Mutex<Vec<Arc<dyn EventSink>>>>,
        processed: Arc<AtomicU64>,
        deps: Deps,
    ) -> Result<Actor, CoreError> {
        let data_dir = PathBuf::from(&cfg.data_dir);
        let cache_dir = PathBuf::from(&cfg.cache_dir);
        std::fs::create_dir_all(&data_dir).map_err(|e| CoreError::Other(e.to_string()))?;
        std::fs::create_dir_all(&cache_dir).map_err(|e| CoreError::Other(e.to_string()))?;
        let db = Db::open(&data_dir.join("hocket.sqlite"), &data_dir.join("backups"))
            .map_err(|e| CoreError::Other(format!("open database: {e}")))?;
        let open_report = db.open_report().clone();
        let clock = deps.clock.clone();
        let settings_store = DbSettingsStore {
            db: db.clone(),
            clock: clock.clone(),
        };
        // A settings document that does not parse is backed up verbatim
        // before defaults take its place; the platform gets an `Error` at
        // start and synced settings are not broadcast until a user edit.
        let (settings, settings_corrupt) = match settings_store.load() {
            Ok(Some(json)) => match Settings::from_json(&json) {
                Ok(s) => {
                    // An older document was migrated on load: keep the
                    // original, then persist the migrated one.
                    if let Some(v) = Settings::document_version(&json)
                        .filter(|v| *v < crate::settings::SETTINGS_DOC_VERSION)
                    {
                        let key = format!("settings.v{v}-backup");
                        match db.saved_state_set_raw(&key, &json, clock.as_ref()) {
                            Ok(()) => {
                                if let Err(e) = s.save(&settings_store) {
                                    tracing::warn!(error = %e, "saving migrated settings");
                                }
                            }
                            Err(e) => {
                                tracing::warn!(error = %e, "backing up settings before migrating")
                            }
                        }
                    }
                    (s, None)
                }
                Err(e) => {
                    let key = format!("settings.corrupt-{}", clock.now_ms() as i64);
                    let backed_up = db.saved_state_set_raw(&key, &json, clock.as_ref());
                    tracing::error!(error = %e, backup = %key, "settings document unreadable; starting from defaults");
                    let detail = match backed_up {
                        Ok(()) => format!("{e}; the original was kept as {key}"),
                        Err(b) => format!("{e}; backing up the original failed: {b}"),
                    };
                    (Settings::new(), Some(detail))
                }
            },
            Ok(None) => (Settings::new(), None),
            Err(e) => {
                tracing::error!(error = %e, "settings store unreadable; starting from defaults");
                (Settings::new(), Some(e.to_string()))
            }
        };
        let audio: AudioSettings = settings
            .get_typed(keys::AUDIO_SETTINGS)
            .unwrap_or_else(default_audio_settings);

        let mut registry = ActionRegistry::new(cfg.platform);
        load_registry_customisation(&mut registry, &settings);

        let jobs = JobQueue::new(db.clone(), clock.clone());
        let outbox = Outbox::new(db.clone(), clock.clone());
        let downloads = Downloads::new(
            db.clone(),
            clock.clone(),
            deps.storage.clone(),
            &data_dir,
            &cache_dir,
            cfg.platform,
        );
        let stream_reader = if cfg.platform == Platform::Coordinator {
            None
        } else {
            let upstream: Option<Arc<dyn super::stream_reader::StreamUpstream>> =
                match deps.stream_upstream.clone() {
                    Some(u) => Some(u),
                    None => match super::stream_reader::ReqwestUpstream::new() {
                        Ok(u) => Some(Arc::new(u)),
                        Err(e) => {
                            tracing::error!(error = %e, "stream reader http client");
                            None
                        }
                    },
                };
            let notify_tx = tx.clone();
            let notify: super::stream_reader::CacheNotify = Arc::new(move |tracks| {
                let _ = notify_tx.send(ActorMsg::Internal(Internal::StreamCacheChanged { tracks }));
            });
            upstream.map(|u| {
                super::stream_reader::StreamReader::new(downloads.clone(), clock.clone(), u, notify)
            })
        };
        let caches = Caches::new(db.clone(), clock.clone(), &cache_dir);
        let seed = deps.seed.unwrap_or_else(|| db.random_seed().unsigned_abs());
        let entropy: Arc<dyn Entropy> = match deps.seed {
            Some(s) => Arc::new(DeterministicEntropy::new(s)),
            None => Arc::new(SystemEntropy),
        };
        let autoplay_settings = settings
            .get_typed(keys::AUTOPLAY_SETTINGS)
            .unwrap_or_else(crate::autoplay::default_settings);
        let mut autoplay = AutoplayEngine::new(autoplay_settings, seed);
        if let Ok(Some(ids)) = db.saved_state_get::<Vec<TrackId>>("autoplay:exclusion") {
            autoplay.restore_exclusion(ids);
        }
        let reducer_policy = ReducerPolicy::new(
            settings.get_i64(keys::QUEUE_HISTORY_CAP).max(10) as usize,
            crate::session::SavedQueuePolicy::default()
                .with_cap(settings.get_i64(keys::QUEUE_SAVED_CAP).max(0) as u32),
        );

        let (backend, backend_poll, external): (
            Arc<dyn PlaybackBackend>,
            Option<PollFn>,
            Option<Arc<ExternalBackend>>,
        ) = match deps.backend {
            BackendChoice::Provided {
                backend,
                sink: _,
                poll,
            } => (backend, poll, None),
            BackendChoice::Auto => {
                let report_tx = tx.clone();
                let sink: crate::audio::backend::ReportSink = Arc::new(move |r: BackendReport| {
                    let _ = report_tx.send(ActorMsg::Internal(Internal::Backend(r)));
                });
                match cfg.audio {
                    AudioMode::External => {
                        let cmd_tx = tx.clone();
                        let commands: crate::audio::external::CommandSink =
                            Arc::new(move |c: BackendCommand| {
                                let _ =
                                    cmd_tx.send(ActorMsg::Internal(Internal::BackendCommand(c)));
                            });
                        let ext = Arc::new(ExternalBackend::new(commands, sink));
                        (ext.clone() as Arc<dyn PlaybackBackend>, None, Some(ext))
                    }
                    AudioMode::Native => (
                        native_backend(&rt, &audio, sink, stream_reader.clone()),
                        None,
                        None,
                    ),
                    AudioMode::None => (Arc::new(NullBackend::new()), None, None),
                }
            }
        };

        let core_stream = cfg.audio == AudioMode::Native;
        let volume: f64 = db
            .saved_state_get::<f64>("volume")
            .ok()
            .flatten()
            .unwrap_or(1.0);
        let now = clock.now_ms();
        let mut actor = Actor {
            cfg,
            clock: clock.clone(),
            rt,
            tx,
            sinks,
            processed,
            db,
            settings,
            settings_store,
            settings_corrupt,
            registry,
            undo: UndoStack::new("", crate::undo::DEFAULT_MAX_BYTES),
            jobs,
            outbox,
            downloads,
            stream_reader,
            core_stream,
            prefetch: Default::default(),
            prime: Default::default(),
            cache: Default::default(),
            caches,
            scrobbler: Scrobbler::new(clock.clone()),
            sleep: SleepTimerMachine::new(clock),
            autoplay: Some(Box::new(autoplay)),
            autoplay_generation: 0,
            autoplay_wanted: false,
            reducer_policy,
            entropy,
            backend,
            backend_poll,
            external,
            audio,
            output_devices: vec![],
            io: deps.io,
            peer_ids: Arc::new(PeerIds::default()),
            conns: HashMap::new(),
            listener_port: None,
            listener_stop: None,
            discovery: None,
            lyrics_http: deps.lyrics_http,
            server: None,
            pending_servers: HashMap::new(),
            engine: None,
            scope: None,
            queued_inputs: VecDeque::new(),
            pending_effects: None,
            last_doc: None,
            last_saved_queues: vec![],
            last_transport: TransportState::default(),
            pending_submits: HashMap::new(),
            pending_cas: HashMap::new(),
            outbox_entries: HashMap::new(),
            playback: Playback {
                volume,
                ..Default::default()
            },
            network: None,
            visible: true,
            focused: true,
            battery_saver: false,
            selection: ActionTarget::None,
            sync_progress: None,
            resume_offer: None,
            picker_open: false,
            picker_targets: vec![],
            media_art: None,
            lyrics_for: None,
            media_art_pending: None,
            log_ring: VecDeque::new(),
            started: false,
            shutting_down: false,
            in_flight: 0,
            flush_in_flight: false,
            doc_dirty_since: None,
            position_saved_at: now,
            last_engine_tick: 0.0,
            last_outbox_retry: now,
            last_sync_check: 0.0,
            last_meta_prune: now,
            open_report,
        };
        actor.undo = UndoStack::new(actor.cfg.device_id.clone(), crate::undo::DEFAULT_MAX_BYTES);
        actor.apply_storage_settings();
        actor.apply_transcoding_settings();
        if let Some(preset) = deps.server {
            actor.attach_preset_server(preset);
        }
        Ok(actor)
    }

    // -- loop -----------------------------------------------------------------

    pub async fn run(mut self, mut rx: mpsc::UnboundedReceiver<ActorMsg>) {
        // Shutdown acknowledgements, signalled after the flush.
        let mut acks: Vec<tokio::sync::oneshot::Sender<()>> = Vec::new();
        while let Some(msg) = rx.recv().await {
            let stop = self.handle_msg(msg, &mut acks);
            self.processed.fetch_add(1, Ordering::SeqCst);
            if stop {
                break;
            }
        }
        self.shutdown();
        // Anything queued behind the shutdown is dropped; further shutdown
        // requests are acknowledged too, since the flush has happened.
        rx.close();
        while let Ok(msg) = rx.try_recv() {
            if let ActorMsg::Shutdown(ack) = msg {
                acks.push(ack);
            }
        }
        for ack in acks {
            let _ = ack.send(());
        }
    }

    fn handle_msg(
        &mut self,
        msg: ActorMsg,
        acks: &mut Vec<tokio::sync::oneshot::Sender<()>>,
    ) -> bool {
        match msg {
            ActorMsg::Command(Command::Shutdown) => {
                self.shutting_down = true;
                return true;
            }
            ActorMsg::Shutdown(ack) => {
                self.shutting_down = true;
                acks.push(ack);
                return true;
            }
            ActorMsg::Command(cmd) => self.handle_command(cmd),
            ActorMsg::Query(q, reply) => self.answer_query(q, reply),
            ActorMsg::Internal(i) => self.handle_internal(i),
        }
        false
    }

    pub(crate) fn handle_internal(&mut self, internal: Internal) {
        match internal {
            Internal::Tick => self.on_tick(),
            Internal::Backend(report) => self.on_backend_report(report),
            Internal::BackendCommand(command) => self.emit(Event::Backend { command }),
            Internal::Jobs(ev) => match ev {
                QueueEvent::JobsChanged(jobs) => {
                    self.on_jobs_changed(&jobs);
                    self.emit(Event::JobsChanged { jobs });
                }
                QueueEvent::ProblemsChanged(problems) => {
                    self.emit(Event::ProblemsChanged { problems })
                }
            },
            Internal::SyncProgress(p) => self.on_sync_progress(p),
            Internal::Probed { server_id, result } => self.on_probed(server_id, result),
            Internal::Connected { peer, url, tx } => {
                self.conns.insert(peer.clone(), tx);
                self.engine_input(Input::Connected { peer, url });
            }
            Internal::ConnectFailed { error } => {
                self.engine_input(Input::ConnectFailed { error });
            }
            Internal::WireIn { peer, msg } => self.engine_input(Input::WireIn { peer, msg }),
            Internal::Disconnected { peer } => {
                self.conns.remove(&peer);
                self.engine_input(Input::Disconnected { peer });
            }
            Internal::ListenerStarted { port } => {
                self.listener_port = Some(port);
                self.engine_input(Input::ListenerStarted { port });
            }
            Internal::ListenerFailed { error } => {
                self.log("warn", format!("LAN listener failed: {error}"));
                self.listener_port = None;
                self.engine_input(Input::ListenerStopped);
            }
            Internal::PeerAccepted { peer, tx } => {
                self.conns.insert(peer.clone(), tx);
                self.engine_input(Input::PeerConnected { peer });
            }
            Internal::Discovery(ev) => match ev {
                crate::connect::discovery::DiscoveryEvent::Found(a) => {
                    if a.device_id != self.cfg.device_id {
                        self.engine_input(Input::PeerDiscovered(a));
                    }
                }
                crate::connect::discovery::DiscoveryEvent::Lost { device_id } => {
                    self.engine_input(Input::PeerLost { device_id });
                }
            },
            Internal::CredentialVerified { peer, ok } => {
                self.engine_input(Input::CredentialVerified { peer, ok });
            }
            Internal::Autoplay {
                engine,
                picks,
                generation,
            } => self.on_autoplay_picks(engine, picks, generation),
            Internal::ServerSearch { results } => self.emit(Event::SearchResults { results }),
            Internal::LyricsFetched {
                server_id,
                track_id,
                lyrics,
                announce,
            } => self.on_lyrics_fetched(server_id, track_id, lyrics, announce),
            Internal::Artwork { id, size, path } => self.on_artwork(id, size, path),
            Internal::OutboxFlushed { report } => self.on_outbox_flushed(report),
            Internal::CasResult { entry_id, result } => self.on_cas_result(entry_id, result),
            Internal::NspWritten {
                filter_id,
                document,
                path,
                error,
            } => {
                if let Some(e) = error {
                    self.toast(format!("Export failed: {e}"), None);
                } else {
                    self.emit(Event::NspExported {
                        filter_id,
                        document,
                        path,
                    });
                }
            }
            Internal::PlaylistCreated { server_id } => {
                self.emit(Event::LibraryChanged {
                    server_id,
                    tables: vec!["playlists".into()],
                    ids: vec![],
                });
            }
            Internal::Toast { message } => self.toast(message, None),
            Internal::StreamCacheChanged { tracks } => self.on_stream_cache_changed(tracks),
            Internal::PrefetchDone {
                track_id,
                generation,
                outcome,
            } => self.on_prefetch_done(track_id, generation, outcome),
            Internal::SkipOffline { key } => self.on_skip_offline(key),
            Internal::ClearOfflineSkips => self.clear_offline_skips(),
            Internal::RetryOfflineClear => self.run_offline_clear(false),
            Internal::PrimeDone { generation } => self.on_prime_done(generation),
            Internal::TaskDone => {
                self.in_flight = self.in_flight.saturating_sub(1);
            }
            Internal::Barrier(reply) => {
                let busy = self.in_flight > 0 || self.flush_in_flight || self.jobs_busy();
                let _ = reply.send(busy);
            }
        }
    }

    fn jobs_busy(&self) -> bool {
        self.jobs
            .jobs()
            .map(|j| {
                j.iter()
                    .any(|j| matches!(j.state, JobState::Running | JobState::Queued))
            })
            .unwrap_or(false)
    }

    /// Spawn a short task and count it (tests wait for it in `settle`).
    /// `TaskDone` is sent from a drop guard, so a task that panics still
    /// decrements `in_flight`.
    pub(crate) fn spawn<F>(&mut self, fut: F)
    where
        F: std::future::Future<Output = ()> + Send + 'static,
    {
        self.in_flight += 1;
        let guard = TaskDoneGuard(self.tx.clone());
        self.rt.spawn(async move {
            let _guard = guard;
            fut.await;
        });
    }

    // -- start / shutdown -------------------------------------------------------

    pub(crate) fn start(&mut self) {
        if self.started {
            self.emit_everything(false);
            return;
        }
        self.started = true;
        if let Some(detail) = self.settings_corrupt.clone() {
            self.error(
                ErrorKind::Storage,
                "Settings could not be read; defaults are in effect",
                Some(detail),
            );
        }
        if self.cfg.coordinator_listen.is_some() {
            // The hosted coordinator role is served by `hocket-coordinator`,
            // which drives the same `connect::Room`; a core in this mode only
            // holds session state and never touches audio or credentials.
            self.log(
                "info",
                "coordinator mode: no audio, no credentials, room served by hocket-coordinator",
            );
        }
        // Servers (metadata only; credentials arrive with AddServer).
        if self.server.is_none() {
            match self.db.servers() {
                Ok(list) => {
                    if let Some(info) = list.into_iter().next() {
                        self.server = Some(ServerState::new(info));
                    }
                }
                Err(e) => self.error(ErrorKind::Storage, "load servers", Some(e.to_string())),
            }
        }
        if self.server.is_some() && self.engine.is_none() {
            self.open_session();
        }
        // Jobs and outbox.
        self.register_job_runners();
        if let Err(e) = self.jobs.recover() {
            self.error(ErrorKind::Storage, "job recovery", Some(e.to_string()));
        }
        if let Err(e) = self.outbox.recover() {
            self.error(ErrorKind::Storage, "outbox recovery", Some(e.to_string()));
        }
        let tx = self.tx.clone();
        self.jobs.on_change(move |ev| {
            let _ = tx.send(ActorMsg::Internal(Internal::Jobs(ev)));
        });
        self.jobs.start(&self.rt);
        self.apply_audio_to_backend();
        if let Err(e) = self.backend.set_volume(self.effective_volume()) {
            self.log("debug", format!("set_volume: {e}"));
        }
        self.output_devices = self.backend.output_devices();
        self.emit_everything(true);
        // Kick the connect tier selection, library sync and the outbox.
        self.engine_input(Input::Tick);
        self.last_engine_tick = self.now();
        self.maybe_start_sync(false, true);
        self.schedule_flush();
    }

    fn shutdown(&mut self) {
        self.flush_persistence(true);
        self.save_position();
        if let Err(e) = self.settings.save(&self.settings_store) {
            tracing::warn!(error = %e, "settings save on shutdown");
        }
        if let Some(a) = &self.autoplay {
            let _ = self.db.saved_state_set(
                "autoplay:exclusion",
                &a.exclusion_ids(),
                self.clock.as_ref(),
            );
        }
        if let Some(d) = &mut self.discovery {
            d.stop();
        }
        if let Some(stop) = self.listener_stop.take() {
            let _ = stop.send(());
        }
        let _ = self.backend.stop();
        self.cancel_prefetch();
        self.cancel_prime();
        if let Some(r) = &self.stream_reader {
            r.stop();
        }
        self.downloads.save_traffic();
        tracing::info!("core stopped");
    }

    /// Re-emit every `*Changed` event with current state, led by `Started`
    /// (the one time `Start` completes) or `Snapshot` (every other replay).
    pub(crate) fn emit_everything(&mut self, first: bool) {
        let snapshot = self.snapshot();
        if first {
            self.emit(Event::Started {
                snapshot: snapshot.clone(),
            });
        } else {
            self.emit(Event::Snapshot {
                snapshot: snapshot.clone(),
            });
        }
        self.emit(Event::ServersChanged {
            servers: snapshot.servers.clone(),
        });
        if let Some(doc) = &snapshot.session {
            self.emit(Event::SessionChanged {
                document: doc.clone(),
            });
        }
        self.emit(Event::QueueChanged {
            queue: snapshot.queue.clone(),
        });
        self.emit(Event::NowPlayingChanged {
            entry: snapshot.queue.current.clone(),
        });
        self.emit(Event::TransportChanged {
            transport: snapshot.transport.clone(),
        });
        self.emit(Event::SavedQueuesChanged {
            queues: snapshot
                .session
                .as_ref()
                .map(|d| d.saved_queues.clone())
                .unwrap_or_default(),
        });
        self.emit(Event::UndoChanged {
            state: snapshot.undo.clone(),
        });
        self.emit(Event::JobsChanged {
            jobs: snapshot.jobs.clone(),
        });
        self.emit(Event::ProblemsChanged {
            problems: snapshot.problems.clone(),
        });
        self.emit(Event::ConnectionChanged {
            state: snapshot.connection.clone(),
        });
        self.emit(Event::NetworkChanged {
            network: snapshot.network.clone(),
        });
        self.emit(Event::DevicesChanged {
            devices: snapshot.devices.clone(),
        });
        self.emit(Event::ResumeOfferChanged {
            offer: snapshot.resume_offer.clone(),
        });
        self.emit(Event::HandoffPickerChanged {
            open: self.picker_open,
            targets: self.picker_targets.clone(),
        });
        let pins = self.pins();
        self.emit(Event::PinsChanged { pins });
        let storage = self.storage_summary();
        self.emit(Event::StorageChanged { storage });
        let filters = self.filters();
        self.emit(Event::FiltersChanged { filters });
        for setting in snapshot.settings.clone() {
            self.emit(Event::SettingChanged { setting });
        }
        self.emit(Event::AudioSettingsChanged {
            settings: snapshot.audio.clone(),
        });
        self.emit(Event::OutputDevicesChanged {
            devices: self.output_devices.clone(),
        });
        self.emit(Event::SleepTimerChanged {
            timer: snapshot.sleep_timer.clone(),
        });
        let shortcuts = self.registry.shortcuts();
        self.emit(Event::ShortcutsChanged { shortcuts });
        for surface in crate::actions::Surface::ALL {
            self.emit(Event::ActionsChanged {
                surface: surface.as_str().into(),
            });
        }
        if let Some(p) = &snapshot.sync_progress {
            self.emit(Event::SyncProgress {
                progress: p.clone(),
            });
        }
        self.emit(Event::MediaSession {
            state: snapshot.media_session,
        });
    }

    // -- tick -----------------------------------------------------------------

    fn on_tick(&mut self) {
        let now = self.now();
        // Virtual-time backends emit what fell due.
        if let Some(poll) = self.backend_poll.clone() {
            for r in poll() {
                // The sink already delivered these onto the channel in
                // production; the scripted backend also returns them, so the
                // poll path delivers directly to keep the order tight.
                let _ = r;
            }
        }
        if !self.started {
            return;
        }
        if now - self.last_engine_tick >= ENGINE_TICK_MS - 1.0 {
            self.last_engine_tick = now;
            self.engine_input(Input::Tick);
        }
        self.prefetch_tick(now);
        self.prime_next();
        self.cache_tick(now);
        // Sleep timer.
        let action = self.sleep.tick();
        self.apply_sleep_action(action);
        // Persistence.
        self.flush_persistence(false);
        if self.playback.playing && now - self.position_saved_at >= POSITION_HEARTBEAT_MS {
            self.save_position();
        }
        // Outbox retry.
        if now - self.last_outbox_retry >= OUTBOX_RETRY_MS {
            self.last_outbox_retry = now;
            if self.outbox.pending_count().unwrap_or(0) > 0 {
                self.schedule_flush();
            }
        }
        // Library sync timer (checked once a minute).
        if now - self.last_sync_check >= 60_000.0 {
            self.last_sync_check = now;
            self.maybe_start_sync(false, false);
        }
        // Housekeeping.
        if now - self.last_meta_prune >= 3_600_000.0 {
            self.last_meta_prune = now;
            self.housekeeping();
        }
    }

    /// Hourly: prune cache metadata, finished outbox entries and terminal
    /// jobs so the database does not grow with listening history.
    pub(crate) fn housekeeping(&mut self) {
        if let Err(e) = self.caches.meta_prune() {
            self.log("warn", format!("cache prune: {e}"));
        }
        if let Err(e) = self.outbox.prune(OUTBOX_RETENTION) {
            self.log("warn", format!("outbox prune: {e}"));
        }
        if let Err(e) = self.jobs.prune() {
            self.log("warn", format!("job prune: {e}"));
        }
        // The OS may have cleared cache files behind our back.
        match self.downloads.reconcile_stream_cache() {
            Ok(changed) if !changed.is_empty() => self.on_stream_cache_changed(changed),
            Ok(_) => {}
            Err(e) => self.log("warn", format!("stream cache reconcile: {e}")),
        }
        self.downloads.save_traffic();
    }

    // -- persistence ----------------------------------------------------------

    pub(crate) fn mark_doc_dirty(&mut self) {
        if self.doc_dirty_since.is_none() {
            self.doc_dirty_since = Some(self.now());
        }
    }

    pub(crate) fn flush_persistence(&mut self, force: bool) {
        let Some(since) = self.doc_dirty_since else {
            return;
        };
        if !force && self.now() - since < DOC_SAVE_DEBOUNCE_MS {
            return;
        }
        self.doc_dirty_since = None;
        let Some(engine) = &self.engine else { return };
        let Some(scope) = self.scope.clone() else {
            return;
        };
        let sync_base = engine.sync_base().cloned();
        let doc_json = match crate::session::save(engine.document()) {
            Ok(json) => json,
            Err(e) => {
                self.error(
                    ErrorKind::Internal,
                    "serialise session",
                    Some(e.to_string()),
                );
                return;
            }
        };
        let state = PersistedConnectState {
            sync_base,
            known_scrobbled: engine.known_scrobbled(),
        };
        let state_json = match serde_json::to_string(&state) {
            Ok(j) => j,
            Err(e) => {
                self.error(
                    ErrorKind::Internal,
                    "serialise connect state",
                    Some(e.to_string()),
                );
                return;
            }
        };
        // One transaction: the document and the sync base it was classified
        // against are never seen apart after a crash.
        let now = self.now();
        let r = self.db.with_tx(|tx| {
            for (key, json) in [
                (format!("session:{scope}"), &doc_json),
                (format!("connect:{scope}"), &state_json),
            ] {
                tx.execute(
                    "INSERT INTO saved_state(key, json, updated_at) VALUES (?1, ?2, ?3)
                     ON CONFLICT(key) DO UPDATE SET json = excluded.json, updated_at = excluded.updated_at",
                    rusqlite::params![key, json, now],
                )?;
            }
            Ok(())
        });
        if let Err(e) = r {
            self.error(ErrorKind::Storage, "save session", Some(e.to_string()));
        }
    }

    pub(crate) fn save_position(&mut self) {
        self.position_saved_at = self.now();
        let Some(scope) = self.scope.clone() else {
            return;
        };
        let pos = SavedPosition {
            key: self.playback.doc_key.clone(),
            position_ms: self.playback.position_now(self.now()),
        };
        if let Err(e) =
            self.db
                .saved_state_set(&format!("position:{scope}"), &pos, self.clock.as_ref())
        {
            self.log("warn", format!("save position: {e}"));
        }
    }

    pub(crate) fn save_settings(&mut self) {
        if let Err(e) = self.settings.save(&self.settings_store) {
            self.error(ErrorKind::Storage, "save settings", Some(e.to_string()));
        }
    }

    // -- helpers --------------------------------------------------------------

    pub(crate) fn now(&self) -> f64 {
        self.clock.now_ms()
    }

    pub(crate) fn session_now(&self) -> f64 {
        self.engine
            .as_ref()
            .map(|e| e.now_session_ms())
            .unwrap_or_else(|| self.now())
    }

    pub(crate) fn emit(&self, event: Event) {
        let sinks = self.sinks.lock().clone();
        for s in sinks {
            s.on_event(event.clone());
        }
    }

    pub(crate) fn log(&mut self, level: &str, message: impl Into<String>) {
        let message = message.into();
        match level {
            "error" => tracing::error!(target: "hocket_core", "{message}"),
            "warn" => tracing::warn!(target: "hocket_core", "{message}"),
            "info" => tracing::info!(target: "hocket_core", "{message}"),
            _ => tracing::debug!(target: "hocket_core", "{message}"),
        }
        self.log_ring
            .push_back(format!("[{:.0}] {level}: {message}", self.now()));
        while self.log_ring.len() > LOG_RING {
            self.log_ring.pop_front();
        }
        if level == "error" || level == "warn" {
            self.emit(Event::Log {
                level: level.into(),
                target: "hocket_core".into(),
                message,
            });
        }
    }

    pub(crate) fn error(&mut self, kind: ErrorKind, message: &str, detail: Option<String>) {
        self.log(
            "error",
            format!("{message}: {}", detail.clone().unwrap_or_default()),
        );
        self.emit(Event::Error {
            kind,
            message: message.into(),
            detail,
        });
    }

    /// A toast: only for an immediate failure of something the user just did,
    /// and for undo.
    /// A player-bar notice: the core's English `message` with its stable
    /// `code` (and variable `detail`) for platforms that localise.
    pub(crate) fn player_notice(
        &mut self,
        code: PlayerNoticeCode,
        message: impl Into<String>,
        detail: Option<String>,
    ) {
        self.emit(Event::PlayerNotice {
            message: Some(message.into()),
            code: Some(code),
            detail,
        });
    }

    /// Clears the player-bar notice.
    pub(crate) fn clear_player_notice(&mut self) {
        self.emit(Event::PlayerNotice {
            message: None,
            code: None,
            detail: None,
        });
    }

    pub(crate) fn toast(&mut self, message: impl Into<String>, action: Option<(String, Command)>) {
        let (action_label, action_command) = match action {
            Some((l, c)) => (Some(l), serde_json::to_string(&c).ok()),
            None => (None, None),
        };
        self.emit(Event::Toast {
            toast: Toast {
                id: crate::util::new_id(),
                message: message.into(),
                action_label,
                action_command,
                duration_ms: 5_000,
            },
        });
    }

    pub(crate) fn server_id(&self) -> Option<String> {
        self.server.as_ref().map(|s| s.info.id.clone())
    }

    pub(crate) fn api(&self) -> Option<Arc<dyn crate::subsonic::SubsonicApi>> {
        self.server.as_ref().and_then(|s| s.api.clone())
    }

    pub(crate) fn effective_volume(&self) -> f64 {
        (self.playback.volume * self.playback.fade_gain.unwrap_or(1.0)).clamp(0.0, 1.0)
    }

    pub(crate) fn snapshot(&mut self) -> Snapshot {
        let servers = self.server_infos();
        let session = self.engine.as_ref().map(|e| e.document().clone());
        let queue = self.queue_view();
        let transport = self.transport_state();
        let (connection, devices) = match &self.engine {
            Some(e) => (e.connection_state(), e.devices()),
            None => (ConnectionState::default(), vec![]),
        };
        let media_session = self.media_session_state(&queue, &transport);
        Snapshot {
            servers,
            session,
            queue,
            transport,
            connection,
            devices,
            jobs: self.jobs.jobs().unwrap_or_default(),
            problems: self.jobs.problems().unwrap_or_default(),
            undo: self.undo.state(),
            settings: self.settings.to_api(),
            audio: self.audio.clone(),
            media_session,
            resume_offer: self.resume_offer.clone(),
            sleep_timer: self.sleep.state(),
            network: self.network.clone(),
            battery_saver: self.battery_saver,
            sync_progress: self.sync_progress.clone(),
        }
    }
}

/// Registry customisation lives in the settings document (`shortcuts` and
/// `actions.order.<surface>`).
fn load_registry_customisation(registry: &mut ActionRegistry, settings: &Settings) {
    let mut c = crate::actions::ActionCustomisation::default();
    if let Some(map) = settings.get(keys::SHORTCUTS).as_object() {
        for (id, v) in map {
            c.shortcuts
                .insert(id.clone(), v.as_str().map(str::to_string));
        }
    }
    for surface in crate::actions::Surface::ALL {
        let key = keys::action_order(surface.as_str());
        if let Some(list) = settings.get(&key).as_array() {
            let ids: Vec<String> = list
                .iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect();
            if !ids.is_empty() {
                c.orders.insert(surface.as_str().to_string(), ids);
            }
        }
    }
    registry.apply_customisation(&c);
}

#[cfg(feature = "native-audio")]
fn native_backend(
    rt: &tokio::runtime::Handle,
    audio: &AudioSettings,
    sink: crate::audio::backend::ReportSink,
    reader: Option<super::stream_reader::StreamReader>,
) -> Arc<dyn PlaybackBackend> {
    use crate::audio::native::{
        NativeBackend, NativeConfig, OutputConfig, RangeFetcher, ReqwestFetcher,
    };
    let http: Arc<dyn RangeFetcher> = Arc::new(ReqwestFetcher::default());
    let fetcher: Arc<dyn RangeFetcher> = match reader {
        Some(reader) => Arc::new(super::stream_reader::CoreStreamFetcher {
            reader,
            fallback: http,
        }),
        None => http,
    };
    let cfg = NativeConfig {
        fetcher,
        runtime: rt.clone(),
        output: OutputConfig {
            device_id: audio.output_device.clone(),
            sample_rate: None,
            null_sink: false,
        },
        audio: audio.clone(),
        watch_devices: true,
    };
    match NativeBackend::new(cfg, sink) {
        Ok(b) => Arc::new(b),
        Err(e) => {
            tracing::error!(error = %e, "native audio unavailable; playback disabled");
            Arc::new(NullBackend::new())
        }
    }
}

#[cfg(not(feature = "native-audio"))]
fn native_backend(
    _rt: &tokio::runtime::Handle,
    _audio: &AudioSettings,
    _sink: crate::audio::backend::ReportSink,
    _reader: Option<super::stream_reader::StreamReader>,
) -> Arc<dyn PlaybackBackend> {
    tracing::error!(
        "AudioMode::Native requested but the native-audio feature is off; playback disabled"
    );
    Arc::new(NullBackend::new())
}
