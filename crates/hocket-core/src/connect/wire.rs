//! The Connect wire protocol: JSON over WebSocket.
//!
//! Every frame is one [`WireMessage`] envelope: `{"protocolVersion": n, "msg": {"type": ..., "data": ...}}`.
//! The `msg` payload is an adjacently tagged [`Msg`], exactly like the FFI
//! types in [`crate::api`], so a protocol log reads the same way an event
//! log does.
//!
//! Compatibility rules (see `docs/design.md`, "Wire format and migrations"):
//!
//! - The handshake negotiates the highest protocol both ends speak and refuses
//!   anything below [`PROTOCOL_MIN`]. See [`negotiate`].
//! - Unknown *fields* round-trip untouched. The envelope and the structured
//!   payloads that a device stores and re-sends carry a flattened `extra` map,
//!   so a stale device never strips state added by a newer one.
//! - Unknown *messages* decode to [`Msg::Unknown`] and are ignored, never
//!   treated as an error.
//! - No `u64`/`i64`, no tuples, no `serde_json::Value` outside the `extra`
//!   maps (which typeshare skips).
//!
//! Topology: every device talks to exactly one *room* (the coordinator role,
//! see [`crate::connect::room`]), which relays, orders and replicates. Client
//! to room messages are documented as "→ room", room to client as "→ client".

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use typeshare::typeshare;

use crate::api::{
    AutoplayProvider, Command, DeviceId, DeviceInfo, EpochMs, Ms, PlayContextArgs, PositionStamp, QueueKey, QueueMode,
    RepeatMode, SavedQueue, ServerId, SessionDocument, SessionId, Setting, TrackId, TransportLease, UndoEntry,
    PROTOCOL_MIN_VERSION, PROTOCOL_VERSION,
};

/// Highest protocol version this build speaks.
pub const PROTOCOL: u32 = PROTOCOL_VERSION;
/// Hard floor. The handshake refuses anything older.
pub const PROTOCOL_MIN: u32 = PROTOCOL_MIN_VERSION;

/// Extra fields preserved verbatim. Typeshare never sees it.
pub type Extra = HashMap<String, Value>;

/// One frame on the wire.
#[typeshare]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct WireMessage {
    /// The sender's protocol version (not the negotiated one), so a log line
    /// says who speaks what.
    pub protocol_version: u32,
    pub msg: Msg,
    /// Unknown envelope fields from newer peers, round-tripped untouched.
    #[typeshare(skip)]
    #[serde(flatten, default, skip_serializing_if = "HashMap::is_empty")]
    pub extra: Extra,
}

impl WireMessage {
    /// Wrap a message with this build's protocol version.
    pub fn new(msg: Msg) -> Self {
        WireMessage { protocol_version: PROTOCOL, msg, extra: HashMap::new() }
    }

    /// Serialise to one JSON text frame.
    pub fn encode(&self) -> Result<String, WireError> {
        serde_json::to_string(self).map_err(WireError::Json)
    }

