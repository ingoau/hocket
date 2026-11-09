//! Turning a beat tracker's raw output into the record's `bpm` and
//! `beat_confidence`. Pure, so it is tested without the model.

use crate::api::Ms;

/// Fewer beats than this give confidence 0 (not a grid worth trusting).
pub const MIN_BEATS: usize = 16;
/// Neighbouring beat intervals each side the local tempo is taken over.
const LOCAL_WINDOW: usize = 8;
/// A beat interval is steady within this fraction of the local median.
const BEAT_TOLERANCE: f64 = 0.08;

fn median(mut v: Vec<f64>) -> Option<f64> {
    if v.is_empty() {
        return None;
    }
    v.sort_unstable_by(f64::total_cmp);
    Some(v[v.len() / 2])
}

fn intervals(ms: &[Ms]) -> Vec<f64> {
    ms.windows(2)
        .map(|w| w[1].saturating_sub(w[0]) as f64)
        .collect()
}

/// Tempo from the median beat interval, to 0.1 BPM.
pub fn bpm(beats: &[Ms]) -> Option<f32> {
    if beats.len() < MIN_BEATS {
        return None;
    }
    let m = median(intervals(beats))?;
    (m > 0.0).then(|| ((60_000.0 / m) * 10.0).round() as f32 / 10.0)
}

/// Share of beat intervals within [`BEAT_TOLERANCE`] of the median of their
/// neighbours: how steady the grid is, tolerant of slow tempo drift.
pub fn beat_steadiness(beats: &[Ms]) -> f64 {
    let iv = intervals(beats);
    if iv.is_empty() {
        return 0.0;
    }
    let steady = (0..iv.len())
        .filter(|&i| {
            let lo = i.saturating_sub(LOCAL_WINDOW);
            let hi = (i + LOCAL_WINDOW + 1).min(iv.len());
            let local = median(iv[lo..hi].to_vec()).unwrap_or(0.0);
            local > 0.0 && ((iv[i] - local) / local).abs() <= BEAT_TOLERANCE
        })
        .count();
    steady as f64 / iv.len() as f64
}

/// Share of bars holding the modal number of beats (a consistent metre).
pub fn bar_consistency(beats: &[Ms], downbeats: &[Ms]) -> f64 {
    let mut counts = [0usize; 17];
    let mut bars = 0usize;
    for w in downbeats.windows(2) {
        let lo = beats.partition_point(|&b| b < w[0]);
        let hi = beats.partition_point(|&b| b < w[1]);
        counts[(hi - lo).min(16)] += 1;
        bars += 1;
    }
    if bars == 0 {
        return 0.0;
    }
    counts[1..].iter().copied().max().unwrap_or(0) as f64 / bars as f64
}

/// How far the beat grid can be trusted, 0..1: beat steadiness × metre
/// consistency, scaled down when the beats cover only part of the audible
/// span (half credit for coverage, so a long ambient intro costs some
/// confidence but not all; the planner judges each end separately anyway).
pub fn confidence(beats: &[Ms], downbeats: &[Ms], audible: (Ms, Ms)) -> f32 {
    if beats.len() < MIN_BEATS || downbeats.len() < 4 {
        return 0.0;
    }
    let span = audible.1.saturating_sub(audible.0).max(1) as f64;
    let covered = beats[beats.len() - 1].saturating_sub(beats[0]) as f64;
    let coverage = (covered / span).clamp(0.0, 1.0);
    let c = beat_steadiness(beats) * bar_consistency(beats, downbeats) * (0.5 + 0.5 * coverage);
    ((c * 1000.0).round() / 1000.0) as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grid(n: usize, ibi: f64, per_bar: usize) -> (Vec<Ms>, Vec<Ms>) {
        let beats: Vec<Ms> = (0..n).map(|i| (1000.0 + i as f64 * ibi) as Ms).collect();
        let downs = beats.iter().copied().step_by(per_bar).collect();
        (beats, downs)
    }

    #[test]
    fn steady_four_four_is_confident() {
        let (b, d) = grid(400, 500.0, 4);
        assert_eq!(bpm(&b), Some(120.0));
        let c = confidence(&b, &d, (1000, 1000 + 399 * 500));
        assert!(c > 0.99, "{c}");
    }

    #[test]
    fn jittery_or_sparse_grids_are_not() {
        let (mut b, d) = grid(400, 500.0, 4);
        // Every third interval 20 % off.
        for (i, x) in b.iter_mut().enumerate() {
            if i % 3 == 0 {
                *x += 100;
            }
        }
        let jitter = confidence(&b, &d, (1000, 200_000));
        assert!(jitter < 0.5, "{jitter}");
        let (b, d) = grid(10, 500.0, 4);
        assert_eq!(confidence(&b, &d, (0, 10_000)), 0.0);
        assert_eq!(bpm(&b), None);
    }

    #[test]
    fn partial_coverage_and_mixed_metre_cost_confidence() {
        let (b, d) = grid(200, 500.0, 4);
        let half = confidence(&b, &d, (1000, 1000 + 2 * 199 * 500));
        assert!((0.7..0.8).contains(&half), "{half}");
        let (b, _) = grid(200, 500.0, 4);
        // Alternate bars of 3 and 4 beats.
        let mut d = Vec::new();
        let mut i = 0;
        let mut three = true;
        while i < b.len() {
            d.push(b[i]);
            i += if three { 3 } else { 4 };
            three = !three;
        }
        let mixed = confidence(&b, &d, (1000, 1000 + 199 * 500));
        assert!(mixed < 0.6, "{mixed}");
    }
}
