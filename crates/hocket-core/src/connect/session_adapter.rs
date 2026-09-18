//! Adapts the real session reducer ([`crate::session::reducer`]) to the
//! [`SessionReducer`] seam. The one place a wire [`SessionOp`] becomes a
//! [`QueueOp`].
//!
//! Determinism: the session reducer draws queue keys and shuffle seeds from
//! an [`Entropy`] source. Here that source is seeded from the op id, so every
//! replica applying the same op materialises the same keys and the same
//! permutation, which is what lets documents converge without shipping them.

use std::sync::Arc;

use parking_lot::Mutex;
use rand::{RngCore, SeedableRng};
use rand_chacha::ChaCha8Rng;

use crate::api::{QueueKey, SessionDocument};
use crate::connect::wire::{AutoplayOpItem, SessionOp};
use crate::connect::{OpContext, ReduceFailure, SessionReducer};
use crate::session::reducer::{reduce, AutoplayItem, QueueOp, ReduceCtx};
use crate::session::saved::SavedQueuePolicy;
use crate::session::{Entropy, DEFAULT_HISTORY_CAP};

/// Per-op entropy: keys `<opId>-<n>`, seeds from ChaCha seeded by the op.
pub struct OpEntropy {
    prefix: String,
    counter: Mutex<u32>,
    rng: Mutex<ChaCha8Rng>,
}

impl OpEntropy {
    pub fn new(prefix: &str, seed: u32) -> Self {
        OpEntropy {
            prefix: prefix.to_string(),
            counter: Mutex::new(0),
            rng: Mutex::new(ChaCha8Rng::seed_from_u64(seed as u64)),
        }
    }
}

impl Entropy for OpEntropy {
    fn next_key(&self) -> QueueKey {
        let mut c = self.counter.lock();
        *c += 1;
        format!("{}-{}", self.prefix, *c)
    }

    fn next_seed(&self) -> u32 {
        self.rng.lock().next_u32()
    }
}

/// The real reducer behind the seam.
#[derive(Debug, Clone)]
pub struct RealReducer {
    pub history_cap: usize,
    pub saved: SavedQueuePolicy,
}

impl Default for RealReducer {
    fn default() -> Self {
        RealReducer { history_cap: DEFAULT_HISTORY_CAP, saved: SavedQueuePolicy::default() }
    }
}

impl RealReducer {
    pub fn shared() -> Arc<dyn SessionReducer> {
        Arc::new(RealReducer::default())
    }
}

