//! The song stash: a clipboard of tracks collected while browsing, then played, queued, saved to a
//! playlist or downloaded as a batch. It belongs to the account (server + user), not the device.
//!
//! The stash is a set of [`StashEntry`] records merged last-write-wins per `entryId`, the same way
//! saved queues and synced settings are, so it rides Connect (`Msg::StashSync`) and the
//! coordinator's replica. Order comes from each entry's `sortKey` (ties by `entryId`), so a move
//! touches one record and concurrent adds on two devices both survive. Position (`updatedAt`) and
//! removal (`removedAt`) have clocks of their own and merge independently, so a move and a
//! concurrent remove both stand. A removal is a tombstone (`removed`, its snapshot dropped), kept
//! for [`TOMBSTONE_TTL_MS`] (at most [`MAX_TOMBSTONES`] of them) so a device that missed it can't
//! bring the song back; one away for longer than that can.
//!
//! Everything here is pure: the actor owns the list, persists it and feeds it to the engine.

use std::cmp::Ordering;
use std::collections::HashSet;

use crate::api::{DeviceId, EpochMs, StashEntry, StashItem, Track, TrackRef};

/// How many songs the stash holds. Adds past it are dropped (and toasted).
pub const MAX_ENTRIES: usize = 500;

/// How long a removed entry is remembered: the coordinator's replica lives as long.
pub const TOMBSTONE_TTL_MS: EpochMs = 30.0 * 24.0 * 3600.0 * 1000.0;

/// How many removed entries are remembered at most (newest first), so clearing a full stash again
/// and again can't grow the synced set (and the Connect frames carrying it) without bound.
pub const MAX_TOMBSTONES: usize = 500;

/// The saved-state key for one account's stash.
pub fn state_key(scope: &str) -> String {
    format!("stash:{scope}")
}

fn greater<T: PartialOrd>(a: T, b: T) -> bool {
    matches!(a.partial_cmp(&b), Some(Ordering::Greater))
}

/// Merge one copy of an entry into ours, field group by field group: the position from the newer
/// `updatedAt` (ties by key), the removal from the newer `removedAt` (ties: removed wins). Both are
/// total orders, so every device ends up with the same entry whatever order copies arrive in.
fn merge_entry(m: &mut StashEntry, t: &StashEntry) -> bool {
    let mut changed = false;
    if greater((t.updated_at, t.sort_key), (m.updated_at, m.sort_key)) {
        let (removed, removed_at) = (m.removed, m.removed_at);
        *m = t.clone();
        m.removed = removed;
        m.removed_at = removed_at;
        changed = true;
    }
    if greater((t.removed_at, t.removed), (m.removed_at, m.removed)) {
        m.removed = t.removed;
        m.removed_at = t.removed_at;
        changed = true;
    }
    changed
}

/// Merge `theirs` into `ours`. Returns the merged set and whether it differs from `ours`.
pub fn merge(ours: &[StashEntry], theirs: &[StashEntry]) -> (Vec<StashEntry>, bool) {
    let mut merged = ours.to_vec();
    let mut changed = false;
    for t in theirs {
        match merged.iter_mut().find(|m| m.entry_id == t.entry_id) {
            Some(m) => changed |= merge_entry(m, t),
            None => {
                merged.push(t.clone());
                changed = true;
            }
        }
    }
    (merged, changed)
}

fn by_position(a: &StashEntry, b: &StashEntry) -> Ordering {
    a.sort_key
        .partial_cmp(&b.sort_key)
        .unwrap_or(Ordering::Equal)
        .then_with(|| a.entry_id.cmp(&b.entry_id))
}

/// The stash as listed: live entries in order, one per track (the first wins when two devices
/// stashed the same song).
pub fn live(entries: &[StashEntry]) -> Vec<&StashEntry> {
    let mut out: Vec<&StashEntry> = entries.iter().filter(|e| !e.removed).collect();
    out.sort_by(|a, b| by_position(a, b));
    let mut seen = HashSet::new();
    out.retain(|e| seen.insert(&e.track));
    out
}

