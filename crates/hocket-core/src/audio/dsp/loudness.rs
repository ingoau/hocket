//! EBU R128 / ITU-R BS.1770-4 loudness: K-weighting plus gated integrated
//! loudness, and the ReplayGain 2.0 values derived from it.
//!
//! ReplayGain 2.0 defines track gain as `−18 LUFS − measured loudness`
//! (the −18 reference keeps files tagged by the original 89 dB SPL
//! algorithm compatible). Downloads call [`compute_replaygain`] when the
//! transcoded payload lost its tags.
//!
//! The measurement follows the spec literally rather than cleverly:
//! K-weighting = a high shelf (+4 dB above ~1.5 kHz) then a high-pass at
//! ~38 Hz, both derived for the actual sample rate the way libebur128 does;
//! 400 ms blocks with 75 % overlap; absolute gate at −70 LUFS; relative gate
//! 10 LU below the ungated mean; channel weights 1.0 for L/R/C, 1.41 for
//! surrounds, LFE excluded.

use crate::api::ReplayGain;
use crate::audio::dsp::biquad::{Biquad, Coefficients};

/// ReplayGain 2.0 reference loudness.
pub const REPLAYGAIN_REFERENCE_LUFS: f64 = -18.0;
/// EBU R128 absolute gate.
pub const ABSOLUTE_GATE_LUFS: f64 = -70.0;
/// Relative gate below the ungated loudness.
pub const RELATIVE_GATE_LU: f64 = 10.0;
/// Gating block length and hop.
pub const BLOCK_MS: f64 = 400.0;
pub const HOP_MS: f64 = 100.0;

/// The two K-weighting stages for a sample rate (libebur128 derivation of
/// the BS.1770 filters, exact for 48 kHz and correct at any other rate).
pub fn k_weighting_coefficients(sample_rate: f64) -> (Coefficients, Coefficients) {
    use std::f64::consts::PI;
    let fs = if sample_rate.is_finite() && sample_rate > 0.0 {
        sample_rate
    } else {
        48_000.0
    };
    // Stage 1: high shelf.
    let f0 = 1_681.974_450_955_533;
    let g = 3.999_843_853_973_347;
    let q = 0.707_175_236_955_419_6;
    let k = (PI * f0 / fs).tan();
    let vh = 10f64.powf(g / 20.0);
    let vb = vh.powf(0.499_666_774_154_541_6);
    let a0 = 1.0 + k / q + k * k;
    let shelf = Coefficients::from_raw(
        vh + vb * k / q + k * k,
        2.0 * (k * k - vh),
        vh - vb * k / q + k * k,
        a0,
        2.0 * (k * k - 1.0),
        1.0 - k / q + k * k,
    );
    // Stage 2: high-pass.
    let f0 = 38.135_470_876_024_44;
    let q = 0.500_327_037_323_877_3;
    let k = (PI * f0 / fs).tan();
    let a0 = 1.0 + k / q + k * k;
    let hp = Coefficients::from_raw(1.0, -2.0, 1.0, a0, 2.0 * (k * k - 1.0), 1.0 - k / q + k * k);
    (shelf, hp)
}

/// Channel weight per BS.1770 for `channels` interleaved channels.
pub fn channel_weights(channels: usize) -> Vec<f64> {
    (0..channels)
        .map(|i| match (channels, i) {
            // 5.1 / 7.1 layouts: L R C LFE Ls Rs (...)
            (6.., 3) => 0.0,
            (5.., 4) | (5.., 5) => 1.41,
            (5, 3) => 1.41,
            _ => 1.0,
        })
        .collect()
}

/// K-weighting filter pair for all channels.
#[derive(Debug, Clone)]
pub struct KWeighting {
    shelf: Biquad,
    highpass: Biquad,
}

impl KWeighting {
    pub fn new(sample_rate: f64, channels: usize) -> Self {
        let (s, h) = k_weighting_coefficients(sample_rate);
        Self {
            shelf: Biquad::new(s, channels),
            highpass: Biquad::new(h, channels),
        }
    }

    #[inline]
    pub fn tick(&mut self, channel: usize, x: f64) -> f64 {
        self.highpass.tick(channel, self.shelf.tick(channel, x))
    }

