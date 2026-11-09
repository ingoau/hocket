//! From the beat tracker's raw output to the record's grid: `beats_ms`,
//! `downbeats_ms`, `bpm` and `beat_confidence`. Pure, so it is tested
//! without the model.
//!
//! Beat This!'s "minimal" post-processing (peak picking at 50 fps) leaves
//! three things the planner trips over, so [`build_grid`] tidies them:
//!
//! 1. **20 ms quantisation.** Peaks sit on frames; a 1 % bar-length error is
//!    150 ms of flamming over 8 bars. Each peak is refined to sub-frame
//!    precision by a parabola through the logits around it ([`refine_peak`]).
//! 2. **Missed and doubled beats.** A gap of 2–4 local beat intervals gets
//!    the missing beats back, evenly spaced; a beat splitting one interval
//!    in two is dropped.
//! 3. **Noisy downbeats** (extra ones on half bars, missed ones). The metre
//!    (3 or 4 beats) is the one most raw downbeat spacings agree with; each
//!    beat's bar phase is a vote of the raw downbeats within ±8 bars; the
//!    downbeats are the beats on that phase. This is the bar-phase step a
//!    DBN would do, done by counting.
//!
//! Confidence is steadiness × agreement × coverage: the share of beat
//! intervals within 8 % of their neighbours, the share of the tracker's
//! downbeats the regular grid keeps, and how much of the audible span the
//! beats cover (half credit, so a long ambient intro costs some confidence
//! but not all; the planner judges each end separately anyway).

use crate::api::Ms;

/// Fewer beats than this give confidence 0 (not a grid worth trusting).
pub const MIN_BEATS: usize = 16;
/// The tracker's frame rate.
pub const FPS: f64 = 50.0;
/// Neighbouring beat intervals each side the local tempo is taken over.
const LOCAL_WINDOW: usize = 8;
/// A beat interval is steady within this fraction of the local median.
const BEAT_TOLERANCE: f64 = 0.08;
/// Bars each side whose raw downbeats vote on a beat's bar phase.
const PHASE_WINDOW_BARS: i64 = 8;
/// Gaps of more than this many beats start a new segment (a break).
const MAX_FILL: i64 = 4;

/// The record's grid.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct BeatGrid {
    pub beats_ms: Vec<Ms>,
    pub downbeats_ms: Vec<Ms>,
    pub beats_per_bar: u32,
    /// Share of the tracker's downbeats that land on the grid's downbeats.
    pub agreement: f64,
}

/// Sub-frame time (s) of the peak picked at `t` seconds: the vertex of the
/// parabola through the logits of the frames either side.
pub fn refine_peak(t: f32, logits: &[f32]) -> f64 {
    let x = f64::from(t) * FPS;
    let k = x.round();
    // Deduplicated peaks already sit between frames: keep them.
    if (x - k).abs() > 1e-3 || k < 1.0 || k as usize + 1 >= logits.len() {
        return x / FPS;
    }
    let k = k as usize;
    let (y0, y1, y2) = (
        f64::from(logits[k - 1]),
        f64::from(logits[k]),
        f64::from(logits[k + 1]),
    );
    let denom = y0 - 2.0 * y1 + y2;
    let delta = if denom < -1e-9 {
        (0.5 * (y0 - y2) / denom).clamp(-0.5, 0.5)
    } else {
        0.0
    };
    (k as f64 + delta) / FPS
}

fn median(mut v: Vec<f64>) -> Option<f64> {
    if v.is_empty() {
        return None;
    }
    v.sort_unstable_by(f64::total_cmp);
    Some(v[v.len() / 2])
}

fn intervals<T: Copy + Into<f64>>(t: &[T]) -> Vec<f64> {
    t.windows(2).map(|w| w[1].into() - w[0].into()).collect()
}

/// Median of the intervals within [`LOCAL_WINDOW`] of each interval.
fn local_medians(iv: &[f64]) -> Vec<f64> {
    (0..iv.len())
        .map(|i| {
            let lo = i.saturating_sub(LOCAL_WINDOW);
            let hi = (i + LOCAL_WINDOW + 1).min(iv.len());
            median(iv[lo..hi].to_vec()).unwrap_or(0.0)
        })
        .collect()
}