/// Wire op → reducer op. `None` for [`SessionOp::Replace`], which the
/// engine handles itself.
pub fn to_queue_op(op: &SessionOp) -> Option<QueueOp> {
    Some(match op {
        SessionOp::PlayContext { args } => QueueOp::PlayContext { args: args.clone() },
        SessionOp::PlayTracks { server_id, track_ids, start_index, label, shuffle, save_outgoing } => QueueOp::PlayTracks {
            server_id: server_id.clone(),
            track_ids: track_ids.clone(),
            start_index: *start_index,
            label: label.clone(),
            shuffle: *shuffle,
            save_outgoing: *save_outgoing,
        },
        SessionOp::PlayNext { server_id, track_ids } => {
            QueueOp::PlayNext { server_id: server_id.clone(), track_ids: track_ids.clone() }
        }
        SessionOp::PlayLater { server_id, track_ids } => {
            QueueOp::PlayLater { server_id: server_id.clone(), track_ids: track_ids.clone() }
        }
        SessionOp::JumpToQueueItem { key } => QueueOp::JumpToQueueItem { key: key.clone() },
        SessionOp::RemoveQueueItems { keys } => QueueOp::RemoveQueueItems { keys: keys.clone() },
        SessionOp::MoveQueueItem { key, to_index } => QueueOp::MoveQueueItem { key: key.clone(), to_index: *to_index },
        SessionOp::ClearQueue => QueueOp::ClearQueue,
        SessionOp::ClearInsertions => QueueOp::ClearInsertions,
        SessionOp::SetShuffle { enabled } => QueueOp::SetShuffle { enabled: *enabled },
        SessionOp::Reshuffle => QueueOp::Reshuffle,
        SessionOp::SetRepeat { mode } => QueueOp::SetRepeat { mode: *mode },
        SessionOp::SetAutoplay { enabled } => QueueOp::SetAutoplay { enabled: *enabled },
        SessionOp::SetQueueMode { mode } => QueueOp::SetQueueMode { mode: *mode },
        SessionOp::Next => QueueOp::Next,
        SessionOp::Previous => QueueOp::Previous,
        SessionOp::SkipUnavailable { key } => QueueOp::SkipUnavailable { key: key.clone() },
        SessionOp::AppendAutoplay { items } => QueueOp::AppendAutoplay {
            items: items
                .iter()
                .map(|i: &AutoplayOpItem| AutoplayItem {
                    track_id: i.track_id.clone(),
                    provider: i.provider,
                    reason: i.reason.clone(),
                    score: i.score,
                })
                .collect(),
        },
        SessionOp::TrackEnded => QueueOp::TrackEnded,
        SessionOp::RestoreSavedQueue { id, tracks } => QueueOp::RestoreSavedQueue { id: id.clone(), tracks: tracks.clone() },
        SessionOp::SetContextTracks { tracks } => QueueOp::SetContextTracks { tracks: tracks.clone() },
        SessionOp::SaveCurrentQueue { pinned } => QueueOp::SaveCurrentQueue { pinned: *pinned },
        SessionOp::PinSavedQueue { id, pinned } => QueueOp::PinSavedQueue { id: id.clone(), pinned: *pinned },
        SessionOp::DeleteSavedQueue { id } => QueueOp::DeleteSavedQueue { id: id.clone() },
        SessionOp::TouchSavedQueue { id } => QueueOp::TouchSavedQueue { id: id.clone() },
        SessionOp::MergeSavedQueues { remote } => QueueOp::MergeSavedQueues { remote: remote.clone() },
        SessionOp::Replace { .. } => return None,
    })
}

