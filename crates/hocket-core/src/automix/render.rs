//! The DSP of a transition. Pure: PCM in, PCM out.
//!
//! - [`render_tail`]: A's audio from `tail_start_ms`, time-stretched by the
//!   plan's tempo ramp, through the high-shelf and low-shelf automation and
//!   the outgoing fade. Rendered ahead of time, in one go.
//! - [`IncomingShaper`]: B's side, applied block by block as B plays: the
//!   incoming fade, `incoming_gain_db` and the mirrored bass swap.
//!
//! The backend sums the two and runs the existing limiter. Everything is
//! computed per absolute frame (filter coefficients move every
//! [`AUTOMATION_BLOCK`] frames of the absolute timeline), so the output
//! doesn't depend on how a caller slices its blocks.

use crate::api::{FadeCurve, ShelfMove, TransitionPlan};
use crate::audio::dsp::biquad::{Biquad, BiquadKind, Coefficients};

use super::stretch::stretch;

/// Filter coefficients are recomputed every this many frames.
pub const AUTOMATION_BLOCK: usize = 32;
/// Shelf filter Q (RBJ); 1/√2 gives a smooth, non-resonant shelf.
const SHELF_Q: f64 = std::f64::consts::FRAC_1_SQRT_2;

/// Gain of `curve` at overlap progress `p` (clamped to 0..=1). `incoming`
/// selects the rising side.
pub fn fade_gain(curve: FadeCurve, p: f64, incoming: bool) -> f64 {
    let p = if p.is_finite() {
        p.clamp(0.0, 1.0)
    } else {
        1.0
    };
    let x = if incoming { p } else { 1.0 - p };
    match curve {
        FadeCurve::Linear => x,
        // sin keeps the endpoints exact (sin 0 = 0, sin π/2 = 1).
        FadeCurve::EqualPower => (x * std::f64::consts::FRAC_PI_2).sin(),
    }
}

/// Frames in `ms` at `rate`.
pub fn ms_to_frames(ms: f64, rate: u32) -> usize {
    (ms.max(0.0) * rate as f64 / 1000.0).round() as usize
}

/// One shelving filter whose gain follows a [`ShelfMove`].
#[derive(Debug, Clone)]
struct ShelfAutomation {
    kind: BiquadKind,
    mv: ShelfMove,
    /// Run the move backwards (B's side of a bass swap).
    mirrored: bool,
    rate: f64,
    filter: Biquad,
}

impl ShelfAutomation {
    fn new(kind: BiquadKind, mv: ShelfMove, mirrored: bool, channels: usize, rate: u32) -> Self {
        let rate = rate as f64;
        let mut s = Self {
            kind,
            mv,
            mirrored,
            rate,
            filter: Biquad::identity(channels),
        };
        s.set_gain(s.gain_db_at(0.0));
        s
    }

    /// Gain in dB at output time `t_ms` (from the tail start).
    fn gain_db_at(&self, t_ms: f64) -> f64 {
        let (from, to) = if self.mirrored {
            (self.mv.to_db as f64, self.mv.from_db as f64)
        } else {
            (self.mv.from_db as f64, self.mv.to_db as f64)
        };
        let start = self.mv.start_ms as f64;
        let len = self.mv.len_ms as f64;
        if t_ms <= start {
            from
        } else if len <= 0.0 || t_ms >= start + len {
            to
        } else {
            from + (to - from) * (t_ms - start) / len
        }
    }

    fn end_ms(&self) -> f64 {
        self.mv.start_ms as f64 + self.mv.len_ms as f64
    }

    fn set_gain(&mut self, db: f64) {
        let c = if db == 0.0 {
            Coefficients::IDENTITY
        } else {
            Coefficients::design(self.kind, self.rate, self.mv.freq_hz as f64, SHELF_Q, db)
        };
        self.filter.set_coefficients(c);
    }

    /// Process `frames` (interleaved) whose first frame is absolute frame
    /// `first` of a timeline that starts at output time `origin_ms`.
    fn process(&mut self, frames: &mut [f32], channels: usize, first: usize, origin_ms: f64) {
        for (j, frame) in frames.chunks_exact_mut(channels).enumerate() {
            let f = first + j;
            if f.is_multiple_of(AUTOMATION_BLOCK) {
                let t = origin_ms + f as f64 * 1000.0 / self.rate;
                self.set_gain(self.gain_db_at(t));
            }
            for (c, s) in frame.iter_mut().enumerate() {
                *s = self.filter.tick(c, *s as f64) as f32;
            }
        }
    }
}

