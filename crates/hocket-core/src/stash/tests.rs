use super::*;
use proptest::prelude::*;

fn track(id: &str) -> Track {
    Track {
        id: id.into(),
        server_id: "s".into(),
        title: format!("Song {id}"),
        ..Track::default()
    }
}

fn tracks(ids: &[&str]) -> Vec<Track> {
    ids.iter().map(|i| track(i)).collect()
}

fn listed(e: &[StashEntry]) -> Vec<String> {
    track_ids(e)
}

#[test]
fn add_appends_and_skips_duplicates() {
    let mut s = vec![];
    assert_eq!(
        add(&mut s, &tracks(&["a", "b"]), &"d1".into(), 10.0),
        (2, 0)
    );
    assert_eq!(
        add(&mut s, &tracks(&["b", "c", "c"]), &"d1".into(), 20.0),
        (1, 0)
    );
    assert_eq!(listed(&s), ["a", "b", "c"]);
}

#[test]
fn add_stops_at_the_cap() {
    let mut s = vec![];
    let many: Vec<Track> = (0..MAX_ENTRIES + 3)
        .map(|i| track(&i.to_string()))
        .collect();
    assert_eq!(add(&mut s, &many, &"d".into(), 1.0), (MAX_ENTRIES, 3));
}

#[test]
fn remove_clear_and_restore() {
    let mut s = vec![];
    add(&mut s, &tracks(&["a", "b", "c"]), &"d".into(), 1.0);
    let b = live(&s)[1].entry_id.clone();
    let removed = set_removed(&mut s, std::slice::from_ref(&b), true, 2.0);
    assert_eq!(removed, vec![b]);
    assert_eq!(listed(&s), ["a", "c"]);
    let cleared = clear(&mut s, 3.0);
    assert_eq!(cleared.len(), 2);
    assert!(listed(&s).is_empty());
    set_removed(&mut s, &cleared, false, 4.0);
    assert_eq!(listed(&s), ["a", "c"]);
    // A removed song can be stashed again.
    assert_eq!(add(&mut s, &tracks(&["b"]), &"d".into(), 5.0), (1, 0));
    assert_eq!(listed(&s), ["a", "c", "b"]);
}

#[test]
fn move_to_reorders_one_entry() {
    let mut s = vec![];
    add(&mut s, &tracks(&["a", "b", "c", "d"]), &"d".into(), 1.0);
    let d = live(&s)[3].entry_id.clone();
    assert!(move_to(&mut s, &d, 0, 2.0));
    assert_eq!(listed(&s), ["d", "a", "b", "c"]);
    let a = live(&s)[1].entry_id.clone();
    assert!(move_to(&mut s, &a, 9, 3.0));
    assert_eq!(listed(&s), ["d", "b", "c", "a"]);
    let c = live(&s)[2].entry_id.clone();
    assert!(move_to(&mut s, &c, 1, 4.0));
    assert_eq!(listed(&s), ["d", "c", "b", "a"]);
    assert!(!move_to(&mut s, &c, 1, 5.0));
}

#[test]
fn many_moves_into_one_gap_renumber() {
    let mut s = vec![];
    add(&mut s, &tracks(&["a", "b", "c"]), &"d".into(), 1.0);
    // Ping-pong the last entry into the first gap until keys collide.
    for i in 0..80 {
        let last = live(&s)[2].entry_id.clone();
        assert!(move_to(&mut s, &last, 1, 2.0 + i as f64));
    }
    assert_eq!(live(&s).len(), 3);
    let keys: Vec<f64> = live(&s).iter().map(|e| e.sort_key).collect();
    assert!(keys.windows(2).all(|w| w[0] < w[1]), "{keys:?}");
}

#[test]
fn concurrent_adds_on_two_devices_both_survive() {
    let mut one = vec![];
    let mut two = vec![];
    add(&mut one, &tracks(&["a"]), &"d1".into(), 1.0);
    add(&mut two, &tracks(&["b"]), &"d2".into(), 1.0);
    let (m1, _) = merge(&one, &two);
    let (m2, _) = merge(&two, &one);
    assert_eq!(listed(&m1), listed(&m2));
    assert_eq!(listed(&m1).len(), 2);
}

