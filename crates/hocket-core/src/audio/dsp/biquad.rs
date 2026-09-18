//! RBJ cookbook biquads (Robert Bristow-Johnson, "Cookbook formulae for
//! audio EQ biquad filter coefficients").
//!
//! Coefficients are computed in `f64`, state runs in `f64` (transposed direct
//! form II, one state pair per channel) and samples are `f32` in and out.
//! State is flushed to zero below [`DENORMAL_FLOOR`] every sample so a long
//! silence never drops the filter into denormal arithmetic.

use std::f64::consts::PI;

/// Below this magnitude filter state is snapped to zero (−360 dB; far below
/// anything audible, far above the `f64` denormal range).
pub const DENORMAL_FLOOR: f64 = 1e-18;

/// Filter shapes the chain uses.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BiquadKind {
    /// Peaking EQ: `gain_db` at `freq`, bandwidth from `q`.
    Peaking,
    LowShelf,
    HighShelf,
    LowPass,
    HighPass,
}

/// Normalised coefficients (`a0 == 1`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Coefficients {
    pub b0: f64,
    pub b1: f64,
    pub b2: f64,
    pub a1: f64,
    pub a2: f64,
}

impl Coefficients {
    /// Pass-through.
    pub const IDENTITY: Coefficients = Coefficients { b0: 1.0, b1: 0.0, b2: 0.0, a1: 0.0, a2: 0.0 };

    /// RBJ design. `q` is clamped to a sane range and `freq` to below
    /// Nyquist so extreme settings stay stable instead of blowing up.
    pub fn design(kind: BiquadKind, sample_rate: f64, freq: f64, q: f64, gain_db: f64) -> Coefficients {
        let sample_rate = if sample_rate.is_finite() && sample_rate > 0.0 { sample_rate } else { 48_000.0 };
        let nyquist = sample_rate / 2.0;
        let freq = if freq.is_finite() { freq.clamp(1.0, nyquist * 0.999) } else { 1000.0 };
        let q = if q.is_finite() { q.clamp(0.025, 40.0) } else { 0.7071 };
        let gain_db = if gain_db.is_finite() { gain_db.clamp(-40.0, 40.0) } else { 0.0 };
        let a = 10f64.powf(gain_db / 40.0);
        let w0 = 2.0 * PI * freq / sample_rate;
        let (sin, cos) = w0.sin_cos();
        let alpha = sin / (2.0 * q);
        let (b0, b1, b2, a0, a1, a2) = match kind {
            BiquadKind::Peaking => {
                (1.0 + alpha * a, -2.0 * cos, 1.0 - alpha * a, 1.0 + alpha / a, -2.0 * cos, 1.0 - alpha / a)
            }
            BiquadKind::LowShelf => {
                let sq = 2.0 * a.sqrt() * alpha;
                (
                    a * ((a + 1.0) - (a - 1.0) * cos + sq),
                    2.0 * a * ((a - 1.0) - (a + 1.0) * cos),
                    a * ((a + 1.0) - (a - 1.0) * cos - sq),
                    (a + 1.0) + (a - 1.0) * cos + sq,
                    -2.0 * ((a - 1.0) + (a + 1.0) * cos),
                    (a + 1.0) + (a - 1.0) * cos - sq,
                )
            }
            BiquadKind::HighShelf => {
                let sq = 2.0 * a.sqrt() * alpha;
                (
                    a * ((a + 1.0) + (a - 1.0) * cos + sq),
                    -2.0 * a * ((a - 1.0) + (a + 1.0) * cos),
                    a * ((a + 1.0) + (a - 1.0) * cos - sq),
                    (a + 1.0) - (a - 1.0) * cos + sq,
                    2.0 * ((a - 1.0) - (a + 1.0) * cos),
                    (a + 1.0) - (a - 1.0) * cos - sq,
                )
            }
            BiquadKind::LowPass => ((1.0 - cos) / 2.0, 1.0 - cos, (1.0 - cos) / 2.0, 1.0 + alpha, -2.0 * cos, 1.0 - alpha),
            BiquadKind::HighPass => ((1.0 + cos) / 2.0, -(1.0 + cos), (1.0 + cos) / 2.0, 1.0 + alpha, -2.0 * cos, 1.0 - alpha),
        };
        Coefficients { b0: b0 / a0, b1: b1 / a0, b2: b2 / a0, a1: a1 / a0, a2: a2 / a0 }
    }

    /// Build from raw un-normalised coefficients (used by the K-weighting
    /// filters whose coefficients come from a different derivation).
    pub fn from_raw(b0: f64, b1: f64, b2: f64, a0: f64, a1: f64, a2: f64) -> Coefficients {
        Coefficients { b0: b0 / a0, b1: b1 / a0, b2: b2 / a0, a1: a1 / a0, a2: a2 / a0 }
    }

