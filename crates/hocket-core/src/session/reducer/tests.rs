//! Unit tests for the flat queue reducer. The invariants over arbitrary op
//! sequences are in `session::proptests`.

use super::*;
use crate::api::{QueueEntry, RatingTarget};
use crate::session::document::{new_document, validate};
use crate::session::saved::SNAPSHOT_MAX_ENTRIES;
use crate::session::DeterministicEntropy;

fn ctx<'a>(e: &'a DeterministicEntropy, now: f64, position_ms: Ms) -> ReduceCtx<'a> {
    ReduceCtx {
        now,
        position_ms,
        entropy: e,
    }
}

fn album(id: &str, n: usize) -> QueueContext {
    QueueContext {
        source: "srv".into(),
        kind: ContextKind::Album { id: id.into() },
        label: format!("Album {id}"),
        sort: SortOrder::Default,
        tracks: (0..n)
            .map(|i| format!("{id}{i}"))
            .map(crate::api::TrackRef::from)
            .collect(),
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
        start_position_ms: None,
    };
    step(doc, e, QueueOp::PlayContext { args }).0
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

fn doc_of(op: QueueOp, doc: &SessionDocument, e: &DeterministicEntropy) -> SessionDocument {
    step(doc, e, op).0
}

fn err(doc: &SessionDocument, e: &DeterministicEntropy, op: QueueOp) -> ReduceError {
    reduce(doc, op, &ctx(e, 2.0, 0)).unwrap_err()
}

fn cur(doc: &SessionDocument) -> &str {
    doc.current
        .as_ref()
        .map(|c| c.track.id.as_str())
        .unwrap_or("")
}

fn list(doc: &SessionDocument) -> Vec<String> {
    Flat::of(doc)
        .list
        .into_iter()
        .map(|i| i.track)
        .map(|t| t.id)
        .collect()
}

fn upcoming(doc: &SessionDocument) -> Vec<String> {
    doc.upcoming
        .iter()
        .map(|i| i.track.clone())
        .map(|t| t.id)
        .collect()
}

fn history(doc: &SessionDocument) -> Vec<String> {
    doc.history
        .iter()
        .map(|i| i.track.clone())
        .map(|t| t.id)
        .collect()
}

fn group(doc: &SessionDocument) -> Vec<String> {
    if doc.current.is_none() {
        return vec![];
    }
    doc.upcoming
        .iter()
        .take_while(|i| i.queued)
        .map(|i| i.track.clone())
        .map(|t| t.id)
        .collect()
}

fn key_of(doc: &SessionDocument, track: &str) -> QueueKey {
    Flat::of(doc)
        .list
        .into_iter()
        .find(|i| i.track == track)
        .map(|i| i.key)
        .unwrap_or_else(|| panic!("no {track}"))
}

fn ids(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

fn play_next(track_ids: &[&str], keys: Vec<QueueKey>, queued: bool) -> QueueOp {
    QueueOp::PlayNext {
        track_ids: (ids(track_ids))
            .into_iter()
            .map(crate::api::TrackRef::from)
            .collect::<Vec<_>>(),
        keys,
        queued,
    }
}

fn add_to_queue(track_ids: &[&str], keys: Vec<QueueKey>) -> QueueOp {
    QueueOp::AddToQueue {
        track_ids: (ids(track_ids))
            .into_iter()
            .map(crate::api::TrackRef::from)
            .collect::<Vec<_>>(),
        keys,
    }
}

fn play_later(track_ids: &[&str], keys: Vec<QueueKey>) -> QueueOp {
    QueueOp::PlayLater {
        track_ids: (ids(track_ids))
            .into_iter()
            .map(crate::api::TrackRef::from)
            .collect::<Vec<_>>(),
        keys,
    }
}

fn fresh() -> SessionDocument {
    new_document("srv:user", "sess".into(), 0.0)
}

// -- navigation ---------------------------------------------------------------

#[test]
fn play_context_starts_at_the_chosen_track_with_the_rest_around_it() {
    let e = DeterministicEntropy::new(1);
    let d = play(&fresh(), &e, album("a", 4), Some(1), false);
    assert_eq!(history(&d), ids(&["a0"]));
    assert_eq!(cur(&d), "a1");
    assert_eq!(upcoming(&d), ids(&["a2", "a3"]));
    assert!(Flat::of(&d).list.iter().all(|i| !i.added && !i.queued));
    let ords: Vec<u32> = Flat::of(&d).list.iter().map(|i| i.ordinal).collect();
    assert_eq!(ords, vec![0, 1, 2, 3]);
}

#[test]
fn next_and_previous_move_the_pointer_and_never_edit_the_list() {
    let e = DeterministicEntropy::new(1);
    let d = play(&fresh(), &e, album("a", 5), Some(0), false);
    let before = Flat::of(&d).list;
    let mut x = d.clone();
    for _ in 0..3 {
        x = doc_of(QueueOp::Next, &x, &e);
    }
    assert_eq!(cur(&x), "a3");
    assert_eq!(Flat::of(&x).list, before, "nothing consumed or reordered");
    for _ in 0..3 {
        x = doc_of(QueueOp::Previous, &x, &e);
    }
    assert_eq!(x.current, d.current);
    assert_eq!(Flat::of(&x).list, before);
}

#[test]
fn jump_puts_the_skipped_entries_behind_the_pointer() {
    let e = DeterministicEntropy::new(1);
    let d = play(&fresh(), &e, album("a", 6), Some(0), false);
    let k = key_of(&d, "a4");
    let j = doc_of(QueueOp::JumpToQueueItem { key: k }, &d, &e);
    assert_eq!(cur(&j), "a4");
    assert_eq!(history(&j), ids(&["a0", "a1", "a2", "a3"]));
    let mut back = j.clone();
    for _ in 0..4 {
        back = doc_of(QueueOp::Previous, &back, &e);
    }
    assert_eq!(back.current, d.current);
    // Jumping back into history works the same way.
    let k0 = key_of(&j, "a1");
    let j2 = doc_of(QueueOp::JumpToQueueItem { key: k0 }, &j, &e);
    assert_eq!(cur(&j2), "a1");
    assert_eq!(list(&j2), list(&d));
    // Jumping to the current entry restarts it.
    let (_, fx) = step(
        &j2,
        &e,
        QueueOp::JumpToQueueItem {
            key: j2.current.clone().unwrap().key,
        },
    );
    assert!(fx.contains(&Effect::RestartCurrent));
}

#[test]
fn previous_at_the_start_restarts() {
    let e = DeterministicEntropy::new(1);
    let d = play(&fresh(), &e, album("a", 3), Some(0), false);
    let (x, fx) = step(&d, &e, QueueOp::Previous);
    assert_eq!(x.current, d.current);
    assert_eq!(fx, vec![Effect::RestartCurrent]);
}

#[test]
fn the_end_of_the_list_stops_or_asks_autoplay() {
    let e = DeterministicEntropy::new(1);
    let d = play(&fresh(), &e, album("a", 2), Some(1), false);
    let (x, fx) = step(&d, &e, QueueOp::Next);
    assert_eq!(cur(&x), "a1");
    assert_eq!(fx, vec![Effect::Stopped]);
    let d = doc_of(QueueOp::SetAutoplay { enabled: true }, &d, &e);
    let (_, fx) = step(&d, &e, QueueOp::TrackEnded);
    assert_eq!(fx, vec![Effect::NeedsAutoplay]);
}

#[test]
fn repeat_all_wraps_both_ways_and_repeat_one_only_on_track_end() {
    let e = DeterministicEntropy::new(1);
    let d = play(&fresh(), &e, album("a", 3), Some(2), false);
    let d = doc_of(
        QueueOp::SetRepeat {
            mode: RepeatMode::All,
        },
        &d,
        &e,
    );
    assert_eq!(
        derive(&d)
            .wrap_next
            .map(|i| i.track)
            .map(|t| t.id)
            .as_deref(),
        Some("a0")
    );
    let w = doc_of(QueueOp::Next, &d, &e);
    assert_eq!(cur(&w), "a0");
    assert_eq!(list(&w), list(&d), "a wrap edits nothing");
    assert_eq!(
        derive(&w)
            .previous
            .map(|i| i.track)
            .map(|t| t.id)
            .as_deref(),
        Some("a2")
    );
    let back = doc_of(QueueOp::Previous, &w, &e);
    assert_eq!(back.current, d.current);

    let one = doc_of(
        QueueOp::SetRepeat {
            mode: RepeatMode::One,
        },
        &w,
        &e,
    );
    let (x, fx) = step(&one, &e, QueueOp::TrackEnded);
    assert_eq!(x.current, one.current);
    assert_eq!(fx, vec![Effect::RestartCurrent]);
    let x = doc_of(QueueOp::Next, &one, &e);
    assert_eq!(cur(&x), "a1", "Next still steps under Repeat One");

    // Repeat All around a single song restarts it.
    let s = play(&fresh(), &e, album("s", 1), Some(0), false);
    let s = doc_of(
        QueueOp::SetRepeat {
            mode: RepeatMode::All,
        },
        &s,
        &e,
    );
    let (x, fx) = step(&s, &e, QueueOp::Next);
    assert_eq!(x.current, s.current);
    assert!(fx.contains(&Effect::RestartCurrent));
}

#[test]
fn unavailable_entries_are_skipped_both_ways_unless_jumped_to() {
    let e = DeterministicEntropy::new(1);
    let d = play(&fresh(), &e, album("a", 4), Some(0), false);
    let k1 = key_of(&d, "a1");
    let d = doc_of(QueueOp::SkipUnavailable { key: k1.clone() }, &d, &e);
    assert_eq!(cur(&d), "a0", "marking an upcoming entry doesn't move");
    let (x, fx) = step(&d, &e, QueueOp::Next);
    assert_eq!(cur(&x), "a2");
    assert!(fx.contains(&Effect::Skipped { key: k1.clone() }));
    let back = doc_of(QueueOp::Previous, &x, &e);
    assert_eq!(cur(&back), "a0", "skipped going back too");
    let j = doc_of(QueueOp::JumpToQueueItem { key: k1.clone() }, &x, &e);
    assert_eq!(cur(&j), "a1");
    assert!(!j.current.as_ref().unwrap().unavailable, "a jump clears it");
    // Marking the current entry moves on.
    let (y, fx) = step(&j, &e, QueueOp::SkipUnavailable { key: k1.clone() });
    assert_eq!(cur(&y), "a2");
    assert!(fx.contains(&Effect::Skipped { key: k1 }));
}

#[test]
fn offline_marks_clear_and_real_failures_stay() {
    let e = DeterministicEntropy::new(1);
    let d = play(&fresh(), &e, album("a", 4), Some(0), false);
    let (k1, k2) = (key_of(&d, "a1"), key_of(&d, "a2"));
    let d = doc_of(QueueOp::SkipOffline { key: k1.clone() }, &d, &e);
    let d = doc_of(QueueOp::SkipUnavailable { key: k2.clone() }, &d, &e);
    assert_eq!(
        offline_skipped(&d).into_iter().collect::<Vec<_>>(),
        vec![k1]
    );
    let d = doc_of(QueueOp::ClearOfflineSkips, &d, &e);
    assert!(!d.upcoming[0].unavailable);
    assert!(d.upcoming[1].unavailable);
    assert!(offline_skipped(&d).is_empty());
}

// -- queueing -----------------------------------------------------------------

#[test]
fn play_next_goes_directly_after_current_and_starts_the_group() {
    let e = DeterministicEntropy::new(1);
    let d = play(&fresh(), &e, album("a", 3), Some(0), false);
    let d = doc_of(play_next(&["x", "y"], vec![], true), &d, &e);
    assert_eq!(upcoming(&d), ids(&["x", "y", "a1", "a2"]));
    assert_eq!(group(&d), ids(&["x", "y"]));
    assert!(d.upcoming[0].added && d.upcoming[0].queued);
    // Ahead of the existing group.
    let d = doc_of(play_next(&["z"], vec![], true), &d, &e);
    assert_eq!(group(&d), ids(&["z", "x", "y"]));
    let v = derive(&d);
    assert_eq!(v.group_count, 3);
    assert!(v.entries[1].1 && v.entries[3].1 && !v.entries[4].1);
}

#[test]
fn play_next_with_the_section_off_is_added_but_not_queued() {
    let e = DeterministicEntropy::new(1);
    let d = play(&fresh(), &e, album("a", 3), Some(0), false);
    let d = doc_of(play_next(&["x"], vec![], false), &d, &e);
    assert_eq!(upcoming(&d), ids(&["x", "a1", "a2"]));
    assert!(d.upcoming[0].added);
    assert!(!d.upcoming[0].queued);
    assert!(group(&d).is_empty());
    // With a group already there it joins it, so the group stays attached.
    let d = doc_of(add_to_queue(&["g"], vec![]), &d, &e);
    assert_eq!(upcoming(&d), ids(&["g", "x", "a1", "a2"]));
    let d = doc_of(play_next(&["y"], vec![], false), &d, &e);
    assert_eq!(group(&d), ids(&["y", "g"]));
}

#[test]
fn add_to_queue_goes_to_the_end_of_the_group() {
    let e = DeterministicEntropy::new(1);
    let d = play(&fresh(), &e, album("a", 3), Some(0), false);
    let d = doc_of(add_to_queue(&["x"], vec![]), &d, &e);
    let d = doc_of(add_to_queue(&["y"], vec![]), &d, &e);
    let d = doc_of(play_next(&["n"], vec![], true), &d, &e);
    assert_eq!(upcoming(&d), ids(&["n", "x", "y", "a1", "a2"]));
    assert_eq!(group(&d), ids(&["n", "x", "y"]));
    // Played through, the group is empty; the next one starts a new group
    // right after the current song.
    let mut p = d.clone();
    for _ in 0..4 {
        p = doc_of(QueueOp::Next, &p, &e);
    }
    assert_eq!(cur(&p), "a1");
    assert!(group(&p).is_empty());
    let p = doc_of(add_to_queue(&["z"], vec![]), &p, &e);
    assert_eq!(upcoming(&p), ids(&["z", "a2"]));
    assert_eq!(group(&p), ids(&["z"]));
}

#[test]
fn play_later_goes_to_the_end_of_the_list() {
    let e = DeterministicEntropy::new(1);
    let d = play(&fresh(), &e, album("a", 3), Some(0), false);
    let d = doc_of(add_to_queue(&["g"], vec![]), &d, &e);
    let d = doc_of(play_later(&["l"], vec![]), &d, &e);
    assert_eq!(upcoming(&d), ids(&["g", "a1", "a2", "l"]));
    let l = d.upcoming.last().unwrap();
    assert!(l.added && !l.queued);
}

#[test]
fn queueing_onto_an_empty_session_starts_playing() {
    let e = DeterministicEntropy::new(1);
    let (d, fx) = step(&fresh(), &e, play_later(&["x", "y"], vec![]));
    assert_eq!(cur(&d), "x");
    assert_eq!(upcoming(&d), ids(&["y"]));
    assert!(matches!(fx[0], Effect::CurrentChanged { .. }));
    assert_eq!(d.context.as_ref().unwrap().label, ADHOC_QUEUE_LABEL);
}

#[test]
fn played_queued_entry_comes_back_to_the_head_of_the_group() {
    let e = DeterministicEntropy::new(1);
    let d = play(&fresh(), &e, album("a", 3), Some(0), false);
    let d = doc_of(add_to_queue(&["x", "y"], vec![]), &d, &e);
    let n = doc_of(QueueOp::Next, &d, &e);
    assert_eq!(cur(&n), "x");
    assert_eq!(group(&n), ids(&["y"]));
    let p = doc_of(QueueOp::Previous, &n, &e);
    assert_eq!(group(&p), ids(&["x", "y"]));
    assert!(same_state(&p, &d), "Next then Previous is the identity");
}

#[test]
fn naming_an_entry_ahead_moves_it_and_one_behind_copies_it() {
    let e = DeterministicEntropy::new(1);
    let d = play(&fresh(), &e, album("a", 5), Some(2), false);
    // Ahead: moved, keeping its key.
    let k4 = key_of(&d, "a4");
    let m = doc_of(play_next(&[], vec![k4.clone()], true), &d, &e);
    assert_eq!(upcoming(&m), ids(&["a4", "a3"]));
    assert_eq!(m.upcoming[0].key, k4);
    assert!(m.upcoming[0].queued);
    assert!(!m.upcoming[0].added, "added never changes");
    assert_eq!(history(&m), history(&d));
    // Behind (history) and the current one: copied, history untouched.
    let k0 = key_of(&d, "a0");
    let kc = d.current.clone().unwrap().key;
    let c = doc_of(add_to_queue(&[], vec![k0.clone(), kc.clone()]), &d, &e);
    assert_eq!(history(&c), history(&d));
    assert_eq!(c.history[0].key, k0);
    assert_eq!(upcoming(&c), ids(&["a0", "a2", "a3", "a4"]));
    assert_ne!(c.upcoming[0].key, k0);
    assert_ne!(c.upcoming[1].key, kc);
    assert!(c.upcoming[0].added && c.upcoming[0].queued);
    // Play Later on an entry ahead moves it to the end, out of the group.
    let g = doc_of(add_to_queue(&["x"], vec![]), &d, &e);
    let kx = key_of(&g, "x");
    let l = doc_of(play_later(&[], vec![kx.clone()]), &g, &e);
    assert_eq!(upcoming(&l), ids(&["a3", "a4", "x"]));
    assert!(!l.upcoming[2].queued);
    // Unknown keys alone are an error.
    assert!(matches!(
        err(&d, &e, play_next(&[], vec!["nope".into()], true)),
        ReduceError::UnknownKey(_)
    ));
}

#[test]
fn autoplay_entries_append_flagged_and_not_added() {
    let e = DeterministicEntropy::new(1);
    let d = play(&fresh(), &e, album("a", 2), Some(1), false);
    let d = doc_of(
        QueueOp::AppendAutoplay {
            items: vec![AutoplayItem {
                track: "auto".into(),
                provider: AutoplayProvider::Random,
                reason: "r".into(),
                score: None,
            }],
        },
        &d,
        &e,
    );
    let last = d.upcoming.last().unwrap();
    assert_eq!(last.track, "auto");
    assert!(!last.added && !last.queued);
    let v = derive(&d).into_view(|id| TrackSummary {
        id: (id.clone()).id,
        ..Default::default()
    });
    assert!(v.entries.last().unwrap().autoplay);
    let n = doc_of(QueueOp::Next, &d, &e);
    assert_eq!(cur(&n), "auto");
}

// -- editing ------------------------------------------------------------------

fn with_group(e: &DeterministicEntropy) -> SessionDocument {
    let d = play(&fresh(), e, album("a", 4), Some(1), false);
    doc_of(add_to_queue(&["x", "y"], vec![]), &d, e)
}

#[test]
fn dragging_decides_group_membership_by_the_op() {
    let e = DeterministicEntropy::new(1);
    let d = with_group(&e);
    assert_eq!(upcoming(&d), ids(&["x", "y", "a2", "a3"]));
    let cur_key = d.current.clone().unwrap().key;
    let (kx, ky, k2, k3) = (
        key_of(&d, "x"),
        key_of(&d, "y"),
        key_of(&d, "a2"),
        key_of(&d, "a3"),
    );
    let mv = |doc: &SessionDocument, key: &QueueKey, after: &QueueKey, in_group: bool| {
        doc_of(
            QueueOp::MoveQueueItem {
                key: key.clone(),
                after: after.clone(),
                in_group,
            },
            doc,
            &e,
        )
    };
    // Drag a3 into the group, to its top.
    let m = mv(&d, &k3, &cur_key, true);
    assert_eq!(upcoming(&m), ids(&["a3", "x", "y", "a2"]));
    assert_eq!(group(&m), ids(&["a3", "x", "y"]));
    // Drag x out of the group, below a2.
    let m = mv(&d, &kx, &k2, false);
    assert_eq!(upcoming(&m), ids(&["y", "a2", "x", "a3"]));
    assert_eq!(group(&m), ids(&["y"]));
    // An in-group drop past the group's end lands at its end.
    let m = mv(&d, &k3, &k2, true);
    assert_eq!(upcoming(&m), ids(&["x", "y", "a3", "a2"]));
    assert_eq!(group(&m), ids(&["x", "y", "a3"]));
    // An out-of-group drop inside the group lands just after it.
    let m = mv(&d, &k2, &cur_key, false);
    assert_eq!(upcoming(&m), ids(&["x", "y", "a2", "a3"]));
    assert_eq!(group(&m), ids(&["x", "y"]));
    // Reorder within the group.
    let m = mv(&d, &kx, &ky, true);
    assert_eq!(group(&m), ids(&["y", "x"]));
    // Dragging every entry out removes the group.
    let m = mv(&mv(&d, &kx, &k3, false), &ky, &k3, false);
    assert!(group(&m).is_empty());
    assert_eq!(upcoming(&m), ids(&["a2", "a3", "y", "x"]));
    // Nothing above Now playing, nothing into or out of history.
    let k0 = key_of(&d, "a0");
    assert_eq!(
        err(
            &d,
            &e,
            QueueOp::MoveQueueItem {
                key: kx.clone(),
                after: k0.clone(),
                in_group: false
            }
        ),
        ReduceError::BadMove
    );
    assert_eq!(
        err(
            &d,
            &e,
            QueueOp::MoveQueueItem {
                key: k0,
                after: cur_key.clone(),
                in_group: true
            }
        ),
        ReduceError::BadMove
    );
    assert_eq!(
        err(
            &d,
            &e,
            QueueOp::MoveQueueItem {
                key: cur_key.clone(),
                after: k3,
                in_group: false
            }
        ),
        ReduceError::BadMove
    );
    assert!(matches!(
        err(
            &d,
            &e,
            QueueOp::MoveQueueItem {
                key: "nope".into(),
                after: cur_key,
                in_group: false
            }
        ),
        ReduceError::UnknownKey(_)
    ));
}

#[test]
fn insert_tracks_lands_after_the_named_entry_like_a_drop() {
    let e = DeterministicEntropy::new(1);
    let d = with_group(&e);
    // history a0, current a1, group x y, then a2 a3
    let cur_key = d.current.clone().unwrap().key;
    let (kx, ky, k2, k3) = (
        key_of(&d, "x"),
        key_of(&d, "y"),
        key_of(&d, "a2"),
        key_of(&d, "a3"),
    );
    let ins = |doc: &SessionDocument,
               tracks: &[&str],
               keys: Vec<QueueKey>,
               after: &QueueKey,
               in_group: bool| {
        doc_of(
            QueueOp::InsertTracks {
                track_ids: (ids(tracks))
                    .into_iter()
                    .map(crate::api::TrackRef::from)
                    .collect::<Vec<_>>(),
                keys,
                after: after.clone(),
                in_group,
            },
            doc,
            &e,
        )
    };
    // Into the group, between x and y.
    let m = ins(&d, &["n1", "n2"], vec![], &kx, true);
    assert_eq!(upcoming(&m), ids(&["x", "n1", "n2", "y", "a2", "a3"]));
    assert_eq!(group(&m), ids(&["x", "n1", "n2", "y"]));
    assert!(m.upcoming[1].added && m.upcoming[1].queued);
    // Out of the group, between a2 and a3.
    let m = ins(&d, &["n"], vec![], &k2, false);
    assert_eq!(upcoming(&m), ids(&["x", "y", "a2", "n", "a3"]));
    assert_eq!(group(&m), ids(&["x", "y"]));
    assert!(m.upcoming[3].added && !m.upcoming[3].queued);
    // Top of the list, in the group.
    let m = ins(&d, &["n"], vec![], &cur_key, true);
    assert_eq!(group(&m), ids(&["n", "x", "y"]));
    // An in-group drop past the group's end lands at its end.
    let m = ins(&d, &["n"], vec![], &k3, true);
    assert_eq!(upcoming(&m), ids(&["x", "y", "n", "a2", "a3"]));
    assert_eq!(group(&m), ids(&["x", "y", "n"]));
    // An out-of-group drop inside the group lands just after it.
    let m = ins(&d, &["n"], vec![], &cur_key, false);
    assert_eq!(upcoming(&m), ids(&["x", "y", "n", "a2", "a3"]));
    assert_eq!(group(&m), ids(&["x", "y"]));
    // Keys ahead of the pointer move (even past `after`), history is copied.
    let k0 = key_of(&d, "a0");
    let m = ins(&d, &[], vec![k3.clone(), k0.clone()], &kx, true);
    assert_eq!(upcoming(&m), ids(&["x", "a3", "a0", "y", "a2"]));
    assert_eq!(history(&m), ids(&["a0"]));
    assert_eq!(key_of(&m, "a3"), k3);
    assert_ne!(m.upcoming[2].key, k0);
    // Moving `after` itself keeps its place.
    let m = ins(&d, &["n"], vec![ky.clone()], &ky, false);
    assert_eq!(upcoming(&m), ids(&["x", "y", "n", "a2", "a3"]));
    assert_eq!(group(&m), ids(&["x"]));
    // Many at once (no event window involved).
    let many: Vec<String> = (0..300).map(|i| format!("m{i}")).collect();
    let many: Vec<&str> = many.iter().map(|s| s.as_str()).collect();
    let m = ins(&d, &many, vec![], &k2, false);
    assert_eq!(m.upcoming.len(), 4 + 300);
    assert_eq!(m.upcoming[3].track, "m0");
    assert_eq!(m.upcoming[302].track, "m299");
    assert_eq!(m.upcoming[303].track, "a3");
    // Shuffle on: the new entries take the ordinals after their predecessor.
    let s = doc_of(QueueOp::SetShuffle { enabled: true }, &d, &e);
    let after = s.upcoming.iter().find(|i| !i.queued).unwrap().clone();
    let m = ins(&s, &["n"], vec![], &after.key, false);
    let n = Flat::of(&m)
        .list
        .into_iter()
        .find(|i| i.track == "n")
        .unwrap();
    assert_eq!(n.ordinal, after.ordinal + 1);
    // Nothing into history; unknown keys are refused.
    let insert = |after: QueueKey| QueueOp::InsertTracks {
        track_ids: (ids(&["n"]))
            .into_iter()
            .map(crate::api::TrackRef::from)
            .collect::<Vec<_>>(),
        keys: vec![],
        after,
        in_group: false,
    };
    assert_eq!(err(&d, &e, insert(k0)), ReduceError::BadMove);
    assert!(matches!(
        err(&d, &e, insert("nope".into())),
        ReduceError::UnknownKey(_)
    ));
    // Nothing is current: nowhere to drop.
    let none = doc_of(
        QueueOp::RemoveQueueItems {
            keys: vec![cur_key.clone(), kx, ky, k2, k3],
        },
        &d,
        &e,
    );
    assert!(none.current.is_none());
    let k0 = key_of(&none, "a0");
    assert_eq!(err(&none, &e, insert(k0)), ReduceError::BadMove);
}

#[test]
fn clear_group_removes_only_the_group() {
    let e = DeterministicEntropy::new(1);
    let d = with_group(&e);
    let c = doc_of(QueueOp::ClearQueueGroup, &d, &e);
    assert_eq!(upcoming(&c), ids(&["a2", "a3"]));
    assert_eq!(history(&c), history(&d));
    assert_eq!(c.current, d.current);
    // Nothing to clear is a no-op (no revision bump).
    let again = doc_of(QueueOp::ClearQueueGroup, &c, &e);
    assert_eq!(again.revision, c.revision);
}

#[test]
fn remove_entries_anywhere_and_the_current_one_moves_on() {
    let e = DeterministicEntropy::new(1);
    let d = with_group(&e);
    let k0 = key_of(&d, "a0");
    let ky = key_of(&d, "y");
    let r = doc_of(QueueOp::RemoveQueueItems { keys: vec![k0, ky] }, &d, &e);
    assert!(history(&r).is_empty());
    assert_eq!(upcoming(&r), ids(&["x", "a2", "a3"]));
    let kc = r.current.clone().unwrap().key;
    let (r2, fx) = step(&r, &e, QueueOp::RemoveQueueItems { keys: vec![kc] });
    assert_eq!(cur(&r2), "x");
    assert!(matches!(fx[0], Effect::CurrentChanged { .. }));
    // The last one going leaves nothing current and stops.
    let last = play(&fresh(), &e, album("b", 2), Some(1), false);
    let kc = last.current.clone().unwrap().key;
    let (gone, fx) = step(&last, &e, QueueOp::RemoveQueueItems { keys: vec![kc] });
    assert!(gone.current.is_none());
    assert_eq!(history(&gone), ids(&["b0"]));
    assert!(fx.contains(&Effect::Stopped));
    let back = doc_of(QueueOp::Previous, &gone, &e);
    assert_eq!(cur(&back), "b0");
    assert!(matches!(
        err(
            &d,
            &e,
            QueueOp::RemoveQueueItems {
                keys: vec!["nope".into()]
            }
        ),
        ReduceError::UnknownKey(_)
    ));
}

#[test]
fn clear_queue_keeps_the_playing_track() {
    let e = DeterministicEntropy::new(1);
    let d = with_group(&e);
    let (c, fx) = step(&d, &e, QueueOp::ClearQueue);
    assert_eq!(list(&c), ids(&["a1"]));
    assert_eq!(
        c.current.as_ref().unwrap().key,
        d.current.as_ref().unwrap().key
    );
    assert!(fx.contains(&Effect::ContextReplaced));
    assert_eq!(c.saved_queues.len(), 1, "the outgoing queue was saved");
}

// -- shuffle ------------------------------------------------------------------

#[test]
fn shuffle_reorders_only_after_the_group_and_off_restores() {
    let e = DeterministicEntropy::new(7);
    let d = play(&fresh(), &e, album("a", 12), Some(3), false);
    let d = doc_of(add_to_queue(&["x", "y"], vec![]), &d, &e);
    let s = doc_of(QueueOp::SetShuffle { enabled: true }, &d, &e);
    assert!(s.shuffle);
    assert_eq!(history(&s), history(&d));
    assert_eq!(s.current, d.current);
    assert_eq!(group(&s), ids(&["x", "y"]));
    assert_ne!(upcoming(&s), upcoming(&d), "the tail is shuffled");
    let mut a = upcoming(&s);
    let mut b = upcoming(&d);
    a.sort();
    b.sort();
    assert_eq!(a, b);
    let o = doc_of(QueueOp::SetShuffle { enabled: false }, &s, &e);
    assert_eq!(Flat::of(&o).list, Flat::of(&d).list);
}

#[test]
fn shuffle_off_puts_added_entries_back_where_they_were_added() {
    let e = DeterministicEntropy::new(3);
    let d = play(&fresh(), &e, album("a", 6), Some(0), true);
    // Shuffled from the start: a0 plays first, the rest in random order.
    assert_eq!(cur(&d), "a0");
    let d = doc_of(play_next(&["n"], vec![], false), &d, &e);
    let d = doc_of(play_later(&["l"], vec![]), &d, &e);
    let o = doc_of(QueueOp::SetShuffle { enabled: false }, &d, &e);
    assert_eq!(upcoming(&o), ids(&["n", "a1", "a2", "a3", "a4", "a5", "l"]));
    // Off from the middle: continues after the current song, then the
    // ones before it.
    let d = play(&fresh(), &e, album("b", 5), Some(2), true);
    let o = doc_of(QueueOp::SetShuffle { enabled: false }, &d, &e);
    assert_eq!(list(&o), ids(&["b2", "b3", "b4", "b0", "b1"]));
}

#[test]
fn reshuffle_keeps_history_group_and_current() {
    let e = DeterministicEntropy::new(5);
    let d = play(&fresh(), &e, album("a", 10), Some(0), true);
    let d = doc_of(QueueOp::Next, &d, &e);
    let d = doc_of(add_to_queue(&["g"], vec![]), &d, &e);
    let r = doc_of(QueueOp::Reshuffle, &d, &e);
    assert_eq!(history(&r), history(&d));
    assert_eq!(r.current, d.current);
    assert_eq!(group(&r), ids(&["g"]));
}

// -- replacing and saved queues ----------------------------------------------

#[test]
fn a_new_context_replaces_everything_and_saves_the_old_one() {
    let e = DeterministicEntropy::new(1);
    let d = play(&fresh(), &e, album("a", 4), Some(0), false);
    let d = doc_of(QueueOp::Next, &d, &e);
    let d = doc_of(add_to_queue(&["x"], vec![]), &d, &e);
    let outgoing = Flat::of(&d).list;
    let b = play(&d, &e, album("b", 3), Some(0), false);
    assert_eq!(list(&b), ids(&["b0", "b1", "b2"]));
    assert!(group(&b).is_empty());
    assert_eq!(b.saved_queues.len(), 1);
    let sq = &b.saved_queues[0];
    assert_eq!(sq.label, "Album a");
    assert_eq!(sq.history.len(), 1);
    assert_eq!(sq.current.as_ref().map(|c| c.track.id.as_str()), Some("a1"));
    assert!(
        sq.context.tracks.is_empty(),
        "ID-referenced: tracks re-resolve"
    );
    // Restoring brings the list back with its group, position and keys.
    let (r, fx) = step(
        &b,
        &e,
        QueueOp::RestoreSavedQueue {
            id: sq.id.clone(),
            tracks: None,
        },
    );
    assert_eq!(Flat::of(&r).list, outgoing);
    assert!(fx.contains(&Effect::PositionRestore { position_ms: 5000 }));
    assert!(!fx
        .iter()
        .any(|f| matches!(f, Effect::ResolveContext { .. })));
    // ... and saved the album it left.
    assert_eq!(r.saved_queues.len(), 2);
}

#[test]
fn a_long_queue_is_saved_by_reference_and_rebuilt_around_its_song() {
    let e = DeterministicEntropy::new(1);
    let n = SNAPSHOT_MAX_ENTRIES + 5;
    let d = play(&fresh(), &e, album("a", n), Some(7), false);
    let b = play(&d, &e, album("b", 2), Some(0), false);
    let sq = b.saved_queues[0].clone();
    assert!(saved::needs_resolution(&sq));
    let (r, fx) = step(
        &b,
        &e,
        QueueOp::RestoreSavedQueue {
            id: sq.id.clone(),
            tracks: None,
        },
    );
    assert!(fx
        .iter()
        .any(|f| matches!(f, Effect::ResolveContext { .. })));
    assert_eq!(cur(&r), "a7");
    let tracks = album("a", n).tracks;
    let r = doc_of(QueueOp::SetContextTracks { tracks }, &r, &e);
    assert_eq!(cur(&r), "a7");
    assert_eq!(r.current.as_ref().unwrap().key, sq.current.unwrap().key);
    assert_eq!(r.history.len(), 7);
    assert_eq!(r.upcoming.len(), n - 8);
}

#[test]
fn saved_queue_management_ops() {
    let e = DeterministicEntropy::new(1);
    let d = play(&fresh(), &e, album("a", 3), Some(0), false);
    let d = doc_of(QueueOp::SaveCurrentQueue { pinned: true }, &d, &e);
    assert!(d.saved_queues[0].pinned);
    let id = d.saved_queues[0].id.clone();
    let r = doc_of(
        QueueOp::RenameQueue {
            id: None,
            label: "Mine".into(),
        },
        &d,
        &e,
    );
    assert_eq!(r.context.as_ref().unwrap().label, "Mine");
    assert_eq!(r.saved_queues[0].label, "Mine");
    assert_eq!(
        err(
            &d,
            &e,
            QueueOp::RenameQueue {
                id: None,
                label: " ".into()
            }
        ),
        ReduceError::EmptyName
    );
    let u = doc_of(
        QueueOp::PinSavedQueue {
            id: id.clone(),
            pinned: false,
        },
        &d,
        &e,
    );
    assert!(!u.saved_queues[0].pinned);
    let x = doc_of(QueueOp::DeleteSavedQueue { id: id.clone() }, &d, &e);
    assert!(x.saved_queues.is_empty());
    assert_eq!(
        err(&x, &e, QueueOp::DeleteSavedQueue { id: id.clone() }),
        ReduceError::UnknownSavedQueue(id)
    );
}

// -- limits and the view -------------------------------------------------------

#[test]
fn history_is_trimmed_from_the_oldest_end() {
    assert_eq!(HISTORY_CAP, 5000);
    let e = DeterministicEntropy::new(1);
    let n = HISTORY_CAP + 20;
    let mut d = play(&fresh(), &e, album("a", n), Some(HISTORY_CAP as u32), false);
    assert_eq!(d.history.len(), HISTORY_CAP);
    assert_eq!(d.history[0].track, "a0");
    for _ in 0..3 {
        d = doc_of(QueueOp::Next, &d, &e);
    }
    assert_eq!(cur(&d), format!("a{}", HISTORY_CAP + 3));
    assert_eq!(d.history.len(), HISTORY_CAP);
    assert_eq!(d.history[0].track, "a3");
    // A jump far ahead trims too.
    let k = key_of(&d, &format!("a{}", n - 2));
    d = doc_of(QueueOp::JumpToQueueItem { key: k }, &d, &e);
    assert_eq!(d.history.len(), HISTORY_CAP);
    assert_eq!(d.history[0].track, "a18");
}

#[test]
fn the_list_is_capped_history_first_then_the_far_end() {
    let e = DeterministicEntropy::new(1);
    // A context longer than the cap plays a window around the chosen track.
    let n = MAX_QUEUE_LEN + 3000;
    let d = play(&fresh(), &e, album("a", n), Some(8000), false);
    assert_eq!(d.history.len() + 1 + d.upcoming.len(), MAX_QUEUE_LEN);
    assert_eq!(d.history.len(), HISTORY_CAP);
    assert_eq!(cur(&d), "a8000");
    assert_eq!(d.context.as_ref().unwrap().tracks.len(), MAX_QUEUE_LEN);
    assert_eq!(d.context.as_ref().unwrap().tracks[0], "a3000");
    // Near the end the window ends with the list.
    let d = play(&fresh(), &e, album("a", n), Some(n as u32 - 1), false);
    assert_eq!(d.history.len(), HISTORY_CAP);
    assert_eq!(cur(&d), format!("a{}", n - 1));
    assert!(d.upcoming.is_empty());
    // Queueing past the cap drops history first...
    let d = play(&fresh(), &e, album("a", 7000), Some(4000), false);
    let more: Vec<String> = (0..4000).map(|i| format!("x{i}")).collect();
    let more: Vec<&str> = more.iter().map(|s| s.as_str()).collect();
    let d = doc_of(play_later(&more, vec![]), &d, &e);
    assert_eq!(d.history.len() + 1 + d.upcoming.len(), MAX_QUEUE_LEN);
    assert_eq!(d.history.len(), 3000);
    assert_eq!(d.history[0].track, "a1000");
    assert_eq!(d.upcoming.last().unwrap().track, "x3999");
    // ...then cuts the far end once there is none left.
    let d = play(&fresh(), &e, album("a", 7000), Some(0), false);
    let d = doc_of(play_next(&more, vec![], false), &d, &e);
    assert_eq!(d.history.len() + 1 + d.upcoming.len(), MAX_QUEUE_LEN);
    assert_eq!(cur(&d), "a0");
    assert_eq!(d.upcoming[0].track, "x0");
    assert_eq!(d.upcoming.last().unwrap().track, "a5999");
}

#[test]
fn the_view_is_windowed_both_ways_with_exact_totals() {
    let e = DeterministicEntropy::new(1);
    let d = play(&fresh(), &e, album("a", 1000), Some(500), false);
    let d = doc_of(add_to_queue(&["x", "y"], vec![]), &d, &e);
    let w = derive_window(&d, QUEUE_VIEW_BEFORE, QUEUE_VIEW_AFTER);
    assert_eq!(w.total, 1002);
    assert_eq!(w.history_count, 500);
    assert_eq!(w.current_index, Some(500));
    assert_eq!(w.group_count, 2);
    assert_eq!(w.total_upcoming, 501);
    assert_eq!(w.window_start, 400);
    assert_eq!(w.entries.len(), 100 + 1 + 100);
    assert_eq!(w.entries[0].0.track, "a400");
    assert_eq!(w.entries[100].0.track, "a500");
    assert!(w.entries[101].1 && w.entries[102].1 && !w.entries[103].1);
    assert_eq!(w.history().count(), 100);
    assert_eq!(w.upcoming().count(), 100);
    // Paging further back and further ahead.
    let back = derive_range(&d, 0, 50);
    assert_eq!(back.window_start, 0);
    assert_eq!(back.entries.len(), 50);
    assert_eq!(back.total, 1002);
    let ahead = derive_range(&d, 990, 50);
    assert_eq!(ahead.entries.len(), 12, "clamped to the end");
    assert_eq!(ahead.entries.last().unwrap().0.track, "a999");
    // The full derive is the whole list.
    let full = derive(&d);
    assert_eq!(full.entries.len(), 1002);
    // Resolved into the wire shape.
    let v = w.into_view(|id| TrackSummary {
        id: id.id.clone(),
        title: id.id.to_uppercase(),
        ..Default::default()
    });
    assert_eq!(v.current.as_ref().unwrap().track.title, "A500");
    assert_eq!(v.group().count(), 2);
    assert_eq!(v.upcoming().count(), 100);
    assert_eq!(v.history().count(), 100);
    let json = serde_json::to_string(&v).unwrap();
    assert!(json.len() < 200_000, "bounded: {}", json.len());
    let _: QueueEntry = v.entries[0].clone();
}

#[test]
fn empty_and_pointer_past_the_end_views() {
    let v = derive(&fresh());
    assert_eq!(v.total, 0);
    assert_eq!(v.current_index, None);
    let e = DeterministicEntropy::new(1);
    let last = play(&fresh(), &e, album("b", 2), Some(1), false);
    let kc = last.current.clone().unwrap().key;
    let gone = doc_of(QueueOp::RemoveQueueItems { keys: vec![kc] }, &last, &e);
    let v = derive(&gone);
    assert_eq!((v.total, v.history_count, v.current_index), (1, 1, None));
    assert_eq!(
        v.previous.map(|i| i.track).map(|t| t.id).as_deref(),
        Some("b0")
    );
}

// -- robustness -----------------------------------------------------------------

#[test]
fn from_command_covers_queue_commands() {
    assert!(QueueOp::from_command(&Command::Next).is_some());
    assert!(QueueOp::from_command(&Command::ClearQueueGroup).is_some());
    assert!(matches!(
        QueueOp::from_command(&Command::PlayNext {
            server_id: "s".into(),
            track_ids: vec![],
            keys: vec![],
        }),
        Some(QueueOp::PlayNext { queued: true, .. })
    ));
    assert!(QueueOp::from_command(&Command::SetRating {
        targets: vec![RatingTarget::Track { id: "t".into() }],
        rating: 3,
    })
    .is_none());
}

#[test]
fn odd_documents_do_not_panic() {
    let e = DeterministicEntropy::new(1);
    let mut d = play(&fresh(), &e, album("a", 4), Some(1), false);
    // Duplicate keys, and entries after the pointer with nothing current.
    let dup = d.upcoming[0].key.clone();
    d.upcoming[1].key = dup.clone();
    d.history[0].key = dup;
    let mut odd = d.clone();
    let c = odd.current.take().unwrap();
    odd.upcoming.insert(0, c);
    let ops = vec![
        QueueOp::Next,
        QueueOp::Previous,
        QueueOp::ClearQueueGroup,
        QueueOp::SetShuffle { enabled: true },
        QueueOp::SetShuffle { enabled: false },
        QueueOp::TrackEnded,
        play_next(&["n"], vec![], true),
        add_to_queue(&["g"], vec![]),
        play_later(&["l"], vec![]),
        QueueOp::ClearQueue,
        QueueOp::SetContextTracks { tracks: vec![] },
    ];
    for doc in [&d, &odd] {
        for op in ops.clone() {
            if let Ok((out, _)) = reduce(doc, op, &ctx(&e, 1.0, 0)) {
                validate(&out).unwrap();
            }
        }
    }
}