fn sane_rate(rate: u32) -> u32 {
    if rate == 0 {
        48_000
    } else {
        rate
    }
}

/// Render the outgoing tail. `pcm` is A from `tail_start_ms` (interleaved,
/// at `rate`, already through the device's DSP chain except volume); it
/// should hold at least `tail_source_ms`, and anything missing plays as
/// silence. Returns exactly `tail_len_ms` of audio at `rate`.
pub fn render_tail(plan: &TransitionPlan, pcm: &[f32], channels: usize, rate: u32) -> Vec<f32> {
    let channels = channels.max(1);
    let rate = sane_rate(rate);
    let out_frames = ms_to_frames(plan.tail_len_ms as f64, rate);
    let mut out = stretch(pcm, channels, rate, plan.tempo.as_ref(), out_frames);

    if let Some(mv) = plan.high_cut {
        ShelfAutomation::new(BiquadKind::HighShelf, mv, false, channels, rate)
            .process(&mut out, channels, 0, 0.0);
    }
    if let Some(mv) = plan.bass_swap {
        ShelfAutomation::new(BiquadKind::LowShelf, mv, false, channels, rate)
            .process(&mut out, channels, 0, 0.0);
    }

    let start = plan.incoming_at_ms.min(plan.tail_len_ms) as f64;
    let len = plan.tail_len_ms as f64 - start;
    for (f, frame) in out.chunks_exact_mut(channels).enumerate() {
        let t = f as f64 * 1000.0 / rate as f64;
        if t < start {
            continue;
        }
        let p = if len > 0.0 { (t - start) / len } else { 1.0 };
        let g = fade_gain(plan.out_curve, p, false) as f32;
        for s in frame {
            *s *= g;
        }
    }
    out
}

/// B's side of a transition, applied in place to B's frames from its first
/// played sample on. Stateful; feed it consecutive blocks of any size.
#[derive(Debug, Clone)]
pub struct IncomingShaper {
    channels: usize,
    rate: u32,
    curve: FadeCurve,
    /// Output time (from the tail start) of B's first frame.
    origin_ms: f64,
    overlap_ms: f64,
    gain_db: f64,
    bass: Option<ShelfAutomation>,
    /// Frames processed so far.
    frame: usize,
}

impl IncomingShaper {
    pub fn new(plan: &TransitionPlan, channels: usize, rate: u32) -> Self {
        let channels = channels.max(1);
        let rate = sane_rate(rate);
        let origin_ms = plan.incoming_at_ms.min(plan.tail_len_ms) as f64;
        let bass = plan.bass_swap.map(|mv| {
            let mut s = ShelfAutomation::new(BiquadKind::LowShelf, mv, true, channels, rate);
            s.set_gain(s.gain_db_at(origin_ms));
            s
        });
        let gain_db = if plan.incoming_gain_db.is_finite() {
            (plan.incoming_gain_db as f64).clamp(-24.0, 12.0)
        } else {
            0.0
        };
        Self {
            channels,
            rate,
            curve: plan.in_curve,
            origin_ms,
            overlap_ms: plan.tail_len_ms as f64 - origin_ms,
            gain_db,
            bass,
            frame: 0,
        }
    }

    fn elapsed_ms(&self, frame: usize) -> f64 {
        frame as f64 * 1000.0 / self.rate as f64
    }

    /// The overlap and every automation are over: B passes through untouched.
    pub fn is_done(&self) -> bool {
        let t = self.origin_ms + self.elapsed_ms(self.frame);
        t >= self.origin_ms + self.overlap_ms && self.bass.as_ref().is_none_or(|b| t >= b.end_ms())
    }

