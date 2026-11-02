//! Where an album started from its own page left off.
//!
//! One record per album, last-write-wins, with a separate removal clock so "start over" or
//! finishing the album reaches every device. A record is offered only while it is under
//! [`TTL_MS`] old. The actor decides when to write one ([`decide`]); this module is pure.

use std::cmp::Ordering;

use crate::api::{AlbumId, AlbumResumeEntry, AlbumResumePoint, EpochMs, Ms};

/// Forgotten after this long, on every device.
pub const TTL_MS: EpochMs = 30.0 * 24.0 * 3600.0 * 1000.0;

/// Position changes smaller than this are not written again while playback runs.
pub const POSITION_QUANTUM_MS: Ms = 5_000;

/// Bound on the synced set (live records and tombstones together).
const MAX_ENTRIES: usize = 400;

/// The saved-state key for one account's album resume map.
pub fn state_key(scope: &str) -> String {
    format!("album-resume:{scope}")
}

/// What the queue is doing, for [`decide`].
#[derive(Debug, Clone, PartialEq)]
pub struct Snapshot {
    /// Set when the queue is an album. Shuffle still names the album; [`Snapshot::shuffled`]
    /// says it was not started in album order.
    pub album_id: Option<AlbumId>,
    pub shuffled: bool,
    pub track_count: u32,
    /// Current item is one of the album's own tracks (not an insertion or autoplay).
    pub from_context: bool,
    /// Index into the album's unshuffled track list.
    pub index: Option<u32>,
    /// Current item was added by autoplay: the album itself has run out.
    pub autoplay: bool,
    pub track_id: String,
    pub position_ms: Ms,
    pub duration_ms: Ms,
    /// The album's last track just ended (the queue stopped, or autoplay was asked for).
    pub finished: bool,
}

/// What to do with the album's record.
#[derive(Debug, Clone, PartialEq)]
pub enum Decision {
    /// Not an album play we record (a playlist, the stash, shuffle, an insertion).
    Ignore,
    Clear {
        album_id: AlbumId,
    },
    Save(AlbumResumeEntry),
}

/// Resume is recorded only for an album started in its own order, once playback has moved
/// past the first track and until the album finishes. The first track clears any record (Play
/// and Start over both land there). Shuffle, playlists and anything that is not this album
/// leave the map alone. With the feature off, clears still apply and new positions are not
/// stored.
pub fn decide(snap: &Snapshot, now: EpochMs, enabled: bool) -> Decision {
    let Some(album_id) = snap.album_id.clone() else {
        return Decision::Ignore;
    };
    if snap.shuffled {
        return Decision::Ignore;
    }
    if snap.finished || snap.autoplay {
        return Decision::Clear { album_id };
    }
    if !snap.from_context {
        return Decision::Ignore;
    }
    let Some(index) = snap.index else {
        return Decision::Ignore;
    };
    // Still on the first track: nothing to resume, and a restart drops the old point.
    if index == 0 {
        return Decision::Clear { album_id };
    }
    if !enabled {
        return Decision::Ignore;
    }
    Decision::Save(AlbumResumeEntry {
        album_id,
        track_id: snap.track_id.clone(),
        track_index: index,
        position_ms: snap.position_ms,
        duration_ms: snap.duration_ms,
        updated_at: now,
        removed: false,
        removed_at: 0.0,
    })
}

fn greater<T: PartialOrd>(a: T, b: T) -> bool {
    matches!(a.partial_cmp(&b), Some(Ordering::Greater))
}

fn merge_entry(m: &mut AlbumResumeEntry, t: &AlbumResumeEntry) -> bool {
    let mut changed = false;
    if greater(t.updated_at, m.updated_at) {
        let (removed, removed_at) = (m.removed, m.removed_at);
        *m = t.clone();
        m.removed = removed;
        m.removed_at = removed_at;
        changed = true;
    }
    // Ties: removed wins, so a clear and a save at the same instant stays cleared.
    if greater((t.removed_at, t.removed), (m.removed_at, m.removed)) {
        m.removed = t.removed;
        m.removed_at = t.removed_at;
        changed = true;
    }
    changed
}

/// Merge `theirs` into `ours`. Returns the merged set and whether it differs from `ours`.
pub fn merge(
    ours: &[AlbumResumeEntry],
    theirs: &[AlbumResumeEntry],
) -> (Vec<AlbumResumeEntry>, bool) {
    let mut merged = ours.to_vec();
    let mut changed = false;
    for t in theirs {
        match merged.iter_mut().find(|m| m.album_id == t.album_id) {
            Some(m) => changed |= merge_entry(m, t),
            None => {
                merged.push(t.clone());
                changed = true;
            }
        }
    }
    sort(&mut merged);
    (merged, changed)
}

