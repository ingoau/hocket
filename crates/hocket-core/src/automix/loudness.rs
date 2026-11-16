//! Loudness envelope and silence edges from PCM (no decoding here), and
//! loudness-only [`AnalysisRecord`]s built from a track's head and tail.
//!
//! K-weighting and channel weights are the BS.1770 ones from
//! [`crate::audio::dsp::loudness`]; integrated loudness uses its gated
//! [`LoudnessMeter`]. The curve is "short-term-ish": 3 s windows centred on
//! points 100 ms apart.

use crate::api::{AnalysisProducer, AnalysisRecord, EpochMs, LoudnessCurve, Ms, SourceFingerprint};
use crate::audio::dsp::loudness::{channel_weights, KWeighting, LoudnessMeter};

use super::record::{compact, LUFS_FLOOR};
use super::ANALYSIS_VERSION;

/// Curve point spacing.
pub const CURVE_HOP_MS: u32 = 100;
/// Curve window length (centred on each point).
pub const CURVE_WINDOW_MS: u32 = 3_000;
/// The audible threshold for silence edges: −60 dBFS.
pub const AUDIBLE_THRESHOLD: f32 = 0.001;
/// How much of each end of a track the records describe.
pub const HEAD_TAIL_MS: u32 = 60_000;
/// `AnalysisRecord::analyser` of loudness-only records.
pub const LOUDNESS_ANALYSER: &str = "loudness";

fn frames_for_ms(ms: f64, rate: f64) -> usize {
    (ms * rate / 1000.0).round().max(0.0) as usize
}

fn ms_for_frames(frames: usize, rate: f64) -> Ms {
    (frames as f64 * 1000.0 / rate).round().min(u32::MAX as f64) as Ms
}

fn to_lufs(power: f64) -> f32 {
    if power <= 0.0 {
        return LUFS_FLOOR;
    }
    ((-0.691 + 10.0 * power.log10()) as f32).max(LUFS_FLOOR)
}

fn sane_rate(rate: u32) -> f64 {
    if rate == 0 {
        48_000.0
    } else {
        rate as f64
    }
}

/// K-weighted, channel-weighted mean power of each 100 ms hop of `pcm`.
fn hop_powers(pcm: &[f32], channels: usize, rate: f64) -> Vec<f64> {
    let channels = channels.max(1);
    let hop = frames_for_ms(CURVE_HOP_MS as f64, rate).max(1);
    let weights = channel_weights(channels);
    let mut filter = KWeighting::new(rate, channels);
    let mut out = Vec::with_capacity(pcm.len() / channels / hop + 1);
    let mut acc = 0.0f64;
    let mut n = 0usize;
    for frame in pcm.chunks_exact(channels) {
        for (ch, &s) in frame.iter().enumerate() {
            let y = filter.tick(ch, s as f64);
            acc += weights[ch] * y * y;
        }
        n += 1;
        if n == hop {
            out.push(acc / n as f64);
            acc = 0.0;
            n = 0;
        }
    }
    if n > 0 {
        out.push(acc / n as f64);
    }
    out
}

/// Short-term loudness curve of `pcm` (interleaved), starting at track
/// position `start_ms`. Point `i` is the loudness of the 3 s window centred
/// on `start_ms + i·100 ms` (clipped at the buffer's edges).
pub fn loudness_curve(pcm: &[f32], channels: usize, rate: u32, start_ms: Ms) -> LoudnessCurve {
    let rate = sane_rate(rate);
    let powers = hop_powers(pcm, channels, rate);
    let half = (CURVE_WINDOW_MS / CURVE_HOP_MS / 2) as usize;
    // Prefix sums keep this linear in the curve length.
    let mut prefix = Vec::with_capacity(powers.len() + 1);
    prefix.push(0.0f64);
    for p in &powers {
        prefix.push(prefix.last().unwrap() + p);
    }
    let lufs = (0..powers.len())
        .map(|i| {
            let lo = i.saturating_sub(half);
            let hi = (i + half).min(powers.len());
            let n = hi - lo;
            to_lufs((prefix[hi] - prefix[lo]) / n.max(1) as f64)
        })
        .collect();
    LoudnessCurve {
        start_ms,
        hop_ms: CURVE_HOP_MS,
        lufs,
    }
}