/// Drop beats that split one beat interval in two, and fill gaps of 2–4
/// beats.
fn clean_beats(beats: &[f64]) -> Vec<f64> {
    if beats.len() < 3 {
        return beats.to_vec();
    }
    let iv = intervals(beats);
    let med = local_medians(&iv);
    // Doubled: beat i+1 where iv[i] + iv[i+1] is one local interval.
    let mut keep = vec![true; beats.len()];
    let mut i = 0;
    while i + 1 < iv.len() {
        let m = med[i];
        if m > 0.0 && iv[i] < 0.65 * m && ((iv[i] + iv[i + 1]) / m - 1.0).abs() < 0.15 {
            keep[i + 1] = false;
            i += 2;
        } else {
            i += 1;
        }
    }
    let kept: Vec<f64> = beats
        .iter()
        .zip(&keep)
        .filter_map(|(&b, &k)| k.then_some(b))
        .collect();
    let iv = intervals(&kept);
    let med = local_medians(&iv);
    let mut out = Vec::with_capacity(kept.len() + 16);
    for (i, &b) in kept.iter().enumerate() {
        out.push(b);
        let Some(&gap) = iv.get(i) else { continue };
        let m = med[i];
        if m <= 0.0 {
            continue;
        }
        let r = gap / m;
        let n = r.round();
        if (2.0..=MAX_FILL as f64).contains(&n) && (r - n).abs() < 0.15 * n {
            for k in 1..n as usize {
                out.push(b + gap * k as f64 / n);
            }
        }
    }
    out
}

/// Beat counts: each beat's index on a continuous count, and its segment
/// (a new one after a break longer than [`MAX_FILL`] beats).
fn counts(beats: &[f64]) -> (Vec<i64>, Vec<usize>) {
    let iv = intervals(beats);
    let med = local_medians(&iv);
    let mut c = vec![0i64; beats.len()];
    let mut seg = vec![0usize; beats.len()];
    for i in 0..iv.len() {
        let steps = if med[i] > 0.0 {
            (iv[i] / med[i]).round().max(1.0) as i64
        } else {
            1
        };
        c[i + 1] = c[i] + steps;
        seg[i + 1] = seg[i] + usize::from(steps > MAX_FILL);
    }
    (c, seg)
}

/// Index of the beat nearest `t` within `tol`.
fn nearest(beats: &[f64], t: f64, tol: f64) -> Option<usize> {
    let i = beats.partition_point(|&b| b < t);
    [i.checked_sub(1), Some(i)]
        .into_iter()
        .flatten()
        .filter(|&j| j < beats.len())
        .min_by(|&a, &b| (beats[a] - t).abs().total_cmp(&(beats[b] - t).abs()))
        .filter(|&j| (beats[j] - t).abs() <= tol)
}

