//! AutoMix's order in the actor (`automix::order`, `docs/automix.md` →
//! *Order*): when AutoMix is on and the order is Hocket's (shuffle, autoplay),
//! the transport owner picks which of the next few entries follows each of
//! the first [`ORDER_AHEAD`] upcoming entries, so each can blend into the one
//! after it.
//!
//! The order is decided ahead, as a chain: the follow-up of the next entry,
//! then of that follow-up, and so on. Each decision, a move or keeping the
//! follow-up there is, is a [`SessionOp::PromoteForMix`], which marks the
//! follow-up `settled` in the document: every device sees the same order,
//! and a settled entry (like one the user dragged) is never moved or passed
//! over again, whoever owns playback next. A shuffle clears the marks. So
//! in steady play each track decides one new pair, about [`ORDER_AHEAD`]
//! entries down, and what is near the top of the queue never moves under
//! the listener; skipping back and forward finds the order it left. After a
//! skip nothing is decided until the track has played a while
//! ([`SETTLE_MS`]), so a burst of skips moves nothing.
//!
//! The chain waits for a record that is on its way (a fetch in flight or
//! about to go out, an analysis running) rather than decide without it. An
//! entry whose own record isn't coming keeps its follow-up undecided (its
//! record may come later); candidates without one count as a typical pair.
//!
//! The next playable entry's follow-up is the only one the playback plan
//! depends on: the current track's tail start was committed against the
//! next track when the current one was preloaded, and the next track's own
//! tail start (its source's `mix_out_ms`) is planned against what follows
//! it. So that pair is not decided once the plan has locked in or a mix is
//! running.
//!
//! Never touched: the "Next in queue" group, anything the user added, an
//! album or playlist in order (shuffle off), and everything past the first
//! entry that isn't ours to order.

use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use crate::api::*;
use crate::automix::order::{choose, mix_score, ORDER_AHEAD, ORDER_WINDOW, SETTLE_MS};
use crate::automix::LOCK_IN_MS;
use crate::connect::wire::SessionOp;
use crate::core::actor::Actor;
use crate::core::handlers::transitions::MixFacts;

#[derive(Debug, Default)]
pub(crate) struct MixOrderState {
    /// Entries whose follow-up was decided here, with the follow-up chosen.
    /// While an entry still has it, it is not decided again (even when a
    /// room's document lacks the `settled` mark).
    decided: HashMap<QueueKey, QueueKey>,
    /// A decision may be due but was held back by something that passes (a
    /// skip just now, a load pending, a record on its way, offline):
    /// position reports ask again.
    retry: bool,
    /// A promotion sent into the next entry's follow-up: replan the
    /// preloaded next track once it shows.
    promoted: Option<QueueKey>,
    /// Times each upcoming entry was passed over.
    passes: HashMap<QueueKey, u8>,
}

/// One step of the chain, in order: each is based on the ones before it.
/// `key` follows `anchor` from now on, passing over `passed` (none: it was
/// there already).
struct Step {
    anchor: QueueKey,
    key: QueueKey,
    passed: Vec<QueueKey>,
    /// A move into the next playable entry's follow-up (the playback plan's).
    replan: bool,
}