/// The listed tracks' ids, in order.
pub fn track_ids(entries: &[StashEntry]) -> Vec<TrackRef> {
    live(entries).iter().map(|e| e.track.clone()).collect()
}

/// Stash `tracks` at the end, skipping ones already listed. Returns how many were added and how
/// many didn't fit.
pub fn add(
    entries: &mut Vec<StashEntry>,
    tracks: &[Track],
    device: &DeviceId,
    now: EpochMs,
) -> (usize, usize) {
    let listed = live(entries);
    let mut present: HashSet<TrackRef> = listed.iter().map(|e| e.track.clone()).collect();
    let mut room = MAX_ENTRIES.saturating_sub(listed.len());
    let mut key = entries
        .iter()
        .filter(|e| !e.removed)
        .map(|e| e.sort_key)
        .fold(0.0, f64::max);
    let (mut added, mut dropped) = (0, 0);
    for (i, t) in tracks.iter().enumerate() {
        if !present.insert(TrackRef::new(t.server_id.clone(), t.id.clone())) {
            continue;
        }
        if room == 0 {
            dropped += 1;
            continue;
        }
        room -= 1;
        key += 1.0;
        added += 1;
        entries.push(StashEntry {
            entry_id: format!("{device}:{}:{i}:{}", now as i64, t.id),
            track: TrackRef::new(t.server_id.clone(), t.id.clone()),
            sort_key: key,
            added_at: now,
            updated_at: now,
            removed: false,
            removed_at: now,
            origin_device: device.clone(),
            title: t.title.clone(),
            artist: t.artist.clone(),
            album: t.album.clone(),
            cover_art: t.cover_art.clone(),
            duration_ms: t.duration_ms,
        });
    }
    (added, dropped)
}

/// Mark entries removed (or, with `removed: false`, restore them), along with any other live entry
/// for the same song (a duplicate from another device that the list hides behind it). Returns the
/// ids that changed.
pub fn set_removed(
    entries: &mut [StashEntry],
    ids: &[String],
    removed: bool,
    now: EpochMs,
) -> Vec<String> {
    let tracks: HashSet<TrackRef> = if removed {
        entries
            .iter()
            .filter(|e| ids.contains(&e.entry_id))
            .map(|e| e.track.clone())
            .collect()
    } else {
        HashSet::new()
    };
    let mut changed = vec![];
    for e in entries.iter_mut() {
        let hit = ids.contains(&e.entry_id) || (removed && !e.removed && tracks.contains(&e.track));
        if e.removed != removed && hit {
            e.removed = removed;
            e.removed_at = bump(e.removed_at, now);
            changed.push(e.entry_id.clone());
        }
    }
    changed
}

/// Remove every listed entry (and the hidden duplicates). Returns the ids removed.
pub fn clear(entries: &mut [StashEntry], now: EpochMs) -> Vec<String> {
    let ids: Vec<String> = entries
        .iter()
        .filter(|e| !e.removed)
        .map(|e| e.entry_id.clone())
        .collect();
    set_removed(entries, &ids, true, now)
}

