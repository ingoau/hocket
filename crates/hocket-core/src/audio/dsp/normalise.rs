//! Queue-wide normalisation: a slow automatic gain control towards a target
//! loudness, sitting in the same chain as ReplayGain so the two never fight
//! (ReplayGain sets the per-track level first; the AGC only corrects what is
//! left, and is off by default).
//!
//! The measure is the same K-weighted, gated short-term loudness used for
//! [`super::loudness`], over a sliding 2 s window updated every 100 ms. The
//! gain moves slowly (≈ 1 s attack, ≈ 3 s release, capped at ±12 dB) so it
//! levels between tracks and long sections rather than pumping on beats,
//! and it never boosts material under −50 LUFS (silence, fade-outs).

use crate::audio::dsp::eq::db_to_linear;
use crate::audio::dsp::loudness::{channel_weights, KWeighting};

/// Target for the AGC.
pub const TARGET_LUFS: f64 = -14.0;
/// Sliding window.
pub const WINDOW_MS: f64 = 2000.0;
/// Below this measured loudness the AGC holds its gain instead of boosting.
pub const GATE_LUFS: f64 = -50.0;
/// Bounds on the correction.
pub const MAX_GAIN_DB: f64 = 12.0;

#[derive(Debug, Clone)]
pub struct Normaliser {
    channels: usize,
    sample_rate: f64,
    weights: Vec<f64>,
    filter: KWeighting,
    hop_frames: usize,
    current: Vec<f64>,
    current_frames: usize,
    /// Ring of sub-block powers covering the window.
    ring: Vec<f64>,
    ring_pos: usize,
    ring_filled: usize,
    /// Current gain in dB (smoothed) and its target.
    gain_db: f64,
    target_gain_db: f64,
    attack_coeff: f64,
    release_coeff: f64,
    target_lufs: f64,
    enabled: bool,
}

impl Normaliser {
    pub fn new(sample_rate: f64, channels: usize) -> Self {
        let channels = channels.max(1);
        let hop_frames = ((sample_rate / 10.0).round() as usize).max(1);
        let ring_len = (WINDOW_MS / 100.0) as usize;
        Self {
            channels,
            sample_rate,
            weights: channel_weights(channels),
            filter: KWeighting::new(sample_rate, channels),
            hop_frames,
            current: vec![0.0; channels],
            current_frames: 0,
            ring: vec![0.0; ring_len],
            ring_pos: 0,
            ring_filled: 0,
            gain_db: 0.0,
            target_gain_db: 0.0,
            // Per-hop (100 ms) one-pole coefficients for ~1 s attack / ~3 s release.
            attack_coeff: 1.0 - (-0.1f64 / 1.0).exp(),
            release_coeff: 1.0 - (-0.1f64 / 3.0).exp(),
            target_lufs: TARGET_LUFS,
            enabled: false,
        }
    }

    pub fn set_enabled(&mut self, enabled: bool) {
        if enabled != self.enabled {
            self.enabled = enabled;
            if !enabled {
                self.reset();
            }
        }
    }

    pub fn enabled(&self) -> bool {
        self.enabled
    }

    pub fn set_target_lufs(&mut self, lufs: f64) {
        self.target_lufs = lufs.clamp(-30.0, -5.0);
    }

    pub fn set_format(&mut self, sample_rate: f64, channels: usize) {
        let enabled = self.enabled;
        let target = self.target_lufs;
        *self = Self::new(sample_rate, channels);
        self.enabled = enabled;
        self.target_lufs = target;
    }

    /// Current gain in dB, for the UI.
    pub fn gain_db(&self) -> f64 {
        self.gain_db
    }

    /// Windowed loudness in LUFS, `None` before the first hop.
    pub fn short_term_lufs(&self) -> Option<f64> {
        if self.ring_filled == 0 {
            return None;
        }
        let n = self.ring_filled.min(self.ring.len());
        let mean = self.ring.iter().take(n).sum::<f64>() / n as f64;
        Some(-0.691 + 10.0 * mean.max(1e-30).log10())
    }

    pub fn reset(&mut self) {
        self.filter.reset();
        self.current.fill(0.0);
        self.current_frames = 0;
        self.ring.fill(0.0);
        self.ring_pos = 0;
        self.ring_filled = 0;
        self.gain_db = 0.0;
        self.target_gain_db = 0.0;
    }

