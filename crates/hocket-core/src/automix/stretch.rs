//! Time-stretch for the outgoing tail: pure-Rust WSOLA (waveform-similarity
//! overlap-add) with a ramped ratio.
//!
//! Why not `signalsmith-stretch` (better quality, MIT): its Rust wrapper runs
//! `bindgen` at build time, which needs libclang on every machine that
//! builds the core (dev hosts, CI, the Android cross build), and links a C++
//! STL into the Android cdylib. The ratios here are small (±8 %) and short
//! (a few bars), where WSOLA's artefacts are minor, so the dependency isn't
//! worth it. See `docs/automix.md` → *Renderer*.
//!
//! The ratio is *speed*: source time consumed per output time. It moves
//! linearly from 1.0 at the tail start to `ratio_end` over `ramp_ms` of
//! output time, then holds. [`source_ms`] is the exact integral, shared with
//! the planner so `TransitionPlan::tail_source_ms` matches what the renderer
//! consumes.

use crate::api::TempoRamp;

/// Ratios are clamped to this range (the planner stays within ±8 %).
pub const MIN_RATIO: f64 = 0.85;
pub const MAX_RATIO: f64 = 1.15;
/// Analysis window (ms); frames overlap by half.
const WINDOW_MS: f64 = 40.0;
/// Search range around the nominal position (ms, each way).
const TOLERANCE_MS: f64 = 8.0;
/// Coarse search step and correlation decimation.
const COARSE: usize = 4;

fn ratio_end(tempo: Option<&TempoRamp>) -> f64 {
    match tempo {
        Some(t) if (t.ratio_end as f64).is_finite() => {
            (t.ratio_end as f64).clamp(MIN_RATIO, MAX_RATIO)
        }
        _ => 1.0,
    }
}

/// Source time consumed after `out` units of output time (any unit, as long
/// as `ramp_len` uses the same one).
fn integral(r: f64, ramp_len: f64, out: f64) -> f64 {
    if out <= 0.0 {
        return 0.0;
    }
    if ramp_len <= 0.0 {
        return r * out;
    }
    if out <= ramp_len {
        out + (r - 1.0) * out * out / (2.0 * ramp_len)
    } else {
        ramp_len * (1.0 + r) / 2.0 + r * (out - ramp_len)
    }
}

/// Source milliseconds consumed by `out_ms` of output under `tempo`.
pub fn source_ms(tempo: Option<&TempoRamp>, out_ms: f64) -> f64 {
    let ramp = tempo.map(|t| t.ramp_ms as f64).unwrap_or(0.0);
    integral(ratio_end(tempo), ramp, out_ms)
}

/// The playback ratio at output time `out_ms`.
pub fn ratio_at(tempo: Option<&TempoRamp>, out_ms: f64) -> f64 {
    let r = ratio_end(tempo);
    match tempo {
        Some(t) if t.ramp_ms > 0 && out_ms < t.ramp_ms as f64 => {
            1.0 + (r - 1.0) * (out_ms.max(0.0) / t.ramp_ms as f64)
        }
        Some(_) => r,
        None => 1.0,
    }
}

/// Stretch interleaved `input` into exactly `out_frames` frames under
/// `tempo`. Input past its end reads as silence. With no tempo (or a ratio
/// of exactly 1) the output is the input, sample for sample.
pub fn stretch(
    input: &[f32],
    channels: usize,
    rate: u32,
    tempo: Option<&TempoRamp>,
    out_frames: usize,
) -> Vec<f32> {
    let channels = channels.max(1);
    let in_frames = input.len() / channels;
    let r = ratio_end(tempo);
    if r == 1.0 {
        let mut out = vec![0.0; out_frames * channels];
        let n = out_frames.min(in_frames) * channels;
        out[..n].copy_from_slice(&input[..n]);
        return out;
    }
    let rate = if rate == 0 { 48_000.0 } else { rate as f64 };
    let ramp_frames = tempo
        .map(|t| t.ramp_ms as f64 * rate / 1000.0)
        .unwrap_or(0.0);

    let n = (((WINDOW_MS * rate / 1000.0) as usize) / 2 * 2).max(16);
    let hop = n / 2;
    let tol = ((TOLERANCE_MS * rate / 1000.0) as usize).max(1);
    // Periodic Hann: overlapping by half, the windows sum to exactly 1.
    let window: Vec<f32> = (0..n)
        .map(|i| (0.5 - 0.5 * (2.0 * std::f64::consts::PI * i as f64 / n as f64).cos()) as f32)
        .collect();
    let mono: Vec<f32> = input
        .chunks_exact(channels)
        .map(|f| f.iter().sum::<f32>())
        .collect();
    let mono_at = |i: usize| mono.get(i).copied().unwrap_or(0.0);

    let mut out = vec![0.0f32; (out_frames + n) * channels];
    let mut prev: usize = 0;
    let mut k = 0usize;
    while k * hop < out_frames {
        let nominal = integral(r, ramp_frames, (k * hop) as f64).round() as usize;
        let pos = if k == 0 {
            0
        } else {
            let natural = prev + hop;
            best_offset(&mono_at, natural, nominal, tol, hop)
        };
        let out_base = k * hop;
        for (i, &wi) in window.iter().enumerate() {
            // The first frame is flat on its leading half so the tail starts
            // exactly where normal playback left off.
            let w = if k == 0 && i < hop { 1.0 } else { wi };
            let src = pos + i;
            if src >= in_frames {
                break;
            }
            let o = (out_base + i) * channels;
            let s = src * channels;
            for c in 0..channels {
                out[o + c] += w * input[s + c];
            }
        }
        prev = pos;
        k += 1;
    }
    out.truncate(out_frames * channels);
    out
}