    /// Parse one JSON text frame. Unknown message types become
    /// [`Msg::Unknown`] (whatever payload they carry); a malformed envelope
    /// or a malformed known message is an error.
    pub fn decode(text: &str) -> Result<Self, WireError> {
        let mut v: Value = serde_json::from_str(text).map_err(WireError::Json)?;
        let unknown = v
            .get("msg")
            .and_then(|m| m.get("type"))
            .and_then(|t| t.as_str())
            .map(|t| !Msg::is_known(t))
            .unwrap_or(false);
        if unknown {
            v["msg"] = serde_json::json!({ "type": "unknown" });
        }
        serde_json::from_value(v).map_err(WireError::Json)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum WireError {
    #[error("wire json: {0}")]
    Json(#[source] serde_json::Error),
    #[error("protocol {offered_min}..={offered_max} not compatible with {ours_min}..={ours_max}")]
    Incompatible { offered_min: u32, offered_max: u32, ours_min: u32, ours_max: u32 },
}

/// Pick the highest protocol both ranges contain, refusing anything below the
/// hard floor on either side.
pub fn negotiate(their_min: u32, their_max: u32) -> Result<u32, WireError> {
    let ours_min = PROTOCOL_MIN;
    let ours_max = PROTOCOL;
    let low = their_min.max(ours_min);
    let high = their_max.min(ours_max);
    if their_max < PROTOCOL_MIN || low > high {
        return Err(WireError::Incompatible { offered_min: their_min, offered_max: their_max, ours_min, ours_max });
    }
    Ok(high)
}

// ---------------------------------------------------------------------------
// Handshake payloads
// ---------------------------------------------------------------------------

/// What the coordinator proxies to Navidrome to verify a client. Subsonic
/// token+salt (or an API key when the server supports it), never a password.
/// The coordinator forwards it once in a `ping` and forgets it.
#[typeshare]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct Credential {
    pub server_url: String,
    pub username: String,
    /// `md5(password + salt)` as hex.
    pub token: Option<String>,
    pub salt: Option<String>,
    /// OpenSubsonic `apiKey` when the server advertises `apiKeyAuthentication`.
    pub api_key: Option<String>,
    /// Subsonic `c` (client name) parameter.
    pub client: String,
    /// Subsonic `v` parameter.
    pub api_version: String,
}

impl Credential {
    /// Query parameters for a Subsonic `ping` (`f=json` included).
    pub fn ping_params(&self) -> Vec<(String, String)> {
        let mut p = vec![
            ("u".to_string(), self.username.clone()),
            ("c".to_string(), self.client.clone()),
            ("v".to_string(), self.api_version.clone()),
            ("f".to_string(), "json".to_string()),
        ];
        if let Some(key) = &self.api_key {
            p.push(("apiKey".to_string(), key.clone()));
        } else {
            if let Some(t) = &self.token {
                p.push(("t".to_string(), t.clone()));
            }
            if let Some(s) = &self.salt {
                p.push(("s".to_string(), s.clone()));
            }
        }
        p
    }
}

/// Session scope key: the Navidrome identity a session belongs to. Every
/// device derives it the same way so rooms key on it.
pub fn scope_key(server_url: &str, username: &str) -> String {
    let url = server_url.trim().trim_end_matches('/').to_ascii_lowercase();
    format!("{url}|{username}")
}

/// The replica a room holds. See `docs/design.md`, "Resume and recovery".
#[typeshare]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ReplicaState {
    pub document: SessionDocument,
    /// LWW set keyed on context identity.
    #[serde(default)]
    pub saved_queues: Vec<SavedQueue>,
    /// LWW set keyed on setting key (account-synced keys only).
    #[serde(default)]
    pub settings: Vec<Setting>,
    /// Per-device last seen.
    #[serde(default)]
    pub devices: Vec<DeviceLease>,
    pub transport_lease: TransportLease,
    /// `(position, takenAt, isPlaying)` as last received, with who sent it.
    pub last_stamp: Option<LastStamp>,
    /// Short dedupe log of submitted scrobbles.
    #[serde(default)]
    pub scrobbles: Vec<ScrobbleRecord>,
    pub updated_at: EpochMs,
    #[typeshare(skip)]
    #[serde(flatten, default, skip_serializing_if = "HashMap::is_empty")]
    pub extra: Extra,
}

#[typeshare]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DeviceLease {
    pub device: DeviceInfo,
    pub last_seen: EpochMs,
}

#[typeshare]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LastStamp {
    pub device_id: DeviceId,
    pub device_name: String,
    pub key: Option<QueueKey>,
    pub position: PositionStamp,
    pub played_ms: Ms,
    /// When the current play of `key` started (session clock), for scrobble identity.
    pub started_at: EpochMs,
    pub scrobbled: bool,
}

#[typeshare]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ScrobbleRecord {
    pub track_id: TrackId,
    pub started_at: EpochMs,
    pub device_id: DeviceId,
    pub recorded_at: EpochMs,
}

#[typeshare]
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum RefuseReason {
    ProtocolTooOld,
    ProtocolTooNew,
    Unauthorised,
    ScopeMismatch,
    Full,
    ShuttingDown,
    Duplicate,
}

#[typeshare]
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum RejectReason {
    /// `baseRevision` did not match the room's revision.
    Stale,
    /// The reducer could not apply the op to the current document.
    Unapplicable,
    /// The op carried a stale transport epoch.
    Fenced,
}

/// Remote control forwarded to whoever owns transport.
#[typeshare]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", tag = "type", content = "data")]
pub enum TransportCommand {
    Play,
    Pause,
    TogglePlay,
    Stop,
    SeekTo { position_ms: Ms },
    SeekBy { delta_ms: i32 },
    SetVolume { volume: f64 },
}

// ---------------------------------------------------------------------------
// Session ops
// ---------------------------------------------------------------------------

/// One autoplay result as it travels in an op.
#[typeshare]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AutoplayOpItem {
    pub track_id: TrackId,
    pub provider: AutoplayProvider,
    pub reason: String,
    pub score: Option<f64>,
}

