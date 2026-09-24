//! The queue reducer: a pure function from `(document, op)` to
//! `(document, effects)`. Every queue mutation in the product goes through
//! here; there is no other path.
//!
//! # Model
//!
//! The queue is derived from four pieces of the [`SessionDocument`]: the
//! context and its unshuffled track list, a shuffle permutation over it (see
//! [`super::shuffle`]), the insertion list, and the cursor. History and the
//! current item are stored [`QueueItem`]s; upcoming context items are derived
//! on demand and carry a synthetic key (`ctx-<index>`) until they become
//! current, at which point they get a real key from the [`Entropy`] source.
//!
//! **Cursor semantics.** `doc.cursor` is the permuted position of the *next*
//! context item to play — the boundary between what has played and what will.
//! `upcoming = perm[cursor..]`. The current item sits outside that range, so
//! pushing it "back to the front of the future" on Previous is exactly
//! `cursor = position(current)`, and this works identically after a jump or a
//! Repeat-All wrap. Next and Previous are inverses; property-tested.
//!
//! **Nothing is consumed.** Next moves the current item to `history` and takes
//! the next item from the future: the insertion list first, then the context
//! at the cursor, then (Repeat All) the context from the top. Previous pops
//! `history` and pushes the current item back where it came from: inserted
//! and autoplay items to the front of `insertions`, context items by moving
//! the cursor. Items flagged `unavailable` are skipped in both directions and
//! only play again when explicitly jumped to (which clears the flag).
//!
//! **Modes.** In `QueueMode::Apple`, Play Next / Play Later go into the
//! insertion list, which plays ahead of the context. In `QueueMode::YouTube`,
//! they are spliced into the context itself: the track is inserted into
//! `context.tracks` (right after the cursor in unshuffled order when shuffle is
//! off; appended when shuffle is on) and every stored `Context { index }`
//! reference at or above the insertion point is renumbered. When shuffled, the
//! explicit `ShuffleState::order` is materialised first so the new item can
//! be placed at the wanted permuted position without rescrambling the rest.
//! The same two helpers (`remove_context_index`, `insert_context_track`) back
//! removal and reordering of context items in both modes. A consequence worth
//! knowing: an ID-referenced context whose tracks were edited this way is
//! snapshotted by ID only, so the edit is not preserved through a saved queue.
//!
//! **Revision.** The revision is bumped exactly once per op that changed the
//! document (compared without revision / `updated_at`); no-ops leave it alone.

use std::collections::{BTreeSet, HashSet};

use crate::api::{
    AutoplayProvider, Command, ContextKind, EpochMs, Ms, PlayContextArgs, QueueContext, QueueItem,
    QueueKey, QueueMode, QueueSource, QueueView, RepeatMode, SavedQueue, ServerId, SessionDocument,
    ShuffleState, SortOrder, TrackId, TrackSummary,
};

use super::document::{bump_revision, same_state};
use super::saved::{self, SavedQueuePolicy};
use super::shuffle::{order_insert_index, order_remove_index, Permutation};
use super::Entropy;

/// Default history cap.
pub const DEFAULT_HISTORY_CAP: usize = 200;
/// Previous within this many milliseconds goes to the previous item; after
/// that it restarts the current one. Symfonium's documented behaviour
/// (support.symfonium.app: "previous only goes back within the first 3 s").
pub const PREVIOUS_RESTART_THRESHOLD_MS: Ms = 3000;

/// Label used for the ad-hoc context created when items are queued onto an
/// empty session or the queue is cleared around the playing track.
pub const ADHOC_QUEUE_LABEL: &str = "Queue";

/// Should a Previous press restart the current track instead of going back?
pub fn previous_should_restart(position_ms: Ms) -> bool {
    position_ms >= PREVIOUS_RESTART_THRESHOLD_MS
}

/// One item handed to [`QueueOp::AppendAutoplay`].
#[derive(Debug, Clone, PartialEq)]
pub struct AutoplayItem {
    pub track_id: TrackId,
    pub provider: AutoplayProvider,
    pub reason: String,
    pub score: Option<f64>,
}

/// Every queue-affecting operation. Mirrors the queue half of
/// [`api::Command`](crate::api::Command) plus the internal ops the actor
/// issues itself (autoplay results, track end, resolved context tracks).
#[derive(Debug, Clone, PartialEq)]
pub enum QueueOp {
    PlayContext {
        args: PlayContextArgs,
    },
    PlayTracks {
        server_id: ServerId,
        track_ids: Vec<TrackId>,
        start_index: u32,
        label: String,
        shuffle: bool,
        save_outgoing: bool,
    },
    PlayNext {
        server_id: ServerId,
        track_ids: Vec<TrackId>,
    },
    PlayLater {
        server_id: ServerId,
        track_ids: Vec<TrackId>,
    },
    JumpToQueueItem {
        key: QueueKey,
    },
    RemoveQueueItems {
        keys: Vec<QueueKey>,
    },
    /// `to_index` is the final index in the combined playing-next + upcoming
    /// list after the move (what a drag-and-drop reports).
    MoveQueueItem {
        key: QueueKey,
        to_index: u32,
    },
    ClearQueue,
    ClearInsertions,
    SetShuffle {
        enabled: bool,
    },
    /// New seed; keeps the current item in place.
    Reshuffle,
    SetRepeat {
        mode: RepeatMode,
    },
    SetAutoplay {
        enabled: bool,
    },
    SetQueueMode {
        mode: QueueMode,
    },
    Next,
    Previous,
    SkipUnavailable {
        key: QueueKey,
    },
    /// `SkipUnavailable` because this device is offline and the item is
    /// neither downloaded nor cached: the same mark, plus the key in the
    /// document's offline set (see [`offline_skipped`]) so that coming back
    /// online can clear it. Local only: not a wire op; the actor pushes the
    /// result as a whole-document replace, which every peer (older builds
    /// included) adopts.
    SkipOffline {
        key: QueueKey,
    },
    /// Clear the marks of every item in the offline set, and the set. Real
    /// failures ([`QueueOp::SkipUnavailable`]) stay marked. Local only, like
    /// [`QueueOp::SkipOffline`].
    ClearOfflineSkips,
    AppendAutoplay {
        items: Vec<AutoplayItem>,
    },
    TrackEnded,
    /// `tracks` carries the re-resolved track list for an ID-referenced
    /// snapshot; `None` restores as stored and asks for resolution.
    RestoreSavedQueue {
        id: String,
        tracks: Option<Vec<TrackId>>,
    },
    /// The actor resolved the context's tracks (after `Effect::ResolveContext`).
    SetContextTracks {
        tracks: Vec<TrackId>,
    },
    /// Explicit "save this queue". With the auto-save cap at 0 an unpinned
    /// explicit save is evicted immediately, so callers normally pin.
    SaveCurrentQueue {
        pinned: bool,
    },
    PinSavedQueue {
        id: String,
        pinned: bool,
    },
    DeleteSavedQueue {
        id: String,
    },
    TouchSavedQueue {
        id: String,
    },
    /// LWW merge of a peer's saved-queue set into ours.
    MergeSavedQueues {
        remote: Vec<SavedQueue>,
    },
}

impl QueueOp {
    /// The reducer op for a public command, or `None` when the command does
    /// not touch the queue.
    pub fn from_command(cmd: &Command) -> Option<QueueOp> {
        Some(match cmd {
            Command::PlayContext { args } => QueueOp::PlayContext { args: args.clone() },
            Command::PlayTracks {
                server_id,
                track_ids,
                start_index,
                label,
                shuffle,
            } => QueueOp::PlayTracks {
                server_id: server_id.clone(),
                track_ids: track_ids.clone(),
                start_index: *start_index,
                label: label.clone(),
                shuffle: *shuffle,
                save_outgoing: true,
            },
            Command::PlayNext {
                server_id,
                track_ids,
            } => QueueOp::PlayNext {
                server_id: server_id.clone(),
                track_ids: track_ids.clone(),
            },
            Command::PlayLater {
                server_id,
                track_ids,
            } => QueueOp::PlayLater {
                server_id: server_id.clone(),
                track_ids: track_ids.clone(),
            },
            Command::JumpToQueueItem { key } => QueueOp::JumpToQueueItem { key: key.clone() },
            Command::RemoveQueueItems { keys } => QueueOp::RemoveQueueItems { keys: keys.clone() },
            Command::MoveQueueItem { key, to_index } => QueueOp::MoveQueueItem {
                key: key.clone(),
                to_index: *to_index,
            },
            Command::ClearQueue => QueueOp::ClearQueue,
            Command::ClearInsertions => QueueOp::ClearInsertions,
            Command::SetShuffle { enabled } => QueueOp::SetShuffle { enabled: *enabled },
            Command::SetRepeat { mode } => QueueOp::SetRepeat { mode: *mode },
            Command::SetAutoplay { enabled } => QueueOp::SetAutoplay { enabled: *enabled },
            Command::SetQueueMode { mode } => QueueOp::SetQueueMode { mode: *mode },
            Command::Next => QueueOp::Next,
            Command::Previous => QueueOp::Previous,
            Command::SkipUnavailable { key } => QueueOp::SkipUnavailable { key: key.clone() },
            Command::RestoreSavedQueue { id } => QueueOp::RestoreSavedQueue {
                id: id.clone(),
                tracks: None,
            },
            Command::PinSavedQueue { id, pinned } => QueueOp::PinSavedQueue {
                id: id.clone(),
                pinned: *pinned,
            },
            Command::DeleteSavedQueue { id } => QueueOp::DeleteSavedQueue { id: id.clone() },
            _ => return None,
        })
    }

    /// Whether the op replaces the whole context (and so snapshots the old one).
    pub fn replaces_context(&self) -> bool {
        matches!(
            self,
            QueueOp::PlayContext { .. }
                | QueueOp::PlayTracks { .. }
                | QueueOp::RestoreSavedQueue { .. }
                | QueueOp::ClearQueue
        )
    }
}

/// What the actor must do after a reduction. The document itself carries the
/// new state; effects only say what needs acting on outside it.
#[derive(Debug, Clone, PartialEq)]
pub enum Effect {
    /// The current item changed (`None` = nothing loaded). Load it and start
    /// at `position_ms`.
    CurrentChanged {
        key: Option<QueueKey>,
        position_ms: Ms,
    },
    /// Restart the current item from the top (repeat one, previous at the
    /// start of the queue, jump to the playing item).
    RestartCurrent,
    /// The queue ran out; the current item stays loaded, playback stops.
    Stopped,
    /// The queue ran out with autoplay on: fetch items, `AppendAutoplay`, then `Next`.
    NeedsAutoplay,
    /// `doc.saved_queues` changed.
    SavedQueuesChanged,
    /// A restored snapshot brought a playback position with it.
    PositionRestore { position_ms: Ms },
    /// A restored context has no track list; resolve it and `SetContextTracks`.
    ResolveContext { context: QueueContext },
    /// The context was replaced (new album / playlist / selection / restore).
    ContextReplaced,
    /// An item flagged unavailable was passed over.
    Skipped { key: QueueKey },
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ReduceError {
    #[error("no queue key {0}")]
    UnknownKey(String),
    #[error("no saved queue {0}")]
    UnknownSavedQueue(String),
    #[error("nothing queued")]
    Empty,
}

/// Environment for one reduction. `position_ms` is the playback position of
/// the current item as the actor knows it (used for snapshots).
pub struct ReduceCtx<'a> {
    pub now: EpochMs,
    pub position_ms: Ms,
    pub history_cap: usize,
    pub saved: SavedQueuePolicy,
    pub entropy: &'a dyn Entropy,
}

