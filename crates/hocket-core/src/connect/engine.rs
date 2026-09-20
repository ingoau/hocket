//! The Connect state machine. See the contract at the top of
//! [`crate::connect`].
//!
//! # Shape
//!
//! Every device has exactly one *upstream*: the room it submits ops to. That
//! is either its own embedded [`Room`] over an in-process loopback (tier 1,
//! alone; or tier 2 when this device is the elected LAN coordinator, with
//! peers connected inbound to its listener) or a remote room over a socket
//! (tier 2 as a LAN client, tier 3 the hosted coordinator). The client-side
//! logic is identical for both; only the delivery differs.
//!
//! When the remote goes away the engine falls back to its own room at once
//! (playback never stops because a socket dropped), remembers the last
//! remotely confirmed state ([`SyncBase`]) and every op confirmed only
//! locally since. On rejoin it replays those ops as a fast-forward when
//! nobody else moved the session, adopts the room's document when it was
//! merely behind, and files its own state as a saved queue when both sides
//! moved (a diverged device never merges and never overwrites).
//!
//! Ops are optimistic: applied locally at submission, confirmed by `OpAck`,
//! rolled back on `OpReject` or when someone else's op commits first.

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::api::{
    ConnectionState, ConnectionTier, DeviceId, DeviceInfo, EpochMs, Ms, PositionStamp, QueueKey, SavedQueue,
    SessionDocument, Setting, TrackId, TransportLease, TransportState, UndoEntry,
};
use crate::connect::clock::{extrapolate, resume_position, ClockSample, OffsetEstimator};
use crate::connect::discovery::{scope_hash, PeerAdvert};
use crate::connect::election::{elect_id, Candidate};
use crate::connect::lease::HeldLease;
use crate::connect::replica::ReplicaExt;
use crate::connect::room::{Room, RoomConfig, RoomInput, RoomOutput};
use crate::connect::transport::Backoff;
use crate::connect::wire::{
    merge_saved_queues, merge_settings, Credential, Msg, RefuseReason, RejectReason, ReplicaState, TransportCommand,
    WireMessage, PROTOCOL, PROTOCOL_MIN,
};
use crate::connect::{
    apply_op, doc_is_trivial, op_context, same_session_state, PeerId, ReducerHandle, SessionOp, SyncPoint,
};
use crate::util::Clock;

/// The peer id of the in-process loopback to the embedded room.
pub const LOOPBACK: &str = "loopback";
/// Default pre-buffer fan-out cap.
pub const DEFAULT_PREBUFFER_FANOUT: usize = 4;
/// Targets discard a pre-buffer nobody picked after this long.
pub const DEFAULT_PREBUFFER_TIMEOUT_MS: f64 = 60_000.0;
/// Scrobbles reached while cut off from the room wait this long for the
/// dedupe log before being submitted on local judgement.
pub const DEFAULT_SCROBBLE_GRACE_MS: f64 = 600_000.0;
/// An attached upstream that says nothing for this long is dead.
pub const DEFAULT_UPSTREAM_IDLE_MS: f64 = 30_000.0;
/// Clock ping cadence once synced.
const PING_INTERVAL_MS: f64 = 5_000.0;
/// Pings sent quickly after joining to converge the offset.
const PING_BURST: u32 = 3;
const PING_BURST_INTERVAL_MS: f64 = 300.0;
/// Connect / handshake give up after this long.
const CONNECT_TIMEOUT_MS: f64 = 15_000.0;
/// While on the LAN tier with a coordinator configured, retry it this often.
const COORDINATOR_RETRY_MS: f64 = 60_000.0;
/// Cap on locally confirmed ops kept for a fast-forward replay.
const UNSYNCED_CAP: usize = 500;
/// Advertise the session revision at most this often.
const ADVERT_MIN_INTERVAL_MS: f64 = 10_000.0;

/// Persisted next to the document: the last state confirmed with a remote room.
pub type SyncBase = SyncPoint;

#[derive(Debug, Clone)]
pub struct EngineConfig {
    pub device: DeviceInfo,
    pub scope: String,
    pub credential: Option<Credential>,
    pub coordinator_url: Option<String>,
    /// `ConnectCoordinator` / `DisconnectCoordinator`.
    pub coordinator_enabled: bool,
    pub lan_enabled: bool,
    pub prebuffer_fanout: usize,
    pub prebuffer_timeout_ms: f64,
    pub scrobble_grace_ms: f64,
    pub upstream_idle_ms: f64,
    pub backoff: Backoff,
    /// Require credential verification of inbound LAN peers (the host must
    /// answer `Output::VerifyCredential`).
    pub verify_lan_peers: bool,
}

impl EngineConfig {
    pub fn new(device: DeviceInfo, scope: impl Into<String>) -> Self {
        EngineConfig {
            device,
            scope: scope.into(),
            credential: None,
            coordinator_url: None,
            coordinator_enabled: true,
            lan_enabled: true,
            prebuffer_fanout: DEFAULT_PREBUFFER_FANOUT,
            prebuffer_timeout_ms: DEFAULT_PREBUFFER_TIMEOUT_MS,
            scrobble_grace_ms: DEFAULT_SCROBBLE_GRACE_MS,
            upstream_idle_ms: DEFAULT_UPSTREAM_IDLE_MS,
            backoff: Backoff::default(),
            verify_lan_peers: false,
        }
    }
}

/// What the actor feeds the engine.
#[derive(Debug, Clone, PartialEq)]
pub enum Input {
    // -- configuration ----------------------------------------------------
    SetCoordinatorUrl(Option<String>),
    ConnectCoordinator,
    DisconnectCoordinator,
    SetLanDiscovery(bool),
    SetCredential(Option<Credential>),

    // -- I/O reports -------------------------------------------------------
    /// The upstream socket requested by `Output::Connect` is open.
    Connected { peer: PeerId, url: String },
    ConnectFailed { error: String },
    /// Any socket closed (upstream or inbound).
    Disconnected { peer: PeerId },
    ListenerStarted { port: u16 },
    ListenerStopped,
    /// An inbound socket to our listener.
    PeerConnected { peer: PeerId },
    WireIn { peer: PeerId, msg: WireMessage },
    /// Answer to `Output::VerifyCredential`.
    CredentialVerified { peer: PeerId, ok: bool },
    PeerDiscovered(PeerAdvert),
    PeerLost { device_id: DeviceId },

    // -- session -----------------------------------------------------------
    LocalOp { op: SessionOp },
    /// Transport changed on this device (play/pause/seek/track). Never on a timer.
    /// `taken_at` is local time; `started_at` is a session-clock identity token
    /// (from [`Engine::now_session_ms`] or a `TakeTransport`).
    LocalStamp {
        key: Option<QueueKey>,
        track_id: Option<TrackId>,
        position: PositionStamp,
        played_ms: Ms,
        started_at: EpochMs,
        scrobbled: bool,
    },
    /// The user wants to play here (nobody owns, or a deliberate takeover).
    ClaimTransport { takeover: bool },
    /// This device stopped on purpose.
    ReleaseTransport,
    /// Play/pause/seek from this device's UI when it may not own transport.
    TransportRequest(TransportCommand),

    // -- handoff -----------------------------------------------------------
    OpenPicker,
    ClosePicker,
    HandoffTo { device_id: DeviceId },
    PreBufferReady { key: QueueKey },
    PreBufferFailed { key: QueueKey },
    ResumeHere,
    DismissResume,

    // -- scrobbling, LWW sets, undo ---------------------------------------
    ScrobbleReached { track_id: TrackId, started_at: EpochMs },
    SavedQueuesChanged(Vec<SavedQueue>),
    SettingChanged(Setting),
    UndoEntryCreated { entry: UndoEntry, before_revision: u32, before: Option<SessionDocument> },

    Tick,
}

/// Why the document changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocChange {
    /// A local op applied optimistically.
    Local,
    /// A remote op committed.
    Remote,
    /// Local ops were rolled back.
    Rollback,
    /// Adopted the room's document (join, resync).
    Sync,
}

/// A dormant resume offer, before the actor resolves the track summary.
#[derive(Debug, Clone, PartialEq)]
pub struct ResumeOfferDraft {
    pub device_id: DeviceId,
    pub device_name: String,
    pub key: QueueKey,
    pub track_id: TrackId,
    pub position_ms: Ms,
    pub played_ms: Ms,
    pub started_at: EpochMs,
    pub scrobbled: bool,
    pub last_seen: EpochMs,
}

/// What the actor must do.
#[derive(Debug, Clone, PartialEq)]
pub enum Output {
    WireOut { peer: PeerId, msg: WireMessage },
    /// Open one upstream socket, trying candidates in order; report with
    /// `Connected` or `ConnectFailed`.
    Connect { candidates: Vec<String> },
    Disconnect { peer: PeerId },
    StartListener,
    StopListener,
    /// Publish (or withdraw) our mDNS record.
    Advertise(Option<PeerAdvert>),
    /// Proxy a Subsonic ping for an inbound LAN peer; answer `CredentialVerified`.
    VerifyCredential { peer: PeerId, credential: Credential },

    DocumentChanged { document: SessionDocument, cause: DocChange },
    /// A remote stamp: extrapolate from it.
    TransportChanged { transport: TransportState },
    LeaseChanged { lease: TransportLease, owns: bool, detached: bool },
    ConnectionChanged(ConnectionState),
    DevicesChanged(Vec<DeviceInfo>),
    PickerChanged { open: bool, targets: Vec<DeviceInfo> },
    ResumeOffer(Option<ResumeOfferDraft>),
    /// Snapshot this document as a saved queue; never merge it.
    FilePreviousStateAsSavedQueue { document: SessionDocument, reason: String },

    PreBuffer { key: QueueKey, track_id: TrackId, position_ms: Ms },
    DiscardPreBuffer,
    /// This device now owns transport and should play from here.
    TakeTransport {
        key: QueueKey,
        track_id: TrackId,
        position_ms: Ms,
        played_ms: Ms,
        started_at: EpochMs,
        scrobbled: bool,
        play: bool,
    },
    /// This device lost or gave up transport: pause, keep the document.
    ReleaseTransport,
    /// Remote control for the transport owner (this device).
    TransportCommand(TransportCommand),
    /// Verdict on a `ScrobbleReached`.
    Scrobble { track_id: TrackId, started_at: EpochMs, allowed: bool },
    SavedQueuesMerged(Vec<SavedQueue>),
    SettingsMerged(Vec<Setting>),
    UndoEntryReceived { entry: UndoEntry, before_revision: u32, before: Option<SessionDocument> },
    /// Our embedded room's replica changed (persist if you like).
    ReplicaChanged(ReplicaState),
    Log { level: &'static str, message: String },
}

#[derive(Debug, Clone)]
struct PendingOp {
    op_id: String,
    op: SessionOp,
    base_revision: u32,
}

#[derive(Debug, Clone, PartialEq)]
enum RemoteState {
    Connecting { since: EpochMs, attempt: u32 },
    Handshaking { peer: PeerId, since: EpochMs, attempt: u32 },
    Attached { peer: PeerId },
    Backoff { until: EpochMs, attempt: u32 },
}

#[derive(Debug, Clone)]
struct Remote {
    tier: ConnectionTier,
    candidates: Vec<String>,
    leader: Option<DeviceId>,
    state: RemoteState,
}

#[derive(Debug, Clone)]
struct CurrentStamp {
    key: Option<QueueKey>,
    track_id: Option<TrackId>,
    /// Session clock.
    position: PositionStamp,
    played_ms: Ms,
    started_at: EpochMs,
    scrobbled: bool,
}

#[derive(Debug, Clone)]
struct Picker {
    key: QueueKey,
    track_id: TrackId,
    targets: Vec<(DeviceId, bool)>,
}

#[derive(Debug, Clone)]
struct PreBuffer {
    from: DeviceId,
    key: QueueKey,
    since: EpochMs,
}

#[derive(Debug, Clone)]
struct TakeInfo {
    key: QueueKey,
    track_id: TrackId,
    position_ms: Ms,
    played_ms: Ms,
    started_at: EpochMs,
    scrobbled: bool,
}

#[derive(Debug, Clone)]
struct DeferredScrobble {
    track_id: TrackId,
    started_at: EpochMs,
    since: EpochMs,
}

/// The Connect engine. One per core.
pub struct Engine {
    cfg: EngineConfig,
    clock: Arc<dyn Clock>,
    reducer: ReducerHandle,
    room: Room,
    listener_port: Option<u16>,
    listener_wanted: bool,
    inbound: Vec<PeerId>,

    doc: SessionDocument,
    confirmed: SessionDocument,
    pending: VecDeque<PendingOp>,
    unsynced: Vec<(String, SessionOp)>,
    unsynced_overflow: bool,
    sync_base: Option<SyncBase>,
    op_counter: u32,

