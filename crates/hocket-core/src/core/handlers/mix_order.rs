//! AutoMix's order in the actor (`automix::order`, `docs/automix.md` →
//! *Order*): when AutoMix is on and the order is Hocket's (shuffle, autoplay),
//! the transport owner picks which of the next few entries follows the next
//! track, so it can blend into it.
//!
//! The choice is made for the entry *after* the next one: the current
//! track's tail start was committed against the next track when the current
//! one was preloaded, so the next track is already fixed, but its own tail
//! start (its source's `mix_out_ms`) is planned against whatever follows it.
//! Once per entry and follow-up, so skipping back and forward again finds
//! the order it left (a reshuffle or an edit that gives the entry another
//! follow-up decides again), and only once the current track has played a
//! while ([`SETTLE_MS`]), so skipping through tracks moves nothing. The move travels as a [`SessionOp::PromoteForMix`], so every
//! device sees the same order.
//!
//! Never touched: the "Next in queue" group, anything the user added, an
//! album or playlist in order (shuffle off), and everything past the first
//! entry that isn't ours to order.

use std::collections::{HashMap, HashSet};

use crate::api::*;
use crate::automix::order::{choose, mix_score, ORDER_WINDOW, SETTLE_MS};
use crate::automix::LOCK_IN_MS;
use crate::connect::wire::SessionOp;
use crate::core::actor::Actor;

#[derive(Debug, Default)]
pub(crate) struct MixOrderState {
    /// Entries whose follow-up was decided, with the follow-up chosen. While
    /// an entry still has it, a revisit (Previous, then Next) decides nothing.
    decided: HashMap<QueueKey, QueueKey>,
    /// A decision is due but was held back by something that passes (the
    /// current track not settled yet, a load pending, a record fetch in
    /// flight, offline): position reports ask again.
    retry: bool,
    /// A promotion sent: replan the preloaded follow-up once it shows.
    promoted: Option<QueueKey>,
    /// Times each upcoming entry was passed over.
    passes: HashMap<QueueKey, u8>,
}

/// Whether `item` is Hocket's to reorder.
fn orderable(doc: &SessionDocument, item: &QueueItem) -> bool {
    !item.queued
        && !item.added
        && !item.unavailable
        && match item.source {
            QueueSource::Autoplay { .. } => true,
            QueueSource::Context { .. } => doc.shuffle,
            QueueSource::Inserted => false,
        }
}

impl Actor {
    /// Run [`Self::order_for_mix`] once the current reaction is over.
    pub(crate) fn post_order_for_mix(&mut self) {
        let _ = self.tx.send(crate::core::ActorMsg::Internal(
            crate::core::Internal::OrderForMix,
        ));
    }

    /// Decide the follow-up of the next entry, if it is ours to decide and
    /// not decided yet. Called whenever the document changes and when
    /// records arrive.
    pub(crate) fn order_for_mix(&mut self) {
        let Some(doc) = self.doc().cloned() else {
            return;
        };
        if let Some(key) = self.mix_order.promoted.clone() {
            if doc.upcoming.get(1).map(|i| &i.key) == Some(&key) {
                // The next track's tail start was planned against the old follow-up.
                self.mix_order.promoted = None;
                self.replan_next();
            }
        }
        self.mix_order.retry = false;
        if !self.owns_transport()
            || self.transition_settings().mode != TransitionMode::AutoMix
            || !self.mix_context_allows()
            || doc.repeat != RepeatMode::Off
            // Too late to replan the next track's tail start against a new
            // follow-up (a committed plan, a mix running).
            || self.locked_tail_start().is_some()
            || self.playback.transition.is_some()
        {
            return;
        }
        let Some(anchor) = doc.upcoming.first().filter(|i| !i.unavailable) else {
            return;
        };
        if self.mix_order.decided.get(&anchor.key) == doc.upcoming.get(1).map(|i| &i.key) {
            return;
        }
        if self.is_offline()
            // Offline, the next playable entry isn't always the document's next.
            || self.share.in_flight.is_some()
            || self.playback.pending_load.is_some()
            // Not while the user skips through tracks: each skip would move
            // the entry after the next one, and the queue would shift under them.
            || self.playback.position_now(self.now()) < self.settle_position()
        {
            self.mix_order.retry = true;
            return;
        }
        let run: Vec<&QueueItem> = doc.upcoming[1..]
            .iter()
            .take_while(|i| orderable(&doc, i))
            .take(ORDER_WINDOW)
            .collect();
        if run.len() < 2 {
            // Nothing to choose from (yet: shuffle may turn on, autoplay append).
            return;
        }
        let Some(a) = self.track_for_item(anchor).map(|t| self.mix_facts(&t)) else {
            return;
        };
        if a.record.is_none() {
            // Nothing to judge by yet: decide when its record arrives.
            return;
        }
        let scores: Vec<Option<f64>> = run
            .iter()
            .map(|item| {
                let b = self.mix_facts(&self.track_for_item(item)?);
                b.record.as_ref()?;
                let plan = self.plan_between(&a, &b, None, false);
                Some(mix_score(
                    plan.as_ref(),
                    a.record.as_ref(),
                    b.record.as_ref(),
                ))
            })
            .collect();
        let front_passes = self.mix_order.passes.get(&run[0].key).copied().unwrap_or(0);
        let pick = choose(&scores, front_passes);
        let in_queue: HashSet<&QueueKey> = doc
            .history
            .iter()
            .chain(doc.current.as_ref())
            .chain(doc.upcoming.iter())
            .map(|i| &i.key)
            .collect();
        self.mix_order.decided.retain(|k, _| in_queue.contains(k));
        self.mix_order
            .decided
            .insert(anchor.key.clone(), run[pick].key.clone());
        let keep: HashSet<&QueueKey> = doc.upcoming.iter().map(|i| &i.key).collect();
        self.mix_order.passes.retain(|k, _| keep.contains(k));
        if pick == 0 {
            return;
        }
        tracing::debug!(
            target: "hocket::automix",
            after = %anchor.track.id,
            picked = %run[pick].track.id,
            from = pick,
            ?scores,
            "mix order: promoted"
        );
        for skipped in &run[..pick] {
            *self
                .mix_order
                .passes
                .entry(skipped.key.clone())
                .or_default() += 1;
        }
        let key = run[pick].key.clone();
        let after = anchor.key.clone();
        self.mix_order.promoted = Some(key.clone());
        if !self.local_op_quiet(SessionOp::PromoteForMix { key, after }) {
            self.mix_order.promoted = None;
        }
    }

    /// The current track's position from which the order may be decided:
    /// [`SETTLE_MS`], or halfway to the point its tail locks in when that
    /// comes sooner (a short track, or a short planned tail start). The same
    /// tail start [`Self::locked_tail_start`] measures from, when planned.
    fn settle_position(&self) -> Ms {
        let end = self
            .playback
            .next
            .as_ref()
            .and_then(|n| n.mix_in.as_ref())
            .map(|p| p.tail_start_ms)
            .or(self.playback.mix_out_ms)
            .unwrap_or_else(|| self.playback.duration_ms());
        SETTLE_MS.min(end.saturating_sub(LOCK_IN_MS) / 2)
    }

    /// A position report: ask again for a decision that was held back, once
    /// the current track has settled.
    pub(crate) fn order_for_mix_at(&mut self, position_ms: Ms) {
        if self.mix_order.retry && position_ms >= self.settle_position() {
            self.order_for_mix();
        }
    }
}
