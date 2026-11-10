//! [`AnalysisRecord`] rules: which record applies to a track, which of two
//! wins, and the small helpers the planner reads records through.

use std::cmp::Ordering;

use crate::api::{AnalysisProducer, AnalysisRecord, SourceFingerprint};

/// Version of every analysis algorithm (beats, loudness curve, silence
/// edges, voice). Bump on any change that would change a record's contents;
/// records of any other version are ignored, never migrated.
pub const ANALYSIS_VERSION: u32 = 2;

/// Duration tolerance between a record's source and the track.
pub const FINGERPRINT_DURATION_TOLERANCE_MS: u32 = 1_000;

/// Whether a record analysed from `record` describes the file `track`:
/// durations within 1 s, and size and suffix equal whenever both sides know them.
pub fn fingerprint_matches(record: &SourceFingerprint, track: &SourceFingerprint) -> bool {
    if record.duration_ms.abs_diff(track.duration_ms) > FINGERPRINT_DURATION_TOLERANCE_MS {
        return false;
    }
    if let (Some(a), Some(b)) = (record.size_bytes, track.size_bytes) {
        if a != b {
            return false;
        }
    }
    if let (Some(a), Some(b)) = (&record.suffix, &track.suffix) {
        if !a.eq_ignore_ascii_case(b) {
            return false;
        }
    }
    true
}

/// Whether the planner may use `record` for the track fingerprinted `track`.
pub fn is_usable(record: &AnalysisRecord, track: &SourceFingerprint) -> bool {
    record.version == ANALYSIS_VERSION
        && record.duration_ms > 0
        && record.audible_start_ms <= record.audible_end_ms
        && fingerprint_matches(&record.source, track)
}

fn producer_rank(p: AnalysisProducer) -> u8 {
    match p {
        AnalysisProducer::Worker => 2,
        AnalysisProducer::Desktop => 1,
        AnalysisProducer::Device => 0,
    }
}

/// Preference between two records for the same track; `Greater` means `a` is
/// better. Higher version, then beats over loudness-only, then Worker over
/// Desktop over Device, then newer.
pub fn preference(a: &AnalysisRecord, b: &AnalysisRecord) -> Ordering {
    a.version
        .cmp(&b.version)
        .then(has_beats(a).cmp(&has_beats(b)))
        .then(producer_rank(a.produced_by).cmp(&producer_rank(b.produced_by)))
        .then(a.created_at.total_cmp(&b.created_at))
}

/// The best usable record among `records` for `track`.
pub fn pick_best<'a>(
    records: impl IntoIterator<Item = &'a AnalysisRecord>,
    track: &SourceFingerprint,
) -> Option<&'a AnalysisRecord> {
    records
        .into_iter()
        .filter(|r| is_usable(r, track))
        .max_by(|a, b| preference(a, b))
}

/// The record carries a beat grid (not loudness-only).
pub fn has_beats(r: &AnalysisRecord) -> bool {
    !r.beats_ms.is_empty()
}

/// Tidy a record for storage and transport: beats sorted and deduplicated,
/// downbeats restricted to beats, curve values rounded to 0.1 LU and floored
/// at −70, voice probabilities rounded to 0.01 within 0..1 (a voice curve
/// without a hop emptied), non-finite numbers dropped. Planning on a compacted record gives
/// the same plan on every device.
pub fn compact(mut r: AnalysisRecord) -> AnalysisRecord {
    r.beats_ms.sort_unstable();
    r.beats_ms.dedup();
    r.downbeats_ms.sort_unstable();
    r.downbeats_ms.dedup();
    let beats = &r.beats_ms;
    r.downbeats_ms.retain(|d| beats.binary_search(d).is_ok());
    for curve in [&mut r.head, &mut r.tail] {
        for v in curve.lufs.iter_mut() {
            *v = round_lu(*v);
        }
    }
    for curve in [&mut r.head_vocals, &mut r.tail_vocals] {
        if curve.hop_ms == 0 {
            curve.voice.clear();
        }
        for v in curve.voice.iter_mut() {
            *v = if v.is_finite() {
                (v.clamp(0.0, 1.0) * 100.0).round() / 100.0
            } else {
                0.0
            };
        }
    }
    r.integrated_lufs = r
        .integrated_lufs
        .filter(|v| v.is_finite())
        .map(|v| (v * 10.0).round() / 10.0);
    r.bpm = r.bpm.filter(|v| v.is_finite() && *v > 0.0);
    r.beat_confidence = if r.beat_confidence.is_finite() {
        r.beat_confidence.clamp(0.0, 1.0)
    } else {
        0.0
    };
    if r.audible_end_ms > r.duration_ms {
        r.audible_end_ms = r.duration_ms;
    }
    if r.audible_start_ms > r.audible_end_ms {
        r.audible_start_ms = r.audible_end_ms;
    }
    r
}