    remote: Option<Remote>,
    upstream_ready: bool,
    last_upstream_msg_at: EpochMs,
    offset: OffsetEstimator,
    last_ping_at: EpochMs,
    pings_in_burst: u32,
    last_reported_offset: Option<(f64, Option<f64>)>,
    last_error: Option<String>,
    coordinator_failures: u32,
    coordinator_retry_at: EpochMs,

    lease: TransportLease,
    held: Option<HeldLease>,
    detached: bool,
    held_remote_epoch: Option<u32>,
    stamp: Option<CurrentStamp>,
    remote_transport: TransportState,
    pending_take: Option<TakeInfo>,

    devices: Vec<DeviceInfo>,
    picker: Option<Picker>,
    prebuffer: Option<PreBuffer>,
    resume: Option<ResumeOfferDraft>,

    deferred_scrobbles: Vec<DeferredScrobble>,
    scrobble_queries: HashMap<String, (TrackId, EpochMs)>,
    /// Scrobbles decided locally while cut off; told to the room on rejoin.
    unreported_scrobbles: Vec<(TrackId, EpochMs)>,
    saved_queues: Vec<SavedQueue>,
    settings: Vec<Setting>,

    lan_peers: Vec<PeerAdvert>,
    last_known_lan: HashMap<DeviceId, String>,
    last_advert: Option<PeerAdvert>,
    last_advert_at: EpochMs,

    to_room: VecDeque<WireMessage>,
    from_room: VecDeque<WireMessage>,
    out: Vec<Output>,
}

impl std::fmt::Debug for Engine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Engine")
            .field("device", &self.cfg.device.id)
            .field("revision", &self.doc.revision)
            .field("remote", &self.remote)
            .field("owns", &self.held.is_some())
            .finish()
    }
}

impl Engine {
    /// `document` is the device's persisted session document, `sync_base`
    /// what was persisted next to it (or `None` on first run).
    pub fn new(
        cfg: EngineConfig,
        clock: Arc<dyn Clock>,
        reducer: ReducerHandle,
        document: SessionDocument,
        sync_base: Option<SyncBase>,
    ) -> Self {
        let now = clock.now_ms();
        let mut room_cfg = RoomConfig::new(cfg.scope.clone());
        room_cfg.verify = cfg.verify_lan_peers;
        room_cfg.session_id = Some(document.session_id.clone());
        let room = Room::new(room_cfg, clock.clone(), reducer.clone(), None);
        let mut e = Engine {
            cfg,
            clock,
            reducer,
            room,
            listener_port: None,
            listener_wanted: false,
            inbound: vec![],
            confirmed: document.clone(),
            doc: document,
            pending: VecDeque::new(),
            unsynced: vec![],
            unsynced_overflow: false,
            sync_base,
            op_counter: 0,
            remote: None,
            upstream_ready: false,
            last_upstream_msg_at: now,
            offset: OffsetEstimator::new(),
            last_ping_at: 0.0,
            pings_in_burst: 0,
            last_reported_offset: None,
            last_error: None,
            coordinator_failures: 0,
            coordinator_retry_at: 0.0,
            lease: TransportLease::default(),
            held: None,
            detached: false,
            held_remote_epoch: None,
            stamp: None,
            remote_transport: TransportState::default(),
            pending_take: None,
            devices: vec![],
            picker: None,
            prebuffer: None,
            resume: None,
            deferred_scrobbles: vec![],
            scrobble_queries: HashMap::new(),
            unreported_scrobbles: vec![],
            saved_queues: vec![],
            settings: vec![],
            lan_peers: vec![],
            last_known_lan: HashMap::new(),
            last_advert: None,
            last_advert_at: 0.0,
            to_room: VecDeque::new(),
            from_room: VecDeque::new(),
            out: vec![],
        };
        e.attach_loopback();
        e.drain_loops();
        e.out.clear();
        e
    }

    // -- accessors ----------------------------------------------------------

    pub fn config(&self) -> &EngineConfig {
        &self.cfg
    }

    pub fn device_id(&self) -> &str {
        &self.cfg.device.id
    }

    /// The live session document (with optimistic ops applied).
    pub fn document(&self) -> &SessionDocument {
        &self.doc
    }

    /// Persist this next to the document.
    pub fn sync_base(&self) -> Option<&SyncBase> {
        self.sync_base.as_ref()
    }

    pub fn owns_transport(&self) -> bool {
        self.held.is_some()
    }

    /// The epoch this device believes it holds, if any.
    pub fn held_epoch(&self) -> Option<u32> {
        self.held.as_ref().map(|h| h.epoch)
    }

    /// Owning transport but unable to reach the room it took it from.
    pub fn is_detached(&self) -> bool {
        self.detached
    }

    pub fn lease(&self) -> &TransportLease {
        &self.lease
    }

    /// Attached to a remote room (or serving one) and past the handshake.
    pub fn is_connected(&self) -> bool {
        matches!(self.remote, Some(Remote { state: RemoteState::Attached { .. }, .. })) && self.upstream_ready
    }

    /// Serving our own room to inbound LAN peers.
    pub fn is_serving(&self) -> bool {
        self.remote.is_none() && !self.inbound.is_empty()
    }

    pub fn room(&self) -> &Room {
        &self.room
    }

    pub fn saved_queues(&self) -> &[SavedQueue] {
        &self.saved_queues
    }

    pub fn settings(&self) -> &[Setting] {
        &self.settings
    }

    pub fn now_local_ms(&self) -> EpochMs {
        self.clock.now_ms()
    }

    /// Session time: local plus the estimated offset (zero on our own room).
    pub fn now_session_ms(&self) -> EpochMs {
        self.clock.now_ms() + self.offset_ms()
    }

    pub fn offset_ms(&self) -> f64 {
        if self.is_connected() {
            self.offset.offset_ms()
        } else {
            0.0
        }
    }

    /// The transport as this device knows it: its own stamp when it owns
    /// transport, the last remote stamp otherwise. Positions are on the
    /// session clock.
    pub fn transport(&self) -> TransportState {
        if let (Some(_), Some(s)) = (&self.held, &self.stamp) {
            TransportState {
                lease: self.lease.clone(),
                position: s.position.clone(),
                buffering: false,
                played_ms: s.played_ms,
                volume: self.remote_transport.volume,
            }
        } else {
            let mut t = self.remote_transport.clone();
            t.lease = self.lease.clone();
            t
        }
    }

    /// Current position of whatever is playing in the session.
    pub fn position_ms(&self) -> Ms {
        extrapolate(&self.transport().position, self.now_session_ms())
    }

    pub fn connection_state(&self) -> ConnectionState {
        let tier = match &self.remote {
            Some(r) => r.tier,
            None if !self.inbound.is_empty() => ConnectionTier::Lan,
            None => ConnectionTier::Local,
        };
        ConnectionState {
            tier,
            connected: self.is_connected() || self.is_serving(),
            coordinator_url: self.cfg.coordinator_url.clone(),
            clock_offset_ms: self.offset_ms(),
            round_trip_ms: if self.is_connected() { self.offset.round_trip_ms() } else { None },
            peer_count: self.devices.iter().filter(|d| !d.is_self).count() as u32,
            error: self.last_error.clone(),
        }
    }

    /// Everyone present, self included.
    pub fn devices(&self) -> Vec<DeviceInfo> {
        let mut me = self.cfg.device.clone();
        me.is_self = true;
        me.playing = self.held.is_some();
        me.ready = false;
        me.last_seen = self.now_session_ms();
        let mut out = vec![me];
        out.extend(self.devices.iter().filter(|d| d.id != self.cfg.device.id).cloned());
        out
    }

    pub fn resume_offer(&self) -> Option<&ResumeOfferDraft> {
        self.resume.as_ref()
    }

    pub fn picker_open(&self) -> bool {
        self.picker.is_some()
    }

    /// Our mDNS record, once the listener has a port.
    pub fn advert(&self) -> Option<PeerAdvert> {
        let port = self.listener_port?;
        Some(PeerAdvert {
            device_id: self.cfg.device.id.clone(),
            device_name: self.cfg.device.name.clone(),
            platform: self.cfg.device.platform,
            scope_hash: scope_hash(&self.cfg.scope),
            port,
            protocol: PROTOCOL,
            session_revision: self.doc.revision,
            serving: self.is_serving(),
            addresses: vec![],
        })
    }

    // -- driving ------------------------------------------------------------

    /// Drive the engine. Act on every output in order.
    pub fn handle(&mut self, input: Input) -> Vec<Output> {
        match input {
            Input::SetCoordinatorUrl(url) => {
                self.cfg.coordinator_url = url;
                self.coordinator_failures = 0;
                self.reevaluate();
            }
            Input::ConnectCoordinator => {
                self.cfg.coordinator_enabled = true;
                self.coordinator_failures = 0;
                self.reevaluate();
            }
            Input::DisconnectCoordinator => {
                self.cfg.coordinator_enabled = false;
                self.reevaluate();
            }
            Input::SetLanDiscovery(enabled) => {
                self.cfg.lan_enabled = enabled;
                if !enabled {
                    self.lan_peers.clear();
                }
                self.reevaluate();
            }
            Input::SetCredential(c) => self.cfg.credential = c,
            Input::Connected { peer, url } => self.on_connected(peer, url),
            Input::ConnectFailed { error } => self.on_upstream_lost(Some(error)),
            Input::Disconnected { peer } => self.on_disconnected(peer),
            Input::ListenerStarted { port } => {
                self.listener_port = Some(port);
                self.maybe_advertise(true);
            }
            Input::ListenerStopped => {
                self.listener_port = None;
                self.maybe_advertise(true);
            }
            Input::PeerConnected { peer } => self.on_peer_connected(peer),
            Input::WireIn { peer, msg } => self.on_wire_in(peer, msg),
            Input::CredentialVerified { peer, ok } => {
                let outs = self.room.handle(RoomInput::Verified { peer, ok });
                self.process_room_outputs(outs);
            }
            Input::PeerDiscovered(advert) => self.on_peer_discovered(advert),
            Input::PeerLost { device_id } => {
                self.lan_peers.retain(|p| p.device_id != device_id);
                self.reevaluate();
            }
            Input::LocalOp { op } => self.on_local_op(op),
            Input::LocalStamp { key, track_id, position, played_ms, started_at, scrobbled } => {
                self.on_local_stamp(key, track_id, position, played_ms, started_at, scrobbled)
            }
            Input::ClaimTransport { takeover } => {
                if self.held.is_none() || takeover {
                    self.upstream_send(Msg::LeaseClaim { epoch_expected: None, takeover });
                }
            }
            Input::ReleaseTransport => {
                if let Some(h) = self.held.take() {
                    self.upstream_send(Msg::LeaseRelease { epoch: h.epoch });
                    self.detached = false;
                    self.emit_lease();
                }
            }
            Input::TransportRequest(cmd) => {
                if self.held.is_some() {
                    self.out.push(Output::TransportCommand(cmd));
                } else {
                    let from = self.cfg.device.id.clone();
                    self.upstream_send(Msg::TransportRequest { command: cmd, from });
                }
            }
            Input::OpenPicker => self.open_picker(),
            Input::ClosePicker => self.close_picker(true),
            Input::HandoffTo { device_id } => self.handoff_to(device_id),
            Input::PreBufferReady { key } => self.on_prebuffer_result(key, true),
            Input::PreBufferFailed { key } => self.on_prebuffer_result(key, false),
            Input::ResumeHere => {
                if let Some(r) = self.resume.clone() {
                    self.pending_take = Some(TakeInfo {
                        key: r.key,
                        track_id: r.track_id,
                        position_ms: r.position_ms,
                        played_ms: r.played_ms,
                        started_at: r.started_at,
                        scrobbled: r.scrobbled,
                    });
                    self.upstream_send(Msg::LeaseClaim { epoch_expected: None, takeover: true });
                }
            }
            Input::DismissResume => {
                if self.resume.take().is_some() {
                    self.out.push(Output::ResumeOffer(None));
                }
            }
            Input::ScrobbleReached { track_id, started_at } => self.on_scrobble_reached(track_id, started_at),
            Input::SavedQueuesChanged(list) => {
                let (merged, _) = merge_saved_queues(&list, &self.saved_queues);
                self.saved_queues = merged;
                let queues = self.saved_queues.clone();
                self.upstream_send(Msg::SavedQueuesSync { queues });
            }
            Input::SettingChanged(setting) => {
                let (merged, _) = merge_settings(&[setting], &self.settings);
                self.settings = merged;
                let settings = self.settings.clone();
                self.upstream_send(Msg::SettingsSync { settings });
            }
            Input::UndoEntryCreated { entry, before_revision, before } => {
                self.upstream_send(Msg::UndoEntryShared { entry, before_revision, before });
            }
            Input::Tick => self.on_tick(),
        }
        self.drain_loops();
        std::mem::take(&mut self.out)
    }