    pub fn reset(&mut self) {
        self.shelf.reset();
        self.highpass.reset();
    }
}

/// Streaming integrated-loudness meter. Feed interleaved `f32`, read the
/// gated loudness at the end (or at any point for a running value).
#[derive(Debug, Clone)]
pub struct LoudnessMeter {
    channels: usize,
    weights: Vec<f64>,
    filter: KWeighting,
    hop_frames: usize,
    /// Sum of squares per channel for the sub-block being filled.
    current: Vec<f64>,
    current_frames: usize,
    /// Completed 100 ms sub-block powers (weighted sum over channels, per frame).
    sub_blocks: Vec<f64>,
    sample_peak: f32,
    total_frames: u64,
}

impl LoudnessMeter {
    pub fn new(sample_rate: f64, channels: usize) -> Self {
        let channels = channels.max(1);
        let hop_frames = ((sample_rate * HOP_MS / 1000.0).round() as usize).max(1);
        Self {
            channels,
            weights: channel_weights(channels),
            filter: KWeighting::new(sample_rate, channels),
            hop_frames,
            current: vec![0.0; channels],
            current_frames: 0,
            sub_blocks: Vec::new(),
            sample_peak: 0.0,
            total_frames: 0,
        }
    }

    pub fn push(&mut self, interleaved: &[f32]) {
        for frame in interleaved.chunks_exact(self.channels) {
            for (ch, &s) in frame.iter().enumerate() {
                let a = s.abs();
                if a > self.sample_peak {
                    self.sample_peak = a;
                }
                let y = self.filter.tick(ch, f64::from(s));
                self.current[ch] += y * y;
            }
            self.current_frames += 1;
            self.total_frames += 1;
            if self.current_frames == self.hop_frames {
                self.finish_sub_block();
            }
        }
    }

    fn finish_sub_block(&mut self) {
        if self.current_frames == 0 {
            return;
        }
        let n = self.current_frames as f64;
        let power: f64 = self
            .current
            .iter()
            .zip(&self.weights)
            .map(|(sum, w)| w * sum / n)
            .sum();
        self.sub_blocks.push(power);
        for c in &mut self.current {
            *c = 0.0;
        }
        self.current_frames = 0;
    }

    /// Highest absolute sample seen (0.0–1.0 for in-range audio).
    pub fn sample_peak(&self) -> f64 {
        f64::from(self.sample_peak)
    }

    pub fn frames(&self) -> u64 {
        self.total_frames
    }

    /// Gated integrated loudness in LUFS, `None` when nothing exceeded the
    /// absolute gate (digital silence) or nothing was pushed.
    pub fn integrated_lufs(&self) -> Option<f64> {
        let mut subs = self.sub_blocks.clone();
        if self.current_frames > 0 {
            // Include the partial tail for short signals.
            let n = self.current_frames as f64;
            let power: f64 = self
                .current
                .iter()
                .zip(&self.weights)
                .map(|(sum, w)| w * sum / n)
                .sum();
            subs.push(power);
        }
        if subs.is_empty() {
            return None;
        }
        let per_block = (BLOCK_MS / HOP_MS) as usize; // 4 sub-blocks per gating block
        let block_powers: Vec<f64> = if subs.len() < per_block {
            vec![subs.iter().sum::<f64>() / subs.len() as f64]
        } else {
            subs.windows(per_block)
                .map(|w| w.iter().sum::<f64>() / per_block as f64)
                .collect()
        };
        let to_lufs = |p: f64| -0.691 + 10.0 * p.max(1e-30).log10();
        let abs_gate: Vec<f64> = block_powers
            .iter()
            .copied()
            .filter(|&p| to_lufs(p) > ABSOLUTE_GATE_LUFS)
            .collect();
        if abs_gate.is_empty() {
            return None;
        }
        let ungated = abs_gate.iter().sum::<f64>() / abs_gate.len() as f64;
        let rel_threshold = to_lufs(ungated) - RELATIVE_GATE_LU;
        let gated: Vec<f64> = abs_gate
            .into_iter()
            .filter(|&p| to_lufs(p) > rel_threshold)
            .collect();
        if gated.is_empty() {
            return None;
        }
        Some(to_lufs(gated.iter().sum::<f64>() / gated.len() as f64))
    }
}