/// The grid from refined beat and downbeat times (seconds).
pub fn build_grid(beats_s: &[f64], downbeats_s: &[f64]) -> BeatGrid {
    let beats = clean_beats(beats_s);
    let to_ms = |s: f64| (s * 1000.0).round().max(0.0) as Ms;
    let beats_ms: Vec<Ms> = beats.iter().map(|&b| to_ms(b)).collect();
    if beats.len() < 2 {
        return BeatGrid {
            beats_ms,
            ..Default::default()
        };
    }
    let (c, seg) = counts(&beats);
    // Raw downbeats as beat indices (snapped within a third of a beat).
    let tol = median(intervals(&beats)).unwrap_or(0.5) / 3.0;
    let mut raw: Vec<usize> = downbeats_s
        .iter()
        .filter_map(|&d| nearest(&beats, d, tol))
        .collect();
    raw.dedup();
    if raw.is_empty() {
        return BeatGrid {
            beats_ms,
            ..Default::default()
        };
    }

    // Metre: 3 or 4, whichever more raw downbeat spacings are multiples of.
    let diffs: Vec<i64> = raw
        .windows(2)
        .filter(|w| seg[w[0]] == seg[w[1]])
        .map(|w| c[w[1]] - c[w[0]])
        .filter(|&d| d > 0 && d <= 16)
        .collect();
    let score = |n: i64| diffs.iter().filter(|&&d| d % n == 0).count();
    let n: i64 = if score(3) > score(4) { 3 } else { 4 };

    // Phase per beat: vote of the raw downbeats within ±8 bars, same segment.
    let reach = PHASE_WINDOW_BARS * n;
    let mut downbeats_ms = Vec::new();
    let mut on_grid = 0usize;
    let mut lo = 0usize;
    let mut hi = 0usize;
    let mut last_phase: Option<i64> = None;
    for i in 0..beats.len() {
        while lo < raw.len() && (seg[raw[lo]] < seg[i] || c[raw[lo]] < c[i] - reach) {
            lo += 1;
        }
        while hi < raw.len() && seg[raw[hi]] == seg[i] && c[raw[hi]] <= c[i] + reach {
            hi += 1;
        }
        hi = hi.max(lo);
        let mut votes = [0usize; 4];
        for &j in &raw[lo..hi] {
            if seg[j] == seg[i] {
                votes[c[j].rem_euclid(n) as usize] += 1;
            }
        }
        let best = (0..n as usize).max_by_key(|&p| votes[p]).unwrap_or(0);
        if votes[best] == 0 {
            last_phase = None;
            continue;
        }
        // Keep the previous phase on a tie, so the grid doesn't flicker.
        let phase = match last_phase {
            Some(p) if votes[p as usize] == votes[best] => p,
            _ => best as i64,
        };
        last_phase = Some(phase);
        if c[i].rem_euclid(n) == phase {
            downbeats_ms.push(beats_ms[i]);
            if raw.binary_search(&i).is_ok() {
                on_grid += 1;
            }
        }
    }
    downbeats_ms.dedup();
    BeatGrid {
        beats_ms,
        downbeats_ms,
        beats_per_bar: n as u32,
        agreement: on_grid as f64 / raw.len() as f64,
    }
}

/// Tempo to 0.1 BPM from the median 8-beat span (finer than one interval).
pub fn bpm(beats: &[Ms]) -> Option<f32> {
    if beats.len() < MIN_BEATS {
        return None;
    }
    let spans: Vec<f64> = beats
        .windows(9)
        .map(|w| f64::from(w[8] - w[0]) / 8.0)
        .collect();
    let m = median(spans)?;
    (m > 0.0).then(|| ((60_000.0 / m) * 10.0).round() as f32 / 10.0)
}

/// Share of beat intervals within [`BEAT_TOLERANCE`] of the median of their
/// neighbours: how steady the grid is, tolerant of slow tempo drift.
pub fn beat_steadiness(beats: &[Ms]) -> f64 {
    let iv: Vec<f64> = intervals(beats);
    if iv.is_empty() {
        return 0.0;
    }
    let med = local_medians(&iv);
    let steady = iv
        .iter()
        .zip(&med)
        .filter(|&(&x, &m)| m > 0.0 && ((x - m) / m).abs() <= BEAT_TOLERANCE)
        .count();
    steady as f64 / iv.len() as f64
}

