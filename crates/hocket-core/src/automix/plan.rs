//! The planner: two tracks (their lengths and usable analysis records), the
//! transition settings and an optional fixed tail start in, one
//! [`TransitionPlan`] out. Deterministic: integer milliseconds and plain
//! `f64` arithmetic, so every device computes the same plan.
//!
//! Cases (`docs/automix.md` → *Planner*):
//!
//! 1. Crossfade mode, or AutoMix without a record for A: a fixed equal-power
//!    fade ending at A's audible end ([`fixed_fade`]).
//! 2. AutoMix with a loudness-only record for A (or beats that don't pass
//!    the gate): a fade placed where A's tail drops 6 LU below its sustained
//!    level ([`smart_fade`]).
//! 3. AutoMix with a trustworthy beat grid on both: `Blend`/`TempoBlend`
//!    over 8 or 16 bars with a bass swap when both are 4/4 within 8 %, else a
//!    beat-aligned `Fade` of 4/2/1 bars ([`beat_plan`]). With voice curves
//!    on both, one voice at a time ([`vocal_blend`], [`vocal_fade`]): the
//!    usual length with the handover between A's last voice and B's first,
//!    else a blend short enough for the instrumental gap, else a short blend
//!    with an early bass swap and a presence cut on A.
//! 4. Rescue: whatever doesn't fit falls back to a plain 6 s fade.
//!
//! The caller has already decided to mix ([`super::rules::should_mix`]) and
//! filtered records to usable ones ([`super::record::is_usable`]).

use crate::api::{
    AnalysisRecord, FadeCurve, LoudnessCurve, Ms, ShelfMove, TempoRamp, TransitionKind,
    TransitionMode, TransitionPlan, TransitionSettings, VocalCurve,
};

use super::record::{has_beats, LUFS_FLOOR};
use super::stretch::source_ms;
use super::AUTOMIX_DEFAULT_FADE_MS;

/// Beat grids below this confidence are ignored.
pub const MIN_BEAT_CONFIDENCE: f32 = 0.5;
/// Downbeats a grid needs in the region that matters (A's last minute, B's first).
pub const MIN_DOWNBEATS: usize = 8;
/// The region of each track the grid is judged on.
pub const GRID_REGION_MS: u32 = 60_000;
/// A bar counts as steady when within this fraction of the median bar.
pub const BAR_TOLERANCE: f64 = 0.04;
/// Share of steady bars a grid needs.
pub const MIN_STEADY_SHARE: f64 = 0.85;
/// Largest tempo gap a blend stretches over.
pub const MAX_STRETCH_GAP: f64 = 0.08;
/// Accumulated drift over the overlap below which a blend needs no stretch.
pub const MAX_UNSTRETCHED_DRIFT_MS: f64 = 25.0;
/// Bars of tempo ramp before a `TempoBlend` overlap.
pub const RAMP_BARS: usize = 4;
/// Never skip more than this much audible A.
pub const MAX_CUT_MS: u32 = 20_000;
/// Smart fade: the overlap's range, and how far below the sustained level the fade-out starts.
pub const SMART_MIN_OVERLAP_MS: u32 = 4_000;
pub const SMART_MAX_OVERLAP_MS: u32 = 10_000;
pub const SMART_DROP_LU: f32 = 6.0;
/// The low-energy test for 16-bar blends: the ends sit this far below the track's integrated loudness.
pub const LOW_ENERGY_LU: f32 = 4.0;
/// Bass-swap shelf.
pub const BASS_SWAP_HZ: f32 = 100.0;
pub const BASS_SWAP_DB: f32 = -26.0;
/// High-shelf cut on the outgoing tail after the handover.
pub const HIGH_CUT_HZ: f32 = 5_000.0;
pub const HIGH_CUT_DB: f32 = -12.0;
/// Longest overlap a 16-bar blend may take.
pub const MAX_LONG_BLEND_MS: f64 = 32_000.0;
/// Beat-aligned fades are at least this long (whole bars of A added until
/// they are): a 1-bar fade at 128 BPM is under 2 s, a near-cut.
pub const MIN_ALIGNED_FADE_MS: f64 = 4_000.0;
/// Late-edit fades are at least this long.
pub const LATE_EDIT_MIN_MS: u32 = 2_000;

/// One side of a transition.
#[derive(Debug, Clone, Copy, Default)]
pub struct PlanTrack<'a> {
    pub duration_ms: Ms,
    /// A usable record (already checked against the track's fingerprint).
    pub record: Option<&'a AnalysisRecord>,
}

impl PlanTrack<'_> {
    /// Last audible position (record) or the end.
    fn audible_end(&self) -> Ms {
        match self.record {
            Some(r) if r.audible_end_ms > 0 => r.audible_end_ms.min(self.duration_ms),
            _ => self.duration_ms,
        }
    }

    /// First audible position (record) or 0.
    fn audible_start(&self) -> Ms {
        match self.record {
            Some(r) => r.audible_start_ms.min(self.audible_end()),
            None => 0,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct PlanInput<'a> {
    pub settings: &'a TransitionSettings,
    /// Outgoing.
    pub a: PlanTrack<'a>,
    /// Incoming.
    pub b: PlanTrack<'a>,
    /// A tail start already committed (`MediaSource::mix_out_ms`): the plan keeps it.
    pub fixed_tail_start_ms: Option<Ms>,
}

/// Plan the transition, `None` when the mode is Off.
pub fn plan_transition(input: &PlanInput<'_>) -> Option<TransitionPlan> {
    let plan = match input.settings.mode {
        TransitionMode::Off => return None,
        TransitionMode::Crossfade => fixed_fade(input, input.settings.crossfade_ms()),
        TransitionMode::AutoMix => automix(input),
    };
    Some(plan)
}

fn automix(input: &PlanInput<'_>) -> TransitionPlan {
    let Some(ra) = input.a.record else {
        return fixed_fade(input, AUTOMIX_DEFAULT_FADE_MS);
    };
    if let Some(rb) = input.b.record {
        if let (Some(ga), Some(gb)) = (
            Grid::of(ra, Region::Tail(input.a.audible_end())),
            Grid::of(rb, Region::Head(input.b.audible_start())),
        ) {
            if let Some(plan) = beat_plan(input, ra, rb, &ga, &gb) {
                return plan;
            }
            return fixed_fade(input, AUTOMIX_DEFAULT_FADE_MS);
        }
    }
    smart_fade(input, ra).unwrap_or_else(|| fixed_fade(input, AUTOMIX_DEFAULT_FADE_MS))
}

/// A plain `Fade` of `overlap_ms` starting at `tail_start` (B from its audible start).
fn fade_at(input: &PlanInput<'_>, tail_start: Ms, overlap_ms: Ms) -> TransitionPlan {
    let tail_start = tail_start.min(input.a.duration_ms);
    let b_from = input.b.audible_start();
    TransitionPlan {
        kind: TransitionKind::Fade,
        tail_start_ms: tail_start,
        tail_source_ms: overlap_ms.min(input.a.duration_ms - tail_start),
        tail_len_ms: overlap_ms,
        incoming_at_ms: 0,
        incoming_from_ms: b_from,
        handover_ms: overlap_ms / 2,
        out_curve: FadeCurve::EqualPower,
        in_curve: FadeCurve::EqualPower,
        tempo: None,
        bass_swap: None,
        high_cut: None,
        incoming_gain_db: 0.0,
    }
}

/// `overlap` limited to half of each track's audible span, so a short track
/// is never all overlap.
fn fit_overlap(input: &PlanInput<'_>, overlap: Ms) -> Ms {
    let a_span = input.a.audible_end() - input.a.audible_start();
    let b_span = input.b.duration_ms.saturating_sub(input.b.audible_start());
    overlap.min(a_span / 2).min(b_span / 2)
}

/// An overlap taking `a_ms` of A and `b_ms` of B stays within half of each
/// track's audible span (the limit [`fit_overlap`] puts on fades). Beat plans
/// can't shorten an overlap off the grid, so they drop a bar count that
/// breaks it.
fn overlap_fits(input: &PlanInput<'_>, a_ms: Ms, b_ms: Ms) -> bool {
    fit_overlap(input, a_ms) == a_ms && fit_overlap(input, b_ms) == b_ms
}

/// Case 1 (and the rescue): an equal-power fade of `overlap_ms` ending at A's
/// audible end, or starting at the fixed tail start.
pub fn fixed_fade(input: &PlanInput<'_>, overlap_ms: Ms) -> TransitionPlan {
    let overlap = fit_overlap(input, overlap_ms);
    let end = input.a.audible_end();
    match input.fixed_tail_start_ms {
        Some(fixed) => {
            // Fade over what's left of A (at most the fit), but at least a second.
            let left = end.saturating_sub(fixed);
            fade_at(input, fixed, overlap.min(left).max(1_000.min(overlap_ms)))
        }
        None => fade_at(input, end - overlap, overlap),
    }
}

/// The plan for a change to what plays next after lock-in: a short `Fade`
/// from the committed tail start (max(2 s, crossfade / 2)).
pub fn late_edit_fade(
    tail_start_ms: Ms,
    a: PlanTrack<'_>,
    b: PlanTrack<'_>,
    settings: &TransitionSettings,
) -> TransitionPlan {
    let input = PlanInput {
        settings,
        a,
        b,
        fixed_tail_start_ms: Some(tail_start_ms),
    };
    let overlap = (settings.crossfade_ms() / 2).max(LATE_EDIT_MIN_MS);
    let b_span = b.duration_ms.saturating_sub(b.audible_start());
    fade_at(&input, tail_start_ms, overlap.min((b_span / 2).max(1)))
}

// ---------------------------------------------------------------------------
// Case 2: loudness-placed fade
// ---------------------------------------------------------------------------

fn median(mut v: Vec<f32>) -> Option<f32> {
    if v.is_empty() {
        return None;
    }
    v.sort_by(|a, b| a.total_cmp(b));
    Some(v[v.len() / 2])
}

/// Points of `curve` at or before `end` as (time, lufs).
fn points(curve: &LoudnessCurve, end: Ms) -> impl Iterator<Item = (Ms, f32)> + '_ {
    curve
        .lufs
        .iter()
        .enumerate()
        .map(move |(i, &v)| {
            // A record is outside data: a huge hop or start must not overflow.
            let step = u32::try_from(i)
                .unwrap_or(u32::MAX)
                .saturating_mul(curve.hop_ms);
            (curve.start_ms.saturating_add(step), v)
        })
        .filter(move |&(t, _)| t <= end)
}

