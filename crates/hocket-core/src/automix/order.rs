//! Mixability-aware order: when AutoMix is on and the order is Hocket's to
//! choose (shuffle, autoplay), which of the next few entries should follow a
//! track. Pure; the actor (`core::handlers::mix_order`) gathers the records
//! and applies the choice as a queue op. `docs/automix.md` → *Order*.
//!
//! The shuffle (or autoplay) order stays the source of randomness: only the
//! first [`ORDER_WINDOW`] entries of it are candidates, so a pick moves at
//! most `ORDER_WINDOW − 1` places forward, and the entry shuffle put first
//! keeps its place unless another candidate scores clearly better
//! ([`PROMOTE_MARGIN`]). An entry passed over [`MAX_PASSES`] times plays
//! next whatever the scores, so a track that mixes badly with everything is
//! delayed, never starved.

use crate::api::{AnalysisRecord, Ms, TransitionKind, TransitionPlan};

use super::key::{harmony, Harmony};
use super::plan::{voice_span, Voices};

/// Candidates considered: the next entries in shuffle order.
pub const ORDER_WINDOW: usize = 6;
/// A candidate replaces the one shuffle put first only when it scores at
/// least this much higher.
pub const PROMOTE_MARGIN: f64 = 0.15;
/// After being passed over this many times an entry is no longer passed over.
pub const MAX_PASSES: u8 = 3;
/// How long the current track plays before the follow-up of the next one is
/// chosen, so a burst of skips never moves anything. Shorter on a track whose
/// tail locks in sooner (`core::handlers::mix_order`).
pub const SETTLE_MS: Ms = 15_000;
/// Score of a candidate without a usable record: about what a random pair
/// scores, so unanalysed tracks are neither preferred nor pushed back.
pub const UNKNOWN_SCORE: f64 = 0.3;

/// How well `plan` (A into B, `None`: they aren't mixed) is likely to sound,
/// 0..1: beat-matched blends first (longer and less stretched is better),
/// then beat-aligned and plain fades; one voice at a time and Camelot
/// neighbours add, a voice clash or a key clash subtracts.
pub fn mix_score(
    plan: Option<&TransitionPlan>,
    ra: Option<&AnalysisRecord>,
    rb: Option<&AnalysisRecord>,
) -> f64 {
    let Some(plan) = plan else {
        return 0.0;
    };
    let overlap = f64::from(plan.tail_len_ms.saturating_sub(plan.incoming_at_ms));
    let blend = matches!(
        plan.kind,
        TransitionKind::Blend | TransitionKind::TempoBlend
    );
    let mut score = if blend {
        let stretch = plan
            .tempo
            .map(|t| (f64::from(t.ratio_end) - 1.0).abs() / 0.08)
            .unwrap_or(0.0)
            .min(1.0);
        0.6 + 0.2 * (overlap / 16_000.0).min(1.0) - 0.1 * stretch
    } else if plan.incoming_from_ms > 0 || plan.incoming_at_ms > 0 {
        // A fade on the beat (B from a downbeat, or onto A's).
        0.3
    } else {
        0.15
    };
    let (Some(ra), Some(rb)) = (ra, rb) else {
        return score.clamp(0.0, 1.0);
    };
    if let Some(v) = Voices::of(ra, rb) {
        // A voice clash (a shaped blend, `plan::shape_clash`) is worth
        // about a beat-aligned fade: not something to reorder for.
        score += if voice_span(plan, &v).is_clean() {
            0.1
        } else {
            -0.35
        };
    }
    // Keys matter most while both tracks are loud together: in a blend.
    let weight = if blend { 1.0 } else { 0.5 };
    score += weight
        * match harmony(ra, rb) {
            Harmony::Compatible => 0.1,
            Harmony::Unknown => 0.03,
            Harmony::Near => 0.0,
            Harmony::Clash => -0.15,
        };
    score.clamp(0.0, 1.0)
}

/// Which candidate should play next: an index into `scores` (the window in
/// shuffle order; `None`: no record to judge by). `front_passes` is how many
/// times `scores[0]`'s entry has already been passed over.
pub fn choose(scores: &[Option<f64>], front_passes: u8) -> usize {
    if scores.len() < 2 || front_passes >= MAX_PASSES {
        return 0;
    }
    let s = |i: usize| scores[i].unwrap_or(UNKNOWN_SCORE);
    // The best, earliest on a tie (closest to the shuffle order).
    let best = (1..scores.len()).fold(0, |b, i| if s(i) > s(b) { i } else { b });
    if best != 0 && s(best) >= s(0) + PROMOTE_MARGIN {
        best
    } else {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::{FadeCurve, MusicalKey, TempoRamp};

    fn plan(kind: TransitionKind, overlap: u32) -> TransitionPlan {
        TransitionPlan {
            kind,
            tail_start_ms: 200_000,
            tail_source_ms: overlap,
            tail_len_ms: overlap,
            incoming_at_ms: 0,
            incoming_from_ms: 0,
            handover_ms: overlap / 2,
            out_curve: FadeCurve::EqualPower,
            in_curve: FadeCurve::EqualPower,
            tempo: None,
            bass_swap: None,
            high_cut: None,
            incoming_gain_db: 0.0,
        }
    }

    #[test]
    fn blends_outscore_fades() {
        let fade = mix_score(Some(&plan(TransitionKind::Fade, 6_000)), None, None);
        let short = mix_score(Some(&plan(TransitionKind::Blend, 4_000)), None, None);
        let long = mix_score(Some(&plan(TransitionKind::Blend, 16_000)), None, None);
        let mut stretched = plan(TransitionKind::TempoBlend, 16_000);
        stretched.tempo = Some(TempoRamp {
            ratio_end: 1.06,
            ramp_ms: 8_000,
        });
        let stretched = mix_score(Some(&stretched), None, None);
        assert!(fade < short && short < stretched && stretched < long);
        assert_eq!(mix_score(None, None, None), 0.0);
    }

    #[test]
    fn keys_move_the_score() {
        let key = |tonic| MusicalKey {
            tonic,
            minor: false,
            strength: 0.8,
        };
        let a = AnalysisRecord {
            key: Some(key(0)),
            ..Default::default()
        };
        let mut b = a.clone();
        let p = plan(TransitionKind::Blend, 16_000);
        let same = mix_score(Some(&p), Some(&a), Some(&b));
        b.key = Some(key(1));
        let clash = mix_score(Some(&p), Some(&a), Some(&b));
        b.key = None;
        let unknown = mix_score(Some(&p), Some(&a), Some(&b));
        assert!(
            clash < unknown && unknown < same,
            "{clash} {unknown} {same}"
        );
    }

    #[test]
    fn choose_keeps_shuffle_order_unless_clearly_better() {
        assert_eq!(choose(&[], 0), 0);
        assert_eq!(choose(&[Some(0.1)], 0), 0);
        assert_eq!(choose(&[Some(0.5), Some(0.6), Some(0.55)], 0), 0);
        assert_eq!(choose(&[Some(0.2), Some(0.5), Some(0.9), Some(0.9)], 0), 2);
        // Unknowns count as a typical pair.
        assert_eq!(choose(&[None, Some(0.4)], 0), 0);
        assert_eq!(choose(&[None, Some(0.8)], 0), 1);
        assert_eq!(choose(&[Some(0.0), None], 0), 1);
        // Passed over enough: it plays.
        assert_eq!(choose(&[Some(0.0), Some(0.9)], MAX_PASSES), 0);
    }
}