#[test]
fn the_same_song_stashed_twice_lists_once() {
    let mut one = vec![];
    let mut two = vec![];
    add(&mut one, &tracks(&["a"]), &"d1".into(), 1.0);
    add(&mut two, &tracks(&["a"]), &"d2".into(), 1.0);
    let (m, _) = merge(&one, &two);
    assert_eq!(listed(&m), ["a"]);
    // Removing the listed one removes the other copy too, as does clearing.
    let first = live(&m)[0].entry_id.clone();
    let mut m2 = m.clone();
    set_removed(&mut m2, &[first], true, 2.0);
    assert!(listed(&m2).is_empty());
    let mut m3 = m.clone();
    clear(&mut m3, 2.0);
    assert!(listed(&m3).is_empty());
}

#[test]
fn a_removal_beats_the_stale_copy() {
    let mut one = vec![];
    add(&mut one, &tracks(&["a"]), &"d1".into(), 1.0);
    let stale = one.clone();
    clear(&mut one, 2.0);
    let (m, changed) = merge(&one, &stale);
    assert!(!changed);
    assert!(listed(&m).is_empty());
}

#[test]
fn prune_forgets_old_tombstones_only() {
    let mut s = vec![];
    add(&mut s, &tracks(&["a", "b"]), &"d".into(), 1.0);
    let a = live(&s)[0].entry_id.clone();
    set_removed(&mut s, &[a], true, 2.0);
    prune(&mut s, 3.0);
    assert_eq!(s.len(), 2);
    prune(&mut s, 3.0 + TOMBSTONE_TTL_MS);
    assert_eq!(s.len(), 1);
}

#[test]
fn items_fall_back_to_the_snapshot() {
    let mut s = vec![];
    add(&mut s, &tracks(&["a", "b"]), &"d".into(), 1.0);
    let items = items(&s, |_| {
        vec![Track {
            title: "Fresh".into(),
            ..track("b")
        }]
    });
    assert_eq!(items[0].track.title, "Song a");
    assert_eq!(items[1].track.title, "Fresh");
}

#[test]
fn a_move_and_a_concurrent_remove_both_stand() {
    let mut one = vec![];
    add(&mut one, &tracks(&["a", "b", "c"]), &"d1".into(), 1.0);
    let mut two = one.clone();
    let c = live(&one)[2].entry_id.clone();
    // Device one removes c; device two, later and unaware, moves c to the top.
    set_removed(&mut one, std::slice::from_ref(&c), true, 10.0);
    move_to(&mut two, &c, 0, 20.0);
    let (m1, _) = merge(&one, &two);
    let (m2, _) = merge(&two, &one);
    assert_eq!(listed(&m1), ["a", "b"]);
    assert_eq!(listed(&m1), listed(&m2));
    // Restored, it comes back where it was moved to.
    let mut m = m1;
    set_removed(&mut m, &[c], false, 30.0);
    assert_eq!(listed(&m), ["c", "a", "b"]);
}

#[test]
fn removing_a_song_removes_its_hidden_duplicate_too() {
    let mut one = vec![];
    let mut two = vec![];
    add(&mut one, &tracks(&["a", "b"]), &"d1".into(), 1.0);
    add(&mut two, &tracks(&["a"]), &"d2".into(), 1.0);
    let (mut m, _) = merge(&one, &two);
    let shown = live(&m)[0].entry_id.clone();
    let removed = set_removed(&mut m, &[shown], true, 2.0);
    assert_eq!(removed.len(), 2);
    assert_eq!(listed(&m), ["b"]);
    set_removed(&mut m, &removed, false, 3.0);
    assert_eq!(listed(&m), ["a", "b"]);
}