/// Where A's tail falls `SMART_DROP_LU` below its sustained level for good
/// (`None` when the curve says nothing).
pub fn fade_out_start(curve: &LoudnessCurve, audible_end: Ms) -> Option<Ms> {
    if curve.hop_ms == 0 {
        return None;
    }
    let audible: Vec<(Ms, f32)> = points(curve, audible_end)
        .filter(|&(_, v)| v > LUFS_FLOOR + 1.0)
        .collect();
    let body: Vec<f32> = audible
        .iter()
        .filter(|&&(t, _)| t.saturating_add(MAX_CUT_MS) <= audible_end)
        .map(|&(_, v)| v)
        .collect();
    let sustained = if body.len() >= 10 {
        median(body)?
    } else {
        median(audible.iter().map(|&(_, v)| v).collect())?
    };
    let threshold = sustained - SMART_DROP_LU;
    let last_loud = points(curve, audible_end)
        .filter(|&(_, v)| v >= threshold)
        .map(|(t, _)| t)
        .last();
    Some(match last_loud {
        Some(t) => t.saturating_add(curve.hop_ms).min(audible_end),
        None => curve.start_ms.min(audible_end),
    })
}

/// Case 2. `None` when A's curve can't place it.
pub fn smart_fade(input: &PlanInput<'_>, ra: &AnalysisRecord) -> Option<TransitionPlan> {
    let end = input.a.audible_end();
    let fit = |overlap: Ms| fit_overlap(input, overlap);
    if let Some(fixed) = input.fixed_tail_start_ms {
        let overlap = end
            .saturating_sub(fixed)
            .clamp(SMART_MIN_OVERLAP_MS, SMART_MAX_OVERLAP_MS);
        return Some(fade_at(input, fixed, fit(overlap)));
    }
    let mix_out = fade_out_start(&ra.tail, end)?;
    let fade_len = end - mix_out.min(end);
    let overlap = fit(fade_len.clamp(SMART_MIN_OVERLAP_MS, SMART_MAX_OVERLAP_MS));
    let latest = end - overlap;
    let earliest = latest.saturating_sub(MAX_CUT_MS);
    let tail_start = mix_out.clamp(earliest, latest);
    Some(fade_at(input, tail_start, overlap))
}

// ---------------------------------------------------------------------------
// Beat grids
// ---------------------------------------------------------------------------

/// Which end of a track a grid is judged on.
#[derive(Debug, Clone, Copy)]
pub enum Region {
    /// The minute before this position (A's audible end).
    Tail(Ms),
    /// The minute after this position (B's audible start).
    Head(Ms),
}

impl Region {
    fn range(self) -> (Ms, Ms) {
        match self {
            Region::Tail(end) => (end.saturating_sub(GRID_REGION_MS), end),
            Region::Head(start) => (start, start.saturating_add(GRID_REGION_MS)),
        }
    }
}

/// A trustworthy beat grid near one end of a track.
#[derive(Debug, Clone, PartialEq)]
pub struct Grid {
    /// Median bar length (ms).
    pub bar_ms: f64,
    pub beats_per_bar: u32,
    pub bpm: f64,
}

impl Grid {
    /// The grid of `r` in `region`, `None` unless confident, dense and steady.
    pub fn of(r: &AnalysisRecord, region: Region) -> Option<Grid> {
        if !has_beats(r) || r.beat_confidence < MIN_BEAT_CONFIDENCE {
            return None;
        }
        let (lo, hi) = region.range();
        let downbeats: Vec<Ms> = r
            .downbeats_ms
            .iter()
            .copied()
            .filter(|&d| d >= lo && d <= hi)
            .collect();
        if downbeats.len() < MIN_DOWNBEATS {
            return None;
        }
        let bar_ms = bar_length(&downbeats)?;
        if grid_steadiness(&downbeats, bar_ms) < MIN_STEADY_SHARE {
            return None;
        }
        let beats_per_bar = beats_per_bar(&r.beats_ms, &downbeats)?;
        Some(Grid {
            bar_ms,
            beats_per_bar,
            bpm: tempo_bpm(bar_ms, beats_per_bar),
        })
    }

    pub fn is_four_four(&self) -> bool {
        self.beats_per_bar == 4
    }
}

/// Median interval between consecutive downbeats.
pub fn bar_length(downbeats: &[Ms]) -> Option<f64> {
    let mut iv: Vec<u32> = downbeats
        .windows(2)
        .map(|w| w[1].saturating_sub(w[0]))
        .collect();
    if iv.is_empty() {
        return None;
    }
    iv.sort_unstable();
    let m = iv[iv.len() / 2] as f64;
    (m > 0.0).then_some(m)
}

/// Relative tempo gap between two tempos, half and double time counting as
/// the same tempo.
pub fn tempo_gap(a_bpm: f64, b_bpm: f64) -> f64 {
    [0.5, 1.0, 2.0]
        .iter()
        .map(|f| (b_bpm * f / a_bpm - 1.0).abs())
        .fold(f64::INFINITY, f64::min)
}

/// Tempo from a bar length and the beats in a bar.
pub fn tempo_bpm(bar_ms: f64, beats_per_bar: u32) -> f64 {
    60_000.0 * beats_per_bar as f64 / bar_ms
}

/// Share of downbeat intervals within [`BAR_TOLERANCE`] of `bar_ms`.
pub fn grid_steadiness(downbeats: &[Ms], bar_ms: f64) -> f64 {
    let iv: Vec<f64> = downbeats
        .windows(2)
        .map(|w| w[1].saturating_sub(w[0]) as f64)
        .collect();
    if iv.is_empty() || bar_ms <= 0.0 {
        return 0.0;
    }
    let steady = iv
        .iter()
        .filter(|&&x| ((x - bar_ms) / bar_ms).abs() <= BAR_TOLERANCE)
        .count();
    steady as f64 / iv.len() as f64
}

/// The usual number of beats between consecutive downbeats (the mode, which
/// must cover at least 80 % of bars), `None` when bars disagree.
pub fn beats_per_bar(beats: &[Ms], downbeats: &[Ms]) -> Option<u32> {
    let mut counts = [0usize; 17];
    let mut bars = 0usize;
    for w in downbeats.windows(2) {
        let lo = beats.partition_point(|&b| b < w[0]);
        let hi = beats.partition_point(|&b| b < w[1]);
        let n = (hi - lo).min(16);
        counts[n] += 1;
        bars += 1;
    }
    let (n, &c) = counts
        .iter()
        .enumerate()
        .skip(1)
        .max_by_key(|&(n, &c)| (c, usize::MAX - n))?;
    (bars > 0 && c * 5 >= bars * 4).then_some(n as u32)
}

