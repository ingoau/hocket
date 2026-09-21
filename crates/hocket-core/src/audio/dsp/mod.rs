//! The DSP chain. Lives in the core deliberately so both platforms sound
//! identical: the same biquads, the same ReplayGain arithmetic, the same
//! limiter.
//!
//! Signal path, interleaved `f32` in place:
//!
//! ```text
//! decoded PCM → ReplayGain (per-track gain, peak-clamped, ramped)
//!             → Equalizer (preamp with clip protection + biquad bands)
//!             → Normalisation (slow K-weighted AGC to −14 LUFS, off by default)
//!             → Soft limiter (−1 dB knee)
//!             → [volume, applied by the output stage]
//! ```
//!
//! [`gain_for_track`] is the one function shared with the external backend:
//! the Android bridge puts its result in `MediaSource.gain_db` and the
//! platform applies it, so ReplayGain decisions are made in exactly one
//! place.

pub mod biquad;
pub mod eq;
pub mod loudness;
pub mod normalise;
pub mod replaygain;
pub mod tags;

pub use biquad::{Biquad, BiquadKind, Coefficients};
pub use eq::{preset, presets, EqOptions, Equalizer, PRESET_NAMES};
pub use loudness::{measure_loudness, Loudness, LoudnessMeter};
pub use normalise::Normaliser;
pub use replaygain::{gain_for_track, SmoothGain, SoftLimiter};

use crate::api::{AudioSettings, EqSettings, ReplayGain, ReplayGainMode};
use eq::db_to_linear;

/// The subset of [`AudioSettings`] the chain acts on.
#[derive(Debug, Clone, PartialEq)]
pub struct DspSettings {
    pub replay_gain: ReplayGainMode,
    pub replay_gain_preamp_db: f64,
    pub normalisation: bool,
    pub eq: EqSettings,
    pub eq_options: EqOptions,
}

impl Default for DspSettings {
    fn default() -> Self {
        Self {
            replay_gain: ReplayGainMode::Auto,
            replay_gain_preamp_db: 0.0,
            normalisation: false,
            eq: eq::flat(),
            eq_options: EqOptions::default(),
        }
    }
}

impl From<&AudioSettings> for DspSettings {
    fn from(a: &AudioSettings) -> Self {
        Self {
            replay_gain: a.replay_gain,
            replay_gain_preamp_db: a.replay_gain_preamp_db,
            normalisation: a.normalisation,
            eq: a.eq.clone(),
            eq_options: EqOptions::default(),
        }
    }
}

/// Ramp length for gain changes that happen mid-stream (settings edits).
pub const GAIN_RAMP_MS: f64 = 20.0;

/// Where the current track's gain comes from.
#[derive(Debug, Clone, PartialEq)]
enum TrackGainSource {
    /// Derived from tags through [`gain_for_track`]; re-derived when the
    /// settings change.
    Tags {
        rg: Option<ReplayGain>,
        is_album_context: bool,
    },
    /// Precomputed by the actor (what `MediaSource.gain_db` carries): the
    /// settings' ReplayGain mode and preamp are already folded in.
    Explicit { gain_db: f64 },
}

/// See the module docs.
#[derive(Debug, Clone)]
pub struct DspChain {
    sample_rate: f64,
    channels: usize,
    settings: DspSettings,
    track_gain: SmoothGain,
    track: TrackGainSource,
    eq: Equalizer,
    normaliser: Normaliser,
    limiter: SoftLimiter,
}

impl DspChain {
    pub fn new(sample_rate: f64, channels: usize, settings: DspSettings) -> Self {
        let channels = channels.max(1);
        let ramp = ((sample_rate * GAIN_RAMP_MS / 1000.0) as usize).max(1);
        let mut eq = Equalizer::new(&settings.eq, sample_rate, channels, settings.eq_options);
        eq.configure(&settings.eq);
        let mut normaliser = Normaliser::new(sample_rate, channels);
        normaliser.set_enabled(settings.normalisation);
        Self {
            sample_rate,
            channels,
            settings,
            track_gain: SmoothGain::new(1.0, ramp),
            track: TrackGainSource::Tags {
                rg: None,
                is_album_context: false,
            },
            eq,
            normaliser,
            limiter: SoftLimiter::default(),
        }
    }

    pub fn sample_rate(&self) -> f64 {
        self.sample_rate
    }

    pub fn channels(&self) -> usize {
        self.channels
    }

    pub fn settings(&self) -> &DspSettings {
        &self.settings
    }

    /// Replace the settings; coefficients are recomputed, the current
    /// track's gain re-derived and ramped to.
    pub fn set_settings(&mut self, settings: DspSettings) {
        if settings == self.settings {
            return;
        }
        self.settings = settings;
        self.eq
            .set_options(self.settings.eq_options, &self.settings.eq);
        self.normaliser.set_enabled(self.settings.normalisation);
        self.track_gain.set_target(self.current_gain_linear());
    }