    /// Shape one block of B (interleaved, `channels` per frame).
    pub fn process(&mut self, samples: &mut [f32]) {
        if self.is_done() {
            self.frame += samples.len() / self.channels;
            return;
        }
        let first = self.frame;
        if let Some(b) = self.bass.as_mut() {
            // ShelfAutomation counts frames on its own timeline: B's frame 0 is at origin_ms.
            b.process(samples, self.channels, first, self.origin_ms);
        }
        for (j, frame) in samples.chunks_exact_mut(self.channels).enumerate() {
            let t = self.elapsed_ms(first + j);
            if t >= self.overlap_ms {
                break;
            }
            let p = if self.overlap_ms > 0.0 {
                t / self.overlap_ms
            } else {
                1.0
            };
            let mut g = fade_gain(self.curve, p, true);
            if self.gain_db != 0.0 {
                g *= 10f64.powf(self.gain_db * (1.0 - p) / 20.0);
            }
            let g = g as f32;
            for s in frame {
                *s *= g;
            }
        }
        self.frame += samples.len() / self.channels;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::{TempoRamp, TransitionKind};

    const RATE: u32 = 44_100;

    fn plan() -> TransitionPlan {
        TransitionPlan {
            kind: TransitionKind::TempoBlend,
            tail_start_ms: 100_000,
            tail_source_ms: 0,
            tail_len_ms: 12_000,
            incoming_at_ms: 4_000,
            incoming_from_ms: 0,
            handover_ms: 8_000,
            out_curve: FadeCurve::EqualPower,
            in_curve: FadeCurve::EqualPower,
            tempo: Some(TempoRamp {
                ratio_end: 1.03,
                ramp_ms: 4_000,
            }),
            bass_swap: Some(ShelfMove {
                freq_hz: 100.0,
                from_db: 0.0,
                to_db: -26.0,
                start_ms: 7_500,
                len_ms: 1_000,
            }),
            high_cut: Some(ShelfMove {
                freq_hz: 6_000.0,
                from_db: 0.0,
                to_db: -9.0,
                start_ms: 8_000,
                len_ms: 4_000,
            }),
            incoming_gain_db: -2.0,
        }
    }

    /// Stereo noise-ish signal: a few sines including bass.
    fn music(ms: u32) -> Vec<f32> {
        let frames = ms_to_frames(ms as f64, RATE);
        (0..frames)
            .flat_map(|i| {
                let t = i as f64 / RATE as f64;
                let s = 0.3 * (2.0 * std::f64::consts::PI * 60.0 * t).sin()
                    + 0.2 * (2.0 * std::f64::consts::PI * 440.0 * t).sin()
                    + 0.1 * (2.0 * std::f64::consts::PI * 8_000.0 * t).sin();
                [s as f32, s as f32]
            })
            .collect()
    }

    fn rms(x: &[f32]) -> f64 {
        (x.iter().map(|&s| (s as f64).powi(2)).sum::<f64>() / x.len().max(1) as f64).sqrt()
    }

    #[test]
    fn fade_curves() {
        for c in [FadeCurve::EqualPower, FadeCurve::Linear] {
            assert_eq!(fade_gain(c, 0.0, false), 1.0);
            assert_eq!(fade_gain(c, 1.0, false), 0.0);
            assert_eq!(fade_gain(c, 0.0, true), 0.0);
            assert_eq!(fade_gain(c, 1.0, true), 1.0);
        }
        let a = fade_gain(FadeCurve::EqualPower, 0.5, false);
        let b = fade_gain(FadeCurve::EqualPower, 0.5, true);
        assert!((a * a + b * b - 1.0).abs() < 1e-12, "constant power");
        assert_eq!(fade_gain(FadeCurve::Linear, 0.25, true), 0.25);
    }

    #[test]
    fn tail_has_planned_length_and_fades_out() {
        let p = plan();
        let pcm = music(14_000);
        let out = render_tail(&p, &pcm, 2, RATE);
        assert_eq!(out.len(), ms_to_frames(12_000.0, RATE) * 2);
        assert!(out.iter().all(|s| s.is_finite()));
        // Before the overlap it's still loud; the last 10 ms is (nearly) silent.
        let early = rms(&out[..ms_to_frames(1_000.0, RATE) * 2]);
        let last = rms(&out[out.len() - ms_to_frames(10.0, RATE) * 2..]);
        assert!(early > 0.2, "{early}");
        assert!(last < 0.002, "{last}");
        // The very start is A untouched (no stretch yet, shelves at 0 dB, before the overlap).
        assert_eq!(&out[..64], &pcm[..64]);
    }

    #[test]
    fn short_or_empty_input_is_padded() {
        let p = plan();
        let out = render_tail(&p, &music(500), 2, RATE);
        assert_eq!(out.len(), ms_to_frames(12_000.0, RATE) * 2);
        assert!(out[out.len() / 2..].iter().all(|&s| s == 0.0));
        let out = render_tail(&p, &[], 2, RATE);
        assert_eq!(out.len(), ms_to_frames(12_000.0, RATE) * 2);
        let mut zero = p.clone();
        zero.tail_len_ms = 0;
        zero.incoming_at_ms = 0;
        assert!(render_tail(&zero, &music(500), 2, RATE).is_empty());
    }

    #[test]
    fn bass_swap_cuts_the_outgoing_bass() {
        let mut p = plan();
        p.tempo = None;
        p.high_cut = None;
        p.out_curve = FadeCurve::Linear;
        p.incoming_at_ms = 11_000; // fade only in the last second
                                   // Pure 60 Hz in: after the swap the tail's level drops by ~26 dB.
        let frames = ms_to_frames(13_000.0, RATE);
        let pcm: Vec<f32> = (0..frames)
            .flat_map(|i| {
                let s = (0.5 * (2.0 * std::f64::consts::PI * 60.0 * i as f64 / RATE as f64).sin())
                    as f32;
                [s, s]
            })
            .collect();
        let out = render_tail(&p, &pcm, 2, RATE);
        let seg = |a: f64, b: f64| rms(&out[ms_to_frames(a, RATE) * 2..ms_to_frames(b, RATE) * 2]);
        let before = seg(6_000.0, 7_000.0);
        let after = seg(9_500.0, 10_500.0);
        let drop_db = 20.0 * (after / before).log10();
        assert!(drop_db < -20.0, "drop {drop_db} dB");
    }

    fn shaped(p: &TransitionPlan, input: &[f32], block_frames: usize) -> Vec<f32> {
        let mut s = IncomingShaper::new(p, 2, RATE);
        let mut out = input.to_vec();
        for chunk in out.chunks_mut(block_frames * 2) {
            s.process(chunk);
        }
        out
    }

    #[test]
    fn shaper_is_block_size_independent() {
        let p = plan();
        let b = music(10_000);
        let reference = shaped(&p, &b, 1);
        for block in [7, 32, 37, 512, 4_096, 100_000] {
            assert_eq!(shaped(&p, &b, block), reference, "block {block}");
        }
        assert!(reference.iter().all(|s| s.is_finite()));
    }

    #[test]
    fn shaper_fades_in_and_then_passes_through() {
        let p = plan();
        let b = music(12_000);
        let out = shaped(&p, &b, 256);
        // Starts silent, and after the overlap (8 s of B) and the swap it is B unchanged.
        assert!(rms(&out[..ms_to_frames(5.0, RATE) * 2]) < 1e-3);
        let tail_from = ms_to_frames(9_000.0, RATE) * 2;
        assert_eq!(&out[tail_from..], &b[tail_from..]);
        // Its bass is cut before the swap (B's side mirrors A's: −26 dB until 7.5 s).
        let mut s = IncomingShaper::new(&p, 2, RATE);
        assert!(!s.is_done());
        let mut buf = b.clone();
        s.process(&mut buf);
        assert!(s.is_done());
    }

    #[test]
    fn overlap_sums_to_roughly_constant_power() {
        // Uncorrelated noise on both sides, plain equal-power fade, no EQ.
        let mut p = plan();
        p.tempo = None;
        p.bass_swap = None;
        p.high_cut = None;
        p.incoming_gain_db = 0.0;
        let mut seed = 1u32;
        let mut noise = |n: usize| -> Vec<f32> {
            (0..n)
                .map(|_| {
                    seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                    (seed >> 8) as f32 / (1u32 << 24) as f32 - 0.5
                })
                .collect()
        };
        let a = noise(ms_to_frames(12_000.0, RATE) * 2);
        let b = noise(ms_to_frames(8_000.0, RATE) * 2);
        let tail = render_tail(&p, &a, 2, RATE);
        let inc = shaped(&p, &b, 1_024);
        let off = ms_to_frames(4_000.0, RATE) * 2;
        let mix: Vec<f32> = tail[off..].iter().zip(&inc).map(|(x, y)| x + y).collect();
        let base = rms(&a);
        for k in 0..8 {
            let w = &mix[ms_to_frames(k as f64 * 1_000.0, RATE) * 2
                ..ms_to_frames(k as f64 * 1_000.0 + 1_000.0, RATE) * 2];
            let db = 20.0 * (rms(w) / base).log10();
            assert!(db.abs() < 1.0, "second {k}: {db} dB");
        }
    }
}
