//! A simulated device: a real [`Engine`] with the real session reducer, plus
//! the thinnest possible model of the actor and the playback backend around
//! it. It reacts to every engine [`Output`] the way the real actor would and
//! turns the I/O ones into [`DeviceEffect`]s for the world to perform.

use std::collections::HashMap;
use std::sync::Arc;

use crate::api::{DeviceId, DeviceInfo, EpochMs, Ms, Platform, PositionStamp, QueueKey, SavedQueue, TrackId};
use crate::connect::discovery::PeerAdvert;
use crate::connect::engine::{Engine, EngineConfig, Input, Output, ResumeOfferDraft};
use crate::connect::session_adapter::RealReducer;
use crate::connect::wire::{Credential, TransportCommand, WireMessage};
use crate::connect::{PeerId, SessionOp, SyncPoint};
use crate::sim::clock::DeviceClock;
use crate::util::Clock;

/// Scrobble rule: 50% or 4 minutes, whichever first (Last.fm).
pub fn scrobble_threshold_ms(duration_ms: Ms) -> Ms {
    (duration_ms / 2).min(240_000)
}

/// Tracks the simulated library knows about, with durations.
#[derive(Debug, Clone, Default)]
pub struct Library {
    pub durations: HashMap<TrackId, Ms>,
}

impl Library {
    pub fn synthetic(n: usize) -> Library {
        let mut durations = HashMap::new();
        for i in 0..n {
            // 20 s .. 5 min, deterministic
            let d = 20_000 + ((i as u32 * 37_919) % 280_000);
            durations.insert(format!("t{i}"), d);
        }
        Library { durations }
    }

    pub fn duration(&self, id: &str) -> Ms {
        self.durations.get(id).copied().unwrap_or(180_000)
    }

    pub fn ids(&self) -> Vec<TrackId> {
        let mut v: Vec<TrackId> = self.durations.keys().cloned().collect();
        v.sort();
        v
    }
}

/// What the world must do for a device.
#[derive(Debug, Clone, PartialEq)]
pub enum DeviceEffect {
    Connect { candidates: Vec<String> },
    Send { peer: PeerId, msg: WireMessage },
    Close { peer: PeerId },
    StartListener,
    StopListener,
    Advertise(Option<PeerAdvert>),
    Verify { peer: PeerId, credential: Credential },
    /// Submit to the (fake) Navidrome.
    Scrobble { track_id: TrackId, started_at: EpochMs },
}

/// The backend model: what is loaded and where it is.
#[derive(Debug, Clone, Default)]
pub struct Playback {
    pub key: Option<QueueKey>,
    pub track_id: Option<TrackId>,
    pub duration_ms: Ms,
    pub position_ms: Ms,
    pub position_at: EpochMs,
    pub playing: bool,
    pub played_ms: Ms,
    pub started_at: EpochMs,
    pub scrobbled: bool,
}

/// State the real app persists; survives a simulated crash.
#[derive(Debug, Clone)]
pub struct Persisted {
    pub document: crate::api::SessionDocument,
    pub sync_base: Option<SyncPoint>,
    pub saved_queues: Vec<SavedQueue>,
    /// Scrobbles reached but without a verdict yet (the outbox).
    pub outbox: Vec<(TrackId, EpochMs)>,
}

pub struct SimDevice {
    pub id: DeviceId,
    pub engine: Engine,
    pub clock: Arc<DeviceClock>,
    pub library: Arc<Library>,
    pub playback: Playback,
    pub saved_queues: Vec<SavedQueue>,
    pub outbox: Vec<(TrackId, EpochMs)>,
    pub resume_offer: Option<ResumeOfferDraft>,
    pub picker_open: bool,
    pub picker_targets: Vec<DeviceInfo>,
    pub filed_count: u32,
    pub scrobbles_reached: Vec<(TrackId, EpochMs)>,
    pub listener_port: Option<u16>,
    pub asleep: bool,
    prebuffer_ready_at: Option<(EpochMs, QueueKey)>,
    next_tick_at: EpochMs,
    queued: Vec<Input>,
    effects: Vec<DeviceEffect>,
    id_counter: u32,
    pub log: Vec<String>,
    pub keep_log: bool,
}

impl std::fmt::Debug for SimDevice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SimDevice").field("id", &self.id).field("engine", &self.engine).finish()
    }
}

