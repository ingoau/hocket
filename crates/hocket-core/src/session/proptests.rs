//! Property tests for the flat queue model (design.md "Queue model").
//!
//! States are reached by running arbitrary op sequences from a fresh
//! session, so every state checked is one the reducer can actually produce.
//! Keyed ops pick their entries by position in the state they are applied
//! to, so they always name real entries.

use std::collections::HashSet;

use proptest::prelude::*;

use super::document::{new_document, validate};
use super::reducer::{derive, reduce, AutoplayItem, Effect, Flat, QueueOp, ReduceCtx, HISTORY_CAP};
use super::DeterministicEntropy;
use crate::api::{
    AutoplayProvider, ContextKind, PlayContextArgs, QueueContext, QueueItem, RepeatMode,
    SessionDocument, SortOrder,
};

fn ctx(e: &DeterministicEntropy) -> ReduceCtx<'_> {
    ReduceCtx {
        now: 1.0,
        position_ms: 0,
        entropy: e,
    }
}

/// An op with its entries picked by position (resolved against the state).
#[derive(Debug, Clone)]
enum Pick {
    Play {
        n: usize,
        start: Option<u32>,
        shuffle: bool,
    },
    PlayNext {
        tracks: usize,
        key: Option<usize>,
        queued: bool,
    },
    AddToQueue {
        tracks: usize,
        key: Option<usize>,
    },
    PlayLater {
        tracks: usize,
        key: Option<usize>,
    },
    Jump(usize),
    Remove(usize),
    Move {
        key: usize,
        after: usize,
        in_group: bool,
    },
    ClearGroup,
    Shuffle(bool),
    Reshuffle,
    Repeat(RepeatMode),
    Autoplay(bool),
    Next,
    Previous,
    TrackEnded,
    Skip(usize),
    AppendAutoplay(usize),
    ClearQueue,
    Restore(usize),
}

fn repeat_mode() -> impl Strategy<Value = RepeatMode> {
    prop_oneof![
        Just(RepeatMode::Off),
        Just(RepeatMode::All),
        Just(RepeatMode::One)
    ]
}

fn pick() -> impl Strategy<Value = Pick> {
    prop_oneof![
        1 => (0usize..14, proptest::option::of(0u32..14), any::<bool>())
            .prop_map(|(n, start, shuffle)| Pick::Play { n, start, shuffle }),
        3 => (0usize..3, proptest::option::of(any::<usize>()), any::<bool>())
            .prop_map(|(tracks, key, queued)| Pick::PlayNext { tracks, key, queued }),
        3 => (0usize..3, proptest::option::of(any::<usize>()))
            .prop_map(|(tracks, key)| Pick::AddToQueue { tracks, key }),
        2 => (0usize..3, proptest::option::of(any::<usize>()))
            .prop_map(|(tracks, key)| Pick::PlayLater { tracks, key }),
        2 => any::<usize>().prop_map(Pick::Jump),
        1 => any::<usize>().prop_map(Pick::Remove),
        3 => (any::<usize>(), any::<usize>(), any::<bool>())
            .prop_map(|(key, after, in_group)| Pick::Move { key, after, in_group }),
        1 => Just(Pick::ClearGroup),
        2 => any::<bool>().prop_map(Pick::Shuffle),
        1 => Just(Pick::Reshuffle),
        1 => repeat_mode().prop_map(Pick::Repeat),
        1 => any::<bool>().prop_map(Pick::Autoplay),
        5 => Just(Pick::Next),
        4 => Just(Pick::Previous),
        2 => Just(Pick::TrackEnded),
        1 => any::<usize>().prop_map(Pick::Skip),
        1 => (1usize..3).prop_map(Pick::AppendAutoplay),
        1 => Just(Pick::ClearQueue),
        1 => any::<usize>().prop_map(Pick::Restore),
    ]
}

fn album(n: usize, tag: u64) -> QueueContext {
    QueueContext {
        server_id: "srv".into(),
        kind: ContextKind::Album {
            id: format!("al{}", tag % 3),
        },
        label: "Album".into(),
        sort: SortOrder::Default,
        tracks: (0..n).map(|i| format!("t{i}")).collect(),
    }
}