    // -- upstream plumbing ---------------------------------------------------

    fn upstream_peer(&self) -> Option<PeerId> {
        match &self.remote {
            Some(Remote { state: RemoteState::Attached { peer }, .. }) => Some(peer.clone()),
            _ => None,
        }
    }

    /// Whether the upstream we submit to is the one whose verdicts count for
    /// scrobble dedupe: a remote room, or our own when no remote is wanted.
    fn upstream_authoritative(&self) -> bool {
        self.remote.is_none() || self.is_connected()
    }

    fn upstream_send(&mut self, msg: Msg) {
        let m = WireMessage::new(msg);
        match self.upstream_peer() {
            Some(peer) => self.out.push(Output::WireOut { peer, msg: m }),
            None => self.to_room.push_back(m),
        }
    }

    fn drain_loops(&mut self) {
        let mut guard = 0;
        loop {
            let mut progressed = false;
            while let Some(m) = self.to_room.pop_front() {
                progressed = true;
                let outs = self.room.handle(RoomInput::Message(LOOPBACK.into(), m));
                self.process_room_outputs(outs);
            }
            while let Some(m) = self.from_room.pop_front() {
                progressed = true;
                self.on_upstream_message(m);
            }
            guard += 1;
            if !progressed || guard > 10_000 {
                break;
            }
        }
    }

    fn process_room_outputs(&mut self, outs: Vec<RoomOutput>) {
        for o in outs {
            match o {
                RoomOutput::Send(peer, msg) => {
                    if peer == LOOPBACK {
                        self.from_room.push_back(msg);
                    } else {
                        self.out.push(Output::WireOut { peer, msg });
                    }
                }
                RoomOutput::Close(peer) => {
                    if peer != LOOPBACK {
                        self.inbound.retain(|p| p != &peer);
                        self.out.push(Output::Disconnect { peer });
                    }
                }
                RoomOutput::Verify { peer, credential } => self.out.push(Output::VerifyCredential { peer, credential }),
                RoomOutput::ReplicaChanged => self.out.push(Output::ReplicaChanged(self.room.replica().clone())),
            }
        }
    }

    /// Point the upstream at our own room and run the handshake over the loopback.
    fn attach_loopback(&mut self) {
        self.upstream_ready = false;
        self.room.adopt_document(self.doc.clone());
        let outs = self.room.handle(RoomInput::Connected(LOOPBACK.into()));
        self.process_room_outputs(outs);
        let hello = self.hello_msg(None);
        self.to_room.push_back(WireMessage::new(hello));
    }

    fn hello_msg(&self, credential: Option<Credential>) -> Msg {
        Msg::Hello {
            device: self.cfg.device.clone(),
            protocol_min: PROTOCOL_MIN,
            protocol_max: PROTOCOL,
            scope: self.cfg.scope.clone(),
            credential,
            session_id: Some(self.doc.session_id.clone()),
            session_revision: self.doc.revision,
            held_epoch: self.held_remote_epoch,
            extra: Default::default(),
        }
    }

    fn emit_connection(&mut self) {
        let s = self.connection_state();
        self.out.push(Output::ConnectionChanged(s));
    }

    fn emit_lease(&mut self) {
        self.out.push(Output::LeaseChanged { lease: self.lease.clone(), owns: self.held.is_some(), detached: self.detached });
    }

    fn emit_devices(&mut self) {
        let d = self.devices();
        self.out.push(Output::DevicesChanged(d));
    }

    fn log(&mut self, level: &'static str, message: impl Into<String>) {
        self.out.push(Output::Log { level, message: message.into() });
    }

    // -- tier selection ------------------------------------------------------

    fn coordinator_target(&self) -> Option<String> {
        if !self.cfg.coordinator_enabled {
            return None;
        }
        self.cfg.coordinator_url.clone()
    }

    fn lan_leader(&self) -> Option<DeviceId> {
        if !self.cfg.lan_enabled || self.lan_peers.is_empty() {
            return None;
        }
        let mut cands: Vec<Candidate> = self
            .lan_peers
            .iter()
            .map(|p| Candidate { device_id: p.device_id.clone(), session_revision: p.session_revision, serving: p.serving })
            .collect();
        cands.push(Candidate {
            device_id: self.cfg.device.id.clone(),
            session_revision: self.doc.revision,
            serving: self.is_serving(),
        });
        elect_id(&cands)
    }

    /// Decide the upstream and start moving towards it.
    fn reevaluate(&mut self) {
        let now = self.now_local_ms();
        let coordinator = self.coordinator_target();
        let coordinator_usable = coordinator.is_some() && (self.coordinator_failures < 2 || now >= self.coordinator_retry_at);
        let leader = self.lan_leader();

        // The listener runs whenever LAN is on; it costs nothing and lets
        // peers find us whichever way the election goes.
        let want_listener = self.cfg.lan_enabled;
        if want_listener != self.listener_wanted {
            self.listener_wanted = want_listener;
            self.out.push(if want_listener { Output::StartListener } else { Output::StopListener });
        }

        let desired: Option<(ConnectionTier, Vec<String>, Option<DeviceId>)> = if coordinator_usable {
            coordinator.map(|u| (ConnectionTier::Coordinator, vec![u], None))
        } else if let Some(l) = leader.filter(|l| l != &self.cfg.device.id) {
            let advert = self.lan_peers.iter().find(|p| p.device_id == l).cloned();
            advert.map(|a| {
                let urls = a.urls(self.last_known_lan.get(&l).map(|s| s.as_str()));
                (ConnectionTier::Lan, urls, Some(l))
            })
        } else {
            None
        };

        match (desired, &self.remote) {
            (None, None) => {}
            (None, Some(_)) => {
                self.drop_remote(None);
                self.emit_connection();
            }
            (Some((tier, candidates, leader)), Some(r)) if r.tier == tier && r.leader == leader => {
                // Same target; refresh candidates for the next attempt.
                if let Some(r) = &mut self.remote {
                    r.candidates = candidates;
                }
            }
            (Some((tier, candidates, leader)), _) => {
                if self.remote.is_some() {
                    self.drop_remote(None);
                }
                self.remote = Some(Remote {
                    tier,
                    candidates: candidates.clone(),
                    leader,
                    state: RemoteState::Connecting { since: now, attempt: 0 },
                });
                self.out.push(Output::Connect { candidates });
                self.emit_connection();
            }
        }
        self.maybe_advertise(false);
    }

    /// Leave the remote (if any) and return to our own room.
    fn drop_remote(&mut self, error: Option<String>) {
        let was_attached = self.is_connected();
        if let Some(r) = self.remote.take() {
            match r.state {
                RemoteState::Attached { peer } | RemoteState::Handshaking { peer, .. } => {
                    self.out.push(Output::WireOut { peer: peer.clone(), msg: WireMessage::new(Msg::Bye { reason: "leaving".into() }) });
                    self.out.push(Output::Disconnect { peer });
                }
                _ => {}
            }
        }
        self.last_error = error;
        self.upstream_ready = false;
        if was_attached {
            self.on_left_remote();
        } else if !self.upstream_ready {
            self.attach_loopback();
        }
    }

    /// We were attached remotely and now aren't: fall back to our own room.
    fn on_left_remote(&mut self) {
        if let Some(h) = &self.held {
            self.held_remote_epoch = Some(h.epoch);
            self.detached = true;
        }
        self.offset.reset();
        self.devices.clear();
        self.emit_devices();
        if self.picker.is_some() {
            self.close_picker(false);
        }
        if self.prebuffer.take().is_some() {
            self.out.push(Output::DiscardPreBuffer);
        }
        self.scrobble_queries.clear();
        // Pending ops will never be acked by the room that's gone: confirm them
        // through our own room instead (same ids, same keys).
        let pending: Vec<PendingOp> = self.pending.drain(..).collect();
        self.doc = self.confirmed.clone();
        self.attach_loopback();
        self.drain_loops();
        for p in pending {
            self.resubmit(p.op_id, p.op);
        }
        if self.held.is_some() {
            self.upstream_send(Msg::LeaseClaim { epoch_expected: None, takeover: true });
        }
        self.emit_lease();
        self.emit_connection();
    }

    fn on_upstream_lost(&mut self, error: Option<String>) {
        let now = self.now_local_ms();
        let Some(r) = &self.remote else { return };
        let attempt = match &r.state {
            RemoteState::Connecting { attempt, .. } | RemoteState::Handshaking { attempt, .. } => *attempt,
            RemoteState::Attached { .. } => 0,
            RemoteState::Backoff { attempt, .. } => *attempt,
        };
        let was_attached = matches!(r.state, RemoteState::Attached { .. });
        let tier = r.tier;
        let delay = self.cfg.backoff.delay_ms(attempt);
        if let Some(r) = &mut self.remote {
            r.state = RemoteState::Backoff { until: now + delay, attempt: attempt + 1 };
        }
        if tier == ConnectionTier::Coordinator && !was_attached {
            self.coordinator_failures += 1;
            self.coordinator_retry_at = now + COORDINATOR_RETRY_MS;
        }
        self.last_error = error;
        if was_attached {
            self.on_left_remote();
        } else {
            self.emit_connection();
        }
        // Losing the coordinator may hand the LAN its turn.
        if tier == ConnectionTier::Coordinator && self.coordinator_failures >= 2 {
            self.reevaluate();
        }
    }

    fn on_connected(&mut self, peer: PeerId, url: String) {
        let now = self.now_local_ms();
        let Some(r) = &mut self.remote else {
            self.out.push(Output::Disconnect { peer });
            return;
        };
        let attempt = match &r.state {
            RemoteState::Connecting { attempt, .. } => *attempt,
            _ => 0,
        };
        r.state = RemoteState::Handshaking { peer: peer.clone(), since: now, attempt };
        if let Some(l) = &r.leader {
            self.last_known_lan.insert(l.clone(), url);
        }
        let credential = self.cfg.credential.clone();
        let hello = self.hello_msg(credential);
        self.last_upstream_msg_at = now;
        self.out.push(Output::WireOut { peer, msg: WireMessage::new(hello) });
    }

    fn on_disconnected(&mut self, peer: PeerId) {
        if self.inbound.contains(&peer) {
            self.inbound.retain(|p| p != &peer);
            let outs = self.room.handle(RoomInput::Disconnected(peer));
            self.process_room_outputs(outs);
            self.maybe_advertise(false);
            self.emit_connection();
            return;
        }
        let is_upstream = matches!(&self.remote, Some(Remote { state: RemoteState::Attached { peer: p } | RemoteState::Handshaking { peer: p, .. }, .. }) if p == &peer);
        if is_upstream {
            self.on_upstream_lost(Some("connection closed".into()));
        }
    }

    fn on_peer_connected(&mut self, peer: PeerId) {
        if self.remote.is_some() {
            // We follow another room; don't fork the session.
            self.out.push(Output::WireOut { peer: peer.clone(), msg: WireMessage::new(Msg::Bye { reason: "not serving".into() }) });
            self.out.push(Output::Disconnect { peer });
            return;
        }
        self.inbound.push(peer.clone());
        let outs = self.room.handle(RoomInput::Connected(peer));
        self.process_room_outputs(outs);
    }

    fn on_wire_in(&mut self, peer: PeerId, msg: WireMessage) {
        if self.inbound.contains(&peer) {
            let outs = self.room.handle(RoomInput::Message(peer, msg));
            self.process_room_outputs(outs);
            let serving_now = self.is_serving();
            if self.last_advert.as_ref().map(|a| a.serving) != Some(serving_now) {
                self.maybe_advertise(true);
            }
            return;
        }
        let is_upstream = matches!(&self.remote, Some(Remote { state: RemoteState::Attached { peer: p } | RemoteState::Handshaking { peer: p, .. }, .. }) if p == &peer);
        if !is_upstream {
            return;
        }
        self.last_upstream_msg_at = self.now_local_ms();
        self.on_upstream_message(msg);
    }

    fn on_peer_discovered(&mut self, advert: PeerAdvert) {
        if advert.device_id == self.cfg.device.id || advert.scope_hash != scope_hash(&self.cfg.scope) {
            return;
        }
        if advert.protocol < PROTOCOL_MIN {
            return;
        }
        match self.lan_peers.iter_mut().find(|p| p.device_id == advert.device_id) {
            Some(p) => *p = advert,
            None => self.lan_peers.push(advert),
        }
        self.reevaluate();
    }

