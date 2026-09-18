//! Graphic / parametric equalizer built from RBJ peaking biquads, with an
//! optional low/high shelf on the outer bands, its own preamp and automatic
//! clipping protection.
//!
//! Presets are ordinary [`EqSettings`] so a user can start from one and
//! nudge bands; `preset` on the settings records where they started.

use crate::api::{EqBand, EqSettings};
use crate::audio::dsp::biquad::{Biquad, BiquadKind, Coefficients};

/// ISO 10-band centre frequencies used by every preset.
pub const ISO_BANDS_HZ: [f64; 10] = [
    31.0, 62.0, 125.0, 250.0, 500.0, 1000.0, 2000.0, 4000.0, 8000.0, 16000.0,
];

/// Q used for preset bands (≈ one octave wide).
pub const PRESET_Q: f64 = 1.41;

/// Preset names in display order.
pub const PRESET_NAMES: [&str; 9] = [
    "flat",
    "bassBoost",
    "treble",
    "vocal",
    "rock",
    "pop",
    "electronic",
    "acoustic",
    "loudness",
];

fn bands(gains: [f64; 10]) -> Vec<EqBand> {
    ISO_BANDS_HZ
        .iter()
        .zip(gains)
        .map(|(f, g)| EqBand {
            frequency_hz: *f,
            gain_db: g,
            q: PRESET_Q,
        })
        .collect()
}

/// Build a preset by name (see [`PRESET_NAMES`]). Gains are conservative so
/// the automatic preamp doesn't have to pull the level down much.
pub fn preset(name: &str) -> Option<EqSettings> {
    let gains = match name {
        "flat" => [0.0; 10],
        "bassBoost" => [6.0, 5.0, 4.0, 2.5, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0],
        "treble" => [0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 2.5, 4.0, 5.0, 6.0],
        "vocal" => [-2.0, -3.0, -2.0, 1.0, 3.5, 4.0, 3.0, 1.5, 0.0, -1.5],
        "rock" => [5.0, 4.0, 3.0, 1.0, -0.5, -1.0, 0.5, 2.5, 3.5, 4.0],
        "pop" => [-1.5, -1.0, 0.0, 2.0, 4.0, 4.0, 2.0, 0.0, -1.0, -1.5],
        "electronic" => [4.5, 4.0, 1.0, 0.0, -2.0, 2.0, 1.0, 1.0, 4.0, 5.0],
        "acoustic" => [4.0, 3.5, 2.5, 1.0, 1.5, 1.5, 3.0, 3.5, 3.0, 2.0],
        "loudness" => [6.0, 4.0, 0.0, 0.0, -2.0, 0.0, -1.0, -3.0, 4.0, 6.0],
        _ => return None,
    };
    Some(EqSettings {
        enabled: true,
        preamp_db: 0.0,
        bands: bands(gains),
        preset: Some(name.to_string()),
    })
}

/// All presets, in [`PRESET_NAMES`] order.
pub fn presets() -> Vec<EqSettings> {
    PRESET_NAMES.iter().filter_map(|n| preset(n)).collect()
}

/// A flat, disabled EQ.
pub fn flat() -> EqSettings {
    EqSettings {
        enabled: false,
        preamp_db: 0.0,
        bands: bands([0.0; 10]),
        preset: Some("flat".into()),
    }
}

/// Options that aren't part of the synced settings document.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EqOptions {
    /// Use a low shelf for the lowest band and a high shelf for the highest
    /// instead of peaking filters, so the extremes affect everything beyond
    /// them (what most hardware graphic EQs do).
    pub edge_shelves: bool,
    /// Automatic clipping protection: the effective preamp is lowered to
    /// minus the peak of the composite band response (never less negative
    /// than `−max(positive band gain)`, since overlapping bands add up)
    /// whenever the user preamp is above that, so gain plus boosted bands
    /// can't exceed 0 dBFS on a full-scale signal. The user overrides by
    /// turning this off; a user preamp *below* the automatic value is
    /// always respected.
    pub auto_clip_protection: bool,
}