/// Result of a measurement.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Loudness {
    /// `None` for silence.
    pub lufs: Option<f64>,
    pub sample_peak: f64,
}

impl Loudness {
    /// ReplayGain 2.0 track values. Silence gets 0 dB gain rather than +∞.
    pub fn to_replaygain(self) -> ReplayGain {
        ReplayGain {
            track_gain_db: Some(
                self.lufs
                    .map(|l| (REPLAYGAIN_REFERENCE_LUFS - l).clamp(-40.0, 40.0))
                    .unwrap_or(0.0),
            ),
            track_peak: Some(self.sample_peak),
            album_gain_db: None,
            album_peak: None,
        }
    }
}

/// Measure a whole interleaved buffer.
pub fn measure_loudness(interleaved: &[f32], channels: usize, sample_rate: f64) -> Loudness {
    let mut m = LoudnessMeter::new(sample_rate, channels);
    m.push(interleaved);
    Loudness {
        lufs: m.integrated_lufs(),
        sample_peak: m.sample_peak(),
    }
}

/// Album gain over several already-measured tracks: the loudness of the
/// concatenation is approximated by power-averaging the per-track values
/// weighted by duration (exact when every track passes its own gates).
/// `tracks` are `(lufs, frames)`; returns `None` when all were silent.
pub fn album_gain_db(tracks: &[(Option<f64>, u64)]) -> Option<f64> {
    let mut power = 0.0;
    let mut frames = 0u64;
    for (l, n) in tracks {
        if let Some(l) = l {
            power += 10f64.powf((l + 0.691) / 10.0) * *n as f64;
            frames += n;
        }
    }
    if frames == 0 {
        return None;
    }
    let lufs = -0.691 + 10.0 * (power / frames as f64).log10();
    Some(REPLAYGAIN_REFERENCE_LUFS - lufs)
}