/// Index of the first downbeat at or after `ms`.
pub fn downbeat_at_or_after(downbeats: &[Ms], ms: Ms) -> Option<usize> {
    let i = downbeats.partition_point(|&d| d < ms);
    (i < downbeats.len()).then_some(i)
}

/// Index of the downbeat nearest `ms` within `tolerance_ms`.
pub fn downbeat_near(downbeats: &[Ms], ms: Ms, tolerance_ms: Ms) -> Option<usize> {
    let i = downbeats.partition_point(|&d| d < ms);
    [i.checked_sub(1), Some(i)]
        .into_iter()
        .flatten()
        .filter(|&j| j < downbeats.len())
        .min_by_key(|&j| downbeats[j].abs_diff(ms))
        .filter(|&j| downbeats[j].abs_diff(ms) <= tolerance_ms)
}

/// Mean of `curve` over `[from, to]`, `None` without points there.
fn mean_over(curve: &LoudnessCurve, from: Ms, to: Ms) -> Option<f32> {
    let v: Vec<f32> = points(curve, to)
        .filter(|&(t, _)| t >= from)
        .map(|(_, v)| v)
        .collect();
    (!v.is_empty()).then(|| v.iter().sum::<f32>() / v.len() as f32)
}

/// Both A's last and B's first 30 s are quiet relative to each track.
fn both_low_energy(ra: &AnalysisRecord, a_end: Ms, rb: &AnalysisRecord, b_start: Ms) -> bool {
    let low = |r: &AnalysisRecord, curve: &LoudnessCurve, from: Ms, to: Ms| match (
        r.integrated_lufs,
        mean_over(curve, from, to),
    ) {
        (Some(i), Some(m)) => m <= i - LOW_ENERGY_LU,
        _ => false,
    };
    low(ra, &ra.tail, a_end.saturating_sub(30_000), a_end)
        && low(rb, &rb.head, b_start, b_start.saturating_add(30_000))
}

// ---------------------------------------------------------------------------
// Vocals: one voice at a time
// ---------------------------------------------------------------------------

/// A voice curve point at or above this counts as singing or rapping.
pub const VOICE_THRESHOLD: f32 = 0.5;
/// A's last voice may run this far past B's first (output time) and still
/// count as one voice at a time: the fades make the overlap's ends lopsided.
pub const VOICE_TOLERANCE_MS: Ms = 500;
/// Bar counts a blend shortens to so its overlap fits the instrumental gap
/// (after the usual 16 or 8).
pub const SHORT_BLEND_BARS: &[usize] = &[4, 2];
/// With voices all the way through, the blend is the fewest bars at least
/// this long.
pub const CLASH_MIN_OVERLAP_MS: f64 = 4_000.0;
/// High-shelf cut on A when voices can't be kept apart: low enough to take
/// the presence out of A's voice.
pub const CLASH_HIGH_CUT_HZ: f32 = 2_000.0;

/// A's tail and B's head voice curves, when both records have them.
#[derive(Debug, Clone, Copy)]
pub struct Voices<'a> {
    pub a: &'a VocalCurve,
    pub b: &'a VocalCurve,
}

impl<'a> Voices<'a> {
    pub fn of(ra: &'a AnalysisRecord, rb: &'a AnalysisRecord) -> Option<Self> {
        let usable = |c: &VocalCurve| c.hop_ms > 0 && !c.voice.is_empty();
        (usable(&ra.tail_vocals) && usable(&rb.head_vocals)).then_some(Voices {
            a: &ra.tail_vocals,
            b: &rb.head_vocals,
        })
    }
}

/// Whether `curve` has a voice at track position `ms` (`false` outside it).
pub fn voiced(curve: &VocalCurve, ms: Ms) -> bool {
    if curve.hop_ms == 0 || ms < curve.start_ms {
        return false;
    }
    let i = ((ms - curve.start_ms) / curve.hop_ms) as usize;
    curve.voice.get(i).is_some_and(|&v| v >= VOICE_THRESHOLD)
}

/// Where voices fall in a plan's overlap, in output time from the tail start.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VoiceSpan {
    /// A's last voiced point.
    pub a_last: Option<Ms>,
    /// B's first voiced point.
    pub b_first: Option<Ms>,
}

impl VoiceSpan {
    /// One voice at a time: A's voice ends before B's starts.
    pub fn is_clean(&self) -> bool {
        match (self.a_last, self.b_first) {
            (Some(a), Some(b)) => a <= b.saturating_add(VOICE_TOLERANCE_MS),
            _ => true,
        }
    }
}

/// Voices over `plan`'s overlap, sampled every curve hop of output time: A
/// through the tempo ramp, B from `incoming_from_ms`.
pub fn voice_span(plan: &TransitionPlan, voices: &Voices<'_>) -> VoiceSpan {
    let step = voices.a.hop_ms.min(voices.b.hop_ms).max(1);
    let mut span = VoiceSpan {
        a_last: None,
        b_first: None,
    };
    let mut t = plan.incoming_at_ms;
    while t < plan.tail_len_ms {
        let a_pos = plan.tail_start_ms + source_ms(plan.tempo.as_ref(), t as f64).round() as Ms;
        if voiced(voices.a, a_pos) {
            span.a_last = Some(t);
        }
        if span.b_first.is_none() && voiced(voices.b, plan.incoming_position_at(t)) {
            span.b_first = Some(t);
        }
        t += step;
    }
    span
}

/// The handover of a clean blend: mid-overlap when that falls between A's
/// last voice and B's first, else B's downbeat in that window nearest the
/// middle (so B's voice comes in at or after it), else the window's edge.
fn clean_handover(plan: &TransitionPlan, span: VoiceSpan, db: &[Ms]) -> Ms {
    let lo = span
        .a_last
        .unwrap_or(plan.incoming_at_ms)
        .max(plan.incoming_at_ms);
    let hi = span
        .b_first
        .unwrap_or(plan.tail_len_ms)
        .min(plan.tail_len_ms);
    let mid = plan.handover_ms;
    if lo > hi || (lo..=hi).contains(&mid) {
        return mid.clamp(lo.min(hi), hi);
    }
    let from = plan.incoming_from_ms;
    db.iter()
        .filter(|&&d| d >= from)
        .map(|&d| plan.incoming_at_ms + (d - from))
        .filter(|t| (lo..=hi).contains(t))
        .min_by_key(|t| t.abs_diff(mid))
        .unwrap_or(mid.clamp(lo, hi))
}

/// Move a blend's handover to `h`: the bass swap over the bar before it (not
/// before B enters), the high cut from it.
fn move_handover(plan: &mut TransitionPlan, h: Ms) {
    plan.handover_ms = h;
    if let Some(swap) = plan.bass_swap.as_mut() {
        let start = h.saturating_sub(swap.len_ms).max(plan.incoming_at_ms);
        swap.start_ms = start;
        swap.len_ms = h - start;
    }
    if let Some(cut) = plan.high_cut.as_mut() {
        cut.start_ms = h;
        cut.len_ms = plan.tail_len_ms - h;
    }
}

/// Voices can't be kept apart: B's bass in by the middle of the way to the
/// handover, and A's presence cut from B's entry to the handover.
fn shape_clash(plan: &mut TransitionPlan) {
    let entry = plan.incoming_at_ms;
    let to_handover = plan.handover_ms - entry;
    if let Some(swap) = plan.bass_swap.as_mut() {
        swap.start_ms = entry;
        swap.len_ms = to_handover / 2;
    }
    if let Some(cut) = plan.high_cut.as_mut() {
        cut.freq_hz = CLASH_HIGH_CUT_HZ;
        cut.start_ms = entry;
        cut.len_ms = to_handover;
    }
}