    /// Convenience for [`AudioSettings`] straight off the seam.
    pub fn apply_audio_settings(&mut self, audio: &AudioSettings) {
        let mut s = DspSettings::from(audio);
        s.eq_options = self.settings.eq_options;
        self.set_settings(s);
    }

    /// Reconfigure for a new stream format. Resets filter state.
    pub fn set_format(&mut self, sample_rate: f64, channels: usize) {
        let channels = channels.max(1);
        if sample_rate == self.sample_rate && channels == self.channels {
            return;
        }
        self.sample_rate = sample_rate;
        self.channels = channels;
        let ramp = ((sample_rate * GAIN_RAMP_MS / 1000.0) as usize).max(1);
        let g = self.track_gain.target();
        self.track_gain = SmoothGain::new(f64::from(g), ramp);
        self.eq.set_format(sample_rate, channels, &self.settings.eq);
        self.normaliser.set_format(sample_rate, channels);
    }

    /// A new track starts at the next sample: snap to its gain (no ramp —
    /// the boundary is the discontinuity) and keep the EQ/AGC state running
    /// so a gapless transition stays seamless.
    pub fn begin_track(&mut self, rg: Option<&ReplayGain>, is_album_context: bool) {
        self.track = TrackGainSource::Tags {
            rg: rg.cloned(),
            is_album_context,
        };
        self.track_gain.snap(self.current_gain_linear());
    }

    /// Like [`Self::begin_track`] but with a gain the actor already computed
    /// (from [`gain_for_track`]) — the native backend uses this so the
    /// external and native paths make the ReplayGain decision in one place.
    pub fn begin_track_gain_db(&mut self, gain_db: f64) {
        let gain_db = if gain_db.is_finite() {
            gain_db.clamp(-40.0, 40.0)
        } else {
            0.0
        };
        self.track = TrackGainSource::Explicit { gain_db };
        self.track_gain.snap(self.current_gain_linear());
    }

    /// Gain in dB the chain applies for the current track.
    pub fn track_gain_db(&self) -> f64 {
        match &self.track {
            TrackGainSource::Tags {
                rg,
                is_album_context,
            } => gain_for_track(
                rg.as_ref(),
                self.settings.replay_gain,
                *is_album_context,
                self.settings.replay_gain_preamp_db,
            ),
            TrackGainSource::Explicit { gain_db } => *gain_db,
        }
    }

    fn current_gain_linear(&self) -> f64 {
        db_to_linear(self.track_gain_db())
    }

    /// Clear filter memory (seek, stop): the next samples are unrelated to
    /// the last ones.
    pub fn reset(&mut self) {
        self.eq.reset();
        self.normaliser.reset();
        self.track_gain.snap(self.current_gain_linear());
    }

    /// Whether processing would change anything (lets the engine skip the
    /// pass entirely for the common "everything off" case).
    pub fn is_bypassed(&self) -> bool {
        self.track_gain.is_unity() && !self.eq.enabled() && !self.normaliser.enabled()
    }

    /// Process interleaved frames in place.
    pub fn process(&mut self, samples: &mut [f32]) {
        if self.is_bypassed() {
            return;
        }
        self.track_gain.process(samples, self.channels);
        self.eq.process(samples);
        self.normaliser.process(samples);
        // Only the stages that can push above full scale need the limiter.
        if self.track_gain.current() > 1.0 || self.eq.enabled() || self.normaliser.enabled() {
            self.limiter.process(samples);
        }
    }

    pub fn equalizer(&self) -> &Equalizer {
        &self.eq
    }

