//! AutoMix's order in the actor (`automix::order`, `docs/automix.md` →
//! *Order*): when AutoMix is on and the order is Hocket's (shuffle, autoplay),
//! the transport owner picks which of the next few entries follows the next
//! track, so it can blend into it.
//!
//! The choice is made for the entry *after* the next one: the current
//! track's tail start was committed against the next track when the current
//! one was preloaded, so the next track is already fixed, but its own tail
//! start (its source's `mix_out_ms`) is planned against whatever follows it.
//! Once per next entry, as soon as its record is known; the move travels as a
//! [`SessionOp::PromoteForMix`], so every device sees the same order.
//!
//! Never touched: the "Next in queue" group, anything the user added, an
//! album or playlist in order (shuffle off), and everything past the first
//! entry that isn't ours to order.

use std::collections::HashMap;

use crate::api::*;
use crate::automix::order::{choose, mix_score, ORDER_WINDOW};
use crate::connect::wire::SessionOp;
use crate::core::actor::Actor;

#[derive(Debug, Default)]
pub(crate) struct MixOrderState {
    /// The next entry whose follow-up was decided (once per entry).
    decided_after: Option<QueueKey>,
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
        if !self.owns_transport()
            || self.transition_settings().mode != TransitionMode::AutoMix
            || !self.mix_context_allows()
            || doc.repeat != RepeatMode::Off
            // Offline, the next playable entry isn't always the document's next.
            || self.is_offline()
            || self.share.in_flight.is_some()
        {
            return;
        }
        let Some(anchor) = doc.upcoming.first().filter(|i| !i.unavailable) else {
            return;
        };
        if self.mix_order.decided_after.as_ref() == Some(&anchor.key) {
            return;
        }
        let run: Vec<&QueueItem> = doc.upcoming[1..]
            .iter()
            .take_while(|i| orderable(&doc, i))
            .take(ORDER_WINDOW)
            .collect();
        if run.len() < 2 {
            self.mix_order.decided_after = Some(anchor.key.clone());
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
        self.mix_order.decided_after = Some(anchor.key.clone());
        let keep: Vec<QueueKey> = doc.upcoming.iter().map(|i| i.key.clone()).collect();
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
}