fn sort(entries: &mut [AlbumResumeEntry]) {
    entries.sort_by(|a, b| a.album_id.cmp(&b.album_id));
}

/// Drop records and tombstones older than [`TTL_MS`], then the oldest past [`MAX_ENTRIES`].
pub fn prune(entries: &mut Vec<AlbumResumeEntry>, now: EpochMs) {
    entries.retain(|e| {
        let stamp = if e.removed {
            e.removed_at.max(e.updated_at)
        } else {
            e.updated_at
        };
        now - stamp < TTL_MS
    });
    if entries.len() > MAX_ENTRIES {
        entries.sort_by(|a, b| {
            b.updated_at
                .max(b.removed_at)
                .partial_cmp(&a.updated_at.max(a.removed_at))
                .unwrap_or(Ordering::Equal)
        });
        entries.truncate(MAX_ENTRIES);
    }
    sort(entries);
}

/// Write `entry` (a live position). A resurrection stamps `removed_at` so it beats an older clear.
/// Returns whether the stored record changed.
pub fn upsert(entries: &mut Vec<AlbumResumeEntry>, mut entry: AlbumResumeEntry) -> bool {
    entry.removed = false;
    if let Some(prev) = entries.iter().find(|e| e.album_id == entry.album_id) {
        if entry.duration_ms == 0 && prev.track_id == entry.track_id && prev.duration_ms > 0 {
            entry.duration_ms = prev.duration_ms;
        }
        if prev.removed {
            entry.removed_at = entry.updated_at;
        } else {
            entry.removed_at = prev.removed_at;
        }
    }
    let changed = match entries.iter_mut().find(|e| e.album_id == entry.album_id) {
        Some(m) => {
            let same = !m.removed
                && m.track_id == entry.track_id
                && m.track_index == entry.track_index
                && m.position_ms == entry.position_ms
                && m.duration_ms == entry.duration_ms;
            if same {
                false
            } else {
                *m = entry;
                true
            }
        }
        None => {
            entries.push(entry);
            true
        }
    };
    if changed {
        sort(entries);
    }
    changed
}

/// Mark the album cleared. Returns whether a live record was removed.
pub fn clear(entries: &mut [AlbumResumeEntry], album_id: &str, now: EpochMs) -> bool {
    let Some(e) = entries
        .iter_mut()
        .find(|e| e.album_id == album_id && !e.removed)
    else {
        return false;
    };
    e.removed = true;
    e.removed_at = now;
    e.updated_at = now;
    true
}

fn live(e: &AlbumResumeEntry, now: EpochMs) -> bool {
    !e.removed && now - e.updated_at < TTL_MS
}

fn to_point(e: &AlbumResumeEntry) -> AlbumResumePoint {
    AlbumResumePoint {
        album_id: e.album_id.clone(),
        track_id: e.track_id.clone(),
        track_index: e.track_index,
        position_ms: e.position_ms,
        duration_ms: e.duration_ms,
        updated_at: e.updated_at,
    }
}

/// The album's resume point, if one is still offered.
pub fn point(
    entries: &[AlbumResumeEntry],
    album_id: &str,
    now: EpochMs,
) -> Option<AlbumResumePoint> {
    entries
        .iter()
        .find(|e| e.album_id == album_id && live(e, now))
        .map(to_point)
}