/// Decode a local file and measure it (feature `native-audio`). This is what
/// the downloads subsystem calls when a transcode dropped the tags.
#[cfg(feature = "native-audio")]
pub fn compute_replaygain(
    path: &std::path::Path,
) -> Result<ReplayGain, crate::audio::native::decoder::DecodeError> {
    let mut meter: Option<LoudnessMeter> = None;
    crate::audio::native::decoder::decode_file(path, |spec, pcm| {
        let m = meter
            .get_or_insert_with(|| LoudnessMeter::new(f64::from(spec.sample_rate), spec.channels));
        m.push(pcm);
    })?;
    let m = meter.ok_or(crate::audio::native::decoder::DecodeError::NoAudio)?;
    Ok(Loudness {
        lufs: m.integrated_lufs(),
        sample_peak: m.sample_peak(),
    }
    .to_replaygain())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::PI;

    fn stereo_sine(freq: f64, amp_dbfs: f64, rate: f64, seconds: f64) -> Vec<f32> {
        let amp = 10f64.powf(amp_dbfs / 20.0);
        let n = (rate * seconds) as usize;
        let mut v = Vec::with_capacity(n * 2);
        for i in 0..n {
            let s = (amp * (2.0 * PI * freq * i as f64 / rate).sin()) as f32;
            v.push(s);
            v.push(s);
        }
        v
    }

    #[test]
    fn ebu_tech_3341_case_1_sine_minus_23_dbfs_reads_minus_23_lufs() {
        // EBU Tech 3341 test 1: 1 kHz stereo sine at −23 dBFS → −23.0 ± 0.1 LUFS.
        for rate in [48_000.0, 44_100.0, 96_000.0] {
            let l = measure_loudness(&stereo_sine(1000.0, -23.0, rate, 20.0), 2, rate);
            let lufs = l.lufs.expect("not silent");
            assert!((lufs + 23.0).abs() < 0.1, "rate {rate}: {lufs}");
            assert!((l.sample_peak - 10f64.powf(-23.0 / 20.0)).abs() < 1e-3);
        }
    }

    #[test]
    fn ebu_tech_3341_case_2_minus_33_dbfs() {
        let l = measure_loudness(&stereo_sine(1000.0, -33.0, 48_000.0, 20.0), 2, 48_000.0);
        assert!((l.lufs.unwrap() + 33.0).abs() < 0.1);
    }

    #[test]
    fn k_weighting_shape() {
        let (shelf, hp) = k_weighting_coefficients(48_000.0);
        assert!(shelf.is_stable() && hp.is_stable());
        let total = |f: f64| shelf.magnitude_db(f, 48_000.0) + hp.magnitude_db(f, 48_000.0);
        assert!(
            (total(1000.0) - 0.691).abs() < 0.05,
            "1 kHz sits at +0.69 dB, cancelled by the −0.691 constant"
        );
        assert!(
            (total(10_000.0) - 4.0).abs() < 0.15,
            "high shelf +4 dB: {}",
            total(10_000.0)
        );
        assert!(
            total(20.0) < -8.0,
            "high-pass rolls off the sub-bass: {}",
            total(20.0)
        );
    }

    #[test]
    fn relative_gate_ignores_quiet_passages() {
        // 10 s at −23 dBFS followed by 10 s at −60 dBFS: the quiet tail is
        // more than 10 LU below and must not drag the integrated value down.
        let rate = 48_000.0;
        let mut v = stereo_sine(1000.0, -23.0, rate, 10.0);
        v.extend(stereo_sine(1000.0, -60.0, rate, 10.0));
        let l = measure_loudness(&v, 2, rate).lufs.unwrap();
        // The three 400 ms blocks straddling the transition are still above
        // the relative gate and pull the value down by ~0.1 LU, as the spec's
        // measure does; a mean over both halves would read about −26.
        assert!((l + 23.0).abs() < 0.3, "{l}");
        // Silence alone is None; a −80 dBFS signal is under the absolute gate.
        assert_eq!(measure_loudness(&vec![0.0; 96_000], 2, rate).lufs, None);
        assert_eq!(
            measure_loudness(&stereo_sine(1000.0, -80.0, rate, 2.0), 2, rate).lufs,
            None
        );
    }

    #[test]
    fn short_signals_still_measure() {
        let l = measure_loudness(&stereo_sine(1000.0, -23.0, 48_000.0, 0.25), 2, 48_000.0);
        assert!((l.lufs.unwrap() + 23.0).abs() < 0.5, "{:?}", l);
        assert_eq!(measure_loudness(&[], 2, 48_000.0).lufs, None);
    }

    #[test]
    fn replaygain_values_from_loudness() {
        let rg = Loudness {
            lufs: Some(-23.0),
            sample_peak: 0.5,
        }
        .to_replaygain();
        assert_eq!(rg.track_gain_db, Some(5.0));
        assert_eq!(rg.track_peak, Some(0.5));
        assert_eq!(
            Loudness {
                lufs: None,
                sample_peak: 0.0
            }
            .to_replaygain()
            .track_gain_db,
            Some(0.0)
        );
        assert_eq!(
            Loudness {
                lufs: Some(-8.0),
                sample_peak: 1.0
            }
            .to_replaygain()
            .track_gain_db,
            Some(-10.0)
        );
    }

    #[test]
    fn album_gain_power_averages() {
        assert!(
            (album_gain_db(&[(Some(-20.0), 100), (Some(-20.0), 300)]).unwrap() - 2.0).abs() < 1e-9
        );
        let mixed = album_gain_db(&[(Some(-10.0), 100), (Some(-30.0), 100)]).unwrap();
        assert!(mixed < 2.0 && mixed > -8.0, "{mixed}");
        assert_eq!(album_gain_db(&[(None, 100)]), None);
    }

    #[test]
    fn mono_and_surround_weights() {
        assert_eq!(channel_weights(1), vec![1.0]);
        assert_eq!(channel_weights(2), vec![1.0, 1.0]);
        assert_eq!(channel_weights(6), vec![1.0, 1.0, 1.0, 0.0, 1.41, 1.41]);
        let mono: Vec<f32> = stereo_sine(1000.0, -23.0, 48_000.0, 5.0)
            .into_iter()
            .step_by(2)
            .collect();
        let l = measure_loudness(&mono, 1, 48_000.0).lufs.unwrap();
        assert!(
            (l + 26.0).abs() < 0.1,
            "a single channel reads 3 dB lower: {l}"
        );
    }
}
