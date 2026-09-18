//! Global undo: command objects with inverses, coalescing, CAS inverses.
//! Owner: core-session.
//!
//! # Entry points for the actor
//!
//! - [`UndoStack::new(device_id, max_bytes)`](UndoStack::new) — one per core.
//! - [`UndoStack::push(cmd, target, now)`](UndoStack::push) after every
//!   undoable action, with a boxed [`UndoableCommand`]:
//!   - [`SessionUndo`] (tier 1): document snapshot before/after; undo means
//!     `Session::replace(before)`. Shared across devices; entries stamped with
//!     their originating device so [`UndoStack::can_undo`] refuses to revert
//!     another device's deliberate action. Peers' entries arrive through
//!     [`UndoStack::push_from`].
//!   - [`RemoteUndo`] (tier 2): per-item prior state; undo yields
//!     [`UndoAction::Cas`] mutations the actor runs through the outbox, each a
//!     compare-and-swap (`expect` current value, `command` to apply). The
//!     actor folds results into a [`CasOutcome`] and attaches
//!     `outcome.note()` with [`UndoStack::set_note`].
//!   - Confirm-instead actions (tier 3) push nothing.
//! - [`UndoStack::undo`] / [`UndoStack::redo`] / [`UndoStack::undo_to`] return
//!   an [`UndoResult`] with the action to perform and the [`ActionTarget`]
//!   the command acted on, so the UI can restore the selection.
//! - [`UndoStack::state`] is the wire [`UndoState`] (history newest first).
//! - "Selection cleared · Restore": [`UndoStack::remember_cleared_selection`]
//!   and [`UndoStack::restore_selection`].
//!
//! Rapid repeats of the same kind on the same target (dragging a rating,
//! nudging a track, holding a key) coalesce into one entry when they land
//! within [`DEFAULT_COALESCE_WINDOW_MS`] of the previous push. The stack is
//! bounded by estimated serialised bytes, not entry count. A new action
//! clears the redo stack.

use std::any::Any;
use std::collections::HashMap;

use crate::api::{ActionTarget, Command, DeviceId, EpochMs, RatingTarget, SessionDocument, UndoEntry, UndoState};

/// Default byte budget for the whole stack (undo + redo).
pub const DEFAULT_MAX_BYTES: usize = 4 * 1024 * 1024;
/// Repeats of the same command kind on the same target within this window coalesce.
pub const DEFAULT_COALESCE_WINDOW_MS: f64 = 800.0;
/// How many entries [`UndoStack::state`] lists for the history sheet.
pub const HISTORY_SHEET_LIMIT: usize = 50;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UndoTier {
    /// Queue ops, context replacement, reorder, shuffle, clear: a snapshot.
    SessionState,
    /// Ratings, loves, playlist edits: per-item prior state, CAS inverse.
    RemoteMutation,
}

/// What performing (redo) or inverting (undo) a command means.
#[derive(Debug, Clone, PartialEq)]
pub enum UndoAction {
    /// Adopt this document (`Session::replace`).
    RestoreDocument(SessionDocument),
    /// Run these compare-and-swap mutations through the outbox.
    Cas(Vec<CasMutation>),
    Nothing,
}

/// One compare-and-swap step: apply `command` only if the item's current
/// value still equals `expect`; otherwise it "changed elsewhere".
#[derive(Debug, Clone, PartialEq)]
pub struct CasMutation {
    pub target: RemoteTarget,
    pub expect: RemoteValue,
    pub command: Command,
}

#[derive(Debug, Clone, PartialEq)]
pub enum RemoteTarget {
    Track { id: String },
    Album { id: String },
    Artist { id: String },
    Playlist { id: String },
    /// A track's membership or position within a playlist.
    PlaylistTrack { playlist_id: String, track_id: String },
}

#[derive(Debug, Clone, PartialEq)]
pub enum RemoteValue {
    Rating(u32),
    Loved(bool),
    /// Present at `index` (or anywhere when `None`).
    Member { present: bool, index: Option<u32> },
    Position(u32),
    Meta { name: String, comment: Option<String>, public: Option<bool> },
}

/// One item of a remote mutation: what it was, what the command set.
#[derive(Debug, Clone, PartialEq)]
pub struct RemoteItem {
    pub target: RemoteTarget,
    pub prior: RemoteValue,
    pub set: RemoteValue,
}

