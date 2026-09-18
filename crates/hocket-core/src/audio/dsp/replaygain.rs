//! ReplayGain: choosing the gain for a track and applying it safely.
//!
//! [`gain_for_track`] is pure and shared by the native chain and the
//! external bridge (it is what goes into [`crate::api::MediaSource::gain_db`]).
//! [`SoftLimiter`] sits at the end of the chain as the last line of defence
//! for anything the peak clamp couldn't foresee (EQ boosts, normalisation,
//! a positive preamp).

use crate::api::{ReplayGain, ReplayGainMode};
use crate::audio::dsp::eq::db_to_linear;

/// Gain to apply for a track, in dB.
///
/// - `Off` → `0.0` regardless of tags or preamp.
/// - `Track` → track gain; `Album` → album gain, falling back to track gain
///   when the album value is missing (and vice versa for `Track`).
/// - `Auto` → album gain when `is_album_context`, track gain otherwise.
/// - The chosen gain is **clamped by the matching peak** so
///   `gain × peak ≤ 1.0` (no clipping on the loudest sample). Missing peak →
///   no clamp.
/// - `preamp_db` is added *after* the clamp: it is the user's explicit
///   override and may push above full scale, which the soft limiter then
///   catches. Tracks without any tag get the preamp only — matching
///   Symfonium, which applies the preamp uniformly and has a separate
///   "gain for untagged tracks" that we leave at 0.
pub fn gain_for_track(
    track_rg: Option<&ReplayGain>,
    mode: ReplayGainMode,
    is_album_context: bool,
    preamp_db: f64,
) -> f64 {
    if mode == ReplayGainMode::Off {
        return 0.0;
    }
    let preamp = if preamp_db.is_finite() {
        preamp_db.clamp(-15.0, 15.0)
    } else {
        0.0
    };
    let Some(rg) = track_rg else { return preamp };
    let want_album = match mode {
        ReplayGainMode::Album => true,
        ReplayGainMode::Track => false,
        ReplayGainMode::Auto => is_album_context,
        ReplayGainMode::Off => unreachable!("handled above"),
    };
    let finite = |v: Option<f64>| v.filter(|x| x.is_finite());
    let (gain, peak) = if want_album {
        match finite(rg.album_gain_db) {
            Some(g) => (Some(g), finite(rg.album_peak).or(finite(rg.track_peak))),
            None => (finite(rg.track_gain_db), finite(rg.track_peak)),
        }
    } else {
        match finite(rg.track_gain_db) {
            Some(g) => (Some(g), finite(rg.track_peak)),
            None => (finite(rg.album_gain_db), finite(rg.album_peak)),
        }
    };
    let Some(gain) = gain else { return preamp };
    let gain = gain.clamp(-40.0, 40.0);
    let clamped = match peak {
        Some(p) if p > 0.0 => {
            // Largest gain that keeps the peak at or below 0 dBFS.
            let ceiling = -20.0 * p.log10();
            gain.min(ceiling)
        }
        _ => gain,
    };
    clamped + preamp
}

/// Soft-knee limiter that maps anything above `threshold` smoothly to below
/// full scale (`tanh` knee), so overshoot from a positive preamp or EQ boost
/// rounds off instead of hard-clipping. Memoryless, so it never colours
/// material that stays under the threshold.
#[derive(Debug, Clone, Copy)]
pub struct SoftLimiter {
    threshold: f32,
    headroom: f32,
}

impl SoftLimiter {
    /// `threshold_db` ≤ 0; the knee starts there and the output asymptotes
    /// to 0 dBFS.
    pub fn new(threshold_db: f64) -> Self {
        let t = db_to_linear(threshold_db.clamp(-12.0, -0.01)) as f32;
        // The knee asymptotes to slightly under 0 dBFS so `tanh` saturating
        // to exactly 1.0 in f32 can never yield a full-scale sample.
        Self {
            threshold: t,
            headroom: (1.0 - t) * 0.998,
        }
    }

