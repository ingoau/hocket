//! Snoozed songs: a track left out of shuffle, playlists, artist play and autoplay until a time,
//! or until the listener unsnoozes it. The set belongs to the account, not the device, and merges
//! last-write-wins per track the way the stash does, so it rides Connect and the coordinator.
//!
//! An entry stores when the snooze ends (`None` means it doesn't). One whose end has passed is
//! dropped lazily: it is already ignored, and the record goes the next time the set is pruned.
//! Unsnooze is a tombstone, kept for [`TOMBSTONE_TTL_MS`] so a device that missed it can't bring
//! the snooze back.
//!
//! Everything here is pure. The actor owns the list, persists it and feeds it to the engine.

use std::collections::HashSet;

use crate::api::{ContextKind, EpochMs, Snooze, SnoozeFor, SnoozeItem, Track, TrackId};

/// How long a removed entry is remembered. Matches the stash: the coordinator's replica lives as long.
pub const TOMBSTONE_TTL_MS: EpochMs = 30.0 * 24.0 * 3600.0 * 1000.0;

const DAY_MS: EpochMs = 24.0 * 3600.0 * 1000.0;

/// The saved-state key for one account's snoozes.
pub fn state_key(scope: &str) -> String {
    format!("snooze:{scope}")
}

/// When a snooze of `duration`, started at `now`, ends. `None` does not end.
pub fn until_for(duration: SnoozeFor, now: EpochMs) -> Option<EpochMs> {
    match duration {
        SnoozeFor::Day => Some(now + DAY_MS),
        SnoozeFor::ThreeDays => Some(now + 3.0 * DAY_MS),
        SnoozeFor::Week => Some(now + 7.0 * DAY_MS),
        SnoozeFor::Month => Some(now + 30.0 * DAY_MS),
        SnoozeFor::Indefinite => None,
    }
}

fn bump(prev: EpochMs, now: EpochMs) -> EpochMs {
    if now > prev {
        now
    } else {
        prev + 1.0
    }
}

fn wins(candidate: &Snooze, current: &Snooze) -> bool {
    candidate.updated_at > current.updated_at
        || (candidate.updated_at == current.updated_at && candidate.removed && !current.removed)
}

/// Still in force at `now`: not removed, and not past its end.
pub fn is_live(entry: &Snooze, now: EpochMs) -> bool {
    !entry.removed && entry.until.map(|u| u > now).unwrap_or(true)
}

/// Merge `theirs` into `ours`. Returns the merged set and whether it differs from `ours`.
pub fn merge(ours: &[Snooze], theirs: &[Snooze]) -> (Vec<Snooze>, bool) {
    let mut merged = ours.to_vec();
    let mut changed = false;
    for t in theirs {
        match merged.iter_mut().find(|m| m.track_id == t.track_id) {
            Some(m) => {
                if wins(t, m) {
                    *m = t.clone();
                    changed = true;
                }
            }
            None => {
                merged.push(t.clone());
                changed = true;
            }
        }
    }
    (merged, changed)
}

/// Forget snoozes that have ended, and tombstones older than [`TOMBSTONE_TTL_MS`].
///
/// An ended snooze becomes a tombstone and keeps its clock, so an older copy from a device that
/// was away cannot bring it back. The clock is not moved forward: a newer snooze from another
/// device still wins. How long the tombstone is kept is measured from when the snooze stopped
/// applying (its end, or the unsnooze), not from when it was set. A month is the same length as
/// the tombstone window, so measuring from the set time would drop a month-long snooze in the
/// same prune that ends it.
pub fn prune(entries: &mut Vec<Snooze>, now: EpochMs) {
    for e in entries.iter_mut() {
        if !e.removed && e.until.is_some_and(|u| u <= now) {
            e.removed = true;
        }
    }
    entries.retain(|e| !e.removed || now - tombstone_from(e) < TOMBSTONE_TTL_MS);
}

/// When a removed entry stopped applying: the later of its end and its clock. An unsnooze has no
/// end, so that is just the clock.
fn tombstone_from(entry: &Snooze) -> EpochMs {
    entry
        .until
        .map_or(entry.updated_at, |until| until.max(entry.updated_at))
}

/// Track ids snoozed at `now`, in a stable order.
pub fn active_ids(entries: &[Snooze], now: EpochMs) -> Vec<TrackId> {
    let mut ids: Vec<TrackId> = entries
        .iter()
        .filter(|e| is_live(e, now))
        .map(|e| e.track_id.clone())
        .collect();
    ids.sort();
    ids
}