/// Whether `item` is Hocket's to reorder.
fn orderable(doc: &SessionDocument, item: &QueueItem) -> bool {
    !item.queued
        && !item.added
        && !item.settled
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

    /// Decide the follow-ups of the first [`ORDER_AHEAD`] upcoming entries
    /// that are ours to decide and not decided yet. Called whenever the
    /// document changes and when records arrive.
    pub(crate) fn order_for_mix(&mut self) {
        let Some(doc) = self.doc() else {
            return;
        };
        // The follow-up of the next playable entry (offline the order is off).
        let follow_up = doc.upcoming.iter().filter(|i| !i.unavailable).nth(1);
        let shown = self
            .mix_order
            .promoted
            .as_ref()
            .is_some_and(|key| follow_up.map(|i| &i.key) == Some(key));
        if shown {
            // The next track's tail start was planned against the old follow-up.
            self.mix_order.promoted = None;
            self.replan_next();
        }
        self.mix_order.retry = false;
        let Some(doc) = self.doc() else {
            return;
        };
        if !self.owns_transport()
            || self.transition_settings().mode != TransitionMode::AutoMix
            || !self.mix_context_allows()
            || doc.repeat != RepeatMode::Off
        {
            return;
        }
        let now = self.now();
        if self.is_offline()
            // Offline, the next playable entry isn't always the document's next.
            || self.playback.pending_load.is_some()
            || self.skip_settling(now)
        {
            self.mix_order.retry = true;
            return;
        }
        let (steps, wait) = self.mix_order_steps(doc);
        self.mix_order.retry = wait;
        if steps.is_empty() {
            return;
        }
        self.prune_mix_order();
        for Step {
            anchor,
            key,
            passed,
            replan,
        } in steps
        {
            if replan {
                self.mix_order.promoted = Some(key.clone());
            }
            if !self.local_op_quiet(SessionOp::PromoteForMix {
                key: key.clone(),
                after: anchor.clone(),
            }) {
                // Not sent: nothing moved, so nothing is decided or passed
                // over, here or further down (decided on top of it).
                if replan {
                    self.mix_order.promoted = None;
                }
                return;
            }
            self.mix_order.decided.insert(anchor, key);
            for k in passed {
                *self.mix_order.passes.entry(k).or_default() += 1;
            }
        }
    }

    /// The chain's steps over `doc` as it is, and whether it stopped to wait
    /// for a record on its way.
    fn mix_order_steps(&self, doc: &SessionDocument) -> (Vec<Step>, bool) {
        let mut look = Lookups::new(self);
        let plan_fixed = self.locked_tail_start().is_some() || self.playback.transition.is_some();
        let mut list: Vec<&QueueItem> = doc.upcoming.iter().collect();
        // What plays next (unavailable entries are skipped).
        let next = list.iter().position(|i| !i.unavailable);
        let mut passes: HashMap<&QueueKey, u8> = HashMap::new();
        let mut steps = vec![];
        for j in 0..ORDER_AHEAD.min(list.len()) {
            let anchor = list[j];
            if (Some(j) == next && plan_fixed)
                || anchor.unavailable
                || self.mix_order.decided.get(&anchor.key) == list.get(j + 1).map(|i| &i.key)
            {
                continue;
            }
            let run: Vec<&QueueItem> = list[j + 1..]
                .iter()
                .copied()
                .take_while(|i| orderable(doc, i))
                .take(ORDER_WINDOW)
                .collect();
            if run.len() < 2 {
                // Nothing to choose from (yet: shuffle may turn on, autoplay
                // append), or the follow-up is settled.
                continue;
            }
            let Some(a) = look.facts(anchor) else {
                continue;
            };
            if look.coming(anchor) {
                return (steps, true);
            }
            if a.record.is_none() {
                // Nothing to judge by: undecided, in case its record comes later.
                continue;
            }
            if run.iter().any(|i| look.coming(i)) {
                return (steps, true);
            }
            let scores: Vec<Option<f64>> = run
                .iter()
                .map(|i| {
                    let b = look.facts(i)?;
                    b.record.as_ref()?;
                    let plan = self.plan_between(&a, &b, None, false);
                    Some(mix_score(
                        plan.as_ref(),
                        a.record.as_ref(),
                        b.record.as_ref(),
                    ))
                })
                .collect();
            let front_passes = self.mix_order.passes.get(&run[0].key).copied().unwrap_or(0)
                + passes.get(&run[0].key).copied().unwrap_or(0);
            let pick = choose(&scores, front_passes);
            if pick > 0 {
                tracing::debug!(
                    target: "hocket::automix",
                    after = %anchor.track.id,
                    picked = %run[pick].track.id,
                    from = pick,
                    depth = j,
                    ?scores,
                    "mix order: promoted"
                );
            }
            for skipped in &run[..pick] {
                *passes.entry(&skipped.key).or_default() += 1;
            }
            steps.push(Step {
                anchor: anchor.key.clone(),
                key: run[pick].key.clone(),
                passed: run[..pick].iter().map(|i| i.key.clone()).collect(),
                replan: pick > 0 && Some(j) == next,
            });
            let moved = list.remove(j + 1 + pick);
            list.insert(j + 1, moved);
        }
        (steps, false)
    }

    /// Forget decisions about entries no longer in the queue, and passes of
    /// entries no longer ahead.
    fn prune_mix_order(&mut self) {
        let Some(doc) = self.doc() else {
            return;
        };
        let in_queue: HashSet<&QueueKey> = doc
            .history
            .iter()
            .chain(doc.current.as_ref())
            .chain(doc.upcoming.iter())
            .map(|i| &i.key)
            .collect();
        let ahead: HashSet<&QueueKey> = doc.upcoming.iter().map(|i| &i.key).collect();
        let gone: Vec<QueueKey> = self
            .mix_order
            .decided
            .keys()
            .filter(|k| !in_queue.contains(k))
            .cloned()
            .collect();
        let behind: Vec<QueueKey> = self
            .mix_order
            .passes
            .keys()
            .filter(|k| !ahead.contains(k))
            .cloned()
            .collect();
        for k in gone {
            self.mix_order.decided.remove(&k);
        }
        for k in behind {
            self.mix_order.passes.remove(&k);
        }
    }

    /// A skip just now: the track it landed on hasn't played a while yet.
    fn skip_settling(&self, now: EpochMs) -> bool {
        self.last_skip_at
            .is_some_and(|t| now - t < f64::from(SETTLE_MS))
            && self.playback.position_now(now) < self.settle_position()
    }

    /// The current track's position from which the order may be decided
    /// after a skip: [`SETTLE_MS`], or halfway to the point its tail locks
    /// in when that comes sooner (a short track, or a short planned tail
    /// start). The same tail start [`Self::locked_tail_start`] measures from,
    /// when planned.
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

    /// A position report: ask again for a decision that was held back.
    /// The cheap reasons it is held back are checked here first.
    pub(crate) fn order_for_mix_at(&mut self) {
        let now = self.now();
        if self.mix_order.retry
            && !self.skip_settling(now)
            && !self.is_offline()
            && self.playback.pending_load.is_none()
        {
            self.order_for_mix();
        }
    }
}