    #[inline]
    pub fn limit(&self, x: f32) -> f32 {
        let a = x.abs();
        if a <= self.threshold {
            x
        } else {
            let y = self.threshold
                + self.headroom * ((a - self.threshold) / (1.0 - self.threshold)).tanh();
            y.copysign(x)
        }
    }

    pub fn process(&self, samples: &mut [f32]) {
        for s in samples.iter_mut() {
            *s = self.limit(*s);
        }
    }
}

impl Default for SoftLimiter {
    fn default() -> Self {
        Self::new(-1.0)
    }
}

/// Gain with a linear ramp so changes never click. Ramps over `ramp_frames`
/// frames from the current value to the target.
#[derive(Debug, Clone)]
pub struct SmoothGain {
    current: f32,
    target: f32,
    step: f32,
    remaining: usize,
    ramp_frames: usize,
}

impl SmoothGain {
    pub fn new(initial: f64, ramp_frames: usize) -> Self {
        let g = initial as f32;
        Self {
            current: g,
            target: g,
            step: 0.0,
            remaining: 0,
            ramp_frames: ramp_frames.max(1),
        }
    }

    pub fn set_target(&mut self, target: f64) {
        let t = target as f32;
        if (t - self.target).abs() < 1e-9 && self.remaining == 0 {
            return;
        }
        self.target = t;
        self.remaining = self.ramp_frames;
        self.step = (t - self.current) / self.ramp_frames as f32;
    }

    /// Jump immediately (track boundary: the new track's gain applies from
    /// its first sample, no crossfade of levels).
    pub fn snap(&mut self, value: f64) {
        self.current = value as f32;
        self.target = self.current;
        self.remaining = 0;
    }

    pub fn current(&self) -> f32 {
        self.current
    }

    pub fn target(&self) -> f32 {
        self.target
    }

    pub fn is_ramping(&self) -> bool {
        self.remaining > 0
    }

    pub fn is_unity(&self) -> bool {
        self.remaining == 0 && (self.current - 1.0).abs() < 1e-7
    }