/// What a track's snooze is at `now`: absent when it isn't snoozed, otherwise its end
/// (`None` inside means it doesn't end).
pub fn state_of(entries: &[Snooze], track_id: &str, now: EpochMs) -> Option<Option<EpochMs>> {
    entries
        .iter()
        .find(|e| e.track_id == track_id && is_live(e, now))
        .map(|e| e.until)
}

/// Snooze `track_id` until `until` (`None` means it doesn't end). Returns whether the live state changed.
pub fn set(
    entries: &mut Vec<Snooze>,
    track_id: &str,
    until: Option<EpochMs>,
    now: EpochMs,
) -> bool {
    let live = state_of(entries, track_id, now);
    if live == Some(until) {
        return false;
    }
    let at = entries
        .iter()
        .find(|e| e.track_id == track_id)
        .map(|e| bump(e.updated_at, now))
        .unwrap_or(now);
    if let Some(e) = entries.iter_mut().find(|e| e.track_id == track_id) {
        e.until = until;
        e.removed = false;
        e.updated_at = at;
    } else {
        entries.push(Snooze {
            track_id: track_id.to_string(),
            until,
            updated_at: at,
            removed: false,
        });
    }
    true
}

/// Unsnooze `track_id`. Returns whether it was snoozed.
pub fn clear(entries: &mut Vec<Snooze>, track_id: &str, now: EpochMs) -> bool {
    if state_of(entries, track_id, now).is_none() {
        return false;
    }
    let at = entries
        .iter()
        .find(|e| e.track_id == track_id)
        .map(|e| bump(e.updated_at, now))
        .unwrap_or(now);
    if let Some(e) = entries.iter_mut().find(|e| e.track_id == track_id) {
        e.removed = true;
        e.updated_at = at;
    }
    true
}

/// Contexts that leave snoozed songs out. An album plays in full, and an ad-hoc list is one the
/// listener picked themselves (a search, a selection).
pub fn leaves_out(kind: &ContextKind) -> bool {
    matches!(
        kind,
        ContextKind::Artist { .. }
            | ContextKind::Playlist { .. }
            | ContextKind::Genre { .. }
            | ContextKind::Filter { .. }
            | ContextKind::Autoplay
    )
}

/// The tracks a context expands into, and where playback starts.
///
/// Snoozed songs are left out of the contexts [`leaves_out`] names. `start` is the song the
/// listener picked: it stays, and the index is moved to wherever it lands. When that would leave
/// nothing (every song snoozed, and they didn't pick one), the list plays as it is.
pub fn filter_context(
    tracks: &[TrackId],
    kind: &ContextKind,
    start: Option<u32>,
    snoozed: &HashSet<TrackId>,
) -> (Vec<TrackId>, Option<u32>) {
    if !leaves_out(kind) || tracks.is_empty() || snoozed.is_empty() {
        return (tracks.to_vec(), start);
    }
    let keep_at = start
        .filter(|i| (*i as usize) < tracks.len())
        .map(|i| i as usize);
    let filtered: Vec<TrackId> = tracks
        .iter()
        .enumerate()
        .filter(|(i, id)| Some(*i) == keep_at || !snoozed.contains(*id))
        .map(|(_, id)| id.clone())
        .collect();
    if filtered.is_empty() {
        return (tracks.to_vec(), start);
    }
    // Count what stays ahead of the picked song. Searching by id would start on an earlier copy.
    let start = keep_at.map(|i| {
        tracks
            .iter()
            .take(i)
            .filter(|id| !snoozed.contains(*id))
            .count() as u32
    });
    (filtered, start)
}

