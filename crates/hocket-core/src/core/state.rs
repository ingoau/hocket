//! Plain state the actor keeps beside its subsystems.

use std::sync::Arc;

use crate::api::*;
use crate::connect::auth::LanKey;
use crate::connect::wire::Credential as WireCredential;
use crate::db::sync::LibrarySync;
use crate::provider::{Provider, ProviderSet};
use crate::subsonic::{Client, SubsonicApi};

/// One installed account: its row, its provider once credentials arrived,
/// and its sync bookkeeping.
pub(crate) struct AccountState {
    pub info: AccountInfo,
    /// Present once the platform supplied credentials (`AddAccount`).
    pub provider: Option<Arc<dyn Provider>>,
    /// The concrete Subsonic client, for the capability probe.
    pub client: Option<Arc<Client>>,
    pub credential: Option<WireCredential>,
    /// The scope's shared LAN key (mutual LAN auth), derived from the
    /// password the user entered; the engine is rebuilt when the session
    /// scope's key changes.
    pub lan_key: Option<LanKey>,
    pub sync: Option<Arc<LibrarySync>>,
    pub probing: bool,
    pub sync_job: Option<JobId>,
    pub last_sync_started: Option<f64>,
    pub ready_tables: Vec<String>,
}

impl AccountState {
    pub fn new(info: AccountInfo) -> AccountState {
        AccountState {
            info,
            provider: None,
            client: None,
            credential: None,
            lan_key: None,
            sync: None,
            probing: false,
            sync_job: None,
            last_sync_started: None,
            ready_tables: vec![],
        }
    }

    /// The Subsonic client, for a Navidrome account with credentials.
    pub fn subsonic(&self) -> Option<Arc<dyn SubsonicApi>> {
        self.provider.as_ref().and_then(|p| p.navidrome().cloned())
    }
}

/// Every account, highest priority first. Replaces the one `server` the
/// core used to hold: library rows, pins, the outbox and queue items name
/// their account (their source), and the registry finds its provider.
#[derive(Default)]
pub(crate) struct AccountRegistry {
    accounts: Vec<AccountState>,
}

impl AccountRegistry {
    pub fn is_empty(&self) -> bool {
        self.accounts.is_empty()
    }

    pub fn len(&self) -> usize {
        self.accounts.len()
    }

    pub fn iter(&self) -> impl Iterator<Item = &AccountState> {
        self.accounts.iter()
    }

    pub fn iter_mut(&mut self) -> impl Iterator<Item = &mut AccountState> {
        self.accounts.iter_mut()
    }

    pub fn get(&self, id: &str) -> Option<&AccountState> {
        self.accounts.iter().find(|a| a.info.id == id)
    }

    pub fn get_mut(&mut self, id: &str) -> Option<&mut AccountState> {
        self.accounts.iter_mut().find(|a| a.info.id == id)
    }

    pub fn contains(&self, id: &str) -> bool {
        self.get(id).is_some()
    }

    /// The account whose ids are valid in `source` (highest priority first).
    pub fn for_source(&self, source: &str) -> Option<&AccountState> {
        self.accounts.iter().find(|a| a.info.source == source)
    }

    /// The account sessions are scoped to until identities arrive: the
    /// highest-priority Navidrome account.
    pub fn primary(&self) -> Option<&AccountState> {
        self.accounts
            .iter()
            .find(|a| a.info.kind == AccountKind::Navidrome)
    }

    /// Add or replace (by id), keeping priority order.
    pub fn insert(&mut self, state: AccountState) {
        self.accounts.retain(|a| a.info.id != state.info.id);
        self.accounts.push(state);
        self.accounts.sort_by_key(|a| a.info.priority);
    }

    pub fn take(&mut self, id: &str) -> Option<AccountState> {
        let i = self.accounts.iter().position(|a| a.info.id == id)?;
        Some(self.accounts.remove(i))
    }

    /// The priority after every installed account's.
    pub fn next_priority(&self) -> u32 {
        self.accounts
            .iter()
            .map(|a| a.info.priority + 1)
            .max()
            .unwrap_or(0)
    }

    /// Put the named accounts first, in that order, the rest after them in
    /// their current order; priorities become 0..n.
    pub fn reorder(&mut self, ids: &[AccountId]) {
        let mut rest = std::mem::take(&mut self.accounts);
        let mut out = Vec::with_capacity(rest.len());
        for id in ids {
            if let Some(i) = rest.iter().position(|a| &a.info.id == id) {
                out.push(rest.remove(i));
            }
        }
        out.extend(rest);
        for (i, a) in out.iter_mut().enumerate() {
            a.info.priority = i as u32;
        }
        self.accounts = out;
    }

    /// Providers with credentials, highest priority first.
    pub fn providers(&self) -> ProviderSet {
        ProviderSet::new(
            self.accounts
                .iter()
                .filter_map(|a| a.provider.clone())
                .collect(),
        )
    }