/// The blend with voices known, in order of preference: the usual length
/// when its voices don't overlap (handover between them); a shorter blend
/// that fits the instrumental gap; else the shortest blend of at least
/// [`CLASH_MIN_OVERLAP_MS`], shaped by [`shape_clash`].
fn vocal_blend(
    options: &[usize],
    bar_ms: f64,
    db: &[Ms],
    voices: &Voices<'_>,
    blend_at: impl Fn(usize) -> Option<TransitionPlan>,
) -> Option<TransitionPlan> {
    let shortest = options.iter().copied().min().unwrap_or(8);
    let tries = options
        .iter()
        .copied()
        .chain(SHORT_BLEND_BARS.iter().copied().filter(|&b| b < shortest));
    for bars in tries {
        let Some(mut plan) = blend_at(bars) else {
            continue;
        };
        let span = voice_span(&plan, voices);
        if span.is_clean() {
            let h = clean_handover(&plan, span, db);
            move_handover(&mut plan, h);
            return Some(plan);
        }
    }
    let mut bars = 2;
    while (bars as f64) * bar_ms < CLASH_MIN_OVERLAP_MS && bars < shortest {
        bars += 1;
    }
    let mut plan = blend_at(bars)?;
    shape_clash(&mut plan);
    Some(plan)
}

/// The beat-aligned fade with voices known: `bars` when its voices don't
/// overlap, else fewer bars (down to [`MIN_ALIGNED_FADE_MS`]) that keep them
/// apart, else the shortest.
fn vocal_fade(
    bars: usize,
    bar_ms: f64,
    voices: &Voices<'_>,
    fade_at: impl Fn(usize) -> Option<TransitionPlan>,
) -> Option<TransitionPlan> {
    let mut shortest = bars;
    while shortest > 1 && ((shortest - 1) as f64) * bar_ms >= MIN_ALIGNED_FADE_MS {
        shortest -= 1;
    }
    (shortest..=bars)
        .rev()
        .filter_map(&fade_at)
        .find(|p| voice_span(p, voices).is_clean())
        .or_else(|| fade_at(shortest))
}

// ---------------------------------------------------------------------------
// Case 3: beat-matched plans
// ---------------------------------------------------------------------------

/// Where an overlap of `bars` bars sits in A: the index of its first
/// downbeat, keeping `pre` bars before it (the ramp) and ending at or before
/// A's audible end. With a fixed tail start, the first downbeat at or after
/// it plus `pre`; otherwise the latest that fits.
fn place_in_a(da: &[Ms], fixed: Option<Ms>, end: Ms, bars: usize, pre: usize) -> Option<usize> {
    let o = match fixed {
        Some(f) => downbeat_at_or_after(da, f)? + pre,
        None => {
            let last_end = da.partition_point(|&d| d <= end).checked_sub(1)?;
            let o = last_end.checked_sub(bars)?;
            if o < pre {
                return None;
            }
            o
        }
    };
    (o + bars < da.len() && da[o + bars] <= end).then_some(o)
}

/// Case 3 (`None`: nothing fits; the caller rescues).
pub fn beat_plan(
    input: &PlanInput<'_>,
    ra: &AnalysisRecord,
    rb: &AnalysisRecord,
    ga: &Grid,
    gb: &Grid,
) -> Option<TransitionPlan> {
    let a_end = input.a.audible_end();
    let b_start = input.b.audible_start();
    let da = &ra.downbeats_ms;
    let db = &rb.downbeats_ms;
    let k = downbeat_at_or_after(db, b_start)?;
    let gap = (gb.bpm / ga.bpm - 1.0).abs();
    let fixed = input.fixed_tail_start_ms;

    let plan = if ga.is_four_four() && gb.is_four_four() && gap <= MAX_STRETCH_GAP {
        // 16 bars only while they stay within 32 s (≥ 120 BPM in 4/4): at
        // 90 BPM they are 43 s, plus the ramp a minute of A.
        let long = both_low_energy(ra, a_end, rb, b_start) && 16.0 * ga.bar_ms <= MAX_LONG_BLEND_MS;
        let options: &[usize] = if long { &[16, 8] } else { &[8] };
        let blend_at = |bars| blend(input, da, db, k, bars, fixed, a_end);
        match Voices::of(ra, rb) {
            Some(v) => vocal_blend(options, ga.bar_ms, db, &v, blend_at),
            None => options.iter().find_map(|&bars| blend_at(bars)),
        }
    } else {
        // Half or double time lines up on every beat of the slower track, so
        // the fade's length goes by the gap counted modulo an octave.
        let gap = tempo_gap(ga.bpm, gb.bpm);
        let mut bars = if gap > 0.20 {
            1
        } else if gap > 0.12 {
            2
        } else {
            4
        };
        while (bars as f64) * ga.bar_ms < MIN_ALIGNED_FADE_MS {
            bars += 1;
        }
        let fade_at = |bars| aligned_fade(input, da, db, k, bars, fixed, a_end);
        match Voices::of(ra, rb) {
            Some(v) => vocal_fade(bars, ga.bar_ms, &v, fade_at),
            None => fade_at(bars),
        }
    }?;
    // Never skip more than MAX_CUT_MS of audible A.
    let overlap_end_in_a = plan.tail_start_ms + plan.tail_source_ms;
    (a_end.saturating_sub(overlap_end_in_a) <= MAX_CUT_MS).then_some(plan)
}

/// `Blend` or `TempoBlend` over `bars` bars.
fn blend(
    input: &PlanInput<'_>,
    da: &[Ms],
    db: &[Ms],
    k: usize,
    bars: usize,
    fixed: Option<Ms>,
    a_end: Ms,
) -> Option<TransitionPlan> {
    if k + bars >= db.len() {
        return None;
    }
    let b_bar = (db[k + bars] - db[k]) as f64 / bars as f64;
    // Decide stretch or not from an unramped placement first.
    let o0 = place_in_a(da, fixed, a_end, bars, 0)?;
    let a_bar0 = (da[o0 + bars] - da[o0]) as f64 / bars as f64;
    let drift = (a_bar0 - b_bar).abs() * bars as f64;
    let stretched = drift > MAX_UNSTRETCHED_DRIFT_MS;
    let pre = if stretched { RAMP_BARS } else { 0 };
    let o = if stretched {
        place_in_a(da, fixed, a_end, bars, pre)?
    } else {
        o0
    };
    let tail_start = match fixed {
        Some(f) => f,
        None => da[o - pre],
    };
    let src_lead = (da[o] - tail_start) as f64;
    let src_overlap = (da[o + bars] - da[o]) as f64;
    let (tempo, incoming_at, overlap_out) = if stretched {
        // The ratio is stored as f32: compute from the stored value so planner and renderer agree.
        let r = ((src_overlap / bars as f64) / b_bar) as f32 as f64;
        if (r - 1.0).abs() > MAX_STRETCH_GAP + 0.005 {
            return None;
        }
        // Linear ramp 1 → r consuming exactly the lead-in: out = 2·src / (1 + r).
        let ramp_ms = (2.0 * src_lead / (1.0 + r)).round() as Ms;
        let tempo = TempoRamp {
            ratio_end: r as f32,
            ramp_ms,
        };
        (Some(tempo), ramp_ms, (src_overlap / r).round() as Ms)
    } else {
        (None, src_lead as Ms, src_overlap as Ms)
    };
    if !overlap_fits(input, src_overlap.round() as Ms, overlap_out) {
        return None;
    }
    let tail_len = incoming_at + overlap_out;
    let tail_source = (source_ms(tempo.as_ref(), tail_len as f64).round() as Ms)
        .min(input.a.duration_ms - tail_start);
    let bar_out = overlap_out / bars as Ms;
    let mid = incoming_at + overlap_out / 2;
    Some(TransitionPlan {
        kind: if stretched {
            TransitionKind::TempoBlend
        } else {
            TransitionKind::Blend
        },
        tail_start_ms: tail_start,
        tail_source_ms: tail_source,
        tail_len_ms: tail_len,
        incoming_at_ms: incoming_at,
        incoming_from_ms: db[k],
        handover_ms: mid,
        out_curve: FadeCurve::EqualPower,
        in_curve: FadeCurve::EqualPower,
        tempo,
        // Over the bar before the midpoint downbeat: complete as B takes over.
        bass_swap: Some(ShelfMove {
            freq_hz: BASS_SWAP_HZ,
            from_db: 0.0,
            to_db: BASS_SWAP_DB,
            start_ms: mid - bar_out,
            len_ms: bar_out,
        }),
        high_cut: Some(ShelfMove {
            freq_hz: HIGH_CUT_HZ,
            from_db: 0.0,
            to_db: HIGH_CUT_DB,
            start_ms: mid,
            len_ms: tail_len - mid,
        }),
        incoming_gain_db: 0.0,
    })
}