    fn maybe_advertise(&mut self, force: bool) {
        if !self.cfg.lan_enabled {
            if self.last_advert.take().is_some() {
                self.out.push(Output::Advertise(None));
            }
            return;
        }
        let now = self.now_local_ms();
        let advert = self.advert();
        if advert == self.last_advert {
            return;
        }
        let only_revision = match (&advert, &self.last_advert) {
            (Some(a), Some(b)) => {
                let mut b2 = b.clone();
                b2.session_revision = a.session_revision;
                &b2 == a
            }
            _ => false,
        };
        if only_revision && !force && now - self.last_advert_at < ADVERT_MIN_INTERVAL_MS {
            return;
        }
        self.last_advert = advert.clone();
        self.last_advert_at = now;
        self.out.push(Output::Advertise(advert));
    }

    // -- upstream messages ----------------------------------------------------

    fn on_upstream_message(&mut self, msg: WireMessage) {
        match msg.msg {
            Msg::Welcome { session_clock_ms, replica, members, .. } => self.on_welcome(session_clock_ms, replica, members),
            Msg::Refuse { reason, message } => {
                self.log("warn", format!("refused: {reason:?}: {message}"));
                let attempt_bump = if reason == RefuseReason::Unauthorised { 4 } else { 0 };
                if let Some(r) = &mut self.remote {
                    if let RemoteState::Handshaking { attempt, .. } = &mut r.state {
                        *attempt += attempt_bump;
                    }
                }
                if let Some(peer) = self.remote.as_ref().and_then(|r| match &r.state {
                    RemoteState::Handshaking { peer, .. } | RemoteState::Attached { peer } => Some(peer.clone()),
                    _ => None,
                }) {
                    self.out.push(Output::Disconnect { peer });
                }
                self.on_upstream_lost(Some(format!("{reason:?}: {message}")));
            }
            Msg::Bye { reason } => {
                if let Some(peer) = self.upstream_peer() {
                    self.out.push(Output::Disconnect { peer });
                }
                self.on_upstream_lost(Some(format!("bye: {reason}")));
            }
            Msg::OpAck { op_id, revision } => self.on_op_ack(op_id, revision),
            Msg::OpReject { op_id, current_revision, reason, document } => self.on_op_reject(op_id, current_revision, reason, document),
            Msg::OpCommitted { op, revision, device_id, op_id } => self.on_op_committed(op, revision, device_id, op_id),
            Msg::Document { document } => self.adopt(document, DocChange::Sync),
            Msg::TransportStamp { device_id, key, position, played_ms, started_at, scrobbled, .. } => {
                if device_id == self.cfg.device.id {
                    return;
                }
                self.remote_transport.position = position;
                self.remote_transport.played_ms = played_ms;
                if let Some(r) = &mut self.resume {
                    if r.device_id == device_id {
                        // The device that was playing is back: no longer dormant.
                        self.resume = None;
                        self.out.push(Output::ResumeOffer(None));
                    }
                }
                let _ = (key, started_at, scrobbled);
                let t = self.transport();
                self.out.push(Output::TransportChanged { transport: t });
            }
            Msg::TransportRequest { command, .. } => {
                if self.held.is_some() {
                    self.out.push(Output::TransportCommand(command));
                }
            }
            Msg::LeaseGranted { lease } => self.on_lease_granted(lease),
            Msg::LeaseFenced { lease, .. } => self.on_lease_fenced(lease),
            Msg::Presence { devices } => {
                self.devices = devices;
                if let Some(p) = &mut self.picker {
                    let present: Vec<DeviceId> = self.devices.iter().map(|d| d.id.clone()).collect();
                    p.targets.retain(|(id, _)| present.contains(id));
                }
                self.emit_devices();
                if self.picker.is_some() {
                    self.emit_picker();
                }
            }
            Msg::ClockPong { t0, t1, t2 } => {
                let t3 = self.now_local_ms();
                self.offset.record(ClockSample { t0, t1, t2, t3 });
                let now = (self.offset.offset_ms().round(), self.offset.round_trip_ms().map(|r| r.round()));
                if self.last_reported_offset != Some(now) {
                    self.last_reported_offset = Some(now);
                    self.emit_connection();
                }
            }
            Msg::HandoffPickerOpen { .. } => {}
            Msg::HandoffPickerClose { from } => {
                if self.prebuffer.as_ref().map(|p| &p.from) == Some(&from) {
                    self.prebuffer = None;
                    self.out.push(Output::DiscardPreBuffer);
                }
            }
            Msg::HandoffPrepare { from, key, track_id, position_ms, .. } => {
                if self.held.is_some() {
                    return; // we're the one playing; nothing to pre-buffer
                }
                let now = self.now_local_ms();
                self.prebuffer = Some(PreBuffer { from, key: key.clone(), since: now });
                self.out.push(Output::PreBuffer { key, track_id, position_ms });
            }
            Msg::HandoffReady { target, key, ready, .. } => {
                if let Some(p) = &mut self.picker {
                    if p.key == key {
                        if let Some(t) = p.targets.iter_mut().find(|(id, _)| id == &target) {
                            t.1 = ready;
                        }
                    }
                }
                if self.picker.is_some() {
                    self.emit_picker();
                }
            }
            Msg::HandoffTakeover { target, key, track_id, position_ms, played_ms, started_at, scrobbled, lease, .. } => {
                if target != self.cfg.device.id {
                    return;
                }
                let Some(lease) = lease else { return };
                self.prebuffer = None;
                self.lease = lease.clone();
                let now = self.now_local_ms();
                self.held = Some(HeldLease::new(lease.epoch, now));
                self.detached = false;
                self.stamp = Some(CurrentStamp {
                    key: Some(key.clone()),
                    track_id: Some(track_id.clone()),
                    position: PositionStamp { position_ms, taken_at: self.now_session_ms(), rate: 1.0, is_playing: true },
                    played_ms,
                    started_at,
                    scrobbled,
                });
                self.out.push(Output::TakeTransport { key, track_id, position_ms, played_ms, started_at, scrobbled, play: true });
                self.emit_lease();
                if self.resume.take().is_some() {
                    self.out.push(Output::ResumeOffer(None));
                }
            }
            Msg::ScrobbleDedupeAnswer { query_id, duplicate } => {
                if let Some((track_id, started_at)) = self.scrobble_queries.remove(&query_id) {
                    self.out.push(Output::Scrobble { track_id, started_at, allowed: !duplicate });
                }
            }
            Msg::SavedQueuesSync { queues } => {
                let (merged, changed) = merge_saved_queues(&self.saved_queues, &queues);
                if changed {
                    self.saved_queues = merged.clone();
                    self.out.push(Output::SavedQueuesMerged(merged));
                }
            }
            Msg::SettingsSync { settings } => {
                let (merged, changed) = merge_settings(&self.settings, &settings);
                if changed {
                    self.settings = merged.clone();
                    self.out.push(Output::SettingsMerged(merged));
                }
            }
            Msg::UndoEntryShared { entry, before_revision, before } => {
                if entry.device_id != self.cfg.device.id {
                    self.out.push(Output::UndoEntryReceived { entry, before_revision, before });
                }
            }
            Msg::Hello { .. }
            | Msg::Op { .. }
            | Msg::SyncRequest
            | Msg::LeaseHeartbeat { .. }
            | Msg::LeaseClaim { .. }
            | Msg::LeaseRelease { .. }
            | Msg::ClockPing { .. }
            | Msg::HandoffRelease { .. }
            | Msg::ScrobbleSubmitted { .. }
            | Msg::ScrobbleDedupeQuery { .. }
            | Msg::Unknown => {}
        }
    }

    fn on_welcome(&mut self, session_clock_ms: EpochMs, replica: Option<ReplicaState>, members: Vec<DeviceInfo>) {
        let now = self.now_local_ms();
        let remote = match &mut self.remote {
            Some(r) => {
                if let RemoteState::Handshaking { peer, .. } = &r.state {
                    let peer = peer.clone();
                    r.state = RemoteState::Attached { peer };
                }
                true
            }
            None => false,
        };
        self.upstream_ready = true;
        self.last_error = None;
        if remote {
            self.coordinator_failures = 0;
            // Provisional offset from the welcome; real pings refine it.
            self.offset.reset();
            self.offset.record(ClockSample { t0: now - 1000.0, t1: session_clock_ms, t2: session_clock_ms, t3: now });
            self.pings_in_burst = 0;
            self.last_ping_at = 0.0;
            self.send_ping();
            // Say goodbye to anyone we were serving: they'll re-elect.
            let inbound: Vec<PeerId> = self.inbound.drain(..).collect();
            for p in inbound {
                self.out.push(Output::WireOut { peer: p.clone(), msg: WireMessage::new(Msg::Bye { reason: "joined a coordinator".into() }) });
                self.out.push(Output::Disconnect { peer: p.clone() });
                let outs = self.room.handle(RoomInput::Disconnected(p));
                self.process_room_outputs(outs);
            }
        }
        self.devices = members;

        // -- reconcile --------------------------------------------------------
        let previous = self.doc.clone();
        let room_doc = replica.as_ref().map(|r| r.document.clone());
        let room_trivial = room_doc.as_ref().map(|d| doc_is_trivial(d) && d.revision == 0).unwrap_or(true);
        let room_rev = room_doc.as_ref().map(|d| d.revision).unwrap_or(0);
        self.pending.clear();

        if room_trivial {
            if !doc_is_trivial(&self.doc) {
                let doc = self.doc.clone();
                self.confirmed = self.doc.clone();
                let base = room_rev;
                self.submit_at(SessionOp::Replace { document: doc }, base, false);
            } else if let Some(rd) = &room_doc {
                self.adopt(rd.clone(), DocChange::Sync);
            }
            self.unsynced.clear();
            self.unsynced_overflow = false;
        } else {
            let rd = room_doc.clone().expect("non-trivial room has a document");
            let fast_forward = remote
                && self.sync_base.as_ref().map(|b| b.session_id == rd.session_id && b.revision == rd.revision).unwrap_or(false);
            let behind = remote
                && self.unsynced.is_empty()
                && !self.unsynced_overflow
                && rd.session_id == self.doc.session_id
                && self.doc.revision <= rd.revision;
            if !remote {
                // Our own room mirrors us; nothing to reconcile.
                self.confirmed = self.doc.clone();
            } else if fast_forward {
                if self.unsynced_overflow {
                    let doc = self.doc.clone();
                    self.confirmed = rd.clone();
                    self.doc = rd.clone();
                    self.submit_at(SessionOp::Replace { document: doc }, rd.revision, false);
                } else {
                    let ops: Vec<(String, SessionOp)> = std::mem::take(&mut self.unsynced);
                    self.confirmed = rd.clone();
                    self.doc = rd.clone();
                    for (id, op) in ops {
                        self.resubmit(id, op);
                    }
                }
                self.unsynced.clear();
                self.unsynced_overflow = false;
                if !same_session_state(&previous, &self.doc) {
                    let d = self.doc.clone();
                    self.out.push(Output::DocumentChanged { document: d, cause: DocChange::Sync });
                }
            } else {
                let file = !behind && !doc_is_trivial(&previous) && !same_session_state(&previous, &rd);
                self.adopt(rd.clone(), DocChange::Sync);
                if file {
                    self.out.push(Output::FilePreviousStateAsSavedQueue {
                        document: previous.clone(),
                        reason: "session moved on while this device was away".into(),
                    });
                }
                self.unsynced.clear();
                self.unsynced_overflow = false;
            }
        }
        if remote {
            self.sync_base = Some(SyncPoint { session_id: self.doc.session_id.clone(), revision: self.confirmed.revision });
        }

        // -- lease and resume -------------------------------------------------
        if let Some(rep) = &replica {
            self.lease = rep.transport_lease.clone();
            self.remote_transport = rep.document.transport.clone();
            self.remote_transport.lease = self.lease.clone();
            let live_owner = if self.lease.expires_at > self.now_session_ms() { self.lease.owner.clone() } else { None };
            if let (Some(stamp), Some(current)) = (&rep.last_stamp, &self.doc.current) {
                if live_owner.is_none()
                    && stamp.device_id != self.cfg.device.id
                    && stamp.key.as_deref() == Some(current.key.as_str())
                    && self.held.is_none()
                {
                    let last_seen = rep.device_last_seen(&stamp.device_id).unwrap_or(stamp.position.taken_at);
                    let draft = ResumeOfferDraft {
                        device_id: stamp.device_id.clone(),
                        device_name: stamp.device_name.clone(),
                        key: current.key.clone(),
                        track_id: current.track_id.clone(),
                        position_ms: resume_position(&stamp.position, self.now_session_ms()),
                        played_ms: stamp.played_ms,
                        started_at: stamp.started_at,
                        scrobbled: stamp.scrobbled,
                        last_seen,
                    };
                    self.resume = Some(draft.clone());
                    self.out.push(Output::ResumeOffer(Some(draft)));
                }
            }
            let (merged, changed) = merge_saved_queues(&self.saved_queues, &rep.saved_queues);
            if changed {
                self.saved_queues = merged.clone();
                self.out.push(Output::SavedQueuesMerged(merged));
            }
            let (merged, changed) = merge_settings(&self.settings, &rep.settings);
            if changed {
                self.settings = merged.clone();
                self.out.push(Output::SettingsMerged(merged));
            }
            if remote {
                let (theirs_plus_ours, ours_has_more) = merge_saved_queues(&rep.saved_queues, &self.saved_queues);
                if ours_has_more {
                    self.upstream_send(Msg::SavedQueuesSync { queues: theirs_plus_ours });
                }
                let (_, ours_has_more) = merge_settings(&rep.settings, &self.settings);
                if ours_has_more {
                    let s = self.settings.clone();
                    self.upstream_send(Msg::SettingsSync { settings: s });
                }
            }
        } else if remote && (!self.saved_queues.is_empty() || !self.settings.is_empty()) {
            let q = self.saved_queues.clone();
            self.upstream_send(Msg::SavedQueuesSync { queues: q });
            let s = self.settings.clone();
            self.upstream_send(Msg::SettingsSync { settings: s });
        }
        if self.held.is_some() {
            let expected = if remote { self.held_remote_epoch } else { None };
            self.upstream_send(Msg::LeaseClaim { epoch_expected: expected, takeover: !remote });
        }
        if remote {
            let unreported = std::mem::take(&mut self.unreported_scrobbles);
            for (track_id, started_at) in unreported {
                let device_id = self.cfg.device.id.clone();
                self.upstream_send(Msg::ScrobbleSubmitted { track_id, started_at, device_id });
            }
            let deferred = std::mem::take(&mut self.deferred_scrobbles);
            for d in deferred {
                self.query_scrobble(d.track_id, d.started_at);
            }
        }
        self.emit_devices();
        self.emit_connection();
        self.maybe_advertise(true);
    }