/// A session-document mutation as it travels between devices. Mirrors
/// [`crate::session::QueueOp`] variant for variant (the adapter in
/// [`crate::connect::session_adapter`] is a mechanical match) plus
/// [`SessionOp::Replace`] for whole-document pushes. Everything a replica
/// needs to apply an op *deterministically* (queue keys, shuffle seeds) is
/// derived from the op id every replica sees, never from local randomness.
#[typeshare]
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", tag = "type", content = "data")]
pub enum SessionOp {
    PlayContext { args: PlayContextArgs },
    PlayTracks {
        server_id: ServerId,
        track_ids: Vec<TrackId>,
        start_index: u32,
        label: String,
        shuffle: bool,
        save_outgoing: bool,
    },
    PlayNext { server_id: ServerId, track_ids: Vec<TrackId> },
    PlayLater { server_id: ServerId, track_ids: Vec<TrackId> },
    JumpToQueueItem { key: QueueKey },
    RemoveQueueItems { keys: Vec<QueueKey> },
    MoveQueueItem { key: QueueKey, to_index: u32 },
    ClearQueue,
    ClearInsertions,
    SetShuffle { enabled: bool },
    Reshuffle,
    SetRepeat { mode: RepeatMode },
    SetAutoplay { enabled: bool },
    SetQueueMode { mode: QueueMode },
    Next,
    Previous,
    /// Mark unplayable and advance; never scrobbles.
    SkipUnavailable { key: QueueKey },
    AppendAutoplay { items: Vec<AutoplayOpItem> },
    TrackEnded,
    RestoreSavedQueue { id: String, tracks: Option<Vec<TrackId>> },
    SetContextTracks { tracks: Vec<TrackId> },
    SaveCurrentQueue { pinned: bool },
    PinSavedQueue { id: String, pinned: bool },
    DeleteSavedQueue { id: String },
    TouchSavedQueue { id: String },
    /// LWW merge of a saved-queue set into the document's.
    MergeSavedQueues { remote: Vec<SavedQueue> },
    /// Whole-document replacement: undo snapshot restore, initial push into an
    /// empty room, a returning device's fast-forward. `revision`, `sessionId`
    /// and `scope` of the target are kept.
    Replace { document: SessionDocument },
}

impl SessionOp {
    /// The wire op for a public command, or `None` when the command does not
    /// touch the session document.
    pub fn from_command(cmd: &Command) -> Option<SessionOp> {
        Some(match cmd {
            Command::PlayContext { args } => SessionOp::PlayContext { args: args.clone() },
            Command::PlayTracks { server_id, track_ids, start_index, label, shuffle } => SessionOp::PlayTracks {
                server_id: server_id.clone(),
                track_ids: track_ids.clone(),
                start_index: *start_index,
                label: label.clone(),
                shuffle: *shuffle,
                save_outgoing: true,
            },
            Command::PlayNext { server_id, track_ids } => {
                SessionOp::PlayNext { server_id: server_id.clone(), track_ids: track_ids.clone() }
            }
            Command::PlayLater { server_id, track_ids } => {
                SessionOp::PlayLater { server_id: server_id.clone(), track_ids: track_ids.clone() }
            }
            Command::JumpToQueueItem { key } => SessionOp::JumpToQueueItem { key: key.clone() },
            Command::RemoveQueueItems { keys } => SessionOp::RemoveQueueItems { keys: keys.clone() },
            Command::MoveQueueItem { key, to_index } => SessionOp::MoveQueueItem { key: key.clone(), to_index: *to_index },
            Command::ClearQueue => SessionOp::ClearQueue,
            Command::ClearInsertions => SessionOp::ClearInsertions,
            Command::SetShuffle { enabled } => SessionOp::SetShuffle { enabled: *enabled },
            Command::SetRepeat { mode } => SessionOp::SetRepeat { mode: *mode },
            Command::SetAutoplay { enabled } => SessionOp::SetAutoplay { enabled: *enabled },
            Command::SetQueueMode { mode } => SessionOp::SetQueueMode { mode: *mode },
            Command::Next => SessionOp::Next,
            Command::Previous => SessionOp::Previous,
            Command::SkipUnavailable { key } => SessionOp::SkipUnavailable { key: key.clone() },
            Command::RestoreSavedQueue { id } => SessionOp::RestoreSavedQueue { id: id.clone(), tracks: None },
            Command::PinSavedQueue { id, pinned } => SessionOp::PinSavedQueue { id: id.clone(), pinned: *pinned },
            Command::DeleteSavedQueue { id } => SessionOp::DeleteSavedQueue { id: id.clone() },
            _ => return None,
        })
    }

