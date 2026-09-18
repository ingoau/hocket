//! Property tests over the reducer (design.md "How it gets tested"): the
//! reducer is total, revisions are monotonic, next∘previous is the identity,
//! keys are unique, the derived queue contains every non-history item exactly
//! once, history never exceeds its cap and the shuffle permutation is a
//! bijection.

use std::collections::HashSet;

use proptest::prelude::*;

use super::document::{new_document, same_state, validate};
use super::reducer::{context_key, derive, reduce, AutoplayItem, Effect, QueueOp, ReduceCtx};
use super::saved::SavedQueuePolicy;
use super::shuffle::{is_bijection, Permutation};
use super::DeterministicEntropy;
use crate::api::{
    AutoplayProvider, ContextKind, PlayContextArgs, QueueContext, QueueItem, QueueMode, QueueSource, RepeatMode,
    SessionDocument, ShuffleState, SortOrder,
};

const CAP: usize = 6;

fn repeat_mode() -> impl Strategy<Value = RepeatMode> {
    prop_oneof![Just(RepeatMode::Off), Just(RepeatMode::All), Just(RepeatMode::One)]
}

fn queue_mode() -> impl Strategy<Value = QueueMode> {
    prop_oneof![Just(QueueMode::Apple), Just(QueueMode::YouTube)]
}

fn non_context_source() -> impl Strategy<Value = QueueSource> {
    prop_oneof![
        2 => Just(QueueSource::Inserted),
        1 => Just(QueueSource::Autoplay { provider: AutoplayProvider::Random, reason: "r".into(), score: None }),
    ]
}

fn source(n: usize) -> BoxedStrategy<QueueSource> {
    if n == 0 {
        return non_context_source().boxed();
    }
    prop_oneof![
        3 => (0..n as u32).prop_map(|index| QueueSource::Context { index }),
        1 => non_context_source(),
    ]
    .boxed()
}

fn item(n: usize) -> impl Strategy<Value = (QueueSource, bool)> {
    (source(n), prop::bool::weighted(0.15))
}

/// Insertion-list items never carry a context source.
fn insertion_item() -> impl Strategy<Value = (QueueSource, bool)> {
    (non_context_source(), prop::bool::weighted(0.15))
}

/// A structurally consistent document: valid indices, unique keys, a cursor
/// within range, an optional shuffle state that may carry an explicit order.
fn consistent_doc() -> impl Strategy<Value = SessionDocument> {
    (0usize..7).prop_flat_map(|n| {
        let has_context = n > 0;
        (
            Just(n),
            prop::collection::vec(item(n), 0..5),
            prop::option::weighted(0.85, item(n)),
            prop::collection::vec(insertion_item(), 0..4),
            0..=(n as u32),
            prop::option::of((any::<u32>(), prop::option::of(0..n.max(1) as u32), prop::bool::ANY)),
            repeat_mode(),
            queue_mode(),
            prop::bool::ANY,
            Just(has_context),
        )
    })
    .prop_map(|(n, history, current, insertions, cursor, shuffle, repeat, mode, autoplay, has_context)| {
        let mut doc = new_document("srv:user", "sess".into(), 0.0);
        let mut key = 0u32;
        let mut mk = |(source, unavailable): (QueueSource, bool)| {
            key += 1;
            let track_id = match &source {
                QueueSource::Context { index } => format!("t{index}"),
                _ => format!("x{key}"),
            };
            QueueItem { key: format!("p{key}"), track_id, source, unavailable }
        };
        if has_context {
            doc.context = Some(QueueContext {
                server_id: "srv".into(),
                kind: ContextKind::AdHoc { label: "gen".into() },
                label: "gen".into(),
                sort: SortOrder::Default,
                tracks: (0..n).map(|i| format!("t{i}")).collect(),
            });
            doc.history = history.into_iter().map(&mut mk).collect();
            doc.current = current.map(&mut mk);
            doc.insertions = insertions.into_iter().map(&mut mk).collect();
            doc.cursor = cursor;
            doc.shuffle = shuffle.map(|(seed, anchor, explicit)| ShuffleState {
                seed,
                anchor,
                order: explicit.then(|| Permutation::seeded(seed.wrapping_add(1), n, None).into_order()),
            });
            // Reducer-generated documents keep every context item that has
            // played (current and history) before the cursor; mirror that.
            let p = Permutation::from_state(doc.shuffle.as_ref(), n);
            let played = doc.current.iter().chain(doc.history.iter()).filter_map(|i| match i.source {
                QueueSource::Context { index } => p.to_position(index),
                _ => None,
            });
            let floor = played.map(|q| q + 1).max().unwrap_or(0);
            doc.cursor = doc.cursor.max(floor).min(n as u32);
        }
        doc.repeat = repeat;
        doc.mode = mode;
        doc.autoplay = autoplay;
        doc
    })
}