/// The live snoozes for the UI, soonest ending first (one that doesn't end, last).
pub fn items(
    entries: &[Snooze],
    now: EpochMs,
    lookup: impl Fn(&[TrackId]) -> Vec<Track>,
) -> Vec<SnoozeItem> {
    let mut live: Vec<&Snooze> = entries.iter().filter(|e| is_live(e, now)).collect();
    live.sort_by(|a, b| {
        match (a.until, b.until) {
            (Some(x), Some(y)) => x.partial_cmp(&y).unwrap_or(std::cmp::Ordering::Equal),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => std::cmp::Ordering::Equal,
        }
        .then_with(|| a.track_id.cmp(&b.track_id))
    });
    let ids: Vec<TrackId> = live.iter().map(|e| e.track_id.clone()).collect();
    let found = lookup(&ids);
    live.into_iter()
        .map(|e| {
            let track = found
                .iter()
                .find(|t| t.id == e.track_id)
                .cloned()
                .unwrap_or_else(|| Track {
                    id: e.track_id.clone(),
                    title: e.track_id.clone(),
                    ..Track::default()
                });
            SnoozeItem {
                track_id: e.track_id.clone(),
                until: e.until,
                track,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(v: &[&str]) -> Vec<TrackId> {
        v.iter().map(|s| s.to_string()).collect()
    }

    fn set_of(v: &[&str]) -> HashSet<TrackId> {
        v.iter().map(|s| s.to_string()).collect()
    }

    fn kind_playlist() -> ContextKind {
        ContextKind::Playlist { id: "p".into() }
    }

    #[test]
    fn durations_land_on_the_day_the_week_and_the_month() {
        assert_eq!(until_for(SnoozeFor::Day, 1_000.0), Some(1_000.0 + DAY_MS));
        assert_eq!(until_for(SnoozeFor::ThreeDays, 0.0), Some(3.0 * DAY_MS));
        assert_eq!(until_for(SnoozeFor::Week, 0.0), Some(7.0 * DAY_MS));
        assert_eq!(until_for(SnoozeFor::Month, 0.0), Some(30.0 * DAY_MS));
        assert_eq!(until_for(SnoozeFor::Indefinite, 5.0), None);
    }

    #[test]
    fn an_expired_snooze_is_ignored_and_then_dropped() {
        let mut entries = vec![Snooze {
            track_id: "t".into(),
            until: Some(50.0),
            updated_at: 1.0,
            removed: false,
        }];
        assert!(is_live(&entries[0], 49.0));
        assert!(!is_live(&entries[0], 50.0));
        assert!(active_ids(&entries, 49.0).contains(&"t".to_string()));
        assert!(active_ids(&entries, 50.0).is_empty());
        prune(&mut entries, 50.0);
        assert!(entries[0].removed, "ended snooze stays as a tombstone");
        assert!(active_ids(&entries, 50.0).is_empty());
    }

    #[test]
    fn an_ended_snooze_is_not_brought_back_by_an_older_copy() {
        let mut local = vec![Snooze {
            track_id: "t".into(),
            until: Some(100.0),
            updated_at: 20.0,
            removed: false,
        }];
        prune(&mut local, 100.0);
        let stale = Snooze {
            track_id: "t".into(),
            until: None,
            updated_at: 10.0,
            removed: false,
        };
        let (mut merged, _) = merge(&local, &[stale]);
        prune(&mut merged, 100.0);
        assert!(active_ids(&merged, 100.0).is_empty());
        assert!(merged.iter().any(|e| e.track_id == "t" && e.removed));
    }

    #[test]
    fn a_month_long_snooze_still_blocks_an_older_copy_after_it_ends() {
        let set_at = 0.0;
        let until = set_at + 30.0 * DAY_MS;
        let mut local = vec![Snooze {
            track_id: "t".into(),
            until: Some(until),
            updated_at: set_at,
            removed: false,
        }];
        // A day after it ended, and 31 days after it was set: past the tombstone window if that
        // window were measured from the set time.
        let prune_at = set_at + 31.0 * DAY_MS;
        prune(&mut local, prune_at);
        assert!(
            local.iter().any(|e| e.track_id == "t" && e.removed),
            "the ended month stays as a tombstone"
        );
        assert_eq!(
            local[0].updated_at, set_at,
            "the clock is not moved forward"
        );
        let stale = Snooze {
            track_id: "t".into(),
            until: None,
            updated_at: set_at - 1.0,
            removed: false,
        };
        let (merged, _) = merge(&local, &[stale]);
        assert!(active_ids(&merged, prune_at).is_empty());
        // A newer snooze from another device still wins.
        let again = Snooze {
            track_id: "t".into(),
            until: None,
            updated_at: prune_at,
            removed: false,
        };
        let (merged, _) = merge(&local, &[again]);
        assert_eq!(state_of(&merged, "t", prune_at), Some(None));
    }

    #[test]
    fn picking_a_later_copy_of_a_repeated_song_starts_on_that_copy() {
        let tracks = ids(&["a", "b", "a"]);
        let (out, start) = filter_context(&tracks, &kind_playlist(), Some(2), &set_of(&["b"]));
        assert_eq!(out, ids(&["a", "a"]));
        assert_eq!(start, Some(1));
    }

    #[test]
    fn an_indefinite_snooze_stays_until_it_is_cleared() {
        let mut entries = vec![];
        assert!(set(&mut entries, "t", None, 10.0));
        assert_eq!(state_of(&entries, "t", 1.0e15), Some(None));
        assert!(clear(&mut entries, "t", 20.0));
        assert_eq!(state_of(&entries, "t", 20.0), None);
        assert!(!clear(&mut entries, "t", 21.0));
        // The tombstone remains, so a stale copy can't bring it back.
        assert_eq!(entries.len(), 1);
        assert!(entries[0].removed);
    }

    #[test]
    fn the_later_write_wins_and_a_tie_prefers_the_removal() {
        let snoozed = Snooze {
            track_id: "t".into(),
            until: Some(100.0),
            updated_at: 5.0,
            removed: false,
        };
        let cleared = Snooze {
            track_id: "t".into(),
            until: Some(100.0),
            updated_at: 5.0,
            removed: true,
        };
        let (merged, _) = merge(&[snoozed.clone()], &[cleared.clone()]);
        assert!(merged[0].removed);
        let later = Snooze {
            updated_at: 6.0,
            removed: false,
            ..snoozed.clone()
        };
        let (merged, changed) = merge(&[cleared], &[later]);
        assert!(changed);
        assert!(!merged[0].removed);
    }

    #[test]
    fn concurrent_snoozes_of_different_songs_both_survive() {
        let a = Snooze {
            track_id: "a".into(),
            until: None,
            updated_at: 1.0,
            removed: false,
        };
        let b = Snooze {
            track_id: "b".into(),
            until: Some(9.0),
            updated_at: 2.0,
            removed: false,
        };
        let (merged, _) = merge(&[a], &[b]);
        assert_eq!(active_ids(&merged, 0.0), ids(&["a", "b"]));
    }

    #[test]
    fn each_context_leaves_snoozed_songs_out_or_keeps_them() {
        let tracks = ids(&["a", "b", "c"]);
        let snoozed = set_of(&["b"]);
        let cases = [
            (ContextKind::Playlist { id: "p".into() }, true),
            (ContextKind::Artist { id: "ar".into() }, true),
            (ContextKind::Genre { name: "g".into() }, true),
            (
                ContextKind::Filter {
                    filter: crate::api::Filter {
                        id: "f".into(),
                        name: "f".into(),
                        root: crate::api::FilterNode::All(vec![]),
                        sort: crate::api::SortOrder::Default,
                        descending: false,
                        limit: None,
                    },
                },
                true,
            ),
            (ContextKind::Autoplay, true),
            (ContextKind::Album { id: "al".into() }, false),
            (
                ContextKind::AdHoc {
                    label: "search".into(),
                },
                false,
            ),
        ];
        for (kind, leaves) in cases {
            let (out, start) = filter_context(&tracks, &kind, None, &snoozed);
            if leaves {
                assert_eq!(out, ids(&["a", "c"]), "{kind:?}");
            } else {
                assert_eq!(out, tracks, "{kind:?}");
            }
            assert_eq!(start, None);
        }
    }

    #[test]
    fn picking_a_snoozed_song_plays_it_and_skips_the_other_snoozed_ones() {
        let tracks = ids(&["a", "b", "c", "d"]);
        let snoozed = set_of(&["b", "c"]);
        let (out, start) = filter_context(&tracks, &kind_playlist(), Some(2), &snoozed);
        assert_eq!(out, ids(&["a", "c", "d"]));
        assert_eq!(start, Some(1));
    }

    #[test]
    fn a_playlist_of_only_snoozed_songs_still_plays() {
        let tracks = ids(&["a", "b"]);
        let (out, start) = filter_context(&tracks, &kind_playlist(), None, &set_of(&["a", "b"]));
        assert_eq!(out, tracks);
        assert_eq!(start, None);
        // Picking one of them plays just that one: the rest stay skipped.
        let (out, start) = filter_context(&tracks, &kind_playlist(), Some(1), &set_of(&["a", "b"]));
        assert_eq!(out, ids(&["b"]));
        assert_eq!(start, Some(0));
    }

    #[test]
    fn an_album_keeps_a_snoozed_song_even_when_one_is_picked() {
        let tracks = ids(&["a", "b"]);
        let kind = ContextKind::Album { id: "al".into() };
        let (out, start) = filter_context(&tracks, &kind, Some(1), &set_of(&["a", "b"]));
        assert_eq!(out, tracks);
        assert_eq!(start, Some(1));
    }
}