    // -- ops -----------------------------------------------------------------

    fn next_op_id(&mut self) -> String {
        self.op_counter += 1;
        format!("{}-{}", self.cfg.device.id, self.op_counter)
    }

    fn on_local_op(&mut self, op: SessionOp) {
        let base = self.doc.revision;
        let op_id = self.next_op_id();
        let ctx = op_context(&op_id, self.now_session_ms(), self.local_position());
        match apply_op(self.reducer.as_ref(), &self.doc, &op, &ctx, base.saturating_add(1)) {
            Ok(next) => {
                self.doc = next;
                let d = self.doc.clone();
                self.out.push(Output::DocumentChanged { document: d, cause: DocChange::Local });
                self.submit_at(op, base, true);
            }
            Err(e) => self.log("debug", format!("local op not applicable: {e}")),
        }
    }

    fn local_position(&self) -> Ms {
        match &self.stamp {
            Some(s) if self.held.is_some() => extrapolate(&s.position, self.now_session_ms()),
            _ => 0,
        }
    }

    /// Queue `op` against `base` and send it upstream. `applied` says the
    /// caller already applied it to `doc`.
    fn submit_at(&mut self, op: SessionOp, base: u32, applied: bool) {
        let op_id = if applied { self.pending_id_for_last() } else { self.next_op_id() };
        if !applied {
            let ctx = op_context(&op_id, self.now_session_ms(), self.local_position());
            match apply_op(self.reducer.as_ref(), &self.doc, &op, &ctx, base.saturating_add(1)) {
                Ok(next) => self.doc = next,
                Err(e) => {
                    self.log("debug", format!("op not applicable: {e}"));
                    return;
                }
            }
        }
        self.pending.push_back(PendingOp { op_id: op_id.clone(), op: op.clone(), base_revision: base });
        let epoch = if op.is_owner_op() { self.held.as_ref().map(|h| h.epoch) } else { None };
        let device_id = self.cfg.device.id.clone();
        self.upstream_send(Msg::Op { base_revision: base, op, device_id, op_id, epoch });
    }

    fn pending_id_for_last(&self) -> String {
        format!("{}-{}", self.cfg.device.id, self.op_counter)
    }

    /// Re-send an op with its original id (keys stay stable).
    fn resubmit(&mut self, op_id: String, op: SessionOp) {
        let base = self.doc.revision;
        let ctx = op_context(&op_id, self.now_session_ms(), self.local_position());
        match apply_op(self.reducer.as_ref(), &self.doc, &op, &ctx, base.saturating_add(1)) {
            Ok(next) => self.doc = next,
            Err(e) => {
                self.log("debug", format!("replayed op not applicable: {e}"));
                return;
            }
        }
        self.pending.push_back(PendingOp { op_id: op_id.clone(), op: op.clone(), base_revision: base });
        let epoch = if op.is_owner_op() { self.held.as_ref().map(|h| h.epoch) } else { None };
        let device_id = self.cfg.device.id.clone();
        self.upstream_send(Msg::Op { base_revision: base, op, device_id, op_id, epoch });
    }

    fn on_op_ack(&mut self, op_id: String, revision: u32) {
        let Some(pos) = self.pending.iter().position(|p| p.op_id == op_id) else { return };
        let p = self.pending.remove(pos).expect("position exists");
        let ctx = op_context(&p.op_id, self.now_session_ms(), self.local_position());
        match apply_op(self.reducer.as_ref(), &self.confirmed, &p.op, &ctx, revision) {
            Ok(next) => self.confirmed = next,
            Err(_) => {
                // Should not happen (same op, same base); resync to be safe.
                self.upstream_send(Msg::SyncRequest);
            }
        }
        if self.pending.is_empty() && self.doc.revision != self.confirmed.revision {
            self.doc.revision = self.confirmed.revision;
            self.doc.updated_at = self.confirmed.updated_at;
        }
        if self.is_connected() {
            self.sync_base = Some(SyncPoint { session_id: self.doc.session_id.clone(), revision });
        } else if self.sync_base.is_some() {
            if self.unsynced.len() >= UNSYNCED_CAP {
                self.unsynced_overflow = true;
                self.unsynced.clear();
            } else if !self.unsynced_overflow {
                self.unsynced.push((p.op_id, p.op));
            }
        }
        self.maybe_advertise(false);
    }

    fn on_op_reject(&mut self, op_id: String, current_revision: u32, reason: RejectReason, document: SessionDocument) {
        if !self.pending.iter().any(|p| p.op_id == op_id) {
            return;
        }
        let previous = self.doc.clone();
        self.pending.clear();
        let had_unsynced = !self.unsynced.is_empty() || self.unsynced_overflow;
        self.confirmed = document.clone();
        self.doc = document;
        self.doc.revision = current_revision;
        let d = self.doc.clone();
        self.out.push(Output::DocumentChanged { document: d, cause: DocChange::Rollback });
        if self.is_connected() {
            self.sync_base = Some(SyncPoint { session_id: self.doc.session_id.clone(), revision: current_revision });
        }
        if had_unsynced && !doc_is_trivial(&previous) && !same_session_state(&previous, &self.doc) {
            self.out.push(Output::FilePreviousStateAsSavedQueue {
                document: previous,
                reason: "offline changes could not be replayed".into(),
            });
        }
        self.unsynced.clear();
        self.unsynced_overflow = false;
        if reason == RejectReason::Fenced {
            self.lose_lease();
        }
    }

    fn on_op_committed(&mut self, op: SessionOp, revision: u32, device_id: DeviceId, op_id: String) {
        if device_id == self.cfg.device.id && self.pending.iter().any(|p| p.op_id == op_id) {
            return;
        }
        if !self.pending.is_empty() {
            // Ours will be rejected (stale base); roll back now so one press = one skip.
            self.pending.clear();
            self.doc = self.confirmed.clone();
        }
        let ctx = op_context(&op_id, self.now_session_ms(), self.local_position());
        match apply_op(self.reducer.as_ref(), &self.confirmed, &op, &ctx, revision) {
            Ok(next) => {
                self.confirmed = next;
                self.doc = self.confirmed.clone();
                let d = self.doc.clone();
                self.out.push(Output::DocumentChanged { document: d, cause: DocChange::Remote });
                if self.is_connected() {
                    self.sync_base = Some(SyncPoint { session_id: self.doc.session_id.clone(), revision });
                }
            }
            Err(e) => {
                self.log("warn", format!("could not apply committed op {op_id}: {e}; resyncing"));
                self.upstream_send(Msg::SyncRequest);
            }
        }
        if self.picker.is_some() && op.changes_current() {
            self.reprepare_picker();
        }
        self.maybe_advertise(false);
    }

    fn adopt(&mut self, document: SessionDocument, cause: DocChange) {
        self.pending.clear();
        self.confirmed = document.clone();
        self.doc = document;
        let d = self.doc.clone();
        self.out.push(Output::DocumentChanged { document: d, cause });
        if self.is_connected() {
            self.sync_base = Some(SyncPoint { session_id: self.doc.session_id.clone(), revision: self.doc.revision });
        }
        self.maybe_advertise(false);
    }

    // -- lease ----------------------------------------------------------------

    fn on_lease_granted(&mut self, lease: TransportLease) {
        let now = self.now_local_ms();
        let mine = lease.owner.as_deref() == Some(self.cfg.device.id.as_str());
        self.lease = lease.clone();
        self.remote_transport.lease = lease.clone();
        if mine {
            match &mut self.held {
                Some(h) => {
                    h.epoch = lease.epoch;
                    h.last_ack_at = now;
                }
                None => self.held = Some(HeldLease::new(lease.epoch, now)),
            }
            if self.is_connected() {
                self.held_remote_epoch = Some(lease.epoch);
            }
            self.detached = false;
            if let Some(t) = self.pending_take.take() {
                self.stamp = Some(CurrentStamp {
                    key: Some(t.key.clone()),
                    track_id: Some(t.track_id.clone()),
                    position: PositionStamp { position_ms: t.position_ms, taken_at: self.now_session_ms(), rate: 1.0, is_playing: true },
                    played_ms: t.played_ms,
                    started_at: t.started_at,
                    scrobbled: t.scrobbled,
                });
                self.out.push(Output::TakeTransport {
                    key: t.key,
                    track_id: t.track_id,
                    position_ms: t.position_ms,
                    played_ms: t.played_ms,
                    started_at: t.started_at,
                    scrobbled: t.scrobbled,
                    play: true,
                });
            }
            self.emit_lease();
        } else if self.held.is_some() {
            self.lose_lease();
        } else {
            self.emit_lease();
        }
        if lease.owner.is_some() && self.resume.take().is_some() {
            self.out.push(Output::ResumeOffer(None));
        }
        self.emit_devices();
    }

    fn on_lease_fenced(&mut self, lease: TransportLease) {
        self.lease = lease.clone();
        self.remote_transport.lease = lease;
        self.pending_take = None;
        if self.held.is_some() {
            self.lose_lease();
        }
    }

    fn lose_lease(&mut self) {
        if self.held.take().is_some() {
            self.detached = false;
            self.held_remote_epoch = None;
            self.out.push(Output::ReleaseTransport);
            if self.picker.is_some() {
                self.close_picker(true);
            }
        }
        self.emit_lease();
    }

    fn on_local_stamp(
        &mut self,
        key: Option<QueueKey>,
        track_id: Option<TrackId>,
        position: PositionStamp,
        played_ms: Ms,
        started_at: EpochMs,
        scrobbled: bool,
    ) {
        let mut p = position;
        p.taken_at += self.offset_ms();
        let key_changed = self.stamp.as_ref().map(|s| s.key != key).unwrap_or(true);
        self.stamp = Some(CurrentStamp { key: key.clone(), track_id: track_id.clone(), position: p.clone(), played_ms, started_at, scrobbled });
        let Some(h) = &self.held else {
            self.log("debug", "stamp from a device that does not own transport ignored");
            return;
        };
        let epoch = h.epoch;
        let device_id = self.cfg.device.id.clone();
        self.upstream_send(Msg::TransportStamp { device_id, key, position: p, played_ms, started_at, scrobbled, epoch });
        if key_changed && self.picker.is_some() {
            self.reprepare_picker();
        }
    }

    // -- handoff ----------------------------------------------------------------

    fn emit_picker(&mut self) {
        let (open, targets) = match &self.picker {
            Some(p) => {
                let list: Vec<DeviceInfo> = p
                    .targets
                    .iter()
                    .filter_map(|(id, ready)| {
                        self.devices.iter().find(|d| &d.id == id).map(|d| {
                            let mut d = d.clone();
                            d.ready = *ready;
                            d
                        })
                    })
                    .collect();
                (true, list)
            }
            None => (false, vec![]),
        };
        self.out.push(Output::PickerChanged { open, targets });
    }