fn any_key() -> impl Strategy<Value = String> {
    prop_oneof![
        (1u32..8).prop_map(|k| format!("p{k}")),
        (1u32..8).prop_map(|k| format!("k{k}")),
        (0u32..7).prop_map(context_key),
        Just("missing".to_string()),
    ]
}

fn track_ids() -> impl Strategy<Value = Vec<String>> {
    prop::collection::vec((0u32..20).prop_map(|i| format!("n{i}")), 0..3)
}

fn op() -> impl Strategy<Value = QueueOp> {
    prop_oneof![
        Just(QueueOp::Next),
        Just(QueueOp::Previous),
        Just(QueueOp::TrackEnded),
        Just(QueueOp::ClearQueue),
        Just(QueueOp::ClearInsertions),
        Just(QueueOp::Reshuffle),
        prop::bool::ANY.prop_map(|enabled| QueueOp::SetShuffle { enabled }),
        prop::bool::ANY.prop_map(|enabled| QueueOp::SetAutoplay { enabled }),
        repeat_mode().prop_map(|mode| QueueOp::SetRepeat { mode }),
        queue_mode().prop_map(|mode| QueueOp::SetQueueMode { mode }),
        any_key().prop_map(|key| QueueOp::JumpToQueueItem { key }),
        any_key().prop_map(|key| QueueOp::SkipUnavailable { key }),
        prop::collection::vec(any_key(), 1..3).prop_map(|keys| QueueOp::RemoveQueueItems { keys }),
        (any_key(), 0u32..10).prop_map(|(key, to_index)| QueueOp::MoveQueueItem { key, to_index }),
        track_ids().prop_map(|track_ids| QueueOp::PlayNext { server_id: "srv".into(), track_ids }),
        track_ids().prop_map(|track_ids| QueueOp::PlayLater { server_id: "srv".into(), track_ids }),
        (track_ids(), 0u32..4, prop::bool::ANY).prop_map(|(track_ids, start_index, shuffle)| QueueOp::PlayTracks {
            server_id: "srv".into(),
            track_ids,
            start_index,
            label: "sel".into(),
            shuffle,
            save_outgoing: true,
        }),
        (track_ids(), prop::option::of(0u32..4), prop::bool::ANY).prop_map(|(tracks, start_index, shuffle)| QueueOp::PlayContext {
            args: PlayContextArgs {
                context: QueueContext {
                    server_id: "srv".into(),
                    kind: ContextKind::Album { id: "al".into() },
                    label: "Album".into(),
                    sort: SortOrder::Default,
                    tracks,
                },
                start_index,
                shuffle,
                save_outgoing: true,
            }
        }),
        track_ids().prop_map(|ids| QueueOp::AppendAutoplay {
            items: ids
                .into_iter()
                .map(|track_id| AutoplayItem { track_id, provider: AutoplayProvider::SimilarSongs, reason: "sim".into(), score: Some(0.5) })
                .collect()
        }),
        track_ids().prop_map(|tracks| QueueOp::SetContextTracks { tracks }),
        prop::bool::ANY.prop_map(|pinned| QueueOp::SaveCurrentQueue { pinned }),
        (0u32..3).prop_map(|i| QueueOp::RestoreSavedQueue { id: format!("k{i}"), tracks: None }),
        (0u32..3).prop_map(|i| QueueOp::DeleteSavedQueue { id: format!("k{i}") }),
    ]
}

