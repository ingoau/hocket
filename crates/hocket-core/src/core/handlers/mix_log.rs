//! One log line per planned AutoMix transition once its outcome is known
//! (`docs/automix.md` → *Outcome log*), so a diagnostics bundle answers
//! "which mixes happened, and did the user bail". [`MixLog`] is a small
//! deterministic state machine fed by the actor at the points where a plan
//! is handed to the backend, the backend reports the transition, and the
//! current item changes; the actor writes each [`OutcomeLine`] at info on
//! `hocket::player` (and to the recent-log ring).
//!
//! Outcomes:
//! - `mixed`: `TransitionStarted`, then the handover, and the incoming track
//!   was not left within [`SKIP_WINDOW_MS`] after it.
//! - `skipped`: the user left during the overlap (Next completing the
//!   transition counts) or within [`SKIP_WINDOW_MS`] after the handover.
//! - `not_started`: A ended, or B took over, with a plan but no
//!   `TransitionStarted` (a late plan, a seek past the tail start, a failed
//!   render: the backend joined gaplessly instead).
//! - `replanned`: the follow-up changed (queue edit, setting, context rule)
//!   before the transition started.
//! - `failed`: the follow-up failed for good during the overlap.
//! - `stopped`: playback stopped here (handoff, stop) during the overlap.
//!
//! A skip away from A before its tail started is no transition outcome: the
//! plan is dropped at debug level.

use crate::api::*;
use crate::core::actor::Actor;

/// A skip this soon after the handover still counts against the mix.
pub(crate) const SKIP_WINDOW_MS: f64 = 20_000.0;

/// One planned transition, as handed to the backend.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Planned {
    /// Backend keys (what the reports carry).
    pub from_key: QueueKey,
    pub to_key: QueueKey,
    pub from_track: TrackId,
    pub to_track: TrackId,
    pub kind: TransitionKind,
    pub tail_len_ms: Ms,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Outcome {
    Mixed,
    NotStarted,
    /// `after_handover_ms`: `None` during the overlap.
    Skipped {
        after_handover_ms: Option<Ms>,
    },
    Replanned,
    Failed,
    Stopped,
}