#[test]
fn moving_a_song_drops_its_hidden_duplicate() {
    let mut one = vec![];
    let mut two = vec![];
    add(&mut one, &tracks(&["a", "b", "c"]), &"d1".into(), 1.0);
    add(&mut two, &tracks(&["a"]), &"d2".into(), 1.0);
    let (mut m, _) = merge(&one, &two);
    let a = live(&m)[0].entry_id.clone();
    assert!(move_to(&mut m, &a, 2, 2.0));
    assert_eq!(listed(&m), ["b", "c", "a"]);
}

#[test]
fn tombstones_are_capped_and_slim() {
    let mut s = vec![];
    for round in 0..3 {
        let ids: Vec<String> = (0..MAX_ENTRIES).map(|i| format!("{round}-{i}")).collect();
        let refs: Vec<&str> = ids.iter().map(|x| x.as_str()).collect();
        add(
            &mut s,
            &tracks(&refs),
            &"d".into(),
            round as f64 * 10.0 + 1.0,
        );
        clear(&mut s, round as f64 * 10.0 + 2.0);
        prune(&mut s, round as f64 * 10.0 + 3.0);
    }
    assert_eq!(s.len(), MAX_TOMBSTONES);
    assert!(s
        .iter()
        .all(|e| e.removed && e.title.is_empty() && e.track_id.starts_with('2')));
}

#[derive(Debug, Clone)]
enum Op {
    Add(u8, u8),
    Remove(u8, u8),
    Move(u8, u8, u8),
    Clear(u8),
    Sync(u8, u8),
}

fn op() -> impl Strategy<Value = Op> {
    prop_oneof![
        (0..3u8, 0..8u8).prop_map(|(d, t)| Op::Add(d, t)),
        (0..3u8, 0..8u8).prop_map(|(d, i)| Op::Remove(d, i)),
        (0..3u8, 0..8u8, 0..8u8).prop_map(|(d, i, j)| Op::Move(d, i, j)),
        (0..3u8).prop_map(Op::Clear),
        (0..3u8, 0..3u8).prop_map(|(a, b)| Op::Sync(a, b)),
    ]
}

proptest! {
    /// Three devices editing and syncing in any order agree once everyone has everyone's set,
    /// whatever order the merges happen in.
    #[test]
    fn replicas_converge(ops in proptest::collection::vec(op(), 0..60)) {
        let mut devs: Vec<Vec<StashEntry>> = vec![vec![], vec![], vec![]];
        for (step, o) in ops.into_iter().enumerate() {
            let now = step as f64 + 1.0;
            match o {
                Op::Add(d, t) => {
                    add(&mut devs[d as usize], &[track(&t.to_string())], &format!("d{d}"), now);
                }
                Op::Remove(d, i) => {
                    let ids: Vec<String> = live(&devs[d as usize])
                        .get(i as usize)
                        .map(|e| e.entry_id.clone())
                        .into_iter()
                        .collect();
                    set_removed(&mut devs[d as usize], &ids, true, now);
                }
                Op::Move(d, i, j) => {
                    if let Some(id) = live(&devs[d as usize]).get(i as usize).map(|e| e.entry_id.clone()) {
                        move_to(&mut devs[d as usize], &id, j as usize, now);
                    }
                }
                Op::Clear(d) => {
                    clear(&mut devs[d as usize], now);
                }
                Op::Sync(a, b) => {
                    let (m, _) = merge(&devs[a as usize], &devs[b as usize]);
                    devs[a as usize] = m;
                }
            }
        }
        let all = devs.iter().fold(vec![], |acc, d| merge(&acc, d).0);
        let forward = devs.iter().fold(vec![], |acc, d| merge(&acc, d).0);
        let backward = devs.iter().rev().fold(vec![], |acc, d| merge(&acc, d).0);
        prop_assert_eq!(listed(&forward), listed(&backward));
        prop_assert_eq!(listed(&all), listed(&forward));
        for d in &devs {
            let (m, _) = merge(d, &all);
            prop_assert_eq!(listed(&m), listed(&all));
        }
    }
}