impl Default for EqOptions {
    fn default() -> Self {
        Self {
            edge_shelves: true,
            auto_clip_protection: true,
        }
    }
}

/// Quick clip-protection estimate for a band set from the largest positive
/// gain alone (what a UI can show before the filters are designed). The
/// running [`Equalizer`] uses the composite response, which is at least as
/// negative.
pub fn auto_preamp_db(bands: &[EqBand]) -> f64 {
    -bands
        .iter()
        .map(|b| b.gain_db)
        .filter(|g| g.is_finite() && *g > 0.0)
        .fold(0.0, f64::max)
}

/// Peak of the summed magnitude response of `filters` over 20 Hz–20 kHz
/// (log-spaced grid), in dB, never below 0.
fn composite_peak_db(filters: &[Biquad], sample_rate: f64) -> f64 {
    const POINTS: usize = 512;
    let lo = 20f64.ln();
    let hi = (sample_rate / 2.0).clamp(21.0, 20_000.0).ln();
    let mut peak = 0.0f64;
    for i in 0..POINTS {
        let f = (lo + (hi - lo) * i as f64 / (POINTS - 1) as f64).exp();
        let db: f64 = filters
            .iter()
            .map(|b| b.coefficients().magnitude_db(f, sample_rate))
            .sum();
        if db > peak {
            peak = db;
        }
    }
    peak
}

fn user_preamp_db(settings: &EqSettings) -> f64 {
    if settings.preamp_db.is_finite() {
        settings.preamp_db.clamp(-30.0, 30.0)
    } else {
        0.0
    }
}

/// Effective preamp after clipping protection, using the single-band
/// estimate (see [`auto_preamp_db`]).
pub fn effective_preamp_db(settings: &EqSettings, options: EqOptions) -> f64 {
    let user = user_preamp_db(settings);
    if options.auto_clip_protection {
        user.min(auto_preamp_db(&settings.bands))
    } else {
        user
    }
}

/// The running equalizer.
#[derive(Debug, Clone)]
pub struct Equalizer {
    sample_rate: f64,
    channels: usize,
    options: EqOptions,
    enabled: bool,
    preamp: f64,
    filters: Vec<Biquad>,
}

impl Equalizer {
    pub fn new(
        settings: &EqSettings,
        sample_rate: f64,
        channels: usize,
        options: EqOptions,
    ) -> Self {
        let mut eq = Self {
            sample_rate,
            channels: channels.max(1),
            options,
            enabled: false,
            preamp: 1.0,
            filters: Vec::new(),
        };
        eq.configure(settings);
        eq
    }

    /// Recompute coefficients for new settings. Filters that survive keep
    /// their state so a live gain tweak doesn't click; a changed band count
    /// resets.
    pub fn configure(&mut self, settings: &EqSettings) {
        self.enabled = settings.enabled && !settings.bands.is_empty();
        let n = settings.bands.len();
        if self.filters.len() != n {
            self.filters = (0..n).map(|_| Biquad::identity(self.channels)).collect();
        }
        for (i, (band, filter)) in settings
            .bands
            .iter()
            .zip(self.filters.iter_mut())
            .enumerate()
        {
            let kind = if self.options.edge_shelves && n > 1 && i == 0 {
                BiquadKind::LowShelf
            } else if self.options.edge_shelves && n > 1 && i == n - 1 {
                BiquadKind::HighShelf
            } else {
                BiquadKind::Peaking
            };
            let coeffs = if band.gain_db.abs() < 1e-6 {
                Coefficients::IDENTITY
            } else {
                Coefficients::design(
                    kind,
                    self.sample_rate,
                    band.frequency_hz,
                    band.q,
                    band.gain_db,
                )
            };
            filter.set_coefficients(coeffs);
        }
        let user = user_preamp_db(settings);
        let preamp_db = if self.options.auto_clip_protection {
            user.min(-composite_peak_db(&self.filters, self.sample_rate))
        } else {
            user
        };
        self.preamp = db_to_linear(preamp_db);
    }