pub fn device_info(id: &str) -> DeviceInfo {
    DeviceInfo {
        id: id.into(),
        name: format!("Device {id}"),
        platform: Platform::Linux,
        app_version: "sim".into(),
        playing: false,
        ready: false,
        last_seen: 0.0,
        is_self: true,
    }
}

impl SimDevice {
    pub fn new(
        id: &str,
        scope: &str,
        clock: Arc<DeviceClock>,
        library: Arc<Library>,
        coordinator_url: Option<String>,
        lan: bool,
        persisted: Option<Persisted>,
    ) -> Self {
        let mut cfg = EngineConfig::new(device_info(id), scope);
        cfg.coordinator_url = coordinator_url;
        cfg.lan_enabled = lan;
        cfg.credential = Some(Credential {
            server_url: "https://music.example".into(),
            username: "user".into(),
            token: Some("tok".into()),
            salt: Some("salt".into()),
            api_key: None,
            client: "hocket-sim".into(),
            api_version: "1.16.1".into(),
        });
        let now = clock.now_ms();
        let (doc, sync_base, saved, outbox) = match persisted {
            Some(p) => (p.document, p.sync_base, p.saved_queues, p.outbox),
            None => (crate::session::new_document(scope, format!("session-{id}"), now), None, vec![], vec![]),
        };
        let world_start = clock.world_now_ms();
        let engine = Engine::new(cfg, clock.clone(), RealReducer::shared(), doc, sync_base);
        let mut d = SimDevice {
            id: id.into(),
            engine,
            clock,
            library,
            playback: Playback::default(),
            saved_queues: saved.clone(),
            outbox: vec![],
            resume_offer: None,
            picker_open: false,
            picker_targets: vec![],
            filed_count: 0,
            scrobbles_reached: vec![],
            listener_port: None,
            asleep: false,
            prebuffer_ready_at: None,
            next_tick_at: world_start,
            queued: vec![],
            effects: vec![],
            id_counter: 0,
            log: vec![],
            keep_log: false,
        };
        if !saved.is_empty() {
            d.handle(Input::SavedQueuesChanged(saved));
        }
        // The outbox retries: ask again for every scrobble without a verdict.
        for (t, s) in outbox {
            d.outbox.push((t.clone(), s));
            d.handle(Input::ScrobbleReached { track_id: t, started_at: s });
        }
        // Kick the tier selection (the engine only connects on a config change or tick).
        d.handle(Input::Tick);
        d
    }

    pub fn persisted(&self) -> Persisted {
        Persisted {
            document: self.engine.document().clone(),
            sync_base: self.engine.sync_base().cloned(),
            saved_queues: self.saved_queues.clone(),
            outbox: self.outbox.clone(),
        }
    }

    /// Device-local time (skewed), what the engine sees.
    pub fn now(&self) -> EpochMs {
        self.clock.now_ms()
    }

    /// World time, for scheduling only.
    pub fn world_now(&self) -> EpochMs {
        self.clock.world_now_ms()
    }

    pub fn owns(&self) -> bool {
        self.engine.owns_transport()
    }

    pub fn take_effects(&mut self) -> Vec<DeviceEffect> {
        std::mem::take(&mut self.effects)
    }

    fn note(&mut self, s: String) {
        if self.keep_log {
            self.log.push(format!("[{:>9.0}] {}: {}", self.now(), self.id, s));
        }
    }

    /// Feed the engine and act on its outputs (recursively, for inputs the
    /// reactions generate).
    pub fn handle(&mut self, input: Input) {
        self.queued.push(input);
        let mut guard = 0;
        while !self.queued.is_empty() {
            let batch: Vec<Input> = std::mem::take(&mut self.queued);
            for input in batch {
                let outs = self.engine.handle(input);
                for o in outs {
                    self.react(o);
                }
            }
            guard += 1;
            assert!(guard < 1000, "device {} reaction loop", self.id);
        }
    }

