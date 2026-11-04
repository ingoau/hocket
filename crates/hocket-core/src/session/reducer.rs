//! The queue reducer: a pure function from `(document, op)` to
//! `(document, effects)`. Every queue mutation in the product goes through
//! here; there is no other path.
//!
//! # Model (design.md "Queue model")
//!
//! The queue is **one flat list with a pointer**. The document stores it as
//! the three runs the pointer splits it into (`history ++ [current] ++
//! upcoming`); the reducer joins them into a [`Flat`] list, applies the op and
//! splits it again. There are no queue modes.
//!
//! **Navigation never edits the list.** Next and Previous move the pointer one
//! playable entry along it; a jump moves it straight to the tapped entry, so
//! the entries jumped over end up behind it, in history. Nothing is consumed:
//! Next^k then Previous^k lands on the same entry, and after jumping k rows
//! ahead Previous^k returns (property-tested). Entries flagged `unavailable`
//! are passed over in both directions and only play when jumped to (which
//! clears the flag). Repeat All wraps Next from the last entry to the first
//! and Previous from the first to the last; Repeat One only replays on a
//! natural track end. The "Previous restarts after 3 s" rule is the actor's
//! ([`previous_should_restart`]); here Previous always steps.
//!
//! **Entries.** Each entry has a stable key (ops name entries by key, never by
//! index), its track, `added` (the user put it there; never changes),
//! `queued` (in the "Next in queue" group: the contiguous run of `queued`
//! entries directly after the current one) and `ordinal` (its place in the
//! unshuffled order). Because group membership is a flag that rides with the
//! entry, Previous after playing a queued entry puts it back at the head of
//! the group.
//!
//! **Edits.** Play Next puts entries directly after the current one (queued
//! when the op says so, which the actor takes from the Queue section
//! setting), Add To Queue at the end of the group (queued), Play Later at the
//! end of the list (not queued). Naming an entry ahead of the pointer moves
//! it; naming one in history (or the current one) inserts a copy, so history
//! is never rewritten. A move carries its target and an explicit `in_group`,
//! never re-derived from neighbours, and cannot go above the current entry or
//! into history. Starting a new context replaces the whole list and
//! auto-saves the outgoing one as a saved queue.
//!
//! **Shuffle** reorders only the entries after the group; history and the
//! group stay put. Turning it off sorts those entries by `ordinal` again,
//! starting after the current entry's ordinal (the ones before it follow), so
//! added entries go back where they were added. While shuffle is off every
//! ordinal is its list index.
//!
//! **Limits.** History keeps up to [`HISTORY_CAP`] entries, trimmed from the
//! oldest end after every op, and the whole list at most [`MAX_QUEUE_LEN`]
//! (more history goes first, then the far end of the list). A context longer
//! than that plays a [`MAX_QUEUE_LEN`] window of it ([`window_tracks`]). Both
//! are protocol constants, never settings: every replica and the coordinator
//! must trim at the same place, and together with the saved-queue budget they
//! keep a document under the Connect frame limit.
//!
//! **Determinism.** Keys and shuffle seeds come only from the op's
//! [`Entropy`], so every Connect replica and the coordinator reduce the same
//! op to the same document.
//!
//! **Revision.** The revision is bumped exactly once per op that changed the
//! document (compared without revision / `updated_at`); no-ops leave it alone.

use std::collections::{BTreeSet, HashSet};

use crate::api::{
    AutoplayProvider, Command, ContextKind, EpochMs, Ms, PlayContextArgs, QueueContext, QueueItem,
    QueueKey, QueueSource, QueueView, RepeatMode, SavedQueue, ServerId, SessionDocument, SortOrder,
    TrackId, TrackSummary,
};

use super::document::{bump_revision, same_state};
use super::saved::{self, SavedQueuePolicy};
use super::shuffle::shuffle_in_place;
use super::Entropy;

/// Entries kept behind the pointer. A protocol constant (see the module docs).
pub const HISTORY_CAP: usize = 5000;
/// Most entries the whole list (history, current and upcoming) holds. A
/// protocol constant (see the module docs).
pub const MAX_QUEUE_LEN: usize = 10_000;
/// Rows of history the UI queue view carries by default (the window behind
/// the pointer). The rest are counted in `history_count` and paged in with
/// `Query::QueueRange`.
pub const QUEUE_VIEW_BEFORE: usize = 100;
/// Rows after the current entry the UI queue view carries by default.
pub const QUEUE_VIEW_AFTER: usize = 100;
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