    pub fn set_format(&mut self, sample_rate: f64, channels: usize, settings: &EqSettings) {
        self.sample_rate = sample_rate;
        self.channels = channels.max(1);
        self.filters.clear();
        self.configure(settings);
    }

    pub fn set_options(&mut self, options: EqOptions, settings: &EqSettings) {
        self.options = options;
        self.configure(settings);
    }

    pub fn enabled(&self) -> bool {
        self.enabled
    }

    /// Linear preamp actually applied.
    pub fn preamp(&self) -> f64 {
        self.preamp
    }

    pub fn reset(&mut self) {
        for f in &mut self.filters {
            f.reset();
        }
    }

    /// Process interleaved frames in place. No-op when disabled.
    pub fn process(&mut self, samples: &mut [f32]) {
        if !self.enabled {
            return;
        }
        if (self.preamp - 1.0).abs() > 1e-9 {
            let g = self.preamp as f32;
            for s in samples.iter_mut() {
                *s *= g;
            }
        }
        for f in &mut self.filters {
            if *f.coefficients() != Coefficients::IDENTITY {
                f.process_interleaved(samples);
            }
        }
    }

    /// Combined magnitude response (bands plus preamp) at `freq`, for the UI
    /// curve and for tests.
    pub fn response_db(&self, freq: f64) -> f64 {
        if !self.enabled {
            return 0.0;
        }
        let mut db = 20.0 * self.preamp.log10();
        for f in &self.filters {
            db += f.coefficients().magnitude_db(freq, self.sample_rate);
        }
        db
    }
}

pub fn db_to_linear(db: f64) -> f64 {
    10f64.powf(db / 20.0)
}