/// Move one entry to `to` in the listed order. Returns whether anything changed.
pub fn move_to(entries: &mut [StashEntry], entry_id: &str, to: usize, now: EpochMs) -> bool {
    let order: Vec<(String, f64)> = live(entries)
        .iter()
        .map(|e| (e.entry_id.clone(), e.sort_key))
        .collect();
    let Some(from) = order.iter().position(|(id, _)| id == entry_id) else {
        return false;
    };
    let to = to.min(order.len() - 1);
    if from == to {
        return false;
    }
    // A duplicate of the song (stashed on another device too) hides behind the listed entry; moved
    // past it, it would take its place. Drop it: the song stays once, where it was moved to.
    let track = entries
        .iter()
        .find(|e| e.entry_id == entry_id)
        .map(|e| e.track.clone());
    for e in entries.iter_mut() {
        if !e.removed && e.entry_id != entry_id && Some(&e.track) == track.as_ref() {
            e.removed = true;
            e.removed_at = bump(e.removed_at, now);
        }
    }
    let others: Vec<f64> = order
        .iter()
        .filter(|(id, _)| id != entry_id)
        .map(|(_, k)| *k)
        .collect();
    let key = match (
        to.checked_sub(1).and_then(|i| others.get(i)),
        others.get(to),
    ) {
        (None, Some(next)) => next - 1.0,
        (Some(prev), None) => prev + 1.0,
        (Some(prev), Some(next)) => (prev + next) / 2.0,
        (None, None) => return false,
    };
    // Keys that halve too often lose precision; renumber the list then (one write per entry).
    let crowded = |k: f64| others.iter().any(|o| (o - k).abs() < 1e-9);
    let Some(e) = entries.iter_mut().find(|e| e.entry_id == entry_id) else {
        return false;
    };
    if !crowded(key) {
        e.sort_key = key;
        e.updated_at = bump(e.updated_at, now);
        return true;
    }
    let mut ids: Vec<String> = order.into_iter().map(|(id, _)| id).collect();
    let moved = ids.remove(from);
    ids.insert(to, moved);
    for (i, id) in ids.iter().enumerate() {
        if let Some(e) = entries.iter_mut().find(|e| &e.entry_id == id) {
            let key = (i + 1) as f64;
            if e.sort_key != key {
                e.sort_key = key;
                e.updated_at = bump(e.updated_at, now);
            }
        }
    }
    true
}

/// Drop tombstones older than [`TOMBSTONE_TTL_MS`] and all but the newest [`MAX_TOMBSTONES`], and
/// what tombstones don't need (the snapshot: a restored entry shows its track from the library).
pub fn prune(entries: &mut Vec<StashEntry>, now: EpochMs) {
    entries.retain(|e| !e.removed || now - e.removed_at < TOMBSTONE_TTL_MS);
    let mut stones: Vec<(EpochMs, String)> = entries
        .iter()
        .filter(|e| e.removed)
        .map(|e| (e.removed_at, e.entry_id.clone()))
        .collect();
    if stones.len() > MAX_TOMBSTONES {
        stones.sort_by(|a, b| {
            b.0.partial_cmp(&a.0)
                .unwrap_or(Ordering::Equal)
                .then_with(|| a.1.cmp(&b.1))
        });
        let drop: HashSet<String> = stones
            .split_off(MAX_TOMBSTONES)
            .into_iter()
            .map(|s| s.1)
            .collect();
        entries.retain(|e| !drop.contains(&e.entry_id));
    }
    for e in entries.iter_mut().filter(|e| e.removed) {
        e.title.clear();
        e.artist = None;
        e.album = None;
        e.cover_art = None;
    }
}

/// An edit's clock: now, but always past the entry's last write, so it wins even when this
/// device's clock is behind the one that wrote last.
fn bump(prev: EpochMs, now: EpochMs) -> EpochMs {
    if now > prev {
        now
    } else {
        prev + 1.0
    }
}

/// The listed entries as items, each with its track from `resolve` or, failing that, the snapshot
/// taken when it was stashed.
pub fn items(
    entries: &[StashEntry],
    resolve: impl Fn(&[TrackRef]) -> Vec<Track>,
) -> Vec<StashItem> {
    let listed = live(entries);
    let ids: Vec<TrackRef> = listed.iter().map(|e| e.track.clone()).collect();
    let tracks = resolve(&ids);
    listed
        .into_iter()
        .map(|e| StashItem {
            entry_id: e.entry_id.clone(),
            added_at: e.added_at,
            track: tracks
                .iter()
                .find(|t| t.id == e.track.id && t.server_id == e.track.source)
                .cloned()
                .unwrap_or_else(|| snapshot(e)),
        })
        .collect()
}

fn snapshot(e: &StashEntry) -> Track {
    Track {
        id: e.track.id.clone(),
        server_id: e.track.source.clone(),
        title: e.title.clone(),
        artist: e.artist.clone(),
        album: e.album.clone(),
        cover_art: e.cover_art.clone(),
        duration_ms: e.duration_ms,
        ..Track::default()
    }
}

#[cfg(test)]
mod tests;