/// Where a queueing op puts its entries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueuePlacement {
    /// Directly after the current entry.
    Next,
    /// At the end of the "Next in queue" group.
    Group,
    /// At the end of the list.
    Later,
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
        /// Where the chosen track starts, in audio time.
        start_position_ms: Ms,
    },
    /// Directly after the current entry. `queued`: into the group (the Queue
    /// section setting); with a group already there the entries join it
    /// regardless, so the group stays attached to the current entry.
    PlayNext {
        server_id: ServerId,
        track_ids: Vec<TrackId>,
        keys: Vec<QueueKey>,
        queued: bool,
    },
    /// At the end of the "Next in queue" group, queued.
    AddToQueue {
        server_id: ServerId,
        track_ids: Vec<TrackId>,
        keys: Vec<QueueKey>,
    },
    /// At the end of the list, not queued.
    PlayLater {
        server_id: ServerId,
        track_ids: Vec<TrackId>,
        keys: Vec<QueueKey>,
    },
    /// Directly after `after` (the current entry or one ahead of the
    /// pointer), in or out of the group, placed like a
    /// [`QueueOp::MoveQueueItem`] drop. `keys` move or copy as for
    /// [`QueueOp::PlayNext`].
    InsertTracks {
        server_id: ServerId,
        track_ids: Vec<TrackId>,
        keys: Vec<QueueKey>,
        after: QueueKey,
        in_group: bool,
    },
    JumpToQueueItem {
        key: QueueKey,
    },
    RemoveQueueItems {
        keys: Vec<QueueKey>,
    },
    /// Move an entry ahead of the pointer to directly after `after` (the
    /// current entry or one ahead of the pointer), in or out of the group.
    MoveQueueItem {
        key: QueueKey,
        after: QueueKey,
        in_group: bool,
    },
    ClearQueue,
    /// Remove the "Next in queue" group's entries.
    ClearQueueGroup,
    SetShuffle {
        enabled: bool,
    },
    /// New seed for the entries after the group.
    Reshuffle,
    SetRepeat {
        mode: RepeatMode,
    },
    SetAutoplay {
        enabled: bool,
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
    /// result as a whole-document replace.
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
    /// `tracks` carries the re-resolved track list for a snapshot stored
    /// without its entries; `None` restores as stored and asks for resolution.
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
    /// `id: None` renames the live context; otherwise that saved queue.
    RenameQueue {
        id: Option<String>,
        label: String,
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
    /// not touch the queue. `PlayNext` is queued (the Queue section setting's
    /// default); the actor overrides it from the setting.
    pub fn from_command(cmd: &Command) -> Option<QueueOp> {
        Some(match cmd {
            Command::PlayContext { args } => QueueOp::PlayContext { args: args.clone() },
            Command::PlayTracks {
                server_id,
                track_ids,
                start_index,
                label,
                shuffle,
                start_position_ms,
            } => QueueOp::PlayTracks {
                server_id: server_id.clone(),
                track_ids: track_ids.clone(),
                start_index: *start_index,
                label: label.clone(),
                shuffle: *shuffle,
                save_outgoing: true,
                start_position_ms: *start_position_ms,
            },
            Command::PlayNext {
                server_id,
                track_ids,
                keys,
            } => QueueOp::PlayNext {
                server_id: server_id.clone(),
                track_ids: track_ids.clone(),
                keys: keys.clone(),
                queued: true,
            },
            Command::AddToQueue {
                server_id,
                track_ids,
                keys,
            } => QueueOp::AddToQueue {
                server_id: server_id.clone(),
                track_ids: track_ids.clone(),
                keys: keys.clone(),
            },
            Command::PlayLater {
                server_id,
                track_ids,
                keys,
            } => QueueOp::PlayLater {
                server_id: server_id.clone(),
                track_ids: track_ids.clone(),
                keys: keys.clone(),
            },
            Command::InsertTracks {
                server_id,
                track_ids,
                keys,
                after,
                in_group,
            } => QueueOp::InsertTracks {
                server_id: server_id.clone(),
                track_ids: track_ids.clone(),
                keys: keys.clone(),
                after: after.clone(),
                in_group: *in_group,
            },
            Command::JumpToQueueItem { key } => QueueOp::JumpToQueueItem { key: key.clone() },
            Command::RemoveQueueItems { keys } => QueueOp::RemoveQueueItems { keys: keys.clone() },
            Command::MoveQueueItem {
                key,
                after,
                in_group,
            } => QueueOp::MoveQueueItem {
                key: key.clone(),
                after: after.clone(),
                in_group: *in_group,
            },
            Command::ClearQueue => QueueOp::ClearQueue,
            Command::ClearQueueGroup => QueueOp::ClearQueueGroup,
            Command::SetShuffle { enabled } => QueueOp::SetShuffle { enabled: *enabled },
            Command::SetRepeat { mode } => QueueOp::SetRepeat { mode: *mode },
            Command::SetAutoplay { enabled } => QueueOp::SetAutoplay { enabled: *enabled },
            Command::Next => QueueOp::Next,
            Command::Previous | Command::PreviousTrack => QueueOp::Previous,
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
            Command::RenameQueue {
                saved_queue_id,
                label,
            } => QueueOp::RenameQueue {
                id: saved_queue_id.clone(),
                label: label.clone(),
            },
            _ => return None,
        })
    }

    /// Whether the op replaces the whole list (and so snapshots the old one).
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
    /// The list was replaced (new album / playlist / selection / restore).
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
    #[error("a queue name can't be empty")]
    EmptyName,
    #[error("queue entries can't be moved above the current one or into history")]
    BadMove,
}

/// Environment for one reduction. `position_ms` is the playback position of
/// the current item as the actor knows it (used for snapshots).
///
/// Nothing here comes from settings: the limits (history, list length, saved
/// queues) are protocol constants, so every replica reduces alike.
pub struct ReduceCtx<'a> {
    pub now: EpochMs,
    pub position_ms: Ms,
    pub entropy: &'a dyn Entropy,
}

/// The saved-queue policy every replica applies to the document.
fn saved_policy() -> SavedQueuePolicy {
    SavedQueuePolicy::default()
}

/// The window of a track list longer than [`MAX_QUEUE_LEN`] that is played:
/// up to [`HISTORY_CAP`] tracks before `anchor` and the rest after it. Returns
/// `lo..hi`; the whole list when it fits.
pub fn window_tracks(len: usize, anchor: usize) -> std::ops::Range<usize> {
    if len <= MAX_QUEUE_LEN {
        return 0..len;
    }
    let lo = anchor
        .min(len)
        .saturating_sub(HISTORY_CAP)
        .min(len - MAX_QUEUE_LEN);
    lo..lo + MAX_QUEUE_LEN
}

// ---------------------------------------------------------------------------
// The flat list
// ---------------------------------------------------------------------------

/// The queue as one list and a pointer. `ptr == list.len()` means nothing is
/// current (the list is all history).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Flat {
    pub list: Vec<QueueItem>,
    pub ptr: usize,
}

impl Flat {
    /// Join a document's runs (moves them out of the document).
    pub fn take(doc: &mut SessionDocument) -> Flat {
        let mut list = std::mem::take(&mut doc.history);
        let ptr = list.len();
        list.extend(doc.current.take());
        // With nothing current, entries after the pointer (not a state the
        // reducer writes) make the first of them current.
        list.append(&mut doc.upcoming);
        Flat { list, ptr }
    }

    /// A copy of a document's list.
    pub fn of(doc: &SessionDocument) -> Flat {
        let mut list = Vec::with_capacity(doc.history.len() + 1 + doc.upcoming.len());
        list.extend(doc.history.iter().cloned());
        let ptr = list.len();
        list.extend(doc.current.iter().cloned());
        list.extend(doc.upcoming.iter().cloned());
        Flat { list, ptr }
    }

    /// Split back into the document's runs.
    pub fn put(mut self, doc: &mut SessionDocument) {
        let ptr = self.ptr.min(self.list.len());
        let mut rest = self.list.split_off(ptr);
        doc.history = self.list;
        if rest.is_empty() {
            doc.current = None;
            doc.upcoming = vec![];
        } else {
            doc.upcoming = rest.split_off(1);
            doc.current = rest.pop();
        }
    }