fn check_invariants(doc: &SessionDocument) {
    validate(doc).unwrap_or_else(|e| panic!("invalid document after reduce: {e}\n{doc:#?}"));
    assert!(doc.history.len() <= CAP, "history over cap");
    let d = derive(doc);
    let mut seen: HashSet<&str> = HashSet::new();
    for it in d.current.iter().chain(d.playing_next.iter()).chain(d.upcoming.iter()).chain(d.history.iter()) {
        assert!(seen.insert(it.key.as_str()), "duplicate derived key {}", it.key);
    }
    for stored in doc.current.iter().chain(doc.insertions.iter()) {
        assert!(seen.contains(stored.key.as_str()), "stored item {} missing from derived queue", stored.key);
    }
    let n = doc.context.as_ref().map(|c| c.tracks.len()).unwrap_or(0);
    let expected_upcoming = n.saturating_sub(doc.cursor as usize);
    assert_eq!(d.upcoming.len(), expected_upcoming, "every context item past the cursor is upcoming exactly once");
    let p = Permutation::from_state(doc.shuffle.as_ref(), n);
    assert!(is_bijection(p.order()));
    assert_eq!(p.len(), n);
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 400, .. ProptestConfig::default() })]

    #[test]
    fn reducer_is_total_and_keeps_invariants(doc in consistent_doc(), ops in prop::collection::vec(op(), 1..12)) {
        let entropy = DeterministicEntropy::new(7);
        let mut cur = doc;
        check_invariants(&cur);
        let mut last_rev = cur.revision;
        for (i, op) in ops.into_iter().enumerate() {
            let ctx = ReduceCtx { now: i as f64, position_ms: 0, history_cap: CAP, saved: SavedQueuePolicy::default().with_cap(2), entropy: &entropy };
            if let Ok((next, _fx)) = reduce(&cur, op.clone(), &ctx) {
                prop_assert!(next.revision >= last_rev, "revision went backwards");
                if !same_state(&cur, &next) {
                    prop_assert!(next.revision > last_rev, "changed document without a revision bump: {op:?}");
                }
                last_rev = next.revision;
                check_invariants(&next);
                cur = next;
            }
        }
    }

    #[test]
    fn next_then_previous_is_identity(doc in consistent_doc()) {
        // An unavailable current item is skipped in both directions by design,
        // so the identity only holds for a playable one.
        prop_assume!(doc.current.as_ref().map(|c| !c.unavailable).unwrap_or(false));
        // A Repeat-All wrap is undone by looking at the most recent context
        // item in history. When the current item is not a context item and
        // history holds no context item either (only possible after the user
        // removed history entries), a wrap leaves no evidence and Previous
        // lands at the top of the context instead of its end; the same track
        // plays next either way, only the upcoming list differs.
        let n = doc.context.as_ref().map(|c| c.tracks.len() as u32).unwrap_or(0);
        let is_ctx = |i: &QueueItem| matches!(i.source, QueueSource::Context { .. });
        let wrap_evidence = doc.current.as_ref().map(is_ctx).unwrap_or(false) || doc.history.iter().any(is_ctx);
        prop_assume!(doc.repeat != RepeatMode::All || doc.cursor < n || wrap_evidence);
        let entropy = DeterministicEntropy::new(3);
        let ctx = ReduceCtx { now: 1.0, position_ms: 0, history_cap: 1000, saved: SavedQueuePolicy::default(), entropy: &entropy };
        let (after_next, fx) = reduce(&doc, QueueOp::Next, &ctx).unwrap();
        let moved = fx.iter().any(|f| matches!(f, Effect::CurrentChanged { .. }));
        if !moved {
            prop_assert!(same_state(&doc, &after_next), "a stopped Next must not change the document");
            return Ok(());
        }
        let (back, _) = reduce(&after_next, QueueOp::Previous, &ctx).unwrap();
        prop_assert!(same_state(&doc, &back), "next∘previous differs\nbefore: {doc:#?}\nafter: {back:#?}");
    }

    #[test]
    fn shuffle_permutation_is_a_bijection(seed in any::<u32>(), n in 0usize..64, anchor in prop::option::of(0u32..64)) {
        let p = Permutation::seeded(seed, n, anchor);
        prop_assert!(is_bijection(p.order()));
        prop_assert_eq!(p.len(), n);
        if let Some(a) = anchor {
            if (a as usize) < n {
                prop_assert_eq!(p.to_context(0), Some(a));
            }
        }
        prop_assert_eq!(Permutation::seeded(seed, n, anchor), p);
    }
}