impl RemoteItem {
    /// The command that moves the item from `from` to `to`, if expressible.
    pub fn command(&self, from: &RemoteValue, to: &RemoteValue) -> Option<Command> {
        use RemoteTarget as T;
        use RemoteValue as V;
        Some(match (&self.target, to) {
            (T::Track { id }, V::Rating(r)) => Command::SetRating { targets: vec![RatingTarget::Track { id: id.clone() }], rating: *r },
            (T::Album { id }, V::Rating(r)) => Command::SetRating { targets: vec![RatingTarget::Album { id: id.clone() }], rating: *r },
            (T::Track { id }, V::Loved(l)) => Command::SetLoved { targets: vec![RatingTarget::Track { id: id.clone() }], loved: *l },
            (T::Album { id }, V::Loved(l)) => Command::SetLoved { targets: vec![RatingTarget::Album { id: id.clone() }], loved: *l },
            (T::Artist { id }, V::Loved(l)) => Command::SetArtistLoved { artist_id: id.clone(), loved: *l },
            (T::PlaylistTrack { playlist_id, track_id }, V::Member { present: true, index }) => {
                Command::PlaylistAdd { playlist_id: playlist_id.clone(), track_ids: vec![track_id.clone()], at_index: *index }
            }
            (T::PlaylistTrack { playlist_id, .. }, V::Member { present: false, .. }) => {
                let index = match from {
                    V::Member { index: Some(i), .. } => *i,
                    _ => return None,
                };
                Command::PlaylistRemove { playlist_id: playlist_id.clone(), indices: vec![index] }
            }
            (T::PlaylistTrack { playlist_id, .. }, V::Position(to)) => {
                let from_index = match from {
                    V::Position(f) => *f,
                    _ => return None,
                };
                Command::PlaylistMove { playlist_id: playlist_id.clone(), from_index, to_index: *to }
            }
            (T::Playlist { id }, V::Meta { name, comment, public }) => {
                Command::RenamePlaylist { playlist_id: id.clone(), name: name.clone(), comment: comment.clone(), public: *public }
            }
            _ => return None,
        })
    }

    /// CAS that restores `prior` if the item still holds `set`.
    pub fn inverse(&self) -> Option<CasMutation> {
        Some(CasMutation { target: self.target.clone(), expect: self.set.clone(), command: self.command(&self.set, &self.prior)? })
    }

    /// CAS that re-applies `set` if the item still holds `prior`.
    pub fn redo(&self) -> Option<CasMutation> {
        Some(CasMutation { target: self.target.clone(), expect: self.prior.clone(), command: self.command(&self.prior, &self.set)? })
    }
}

/// Result of one CAS step, as the actor observed it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CasResult {
    Applied,
    /// The current value no longer matched: changed elsewhere, left alone.
    Skipped,
    /// The mutation itself failed.
    Failed,
}

/// Aggregates CAS results into the honest note: "undid 487 of 500, 13 changed elsewhere".
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CasOutcome {
    pub total: usize,
    pub applied: usize,
    pub skipped: usize,
    pub failed: usize,
}

impl CasOutcome {
    pub fn new(total: usize) -> CasOutcome {
        CasOutcome { total, ..Default::default() }
    }

    pub fn record(&mut self, result: CasResult) {
        match result {
            CasResult::Applied => self.applied += 1,
            CasResult::Skipped => self.skipped += 1,
            CasResult::Failed => self.failed += 1,
        }
    }

    pub fn is_complete(&self) -> bool {
        self.applied + self.skipped + self.failed >= self.total
    }

    /// `None` when everything was undone cleanly.
    pub fn note(&self) -> Option<String> {
        if self.skipped == 0 && self.failed == 0 {
            return None;
        }
        let mut s = format!("undid {} of {}", self.applied, self.total);
        if self.skipped > 0 {
            s.push_str(&format!(", {} changed elsewhere", self.skipped));
        }
        if self.failed > 0 {
            s.push_str(&format!(", {} failed", self.failed));
        }
        Some(s)
    }
}

