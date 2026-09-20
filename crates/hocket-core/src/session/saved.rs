//! Saved queues: snapshots of the outgoing queue on context replacement.
//!
//! Rules (design.md "Saved queues"): auto-save on replacement, skip trivial
//! queues, cap unpinned entries (default 10, configurable 0–50, 0 disables
//! auto-save), LRU eviction on `last_interacted_at`, 30-day expiry for
//! unpinned, dedupe on context identity, pinned entries are exempt from
//! everything, snapshots of ID-referenced contexts omit their track list, and
//! the whole set is bounded in bytes. Sync is a last-write-wins set keyed on
//! context identity with `updated_at` as the clock.

use std::collections::HashMap;

use md5::{Digest, Md5};

use crate::api::{ContextKind, EpochMs, Ms, QueueContext, SavedQueue, SessionDocument};

/// Maximum configurable unpinned cap.
pub const MAX_CAP: u32 = 50;
/// Default unpinned cap.
pub const DEFAULT_CAP: u32 = 10;
/// Default total byte budget for all snapshots (pinned included in the sum,
/// only unpinned are evicted to meet it).
pub const DEFAULT_BYTE_CAP: usize = 512 * 1024;
/// Unpinned entries expire after this long without interaction.
pub const DEFAULT_EXPIRY_MS: f64 = 30.0 * 24.0 * 60.0 * 60.0 * 1000.0;

#[derive(Debug, Clone, PartialEq)]
pub struct SavedQueuePolicy {
    /// Unpinned entries kept. Zero disables auto-save.
    pub cap: u32,
    pub byte_cap: usize,
    pub expiry_ms: f64,
}

impl Default for SavedQueuePolicy {
    fn default() -> Self {
        SavedQueuePolicy {
            cap: DEFAULT_CAP,
            byte_cap: DEFAULT_BYTE_CAP,
            expiry_ms: DEFAULT_EXPIRY_MS,
        }
    }
}

impl SavedQueuePolicy {
    /// Clamp a user-supplied cap into `0..=MAX_CAP`.
    pub fn with_cap(mut self, cap: u32) -> Self {
        self.cap = cap.min(MAX_CAP);
        self
    }
}

/// Whether a context is referenced by ID (its tracks can be re-resolved) or
/// carries its own track list.
pub fn is_id_referenced(kind: &ContextKind) -> bool {
    !matches!(kind, ContextKind::AdHoc { .. } | ContextKind::Autoplay)
}

/// Stable identity for dedupe and sync. `server_id` + kind + sort; ad-hoc and
/// autoplay contexts hash their track list instead of an ID.
pub fn context_identity(ctx: &QueueContext) -> String {
    let kind = match &ctx.kind {
        ContextKind::Album { id } => format!("album:{id}"),
        ContextKind::Artist { id } => format!("artist:{id}"),
        ContextKind::Playlist { id } => format!("playlist:{id}"),
        ContextKind::Genre { name } => format!("genre:{name}"),
        ContextKind::Filter { filter } => format!("filter:{}", filter.id),
        ContextKind::AdHoc { .. } => format!("adhoc:{}", track_hash(&ctx.tracks)),
        ContextKind::Autoplay => format!("autoplay:{}", track_hash(&ctx.tracks)),
    };
    format!("{}|{}|{:?}", ctx.server_id, kind, ctx.sort)
}

fn track_hash(tracks: &[String]) -> String {
    let mut h = Md5::new();
    for t in tracks {
        h.update(t.as_bytes());
        h.update([0u8]);
    }
    hex::encode(h.finalize())
}

/// Identity of a saved entry.
pub fn identity_of(saved: &SavedQueue) -> String {
    context_identity(&saved.context)
}

/// A queue not worth keeping: nothing was ever played from it, or it holds at
/// most one item in total.
pub fn is_trivial(doc: &SessionDocument) -> bool {
    let Some(ctx) = &doc.context else { return true };
    if doc.current.is_none() {
        return true;
    }
    ctx.tracks.len() + doc.insertions.len() + doc.history.len() <= 1
}

/// A snapshot needs the actor to re-resolve its track list before it can be
/// fully restored.
pub fn needs_resolution(saved: &SavedQueue) -> bool {
    is_id_referenced(&saved.context.kind) && saved.context.tracks.is_empty()
}