    /// Magnitude response in dB at `freq` for `sample_rate`, evaluating
    /// `H(e^{jw})` directly. Used by tests and by the EQ's clip-protection
    /// estimate.
    pub fn magnitude_db(&self, freq: f64, sample_rate: f64) -> f64 {
        let w = 2.0 * PI * freq / sample_rate;
        let (s1, c1) = w.sin_cos();
        let (s2, c2) = (2.0 * w).sin_cos();
        // H = (b0 + b1 z^-1 + b2 z^-2) / (1 + a1 z^-1 + a2 z^-2), z^-1 = e^{-jw}
        let num_re = self.b0 + self.b1 * c1 + self.b2 * c2;
        let num_im = -(self.b1 * s1 + self.b2 * s2);
        let den_re = 1.0 + self.a1 * c1 + self.a2 * c2;
        let den_im = -(self.a1 * s1 + self.a2 * s2);
        let num = num_re * num_re + num_im * num_im;
        let den = den_re * den_re + den_im * den_im;
        10.0 * (num / den.max(1e-300)).log10()
    }

    /// Poles inside the unit circle (a necessary and sufficient stability
    /// condition for a second-order section): `|a2| < 1` and `|a1| < 1 + a2`.
    pub fn is_stable(&self) -> bool {
        self.a2.abs() < 1.0 && self.a1.abs() < 1.0 + self.a2
    }
}

/// A biquad with per-channel state.
#[derive(Debug, Clone)]
pub struct Biquad {
    coeffs: Coefficients,
    /// Transposed direct form II state, two per channel.
    state: Vec<[f64; 2]>,
}

impl Biquad {
    pub fn new(coeffs: Coefficients, channels: usize) -> Self {
        Self { coeffs, state: vec![[0.0; 2]; channels.max(1)] }
    }

    pub fn identity(channels: usize) -> Self {
        Self::new(Coefficients::IDENTITY, channels)
    }

    pub fn coefficients(&self) -> &Coefficients {
        &self.coeffs
    }

    /// Swap coefficients without clearing state (smooth parameter changes).
    pub fn set_coefficients(&mut self, coeffs: Coefficients) {
        self.coeffs = coeffs;
    }

    pub fn channels(&self) -> usize {
        self.state.len()
    }

    pub fn set_channels(&mut self, channels: usize) {
        self.state = vec![[0.0; 2]; channels.max(1)];
    }

    pub fn reset(&mut self) {
        for s in &mut self.state {
            *s = [0.0; 2];
        }
    }

    /// Process one sample of one channel.
    #[inline]
    pub fn tick(&mut self, channel: usize, x: f64) -> f64 {
        let c = &self.coeffs;
        let s = &mut self.state[channel];
        let y = c.b0 * x + s[0];
        s[0] = c.b1 * x - c.a1 * y + s[1];
        s[1] = c.b2 * x - c.a2 * y;
        if s[0].abs() < DENORMAL_FLOOR {
            s[0] = 0.0;
        }
        if s[1].abs() < DENORMAL_FLOOR {
            s[1] = 0.0;
        }
        y
    }