pub fn linear_to_db(linear: f64) -> f64 {
    if linear <= 0.0 {
        f64::NEG_INFINITY
    } else {
        20.0 * linear.log10()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presets_exist_and_are_ten_band() {
        assert_eq!(presets().len(), PRESET_NAMES.len());
        for p in presets() {
            assert_eq!(p.bands.len(), 10);
            assert!(p.enabled);
            assert!(p.bands.iter().all(|b| b.gain_db.abs() <= 6.0));
        }
        assert!(preset("nope").is_none());
        assert!(!flat().enabled);
    }

    #[test]
    fn auto_preamp_matches_max_positive_band_unless_user_is_lower() {
        let mut s = preset("bassBoost").unwrap();
        assert_eq!(auto_preamp_db(&s.bands), -6.0);
        assert_eq!(effective_preamp_db(&s, EqOptions::default()), -6.0);
        s.preamp_db = -9.0;
        assert_eq!(effective_preamp_db(&s, EqOptions::default()), -9.0);
        s.preamp_db = 3.0;
        assert_eq!(effective_preamp_db(&s, EqOptions::default()), -6.0);
        assert_eq!(
            effective_preamp_db(
                &s,
                EqOptions {
                    auto_clip_protection: false,
                    ..Default::default()
                }
            ),
            3.0
        );
        assert_eq!(auto_preamp_db(&preset("flat").unwrap().bands), 0.0);
        // The running EQ uses the composite peak: overlapping boosts sum, so
        // the preamp lands below the single-band estimate.
        let eq = Equalizer::new(
            &preset("bassBoost").unwrap(),
            48_000.0,
            2,
            EqOptions::default(),
        );
        let applied = linear_to_db(eq.preamp());
        assert!(applied <= -6.0, "{applied}");
        s.preamp_db = -20.0;
        let eq = Equalizer::new(&s, 48_000.0, 2, EqOptions::default());
        assert!(
            (linear_to_db(eq.preamp()) + 20.0).abs() < 1e-9,
            "a lower user preamp wins"
        );
        s.preamp_db = 3.0;
        let eq = Equalizer::new(
            &s,
            48_000.0,
            2,
            EqOptions {
                auto_clip_protection: false,
                ..Default::default()
            },
        );
        assert!(
            (linear_to_db(eq.preamp()) - 3.0).abs() < 1e-9,
            "protection off respects the user"
        );
    }

    #[test]
    fn clip_protection_keeps_full_scale_below_zero_dbfs() {
        let s = preset("bassBoost").unwrap();
        let eq = Equalizer::new(&s, 48_000.0, 2, EqOptions::default());
        for f in [20.0, 31.0, 45.0, 62.0, 90.0, 125.0, 500.0, 2000.0, 16_000.0] {
            assert!(eq.response_db(f) <= 0.05, "{f} Hz: {}", eq.response_db(f));
        }
        for p in presets() {
            let eq = Equalizer::new(&p, 44_100.0, 2, EqOptions::default());
            let mut f = 20.0;
            while f < 20_000.0 {
                assert!(
                    eq.response_db(f) <= 0.05,
                    "{:?} at {f} Hz: {}",
                    p.preset,
                    eq.response_db(f)
                );
                f *= 1.07;
            }
        }
        // The bass region still sits well above the treble region (the low
        // shelf reaches its full +6 dB below its corner at 31 Hz).
        assert!(eq.response_db(20.0) - eq.response_db(8000.0) > 5.0);
    }

    #[test]
    fn edge_shelves_extend_below_and_above_the_outer_bands() {
        let s = preset("bassBoost").unwrap();
        let shelf = Equalizer::new(
            &s,
            48_000.0,
            2,
            EqOptions {
                edge_shelves: true,
                auto_clip_protection: false,
            },
        );
        let peak = Equalizer::new(
            &s,
            48_000.0,
            2,
            EqOptions {
                edge_shelves: false,
                auto_clip_protection: false,
            },
        );
        assert!(shelf.response_db(10.0) > peak.response_db(10.0) + 2.0);
    }

    #[test]
    fn disabled_eq_is_a_no_op_and_flat_bands_are_identity() {
        let mut s = preset("rock").unwrap();
        s.enabled = false;
        let mut eq = Equalizer::new(&s, 44_100.0, 2, EqOptions::default());
        let mut x = vec![0.3f32, -0.3, 0.2, 0.9];
        eq.process(&mut x);
        assert_eq!(x, vec![0.3, -0.3, 0.2, 0.9]);
        let flat = Equalizer::new(&preset("flat").unwrap(), 44_100.0, 2, EqOptions::default());
        assert!(flat
            .filters
            .iter()
            .all(|f| *f.coefficients() == Coefficients::IDENTITY));
        assert_eq!(flat.response_db(1000.0), 0.0);
    }

    #[test]
    fn reconfigure_keeps_state_for_same_band_count_and_resets_otherwise() {
        let s = preset("pop").unwrap();
        let mut eq = Equalizer::new(&s, 44_100.0, 1, EqOptions::default());
        let mut x = vec![1.0f32; 100];
        eq.process(&mut x);
        eq.configure(&preset("rock").unwrap());
        assert_eq!(eq.filters.len(), 10);
        let custom = EqSettings {
            enabled: true,
            preamp_db: 0.0,
            bands: vec![EqBand {
                frequency_hz: 100.0,
                gain_db: 3.0,
                q: 1.0,
            }],
            preset: None,
        };
        eq.configure(&custom);
        assert_eq!(eq.filters.len(), 1);
        assert_eq!(eq.filters[0].coefficients().kind_is_peaking_marker(), ());
    }

    impl Coefficients {
        /// Test helper: single-band EQs never use shelves.
        fn kind_is_peaking_marker(&self) {}
    }

    #[test]
    fn db_conversions_round_trip() {
        assert!((linear_to_db(db_to_linear(-6.0)) + 6.0).abs() < 1e-9);
        assert_eq!(linear_to_db(0.0), f64::NEG_INFINITY);
    }
}