    /// True for ops that change what is playing (track change) rather than
    /// only the queue around it. Used for the replica write cadence.
    pub fn changes_current(&self) -> bool {
        matches!(
            self,
            SessionOp::PlayContext { .. }
                | SessionOp::PlayTracks { .. }
                | SessionOp::Next
                | SessionOp::Previous
                | SessionOp::JumpToQueueItem { .. }
                | SessionOp::ClearQueue
                | SessionOp::SkipUnavailable { .. }
                | SessionOp::TrackEnded
                | SessionOp::RestoreSavedQueue { .. }
                | SessionOp::Replace { .. }
        )
    }

    /// Ops only the transport owner issues (they follow playback itself).
    pub fn is_owner_op(&self) -> bool {
        matches!(self, SessionOp::TrackEnded | SessionOp::SkipUnavailable { .. })
    }
}

// ---------------------------------------------------------------------------
// Messages
// ---------------------------------------------------------------------------

/// Everything that crosses the wire. Struct variants keep the JSON readable.
#[typeshare]
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", tag = "type", content = "data")]
pub enum Msg {
    // -- handshake ------------------------------------------------------
    /// → room. First message on a connection.
    Hello {
        device: DeviceInfo,
        protocol_min: u32,
        protocol_max: u32,
        scope: String,
        /// Present when the room verifies (hosted coordinator). Never stored.
        credential: Option<Credential>,
        session_id: Option<SessionId>,
        session_revision: u32,
        /// The transport epoch this device last held, to reclaim inside the window.
        held_epoch: Option<u32>,
        #[typeshare(skip)]
        #[serde(flatten, default, skip_serializing_if = "HashMap::is_empty")]
        extra: Extra,
    },
    /// → client. Admitted.
    Welcome {
        session_clock_ms: EpochMs,
        accepted_protocol: u32,
        /// `None` when the room has nothing yet: the joiner pushes its document.
        replica: Option<ReplicaState>,
        /// Currently connected devices, excluding the joiner.
        members: Vec<DeviceInfo>,
        #[typeshare(skip)]
        #[serde(flatten, default, skip_serializing_if = "HashMap::is_empty")]
        extra: Extra,
    },
    /// → client. Not admitted; the room closes the connection afterwards.
    Refuse { reason: RefuseReason, message: String },
    /// Either direction. Sent before closing on purpose.
    Bye { reason: String },

    // -- session ops ----------------------------------------------------
    /// → room. Targets `baseRevision`; `epoch` is set for ops the transport owner issues
    /// (auto-advance) so a fenced device cannot push them. `at` (session clock) and
    /// `positionMs` are the originator's, and every replica applies the op with
    /// them so time-derived fields come out identical everywhere.
    Op {
        base_revision: u32,
        op: SessionOp,
        device_id: DeviceId,
        op_id: String,
        epoch: Option<u32>,
        #[serde(default)]
        at: EpochMs,
        #[serde(default)]
        position_ms: Ms,
    },
    /// → client (the originator).
    OpAck { op_id: String, revision: u32 },
    /// → client (the originator). Carries the current document for rollback.
    OpReject { op_id: String, current_revision: u32, reason: RejectReason, document: SessionDocument },
    /// → client (everyone but the originator). An op the room accepted.
    OpCommitted {
        op: SessionOp,
        revision: u32,
        device_id: DeviceId,
        op_id: String,
        #[serde(default)]
        at: EpochMs,
        #[serde(default)]
        position_ms: Ms,
    },
    /// → client. Full document (join, resync).
    Document { document: SessionDocument },
    /// → room. Ask for a full document.
    SyncRequest,