    fn react(&mut self, o: Output) {
        match o {
            Output::WireOut { peer, msg } => self.effects.push(DeviceEffect::Send { peer, msg }),
            Output::Connect { candidates } => self.effects.push(DeviceEffect::Connect { candidates }),
            Output::Disconnect { peer } => self.effects.push(DeviceEffect::Close { peer }),
            Output::StartListener => self.effects.push(DeviceEffect::StartListener),
            Output::StopListener => self.effects.push(DeviceEffect::StopListener),
            Output::Advertise(a) => self.effects.push(DeviceEffect::Advertise(a)),
            Output::VerifyCredential { peer, credential } => self.effects.push(DeviceEffect::Verify { peer, credential }),
            Output::DocumentChanged { document, cause } => {
                self.note(format!("doc rev {} ({cause:?}) current={:?}", document.revision, document.current.as_ref().map(|c| &c.track_id)));
                if self.owns() {
                    let key = document.current.as_ref().map(|c| c.key.clone());
                    if key != self.playback.key {
                        match document.current.clone() {
                            Some(item) => self.load(item.key, item.track_id, 0, 0, self.engine.now_session_ms(), false, self.playback.playing),
                            None => {
                                self.playback = Playback::default();
                                self.stamp();
                            }
                        }
                    }
                }
            }
            Output::LeaseChanged { owns, detached, .. } => {
                self.note(format!("lease owns={owns} detached={detached}"));
                if owns {
                    if self.playback.key.is_none() {
                        if let Some(item) = self.engine.document().current.clone() {
                            self.load(item.key, item.track_id, 0, 0, self.engine.now_session_ms(), false, true);
                        }
                    } else if !self.playback.playing {
                        self.set_playing(true);
                    }
                } else {
                    self.set_playing(false);
                }
            }
            Output::TakeTransport { key, track_id, position_ms, played_ms, started_at, scrobbled, play } => {
                self.note(format!("take transport {track_id} @ {position_ms} played={played_ms}"));
                self.load(key, track_id, position_ms, played_ms, started_at, scrobbled, play);
            }
            Output::ReleaseTransport => {
                self.note("release transport".into());
                self.set_playing(false);
            }
            Output::TransportCommand(cmd) => self.transport_command(cmd),
            Output::PreBuffer { key, .. } => {
                self.prebuffer_ready_at = Some((self.world_now() + 500.0, key));
            }
            Output::DiscardPreBuffer => self.prebuffer_ready_at = None,
            Output::Scrobble { track_id, started_at, allowed } => {
                self.outbox.retain(|(t, s)| !(t == &track_id && *s == started_at));
                if allowed {
                    self.note(format!("scrobble {track_id} @ {started_at}"));
                    self.effects.push(DeviceEffect::Scrobble { track_id, started_at });
                } else {
                    self.note(format!("scrobble {track_id} @ {started_at} was a duplicate"));
                }
            }
            Output::FilePreviousStateAsSavedQueue { document, reason } => {
                self.note(format!("filing previous state as saved queue: {reason}"));
                self.filed_count += 1;
                self.id_counter += 1;
                let id = format!("sq-{}-{}", self.id, self.id_counter);
                if let Some(mut sq) = crate::session::saved::snapshot(&document, self.playback.position_ms, self.now(), id) {
                    sq.updated_at = self.engine.now_session_ms();
                    sq.label = format!("{} (from {})", sq.label, self.id);
                    let (merged, _) = crate::connect::wire::merge_saved_queues(&self.saved_queues, &[sq]);
                    self.saved_queues = merged.clone();
                    self.queued.push(Input::SavedQueuesChanged(merged));
                }
            }
            Output::SavedQueuesMerged(list) => self.saved_queues = list,
            Output::ResumeOffer(o) => self.resume_offer = o,
            Output::PickerChanged { open, targets } => {
                self.picker_open = open;
                self.picker_targets = targets;
            }
            Output::ConnectionChanged(_)
            | Output::DevicesChanged(_)
            | Output::TransportChanged { .. }
            | Output::SettingsMerged(_)
            | Output::UndoEntryReceived { .. }
            | Output::ReplicaChanged(_)
            | Output::Log { .. } => {}
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn load(&mut self, key: QueueKey, track_id: TrackId, position_ms: Ms, played_ms: Ms, started_at: EpochMs, scrobbled: bool, play: bool) {
        let duration_ms = self.library.duration(&track_id);
        self.playback = Playback {
            key: Some(key),
            track_id: Some(track_id),
            duration_ms,
            position_ms,
            position_at: self.now(),
            playing: play,
            played_ms,
            started_at,
            scrobbled,
        };
        self.stamp();
    }

    fn set_playing(&mut self, playing: bool) {
        if self.playback.key.is_none() || self.playback.playing == playing {
            return;
        }
        self.advance();
        self.playback.playing = playing;
        self.stamp();
    }

    fn transport_command(&mut self, cmd: TransportCommand) {
        match cmd {
            TransportCommand::Play => self.set_playing(true),
            TransportCommand::Pause | TransportCommand::Stop => self.set_playing(false),
            TransportCommand::TogglePlay => {
                let p = self.playback.playing;
                self.set_playing(!p)
            }
            TransportCommand::SeekTo { position_ms } => {
                self.advance();
                self.playback.position_ms = position_ms.min(self.playback.duration_ms);
                self.stamp();
            }
            TransportCommand::SeekBy { delta_ms } => {
                self.advance();
                let p = self.playback.position_ms as i64 + delta_ms as i64;
                self.playback.position_ms = p.clamp(0, self.playback.duration_ms as i64) as Ms;
                self.stamp();
            }
            TransportCommand::SetVolume { .. } => {}
        }
    }

    /// Send a stamp: only ever from here, only on change.
    fn stamp(&mut self) {
        if !self.owns() {
            return;
        }
        let p = &self.playback;
        let input = Input::LocalStamp {
            key: p.key.clone(),
            track_id: p.track_id.clone(),
            position: PositionStamp { position_ms: p.position_ms, taken_at: p.position_at, rate: 1.0, is_playing: p.playing },
            played_ms: p.played_ms,
            started_at: p.started_at,
            scrobbled: p.scrobbled,
        };
        self.queued.push(input);
    }

    /// Move playback forward to now; detect track end and scrobble threshold.
    fn advance(&mut self) {
        let now = self.now();
        if !self.owns() || !self.playback.playing || self.playback.key.is_none() {
            self.playback.position_at = now;
            return;
        }
        let elapsed = (now - self.playback.position_at).max(0.0) as Ms;
        self.playback.position_at = now;
        self.playback.position_ms = self.playback.position_ms.saturating_add(elapsed);
        self.playback.played_ms = self.playback.played_ms.saturating_add(elapsed);
        if !self.playback.scrobbled && self.playback.played_ms >= scrobble_threshold_ms(self.playback.duration_ms) {
            self.playback.scrobbled = true;
            let t = self.playback.track_id.clone().expect("loaded");
            let s = self.playback.started_at;
            self.scrobbles_reached.push((t.clone(), s));
            self.outbox.push((t.clone(), s));
            self.queued.push(Input::ScrobbleReached { track_id: t, started_at: s });
            self.stamp();
        }
        if self.playback.position_ms >= self.playback.duration_ms {
            self.playback.position_ms = self.playback.duration_ms;
            self.playback.playing = false;
            self.queued.push(Input::LocalOp { op: SessionOp::TrackEnded });
            // If the document did not change (queue ran out) we stay stopped on the last item.
            self.playback.playing = true;
        }
    }

    /// Called by the world every scheduling step.
    pub fn step(&mut self) {
        if self.asleep {
            return;
        }
        let now = self.world_now();
        self.advance();
        if let Some((at, key)) = self.prebuffer_ready_at.clone() {
            if now >= at {
                self.prebuffer_ready_at = None;
                self.queued.push(Input::PreBufferReady { key });
            }
        }
        if now >= self.next_tick_at {
            self.next_tick_at = now + 1000.0;
            self.queued.push(Input::Tick);
        }
        if !self.queued.is_empty() {
            let first = self.queued.remove(0);
            self.handle(first);
        }
        // Playback may have run into the end of the queue: nothing to play.
        if self.owns() && self.engine.document().current.is_none() && self.playback.key.is_some() {
            self.playback = Playback::default();
        }
    }

    pub fn next_tick_at(&self) -> EpochMs {
        self.next_tick_at.min(self.prebuffer_ready_at.as_ref().map(|(t, _)| *t).unwrap_or(f64::MAX))
    }

    /// Current playback position (for assertions and snapshots).
    pub fn position_ms(&self) -> Ms {
        if self.playback.playing && self.owns() {
            self.playback.position_ms + (self.now() - self.playback.position_at).max(0.0) as Ms
        } else {
            self.playback.position_ms
        }
    }
}