    pub fn len(&self) -> usize {
        self.list.len()
    }

    pub fn is_empty(&self) -> bool {
        self.list.is_empty()
    }

    pub fn current(&self) -> Option<&QueueItem> {
        self.list.get(self.ptr)
    }

    pub fn has_current(&self) -> bool {
        self.ptr < self.list.len()
    }

    pub fn index_of(&self, key: &str) -> Option<usize> {
        self.list.iter().position(|i| i.key == key)
    }

    /// Where "directly after the current entry" is.
    fn after_current(&self) -> usize {
        if self.has_current() {
            self.ptr + 1
        } else {
            self.ptr
        }
    }

    /// One past the "Next in queue" group: the end of the run of `queued`
    /// entries directly after the current one.
    pub fn group_end(&self) -> usize {
        let start = self.after_current();
        let mut end = start;
        if self.has_current() {
            while end < self.list.len() && self.list[end].queued {
                end += 1;
            }
        }
        end
    }

    /// The nearest playable entry after the pointer, wrapping under Repeat All
    /// (to the current entry itself last). Also returns the unavailable
    /// entries passed over.
    fn forward(&self, wrap: bool) -> Option<(usize, Vec<usize>)> {
        let n = self.list.len();
        let mut passed = vec![];
        let start = self.after_current();
        let wrap_end = if self.has_current() { self.ptr + 1 } else { n };
        let order = (start..n).chain((0..wrap_end).filter(|_| wrap));
        for i in order {
            if self.list[i].unavailable {
                passed.push(i);
            } else {
                return Some((i, passed));
            }
        }
        None
    }

    /// The nearest playable entry before the pointer, wrapping under Repeat
    /// All to the last one (and the current entry itself last).
    fn backward(&self, wrap: bool) -> Option<usize> {
        let n = self.list.len();
        let behind = (0..self.ptr.min(n)).rev();
        let wrap_from = if self.has_current() { self.ptr } else { n };
        let wrapped = (wrap_from..n).rev().filter(|_| wrap);
        behind.chain(wrapped).find(|&i| !self.list[i].unavailable)
    }

    /// Drop the oldest history beyond `cap`.
    fn trim_history(&mut self, cap: usize) {
        if self.ptr > cap {
            let excess = self.ptr - cap;
            self.list.drain(..excess);
            self.ptr -= excess;
        }
    }

    /// Keep the list to `max` entries: the oldest history goes first, then
    /// the far end of the list.
    fn cap_len(&mut self, max: usize) {
        if self.list.len() <= max {
            return;
        }
        let excess = (self.list.len() - max).min(self.ptr);
        self.list.drain(..excess);
        self.ptr -= excess;
        // Still too long only when all history went (the pointer is at 0).
        self.list.truncate(max);
    }

    /// While shuffle is off, the list order is the unshuffled order.
    fn renumber(&mut self) {
        for (i, item) in self.list.iter_mut().enumerate() {
            item.ordinal = i as u32;
        }
    }

    /// Give `count` entries just inserted at `at` the ordinals right after
    /// the entry before them, shifting every later ordinal up (shuffle on).
    fn ordinals_after_previous(&mut self, at: usize, count: usize) {
        let base = match at.checked_sub(1).and_then(|p| self.list.get(p)) {
            Some(prev) => prev.ordinal.saturating_add(1),
            None => 0,
        };
        let range = at..at + count;
        for (i, item) in self.list.iter_mut().enumerate() {
            if !range.contains(&i) && item.ordinal >= base {
                item.ordinal = item.ordinal.saturating_add(count as u32);
            }
        }
        for (k, i) in range.enumerate() {
            if let Some(item) = self.list.get_mut(i) {
                item.ordinal = base.saturating_add(k as u32);
            }
        }
    }