fn key_at(doc: &SessionDocument, i: usize) -> Option<String> {
    let f = Flat::of(doc);
    (!f.is_empty()).then(|| f.list[i % f.len()].key.clone())
}

fn new_tracks(n: usize, salt: usize) -> Vec<String> {
    (0..n).map(|i| format!("q{salt}-{i}")).collect()
}

/// The reducer op a pick stands for in `doc` (`None` when it names nothing).
fn resolve(doc: &SessionDocument, p: &Pick, salt: usize) -> Option<QueueOp> {
    let keys = |k: &Option<usize>| k.and_then(|i| key_at(doc, i)).into_iter().collect();
    Some(match p {
        Pick::Play { n, start, shuffle } => QueueOp::PlayContext {
            args: PlayContextArgs {
                context: album(*n, salt as u64),
                start_index: *start,
                shuffle: *shuffle,
                save_outgoing: true,
                start_position_ms: None,
            },
        },
        Pick::PlayNext {
            tracks,
            key,
            queued,
        } => QueueOp::PlayNext {
            server_id: "srv".into(),
            track_ids: new_tracks(*tracks, salt),
            keys: keys(key),
            queued: *queued,
        },
        Pick::AddToQueue { tracks, key } => QueueOp::AddToQueue {
            server_id: "srv".into(),
            track_ids: new_tracks(*tracks, salt),
            keys: keys(key),
        },
        Pick::PlayLater { tracks, key } => QueueOp::PlayLater {
            server_id: "srv".into(),
            track_ids: new_tracks(*tracks, salt),
            keys: keys(key),
        },
        Pick::Jump(i) => QueueOp::JumpToQueueItem {
            key: key_at(doc, *i)?,
        },
        Pick::Remove(i) => QueueOp::RemoveQueueItems {
            keys: vec![key_at(doc, *i)?],
        },
        Pick::Move {
            key,
            after,
            in_group,
        } => QueueOp::MoveQueueItem {
            key: key_at(doc, *key)?,
            after: key_at(doc, *after)?,
            in_group: *in_group,
        },
        Pick::ClearGroup => QueueOp::ClearQueueGroup,
        Pick::Shuffle(on) => QueueOp::SetShuffle { enabled: *on },
        Pick::Reshuffle => QueueOp::Reshuffle,
        Pick::Repeat(mode) => QueueOp::SetRepeat { mode: *mode },
        Pick::Autoplay(on) => QueueOp::SetAutoplay { enabled: *on },
        Pick::Next => QueueOp::Next,
        Pick::Previous => QueueOp::Previous,
        Pick::TrackEnded => QueueOp::TrackEnded,
        Pick::Skip(i) => QueueOp::SkipUnavailable {
            key: key_at(doc, *i)?,
        },
        Pick::AppendAutoplay(n) => QueueOp::AppendAutoplay {
            items: (0..*n)
                .map(|i| AutoplayItem {
                    track_id: format!("auto{salt}-{i}"),
                    provider: AutoplayProvider::Random,
                    reason: "r".into(),
                    score: None,
                })
                .collect(),
        },
        Pick::ClearQueue => QueueOp::ClearQueue,
        Pick::Restore(i) => {
            if doc.saved_queues.is_empty() {
                return None;
            }
            let q = &doc.saved_queues[i % doc.saved_queues.len()];
            QueueOp::RestoreSavedQueue {
                id: q.id.clone(),
                tracks: None,
            }
        }
    })
}

/// Run picks from a fresh session that starts with an album.
fn reach(start: usize, picks: &[Pick], e: &DeterministicEntropy) -> SessionDocument {
    let mut doc = new_document("srv:u", "s".into(), 0.0);
    let first = Pick::Play {
        n: start,
        start: Some(0),
        shuffle: false,
    };
    for (salt, p) in std::iter::once(&first).chain(picks.iter()).enumerate() {
        if let Some(op) = resolve(&doc, p, salt) {
            if let Ok((next, _)) = reduce(&doc, op, &ctx(e)) {
                check_invariants(&next);
                doc = next;
            }
        }
    }
    doc
}