/// The queue as the UI sees it, before track summaries are resolved.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct DerivedQueue {
    pub context_label: Option<String>,
    pub history: Vec<QueueItem>,
    pub current: Option<QueueItem>,
    pub playing_next: Vec<QueueItem>,
    pub upcoming: Vec<QueueItem>,
    pub shuffle: bool,
    pub repeat: RepeatMode,
    pub autoplay: bool,
    pub mode: QueueMode,
    pub total_upcoming: u32,
}

impl DerivedQueue {
    /// Resolve into the wire shape. `resolve` maps a track id to its summary
    /// (the actor answers from the mirror; unknown ids get a bare summary).
    pub fn into_view(self, mut resolve: impl FnMut(&TrackId) -> TrackSummary) -> QueueView {
        let mut entry = |item: QueueItem| crate::api::QueueEntry {
            track: resolve(&item.track_id),
            item,
        };
        QueueView {
            context_label: self.context_label,
            history: self.history.into_iter().map(&mut entry).collect(),
            current: self.current.map(&mut entry),
            playing_next: self.playing_next.into_iter().map(&mut entry).collect(),
            upcoming: self.upcoming.into_iter().map(&mut entry).collect(),
            shuffle: self.shuffle,
            repeat: self.repeat,
            autoplay: self.autoplay,
            mode: self.mode,
            total_upcoming: self.total_upcoming,
        }
    }

    /// Every item that is not history, in play order: current, playing next, upcoming.
    pub fn future_keys(&self) -> Vec<QueueKey> {
        self.current
            .iter()
            .chain(self.playing_next.iter())
            .chain(self.upcoming.iter())
            .map(|i| i.key.clone())
            .collect()
    }
}

/// Synthetic key of a not-yet-materialised context item.
pub fn context_key(index: u32) -> QueueKey {
    format!("ctx-{index}")
}

/// Parse a synthetic context key.
pub fn parse_context_key(key: &str) -> Option<u32> {
    key.strip_prefix("ctx-")?.parse().ok()
}

fn tracks(doc: &SessionDocument) -> &[TrackId] {
    doc.context
        .as_ref()
        .map(|c| c.tracks.as_slice())
        .unwrap_or(&[])
}

fn track_count(doc: &SessionDocument) -> usize {
    tracks(doc).len()
}

fn perm(doc: &SessionDocument) -> Permutation {
    Permutation::from_state(doc.shuffle.as_ref(), track_count(doc))
}

/// Derive the visible queue.
pub fn derive(doc: &SessionDocument) -> DerivedQueue {
    let p = perm(doc);
    let n = p.len();
    let start = (doc.cursor as usize).min(n);
    let ts = tracks(doc);
    let upcoming: Vec<QueueItem> = (start..n)
        .filter_map(|pos| p.to_context(pos as u32))
        .map(|index| QueueItem {
            key: context_key(index),
            track_id: ts.get(index as usize).cloned().unwrap_or_default(),
            source: QueueSource::Context { index },
            unavailable: false,
        })
        .collect();
    DerivedQueue {
        context_label: doc.context.as_ref().map(|c| c.label.clone()),
        history: doc.history.clone(),
        current: doc.current.clone(),
        playing_next: doc.insertions.clone(),
        total_upcoming: (doc.insertions.len() + upcoming.len()) as u32,
        upcoming,
        shuffle: doc.shuffle.is_some(),
        repeat: doc.repeat,
        autoplay: doc.autoplay,
        mode: doc.mode,
    }
}

/// Top-level document field (kept in [`SessionDocument::extra`], so the
/// public document shape is unchanged) listing the keys of items marked
/// unavailable only because the owning device was offline. A build that
/// predates it sees plain marks (skipped until jumped to) and carries the
/// field along untouched.
pub const OFFLINE_SKIPPED_FIELD: &str = "offlineSkipped";

/// Keys marked unavailable because the device was offline (see
/// [`OFFLINE_SKIPPED_FIELD`]). Malformed content reads as empty.
pub fn offline_skipped(doc: &SessionDocument) -> BTreeSet<QueueKey> {
    doc.extra
        .get(OFFLINE_SKIPPED_FIELD)
        .and_then(|raw| serde_json::from_str(raw).ok())
        .unwrap_or_default()
}

fn set_offline_skipped(doc: &mut SessionDocument, keys: &BTreeSet<QueueKey>) {
    if keys.is_empty() {
        doc.extra.remove(OFFLINE_SKIPPED_FIELD);
    } else if let Ok(raw) = serde_json::to_string(keys) {
        doc.extra.insert(OFFLINE_SKIPPED_FIELD.into(), raw);
    }
}

/// Keep the offline set to items that are still stored and still marked: a
/// jump clears the mark, history drops old items, removal drops the item.
/// Documents without the field are untouched.
fn prune_offline_skipped(doc: &mut SessionDocument) {
    if !doc.extra.contains_key(OFFLINE_SKIPPED_FIELD) {
        return;
    }
    let marked: HashSet<&str> = doc
        .current
        .iter()
        .chain(doc.history.iter())
        .chain(doc.insertions.iter())
        .filter(|i| i.unavailable)
        .map(|i| i.key.as_str())
        .collect();
    let mut keys = offline_skipped(doc);
    keys.retain(|k| marked.contains(k.as_str()));
    set_offline_skipped(doc, &keys);
}

/// Reduce one op. Never panics, whatever the document or op.
pub fn reduce(
    doc: &SessionDocument,
    op: QueueOp,
    ctx: &ReduceCtx<'_>,
) -> Result<(SessionDocument, Vec<Effect>), ReduceError> {
    let mut next = doc.clone();
    let mut effects = Vec::new();
    let mut start_position: Ms = 0;
    let mut r = Reducer {
        doc: &mut next,
        ctx,
        effects: &mut effects,
        start_position: &mut start_position,
    };
    r.apply(op)?;
    prune_offline_skipped(&mut next);
    if next.current.as_ref().map(|c| &c.key) != doc.current.as_ref().map(|c| &c.key) {
        effects.insert(
            0,
            Effect::CurrentChanged {
                key: next.current.as_ref().map(|c| c.key.clone()),
                position_ms: start_position,
            },
        );
    }
    if next.saved_queues != doc.saved_queues {
        effects.push(Effect::SavedQueuesChanged);
    }
    if !same_state(doc, &next) {
        bump_revision(&mut next, ctx.now);
    }
    Ok((next, effects))
}

struct Reducer<'a, 'c> {
    doc: &'a mut SessionDocument,
    ctx: &'a ReduceCtx<'c>,
    effects: &'a mut Vec<Effect>,
    start_position: &'a mut Ms,
}

/// Where a Next would take an item from.
enum Candidate {
    Insertion,
    Context { position: u32 },
    Wrap,
}

impl Reducer<'_, '_> {
    fn apply(&mut self, op: QueueOp) -> Result<(), ReduceError> {
        match op {
            QueueOp::PlayContext { args } => self.play_context(
                args.context,
                args.start_index,
                args.shuffle,
                args.save_outgoing,
            ),
            QueueOp::PlayTracks {
                server_id,
                track_ids,
                start_index,
                label,
                shuffle,
                save_outgoing,
            } => {
                let context = QueueContext {
                    server_id,
                    kind: ContextKind::AdHoc {
                        label: label.clone(),
                    },
                    label,
                    sort: SortOrder::Default,
                    tracks: track_ids,
                };
                self.play_context(context, Some(start_index), shuffle, save_outgoing)
            }
            QueueOp::PlayNext {
                server_id,
                track_ids,
            } => self.queue_tracks(server_id, track_ids, true),
            QueueOp::PlayLater {
                server_id,
                track_ids,
            } => self.queue_tracks(server_id, track_ids, false),
            QueueOp::JumpToQueueItem { key } => self.jump(&key),
            QueueOp::RemoveQueueItems { keys } => self.remove(&keys),
            QueueOp::MoveQueueItem { key, to_index } => self.move_item(&key, to_index as usize),
            QueueOp::ClearQueue => self.clear_queue(),
            QueueOp::ClearInsertions => {
                self.doc.insertions.clear();
                Ok(())
            }
            QueueOp::SetShuffle { enabled } => {
                if enabled {
                    if self.doc.shuffle.is_none() {
                        self.shuffle_on();
                    }
                } else {
                    self.shuffle_off();
                }
                Ok(())
            }
            QueueOp::Reshuffle => {
                self.shuffle_on();
                Ok(())
            }
            QueueOp::SetRepeat { mode } => {
                self.doc.repeat = mode;
                Ok(())
            }
            QueueOp::SetAutoplay { enabled } => {
                self.doc.autoplay = enabled;
                Ok(())
            }
            QueueOp::SetQueueMode { mode } => {
                self.doc.mode = mode;
                if mode == QueueMode::YouTube {
                    self.fold_insertions();
                }
                Ok(())
            }
            QueueOp::Next => {
                if !self.advance() {
                    self.end_of_queue();
                }
                Ok(())
            }
            QueueOp::Previous => self.previous(None),
            QueueOp::SkipUnavailable { key } => {
                // A real failure: whatever the item was skipped for before,
                // it is not an offline mark any more.
                let mut offline = offline_skipped(self.doc);
                if offline.remove(&key) {
                    set_offline_skipped(self.doc, &offline);
                }
                self.skip_unavailable(&key)
            }
            QueueOp::SkipOffline { key } => {
                self.skip_unavailable(&key)?;
                if self.is_marked(&key) {
                    let mut offline = offline_skipped(self.doc);
                    offline.insert(key);
                    set_offline_skipped(self.doc, &offline);
                }
                Ok(())
            }
            QueueOp::ClearOfflineSkips => {
                let offline = offline_skipped(self.doc);
                let d = &mut *self.doc;
                for item in d
                    .current
                    .iter_mut()
                    .chain(d.history.iter_mut())
                    .chain(d.insertions.iter_mut())
                {
                    if offline.contains(&item.key) {
                        item.unavailable = false;
                    }
                }
                set_offline_skipped(self.doc, &BTreeSet::new());
                Ok(())
            }
            QueueOp::AppendAutoplay { items } => {
                for it in items {
                    let key = self.ctx.entropy.next_key();
                    self.doc.insertions.push(QueueItem {
                        key,
                        track_id: it.track_id,
                        source: QueueSource::Autoplay {
                            provider: it.provider,
                            reason: it.reason,
                            score: it.score,
                        },
                        unavailable: false,
                    });
                }
                Ok(())
            }
            QueueOp::TrackEnded => {
                if self.doc.repeat == RepeatMode::One && self.doc.current.is_some() {
                    self.effects.push(Effect::RestartCurrent);
                } else if !self.advance() {
                    self.end_of_queue();
                }
                Ok(())
            }
            QueueOp::RestoreSavedQueue { id, tracks } => self.restore(&id, tracks),
            QueueOp::SetContextTracks { tracks } => {
                if let Some(c) = self.doc.context.as_mut() {
                    c.tracks = tracks;
                    self.reconcile();
                }
                Ok(())
            }
            QueueOp::SaveCurrentQueue { pinned } => {
                let id = self.ctx.entropy.next_key();
                if let Some(mut s) =
                    saved::snapshot(self.doc, self.ctx.position_ms, self.ctx.now, id)
                {
                    s.pinned = pinned;
                    saved::upsert(&mut self.doc.saved_queues, s, &self.ctx.saved, self.ctx.now);
                    // An explicit pin must win over a pre-existing unpinned entry.
                    if pinned {
                        if let Some(q) = self.doc.saved_queues.iter_mut().find(|q| {
                            self.doc.context.as_ref().map(saved::context_identity)
                                == Some(saved::identity_of(q))
                        }) {
                            q.pinned = true;
                        }
                    }
                }
                Ok(())
            }
            QueueOp::PinSavedQueue { id, pinned } => {
                let q = self
                    .doc
                    .saved_queues
                    .iter_mut()
                    .find(|q| q.id == id)
                    .ok_or(ReduceError::UnknownSavedQueue(id))?;
                q.pinned = pinned;
                q.updated_at = self.ctx.now;
                saved::enforce(&mut self.doc.saved_queues, &self.ctx.saved, self.ctx.now);
                Ok(())
            }
            QueueOp::DeleteSavedQueue { id } => {
                let before = self.doc.saved_queues.len();
                self.doc.saved_queues.retain(|q| q.id != id);
                if self.doc.saved_queues.len() == before {
                    return Err(ReduceError::UnknownSavedQueue(id));
                }
                Ok(())
            }
            QueueOp::TouchSavedQueue { id } => {
                let q = self
                    .doc
                    .saved_queues
                    .iter_mut()
                    .find(|q| q.id == id)
                    .ok_or(ReduceError::UnknownSavedQueue(id))?;
                q.last_interacted_at = self.ctx.now;
                q.updated_at = self.ctx.now;
                saved::enforce(&mut self.doc.saved_queues, &self.ctx.saved, self.ctx.now);
                Ok(())
            }
            QueueOp::MergeSavedQueues { remote } => {
                self.doc.saved_queues = saved::merge_saved_queues(&self.doc.saved_queues, &remote);
                saved::enforce(&mut self.doc.saved_queues, &self.ctx.saved, self.ctx.now);
                Ok(())
            }
        }
    }