    /// Apply to interleaved frames of `channels`.
    pub fn process(&mut self, samples: &mut [f32], channels: usize) {
        let channels = channels.max(1);
        if self.remaining == 0 {
            if (self.current - 1.0).abs() > 1e-7 {
                let g = self.current;
                for s in samples.iter_mut() {
                    *s *= g;
                }
            }
            return;
        }
        for frame in samples.chunks_mut(channels) {
            if self.remaining > 0 {
                self.current += self.step;
                self.remaining -= 1;
                if self.remaining == 0 {
                    self.current = self.target;
                }
            }
            let g = self.current;
            for s in frame {
                *s *= g;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rg(tg: Option<f64>, tp: Option<f64>, ag: Option<f64>, ap: Option<f64>) -> ReplayGain {
        ReplayGain {
            track_gain_db: tg,
            track_peak: tp,
            album_gain_db: ag,
            album_peak: ap,
        }
    }

    #[test]
    fn modes_pick_the_right_field_and_fall_back() {
        let r = rg(Some(-6.0), Some(0.5), Some(-8.0), Some(0.6));
        assert_eq!(
            gain_for_track(Some(&r), ReplayGainMode::Off, true, 3.0),
            0.0
        );
        assert_eq!(
            gain_for_track(Some(&r), ReplayGainMode::Track, true, 0.0),
            -6.0
        );
        assert_eq!(
            gain_for_track(Some(&r), ReplayGainMode::Album, false, 0.0),
            -8.0
        );
        assert_eq!(
            gain_for_track(Some(&r), ReplayGainMode::Auto, true, 0.0),
            -8.0
        );
        assert_eq!(
            gain_for_track(Some(&r), ReplayGainMode::Auto, false, 0.0),
            -6.0
        );
        let only_track = rg(Some(-6.0), None, None, None);
        assert_eq!(
            gain_for_track(Some(&only_track), ReplayGainMode::Album, true, 0.0),
            -6.0
        );
        let only_album = rg(None, None, Some(-2.0), None);
        assert_eq!(
            gain_for_track(Some(&only_album), ReplayGainMode::Track, false, 0.0),
            -2.0
        );
    }

    #[test]
    fn untagged_tracks_get_the_preamp_only() {
        assert_eq!(
            gain_for_track(None, ReplayGainMode::Track, false, -3.0),
            -3.0
        );
        assert_eq!(
            gain_for_track(
                Some(&rg(None, None, None, None)),
                ReplayGainMode::Auto,
                true,
                2.0
            ),
            2.0
        );
        assert_eq!(
            gain_for_track(None, ReplayGainMode::Track, false, f64::NAN),
            0.0
        );
        assert_eq!(
            gain_for_track(None, ReplayGainMode::Track, false, 99.0),
            15.0
        );
    }

    #[test]
    fn peak_clamps_the_gain_but_preamp_is_added_after() {
        // +9 dB gain on a track peaking at 0.5 (−6 dBFS) is clamped to +6 dB.
        let r = rg(Some(9.0), Some(0.5), None, None);
        assert!(
            (gain_for_track(Some(&r), ReplayGainMode::Track, false, 0.0) - 6.0206).abs() < 1e-3
        );
        assert!(
            (gain_for_track(Some(&r), ReplayGainMode::Track, false, 2.0) - 8.0206).abs() < 1e-3
        );
        // Negative gain is never affected by the clamp.
        let r = rg(Some(-9.0), Some(1.0), None, None);
        assert_eq!(
            gain_for_track(Some(&r), ReplayGainMode::Track, false, 0.0),
            -9.0
        );
        // Peak 0 / missing → no clamp.
        let r = rg(Some(9.0), Some(0.0), None, None);
        assert_eq!(
            gain_for_track(Some(&r), ReplayGainMode::Track, false, 0.0),
            9.0
        );
        // Album mode falls back to the track peak when the album peak is missing.
        let r = rg(Some(1.0), Some(0.25), Some(20.0), None);
        assert!(
            (gain_for_track(Some(&r), ReplayGainMode::Album, true, 0.0) - 12.0412).abs() < 1e-3
        );
    }

    #[test]
    fn soft_limiter_is_transparent_below_threshold_and_bounded_above() {
        let l = SoftLimiter::new(-1.0);
        assert_eq!(l.limit(0.5), 0.5);
        assert_eq!(l.limit(-0.8), -0.8);
        for x in [0.95f32, 1.0, 1.5, 4.0, 100.0] {
            let y = l.limit(x);
            assert!(y < 1.0 && y > 0.89, "{x} -> {y}");
            assert_eq!(l.limit(-x), -y);
        }
        assert!(l.limit(2.0) > l.limit(1.5), "monotonic");
        let mut v = vec![3.0f32, -3.0, 0.1];
        l.process(&mut v);
        assert!(v[0] < 1.0 && v[1] > -1.0 && v[2] == 0.1);
    }

    #[test]
    fn smooth_gain_ramps_linearly_and_snaps() {
        let mut g = SmoothGain::new(1.0, 4);
        g.set_target(0.0);
        assert!(g.is_ramping());
        let mut x = vec![1.0f32; 8];
        g.process(&mut x, 2);
        assert_eq!(x, vec![0.75, 0.75, 0.5, 0.5, 0.25, 0.25, 0.0, 0.0]);
        assert!(!g.is_ramping());
        let mut y = vec![1.0f32; 2];
        g.process(&mut y, 2);
        assert_eq!(y, vec![0.0, 0.0]);
        g.snap(0.5);
        let mut z = vec![1.0f32; 2];
        g.process(&mut z, 1);
        assert_eq!(z, vec![0.5, 0.5]);
        g.snap(1.0);
        assert!(g.is_unity());
    }
}
