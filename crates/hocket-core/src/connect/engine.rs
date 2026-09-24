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

use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::api::{
    ConnectionState, ConnectionTier, DeviceId, DeviceInfo, EpochMs, Ms, PositionStamp, QueueKey,
    SavedQueue, SessionDocument, Setting, TrackId, TransportLease, TransportState, UndoEntry,
};
use crate::connect::auth::{self, LanKey, Role};
use crate::connect::clock::{extrapolate, resume_position, ClockSample, OffsetEstimator};
use crate::connect::discovery::{scope_hash, PeerAdvert};
use crate::connect::election::{elect_id, Candidate};
use crate::connect::lease::HeldLease;
use crate::connect::replica::ReplicaExt;
use crate::connect::room::{LanAuth, Room, RoomConfig, RoomInput, RoomOutput};
use crate::connect::transport::Backoff;
use crate::connect::wire::{
    merge_saved_queues, merge_settings, Credential, LastStamp, Msg, RefuseReason, RejectReason,
    ReplicaState, TransportCommand, WireMessage, PROTOCOL, PROTOCOL_MIN,
};
use crate::connect::{
    apply_op, doc_is_trivial, op_context, same_session_state, PeerId, ReducerHandle, SessionOp,
    SyncPoint, LOOPBACK,
};
use crate::util::Clock;

/// Default pre-buffer fan-out cap.
pub const DEFAULT_PREBUFFER_FANOUT: usize = 4;
/// Targets discard a pre-buffer nobody picked after this long.
pub const DEFAULT_PREBUFFER_TIMEOUT_MS: f64 = 60_000.0;
/// Scrobbles reached while cut off from the room wait this long for the
/// dedupe log before being submitted on local judgement.
pub const DEFAULT_SCROBBLE_GRACE_MS: f64 = 600_000.0;
/// An attached upstream that says nothing for this long is dead.
pub const DEFAULT_UPSTREAM_IDLE_MS: f64 = 30_000.0;
/// A scrobble dedupe query without an answer is asked again after this long.
pub const SCROBBLE_QUERY_RETRY_MS: f64 = 10_000.0;
/// A LAN leader whose members have all been silent this long (two missed
/// heartbeat/ping periods) may be the one that is cut off: its own room
/// stops being authoritative for scrobble verdicts until they speak again.
pub const MEMBER_QUIET_MS: f64 = 12_000.0;
/// After the engine starts (or LAN discovery is switched on) mDNS needs a
/// moment to report the peers already on the network: until then being
/// alone proves nothing, and our own room is not the scrobble authority (a
/// restarted device would otherwise judge its outbox in a room of one).
pub const LAN_SETTLE_MS: f64 = 5_000.0;
/// Clock ping cadence once synced.
const PING_INTERVAL_MS: f64 = 5_000.0;
/// Pings sent quickly after joining to converge the offset.
const PING_BURST: u32 = 3;
const PING_BURST_INTERVAL_MS: f64 = 300.0;
/// Connect / handshake give up after this long.
const CONNECT_TIMEOUT_MS: f64 = 15_000.0;
/// While on the LAN tier with a coordinator configured, retry it this often.
const COORDINATOR_RETRY_MS: f64 = 60_000.0;
/// A LAN peer that failed the mutual proof (or refused our challenge) is
/// left out of elections for this long. Its advert alone earns no trust,
/// so a flapping advert cannot reset the clock.
pub const LAN_BLOCK_MS: f64 = 600_000.0;
/// A LAN leader that keeps turning us away before the proof (`Bye`,
/// `Refuse{Full}`) is blocked after this many consecutive attempts, so an
/// unauthenticated advert cannot keep every device busy knocking. The
/// block starts short (an honest peer mid-election is turned away too)
/// and doubles each time, up to [`LAN_BLOCK_MS`].
const LAN_STRIKES: u32 = 3;
const LAN_STRIKE_BLOCK_MS: f64 = 30_000.0;
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
    /// Sent only to a coordinator over TLS (see [`EngineConfig::allow_insecure_coordinator`]).
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
    /// The scope's shared LAN key from
    /// [`auth::derive_lan_key`](crate::connect::auth::derive_lan_key). LAN
    /// rooms are served and followed only with one: `None` means this
    /// device never serves or follows LAN peers (it still advertises
    /// nothing and refuses inbound sockets).
    pub lan_key: Option<LanKey>,
    /// Let the credential travel to a `ws://` coordinator on a private
    /// (non-loopback) host: the `connect.allowInsecureCoordinator` setting.
    /// A `ws://` coordinator on a public host is never used.
    pub allow_insecure_coordinator: bool,
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
            lan_key: None,
            allow_insecure_coordinator: false,
        }
    }

    fn room_lan_auth(&self) -> LanAuth {
        match &self.lan_key {
            Some(key) => LanAuth::Key {
                key: key.clone(),
                device_id: self.device.id.clone(),
            },
            None => LanAuth::Closed,
        }
    }
}

/// A `(track, startedAt)` pair this device knows was scrobbled, and by whom.
/// Persist [`Engine::known_scrobbled`] and hand it back through
/// [`Engine::restore_known_scrobbled`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KnownScrobble {
    pub track_id: TrackId,
    pub started_at: EpochMs,
    pub device_id: DeviceId,
}

/// What the actor feeds the engine.
///
/// Variants carry whole frames and documents on purpose (handled once, then
/// dropped); boxing would only move the allocation.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, PartialEq)]
pub enum Input {
    // -- configuration ----------------------------------------------------
    SetCoordinatorUrl(Option<String>),
    ConnectCoordinator,
    DisconnectCoordinator,
    SetLanDiscovery(bool),
    SetCredential(Option<Credential>),
    /// The scope's LAN key changed (password change); `None` closes the LAN.
    SetLanKey(Option<LanKey>),
    /// The `connect.allowInsecureCoordinator` setting changed.
    SetAllowInsecureCoordinator(bool),

    // -- I/O reports -------------------------------------------------------
    /// The upstream socket requested by `Output::Connect` is open.
    Connected {
        peer: PeerId,
        url: String,
    },
    ConnectFailed {
        error: String,
    },
    /// Any socket closed (upstream or inbound).
    Disconnected {
        peer: PeerId,
    },
    ListenerStarted {
        port: u16,
    },
    ListenerStopped,
    /// An inbound socket to our listener.
    PeerConnected {
        peer: PeerId,
    },
    WireIn {
        peer: PeerId,
        msg: WireMessage,
    },
    /// Answer to `Output::VerifyCredential`.
    CredentialVerified {
        peer: PeerId,
        ok: bool,
    },
    PeerDiscovered(PeerAdvert),
    PeerLost {
        device_id: DeviceId,
    },

    // -- session -----------------------------------------------------------
    LocalOp {
        op: SessionOp,
    },
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
    ClaimTransport {
        takeover: bool,
    },
    /// This device stopped on purpose.
    ReleaseTransport,
    /// Play/pause/seek from this device's UI when it may not own transport.
    TransportRequest(TransportCommand),

    // -- handoff -----------------------------------------------------------
    OpenPicker,
    ClosePicker,
    HandoffTo {
        device_id: DeviceId,
    },
    PreBufferReady {
        key: QueueKey,
    },
    PreBufferFailed {
        key: QueueKey,
    },
    ResumeHere,
    DismissResume,

    // -- scrobbling, LWW sets, undo ---------------------------------------
    ScrobbleReached {
        track_id: TrackId,
        started_at: EpochMs,
    },
    SavedQueuesChanged(Vec<SavedQueue>),
    SettingChanged(Setting),
    UndoEntryCreated {
        entry: UndoEntry,
        before_revision: u32,
        before: Option<SessionDocument>,
    },

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
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, PartialEq)]
pub enum Output {
    WireOut {
        peer: PeerId,
        msg: WireMessage,
    },
    /// Open one upstream socket, trying candidates in order; report with
    /// `Connected` or `ConnectFailed`.
    Connect {
        candidates: Vec<String>,
    },
    Disconnect {
        peer: PeerId,
    },
    StartListener,
    StopListener,
    /// Publish (or withdraw) our mDNS record.
    Advertise(Option<PeerAdvert>),
    /// Proxy a Subsonic ping for an inbound LAN peer; answer `CredentialVerified`.
    VerifyCredential {
        peer: PeerId,
        credential: Credential,
    },

    DocumentChanged {
        document: SessionDocument,
        cause: DocChange,
    },
    /// A remote stamp: extrapolate from it.
    TransportChanged {
        transport: TransportState,
    },
    LeaseChanged {
        lease: TransportLease,
        owns: bool,
        detached: bool,
    },
    ConnectionChanged(ConnectionState),
    DevicesChanged(Vec<DeviceInfo>),
    PickerChanged {
        open: bool,
        targets: Vec<DeviceInfo>,
    },
    ResumeOffer(Option<ResumeOfferDraft>),
    /// Snapshot this document as a saved queue; never merge it.
    FilePreviousStateAsSavedQueue {
        document: SessionDocument,
        reason: String,
    },

    PreBuffer {
        key: QueueKey,
        track_id: TrackId,
        position_ms: Ms,
    },
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
    Scrobble {
        track_id: TrackId,
        started_at: EpochMs,
        allowed: bool,
    },
    SavedQueuesMerged(Vec<SavedQueue>),
    SettingsMerged(Vec<Setting>),
    UndoEntryReceived {
        entry: UndoEntry,
        before_revision: u32,
        before: Option<SessionDocument>,
    },
    /// Our embedded room's replica changed (persist if you like).
    ReplicaChanged(ReplicaState),
    Log {
        level: &'static str,
        message: String,
    },
}

#[derive(Debug, Clone)]
struct PendingOp {
    op_id: String,
    op: SessionOp,
    /// Session time and playback position the op was made with; replicas
    /// apply it with the same values.
    at: EpochMs,
    position_ms: Ms,
}

#[derive(Debug, Clone, PartialEq)]
enum RemoteState {
    Connecting {
        since: EpochMs,
        attempt: u32,
    },
    Handshaking {
        peer: PeerId,
        since: EpochMs,
        attempt: u32,
    },
    Attached {
        peer: PeerId,
    },
    Backoff {
        until: EpochMs,
        attempt: u32,
    },
}

/// Where the mutual LAN proof with an upstream leader stands.
#[derive(Debug, Clone, PartialEq)]
struct LanHandshake {
    /// The nonce we challenged the leader with.
    our_nonce: String,
    /// The leader answered it correctly.
    leader_proven: bool,
    /// The nonce the leader challenged us with.
    their_nonce: Option<String>,
    /// Our proof and Hello went out.
    hello_sent: bool,
}

#[derive(Debug, Clone)]
struct Remote {
    tier: ConnectionTier,
    candidates: Vec<String>,
    leader: Option<DeviceId>,
    state: RemoteState,
    /// LAN tier only.
    handshake: Option<LanHandshake>,
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
    started: bool,
    /// When the last inbound member left while we were serving: a leader
    /// that lost everyone is as cut off as a client that lost its room, so
    /// its own room stops being authoritative for scrobble verdicts.
    lost_members_at: Option<EpochMs>,
    /// Flush deferred scrobble queries at this time (a beat after members
    /// return, so their announcements land in our log first).
    flush_deferred_at: Option<EpochMs>,
    inbound: Vec<PeerId>,

    doc: SessionDocument,
    confirmed: SessionDocument,
    pending: VecDeque<PendingOp>,
    /// Ops rolled back locally that the room may still accept: it commits
    /// them if their base happens to match after someone else's op, and then
    /// only the originator ever hears of it (an `OpAck`). Bounded.
    abandoned: VecDeque<PendingOp>,
    unsynced: Vec<PendingOp>,
    unsynced_overflow: bool,
    sync_base: Option<SyncBase>,
    op_counter: u32,

    remote: Option<Remote>,
    upstream_ready: bool,
    last_upstream_msg_at: EpochMs,
    offset: OffsetEstimator,
    /// Offset implied by the welcome, used until the first pong lands.
    provisional_offset: Option<f64>,
    last_ping_at: EpochMs,
    pings_in_burst: u32,
    last_reported_offset: Option<(f64, Option<f64>)>,
    last_error: Option<String>,
    coordinator_failures: u32,
    coordinator_retry_at: EpochMs,

    lease: TransportLease,
    held: Option<HeldLease>,
    /// Whether the lease we hold was granted by a remote room (as opposed to
    /// our own, while alone or cut off).
    held_from_remote: bool,
    detached: bool,
    held_remote_epoch: Option<u32>,
    stamp: Option<CurrentStamp>,
    remote_transport: TransportState,
    /// The last stamp another device sent (for resume offers when its lease lapses).
    last_remote_stamp: Option<LastStamp>,
    pending_take: Option<TakeInfo>,

    devices: Vec<DeviceInfo>,
    picker: Option<Picker>,
    prebuffer: Option<PreBuffer>,
    resume: Option<ResumeOfferDraft>,

    deferred_scrobbles: Vec<DeferredScrobble>,
    /// Outstanding dedupe queries: id → (track, startedAt, sent at). Re-sent
    /// after [`SCROBBLE_QUERY_RETRY_MS`] without an answer.
    scrobble_queries: BTreeMap<String, (TrackId, EpochMs, EpochMs)>,
    /// Scrobbles decided locally while cut off; told to the room on rejoin.
    unreported_scrobbles: Vec<(TrackId, EpochMs)>,
    /// Every `(track, startedAt)` this device knows was scrobbled by someone:
    /// seeds our own room's dedupe log when we host it (LAN leadership moves
    /// the room between devices; the log must move with the session).
    known_scrobbled: Vec<(TrackId, EpochMs, DeviceId)>,
    saved_queues: Vec<SavedQueue>,
    settings: Vec<Setting>,