/// Every album that still offers resume, in album-id order.
pub fn points(entries: &[AlbumResumeEntry], now: EpochMs) -> Vec<AlbumResumePoint> {
    let mut out: Vec<_> = entries
        .iter()
        .filter(|e| live(e, now))
        .map(to_point)
        .collect();
    out.sort_by(|a, b| a.album_id.cmp(&b.album_id));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snap(album: Option<&str>, index: Option<u32>, count: u32) -> Snapshot {
        Snapshot {
            album_id: album.map(str::to_string),
            shuffled: false,
            track_count: count,
            from_context: index.is_some(),
            index,
            autoplay: false,
            track_id: index.map(|i| format!("t{i}")).unwrap_or_default(),
            position_ms: 12_000,
            duration_ms: 180_000,
            finished: false,
        }
    }

    #[test]
    fn only_an_album_started_in_order_and_past_its_first_track() {
        let now = 1_000.0;
        assert!(matches!(
            decide(&snap(None, Some(2), 8), now, true),
            Decision::Ignore
        ));
        let mut playlist = snap(None, Some(2), 8);
        playlist.album_id = None;
        assert!(matches!(decide(&playlist, now, true), Decision::Ignore));

        let mut shuffled = snap(Some("al"), Some(3), 8);
        shuffled.shuffled = true;
        assert!(
            matches!(decide(&shuffled, now, true), Decision::Ignore),
            "shuffle is not an album play"
        );

        let mut inserted = snap(Some("al"), None, 8);
        inserted.from_context = false;
        assert!(matches!(decide(&inserted, now, true), Decision::Ignore));

        assert!(matches!(
            decide(&snap(Some("al"), Some(0), 8), now, true),
            Decision::Clear { .. }
        ));
        match decide(&snap(Some("al"), Some(2), 8), now, true) {
            Decision::Save(e) => {
                assert_eq!(e.album_id, "al");
                assert_eq!(e.track_id, "t2");
                assert_eq!(e.track_index, 2);
                assert_eq!(e.position_ms, 12_000);
            }
            other => panic!("{other:?}"),
        }
        // The last track, before it finishes, is still a place to resume.
        match decide(&snap(Some("al"), Some(7), 8), now, true) {
            Decision::Save(e) => assert_eq!(e.track_index, 7),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn finishing_the_album_clears_it_and_the_switch_blocks_saves() {
        let mut done = snap(Some("al"), Some(7), 8);
        done.finished = true;
        assert!(matches!(decide(&done, 1.0, true), Decision::Clear { .. }));
        let mut auto = snap(Some("al"), None, 8);
        auto.from_context = false;
        auto.autoplay = true;
        assert!(matches!(decide(&auto, 1.0, true), Decision::Clear { .. }));
        // Off: a restart still clears, a middle track is not stored.
        assert!(matches!(
            decide(&snap(Some("al"), Some(0), 8), 1.0, false),
            Decision::Clear { .. }
        ));
        assert!(matches!(
            decide(&snap(Some("al"), Some(3), 8), 1.0, false),
            Decision::Ignore
        ));
    }

    #[test]
    fn records_expire_after_thirty_days_and_merge_per_album() {
        let now = 10_000_000.0;
        let mut fresh = entry("a", 2, now - 1_000.0);
        let stale = entry("b", 3, now - TTL_MS - 1.0);
        let mut entries = vec![fresh.clone(), stale.clone()];
        prune(&mut entries, now);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].album_id, "a");
        assert!(point(&entries, "b", now).is_none());

        fresh.position_ms = 40_000;
        fresh.updated_at = now;
        let (merged, changed) = merge(std::slice::from_ref(&stale), std::slice::from_ref(&fresh));
        assert!(changed);
        assert_eq!(point(&merged, "a", now).unwrap().position_ms, 40_000);
        // An older copy of the same album does not walk the position back.
        let older = entry("a", 1, now - 50_000.0);
        let (again, changed) = merge(&merged, &[older]);
        assert!(!changed);
        assert_eq!(point(&again, "a", now).unwrap().track_index, 2);

        // A clear beats an older position, and a later position beats the clear.
        let mut cleared = fresh.clone();
        assert!(clear(&mut vec_of(&mut cleared), "a", now + 10.0));
        let tomb = vec_of_owned(cleared.clone(), now + 10.0);
        let (merged, _) = merge(&[fresh.clone()], &tomb);
        assert!(point(&merged, "a", now + 10.0).is_none());
        let mut later = entry("a", 4, now + 20.0);
        later.removed_at = now + 20.0;
        assert!(upsert(&mut merged.clone(), later.clone()));
        let (revived, changed) = merge(&merged, &[later]);
        assert!(changed);
        assert_eq!(point(&revived, "a", now + 20.0).unwrap().track_index, 4);
    }

    fn entry(album: &str, index: u32, at: f64) -> AlbumResumeEntry {
        AlbumResumeEntry {
            album_id: album.into(),
            track_id: format!("t{index}"),
            track_index: index,
            position_ms: 1_000,
            duration_ms: 180_000,
            updated_at: at,
            removed: false,
            removed_at: 0.0,
        }
    }

    fn vec_of(e: &mut AlbumResumeEntry) -> Vec<AlbumResumeEntry> {
        vec![e.clone()]
    }

    fn vec_of_owned(mut e: AlbumResumeEntry, at: f64) -> Vec<AlbumResumeEntry> {
        e.removed = true;
        e.removed_at = at;
        e.updated_at = at;
        vec![e]
    }
}