impl Outcome {
    pub fn name(self) -> &'static str {
        match self {
            Outcome::Mixed => "mixed",
            Outcome::NotStarted => "not_started",
            Outcome::Skipped { .. } => "skipped",
            Outcome::Replanned => "replanned",
            Outcome::Failed => "failed",
            Outcome::Stopped => "stopped",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct OutcomeLine {
    pub plan: Planned,
    pub outcome: Outcome,
}

#[derive(Debug, Default)]
pub(crate) struct MixLog {
    /// The plan into the preloaded follow-up, and whether its transition
    /// has started (the backend reported `TransitionStarted`).
    armed: Option<(Planned, bool)>,
    /// A mix that handed over at this local time: `mixed` unless the user
    /// leaves within [`SKIP_WINDOW_MS`].
    handed: Option<(Planned, EpochMs)>,
}

impl MixLog {
    fn line(plan: Planned, outcome: Outcome) -> OutcomeLine {
        OutcomeLine { plan, outcome }
    }

    /// A handed-over mix whose skip window has passed.
    pub fn tick(&mut self, now: EpochMs, out: &mut Vec<OutcomeLine>) {
        if self
            .handed
            .as_ref()
            .is_some_and(|(_, at)| now - at > SKIP_WINDOW_MS)
        {
            let (plan, _) = self.handed.take().expect("checked");
            out.push(Self::line(plan, Outcome::Mixed));
        }
    }

    /// The follow-up handed to the backend now, with its plan (`None`: no
    /// follow-up, or one joined without a plan).
    pub fn arm(&mut self, now: EpochMs, plan: Option<Planned>, out: &mut Vec<OutcomeLine>) {
        self.tick(now, out);
        match self.armed.take() {
            Some((old, true)) => match plan.as_ref() {
                // The handover came before its report did.
                Some(p) if p.from_key == old.to_key => self.hand_over(old, now, out),
                // The same pair armed again (A reloaded, e.g. a retry): its
                // transition has to be reported again.
                Some(p) if p.from_key == old.from_key && p.to_key == old.to_key => {}
                // The core never replaces the follow-up during the overlap;
                // keep the running one.
                Some(p) if p.from_key == old.from_key => {
                    self.armed = Some((old, true));
                    return;
                }
                None => {
                    self.armed = Some((old, true));
                    return;
                }
                // Another source: the running mix was abandoned.
                Some(_) => out.push(Self::line(old, Outcome::Stopped)),
            },
            Some((old, false)) => {
                let same_pair = plan
                    .as_ref()
                    .is_some_and(|p| p.from_key == old.from_key && p.to_key == old.to_key);
                if !same_pair {
                    out.push(Self::line(old, Outcome::Replanned));
                }
            }
            None => {}
        }
        self.armed = plan.map(|p| (p, false));
    }

    fn hand_over(&mut self, plan: Planned, now: EpochMs, out: &mut Vec<OutcomeLine>) {
        if let Some((prev, _)) = self.handed.take() {
            out.push(Self::line(prev, Outcome::Mixed));
        }
        self.handed = Some((plan, now));
    }

    /// `TransitionStarted` for the armed pair.
    pub fn started(&mut self, now: EpochMs, from: &str, to: &str, out: &mut Vec<OutcomeLine>) {
        self.tick(now, out);
        if let Some((p, started)) = &mut self.armed {
            if p.from_key == from && p.to_key == to {
                *started = true;
            }
        }
    }

    /// A seek cancelled the running transition: A plays on, the plan stays
    /// armed (the backend may start it again).
    pub fn cancelled(&mut self) {
        if let Some((_, started)) = &mut self.armed {
            *started = false;
        }
    }

    /// The backend reported `Ended` for `key`.
    pub fn ended(&mut self, now: EpochMs, key: &str, out: &mut Vec<OutcomeLine>) {
        self.tick(now, out);
        if self.handed.as_ref().is_some_and(|(p, _)| p.to_key == key) {
            // B played to its end inside the window: the mix stood.
            let (plan, _) = self.handed.take().expect("checked");
            out.push(Self::line(plan, Outcome::Mixed));
        }
        if self
            .armed
            .as_ref()
            .is_some_and(|(p, started)| p.from_key == key && !started)
        {
            let (plan, _) = self.armed.take().expect("checked");
            out.push(Self::line(plan, Outcome::NotStarted));
        }
    }

    /// The backend reported `TransitionedToNext` for `key`.
    pub fn transitioned(&mut self, now: EpochMs, key: &str, out: &mut Vec<OutcomeLine>) {
        self.tick(now, out);
        if self.handed.as_ref().is_some_and(|(p, _)| p.to_key != key) {
            // The mixed-in B moved on to its own follow-up inside the
            // window: the mix stood.
            let (plan, _) = self.handed.take().expect("checked");
            out.push(Self::line(plan, Outcome::Mixed));
        }
        if !self.armed.as_ref().is_some_and(|(p, _)| p.to_key == key) {
            return;
        }
        let (plan, started) = self.armed.take().expect("checked");
        if started {
            self.hand_over(plan, now, out);
        } else {
            out.push(Self::line(plan, Outcome::NotStarted));
        }
    }

    /// The user left the current item for another (Next during the overlap,
    /// a skip, a jump, a new queue; here or from a peer).
    pub fn left(&mut self, now: EpochMs, out: &mut Vec<OutcomeLine>) {
        self.tick(now, out);
        match self.armed.take() {
            Some((plan, true)) => out.push(Self::line(
                plan,
                Outcome::Skipped {
                    after_handover_ms: None,
                },
            )),
            Some((plan, false)) => {
                tracing::debug!(target: "hocket::player", from = %plan.from_key, to = %plan.to_key, "mix plan dropped: left before its tail");
            }
            None => {}
        }
        if let Some((plan, at)) = self.handed.take() {
            let after = (now - at).max(0.0) as Ms;
            out.push(Self::line(
                plan,
                Outcome::Skipped {
                    after_handover_ms: Some(after),
                },
            ));
        }
    }

    /// The follow-up `key` failed for good.
    pub fn failed(&mut self, now: EpochMs, key: &str, out: &mut Vec<OutcomeLine>) {
        self.tick(now, out);
        if self
            .armed
            .as_ref()
            .is_some_and(|(p, started)| p.to_key == key && *started)
        {
            let (plan, _) = self.armed.take().expect("checked");
            out.push(Self::line(plan, Outcome::Failed));
        }
    }

    /// Nothing plays here any more (stop, transport handed to a peer).
    pub fn unloaded(&mut self, now: EpochMs, out: &mut Vec<OutcomeLine>) {
        self.tick(now, out);
        if let Some((plan, true)) = self.armed.take() {
            out.push(Self::line(plan, Outcome::Stopped));
        }
        if let Some((plan, _)) = self.handed.take() {
            out.push(Self::line(plan, Outcome::Mixed));
        }
    }
}

impl Actor {
    /// The plan in the follow-up just handed to the backend.
    pub(crate) fn mix_log_arm(&mut self) {
        let plan = match (
            &self.playback.backend_key,
            &self.playback.track,
            &self.playback.next,
        ) {
            (Some(from_key), Some(from), Some(next)) => next.mix_in.as_ref().map(|p| Planned {
                from_key: from_key.clone(),
                to_key: next.key.clone(),
                from_track: from.id.clone(),
                to_track: next.track.id.clone(),
                kind: p.kind,
                tail_len_ms: p.tail_len_ms,
            }),
            _ => None,
        };
        let now = self.now();
        self.mix_log_with(|log, out| log.arm(now, plan, out));
    }

    /// Run `f` on the outcome log and write the lines it produced.
    pub(crate) fn mix_log_with(&mut self, f: impl FnOnce(&mut MixLog, &mut Vec<OutcomeLine>)) {
        let mut out = vec![];
        f(&mut self.mix_log, &mut out);
        for l in out {
            self.write_mix_outcome(&l);
        }
    }

    fn write_mix_outcome(&mut self, l: &OutcomeLine) {
        let p = &l.plan;
        let after = match l.outcome {
            Outcome::Skipped { after_handover_ms } => after_handover_ms,
            _ => None,
        };
        tracing::info!(
            target: "hocket::player",
            from_key = %p.from_key,
            to_key = %p.to_key,
            from_track = %p.from_track,
            to_track = %p.to_track,
            kind = ?p.kind,
            tail_len_ms = p.tail_len_ms,
            outcome = l.outcome.name(),
            after_handover_ms = ?after,
            "automix outcome"
        );
        let after = after
            .map(|a| format!(" after_handover_ms={a}"))
            .unwrap_or_default();
        self.push_log_ring(
            "info",
            format!(
                "automix outcome={} kind={:?} tail_len_ms={} from={} ({}) to={} ({}){after}",
                l.outcome.name(),
                p.kind,
                p.tail_len_ms,
                p.from_track,
                p.from_key,
                p.to_track,
                p.to_key
            ),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plan(from: &str, to: &str) -> Planned {
        Planned {
            from_key: from.into(),
            to_key: to.into(),
            from_track: format!("t-{from}"),
            to_track: format!("t-{to}"),
            kind: TransitionKind::Blend,
            tail_len_ms: 30_000,
        }
    }

    fn outcomes(out: &[OutcomeLine]) -> Vec<(String, String, Outcome)> {
        out.iter()
            .map(|l| (l.plan.from_key.clone(), l.plan.to_key.clone(), l.outcome))
            .collect()
    }

    fn o(from: &str, to: &str, outcome: Outcome) -> (String, String, Outcome) {
        (from.into(), to.into(), outcome)
    }

    #[test]
    fn a_new_source_during_the_overlap_stops_the_running_mix() {
        // Unloaded without a hook (queue cleared), then something else plays.
        let (mut log, mut out) = (MixLog::default(), vec![]);
        log.arm(0.0, Some(plan("a", "b")), &mut out);
        log.started(1_000.0, "a", "b", &mut out);
        log.arm(5_000.0, Some(plan("x", "y")), &mut out);
        assert_eq!(outcomes(&out), vec![o("a", "b", Outcome::Stopped)]);
        // The new pair is tracked.
        log.started(6_000.0, "x", "y", &mut out);
        log.ended(10_000.0, "x", &mut out);
        log.transitioned(10_000.0, "y", &mut out);
        log.tick(10_001.0 + SKIP_WINDOW_MS, &mut out);
        assert_eq!(outcomes(&out)[1], o("x", "y", Outcome::Mixed));
    }

    #[test]
    fn a_reloaded_source_must_start_its_transition_again() {
        let (mut log, mut out) = (MixLog::default(), vec![]);
        log.arm(0.0, Some(plan("a", "b")), &mut out);
        log.started(1_000.0, "a", "b", &mut out);
        // A failed and was reloaded: same pair armed again, never restarted.
        log.arm(2_000.0, Some(plan("a", "b")), &mut out);
        log.ended(30_000.0, "a", &mut out);
        assert_eq!(outcomes(&out), vec![o("a", "b", Outcome::NotStarted)]);
    }

    #[test]
    fn a_mixed_in_track_moving_on_settles_its_mix() {
        let (mut log, mut out) = (MixLog::default(), vec![]);
        log.arm(0.0, Some(plan("a", "b")), &mut out);
        log.started(1_000.0, "a", "b", &mut out);
        log.transitioned(5_000.0, "b", &mut out);
        // B joins C gaplessly (no plan) inside the window; a skip of C
        // is not B's mix's fault.
        log.arm(5_000.0, None, &mut out);
        log.transitioned(10_000.0, "c", &mut out);
        assert_eq!(outcomes(&out), vec![o("a", "b", Outcome::Mixed)]);
        log.left(12_000.0, &mut out);
        assert_eq!(out.len(), 1, "{out:?}");
    }

    #[test]
    fn a_mix_that_stands_is_mixed_once_the_window_passes() {
        let (mut log, mut out) = (MixLog::default(), vec![]);
        log.arm(0.0, Some(plan("a", "b")), &mut out);
        log.started(1_000.0, "a", "b", &mut out);
        log.ended(31_000.0, "a", &mut out);
        log.transitioned(31_000.0, "b", &mut out);
        // B→C armed at the handover; nothing to say yet.
        log.arm(31_000.0, Some(plan("b", "c")), &mut out);
        log.tick(31_000.0 + SKIP_WINDOW_MS, &mut out);
        assert!(out.is_empty(), "{out:?}");
        log.tick(31_001.0 + SKIP_WINDOW_MS, &mut out);
        assert_eq!(outcomes(&out), vec![o("a", "b", Outcome::Mixed)]);
        // Emitted once.
        log.tick(100_000.0, &mut out);
        assert_eq!(out.len(), 1);
    }

    #[test]
    fn leaving_during_the_overlap_or_soon_after_is_skipped() {
        let (mut log, mut out) = (MixLog::default(), vec![]);
        log.arm(0.0, Some(plan("a", "b")), &mut out);
        log.started(1_000.0, "a", "b", &mut out);
        log.left(5_000.0, &mut out);
        // The completed transition's reports change nothing.
        log.ended(5_100.0, "a", &mut out);
        log.transitioned(5_100.0, "b", &mut out);
        assert_eq!(
            outcomes(&out),
            vec![o(
                "a",
                "b",
                Outcome::Skipped {
                    after_handover_ms: None
                }
            )]
        );

        let (mut log, mut out) = (MixLog::default(), vec![]);
        log.arm(0.0, Some(plan("a", "b")), &mut out);
        log.started(1_000.0, "a", "b", &mut out);
        log.transitioned(31_000.0, "b", &mut out);
        log.left(43_000.0, &mut out);
        assert_eq!(
            outcomes(&out),
            vec![o(
                "a",
                "b",
                Outcome::Skipped {
                    after_handover_ms: Some(12_000)
                }
            )]
        );
    }

    #[test]
    fn a_plan_the_backend_never_started() {
        let (mut log, mut out) = (MixLog::default(), vec![]);
        log.arm(0.0, Some(plan("a", "b")), &mut out);
        log.ended(200_000.0, "a", &mut out);
        log.transitioned(200_000.0, "b", &mut out);
        assert_eq!(outcomes(&out), vec![o("a", "b", Outcome::NotStarted)]);

        // B taking over without an `Ended` for A (Media3 auto-advance).
        let (mut log, mut out) = (MixLog::default(), vec![]);
        log.arm(0.0, Some(plan("a", "b")), &mut out);
        log.transitioned(200_000.0, "b", &mut out);
        assert_eq!(outcomes(&out), vec![o("a", "b", Outcome::NotStarted)]);

        // A seek cancelled the transition and it never restarted.
        let (mut log, mut out) = (MixLog::default(), vec![]);
        log.arm(0.0, Some(plan("a", "b")), &mut out);
        log.started(1_000.0, "a", "b", &mut out);
        log.cancelled();
        log.ended(200_000.0, "a", &mut out);
        assert_eq!(outcomes(&out), vec![o("a", "b", Outcome::NotStarted)]);
    }

    #[test]
    fn a_changed_follow_up_is_replanned_but_a_new_plan_for_it_is_not() {
        let (mut log, mut out) = (MixLog::default(), vec![]);
        log.arm(0.0, Some(plan("a", "b")), &mut out);
        // A record arrived: the same pair, planned again.
        let mut better = plan("a", "b");
        better.kind = TransitionKind::TempoBlend;
        log.arm(1_000.0, Some(better), &mut out);
        assert!(out.is_empty());
        // Play Next put c in front.
        log.arm(2_000.0, Some(plan("a", "c")), &mut out);
        // And transitions went off.
        log.arm(3_000.0, None, &mut out);
        assert_eq!(
            outcomes(&out),
            vec![
                o("a", "b", Outcome::Replanned),
                o("a", "c", Outcome::Replanned)
            ]
        );
        assert_eq!(out[0].plan.kind, TransitionKind::TempoBlend);
    }

    #[test]
    fn leaving_before_the_tail_says_nothing() {
        let (mut log, mut out) = (MixLog::default(), vec![]);
        log.arm(0.0, Some(plan("a", "b")), &mut out);
        log.left(10_000.0, &mut out);
        log.arm(10_000.0, Some(plan("x", "y")), &mut out);
        assert!(out.is_empty(), "{out:?}");
    }

    #[test]
    fn failures_stops_and_short_tracks() {
        let (mut log, mut out) = (MixLog::default(), vec![]);
        log.arm(0.0, Some(plan("a", "b")), &mut out);
        log.started(1_000.0, "a", "b", &mut out);
        log.failed(2_000.0, "b", &mut out);
        assert_eq!(outcomes(&out), vec![o("a", "b", Outcome::Failed)]);

        let (mut log, mut out) = (MixLog::default(), vec![]);
        log.arm(0.0, Some(plan("a", "b")), &mut out);
        log.started(1_000.0, "a", "b", &mut out);
        log.unloaded(2_000.0, &mut out);
        assert_eq!(outcomes(&out), vec![o("a", "b", Outcome::Stopped)]);

        // B shorter than the window ends on its own: the mix stood.
        let (mut log, mut out) = (MixLog::default(), vec![]);
        log.arm(0.0, Some(plan("a", "b")), &mut out);
        log.started(1_000.0, "a", "b", &mut out);
        log.transitioned(5_000.0, "b", &mut out);
        log.ended(15_000.0, "b", &mut out);
        assert_eq!(outcomes(&out), vec![o("a", "b", Outcome::Mixed)]);
    }

    #[test]
    fn a_handover_seen_before_its_report() {
        // The core arms B→C before `TransitionedToNext(b)` arrives.
        let (mut log, mut out) = (MixLog::default(), vec![]);
        log.arm(0.0, Some(plan("a", "b")), &mut out);
        log.started(1_000.0, "a", "b", &mut out);
        log.arm(31_000.0, Some(plan("b", "c")), &mut out);
        log.transitioned(31_000.0, "b", &mut out);
        log.left(35_000.0, &mut out);
        assert_eq!(
            outcomes(&out),
            vec![o(
                "a",
                "b",
                Outcome::Skipped {
                    after_handover_ms: Some(4_000)
                }
            )]
        );
    }
}