    /// Process interleaved frames in place.
    pub fn process_interleaved(&mut self, samples: &mut [f32]) {
        let channels = self.state.len();
        for frame in samples.chunks_exact_mut(channels) {
            for (ch, s) in frame.iter_mut().enumerate() {
                *s = self.tick(ch, f64::from(*s)) as f32;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(freq: f64, rate: f64, n: usize) -> Vec<f32> {
        (0..n).map(|i| (2.0 * PI * freq * i as f64 / rate).sin() as f32).collect()
    }

    fn rms_db(x: &[f32]) -> f64 {
        let ms = x.iter().map(|v| f64::from(*v) * f64::from(*v)).sum::<f64>() / x.len() as f64;
        10.0 * ms.log10()
    }

    #[test]
    fn peaking_band_boosts_its_centre_by_six_db_in_the_time_domain() {
        let rate = 48_000.0;
        let c = Coefficients::design(BiquadKind::Peaking, rate, 1000.0, 1.0, 6.0);
        let mut bq = Biquad::new(c, 1);
        let mut x = sine(1000.0, rate, 48_000);
        let before = rms_db(&x[8000..]);
        bq.process_interleaved(&mut x);
        let after = rms_db(&x[8000..]);
        assert!((after - before - 6.0).abs() < 0.05, "gain {}", after - before);
        // Far from the band the gain is ~0 dB.
        let mut y = sine(100.0, rate, 48_000);
        let b = rms_db(&y[8000..]);
        bq.reset();
        bq.process_interleaved(&mut y);
        assert!((rms_db(&y[8000..]) - b).abs() < 0.2);
    }

    #[test]
    fn frequency_response_matches_design_targets() {
        let rate = 44_100.0;
        let peak = Coefficients::design(BiquadKind::Peaking, rate, 2000.0, 1.4, -9.0);
        assert!((peak.magnitude_db(2000.0, rate) + 9.0).abs() < 1e-6);
        assert!(peak.magnitude_db(20.0, rate).abs() < 0.05);
        let low = Coefficients::design(BiquadKind::LowShelf, rate, 200.0, 0.707, 5.0);
        assert!((low.magnitude_db(10.0, rate) - 5.0).abs() < 0.05, "{}", low.magnitude_db(10.0, rate));
        assert!(low.magnitude_db(10_000.0, rate).abs() < 0.05);
        assert!((low.magnitude_db(200.0, rate) - 2.5).abs() < 0.1, "shelf midpoint is half gain");
        let high = Coefficients::design(BiquadKind::HighShelf, rate, 6000.0, 0.707, -4.0);
        assert!((high.magnitude_db(20_000.0, rate) + 4.0).abs() < 0.1);
        assert!(high.magnitude_db(100.0, rate).abs() < 0.05);
        let lp = Coefficients::design(BiquadKind::LowPass, rate, 1000.0, 0.7071, 0.0);
        assert!((lp.magnitude_db(1000.0, rate) + 3.0).abs() < 0.05);
        assert!(lp.magnitude_db(10_000.0, rate) < -35.0);
        let hp = Coefficients::design(BiquadKind::HighPass, rate, 1000.0, 0.7071, 0.0);
        assert!((hp.magnitude_db(1000.0, rate) + 3.0).abs() < 0.05);
        assert!(hp.magnitude_db(50.0, rate) < -45.0);
    }

    #[test]
    fn stays_stable_at_extreme_q_and_frequency() {
        let rate = 44_100.0;
        for (freq, q, gain) in [
            (20.0, 40.0, 40.0),
            (22_000.0, 0.01, -40.0),
            (1e9, 1e9, 1e9),
            (f64::NAN, f64::INFINITY, f64::NEG_INFINITY),
            (0.0, 0.0, 0.0),
        ] {
            for kind in [BiquadKind::Peaking, BiquadKind::LowShelf, BiquadKind::HighShelf, BiquadKind::LowPass, BiquadKind::HighPass] {
                let c = Coefficients::design(kind, rate, freq, q, gain);
                assert!(c.is_stable(), "{kind:?} f={freq} q={q} g={gain}: {c:?}");
                let mut bq = Biquad::new(c, 2);
                let mut x: Vec<f32> = (0..20_000).map(|i| if i % 997 == 0 { 1.0 } else { 0.0 }).collect();
                bq.process_interleaved(&mut x);
                assert!(x.iter().all(|v| v.is_finite()), "{kind:?} produced non-finite output");
                assert!(x.iter().all(|v| v.abs() < 1000.0), "{kind:?} ran away");
            }
        }
        assert!(!Coefficients { b0: 1.0, b1: 0.0, b2: 0.0, a1: 0.0, a2: 1.5 }.is_stable());
    }

    #[test]
    fn denormals_are_flushed_after_long_silence() {
        let c = Coefficients::design(BiquadKind::Peaking, 48_000.0, 60.0, 30.0, 40.0);
        let mut bq = Biquad::new(c, 1);
        let mut x = vec![0.0f32; 10];
        x[0] = 1.0;
        bq.process_interleaved(&mut x);
        let mut silence = vec![0.0f32; 48_000 * 30];
        bq.process_interleaved(&mut silence);
        for s in &bq.state {
            assert!(s[0] == 0.0 || s[0].is_normal());
            assert!(s[1] == 0.0 || s[1].is_normal());
        }
        assert_eq!(bq.state[0], [0.0, 0.0], "state decays to exactly zero");
    }

    #[test]
    fn identity_passes_through_and_channel_state_is_independent() {
        let mut bq = Biquad::identity(2);
        let mut x = vec![0.5, -0.25, 0.125, 1.0];
        bq.process_interleaved(&mut x);
        assert_eq!(x, vec![0.5, -0.25, 0.125, 1.0]);
        let mut lp = Biquad::new(Coefficients::design(BiquadKind::LowPass, 48_000.0, 100.0, 0.7, 0.0), 2);
        // Impulse on the left only: the right channel stays silent.
        let mut y = vec![0.0f32; 200];
        y[0] = 1.0;
        lp.process_interleaved(&mut y);
        assert!(y.iter().skip(1).step_by(2).all(|v| *v == 0.0));
        assert!(y.iter().step_by(2).any(|v| *v != 0.0));
    }
}