    fn end_hop(&mut self) {
        let n = self.current_frames as f64;
        let power: f64 = self
            .current
            .iter()
            .zip(&self.weights)
            .map(|(s, w)| w * s / n)
            .sum();
        self.ring[self.ring_pos] = power;
        self.ring_pos = (self.ring_pos + 1) % self.ring.len();
        self.ring_filled = (self.ring_filled + 1).min(self.ring.len());
        self.current.fill(0.0);
        self.current_frames = 0;
        if let Some(l) = self.short_term_lufs() {
            if l > GATE_LUFS {
                self.target_gain_db = (self.target_lufs - l).clamp(-MAX_GAIN_DB, MAX_GAIN_DB);
            }
        }
        let coeff = if self.target_gain_db < self.gain_db {
            self.attack_coeff
        } else {
            self.release_coeff
        };
        self.gain_db += (self.target_gain_db - self.gain_db) * coeff;
    }

    /// Process interleaved frames in place. No-op when disabled.
    pub fn process(&mut self, samples: &mut [f32]) {
        if !self.enabled {
            return;
        }
        let _ = self.sample_rate;
        let mut gain = db_to_linear(self.gain_db) as f32;
        for frame in samples.chunks_exact_mut(self.channels) {
            for (ch, s) in frame.iter_mut().enumerate() {
                let y = self.filter.tick(ch, f64::from(*s));
                self.current[ch] += y * y;
                *s *= gain;
            }
            self.current_frames += 1;
            if self.current_frames == self.hop_frames {
                self.end_hop();
                gain = db_to_linear(self.gain_db) as f32;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::dsp::loudness::measure_loudness;
    use std::f64::consts::PI;

    fn stereo_sine(amp_dbfs: f64, rate: f64, seconds: f64) -> Vec<f32> {
        let amp = 10f64.powf(amp_dbfs / 20.0);
        (0..(rate * seconds) as usize)
            .flat_map(|i| {
                let s = (amp * (2.0 * PI * 1000.0 * i as f64 / rate).sin()) as f32;
                [s, s]
            })
            .collect()
    }

    #[test]
    fn converges_towards_the_target() {
        let rate = 48_000.0;
        let mut n = Normaliser::new(rate, 2);
        n.set_enabled(true);
        let mut quiet = stereo_sine(-30.0, rate, 20.0);
        n.process(&mut quiet);
        assert!(
            (n.gain_db() - 12.0).abs() < 0.5,
            "capped boost for a −30 LUFS signal: {}",
            n.gain_db()
        );
        let mut loud = stereo_sine(-6.0, rate, 20.0);
        n.process(&mut loud);
        assert!(
            (n.gain_db() + 8.0).abs() < 0.5,
            "−6 → −14 needs −8 dB: {}",
            n.gain_db()
        );
        // The tail of the loud signal is now near the target.
        let tail = &loud[loud.len() - 2 * 48_000 * 3..];
        let l = measure_loudness(tail, 2, rate).lufs.unwrap();
        assert!((l - TARGET_LUFS).abs() < 1.0, "{l}");
    }

    #[test]
    fn silence_does_not_get_boosted_and_disabled_is_a_no_op() {
        let mut n = Normaliser::new(44_100.0, 2);
        n.set_enabled(true);
        let mut z = vec![0.0f32; 44_100 * 4];
        n.process(&mut z);
        assert_eq!(n.gain_db(), 0.0);
        n.set_enabled(false);
        let mut x = stereo_sine(-40.0, 44_100.0, 3.0);
        let before = x.clone();
        n.process(&mut x);
        assert_eq!(x, before);
    }

    #[test]
    fn attack_is_faster_than_release() {
        let rate = 48_000.0;
        let mut n = Normaliser::new(rate, 2);
        n.set_enabled(true);
        n.process(&mut stereo_sine(-14.0, rate, 5.0));
        assert!(n.gain_db().abs() < 0.3);
        let mut loud = stereo_sine(-4.0, rate, 1.5);
        n.process(&mut loud);
        let after_attack = n.gain_db();
        let mut n2 = Normaliser::new(rate, 2);
        n2.set_enabled(true);
        n2.process(&mut stereo_sine(-14.0, rate, 5.0));
        n2.process(&mut stereo_sine(-24.0, rate, 1.5));
        let after_release = n2.gain_db();
        assert!(
            after_attack.abs() > after_release.abs(),
            "attack {after_attack} release {after_release}"
        );
    }
}