    // -- helpers -----------------------------------------------------------

    fn n(&self) -> usize {
        track_count(self.doc)
    }

    fn perm(&self) -> Permutation {
        perm(self.doc)
    }

    fn fresh_context_item(&self, index: u32) -> Option<QueueItem> {
        let track_id = tracks(self.doc).get(index as usize)?.clone();
        Some(QueueItem {
            key: self.ctx.entropy.next_key(),
            track_id,
            source: QueueSource::Context { index },
            unavailable: false,
        })
    }

    fn push_history(&mut self, item: QueueItem) {
        self.doc.history.push(item);
        let cap = self.ctx.history_cap;
        if self.doc.history.len() > cap {
            let excess = self.doc.history.len() - cap;
            self.doc.history.drain(..excess);
        }
    }

    /// Unshuffled index of the context item that played most recently
    /// (the current one if it is a context item, else the one before the cursor).
    fn last_played_context_index(&self) -> Option<u32> {
        if let Some(QueueItem {
            source: QueueSource::Context { index },
            ..
        }) = &self.doc.current
        {
            return Some(*index);
        }
        if self.doc.cursor > 0 {
            return self.perm().to_context(self.doc.cursor - 1);
        }
        None
    }

    /// Materialise the explicit order so it can be edited.
    fn ensure_order(&mut self) {
        let n = self.n();
        let p = self.perm();
        if let Some(s) = self.doc.shuffle.as_mut() {
            if s.order.as_ref().map(|o| o.len() != n).unwrap_or(true) {
                s.order = Some(p.into_order());
            }
        }
    }

    fn shift_refs(&mut self, from_index: u32, delta: i64, removed: Option<u32>) {
        let fix = |item: &mut QueueItem| {
            if let QueueSource::Context { index } = item.source {
                if Some(index) == removed {
                    // The track it referenced is gone from the context; keep the
                    // item playable by its track id.
                    item.source = QueueSource::Inserted;
                } else if index >= from_index {
                    let new = (index as i64 + delta).max(0) as u32;
                    item.source = QueueSource::Context { index: new };
                }
            }
        };
        if let Some(c) = self.doc.current.as_mut() {
            fix(c);
        }
        for h in self.doc.history.iter_mut() {
            fix(h);
        }
        for i in self.doc.insertions.iter_mut() {
            fix(i);
        }
    }

    /// Remove `index` from the context, renumbering everything above it.
    fn remove_context_index(&mut self, index: u32) {
        let n = self.n();
        if index as usize >= n {
            return;
        }
        let position = self.perm().to_position(index);
        if self.doc.shuffle.is_some() {
            self.ensure_order();
            if let Some(s) = self.doc.shuffle.as_mut() {
                if let Some(o) = s.order.as_mut() {
                    order_remove_index(o, index);
                }
                s.anchor = match s.anchor {
                    Some(a) if a == index => None,
                    Some(a) if a > index => Some(a - 1),
                    other => other,
                };
            }
        }
        if let Some(c) = self.doc.context.as_mut() {
            c.tracks.remove(index as usize);
        }
        self.shift_refs(index + 1, -1, Some(index));
        if let Some(p) = position {
            if p < self.doc.cursor {
                self.doc.cursor -= 1;
            }
        }
        self.clamp_cursor();
    }

    /// Insert a track at unshuffled `index`, showing at permuted `position`.
    fn insert_context_track(&mut self, index: u32, position: u32, track_id: TrackId) {
        let n = self.n() as u32;
        let index = index.min(n);
        let position = position.min(n);
        if self.doc.shuffle.is_some() {
            self.ensure_order();
            if let Some(s) = self.doc.shuffle.as_mut() {
                if let Some(o) = s.order.as_mut() {
                    order_insert_index(o, index, position as usize);
                }
                if let Some(a) = s.anchor {
                    if a >= index {
                        s.anchor = Some(a + 1);
                    }
                }
            }
        }
        if let Some(c) = self.doc.context.as_mut() {
            c.tracks.insert(index as usize, track_id);
        }
        self.shift_refs(index, 1, None);
        if position < self.doc.cursor {
            self.doc.cursor += 1;
        }
    }

    fn clamp_cursor(&mut self) {
        let n = self.n() as u32;
        if self.doc.cursor > n {
            self.doc.cursor = n;
        }
    }

    /// After the track list changed under stored items: fix indices whose
    /// track no longer matches, drop a stale explicit order, clamp.
    fn reconcile(&mut self) {
        let n = self.n();
        let ts: Vec<TrackId> = tracks(self.doc).to_vec();
        let fix = |item: &mut QueueItem| {
            if let QueueSource::Context { index } = item.source {
                if ts.get(index as usize) != Some(&item.track_id) {
                    match ts.iter().position(|t| *t == item.track_id) {
                        Some(i) => item.source = QueueSource::Context { index: i as u32 },
                        None => item.source = QueueSource::Inserted,
                    }
                }
            }
        };
        if let Some(c) = self.doc.current.as_mut() {
            fix(c);
        }
        self.doc.history.iter_mut().for_each(fix);
        self.doc.insertions.iter_mut().for_each(fix);
        if let Some(s) = self.doc.shuffle.as_mut() {
            if s.order.as_ref().map(|o| o.len() != n).unwrap_or(false) {
                s.order = None;
            }
            if s.anchor.map(|a| a as usize >= n).unwrap_or(false) {
                s.anchor = None;
            }
        }
        self.clamp_cursor();
    }

    fn snapshot_outgoing(&mut self) {
        let id = self.ctx.entropy.next_key();
        if let Some(s) = saved::snapshot(self.doc, self.ctx.position_ms, self.ctx.now, id) {
            saved::upsert(&mut self.doc.saved_queues, s, &self.ctx.saved, self.ctx.now);
        }
    }

    // -- context replacement -------------------------------------------------

    fn play_context(
        &mut self,
        context: QueueContext,
        start_index: Option<u32>,
        shuffle: bool,
        save_outgoing: bool,
    ) -> Result<(), ReduceError> {
        if save_outgoing {
            self.snapshot_outgoing();
        }
        let n = context.tracks.len() as u32;
        let start = start_index.map(|s| if n == 0 { 0 } else { s.min(n - 1) });
        self.doc.context = Some(context);
        self.doc.history.clear();
        self.doc.insertions.clear();
        self.doc.current = None;
        self.doc.cursor = 0;
        self.doc.shuffle = if shuffle {
            Some(ShuffleState {
                seed: self.ctx.entropy.next_seed(),
                anchor: start.filter(|_| n > 0),
                order: None,
            })
        } else {
            None
        };
        if n > 0 {
            let p = self.perm();
            let position = match (shuffle, start) {
                (true, _) => 0,
                (false, Some(s)) => s,
                (false, None) => 0,
            };
            if let Some(index) = p.to_context(position) {
                self.doc.current = self.fresh_context_item(index);
                self.doc.cursor = position + 1;
            }
        }
        self.effects.push(Effect::ContextReplaced);
        Ok(())
    }

    fn clear_queue(&mut self) -> Result<(), ReduceError> {
        self.snapshot_outgoing();
        let server_id = self
            .doc
            .context
            .as_ref()
            .map(|c| c.server_id.clone())
            .unwrap_or_default();
        self.doc.history.clear();
        self.doc.insertions.clear();
        self.doc.shuffle = None;
        match self.doc.current.as_mut() {
            Some(cur) => {
                cur.source = QueueSource::Context { index: 0 };
                let tracks = vec![cur.track_id.clone()];
                self.doc.context = Some(QueueContext {
                    server_id,
                    kind: ContextKind::AdHoc {
                        label: ADHOC_QUEUE_LABEL.into(),
                    },
                    label: ADHOC_QUEUE_LABEL.into(),
                    sort: SortOrder::Default,
                    tracks,
                });
                self.doc.cursor = 1;
            }
            None => {
                self.doc.context = None;
                self.doc.cursor = 0;
            }
        }
        self.effects.push(Effect::ContextReplaced);
        Ok(())
    }