/// A command object with an inverse.
pub trait UndoableCommand: Send + Sync {
    /// Coalescing identity (normally the action id).
    fn kind(&self) -> &str;
    fn label(&self) -> String;
    fn tier(&self) -> UndoTier;
    /// Perform (redo).
    fn apply(&self) -> UndoAction;
    /// Invert (undo).
    fn inverse(&self) -> UndoAction;
    /// Estimated serialised size, for the byte bound.
    fn size_bytes(&self) -> usize;
    /// Absorb a rapid repeat. Returns false when the two cannot merge.
    fn coalesce(&mut self, next: &dyn UndoableCommand) -> bool;
    fn as_any(&self) -> &dyn Any;
}

/// Tier 1: a document snapshot before and after.
#[derive(Debug, Clone, PartialEq)]
pub struct SessionUndo {
    pub kind: String,
    pub label: String,
    pub before: SessionDocument,
    pub after: SessionDocument,
}

impl SessionUndo {
    pub fn new(kind: impl Into<String>, label: impl Into<String>, before: SessionDocument, after: SessionDocument) -> SessionUndo {
        SessionUndo { kind: kind.into(), label: label.into(), before, after }
    }
}

fn json_size<T: serde::Serialize>(v: &T) -> usize {
    serde_json::to_vec(v).map(|b| b.len()).unwrap_or(0)
}