fn group_len(doc: &SessionDocument) -> usize {
    if doc.current.is_none() {
        return 0;
    }
    doc.upcoming.iter().take_while(|i| i.queued).count()
}

fn keys(list: &[QueueItem]) -> Vec<String> {
    list.iter().map(|i| i.key.clone()).collect()
}

fn check_invariants(doc: &SessionDocument) {
    validate(doc).expect("valid document");
    if doc.current.is_none() {
        assert!(doc.upcoming.is_empty(), "nothing after the pointer");
    }
    assert!(doc.history.len() <= HISTORY_CAP);
    let f = Flat::of(doc);
    let mut seen = HashSet::new();
    for i in &f.list {
        assert!(seen.insert(i.key.clone()), "unique keys");
    }
    let mut ords = HashSet::new();
    for (idx, i) in f.list.iter().enumerate() {
        assert!(ords.insert(i.ordinal), "unique ordinals");
        if !doc.shuffle {
            assert_eq!(i.ordinal as usize, idx, "ordinal is the index unshuffled");
        }
    }
    // The view agrees with the document.
    let v = derive(doc);
    assert_eq!(v.total as usize, f.len());
    assert_eq!(v.group_count as usize, group_len(doc));
    assert_eq!(v.entries.iter().filter(|(_, g)| *g).count(), group_len(doc));
}

/// Number of playable entries in `range` of the flat list.
fn playable(f: &Flat, range: std::ops::Range<usize>) -> usize {
    f.list[range].iter().filter(|i| !i.unavailable).count()
}

fn apply(
    doc: &SessionDocument,
    op: QueueOp,
    e: &DeterministicEntropy,
) -> (SessionDocument, Vec<Effect>) {
    let (next, fx) = reduce(doc, op, &ctx(e)).expect("applies");
    check_invariants(&next);
    (next, fx)
}