/// The candidate start in `nominal ± tol` whose first `len` frames best
/// match the natural continuation at `natural` (normalised cross-correlation
/// on the mono mix; coarse decimated search, then a fine one around the best).
fn best_offset(
    mono: &impl Fn(usize) -> f32,
    natural: usize,
    nominal: usize,
    tol: usize,
    len: usize,
) -> usize {
    let lo = nominal.saturating_sub(tol);
    let hi = nominal + tol;
    let score = |cand: usize, step: usize| -> f64 {
        let (mut xy, mut xx, mut yy) = (0.0f64, 0.0f64, 0.0f64);
        let mut i = 0;
        while i < len {
            let x = mono(cand + i) as f64;
            let y = mono(natural + i) as f64;
            xy += x * y;
            xx += x * x;
            yy += y * y;
            i += step;
        }
        if xx <= 1e-12 || yy <= 1e-12 {
            // Silence: no preference, other than staying on the nominal path.
            return 0.0;
        }
        xy / (xx * yy).sqrt()
    };
    let pick = |range: &mut dyn Iterator<Item = usize>, step: usize| -> usize {
        let mut best = nominal.clamp(lo, hi);
        let mut best_score = f64::NEG_INFINITY;
        for cand in range {
            let s = score(cand, step);
            // Ties go to the candidate nearest the nominal position.
            let closer = cand.abs_diff(nominal) < best.abs_diff(nominal);
            if s > best_score + 1e-9 || ((s - best_score).abs() <= 1e-9 && closer) {
                best = cand;
                best_score = s;
            }
        }
        best
    };
    let coarse = pick(&mut (lo..=hi).step_by(COARSE), COARSE);
    let fine_lo = coarse.saturating_sub(COARSE - 1).max(lo);
    let fine_hi = (coarse + COARSE - 1).min(hi);
    pick(&mut (fine_lo..=fine_hi), 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: u32 = 48_000;

    /// Mono click track: a 1 kHz burst with a 15 ms decay every `period_ms`.
    fn clicks(period_ms: f64, total_ms: f64) -> Vec<f32> {
        let frames = (total_ms * RATE as f64 / 1000.0) as usize;
        let period = period_ms * RATE as f64 / 1000.0;
        (0..frames)
            .map(|i| {
                let t = (i as f64 % period) / RATE as f64;
                (0.8 * (-t / 0.015).exp() * (2.0 * std::f64::consts::PI * 1000.0 * t).sin()) as f32
            })
            .collect()
    }

    /// Onset times (ms) of bursts: envelope crossing half of full scale after quiet.
    fn onsets(x: &[f32]) -> Vec<f64> {
        let mut out = Vec::new();
        let mut env = 0.0f32;
        let mut armed = true;
        for (i, s) in x.iter().enumerate() {
            env = env.max(s.abs()) * 0.999;
            if armed && env > 0.4 {
                out.push(i as f64 * 1000.0 / RATE as f64);
                armed = false;
            }
            if env < 0.05 {
                armed = true;
            }
        }
        out
    }

    #[test]
    fn source_integral() {
        let t = TempoRamp {
            ratio_end: 1.05,
            ramp_ms: 2_000,
        };
        // ratio_end is an f32: 1.05 is stored as 1.04999995.
        assert!((source_ms(Some(&t), 2_000.0) - 2_050.0).abs() < 1e-3);
        assert!((source_ms(Some(&t), 4_000.0) - 4_150.0).abs() < 1e-3);
        assert!((source_ms(None, 4_000.0) - 4_000.0).abs() < 1e-9);
        assert!((ratio_at(Some(&t), 1_000.0) - 1.025).abs() < 1e-6);
        assert!((ratio_at(Some(&t), 9_000.0) - 1.05).abs() < 1e-6);
    }

    #[test]
    fn unity_is_identity() {
        let x = clicks(500.0, 2_000.0);
        let y = stretch(&x, 1, RATE, None, x.len() + 100);
        assert_eq!(&y[..x.len()], &x[..]);
        assert!(y[x.len()..].iter().all(|&s| s == 0.0));
    }

    #[test]
    fn constant_ratio_moves_the_beats() {
        // 120 BPM sped up 5 %: beats every 476.2 ms.
        let x = clicks(500.0, 20_000.0);
        let t = TempoRamp {
            ratio_end: 1.05,
            ramp_ms: 0,
        };
        let out_frames = (15_000.0 * RATE as f64 / 1000.0) as usize;
        let y = stretch(&x, 1, RATE, Some(&t), out_frames);
        assert_eq!(y.len(), out_frames);
        assert!(y.iter().all(|s| s.is_finite()));
        let on = onsets(&y);
        assert!(on.len() >= 30, "{} onsets", on.len());
        let gaps: Vec<f64> = on.windows(2).map(|w| w[1] - w[0]).collect();
        let mean = gaps.iter().sum::<f64>() / gaps.len() as f64;
        assert!((mean - 500.0 / 1.05).abs() < 2.0, "mean spacing {mean}");
        for g in &gaps {
            assert!((g - 500.0 / 1.05).abs() < 12.0, "gap {g}");
        }
        // Beat n lands where the integral says it should.
        let expected_last = (on.len() - 1) as f64 * 500.0 / 1.05;
        assert!((on[on.len() - 1] - on[0] - expected_last).abs() < 12.0);
    }

    #[test]
    fn slowing_down_also_works() {
        let x = clicks(500.0, 20_000.0);
        let t = TempoRamp {
            ratio_end: 0.95,
            ramp_ms: 0,
        };
        let y = stretch(&x, 1, RATE, Some(&t), 15 * RATE as usize);
        let on = onsets(&y);
        let gaps: Vec<f64> = on.windows(2).map(|w| w[1] - w[0]).collect();
        let mean = gaps.iter().sum::<f64>() / gaps.len() as f64;
        assert!((mean - 500.0 / 0.95).abs() < 2.0, "mean spacing {mean}");
    }

    #[test]
    fn ramped_ratio_follows_the_integral() {
        let x = clicks(500.0, 30_000.0);
        let t = TempoRamp {
            ratio_end: 1.06,
            ramp_ms: 8_000,
        };
        let y = stretch(&x, 1, RATE, Some(&t), 20 * RATE as usize);
        let on = onsets(&y);
        // Each output onset maps back to a source beat on the 500 ms grid.
        for o in &on {
            let src = source_ms(Some(&t), *o);
            let off = (src / 500.0 - (src / 500.0).round()).abs() * 500.0;
            assert!(off < 12.0, "onset {o} maps to source {src}");
        }
        // Early gaps are near 500 ms, late ones near 500/1.06.
        let gaps: Vec<f64> = on.windows(2).map(|w| w[1] - w[0]).collect();
        assert!((gaps[0] - 500.0).abs() < 15.0, "{}", gaps[0]);
        let last = gaps[gaps.len() - 1];
        assert!((last - 500.0 / 1.06).abs() < 15.0, "{last}");
    }

    #[test]
    fn stereo_and_short_input() {
        let mono = clicks(500.0, 1_000.0);
        let stereo: Vec<f32> = mono.iter().flat_map(|&s| [s, -s]).collect();
        let t = TempoRamp {
            ratio_end: 1.04,
            ramp_ms: 500,
        };
        // Asking for more than the input holds pads with silence.
        let y = stretch(&stereo, 2, RATE, Some(&t), 2 * RATE as usize);
        assert_eq!(y.len(), 4 * RATE as usize);
        assert!(y.iter().all(|s| s.is_finite()));
        assert!(y[y.len() - 2_000..].iter().all(|&s| s == 0.0));
        // Channels stay channels.
        assert!(y
            .as_chunks::<2>()
            .0
            .iter()
            .all(|f| (f[0] + f[1]).abs() < 1e-6));
        // Start is seamless: the first half-window is the input itself.
        assert_eq!(&y[..400], &stereo[..400]);
    }
}