/// Snapshot the live queue. `None` when trivial. `id` is the new entry's id
/// (a dedupe against an existing entry keeps the old id instead).
pub fn snapshot(
    doc: &SessionDocument,
    position_ms: Ms,
    now: EpochMs,
    id: String,
) -> Option<SavedQueue> {
    if is_trivial(doc) {
        return None;
    }
    let ctx = doc.context.as_ref()?;
    let track_count = ctx.tracks.len() as u32;
    let mut context = ctx.clone();
    if is_id_referenced(&context.kind) {
        context.tracks.clear();
    }
    Some(SavedQueue {
        id,
        label: ctx.label.clone(),
        context,
        cursor: doc.cursor,
        current: doc.current.clone(),
        history: doc.history.clone(),
        insertions: doc.insertions.clone(),
        shuffle: doc.shuffle.clone(),
        repeat: doc.repeat,
        position_ms,
        pinned: false,
        created_at: now,
        last_interacted_at: now,
        updated_at: now,
        track_count,
        cover_art: None,
    })
}

/// Serialised size estimate used for the byte cap.
pub fn estimate_bytes(saved: &SavedQueue) -> usize {
    serde_json::to_vec(saved).map(|v| v.len()).unwrap_or(0)
}

/// Total estimated size of a list.
pub fn total_bytes(list: &[SavedQueue]) -> usize {
    list.iter().map(estimate_bytes).sum()
}

/// Insert or refresh (dedupe on identity) and enforce the policy. Returns
/// whether the list changed. With `cap == 0` only an existing pinned entry
/// of the same identity is refreshed.
pub fn upsert(
    list: &mut Vec<SavedQueue>,
    mut entry: SavedQueue,
    policy: &SavedQueuePolicy,
    now: EpochMs,
) -> bool {
    let identity = identity_of(&entry);
    if let Some(existing) = list.iter_mut().find(|q| identity_of(q) == identity) {
        entry.id = existing.id.clone();
        entry.pinned = existing.pinned;
        entry.created_at = existing.created_at;
        entry.cover_art = entry.cover_art.or_else(|| existing.cover_art.clone());
        entry.last_interacted_at = now;
        entry.updated_at = now;
        *existing = entry;
        enforce(list, policy, now);
        return true;
    }
    if policy.cap == 0 && !entry.pinned {
        return false;
    }
    list.push(entry);
    enforce(list, policy, now);
    true
}

/// Apply expiry, the count cap and the byte cap; sort newest-interacted first.
/// Returns whether anything changed.
pub fn enforce(list: &mut Vec<SavedQueue>, policy: &SavedQueuePolicy, now: EpochMs) -> bool {
    let before = list.clone();
    list.retain(|q| q.pinned || now - q.last_interacted_at <= policy.expiry_ms);
    sort_recent_first(list);
    // Count cap: the unpinned entries beyond `cap`, walking from the least recent.
    let mut unpinned_seen = 0u32;
    let mut keep: Vec<bool> = Vec::with_capacity(list.len());
    for q in list.iter() {
        if q.pinned {
            keep.push(true);
        } else {
            unpinned_seen += 1;
            keep.push(unpinned_seen <= policy.cap);
        }
    }
    let mut i = 0;
    list.retain(|_| {
        let k = keep[i];
        i += 1;
        k
    });
    // Byte cap: evict LRU unpinned until under budget (or none left to evict).
    while total_bytes(list) > policy.byte_cap {
        let Some(pos) = list.iter().rposition(|q| !q.pinned) else {
            break;
        };
        list.remove(pos);
    }
    *list != before
}