    fn open_picker(&mut self) {
        if self.held.is_none() {
            self.log("debug", "picker needs transport ownership");
            self.out.push(Output::PickerChanged { open: false, targets: vec![] });
            return;
        }
        let Some(stamp) = self.stamp.clone() else {
            self.out.push(Output::PickerChanged { open: false, targets: vec![] });
            return;
        };
        let (Some(key), Some(track_id)) = (stamp.key.clone(), stamp.track_id.clone()) else {
            self.out.push(Output::PickerChanged { open: false, targets: vec![] });
            return;
        };
        let targets: Vec<(DeviceId, bool)> = self
            .devices
            .iter()
            .filter(|d| d.id != self.cfg.device.id)
            .take(self.cfg.prebuffer_fanout)
            .map(|d| (d.id.clone(), false))
            .collect();
        self.picker = Some(Picker { key, track_id, targets });
        let from = self.cfg.device.id.clone();
        self.upstream_send(Msg::HandoffPickerOpen { from });
        self.reprepare_picker();
    }

    fn reprepare_picker(&mut self) {
        let Some(stamp) = self.stamp.clone() else { return };
        let (Some(key), Some(track_id)) = (stamp.key.clone(), stamp.track_id.clone()) else {
            self.close_picker(true);
            return;
        };
        let position_ms = extrapolate(&stamp.position, self.now_session_ms());
        let targets: Vec<DeviceId> = match &mut self.picker {
            Some(p) => {
                p.key = key.clone();
                p.track_id = track_id.clone();
                for t in &mut p.targets {
                    t.1 = false;
                }
                p.targets.iter().map(|(id, _)| id.clone()).collect()
            }
            None => return,
        };
        let from = self.cfg.device.id.clone();
        for target in targets {
            self.upstream_send(Msg::HandoffPrepare {
                from: from.clone(),
                target,
                key: key.clone(),
                track_id: track_id.clone(),
                position_ms,
            });
        }
        self.emit_picker();
    }

    fn close_picker(&mut self, notify: bool) {
        if self.picker.take().is_some() {
            if notify {
                let from = self.cfg.device.id.clone();
                self.upstream_send(Msg::HandoffPickerClose { from });
            }
            self.out.push(Output::PickerChanged { open: false, targets: vec![] });
        }
    }

    fn handoff_to(&mut self, device_id: DeviceId) {
        let (Some(h), Some(stamp), Some(p)) = (self.held.clone(), self.stamp.clone(), self.picker.clone()) else {
            self.log("debug", "handoff needs an open picker and transport ownership");
            return;
        };
        if !p.targets.iter().any(|(id, _)| id == &device_id) && !self.devices.iter().any(|d| d.id == device_id) {
            self.log("debug", "handoff target is not present");
            return;
        }
        let position_ms = extrapolate(&stamp.position, self.now_session_ms());
        let from = self.cfg.device.id.clone();
        self.upstream_send(Msg::HandoffTakeover {
            from,
            target: device_id,
            key: p.key,
            track_id: p.track_id,
            position_ms,
            played_ms: stamp.played_ms + position_ms.saturating_sub(stamp.position.position_ms),
            started_at: stamp.started_at,
            scrobbled: stamp.scrobbled,
            epoch: h.epoch,
            lease: None,
        });
        // Release locally right away: the room moves the lease atomically.
        self.held = None;
        self.detached = false;
        self.held_remote_epoch = None;
        self.out.push(Output::ReleaseTransport);
        self.picker = None;
        self.out.push(Output::PickerChanged { open: false, targets: vec![] });
        self.emit_lease();
    }

    fn on_prebuffer_result(&mut self, key: QueueKey, ready: bool) {
        let Some(p) = self.prebuffer.clone() else { return };
        if p.key != key {
            return;
        }
        let target = self.cfg.device.id.clone();
        self.upstream_send(Msg::HandoffReady { from: p.from, target, key, ready });
        if !ready {
            self.prebuffer = None;
        }
    }

    // -- scrobbling -------------------------------------------------------------

    fn on_scrobble_reached(&mut self, track_id: TrackId, started_at: EpochMs) {
        if self.upstream_authoritative() {
            self.query_scrobble(track_id, started_at);
        } else {
            let now = self.now_local_ms();
            self.deferred_scrobbles.push(DeferredScrobble { track_id, started_at, since: now });
        }
    }

    fn query_scrobble(&mut self, track_id: TrackId, started_at: EpochMs) {
        let query_id = self.next_op_id();
        self.scrobble_queries.insert(query_id.clone(), (track_id.clone(), started_at));
        let device_id = self.cfg.device.id.clone();
        self.upstream_send(Msg::ScrobbleDedupeQuery { query_id, track_id, started_at, device_id });
    }

    fn send_ping(&mut self) {
        let t0 = self.now_local_ms();
        self.last_ping_at = t0;
        self.pings_in_burst += 1;
        self.upstream_send(Msg::ClockPing { t0 });
    }

    // -- tick -------------------------------------------------------------------

    fn on_tick(&mut self) {
        let now = self.now_local_ms();

        // Own room timers (lease lapse for inbound peers, member timeouts).
        let outs = self.room.handle(RoomInput::Tick);
        self.process_room_outputs(outs);

        // Remote connection lifecycle.
        if let Some(r) = self.remote.clone() {
            match r.state {
                RemoteState::Connecting { since, .. } if now - since > CONNECT_TIMEOUT_MS => {
                    self.on_upstream_lost(Some("connect timed out".into()));
                }
                RemoteState::Handshaking { peer, since, .. } if now - since > CONNECT_TIMEOUT_MS => {
                    self.out.push(Output::Disconnect { peer });
                    self.on_upstream_lost(Some("handshake timed out".into()));
                }
                RemoteState::Backoff { until, attempt } if now >= until => {
                    if let Some(r) = &mut self.remote {
                        r.state = RemoteState::Connecting { since: now, attempt };
                        let candidates = r.candidates.clone();
                        self.out.push(Output::Connect { candidates });
                    }
                }
                RemoteState::Attached { peer } => {
                    if now - self.last_upstream_msg_at > self.cfg.upstream_idle_ms {
                        self.out.push(Output::Disconnect { peer });
                        self.on_upstream_lost(Some("upstream silent".into()));
                    } else {
                        let interval = if self.pings_in_burst < PING_BURST { PING_BURST_INTERVAL_MS } else { PING_INTERVAL_MS };
                        if now - self.last_ping_at >= interval {
                            self.send_ping();
                        }
                    }
                }
                _ => {}
            }
        } else if self.coordinator_target().is_some() && now >= self.coordinator_retry_at {
            self.reevaluate();
        }
        if matches!(self.remote, Some(Remote { tier: ConnectionTier::Lan, .. }))
            && self.coordinator_target().is_some()
            && now >= self.coordinator_retry_at
        {
            self.coordinator_failures = 0;
            self.reevaluate();
        }

        // Heartbeat while owning transport.
        if let Some(h) = self.held.clone() {
            if h.heartbeat_due(now) && self.upstream_authoritative() {
                if let Some(h) = &mut self.held {
                    h.last_heartbeat_at = now;
                }
                self.upstream_send(Msg::LeaseHeartbeat { epoch: h.epoch });
            }
            if h.lapsed(now) && !self.detached && self.remote.is_some() {
                self.detached = true;
                self.held_remote_epoch = Some(h.epoch);
                self.emit_lease();
            }
        }

        // Pre-buffer nobody picked.
        if let Some(p) = self.prebuffer.clone() {
            if now - p.since >= self.cfg.prebuffer_timeout_ms {
                self.prebuffer = None;
                self.out.push(Output::DiscardPreBuffer);
                let target = self.cfg.device.id.clone();
                self.upstream_send(Msg::HandoffReady { from: p.from, target, key: p.key, ready: false });
            }
        }

        // Deferred scrobbles past the grace period: local judgement.
        let grace = self.cfg.scrobble_grace_ms;
        let (expired, keep): (Vec<DeferredScrobble>, Vec<DeferredScrobble>) =
            self.deferred_scrobbles.drain(..).partition(|d| now - d.since >= grace);
        self.deferred_scrobbles = keep;
        for d in expired {
            self.unreported_scrobbles.push((d.track_id.clone(), d.started_at));
            self.out.push(Output::Scrobble { track_id: d.track_id, started_at: d.started_at, allowed: true });
        }

        self.maybe_advertise(false);
    }
}