impl UndoableCommand for SessionUndo {
    fn kind(&self) -> &str {
        &self.kind
    }
    fn label(&self) -> String {
        self.label.clone()
    }
    fn tier(&self) -> UndoTier {
        UndoTier::SessionState
    }
    fn apply(&self) -> UndoAction {
        UndoAction::RestoreDocument(self.after.clone())
    }
    fn inverse(&self) -> UndoAction {
        UndoAction::RestoreDocument(self.before.clone())
    }
    fn size_bytes(&self) -> usize {
        json_size(&self.before) + json_size(&self.after)
    }
    fn coalesce(&mut self, next: &dyn UndoableCommand) -> bool {
        match next.as_any().downcast_ref::<SessionUndo>() {
            Some(n) => {
                self.after = n.after.clone();
                self.label = n.label.clone();
                true
            }
            None => false,
        }
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// Tier 2: per-item prior state with compare-and-swap inverses.
#[derive(Debug, Clone, PartialEq)]
pub struct RemoteUndo {
    pub kind: String,
    pub label: String,
    pub items: Vec<RemoteItem>,
}

impl RemoteUndo {
    pub fn new(kind: impl Into<String>, label: impl Into<String>, items: Vec<RemoteItem>) -> RemoteUndo {
        RemoteUndo { kind: kind.into(), label: label.into(), items }
    }
}

impl UndoableCommand for RemoteUndo {
    fn kind(&self) -> &str {
        &self.kind
    }
    fn label(&self) -> String {
        self.label.clone()
    }
    fn tier(&self) -> UndoTier {
        UndoTier::RemoteMutation
    }
    fn apply(&self) -> UndoAction {
        UndoAction::Cas(self.items.iter().filter_map(RemoteItem::redo).collect())
    }
    fn inverse(&self) -> UndoAction {
        UndoAction::Cas(self.items.iter().filter_map(RemoteItem::inverse).collect())
    }
    fn size_bytes(&self) -> usize {
        // Roughly what the three fields cost in JSON.
        self.items.len() * 96 + self.label.len() + self.kind.len()
    }
    fn coalesce(&mut self, next: &dyn UndoableCommand) -> bool {
        let Some(n) = next.as_any().downcast_ref::<RemoteUndo>() else { return false };
        // Same target: keep the first prior, take the latest set. New targets append.
        for item in &n.items {
            match self.items.iter_mut().find(|i| i.target == item.target) {
                Some(existing) => existing.set = item.set.clone(),
                None => self.items.push(item.clone()),
            }
        }
        self.label = n.label.clone();
        true
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
}

struct Entry {
    id: String,
    kind: String,
    device_id: DeviceId,
    at: EpochMs,
    target: ActionTarget,
    command: Box<dyn UndoableCommand>,
    note: Option<String>,
    bytes: usize,
}

impl Entry {
    fn wire(&self) -> UndoEntry {
        UndoEntry { id: self.id.clone(), label: self.command.label(), device_id: self.device_id.clone(), at: self.at, note: self.note.clone() }
    }
}

/// What the actor performs after `undo` / `redo`.
#[derive(Debug, Clone, PartialEq)]
pub struct UndoResult {
    pub entry_id: String,
    pub label: String,
    pub tier: UndoTier,
    pub action: UndoAction,
    /// The selection the command acted on; restore it.
    pub target: ActionTarget,
    pub device_id: DeviceId,
}

/// Shared undo can't revert another device's deliberate action.
pub fn can_undo_entry(entry: &UndoEntry, this_device: &str) -> bool {
    entry.device_id == this_device
}

pub struct UndoStack {
    device_id: DeviceId,
    max_bytes: usize,
    coalesce_window_ms: f64,
    undo: Vec<Entry>,
    redo: Vec<Entry>,
    cleared_selection: Option<ActionTarget>,
    seq: u64,
}

impl UndoStack {
    pub fn new(device_id: impl Into<DeviceId>, max_bytes: usize) -> UndoStack {
        UndoStack {
            device_id: device_id.into(),
            max_bytes,
            coalesce_window_ms: DEFAULT_COALESCE_WINDOW_MS,
            undo: vec![],
            redo: vec![],
            cleared_selection: None,
            seq: 0,
        }
    }

    pub fn with_coalesce_window(mut self, window_ms: f64) -> UndoStack {
        self.coalesce_window_ms = window_ms;
        self
    }

    pub fn device_id(&self) -> &str {
        &self.device_id
    }

    pub fn len(&self) -> usize {
        self.undo.len()
    }

    pub fn is_empty(&self) -> bool {
        self.undo.is_empty()
    }

    pub fn redo_len(&self) -> usize {
        self.redo.len()
    }

    /// Estimated bytes held by both stacks.
    pub fn bytes(&self) -> usize {
        self.undo.iter().chain(self.redo.iter()).map(|e| e.bytes).sum()
    }

    pub fn clear(&mut self) {
        self.undo.clear();
        self.redo.clear();
    }

    /// Record an action this device performed. Returns the entry id (an
    /// existing one when the action coalesced into it).
    pub fn push(&mut self, command: Box<dyn UndoableCommand>, target: ActionTarget, now: EpochMs) -> String {
        let device = self.device_id.clone();
        self.push_from(command, target, now, device)
    }

    /// Record an action originating from `device_id` (a peer's session-tier entry).
    pub fn push_from(&mut self, command: Box<dyn UndoableCommand>, target: ActionTarget, now: EpochMs, device_id: DeviceId) -> String {
        self.redo.clear();
        if let Some(top) = self.undo.last_mut() {
            let within = now - top.at <= self.coalesce_window_ms && now >= top.at;
            if within && top.kind == command.kind() && top.target == target && top.device_id == device_id && top.command.coalesce(command.as_ref()) {
                top.at = now;
                top.bytes = top.command.size_bytes();
                let id = top.id.clone();
                self.enforce_bytes();
                return id;
            }
        }
        self.seq += 1;
        let id = format!("u{}", self.seq);
        let bytes = command.size_bytes();
        self.undo.push(Entry { id: id.clone(), kind: command.kind().to_string(), device_id, at: now, target, command, note: None, bytes });
        self.enforce_bytes();
        id
    }

    fn enforce_bytes(&mut self) {
        while self.bytes() > self.max_bytes {
            if self.undo.len() > 1 {
                self.undo.remove(0);
            } else if !self.redo.is_empty() {
                self.redo.remove(0);
            } else {
                break;
            }
        }
    }

    /// Whether this device may undo the top entry.
    pub fn can_undo(&self) -> bool {
        self.undo.last().map(|e| e.device_id == self.device_id).unwrap_or(false)
    }

    pub fn can_redo(&self) -> bool {
        self.redo.last().map(|e| e.device_id == self.device_id).unwrap_or(false)
    }

    pub fn undo(&mut self) -> Option<UndoResult> {
        if !self.can_undo() {
            return None;
        }
        let entry = self.undo.pop()?;
        let result = UndoResult {
            entry_id: entry.id.clone(),
            label: entry.command.label(),
            tier: entry.command.tier(),
            action: entry.command.inverse(),
            target: entry.target.clone(),
            device_id: entry.device_id.clone(),
        };
        self.redo.push(entry);
        Some(result)
    }

    pub fn redo(&mut self) -> Option<UndoResult> {
        if !self.can_redo() {
            return None;
        }
        let entry = self.redo.pop()?;
        let result = UndoResult {
            entry_id: entry.id.clone(),
            label: entry.command.label(),
            tier: entry.command.tier(),
            action: entry.command.apply(),
            target: entry.target.clone(),
            device_id: entry.device_id.clone(),
        };
        self.undo.push(entry);
        Some(result)
    }

    /// Undo from the top down to and including `id` (the history sheet).
    /// Stops early at an entry another device owns. Results are in the order
    /// to perform them.
    pub fn undo_to(&mut self, id: &str) -> Vec<UndoResult> {
        let Some(pos) = self.undo.iter().position(|e| e.id == id) else { return vec![] };
        let mut out = vec![];
        while self.undo.len() > pos {
            match self.undo() {
                Some(r) => out.push(r),
                None => break,
            }
        }
        out
    }

    /// Attach a note ("undid 487 of 500, 13 changed elsewhere") to an entry
    /// on either stack.
    pub fn set_note(&mut self, id: &str, note: Option<String>) -> bool {
        match self.undo.iter_mut().chain(self.redo.iter_mut()).find(|e| e.id == id) {
            Some(e) => {
                e.note = note;
                true
            }
            None => false,
        }
    }

    /// A large selection was destroyed by something non-undoable: keep it for
    /// the "Selection cleared · Restore" toast.
    pub fn remember_cleared_selection(&mut self, target: ActionTarget) {
        if !matches!(target, ActionTarget::None) {
            self.cleared_selection = Some(target);
        }
    }

    pub fn has_cleared_selection(&self) -> bool {
        self.cleared_selection.is_some()
    }

    pub fn restore_selection(&mut self) -> Option<ActionTarget> {
        self.cleared_selection.take()
    }

    /// The wire state: labels for the buttons, history newest first.
    pub fn state(&self) -> UndoState {
        UndoState {
            can_undo: self.can_undo(),
            undo_label: self.undo.last().filter(|e| e.device_id == self.device_id).map(|e| e.command.label()),
            can_redo: self.can_redo(),
            redo_label: self.redo.last().filter(|e| e.device_id == self.device_id).map(|e| e.command.label()),
            history: self.undo.iter().rev().take(HISTORY_SHEET_LIMIT).map(Entry::wire).collect(),
        }
    }

    /// Per-entry view for the history sheet, keyed by id.
    pub fn entries(&self) -> HashMap<String, UndoEntry> {
        self.undo.iter().map(|e| (e.id.clone(), e.wire())).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::document::new_document;

    fn doc(rev: u32) -> SessionDocument {
        let mut d = new_document("s", "id".into(), 0.0);
        d.revision = rev;
        d
    }

    fn session_cmd(kind: &str, before: u32, after: u32) -> Box<dyn UndoableCommand> {
        Box::new(SessionUndo::new(kind, format!("{kind} {before}->{after}"), doc(before), doc(after)))
    }

    fn rating_item(id: &str, prior: u32, set: u32) -> RemoteItem {
        RemoteItem { target: RemoteTarget::Track { id: id.into() }, prior: RemoteValue::Rating(prior), set: RemoteValue::Rating(set) }
    }

    fn tracks(ids: &[&str]) -> ActionTarget {
        ActionTarget::Tracks { ids: ids.iter().map(|s| s.to_string()).collect() }
    }

    #[test]
    fn undo_redo_round_trip_restores_selection() {
        let mut s = UndoStack::new("dev", DEFAULT_MAX_BYTES);
        assert!(!s.can_undo());
        let id = s.push(session_cmd("next", 1, 2), tracks(&["a", "b"]), 1000.0);
        assert_eq!(id, "u1");
        let st = s.state();
        assert!(st.can_undo);
        assert_eq!(st.undo_label.as_deref(), Some("next 1->2"));
        assert!(!st.can_redo);
        assert_eq!(st.history.len(), 1);
        assert_eq!(st.history[0].device_id, "dev");

        let r = s.undo().unwrap();
        assert_eq!(r.action, UndoAction::RestoreDocument(doc(1)));
        assert_eq!(r.target, tracks(&["a", "b"]));
        assert_eq!(r.tier, UndoTier::SessionState);
        assert!(s.state().can_redo);
        assert_eq!(s.state().redo_label.as_deref(), Some("next 1->2"));
        assert!(s.undo().is_none());

        let r = s.redo().unwrap();
        assert_eq!(r.action, UndoAction::RestoreDocument(doc(2)));
        assert!(s.redo().is_none());
        assert_eq!(s.len(), 1);
    }

    #[test]
    fn new_action_clears_redo() {
        let mut s = UndoStack::new("dev", DEFAULT_MAX_BYTES);
        s.push(session_cmd("a", 1, 2), ActionTarget::None, 1000.0);
        s.undo();
        assert_eq!(s.redo_len(), 1);
        s.push(session_cmd("b", 1, 3), ActionTarget::None, 5000.0);
        assert_eq!(s.redo_len(), 0);
        assert!(!s.can_redo());
    }

    #[test]
    fn rapid_repeats_coalesce_within_window_only() {
        let mut s = UndoStack::new("dev", DEFAULT_MAX_BYTES);
        let a = s.push(session_cmd("nudge", 1, 2), tracks(&["a"]), 1000.0);
        let b = s.push(session_cmd("nudge", 2, 3), tracks(&["a"]), 1500.0);
        assert_eq!(a, b, "same kind + target inside the window coalesces");
        assert_eq!(s.len(), 1);
        let r = s.undo().unwrap();
        assert_eq!(r.action, UndoAction::RestoreDocument(doc(1)), "undo goes back to the first before");
        assert_eq!(r.label, "nudge 2->3");
        s.redo();
        // Different target: no coalescing.
        let c = s.push(session_cmd("nudge", 3, 4), tracks(&["b"]), 1600.0);
        assert_ne!(c, b);
        // Different kind: no coalescing.
        let d = s.push(session_cmd("shuffle", 4, 5), tracks(&["b"]), 1700.0);
        assert_ne!(d, c);
        // Outside the window: no coalescing.
        let e = s.push(session_cmd("shuffle", 5, 6), tracks(&["b"]), 1700.0 + DEFAULT_COALESCE_WINDOW_MS + 1.0);
        assert_ne!(e, d);
        assert_eq!(s.len(), 4);
    }

    #[test]
    fn remote_coalescing_keeps_first_prior_and_last_set() {
        let mut s = UndoStack::new("dev", DEFAULT_MAX_BYTES).with_coalesce_window(500.0);
        s.push(Box::new(RemoteUndo::new("rate", "Rate 3", vec![rating_item("t", 0, 3)])), tracks(&["t"]), 100.0);
        s.push(Box::new(RemoteUndo::new("rate", "Rate 4", vec![rating_item("t", 3, 4), rating_item("u", 1, 4)])), tracks(&["t"]), 300.0);
        assert_eq!(s.len(), 1);
        let r = s.undo().unwrap();
        assert_eq!(r.tier, UndoTier::RemoteMutation);
        match r.action {
            UndoAction::Cas(m) => {
                assert_eq!(m.len(), 2);
                assert_eq!(m[0].expect, RemoteValue::Rating(4));
                assert_eq!(m[0].command, Command::SetRating { targets: vec![RatingTarget::Track { id: "t".into() }], rating: 0 });
                assert_eq!(m[1].command, Command::SetRating { targets: vec![RatingTarget::Track { id: "u".into() }], rating: 1 });
            }
            other => panic!("{other:?}"),
        }
        let r = s.redo().unwrap();
        match r.action {
            UndoAction::Cas(m) => {
                assert_eq!(m[0].expect, RemoteValue::Rating(0));
                assert_eq!(m[0].command, Command::SetRating { targets: vec![RatingTarget::Track { id: "t".into() }], rating: 4 });
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn remote_item_commands() {
        let love = RemoteItem { target: RemoteTarget::Album { id: "al".into() }, prior: RemoteValue::Loved(false), set: RemoteValue::Loved(true) };
        assert_eq!(love.inverse().unwrap().command, Command::SetLoved { targets: vec![RatingTarget::Album { id: "al".into() }], loved: false });
        let artist = RemoteItem { target: RemoteTarget::Artist { id: "ar".into() }, prior: RemoteValue::Loved(true), set: RemoteValue::Loved(false) };
        assert_eq!(artist.inverse().unwrap().command, Command::SetArtistLoved { artist_id: "ar".into(), loved: true });
        let added = RemoteItem {
            target: RemoteTarget::PlaylistTrack { playlist_id: "p".into(), track_id: "t".into() },
            prior: RemoteValue::Member { present: false, index: None },
            set: RemoteValue::Member { present: true, index: Some(4) },
        };
        assert_eq!(added.inverse().unwrap().command, Command::PlaylistRemove { playlist_id: "p".into(), indices: vec![4] });
        assert_eq!(added.redo().unwrap().command, Command::PlaylistAdd { playlist_id: "p".into(), track_ids: vec!["t".into()], at_index: Some(4) });
        let removed = RemoteItem {
            target: RemoteTarget::PlaylistTrack { playlist_id: "p".into(), track_id: "t".into() },
            prior: RemoteValue::Member { present: true, index: Some(2) },
            set: RemoteValue::Member { present: false, index: None },
        };
        assert_eq!(removed.inverse().unwrap().command, Command::PlaylistAdd { playlist_id: "p".into(), track_ids: vec!["t".into()], at_index: Some(2) });
        let moved = RemoteItem {
            target: RemoteTarget::PlaylistTrack { playlist_id: "p".into(), track_id: "t".into() },
            prior: RemoteValue::Position(1),
            set: RemoteValue::Position(5),
        };
        assert_eq!(moved.inverse().unwrap().command, Command::PlaylistMove { playlist_id: "p".into(), from_index: 5, to_index: 1 });
        let meta = RemoteItem {
            target: RemoteTarget::Playlist { id: "p".into() },
            prior: RemoteValue::Meta { name: "old".into(), comment: None, public: Some(false) },
            set: RemoteValue::Meta { name: "new".into(), comment: Some("c".into()), public: Some(true) },
        };
        assert_eq!(
            meta.inverse().unwrap().command,
            Command::RenamePlaylist { playlist_id: "p".into(), name: "old".into(), comment: None, public: Some(false) }
        );
        // Nonsense combinations are simply not expressible.
        let bad = RemoteItem { target: RemoteTarget::Artist { id: "x".into() }, prior: RemoteValue::Rating(1), set: RemoteValue::Rating(2) };
        assert!(bad.inverse().is_none());
    }

    #[test]
    fn cas_outcome_note() {
        let mut o = CasOutcome::new(500);
        for _ in 0..487 {
            o.record(CasResult::Applied);
        }
        for _ in 0..13 {
            o.record(CasResult::Skipped);
        }
        assert!(o.is_complete());
        assert_eq!(o.note().as_deref(), Some("undid 487 of 500, 13 changed elsewhere"));
        let mut clean = CasOutcome::new(3);
        for _ in 0..3 {
            clean.record(CasResult::Applied);
        }
        assert_eq!(clean.note(), None);
        let mut failed = CasOutcome::new(2);
        failed.record(CasResult::Applied);
        failed.record(CasResult::Failed);
        assert_eq!(failed.note().as_deref(), Some("undid 1 of 2, 1 failed"));
        // Notes attach to entries on either stack.
        let mut s = UndoStack::new("dev", DEFAULT_MAX_BYTES);
        let id = s.push(Box::new(RemoteUndo::new("rate", "Rate", vec![rating_item("t", 0, 3)])), ActionTarget::None, 1.0);
        s.undo();
        assert!(s.set_note(&id, o.note()));
        assert!(!s.set_note("nope", None));
        s.redo();
        assert_eq!(s.state().history[0].note.as_deref(), Some("undid 487 of 500, 13 changed elsewhere"));
    }

    #[test]
    fn other_devices_entries_are_not_undoable_here() {
        let mut s = UndoStack::new("phone", DEFAULT_MAX_BYTES);
        s.push(session_cmd("a", 1, 2), ActionTarget::None, 1.0);
        s.push_from(session_cmd("b", 2, 3), ActionTarget::None, 2.0, "laptop".into());
        assert!(!s.can_undo(), "top entry belongs to the laptop");
        assert!(s.undo().is_none());
        let st = s.state();
        assert!(!st.can_undo);
        assert_eq!(st.undo_label, None);
        assert_eq!(st.history[0].device_id, "laptop");
        assert!(can_undo_entry(&st.history[1], "phone"));
        assert!(!can_undo_entry(&st.history[0], "phone"));
        // A same-kind action from another device never coalesces with ours.
        s.push_from(session_cmd("b", 3, 4), ActionTarget::None, 2.5, "laptop".into());
        s.push(session_cmd("b", 4, 5), ActionTarget::None, 2.6);
        assert_eq!(s.len(), 3, "laptop's two coalesced, ours did not join them");
        assert!(s.can_undo());
        // undo_to stops at the foreign entry.
        let results = s.undo_to("u1");
        assert_eq!(results.len(), 1);
        assert_eq!(s.len(), 2);
    }

    #[test]
    fn undo_to_walks_down_the_stack() {
        let mut s = UndoStack::new("dev", DEFAULT_MAX_BYTES);
        let first = s.push(session_cmd("a", 1, 2), ActionTarget::None, 1.0);
        s.push(session_cmd("b", 2, 3), ActionTarget::None, 5000.0);
        s.push(session_cmd("c", 3, 4), ActionTarget::None, 9000.0);
        let results = s.undo_to(&first);
        assert_eq!(results.len(), 3);
        assert_eq!(results.last().unwrap().action, UndoAction::RestoreDocument(doc(1)));
        assert_eq!(s.len(), 0);
        assert_eq!(s.redo_len(), 3);
        assert!(s.undo_to("missing").is_empty());
    }

    #[test]
    fn bounded_by_bytes_not_count() {
        let one = session_cmd("a", 1, 2).size_bytes();
        let mut s = UndoStack::new("dev", one * 3 + one / 2);
        for i in 0..10u32 {
            s.push(session_cmd("a", i, i + 1), ActionTarget::None, (i as f64) * 10_000.0);
        }
        assert_eq!(s.len(), 3);
        assert!(s.bytes() <= one * 3 + one / 2);
        assert_eq!(s.state().history[0].label, "a 9->10");
        // The newest entry always survives, even alone over budget.
        let mut tiny = UndoStack::new("dev", 1);
        tiny.push(session_cmd("a", 1, 2), ActionTarget::None, 1.0);
        assert_eq!(tiny.len(), 1);
        // Redo entries count too and are evicted after older undo entries.
        let mut s = UndoStack::new("dev", one * 2 + one / 2);
        s.push(session_cmd("a", 1, 2), ActionTarget::None, 1.0);
        s.push(session_cmd("b", 2, 3), ActionTarget::None, 10_000.0);
        s.undo();
        assert_eq!(s.redo_len(), 1);
        s.push(session_cmd("c", 3, 4), ActionTarget::None, 20_000.0);
        assert_eq!(s.redo_len(), 0);
        assert_eq!(s.len(), 2);
    }

    #[test]
    fn cleared_selection_restore() {
        let mut s = UndoStack::new("dev", DEFAULT_MAX_BYTES);
        s.remember_cleared_selection(ActionTarget::None);
        assert!(!s.has_cleared_selection());
        s.remember_cleared_selection(tracks(&["a", "b", "c"]));
        assert!(s.has_cleared_selection());
        assert_eq!(s.restore_selection(), Some(tracks(&["a", "b", "c"])));
        assert_eq!(s.restore_selection(), None);
    }

    #[test]
    fn history_sheet_is_newest_first_and_capped() {
        let mut s = UndoStack::new("dev", DEFAULT_MAX_BYTES);
        for i in 0..(HISTORY_SHEET_LIMIT as u32 + 5) {
            s.push(session_cmd("a", i, i + 1), ActionTarget::None, (i as f64) * 10_000.0);
        }
        let st = s.state();
        assert_eq!(st.history.len(), HISTORY_SHEET_LIMIT);
        assert_eq!(st.history[0].label, format!("a {}->{}", HISTORY_SHEET_LIMIT + 4, HISTORY_SHEET_LIMIT + 5));
        assert!(st.history[0].at > st.history[1].at);
        assert_eq!(s.entries().len(), HISTORY_SHEET_LIMIT + 5);
        s.clear();
        assert!(s.is_empty());
        assert_eq!(s.bytes(), 0);
    }
}