    // -- transport ------------------------------------------------------
    /// → room, relayed → client. Only on change, never on a timer.
    TransportStamp {
        device_id: DeviceId,
        key: Option<QueueKey>,
        position: PositionStamp,
        played_ms: Ms,
        started_at: EpochMs,
        scrobbled: bool,
        epoch: u32,
    },
    /// → room, relayed to the owner: a non-owner's remote control.
    TransportRequest { command: TransportCommand, from: DeviceId },
    /// → room. Every 5 s while owning transport. `sentAt` (sender's clock) is
    /// echoed back as `ackOf` so the owner knows exactly which heartbeat was
    /// answered.
    LeaseHeartbeat {
        epoch: u32,
        #[serde(default)]
        sent_at: EpochMs,
    },
    /// → room. `epochExpected` is the epoch a reconnecting device believes it holds.
    /// `takeover` is a deliberate pick from the picker and always wins.
    LeaseClaim {
        epoch_expected: Option<u32>,
        takeover: bool,
        #[serde(default)]
        sent_at: EpochMs,
    },
    /// → room. Owner gives transport up.
    LeaseRelease { epoch: u32 },
    /// → client (broadcast on every change, and to the owner on renewal).
    /// `ackOf` echoes the `sentAt` of the heartbeat/claim this answers.
    LeaseGranted {
        lease: TransportLease,
        #[serde(default)]
        ack_of: Option<EpochMs>,
    },
    /// → client. A stamp, heartbeat or op carried a stale epoch.
    LeaseFenced { current_epoch: u32, lease: TransportLease },

    // -- presence and clock ---------------------------------------------
    /// → client. Presence means connected; sleeping devices are absent.
    Presence { devices: Vec<DeviceInfo> },
    /// → room.
    ClockPing { t0: EpochMs },
    /// → client. `t1` receive time, `t2` send time, both on the session clock.
    ClockPong { t0: EpochMs, t1: EpochMs, t2: EpochMs },

    // -- handoff --------------------------------------------------------
    /// → room, relayed. The source opened its picker.
    HandoffPickerOpen { from: DeviceId },
    HandoffPickerClose { from: DeviceId },
    /// → room, routed to `target`. Pre-buffer this item at this position; the
    /// target resolves its own stream URL.
    HandoffPrepare { from: DeviceId, target: DeviceId, key: QueueKey, track_id: TrackId, position_ms: Ms },
    /// → room, routed to `from`. Target reports readiness.
    HandoffReady { from: DeviceId, target: DeviceId, key: QueueKey, ready: bool },
    /// → room (source), then routed to `target` with `lease` filled in by the
    /// room after it transferred ownership.
    HandoffTakeover {
        from: DeviceId,
        target: DeviceId,
        key: QueueKey,
        track_id: TrackId,
        position_ms: Ms,
        played_ms: Ms,
        started_at: EpochMs,
        scrobbled: bool,
        epoch: u32,
        lease: Option<TransportLease>,
    },
    /// → room. Source finished releasing after a takeover.
    HandoffRelease { epoch: u32 },

    // -- scrobbling -----------------------------------------------------
    /// → room. Record a submitted scrobble in the dedupe log.
    ScrobbleSubmitted { track_id: TrackId, started_at: EpochMs, device_id: DeviceId },
    /// → room. Claim a pair before submitting; the room records it if new.
    ScrobbleDedupeQuery { query_id: String, track_id: TrackId, started_at: EpochMs, device_id: DeviceId },
    /// → client.
    ScrobbleDedupeAnswer { query_id: String, duplicate: bool },

    // -- LWW sets -------------------------------------------------------
    /// Either direction. Merged LWW on `updatedAt` per context identity.
    SavedQueuesSync { queues: Vec<SavedQueue> },
    /// Either direction. Merged LWW on `updatedAt` per key.
    SettingsSync { settings: Vec<Setting> },

    // -- undo -----------------------------------------------------------
    /// Either direction. A session-tier undo entry shared with the session,
    /// stamped with its originating device.
    UndoEntryShared { entry: UndoEntry, before_revision: u32, before: Option<SessionDocument> },

    /// Anything this build does not know. Ignored.
    #[serde(other)]
    Unknown,
}

