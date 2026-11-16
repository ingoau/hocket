//! The library that belongs to the identity rather than to an account
//! (`docs/providers.md`, Phase 3): manual dedup links, Mixed playlists and
//! the Hocket library (stars and adds for accounts that can't keep them).
//!
//! All three travel as one last-write-wins set of [`LibraryRecord`]s keyed
//! by [`LibraryRecord::key`], merged the way the stash and snoozes are, so
//! they ride Connect rooms and the coordinator's replica. [`links`] is the
//! matcher that turns the mirror plus the manual links into groups.
//!
//! Everything here is pure. The actor owns the set, persists it, feeds it
//! to the engine and applies it to the database.

pub mod links;

use serde::{Deserialize, Serialize};

use crate::api::{EpochMs, Track, TrackRef};

/// The source Mixed playlists are listed under (`Playlist::server_id`).
pub const HOCKET_SOURCE: &str = "hocket";

/// How long a deleted record is remembered, so a device that missed the
/// delete can't bring it back. Matches the replica's lifetime.
pub const TOMBSTONE_TTL_MS: EpochMs = 30.0 * 24.0 * 3600.0 * 1000.0;

/// The saved-state key for an identity's library records.
pub fn state_key(scope: &str) -> String {
    format!("library:{scope}")
}

/// One record of the identity's library.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LibraryRecord {
    /// What the record is about; one record per key (see the constructors).
    pub key: String,
    /// LWW clock.
    pub updated_at: EpochMs,
    /// A tombstone (a deleted playlist, an unstarred item).
    #[serde(default)]
    pub removed: bool,
    pub body: RecordBody,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", tag = "type", content = "data")]
#[allow(clippy::large_enum_variant)]
pub enum RecordBody {
    /// Two songs are (or are not) the same: "Link to…" and "Not the same song".
    Link {
        a: TrackRef,
        b: TrackRef,
        same: bool,
    },
    /// A Mixed playlist.
    Playlist(MixedPlaylist),
    /// A song starred or added on an account that can't keep it.
    Saved(SavedTrack),
}

/// A playlist Hocket keeps, whose songs may come from any account.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct MixedPlaylist {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub comment: Option<String>,
    pub tracks: Vec<TrackRef>,
    pub created: EpochMs,
}

/// A song in the Hocket library: starred, added, or both. Carries what a
/// device needs to list it without the account's mirror.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct SavedTrack {
    pub track: TrackRef,
    #[serde(default)]
    pub starred: bool,
    #[serde(default)]
    pub added: bool,
    /// The song's metadata when it was saved.
    #[serde(default)]
    pub snapshot: Option<Track>,
}

fn ref_key(r: &TrackRef) -> String {
    format!("{}\u{1}{}", r.source, r.id)
}

impl LibraryRecord {
    /// The key of the link between `a` and `b` (in either order).
    pub fn link_key(a: &TrackRef, b: &TrackRef) -> String {
        let (x, y) = (ref_key(a), ref_key(b));
        let (x, y) = if x <= y { (x, y) } else { (y, x) };
        format!("link:{x}\u{2}{y}")
    }

    pub fn playlist_key(id: &str) -> String {
        format!("playlist:{id}")
    }

    pub fn saved_key(track: &TrackRef) -> String {
        format!("saved:{}", ref_key(track))
    }

    pub fn link(a: TrackRef, b: TrackRef, same: bool, now: EpochMs) -> LibraryRecord {
        LibraryRecord {
            key: Self::link_key(&a, &b),
            updated_at: now,
            removed: false,
            body: RecordBody::Link { a, b, same },
        }
    }

    pub fn playlist(p: MixedPlaylist, now: EpochMs) -> LibraryRecord {
        LibraryRecord {
            key: Self::playlist_key(&p.id),
            updated_at: now,
            removed: false,
            body: RecordBody::Playlist(p),
        }
    }

    pub fn saved(s: SavedTrack, now: EpochMs) -> LibraryRecord {
        LibraryRecord {
            key: Self::saved_key(&s.track),
            updated_at: now,
            removed: !s.starred && !s.added,
            body: RecordBody::Saved(s),
        }
    }
}

/// `candidate` beats `current`: newer, then a tombstone, then the larger
/// body (so every replica settles a tie the same way).
fn wins(candidate: &LibraryRecord, current: &LibraryRecord) -> bool {
    if candidate.updated_at != current.updated_at {
        return candidate.updated_at > current.updated_at;
    }
    if candidate.removed != current.removed {
        return candidate.removed;
    }
    let c = serde_json::to_string(&candidate.body).unwrap_or_default();
    let o = serde_json::to_string(&current.body).unwrap_or_default();
    c > o
}

/// Merge `theirs` into `ours`. Returns the merged set (sorted by key) and
/// whether it differs from `ours`.
pub fn merge(ours: &[LibraryRecord], theirs: &[LibraryRecord]) -> (Vec<LibraryRecord>, bool) {
    let mut merged: std::collections::BTreeMap<String, LibraryRecord> =
        ours.iter().map(|r| (r.key.clone(), r.clone())).collect();
    let mut changed = false;
    for t in theirs {
        match merged.get(&t.key) {
            Some(m) if !wins(t, m) => {}
            _ => {
                merged.insert(t.key.clone(), t.clone());
                changed = true;
            }
        }
    }
    (merged.into_values().collect(), changed)
}