/// See the module docs.
pub fn confidence(grid: &BeatGrid, audible: (Ms, Ms)) -> f32 {
    let beats = &grid.beats_ms;
    if beats.len() < MIN_BEATS || grid.downbeats_ms.len() < 4 {
        return 0.0;
    }
    let span = audible.1.saturating_sub(audible.0).max(1) as f64;
    let covered = beats[beats.len() - 1].saturating_sub(beats[0]) as f64;
    let coverage = (covered / span).clamp(0.0, 1.0);
    let c = beat_steadiness(beats) * grid.agreement * (0.5 + 0.5 * coverage);
    ((c * 1000.0).round() / 1000.0) as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    fn times(n: usize, ibi: f64) -> Vec<f64> {
        (0..n).map(|i| 1.0 + i as f64 * ibi).collect()
    }

    fn every(beats: &[f64], k: usize, offset: usize) -> Vec<f64> {
        beats.iter().copied().skip(offset).step_by(k).collect()
    }

    #[test]
    fn steady_four_four_is_confident() {
        let b = times(400, 0.5);
        let g = build_grid(&b, &every(&b, 4, 1));
        assert_eq!(g.beats_per_bar, 4);
        assert_eq!(g.beats_ms.len(), 400);
        assert_eq!(g.downbeats_ms.len(), 100);
        assert_eq!(g.downbeats_ms[0], 1500);
        assert_eq!(bpm(&g.beats_ms), Some(120.0));
        let c = confidence(&g, (1000, 1000 + 399 * 500));
        assert!(c > 0.99, "{c}");
    }

    #[test]
    fn noisy_downbeats_are_regularised() {
        let b = times(400, 0.5);
        // Every bar's downbeat, plus a spurious one on every third half bar,
        // and every fifth downbeat missed.
        let mut d: Vec<f64> = every(&b, 4, 0)
            .into_iter()
            .enumerate()
            .filter(|(i, _)| i % 5 != 4)
            .map(|(_, t)| t)
            .collect();
        d.extend(every(&b, 12, 2));
        d.sort_by(f64::total_cmp);
        let g = build_grid(&b, &d);
        assert_eq!(g.beats_per_bar, 4);
        assert_eq!(g.downbeats_ms.len(), 100);
        assert!(g.downbeats_ms.iter().all(|&x| (x - 1000) % 2000 == 0));
        assert!(g.agreement > 0.7 && g.agreement < 0.8, "{}", g.agreement);
    }

    #[test]
    fn missed_and_doubled_beats_are_repaired() {
        let full = times(100, 0.5);
        let mut b: Vec<f64> = full
            .iter()
            .enumerate()
            .filter(|(i, _)| *i != 10 && *i != 50 && *i != 51)
            .map(|(_, &t)| t)
            .collect();
        b.push(30.0 * 0.5 + 1.0 + 0.24);
        b.sort_by(f64::total_cmp);
        let g = build_grid(&b, &every(&full, 4, 0));
        assert_eq!(g.beats_ms.len(), 100, "{:?}", g.beats_ms);
        assert_eq!(g.downbeats_ms.len(), 25);
    }

    #[test]
    fn three_four_is_found_and_phase_follows_a_break() {
        let mut b = times(120, 0.4);
        // A 6 s break, then the grid resumes on an offset.
        let resume = b[b.len() - 1] + 6.0;
        b.extend((0..120).map(|i| resume + 0.1 + i as f64 * 0.4));
        let d: Vec<f64> = every(&b[..120], 3, 0)
            .into_iter()
            .chain(every(&b[120..], 3, 1))
            .collect();
        let g = build_grid(&b, &d);
        assert_eq!(g.beats_per_bar, 3);
        assert_eq!(g.downbeats_ms.len(), 80);
        assert!(g.agreement > 0.99);
    }

    #[test]
    fn sparse_or_jittery_grids_are_not_trusted() {
        let b = times(10, 0.5);
        let g = build_grid(&b, &every(&b, 4, 0));
        assert_eq!(confidence(&g, (0, 10_000)), 0.0);
        assert_eq!(bpm(&g.beats_ms), None);
        let b: Vec<f64> = times(400, 0.5)
            .into_iter()
            .enumerate()
            .map(|(i, t)| if i % 3 == 0 { t + 0.1 } else { t })
            .collect();
        let g = build_grid(&b, &every(&b, 4, 0));
        let c = confidence(&g, (1000, 200_000));
        assert!(c < 0.5, "{c}");
    }

    #[test]
    fn peaks_refine_between_frames() {
        // Logits symmetric about frame 10.25 -> vertex at 10.25.
        let l: Vec<f32> = (0..20)
            .map(|k| -((k as f32 - 10.25) * (k as f32 - 10.25)))
            .collect();
        let t = refine_peak(10.0 / 50.0, &l);
        assert!((t * 50.0 - 10.25).abs() < 1e-6, "{}", t * 50.0);
        // Already fractional (deduplicated) peaks are kept.
        assert!((refine_peak(10.5 / 50.0, &l) * 50.0 - 10.5).abs() < 1e-4);
    }
}