    /// Ordinals at the end of the unshuffled order for `count` entries at `at`.
    fn ordinals_at_end(&mut self, at: usize, count: usize) {
        let others = self
            .list
            .iter()
            .enumerate()
            .filter(|(i, _)| !(at..at + count).contains(i))
            .map(|(_, it)| it.ordinal)
            .max();
        let base = others.map(|m| m.saturating_add(1)).unwrap_or(0);
        for (k, i) in (at..at + count).enumerate() {
            if let Some(item) = self.list.get_mut(i) {
                item.ordinal = base.saturating_add(k as u32);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Derived view
// ---------------------------------------------------------------------------

/// The queue as the UI sees it, before track summaries are resolved: a
/// window of rows from the flat list plus exact totals.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct DerivedQueue {
    pub context_label: Option<String>,
    /// `(entry, in the group)`, in list order.
    pub entries: Vec<(QueueItem, bool)>,
    pub window_start: u32,
    pub total: u32,
    pub history_count: u32,
    pub current_index: Option<u32>,
    pub group_count: u32,
    pub total_upcoming: u32,
    pub current: Option<QueueItem>,
    pub shuffle: bool,
    pub repeat: RepeatMode,
    pub autoplay: bool,
    /// See [`crate::api::QueueView::wrap_next`].
    pub wrap_next: Option<QueueItem>,
    /// See [`crate::api::QueueView::previous`].
    pub previous: Option<QueueItem>,
}

impl DerivedQueue {
    /// Resolve into the wire shape. `resolve` maps a track id to its summary
    /// (the actor answers from the mirror; unknown ids get a bare summary).
    pub fn into_view(self, mut resolve: impl FnMut(&TrackId) -> TrackSummary) -> QueueView {
        let mut entry = |item: QueueItem, in_group: bool| crate::api::QueueEntry {
            track: resolve(&item.track_id),
            autoplay: matches!(item.source, QueueSource::Autoplay { .. }),
            in_group,
            item,
        };
        QueueView {
            context_label: self.context_label,
            entries: self.entries.into_iter().map(|(i, g)| entry(i, g)).collect(),
            window_start: self.window_start,
            total: self.total,
            history_count: self.history_count,
            current_index: self.current_index,
            group_count: self.group_count,
            total_upcoming: self.total_upcoming,
            current: self.current.map(|i| entry(i, false)),
            shuffle: self.shuffle,
            repeat: self.repeat,
            autoplay: self.autoplay,
            wrap_next: self.wrap_next.map(|i| entry(i, false)),
            previous: self.previous.map(|i| entry(i, false)),
        }
    }

    /// The window's entries after the current one, in play order.
    pub fn upcoming(&self) -> impl Iterator<Item = &QueueItem> {
        let first = self.current_index.map(|c| c + 1).unwrap_or(self.total);
        let start = self.window_start;
        self.entries
            .iter()
            .enumerate()
            .filter(move |(i, _)| start + *i as u32 >= first)
            .map(|(_, (item, _))| item)
    }

    /// The window's entries behind the pointer, oldest first.
    pub fn history(&self) -> impl Iterator<Item = &QueueItem> {
        let end = self.history_count;
        let start = self.window_start;
        self.entries
            .iter()
            .enumerate()
            .filter(move |(i, _)| start + (*i as u32) < end)
            .map(|(_, (item, _))| item)
    }
}

/// The whole list as the UI sees it.
pub fn derive(doc: &SessionDocument) -> DerivedQueue {
    derive_range(doc, 0, usize::MAX)
}

/// The view with at most `before` rows of history and `after` rows past the
/// current entry: what the UI is sent on every change. Totals are exact.
pub fn derive_window(doc: &SessionDocument, before: usize, after: usize) -> DerivedQueue {
    let ptr = doc.history.len();
    let start = ptr.saturating_sub(before);
    let count = (ptr - start).saturating_add(1).saturating_add(after);
    derive_range(doc, start, count)
}

/// Rows `start..start + count` of the list (clamped), with exact totals.
pub fn derive_range(doc: &SessionDocument, start: usize, count: usize) -> DerivedQueue {
    let h = doc.history.len();
    let has_current = doc.current.is_some();
    let total = h + usize::from(has_current) + doc.upcoming.len();
    let group_count = if has_current {
        doc.upcoming.iter().take_while(|i| i.queued).count()
    } else {
        0
    };
    let start = start.min(total);
    let end = start.saturating_add(count).min(total);
    let item_at = |i: usize| -> &QueueItem {
        if i < h {
            &doc.history[i]
        } else if has_current && i == h {
            doc.current.as_ref().expect("checked")
        } else {
            &doc.upcoming[i - h - usize::from(has_current)]
        }
    };
    let entries = (start..end)
        .map(|i| {
            let in_group = has_current && i > h && i <= h + group_count;
            (item_at(i).clone(), in_group)
        })
        .collect();
    let wrap = doc.repeat == RepeatMode::All;
    let flat_view = FlatView { doc };
    let wrap_next = if wrap && !doc.upcoming.iter().any(|i| !i.unavailable) {
        flat_view
            .first_playable_through_current()
            .map(|i| item_at(i).clone())
    } else {
        None
    };
    let previous = flat_view.previous(wrap).map(|i| item_at(i).clone());
    DerivedQueue {
        context_label: doc.context.as_ref().map(|c| c.label.clone()),
        entries,
        window_start: start as u32,
        total: total as u32,
        history_count: h as u32,
        current_index: has_current.then_some(h as u32),
        group_count: group_count as u32,
        total_upcoming: doc.upcoming.len() as u32,
        current: doc.current.clone(),
        shuffle: doc.shuffle,
        repeat: doc.repeat,
        autoplay: doc.autoplay,
        wrap_next,
        previous,
    }
}

/// Searches over a document's runs without joining them.
struct FlatView<'a> {
    doc: &'a SessionDocument,
}

impl FlatView<'_> {
    /// Under Repeat All with nothing playable ahead: the first playable entry
    /// from the top, the current one last.
    fn first_playable_through_current(&self) -> Option<usize> {
        let h = self.doc.history.len();
        if let Some(i) = self.doc.history.iter().position(|i| !i.unavailable) {
            return Some(i);
        }
        self.doc
            .current
            .as_ref()
            .filter(|c| !c.unavailable)
            .map(|_| h)
    }

    /// What Previous lands on: the nearest playable entry behind the pointer,
    /// or under Repeat All the last playable one ahead of it.
    fn previous(&self, wrap: bool) -> Option<usize> {
        let h = self.doc.history.len();
        if let Some(i) = self.doc.history.iter().rposition(|i| !i.unavailable) {
            return Some(i);
        }
        if !wrap {
            return None;
        }
        let first_upcoming = h + usize::from(self.doc.current.is_some());
        self.doc
            .upcoming
            .iter()
            .rposition(|i| !i.unavailable)
            .map(|i| first_upcoming + i)
    }
}

/// Top-level document field (kept in [`SessionDocument::extra`]) listing the
/// keys of items marked unavailable only because the owning device was
/// offline.
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

/// Keep the offline set to items that are still in the list and still marked.
fn prune_offline_skipped(doc: &mut SessionDocument) {
    if !doc.extra.contains_key(OFFLINE_SKIPPED_FIELD) {
        return;
    }
    let marked: HashSet<&str> = doc
        .history
        .iter()
        .chain(doc.current.iter())
        .chain(doc.upcoming.iter())
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
    let flat = Flat::take(&mut next);
    let mut r = Reducer {
        doc: &mut next,
        q: flat,
        ctx,
        effects: &mut effects,
        start_position: &mut start_position,
    };
    r.repair_duplicate_keys();
    r.apply(op)?;
    r.q.trim_history(HISTORY_CAP);
    r.q.cap_len(MAX_QUEUE_LEN);
    if !r.doc.shuffle {
        r.q.renumber();
    }
    let Reducer { q, .. } = r;
    q.put(&mut next);
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
    /// Everything but the list (moved out into `q` until the end).
    doc: &'a mut SessionDocument,
    q: Flat,
    ctx: &'a ReduceCtx<'c>,
    effects: &'a mut Vec<Effect>,
    start_position: &'a mut Ms,
}

impl Reducer<'_, '_> {
    fn apply(&mut self, op: QueueOp) -> Result<(), ReduceError> {
        match op {
            QueueOp::PlayContext { args } => {
                self.play_context(
                    args.context,
                    args.start_index,
                    args.shuffle,
                    args.save_outgoing,
                    args.start_position_ms.unwrap_or(0),
                );
                Ok(())
            }
            QueueOp::PlayTracks {
                server_id,
                track_ids,
                start_index,
                label,
                shuffle,
                save_outgoing,
                start_position_ms,
            } => {
                let context = adhoc_context(server_id, label, track_ids);
                self.play_context(
                    context,
                    Some(start_index),
                    shuffle,
                    save_outgoing,
                    start_position_ms,
                );
                Ok(())
            }
            QueueOp::PlayNext {
                server_id,
                track_ids,
                keys,
                queued,
            } => self.queue(server_id, track_ids, keys, QueuePlacement::Next, queued),
            QueueOp::AddToQueue {
                server_id,
                track_ids,
                keys,
            } => self.queue(server_id, track_ids, keys, QueuePlacement::Group, true),
            QueueOp::PlayLater {
                server_id,
                track_ids,
                keys,
            } => self.queue(server_id, track_ids, keys, QueuePlacement::Later, false),
            QueueOp::InsertTracks {
                server_id: _,
                track_ids,
                keys,
                after,
                in_group,
            } => self.insert_after(track_ids, keys, &after, in_group),
            QueueOp::JumpToQueueItem { key } => self.jump(&key),
            QueueOp::RemoveQueueItems { keys } => self.remove(&keys),
            QueueOp::MoveQueueItem {
                key,
                after,
                in_group,
            } => self.move_item(&key, &after, in_group),
            QueueOp::ClearQueue => {
                self.clear_queue();
                Ok(())
            }
            QueueOp::ClearQueueGroup => {
                let (start, end) = (self.q.after_current(), self.q.group_end());
                self.q.list.drain(start..end);
                Ok(())
            }
            QueueOp::SetShuffle { enabled } => {
                if enabled {
                    if !self.doc.shuffle {
                        self.shuffle_tail();
                    }
                } else if self.doc.shuffle {
                    self.unshuffle_tail();
                }
                Ok(())
            }
            QueueOp::Reshuffle => {
                self.shuffle_tail();
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
            QueueOp::Next => {
                if !self.advance() {
                    self.end_of_queue();
                }
                Ok(())
            }
            QueueOp::Previous => {
                self.previous();
                Ok(())
            }
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
                if self.q.list.iter().any(|i| i.key == key && i.unavailable) {
                    let mut offline = offline_skipped(self.doc);
                    offline.insert(key);
                    set_offline_skipped(self.doc, &offline);
                }
                Ok(())
            }
            QueueOp::ClearOfflineSkips => {
                let offline = offline_skipped(self.doc);
                for item in self.q.list.iter_mut() {
                    if offline.contains(&item.key) {
                        item.unavailable = false;
                    }
                }
                set_offline_skipped(self.doc, &BTreeSet::new());
                Ok(())
            }
            QueueOp::AppendAutoplay { items } => {
                let at = self.q.len();
                let count = items.len();
                for it in items {
                    let key = self.ctx.entropy.next_key();
                    self.q.list.push(QueueItem {
                        key,
                        track_id: it.track_id,
                        source: QueueSource::Autoplay {
                            provider: it.provider,
                            reason: it.reason,
                            score: it.score,
                        },
                        unavailable: false,
                        added: false,
                        queued: false,
                        ordinal: 0,
                    });
                }
                self.q.ordinals_at_end(at, count);
                Ok(())
            }
            QueueOp::TrackEnded => {
                if self.doc.repeat == RepeatMode::One && self.q.has_current() {
                    self.effects.push(Effect::RestartCurrent);
                } else if !self.advance() {
                    self.end_of_queue();
                }
                Ok(())
            }
            QueueOp::RestoreSavedQueue { id, tracks } => self.restore(&id, tracks),
            QueueOp::SetContextTracks { tracks } => {
                let Some(c) = self.doc.context.as_mut() else {
                    return Ok(());
                };
                let mut tracks = tracks;
                window_around(&mut tracks, self.q.current());
                c.tracks = tracks.clone();
                // A list waiting for its tracks (a context started or restored
                // without them) is built from them, around its current entry.
                if self.q.len() <= 1 {
                    self.expand_from_tracks(&tracks);
                }
                Ok(())
            }
            QueueOp::SaveCurrentQueue { pinned } => {
                let id = self.ctx.entropy.next_key();
                if let Some(mut s) = self.snapshot(id) {
                    s.pinned = pinned;
                    saved::upsert(&mut self.doc.saved_queues, s, &saved_policy(), self.ctx.now);
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
                saved::enforce(&mut self.doc.saved_queues, &saved_policy(), self.ctx.now);
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
            QueueOp::RenameQueue { id, label } => self.rename(id, label),
            QueueOp::TouchSavedQueue { id } => {
                let q = self
                    .doc
                    .saved_queues
                    .iter_mut()
                    .find(|q| q.id == id)
                    .ok_or(ReduceError::UnknownSavedQueue(id))?;
                q.last_interacted_at = self.ctx.now;
                q.updated_at = self.ctx.now;
                saved::enforce(&mut self.doc.saved_queues, &saved_policy(), self.ctx.now);
                Ok(())
            }
            QueueOp::MergeSavedQueues { remote } => {
                self.doc.saved_queues = saved::merge_saved_queues(&self.doc.saved_queues, &remote);
                saved::enforce(&mut self.doc.saved_queues, &saved_policy(), self.ctx.now);
                Ok(())
            }
        }
    }

    // -- helpers -----------------------------------------------------------

    fn fresh(&self, track_id: TrackId, source: QueueSource, added: bool) -> QueueItem {
        QueueItem {
            key: self.ctx.entropy.next_key(),
            track_id,
            source,
            unavailable: false,
            added,
            queued: false,
            ordinal: 0,
        }
    }

    /// Entries for a context's tracks, ordinals by index.
    fn context_entries(&self, tracks: &[TrackId]) -> Vec<QueueItem> {
        tracks
            .iter()
            .enumerate()
            .map(|(i, t)| {
                let mut item =
                    self.fresh(t.clone(), QueueSource::Context { index: i as u32 }, false);
                item.ordinal = i as u32;
                item
            })
            .collect()
    }

    /// Snapshot of the live queue (the list is in `q` while reducing).
    fn snapshot(&self, id: String) -> Option<SavedQueue> {
        saved::snapshot_parts(
            self.doc,
            &self.q.list,
            self.q.ptr,
            self.ctx.position_ms,
            self.ctx.now,
            id,
        )
    }

    fn snapshot_outgoing(&mut self) {
        let id = self.ctx.entropy.next_key();
        if let Some(s) = self.snapshot(id) {
            saved::upsert(&mut self.doc.saved_queues, s, &saved_policy(), self.ctx.now);
        }
    }

    /// Earlier builds and peers could produce duplicate keys; a live document
    /// carrying them crashes clients that need unique keys. Rename all but
    /// the first of each before every op; the current entry keeps its key so
    /// playback isn't disturbed. Draws entropy only when a duplicate exists,
    /// and the entropy is per op, so every replica repairs the same way.
    fn repair_duplicate_keys(&mut self) {
        let mut seen: HashSet<QueueKey> = HashSet::new();
        if let Some(c) = self.q.current() {
            seen.insert(c.key.clone());
        }
        let ptr = self.q.ptr;
        let entropy = self.ctx.entropy;
        for (i, item) in self.q.list.iter_mut().enumerate() {
            if i == ptr {
                continue;
            }
            while !seen.insert(item.key.clone()) {
                item.key = entropy.next_key();
            }
        }
    }

    // -- replacing the list --------------------------------------------------

    fn play_context(
        &mut self,
        context: QueueContext,
        start_index: Option<u32>,
        shuffle: bool,
        save_outgoing: bool,
        start_position_ms: Ms,
    ) {
        if save_outgoing {
            self.snapshot_outgoing();
        }
        let mut context = context;
        let window = window_tracks(context.tracks.len(), start_index.unwrap_or(0) as usize);
        if window.len() < context.tracks.len() {
            context.tracks = context.tracks[window.clone()].to_vec();
        }
        let start_index = start_index.map(|s| (s as usize).saturating_sub(window.start) as u32);
        let n = context.tracks.len();
        let mut list = self.context_entries(&context.tracks);
        let start = start_index.map(|s| (s as usize).min(n.saturating_sub(1)));
        let ptr = if shuffle && n > 0 {
            let seed = self.ctx.entropy.next_seed();
            match start {
                Some(s) => {
                    // The chosen track plays first; the rest follow shuffled.
                    let chosen = list.remove(s);
                    shuffle_in_place(&mut list, seed);
                    list.insert(0, chosen);
                }
                None => shuffle_in_place(&mut list, seed),
            }
            0
        } else {
            start.unwrap_or(0)
        };
        self.doc.context = Some(context);
        self.doc.shuffle = shuffle;
        self.q = Flat { list, ptr };
        if self.q.has_current() {
            *self.start_position = start_position_ms;
        }
        self.effects.push(Effect::ContextReplaced);
    }

    fn clear_queue(&mut self) {
        self.snapshot_outgoing();
        let server_id = self
            .doc
            .context
            .as_ref()
            .map(|c| c.server_id.clone())
            .unwrap_or_default();
        self.doc.shuffle = false;
        match self.q.current().cloned() {
            Some(mut cur) => {
                cur.source = QueueSource::Context { index: 0 };
                cur.queued = false;
                cur.ordinal = 0;
                self.doc.context = Some(adhoc_context(
                    server_id,
                    ADHOC_QUEUE_LABEL.into(),
                    vec![cur.track_id.clone()],
                ));
                self.q = Flat {
                    list: vec![cur],
                    ptr: 0,
                };
            }
            None => {
                self.doc.context = None;
                self.q = Flat::default();
            }
        }
        self.effects.push(Effect::ContextReplaced);
    }

    fn restore(&mut self, id: &str, tracks: Option<Vec<TrackId>>) -> Result<(), ReduceError> {
        let entry = self
            .doc
            .saved_queues
            .iter()
            .find(|q| q.id == id)
            .cloned()
            .ok_or_else(|| ReduceError::UnknownSavedQueue(id.into()))?;
        self.snapshot_outgoing();
        // The entry may have moved (or been merged) by the snapshot's dedupe.
        if let Some(q) = self
            .doc
            .saved_queues
            .iter_mut()
            .find(|q| saved::identity_of(q) == saved::identity_of(&entry))
        {
            q.last_interacted_at = self.ctx.now;
            q.updated_at = self.ctx.now;
        }
        let mut context = entry.context.clone();
        if let Some(mut t) = tracks {
            window_around(&mut t, entry.current.as_ref());
            context.tracks = t;
        }
        let mut list = entry.history.clone();
        let ptr = list.len();
        list.extend(entry.current.clone());
        list.extend(entry.upcoming.iter().cloned());
        self.doc.context = Some(context.clone());
        self.doc.shuffle = entry.shuffle;
        self.doc.repeat = entry.repeat;
        self.q = Flat { list, ptr };
        self.repair_duplicate_keys();
        if self.q.len() <= 1 {
            // Stored without its entries (too long to keep): rebuild them
            // from the track list, or ask for it.
            if !context.tracks.is_empty() {
                self.expand_from_tracks(&context.tracks);
            } else if saved::is_id_referenced(&context.kind) {
                self.effects.push(Effect::ResolveContext { context });
            }
        }
        *self.start_position = entry.position_ms;
        self.effects.push(Effect::ContextReplaced);
        self.effects.push(Effect::PositionRestore {
            position_ms: entry.position_ms,
        });
        Ok(())
    }

    /// Build the list from a resolved track list around the current entry
    /// (kept, with its key), or from the top with nothing current.
    fn expand_from_tracks(&mut self, tracks: &[TrackId]) {
        if tracks.is_empty() {
            return;
        }
        let current = self.q.current().cloned();
        let mut list = self.context_entries(tracks);
        let ptr = match current {
            Some(cur) => {
                let at = match cur.source {
                    QueueSource::Context { index }
                        if tracks.get(index as usize) == Some(&cur.track_id) =>
                    {
                        Some(index as usize)
                    }
                    _ => tracks.iter().position(|t| *t == cur.track_id),
                };
                match at {
                    Some(i) => {
                        let ordinal = list[i].ordinal;
                        list[i] = QueueItem { ordinal, ..cur };
                        i
                    }
                    None => {
                        list.insert(0, cur);
                        0
                    }
                }
            }
            None => 0,
        };
        self.q = Flat { list, ptr };
        if self.doc.shuffle {
            self.q.renumber();
            self.shuffle_tail();
        }
    }

    fn rename(&mut self, id: Option<String>, label: String) -> Result<(), ReduceError> {
        let label = label.trim().to_string();
        if label.is_empty() {
            return Err(ReduceError::EmptyName);
        }
        // The playing queue and its saved entry are one queue to the user: a saved queue
        // is re-snapshotted from the live context when it is left (matched by identity,
        // not label), so renaming either renames both, or the old name would come back.
        let identity = match &id {
            None => saved::context_identity(self.doc.context.as_ref().ok_or(ReduceError::Empty)?),
            Some(id) => saved::context_identity(
                &self
                    .doc
                    .saved_queues
                    .iter()
                    .find(|q| &q.id == id)
                    .ok_or_else(|| ReduceError::UnknownSavedQueue(id.clone()))?
                    .context,
            ),
        };
        if let Some(ctx) = self.doc.context.as_mut() {
            if saved::context_identity(ctx) == identity {
                ctx.label = label.clone();
            }
        }
        let now = self.ctx.now;
        for q in self.doc.saved_queues.iter_mut() {
            if id.as_deref().is_some_and(|id| q.id == id)
                || saved::context_identity(&q.context) == identity
            {
                q.label = label.clone();
                q.context.label = label.clone();
                q.updated_at = now;
            }
        }
        Ok(())
    }

    // -- queueing ----------------------------------------------------------

    fn queue(
        &mut self,
        server_id: ServerId,
        track_ids: Vec<TrackId>,
        keys: Vec<QueueKey>,
        placement: QueuePlacement,
        queued: bool,
    ) -> Result<(), ReduceError> {
        if track_ids.is_empty() && keys.is_empty() {
            return Ok(());
        }
        if self.doc.context.is_none() && self.q.is_empty() && keys.is_empty() {
            // Nothing to queue behind: this becomes the queue and starts playing.
            let context = adhoc_context(server_id, ADHOC_QUEUE_LABEL.into(), track_ids);
            self.play_context(context, Some(0), false, false, 0);
            return Ok(());
        }
        let mut items = self.take_for_queueing(track_ids, &keys)?;
        if items.is_empty() {
            return Ok(());
        }
        let has_group = self.q.group_end() > self.q.after_current();
        let flag = match placement {
            // A group already after the current entry stays attached to it.
            QueuePlacement::Next => queued || has_group,
            QueuePlacement::Group => true,
            QueuePlacement::Later => false,
        };
        for item in items.iter_mut() {
            item.queued = flag;
        }
        let at = match placement {
            QueuePlacement::Next => self.q.after_current(),
            QueuePlacement::Group => self.q.group_end(),
            QueuePlacement::Later => self.q.len(),
        };
        let count = items.len();
        self.q.list.splice(at..at, items);
        if self.doc.shuffle {
            match placement {
                QueuePlacement::Later => self.q.ordinals_at_end(at, count),
                _ => self.q.ordinals_after_previous(at, count),
            }
        }
        if !self.q.has_current() {
            // Queued onto a list whose current entry was removed: the first
            // of them is current now.
            self.q.ptr = at.min(self.q.ptr);
        }
        Ok(())
    }

    /// The entries a queueing op puts somewhere: those named by key (ahead
    /// of the pointer they are taken out to move; elsewhere they are copied,
    /// so history is never rewritten), then new ones for `track_ids`.
    fn take_for_queueing(
        &mut self,
        track_ids: Vec<TrackId>,
        keys: &[QueueKey],
    ) -> Result<Vec<QueueItem>, ReduceError> {
        let mut items: Vec<QueueItem> = vec![];
        let mut found_any = false;
        for key in keys {
            let Some(i) = self.q.index_of(key) else {
                continue;
            };
            found_any = true;
            if i > self.q.ptr && self.q.has_current() {
                items.push(self.q.list.remove(i));
            } else {
                let track = self.q.list[i].track_id.clone();
                items.push(self.fresh(track, QueueSource::Inserted, true));
            }
        }
        if !keys.is_empty() && !found_any && track_ids.is_empty() {
            return Err(ReduceError::UnknownKey(keys.join(",")));
        }
        for t in track_ids {
            items.push(self.fresh(t, QueueSource::Inserted, true));
        }
        Ok(items)
    }

    /// [`QueueOp::InsertTracks`]: the entries go directly after `after`,
    /// clamped to the group like a move (an `in_group` drop past the group's
    /// end lands at its end, an out-of-group one inside it just after it).
    fn insert_after(
        &mut self,
        track_ids: Vec<TrackId>,
        keys: Vec<QueueKey>,
        after: &str,
        in_group: bool,
    ) -> Result<(), ReduceError> {
        let a = self
            .q
            .index_of(after)
            .ok_or_else(|| ReduceError::UnknownKey(after.into()))?;
        if !self.q.has_current() || a < self.q.ptr {
            return Err(ReduceError::BadMove);
        }
        if track_ids.is_empty() && keys.is_empty() {
            return Ok(());
        }
        // Where `after` ends up once the moved entries are out: just after
        // the entries kept up to it (if `after` itself moves, its old place).
        let moving: HashSet<&str> = keys
            .iter()
            .filter_map(|k| self.q.index_of(k).filter(|&i| i > self.q.ptr))
            .map(|i| self.q.list[i].key.as_str())
            .collect();
        let target = self.q.list[..=a]
            .iter()
            .filter(|i| !moving.contains(i.key.as_str()))
            .count();
        let mut items = self.take_for_queueing(track_ids, &keys)?;
        if items.is_empty() {
            return Ok(());
        }
        let group_end = self.q.group_end();
        let at = if in_group {
            target.min(group_end)
        } else {
            target.max(group_end)
        };
        for item in items.iter_mut() {
            item.queued = in_group;
        }
        let count = items.len();
        self.q.list.splice(at..at, items);
        if self.doc.shuffle {
            self.q.ordinals_after_previous(at, count);
        }
        Ok(())
    }

    // -- shuffle -------------------------------------------------------------

    /// Shuffle the entries after the group. Queued entries further down (a
    /// group left behind by going back, which re-forms on return) keep their
    /// places, so shuffling never changes what is in the group.
    fn shuffle_tail(&mut self) {
        let seed = self.ctx.entropy.next_seed();
        self.reorder_tail(|items| shuffle_in_place(items, seed));
        self.doc.shuffle = true;
    }

    /// Put the entries after the group back in unshuffled order, continuing
    /// from the current entry's place in it (the ones before it follow).
    fn unshuffle_tail(&mut self) {
        let pivot = self.q.current().map(|c| c.ordinal);
        self.reorder_tail(|items| {
            items.sort_by_key(|i| (pivot.is_some_and(|p| i.ordinal < p), i.ordinal))
        });
        self.doc.shuffle = false;
    }

    /// Reorder the unqueued entries after the group among their own slots.
    fn reorder_tail(&mut self, f: impl FnOnce(&mut [QueueItem])) {
        let from = self.q.group_end();
        let tail: Vec<QueueItem> = self.q.list.drain(from..).collect();
        let fixed: Vec<bool> = tail.iter().map(|i| i.queued).collect();
        let (mut queued, mut movable): (Vec<QueueItem>, Vec<QueueItem>) =
            tail.into_iter().partition(|i| i.queued);
        f(&mut movable);
        queued.reverse();
        movable.reverse();
        for is_fixed in fixed {
            let next = if is_fixed {
                queued.pop()
            } else {
                movable.pop()
            };
            self.q.list.extend(next);
        }
    }

    // -- navigation ----------------------------------------------------------

    /// Move to the next playable entry. Returns false (and leaves the pointer
    /// alone) when there is none.
    fn advance(&mut self) -> bool {
        let wrap = self.doc.repeat == RepeatMode::All;
        let Some((to, passed)) = self.q.forward(wrap) else {
            return false;
        };
        for i in passed {
            self.effects.push(Effect::Skipped {
                key: self.q.list[i].key.clone(),
            });
        }
        if to == self.q.ptr {
            // Repeat All around a one-song list.
            self.effects.push(Effect::RestartCurrent);
        }
        self.q.ptr = to;
        true
    }

    fn end_of_queue(&mut self) {
        if self.doc.autoplay && self.q.has_current() {
            self.effects.push(Effect::NeedsAutoplay);
        } else {
            self.effects.push(Effect::Stopped);
        }
    }

    fn previous(&mut self) {
        let wrap = self.doc.repeat == RepeatMode::All;
        match self.q.backward(wrap) {
            Some(i) if i != self.q.ptr => self.q.ptr = i,
            _ if self.q.has_current() => self.effects.push(Effect::RestartCurrent),
            _ => {}
        }
    }

    fn jump(&mut self, key: &str) -> Result<(), ReduceError> {
        let i = self
            .q
            .index_of(key)
            .ok_or_else(|| ReduceError::UnknownKey(key.into()))?;
        self.q.list[i].unavailable = false;
        if i == self.q.ptr {
            self.effects.push(Effect::RestartCurrent);
        }
        self.q.ptr = i;
        Ok(())
    }

    fn skip_unavailable(&mut self, key: &str) -> Result<(), ReduceError> {
        let i = self
            .q
            .index_of(key)
            .ok_or_else(|| ReduceError::UnknownKey(key.into()))?;
        self.q.list[i].unavailable = true;
        if i == self.q.ptr {
            self.effects.push(Effect::Skipped { key: key.into() });
            if !self.advance() {
                self.end_of_queue();
            }
        }
        Ok(())
    }

    // -- editing -------------------------------------------------------------

    fn remove(&mut self, keys: &[QueueKey]) -> Result<(), ReduceError> {
        let wanted: HashSet<&str> = keys.iter().map(|k| k.as_str()).collect();
        let had_current = self.q.has_current();
        let current_removed = self
            .q
            .current()
            .is_some_and(|c| wanted.contains(c.key.as_str()));
        let before_ptr = self.q.list[..self.q.ptr.min(self.q.len())]
            .iter()
            .filter(|i| wanted.contains(i.key.as_str()))
            .count();
        let before = self.q.len();
        self.q.list.retain(|i| !wanted.contains(i.key.as_str()));
        if self.q.len() == before {
            return Err(ReduceError::UnknownKey(keys.join(",")));
        }
        self.q.ptr -= before_ptr;
        if current_removed {
            // The next playable entry takes its place; the unavailable ones
            // on the way end up behind the pointer.
            let next = (self.q.ptr..self.q.len()).find(|&i| !self.q.list[i].unavailable);
            match next {
                Some(i) => self.q.ptr = i,
                None => {
                    self.q.ptr = self.q.len();
                    if had_current {
                        self.effects.push(Effect::Stopped);
                    }
                }
            }
        }
        Ok(())
    }

    fn move_item(&mut self, key: &str, after: &str, in_group: bool) -> Result<(), ReduceError> {
        let i = self
            .q
            .index_of(key)
            .ok_or_else(|| ReduceError::UnknownKey(key.into()))?;
        let a = self
            .q
            .index_of(after)
            .ok_or_else(|| ReduceError::UnknownKey(after.into()))?;
        if !self.q.has_current() || i <= self.q.ptr || a < self.q.ptr {
            return Err(ReduceError::BadMove);
        }
        let mut item = self.q.list.remove(i);
        // Where `after` is now; dropping onto itself keeps its place.
        let target = if a == i {
            i
        } else if a > i {
            a
        } else {
            a + 1
        };
        let group_end = self.q.group_end();
        let at = if in_group {
            target.min(group_end)
        } else {
            target.max(group_end)
        };
        item.queued = in_group;
        self.q.list.insert(at, item);
        if self.doc.shuffle {
            self.q.ordinals_after_previous(at, 1);
        }
        Ok(())
    }
}

/// Cut a track list longer than [`MAX_QUEUE_LEN`] to the window around
/// `current`'s track (its context index when that still matches, else its
/// first occurrence).
pub fn window_around(tracks: &mut Vec<TrackId>, current: Option<&QueueItem>) {
    if tracks.len() <= MAX_QUEUE_LEN {
        return;
    }
    let anchor = current
        .and_then(|cur| match cur.source {
            QueueSource::Context { index } if tracks.get(index as usize) == Some(&cur.track_id) => {
                Some(index as usize)
            }
            _ => tracks.iter().position(|t| *t == cur.track_id),
        })
        .unwrap_or(0);
    let window = window_tracks(tracks.len(), anchor);
    tracks.truncate(window.end);
    tracks.drain(..window.start);
}

/// An ad-hoc context over a track list.
fn adhoc_context(server_id: ServerId, label: String, tracks: Vec<TrackId>) -> QueueContext {
    QueueContext {
        server_id,
        kind: ContextKind::AdHoc {
            label: label.clone(),
        },
        label,
        sort: SortOrder::Default,
        tracks,
    }
}

#[cfg(test)]
mod tests;