    fn restore(&mut self, id: &str, tracks: Option<Vec<TrackId>>) -> Result<(), ReduceError> {
        let pos = self
            .doc
            .saved_queues
            .iter()
            .position(|q| q.id == id)
            .ok_or_else(|| ReduceError::UnknownSavedQueue(id.into()))?;
        let saved_entry = self.doc.saved_queues[pos].clone();
        self.snapshot_outgoing();
        // The entry may have moved (or been merged) by the snapshot's dedupe.
        if let Some(q) = self
            .doc
            .saved_queues
            .iter_mut()
            .find(|q| saved::identity_of(q) == saved::identity_of(&saved_entry))
        {
            q.last_interacted_at = self.ctx.now;
            q.updated_at = self.ctx.now;
        }
        let mut context = saved_entry.context.clone();
        if let Some(t) = tracks {
            context.tracks = t;
        }
        let unresolved = saved::is_id_referenced(&context.kind) && context.tracks.is_empty();
        self.doc.context = Some(context.clone());
        self.doc.cursor = saved_entry.cursor;
        self.doc.current = saved_entry.current.clone();
        self.doc.history = saved_entry.history.clone();
        self.doc.insertions = saved_entry.insertions.clone();
        self.doc.shuffle = saved_entry.shuffle.clone();
        self.doc.repeat = saved_entry.repeat;
        self.dedupe_keys();
        if unresolved {
            self.effects.push(Effect::ResolveContext { context });
        } else {
            self.reconcile();
        }
        *self.start_position = saved_entry.position_ms;
        self.effects.push(Effect::ContextReplaced);
        self.effects.push(Effect::PositionRestore {
            position_ms: saved_entry.position_ms,
        });
        Ok(())
    }

    /// A snapshot written by another build could carry duplicate keys; make
    /// them unique so the invariant holds for everything the reducer emits.
    fn dedupe_keys(&mut self) {
        let mut seen: HashSet<QueueKey> = HashSet::new();
        let entropy = self.ctx.entropy;
        let mut fix = |item: &mut QueueItem| {
            while !seen.insert(item.key.clone()) || parse_context_key(&item.key).is_some() {
                item.key = entropy.next_key();
            }
        };
        self.doc.history.iter_mut().for_each(&mut fix);
        if let Some(c) = self.doc.current.as_mut() {
            fix(c);
        }
        self.doc.insertions.iter_mut().for_each(&mut fix);
    }

    // -- queueing ----------------------------------------------------------

    fn queue_tracks(
        &mut self,
        server_id: ServerId,
        track_ids: Vec<TrackId>,
        next: bool,
    ) -> Result<(), ReduceError> {
        if track_ids.is_empty() {
            return Ok(());
        }
        if self.doc.context.is_none() {
            // Nothing to queue behind: this becomes the queue and starts playing.
            let context = QueueContext {
                server_id,
                kind: ContextKind::AdHoc {
                    label: ADHOC_QUEUE_LABEL.into(),
                },
                label: ADHOC_QUEUE_LABEL.into(),
                sort: SortOrder::Default,
                tracks: track_ids,
            };
            return self.play_context(context, Some(0), false, false);
        }
        match self.doc.mode {
            QueueMode::Apple => {
                let items: Vec<QueueItem> = track_ids
                    .into_iter()
                    .map(|t| QueueItem {
                        key: self.ctx.entropy.next_key(),
                        track_id: t,
                        source: QueueSource::Inserted,
                        unavailable: false,
                    })
                    .collect();
                let at = if next {
                    0
                } else {
                    // After the last explicitly queued item, ahead of any autoplay tail.
                    self.doc
                        .insertions
                        .iter()
                        .rposition(|i| matches!(i.source, QueueSource::Inserted))
                        .map(|p| p + 1)
                        .unwrap_or(0)
                };
                let tail = self.doc.insertions.split_off(at);
                self.doc.insertions.extend(items);
                self.doc.insertions.extend(tail);
            }
            QueueMode::YouTube => {
                for (k, t) in track_ids.into_iter().enumerate() {
                    let n = self.n() as u32;
                    let (index, position) = if next {
                        let position = self.doc.cursor + k as u32;
                        let index = if self.doc.shuffle.is_none() {
                            position
                        } else {
                            n
                        };
                        (index, position)
                    } else {
                        (n, n)
                    };
                    self.insert_context_track(index, position, t);
                }
            }
        }
        Ok(())
    }

    /// YouTube mode has no separate insertion list: splice pending Play Next
    /// items into the context after the cursor. Autoplay items stay.
    fn fold_insertions(&mut self) {
        if self.doc.context.is_none() {
            return;
        }
        let pending: Vec<QueueItem> = self
            .doc
            .insertions
            .iter()
            .filter(|i| matches!(i.source, QueueSource::Inserted))
            .cloned()
            .collect();
        if pending.is_empty() {
            return;
        }
        self.doc
            .insertions
            .retain(|i| !matches!(i.source, QueueSource::Inserted));
        for (k, item) in pending.into_iter().enumerate() {
            let n = self.n() as u32;
            let position = self.doc.cursor + k as u32;
            let index = if self.doc.shuffle.is_none() {
                position
            } else {
                n
            };
            self.insert_context_track(index, position, item.track_id);
        }
    }

    // -- shuffle -------------------------------------------------------------

    fn shuffle_on(&mut self) {
        let anchor = self
            .last_played_context_index()
            .filter(|&a| (a as usize) < self.n());
        self.doc.shuffle = Some(ShuffleState {
            seed: self.ctx.entropy.next_seed(),
            anchor,
            order: None,
        });
        self.doc.cursor = if anchor.is_some() { 1 } else { 0 };
        self.clamp_cursor();
    }

    fn shuffle_off(&mut self) {
        if self.doc.shuffle.is_none() {
            return;
        }
        let last = self.last_played_context_index();
        self.doc.shuffle = None;
        self.doc.cursor = last.map(|i| i + 1).unwrap_or(0);
        self.clamp_cursor();
    }

    // -- navigation ----------------------------------------------------------

    fn candidate(&self) -> Option<Candidate> {
        if !self.doc.insertions.is_empty() {
            return Some(Candidate::Insertion);
        }
        let n = self.n() as u32;
        if self.doc.cursor < n {
            return Some(Candidate::Context {
                position: self.doc.cursor,
            });
        }
        if self.doc.repeat == RepeatMode::All && n > 0 {
            return Some(Candidate::Wrap);
        }
        None
    }

    /// Move to the next playable item. Returns false (and leaves the document
    /// untouched) when there is none.
    fn advance(&mut self) -> bool {
        let snapshot = self.doc.clone();
        let effects_len = self.effects.len();
        // The outgoing item precedes anything skipped on the way to the next one.
        if let Some(cur) = self.doc.current.take() {
            self.push_history(cur);
        }
        loop {
            let Some(c) = self.candidate() else {
                *self.doc = snapshot;
                self.effects.truncate(effects_len);
                return false;
            };
            let item = match c {
                Candidate::Insertion => {
                    let item = self.doc.insertions.remove(0);
                    if item.unavailable {
                        self.effects.push(Effect::Skipped {
                            key: item.key.clone(),
                        });
                        self.push_history(item);
                        continue;
                    }
                    item
                }
                Candidate::Context { position } => {
                    let Some(item) = self
                        .perm()
                        .to_context(position)
                        .and_then(|i| self.fresh_context_item(i))
                    else {
                        self.doc.cursor = self.n() as u32;
                        continue;
                    };
                    self.doc.cursor = position + 1;
                    item
                }
                Candidate::Wrap => {
                    let Some(item) = self
                        .perm()
                        .to_context(0)
                        .and_then(|i| self.fresh_context_item(i))
                    else {
                        *self.doc = snapshot;
                        self.effects.truncate(effects_len);
                        return false;
                    };
                    self.doc.cursor = 1;
                    item
                }
            };
            self.doc.current = Some(item);
            return true;
        }
    }

    fn end_of_queue(&mut self) {
        if self.doc.autoplay && self.doc.current.is_some() {
            self.effects.push(Effect::NeedsAutoplay);
        } else {
            self.effects.push(Effect::Stopped);
        }
    }

    /// Push the current item back to the front of the future, preserving its
    /// source. A context item normally goes back by moving the cursor to its
    /// position; if the most recent context item in history sits at or after
    /// that position, the current item was reached by a Repeat-All wrap and
    /// the cursor goes back to the end instead, so the wrap replays.
    fn push_current_to_future(&mut self) {
        let Some(cur) = self.doc.current.take() else {
            return;
        };
        match cur.source {
            QueueSource::Context { index } => {
                let p = self.perm();
                if let Some(pos) = p.to_position(index) {
                    let last_played = self.doc.history.iter().rev().find_map(|h| match h.source {
                        QueueSource::Context { index } => p.to_position(index),
                        _ => None,
                    });
                    let wrapped = last_played.map(|q| q >= pos).unwrap_or(false);
                    self.doc.cursor = if wrapped { p.len() as u32 } else { pos };
                }
            }
            QueueSource::Inserted | QueueSource::Autoplay { .. } => {
                self.doc.insertions.insert(0, cur);
            }
        }
    }

    /// Walk history back one playable item (or to `target`, which is allowed
    /// even when flagged).
    fn previous(&mut self, target: Option<&str>) -> Result<(), ReduceError> {
        let snapshot = self.doc.clone();
        while !self.doc.history.is_empty() {
            self.push_current_to_future();
            let Some(item) = self.doc.history.pop() else {
                break;
            };
            let is_target = target == Some(item.key.as_str());
            if item.unavailable && !is_target {
                // Skipped going forward, skipped going back.
                self.doc.current = Some(item);
                self.push_current_to_future();
                continue;
            }
            let mut item = item;
            if is_target {
                item.unavailable = false;
            }
            if let QueueSource::Context { index } = item.source {
                // Its own position is behind the cursor by construction; keep the
                // cursor where the outgoing item put it unless that is stale.
                if self.perm().to_position(index).is_none() {
                    item.source = QueueSource::Inserted;
                }
            }
            self.doc.current = Some(item);
            if target.is_some() && !is_target {
                continue;
            }
            return Ok(());
        }
        if target.is_some() {
            *self.doc = snapshot;
            return Err(ReduceError::UnknownKey(target.unwrap_or_default().into()));
        }
        // History exhausted (or capped away): fall back to context order.
        *self.doc = snapshot;
        let n = self.n() as u32;
        match self.doc.current.clone() {
            Some(QueueItem {
                source: QueueSource::Context { index },
                ..
            }) => {
                let p = self.perm();
                match p.to_position(index) {
                    Some(q) if q > 0 => {
                        if let Some(item) =
                            p.to_context(q - 1).and_then(|i| self.fresh_context_item(i))
                        {
                            self.doc.current = Some(item);
                            self.doc.cursor = q;
                        }
                    }
                    _ => self.effects.push(Effect::RestartCurrent),
                }
            }
            Some(_) if n > 0 && self.doc.cursor > 0 => {
                let q = self.doc.cursor - 1;
                if let Some(item) = self
                    .perm()
                    .to_context(q)
                    .and_then(|i| self.fresh_context_item(i))
                {
                    self.push_current_to_future();
                    self.doc.current = Some(item);
                    self.doc.cursor = q + 1;
                } else {
                    self.effects.push(Effect::RestartCurrent);
                }
            }
            Some(_) => self.effects.push(Effect::RestartCurrent),
            None => {}
        }
        Ok(())
    }