    lan_peers: Vec<PeerAdvert>,
    /// LAN peers that failed the mutual proof (or refused us), with the
    /// local time until which the election ignores them.
    lan_blocklist: BTreeMap<DeviceId, EpochMs>,
    /// Consecutive pre-proof turn-aways per LAN leader (see [`LAN_STRIKES`])
    /// and how many blocks that earned so far (doubling the next one).
    lan_strikes: BTreeMap<DeviceId, (u32, u32)>,
    /// We followed a LAN leader while a coordinator is configured: that
    /// leader carries the LAN session to the coordinator, so on the next
    /// coordinator welcome we adopt without filing and without replaying.
    lan_deferred: bool,
    /// Until when LAN discovery is still settling (see [`LAN_SETTLE_MS`]).
    lan_settle_until: EpochMs,
    /// LAN devices that proved the scope's key in a room with us (joined
    /// ours, or we joined theirs, or we met them there). Only these are
    /// waited for before a lone leader judges scrobbles: an advert alone
    /// (a rogue's, say) earns no such patience.
    lan_proven: BTreeSet<DeviceId>,
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
            .field("upstream_ready", &self.upstream_ready)
            .field("inbound", &self.inbound.len())
            .field("owns", &self.held.is_some())
            .field("detached", &self.detached)
            .field("lan_deferred", &self.lan_deferred)
            .field(
                "lan_blocklist",
                &self.lan_blocklist.keys().collect::<Vec<_>>(),
            )
            .field("lost_members_at", &self.lost_members_at)
            .field("deferred_scrobbles", &self.deferred_scrobbles.len())
            .field("scrobble_queries", &self.scrobble_queries.len())
            .field(
                "lan_peers",
                &self
                    .lan_peers
                    .iter()
                    .map(|p| (p.device_id.clone(), p.serving, p.session_revision))
                    .collect::<Vec<_>>(),
            )
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
        room_cfg.lan_auth = cfg.room_lan_auth();
        room_cfg.session_id = Some(document.session_id.clone());
        let room = Room::new(room_cfg, clock.clone(), reducer.clone(), None);
        let mut e = Engine {
            cfg,
            clock,
            reducer,
            room,
            listener_port: None,
            listener_wanted: false,
            started: false,
            lost_members_at: None,
            flush_deferred_at: None,
            inbound: vec![],
            confirmed: document.clone(),
            doc: document,
            pending: VecDeque::new(),
            abandoned: VecDeque::new(),
            unsynced: vec![],
            unsynced_overflow: false,
            sync_base,
            op_counter: 0,
            remote: None,
            upstream_ready: false,
            last_upstream_msg_at: now,
            offset: OffsetEstimator::new(),
            provisional_offset: None,
            last_ping_at: 0.0,
            pings_in_burst: 0,
            last_reported_offset: None,
            last_error: None,
            coordinator_failures: 0,
            coordinator_retry_at: 0.0,
            lease: TransportLease::default(),
            held: None,
            held_from_remote: false,
            detached: false,
            held_remote_epoch: None,
            stamp: None,
            remote_transport: TransportState::default(),
            last_remote_stamp: None,
            pending_take: None,
            devices: vec![],
            picker: None,
            prebuffer: None,
            resume: None,
            deferred_scrobbles: vec![],
            scrobble_queries: BTreeMap::new(),
            unreported_scrobbles: vec![],
            known_scrobbled: vec![],
            saved_queues: vec![],
            settings: vec![],
            lan_peers: vec![],
            lan_blocklist: BTreeMap::new(),
            lan_strikes: BTreeMap::new(),
            lan_deferred: false,
            lan_settle_until: now + LAN_SETTLE_MS,
            lan_proven: BTreeSet::new(),
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

    /// Every scrobble this device knows about, its own room's dedupe log
    /// included (what it judged for LAN members while it led); persist it
    /// with the sync base so a restarted LAN leader still answers dedupe
    /// queries. At most 500 entries, newest last.
    pub fn known_scrobbled(&self) -> Vec<KnownScrobble> {
        let mut out: Vec<KnownScrobble> = vec![];
        let own_room = self
            .room
            .replica()
            .scrobbles
            .iter()
            .map(|r| (&r.track_id, r.started_at, &r.device_id));
        let known = self.known_scrobbled.iter().map(|(t, s, d)| (t, *s, d));
        for (track_id, started_at, device_id) in known.chain(own_room) {
            if out
                .iter()
                .any(|k| &k.track_id == track_id && (k.started_at - started_at).abs() < 1000.0)
            {
                continue;
            }
            out.push(KnownScrobble {
                track_id: track_id.clone(),
                started_at,
                device_id: device_id.clone(),
            });
        }
        let excess = out.len().saturating_sub(500);
        out.drain(..excess);
        out
    }

    /// Restore what [`Engine::known_scrobbled`] returned before a restart
    /// (call right after [`Engine::new`]). Seeds the embedded room's log
    /// with all of it; other devices' scrobbles also become known locally.
    /// Our own entries only go to the room: they may be claims we never got
    /// to submit (killed between the room's answer and the submission), and
    /// the outbox asking again must hear "yours" from the room rather than
    /// "already done" from us.
    pub fn restore_known_scrobbled(&mut self, known: Vec<KnownScrobble>) {
        let me = self.cfg.device.id.clone();
        let mut seed: Vec<(TrackId, EpochMs, DeviceId)> = vec![];
        for k in known {
            if k.device_id != me {
                self.learn_scrobbled(&k.track_id, k.started_at, &k.device_id);
            }
            seed.push((k.track_id, k.started_at, k.device_id));
        }
        self.room.seed_scrobbles(&seed);
    }

    pub fn owns_transport(&self) -> bool {
        self.held.is_some()
    }

    /// The epoch this device believes it holds, if any.
    pub fn held_epoch(&self) -> Option<u32> {
        self.held.as_ref().map(|h| h.epoch)
    }

    /// The lease we hold was granted by the remote room we are attached to
    /// (false while it came from our own room: alone, serving, or cut off).
    pub fn held_from_remote(&self) -> bool {
        self.held.is_some() && self.held_from_remote
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
        matches!(
            self.remote,
            Some(Remote {
                state: RemoteState::Attached { .. },
                ..
            })
        ) && self.upstream_ready
    }

    /// Serving our own room to inbound LAN peers.
    pub fn is_serving(&self) -> bool {
        self.remote.is_none() && !self.inbound.is_empty()
    }

    pub fn room(&self) -> &Room {
        &self.room
    }

    /// Optimistic ops awaiting the room's verdict.
    pub fn pending_count(&self) -> usize {
        self.pending.len()
    }

    /// Locally confirmed ops not yet confirmed by a remote room.
    pub fn has_unsynced(&self) -> bool {
        !self.unsynced.is_empty() || self.unsynced_overflow
    }

    /// The LAN leader this device follows (tier 2 client), if any.
    pub fn lan_leader(&self) -> Option<&DeviceId> {
        match &self.remote {
            Some(Remote {
                tier: ConnectionTier::Lan,
                leader,
                ..
            }) => leader.as_ref(),
            _ => None,
        }
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
        if !self.is_connected() {
            0.0
        } else if self.offset.sample_count() > 0 {
            self.offset.offset_ms()
        } else {
            self.provisional_offset.unwrap_or(0.0)
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
            round_trip_ms: if self.is_connected() {
                self.offset.round_trip_ms()
            } else {
                None
            },
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
        out.extend(
            self.devices
                .iter()
                .filter(|d| d.id != self.cfg.device.id)
                .cloned(),
        );
        out
    }

    pub fn resume_offer(&self) -> Option<&ResumeOfferDraft> {
        self.resume.as_ref()
    }

    pub fn picker_open(&self) -> bool {
        self.picker.is_some()
    }

    /// Our mDNS record, once the listener has a port (and only with a LAN
    /// key: without one nobody could join us).
    pub fn advert(&self) -> Option<PeerAdvert> {
        let port = self.listener_port?;
        self.cfg.lan_key.as_ref()?;
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
                if enabled && !self.cfg.lan_enabled {
                    self.lan_settle_until = self.now_local_ms() + LAN_SETTLE_MS;
                }
                self.cfg.lan_enabled = enabled;
                if !enabled {
                    self.lan_peers.clear();
                }
                self.reevaluate();
            }
            Input::SetCredential(c) => self.cfg.credential = c,
            Input::SetLanKey(key) => {
                if key.is_some() && self.cfg.lan_key.is_none() {
                    self.lan_settle_until = self.now_local_ms() + LAN_SETTLE_MS;
                }
                self.cfg.lan_key = key;
                self.room.set_lan_auth(self.cfg.room_lan_auth());
                self.lan_proven.clear();
                self.lan_blocklist.clear();
                self.lan_strikes.clear();
                // Inbound members were admitted under the old key.
                let inbound: Vec<PeerId> = self.inbound.drain(..).collect();
                for p in inbound {
                    self.out.push(Output::WireOut {
                        peer: p.clone(),
                        msg: WireMessage::new(Msg::Bye {
                            reason: "LAN key changed".into(),
                        }),
                    });
                    self.out.push(Output::Disconnect { peer: p.clone() });
                    let outs = self.room.handle(RoomInput::Disconnected(p));
                    self.process_room_outputs(outs);
                }
                if matches!(
                    self.remote,
                    Some(Remote {
                        tier: ConnectionTier::Lan,
                        ..
                    })
                ) {
                    self.drop_remote(None);
                }
                self.reevaluate();
                self.emit_connection();
            }
            Input::SetAllowInsecureCoordinator(allow) => {
                self.cfg.allow_insecure_coordinator = allow;
                self.coordinator_failures = 0;
                self.reevaluate();
            }
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
                // The block outlives the advert on purpose; strikes don't.
                self.lan_strikes.remove(&device_id);
                self.reevaluate();
            }
            Input::LocalOp { op } => self.on_local_op(op),
            Input::LocalStamp {
                key,
                track_id,
                position,
                played_ms,
                started_at,
                scrobbled,
            } => self.on_local_stamp(key, track_id, position, played_ms, started_at, scrobbled),
            Input::ClaimTransport { takeover } => {
                if self.held.is_none() || takeover {
                    self.claim(None, takeover);
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
                    self.claim(None, true);
                }
            }
            Input::DismissResume => {
                if self.resume.take().is_some() {
                    self.out.push(Output::ResumeOffer(None));
                }
            }
            Input::ScrobbleReached {
                track_id,
                started_at,
            } => self.on_scrobble_reached(track_id, started_at),
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
            Input::UndoEntryCreated {
                entry,
                before_revision,
                before,
            } => {
                self.upstream_send(Msg::UndoEntryShared {
                    entry,
                    before_revision,
                    before,
                });
            }
            Input::Tick => self.on_tick(),
        }
        self.drain_loops();
        std::mem::take(&mut self.out)
    }

    // -- upstream plumbing ---------------------------------------------------

    fn upstream_peer(&self) -> Option<PeerId> {
        match &self.remote {
            Some(Remote {
                state: RemoteState::Attached { peer },
                ..
            }) => Some(peer.clone()),
            _ => None,
        }
    }

    /// The room we are attached to is the one whose revisions anchor
    /// `sync_base`: the coordinator, or a LAN leader when no coordinator is
    /// configured at all. While a coordinator exists, a LAN room is a
    /// stopgap whose leader carries the session back to it, so followers
    /// keep their coordinator sync point untouched.
    fn records_sync_base(&self) -> bool {
        match &self.remote {
            Some(r) if self.is_connected() => {
                r.tier == ConnectionTier::Coordinator || self.coordinator_target().is_none()
            }
            _ => false,
        }
    }

    /// Attached to a LAN leader while a coordinator is configured.
    fn lan_follower_of_stopgap(&self) -> bool {
        matches!(
            &self.remote,
            Some(Remote {
                tier: ConnectionTier::Lan,
                ..
            })
        ) && self.is_connected()
            && self.coordinator_target().is_some()
    }

    /// Whether the upstream we submit to is the one whose verdicts count for
    /// scrobble dedupe: a remote room we are attached to, or our own when no
    /// remote is wanted at all (no coordinator configured, no LAN peers).
    fn upstream_authoritative(&self) -> bool {
        if self.is_connected() {
            return true;
        }
        if self.remote.is_some() || self.coordinator_target().is_some() {
            return false; // a remote room is wanted and we are not attached to it
        }
        // LAN or alone: our own room is the authority when we are the leader
        // (or nobody else is around) and, if we have members, we have heard
        // from one recently. Members all silent, or all gone a moment ago,
        // means we may be the one who is cut off.
        let now = self.now_local_ms();
        if self.cfg.lan_enabled && self.cfg.lan_key.is_some() && now < self.lan_settle_until {
            return false; // nobody around yet may just mean not heard yet
        }
        if !self.inbound.is_empty() {
            match self.room.last_member_seen() {
                Some(t) if now - t <= MEMBER_QUIET_MS => {}
                _ => return false,
            }
        } else if let Some(lost) = self.lost_members_at {
            if now - lost < self.cfg.scrobble_grace_ms {
                return false;
            }
        }
        match self.elect_lan_leader() {
            None => true,
            Some(l) if l != self.cfg.device.id => false,
            Some(_) => {
                // We lead, but alone while a device we have shared a room
                // with is in view: it is on its way to us (or cut off from
                // us, in a room of its own), and what it knows was
                // scrobbled only reaches our room once it joins. Its
                // announcements, or the grace period, decide.
                !(self.inbound.is_empty() && self.proven_peer_in_view(now))
            }
        }
    }

    /// A LAN peer that proved the key with us before is advertising (and is
    /// not blocked).
    fn proven_peer_in_view(&self, now: EpochMs) -> bool {
        self.lan_peers.iter().any(|p| {
            self.lan_proven.contains(&p.device_id)
                && self
                    .lan_blocklist
                    .get(&p.device_id)
                    .map(|until| now >= *until)
                    .unwrap_or(true)
        })
    }

    /// Remember the devices in our own room as proven LAN peers.
    fn note_proven_members(&mut self) {
        let me = self.cfg.device.id.clone();
        for id in self.room.member_device_ids() {
            if id != me {
                self.lan_proven.insert(id);
            }
        }
    }

    /// Everything this device knows was scrobbled, as room announcements.
    fn announce_known_scrobbles(&mut self) {
        // Our own room's log too: a LAN leader carries what its members
        // scrobbled to the coordinator it joins.
        let known: Vec<KnownScrobble> = self.known_scrobbled().into_iter().rev().take(64).collect();
        for KnownScrobble {
            track_id,
            started_at,
            device_id,
        } in known
        {
            self.upstream_send(Msg::ScrobbleSubmitted {
                track_id,
                started_at,
                device_id,
            });
        }
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
                self.on_upstream_message(m, true);
            }
            guard += 1;
            if !progressed || guard > 10_000 {
                break;
            }
        }
    }

    fn process_room_outputs(&mut self, outs: Vec<RoomOutput>) {
        let was_serving = self.is_serving();
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
                RoomOutput::Verify { peer, credential } => {
                    self.out.push(Output::VerifyCredential { peer, credential })
                }
                RoomOutput::ReplicaChanged => self
                    .out
                    .push(Output::ReplicaChanged(self.room.replica().clone())),
                // Heartbeat-only: nothing worth persisting on a device.
                RoomOutput::ReplicaTouched => {}
            }
        }
        if was_serving && !self.is_serving() {
            // The room timed our last member out: we may be the one cut off.
            self.lost_members_at = Some(self.now_local_ms());
        }
    }

    /// A peer's `scrobbled` flag for a play (in a stamp or a handoff) says
    /// the play was scrobbled, not who did it. When we reached that play
    /// ourselves and our own verdict is still pending, the flag is most
    /// likely our own reach coming back around: recording it under the
    /// peer's name would have us announce "the peer did it" and then get
    /// our own query answered "duplicate", and nobody would submit.
    fn learn_scrobbled_from_peer(&mut self, track_id: &str, started_at: EpochMs, device_id: &str) {
        let same = |t: &TrackId, s: EpochMs| t == track_id && (s - started_at).abs() < 1000.0;
        let own_pending = self
            .deferred_scrobbles
            .iter()
            .any(|d| same(&d.track_id, d.started_at))
            || self.scrobble_queries.values().any(|(t, s, _)| same(t, *s));
        if !own_pending {
            self.learn_scrobbled(track_id, started_at, device_id);
        }
    }

    fn learn_scrobbled(&mut self, track_id: &str, started_at: EpochMs, device_id: &str) {
        if !self
            .known_scrobbled
            .iter()
            .any(|(t, s, _)| t == track_id && (s - started_at).abs() < 1000.0)
        {
            self.known_scrobbled
                .push((track_id.to_string(), started_at, device_id.to_string()));
            if self.known_scrobbled.len() > 500 {
                self.known_scrobbled.remove(0);
            }
        }
    }

    /// Point the upstream at our own room and run the handshake over the loopback.
    fn attach_loopback(&mut self) {
        self.upstream_ready = false;
        self.room.adopt_document(self.doc.clone());
        let known = self.known_scrobbled.clone();
        self.room.seed_scrobbles(&known);
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
        self.out.push(Output::LeaseChanged {
            lease: self.lease.clone(),
            owns: self.held.is_some(),
            detached: self.detached,
        });
    }

    fn emit_devices(&mut self) {
        let d = self.devices();
        self.out.push(Output::DevicesChanged(d));
    }

    fn log(&mut self, level: &'static str, message: impl Into<String>) {
        self.out.push(Output::Log {
            level,
            message: message.into(),
        });
    }

    // -- tier selection ------------------------------------------------------

    /// The coordinator URL we may use: configured, enabled, and acceptable
    /// for carrying the credential (`wss://`, or `ws://` on loopback, or a
    /// private host with the insecure override).
    fn coordinator_target(&self) -> Option<String> {
        if !self.cfg.coordinator_enabled {
            return None;
        }
        let url = self.cfg.coordinator_url.clone()?;
        auth::coordinator_url_check(&url, self.cfg.allow_insecure_coordinator)
            .ok()
            .map(|_| url)
    }

    fn elect_lan_leader(&self) -> Option<DeviceId> {
        if !self.cfg.lan_enabled || self.cfg.lan_key.is_none() || self.lan_peers.is_empty() {
            return None;
        }
        let now = self.now_local_ms();
        let mut cands: Vec<Candidate> = self
            .lan_peers
            .iter()
            .filter(|p| {
                self.lan_blocklist
                    .get(&p.device_id)
                    .map(|until| now >= *until)
                    .unwrap_or(true)
            })
            .map(|p| Candidate {
                device_id: p.device_id.clone(),
                session_revision: p.session_revision,
                serving: p.serving,
            })
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
        if coordinator.is_none() && self.cfg.coordinator_enabled {
            if let Some(url) = &self.cfg.coordinator_url {
                if let Err(e) =
                    auth::coordinator_url_check(url, self.cfg.allow_insecure_coordinator)
                {
                    let msg = format!("coordinator URL not used: {e}");
                    if self.last_error.as_deref() != Some(msg.as_str()) {
                        self.last_error = Some(msg.clone());
                        self.log("warn", msg);
                    }
                }
            }
        }
        let leader = self.elect_lan_leader();
        let lan_alternative = leader
            .as_ref()
            .map(|l| l != &self.cfg.device.id)
            .unwrap_or(false)
            || (leader.is_some() && !self.lan_peers.is_empty());
        let coordinator_usable = coordinator.is_some()
            && (self.coordinator_failures < 2
                || now >= self.coordinator_retry_at
                || !lan_alternative);

        // The listener runs whenever LAN is on; it costs nothing and lets
        // peers find us whichever way the election goes.
        let want_listener = self.cfg.lan_enabled;
        if want_listener != self.listener_wanted {
            self.listener_wanted = want_listener;
            self.out.push(if want_listener {
                Output::StartListener
            } else {
                Output::StopListener
            });
        }

        let desired: Option<(ConnectionTier, Vec<String>, Option<DeviceId>)> = if coordinator_usable
        {
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
                self.on_became_local();
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
                    state: RemoteState::Connecting {
                        since: now,
                        attempt: 0,
                    },
                    handshake: None,
                });
                self.out.push(Output::Connect { candidates });
                self.emit_connection();
            }
        }
        self.maybe_advertise(false);
    }

    /// Inbound members are back: our room is in touch with the session
    /// again. Their scrobble announcements arrive right after their Hello;
    /// flush our deferred queries a beat later so they are judged against them.
    fn members_returned(&mut self) {
        self.lost_members_at = None;
    }

    /// Our own room is now the authority (alone by choice, or elected LAN
    /// coordinator): nothing is "detached" from anywhere any more, and
    /// scrobbles waiting for a remote verdict get ours.
    fn on_became_local(&mut self) {
        if self.remote.is_some() {
            return;
        }
        if self.lan_deferred {
            // We followed a LAN leader that is gone: whatever the LAN did
            // since our coordinator sync point is now ours to carry, and
            // only a whole-document push can reproduce it.
            self.lan_deferred = false;
            let moved = self
                .sync_base
                .as_ref()
                .map(|b| {
                    b.session_id != self.confirmed.session_id
                        || b.revision != self.confirmed.revision
                })
                .unwrap_or(false);
            if moved {
                self.unsynced_overflow = true;
                self.unsynced.clear();
            }
        }
        if self.detached || self.held_remote_epoch.is_some() {
            self.detached = false;
            self.held_remote_epoch = None;
            if self.held.is_some() {
                self.emit_lease();
            }
        }
        if !self.inbound.is_empty() {
            self.members_returned();
        }
        // Only our own room's verdicts count now; if we lost our members
        // recently they wait for them (or the grace) instead.
        if self.upstream_authoritative() && self.flush_deferred_at.is_none() {
            let deferred = std::mem::take(&mut self.deferred_scrobbles);
            for d in deferred {
                self.query_scrobble(d.track_id, d.started_at);
            }
        }
    }

    /// Leave the remote (if any) and return to our own room.
    fn drop_remote(&mut self, error: Option<String>) {
        let was_attached = self.is_connected();
        if let Some(r) = self.remote.take() {
            match r.state {
                RemoteState::Attached { peer } | RemoteState::Handshaking { peer, .. } => {
                    self.out.push(Output::WireOut {
                        peer: peer.clone(),
                        msg: WireMessage::new(Msg::Bye {
                            reason: "leaving".into(),
                        }),
                    });
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
            self.held_from_remote = false;
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
        // Queries the vanished room never answered are asked again on rejoin.
        let now = self.now_local_ms();
        for (_, (track_id, started_at, _)) in std::mem::take(&mut self.scrobble_queries) {
            self.deferred_scrobbles.push(DeferredScrobble {
                track_id,
                started_at,
                since: now,
            });
        }
        // Pending ops will never be acked by the room that's gone: confirm them
        // through our own room instead (same ids, same keys).
        let pending: Vec<PendingOp> = self.pending.drain(..).collect();
        self.doc = self.confirmed.clone();
        self.attach_loopback();
        self.drain_loops();
        for p in pending {
            self.resubmit(p);
        }
        if self.held.is_some() {
            self.claim(None, true);
        }
        self.emit_lease();
        self.emit_connection();
    }

    fn on_upstream_lost(&mut self, error: Option<String>) {
        let now = self.now_local_ms();
        let Some(r) = &self.remote else { return };
        let attempt = match &r.state {
            RemoteState::Connecting { attempt, .. } | RemoteState::Handshaking { attempt, .. } => {
                *attempt
            }
            RemoteState::Attached { .. } => 0,
            RemoteState::Backoff { attempt, .. } => *attempt,
        };
        let was_attached = matches!(r.state, RemoteState::Attached { .. });
        let tier = r.tier;
        let delay = self.cfg.backoff.delay_ms(attempt);
        self.log(
            "debug",
            format!(
                "upstream {tier:?} {} lost (attempt {attempt}): {}; retry in {delay:.0} ms",
                r.leader.as_deref().unwrap_or("coordinator"),
                error.as_deref().unwrap_or("closed")
            ),
        );
        if let Some(r) = &mut self.remote {
            r.state = RemoteState::Backoff {
                until: now + delay,
                attempt: attempt + 1,
            };
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
        if tier == ConnectionTier::Coordinator
            && self.coordinator_failures >= 2
            && !self.lan_peers.is_empty()
        {
            self.reevaluate();
        }
    }

    fn on_connected(&mut self, peer: PeerId, url: String) {
        let now = self.now_local_ms();
        let Some(r) = &mut self.remote else {
            self.out.push(Output::Disconnect { peer });
            return;
        };
        if !r.candidates.contains(&url) || !matches!(r.state, RemoteState::Connecting { .. }) {
            // A socket from an attempt the election has since superseded (or a
            // duplicate): it leads to the wrong room. Never adopt it.
            self.log("debug", format!("dropping a stale connection to {url}"));
            self.out.push(Output::Disconnect { peer });
            return;
        }
        let attempt = match &r.state {
            RemoteState::Connecting { attempt, .. } => *attempt,
            _ => 0,
        };
        r.state = RemoteState::Handshaking {
            peer: peer.clone(),
            since: now,
            attempt,
        };
        if let Some(l) = &r.leader {
            self.last_known_lan.insert(l.clone(), url.clone());
        }
        let tier = r.tier;
        self.last_upstream_msg_at = now;
        match tier {
            ConnectionTier::Coordinator => {
                // The credential goes to the coordinator only, and only when
                // the URL passed `coordinator_url_check` (TLS, loopback, or
                // the explicit insecure override on a private host).
                let credential =
                    if auth::coordinator_url_check(&url, self.cfg.allow_insecure_coordinator)
                        .is_ok()
                    {
                        self.cfg.credential.clone()
                    } else {
                        None
                    };
                let hello = self.hello_msg(credential);
                self.out.push(Output::WireOut {
                    peer,
                    msg: WireMessage::new(hello),
                });
            }
            _ => {
                // LAN: say nothing but a nonce until the leader proves it
                // holds the scope's key. The Hello (and never a credential)
                // follows its proof.
                if self.cfg.lan_key.is_none() {
                    self.out.push(Output::Disconnect { peer });
                    self.drop_remote(Some("no LAN key".into()));
                    self.emit_connection();
                    return;
                }
                let nonce = auth::new_nonce();
                if let Some(r) = &mut self.remote {
                    r.handshake = Some(LanHandshake {
                        our_nonce: nonce.clone(),
                        leader_proven: false,
                        their_nonce: None,
                        hello_sent: false,
                    });
                }
                self.out.push(Output::WireOut {
                    peer,
                    msg: WireMessage::new(Msg::Challenge { nonce }),
                });
            }
        }
    }

    /// The LAN leader we are talking to failed the proof (or refused us):
    /// forget it until its advert goes away and re-run the election.
    fn reject_lan_leader(&mut self, reason: &str) {
        let Some(r) = &self.remote else { return };
        if r.tier != ConnectionTier::Lan {
            return;
        }
        if let Some(l) = r.leader.clone() {
            self.log(
                "warn",
                format!("LAN peer {l} rejected: {reason}; ignoring it for a while"),
            );
            let until = self.now_local_ms() + LAN_BLOCK_MS;
            self.lan_blocklist.insert(l.clone(), until);
            self.lan_strikes.remove(&l);
        }
        self.drop_remote(Some(format!("LAN peer rejected: {reason}")));
        self.reevaluate();
        self.emit_connection();
    }

    /// The LAN leader turned us away before the proof (`Bye`, full): an
    /// ordinary loss the first times, a block once it keeps happening.
    /// Returns whether it was blocked.
    fn lan_leader_strike(&mut self, reason: &str) -> bool {
        let Some(l) = self.remote.as_ref().and_then(|r| r.leader.clone()) else {
            return false;
        };
        // Only a peer that claims to serve is lying when it turns us away.
        // One that advertises it is not serving (it follows someone else,
        // mid-election) is honest about it: plain backoff, no strike, or
        // honest devices that all chased the same bad advert would end up
        // ignoring each other.
        let claims_serving = self.lan_peers.iter().any(|p| p.device_id == l && p.serving);
        if !claims_serving {
            return false;
        }
        let (strikes, blocks) = self.lan_strikes.entry(l.clone()).or_insert((0, 0));
        *strikes += 1;
        if *strikes < LAN_STRIKES {
            return false;
        }
        *strikes = 0;
        let level = *blocks;
        *blocks = blocks.saturating_add(1);
        let block = (LAN_STRIKE_BLOCK_MS * 2f64.powi(level.min(16) as i32)).min(LAN_BLOCK_MS);
        self.log(
            "warn",
            format!(
                "LAN peer {l} {reason}; ignoring it for {:.0} s",
                block / 1000.0
            ),
        );
        let until = self.now_local_ms() + block;
        self.lan_blocklist.insert(l, until);
        self.drop_remote(Some(format!("LAN peer rejected: {reason}")));
        self.reevaluate();
        self.emit_connection();
        true
    }

    /// Frames from a LAN upstream before our Hello went out: only the
    /// leader's proof and challenge are acceptable. Returns `true` when the
    /// frame was consumed here.
    fn on_lan_handshake_frame(&mut self, msg: &Msg) -> bool {
        let Some(Remote {
            tier: ConnectionTier::Lan,
            leader,
            state: RemoteState::Handshaking { peer, .. },
            handshake: Some(h),
            ..
        }) = &self.remote
        else {
            return false;
        };
        if h.hello_sent {
            return false;
        }
        let peer = peer.clone();
        let leader = leader.clone();
        let mut h = h.clone();
        match msg {
            Msg::Proof { device_id, mac } => {
                let key = self.cfg.lan_key.clone();
                let ok = key
                    .as_ref()
                    .map(|k| {
                        !h.leader_proven
                            && Some(device_id) == leader.as_ref()
                            && auth::verify(k, Role::Leader, &h.our_nonce, device_id, mac)
                    })
                    .unwrap_or(false);
                if !ok {
                    self.reject_lan_leader("its proof did not check out");
                    return true;
                }
                h.leader_proven = true;
            }
            Msg::Challenge { nonce } => {
                if !auth::nonce_valid(nonce) || h.their_nonce.is_some() {
                    self.reject_lan_leader("bad challenge");
                    return true;
                }
                h.their_nonce = Some(nonce.clone());
            }
            Msg::Refuse {
                reason: RefuseReason::Unauthorised,
                ..
            } => {
                // Refused before we said anything but a nonce: it has no key
                // for this scope, or not ours.
                self.reject_lan_leader("it refused our challenge");
                return true;
            }
            Msg::Refuse { .. } | Msg::Bye { .. } => {
                // Not serving right now (mid-election, full): an ordinary
                // loss with backoff, unless it keeps happening.
                return self.lan_leader_strike("it keeps turning us away");
            }
            other => {
                self.reject_lan_leader(&format!("sent {} before proving itself", other.name()));
                return true;
            }
        }
        if h.leader_proven && !h.hello_sent {
            if let (Some(theirs), Some(key)) = (h.their_nonce.clone(), self.cfg.lan_key.clone()) {
                let me = self.cfg.device.id.clone();
                let mac = auth::prove(&key, Role::Joiner, &theirs, &me);
                self.out.push(Output::WireOut {
                    peer: peer.clone(),
                    msg: WireMessage::new(Msg::Proof { device_id: me, mac }),
                });
                let hello = self.hello_msg(None);
                self.out.push(Output::WireOut {
                    peer,
                    msg: WireMessage::new(hello),
                });
                h.hello_sent = true;
            }
        }
        if let Some(r) = &mut self.remote {
            r.handshake = Some(h);
        }
        true
    }

    fn on_disconnected(&mut self, peer: PeerId) {
        if self.inbound.contains(&peer) {
            let was_serving = self.is_serving();
            self.inbound.retain(|p| p != &peer);
            let outs = self.room.handle(RoomInput::Disconnected(peer));
            self.process_room_outputs(outs);
            if was_serving && !self.is_serving() {
                self.lost_members_at = Some(self.now_local_ms());
            }
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
            self.log(
                "debug",
                "refused an inbound peer: this device follows another room",
            );
            self.out.push(Output::WireOut {
                peer: peer.clone(),
                msg: WireMessage::new(Msg::Bye {
                    reason: "not serving".into(),
                }),
            });
            self.out.push(Output::Disconnect { peer });
            return;
        }
        let was_serving = self.is_serving();
        self.inbound.push(peer.clone());
        let outs = self.room.handle(RoomInput::Connected(peer));
        self.process_room_outputs(outs);
        if !was_serving && self.is_serving() {
            self.members_returned();
        }
    }

    fn on_wire_in(&mut self, peer: PeerId, msg: WireMessage) {
        if self.inbound.contains(&peer) {
            let was_serving = self.is_serving();
            let outs = self.room.handle(RoomInput::Message(peer, msg));
            self.process_room_outputs(outs);
            self.note_proven_members();
            let serving_now = self.is_serving();
            if serving_now && !was_serving {
                self.members_returned();
            }
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
        self.on_upstream_message(msg, false);
    }

    fn on_peer_discovered(&mut self, advert: PeerAdvert) {
        if advert.device_id == self.cfg.device.id
            || advert.scope_hash != scope_hash(&self.cfg.scope)
        {
            return;
        }
        if advert.protocol < PROTOCOL_MIN || self.cfg.lan_key.is_none() {
            return;
        }
        match self
            .lan_peers
            .iter_mut()
            .find(|p| p.device_id == advert.device_id)
        {
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

    fn on_upstream_message(&mut self, msg: WireMessage, from_loopback: bool) {
        if !from_loopback && self.on_lan_handshake_frame(&msg.msg) {
            return;
        }
        match msg.msg {
            Msg::Welcome {
                session_clock_ms,
                replica,
                members,
                ..
            } => self.on_welcome(session_clock_ms, replica, members, !from_loopback),
            Msg::Refuse { .. } | Msg::Bye { .. } if from_loopback => {
                self.log("warn", "own room refused the loopback; re-attaching");
                self.attach_loopback();
            }
            Msg::Refuse { reason, message } => {
                self.log("warn", format!("refused: {reason:?}: {message}"));
                if reason == RefuseReason::Unauthorised
                    && matches!(
                        self.remote,
                        Some(Remote {
                            tier: ConnectionTier::Lan,
                            ..
                        })
                    )
                {
                    // Our keys differ (a password changed somewhere): don't
                    // hammer it, and let the election look elsewhere.
                    self.reject_lan_leader("it rejected our proof");
                    return;
                }
                let attempt_bump = if reason == RefuseReason::Unauthorised {
                    4
                } else {
                    0
                };
                if let Some(r) = &mut self.remote {
                    if let RemoteState::Handshaking { attempt, .. } = &mut r.state {
                        *attempt += attempt_bump;
                    }
                }
                if let Some(peer) = self.remote.as_ref().and_then(|r| match &r.state {
                    RemoteState::Handshaking { peer, .. } | RemoteState::Attached { peer } => {
                        Some(peer.clone())
                    }
                    _ => None,
                }) {
                    self.out.push(Output::Disconnect { peer });
                }
                self.on_upstream_lost(Some(format!("{reason:?}: {message}")));
            }
            Msg::Bye { reason } => {
                self.log("info", format!("upstream said bye: {reason}"));
                if let Some(peer) = self.upstream_peer() {
                    self.out.push(Output::Disconnect { peer });
                }
                self.on_upstream_lost(Some(format!("bye: {reason}")));
            }
            Msg::OpAck { op_id, revision } => self.on_op_ack(op_id, revision),
            Msg::OpReject {
                op_id,
                current_revision,
                reason,
                document,
            } => self.on_op_reject(op_id, current_revision, reason, document),
            Msg::OpCommitted {
                op,
                revision,
                device_id,
                op_id,
                at,
                position_ms,
            } => self.on_op_committed(op, revision, device_id, op_id, at, position_ms),
            Msg::Document { document } => self.adopt(document, DocChange::Sync),
            Msg::TransportStamp {
                device_id,
                key,
                position,
                played_ms,
                started_at,
                scrobbled,
                ..
            } => {
                if device_id == self.cfg.device.id {
                    return;
                }
                self.remote_transport.position = position.clone();
                self.remote_transport.played_ms = played_ms;
                let device_name = self
                    .devices
                    .iter()
                    .find(|d| d.id == device_id)
                    .map(|d| d.name.clone())
                    .unwrap_or_else(|| device_id.clone());
                self.last_remote_stamp = Some(LastStamp {
                    device_id: device_id.clone(),
                    device_name,
                    key: key.clone(),
                    position: position.clone(),
                    played_ms,
                    started_at,
                    scrobbled,
                });
                if scrobbled {
                    if let Some(item) = self
                        .doc
                        .current
                        .clone()
                        .filter(|c| Some(c.key.as_str()) == key.as_deref())
                    {
                        self.learn_scrobbled_from_peer(&item.track_id, started_at, &device_id);
                    }
                }
                if let Some(r) = &mut self.resume {
                    if r.device_id == device_id {
                        // The device that was playing is back: no longer dormant.
                        self.resume = None;
                        self.out.push(Output::ResumeOffer(None));
                    }
                }
                let t = self.transport();
                self.out.push(Output::TransportChanged { transport: t });
            }
            Msg::TransportRequest { command, .. } => {
                if self.held.is_some() {
                    self.out.push(Output::TransportCommand(command));
                }
            }
            Msg::LeaseGranted { lease, ack_of } => {
                if from_loopback && lease.owner.is_none() && self.held.is_some() {
                    // Our own room lapsed us (a long suspend); it holds nothing
                    // authoritative, so just take it back.
                    self.claim(None, true);
                } else {
                    self.on_lease_granted(lease, ack_of, from_loopback)
                }
            }
            Msg::LeaseFenced { lease, .. } => {
                if from_loopback && self.remote.is_some() && self.held.is_some() {
                    // Detached: our own room only stands in for the remote one;
                    // it cannot fence a lease it never granted.
                    self.claim(None, true);
                } else {
                    self.on_lease_fenced(lease)
                }
            }
            Msg::Presence { devices } => {
                self.devices = devices;
                if let Some(p) = &mut self.picker {
                    let present: Vec<DeviceId> =
                        self.devices.iter().map(|d| d.id.clone()).collect();
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
                let now = (
                    self.offset.offset_ms().round(),
                    self.offset.round_trip_ms().map(|r| r.round()),
                );
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
            Msg::HandoffPrepare {
                from,
                key,
                track_id,
                position_ms,
                ..
            } => {
                if self.held.is_some() {
                    return; // we're the one playing; nothing to pre-buffer
                }
                let now = self.now_local_ms();
                self.prebuffer = Some(PreBuffer {
                    from,
                    key: key.clone(),
                    since: now,
                });
                self.out.push(Output::PreBuffer {
                    key,
                    track_id,
                    position_ms,
                });
            }
            Msg::HandoffReady {
                target, key, ready, ..
            } => {
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
            Msg::HandoffTakeover {
                from,
                target,
                key,
                track_id,
                position_ms,
                played_ms,
                started_at,
                scrobbled,
                lease,
                ..
            } => {
                if target != self.cfg.device.id {
                    return;
                }
                let Some(lease) = lease else { return };
                if scrobbled {
                    self.learn_scrobbled_from_peer(&track_id, started_at, &from);
                }
                self.prebuffer = None;
                self.lease = lease.clone();
                let now = self.now_local_ms();
                self.held = Some(HeldLease::new(lease.epoch, now));
                self.held_from_remote = !from_loopback;
                self.detached = false;
                self.stamp = Some(CurrentStamp {
                    key: Some(key.clone()),
                    track_id: Some(track_id.clone()),
                    position: PositionStamp {
                        position_ms,
                        taken_at: self.now_session_ms(),
                        rate: 1.0,
                        is_playing: true,
                    },
                    played_ms,
                    started_at,
                    scrobbled,
                });
                self.out.push(Output::TakeTransport {
                    key,
                    track_id,
                    position_ms,
                    played_ms,
                    started_at,
                    scrobbled,
                    play: true,
                });
                self.emit_lease();
                if self.resume.take().is_some() {
                    self.out.push(Output::ResumeOffer(None));
                }
            }
            Msg::ScrobbleDedupeAnswer {
                query_id,
                duplicate,
            } => {
                if let Some((track_id, started_at, _)) = self.scrobble_queries.remove(&query_id) {
                    let me = self.cfg.device.id.clone();
                    self.learn_scrobbled(&track_id, started_at, &me);
                    self.out.push(Output::Scrobble {
                        track_id,
                        started_at,
                        allowed: !duplicate,
                    });
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
            Msg::UndoEntryShared {
                entry,
                before_revision,
                before,
            } => {
                if entry.device_id != self.cfg.device.id {
                    self.out.push(Output::UndoEntryReceived {
                        entry,
                        before_revision,
                        before,
                    });
                }
            }
            Msg::Hello { .. }
            | Msg::Challenge { .. }
            | Msg::Proof { .. }
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

    fn on_welcome(
        &mut self,
        session_clock_ms: EpochMs,
        replica: Option<ReplicaState>,
        members: Vec<DeviceInfo>,
        remote: bool,
    ) {
        let now = self.now_local_ms();
        if remote {
            match &mut self.remote {
                Some(Remote { state, .. }) if matches!(state, RemoteState::Handshaking { .. }) => {
                    if let RemoteState::Handshaking { peer, .. } = state {
                        let peer = peer.clone();
                        *state = RemoteState::Attached { peer };
                    }
                }
                _ => return, // a stray welcome from a socket we no longer follow
            }
        }
        let via_coordinator = remote
            && matches!(
                &self.remote,
                Some(Remote {
                    tier: ConnectionTier::Coordinator,
                    ..
                })
            );
        // Followed a LAN leader while a coordinator is configured: it carries
        // the LAN session back; we adopt the coordinator's word when we get
        // there, filing and replaying nothing.
        let deferred = remote && via_coordinator && self.lan_deferred;
        if remote && !via_coordinator {
            if let Some(l) = self.remote.as_ref().and_then(|r| r.leader.clone()) {
                self.lan_strikes.remove(&l);
                // It proved the key, and so did everyone it admitted.
                self.lan_proven.insert(l);
            }
            let me = self.cfg.device.id.clone();
            self.lan_proven
                .extend(members.iter().map(|m| m.id.clone()).filter(|id| id != &me));
            if self.coordinator_target().is_some() {
                self.lan_deferred = true;
            }
        }
        if via_coordinator {
            self.lan_deferred = false;
        }
        self.upstream_ready = true;
        self.last_error = None;
        if via_coordinator {
            // Only the coordinator itself clears its failure count: a LAN
            // welcome must not make the next election try it again at once
            // (that flaps between the LAN room and a dead coordinator).
            self.coordinator_failures = 0;
        }
        if remote {
            // Provisional offset from the welcome; real pings replace it.
            self.offset.reset();
            self.provisional_offset = Some(session_clock_ms - now);
            self.pings_in_burst = 0;
            self.last_ping_at = 0.0;
            self.send_ping();
            // Say goodbye to anyone we were serving: they'll re-elect.
            let inbound: Vec<PeerId> = self.inbound.drain(..).collect();
            for p in inbound {
                self.out.push(Output::WireOut {
                    peer: p.clone(),
                    msg: WireMessage::new(Msg::Bye {
                        reason: "joined a coordinator".into(),
                    }),
                });
                self.out.push(Output::Disconnect { peer: p.clone() });
                let outs = self.room.handle(RoomInput::Disconnected(p));
                self.process_room_outputs(outs);
            }
        }
        self.devices = members;

        // -- reconcile --------------------------------------------------------
        let previous = self.doc.clone();
        let room_doc = replica.as_ref().map(|r| r.document.clone());
        let room_trivial = room_doc
            .as_ref()
            .map(|d| doc_is_trivial(d) && d.revision == 0)
            .unwrap_or(true);
        let room_rev = room_doc.as_ref().map(|d| d.revision).unwrap_or(0);
        self.pending.clear();
        if room_trivial {
            if !doc_is_trivial(&self.doc) {
                let doc = self.doc.clone();
                self.confirmed = self.doc.clone();
                let base = room_rev;
                self.submit_at(SessionOp::Replace { document: doc }, base);
            } else if let Some(rd) = &room_doc {
                self.adopt(rd.clone(), DocChange::Sync);
            }
            self.unsynced.clear();
            self.unsynced_overflow = false;
        } else {
            let rd = room_doc.clone().expect("non-trivial room has a document");
            let fast_forward = remote
                && self
                    .sync_base
                    .as_ref()
                    .map(|b| b.session_id == rd.session_id && b.revision == rd.revision)
                    .unwrap_or(false);
            let behind = remote
                && self.unsynced.is_empty()
                && !self.unsynced_overflow
                && rd.session_id == self.doc.session_id
                && self.doc.revision <= rd.revision;
            if !remote {
                // Our own room mirrors us; nothing to reconcile.
                self.confirmed = self.doc.clone();
            } else if deferred {
                self.adopt(rd.clone(), DocChange::Sync);
                self.unsynced.clear();
                self.unsynced_overflow = false;
            } else if fast_forward {
                if self.unsynced_overflow {
                    let doc = self.doc.clone();
                    self.confirmed = rd.clone();
                    self.doc = rd.clone();
                    self.submit_at(SessionOp::Replace { document: doc }, rd.revision);
                } else {
                    let ops: Vec<PendingOp> = std::mem::take(&mut self.unsynced);
                    self.confirmed = rd.clone();
                    self.doc = rd.clone();
                    for p in ops {
                        self.resubmit(p);
                    }
                }
                self.unsynced.clear();
                self.unsynced_overflow = false;
                if !same_session_state(&previous, &self.doc) {
                    let d = self.doc.clone();
                    self.out.push(Output::DocumentChanged {
                        document: d,
                        cause: DocChange::Sync,
                    });
                }
            } else {
                let file =
                    !behind && !doc_is_trivial(&previous) && !same_session_state(&previous, &rd);
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
        if self.records_sync_base() {
            self.sync_base = Some(SyncPoint {
                session_id: self.doc.session_id.clone(),
                revision: self.confirmed.revision,
            });
        }

        // -- lease and resume -------------------------------------------------
        if let Some(rep) = &replica {
            self.lease = rep.transport_lease.clone();
            self.remote_transport = rep.document.transport.clone();
            self.remote_transport.lease = self.lease.clone();
            if let Some(stamp) = &rep.last_stamp {
                if stamp.device_id != self.cfg.device.id {
                    self.last_remote_stamp = Some(stamp.clone());
                }
            }
            for r in &rep.scrobbles {
                self.learn_scrobbled(&r.track_id, r.started_at, &r.device_id);
            }
            let last_seen = rep
                .last_stamp
                .as_ref()
                .and_then(|s| rep.device_last_seen(&s.device_id));
            self.maybe_offer_resume(last_seen);
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
                let (theirs_plus_ours, ours_has_more) =
                    merge_saved_queues(&rep.saved_queues, &self.saved_queues);
                if ours_has_more {
                    self.upstream_send(Msg::SavedQueuesSync {
                        queues: theirs_plus_ours,
                    });
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
            self.claim(expected, !remote);
        }
        if remote {
            let unreported = std::mem::take(&mut self.unreported_scrobbles);
            for (track_id, started_at) in unreported {
                let device_id = self.cfg.device.id.clone();
                self.upstream_send(Msg::ScrobbleSubmitted {
                    track_id,
                    started_at,
                    device_id,
                });
            }
            self.announce_known_scrobbles();
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
        let at = self.now_session_ms();
        let position_ms = self.local_position();
        let ctx = op_context(&op_id, at, position_ms);
        match apply_op(
            self.reducer.as_ref(),
            &self.doc,
            &op,
            &ctx,
            base.saturating_add(1),
        ) {
            Ok(next) => {
                self.doc = next;
                let d = self.doc.clone();
                self.out.push(Output::DocumentChanged {
                    document: d,
                    cause: DocChange::Local,
                });
                self.send_op(
                    PendingOp {
                        op_id,
                        op,
                        at,
                        position_ms,
                    },
                    base,
                );
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

    /// Apply `op` to the live document at the next revision and send it.
    fn submit_at(&mut self, op: SessionOp, base: u32) {
        let op_id = self.next_op_id();
        let at = self.now_session_ms();
        let position_ms = self.local_position();
        self.resubmit_at(
            PendingOp {
                op_id,
                op,
                at,
                position_ms,
            },
            base,
        );
    }

    /// Re-send a pending op with its original id, time and position (keys
    /// and timestamps stay stable) against the current revision.
    fn resubmit(&mut self, p: PendingOp) {
        let base = self.doc.revision;
        self.resubmit_at(p, base);
    }

    fn resubmit_at(&mut self, p: PendingOp, base: u32) {
        let ctx = op_context(&p.op_id, p.at, p.position_ms);
        match apply_op(
            self.reducer.as_ref(),
            &self.doc,
            &p.op,
            &ctx,
            base.saturating_add(1),
        ) {
            Ok(next) => self.doc = next,
            Err(e) => {
                self.log("debug", format!("op not applicable: {e}"));
                return;
            }
        }
        self.send_op(p, base);
    }

    fn send_op(&mut self, p: PendingOp, base: u32) {
        let epoch = if p.op.is_owner_op() {
            self.held.as_ref().map(|h| h.epoch)
        } else {
            None
        };
        let device_id = self.cfg.device.id.clone();
        let msg = Msg::Op {
            base_revision: base,
            op: p.op.clone(),
            device_id,
            op_id: p.op_id.clone(),
            epoch,
            at: p.at,
            position_ms: p.position_ms,
        };
        self.pending.push_back(p);
        self.upstream_send(msg);
    }

    /// Forget the optimistic ops but keep them around for a late ack.
    fn abandon_pending(&mut self) {
        for p in self.pending.drain(..) {
            self.abandoned.push_back(p);
        }
        while self.abandoned.len() > 64 {
            self.abandoned.pop_front();
        }
    }

    fn on_op_ack(&mut self, op_id: String, revision: u32) {
        let Some(pos) = self.pending.iter().position(|p| p.op_id == op_id) else {
            // A rolled-back op the room accepted anyway: it is now part of the
            // session, so apply it exactly like anyone else's commit.
            if let Some(pos) = self.abandoned.iter().position(|p| p.op_id == op_id) {
                let p = self.abandoned.remove(pos).expect("position exists");
                let me = self.cfg.device.id.clone();
                self.on_op_committed(p.op, revision, me, p.op_id, p.at, p.position_ms);
            }
            return;
        };
        let p = self.pending.remove(pos).expect("position exists");
        let ctx = op_context(&p.op_id, p.at, p.position_ms);
        match apply_op(
            self.reducer.as_ref(),
            &self.confirmed,
            &p.op,
            &ctx,
            revision,
        ) {
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
        if self.records_sync_base() {
            self.sync_base = Some(SyncPoint {
                session_id: self.doc.session_id.clone(),
                revision,
            });
        } else if self.sync_base.is_some() && !self.lan_follower_of_stopgap() {
            if self.unsynced.len() >= UNSYNCED_CAP {
                self.unsynced_overflow = true;
                self.unsynced.clear();
            } else if !self.unsynced_overflow {
                self.unsynced.push(p);
            }
        }
        self.maybe_advertise(false);
    }

    fn on_op_reject(
        &mut self,
        op_id: String,
        current_revision: u32,
        reason: RejectReason,
        document: SessionDocument,
    ) {
        if !self.pending.iter().any(|p| p.op_id == op_id) {
            return;
        }
        let previous = self.doc.clone();
        self.log(
            "info",
            format!(
                "op {op_id} rejected: {reason:?} at room revision {current_revision} (ours {})",
                previous.revision
            ),
        );
        // Ops after the rejected one were chained on a base the room never
        // saw; the room may still accept them if the revisions line up.
        self.abandon_pending();
        let had_unsynced = !self.unsynced.is_empty() || self.unsynced_overflow;
        self.confirmed = document.clone();
        self.doc = document;
        self.doc.revision = current_revision;
        let d = self.doc.clone();
        self.out.push(Output::DocumentChanged {
            document: d,
            cause: DocChange::Rollback,
        });
        if self.records_sync_base() {
            self.sync_base = Some(SyncPoint {
                session_id: self.doc.session_id.clone(),
                revision: current_revision,
            });
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

    fn on_op_committed(
        &mut self,
        op: SessionOp,
        revision: u32,
        device_id: DeviceId,
        op_id: String,
        at: EpochMs,
        position_ms: Ms,
    ) {
        if device_id == self.cfg.device.id && self.pending.iter().any(|p| p.op_id == op_id) {
            return;
        }
        self.abandoned.retain(|p| p.op_id != op_id);
        if revision <= self.confirmed.revision {
            // Already incorporated (a full document arrived first). Revisions
            // are monotonic, so this can only be a duplicate or a late frame.
            return;
        }
        if revision > self.confirmed.revision.saturating_add(1) {
            self.log(
                "warn",
                format!(
                    "commit gap: at {} got {revision}; resyncing",
                    self.confirmed.revision
                ),
            );
            self.upstream_send(Msg::SyncRequest);
            return;
        }
        if !self.pending.is_empty() {
            // Ours are probably stale now; roll back so one press = one skip.
            // The room may still accept a chained one, hence "abandoned".
            self.abandon_pending();
            self.doc = self.confirmed.clone();
        }
        let at = if at > 0.0 { at } else { self.now_session_ms() };
        let ctx = op_context(&op_id, at, position_ms);
        match apply_op(self.reducer.as_ref(), &self.confirmed, &op, &ctx, revision) {
            Ok(next) => {
                self.confirmed = next;
                self.doc = self.confirmed.clone();
                let d = self.doc.clone();
                self.out.push(Output::DocumentChanged {
                    document: d,
                    cause: DocChange::Remote,
                });
                if self.records_sync_base() {
                    self.sync_base = Some(SyncPoint {
                        session_id: self.doc.session_id.clone(),
                        revision,
                    });
                } else if self.remote.is_none()
                    && self.sync_base.is_some()
                    && device_id != self.cfg.device.id
                {
                    // Someone else's op landed in our own room (we serve the
                    // LAN): our own op log no longer reproduces this state, so
                    // the next fast-forward pushes the whole document.
                    self.unsynced_overflow = true;
                    self.unsynced.clear();
                }
            }
            Err(e) => {
                self.log(
                    "warn",
                    format!("could not apply committed op {op_id}: {e}; resyncing"),
                );
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
        self.abandoned.clear();
        self.confirmed = document.clone();
        self.doc = document;
        let d = self.doc.clone();
        self.out
            .push(Output::DocumentChanged { document: d, cause });
        if self.records_sync_base() {
            self.sync_base = Some(SyncPoint {
                session_id: self.doc.session_id.clone(),
                revision: self.doc.revision,
            });
        }
        self.maybe_advertise(false);
    }

    // -- lease ----------------------------------------------------------------

    /// "X was playing Y · Resume here": dormant, only when nobody holds the
    /// lease and the last stamp came from someone else for what is current.
    fn maybe_offer_resume(&mut self, last_seen: Option<EpochMs>) {
        let live_owner = if self.lease.expires_at > self.now_session_ms() {
            self.lease.owner.clone()
        } else {
            None
        };
        if live_owner.is_some() || self.held.is_some() {
            return;
        }
        let (Some(stamp), Some(current)) =
            (self.last_remote_stamp.clone(), self.doc.current.clone())
        else {
            return;
        };
        if stamp.device_id == self.cfg.device.id
            || stamp.key.as_deref() != Some(current.key.as_str())
        {
            return;
        }
        let position_ms = resume_position(&stamp.position, self.now_session_ms());
        // Whatever played on since the stamp counts towards the scrobble too
        // (unless the gap was so long we snapped to the start).
        let played_ms = if position_ms >= stamp.position.position_ms {
            stamp
                .played_ms
                .saturating_add(position_ms - stamp.position.position_ms)
        } else {
            stamp.played_ms
        };
        let draft = ResumeOfferDraft {
            device_id: stamp.device_id.clone(),
            device_name: stamp.device_name.clone(),
            key: current.key.clone(),
            track_id: current.track_id.clone(),
            position_ms,
            played_ms,
            started_at: stamp.started_at,
            scrobbled: stamp.scrobbled,
            last_seen: last_seen.unwrap_or(stamp.position.taken_at),
        };
        if self.resume.as_ref() != Some(&draft) {
            self.resume = Some(draft.clone());
            self.out.push(Output::ResumeOffer(Some(draft)));
        }
    }

    /// Send a lease claim, stamping it so the answer can be credited.
    fn claim(&mut self, epoch_expected: Option<u32>, takeover: bool) {
        let now = self.now_local_ms();
        if let Some(h) = &mut self.held {
            h.sent(now);
        }
        self.upstream_send(Msg::LeaseClaim {
            epoch_expected,
            takeover,
            sent_at: now,
        });
    }

    fn on_lease_granted(
        &mut self,
        lease: TransportLease,
        ack_of: Option<EpochMs>,
        from_loopback: bool,
    ) {
        let now = self.now_local_ms();
        let mine = lease.owner.as_deref() == Some(self.cfg.device.id.as_str());
        self.lease = lease.clone();
        self.remote_transport.lease = lease.clone();
        if mine {
            self.held_from_remote = !from_loopback;
            match &mut self.held {
                Some(h) => {
                    if h.epoch != lease.epoch {
                        // A fresh grant (takeover, reclaim): the room vouched for us just now.
                        h.epoch = lease.epoch;
                        h.acked(ack_of.unwrap_or(now));
                    } else if let Some(t) = ack_of {
                        h.acked(t);
                    }
                }
                None => self.held = Some(HeldLease::new(lease.epoch, ack_of.unwrap_or(now))),
            }
            if self.is_connected() {
                // Only the remote room's word ends detachment; our own room
                // granting us the lease while cut off proves nothing.
                self.held_remote_epoch = Some(lease.epoch);
                self.detached = false;
            }
            if let Some(t) = self.pending_take.take() {
                self.stamp = Some(CurrentStamp {
                    key: Some(t.key.clone()),
                    track_id: Some(t.track_id.clone()),
                    position: PositionStamp {
                        position_ms: t.position_ms,
                        taken_at: self.now_session_ms(),
                        rate: 1.0,
                        is_playing: true,
                    },
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
        if lease.owner.is_none() && self.is_connected() {
            // The player went quiet and its lease lapsed: offer, never auto-resume.
            self.maybe_offer_resume(None);
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
        self.stamp = Some(CurrentStamp {
            key: key.clone(),
            track_id: track_id.clone(),
            position: p.clone(),
            played_ms,
            started_at,
            scrobbled,
        });
        let Some(h) = &self.held else {
            self.log(
                "debug",
                "stamp from a device that does not own transport ignored",
            );
            return;
        };
        let epoch = h.epoch;
        let device_id = self.cfg.device.id.clone();
        self.upstream_send(Msg::TransportStamp {
            device_id,
            key,
            position: p,
            played_ms,
            started_at,
            scrobbled,
            epoch,
        });
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
            self.out.push(Output::PickerChanged {
                open: false,
                targets: vec![],
            });
            return;
        }
        let Some(stamp) = self.stamp.clone() else {
            self.out.push(Output::PickerChanged {
                open: false,
                targets: vec![],
            });
            return;
        };
        let (Some(key), Some(track_id)) = (stamp.key.clone(), stamp.track_id.clone()) else {
            self.out.push(Output::PickerChanged {
                open: false,
                targets: vec![],
            });
            return;
        };
        let targets: Vec<(DeviceId, bool)> = self
            .devices
            .iter()
            .filter(|d| d.id != self.cfg.device.id)
            .take(self.cfg.prebuffer_fanout)
            .map(|d| (d.id.clone(), false))
            .collect();
        self.picker = Some(Picker {
            key,
            track_id,
            targets,
        });
        let from = self.cfg.device.id.clone();
        self.upstream_send(Msg::HandoffPickerOpen { from });
        self.reprepare_picker();
    }

    fn reprepare_picker(&mut self) {
        let Some(stamp) = self.stamp.clone() else {
            return;
        };
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
            self.out.push(Output::PickerChanged {
                open: false,
                targets: vec![],
            });
        }
    }

    fn handoff_to(&mut self, device_id: DeviceId) {
        let (Some(h), Some(stamp), Some(p)) =
            (self.held.clone(), self.stamp.clone(), self.picker.clone())
        else {
            self.log(
                "debug",
                "handoff needs an open picker and transport ownership",
            );
            return;
        };
        if !p.targets.iter().any(|(id, _)| id == &device_id)
            && !self.devices.iter().any(|d| d.id == device_id)
        {
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
        self.out.push(Output::PickerChanged {
            open: false,
            targets: vec![],
        });
        self.emit_lease();
    }

    fn on_prebuffer_result(&mut self, key: QueueKey, ready: bool) {
        let Some(p) = self.prebuffer.clone() else {
            return;
        };
        if p.key != key {
            return;
        }
        let target = self.cfg.device.id.clone();
        self.upstream_send(Msg::HandoffReady {
            from: p.from,
            target,
            key,
            ready,
        });
        if !ready {
            self.prebuffer = None;
        }
    }

    // -- scrobbling -------------------------------------------------------------

    fn on_scrobble_reached(&mut self, track_id: TrackId, started_at: EpochMs) {
        let same = |t: &TrackId, s: EpochMs| t == &track_id && (s - started_at).abs() < 1000.0;
        let me = self.cfg.device.id.clone();
        // What we know, and what our own room judged for other devices (a
        // LAN leader that has since moved on to the coordinator still knows
        // its members' scrobbles).
        if self.known_scrobbled.iter().any(|(t, s, _)| same(t, *s))
            || self
                .room
                .replica()
                .scrobbles
                .iter()
                .any(|r| r.device_id != me && same(&r.track_id, r.started_at))
        {
            // This play was already scrobbled (by us or by whoever handed it
            // over); nobody needs to be asked.
            self.out.push(Output::Scrobble {
                track_id,
                started_at,
                allowed: false,
            });
            return;
        }
        if self.upstream_authoritative() {
            self.query_scrobble(track_id, started_at);
        } else {
            let now = self.now_local_ms();
            self.deferred_scrobbles.push(DeferredScrobble {
                track_id,
                started_at,
                since: now,
            });
        }
    }

    fn query_scrobble(&mut self, track_id: TrackId, started_at: EpochMs) {
        let query_id = self.next_op_id();
        let now = self.now_local_ms();
        self.scrobble_queries
            .insert(query_id.clone(), (track_id.clone(), started_at, now));
        let device_id = self.cfg.device.id.clone();
        self.upstream_send(Msg::ScrobbleDedupeQuery {
            query_id,
            track_id,
            started_at,
            device_id,
        });
    }

    /// Re-ask queries a lossy link swallowed. Same id: a late first answer
    /// settles it and the second is ignored.
    fn retry_scrobble_queries(&mut self, now: EpochMs) {
        let due: Vec<(String, TrackId, EpochMs)> = self
            .scrobble_queries
            .iter()
            .filter(|(_, (_, _, sent))| now - sent >= SCROBBLE_QUERY_RETRY_MS)
            .map(|(id, (t, s, _))| (id.clone(), t.clone(), *s))
            .collect();
        for (query_id, track_id, started_at) in due {
            if let Some(e) = self.scrobble_queries.get_mut(&query_id) {
                e.2 = now;
            }
            let device_id = self.cfg.device.id.clone();
            self.upstream_send(Msg::ScrobbleDedupeQuery {
                query_id,
                track_id,
                started_at,
                device_id,
            });
        }
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
        if !self.started {
            // First tick: pick a tier (start the listener, connect the coordinator).
            self.started = true;
            self.reevaluate();
        }

        // Own room timers (lease lapse for inbound peers, member timeouts).
        let outs = self.room.handle(RoomInput::Tick);
        self.process_room_outputs(outs);

        // A LAN block ran out: the election (and with it who is authoritative
        // for scrobbles) changes, so act on it now rather than at the next
        // advert.
        let before = self.lan_blocklist.len();
        self.lan_blocklist.retain(|_, until| now < *until);
        if self.lan_blocklist.len() != before {
            self.reevaluate();
        }

        // Remote connection lifecycle.
        if let Some(r) = self.remote.clone() {
            match r.state {
                RemoteState::Connecting { since, .. } if now - since > CONNECT_TIMEOUT_MS => {
                    self.on_upstream_lost(Some("connect timed out".into()));
                }
                RemoteState::Handshaking { peer, since, .. }
                    if now - since > CONNECT_TIMEOUT_MS =>
                {
                    self.out.push(Output::Disconnect { peer });
                    self.on_upstream_lost(Some("handshake timed out".into()));
                }
                RemoteState::Backoff { until, attempt } if now >= until => {
                    if let Some(r) = &mut self.remote {
                        r.state = RemoteState::Connecting {
                            since: now,
                            attempt,
                        };
                        let candidates = r.candidates.clone();
                        self.out.push(Output::Connect { candidates });
                    }
                }
                RemoteState::Attached { peer } => {
                    if now - self.last_upstream_msg_at > self.cfg.upstream_idle_ms {
                        self.out.push(Output::Disconnect { peer });
                        self.on_upstream_lost(Some("upstream silent".into()));
                    } else {
                        let interval = if self.pings_in_burst < PING_BURST {
                            PING_BURST_INTERVAL_MS
                        } else {
                            PING_INTERVAL_MS
                        };
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
        if matches!(
            self.remote,
            Some(Remote {
                tier: ConnectionTier::Lan,
                ..
            })
        ) && self.coordinator_target().is_some()
            && now >= self.coordinator_retry_at
        {
            self.coordinator_failures = 0;
            self.reevaluate();
        }

        // Heartbeat while owning transport.
        if let Some(h) = self.held.clone() {
            if h.heartbeat_due(now) {
                if let Some(h) = &mut self.held {
                    h.sent(now);
                }
                self.upstream_send(Msg::LeaseHeartbeat {
                    epoch: h.epoch,
                    sent_at: now,
                });
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
                self.upstream_send(Msg::HandoffReady {
                    from: p.from,
                    target,
                    key: p.key,
                    ready: false,
                });
            }
        }

        if self.upstream_authoritative() {
            self.retry_scrobble_queries(now);
            // Deferred verdicts go to our own room a beat after it became
            // authoritative again, so returning members' announcements land first.
            if !self.deferred_scrobbles.is_empty() && self.flush_deferred_at.is_none() {
                self.flush_deferred_at = Some(now + 2_000.0);
            }
            if self.flush_deferred_at.map(|t| now >= t).unwrap_or(false) {
                self.flush_deferred_at = None;
                let deferred = std::mem::take(&mut self.deferred_scrobbles);
                for d in deferred {
                    self.query_scrobble(d.track_id, d.started_at);
                }
            }
        } else {
            self.flush_deferred_at = None;
        }

        // Deferred scrobbles past the grace period: local judgement.
        let grace = self.cfg.scrobble_grace_ms;
        let (expired, keep): (Vec<DeferredScrobble>, Vec<DeferredScrobble>) = self
            .deferred_scrobbles
            .drain(..)
            .partition(|d| now - d.since >= grace);
        self.deferred_scrobbles = keep;
        for d in expired {
            let me = self.cfg.device.id.clone();
            self.learn_scrobbled(&d.track_id, d.started_at, &me);
            self.unreported_scrobbles
                .push((d.track_id.clone(), d.started_at));
            self.out.push(Output::Scrobble {
                track_id: d.track_id,
                started_at: d.started_at,
                allowed: true,
            });
        }

        self.maybe_advertise(false);
    }
}

/// Persisted alongside the document by the actor.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PersistedConnectState {
    pub sync_base: Option<SyncBase>,
    /// Plays this device knows are scrobbled (its own and, when it led a LAN
    /// room, its members'), so a restarted leader still answers dedupe
    /// queries. Restore with [`Engine::restore_known_scrobbled`].
    #[serde(default)]
    pub known_scrobbled: Vec<KnownScrobble>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn persisted_connect_state_reads_old_rows_and_round_trips_known_scrobbles() {
        let old: PersistedConnectState = serde_json::from_str(r#"{"syncBase":null}"#).unwrap();
        assert!(
            old.known_scrobbled.is_empty(),
            "rows written before the field existed still load"
        );
        let state = PersistedConnectState {
            sync_base: None,
            known_scrobbled: vec![KnownScrobble {
                track_id: "t1".into(),
                started_at: 1_000.0,
                device_id: "d1".into(),
            }],
        };
        let back: PersistedConnectState =
            serde_json::from_str(&serde_json::to_string(&state).unwrap()).unwrap();
        assert_eq!(back, state);
    }
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

    fn key() -> LanKey {
        auth::test_key(b"engine-test-key")
    }

    fn credential() -> Credential {
        Credential {
            server_url: "https://music.example".into(),
            username: "u".into(),
            token: Some("tok".into()),
            salt: Some("salt".into()),
            api_key: None,
            client: "hocket".into(),
            api_version: "1.16.1".into(),
        }
    }

    fn engine(id: &str) -> (Engine, Arc<TestClock>) {
        let clock = Arc::new(TestClock(AtomicU64::new(10_000)));
        let mut cfg = EngineConfig::new(dev(id), "scope");
        cfg.lan_enabled = false;
        cfg.lan_key = Some(key());
        cfg.credential = Some(credential());
        let doc = crate::session::new_document("scope", format!("sid-{id}"), 0.0);
        let e = Engine::new(cfg, clock.clone(), RealReducer::shared(), doc, None);
        (e, clock)
    }

    fn advert(id: &str, rev: u32) -> PeerAdvert {
        PeerAdvert {
            device_id: id.into(),
            device_name: id.into(),
            platform: Platform::Android,
            scope_hash: scope_hash("scope"),
            port: 5,
            protocol: PROTOCOL,
            session_revision: rev,
            serving: false,
            addresses: vec!["10.0.0.2".into()],
        }
    }

    /// Play the leader side of the LAN handshake for an engine that just
    /// connected upstream on `peer`; returns the engine's outputs after its
    /// Hello went out.
    fn lan_leader_proves(e: &mut Engine, peer: &str, leader: &str, key: &LanKey) -> Vec<Output> {
        let outs = e.handle(Input::Connected {
            peer: peer.into(),
            url: "ws://10.0.0.2:5/".into(),
        });
        let w = wire_outs(&outs);
        assert_eq!(w.len(), 1, "only a challenge goes out first: {w:?}");
        let nonce = match &w[0].1 {
            Msg::Challenge { nonce } => nonce.clone(),
            other => panic!("expected a challenge, got {other:?}"),
        };
        let mac = auth::prove(key, Role::Leader, &nonce, leader);
        let outs = e.handle(Input::WireIn {
            peer: peer.into(),
            msg: WireMessage::new(Msg::Proof {
                device_id: leader.into(),
                mac,
            }),
        });
        assert!(wire_outs(&outs).is_empty());
        e.handle(Input::WireIn {
            peer: peer.into(),
            msg: WireMessage::new(Msg::Challenge {
                nonce: auth::new_nonce(),
            }),
        })
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
        assert!(matches!(
            outs[0],
            Output::DocumentChanged {
                cause: DocChange::Local,
                ..
            }
        ));
        assert_eq!(e.document().revision, 1);
        assert_eq!(e.document().current.as_ref().unwrap().track_id, "t1");
        assert_eq!(e.room().revision(), 1);
        assert!(e.pending.is_empty());
        assert!(wire_outs(&outs).is_empty());
        assert_eq!(e.connection_state().tier, ConnectionTier::Local);
        let outs = e.handle(Input::LocalOp {
            op: SessionOp::Next,
        });
        assert!(outs.iter().any(|o| matches!(o, Output::ReplicaChanged(_))));
        assert_eq!(e.document().current.as_ref().unwrap().track_id, "t2");
    }

    #[test]
    fn claim_and_release_transport_alone() {
        let (mut e, _) = engine("a");
        e.handle(Input::LocalOp { op: play_op() });
        let outs = e.handle(Input::ClaimTransport { takeover: false });
        assert!(outs.iter().any(|o| matches!(
            o,
            Output::LeaseChanged {
                owns: true,
                detached: false,
                ..
            }
        )));
        assert!(e.owns_transport());
        assert!(e.devices()[0].playing);
        let outs = e.handle(Input::ReleaseTransport);
        assert!(outs
            .iter()
            .any(|o| matches!(o, Output::LeaseChanged { owns: false, .. })));
        assert!(!e.owns_transport());
    }

    #[test]
    fn coordinator_handshake_and_fresh_room_push() {
        let (mut e, _) = engine("a");
        e.handle(Input::LocalOp { op: play_op() });
        let outs = e.handle(Input::SetCoordinatorUrl(Some("wss://c/".into())));
        assert!(outs
            .iter()
            .any(|o| matches!(o, Output::Connect { candidates } if candidates[0] == "wss://c/")));
        let outs = e.handle(Input::Connected {
            peer: "up".into(),
            url: "wss://c/".into(),
        });
        let w = wire_outs(&outs);
        assert!(
            matches!(&w[0], (p, Msg::Hello { session_revision: 1, credential: Some(_), .. }) if p == "up")
        );
        let outs = e.handle(Input::WireIn {
            peer: "up".into(),
            msg: WireMessage::new(Msg::Welcome {
                session_clock_ms: 500_000.0,
                accepted_protocol: PROTOCOL,
                replica: None,
                members: vec![],
                extra: Default::default(),
            }),
        });
        let w = wire_outs(&outs);
        assert!(w.iter().any(|(_, m)| matches!(m, Msg::ClockPing { .. })));
        assert!(w.iter().any(|(_, m)| matches!(
            m,
            Msg::Op {
                base_revision: 0,
                op: SessionOp::Replace { .. },
                ..
            }
        )));
        assert!(e.is_connected());
        assert_eq!(e.connection_state().tier, ConnectionTier::Coordinator);
        // provisional offset until the first pong
        assert_eq!(e.offset_ms(), 490_000.0);
        let outs = e.handle(Input::WireIn {
            peer: "up".into(),
            msg: WireMessage::new(Msg::ClockPong {
                t0: 10_000.0,
                t1: 500_040.0,
                t2: 500_040.0,
            }),
        });
        assert!(outs
            .iter()
            .any(|o| matches!(o, Output::ConnectionChanged(s) if s.round_trip_ms.is_some())));
        let outs = e.handle(Input::WireIn {
            peer: "up".into(),
            msg: WireMessage::new(Msg::OpAck {
                op_id: "a-2".into(),
                revision: 1,
            }),
        });
        assert!(
            outs.is_empty()
                || !outs
                    .iter()
                    .any(|o| matches!(o, Output::DocumentChanged { .. }))
        );
        assert_eq!(e.sync_base().unwrap().revision, 1);
        assert!(e.pending.is_empty());
    }

    fn attach(
        e: &mut Engine,
        replica: Option<ReplicaState>,
        members: Vec<DeviceInfo>,
    ) -> Vec<Output> {
        e.handle(Input::SetCoordinatorUrl(Some("wss://c/".into())));
        e.handle(Input::Connected {
            peer: "up".into(),
            url: "wss://c/".into(),
        });
        e.handle(Input::WireIn {
            peer: "up".into(),
            msg: WireMessage::new(Msg::Welcome {
                session_clock_ms: 10_000.0,
                accepted_protocol: PROTOCOL,
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
        assert!(!outs
            .iter()
            .any(|o| matches!(o, Output::FilePreviousStateAsSavedQueue { .. })));
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
        let (mut e, clock) = engine("a");
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
        assert!(outs
            .iter()
            .any(|o| matches!(o, Output::ConnectionChanged(s) if !s.connected)));
        e.handle(Input::LocalOp {
            op: SessionOp::Next,
        });
        e.handle(Input::LocalOp {
            op: SessionOp::Next,
        });
        assert_eq!(e.unsynced.len(), 2);
        assert_eq!(e.document().current.as_ref().unwrap().track_id, "t3");
        // reconnect: room still at revision 2 → replay
        clock.0.store(100_000, Ordering::SeqCst);
        e.handle(Input::Tick);
        e.handle(Input::Connected {
            peer: "up2".into(),
            url: "wss://c/".into(),
        });
        let outs = e.handle(Input::WireIn {
            peer: "up2".into(),
            msg: WireMessage::new(Msg::Welcome {
                session_clock_ms: 100_000.0,
                accepted_protocol: PROTOCOL,
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
        assert!(matches!(
            ops[0],
            Msg::Op {
                base_revision: 2,
                op: SessionOp::Next,
                ..
            }
        ));
        assert!(matches!(
            ops[1],
            Msg::Op {
                base_revision: 3,
                op: SessionOp::Next,
                ..
            }
        ));
        assert!(!outs
            .iter()
            .any(|o| matches!(o, Output::FilePreviousStateAsSavedQueue { .. })));
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
            msg: WireMessage::new(Msg::OpCommitted {
                op: play_op(),
                revision: 2,
                device_id: "b".into(),
                op_id: "b-1".into(),
                at: 1.0,
                position_ms: 0,
            }),
        });
        assert_eq!(e.document().current.as_ref().unwrap().track_id, "t1");
        e.handle(Input::LocalOp {
            op: SessionOp::Next,
        });
        assert_eq!(e.document().current.as_ref().unwrap().track_id, "t2");
        assert_eq!(e.pending.len(), 1);
        // b's next commits at revision 3 before ours is answered
        let outs = e.handle(Input::WireIn {
            peer: "up".into(),
            msg: WireMessage::new(Msg::OpCommitted {
                op: SessionOp::Next,
                revision: 3,
                device_id: "b".into(),
                op_id: "b-2".into(),
                at: 2.0,
                position_ms: 0,
            }),
        });
        assert!(outs.iter().any(|o| matches!(
            o,
            Output::DocumentChanged {
                cause: DocChange::Remote,
                ..
            }
        )));
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
        assert!(!outs
            .iter()
            .any(|o| matches!(o, Output::DocumentChanged { .. })));
    }

    #[test]
    fn fenced_owner_releases_and_files_diverged_state() {
        let (mut e, clock) = engine("a");
        let mut room_doc = crate::session::new_document("scope", "s".into(), 0.0);
        room_doc.revision = 1;
        attach(&mut e, Some(replica_with(room_doc.clone())), vec![]);
        e.handle(Input::LocalOp { op: play_op() });
        e.handle(Input::WireIn {
            peer: "up".into(),
            msg: WireMessage::new(Msg::OpAck {
                op_id: "a-2".into(),
                revision: 2,
            }),
        });
        e.handle(Input::ClaimTransport { takeover: false });
        let lease = TransportLease {
            owner: Some("a".into()),
            epoch: 1,
            expires_at: 30_000.0,
        };
        let outs = e.handle(Input::WireIn {
            peer: "up".into(),
            msg: WireMessage::new(Msg::LeaseGranted {
                lease,
                ack_of: None,
            }),
        });
        assert!(outs
            .iter()
            .any(|o| matches!(o, Output::LeaseChanged { owns: true, .. })));
        // network dies; we keep playing detached and make an offline change
        e.handle(Input::Disconnected { peer: "up".into() });
        assert!(e.owns_transport() && e.is_detached());
        e.handle(Input::LocalOp {
            op: SessionOp::Next,
        });
        // meanwhile the session moved on: room at revision 5, someone else owns
        clock.0.store(200_000, Ordering::SeqCst);
        e.handle(Input::Tick);
        e.handle(Input::Connected {
            peer: "up2".into(),
            url: "wss://c/".into(),
        });
        let mut moved = e.confirmed.clone();
        moved.revision = 5;
        moved.session_id = "s".into();
        moved.autoplay = true;
        let mut rep = replica_with(moved);
        rep.transport_lease = TransportLease {
            owner: Some("b".into()),
            epoch: 3,
            expires_at: 999_999.0,
        };
        let outs = e.handle(Input::WireIn {
            peer: "up2".into(),
            msg: WireMessage::new(Msg::Welcome {
                session_clock_ms: 200_000.0,
                accepted_protocol: PROTOCOL,
                replica: Some(rep),
                members: vec![dev("b")],
                extra: Default::default(),
            }),
        });
        assert!(outs
            .iter()
            .any(|o| matches!(o, Output::FilePreviousStateAsSavedQueue { .. })));
        assert!(wire_outs(&outs).iter().any(|(_, m)| matches!(
            m,
            Msg::LeaseClaim {
                epoch_expected: Some(1),
                takeover: false,
                ..
            }
        )));
        let outs = e.handle(Input::WireIn {
            peer: "up2".into(),
            msg: WireMessage::new(Msg::LeaseFenced {
                current_epoch: 3,
                lease: TransportLease {
                    owner: Some("b".into()),
                    epoch: 3,
                    expires_at: 999_999.0,
                },
            }),
        });
        assert!(outs.iter().any(|o| matches!(o, Output::ReleaseTransport)));
        assert!(outs
            .iter()
            .any(|o| matches!(o, Output::LeaseChanged { owns: false, .. })));
        assert!(!e.owns_transport());
    }

    #[test]
    fn handoff_source_and_target_sequence() {
        let (mut src, _) = engine("a");
        let mut room_doc = crate::session::new_document("scope", "s".into(), 0.0);
        room_doc.revision = 1;
        attach(
            &mut src,
            Some(replica_with(room_doc.clone())),
            vec![dev("b"), dev("c")],
        );
        src.handle(Input::ClaimTransport { takeover: false });
        src.handle(Input::WireIn {
            peer: "up".into(),
            msg: WireMessage::new(Msg::LeaseGranted {
                ack_of: None,
                lease: TransportLease {
                    owner: Some("a".into()),
                    epoch: 1,
                    expires_at: 99_999.0,
                },
            }),
        });
        src.handle(Input::LocalStamp {
            key: Some("k".into()),
            track_id: Some("t".into()),
            position: PositionStamp {
                position_ms: 1000,
                taken_at: 10_000.0,
                rate: 1.0,
                is_playing: true,
            },
            played_ms: 1000,
            started_at: 5.0,
            scrobbled: false,
        });
        let outs = src.handle(Input::OpenPicker);
        let w = wire_outs(&outs);
        assert!(w
            .iter()
            .any(|(_, m)| matches!(m, Msg::HandoffPickerOpen { .. })));
        assert_eq!(
            w.iter()
                .filter(|(_, m)| matches!(m, Msg::HandoffPrepare { .. }))
                .count(),
            2
        );
        assert!(outs.iter().any(
            |o| matches!(o, Output::PickerChanged { open: true, targets } if targets.len() == 2)
        ));
        let outs = src.handle(Input::WireIn {
            peer: "up".into(),
            msg: WireMessage::new(Msg::HandoffReady {
                from: "a".into(),
                target: "b".into(),
                key: "k".into(),
                ready: true,
            }),
        });
        assert!(outs.iter().any(|o| matches!(o, Output::PickerChanged { targets, .. } if targets.iter().any(|d| d.id == "b" && d.ready))));
        let outs = src.handle(Input::HandoffTo {
            device_id: "b".into(),
        });
        let w = wire_outs(&outs);
        assert!(w.iter().any(|(_, m)| matches!(m, Msg::HandoffTakeover { target, epoch: 1, played_ms, .. } if target == "b" && *played_ms >= 1000)));
        assert!(outs.iter().any(|o| matches!(o, Output::ReleaseTransport)));
        assert!(!src.owns_transport());

        // target side
        let (mut tgt, _) = engine("b");
        attach(&mut tgt, Some(replica_with(room_doc)), vec![dev("a")]);
        let outs = tgt.handle(Input::WireIn {
            peer: "up".into(),
            msg: WireMessage::new(Msg::HandoffPrepare {
                from: "a".into(),
                target: "b".into(),
                key: "k".into(),
                track_id: "t".into(),
                position_ms: 1000,
            }),
        });
        assert!(outs.iter().any(|o| matches!(
            o,
            Output::PreBuffer {
                position_ms: 1000,
                ..
            }
        )));
        let outs = tgt.handle(Input::PreBufferReady { key: "k".into() });
        assert!(wire_outs(&outs)
            .iter()
            .any(|(_, m)| matches!(m, Msg::HandoffReady { ready: true, .. })));
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
                lease: Some(TransportLease {
                    owner: Some("b".into()),
                    epoch: 2,
                    expires_at: 99_999.0,
                }),
            }),
        });
        assert!(outs.iter().any(|o| matches!(
            o,
            Output::TakeTransport {
                position_ms: 1500,
                played_ms: 1500,
                play: true,
                ..
            }
        )));
        assert!(tgt.owns_transport());
        assert_eq!(tgt.held_epoch(), Some(2));
    }

    #[test]
    fn prebuffer_times_out_and_is_discarded() {
        let (mut e, clock) = engine("b");
        e.cfg.upstream_idle_ms = 1e12;
        attach(&mut e, None, vec![dev("a")]);
        e.handle(Input::WireIn {
            peer: "up".into(),
            msg: WireMessage::new(Msg::HandoffPrepare {
                from: "a".into(),
                target: "b".into(),
                key: "k".into(),
                track_id: "t".into(),
                position_ms: 0,
            }),
        });
        clock.0.store(10_000 + 61_000, Ordering::SeqCst);
        let outs = e.handle(Input::Tick);
        assert!(outs.iter().any(|o| matches!(o, Output::DiscardPreBuffer)));
        assert!(wire_outs(&outs)
            .iter()
            .any(|(_, m)| matches!(m, Msg::HandoffReady { ready: false, .. })));
    }

    #[test]
    fn resume_offer_is_dormant_and_explicit() {
        let (mut e, _) = engine("a");
        let mut room_doc = crate::session::new_document("scope", "s".into(), 0.0);
        room_doc.revision = 1;
        room_doc.current = Some(crate::api::QueueItem {
            key: "k".into(),
            track_id: "t".into(),
            source: crate::api::QueueSource::Inserted,
            unavailable: false,
        });
        let mut rep = replica_with(room_doc);
        rep.last_stamp = Some(crate::connect::wire::LastStamp {
            device_id: "pixel".into(),
            device_name: "Pixel".into(),
            key: Some("k".into()),
            position: PositionStamp {
                position_ms: 30_000,
                taken_at: 9_000.0,
                rate: 1.0,
                is_playing: true,
            },
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
        assert!(!outs.iter().any(|o| matches!(
            o,
            Output::TakeTransport { .. } | Output::LeaseChanged { owns: true, .. }
        )));
        let outs = e.handle(Input::ResumeHere);
        assert!(wire_outs(&outs)
            .iter()
            .any(|(_, m)| matches!(m, Msg::LeaseClaim { takeover: true, .. })));
        let outs = e.handle(Input::WireIn {
            peer: "up".into(),
            msg: WireMessage::new(Msg::LeaseGranted {
                ack_of: None,
                lease: TransportLease {
                    owner: Some("a".into()),
                    epoch: 4,
                    expires_at: 99_999.0,
                },
            }),
        });
        assert!(outs.iter().any(|o| matches!(
            o,
            Output::TakeTransport {
                position_ms: 31_000,
                played_ms: 31_000,
                ..
            }
        )));
        assert!(outs.iter().any(|o| matches!(o, Output::ResumeOffer(None))));
        assert!(e.resume_offer().is_none());
    }

    #[test]
    fn scrobble_verdicts_follow_the_room_and_defer_when_cut_off() {
        let (mut e, clock) = engine("a");
        // alone: own room answers immediately
        let outs = e.handle(Input::ScrobbleReached {
            track_id: "t".into(),
            started_at: 1.0,
        });
        assert!(outs
            .iter()
            .any(|o| matches!(o, Output::Scrobble { allowed: true, .. })));
        let outs = e.handle(Input::ScrobbleReached {
            track_id: "t".into(),
            started_at: 1.0,
        });
        assert!(outs
            .iter()
            .any(|o| matches!(o, Output::Scrobble { allowed: false, .. }))); // already scrobbled here
        attach(&mut e, None, vec![]);
        let outs = e.handle(Input::ScrobbleReached {
            track_id: "t2".into(),
            started_at: 2.0,
        });
        assert!(wire_outs(&outs)
            .iter()
            .any(|(_, m)| matches!(m, Msg::ScrobbleDedupeQuery { .. })));
        let qid = wire_outs(&outs)
            .iter()
            .find_map(|(_, m)| match m {
                Msg::ScrobbleDedupeQuery { query_id, .. } => Some(query_id.clone()),
                _ => None,
            })
            .unwrap();
        let outs = e.handle(Input::WireIn {
            peer: "up".into(),
            msg: WireMessage::new(Msg::ScrobbleDedupeAnswer {
                query_id: qid,
                duplicate: true,
            }),
        });
        assert!(outs
            .iter()
            .any(|o| matches!(o, Output::Scrobble { allowed: false, .. })));
        // cut off: defer, then local judgement after the grace period
        e.handle(Input::Disconnected { peer: "up".into() });
        let outs = e.handle(Input::ScrobbleReached {
            track_id: "t3".into(),
            started_at: 3.0,
        });
        assert!(!outs.iter().any(|o| matches!(o, Output::Scrobble { .. })));
        clock.0.store(10_000 + 700_000, Ordering::SeqCst);
        let outs = e.handle(Input::Tick);
        assert!(outs
            .iter()
            .any(|o| matches!(o, Output::Scrobble { allowed: true, .. })));
    }

    #[test]
    fn reconnect_backs_off_and_retries() {
        let (mut e, clock) = engine("a");
        e.handle(Input::SetCoordinatorUrl(Some("wss://c/".into())));
        let outs = e.handle(Input::ConnectFailed {
            error: "refused".into(),
        });
        assert!(outs.iter().any(
            |o| matches!(o, Output::ConnectionChanged(s) if s.error.as_deref() == Some("refused"))
        ));
        let outs = e.handle(Input::Tick);
        assert!(!outs.iter().any(|o| matches!(o, Output::Connect { .. })));
        clock.0.store(10_000 + 1_100, Ordering::SeqCst);
        let outs = e.handle(Input::Tick);
        assert!(outs.iter().any(|o| matches!(o, Output::Connect { .. })));
        e.handle(Input::ConnectFailed {
            error: "refused".into(),
        });
        clock.0.store(10_000 + 1_100 + 1_500, Ordering::SeqCst);
        assert!(!e
            .handle(Input::Tick)
            .iter()
            .any(|o| matches!(o, Output::Connect { .. })));
        clock.0.store(10_000 + 1_100 + 2_100, Ordering::SeqCst);
        assert!(e
            .handle(Input::Tick)
            .iter()
            .any(|o| matches!(o, Output::Connect { .. })));
    }

    #[test]
    fn heartbeats_and_detachment() {
        let (mut e, clock) = engine("a");
        attach(&mut e, None, vec![]);
        e.handle(Input::ClaimTransport { takeover: false });
        e.handle(Input::WireIn {
            peer: "up".into(),
            msg: WireMessage::new(Msg::LeaseGranted {
                ack_of: None,
                lease: TransportLease {
                    owner: Some("a".into()),
                    epoch: 1,
                    expires_at: 99_999.0,
                },
            }),
        });
        clock.0.store(15_100, Ordering::SeqCst);
        let outs = e.handle(Input::Tick);
        assert!(wire_outs(&outs)
            .iter()
            .any(|(_, m)| matches!(m, Msg::LeaseHeartbeat { epoch: 1, .. })));
        // no acks: after 20 s we're detached but still playing
        clock.0.store(29_000, Ordering::SeqCst);
        let outs = e.handle(Input::Tick);
        assert!(outs.iter().any(|o| matches!(
            o,
            Output::LeaseChanged {
                owns: true,
                detached: true,
                ..
            }
        )));
        assert!(e.owns_transport());
    }

    #[test]
    fn lan_election_picks_leader_and_connects_or_serves() {
        let (mut e, _) = engine("m");
        e.handle(Input::SetLanDiscovery(true));
        // a peer with a higher revision wins: we connect to it
        let outs = e.handle(Input::PeerDiscovered(advert("z", 9)));
        assert!(outs.iter().any(
            |o| matches!(o, Output::Connect { candidates } if candidates[0] == "ws://10.0.0.2:5/")
        ));
        assert_eq!(e.connection_state().tier, ConnectionTier::Lan);
        // it disappears: we're alone again
        let outs = e.handle(Input::PeerLost {
            device_id: "z".into(),
        });
        assert!(outs
            .iter()
            .any(|o| matches!(o, Output::ConnectionChanged(s) if s.tier == ConnectionTier::Local)));
        // a lower-id peer with the same revision (0) wins over us ("m")
        e.handle(Input::PeerDiscovered(advert("a", 0)));
        assert!(matches!(
            e.remote,
            Some(Remote {
                tier: ConnectionTier::Lan,
                ..
            })
        ));
        // a higher-id peer with the same revision: we win and serve
        e.handle(Input::PeerLost {
            device_id: "a".into(),
        });
        let outs = e.handle(Input::PeerDiscovered(advert("q", 0)));
        assert!(!outs.iter().any(|o| matches!(o, Output::Connect { .. })));
        assert!(e.remote.is_none());
        // other-scope peers are ignored
        let mut foreign = advert("b", 99);
        foreign.scope_hash = "nope".into();
        e.handle(Input::PeerDiscovered(foreign));
        assert!(e.remote.is_none());
        // inbound peer joins our room: challenge, proof, then Hello
        e.handle(Input::ListenerStarted { port: 7 });
        e.handle(Input::PeerConnected { peer: "in1".into() });
        let nonce = auth::new_nonce();
        let outs = e.handle(Input::WireIn {
            peer: "in1".into(),
            msg: WireMessage::new(Msg::Challenge {
                nonce: nonce.clone(),
            }),
        });
        let w = wire_outs(&outs);
        assert!(
            matches!(&w[0], (p, Msg::Proof { device_id, mac }) if p == "in1" && device_id == "m" && auth::verify(&key(), Role::Leader, &nonce, "m", mac))
        );
        let their_nonce = match &w[1].1 {
            Msg::Challenge { nonce } => nonce.clone(),
            other => panic!("expected a challenge, got {other:?}"),
        };
        e.handle(Input::WireIn {
            peer: "in1".into(),
            msg: WireMessage::new(Msg::Proof {
                device_id: "q".into(),
                mac: auth::prove(&key(), Role::Joiner, &their_nonce, "q"),
            }),
        });
        let outs = e.handle(Input::WireIn {
            peer: "in1".into(),
            msg: WireMessage::new(Msg::Hello {
                device: dev("q"),
                protocol_min: PROTOCOL_MIN,
                protocol_max: PROTOCOL,
                scope: "scope".into(),
                credential: None,
                session_id: None,
                session_revision: 0,
                held_epoch: None,
                extra: Default::default(),
            }),
        });
        assert!(wire_outs(&outs)
            .iter()
            .any(|(p, m)| p == "in1" && matches!(m, Msg::Welcome { .. })));
        assert!(e.is_serving());
        assert_eq!(e.connection_state().tier, ConnectionTier::Lan);
        // the record went out as soon as the socket made us a server
        assert!(matches!(e.advert(), Some(a) if a.serving && a.port == 7));
        assert_eq!(e.last_advert.as_ref().map(|a| a.serving), Some(true));
        // a peer that skips the proof is refused
        e.handle(Input::PeerConnected { peer: "in2".into() });
        let outs = e.handle(Input::WireIn {
            peer: "in2".into(),
            msg: WireMessage::new(Msg::Hello {
                device: dev("r"),
                protocol_min: PROTOCOL_MIN,
                protocol_max: PROTOCOL,
                scope: "scope".into(),
                credential: None,
                session_id: None,
                session_revision: 0,
                held_epoch: None,
                extra: Default::default(),
            }),
        });
        assert!(wire_outs(&outs).iter().any(|(p, m)| p == "in2"
            && matches!(
                m,
                Msg::Refuse {
                    reason: RefuseReason::Unauthorised,
                    ..
                }
            )));
        assert!(outs
            .iter()
            .any(|o| matches!(o, Output::Disconnect { peer } if peer == "in2")));
    }

    #[test]
    fn lan_hello_never_carries_the_credential_and_waits_for_the_leaders_proof() {
        let (mut e, _) = engine("m");
        e.handle(Input::LocalOp { op: play_op() });
        e.handle(Input::SetLanDiscovery(true));
        e.handle(Input::PeerDiscovered(advert("z", 9)));
        let outs = lan_leader_proves(&mut e, "up", "z", &key());
        let w = wire_outs(&outs);
        assert!(matches!(&w[0], (_, Msg::Proof { device_id, .. }) if device_id == "m"));
        assert!(matches!(
            &w[1],
            (
                _,
                Msg::Hello {
                    credential: None,
                    session_revision: 1,
                    ..
                }
            )
        ));
        assert_eq!(w.len(), 2);
        // and the welcome attaches us as usual
        let outs = e.handle(Input::WireIn {
            peer: "up".into(),
            msg: WireMessage::new(Msg::Welcome {
                session_clock_ms: 10_000.0,
                accepted_protocol: PROTOCOL,
                replica: None,
                members: vec![],
                extra: Default::default(),
            }),
        });
        assert!(e.is_connected());
        assert_eq!(e.lan_leader().map(String::as_str), Some("z"));
        assert!(wire_outs(&outs).iter().any(|(_, m)| matches!(
            m,
            Msg::Op {
                op: SessionOp::Replace { .. },
                ..
            }
        )));
    }

    #[test]
    fn rogue_lan_leader_is_blocklisted_and_learns_nothing() {
        let (mut e, _) = engine("m");
        e.handle(Input::LocalOp { op: play_op() });
        e.handle(Input::SetLanDiscovery(true));
        // two peers: the rogue wins the election with a huge revision
        e.handle(Input::PeerDiscovered(advert("honest", 3)));
        let outs = e.handle(Input::PeerDiscovered(advert("rogue", u32::MAX)));
        assert!(outs.iter().any(|o| matches!(o, Output::Connect { .. })));
        assert_eq!(e.remote.as_ref().unwrap().leader.as_deref(), Some("rogue"));
        let outs = e.handle(Input::Connected {
            peer: "up".into(),
            url: "ws://10.0.0.2:5/".into(),
        });
        let nonce = match &wire_outs(&outs)[0].1 {
            Msg::Challenge { nonce } => nonce.clone(),
            other => panic!("{other:?}"),
        };
        // the rogue skips the proof and sends a Welcome to fish for the document
        let outs = e.handle(Input::WireIn {
            peer: "up".into(),
            msg: WireMessage::new(Msg::Welcome {
                session_clock_ms: 0.0,
                accepted_protocol: PROTOCOL,
                replica: None,
                members: vec![],
                extra: Default::default(),
            }),
        });
        assert!(!wire_outs(&outs)
            .iter()
            .any(|(p, m)| p == "up" && !matches!(m, Msg::Bye { .. })));
        assert!(outs
            .iter()
            .any(|o| matches!(o, Output::Disconnect { peer } if peer == "up")));
        assert!(!e.is_connected());
        // the election moved on to the honest peer
        assert_eq!(e.remote.as_ref().unwrap().leader.as_deref(), Some("honest"));
        assert!(outs.iter().any(
            |o| matches!(o, Output::Connect { candidates } if candidates[0] == "ws://10.0.0.2:5/")
        ));
        // a rogue with the wrong key is rejected at the proof, likewise
        e.handle(Input::PeerLost {
            device_id: "rogue".into(),
        });
        e.handle(Input::PeerDiscovered(advert("rogue2", u32::MAX)));
        let outs = e.handle(Input::Connected {
            peer: "up2".into(),
            url: "ws://10.0.0.2:5/".into(),
        });
        let nonce2 = match &wire_outs(&outs)[0].1 {
            Msg::Challenge { nonce } => nonce.clone(),
            other => panic!("{other:?}"),
        };
        let wrong = auth::test_key(b"wrong");
        let outs = e.handle(Input::WireIn {
            peer: "up2".into(),
            msg: WireMessage::new(Msg::Proof {
                device_id: "rogue2".into(),
                mac: auth::prove(&wrong, Role::Leader, &nonce2, "rogue2"),
            }),
        });
        assert!(!wire_outs(&outs)
            .iter()
            .any(|(_, m)| matches!(m, Msg::Hello { .. } | Msg::Proof { .. })));
        assert!(e.lan_blocklist.contains_key("rogue2"));
        assert_eq!(e.remote.as_ref().unwrap().leader.as_deref(), Some("honest"));
        let _ = nonce;
        // a proof for the right key but another identity (a relayed honest
        // device's) is no better: it must name the leader we elected
        e.handle(Input::PeerLost {
            device_id: "rogue2".into(),
        });
        e.handle(Input::PeerDiscovered(advert("rogue3", u32::MAX)));
        assert_eq!(e.remote.as_ref().unwrap().leader.as_deref(), Some("rogue3"));
        let outs = e.handle(Input::Connected {
            peer: "up3".into(),
            url: "ws://10.0.0.2:5/".into(),
        });
        let nonce3 = match &wire_outs(&outs)[0].1 {
            Msg::Challenge { nonce } => nonce.clone(),
            other => panic!("{other:?}"),
        };
        let outs = e.handle(Input::WireIn {
            peer: "up3".into(),
            msg: WireMessage::new(Msg::Proof {
                device_id: "honest".into(),
                mac: auth::prove(&key(), Role::Leader, &nonce3, "honest"),
            }),
        });
        assert!(!wire_outs(&outs)
            .iter()
            .any(|(_, m)| matches!(m, Msg::Hello { .. } | Msg::Proof { .. })));
        assert!(e.lan_blocklist.contains_key("rogue3"));
        assert_eq!(e.remote.as_ref().unwrap().leader.as_deref(), Some("honest"));
    }

    /// Knock on the elected LAN leader `n` times and get a `Bye` each time
    /// before any proof; returns the last outputs.
    fn turned_away(e: &mut Engine, clock: &TestClock, n: usize) -> Vec<Output> {
        let mut outs = vec![];
        for i in 0..n {
            let peer = format!("knock-{i}-{}", clock.0.load(Ordering::SeqCst));
            let o = e.handle(Input::Connected {
                peer: peer.clone(),
                url: "ws://10.0.0.2:5/".into(),
            });
            assert!(matches!(&wire_outs(&o)[0].1, Msg::Challenge { .. }));
            outs = e.handle(Input::WireIn {
                peer,
                msg: WireMessage::new(Msg::Bye {
                    reason: "not serving".into(),
                }),
            });
            if i + 1 < n {
                // wait out the backoff: the engine knocks again
                clock.0.fetch_add(40_000, Ordering::SeqCst);
                let o = e.handle(Input::Tick);
                assert!(o.iter().any(|o| matches!(o, Output::Connect { .. })));
            }
        }
        outs
    }

    #[test]
    fn lan_turn_aways_block_only_a_peer_that_claims_to_serve() {
        // an honest peer that says it is not serving (it follows someone
        // else) and turns us away is retried with backoff, never blocked
        let (mut e, clock) = engine("m");
        e.handle(Input::SetLanDiscovery(true));
        e.handle(Input::PeerDiscovered(advert("z", 9)));
        turned_away(&mut e, &clock, LAN_STRIKES as usize + 2);
        assert!(e.lan_blocklist.is_empty());
        assert_eq!(e.remote.as_ref().unwrap().leader.as_deref(), Some("z"));

        // one that claims to serve and still turns us away is blocked after
        // a few tries, and tried again (once) when the block runs out
        let (mut e, clock) = engine("m");
        e.handle(Input::SetLanDiscovery(true));
        let mut liar = advert("s", 9);
        liar.serving = true;
        e.handle(Input::PeerDiscovered(liar));
        let outs = turned_away(&mut e, &clock, LAN_STRIKES as usize);
        assert!(e.lan_blocklist.contains_key("s"));
        assert!(e.remote.is_none(), "alone again: we are our own room");
        assert!(!outs.iter().any(|o| matches!(o, Output::Connect { .. })));
        clock
            .0
            .fetch_add(LAN_STRIKE_BLOCK_MS as u64 - 1_000, Ordering::SeqCst);
        let outs = e.handle(Input::Tick);
        assert!(!outs.iter().any(|o| matches!(o, Output::Connect { .. })));
        clock.0.fetch_add(2_000, Ordering::SeqCst);
        let outs = e.handle(Input::Tick);
        assert!(e.lan_blocklist.is_empty());
        assert!(outs.iter().any(
            |o| matches!(o, Output::Connect { candidates } if candidates[0] == "ws://10.0.0.2:5/")
        ));
        // the next block is longer
        turned_away(&mut e, &clock, LAN_STRIKES as usize);
        let until = e.lan_blocklist["s"];
        assert!(until - e.now_local_ms() > LAN_STRIKE_BLOCK_MS * 1.5);
    }

    #[test]
    fn without_a_lan_key_the_lan_is_closed() {
        let (mut e, _) = engine("m");
        e.handle(Input::SetLanKey(None));
        e.handle(Input::SetLanDiscovery(true));
        let outs = e.handle(Input::PeerDiscovered(advert("z", 9)));
        assert!(!outs.iter().any(|o| matches!(o, Output::Connect { .. })));
        assert!(e.remote.is_none());
        e.handle(Input::ListenerStarted { port: 7 });
        assert!(e.advert().is_none());
        e.handle(Input::PeerConnected { peer: "in1".into() });
        let outs = e.handle(Input::WireIn {
            peer: "in1".into(),
            msg: WireMessage::new(Msg::Challenge {
                nonce: auth::new_nonce(),
            }),
        });
        assert!(wire_outs(&outs)
            .iter()
            .any(|(_, m)| matches!(m, Msg::Refuse { .. })));
    }

    #[test]
    fn credential_goes_only_to_a_coordinator_over_tls_or_loopback() {
        let hello_credential = |url: &str, allow: bool| -> Option<Option<Credential>> {
            let (mut e, _) = engine("a");
            e.cfg.allow_insecure_coordinator = allow;
            let outs = e.handle(Input::SetCoordinatorUrl(Some(url.into())));
            if !outs.iter().any(|o| matches!(o, Output::Connect { .. })) {
                return None;
            }
            let outs = e.handle(Input::Connected {
                peer: "up".into(),
                url: url.into(),
            });
            wire_outs(&outs).into_iter().find_map(|(_, m)| match m {
                Msg::Hello { credential, .. } => Some(credential),
                _ => None,
            })
        };
        assert!(matches!(hello_credential("wss://c/", false), Some(Some(_))));
        assert!(matches!(
            hello_credential("ws://127.0.0.1:7373/", false),
            Some(Some(_))
        ));
        // a plaintext coordinator on a private host needs the explicit override
        assert!(hello_credential("ws://192.168.1.2:7373/", false).is_none());
        assert!(matches!(
            hello_credential("ws://192.168.1.2:7373/", true),
            Some(Some(_))
        ));
        // and a plaintext coordinator on the internet is never used
        assert!(hello_credential("ws://c.example/", true).is_none());
        let (mut e, _) = engine("a");
        let outs = e.handle(Input::SetCoordinatorUrl(Some("ws://c.example/".into())));
        assert!(outs
            .iter()
            .any(|o| matches!(o, Output::Log { level: "warn", .. })));
        assert!(e.connection_state().error.is_some());
    }

    #[test]
    fn lan_follower_keeps_its_coordinator_sync_point_and_files_nothing() {
        let (mut e, clock) = engine("b");
        e.cfg.lan_enabled = true;
        e.cfg.upstream_idle_ms = 1e12;
        // synced with the coordinator at revision 2 of session "s"
        let mut room_doc = crate::session::new_document("scope", "s".into(), 0.0);
        room_doc.revision = 2;
        attach(&mut e, Some(replica_with(room_doc.clone())), vec![]);
        assert_eq!(e.sync_base().unwrap().revision, 2);
        // the coordinator goes away for good (two failed attempts) and a LAN leader appears
        e.handle(Input::Disconnected { peer: "up".into() });
        clock.0.store(20_000, Ordering::SeqCst);
        e.handle(Input::Tick);
        e.handle(Input::ConnectFailed {
            error: "down".into(),
        });
        clock.0.store(40_000, Ordering::SeqCst);
        e.handle(Input::Tick);
        e.handle(Input::ConnectFailed {
            error: "down".into(),
        });
        let outs = e.handle(Input::PeerDiscovered(advert("a", 2)));
        assert!(outs.iter().any(
            |o| matches!(o, Output::Connect { candidates } if candidates[0] == "ws://10.0.0.2:5/")
        ));
        lan_leader_proves(&mut e, "lan", "a", &key());
        e.handle(Input::WireIn {
            peer: "lan".into(),
            msg: WireMessage::new(Msg::Welcome {
                session_clock_ms: 40_000.0,
                accepted_protocol: PROTOCOL,
                replica: Some(replica_with(room_doc.clone())),
                members: vec![dev("a")],
                extra: Default::default(),
            }),
        });
        assert!(e.is_connected());
        assert_eq!(e.lan_leader().map(String::as_str), Some("a"));
        // the LAN session moves on: the leader's op, and one of ours
        e.handle(Input::WireIn {
            peer: "lan".into(),
            msg: WireMessage::new(Msg::OpCommitted {
                op: play_op(),
                revision: 3,
                device_id: "a".into(),
                op_id: "a-1".into(),
                at: 1.0,
                position_ms: 0,
            }),
        });
        e.handle(Input::LocalOp {
            op: SessionOp::Next,
        });
        e.handle(Input::WireIn {
            peer: "lan".into(),
            msg: WireMessage::new(Msg::OpAck {
                op_id: "b-2".into(),
                revision: 4,
            }),
        });
        assert_eq!(e.document().revision, 4);
        // the coordinator point is untouched and nothing is queued for replay
        assert_eq!(e.sync_base().unwrap().revision, 2);
        assert!(!e.has_unsynced());
        // the coordinator is back, still at revision 2: adopt it, file nothing;
        // the leader brings the LAN state
        clock.0.store(200_000, Ordering::SeqCst);
        let outs = e.handle(Input::Tick);
        assert!(outs
            .iter()
            .any(|o| matches!(o, Output::Connect { candidates } if candidates[0] == "wss://c/")));
        e.handle(Input::Connected {
            peer: "up2".into(),
            url: "wss://c/".into(),
        });
        let outs = e.handle(Input::WireIn {
            peer: "up2".into(),
            msg: WireMessage::new(Msg::Welcome {
                session_clock_ms: 200_000.0,
                accepted_protocol: PROTOCOL,
                replica: Some(replica_with(room_doc)),
                members: vec![],
                extra: Default::default(),
            }),
        });
        assert!(!outs
            .iter()
            .any(|o| matches!(o, Output::FilePreviousStateAsSavedQueue { .. })));
        assert!(!wire_outs(&outs)
            .iter()
            .any(|(_, m)| matches!(m, Msg::Op { .. })));
        assert_eq!(e.document().revision, 2);
        assert_eq!(e.connection_state().tier, ConnectionTier::Coordinator);
    }

    #[test]
    fn lan_leader_pushes_the_whole_document_when_members_contributed() {
        let (mut e, clock) = engine("a");
        e.cfg.lan_enabled = true;
        let mut room_doc = crate::session::new_document("scope", "s".into(), 0.0);
        room_doc.revision = 2;
        attach(&mut e, Some(replica_with(room_doc.clone())), vec![]);
        // the coordinator goes away for good (two failed attempts); a peer
        // behind us appears, so the election makes us the LAN room
        e.handle(Input::Disconnected { peer: "up".into() });
        clock.0.store(20_000, Ordering::SeqCst);
        e.handle(Input::Tick);
        e.handle(Input::ConnectFailed {
            error: "down".into(),
        });
        clock.0.store(40_000, Ordering::SeqCst);
        e.handle(Input::Tick);
        e.handle(Input::ConnectFailed {
            error: "down".into(),
        });
        e.handle(Input::PeerDiscovered(advert("q", 0)));
        assert!(e.remote.is_none(), "we lead the LAN");
        // we serve the LAN: a member's op lands in our room
        e.handle(Input::ListenerStarted { port: 7 });
        e.handle(Input::PeerConnected { peer: "in1".into() });
        let nonce = auth::new_nonce();
        let outs = e.handle(Input::WireIn {
            peer: "in1".into(),
            msg: WireMessage::new(Msg::Challenge { nonce }),
        });
        let theirs = match &wire_outs(&outs)[1].1 {
            Msg::Challenge { nonce } => nonce.clone(),
            other => panic!("{other:?}"),
        };
        e.handle(Input::WireIn {
            peer: "in1".into(),
            msg: WireMessage::new(Msg::Proof {
                device_id: "q".into(),
                mac: auth::prove(&key(), Role::Joiner, &theirs, "q"),
            }),
        });
        e.handle(Input::WireIn {
            peer: "in1".into(),
            msg: WireMessage::new(Msg::Hello {
                device: dev("q"),
                protocol_min: PROTOCOL_MIN,
                protocol_max: PROTOCOL,
                scope: "scope".into(),
                credential: None,
                session_id: None,
                session_revision: 0,
                held_epoch: None,
                extra: Default::default(),
            }),
        });
        assert!(e.is_serving());
        e.handle(Input::WireIn {
            peer: "in1".into(),
            msg: WireMessage::new(Msg::Op {
                base_revision: 2,
                op: play_op(),
                device_id: "q".into(),
                op_id: "q-1".into(),
                epoch: None,
                at: 1.0,
                position_ms: 0,
            }),
        });
        assert_eq!(e.document().revision, 3);
        assert!(e.unsynced_overflow);
        // back on the coordinator, still at 2: one Replace carries the LAN state
        clock.0.store(100_000, Ordering::SeqCst);
        e.handle(Input::Tick);
        e.handle(Input::Connected {
            peer: "up2".into(),
            url: "wss://c/".into(),
        });
        let outs = e.handle(Input::WireIn {
            peer: "up2".into(),
            msg: WireMessage::new(Msg::Welcome {
                session_clock_ms: 100_000.0,
                accepted_protocol: PROTOCOL,
                replica: Some(replica_with(room_doc)),
                members: vec![],
                extra: Default::default(),
            }),
        });
        assert!(!outs
            .iter()
            .any(|o| matches!(o, Output::FilePreviousStateAsSavedQueue { .. })));
        assert!(wire_outs(&outs).iter().any(|(_, m)| matches!(
            m,
            Msg::Op {
                base_revision: 2,
                op: SessionOp::Replace { document },
                ..
            } if document.current.is_some()
        )));
    }

    /// Admit `id` on inbound socket `peer` into `e`'s own room (the full
    /// LAN handshake).
    fn admit_inbound(e: &mut Engine, peer: &str, id: &str) {
        e.handle(Input::ListenerStarted { port: 7 });
        e.handle(Input::PeerConnected { peer: peer.into() });
        let outs = e.handle(Input::WireIn {
            peer: peer.into(),
            msg: WireMessage::new(Msg::Challenge {
                nonce: auth::new_nonce(),
            }),
        });
        let theirs = match &wire_outs(&outs)[1].1 {
            Msg::Challenge { nonce } => nonce.clone(),
            other => panic!("{other:?}"),
        };
        e.handle(Input::WireIn {
            peer: peer.into(),
            msg: WireMessage::new(Msg::Proof {
                device_id: id.into(),
                mac: auth::prove(&key(), Role::Joiner, &theirs, id),
            }),
        });
        let outs = e.handle(Input::WireIn {
            peer: peer.into(),
            msg: WireMessage::new(Msg::Hello {
                device: dev(id),
                protocol_min: PROTOCOL_MIN,
                protocol_max: PROTOCOL,
                scope: "scope".into(),
                credential: None,
                session_id: None,
                session_revision: 0,
                held_epoch: None,
                extra: Default::default(),
            }),
        });
        assert!(wire_outs(&outs)
            .iter()
            .any(|(p, m)| p == peer && matches!(m, Msg::Welcome { .. })));
    }

    #[test]
    fn restored_leader_room_answers_dedupe_for_its_members_plays() {
        // a LAN leader's room judges a member's scrobble
        let (mut e, _) = engine("m");
        admit_inbound(&mut e, "in1", "q");
        let outs = e.handle(Input::WireIn {
            peer: "in1".into(),
            msg: WireMessage::new(Msg::ScrobbleDedupeQuery {
                query_id: "q-1".into(),
                track_id: "t".into(),
                started_at: 5_000.0,
                device_id: "q".into(),
            }),
        });
        assert!(wire_outs(&outs).iter().any(|(p, m)| p == "in1"
            && matches!(
                m,
                Msg::ScrobbleDedupeAnswer {
                    duplicate: false,
                    ..
                }
            )));
        // what the leader persists includes its room's log
        let known = e.known_scrobbled();
        assert!(known
            .iter()
            .any(|k| k.track_id == "t" && k.device_id == "q"));
        // killed and restarted: the new room still knows, so another member
        // asking about the same play is told it is taken
        let (mut fresh, _) = engine("m");
        fresh.restore_known_scrobbled(known);
        admit_inbound(&mut fresh, "in2", "r");
        let outs = fresh.handle(Input::WireIn {
            peer: "in2".into(),
            msg: WireMessage::new(Msg::ScrobbleDedupeQuery {
                query_id: "r-1".into(),
                track_id: "t".into(),
                started_at: 5_000.0,
                device_id: "r".into(),
            }),
        });
        let duplicate = wire_outs(&outs)
            .into_iter()
            .find_map(|(p, m)| match m {
                Msg::ScrobbleDedupeAnswer { duplicate, .. } if p == "in2" => Some(duplicate),
                _ => None,
            })
            .expect("an answer");
        assert!(duplicate, "the restored room remembers q's claim");
        // and the leader itself would not scrobble it either
        let outs = fresh.handle(Input::ScrobbleReached {
            track_id: "t".into(),
            started_at: 5_000.0,
        });
        assert!(outs
            .iter()
            .any(|o| matches!(o, Output::Scrobble { allowed: false, .. })));
    }

    fn scrobble_verdicts(outs: &[Output]) -> Vec<bool> {
        outs.iter()
            .filter_map(|o| match o {
                Output::Scrobble { allowed, .. } => Some(*allowed),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn a_fresh_lan_device_is_no_scrobble_authority_until_discovery_settles() {
        let (mut e, clock) = engine("m");
        e.handle(Input::SetLanDiscovery(true));
        // nobody in view yet: that may only mean nobody heard yet
        let outs = e.handle(Input::ScrobbleReached {
            track_id: "t".into(),
            started_at: 1.0,
        });
        assert!(scrobble_verdicts(&outs).is_empty(), "deferred");
        let mut verdicts = vec![];
        for _ in 0..10 {
            clock.0.fetch_add(1_000, Ordering::SeqCst);
            verdicts.extend(scrobble_verdicts(&e.handle(Input::Tick)));
        }
        // settled and still alone: our own room judges it
        assert_eq!(verdicts, vec![true]);
    }

    #[test]
    fn a_lone_leader_waits_for_a_proven_peer_in_view() {
        let (mut e, clock) = engine("m");
        e.handle(Input::LocalOp { op: play_op() });
        e.handle(Input::SetLanDiscovery(true));
        clock
            .0
            .fetch_add(LAN_SETTLE_MS as u64 + 1, Ordering::SeqCst);
        // an advert we never shared a room with (a rogue's, say) earns no
        // patience: we lead (higher revision) and judge at once
        e.handle(Input::PeerDiscovered(advert("y", 0)));
        assert!(e.remote.is_none());
        let outs = e.handle(Input::ScrobbleReached {
            track_id: "t".into(),
            started_at: 1.0,
        });
        assert_eq!(scrobble_verdicts(&outs), vec![true]);
        // a device that was in a room with us is in view but not with us:
        // what it knows reaches us only when it joins, so we wait for it
        e.lan_proven.insert("z".into());
        e.handle(Input::PeerDiscovered(advert("z", 0)));
        assert!(e.remote.is_none(), "we still lead");
        let outs = e.handle(Input::ScrobbleReached {
            track_id: "u".into(),
            started_at: 2.0,
        });
        assert!(scrobble_verdicts(&outs).is_empty(), "deferred");
        clock.0.fetch_add(5_000, Ordering::SeqCst);
        assert!(scrobble_verdicts(&e.handle(Input::Tick)).is_empty());
        // it went away (or joined and left): our room decides after a beat
        e.handle(Input::PeerLost {
            device_id: "z".into(),
        });
        let mut verdicts = vec![];
        for _ in 0..4 {
            clock.0.fetch_add(1_000, Ordering::SeqCst);
            verdicts.extend(scrobble_verdicts(&e.handle(Input::Tick)));
        }
        assert_eq!(verdicts, vec![true]);
    }

    #[test]
    fn a_peers_scrobbled_flag_is_not_credited_to_it_while_our_own_verdict_is_pending() {
        let (mut e, _) = engine("a");
        let mut room_doc = crate::session::new_document("scope", "s".into(), 0.0);
        room_doc.revision = 1;
        attach(&mut e, Some(replica_with(room_doc)), vec![dev("b")]);
        e.handle(Input::LocalOp { op: play_op() });
        e.handle(Input::WireIn {
            peer: "up".into(),
            msg: WireMessage::new(Msg::OpAck {
                op_id: "a-1".into(),
                revision: 2,
            }),
        });
        let current = e.document().current.clone().expect("playing");
        let started_at = 5_000.0;
        // we reached the play; our query is out, the verdict not back yet
        let outs = e.handle(Input::ScrobbleReached {
            track_id: current.track_id.clone(),
            started_at,
        });
        assert!(wire_outs(&outs)
            .iter()
            .any(|(_, m)| matches!(m, Msg::ScrobbleDedupeQuery { .. })));
        // b (which took the play over from us) stamps it "scrobbled": that
        // is our own reach coming back, not b's scrobble
        let stamp = |device: &str, key: &QueueKey| {
            WireMessage::new(Msg::TransportStamp {
                device_id: device.into(),
                key: Some(key.clone()),
                position: PositionStamp {
                    position_ms: 60_000,
                    taken_at: 10_000.0,
                    rate: 1.0,
                    is_playing: true,
                },
                played_ms: 60_000,
                started_at,
                scrobbled: true,
                epoch: 1,
            })
        };
        e.handle(Input::WireIn {
            peer: "up".into(),
            msg: stamp("b", &current.key),
        });
        assert!(
            !e.known_scrobbled()
                .iter()
                .any(|k| k.track_id == current.track_id && k.device_id == "b"),
            "not credited to b"
        );
        // with nothing of ours pending, the flag is knowledge as before
        let (mut other, _) = engine("c");
        let mut room_doc = crate::session::new_document("scope", "s".into(), 0.0);
        room_doc.revision = 1;
        attach(&mut other, Some(replica_with(room_doc)), vec![dev("b")]);
        other.handle(Input::LocalOp { op: play_op() });
        other.handle(Input::WireIn {
            peer: "up".into(),
            msg: WireMessage::new(Msg::OpAck {
                op_id: "c-1".into(),
                revision: 2,
            }),
        });
        let key = other.document().current.clone().unwrap().key;
        other.handle(Input::WireIn {
            peer: "up".into(),
            msg: stamp("b", &key),
        });
        assert!(other
            .known_scrobbled()
            .iter()
            .any(|k| k.track_id == current.track_id && k.device_id == "b"));
    }

    #[test]
    fn a_lan_leader_carries_its_members_scrobbles_to_the_coordinator() {
        let (mut e, _) = engine("m");
        admit_inbound(&mut e, "in1", "q");
        e.handle(Input::WireIn {
            peer: "in1".into(),
            msg: WireMessage::new(Msg::ScrobbleDedupeQuery {
                query_id: "q-1".into(),
                track_id: "t".into(),
                started_at: 5_000.0,
                device_id: "q".into(),
            }),
        });
        // the leader takes the play over and reaches it itself: q has it
        let outs = e.handle(Input::ScrobbleReached {
            track_id: "t".into(),
            started_at: 5_000.0,
        });
        assert_eq!(scrobble_verdicts(&outs), vec![false]);
        // joining the coordinator, it announces q's scrobble there
        let outs = attach(&mut e, None, vec![]);
        assert!(wire_outs(&outs).iter().any(|(p, m)| p == "up"
            && matches!(
                m,
                Msg::ScrobbleSubmitted { track_id, device_id, .. } if track_id == "t" && device_id == "q"
            )));
    }

    #[test]
    fn known_scrobbles_round_trip_through_restore() {
        let (mut e, _) = engine("a");
        e.handle(Input::ScrobbleReached {
            track_id: "t".into(),
            started_at: 1.0,
        });
        let known = e.known_scrobbled();
        assert_eq!(known.len(), 1);
        let json = serde_json::to_string(&known).unwrap();
        let back: Vec<KnownScrobble> = serde_json::from_str(&json).unwrap();
        // another device restores it: both it and its room know the pair
        let (mut other, _) = engine("b");
        other.restore_known_scrobbled(back.clone());
        let outs = other.handle(Input::ScrobbleReached {
            track_id: "t".into(),
            started_at: 1.0,
        });
        assert!(outs
            .iter()
            .any(|o| matches!(o, Output::Scrobble { allowed: false, .. })));
        assert_eq!(other.room().replica().scrobbles.len(), 1);
        // the device itself, restarted with the pair still in its outbox (it
        // may never have submitted): its room says "yours", so it submits
        let (mut fresh, _) = engine("a");
        fresh.restore_known_scrobbled(back);
        assert_eq!(fresh.room().replica().scrobbles.len(), 1);
        let outs = fresh.handle(Input::ScrobbleReached {
            track_id: "t".into(),
            started_at: 1.0,
        });
        assert!(outs
            .iter()
            .any(|o| matches!(o, Output::Scrobble { allowed: true, .. })));
    }

    #[test]
    fn unknown_wire_messages_are_ignored() {
        let (mut e, _) = engine("a");
        attach(&mut e, None, vec![]);
        let outs = e.handle(Input::WireIn {
            peer: "up".into(),
            msg: WireMessage::new(Msg::Unknown),
        });
        assert!(outs.is_empty());
        let outs = e.handle(Input::WireIn {
            peer: "stranger".into(),
            msg: WireMessage::new(Msg::SyncRequest),
        });
        assert!(outs.is_empty());
    }

    #[test]
    fn saved_queues_and_settings_sync_out_and_merge_in() {
        use crate::api::{Setting, SettingScope};
        let (mut e, _) = engine("a");
        attach(&mut e, None, vec![]);
        let s = Setting {
            key: "k".into(),
            value: "1".into(),
            scope: SettingScope::AccountSynced,
            updated_at: 5.0,
        };
        let outs = e.handle(Input::SettingChanged(s.clone()));
        assert!(wire_outs(&outs)
            .iter()
            .any(|(_, m)| matches!(m, Msg::SettingsSync { settings } if settings.len() == 1)));
        let newer = Setting {
            value: "2".into(),
            updated_at: 9.0,
            ..s.clone()
        };
        let outs = e.handle(Input::WireIn {
            peer: "up".into(),
            msg: WireMessage::new(Msg::SettingsSync {
                settings: vec![newer],
            }),
        });
        assert!(outs
            .iter()
            .any(|o| matches!(o, Output::SettingsMerged(m) if m[0].value == "2")));
        let older = Setting {
            value: "0".into(),
            updated_at: 1.0,
            ..s
        };
        let outs = e.handle(Input::WireIn {
            peer: "up".into(),
            msg: WireMessage::new(Msg::SettingsSync {
                settings: vec![older],
            }),
        });
        assert!(!outs.iter().any(|o| matches!(o, Output::SettingsMerged(_))));
        assert_eq!(e.settings()[0].value, "2");
    }
}