    pub fn ids(&self) -> Vec<AccountId> {
        self.accounts.iter().map(|a| a.info.id.clone()).collect()
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
    /// This device's volume slider position (0–1).
    pub volume: f64,
    /// This device is muted; `volume` is the level it goes back to.
    pub muted: bool,
    /// The platform owns this device's volume (Android's music stream): it
    /// reports changes with `BackendReport::SystemVolume` and is driven with
    /// `set_system_volume`; the in-player gain carries only the sleep fade.
    pub system_volume: bool,
    /// A volume just sent to the device playing elsewhere, shown until its
    /// stamps catch up (so a dragged slider doesn't jump back): the device,
    /// the volume and when it was sent (local clock).
    pub remote_volume_hold: Option<(DeviceId, DeviceVolume, f64)>,
    /// Sleep-timer fade multiplier applied on top of the user volume.
    pub fade_gain: Option<f64>,
    /// Waiting for a gapless `TransitionedToNext` for this doc key.
    pub awaiting_transition: bool,
    /// When the document's current item was adopted as the backend's
    /// preloaded follow-up without a `Load` (a gapless boundary), until
    /// the backend confirms with `TransitionedToNext`. Past
    /// [`crate::core::actor::ADOPTION_TIMEOUT_MS`] the item is loaded
    /// explicitly: a backend that ended without a follow-up in place (a
    /// `SetNext` still in flight, a preload it dropped) would otherwise
    /// leave the session on an item nothing plays.
    pub adopted_at: Option<EpochMs>,
    /// Silenced by a transient audio-focus loss (a call): the backend still
    /// means to play and resumes on its own (reporting `Playing`) when focus
    /// returns, or, had an item been loaded paused meanwhile, stays paused.
    /// A lease change and the retry after a stream error do not start
    /// playback while it holds; the user's own actions do, and so, for now,
    /// do the core's other reloads and skips.
    pub focus_suspended: bool,
    /// A skip in a burst moved to this item but the backend has not been
    /// given it yet: it is loaded once no further skip arrived by this local
    /// time (see `Actor::defer_load`). Until then the item shows as loading
    /// (`buffering`), the backend holds the old item paused, and
    /// `backend_key` is unset so the old item's reports are ignored.
    pub pending_load: Option<EpochMs>,
    /// AutoMix: the loaded source's `mix_out_ms` (as handed to the backend).
    pub mix_out_ms: Option<Ms>,
    /// AutoMix: a transition into `next` the backend reported starting
    /// (`TransitionStarted`) whose handover has not come yet.
    pub transition: Option<ActiveTransition>,
    /// Where the item adopted at the last boundary started: 0 after a gapless
    /// join, B's position at the handover after a mix.
    pub adopted_position_ms: Ms,
}

/// A transition the backend is playing (see `Playback::transition`).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ActiveTransition {
    pub from: QueueKey,
    pub to: QueueKey,
}

impl Playback {
    pub fn position_now(&self, now: EpochMs) -> Ms {
        if self.playing && self.loaded && !self.buffering {
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

    /// What plays, named by its account.
    pub fn track_ref(&self) -> Option<TrackRef> {
        self.track
            .as_ref()
            .map(|t| TrackRef::new(t.server_id.clone(), t.id.clone()))
    }

    pub fn duration_ms(&self) -> Ms {
        self.track.as_ref().map(|t| t.duration_ms).unwrap_or(0)
    }
}

/// This device's playback, remembered so a preview can give it back.
#[derive(Debug, Clone)]
pub(crate) struct PreviewHold {
    pub was_playing: bool,
}

/// The preview session. Not the transport: it never enters the queue,
/// play history, scrobbles or the saved resume position.
#[derive(Debug, Clone)]
pub(crate) struct PreviewSession {
    pub track_id: TrackId,
    pub title: String,
    pub duration_ms: Ms,
    pub position_ms: Ms,
    pub playing: bool,
    /// Set when this device's own playback was paused for the preview.
    /// `None` when a Connect peer is playing: that peer is left alone.
    pub hold: Option<PreviewHold>,
    /// The start offset has been applied. A song whose duration arrives
    /// only once the player opens is seeked to 30% then.
    pub positioned: bool,
}

/// Persisted next to the session document.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub(crate) struct SavedPosition {
    pub key: Option<QueueKey>,
    pub position_ms: Ms,
}

/// A play that reached its scrobble threshold and waits for the session's
/// verdict (`Output::Scrobble`). Kept in `saved_state` under
/// [`pending_scrobbles_key`] from the moment it is asked about until the
/// verdict is recorded (in the same transaction), and asked about again
/// after a restart (connect/mod.rs rule 8), so no play is lost to a restart
/// and none is recorded twice.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PendingScrobble {
    pub server_id: ServerId,
    pub track_id: TrackId,
    /// The play's identity: its start on the session clock (what the engine
    /// is asked about).
    pub started_at: EpochMs,
    /// What the play is recorded and submitted with.
    pub played_at: EpochMs,
    pub played_ms: Ms,
}

impl PendingScrobble {
    /// The same play (the engine's own tolerance for a `startedAt`).
    pub fn is(&self, track: &TrackRef, started_at: EpochMs) -> bool {
        self.server_id == track.source
            && self.track_id == track.id
            && (self.started_at - started_at).abs() < 1.0
    }

    pub fn track(&self) -> TrackRef {
        TrackRef::new(self.server_id.clone(), self.track_id.clone())
    }
}

/// `saved_state` key of a scope's [`PendingScrobble`]s.
pub(crate) fn pending_scrobbles_key(scope: &str) -> String {
    format!("pendingScrobbles:{scope}")
}

/// Bookkeeping for an undo whose inverse runs as compare-and-swap steps.
#[derive(Debug, Clone)]
pub(crate) struct PendingCas {
    pub outcome: crate::undo::CasOutcome,
    pub label: String,
    pub redo: bool,
    /// Rated or loved items the undo touches, announced once it completes.
    pub items: Vec<(crate::api::LibraryItemKind, String)>,
}

/// Track summaries read from the mirror, valid while the mirror's
/// `tracks` generation is unchanged ([`crate::db::Db::tracks_generation`]).
#[derive(Debug, Default)]
pub(crate) struct SummaryCache {
    pub generation: u64,
    pub map: std::collections::HashMap<TrackRef, TrackSummary>,
}

impl SummaryCache {
    /// Upper bound on cached summaries (a few MB at most).
    pub const CAP: usize = 20_000;
}
