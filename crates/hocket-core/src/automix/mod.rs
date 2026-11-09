//! AutoMix: transitions between consecutive tracks, from a plain crossfade up
//! to beat-matched blends. `docs/automix.md` is the contract; `docs/design.md`
//! → *AutoMix* records the decisions.
//!
//! Everything here is pure: plain data and PCM buffers in, plans and PCM out,
//! no I/O, no decoding, and it compiles on every target (Android included).
//!
//! - [`record`]: [`AnalysisRecord`] usability and preference.
//! - [`loudness`]: loudness curves, silence edges and loudness-only records from PCM.
//! - [`rules`]: when not to mix ([`rules::should_mix`]).
//! - [`plan`]: the deterministic planner ([`plan::plan_transition`]).
//! - [`render`]: the outgoing tail ([`render::render_tail`]) and [`render::IncomingShaper`].
//! - [`stretch`]: the time-stretcher behind the tail's tempo ramp.

pub mod loudness;
pub mod plan;
pub mod record;
pub mod render;
pub mod rules;
pub mod stretch;

pub use crate::api::{
    AnalysisProducer, AnalysisRecord, FadeCurve, LoudnessCurve, ShelfMove, SourceFingerprint,
    TempoRamp, TransitionKind, TransitionMode, TransitionPlan, TransitionSettings,
};
pub use plan::{late_edit_fade, plan_transition, PlanInput, PlanTrack};
pub use record::{fingerprint_matches, is_usable, pick_best, ANALYSIS_VERSION};
pub use render::{render_tail, IncomingShaper};
pub use rules::{should_mix, MixContext, MixDecision, MixTrack, NoMixReason};

/// Shortest and longest crossfade the setting allows, in seconds.
pub const CROSSFADE_MIN_SECONDS: f64 = 1.0;
pub const CROSSFADE_MAX_SECONDS: f64 = 12.0;
/// AutoMix's fade length when it has nothing better to go on.
pub const AUTOMIX_DEFAULT_FADE_MS: u32 = 6_000;

impl TransitionMode {
    /// Whether this mode ever mixes.
    pub fn is_on(self) -> bool {
        !matches!(self, TransitionMode::Off)
    }
}

impl TransitionSettings {
    /// `crossfade_seconds` clamped to 1..=12 (6 when not finite), in milliseconds.
    pub fn crossfade_ms(&self) -> u32 {
        let s = if self.crossfade_seconds.is_finite() {
            self.crossfade_seconds
                .clamp(CROSSFADE_MIN_SECONDS, CROSSFADE_MAX_SECONDS)
        } else {
            6.0
        };
        (s * 1000.0).round() as u32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crossfade_is_clamped() {
        let mut s = TransitionSettings::default();
        assert_eq!(s.mode, TransitionMode::Off);
        assert_eq!(s.crossfade_ms(), 6_000);
        s.crossfade_seconds = 0.2;
        assert_eq!(s.crossfade_ms(), 1_000);
        s.crossfade_seconds = 40.0;
        assert_eq!(s.crossfade_ms(), 12_000);
        s.crossfade_seconds = f64::NAN;
        assert_eq!(s.crossfade_ms(), 6_000);
    }

    #[test]
    fn settings_round_trip_camel_case() {
        let s = TransitionSettings {
            mode: TransitionMode::AutoMix,
            crossfade_seconds: 8.0,
        };
        let json = serde_json::to_string(&s).unwrap();
        assert_eq!(json, r#"{"mode":"autoMix","crossfadeSeconds":8.0}"#);
        assert_eq!(
            serde_json::from_str::<TransitionSettings>(&json).unwrap(),
            s
        );
        assert!(TransitionMode::Crossfade.is_on());
        assert!(!TransitionMode::Off.is_on());
    }
}
