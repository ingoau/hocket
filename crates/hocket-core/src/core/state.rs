//! Plain state the actor keeps beside its subsystems.

use std::sync::Arc;

use crate::api::*;
use crate::connect::wire::Credential as WireCredential;
use crate::db::sync::LibrarySync;
use crate::subsonic::{Client, SubsonicApi};

/// The one server (the registry always holds one for now).
pub(crate) struct ServerState {
    pub info: ServerInfo,
    /// Present once the platform supplied credentials (`AddServer`).
    pub api: Option<Arc<dyn SubsonicApi>>,
    /// The concrete client, for the capability probe.
    pub client: Option<Arc<Client>>,
    pub credential: Option<WireCredential>,
    pub sync: Option<Arc<LibrarySync>>,
    pub probing: bool,
    pub sync_job: Option<JobId>,
    pub last_sync_started: Option<f64>,
    pub ready_tables: Vec<String>,
}

impl ServerState {
    pub fn new(info: ServerInfo) -> ServerState {
        ServerState {
            info,
            api: None,
            client: None,
            credential: None,
            sync: None,
            probing: false,
            sync_job: None,
            last_sync_started: None,
            ready_tables: vec![],
        }
    }
}

/// What this device's backend has loaded and where it is.
#[derive(Debug, Clone, Default)]
pub(crate) struct Playback {
    /// Key of the document item the loaded media stands for.
    pub doc_key: Option<QueueKey>,
    /// Key the backend was given (differs from `doc_key` after a gapless
    /// transition: the preloaded item carried its pre-materialisation key).
    pub backend_key: Option<QueueKey>,
    pub track: Option<Track>,
    /// Last known position and the local time it was taken.
    pub position_ms: Ms,
    pub position_at: EpochMs,
    pub playing: bool,
    pub buffering: bool,
    /// Session-clock identity of this play (scrobble dedupe key).
    pub started_at: EpochMs,
    /// Local wall time the play started (play history).
    pub started_local: EpochMs,
    pub scrobbled: bool,
    /// Preloaded follow-up and the document item it stands for.
    pub next: Option<MediaSource>,
    pub next_doc_key: Option<QueueKey>,
    /// The user wants playback (a play command or a play-from-here op).
    pub want_playing: bool,
    /// Consecutive fatal load failures of the current item.
    pub load_failures: u32,
    /// Consecutive items skipped as unavailable.
    pub consecutive_skips: u32,
    /// Something is loaded in the backend.
    pub loaded: bool,
    /// Position to land on the next time this key is loaded (undo, restore).
    pub restore_position: Option<(QueueKey, Ms)>,
    pub volume: f64,
    /// Sleep-timer fade multiplier applied on top of the user volume.
    pub fade_gain: Option<f64>,
    /// Waiting for a gapless `TransitionedToNext` for this doc key.
    pub awaiting_transition: bool,
}

impl Playback {
    pub fn position_now(&self, now: EpochMs) -> Ms {
        if self.playing && self.loaded {
            let elapsed = (now - self.position_at).max(0.0);
            let pos = f64::from(self.position_ms) + elapsed;
            let cap = self
                .track
                .as_ref()
                .map(|t| f64::from(t.duration_ms))
                .filter(|d| *d > 0.0)
                .unwrap_or(f64::MAX);
            pos.min(cap).min(f64::from(u32::MAX)) as Ms
        } else {
            self.position_ms
        }
    }

    pub fn track_id(&self) -> Option<&str> {
        self.track.as_ref().map(|t| t.id.as_str())
    }

    pub fn duration_ms(&self) -> Ms {
        self.track.as_ref().map(|t| t.duration_ms).unwrap_or(0)
    }
}

/// Persisted next to the session document.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub(crate) struct SavedPosition {
    pub key: Option<QueueKey>,
    pub position_ms: Ms,
}

/// Bookkeeping for an undo whose inverse runs as compare-and-swap steps.
#[derive(Debug, Clone)]
pub(crate) struct PendingCas {
    pub outcome: crate::undo::CasOutcome,
    pub label: String,
    pub redo: bool,
}
