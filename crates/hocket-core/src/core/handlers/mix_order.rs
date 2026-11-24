//! AutoMix's order in the actor (`automix::order`, `docs/automix.md` →
//! *Order*): when AutoMix is on and the order is Hocket's (shuffle, autoplay),
//! the transport owner picks which of the next few entries follows each of
//! the first [`ORDER_AHEAD`] upcoming entries, so each can blend into the one
//! after it.
//!
//! The order is decided ahead, as a chain: the follow-up of the next entry,
//! then of that follow-up, and so on. Once a pair is decided it stays (a
//! reshuffle or an edit that gives an entry another follow-up decides it
//! again), so in steady play each track decides one new pair, about
//! [`ORDER_AHEAD`] entries down, and what is near the top of the queue never
//! moves under the listener. Skipping back and forward finds the order it
//! left. After a skip nothing is decided until the track has played a while
//! ([`SETTLE_MS`]), so a burst of skips moves nothing.
//!
//! The chain waits for a record that is on its way (a fetch in flight or
//! about to go out, an analysis running) rather than decide without it. An
//! entry whose own record isn't coming keeps its follow-up undecided (its
//! record may come later); candidates without one count as a typical pair.
//!
//! The next entry's follow-up is the only one the playback plan depends on:
//! the current track's tail start was committed against the next track when
//! the current one was preloaded, and the next track's own tail start (its
//! source's `mix_out_ms`) is planned against what follows it. So that pair is
//! not decided once the plan has locked in or a mix is running. A move
//! travels as a [`SessionOp::PromoteForMix`], so every device sees the same
//! order.
//!
//! Never touched: the "Next in queue" group, anything the user added, an
//! album or playlist in order (shuffle off), and everything past the first
//! entry that isn't ours to order.

use std::collections::{HashMap, HashSet};

use crate::api::*;
use crate::automix::order::{choose, mix_score, ORDER_AHEAD, ORDER_WINDOW, SETTLE_MS};
use crate::automix::LOCK_IN_MS;
use crate::connect::wire::SessionOp;
use crate::core::actor::Actor;

#[derive(Debug, Default)]
pub(crate) struct MixOrderState {
    /// Entries whose follow-up was decided, with the follow-up chosen. While
    /// an entry still has it, it is not decided again.
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
enum Step {
    /// `anchor` keeps the follow-up it has.
    Keep { anchor: QueueKey, follow: QueueKey },
    /// `key` moves to directly after `anchor`, passing over `passed`.
    Promote {
        anchor: QueueKey,
        key: QueueKey,
        passed: Vec<QueueKey>,
        /// The next entry's follow-up (the playback plan's).
        next: bool,
    },
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

    /// Decide the follow-ups of the first [`ORDER_AHEAD`] upcoming entries
    /// that are ours to decide and not decided yet. Called whenever the
    /// document changes and when records arrive.
    pub(crate) fn order_for_mix(&mut self) {
        let Some(doc) = self.doc() else {
            return;
        };
        let shown = self
            .mix_order
            .promoted
            .as_ref()
            .is_some_and(|key| doc.upcoming.get(1).map(|i| &i.key) == Some(key));
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
        for step in steps {
            match step {
                Step::Keep { anchor, follow } => {
                    self.mix_order.decided.insert(anchor, follow);
                }
                Step::Promote {
                    anchor,
                    key,
                    passed,
                    next,
                } => {
                    if next {
                        self.mix_order.promoted = Some(key.clone());
                    }
                    if !self.local_op_quiet(SessionOp::PromoteForMix {
                        key: key.clone(),
                        after: anchor.clone(),
                    }) {
                        // Not sent: nothing moved, so nothing is decided or
                        // passed over, here or further down (decided on top of it).
                        if next {
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
        }
    }

    /// The chain's steps over `doc` as it is, and whether it stopped to wait
    /// for a record on its way.
    fn mix_order_steps(&self, doc: &SessionDocument) -> (Vec<Step>, bool) {
        let now = self.now();
        let plan_fixed = self.locked_tail_start().is_some() || self.playback.transition.is_some();
        let mut list: Vec<&QueueItem> = doc.upcoming.iter().collect();
        let mut passes: HashMap<&QueueKey, u8> = HashMap::new();
        let mut steps = vec![];
        for j in 0..ORDER_AHEAD.min(list.len()) {
            let anchor = list[j];
            if (j == 0 && plan_fixed)
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
                // Nothing to choose from (yet: shuffle may turn on, autoplay append).
                continue;
            }
            let Some(anchor_track) = self.track_for_item(anchor) else {
                continue;
            };
            let tracks: Vec<Option<Track>> = run.iter().map(|i| self.track_for_item(i)).collect();
            if std::iter::once(&anchor_track)
                .chain(tracks.iter().flatten())
                .any(|t| self.record_coming(t, now))
            {
                return (steps, true);
            }
            let a = self.mix_facts(&anchor_track);
            if a.record.is_none() {
                // Nothing to judge by: undecided, in case its record comes later.
                continue;
            }
            let scores: Vec<Option<f64>> = tracks
                .iter()
                .map(|t| {
                    let b = self.mix_facts(t.as_ref()?);
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
            if pick == 0 {
                steps.push(Step::Keep {
                    anchor: anchor.key.clone(),
                    follow: run[0].key.clone(),
                });
                continue;
            }
            tracing::debug!(
                target: "hocket::automix",
                after = %anchor.track.id,
                picked = %run[pick].track.id,
                from = pick,
                depth = j,
                ?scores,
                "mix order: promoted"
            );
            for skipped in &run[..pick] {
                *passes.entry(&skipped.key).or_default() += 1;
            }
            steps.push(Step::Promote {
                anchor: anchor.key.clone(),
                key: run[pick].key.clone(),
                passed: run[..pick].iter().map(|i| i.key.clone()).collect(),
                next: j == 0,
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

    /// Whether a record better than the one `track` has is on its way: a
    /// fetch in flight or about to go out, or an analysis here.
    fn record_coming(&self, track: &Track, now: EpochMs) -> bool {
        self.needs_analysis(track)
            && (self.analysis_fetch_coming(track, now) || self.local_analysis_coming(track))
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