impl SessionReducer for RealReducer {
    fn apply(&self, doc: &SessionDocument, op: &SessionOp, ctx: &OpContext) -> Result<SessionDocument, ReduceFailure> {
        let Some(qop) = to_queue_op(op) else {
            return Err(ReduceFailure("replace is handled by the engine".into()));
        };
        let entropy = OpEntropy::new(&ctx.key_prefix, ctx.seed);
        let rctx = ReduceCtx {
            now: ctx.now,
            position_ms: ctx.position_ms,
            history_cap: self.history_cap,
            saved: self.saved.clone(),
            entropy: &entropy,
        };
        let (next, _effects) = reduce(doc, qop, &rctx).map_err(|e| ReduceFailure(e.to_string()))?;
        Ok(next)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::{ContextKind, PlayContextArgs, QueueContext, SortOrder};
    use crate::connect::{apply_op, op_context};

    fn ctx_op(n: usize) -> SessionOp {
        SessionOp::PlayContext {
            args: PlayContextArgs {
                context: QueueContext {
                    server_id: "srv".into(),
                    kind: ContextKind::Album { id: "alb".into() },
                    label: "Album".into(),
                    sort: SortOrder::Default,
                    tracks: (0..n).map(|i| format!("t{i}")).collect(),
                },
                start_index: Some(0),
                shuffle: true,
                save_outgoing: false,
            },
        }
    }

    #[test]
    fn same_op_same_id_converges_on_two_replicas() {
        let r = RealReducer::default();
        let base = crate::session::new_document("s", "sid".into(), 0.0);
        let ctx = op_context("op-1", 10.0, 0);
        let a = apply_op(&r, &base, &ctx_op(12), &ctx, 1).unwrap();
        let b = apply_op(&r, &base, &ctx_op(12), &ctx, 1).unwrap();
        assert_eq!(a, b);
        assert_eq!(a.revision, 1);
        assert!(a.shuffle.is_some());
        let ctx2 = op_context("op-2", 11.0, 0);
        let a2 = apply_op(&r, &a, &SessionOp::Next, &ctx2, 2).unwrap();
        let b2 = apply_op(&r, &b, &SessionOp::Next, &ctx2, 2).unwrap();
        assert_eq!(a2, b2);
        assert_eq!(a2.current.as_ref().unwrap().key, "op-2-1");
    }

    #[test]
    fn different_op_ids_give_different_keys() {
        let r = RealReducer::default();
        let base = crate::session::new_document("s", "sid".into(), 0.0);
        let a = apply_op(&r, &base, &ctx_op(3), &op_context("x", 1.0, 0), 1).unwrap();
        let b = apply_op(&r, &base, &ctx_op(3), &op_context("y", 1.0, 0), 1).unwrap();
        assert_ne!(a.current.as_ref().unwrap().key, b.current.as_ref().unwrap().key);
    }

    #[test]
    fn replace_keeps_identity_and_lease() {
        let r = RealReducer::default();
        let mut base = crate::session::new_document("scope", "sid".into(), 0.0);
        base.transport.lease.epoch = 7;
        let mut other = crate::session::new_document("other", "other-sid".into(), 0.0);
        other.autoplay = true;
        let out = apply_op(&r, &base, &SessionOp::Replace { document: other }, &op_context("o", 5.0, 0), 4).unwrap();
        assert_eq!(out.session_id, "sid");
        assert_eq!(out.scope, "scope");
        assert_eq!(out.revision, 4);
        assert!(out.autoplay);
        assert_eq!(out.transport.lease.epoch, 7);
    }

    #[test]
    fn unapplicable_op_is_an_error_not_a_panic() {
        let r = RealReducer::default();
        let base = crate::session::new_document("s", "sid".into(), 0.0);
        let err = apply_op(&r, &base, &SessionOp::JumpToQueueItem { key: "nope".into() }, &op_context("o", 1.0, 0), 1);
        assert!(err.is_err());
    }

    #[test]
    fn every_wire_op_maps() {
        use crate::api::{QueueMode, RepeatMode};
        let ops = vec![
            SessionOp::PlayNext { server_id: "s".into(), track_ids: vec![] },
            SessionOp::PlayLater { server_id: "s".into(), track_ids: vec![] },
            SessionOp::JumpToQueueItem { key: "k".into() },
            SessionOp::RemoveQueueItems { keys: vec![] },
            SessionOp::MoveQueueItem { key: "k".into(), to_index: 0 },
            SessionOp::ClearQueue,
            SessionOp::ClearInsertions,
            SessionOp::SetShuffle { enabled: true },
            SessionOp::Reshuffle,
            SessionOp::SetRepeat { mode: RepeatMode::All },
            SessionOp::SetAutoplay { enabled: true },
            SessionOp::SetQueueMode { mode: QueueMode::YouTube },
            SessionOp::Next,
            SessionOp::Previous,
            SessionOp::SkipUnavailable { key: "k".into() },
            SessionOp::AppendAutoplay { items: vec![] },
            SessionOp::TrackEnded,
            SessionOp::RestoreSavedQueue { id: "i".into(), tracks: None },
            SessionOp::SetContextTracks { tracks: vec![] },
            SessionOp::SaveCurrentQueue { pinned: true },
            SessionOp::PinSavedQueue { id: "i".into(), pinned: true },
            SessionOp::DeleteSavedQueue { id: "i".into() },
            SessionOp::TouchSavedQueue { id: "i".into() },
            SessionOp::MergeSavedQueues { remote: vec![] },
        ];
        for op in ops {
            assert!(to_queue_op(&op).is_some(), "{op:?}");
        }
        assert!(to_queue_op(&SessionOp::Replace { document: crate::session::new_document("s", "x".into(), 0.0) }).is_none());
    }
}