    pub fn normaliser(&self) -> &Normaliser {
        &self.normaliser
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::EqBand;
    use std::f64::consts::PI;

    fn sine(freq: f64, rate: f64, frames: usize, amp: f32) -> Vec<f32> {
        (0..frames)
            .flat_map(|i| {
                let s = amp * (2.0 * PI * freq * i as f64 / rate).sin() as f32;
                [s, s]
            })
            .collect()
    }

    fn rms_db(x: &[f32]) -> f64 {
        10.0 * (x.iter().map(|v| f64::from(*v).powi(2)).sum::<f64>() / x.len() as f64).log10()
    }

    #[test]
    fn everything_off_is_bit_exact_passthrough() {
        let mut chain = DspChain::new(
            48_000.0,
            2,
            DspSettings {
                replay_gain: ReplayGainMode::Off,
                ..Default::default()
            },
        );
        chain.begin_track(
            Some(&ReplayGain {
                track_gain_db: Some(-6.0),
                ..Default::default()
            }),
            false,
        );
        assert!(chain.is_bypassed());
        let x = sine(440.0, 48_000.0, 1000, 0.5);
        let mut y = x.clone();
        chain.process(&mut y);
        assert_eq!(x, y);
    }

    #[test]
    fn replaygain_applies_per_track_and_snaps_at_boundaries() {
        let mut chain = DspChain::new(
            48_000.0,
            2,
            DspSettings {
                replay_gain: ReplayGainMode::Track,
                ..Default::default()
            },
        );
        chain.begin_track(
            Some(&ReplayGain {
                track_gain_db: Some(-6.0),
                track_peak: Some(0.5),
                ..Default::default()
            }),
            false,
        );
        assert_eq!(chain.track_gain_db(), -6.0);
        let mut x = sine(1000.0, 48_000.0, 4800, 0.5);
        let before = rms_db(&x);
        chain.process(&mut x);
        assert!((rms_db(&x) - before + 6.0).abs() < 0.01);
        chain.begin_track(
            Some(&ReplayGain {
                track_gain_db: Some(3.0),
                track_peak: Some(0.5),
                ..Default::default()
            }),
            false,
        );
        let mut y = sine(1000.0, 48_000.0, 4800, 0.5);
        let before = rms_db(&y);
        chain.process(&mut y);
        assert!(
            (rms_db(&y) - before - 3.0).abs() < 0.01,
            "no ramp at a boundary"
        );
        assert!(y.iter().all(|v| v.abs() <= 1.0));
    }

    #[test]
    fn settings_change_ramps_instead_of_stepping() {
        let mut chain = DspChain::new(
            48_000.0,
            2,
            DspSettings {
                replay_gain: ReplayGainMode::Track,
                ..Default::default()
            },
        );
        chain.begin_track(
            Some(&ReplayGain {
                track_gain_db: Some(0.0),
                ..Default::default()
            }),
            false,
        );
        let mut s = chain.settings().clone();
        s.replay_gain_preamp_db = -12.0;
        chain.set_settings(s);
        let mut x = vec![1.0f32; 2 * 2000];
        chain.process(&mut x);
        // 20 ms ramp = 960 frames: still ramping at frame 100, done by 1000.
        assert!(x[200] > 0.5 && x[200] < 1.0, "{}", x[200]);
        assert!((f64::from(x[2 * 1500]) - db_to_linear(-12.0)).abs() < 1e-4);
    }

    #[test]
    fn eq_and_limiter_keep_the_output_in_range() {
        let mut settings = DspSettings {
            replay_gain: ReplayGainMode::Off,
            ..Default::default()
        };
        settings.eq = EqSettings {
            enabled: true,
            preamp_db: 0.0,
            bands: vec![EqBand {
                frequency_hz: 1000.0,
                gain_db: 12.0,
                q: 1.0,
            }],
            preset: None,
        };
        settings.eq_options.auto_clip_protection = false;
        let mut chain = DspChain::new(48_000.0, 2, settings);
        chain.begin_track(None, false);
        let mut x = sine(1000.0, 48_000.0, 48_000, 0.9);
        chain.process(&mut x);
        assert!(
            x.iter().all(|v| v.abs() < 1.0),
            "limiter bounds the boosted band"
        );
        assert!(
            rms_db(&x[48_000..]) > rms_db(&sine(1000.0, 48_000.0, 48_000, 0.9)[48_000..]) + 1.0
        );
    }

    #[test]
    fn audio_settings_map_onto_the_chain() {
        let audio = AudioSettings {
            replay_gain: ReplayGainMode::Album,
            replay_gain_preamp_db: 2.0,
            normalisation: true,
            eq: preset("rock").unwrap(),
            gapless: true,
            output_device: None,
            exclusive: false,
        };
        let mut chain = DspChain::new(44_100.0, 2, DspSettings::default());
        chain.apply_audio_settings(&audio);
        assert_eq!(chain.settings().replay_gain, ReplayGainMode::Album);
        assert!(chain.equalizer().enabled());
        assert!(chain.normaliser().enabled());
        chain.begin_track(
            Some(&ReplayGain {
                album_gain_db: Some(-4.0),
                ..Default::default()
            }),
            true,
        );
        assert_eq!(chain.track_gain_db(), -2.0);
        chain.set_format(96_000.0, 1);
        assert_eq!(chain.channels(), 1);
        assert_eq!(chain.sample_rate(), 96_000.0);
        let mut x = vec![0.1f32; 960];
        chain.process(&mut x);
        assert!(x.iter().all(|v| v.is_finite()));
        chain.reset();
    }

    #[test]
    fn explicit_gain_survives_settings_changes() {
        let mut chain = DspChain::new(
            48_000.0,
            2,
            DspSettings {
                replay_gain: ReplayGainMode::Off,
                ..Default::default()
            },
        );
        chain.begin_track_gain_db(-4.5);
        assert_eq!(chain.track_gain_db(), -4.5);
        let mut s = chain.settings().clone();
        s.replay_gain = ReplayGainMode::Track;
        s.replay_gain_preamp_db = 6.0;
        chain.set_settings(s);
        assert_eq!(
            chain.track_gain_db(),
            -4.5,
            "the actor folded settings in already"
        );
        chain.begin_track_gain_db(f64::NAN);
        assert_eq!(chain.track_gain_db(), 0.0);
        assert!(chain.is_bypassed());
    }
}