/// A beat-aligned `Fade` of `bars` of A's bars: B's first downbeat lands on A's.
fn aligned_fade(
    input: &PlanInput<'_>,
    da: &[Ms],
    db: &[Ms],
    k: usize,
    bars: usize,
    fixed: Option<Ms>,
    a_end: Ms,
) -> Option<TransitionPlan> {
    let o = place_in_a(da, fixed, a_end, bars, 0)?;
    let tail_start = fixed.unwrap_or(da[o]);
    let incoming_at = da[o] - tail_start;
    let overlap = da[o + bars] - da[o];
    if !overlap_fits(input, overlap, overlap) {
        return None;
    }
    let tail_len = incoming_at + overlap;
    Some(TransitionPlan {
        kind: TransitionKind::Fade,
        tail_start_ms: tail_start,
        tail_source_ms: tail_len.min(input.a.duration_ms - tail_start),
        tail_len_ms: tail_len,
        incoming_at_ms: incoming_at,
        incoming_from_ms: db[k],
        handover_ms: incoming_at + overlap / 2,
        out_curve: FadeCurve::EqualPower,
        in_curve: FadeCurve::EqualPower,
        tempo: None,
        bass_swap: None,
        high_cut: None,
        incoming_gain_db: 0.0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::{AnalysisProducer, SourceFingerprint};
    use crate::automix::ANALYSIS_VERSION;

    const DUR: Ms = 240_000;

    fn settings(mode: TransitionMode) -> TransitionSettings {
        TransitionSettings {
            mode,
            crossfade_seconds: 6.0,
        }
    }

    fn flat_curve(start_ms: Ms, len_ms: Ms, lufs: f32) -> LoudnessCurve {
        LoudnessCurve {
            start_ms,
            hop_ms: 100,
            lufs: vec![lufs; (len_ms / 100) as usize],
        }
    }

    /// A loudness-only record: audible 500 ms .. DUR − 1 s, flat at −9 LUFS.
    fn loud(duration: Ms) -> AnalysisRecord {
        AnalysisRecord {
            version: ANALYSIS_VERSION,
            analyser: "loudness".into(),
            produced_by: AnalysisProducer::Device,
            source: SourceFingerprint {
                duration_ms: duration,
                ..Default::default()
            },
            duration_ms: duration,
            integrated_lufs: Some(-9.0),
            audible_start_ms: 500,
            audible_end_ms: duration - 1_000,
            head: flat_curve(0, 60_000.min(duration), -9.0),
            tail: flat_curve(duration.saturating_sub(60_000), 60_000.min(duration), -9.0),
            ..Default::default()
        }
    }

    /// A record with a steady grid at `bpm`, `beats_per_bar` beats per bar, first beat at `first_ms`.
    fn beats(duration: Ms, bpm: f64, beats_per_bar: usize, first_ms: f64) -> AnalysisRecord {
        let mut r = loud(duration);
        let beat = 60_000.0 / bpm;
        let mut t = first_ms;
        let mut i = 0;
        while t < (duration - 1_000) as f64 {
            let ms = t.round() as Ms;
            r.beats_ms.push(ms);
            if i % beats_per_bar == 0 {
                r.downbeats_ms.push(ms);
            }
            t += beat;
            i += 1;
        }
        r.bpm = Some(bpm as f32);
        r.beat_confidence = 0.9;
        r.analyser = "beat-this-small/test".into();
        r
    }

    fn input<'a>(
        s: &'a TransitionSettings,
        a: Option<&'a AnalysisRecord>,
        b: Option<&'a AnalysisRecord>,
        fixed: Option<Ms>,
    ) -> PlanInput<'a> {
        PlanInput {
            settings: s,
            a: PlanTrack {
                duration_ms: a.map(|r| r.duration_ms).unwrap_or(DUR),
                record: a,
            },
            b: PlanTrack {
                duration_ms: b.map(|r| r.duration_ms).unwrap_or(DUR),
                record: b,
            },
            fixed_tail_start_ms: fixed,
        }
    }

    /// Invariants every plan holds.
    fn check(p: &TransitionPlan, i: &PlanInput<'_>) {
        assert!(p.incoming_at_ms <= p.tail_len_ms, "{p:?}");
        assert!(
            p.handover_ms >= p.incoming_at_ms && p.handover_ms <= p.tail_len_ms,
            "{p:?}"
        );
        assert!(
            p.tail_start_ms + p.tail_source_ms <= i.a.duration_ms,
            "{p:?}"
        );
        assert!(p.incoming_from_ms < i.b.duration_ms, "{p:?}");
        if let Some(f) = i.fixed_tail_start_ms {
            assert_eq!(p.tail_start_ms, f);
        }
        for mv in [p.bass_swap, p.high_cut].into_iter().flatten() {
            assert!(mv.start_ms + mv.len_ms <= p.tail_len_ms, "{p:?}");
        }
    }

    fn plan(i: &PlanInput<'_>) -> TransitionPlan {
        let p = plan_transition(i).unwrap();
        check(&p, i);
        // Deterministic.
        assert_eq!(plan_transition(i).unwrap(), p);
        p
    }

    #[test]
    fn off_plans_nothing() {
        let s = settings(TransitionMode::Off);
        assert_eq!(plan_transition(&input(&s, None, None, None)), None);
    }

    #[test]
    fn crossfade_without_records_ends_at_the_end() {
        let s = TransitionSettings {
            mode: TransitionMode::Crossfade,
            crossfade_seconds: 8.0,
        };
        let i = input(&s, None, None, None);
        let p = plan(&i);
        assert_eq!(p.kind, TransitionKind::Fade);
        assert_eq!(p.tail_start_ms, DUR - 8_000);
        assert_eq!(p.tail_len_ms, 8_000);
        assert_eq!(p.tail_source_ms, 8_000);
        assert_eq!(p.incoming_at_ms, 0);
        assert_eq!(p.incoming_from_ms, 0);
        assert_eq!(p.handover_ms, 4_000);
        assert_eq!(p.out_curve, FadeCurve::EqualPower);
    }

    #[test]
    fn crossfade_uses_audible_edges_and_ignores_beats() {
        let s = settings(TransitionMode::Crossfade);
        let (a, b) = (beats(DUR, 120.0, 4, 500.0), beats(DUR, 120.0, 4, 500.0));
        let i = input(&s, Some(&a), Some(&b), None);
        let p = plan(&i);
        assert_eq!(p.kind, TransitionKind::Fade);
        assert_eq!(p.tail_start_ms, DUR - 1_000 - 6_000);
        assert_eq!(p.incoming_from_ms, 500);
    }

    #[test]
    fn automix_without_records_is_a_six_second_fade() {
        let s = TransitionSettings {
            mode: TransitionMode::AutoMix,
            crossfade_seconds: 11.0,
        };
        let i = input(&s, None, None, None);
        let p = plan(&i);
        assert_eq!(p.kind, TransitionKind::Fade);
        assert_eq!(p.tail_len_ms, 6_000);
        assert_eq!(p.tail_start_ms, DUR - 6_000);
    }

    #[test]
    fn loudness_only_finds_the_fade_out() {
        let s = settings(TransitionMode::AutoMix);
        // A's tail: flat −9 LUFS, then from 12 s before the audible end it fades by 1 LU / s.
        let mut a = loud(DUR);
        let end = a.audible_end_ms;
        let fade_from = end - 12_000;
        for (i, v) in a.tail.lufs.iter_mut().enumerate() {
            let t = a.tail.start_ms + i as Ms * 100;
            if t > fade_from {
                *v = -9.0 - (t - fade_from) as f32 / 1_000.0;
            }
        }
        let b = loud(DUR);
        let i = input(&s, Some(&a), Some(&b), None);
        let p = plan(&i);
        assert_eq!(p.kind, TransitionKind::Fade);
        // −6 LU is reached 6 s into the fade: 6 s left, overlap 6 s, ending at the audible end.
        assert!(
            (fade_from + 5_900..=fade_from + 6_200).contains(&p.tail_start_ms),
            "{p:?}"
        );
        assert!((5_800..=6_100).contains(&p.tail_len_ms), "{p:?}");
        assert_eq!(p.incoming_from_ms, 500);

        // A flat ending (no fade) gets the 4 s minimum at the audible end.
        let flat = loud(DUR);
        let p = plan(&input(&s, Some(&flat), Some(&b), None));
        assert_eq!(p.tail_len_ms, 4_000);
        assert_eq!(p.tail_start_ms, flat.audible_end_ms - 4_000);

        // A long fade is capped at 10 s and never cuts more than 20 s of audible A.
        let mut long = loud(DUR);
        let lend = long.audible_end_ms;
        for (i, v) in long.tail.lufs.iter_mut().enumerate() {
            let t = long.tail.start_ms + i as Ms * 100;
            if t > lend - 50_000 {
                *v = -9.0 - (t - (lend - 50_000)) as f32 / 1_000.0;
            }
        }
        let p = plan(&input(&s, Some(&long), Some(&b), None));
        assert_eq!(p.tail_len_ms, 10_000);
        assert!(
            lend - (p.tail_start_ms + p.tail_len_ms) <= MAX_CUT_MS,
            "{p:?}"
        );
    }

    #[test]
    fn steady_close_tempos_tempo_blend() {
        let s = settings(TransitionMode::AutoMix);
        let a = beats(DUR, 120.0, 4, 250.0);
        let b = beats(DUR, 124.0, 4, 800.0);
        let i = input(&s, Some(&a), Some(&b), None);
        let p = plan(&i);
        assert_eq!(p.kind, TransitionKind::TempoBlend);
        let t = p.tempo.unwrap();
        assert!((t.ratio_end as f64 - 124.0 / 120.0).abs() < 0.002, "{t:?}");
        // Tail starts on a downbeat of A, B enters on its first downbeat.
        assert!(a.downbeats_ms.contains(&p.tail_start_ms));
        assert_eq!(p.incoming_from_ms, b.downbeats_ms[0]);
        // Ramp over 4 bars of A (2 s each), consumed exactly by incoming_at.
        let ramp_src = source_ms(p.tempo.as_ref(), p.incoming_at_ms as f64);
        assert!((ramp_src - 8_000.0).abs() < 2.0, "{ramp_src}");
        // 8 bars of B's tempo: 8 × 4 × 60000/124 ms.
        let want = 8.0 * 4.0 * 60_000.0 / 124.0;
        assert!(
            ((p.tail_len_ms - p.incoming_at_ms) as f64 - want).abs() < 3.0,
            "{p:?}"
        );
        // Overlap ends on a downbeat of A, before its audible end and within a bar of it.
        let end_in_a = p.tail_start_ms + p.tail_source_ms;
        assert!(
            a.downbeats_ms.iter().any(|&d| d.abs_diff(end_in_a) <= 2),
            "{end_in_a}"
        );
        assert!(end_in_a <= a.audible_end_ms && a.audible_end_ms - end_in_a < 2_000);
        // Bass swap completes at the handover, which is mid-overlap.
        let swap = p.bass_swap.unwrap();
        assert_eq!(swap.start_ms + swap.len_ms, p.handover_ms);
        assert_eq!(swap.to_db, BASS_SWAP_DB);
        assert_eq!(
            p.handover_ms,
            p.incoming_at_ms + (p.tail_len_ms - p.incoming_at_ms) / 2
        );
    }

    #[test]
    fn matching_tempos_blend_without_stretch() {
        let s = settings(TransitionMode::AutoMix);
        let a = beats(DUR, 120.0, 4, 250.0);
        let b = beats(DUR, 120.0, 4, 1_000.0);
        let p = plan(&input(&s, Some(&a), Some(&b), None));
        assert_eq!(p.kind, TransitionKind::Blend);
        assert_eq!(p.tempo, None);
        assert_eq!(p.incoming_at_ms, 0);
        assert_eq!(p.tail_len_ms, 16_000);
        assert_eq!(p.tail_source_ms, 16_000);
        assert!(p.bass_swap.is_some());
    }

    #[test]
    fn quiet_ends_get_sixteen_bars() {
        let s = settings(TransitionMode::AutoMix);
        let mut a = beats(DUR, 120.0, 4, 250.0);
        let mut b = beats(DUR, 120.0, 4, 1_000.0);
        // Ends at −9 LUFS in tracks integrating to −4: both well below their bodies.
        a.integrated_lufs = Some(-4.0);
        b.integrated_lufs = Some(-4.0);
        let p = plan(&input(&s, Some(&a), Some(&b), None));
        assert_eq!(p.kind, TransitionKind::Blend);
        assert_eq!(p.tail_len_ms, 32_000);
    }

    #[test]
    fn wide_gaps_fade_on_the_beat() {
        let s = settings(TransitionMode::AutoMix);
        let a = beats(DUR, 120.0, 4, 250.0);
        // 150 BPM is a 1-bar gap, lengthened to 4 s; 236 BPM is double
        // time of 118, a 4-bar gap.
        for (bpm_b, bars) in [(132.0, 4u32), (138.0, 2), (150.0, 2), (236.0, 4)] {
            let b = beats(DUR, bpm_b, 4, 600.0);
            let p = plan(&input(&s, Some(&a), Some(&b), None));
            assert_eq!(p.kind, TransitionKind::Fade, "{bpm_b}");
            assert_eq!(p.tail_len_ms, bars * 2_000, "{bpm_b}");
            assert!(a.downbeats_ms.contains(&p.tail_start_ms));
            assert_eq!(p.incoming_from_ms, b.downbeats_ms[0]);
            assert!(p.bass_swap.is_none());
        }
    }

    /// Beat plans keep the overlap within half of each track's audible span,
    /// as fades do: a short B drops 8 bars (16 s) of blend or 4 bars (8 s)
    /// of aligned fade for the rescue fade.
    #[test]
    fn beat_plans_stay_within_half_a_short_track() {
        let s = settings(TransitionMode::AutoMix);
        let a = beats(DUR, 120.0, 4, 250.0);
        for (b, beat_len) in [
            (beats(30_000, 120.0, 4, 1_000.0), 16_000),
            (beats(14_000, 132.0, 4, 600.0), 8_000),
        ] {
            let i = input(&s, Some(&a), Some(&b), None);
            let p = plan(&i);
            let b_half = (b.duration_ms - b.audible_start_ms) / 2;
            let overlap = p.tail_len_ms - p.incoming_at_ms;
            assert!(overlap <= b_half, "{overlap} > {b_half}: {p:?}");
            assert!(overlap < beat_len, "{p:?}");
            assert_eq!(p.kind, TransitionKind::Fade, "{p:?}");
        }
    }

    /// A record's curve is outside data: a hop that runs past `u32` doesn't
    /// overflow.
    #[test]
    fn a_huge_hop_does_not_overflow() {
        let curve = LoudnessCurve {
            start_ms: u32::MAX - 10,
            hop_ms: u32::MAX / 2,
            lufs: vec![-9.0; 4],
        };
        assert_eq!(points(&curve, u32::MAX).count(), 4);
        assert!(fade_out_start(&curve, u32::MAX).is_some());
    }

    #[test]
    fn tempo_gap_counts_octaves() {
        assert!((tempo_gap(120.0, 132.0) - 0.1).abs() < 1e-9);
        assert!((tempo_gap(120.0, 236.0) - 1.0 / 60.0).abs() < 1e-9);
        assert!((tempo_gap(140.0, 72.0) - (144.0 / 140.0 - 1.0)).abs() < 1e-9);
    }

    #[test]
    fn three_four_fades_on_the_beat() {
        let s = settings(TransitionMode::AutoMix);
        let a = beats(DUR, 120.0, 3, 250.0);
        let b = beats(DUR, 121.0, 4, 600.0);
        let ga = Grid::of(&a, Region::Tail(a.audible_end_ms)).unwrap();
        assert_eq!(ga.beats_per_bar, 3);
        assert!((ga.bpm - 120.0).abs() < 0.5);
        let p = plan(&input(&s, Some(&a), Some(&b), None));
        assert_eq!(p.kind, TransitionKind::Fade);
        assert_eq!(p.tail_len_ms, 4 * 1_500);
    }

    #[test]
    fn low_confidence_or_unsteady_falls_back_to_loudness() {
        let s = settings(TransitionMode::AutoMix);
        let mut a = beats(DUR, 120.0, 4, 250.0);
        a.beat_confidence = 0.3;
        let b = beats(DUR, 120.0, 4, 600.0);
        let p = plan(&input(&s, Some(&a), Some(&b), None));
        assert_eq!(p.kind, TransitionKind::Fade);
        assert_eq!(p.tail_len_ms, SMART_MIN_OVERLAP_MS);

        // Jittered downbeats: not steady.
        let mut c = beats(DUR, 120.0, 4, 250.0);
        for (i, d) in c.downbeats_ms.iter_mut().enumerate() {
            *d += [0, 300, 0, 450, 120][i % 5];
        }
        assert!(Grid::of(&c, Region::Tail(c.audible_end_ms)).is_none());
        let p = plan(&input(&s, Some(&c), Some(&b), None));
        assert_eq!(p.kind, TransitionKind::Fade);

        // B loudness-only: A's loudness places the fade.
        let lb = loud(DUR);
        let p = plan(&input(&s, Some(&a), Some(&lb), None));
        assert_eq!(p.kind, TransitionKind::Fade);
    }

    #[test]
    fn short_tracks_are_safe() {
        let s = settings(TransitionMode::AutoMix);
        for dur in [3_000, 9_000, 20_000] {
            let a = beats(dur, 120.0, 4, 100.0);
            let b = beats(dur, 122.0, 4, 100.0);
            let p = plan(&input(&s, Some(&a), Some(&b), None));
            assert_eq!(p.kind, TransitionKind::Fade, "{dur}");
            assert!(p.tail_len_ms <= dur / 2, "{dur}: {p:?}");
            let p = plan(&input(&s, None, None, None));
            assert!(p.tail_len_ms <= 6_000);
        }
    }

    #[test]
    fn fixed_tail_start_is_kept() {
        let a = beats(DUR, 120.0, 4, 250.0);
        let b = beats(DUR, 124.0, 4, 800.0);
        let auto = settings(TransitionMode::AutoMix);
        // The plan made with the predicted next track, then replanned with the same tail start.
        let first = plan(&input(&auto, Some(&a), Some(&b), None));
        let again = plan(&input(&auto, Some(&a), Some(&b), Some(first.tail_start_ms)));
        assert_eq!(again, first);

        // A different next track after commit: still from the committed start.
        let c = beats(DUR, 120.0, 4, 300.0);
        let p = plan(&input(&auto, Some(&a), Some(&c), Some(first.tail_start_ms)));
        assert_eq!(p.kind, TransitionKind::Blend);
        assert_eq!(p.tempo, None);
        assert_eq!(
            p.incoming_at_ms, 0,
            "a blend needs no ramp: it starts at once"
        );

        // Off a downbeat: the overlap begins at the next one.
        let fixed = first.tail_start_ms + 700;
        let p = plan(&input(&auto, Some(&a), Some(&c), Some(fixed)));
        assert_eq!(p.incoming_at_ms, 1_300);

        // Loudness-only and no-record cases keep it too.
        let la = loud(DUR);
        let p = plan(&input(&auto, Some(&la), None, Some(200_000)));
        assert_eq!(p.tail_len_ms, 10_000);
        let xf = settings(TransitionMode::Crossfade);
        let p = plan(&input(&xf, None, None, Some(230_000)));
        assert_eq!(p.tail_len_ms, 6_000);
        // Too late for A's grid: a rescue fade, still at the fixed start.
        let p = plan(&input(&auto, Some(&a), Some(&b), Some(DUR - 3_000)));
        assert_eq!(p.kind, TransitionKind::Fade);
        assert!(p.tail_len_ms >= 1_000);
    }

    #[test]
    fn late_edit_is_a_short_fade_from_the_committed_start() {
        let s = TransitionSettings {
            mode: TransitionMode::AutoMix,
            crossfade_seconds: 10.0,
        };
        let b = loud(DUR);
        let p = late_edit_fade(
            200_000,
            PlanTrack {
                duration_ms: DUR,
                record: None,
            },
            PlanTrack {
                duration_ms: DUR,
                record: Some(&b),
            },
            &s,
        );
        assert_eq!(p.kind, TransitionKind::Fade);
        assert_eq!(p.tail_start_ms, 200_000);
        assert_eq!(p.tail_len_ms, 5_000);
        assert_eq!(p.incoming_from_ms, 500);
        let s = TransitionSettings {
            crossfade_seconds: 1.0,
            ..s
        };
        let p = late_edit_fade(
            200_000,
            PlanTrack {
                duration_ms: DUR,
                record: None,
            },
            PlanTrack {
                duration_ms: DUR,
                record: None,
            },
            &s,
        );
        assert_eq!(p.tail_len_ms, LATE_EDIT_MIN_MS);
    }

    #[test]
    fn grid_helpers() {
        let db = [0, 2_000, 4_000, 6_000, 8_010, 10_000];
        assert_eq!(bar_length(&db), Some(2_000.0));
        assert_eq!(grid_steadiness(&db, 2_000.0), 1.0);
        assert_eq!(grid_steadiness(&[0, 2_000, 5_000], 2_000.0), 0.5);
        let beats: Vec<Ms> = (0..20).map(|i| i * 500).collect();
        assert_eq!(
            beats_per_bar(&beats, &[0, 2_000, 4_000, 6_000, 8_000]),
            Some(4)
        );
        assert_eq!(
            beats_per_bar(&beats, &[0, 1_500, 3_000, 4_500, 6_000]),
            Some(3)
        );
        // Bars that disagree have no meter.
        assert_eq!(
            beats_per_bar(&beats, &[0, 2_000, 3_500, 6_000, 7_000]),
            None
        );
        assert!((tempo_bpm(2_000.0, 4) - 120.0).abs() < 1e-9);
        assert_eq!(downbeat_at_or_after(&db, 2_001), Some(2));
        assert_eq!(downbeat_at_or_after(&db, 10_001), None);
        assert_eq!(downbeat_near(&db, 7_900, 200), Some(4));
        assert_eq!(downbeat_near(&db, 7_000, 200), None);
        assert_eq!(downbeat_near(&db, 1_950, 100), Some(1));
    }

    // -- Vocals -------------------------------------------------------------

    /// A voice curve over `[start, start + len)` (100 ms points), voiced in `ranges`.
    fn voice(start: Ms, len: Ms, ranges: &[(Ms, Ms)]) -> VocalCurve {
        VocalCurve {
            start_ms: start,
            hop_ms: 100,
            voice: (0..len / 100)
                .map(|i| {
                    let t = start + i * 100;
                    if ranges.iter().any(|&(a, b)| t >= a && t < b) {
                        0.9
                    } else {
                        0.1
                    }
                })
                .collect(),
        }
    }

    /// 120 BPM 4/4 tracks (A's downbeats at 250 + 2000k, audible to DUR − 1 s;
    /// B's at 1000 + 2000k): A sings until `a_until`, B from `b_from`.
    fn sung(a_until: Ms, b_from: Ms) -> (AnalysisRecord, AnalysisRecord) {
        let mut a = beats(DUR, 120.0, 4, 250.0);
        let mut b = beats(DUR, 120.0, 4, 1_000.0);
        a.tail_vocals = voice(DUR - 36_000, 36_000, &[(0, a_until)]);
        b.head_vocals = voice(0, 36_000, &[(b_from, DUR)]);
        (a, b)
    }

    fn span(p: &TransitionPlan, a: &AnalysisRecord, b: &AnalysisRecord) -> VoiceSpan {
        voice_span(p, &Voices::of(a, b).unwrap())
    }

    #[test]
    fn voice_helpers() {
        let c = voice(10_000, 1_000, &[(10_300, 10_500)]);
        assert!(!voiced(&c, 9_900));
        assert!(!voiced(&c, 10_200));
        assert!(voiced(&c, 10_300) && voiced(&c, 10_499));
        assert!(!voiced(&c, 10_500));
        assert!(!voiced(&c, 11_000), "past the curve");
        let clean = |a, b| {
            VoiceSpan {
                a_last: a,
                b_first: b,
            }
            .is_clean()
        };
        assert!(clean(None, Some(0)) && clean(Some(9_000), None));
        assert!(clean(Some(4_000), Some(5_000)));
        assert!(clean(Some(5_400), Some(5_000)), "within the tolerance");
        assert!(!clean(Some(5_600), Some(5_000)));
        let (a, mut b) = sung(0, 0);
        assert!(Voices::of(&a, &b).is_some());
        b.head_vocals = VocalCurve::default();
        assert!(Voices::of(&a, &b).is_none());
    }

    /// Without voice curves on both sides the plan is the one without vocals.
    #[test]
    fn no_voice_data_plans_as_before() {
        let s = settings(TransitionMode::AutoMix);
        let plain_a = beats(DUR, 120.0, 4, 250.0);
        let plain_b = beats(DUR, 120.0, 4, 1_000.0);
        let before = plan(&input(&s, Some(&plain_a), Some(&plain_b), None));
        let (a, b) = sung(DUR, 0);
        assert_eq!(plan(&input(&s, Some(&a), Some(&plain_b), None)), before);
        assert_eq!(plan(&input(&s, Some(&plain_a), Some(&b), None)), before);
    }

    /// (a) The voices already take turns: the usual 8 bars, and the handover
    /// stays mid-overlap when that is between them.
    #[test]
    fn voices_in_turn_keep_the_blend() {
        let s = settings(TransitionMode::AutoMix);
        let plain = plan(&input(
            &s,
            Some(&beats(DUR, 120.0, 4, 250.0)),
            Some(&beats(DUR, 120.0, 4, 1_000.0)),
            None,
        ));
        // Overlap: A 222.25–238.25 s; A sings to 230 s (output 7.75 s), B from 10 s (output 9 s).
        let (a, b) = sung(230_000, 10_000);
        let p = plan(&input(&s, Some(&a), Some(&b), None));
        assert_eq!(p, plain);
        let sp = span(&p, &a, &b);
        assert_eq!(sp.a_last, Some(7_700));
        assert_eq!(sp.b_first, Some(9_000));
        assert!(sp.is_clean());
    }

    /// (a) With the middle outside the gap, the handover moves to B's downbeat
    /// in it, and B's voice comes in on or after it.
    #[test]
    fn handover_moves_between_the_voices() {
        let s = settings(TransitionMode::AutoMix);
        // A sings to 233 s (output 10.75 s), B from 14 s (output 13 s).
        let (a, b) = sung(233_000, 14_000);
        let p = plan(&input(&s, Some(&a), Some(&b), None));
        assert_eq!(p.kind, TransitionKind::Blend);
        assert_eq!(p.tail_len_ms, 16_000, "still 8 bars");
        assert_eq!(p.handover_ms, 12_000, "B's downbeat at 13 s");
        let swap = p.bass_swap.unwrap();
        assert_eq!((swap.start_ms, swap.len_ms), (10_000, 2_000));
        let cut = p.high_cut.unwrap();
        assert_eq!(
            (cut.start_ms, cut.len_ms, cut.freq_hz),
            (12_000, 4_000, HIGH_CUT_HZ)
        );
        let sp = span(&p, &a, &b);
        assert!(sp.a_last.unwrap() <= p.handover_ms && p.handover_ms <= sp.b_first.unwrap());
    }

    /// (b) 8 and 4 bars would put the voices on top of each other; 2 bars fit
    /// the gap (A's last 2.25 s and B's first 4 s are instrumental).
    #[test]
    fn blend_shortens_to_the_instrumental_gap() {
        let s = settings(TransitionMode::AutoMix);
        let (a, b) = sung(236_000, 5_000);
        let p = plan(&input(&s, Some(&a), Some(&b), None));
        assert_eq!(p.kind, TransitionKind::Blend);
        assert_eq!(p.tail_len_ms, 4_000);
        assert_eq!(p.tail_start_ms, 234_250, "still ends on A's last downbeat");
        assert!(span(&p, &a, &b).is_clean());
        assert_eq!(p.high_cut.unwrap().freq_hz, HIGH_CUT_HZ);
    }

    /// (c) Voices all the way through: the shortest blend of at least 4 s,
    /// bass swapped early and A's presence cut from B's entry.
    #[test]
    fn voices_throughout_get_a_short_shaped_blend() {
        let s = settings(TransitionMode::AutoMix);
        let (a, b) = sung(DUR, 0);
        let p = plan(&input(&s, Some(&a), Some(&b), None));
        assert_eq!(p.kind, TransitionKind::Blend);
        assert_eq!(p.tail_len_ms, 4_000);
        assert_eq!(p.handover_ms, 2_000);
        let swap = p.bass_swap.unwrap();
        assert_eq!((swap.start_ms, swap.len_ms), (0, 1_000));
        let cut = p.high_cut.unwrap();
        assert_eq!(
            (cut.start_ms, cut.len_ms, cut.freq_hz),
            (0, 2_000, CLASH_HIGH_CUT_HZ)
        );
        assert!(!span(&p, &a, &b).is_clean());
        // At 90 BPM two bars are 5.3 s: still the shortest.
        let mut a = beats(DUR, 90.0, 4, 250.0);
        let mut b = beats(DUR, 90.0, 4, 1_000.0);
        a.tail_vocals = voice(DUR - 36_000, 36_000, &[(0, DUR)]);
        b.head_vocals = voice(0, 36_000, &[(0, DUR)]);
        let p = plan(&input(&s, Some(&a), Some(&b), None));
        assert_eq!(p.tail_len_ms - p.incoming_at_ms, 5_333);
        // At 170 BPM two bars are 2.8 s: three.
        let mut a = beats(DUR, 170.0, 4, 250.0);
        let mut b = beats(DUR, 170.0, 4, 1_000.0);
        a.tail_vocals = voice(DUR - 36_000, 36_000, &[(0, DUR)]);
        b.head_vocals = voice(0, 36_000, &[(0, DUR)]);
        let p = plan(&input(&s, Some(&a), Some(&b), None));
        let bars = (p.tail_len_ms - p.incoming_at_ms) as f64 / (4.0 * 60_000.0 / 170.0);
        assert!((bars - 3.0).abs() < 0.01, "{bars}");
    }

    /// A tempo blend reads A's voice through the ramp's stretch.
    #[test]
    fn tempo_blends_follow_the_stretch() {
        let s = settings(TransitionMode::AutoMix);
        let mut a = beats(DUR, 120.0, 4, 250.0);
        let mut b = beats(DUR, 124.0, 4, 800.0);
        a.tail_vocals = voice(DUR - 36_000, 36_000, &[(0, 230_000)]);
        b.head_vocals = voice(0, 36_000, &[(20_000, DUR)]);
        let p = plan(&input(&s, Some(&a), Some(&b), None));
        assert_eq!(p.kind, TransitionKind::TempoBlend);
        let sp = span(&p, &a, &b);
        assert!(sp.is_clean(), "{sp:?}");
        // A's voice ends at source 230 s: output time through the ramp.
        let a_last = sp.a_last.unwrap() as f64;
        let src = p.tail_start_ms as f64 + source_ms(p.tempo.as_ref(), a_last);
        assert!((src - 230_000.0).abs() < 150.0, "{src}");
        // B sings over the whole overlap's second half: shortened until clean.
        b.head_vocals = voice(0, 36_000, &[(3_000, DUR)]);
        let p = plan(&input(&s, Some(&a), Some(&b), None));
        assert!(span(&p, &a, &b).is_clean());
        assert!(p.tail_len_ms - p.incoming_at_ms < 16_000);
    }

    /// The beat-aligned fade drops bars to keep voices apart, to 4 s at least.
    #[test]
    fn aligned_fades_shorten_for_voices() {
        let s = settings(TransitionMode::AutoMix);
        // 132 BPM: a 4-bar (8 s) fade without voices.
        let mut a = beats(DUR, 120.0, 4, 250.0);
        let mut b = beats(DUR, 132.0, 4, 600.0);
        assert_eq!(
            plan(&input(&s, Some(&a), Some(&b), None)).tail_len_ms,
            8_000
        );
        // A sings to 235.5 s; B from 4 s: 3 bars (6 s) keep them apart.
        a.tail_vocals = voice(DUR - 36_000, 36_000, &[(0, 235_500)]);
        b.head_vocals = voice(0, 36_000, &[(4_000, DUR)]);
        let p = plan(&input(&s, Some(&a), Some(&b), None));
        assert_eq!(p.kind, TransitionKind::Fade);
        assert_eq!(p.tail_len_ms, 6_000);
        assert!(span(&p, &a, &b).is_clean());
        // Singing throughout: the shortest, 2 bars.
        a.tail_vocals = voice(DUR - 36_000, 36_000, &[(0, DUR)]);
        b.head_vocals = voice(0, 36_000, &[(0, DUR)]);
        assert_eq!(
            plan(&input(&s, Some(&a), Some(&b), None)).tail_len_ms,
            4_000
        );
    }

    /// A committed tail start is kept with voices too.
    #[test]
    fn voices_with_a_fixed_tail_start() {
        let s = settings(TransitionMode::AutoMix);
        let (a, b) = sung(DUR, 0);
        let p = plan(&input(&s, Some(&a), Some(&b), Some(222_250)));
        assert_eq!(p.tail_start_ms, 222_250);
        assert_eq!(p.kind, TransitionKind::Blend);
    }
}
