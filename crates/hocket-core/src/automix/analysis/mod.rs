//! Full AutoMix analysis (feature `automix-analysis`): decode a file (or take
//! PCM), track beats and downbeats with Beat This! and measure loudness, and
//! build an [`AnalysisRecord`].
//!
//! - [`Analyser`] holds the two embedded models (load once, reuse; loading
//!   takes ~50 ms). It is `Send` but not `Sync`: one per worker thread.
//! - [`Analyser::analyse_file`] streams the file: peak memory is the models,
//!   the mel spectrogram (~1.5 MB per minute), and 60 s of head and tail PCM,
//!   whatever the track's length.
//! - [`Analyser::analyse_pcm`] does the same over a buffer already decoded.
//!
//! Loudness (curves, audible edges) is phase 1's [`super::loudness`];
//! integrated loudness is measured over the whole track (loudness-only
//! records approximate it from head and tail). The caller sets the
//! producer, and may replace `source` with the library's fingerprint of the
//! track (the record's own is the file's: decoded duration, size, extension).
//!
//! See `NOTICE` for the beat model's provenance and licences.

mod grid;
mod model;

use std::collections::VecDeque;
use std::path::Path;

pub use grid::{beat_steadiness, bpm, build_grid, confidence, refine_peak, BeatGrid};

use beat_this::{BeatStream, BeatThis};

use crate::api::{AnalysisProducer, AnalysisRecord, EpochMs, Ms, SourceFingerprint};
use crate::audio::decoder::{decode_file, DecodeError};
use crate::audio::dsp::loudness::LoudnessMeter;

use super::loudness::{loudness_record, HeadTail, HEAD_TAIL_MS};
use super::record::compact;
use model::EmbeddedModel;

/// `AnalysisRecord::analyser` of full records.
pub const ANALYSER: &str = "beat-this-small/1.1.0";

#[derive(Debug, thiserror::Error)]
pub enum AnalysisError {
    #[error(transparent)]
    Decode(#[from] DecodeError),
    #[error("beat model: {0}")]
    Model(String),
    #[error("no audio")]
    NoAudio,
}

fn model_err(e: anyhow::Error) -> AnalysisError {
    AnalysisError::Model(format!("{e:#}"))
}

/// The beat tracker, ready to analyse tracks one after another.
pub struct Analyser {
    bt: BeatThis<EmbeddedModel>,
}

impl Analyser {
    /// Load the embedded models. `threads` is the inference thread count;
    /// 0 shares rten's default pool (one per physical core). Background
    /// analysis should pass 1.
    pub fn new(threads: usize) -> Result<Self, AnalysisError> {
        let (mel, beat) = EmbeddedModel::load_pair(threads).map_err(model_err)?;
        Ok(Self {
            bt: BeatThis::from_models(mel, beat),
        })
    }

    /// Decode and analyse the file at `path`.
    pub fn analyse_file(
        &mut self,
        path: &Path,
        produced_by: AnalysisProducer,
    ) -> Result<AnalysisRecord, AnalysisError> {
        Ok(self.analyse_file_raw(path, produced_by)?.0)
    }

    /// [`Self::analyse_file`], also returning the tracker's refined output
    /// (so a tool can rebuild the grid with [`apply_grid`] without the model).
    pub fn analyse_file_raw(
        &mut self,
        path: &Path,
        produced_by: AnalysisProducer,
    ) -> Result<(AnalysisRecord, RawBeats), AnalysisError> {
        let mut acc: Option<Accumulator<'_>> = None;
        let mut failed: Option<AnalysisError> = None;
        let bt = &mut self.bt;
        let mut bt = Some(bt);
        decode_file(path, |spec, pcm| {
            if failed.is_some() {
                return;
            }
            if acc.is_none() {
                match Accumulator::new(bt.take().expect("once"), spec.channels, spec.sample_rate) {
                    Ok(a) => acc = Some(a),
                    Err(e) => {
                        failed = Some(e);
                        return;
                    }
                }
            }
            if let Err(e) = acc.as_mut().expect("set").push(pcm) {
                failed = Some(e);
            }
        })?;
        if let Some(e) = failed {
            return Err(e);
        }
        let source = SourceFingerprint {
            duration_ms: 0,
            size_bytes: std::fs::metadata(path).ok().map(|m| m.len() as f64),
            suffix: path
                .extension()
                .and_then(|e| e.to_str())
                .map(str::to_ascii_lowercase),
            transcoded: false,
        };
        acc.ok_or(AnalysisError::NoAudio)?
            .finish(source, produced_by, crate::util::now_ms())
    }