/// Put `record` into `set` with a clock past the one it replaces.
pub fn put(set: &mut Vec<LibraryRecord>, mut record: LibraryRecord, now: EpochMs) {
    let prev = set
        .iter()
        .find(|r| r.key == record.key)
        .map(|r| r.updated_at);
    record.updated_at = match prev {
        Some(p) if p >= now => p + 1.0,
        _ => now,
    };
    let (merged, _) = merge(set, std::slice::from_ref(&record));
    *set = merged;
}

/// Forget tombstones older than [`TOMBSTONE_TTL_MS`].
pub fn prune(set: &mut Vec<LibraryRecord>, now: EpochMs) {
    set.retain(|r| !r.removed || now - r.updated_at < TOMBSTONE_TTL_MS);
}

/// The manual links, as the matcher takes them.
pub fn manual_links(set: &[LibraryRecord]) -> Vec<links::ManualLink> {
    set.iter()
        .filter(|r| !r.removed)
        .filter_map(|r| match &r.body {
            RecordBody::Link { a, b, same } => Some(links::ManualLink {
                a: a.clone(),
                b: b.clone(),
                same: *same,
            }),
            _ => None,
        })
        .collect()
}

/// The live Mixed playlists, in creation order.
pub fn playlists(set: &[LibraryRecord]) -> Vec<MixedPlaylist> {
    let mut out: Vec<MixedPlaylist> = set
        .iter()
        .filter(|r| !r.removed)
        .filter_map(|r| match &r.body {
            RecordBody::Playlist(p) => Some(p.clone()),
            _ => None,
        })
        .collect();
    out.sort_by(|a, b| a.created.total_cmp(&b.created).then(a.id.cmp(&b.id)));
    out
}

pub fn playlist(set: &[LibraryRecord], id: &str) -> Option<MixedPlaylist> {
    let key = LibraryRecord::playlist_key(id);
    set.iter()
        .find(|r| r.key == key && !r.removed)
        .and_then(|r| match &r.body {
            RecordBody::Playlist(p) => Some(p.clone()),
            _ => None,
        })
}

/// The live Hocket library entries.
pub fn saved(set: &[LibraryRecord]) -> Vec<SavedTrack> {
    set.iter()
        .filter(|r| !r.removed)
        .filter_map(|r| match &r.body {
            RecordBody::Saved(s) => Some(s.clone()),
            _ => None,
        })
        .collect()
}

pub fn saved_entry(set: &[LibraryRecord], track: &TrackRef) -> Option<SavedTrack> {
    let key = LibraryRecord::saved_key(track);
    set.iter()
        .find(|r| r.key == key && !r.removed)
        .and_then(|r| match &r.body {
            RecordBody::Saved(s) => Some(s.clone()),
            _ => None,
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(source: &str, id: &str) -> TrackRef {
        TrackRef::new(source, id)
    }

    #[test]
    fn link_keys_ignore_order() {
        assert_eq!(
            LibraryRecord::link_key(&r("a", "1"), &r("b", "2")),
            LibraryRecord::link_key(&r("b", "2"), &r("a", "1"))
        );
    }

    #[test]
    fn last_write_wins_and_merges_commute() {
        let mut ours = vec![];
        put(
            &mut ours,
            LibraryRecord::link(r("a", "1"), r("b", "2"), true, 0.0),
            10.0,
        );
        let mut theirs = vec![];
        put(
            &mut theirs,
            LibraryRecord::link(r("b", "2"), r("a", "1"), false, 0.0),
            20.0,
        );
        put(
            &mut theirs,
            LibraryRecord::playlist(
                MixedPlaylist {
                    id: "p".into(),
                    name: "Mix".into(),
                    tracks: vec![r("a", "1"), r("b", "3")],
                    ..Default::default()
                },
                0.0,
            ),
            20.0,
        );
        let (m1, changed) = merge(&ours, &theirs);
        assert!(changed);
        let (m2, _) = merge(&theirs, &ours);
        assert_eq!(m1, m2);
        assert_eq!(manual_links(&m1).len(), 1);
        assert!(
            !manual_links(&m1)[0].same,
            "the newer \"not the same\" wins"
        );
        assert_eq!(playlists(&m1)[0].tracks.len(), 2);
        // Merging again changes nothing.
        assert!(!merge(&m1, &m2).1);
    }

    #[test]
    fn a_put_always_moves_the_clock_forward() {
        let mut set = vec![];
        put(
            &mut set,
            LibraryRecord::link(r("a", "1"), r("b", "2"), true, 0.0),
            50.0,
        );
        // A device whose clock is behind still overrides its own last write.
        put(
            &mut set,
            LibraryRecord::link(r("a", "1"), r("b", "2"), false, 0.0),
            40.0,
        );
        assert!(!manual_links(&set)[0].same);
        assert_eq!(set[0].updated_at, 51.0);
    }

    #[test]
    fn unstarring_is_a_tombstone_that_expires() {
        let mut set = vec![];
        let s = SavedTrack {
            track: r("y", "1"),
            starred: true,
            ..Default::default()
        };
        put(&mut set, LibraryRecord::saved(s.clone(), 0.0), 1.0);
        assert_eq!(saved(&set).len(), 1);
        put(
            &mut set,
            LibraryRecord::saved(
                SavedTrack {
                    starred: false,
                    ..s
                },
                0.0,
            ),
            2.0,
        );
        assert!(saved(&set).is_empty());
        prune(&mut set, 2.0 + TOMBSTONE_TTL_MS + 1.0);
        assert!(set.is_empty());
    }
}