/// First and last frame holding a sample above −60 dBFS; `None` for silence.
pub fn audible_frames(pcm: &[f32], channels: usize) -> Option<(usize, usize)> {
    let channels = channels.max(1);
    let loud = |s: &f32| s.abs() > AUDIBLE_THRESHOLD;
    let first = pcm.iter().position(loud)? / channels;
    let last = pcm.iter().rposition(loud)? / channels;
    Some((first, last))
}

/// Gated integrated loudness of `pcm` (BS.1770 / EBU R128), `None` for silence.
pub fn integrated_lufs(pcm: &[f32], channels: usize, rate: u32) -> Option<f32> {
    let mut m = LoudnessMeter::new(sane_rate(rate), channels.max(1));
    m.push(pcm);
    m.integrated_lufs().map(|v| v as f32)
}

/// What a loudness-only record is built from: the decoded start and end of a
/// track, interleaved at `rate`.
pub struct HeadTail<'a> {
    /// PCM from position 0 (ideally [`HEAD_TAIL_MS`] long, or the whole track).
    pub head: &'a [f32],
    /// PCM ending at the track's end (ideally [`HEAD_TAIL_MS`] long).
    pub tail: &'a [f32],
    /// Track position of `tail`'s first frame.
    pub tail_start_ms: Ms,
    pub channels: usize,
    pub rate: u32,
}

/// A loudness-only [`AnalysisRecord`] (no beats) for a track of
/// `source.duration_ms`. Integrated loudness is measured over head and tail
/// together: an approximation of the whole track, good enough to compare a
/// track's ends with its body.
pub fn loudness_record(
    pcm: HeadTail<'_>,
    source: SourceFingerprint,
    produced_by: AnalysisProducer,
    created_at: EpochMs,
) -> AnalysisRecord {
    let channels = pcm.channels.max(1);
    let rate = sane_rate(pcm.rate);
    let duration_ms = source.duration_ms;
    let head_len_ms = ms_for_frames(pcm.head.len() / channels, rate);

    let audible_start_ms = match audible_frames(pcm.head, channels) {
        Some((first, _)) => ms_for_frames(first, rate),
        // A silent head: the music starts somewhere after it.
        None => head_len_ms,
    };
    let audible_end_ms = match audible_frames(pcm.tail, channels) {
        Some((_, last)) => pcm
            .tail_start_ms
            .saturating_add(ms_for_frames(last + 1, rate)),
        None => pcm.tail_start_ms,
    }
    .min(duration_ms);

    let mut meter = LoudnessMeter::new(rate, channels);
    meter.push(pcm.head);
    if pcm.tail_start_ms >= head_len_ms {
        meter.push(pcm.tail);
    }
    compact(AnalysisRecord {
        version: ANALYSIS_VERSION,
        analyser: LOUDNESS_ANALYSER.into(),
        produced_by,
        source,
        duration_ms,
        integrated_lufs: meter.integrated_lufs().map(|v| v as f32),
        audible_start_ms: audible_start_ms.min(duration_ms),
        audible_end_ms,
        head: loudness_curve(pcm.head, channels, pcm.rate, 0),
        tail: loudness_curve(pcm.tail, channels, pcm.rate, pcm.tail_start_ms),
        head_vocals: Default::default(),
        tail_vocals: Default::default(),
        bpm: None,
        beats_ms: Vec::new(),
        downbeats_ms: Vec::new(),
        beat_confidence: 0.0,
        created_at,
    })
}

/// Loudness at track position `ms` from a curve, `None` outside it.
pub fn curve_at(curve: &LoudnessCurve, ms: Ms) -> Option<f32> {
    if ms < curve.start_ms || curve.hop_ms == 0 {
        return None;
    }
    let i = ((ms - curve.start_ms) / curve.hop_ms) as usize;
    curve.lufs.get(i).copied()
}