    fn jump(&mut self, key: &str) -> Result<(), ReduceError> {
        if self.doc.current.as_ref().map(|c| c.key.as_str()) == Some(key) {
            if let Some(c) = self.doc.current.as_mut() {
                c.unavailable = false;
            }
            self.effects.push(Effect::RestartCurrent);
            return Ok(());
        }
        if self.doc.history.iter().any(|h| h.key == key) {
            return self.previous(Some(key));
        }
        if let Some(i) = self.doc.insertions.iter().position(|it| it.key == key) {
            let mut item = self.doc.insertions.remove(i);
            item.unavailable = false;
            if let Some(cur) = self.doc.current.take() {
                self.push_history(cur);
            }
            self.doc.current = Some(item);
            return Ok(());
        }
        if let Some(index) = parse_context_key(key) {
            let p = self.perm();
            if let Some(position) = p.to_position(index) {
                if position >= self.doc.cursor {
                    if let Some(item) = self.fresh_context_item(index) {
                        if let Some(cur) = self.doc.current.take() {
                            self.push_history(cur);
                        }
                        self.doc.current = Some(item);
                        self.doc.cursor = position + 1;
                        return Ok(());
                    }
                }
            }
        }
        Err(ReduceError::UnknownKey(key.into()))
    }

    /// A stored item with this key carries the unavailable mark.
    fn is_marked(&self, key: &str) -> bool {
        let d = &*self.doc;
        d.current
            .iter()
            .chain(d.history.iter())
            .chain(d.insertions.iter())
            .any(|i| i.key == key && i.unavailable)
    }

    fn skip_unavailable(&mut self, key: &str) -> Result<(), ReduceError> {
        if self.doc.current.as_ref().map(|c| c.key.as_str()) == Some(key) {
            if let Some(c) = self.doc.current.as_mut() {
                c.unavailable = true;
            }
            self.effects.push(Effect::Skipped { key: key.into() });
            if !self.advance() {
                self.end_of_queue();
            }
            return Ok(());
        }
        if let Some(h) = self.doc.history.iter_mut().find(|h| h.key == key) {
            h.unavailable = true;
            return Ok(());
        }
        if let Some(i) = self.doc.insertions.iter_mut().find(|i| i.key == key) {
            i.unavailable = true;
            return Ok(());
        }
        if let Some(index) = parse_context_key(key) {
            // Not materialised, so nowhere to keep the flag: drop it from the queue.
            if (index as usize) < self.n() {
                self.remove_context_index(index);
                self.effects.push(Effect::Skipped { key: key.into() });
                return Ok(());
            }
        }
        Err(ReduceError::UnknownKey(key.into()))
    }

    // -- editing ---------------------------------------------------------

    fn remove(&mut self, keys: &[QueueKey]) -> Result<(), ReduceError> {
        let wanted: HashSet<&str> = keys.iter().map(|k| k.as_str()).collect();
        let mut found = false;
        let before = self.doc.history.len();
        self.doc
            .history
            .retain(|h| !wanted.contains(h.key.as_str()));
        found |= self.doc.history.len() != before;
        let before = self.doc.insertions.len();
        self.doc
            .insertions
            .retain(|i| !wanted.contains(i.key.as_str()));
        found |= self.doc.insertions.len() != before;

        let mut context_indices: Vec<u32> =
            keys.iter().filter_map(|k| parse_context_key(k)).collect();
        let n = self.n();
        let p = self.perm();
        // Only items actually visible as upcoming are removable this way.
        context_indices.retain(|&i| {
            (i as usize) < n
                && p.to_position(i)
                    .map(|pos| pos >= self.doc.cursor)
                    .unwrap_or(false)
        });

        if let Some(cur) = self.doc.current.clone() {
            if wanted.contains(cur.key.as_str()) {
                found = true;
                let removed_index = match cur.source {
                    QueueSource::Context { index } => Some(index),
                    _ => None,
                };
                // Advance without keeping the removed item in history.
                self.doc.current = None;
                if !self.advance() {
                    self.effects.push(Effect::Stopped);
                }
                if let Some(i) = removed_index {
                    // The new current may reference an index above it; renumbering handles that.
                    self.remove_context_index(i);
                    context_indices = context_indices
                        .into_iter()
                        .filter(|&x| x != i)
                        .map(|x| if x > i { x - 1 } else { x })
                        .collect();
                }
            }
        }
        if !context_indices.is_empty() {
            found = true;
            context_indices.sort_unstable();
            context_indices.dedup();
            for i in context_indices.into_iter().rev() {
                self.remove_context_index(i);
            }
        }
        if found {
            Ok(())
        } else {
            Err(ReduceError::UnknownKey(keys.join(",")))
        }
    }

    fn move_item(&mut self, key: &str, to_index: usize) -> Result<(), ReduceError> {
        // Take the item out.
        let (track_id, source): (TrackId, QueueSource) =
            if let Some(i) = self.doc.insertions.iter().position(|it| it.key == key) {
                let it = self.doc.insertions.remove(i);
                (it.track_id, it.source)
            } else if let Some(index) = parse_context_key(key) {
                let p = self.perm();
                let visible = (index as usize) < self.n()
                    && p.to_position(index)
                        .map(|pos| pos >= self.doc.cursor)
                        .unwrap_or(false);
                if !visible {
                    return Err(ReduceError::UnknownKey(key.into()));
                }
                let track_id = tracks(self.doc)[index as usize].clone();
                self.remove_context_index(index);
                (track_id, QueueSource::Context { index })
            } else {
                return Err(ReduceError::UnknownKey(key.into()));
            };
        // Place it.
        let ins_len = self.doc.insertions.len();
        let ctx_len = (self.n() as u32).saturating_sub(self.doc.cursor) as usize;
        let has_context = self.doc.context.is_some();
        let target = to_index.min(ins_len + ctx_len);
        if target < ins_len || !has_context {
            let source = match source {
                QueueSource::Autoplay { .. } => source,
                _ => QueueSource::Inserted,
            };
            let at = target.min(ins_len);
            self.doc.insertions.insert(
                at,
                QueueItem {
                    key: self.ctx.entropy.next_key(),
                    track_id,
                    source,
                    unavailable: false,
                },
            );
        } else {
            let position = self.doc.cursor + (target - ins_len) as u32;
            let index = if self.doc.shuffle.is_none() {
                position
            } else {
                self.n() as u32
            };
            self.insert_context_track(index, position, track_id);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::{QueueEntry, RatingTarget};
    use crate::session::document::{new_document, validate};
    use crate::session::DeterministicEntropy;

    fn ctx<'a>(e: &'a DeterministicEntropy, now: f64, position_ms: Ms) -> ReduceCtx<'a> {
        ReduceCtx {
            now,
            position_ms,
            history_cap: DEFAULT_HISTORY_CAP,
            saved: SavedQueuePolicy::default(),
            entropy: e,
        }
    }

    fn album(id: &str, n: usize) -> QueueContext {
        QueueContext {
            server_id: "srv".into(),
            kind: ContextKind::Album { id: id.into() },
            label: format!("Album {id}"),
            sort: SortOrder::Default,
            tracks: (0..n).map(|i| format!("{id}-t{i}")).collect(),
        }
    }

    fn play(
        doc: &SessionDocument,
        e: &DeterministicEntropy,
        context: QueueContext,
        start: Option<u32>,
        shuffle: bool,
    ) -> SessionDocument {
        let args = PlayContextArgs {
            context,
            start_index: start,
            shuffle,
            save_outgoing: true,
        };
        reduce(doc, QueueOp::PlayContext { args }, &ctx(e, 1.0, 0))
            .unwrap()
            .0
    }

    fn step(
        doc: &SessionDocument,
        e: &DeterministicEntropy,
        op: QueueOp,
    ) -> (SessionDocument, Vec<Effect>) {
        let out = reduce(doc, op, &ctx(e, 2.0, 5000)).unwrap();
        validate(&out.0).unwrap();
        out
    }

    fn cur_track(doc: &SessionDocument) -> &str {
        doc.current
            .as_ref()
            .map(|c| c.track_id.as_str())
            .unwrap_or("")
    }

    fn upcoming_tracks(doc: &SessionDocument) -> Vec<String> {
        derive(doc)
            .upcoming
            .into_iter()
            .map(|i| i.track_id)
            .collect()
    }

    fn playing_next_tracks(doc: &SessionDocument) -> Vec<String> {
        derive(doc)
            .playing_next
            .into_iter()
            .map(|i| i.track_id)
            .collect()
    }

    #[test]
    fn play_context_and_walk() {
        let e = DeterministicEntropy::new(1);
        let d0 = new_document("s", "id".into(), 0.0);
        let d1 = play(&d0, &e, album("a", 3), Some(1), false);
        assert_eq!(d1.revision, 1);
        assert_eq!(cur_track(&d1), "a-t1");
        assert_eq!(upcoming_tracks(&d1), vec!["a-t2"]);
        let (d2, fx) = step(&d1, &e, QueueOp::Next);
        assert_eq!(cur_track(&d2), "a-t2");
        assert!(matches!(fx[0], Effect::CurrentChanged { .. }));
        assert_eq!(d2.history.len(), 1);
        let (d3, fx) = step(&d2, &e, QueueOp::Next);
        assert_eq!(fx, vec![Effect::Stopped]);
        assert_eq!(d3.revision, d2.revision, "no-op does not bump the revision");
        let (d4, _) = step(&d3, &e, QueueOp::SetAutoplay { enabled: true });
        let (_, fx) = step(&d4, &e, QueueOp::TrackEnded);
        assert_eq!(fx, vec![Effect::NeedsAutoplay]);
    }

    #[test]
    fn next_then_previous_is_identity_with_insertions() {
        let e = DeterministicEntropy::new(2);
        let d = play(
            &new_document("s", "id".into(), 0.0),
            &e,
            album("a", 4),
            Some(0),
            false,
        );
        let (d, _) = step(
            &d,
            &e,
            QueueOp::PlayNext {
                server_id: "srv".into(),
                track_ids: vec!["x".into(), "y".into()],
            },
        );
        assert_eq!(playing_next_tracks(&d), vec!["x", "y"]);
        let (n1, _) = step(&d, &e, QueueOp::Next);
        assert_eq!(cur_track(&n1), "x");
        let (p1, _) = step(&n1, &e, QueueOp::Previous);
        assert!(same_state(&d, &p1));
        assert!(p1.revision > n1.revision);
        // Through the whole queue and back.
        let mut walk = vec![d.clone()];
        let mut cur = d.clone();
        loop {
            let (n, fx) = step(&cur, &e, QueueOp::Next);
            if fx.contains(&Effect::Stopped) {
                break;
            }
            walk.push(n.clone());
            cur = n;
        }
        assert_eq!(cur_track(&cur), "a-t3");
        assert_eq!(cur.history.len(), 5);
        while let Some(expected) = walk.pop() {
            assert!(
                same_state(&expected, &cur),
                "walk back mismatch at {}",
                cur_track(&expected)
            );
            let (p, _) = step(&cur, &e, QueueOp::Previous);
            cur = p;
        }
    }