/// One pass's lookups, each made once: entries' tracks and records come up
/// again in the windows of the anchors around them.
struct Lookups<'a> {
    actor: &'a Actor,
    now: EpochMs,
    /// The tracks analysed here next (computed on first use).
    local: Option<Vec<Track>>,
    facts: HashMap<QueueKey, Option<Rc<MixFacts>>>,
    coming: HashMap<QueueKey, bool>,
}

impl<'a> Lookups<'a> {
    fn new(actor: &'a Actor) -> Self {
        Self {
            actor,
            now: actor.now(),
            local: None,
            facts: HashMap::new(),
            coming: HashMap::new(),
        }
    }

    fn facts(&mut self, item: &QueueItem) -> Option<Rc<MixFacts>> {
        let actor = self.actor;
        self.facts
            .entry(item.key.clone())
            .or_insert_with(|| {
                actor
                    .track_for_item(item)
                    .map(|t| Rc::new(actor.mix_facts(&t)))
            })
            .clone()
    }

    /// Whether a record better than the one the entry's track has is on its
    /// way: a fetch in flight or about to go out, or an analysis here.
    fn coming(&mut self, item: &QueueItem) -> bool {
        if let Some(c) = self.coming.get(&item.key) {
            return *c;
        }
        let actor = self.actor;
        let c = match self.facts(item) {
            Some(f) if Actor::record_is_lacking(f.record.as_ref()) => {
                actor.analysis_fetch_coming(&f.track, self.now)
                    || actor.local_analysis_coming(
                        &f.track,
                        self.local
                            .get_or_insert_with(|| actor.analysis_candidates()),
                    )
            }
            _ => false,
        };
        self.coming.insert(item.key.clone(), c);
        c
    }
}
