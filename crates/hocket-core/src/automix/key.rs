//! Musical keys on the Camelot wheel, and how well two keys mix.
//!
//! A [`MusicalKey`] comes from the analysis worker (`analysis::key`); this
//! module is the pure part every device runs: Camelot codes and the
//! [`Harmony`] between A's tail key and B's head key, which the planner
//! (`plan`) and mixability scoring (`order`) read.
//!
//! Camelot neighbours are the keys a DJ mixes between without a clash: the
//! same key, a fifth up or down (±1 on the wheel), and the relative major or
//! minor (same number, other letter). They are also the estimator's usual
//! mistakes (a fifth off, major for its relative minor), so a wrong estimate
//! usually still lands on a compatible key.

use crate::api::{AnalysisRecord, MusicalKey};

/// Keys weaker than this (correlation with the key profile) are treated as
/// unknown: atonal, percussive or modulating material gives a flat chroma.
/// On the corpus ~90 % of tracks reach it, and 91 % of those land on a
/// Camelot neighbour of Essentia's estimate (55 % of the rest do).
pub const MIN_KEY_STRENGTH: f32 = 0.7;

/// How two keys sound together.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Harmony {
    /// Two keys a semitone or tritone apart, or otherwise far round the
    /// wheel: a long blend sounds wrong.
    Clash,
    /// Two steps round the wheel, or a diagonal (a fifth and a mode change):
    /// fine for a short blend.
    Near,
    /// One key unknown or too weak to trust: plan as before keys.
    Unknown,
    /// Same key, a fifth apart, or relative major/minor.
    Compatible,
}

impl Harmony {
    /// Whether the planner keeps its usual blend lengths.
    pub fn allows_long_blend(self) -> bool {
        matches!(self, Harmony::Compatible | Harmony::Unknown)
    }
}

impl MusicalKey {
    /// Camelot number, 1..=12 (C major and A minor are 8).
    pub fn camelot_number(&self) -> u32 {
        // The relative major's tonic: a minor key's is three semitones up.
        let major_tonic = if self.minor {
            (self.tonic + 3) % 12
        } else {
            self.tonic % 12
        };
        // Each fifth up is one step round the wheel; C is 8.
        (major_tonic * 7 + 7) % 12 + 1
    }

    /// Camelot code, e.g. `8B` (C major) or `8A` (A minor).
    pub fn camelot(&self) -> String {
        format!(
            "{}{}",
            self.camelot_number(),
            if self.minor { 'A' } else { 'B' }
        )
    }

    /// Conventional name, e.g. `C`, `F#m`.
    pub fn name(&self) -> String {
        const NAMES: [&str; 12] = [
            "C", "C#", "D", "Eb", "E", "F", "F#", "G", "Ab", "A", "Bb", "B",
        ];
        format!(
            "{}{}",
            NAMES[(self.tonic % 12) as usize],
            if self.minor { "m" } else { "" }
        )
    }

    /// Strong enough to plan with.
    pub fn is_trusted(&self) -> bool {
        self.strength.is_finite() && self.strength >= MIN_KEY_STRENGTH
    }
}

/// Steps between two Camelot numbers round the wheel (0..=6).
fn wheel_steps(a: u32, b: u32) -> u32 {
    let d = a.abs_diff(b) % 12;
    d.min(12 - d)
}

/// How `a` into `b` sounds, ignoring trust (see [`harmony`]).
pub fn key_harmony(a: &MusicalKey, b: &MusicalKey) -> Harmony {
    let steps = wheel_steps(a.camelot_number(), b.camelot_number());
    match (steps, a.minor == b.minor) {
        (0, _) | (1, true) => Harmony::Compatible,
        (1, false) | (2, true) => Harmony::Near,
        _ => Harmony::Clash,
    }
}

/// The key of `r` at its end (`tail`) or start: the region's estimate when
/// trusted, else the whole track's, else `None`.
pub fn end_key(r: &AnalysisRecord, tail: bool) -> Option<&MusicalKey> {
    let region = if tail { &r.tail_key } else { &r.head_key };
    region
        .as_ref()
        .filter(|k| k.is_trusted())
        .or(r.key.as_ref().filter(|k| k.is_trusted()))
}

/// How A's tail into B's head sounds; [`Harmony::Unknown`] unless both have
/// a trusted key.
pub fn harmony(ra: &AnalysisRecord, rb: &AnalysisRecord) -> Harmony {
    match (end_key(ra, true), end_key(rb, false)) {
        (Some(a), Some(b)) => key_harmony(a, b),
        _ => Harmony::Unknown,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn k(tonic: u32, minor: bool) -> MusicalKey {
        MusicalKey {
            tonic,
            minor,
            strength: 0.8,
        }
    }

    #[test]
    fn camelot_codes() {
        assert_eq!(k(0, false).camelot(), "8B"); // C
        assert_eq!(k(9, true).camelot(), "8A"); // Am
        assert_eq!(k(7, false).camelot(), "9B"); // G
        assert_eq!(k(5, false).camelot(), "7B"); // F
        assert_eq!(k(4, true).camelot(), "9A"); // Em
        assert_eq!(k(6, false).camelot(), "2B"); // F#
        assert_eq!(k(8, true).camelot(), "1A"); // Abm
        assert_eq!(k(11, false).camelot(), "1B"); // B
        assert_eq!(k(10, true).name(), "Bbm");
        // Every code exactly once.
        let mut codes: Vec<String> = (0..12)
            .flat_map(|t| [k(t, false).camelot(), k(t, true).camelot()])
            .collect();
        codes.sort();
        codes.dedup();
        assert_eq!(codes.len(), 24);
    }

    #[test]
    fn harmony_of_keys() {
        use Harmony::*;
        let c = k(0, false);
        assert_eq!(key_harmony(&c, &c), Compatible);
        assert_eq!(key_harmony(&c, &k(7, false)), Compatible); // G, a fifth up
        assert_eq!(key_harmony(&c, &k(5, false)), Compatible); // F
        assert_eq!(key_harmony(&c, &k(9, true)), Compatible); // Am, relative
        assert_eq!(key_harmony(&c, &k(4, true)), Near); // Em, diagonal
        assert_eq!(key_harmony(&c, &k(2, false)), Near); // D, two fifths
        assert_eq!(key_harmony(&c, &k(1, false)), Clash); // Db, a semitone
        assert_eq!(key_harmony(&c, &k(6, false)), Clash); // F#, a tritone
        assert_eq!(key_harmony(&k(11, false), &k(6, false)), Compatible); // 1B → 2B wraps
        assert_eq!(key_harmony(&k(6, false), &k(11, false)), Compatible);
        assert!(Clash < Near && Near < Unknown && Unknown < Compatible);
    }

    #[test]
    fn harmony_of_records_prefers_trusted_regions() {
        let mut a = AnalysisRecord {
            key: Some(k(0, false)),
            ..Default::default()
        };
        let mut b = AnalysisRecord {
            key: Some(k(1, false)),
            ..Default::default()
        };
        assert_eq!(harmony(&a, &b), Harmony::Clash);
        // B's head is in C after all: the region wins.
        b.head_key = Some(k(0, false));
        assert_eq!(harmony(&a, &b), Harmony::Compatible);
        // A weak tail estimate falls back to the track key.
        a.tail_key = Some(MusicalKey {
            strength: 0.1,
            ..k(6, false)
        });
        assert_eq!(harmony(&a, &b), Harmony::Compatible);
        a.key = None;
        assert_eq!(harmony(&a, &b), Harmony::Unknown);
    }
}