/// Loudness floor of the curves (the absolute gate of BS.1770).
pub const LUFS_FLOOR: f32 = -70.0;

fn round_lu(v: f32) -> f32 {
    if v.is_finite() {
        ((v * 10.0).round() / 10.0).max(LUFS_FLOOR)
    } else {
        LUFS_FLOOR
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fp(duration_ms: u32) -> SourceFingerprint {
        SourceFingerprint {
            duration_ms,
            size_bytes: Some(1_000.0),
            suffix: Some("flac".into()),
            transcoded: false,
        }
    }

    fn rec(producer: AnalysisProducer, beats: bool, created_at: f64) -> AnalysisRecord {
        AnalysisRecord {
            version: ANALYSIS_VERSION,
            analyser: "test".into(),
            produced_by: producer,
            source: fp(200_000),
            duration_ms: 200_000,
            audible_end_ms: 199_000,
            beats_ms: if beats { vec![0, 500, 1000] } else { vec![] },
            created_at,
            ..Default::default()
        }
    }

    #[test]
    fn fingerprint_tolerates_a_second_and_unknowns() {
        assert!(fingerprint_matches(&fp(200_000), &fp(200_900)));
        assert!(!fingerprint_matches(&fp(200_000), &fp(201_100)));
        let mut other = fp(200_000);
        other.size_bytes = Some(999.0);
        assert!(!fingerprint_matches(&fp(200_000), &other));
        other.size_bytes = None;
        assert!(fingerprint_matches(&fp(200_000), &other));
        other.suffix = Some("FLAC".into());
        assert!(fingerprint_matches(&fp(200_000), &other));
        other.suffix = Some("mp3".into());
        assert!(!fingerprint_matches(&fp(200_000), &other));
    }

    #[test]
    fn other_versions_are_unusable() {
        let mut r = rec(AnalysisProducer::Worker, true, 0.0);
        assert!(is_usable(&r, &fp(200_000)));
        r.version = ANALYSIS_VERSION + 1;
        assert!(!is_usable(&r, &fp(200_000)));
    }

    #[test]
    fn preference_order() {
        let worker_loud = rec(AnalysisProducer::Worker, false, 5.0);
        let device_beats = rec(AnalysisProducer::Device, true, 1.0);
        let desktop_beats = rec(AnalysisProducer::Desktop, true, 1.0);
        let desktop_beats_new = rec(AnalysisProducer::Desktop, true, 2.0);
        let all = [
            worker_loud.clone(),
            device_beats.clone(),
            desktop_beats.clone(),
            desktop_beats_new.clone(),
        ];
        assert_eq!(pick_best(&all, &fp(200_000)), Some(&desktop_beats_new));
        assert_eq!(
            preference(&device_beats, &worker_loud),
            Ordering::Greater,
            "beats beat the producer"
        );
        assert_eq!(preference(&desktop_beats, &device_beats), Ordering::Greater);
        let mut old = desktop_beats_new.clone();
        old.version = 0;
        assert_eq!(preference(&old, &device_beats), Ordering::Less);
        assert_eq!(pick_best(&all, &fp(100_000)), None);
    }

    #[test]
    fn compact_tidies() {
        let mut r = rec(AnalysisProducer::Worker, true, 0.0);
        r.beats_ms = vec![1000, 0, 500, 500];
        r.downbeats_ms = vec![1000, 0, 750];
        r.head.lufs = vec![-12.345, f32::NEG_INFINITY, -90.0];
        r.beat_confidence = 3.0;
        r.bpm = Some(f32::NAN);
        r.head_vocals = crate::api::VocalCurve {
            start_ms: 0,
            hop_ms: 100,
            voice: vec![0.123, f32::NAN, 1.5, -0.2],
        };
        r.tail_vocals = crate::api::VocalCurve {
            start_ms: 0,
            hop_ms: 0,
            voice: vec![0.5],
        };
        let c = compact(r);
        assert_eq!(c.head_vocals.voice, vec![0.12, 0.0, 1.0, 0.0]);
        assert!(c.tail_vocals.voice.is_empty());
        assert_eq!(c.beats_ms, vec![0, 500, 1000]);
        assert_eq!(c.downbeats_ms, vec![0, 1000]);
        assert_eq!(c.head.lufs, vec![-12.3, -70.0, -70.0]);
        assert_eq!(c.beat_confidence, 1.0);
        assert_eq!(c.bpm, None);
    }
}