impl Msg {
    /// Every tag this build understands (the `type` values [`Msg::name`] returns).
    pub const KNOWN: &'static [&'static str] = &[
        "hello",
        "welcome",
        "refuse",
        "bye",
        "op",
        "opAck",
        "opReject",
        "opCommitted",
        "document",
        "syncRequest",
        "transportStamp",
        "transportRequest",
        "leaseHeartbeat",
        "leaseClaim",
        "leaseRelease",
        "leaseGranted",
        "leaseFenced",
        "presence",
        "clockPing",
        "clockPong",
        "handoffPickerOpen",
        "handoffPickerClose",
        "handoffPrepare",
        "handoffReady",
        "handoffTakeover",
        "handoffRelease",
        "scrobbleSubmitted",
        "scrobbleDedupeQuery",
        "scrobbleDedupeAnswer",
        "savedQueuesSync",
        "settingsSync",
        "undoEntryShared",
        "unknown",
    ];

    pub fn is_known(tag: &str) -> bool {
        Self::KNOWN.contains(&tag)
    }

    /// Short name for logs.
    pub fn name(&self) -> &'static str {
        match self {
            Msg::Hello { .. } => "hello",
            Msg::Welcome { .. } => "welcome",
            Msg::Refuse { .. } => "refuse",
            Msg::Bye { .. } => "bye",
            Msg::Op { .. } => "op",
            Msg::OpAck { .. } => "opAck",
            Msg::OpReject { .. } => "opReject",
            Msg::OpCommitted { .. } => "opCommitted",
            Msg::Document { .. } => "document",
            Msg::SyncRequest => "syncRequest",
            Msg::TransportStamp { .. } => "transportStamp",
            Msg::TransportRequest { .. } => "transportRequest",
            Msg::LeaseHeartbeat { .. } => "leaseHeartbeat",
            Msg::LeaseClaim { .. } => "leaseClaim",
            Msg::LeaseRelease { .. } => "leaseRelease",
            Msg::LeaseGranted { .. } => "leaseGranted",
            Msg::LeaseFenced { .. } => "leaseFenced",
            Msg::Presence { .. } => "presence",
            Msg::ClockPing { .. } => "clockPing",
            Msg::ClockPong { .. } => "clockPong",
            Msg::HandoffPickerOpen { .. } => "handoffPickerOpen",
            Msg::HandoffPickerClose { .. } => "handoffPickerClose",
            Msg::HandoffPrepare { .. } => "handoffPrepare",
            Msg::HandoffReady { .. } => "handoffReady",
            Msg::HandoffTakeover { .. } => "handoffTakeover",
            Msg::HandoffRelease { .. } => "handoffRelease",
            Msg::ScrobbleSubmitted { .. } => "scrobbleSubmitted",
            Msg::ScrobbleDedupeQuery { .. } => "scrobbleDedupeQuery",
            Msg::ScrobbleDedupeAnswer { .. } => "scrobbleDedupeAnswer",
            Msg::SavedQueuesSync { .. } => "savedQueuesSync",
            Msg::SettingsSync { .. } => "settingsSync",
            Msg::UndoEntryShared { .. } => "undoEntryShared",
            Msg::Unknown => "unknown",
        }
    }
}

/// Merge two LWW saved-queue sets. Keyed on context identity (the saved
/// queue `id`, which the session module derives from the context); the newer
/// `updatedAt` wins, ties keep `ours`. Returns the merged set and whether
/// `ours` changed.
pub fn merge_saved_queues(ours: &[SavedQueue], theirs: &[SavedQueue]) -> (Vec<SavedQueue>, bool) {
    let mut merged: Vec<SavedQueue> = ours.to_vec();
    let mut changed = false;
    for q in theirs {
        match merged.iter_mut().find(|m| m.id == q.id) {
            Some(existing) => {
                if q.updated_at > existing.updated_at {
                    *existing = q.clone();
                    changed = true;
                }
            }
            None => {
                merged.push(q.clone());
                changed = true;
            }
        }
    }
    (merged, changed)
}