    #[test]
    fn previous_at_start_restarts() {
        let e = DeterministicEntropy::new(3);
        let d = play(
            &new_document("s", "id".into(), 0.0),
            &e,
            album("a", 2),
            Some(0),
            false,
        );
        let (p, fx) = step(&d, &e, QueueOp::Previous);
        assert_eq!(fx, vec![Effect::RestartCurrent]);
        assert!(same_state(&d, &p));
        assert!(!previous_should_restart(2999));
        assert!(previous_should_restart(3000));
    }

    #[test]
    fn history_cap_falls_back_to_context_order() {
        let e = DeterministicEntropy::new(4);
        let d = play(
            &new_document("s", "id".into(), 0.0),
            &e,
            album("a", 6),
            Some(0),
            false,
        );
        let small = ReduceCtx {
            now: 1.0,
            position_ms: 0,
            history_cap: 2,
            saved: SavedQueuePolicy::default(),
            entropy: &e,
        };
        let mut cur = d;
        for _ in 0..4 {
            cur = reduce(&cur, QueueOp::Next, &small).unwrap().0;
        }
        assert_eq!(cur_track(&cur), "a-t4");
        assert_eq!(cur.history.len(), 2);
        for expect in ["a-t3", "a-t2", "a-t1", "a-t0"] {
            cur = reduce(&cur, QueueOp::Previous, &small).unwrap().0;
            assert_eq!(cur_track(&cur), expect);
            validate(&cur).unwrap();
        }
        let (_, fx) = reduce(&cur, QueueOp::Previous, &small).unwrap();
        assert_eq!(fx, vec![Effect::RestartCurrent]);
    }

    #[test]
    fn repeat_modes() {
        let e = DeterministicEntropy::new(5);
        let d = play(
            &new_document("s", "id".into(), 0.0),
            &e,
            album("a", 2),
            Some(1),
            false,
        );
        let (d, _) = step(
            &d,
            &e,
            QueueOp::SetRepeat {
                mode: RepeatMode::All,
            },
        );
        let (w, fx) = step(&d, &e, QueueOp::TrackEnded);
        assert_eq!(cur_track(&w), "a-t0");
        assert!(matches!(fx[0], Effect::CurrentChanged { .. }));
        // Wrap then previous returns to the end.
        let (b, _) = step(&w, &e, QueueOp::Previous);
        assert!(same_state(&d, &b));
        // Repeat one restarts on end, but a manual next still advances.
        let (one, _) = step(
            &d,
            &e,
            QueueOp::SetRepeat {
                mode: RepeatMode::One,
            },
        );
        let (_, fx) = step(&one, &e, QueueOp::TrackEnded);
        assert_eq!(fx, vec![Effect::RestartCurrent]);
        let (_, fx) = step(&one, &e, QueueOp::Next);
        assert_eq!(fx, vec![Effect::Stopped]);
        // Repeat all loops the context only: an inserted item is not replayed.
        let (d2, _) = step(
            &d,
            &e,
            QueueOp::PlayNext {
                server_id: "srv".into(),
                track_ids: vec!["x".into()],
            },
        );
        let (d3, _) = step(&d2, &e, QueueOp::Next);
        assert_eq!(cur_track(&d3), "x");
        let (d4, _) = step(&d3, &e, QueueOp::Next);
        assert_eq!(cur_track(&d4), "a-t0");
        let (d5, _) = step(&d4, &e, QueueOp::Next);
        assert_eq!(cur_track(&d5), "a-t1");
        let (d6, _) = step(&d5, &e, QueueOp::Next);
        assert_eq!(cur_track(&d6), "a-t0");
        assert!(d6.insertions.is_empty());
    }

    #[test]
    fn shuffle_keeps_current_first_and_off_restores_place() {
        let e = DeterministicEntropy::new(6);
        let d = play(
            &new_document("s", "id".into(), 0.0),
            &e,
            album("a", 10),
            Some(4),
            false,
        );
        let (s, _) = step(&d, &e, QueueOp::SetShuffle { enabled: true });
        assert_eq!(cur_track(&s), "a-t4");
        assert_eq!(s.shuffle.as_ref().unwrap().anchor, Some(4));
        assert_eq!(s.cursor, 1);
        let up = upcoming_tracks(&s);
        assert_eq!(up.len(), 9);
        assert!(!up.contains(&"a-t4".to_string()));
        assert_ne!(
            up,
            (5..10)
                .chain(0..4)
                .map(|i| format!("a-t{i}"))
                .collect::<Vec<_>>()
        );
        let (s2, _) = step(&s, &e, QueueOp::Next);
        let (s3, _) = step(&s2, &e, QueueOp::Next);
        let (off, _) = step(&s3, &e, QueueOp::SetShuffle { enabled: false });
        let idx = match off.current.as_ref().unwrap().source {
            QueueSource::Context { index } => index,
            _ => panic!(),
        };
        assert_eq!(off.cursor, idx + 1);
        assert_eq!(
            upcoming_tracks(&off),
            (idx + 1..10).map(|i| format!("a-t{i}")).collect::<Vec<_>>()
        );
        // Previous still walks the real history, not the unshuffled order.
        let (back, _) = step(&off, &e, QueueOp::Previous);
        assert_eq!(cur_track(&back), cur_track(&s2));
        // Reshuffle keeps the current item and changes the seed.
        let (re, _) = step(&s3, &e, QueueOp::Reshuffle);
        assert_eq!(cur_track(&re), cur_track(&s3));
        assert_ne!(
            re.shuffle.as_ref().unwrap().seed,
            s3.shuffle.as_ref().unwrap().seed
        );
        assert_eq!(re.cursor, 1);
    }

    #[test]
    fn shuffled_play_context_without_start_picks_random_first() {
        let e = DeterministicEntropy::new(7);
        let d = play(
            &new_document("s", "id".into(), 0.0),
            &e,
            album("a", 20),
            None,
            true,
        );
        assert!(d.current.is_some());
        assert_eq!(d.cursor, 1);
        assert_eq!(upcoming_tracks(&d).len(), 19);
        let with_start = play(
            &new_document("s", "id".into(), 0.0),
            &e,
            album("a", 20),
            Some(7),
            true,
        );
        assert_eq!(cur_track(&with_start), "a-t7");
    }

    #[test]
    fn youtube_mode_splices_into_context() {
        let e = DeterministicEntropy::new(8);
        let d = play(
            &new_document("s", "id".into(), 0.0),
            &e,
            album("a", 3),
            Some(0),
            false,
        );
        let (d, _) = step(
            &d,
            &e,
            QueueOp::SetQueueMode {
                mode: QueueMode::YouTube,
            },
        );
        let (d, _) = step(
            &d,
            &e,
            QueueOp::PlayNext {
                server_id: "srv".into(),
                track_ids: vec!["x".into(), "y".into()],
            },
        );
        assert!(d.insertions.is_empty());
        assert_eq!(
            d.context.as_ref().unwrap().tracks,
            vec!["a-t0", "x", "y", "a-t1", "a-t2"]
        );
        assert_eq!(upcoming_tracks(&d), vec!["x", "y", "a-t1", "a-t2"]);
        let (d, _) = step(
            &d,
            &e,
            QueueOp::PlayLater {
                server_id: "srv".into(),
                track_ids: vec!["z".into()],
            },
        );
        assert_eq!(upcoming_tracks(&d), vec!["x", "y", "a-t1", "a-t2", "z"]);
        // Repeat All now loops the spliced items too.
        let (d, _) = step(
            &d,
            &e,
            QueueOp::SetRepeat {
                mode: RepeatMode::All,
            },
        );
        let mut cur = d;
        let mut seen = vec![];
        for _ in 0..7 {
            cur = step(&cur, &e, QueueOp::Next).0;
            seen.push(cur_track(&cur).to_string());
        }
        assert_eq!(seen, vec!["x", "y", "a-t1", "a-t2", "z", "a-t0", "x"]);
        // Shuffled YouTube mode: play-next still lands right after the current item.
        let d = play(
            &new_document("s", "id".into(), 0.0),
            &e,
            album("b", 8),
            Some(3),
            true,
        );
        let (d, _) = step(
            &d,
            &e,
            QueueOp::SetQueueMode {
                mode: QueueMode::YouTube,
            },
        );
        let before = upcoming_tracks(&d);
        let (d, _) = step(
            &d,
            &e,
            QueueOp::PlayNext {
                server_id: "srv".into(),
                track_ids: vec!["p".into(), "q".into()],
            },
        );
        let after = upcoming_tracks(&d);
        assert_eq!(&after[..2], &["p", "q"]);
        assert_eq!(
            &after[2..],
            &before[..],
            "the rest of the shuffled order is untouched"
        );
        assert!(d.shuffle.as_ref().unwrap().order.is_some());
        // Switching to YouTube mode folds pending insertions into the context.
        let d = play(
            &new_document("s", "id".into(), 0.0),
            &e,
            album("c", 2),
            Some(0),
            false,
        );
        let (d, _) = step(
            &d,
            &e,
            QueueOp::PlayNext {
                server_id: "srv".into(),
                track_ids: vec!["i".into()],
            },
        );
        let (d, _) = step(
            &d,
            &e,
            QueueOp::SetQueueMode {
                mode: QueueMode::YouTube,
            },
        );
        assert!(d.insertions.is_empty());
        assert_eq!(upcoming_tracks(&d), vec!["i", "c-t1"]);
    }