fn with_repeat(
    doc: &SessionDocument,
    mode: RepeatMode,
    e: &DeterministicEntropy,
) -> SessionDocument {
    apply(doc, QueueOp::SetRepeat { mode }, e).0
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 256, max_global_rejects: 8192, ..ProptestConfig::default() })]

    /// Every reachable state is valid (checked after every op in `reach`).
    #[test]
    fn reachable_states_hold_the_invariants(
        n in 0usize..12,
        picks in proptest::collection::vec(pick(), 0..40),
    ) {
        let e = DeterministicEntropy::new(1);
        reach(n, &picks, &e);
    }

    /// Next^k then Previous^k lands on the same entry, unless an end of the
    /// list (or a wrap) was hit on the way.
    #[test]
    fn next_k_then_previous_k_is_the_identity(
        n in 1usize..12,
        picks in proptest::collection::vec(pick(), 0..30),
        k in 1usize..8,
    ) {
        let e = DeterministicEntropy::new(2);
        let doc = with_repeat(&reach(n, &picks, &e), RepeatMode::Off, &e);
        prop_assume!(doc.current.as_ref().is_some_and(|c| !c.unavailable));
        let before = Flat::of(&doc);
        let mut x = doc.clone();
        for _ in 0..k {
            let ptr = x.history.len();
            let (next, _) = apply(&x, QueueOp::Next, &e);
            prop_assume!(next.history.len() > ptr);
            x = next;
        }
        for _ in 0..k {
            x = apply(&x, QueueOp::Previous, &e).0;
        }
        prop_assert_eq!(&x.current, &doc.current);
        prop_assert_eq!(Flat::of(&x), before);
    }

    /// Jumping to an entry ahead, then Previous once per playable row
    /// jumped over, returns to where you were.
    #[test]
    fn jump_ahead_then_previous_returns(
        n in 2usize..12,
        picks in proptest::collection::vec(pick(), 0..30),
        target in any::<usize>(),
    ) {
        let e = DeterministicEntropy::new(3);
        let doc = with_repeat(&reach(n, &picks, &e), RepeatMode::Off, &e);
        prop_assume!(doc.current.as_ref().is_some_and(|c| !c.unavailable));
        let f = Flat::of(&doc);
        prop_assume!(f.ptr + 1 < f.len());
        let t = f.ptr + 1 + target % (f.len() - f.ptr - 1);
        let key = f.list[t].key.clone();
        let (j, _) = apply(&doc, QueueOp::JumpToQueueItem { key }, &e);
        let steps = playable(&f, f.ptr..t);
        let mut x = j;
        for _ in 0..steps {
            x = apply(&x, QueueOp::Previous, &e).0;
        }
        prop_assert_eq!(
            x.current.as_ref().map(|c| &c.key),
            doc.current.as_ref().map(|c| &c.key)
        );
    }

    /// Navigation never adds, removes or reorders entries.
    #[test]
    fn navigation_never_edits_the_list(
        n in 0usize..12,
        picks in proptest::collection::vec(pick(), 0..30),
        nav in proptest::collection::vec(
            prop_oneof![Just(0u8), Just(1), Just(2), Just(3)].prop_flat_map(|kind| (Just(kind), any::<usize>())),
            1..12,
        ),
    ) {
        let e = DeterministicEntropy::new(4);
        let doc = reach(n, &picks, &e);
        let before = Flat::of(&doc).list;
        let mut x = doc;
        for (kind, i) in nav {
            let op = match kind {
                0 => QueueOp::Next,
                1 => QueueOp::Previous,
                2 => QueueOp::TrackEnded,
                _ => match key_at(&x, i) {
                    Some(key) => QueueOp::JumpToQueueItem { key },
                    None => QueueOp::Next,
                },
            };
            x = apply(&x, op, &e).0;
        }
        let after = Flat::of(&x).list;
        // A jump clears the target's unavailable mark; nothing else changes.
        let strip = |l: &[QueueItem]| -> Vec<QueueItem> {
            l.iter().cloned().map(|mut i| { i.unavailable = false; i }).collect()
        };
        prop_assert_eq!(keys(&after), keys(&before));
        prop_assert_eq!(strip(&after), strip(&before));
    }

    /// Under Repeat All, Next^k then Previous^k always comes back, wraps
    /// included: the two wrap symmetrically.
    #[test]
    fn repeat_all_wraps_symmetrically(
        n in 1usize..8,
        picks in proptest::collection::vec(pick(), 0..30),
        k in 1usize..20,
    ) {
        let e = DeterministicEntropy::new(5);
        let doc = with_repeat(&reach(n, &picks, &e), RepeatMode::All, &e);
        prop_assume!(doc.current.as_ref().is_some_and(|c| !c.unavailable));
        let before = Flat::of(&doc);
        let mut x = doc.clone();
        for _ in 0..k {
            x = apply(&x, QueueOp::Next, &e).0;
        }
        prop_assert!(x.current.is_some());
        for _ in 0..k {
            x = apply(&x, QueueOp::Previous, &e).0;
        }
        prop_assert_eq!(&x.current, &doc.current);
        prop_assert_eq!(Flat::of(&x), before);
    }

    /// The group is what the queueing ops say it is: Add To Queue appends to
    /// it, queued Play Next prepends to it, and a move's `in_group` decides
    /// the moved entry's membership without disturbing the others.
    #[test]
    fn group_edits_keep_the_group_contiguous_after_current(
        n in 1usize..10,
        picks in proptest::collection::vec(pick(), 0..30),
        tracks in 1usize..3,
        key in any::<usize>(),
        after in any::<usize>(),
        in_group in any::<bool>(),
    ) {
        let e = DeterministicEntropy::new(6);
        let doc = reach(n, &picks, &e);
        prop_assume!(doc.current.is_some());
        let g = group_len(&doc);
        let group_keys = keys(&doc.upcoming[..g]);

        let (a, _) = apply(&doc, resolve(&doc, &Pick::AddToQueue { tracks, key: None }, 900).unwrap(), &e);
        prop_assert_eq!(group_len(&a), g + tracks);
        prop_assert_eq!(keys(&a.upcoming[..g]), group_keys.clone());
        prop_assert!(a.upcoming[g..g + tracks].iter().all(|i| i.added && i.queued));

        let (p, _) = apply(&doc, resolve(&doc, &Pick::PlayNext { tracks, key: None, queued: true }, 901).unwrap(), &e);
        prop_assert_eq!(group_len(&p), g + tracks);
        prop_assert_eq!(keys(&p.upcoming[tracks..tracks + g]), group_keys.clone());

        let (l, _) = apply(&doc, resolve(&doc, &Pick::PlayLater { tracks, key: None }, 902).unwrap(), &e);
        prop_assert_eq!(keys(&l.upcoming[..g]), group_keys.clone());
        prop_assert!(!l.upcoming.last().unwrap().queued);

        // A move of an entry ahead of the pointer to after the current one or
        // an entry ahead.
        prop_assume!(!doc.upcoming.is_empty());
        let moved = doc.upcoming[key % doc.upcoming.len()].key.clone();
        let targets: Vec<String> = doc.current.iter().chain(doc.upcoming.iter()).map(|i| i.key.clone()).collect();
        let target = targets[after % targets.len()].clone();
        let (m, _) = apply(&doc, QueueOp::MoveQueueItem { key: moved.clone(), after: target, in_group }, &e);
        let mg = keys(&m.upcoming[..group_len(&m)]);
        prop_assert_eq!(mg.contains(&moved), in_group);
        // The rest of the group stays in it, in order. (A queued run left
        // further down by navigating back can join it once the entry that
        // separated it moves away: group membership is the flag.)
        let others: Vec<String> = group_keys.iter().filter(|k| **k != moved).cloned().collect();
        let kept: Vec<String> = mg.iter().filter(|k| **k != moved).cloned().collect();
        let mut rest = kept.iter();
        prop_assert!(others.iter().all(|k| rest.any(|x| x == k)), "{:?} in {:?}", others, kept);
        prop_assert_eq!(keys(&m.history), keys(&doc.history));
        prop_assert_eq!(
            m.current.as_ref().map(|c| &c.key),
            doc.current.as_ref().map(|c| &c.key)
        );
    }

    /// Shuffle leaves history, the current entry and the group in place and
    /// only permutes what follows; turning it straight off again restores
    /// the order exactly.
    #[test]
    fn shuffle_keeps_history_and_group_and_off_restores(
        n in 0usize..14,
        picks in proptest::collection::vec(pick(), 0..30),
    ) {
        let e = DeterministicEntropy::new(7);
        let doc = reach(n, &picks, &e);
        let g = group_len(&doc);
        let permutes = |a: &SessionDocument, b: &SessionDocument| -> bool {
            let mut x = keys(&a.upcoming[g..]);
            let mut y = keys(&b.upcoming[g..]);
            x.sort();
            y.sort();
            x == y
        };
        let toggled = apply(&doc, QueueOp::SetShuffle { enabled: !doc.shuffle }, &e).0;
        // (Ordinals are renumbered when shuffle goes off; keys stay put.)
        for d in [&toggled, &apply(&doc, QueueOp::Reshuffle, &e).0] {
            prop_assert_eq!(keys(&d.history), keys(&doc.history));
            prop_assert_eq!(
                d.current.as_ref().map(|c| &c.key),
                doc.current.as_ref().map(|c| &c.key)
            );
            prop_assert_eq!(keys(&d.upcoming[..g]), keys(&doc.upcoming[..g]));
            prop_assert!(permutes(d, &doc));
        }
        if !doc.shuffle {
            let off = apply(&toggled, QueueOp::SetShuffle { enabled: false }, &e).0;
            prop_assert_eq!(Flat::of(&off), Flat::of(&doc));
        }
    }

    /// Two replicas applying the same ops with the same entropy agree.
    #[test]
    fn replicas_converge(
        n in 0usize..10,
        picks in proptest::collection::vec(pick(), 0..30),
    ) {
        let a = reach(n, &picks, &DeterministicEntropy::new(9));
        let b = reach(n, &picks, &DeterministicEntropy::new(9));
        prop_assert_eq!(a, b);
    }
}