/// Merge two LWW setting sets keyed on `key`. Newer `updatedAt` wins, ties keep `ours`.
pub fn merge_settings(ours: &[Setting], theirs: &[Setting]) -> (Vec<Setting>, bool) {
    let mut merged: Vec<Setting> = ours.to_vec();
    let mut changed = false;
    for s in theirs {
        match merged.iter_mut().find(|m| m.key == s.key) {
            Some(existing) => {
                if s.updated_at > existing.updated_at {
                    *existing = s.clone();
                    changed = true;
                }
            }
            None => {
                merged.push(s.clone());
                changed = true;
            }
        }
    }
    (merged, changed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::{Platform, QueueMode, RepeatMode, SettingScope, TransportState};

    pub(crate) fn device(id: &str) -> DeviceInfo {
        DeviceInfo {
            id: id.into(),
            name: format!("Device {id}"),
            platform: Platform::Linux,
            app_version: "0.1".into(),
            playing: false,
            ready: false,
            last_seen: 0.0,
            is_self: false,
        }
    }

    pub(crate) fn doc() -> SessionDocument {
        SessionDocument {
            schema_version: 1,
            session_id: "s1".into(),
            scope: "srv|user".into(),
            revision: 3,
            updated_at: 10.0,
            context: None,
            mode: QueueMode::Apple,
            cursor: 0,
            current: None,
            history: vec![],
            insertions: vec![],
            shuffle: None,
            repeat: RepeatMode::Off,
            autoplay: false,
            transport: TransportState::default(),
            saved_queues: vec![],
            extra: HashMap::new(),
        }
    }

    #[test]
    fn envelope_round_trips_and_is_adjacently_tagged() {
        let m = WireMessage::new(Msg::LeaseHeartbeat { epoch: 4, sent_at: 1.0 });
        let text = m.encode().unwrap();
        assert!(text.contains("\"type\":\"leaseHeartbeat\""));
        // Struct-variant fields keep serde's default names, as in `api.rs`.
        assert!(text.contains("\"data\":{\"epoch\":4,\"sent_at\":1"), "{text}");
        assert!(text.contains("\"protocolVersion\":1"));
        let back = WireMessage::decode(&text).unwrap();
        assert_eq!(back, m);
    }

    #[test]
    fn unit_variants_encode_without_data() {
        let text = WireMessage::new(Msg::SyncRequest).encode().unwrap();
        assert!(text.contains("{\"type\":\"syncRequest\"}"));
        assert_eq!(WireMessage::decode(&text).unwrap().msg, Msg::SyncRequest);
    }

    #[test]
    fn unknown_message_type_decodes_to_unknown() {
        let text = r#"{"protocolVersion":7,"msg":{"type":"teleport","data":{"where":"there"}}}"#;
        let m = WireMessage::decode(text).unwrap();
        assert_eq!(m.msg, Msg::Unknown);
        assert_eq!(m.protocol_version, 7);
    }

    #[test]
    fn known_tags_match_serialised_names() {
        let samples = vec![
            Msg::SyncRequest,
            Msg::Bye { reason: "x".into() },
            Msg::ClockPing { t0: 0.0 },
            Msg::LeaseHeartbeat { epoch: 0, sent_at: 0.0 },
            Msg::HandoffPickerOpen { from: "a".into() },
            Msg::ScrobbleDedupeAnswer { query_id: "q".into(), duplicate: false },
            Msg::SavedQueuesSync { queues: vec![] },
            Msg::SettingsSync { settings: vec![] },
            Msg::Unknown,
        ];
        for m in samples {
            let v = serde_json::to_value(&m).unwrap();
            let tag = v["type"].as_str().unwrap();
            assert_eq!(tag, m.name());
            assert!(Msg::is_known(tag), "{tag}");
        }
        // a malformed known message is still an error
        assert!(WireMessage::decode(r#"{"protocolVersion":1,"msg":{"type":"op","data":{"nope":1}}}"#).is_err());
    }

    #[test]
    fn unknown_envelope_fields_survive_round_trip() {
        let text = r#"{"protocolVersion":2,"msg":{"type":"syncRequest"},"traceId":"abc","nested":{"a":[1,2]}}"#;
        let m = WireMessage::decode(text).unwrap();
        assert_eq!(m.extra.get("traceId").unwrap(), "abc");
        let out = m.encode().unwrap();
        let v: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["traceId"], "abc");
        assert_eq!(v["nested"]["a"][1], 2);
        assert_eq!(v["msg"]["type"], "syncRequest");
    }

    #[test]
    fn unknown_hello_fields_survive_round_trip() {
        let hello = Msg::Hello {
            device: device("a"),
            protocol_min: 1,
            protocol_max: 1,
            scope: "s".into(),
            credential: None,
            session_id: None,
            session_revision: 0,
            held_epoch: None,
            extra: HashMap::new(),
        };
        let mut v: Value = serde_json::to_value(WireMessage::new(hello)).unwrap();
        v["msg"]["data"]["futureField"] = serde_json::json!({"deep": true});
        let text = v.to_string();
        let m = WireMessage::decode(&text).unwrap();
        match &m.msg {
            Msg::Hello { extra, .. } => assert_eq!(extra["futureField"]["deep"], true),
            other => panic!("wrong variant {other:?}"),
        }
        let back: Value = serde_json::from_str(&m.encode().unwrap()).unwrap();
        assert_eq!(back["msg"]["data"]["futureField"]["deep"], true);
    }

    #[test]
    fn unknown_replica_fields_survive_round_trip() {
        let replica = ReplicaState {
            document: doc(),
            saved_queues: vec![],
            settings: vec![],
            devices: vec![],
            transport_lease: TransportLease::default(),
            last_stamp: None,
            scrobbles: vec![],
            updated_at: 1.0,
            extra: HashMap::new(),
        };
        let mut v = serde_json::to_value(&replica).unwrap();
        v["newerThing"] = serde_json::json!([1, 2, 3]);
        let r: ReplicaState = serde_json::from_value(v).unwrap();
        let back = serde_json::to_value(&r).unwrap();
        assert_eq!(back["newerThing"][2], 3);
    }

    #[test]
    fn negotiate_picks_highest_common_and_refuses_below_floor() {
        assert_eq!(negotiate(1, 1).unwrap(), 1);
        assert_eq!(negotiate(1, 9).unwrap(), PROTOCOL);
        assert!(negotiate(0, 0).is_err());
        assert!(negotiate(PROTOCOL + 1, PROTOCOL + 5).is_err());
    }

    #[test]
    fn credential_ping_params_prefer_api_key() {
        let mut c = Credential {
            server_url: "https://x".into(),
            username: "u".into(),
            token: Some("t".into()),
            salt: Some("s".into()),
            api_key: None,
            client: "hocket".into(),
            api_version: "1.16.1".into(),
        };
        let p = c.ping_params();
        assert!(p.iter().any(|(k, v)| k == "t" && v == "t"));
        assert!(p.iter().any(|(k, v)| k == "s" && v == "s"));
        c.api_key = Some("key".into());
        let p = c.ping_params();
        assert!(p.iter().any(|(k, _)| k == "apiKey"));
        assert!(!p.iter().any(|(k, _)| k == "t"));
    }

    #[test]
    fn scope_key_normalises() {
        assert_eq!(scope_key("HTTPS://Music.Example/", "bob"), "https://music.example|bob");
        assert_eq!(scope_key(" https://music.example ", "bob"), scope_key("https://music.example/", "bob"));
    }

    fn sq(id: &str, at: f64, pinned: bool) -> SavedQueue {
        SavedQueue {
            id: id.into(),
            context: crate::api::QueueContext {
                server_id: "srv".into(),
                kind: crate::api::ContextKind::Album { id: id.into() },
                label: id.into(),
                sort: Default::default(),
                tracks: vec![],
            },
            label: id.into(),
            cursor: 0,
            current: None,
            history: vec![],
            insertions: vec![],
            shuffle: None,
            repeat: RepeatMode::Off,
            position_ms: 0,
            pinned,
            created_at: at,
            last_interacted_at: at,
            updated_at: at,
            track_count: 0,
            cover_art: None,
        }
    }

    #[test]
    fn saved_queue_merge_is_lww_and_commutative_in_result() {
        let ours = vec![sq("a", 10.0, false), sq("b", 5.0, true)];
        let theirs = vec![sq("a", 20.0, true), sq("c", 1.0, false)];
        let (m1, changed) = merge_saved_queues(&ours, &theirs);
        assert!(changed);
        let a = m1.iter().find(|q| q.id == "a").unwrap();
        assert!(a.pinned && a.updated_at == 20.0);
        assert!(m1.iter().any(|q| q.id == "c"));
        let (m2, _) = merge_saved_queues(&theirs, &ours);
        let mut s1: Vec<_> = m1.iter().map(|q| (q.id.clone(), q.updated_at, q.pinned)).collect();
        let mut s2: Vec<_> = m2.iter().map(|q| (q.id.clone(), q.updated_at, q.pinned)).collect();
        s1.sort_by(|x, y| x.0.cmp(&y.0));
        s2.sort_by(|x, y| x.0.cmp(&y.0));
        assert_eq!(s1, s2);
        let (_, changed) = merge_saved_queues(&m1, &theirs);
        assert!(!changed);
    }

    #[test]
    fn settings_merge_is_lww() {
        let s = |k: &str, v: &str, at: f64| Setting {
            key: k.into(),
            value: v.into(),
            scope: SettingScope::AccountSynced,
            updated_at: at,
        };
        let (m, changed) = merge_settings(&[s("theme", "dark", 5.0)], &[s("theme", "light", 3.0), s("x", "1", 1.0)]);
        assert!(changed);
        assert_eq!(m.iter().find(|x| x.key == "theme").unwrap().value, "dark");
        assert_eq!(m.len(), 2);
    }

    #[test]
    fn session_op_round_trips_with_document() {
        let op = SessionOp::Replace { document: doc() };
        let text = serde_json::to_string(&Msg::Op {
            base_revision: 3,
            op,
            device_id: "d".into(),
            op_id: "o".into(),
            epoch: Some(2),
            at: 1.0,
            position_ms: 0,
        })
        .unwrap();
        assert!(text.contains("\"type\":\"replace\""));
        let back: Msg = serde_json::from_str(&text).unwrap();
        assert!(matches!(back, Msg::Op { base_revision: 3, .. }));
    }
}