    #[test]
    fn remove_and_move() {
        let e = DeterministicEntropy::new(9);
        let d = play(
            &new_document("s", "id".into(), 0.0),
            &e,
            album("a", 5),
            Some(0),
            false,
        );
        let (d, _) = step(
            &d,
            &e,
            QueueOp::PlayNext {
                server_id: "srv".into(),
                track_ids: vec!["x".into(), "y".into()],
            },
        );
        // Remove an upcoming context item and an inserted one.
        let y = derive(&d).playing_next[1].key.clone();
        let (d, _) = step(
            &d,
            &e,
            QueueOp::RemoveQueueItems {
                keys: vec![context_key(2), y],
            },
        );
        assert_eq!(playing_next_tracks(&d), vec!["x"]);
        assert_eq!(upcoming_tracks(&d), vec!["a-t1", "a-t3", "a-t4"]);
        assert_eq!(cur_track(&d), "a-t0");
        // Move a context item into playing next: it becomes Inserted. Synthetic
        // keys are index-based, so they are re-read from the derived queue
        // after every change (the UI gets a fresh QueueView each time).
        let t3 = derive(&d)
            .upcoming
            .iter()
            .find(|i| i.track_id == "a-t3")
            .unwrap()
            .key
            .clone();
        let (d, _) = step(
            &d,
            &e,
            QueueOp::MoveQueueItem {
                key: t3,
                to_index: 0,
            },
        );
        assert_eq!(playing_next_tracks(&d), vec!["a-t3", "x"]);
        assert_eq!(d.insertions[0].source, QueueSource::Inserted);
        assert_eq!(upcoming_tracks(&d), vec!["a-t1", "a-t4"]);
        // Move an inserted item into upcoming: it becomes a context item.
        let x = d.insertions[1].key.clone();
        let (d, _) = step(
            &d,
            &e,
            QueueOp::MoveQueueItem {
                key: x,
                to_index: 2,
            },
        );
        assert_eq!(playing_next_tracks(&d), vec!["a-t3"]);
        assert_eq!(upcoming_tracks(&d), vec!["a-t1", "x", "a-t4"]);
        // Reorder within upcoming.
        let key = derive(&d).upcoming[2].key.clone();
        let (d, _) = step(&d, &e, QueueOp::MoveQueueItem { key, to_index: 1 });
        assert_eq!(upcoming_tracks(&d), vec!["a-t4", "a-t1", "x"]);
        // Removing the current item advances and drops it.
        let cur_key = d.current.as_ref().unwrap().key.clone();
        let (d, fx) = step(
            &d,
            &e,
            QueueOp::RemoveQueueItems {
                keys: vec![cur_key],
            },
        );
        assert_eq!(cur_track(&d), "a-t3");
        assert!(d.history.is_empty());
        assert!(matches!(fx[0], Effect::CurrentChanged { .. }));
        assert!(!d
            .context
            .as_ref()
            .unwrap()
            .tracks
            .contains(&"a-t0".to_string()));
        // Unknown key is an error, not a panic.
        assert_eq!(
            step_err(
                &d,
                &e,
                QueueOp::RemoveQueueItems {
                    keys: vec!["nope".into()]
                }
            ),
            ReduceError::UnknownKey("nope".into())
        );
    }

    fn step_err(doc: &SessionDocument, e: &DeterministicEntropy, op: QueueOp) -> ReduceError {
        reduce(doc, op, &ctx(e, 2.0, 0)).unwrap_err()
    }

    #[test]
    fn remove_under_shuffle_keeps_order() {
        let e = DeterministicEntropy::new(10);
        let d = play(
            &new_document("s", "id".into(), 0.0),
            &e,
            album("a", 8),
            Some(0),
            true,
        );
        let before = upcoming_tracks(&d);
        let victim = derive(&d).upcoming[3].clone();
        let (d, _) = step(
            &d,
            &e,
            QueueOp::RemoveQueueItems {
                keys: vec![victim.key],
            },
        );
        let after = upcoming_tracks(&d);
        let expected: Vec<String> = before
            .into_iter()
            .filter(|t| *t != victim.track_id)
            .collect();
        assert_eq!(after, expected);
    }

    #[test]
    fn jump_semantics() {
        let e = DeterministicEntropy::new(11);
        let d = play(
            &new_document("s", "id".into(), 0.0),
            &e,
            album("a", 5),
            Some(0),
            false,
        );
        let (d, _) = step(
            &d,
            &e,
            QueueOp::PlayNext {
                server_id: "srv".into(),
                track_ids: vec!["x".into()],
            },
        );
        // Jump ahead in the context keeps the insertion for later.
        let (j, _) = step(
            &d,
            &e,
            QueueOp::JumpToQueueItem {
                key: context_key(3),
            },
        );
        assert_eq!(cur_track(&j), "a-t3");
        assert_eq!(playing_next_tracks(&j), vec!["x"]);
        assert_eq!(upcoming_tracks(&j), vec!["a-t4"]);
        assert_eq!(j.history.len(), 1);
        // Jump back into history rewinds; the items the forward jump skipped
        // stay skipped (Apple / Spotify semantics), the insertion is still pending.
        let h = j.history[0].key.clone();
        let (b, _) = step(&j, &e, QueueOp::JumpToQueueItem { key: h });
        assert_eq!(cur_track(&b), "a-t0");
        assert!(b.history.is_empty());
        assert_eq!(playing_next_tracks(&b), vec!["x"]);
        assert_eq!(upcoming_tracks(&b), vec!["a-t3", "a-t4"]);
        // Jump to current restarts.
        let (_, fx) = step(
            &d,
            &e,
            QueueOp::JumpToQueueItem {
                key: d.current.as_ref().unwrap().key.clone(),
            },
        );
        assert_eq!(fx, vec![Effect::RestartCurrent]);
        // Jump to an inserted item.
        let x = d.insertions[0].key.clone();
        let (i, _) = step(&d, &e, QueueOp::JumpToQueueItem { key: x });
        assert_eq!(cur_track(&i), "x");
        assert_eq!(
            step_err(&d, &e, QueueOp::JumpToQueueItem { key: "zzz".into() }),
            ReduceError::UnknownKey("zzz".into())
        );
    }

    #[test]
    fn unavailable_items_are_skipped_both_ways_unless_jumped_to() {
        let e = DeterministicEntropy::new(12);
        let d = play(
            &new_document("s", "id".into(), 0.0),
            &e,
            album("a", 3),
            Some(0),
            false,
        );
        let (d, _) = step(
            &d,
            &e,
            QueueOp::PlayNext {
                server_id: "srv".into(),
                track_ids: vec!["bad".into(), "good".into()],
            },
        );
        let bad = d.insertions[0].key.clone();
        let (d, _) = step(&d, &e, QueueOp::SkipUnavailable { key: bad.clone() });
        let (n, fx) = step(&d, &e, QueueOp::Next);
        assert_eq!(cur_track(&n), "good");
        assert!(fx.contains(&Effect::Skipped { key: bad.clone() }));
        assert!(n.history.iter().any(|h| h.key == bad && h.unavailable));
        let (p, _) = step(&n, &e, QueueOp::Previous);
        assert_eq!(cur_track(&p), "a-t0");
        assert!(same_state(&p, &d));
        let (j, _) = step(&n, &e, QueueOp::JumpToQueueItem { key: bad.clone() });
        assert_eq!(cur_track(&j), "bad");
        assert!(!j.current.as_ref().unwrap().unavailable);
        // Skip the current: flagged, advanced.
        let cur = d.current.as_ref().unwrap().key.clone();
        let (s, fx) = step(&d, &e, QueueOp::SkipUnavailable { key: cur.clone() });
        assert_eq!(cur_track(&s), "good");
        assert!(fx.contains(&Effect::Skipped { key: cur.clone() }));
        assert!(s.history.iter().any(|h| h.key == cur && h.unavailable));
        // Skipping the last playable item stops (or asks for autoplay).
        let last = play(
            &new_document("s", "id".into(), 0.0),
            &e,
            album("z", 1),
            Some(0),
            false,
        );
        let k = last.current.as_ref().unwrap().key.clone();
        let (l, fx) = step(&last, &e, QueueOp::SkipUnavailable { key: k });
        assert!(l.current.as_ref().unwrap().unavailable);
        assert!(fx.contains(&Effect::Stopped));
        // Skipping a derived upcoming item removes it.
        let (r, _) = step(
            &d,
            &e,
            QueueOp::SkipUnavailable {
                key: context_key(2),
            },
        );
        assert_eq!(upcoming_tracks(&r), vec!["a-t1"]);
    }

    #[test]
    fn offline_marks_clear_and_real_failures_stay() {
        let e = DeterministicEntropy::new(21);
        let d = play(
            &new_document("s", "id".into(), 0.0),
            &e,
            album("a", 1),
            Some(0),
            false,
        );
        let (d, _) = step(
            &d,
            &e,
            QueueOp::PlayNext {
                server_id: "srv".into(),
                track_ids: vec!["off1".into(), "broken".into(), "off2".into(), "ok".into()],
            },
        );
        let keys: Vec<QueueKey> = d.insertions.iter().map(|i| i.key.clone()).collect();
        let (off1, broken, off2) = (keys[0].clone(), keys[1].clone(), keys[2].clone());
        // The current item goes offline-skipped; the others are marked while queued.
        let (d, _) = step(&d, &e, QueueOp::Next);
        assert_eq!(cur_track(&d), "off1");
        let (d, fx) = step(&d, &e, QueueOp::SkipOffline { key: off1.clone() });
        assert!(fx.contains(&Effect::Skipped { key: off1.clone() }));
        assert_eq!(cur_track(&d), "broken");
        let (d, _) = step(
            &d,
            &e,
            QueueOp::SkipUnavailable {
                key: broken.clone(),
            },
        );
        let (d, _) = step(&d, &e, QueueOp::SkipOffline { key: off2.clone() });
        assert_eq!(cur_track(&d), "ok");
        assert_eq!(
            offline_skipped(&d),
            [off1.clone(), off2.clone()].into_iter().collect()
        );
        // An older build sees the set as an opaque top-level field.
        let json = super::super::save(&d).unwrap();
        assert!(json.contains(OFFLINE_SKIPPED_FIELD), "{json}");
        let (c, fx) = step(&d, &e, QueueOp::ClearOfflineSkips);
        assert!(fx.is_empty(), "{fx:?}");
        assert!(offline_skipped(&c).is_empty());
        assert!(!c.extra.contains_key(OFFLINE_SKIPPED_FIELD));
        let marked = |doc: &SessionDocument, k: &str| {
            doc.history
                .iter()
                .chain(doc.insertions.iter())
                .any(|i| i.key == k && i.unavailable)
        };
        assert!(!marked(&c, &off1));
        assert!(!marked(&c, &off2));
        assert!(marked(&c, &broken), "a load failure stays marked");
        assert!(c.revision > d.revision);
        // Nothing to clear is a no-op.
        let (c2, _) = step(&c, &e, QueueOp::ClearOfflineSkips);
        assert!(same_state(&c, &c2));
        // A later real failure of an offline-skipped item is a real failure.
        let (j, _) = step(&d, &e, QueueOp::SkipUnavailable { key: off1.clone() });
        assert!(!offline_skipped(&j).contains(&off1));
        // A jump clears the mark and drops the key from the set.
        let (j, _) = step(&d, &e, QueueOp::JumpToQueueItem { key: off2.clone() });
        assert_eq!(offline_skipped(&j), [off1.clone()].into_iter().collect());
        // A plain mark from an older build (no set) is never cleared.
        let mut old = d.clone();
        old.extra.clear();
        let (o, _) = step(&old, &e, QueueOp::ClearOfflineSkips);
        assert!(marked(&o, &off1) && marked(&o, &off2));
    }