/// Track position of curve point `i`.
pub fn curve_time(curve: &LoudnessCurve, i: usize) -> Ms {
    curve
        .start_ms
        .saturating_add((i as u32).saturating_mul(curve.hop_ms))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub const RATE: u32 = 48_000;

    /// Stereo sine at `amp` for `ms`, surrounded by silence.
    pub fn tone(silence_before_ms: u32, ms: u32, silence_after_ms: u32, amp: f32) -> Vec<f32> {
        let f = |ms: u32| frames_for_ms(ms as f64, RATE as f64);
        let mut out = vec![0.0; f(silence_before_ms) * 2];
        for i in 0..f(ms) {
            let s = amp * (2.0 * std::f32::consts::PI * 440.0 * i as f32 / RATE as f32).sin();
            out.push(s);
            out.push(s);
        }
        out.extend(std::iter::repeat_n(0.0, f(silence_after_ms) * 2));
        out
    }

    #[test]
    fn audible_edges_find_the_tone() {
        let pcm = tone(1_500, 4_000, 2_000, 0.5);
        let (a, b) = audible_frames(&pcm, 2).unwrap();
        let a_ms = ms_for_frames(a, RATE as f64);
        let b_ms = ms_for_frames(b, RATE as f64);
        assert!((1_500..=1_501).contains(&a_ms), "{a_ms}");
        assert!((5_495..=5_500).contains(&b_ms), "{b_ms}");
        assert_eq!(audible_frames(&[0.0; 100], 2), None);
        // −70 dBFS is below the threshold.
        assert_eq!(audible_frames(&tone(0, 100, 0, 0.000_3), 2), None);
    }

    #[test]
    fn curve_tracks_level_and_floors_silence() {
        let pcm = tone(5_000, 10_000, 5_000, 0.5);
        let c = loudness_curve(&pcm, 2, RATE, 1_000);
        assert_eq!(c.hop_ms, 100);
        assert_eq!(c.start_ms, 1_000);
        assert_eq!(c.lufs.len(), 200);
        // Silence well away from the tone is floored, the middle of the tone is loud.
        assert_eq!(c.lufs[10], LUFS_FLOOR);
        assert_eq!(c.lufs[195], LUFS_FLOOR);
        let mid = c.lufs[100];
        // A 0.5-amplitude 440 Hz sine on two channels: about −6 dBFS RMS −3 dB, +3 dB for two channels.
        assert!((-10.0..-5.0).contains(&mid), "{mid}");
        assert!(c.lufs.iter().all(|v| v.is_finite()));
        // Halfway into the window edge the level is ~3 dB lower.
        assert!(
            c.lufs[50] < mid - 2.0 && c.lufs[50] > mid - 4.5,
            "{}",
            c.lufs[50]
        );
        assert_eq!(curve_at(&c, 11_000), Some(mid));
        assert_eq!(curve_at(&c, 500), None);
        assert_eq!(curve_time(&c, 3), 1_300);
    }

    #[test]
    fn integrated_matches_the_dsp_meter() {
        let pcm = tone(0, 5_000, 0, 0.25);
        let ours = integrated_lufs(&pcm, 2, RATE).unwrap();
        let dsp = crate::audio::dsp::loudness::measure_loudness(&pcm, 2, RATE as f64)
            .lufs
            .unwrap();
        assert!((ours as f64 - dsp).abs() < 1e-3);
        assert_eq!(integrated_lufs(&[0.0; 9_600], 2, RATE), None);
    }

    #[test]
    fn loudness_record_from_head_and_tail() {
        // A 120 s "track": 2 s of silence, tone, 3 s of silence at the end.
        let head = tone(2_000, 58_000, 0, 0.5);
        let tail = tone(0, 57_000, 3_000, 0.5);
        let source = SourceFingerprint {
            duration_ms: 120_000,
            size_bytes: None,
            suffix: Some("flac".into()),
            transcoded: false,
        };
        let r = loudness_record(
            HeadTail {
                head: &head,
                tail: &tail,
                tail_start_ms: 60_000,
                channels: 2,
                rate: RATE,
            },
            source.clone(),
            AnalysisProducer::Device,
            1.0,
        );
        assert_eq!(r.version, ANALYSIS_VERSION);
        assert_eq!(r.analyser, LOUDNESS_ANALYSER);
        assert!(r.beats_ms.is_empty());
        assert_eq!(r.duration_ms, 120_000);
        assert!((2_000..=2_001).contains(&r.audible_start_ms));
        assert!(
            (116_990..=117_000).contains(&r.audible_end_ms),
            "{}",
            r.audible_end_ms
        );
        assert_eq!(r.head.start_ms, 0);
        assert_eq!(r.tail.start_ms, 60_000);
        assert_eq!(r.head.lufs.len(), 600);
        assert!(r.integrated_lufs.unwrap() < -5.0);
        assert!(super::super::record::is_usable(&r, &source));
        // Round-trips as JSON.
        let json = serde_json::to_string(&r).unwrap();
        assert!(json.contains("\"audibleStartMs\""));
        assert_eq!(serde_json::from_str::<AnalysisRecord>(&json).unwrap(), r);
    }
}