    /// Analyse interleaved PCM of a whole track. `source.duration_ms` is
    /// replaced by the buffer's length.
    pub fn analyse_pcm(
        &mut self,
        pcm: &[f32],
        channels: usize,
        rate: u32,
        source: SourceFingerprint,
        produced_by: AnalysisProducer,
        created_at: EpochMs,
    ) -> Result<AnalysisRecord, AnalysisError> {
        let mut acc = Accumulator::new(&mut self.bt, channels, rate)?;
        for chunk in pcm.chunks(channels.max(1) * 8192) {
            acc.push(chunk)?;
        }
        Ok(acc.finish(source, produced_by, created_at)?.0)
    }
}

/// The beat tracker's output, peaks refined to sub-frame times (seconds).
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct RawBeats {
    pub beats_s: Vec<f64>,
    pub downbeats_s: Vec<f64>,
}

/// Set `record`'s grid, tempo and confidence from the tracker's output (and
/// mark it a full record).
pub fn apply_grid(record: &mut AnalysisRecord, raw: &RawBeats) {
    let grid = build_grid(&raw.beats_s, &raw.downbeats_s);
    record.bpm = bpm(&grid.beats_ms);
    record.beat_confidence = confidence(&grid, (record.audible_start_ms, record.audible_end_ms));
    record.beats_ms = grid.beats_ms;
    record.downbeats_ms = grid.downbeats_ms;
    record.analyser = ANALYSER.into();
    *record = compact(std::mem::take(record));
}

/// Analyse one file with a fresh single-threaded [`Analyser`].
pub fn analyse_file(
    path: &Path,
    produced_by: AnalysisProducer,
) -> Result<AnalysisRecord, AnalysisError> {
    Analyser::new(1)?.analyse_file(path, produced_by)
}

/// Streaming state: the beat stream, a whole-track loudness meter, and the
/// first and last [`HEAD_TAIL_MS`] of PCM for the curves and silence edges.
struct Accumulator<'a> {
    beats: BeatStream<'a, EmbeddedModel>,
    meter: LoudnessMeter,
    channels: usize,
    rate: u32,
    head: Vec<f32>,
    tail: VecDeque<f32>,
    edge_samples: usize,
    frames: u64,
    mono: Vec<f32>,
}

impl<'a> Accumulator<'a> {
    fn new(
        bt: &'a mut BeatThis<EmbeddedModel>,
        channels: usize,
        rate: u32,
    ) -> Result<Self, AnalysisError> {
        if rate == 0 || channels == 0 {
            return Err(AnalysisError::NoAudio);
        }
        let edge_samples = (u64::from(HEAD_TAIL_MS) * u64::from(rate) / 1000) as usize * channels;
        Ok(Self {
            beats: bt.stream(rate).map_err(model_err)?,
            meter: LoudnessMeter::new(f64::from(rate), channels),
            channels,
            rate,
            head: Vec::with_capacity(edge_samples),
            tail: VecDeque::with_capacity(edge_samples),
            edge_samples,
            frames: 0,
            mono: Vec::new(),
        })
    }

    fn push(&mut self, pcm: &[f32]) -> Result<(), AnalysisError> {
        let ch = self.channels;
        let pcm = &pcm[..pcm.len() - pcm.len() % ch];
        self.frames += (pcm.len() / ch) as u64;
        self.meter.push(pcm);
        let room = self.edge_samples - self.head.len();
        self.head.extend_from_slice(&pcm[..room.min(pcm.len())]);
        let keep = pcm.len().min(self.edge_samples);
        let drop = (self.tail.len() + keep).saturating_sub(self.edge_samples);
        self.tail.drain(..drop);
        self.tail.extend(&pcm[pcm.len() - keep..]);
        self.mono.clear();
        let scale = 1.0 / ch as f32;
        self.mono
            .extend(pcm.chunks_exact(ch).map(|f| f.iter().sum::<f32>() * scale));
        self.beats.push(&self.mono).map_err(model_err)
    }