fn sort_recent_first(list: &mut [SavedQueue]) {
    list.sort_by(|a, b| {
        b.last_interacted_at
            .partial_cmp(&a.last_interacted_at)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
}

/// Last-write-wins merge keyed on context identity. `updated_at` decides;
/// ties keep the local entry. Pin state rides along with the winning record.
pub fn merge_saved_queues(local: &[SavedQueue], remote: &[SavedQueue]) -> Vec<SavedQueue> {
    let mut by_identity: HashMap<String, SavedQueue> = HashMap::new();
    let mut order: Vec<String> = vec![];
    for q in local {
        let id = identity_of(q);
        if !by_identity.contains_key(&id) {
            order.push(id.clone());
        }
        by_identity.entry(id).or_insert_with(|| q.clone());
    }
    for q in remote {
        let id = identity_of(q);
        match by_identity.get(&id) {
            Some(existing) if existing.updated_at >= q.updated_at => {}
            Some(_) => {
                by_identity.insert(id, q.clone());
            }
            None => {
                order.push(id.clone());
                by_identity.insert(id, q.clone());
            }
        }
    }
    let mut out: Vec<SavedQueue> = order
        .into_iter()
        .filter_map(|id| by_identity.remove(&id))
        .collect();
    sort_recent_first(&mut out);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::{QueueItem, QueueSource, SortOrder};

    fn ctx(kind: ContextKind, tracks: &[&str]) -> QueueContext {
        QueueContext {
            server_id: "srv".into(),
            kind,
            label: "L".into(),
            sort: SortOrder::Default,
            tracks: tracks.iter().map(|s| s.to_string()).collect(),
        }
    }

    fn entry(id: &str, kind: ContextKind, tracks: &[&str], at: f64) -> SavedQueue {
        SavedQueue {
            id: id.into(),
            context: ctx(kind, tracks),
            label: "L".into(),
            cursor: 0,
            current: None,
            history: vec![],
            insertions: vec![],
            shuffle: None,
            repeat: Default::default(),
            position_ms: 0,
            pinned: false,
            created_at: at,
            last_interacted_at: at,
            updated_at: at,
            track_count: tracks.len() as u32,
            cover_art: None,
        }
    }

    fn album(id: &str) -> ContextKind {
        ContextKind::Album { id: id.into() }
    }

    #[test]
    fn identity_rules() {
        let a = ctx(album("a1"), &["t1", "t2"]);
        let mut a_other_tracks = a.clone();
        a_other_tracks.tracks = vec!["x".into()];
        assert_eq!(context_identity(&a), context_identity(&a_other_tracks));
        let mut sorted = a.clone();
        sorted.sort = SortOrder::Title;
        assert_ne!(context_identity(&a), context_identity(&sorted));

        let adhoc = ctx(ContextKind::AdHoc { label: "s".into() }, &["t1", "t2"]);
        let adhoc2 = ctx(
            ContextKind::AdHoc {
                label: "other label".into(),
            },
            &["t1", "t2"],
        );
        let adhoc3 = ctx(ContextKind::AdHoc { label: "s".into() }, &["t2", "t1"]);
        assert_eq!(context_identity(&adhoc), context_identity(&adhoc2));
        assert_ne!(context_identity(&adhoc), context_identity(&adhoc3));
    }

    #[test]
    fn snapshot_omits_tracks_for_id_referenced_only() {
        let mut doc = crate::session::document::new_document("s", "id".into(), 0.0);
        doc.context = Some(ctx(album("a"), &["t1", "t2", "t3"]));
        doc.current = Some(QueueItem {
            key: "k".into(),
            track_id: "t1".into(),
            source: QueueSource::Context { index: 0 },
            unavailable: false,
        });
        let s = snapshot(&doc, 1234, 10.0, "sq1".into()).unwrap();
        assert!(s.context.tracks.is_empty());
        assert!(needs_resolution(&s));
        assert_eq!(s.track_count, 3);
        assert_eq!(s.position_ms, 1234);

        doc.context = Some(ctx(
            ContextKind::AdHoc {
                label: "sel".into(),
            },
            &["t1", "t2", "t3"],
        ));
        let s = snapshot(&doc, 0, 10.0, "sq2".into()).unwrap();
        assert_eq!(s.context.tracks.len(), 3);
        assert!(!needs_resolution(&s));
    }

    #[test]
    fn trivial_queues_are_skipped() {
        let mut doc = crate::session::document::new_document("s", "id".into(), 0.0);
        assert!(snapshot(&doc, 0, 0.0, "x".into()).is_none());
        doc.context = Some(ctx(album("a"), &["t1", "t2"]));
        // Never played from.
        assert!(snapshot(&doc, 0, 0.0, "x".into()).is_none());
        doc.current = Some(QueueItem {
            key: "k".into(),
            track_id: "t1".into(),
            source: QueueSource::Context { index: 0 },
            unavailable: false,
        });
        assert!(snapshot(&doc, 0, 0.0, "x".into()).is_some());
        // Single track.
        doc.context.as_mut().unwrap().tracks.truncate(1);
        assert!(snapshot(&doc, 0, 0.0, "x".into()).is_none());
    }

    #[test]
    fn dedupe_keeps_id_and_pin() {
        let policy = SavedQueuePolicy::default();
        let mut list = vec![];
        let mut first = entry("id1", album("a"), &[], 100.0);
        first.pinned = true;
        assert!(upsert(&mut list, first, &policy, 100.0));
        let second = entry("id2", album("a"), &[], 200.0);
        assert!(upsert(&mut list, second, &policy, 200.0));
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].id, "id1");
        assert!(list[0].pinned);
        assert_eq!(list[0].last_interacted_at, 200.0);
        assert_eq!(list[0].created_at, 100.0);
    }

    #[test]
    fn cap_evicts_lru_unpinned_and_spares_pinned() {
        let policy = SavedQueuePolicy::default().with_cap(2);
        let mut list = vec![];
        let mut pinned = entry("p", album("p"), &[], 1.0);
        pinned.pinned = true;
        upsert(&mut list, pinned, &policy, 1.0);
        for (i, at) in [(1, 10.0), (2, 20.0), (3, 30.0)] {
            let e = entry(&format!("u{i}"), album(&format!("a{i}")), &[], at);
            upsert(&mut list, e, &policy, at);
        }
        let ids: Vec<&str> = list.iter().map(|q| q.id.as_str()).collect();
        assert_eq!(ids, vec!["u3", "u2", "p"]);
        // Touching u2 makes u3 the LRU when a new one arrives.
        list.iter_mut()
            .find(|q| q.id == "u2")
            .unwrap()
            .last_interacted_at = 40.0;
        upsert(
            &mut list,
            entry("u4", album("a4"), &[], 50.0),
            &policy,
            50.0,
        );
        let ids: Vec<&str> = list.iter().map(|q| q.id.as_str()).collect();
        assert_eq!(ids, vec!["u4", "u2", "p"]);
    }

    #[test]
    fn cap_zero_disables_autosave_but_refreshes_pinned() {
        let policy = SavedQueuePolicy::default().with_cap(0);
        let mut list = vec![];
        assert!(!upsert(
            &mut list,
            entry("u1", album("a"), &[], 1.0),
            &policy,
            1.0
        ));
        assert!(list.is_empty());
        let mut p = entry("p", album("b"), &[], 1.0);
        p.pinned = true;
        list.push(p);
        assert!(upsert(
            &mut list,
            entry("x", album("b"), &[], 2.0),
            &policy,
            2.0
        ));
        assert_eq!(list.len(), 1);
        assert!(list[0].pinned);
        assert_eq!(list[0].last_interacted_at, 2.0);
        assert!(SavedQueuePolicy::default().with_cap(999).cap == MAX_CAP);
    }

    #[test]
    fn expiry_drops_unpinned_only() {
        let policy = SavedQueuePolicy::default();
        let old = entry("old", album("a"), &[], 0.0);
        let mut old_pinned = entry("op", album("b"), &[], 0.0);
        old_pinned.pinned = true;
        let mut list = vec![old, old_pinned];
        let now = DEFAULT_EXPIRY_MS + 1.0;
        assert!(enforce(&mut list, &policy, now));
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].id, "op");
        assert!(!enforce(&mut list, &policy, now));
    }

    #[test]
    fn byte_cap_evicts_lru_unpinned() {
        let big_tracks: Vec<String> = (0..500).map(|i| format!("track-{i:06}")).collect();
        let big: Vec<&str> = big_tracks.iter().map(|s| s.as_str()).collect();
        let one = estimate_bytes(&entry(
            "x",
            ContextKind::AdHoc { label: "s".into() },
            &big,
            0.0,
        ));
        let policy = SavedQueuePolicy {
            cap: 10,
            byte_cap: one * 2 + one / 2,
            expiry_ms: DEFAULT_EXPIRY_MS,
        };
        let mut list = vec![];
        for i in 0..4 {
            let mut tracks = big.clone();
            tracks[0] = if i == 0 {
                "a"
            } else if i == 1 {
                "b"
            } else if i == 2 {
                "c"
            } else {
                "d"
            };
            let mut e = entry(
                &format!("e{i}"),
                ContextKind::AdHoc { label: "s".into() },
                &tracks,
                i as f64,
            );
            e.pinned = i == 0;
            upsert(&mut list, e, &policy, i as f64);
        }
        let ids: Vec<&str> = list.iter().map(|q| q.id.as_str()).collect();
        assert_eq!(ids, vec!["e3", "e0"]);
        assert!(total_bytes(&list) <= policy.byte_cap);
    }

    #[test]
    fn lww_merge() {
        let l1 = entry("l1", album("a"), &[], 100.0);
        let mut l2 = entry("l2", album("b"), &[], 50.0);
        l2.pinned = true;
        let mut r1 = entry("r1", album("a"), &[], 200.0);
        r1.pinned = true;
        let r2 = entry("r2", album("b"), &[], 40.0);
        let r3 = entry("r3", album("c"), &[], 10.0);
        let merged = merge_saved_queues(&[l1, l2], &[r1, r2, r3]);
        assert_eq!(merged.len(), 3);
        let a = merged
            .iter()
            .find(|q| matches!(q.context.kind, ContextKind::Album { ref id } if id == "a"))
            .unwrap();
        assert_eq!(a.id, "r1");
        assert!(a.pinned, "pin state rides with the winner");
        let b = merged
            .iter()
            .find(|q| matches!(q.context.kind, ContextKind::Album { ref id } if id == "b"))
            .unwrap();
        assert_eq!(b.id, "l2");
        assert!(b.pinned);
        assert!(merged.iter().any(|q| q.id == "r3"));
        // Ties keep local.
        let tie = merge_saved_queues(
            &[entry("l", album("z"), &[], 5.0)],
            &[entry("r", album("z"), &[], 5.0)],
        );
        assert_eq!(tie[0].id, "l");
    }
}