    #[test]
    fn saved_queue_round_trip_with_position_and_history() {
        let e = DeterministicEntropy::new(13);
        let d = play(
            &new_document("s", "id".into(), 0.0),
            &e,
            album("a", 4),
            Some(0),
            false,
        );
        let (d, _) = step(&d, &e, QueueOp::Next);
        let (d, _) = step(
            &d,
            &e,
            QueueOp::PlayNext {
                server_id: "srv".into(),
                track_ids: vec!["x".into()],
            },
        );
        // Replacing the context snapshots the outgoing one, position included.
        let args = PlayContextArgs {
            context: album("b", 3),
            start_index: Some(0),
            shuffle: false,
            save_outgoing: true,
        };
        let (d2, fx) = reduce(&d, QueueOp::PlayContext { args }, &ctx(&e, 50.0, 42_000)).unwrap();
        assert!(fx.contains(&Effect::SavedQueuesChanged));
        assert!(fx.contains(&Effect::ContextReplaced));
        assert_eq!(d2.saved_queues.len(), 1);
        let sq = &d2.saved_queues[0];
        assert_eq!(sq.position_ms, 42_000);
        assert!(
            sq.context.tracks.is_empty(),
            "ID-referenced snapshots omit tracks"
        );
        assert_eq!(sq.history.len(), 1);
        assert_eq!(sq.insertions.len(), 1);
        assert!(saved::needs_resolution(sq));
        // Restore without tracks asks for resolution; with tracks it is complete.
        let (r, fx) = step(
            &d2,
            &e,
            QueueOp::RestoreSavedQueue {
                id: sq.id.clone(),
                tracks: None,
            },
        );
        assert!(fx
            .iter()
            .any(|f| matches!(f, Effect::ResolveContext { .. })));
        assert!(fx.contains(&Effect::PositionRestore {
            position_ms: 42_000
        }));
        assert!(matches!(
            fx[0],
            Effect::CurrentChanged {
                position_ms: 42_000,
                ..
            }
        ));
        assert_eq!(cur_track(&r), "a-t1");
        let (r2, _) = step(
            &r,
            &e,
            QueueOp::SetContextTracks {
                tracks: album("a", 4).tracks,
            },
        );
        assert!(same_state_ignoring_saved(&r2, &d));
        // The album-b queue got saved on the way out, and the restored entry was touched.
        assert_eq!(r2.saved_queues.len(), 2);
        let (full, fx) = step(
            &d2,
            &e,
            QueueOp::RestoreSavedQueue {
                id: sq.id.clone(),
                tracks: Some(album("a", 4).tracks),
            },
        );
        assert!(!fx
            .iter()
            .any(|f| matches!(f, Effect::ResolveContext { .. })));
        assert_eq!(upcoming_tracks(&full), vec!["a-t2", "a-t3"]);
        assert_eq!(playing_next_tracks(&full), vec!["x"]);
        assert_eq!(full.history.len(), 1);
        // Playlist changed since: the stored index no longer matches and is repaired.
        let mut moved = album("a", 4).tracks;
        moved.swap(1, 3);
        let (fixed, _) = step(
            &d2,
            &e,
            QueueOp::RestoreSavedQueue {
                id: sq.id.clone(),
                tracks: Some(moved),
            },
        );
        assert_eq!(cur_track(&fixed), "a-t1");
        assert_eq!(
            fixed.current.as_ref().unwrap().source,
            QueueSource::Context { index: 3 }
        );
        assert_eq!(
            step_err(
                &d2,
                &e,
                QueueOp::RestoreSavedQueue {
                    id: "nope".into(),
                    tracks: None
                }
            ),
            ReduceError::UnknownSavedQueue("nope".into())
        );
    }

    fn same_state_ignoring_saved(a: &SessionDocument, b: &SessionDocument) -> bool {
        let mut a = a.clone();
        let mut b = b.clone();
        a.saved_queues.clear();
        b.saved_queues.clear();
        same_state(&a, &b)
    }

    #[test]
    fn saved_queue_management_ops() {
        let e = DeterministicEntropy::new(14);
        let d = play(
            &new_document("s", "id".into(), 0.0),
            &e,
            album("a", 2),
            Some(0),
            false,
        );
        let (d, fx) = step(&d, &e, QueueOp::SaveCurrentQueue { pinned: true });
        assert!(fx.contains(&Effect::SavedQueuesChanged));
        let id = d.saved_queues[0].id.clone();
        assert!(d.saved_queues[0].pinned);
        let (d, _) = step(
            &d,
            &e,
            QueueOp::PinSavedQueue {
                id: id.clone(),
                pinned: false,
            },
        );
        assert!(!d.saved_queues[0].pinned);
        let (d, _) = step(&d, &e, QueueOp::TouchSavedQueue { id: id.clone() });
        assert_eq!(d.saved_queues[0].last_interacted_at, 2.0);
        let mut remote = d.saved_queues[0].clone();
        remote.id = "remote".into();
        remote.pinned = true;
        remote.updated_at = 999.0;
        let (m, _) = step(
            &d,
            &e,
            QueueOp::MergeSavedQueues {
                remote: vec![remote],
            },
        );
        assert_eq!(m.saved_queues.len(), 1);
        assert_eq!(m.saved_queues[0].id, "remote");
        let (gone, _) = step(
            &m,
            &e,
            QueueOp::DeleteSavedQueue {
                id: "remote".into(),
            },
        );
        assert!(gone.saved_queues.is_empty());
        assert_eq!(
            step_err(
                &gone,
                &e,
                QueueOp::DeleteSavedQueue {
                    id: "remote".into()
                }
            ),
            ReduceError::UnknownSavedQueue("remote".into())
        );
        // Trivial queues are not auto-saved.
        let single = play(
            &new_document("s", "id".into(), 0.0),
            &e,
            album("one", 1),
            Some(0),
            false,
        );
        let next = play(&single, &e, album("two", 3), Some(0), false);
        assert!(next.saved_queues.is_empty());
    }

    #[test]
    fn clear_queue_keeps_the_playing_track() {
        let e = DeterministicEntropy::new(15);
        let d = play(
            &new_document("s", "id".into(), 0.0),
            &e,
            album("a", 4),
            Some(1),
            true,
        );
        let (d, _) = step(
            &d,
            &e,
            QueueOp::PlayLater {
                server_id: "srv".into(),
                track_ids: vec!["x".into()],
            },
        );
        let key = d.current.as_ref().unwrap().key.clone();
        let (c, fx) = step(&d, &e, QueueOp::ClearQueue);
        assert_eq!(c.current.as_ref().unwrap().key, key);
        assert!(!fx
            .iter()
            .any(|f| matches!(f, Effect::CurrentChanged { .. })));
        assert!(fx.contains(&Effect::SavedQueuesChanged));
        assert!(derive(&c).upcoming.is_empty());
        assert!(derive(&c).playing_next.is_empty());
        assert!(c.shuffle.is_none());
        assert_eq!(c.context.as_ref().unwrap().tracks, vec!["a-t1"]);
        let (_, fx) = step(&c, &e, QueueOp::Next);
        assert_eq!(fx, vec![Effect::Stopped]);
        // Queueing onto an empty session starts playing.
        let empty = new_document("s", "id".into(), 0.0);
        let (q, fx) = step(
            &empty,
            &e,
            QueueOp::PlayNext {
                server_id: "srv".into(),
                track_ids: vec!["a".into(), "b".into()],
            },
        );
        assert_eq!(cur_track(&q), "a");
        assert!(matches!(fx[0], Effect::CurrentChanged { .. }));
        assert_eq!(upcoming_tracks(&q), vec!["b"]);
        assert!(q.saved_queues.is_empty());
    }

    #[test]
    fn autoplay_items_append_and_play_last() {
        let e = DeterministicEntropy::new(16);
        let d = play(
            &new_document("s", "id".into(), 0.0),
            &e,
            album("a", 1),
            Some(0),
            false,
        );
        let items = vec![AutoplayItem {
            track_id: "r1".into(),
            provider: AutoplayProvider::SonicSimilarity,
            reason: "similar".into(),
            score: Some(0.9),
        }];
        let (d, _) = step(&d, &e, QueueOp::AppendAutoplay { items });
        let (d, _) = step(
            &d,
            &e,
            QueueOp::PlayLater {
                server_id: "srv".into(),
                track_ids: vec!["later".into()],
            },
        );
        assert_eq!(
            playing_next_tracks(&d),
            vec!["later", "r1"],
            "play later goes ahead of the autoplay tail"
        );
        let (d, _) = step(
            &d,
            &e,
            QueueOp::PlayNext {
                server_id: "srv".into(),
                track_ids: vec!["next".into()],
            },
        );
        assert_eq!(playing_next_tracks(&d), vec!["next", "later", "r1"]);
        let (d, _) = step(&d, &e, QueueOp::ClearInsertions);
        assert!(d.insertions.is_empty());
    }

    #[test]
    fn derived_view_resolves_tracks() {
        let e = DeterministicEntropy::new(17);
        let d = play(
            &new_document("s", "id".into(), 0.0),
            &e,
            album("a", 3),
            Some(0),
            false,
        );
        let v = derive(&d).into_view(|id| TrackSummary {
            id: id.clone(),
            title: id.to_uppercase(),
            ..Default::default()
        });
        assert_eq!(
            v.current
                .as_ref()
                .map(|c: &QueueEntry| c.track.title.as_str()),
            Some("A-T0")
        );
        assert_eq!(v.upcoming.len(), 2);
        assert_eq!(v.total_upcoming, 2);
        assert_eq!(v.context_label.as_deref(), Some("Album a"));
    }

    #[test]
    fn from_command_covers_queue_commands() {
        assert!(QueueOp::from_command(&Command::Next).is_some());
        assert!(QueueOp::from_command(&Command::ClearQueue).is_some());
        assert!(QueueOp::from_command(&Command::SetRating {
            targets: vec![RatingTarget::Track { id: "t".into() }],
            rating: 3
        })
        .is_none());
        assert!(QueueOp::PlayContext {
            args: PlayContextArgs {
                context: album("a", 1),
                start_index: None,
                shuffle: false,
                save_outgoing: true
            }
        }
        .replaces_context());
    }

    #[test]
    fn garbage_documents_do_not_panic() {
        let e = DeterministicEntropy::new(18);
        let mut d = play(
            &new_document("s", "id".into(), 0.0),
            &e,
            album("a", 3),
            Some(0),
            true,
        );
        d.cursor = 99;
        d.history.push(QueueItem {
            key: "h".into(),
            track_id: "gone".into(),
            source: QueueSource::Context { index: 77 },
            unavailable: false,
        });
        d.shuffle = Some(ShuffleState {
            seed: 1,
            anchor: Some(50),
            order: Some(vec![9, 9]),
        });
        for op in [
            QueueOp::Next,
            QueueOp::Previous,
            QueueOp::TrackEnded,
            QueueOp::SetShuffle { enabled: false },
            QueueOp::JumpToQueueItem { key: "h".into() },
            QueueOp::MoveQueueItem {
                key: context_key(1),
                to_index: 50,
            },
            QueueOp::RemoveQueueItems {
                keys: vec![context_key(2)],
            },
            QueueOp::ClearQueue,
        ] {
            let _ = reduce(&d, op, &ctx(&e, 1.0, 0));
        }
    }
}