    fn finish(
        self,
        mut source: SourceFingerprint,
        produced_by: AnalysisProducer,
        created_at: EpochMs,
    ) -> Result<(AnalysisRecord, RawBeats), AnalysisError> {
        if self.frames == 0 {
            return Err(AnalysisError::NoAudio);
        }
        let duration_ms = (self.frames as f64 * 1000.0 / f64::from(self.rate)).round() as Ms;
        source.duration_ms = duration_ms;
        let tail: Vec<f32> = self.tail.into_iter().collect();
        let tail_frames = (tail.len() / self.channels) as f64;
        let tail_start_ms =
            ((self.frames as f64 - tail_frames) * 1000.0 / f64::from(self.rate)).round() as Ms;
        let mut record = loudness_record(
            HeadTail {
                head: &self.head,
                tail: &tail,
                tail_start_ms,
                channels: self.channels,
                rate: self.rate,
            },
            source,
            produced_by,
            created_at,
        );
        record.integrated_lufs = self.meter.integrated_lufs().map(|v| v as f32);

        let beats = self.beats.finish().map_err(model_err)?;
        let refined = |t: &[f32], logits: &[f32]| -> Vec<f64> {
            t.iter().map(|&x| refine_peak(x, logits)).collect()
        };
        let raw = RawBeats {
            beats_s: refined(&beats.beats, &beats.beat_logits),
            downbeats_s: refined(&beats.downbeats, &beats.downbeat_logits),
        };
        apply_grid(&mut record, &raw);
        Ok((record, raw))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 30 s of a click track at 120 BPM with an accented downbeat every 4
    /// beats, stereo 44.1 kHz.
    fn clicks(seconds: f64) -> Vec<f32> {
        let rate = 44_100.0;
        let n = (seconds * rate) as usize;
        let mut v = vec![0.0f32; n * 2];
        let beat = (0.5 * rate) as usize;
        for (k, start) in (0..n).step_by(beat).enumerate() {
            let amp = if k % 4 == 0 { 0.9 } else { 0.4 };
            let f = if k % 4 == 0 { 1_000.0 } else { 2_000.0 };
            for i in 0..(0.05 * rate) as usize {
                if start + i >= n {
                    break;
                }
                let env = (-(i as f64) / (0.01 * rate)).exp();
                let s =
                    (amp * env * (2.0 * std::f64::consts::PI * f * i as f64 / rate).sin()) as f32;
                v[(start + i) * 2] = s;
                v[(start + i) * 2 + 1] = s;
            }
        }
        v
    }

    #[test]
    fn analyses_a_click_track() {
        let pcm = clicks(30.0);
        let mut a = Analyser::new(1).unwrap();
        let r = a
            .analyse_pcm(
                &pcm,
                2,
                44_100,
                SourceFingerprint::default(),
                AnalysisProducer::Desktop,
                1.0,
            )
            .unwrap();
        assert_eq!(r.duration_ms, 30_000);
        assert_eq!(r.source.duration_ms, 30_000);
        assert_eq!(r.analyser, ANALYSER);
        assert!(r.beats_ms.len() >= 50, "{} beats", r.beats_ms.len());
        let bpm = r.bpm.unwrap();
        assert!((bpm - 120.0).abs() < 2.0, "{bpm}");
        assert!(!r.downbeats_ms.is_empty());
        assert!(r.integrated_lufs.is_some());
        assert!(r.audible_start_ms < 50);
        assert_eq!(r.head.lufs.len(), 300);
    }

    #[test]
    fn silence_is_an_error_free_empty_grid() {
        let pcm = vec![0.0f32; 44_100 * 2 * 5];
        let mut a = Analyser::new(1).unwrap();
        let r = a
            .analyse_pcm(
                &pcm,
                2,
                44_100,
                SourceFingerprint::default(),
                AnalysisProducer::Desktop,
                1.0,
            )
            .unwrap();
        assert!(r.beats_ms.is_empty());
        assert_eq!(r.beat_confidence, 0.0);
        assert_eq!(r.bpm, None);
    }
}