/// Persisted alongside the document by the actor.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PersistedConnectState {
    pub sync_base: Option<SyncBase>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::Platform;
    use crate::connect::session_adapter::RealReducer;
    use std::sync::atomic::{AtomicU64, Ordering};

    struct TestClock(AtomicU64);
    impl Clock for TestClock {
        fn now_ms(&self) -> f64 {
            self.0.load(Ordering::SeqCst) as f64
        }
    }

    fn dev(id: &str) -> DeviceInfo {
        DeviceInfo {
            id: id.into(),
            name: id.into(),
            platform: Platform::Linux,
            app_version: "1".into(),
            playing: false,
            ready: false,
            last_seen: 0.0,
            is_self: true,
        }
    }

    fn engine(id: &str) -> (Engine, Arc<TestClock>) {
        let clock = Arc::new(TestClock(AtomicU64::new(10_000)));
        let mut cfg = EngineConfig::new(dev(id), "scope");
        cfg.lan_enabled = false;
        let doc = crate::session::new_document("scope", format!("sid-{id}"), 0.0);
        let e = Engine::new(cfg, clock.clone(), RealReducer::shared(), doc, None);
        (e, clock)
    }

    fn play_op() -> SessionOp {
        SessionOp::PlayTracks {
            server_id: "srv".into(),
            track_ids: vec!["t1".into(), "t2".into(), "t3".into()],
            start_index: 0,
            label: "sel".into(),
            shuffle: false,
            save_outgoing: false,
        }
    }

    fn wire_outs(outs: &[Output]) -> Vec<(PeerId, Msg)> {
        outs.iter()
            .filter_map(|o| match o {
                Output::WireOut { peer, msg } => Some((peer.clone(), msg.msg.clone())),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn alone_ops_confirm_immediately_through_own_room() {
        let (mut e, _) = engine("a");
        let outs = e.handle(Input::LocalOp { op: play_op() });
        assert!(matches!(outs[0], Output::DocumentChanged { cause: DocChange::Local, .. }));
        assert_eq!(e.document().revision, 1);
        assert_eq!(e.document().current.as_ref().unwrap().track_id, "t1");
        assert_eq!(e.room().revision(), 1);
        assert!(e.pending.is_empty());
        assert!(wire_outs(&outs).is_empty());
        assert_eq!(e.connection_state().tier, ConnectionTier::Local);
        let outs = e.handle(Input::LocalOp { op: SessionOp::Next });
        assert!(outs.iter().any(|o| matches!(o, Output::ReplicaChanged(_))));
        assert_eq!(e.document().current.as_ref().unwrap().track_id, "t2");
    }

    #[test]
    fn claim_and_release_transport_alone() {
        let (mut e, _) = engine("a");
        e.handle(Input::LocalOp { op: play_op() });
        let outs = e.handle(Input::ClaimTransport { takeover: false });
        assert!(outs.iter().any(|o| matches!(o, Output::LeaseChanged { owns: true, detached: false, .. })));
        assert!(e.owns_transport());
        assert_eq!(e.devices()[0].playing, true);
        let outs = e.handle(Input::ReleaseTransport);
        assert!(outs.iter().any(|o| matches!(o, Output::LeaseChanged { owns: false, .. })));
        assert!(!e.owns_transport());
    }

    #[test]
    fn coordinator_handshake_and_fresh_room_push() {
        let (mut e, _) = engine("a");
        e.handle(Input::LocalOp { op: play_op() });
        let outs = e.handle(Input::SetCoordinatorUrl(Some("wss://c/".into())));
        assert!(outs.iter().any(|o| matches!(o, Output::Connect { candidates } if candidates[0] == "wss://c/")));
        let outs = e.handle(Input::Connected { peer: "up".into(), url: "wss://c/".into() });
        let w = wire_outs(&outs);
        assert!(matches!(&w[0], (p, Msg::Hello { session_revision: 1, .. }) if p == "up"));
        let outs = e.handle(Input::WireIn {
            peer: "up".into(),
            msg: WireMessage::new(Msg::Welcome {
                session_clock_ms: 500_000.0,
                accepted_protocol: 1,
                replica: None,
                members: vec![],
                extra: Default::default(),
            }),
        });
        let w = wire_outs(&outs);
        assert!(w.iter().any(|(_, m)| matches!(m, Msg::ClockPing { .. })));
        assert!(w.iter().any(|(_, m)| matches!(m, Msg::Op { base_revision: 0, op: SessionOp::Replace { .. }, .. })));
        assert!(e.is_connected());
        assert_eq!(e.connection_state().tier, ConnectionTier::Coordinator);
        // provisional offset ~ 490_000
        assert!((e.offset_ms() - 490_000.0).abs() < 2000.0);
        let outs = e.handle(Input::WireIn { peer: "up".into(), msg: WireMessage::new(Msg::OpAck { op_id: "a-2".into(), revision: 1 }) });
        assert!(outs.is_empty() || !outs.iter().any(|o| matches!(o, Output::DocumentChanged { .. })));
        assert_eq!(e.sync_base().unwrap().revision, 1);
        assert!(e.pending.is_empty());
    }

    fn attach(e: &mut Engine, replica: Option<ReplicaState>, members: Vec<DeviceInfo>) -> Vec<Output> {
        e.handle(Input::SetCoordinatorUrl(Some("wss://c/".into())));
        e.handle(Input::Connected { peer: "up".into(), url: "wss://c/".into() });
        e.handle(Input::WireIn {
            peer: "up".into(),
            msg: WireMessage::new(Msg::Welcome {
                session_clock_ms: 10_000.0,
                accepted_protocol: 1,
                replica,
                members,
                extra: Default::default(),
            }),
        })
    }

    fn replica_with(doc: SessionDocument) -> ReplicaState {
        crate::connect::replica::new_replica(doc, 0.0)
    }

    #[test]
    fn behind_device_adopts_without_filing() {
        let (mut e, _) = engine("a");
        // pretend we synced at revision 1 of session "s"
        let mut room_doc = crate::session::new_document("scope", "s".into(), 0.0);
        room_doc.revision = 1;
        e.doc = room_doc.clone();
        e.confirmed = room_doc.clone();
        let mut newer = room_doc.clone();
        newer.revision = 5;
        newer.autoplay = true;
        let outs = attach(&mut e, Some(replica_with(newer)), vec![]);
        assert!(outs.iter().any(|o| matches!(o, Output::DocumentChanged { cause: DocChange::Sync, document } if document.revision == 5 && document.autoplay)));
        assert!(!outs.iter().any(|o| matches!(o, Output::FilePreviousStateAsSavedQueue { .. })));
        assert_eq!(e.sync_base().unwrap().revision, 5);
    }

    #[test]
    fn diverged_device_files_saved_queue_and_adopts() {
        let (mut e, _) = engine("a");
        e.handle(Input::LocalOp { op: play_op() }); // local state, never synced
        let mut room_doc = crate::session::new_document("scope", "other".into(), 0.0);
        room_doc.revision = 3;
        room_doc.autoplay = true;
        let outs = attach(&mut e, Some(replica_with(room_doc)), vec![dev("b")]);
        assert!(outs.iter().any(|o| matches!(o, Output::FilePreviousStateAsSavedQueue { document, .. } if document.current.is_some())));
        assert!(outs.iter().any(|o| matches!(o, Output::DocumentChanged { cause: DocChange::Sync, document } if document.session_id == "other")));
        assert_eq!(e.document().session_id, "other");
        assert!(e.document().current.is_none());
    }

    #[test]
    fn offline_ops_fast_forward_when_nobody_moved() {
        let (mut e, _) = engine("a");
        let mut room_doc = crate::session::new_document("scope", "s".into(), 0.0);
        room_doc.revision = 2;
        room_doc.context = Some(crate::api::QueueContext {
            server_id: "srv".into(),
            kind: crate::api::ContextKind::AdHoc { label: "x".into() },
            label: "x".into(),
            sort: Default::default(),
            tracks: vec!["t1".into(), "t2".into(), "t3".into()],
        });
        room_doc.current = Some(crate::api::QueueItem {
            key: "k1".into(),
            track_id: "t1".into(),
            source: crate::api::QueueSource::Context { index: 0 },
            unavailable: false,
        });
        room_doc.cursor = 1;
        // attached and synced at revision 2
        attach(&mut e, Some(replica_with(room_doc.clone())), vec![]);
        assert_eq!(e.sync_base().unwrap().revision, 2);
        // connection drops; user presses next twice offline
        let outs = e.handle(Input::Disconnected { peer: "up".into() });
        assert!(outs.iter().any(|o| matches!(o, Output::ConnectionChanged(s) if !s.connected)));
        e.handle(Input::LocalOp { op: SessionOp::Next });
        e.handle(Input::LocalOp { op: SessionOp::Next });
        assert_eq!(e.unsynced.len(), 2);
        assert_eq!(e.document().current.as_ref().unwrap().track_id, "t3");
        // reconnect: room still at revision 2 → replay
        e.clock.0.store(100_000, Ordering::SeqCst);
        e.handle(Input::Tick);
        e.handle(Input::Connected { peer: "up2".into(), url: "wss://c/".into() });
        let outs = e.handle(Input::WireIn {
            peer: "up2".into(),
            msg: WireMessage::new(Msg::Welcome {
                session_clock_ms: 100_000.0,
                accepted_protocol: 1,
                replica: Some(replica_with(room_doc)),
                members: vec![],
                extra: Default::default(),
            }),
        });
        let ops: Vec<&Msg> = outs
            .iter()
            .filter_map(|o| match o {
                Output::WireOut { msg, .. } if matches!(msg.msg, Msg::Op { .. }) => Some(&msg.msg),
                _ => None,
            })
            .collect();
        assert_eq!(ops.len(), 2);
        assert!(matches!(ops[0], Msg::Op { base_revision: 2, op: SessionOp::Next, .. }));
        assert!(matches!(ops[1], Msg::Op { base_revision: 3, op: SessionOp::Next, .. }));
        assert!(!outs.iter().any(|o| matches!(o, Output::FilePreviousStateAsSavedQueue { .. })));
        assert_eq!(e.document().current.as_ref().unwrap().track_id, "t3");
    }

    #[test]
    fn optimistic_op_rolls_back_when_someone_else_commits_first() {
        let (mut e, _) = engine("a");
        let mut room_doc = crate::session::new_document("scope", "s".into(), 0.0);
        room_doc.revision = 1;
        attach(&mut e, Some(replica_with(room_doc)), vec![dev("b")]);
        e.handle(Input::WireIn {
            peer: "up".into(),
            msg: WireMessage::new(Msg::OpCommitted { op: play_op(), revision: 2, device_id: "b".into(), op_id: "b-1".into() }),
        });
        assert_eq!(e.document().current.as_ref().unwrap().track_id, "t1");
        e.handle(Input::LocalOp { op: SessionOp::Next });
        assert_eq!(e.document().current.as_ref().unwrap().track_id, "t2");
        assert_eq!(e.pending.len(), 1);
        // b's next commits at revision 3 before ours is answered
        let outs = e.handle(Input::WireIn {
            peer: "up".into(),
            msg: WireMessage::new(Msg::OpCommitted { op: SessionOp::Next, revision: 3, device_id: "b".into(), op_id: "b-2".into() }),
        });
        assert!(outs.iter().any(|o| matches!(o, Output::DocumentChanged { cause: DocChange::Remote, .. })));
        assert!(e.pending.is_empty());
        // one skip, not two
        assert_eq!(e.document().current.as_ref().unwrap().track_id, "t2");
        assert_eq!(e.document().revision, 3);
        // the late reject for ours is a no-op
        let outs = e.handle(Input::WireIn {
            peer: "up".into(),
            msg: WireMessage::new(Msg::OpReject {
                op_id: "a-2".into(),
                current_revision: 3,
                reason: RejectReason::Stale,
                document: e.document().clone(),
            }),
        });
        assert!(!outs.iter().any(|o| matches!(o, Output::DocumentChanged { .. })));
    }

    #[test]
    fn fenced_owner_releases_and_files_diverged_state() {
        let (mut e, clock) = engine("a");
        let mut room_doc = crate::session::new_document("scope", "s".into(), 0.0);
        room_doc.revision = 1;
        attach(&mut e, Some(replica_with(room_doc.clone())), vec![]);
        e.handle(Input::LocalOp { op: play_op() });
        e.handle(Input::WireIn { peer: "up".into(), msg: WireMessage::new(Msg::OpAck { op_id: "a-2".into(), revision: 2 }) });
        e.handle(Input::ClaimTransport { takeover: false });
        let lease = TransportLease { owner: Some("a".into()), epoch: 1, expires_at: 30_000.0 };
        let outs = e.handle(Input::WireIn { peer: "up".into(), msg: WireMessage::new(Msg::LeaseGranted { lease }) });
        assert!(outs.iter().any(|o| matches!(o, Output::LeaseChanged { owns: true, .. })));
        // network dies; we keep playing detached and make an offline change
        e.handle(Input::Disconnected { peer: "up".into() });
        assert!(e.owns_transport() && e.is_detached());
        e.handle(Input::LocalOp { op: SessionOp::Next });
        // meanwhile the session moved on: room at revision 5, someone else owns
        clock.0.store(200_000, Ordering::SeqCst);
        e.handle(Input::Tick);
        e.handle(Input::Connected { peer: "up2".into(), url: "wss://c/".into() });
        let mut moved = e.confirmed.clone();
        moved.revision = 5;
        moved.session_id = "s".into();
        moved.autoplay = true;
        let mut rep = replica_with(moved);
        rep.transport_lease = TransportLease { owner: Some("b".into()), epoch: 3, expires_at: 999_999.0 };
        let outs = e.handle(Input::WireIn {
            peer: "up2".into(),
            msg: WireMessage::new(Msg::Welcome { session_clock_ms: 200_000.0, accepted_protocol: 1, replica: Some(rep), members: vec![dev("b")], extra: Default::default() }),
        });
        assert!(outs.iter().any(|o| matches!(o, Output::FilePreviousStateAsSavedQueue { .. })));
        assert!(wire_outs(&outs).iter().any(|(_, m)| matches!(m, Msg::LeaseClaim { epoch_expected: Some(1), takeover: false })));
        let outs = e.handle(Input::WireIn {
            peer: "up2".into(),
            msg: WireMessage::new(Msg::LeaseFenced { current_epoch: 3, lease: TransportLease { owner: Some("b".into()), epoch: 3, expires_at: 999_999.0 } }),
        });
        assert!(outs.iter().any(|o| matches!(o, Output::ReleaseTransport)));
        assert!(outs.iter().any(|o| matches!(o, Output::LeaseChanged { owns: false, .. })));
        assert!(!e.owns_transport());
    }

    #[test]
    fn handoff_source_and_target_sequence() {
        let (mut src, _) = engine("a");
        let mut room_doc = crate::session::new_document("scope", "s".into(), 0.0);
        room_doc.revision = 1;
        attach(&mut src, Some(replica_with(room_doc.clone())), vec![dev("b"), dev("c")]);
        src.handle(Input::ClaimTransport { takeover: false });
        src.handle(Input::WireIn {
            peer: "up".into(),
            msg: WireMessage::new(Msg::LeaseGranted { lease: TransportLease { owner: Some("a".into()), epoch: 1, expires_at: 99_999.0 } }),
        });
        src.handle(Input::LocalStamp {
            key: Some("k".into()),
            track_id: Some("t".into()),
            position: PositionStamp { position_ms: 1000, taken_at: 10_000.0, rate: 1.0, is_playing: true },
            played_ms: 1000,
            started_at: 5.0,
            scrobbled: false,
        });
        let outs = src.handle(Input::OpenPicker);
        let w = wire_outs(&outs);
        assert!(w.iter().any(|(_, m)| matches!(m, Msg::HandoffPickerOpen { .. })));
        assert_eq!(w.iter().filter(|(_, m)| matches!(m, Msg::HandoffPrepare { .. })).count(), 2);
        assert!(outs.iter().any(|o| matches!(o, Output::PickerChanged { open: true, targets } if targets.len() == 2)));
        let outs = src.handle(Input::WireIn {
            peer: "up".into(),
            msg: WireMessage::new(Msg::HandoffReady { from: "a".into(), target: "b".into(), key: "k".into(), ready: true }),
        });
        assert!(outs.iter().any(|o| matches!(o, Output::PickerChanged { targets, .. } if targets.iter().any(|d| d.id == "b" && d.ready))));
        let outs = src.handle(Input::HandoffTo { device_id: "b".into() });
        let w = wire_outs(&outs);
        assert!(w.iter().any(|(_, m)| matches!(m, Msg::HandoffTakeover { target, epoch: 1, played_ms, .. } if target == "b" && *played_ms >= 1000)));
        assert!(outs.iter().any(|o| matches!(o, Output::ReleaseTransport)));
        assert!(!src.owns_transport());

        // target side
        let (mut tgt, _) = engine("b");
        attach(&mut tgt, Some(replica_with(room_doc)), vec![dev("a")]);
        let outs = tgt.handle(Input::WireIn {
            peer: "up".into(),
            msg: WireMessage::new(Msg::HandoffPrepare { from: "a".into(), target: "b".into(), key: "k".into(), track_id: "t".into(), position_ms: 1000 }),
        });
        assert!(outs.iter().any(|o| matches!(o, Output::PreBuffer { position_ms: 1000, .. })));
        let outs = tgt.handle(Input::PreBufferReady { key: "k".into() });
        assert!(wire_outs(&outs).iter().any(|(_, m)| matches!(m, Msg::HandoffReady { ready: true, .. })));
        let outs = tgt.handle(Input::WireIn {
            peer: "up".into(),
            msg: WireMessage::new(Msg::HandoffTakeover {
                from: "a".into(),
                target: "b".into(),
                key: "k".into(),
                track_id: "t".into(),
                position_ms: 1500,
                played_ms: 1500,
                started_at: 5.0,
                scrobbled: false,
                epoch: 1,
                lease: Some(TransportLease { owner: Some("b".into()), epoch: 2, expires_at: 99_999.0 }),
            }),
        });
        assert!(outs.iter().any(|o| matches!(o, Output::TakeTransport { position_ms: 1500, played_ms: 1500, play: true, .. })));
        assert!(tgt.owns_transport());
        assert_eq!(tgt.held_epoch(), Some(2));
    }

    #[test]
    fn prebuffer_times_out_and_is_discarded() {
        let (mut e, clock) = engine("b");
        attach(&mut e, None, vec![dev("a")]);
        e.handle(Input::WireIn {
            peer: "up".into(),
            msg: WireMessage::new(Msg::HandoffPrepare { from: "a".into(), target: "b".into(), key: "k".into(), track_id: "t".into(), position_ms: 0 }),
        });
        clock.0.store(10_000 + 61_000, Ordering::SeqCst);
        let outs = e.handle(Input::Tick);
        assert!(outs.iter().any(|o| matches!(o, Output::DiscardPreBuffer)));
        assert!(wire_outs(&outs).iter().any(|(_, m)| matches!(m, Msg::HandoffReady { ready: false, .. })));
    }

    #[test]
    fn resume_offer_is_dormant_and_explicit() {
        let (mut e, _) = engine("a");
        let mut room_doc = crate::session::new_document("scope", "s".into(), 0.0);
        room_doc.revision = 1;
        room_doc.current = Some(crate::api::QueueItem { key: "k".into(), track_id: "t".into(), source: crate::api::QueueSource::Inserted, unavailable: false });
        let mut rep = replica_with(room_doc);
        rep.last_stamp = Some(crate::connect::wire::LastStamp {
            device_id: "pixel".into(),
            device_name: "Pixel".into(),
            key: Some("k".into()),
            position: PositionStamp { position_ms: 30_000, taken_at: 9_000.0, rate: 1.0, is_playing: true },
            played_ms: 30_000,
            started_at: 1.0,
            scrobbled: false,
        });
        let outs = attach(&mut e, Some(rep), vec![]);
        let draft = outs.iter().find_map(|o| match o {
            Output::ResumeOffer(Some(d)) => Some(d.clone()),
            _ => None,
        });
        let draft = draft.expect("offer");
        assert_eq!(draft.device_name, "Pixel");
        assert_eq!(draft.position_ms, 31_000);
        assert!(!outs.iter().any(|o| matches!(o, Output::TakeTransport { .. } | Output::LeaseChanged { owns: true, .. })));
        let outs = e.handle(Input::ResumeHere);
        assert!(wire_outs(&outs).iter().any(|(_, m)| matches!(m, Msg::LeaseClaim { takeover: true, .. })));
        let outs = e.handle(Input::WireIn {
            peer: "up".into(),
            msg: WireMessage::new(Msg::LeaseGranted { lease: TransportLease { owner: Some("a".into()), epoch: 4, expires_at: 99_999.0 } }),
        });
        assert!(outs.iter().any(|o| matches!(o, Output::TakeTransport { position_ms: 31_000, played_ms: 30_000, .. })));
        assert!(outs.iter().any(|o| matches!(o, Output::ResumeOffer(None))));
        assert!(e.resume_offer().is_none());
    }

    #[test]
    fn scrobble_verdicts_follow_the_room_and_defer_when_cut_off() {
        let (mut e, clock) = engine("a");
        // alone: own room answers immediately
        let outs = e.handle(Input::ScrobbleReached { track_id: "t".into(), started_at: 1.0 });
        assert!(outs.iter().any(|o| matches!(o, Output::Scrobble { allowed: true, .. })));
        let outs = e.handle(Input::ScrobbleReached { track_id: "t".into(), started_at: 1.0 });
        assert!(outs.iter().any(|o| matches!(o, Output::Scrobble { allowed: true, .. }))); // own retry
        attach(&mut e, None, vec![]);
        let outs = e.handle(Input::ScrobbleReached { track_id: "t2".into(), started_at: 2.0 });
        assert!(wire_outs(&outs).iter().any(|(_, m)| matches!(m, Msg::ScrobbleDedupeQuery { .. })));
        let qid = wire_outs(&outs)
            .iter()
            .find_map(|(_, m)| match m {
                Msg::ScrobbleDedupeQuery { query_id, .. } => Some(query_id.clone()),
                _ => None,
            })
            .unwrap();
        let outs = e.handle(Input::WireIn { peer: "up".into(), msg: WireMessage::new(Msg::ScrobbleDedupeAnswer { query_id: qid, duplicate: true }) });
        assert!(outs.iter().any(|o| matches!(o, Output::Scrobble { allowed: false, .. })));
        // cut off: defer, then local judgement after the grace period
        e.handle(Input::Disconnected { peer: "up".into() });
        let outs = e.handle(Input::ScrobbleReached { track_id: "t3".into(), started_at: 3.0 });
        assert!(!outs.iter().any(|o| matches!(o, Output::Scrobble { .. })));
        clock.0.store(10_000 + 700_000, Ordering::SeqCst);
        let outs = e.handle(Input::Tick);
        assert!(outs.iter().any(|o| matches!(o, Output::Scrobble { allowed: true, .. })));
    }

    #[test]
    fn reconnect_backs_off_and_retries() {
        let (mut e, clock) = engine("a");
        e.handle(Input::SetCoordinatorUrl(Some("wss://c/".into())));
        let outs = e.handle(Input::ConnectFailed { error: "refused".into() });
        assert!(outs.iter().any(|o| matches!(o, Output::ConnectionChanged(s) if s.error.as_deref() == Some("refused"))));
        let outs = e.handle(Input::Tick);
        assert!(!outs.iter().any(|o| matches!(o, Output::Connect { .. })));
        clock.0.store(10_000 + 1_100, Ordering::SeqCst);
        let outs = e.handle(Input::Tick);
        assert!(outs.iter().any(|o| matches!(o, Output::Connect { .. })));
        e.handle(Input::ConnectFailed { error: "refused".into() });
        clock.0.store(10_000 + 1_100 + 1_500, Ordering::SeqCst);
        assert!(!e.handle(Input::Tick).iter().any(|o| matches!(o, Output::Connect { .. })));
        clock.0.store(10_000 + 1_100 + 2_100, Ordering::SeqCst);
        assert!(e.handle(Input::Tick).iter().any(|o| matches!(o, Output::Connect { .. })));
    }

    #[test]
    fn heartbeats_and_detachment() {
        let (mut e, clock) = engine("a");
        attach(&mut e, None, vec![]);
        e.handle(Input::ClaimTransport { takeover: false });
        e.handle(Input::WireIn {
            peer: "up".into(),
            msg: WireMessage::new(Msg::LeaseGranted { lease: TransportLease { owner: Some("a".into()), epoch: 1, expires_at: 99_999.0 } }),
        });
        clock.0.store(15_100, Ordering::SeqCst);
        let outs = e.handle(Input::Tick);
        assert!(wire_outs(&outs).iter().any(|(_, m)| matches!(m, Msg::LeaseHeartbeat { epoch: 1 })));
        // no acks: after 20 s we're detached but still playing
        clock.0.store(31_000, Ordering::SeqCst);
        let outs = e.handle(Input::Tick);
        assert!(outs.iter().any(|o| matches!(o, Output::LeaseChanged { owns: true, detached: true, .. })));
        assert!(e.owns_transport());
    }

    #[test]
    fn lan_election_picks_leader_and_connects_or_serves() {
        let (mut e, _) = engine("m");
        e.handle(Input::SetLanDiscovery(true));
        let advert = |id: &str, rev: u32| PeerAdvert {
            device_id: id.into(),
            device_name: id.into(),
            platform: Platform::Android,
            scope_hash: scope_hash("scope"),
            port: 5,
            protocol: 1,
            session_revision: rev,
            serving: false,
            addresses: vec!["10.0.0.2".into()],
        };
        // a peer with a higher revision wins: we connect to it
        let outs = e.handle(Input::PeerDiscovered(advert("z", 9)));
        assert!(outs.iter().any(|o| matches!(o, Output::Connect { candidates } if candidates[0] == "ws://10.0.0.2:5/")));
        assert_eq!(e.connection_state().tier, ConnectionTier::Lan);
        // it disappears: we're alone again
        let outs = e.handle(Input::PeerLost { device_id: "z".into() });
        assert!(outs.iter().any(|o| matches!(o, Output::ConnectionChanged(s) if s.tier == ConnectionTier::Local)));
        // a lower-id peer with the same revision (0) wins over us ("m")
        e.handle(Input::PeerDiscovered(advert("a", 0)));
        assert!(matches!(e.remote, Some(Remote { tier: ConnectionTier::Lan, .. })));
        // a higher-id peer with the same revision: we win and serve
        e.handle(Input::PeerLost { device_id: "a".into() });
        let outs = e.handle(Input::PeerDiscovered(advert("q", 0)));
        assert!(!outs.iter().any(|o| matches!(o, Output::Connect { .. })));
        assert!(e.remote.is_none());
        // other-scope peers are ignored
        let mut foreign = advert("b", 99);
        foreign.scope_hash = "nope".into();
        e.handle(Input::PeerDiscovered(foreign));
        assert!(e.remote.is_none());
        // inbound peer joins our room
        e.handle(Input::ListenerStarted { port: 7 });
        e.handle(Input::PeerConnected { peer: "in1".into() });
        let outs = e.handle(Input::WireIn {
            peer: "in1".into(),
            msg: WireMessage::new(Msg::Hello {
                device: dev("q"),
                protocol_min: 1,
                protocol_max: 1,
                scope: "scope".into(),
                credential: None,
                session_id: None,
                session_revision: 0,
                held_epoch: None,
                extra: Default::default(),
            }),
        });
        assert!(wire_outs(&outs).iter().any(|(p, m)| p == "in1" && matches!(m, Msg::Welcome { .. })));
        assert!(e.is_serving());
        assert_eq!(e.connection_state().tier, ConnectionTier::Lan);
        assert!(outs.iter().any(|o| matches!(o, Output::Advertise(Some(a)) if a.serving && a.port == 7)));
    }

    #[test]
    fn unknown_wire_messages_are_ignored() {
        let (mut e, _) = engine("a");
        attach(&mut e, None, vec![]);
        let outs = e.handle(Input::WireIn { peer: "up".into(), msg: WireMessage::new(Msg::Unknown) });
        assert!(outs.is_empty());
        let outs = e.handle(Input::WireIn { peer: "stranger".into(), msg: WireMessage::new(Msg::SyncRequest) });
        assert!(outs.is_empty());
    }

    #[test]
    fn saved_queues_and_settings_sync_out_and_merge_in() {
        use crate::api::{Setting, SettingScope};
        let (mut e, _) = engine("a");
        attach(&mut e, None, vec![]);
        let s = Setting { key: "k".into(), value: "1".into(), scope: SettingScope::AccountSynced, updated_at: 5.0 };
        let outs = e.handle(Input::SettingChanged(s.clone()));
        assert!(wire_outs(&outs).iter().any(|(_, m)| matches!(m, Msg::SettingsSync { settings } if settings.len() == 1)));
        let newer = Setting { value: "2".into(), updated_at: 9.0, ..s.clone() };
        let outs = e.handle(Input::WireIn { peer: "up".into(), msg: WireMessage::new(Msg::SettingsSync { settings: vec![newer] }) });
        assert!(outs.iter().any(|o| matches!(o, Output::SettingsMerged(m) if m[0].value == "2")));
        let older = Setting { value: "0".into(), updated_at: 1.0, ..s };
        let outs = e.handle(Input::WireIn { peer: "up".into(), msg: WireMessage::new(Msg::SettingsSync { settings: vec![older] }) });
        assert!(!outs.iter().any(|o| matches!(o, Output::SettingsMerged(_))));
        assert_eq!(e.settings()[0].value, "2");
    }
}
