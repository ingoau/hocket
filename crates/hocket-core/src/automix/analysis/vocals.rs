//! Singing-voice detection with musicnn (Pons & Serra, MTG-UPF; the
//! MagnaTagATune model): where in a track's head and tail someone sings or
//! raps.
//!
//! The embedded ONNX graph is the network only (log-mel patches in, the 50
//! MagnaTagATune tag probabilities out); the front end is here, a port of
//! the reference `extractor.py` (librosa defaults): 16 kHz mono, 512-point
//! periodic-Hann frames every 256 samples, centred with zero padding, power
//! spectrum, 96 Slaney mel bands over 0–8 kHz, `log10(10000·mel + 1)`, 187-
//! frame (3 s) patches. The front end matches librosa to float rounding
//! (`parity_with_reference`), so the tags are the published model's.
//!
//! A patch's voice probability weighs the vocal tags against the no-vocal
//! ones ([`voice_probability`]). Patches don't overlap, and only the first
//! and last [`VOCAL_REGION_MS`] of a track are analysed: the network costs
//! ~45 ms a patch on one core, ~1.1 s a record. See `NOTICE` for provenance
//! and licences.

use std::sync::Arc;

use anyhow::{anyhow, Result};
use realfft::{RealFftPlanner, RealToComplex};
use rten::{Model as Graph, NodeId, RunOptions, ThreadPool, ValueView};
use rten_tensor::Layout;

use crate::api::{Ms, VocalCurve};
use crate::audio::resample::convert;

/// musicnn's MTT_musicnn network (~3 MB), converted from its checkpoint (see `NOTICE`).
static MODEL: &[u8] = include_bytes!("models/musicnn_mtt.onnx");

const SAMPLE_RATE: u32 = 16_000;
const FFT: usize = 512;
const HOP: usize = 256;
const BINS: usize = FFT / 2 + 1;
const MEL_BANDS: usize = 96;
/// Frames in a patch (3 s).
pub const PATCH_FRAMES: usize = 187;
/// Frames between patch starts: patches don't overlap.
pub const PATCH_HOP_FRAMES: usize = PATCH_FRAMES;
/// A patch's length (187 × 256 samples at 16 kHz).
pub const PATCH_MS: Ms = 2_992;
/// How much of each end of a track is analysed: 12 patches. Blends are at
/// most 32 s, so B's first and A's last 36 s hold every overlap the planner
/// makes.
pub const VOCAL_REGION_MS: Ms = 12 * PATCH_MS;
/// Point spacing of a [`VocalCurve`].
pub const VOCAL_HOP_MS: Ms = 100;
/// Patches per inference call.
const BATCH: usize = 16;
/// Tags scored.
pub const TAGS: usize = 50;

/// MagnaTagATune tags that mean a voice (indices into the model's output):
/// vocal, female, opera, male, singing, vocals, woman, male vocal, man,
/// choir, voice, male voice, female vocal, female voice, choral.
pub const VOICE_TAGS: &[usize] = &[13, 15, 17, 18, 19, 20, 26, 27, 33, 35, 36, 39, 40, 48, 49];
/// The tags that mean no voice: no vocals, no vocal, no voice.
pub const NO_VOICE_TAGS: &[usize] = &[21, 28, 44];

/// Voice probability of one patch's tags: the strongest vocal tag against
/// the strongest no-vocal tag, `v / (v + n)`.
pub fn voice_probability(tags: &[f32]) -> f32 {
    let strongest = |idx: &[usize]| idx.iter().map(|&i| tags[i]).fold(0.0f32, f32::max);
    let v = strongest(VOICE_TAGS);
    let n = strongest(NO_VOICE_TAGS);
    if v + n <= 0.0 {
        0.0
    } else {
        (v / (v + n)).clamp(0.0, 1.0)
    }
}

/// The detector: the network and the front end's FFT, window and mel matrix.
pub struct VoiceDetector {
    graph: Graph,
    input: NodeId,
    output: NodeId,
    pool: Option<Arc<ThreadPool>>,
    fft: Arc<dyn RealToComplex<f32>>,
    window: Vec<f32>,
    /// One row of `BINS` weights per band.
    mel: Vec<[f32; BINS]>,
}

/// Tag probabilities of consecutive patches of one buffer.
#[derive(Debug, Clone, Default)]
pub struct PatchTags {
    /// Centre of each patch, ms from the buffer's start.
    pub centres_ms: Vec<f64>,
    /// `TAGS` probabilities per patch.
    pub tags: Vec<f32>,
}

impl PatchTags {
    pub fn len(&self) -> usize {
        self.centres_ms.len()
    }

    pub fn is_empty(&self) -> bool {
        self.centres_ms.is_empty()
    }

    pub fn patch(&self, i: usize) -> &[f32] {
        &self.tags[i * TAGS..(i + 1) * TAGS]
    }

    /// [`voice_probability`] of each patch.
    pub fn voice(&self) -> Vec<f32> {
        (0..self.len())
            .map(|i| voice_probability(self.patch(i)))
            .collect()
    }
}

impl VoiceDetector {
    /// Load the embedded network; `pool` as for the beat models.
    pub fn new(pool: Option<Arc<ThreadPool>>) -> Result<Self> {
        let graph = Graph::load_static_slice(MODEL)?;
        let input = *graph
            .input_ids()
            .first()
            .ok_or_else(|| anyhow!("musicnn: no input"))?;
        let output = *graph
            .output_ids()
            .first()
            .ok_or_else(|| anyhow!("musicnn: no output"))?;
        let fft = RealFftPlanner::<f32>::new().plan_fft_forward(FFT);
        // Periodic Hann (scipy's get_window with fftbins, as librosa).
        let window = (0..FFT)
            .map(|n| {
                (0.5 - 0.5 * (2.0 * std::f64::consts::PI * n as f64 / FFT as f64).cos()) as f32
            })
            .collect();
        Ok(Self {
            graph,
            input,
            output,
            pool,
            fft,
            window,
            mel: mel_matrix(),
        })
    }

    /// Log-mel frames (`MEL_BANDS` each) of 16 kHz mono: centred frames
    /// (zero padding of half a frame at each end, as librosa 0.10).
    pub fn log_mel(&self, mono16k: &[f32]) -> Vec<f32> {
        let pad = FFT / 2;
        let mut wave = vec![0.0f32; mono16k.len() + 2 * pad];
        wave[pad..pad + mono16k.len()].copy_from_slice(mono16k);
        let frames = 1 + (wave.len() - FFT) / HOP;
        let mut out = Vec::with_capacity(frames * MEL_BANDS);
        let mut buf = self.fft.make_input_vec();
        let mut spec = self.fft.make_output_vec();
        let mut power = vec![0.0f32; BINS];
        for f in 0..frames {
            let frame = &wave[f * HOP..f * HOP + FFT];
            for ((b, &x), &w) in buf.iter_mut().zip(frame).zip(&self.window) {
                *b = x * w;
            }
            self.fft
                .process(&mut buf, &mut spec)
                .expect("fft lengths are fixed");
            for (p, c) in power.iter_mut().zip(&spec) {
                *p = c.norm_sqr();
            }
            for band in &self.mel {
                let acc: f32 = band.iter().zip(&power).map(|(w, p)| w * p).sum();
                out.push((10_000.0 * acc + 1.0).log10());
            }
        }
        out
    }

    /// Tag probabilities of every patch of `mono16k`, zero-padded at the end
    /// to whole patches (at least one; a last patch less than half real is
    /// dropped).
    pub fn tags_16k(&mut self, mono16k: &[f32]) -> Result<PatchTags> {
        let mut mel = self.log_mel(mono16k);
        let frames = mel.len() / MEL_BANDS;
        let mut patches = if frames <= PATCH_FRAMES {
            1
        } else {
            1 + (frames - PATCH_FRAMES).div_ceil(PATCH_HOP_FRAMES)
        };
        if patches > 1 && frames - (patches - 1) * PATCH_HOP_FRAMES < PATCH_FRAMES / 2 {
            patches -= 1;
        }
        let need = (patches - 1) * PATCH_HOP_FRAMES + PATCH_FRAMES;
        // log10(1) = 0: padding frames are silence.
        mel.resize(need * MEL_BANDS, 0.0);
        let frame_ms = 1000.0 * HOP as f64 / SAMPLE_RATE as f64;
        let mut out = PatchTags::default();
        let mut batch = Vec::with_capacity(BATCH * PATCH_FRAMES * MEL_BANDS);
        let mut start = 0;
        while start < patches {
            let n = BATCH.min(patches - start);
            batch.clear();
            for p in start..start + n {
                let f = p * PATCH_HOP_FRAMES;
                batch.extend_from_slice(&mel[f * MEL_BANDS..(f + PATCH_FRAMES) * MEL_BANDS]);
                // Frame f is centred on sample f·HOP.
                out.centres_ms
                    .push((f as f64 + (PATCH_FRAMES - 1) as f64 / 2.0) * frame_ms);
            }
            let view = ValueView::from_shape(&[n, PATCH_FRAMES, MEL_BANDS][..], batch.as_slice())
                .map_err(|e| anyhow!("musicnn input: {e}"))?;
            let opts = self
                .pool
                .as_ref()
                .map(|p| RunOptions::default().with_thread_pool(Some(p.clone())));
            let mut values =
                self.graph
                    .run(vec![(self.input, view.into())], &[self.output], opts)?;
            let t = values
                .remove(0)
                .into_tensor::<f32>()
                .ok_or_else(|| anyhow!("musicnn: output is not f32"))?;
            if t.shape() != [n, TAGS] {
                return Err(anyhow!("musicnn: output shape {:?}", t.shape()));
            }
            out.tags.extend_from_slice(&t.into_data());
            start += n;
        }
        Ok(out)
    }

    /// Tag probabilities of interleaved PCM at `rate`.
    pub fn tags(&mut self, pcm: &[f32], channels: usize, rate: u32) -> Result<PatchTags> {
        let mono16k =
            convert(pcm, channels, rate, 1, SAMPLE_RATE).map_err(|e| anyhow!("resample: {e}"))?;
        self.tags_16k(&mono16k)
    }

    /// The [`VocalCurve`] of interleaved PCM at `rate` that starts at track
    /// position `start_ms`: each patch's probability held over its span.
    pub fn curve(
        &mut self,
        pcm: &[f32],
        channels: usize,
        rate: u32,
        start_ms: Ms,
    ) -> Result<VocalCurve> {
        let channels = channels.max(1);
        let frames = pcm.len() / channels;
        if frames == 0 || rate == 0 {
            return Ok(VocalCurve::default());
        }
        let len_ms = (frames as u64 * 1000 / u64::from(rate)) as Ms;
        let voice = self.tags(pcm, channels, rate)?.voice();
        let points = len_ms.div_ceil(VOCAL_HOP_MS) as usize;
        Ok(VocalCurve {
            start_ms,
            hop_ms: VOCAL_HOP_MS,
            voice: (0..points)
                .map(|i| {
                    let p = (i as Ms * VOCAL_HOP_MS / PATCH_MS) as usize;
                    voice[p.min(voice.len() - 1)]
                })
                .collect(),
        })
    }

    /// The [`VocalCurve`]s of a track's first and last [`VOCAL_REGION_MS`]
    /// from its head and tail PCM (as kept for the loudness curves:
    /// `tail` ends at the track's end, `duration_ms`).
    pub fn head_tail(
        &mut self,
        head: &[f32],
        tail: &[f32],
        channels: usize,
        rate: u32,
        duration_ms: Ms,
    ) -> Result<(VocalCurve, VocalCurve)> {
        let channels = channels.max(1);
        let region = (u64::from(VOCAL_REGION_MS) * u64::from(rate) / 1000) as usize * channels;
        let head = &head[..region.min(head.len() - head.len() % channels)];
        let tail = &tail[tail.len() - region.min(tail.len() - tail.len() % channels)..];
        let rate64 = u64::from(rate.max(1));
        let tail_ms = ((tail.len() / channels) as u64 * 1000 + rate64 / 2) / rate64;
        let tail_start = duration_ms.saturating_sub(tail_ms as Ms);
        Ok((
            self.curve(head, channels, rate, 0)?,
            self.curve(tail, channels, rate, tail_start)?,
        ))
    }
}

/// Slaney's mel scale (librosa's default, `htk=False`).
fn hz_to_mel(hz: f64) -> f64 {
    const F_SP: f64 = 200.0 / 3.0;
    const MIN_LOG_HZ: f64 = 1_000.0;
    let min_log_mel = MIN_LOG_HZ / F_SP;
    let logstep = 6.4f64.ln() / 27.0;
    if hz >= MIN_LOG_HZ {
        min_log_mel + (hz / MIN_LOG_HZ).ln() / logstep
    } else {
        hz / F_SP
    }
}

fn mel_to_hz(mel: f64) -> f64 {
    const F_SP: f64 = 200.0 / 3.0;
    const MIN_LOG_HZ: f64 = 1_000.0;
    let min_log_mel = MIN_LOG_HZ / F_SP;
    let logstep = 6.4f64.ln() / 27.0;
    if mel >= min_log_mel {
        MIN_LOG_HZ * (logstep * (mel - min_log_mel)).exp()
    } else {
        F_SP * mel
    }
}

/// `librosa.filters.mel(sr=16000, n_fft=512, n_mels=96)`: Slaney-normalised
/// triangles over 0–8 kHz, one row per band.
fn mel_matrix() -> Vec<[f32; BINS]> {
    let nyquist = SAMPLE_RATE as f64 / 2.0;
    let (lo, hi) = (hz_to_mel(0.0), hz_to_mel(nyquist));
    let edges: Vec<f64> = (0..MEL_BANDS + 2)
        .map(|i| mel_to_hz(lo + (hi - lo) * i as f64 / (MEL_BANDS + 1) as f64))
        .collect();
    let mut m = vec![[0.0f32; BINS]; MEL_BANDS];
    for band in 0..MEL_BANDS {
        let (l, c, u) = (edges[band], edges[band + 1], edges[band + 2]);
        let enorm = 2.0 / (u - l);
        for (bin, out) in m[band].iter_mut().enumerate() {
            let f = nyquist * bin as f64 / (BINS - 1) as f64;
            let w = ((f - l) / (c - l)).min((u - f) / (u - c)).max(0.0);
            *out = (w * enorm) as f32;
        }
    }
    m
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(seconds: f64, hz: f64) -> Vec<f32> {
        (0..(seconds * SAMPLE_RATE as f64) as usize)
            .map(|i| {
                (0.3 * (2.0 * std::f64::consts::PI * hz * i as f64 / SAMPLE_RATE as f64).sin())
                    as f32
            })
            .collect()
    }

    #[test]
    fn patches_cover_the_buffer() {
        let mut d = VoiceDetector::new(None).unwrap();
        // Shorter than a patch: one patch.
        let t = d.tags_16k(&tone(1.0, 440.0)).unwrap();
        assert_eq!(t.len(), 1);
        assert_eq!(t.tags.len(), TAGS);
        // 11 s = 688 frames: 3 whole patches and a padded one 127 frames real.
        let t = d.tags_16k(&tone(11.0, 440.0)).unwrap();
        assert_eq!(t.len(), 4);
        assert!((t.centres_ms[1] - t.centres_ms[0] - 2_992.0).abs() < 1e-9);
        assert!((t.centres_ms[0] - 1_488.0).abs() < 1e-9);
        // 10 s = 626 frames: the fourth patch would be 65 frames real, dropped.
        assert_eq!(d.tags_16k(&tone(10.0, 440.0)).unwrap().len(), 3);
        assert!(t
            .tags
            .iter()
            .all(|v| v.is_finite() && (0.0..=1.0).contains(v)));
    }

    #[test]
    fn voice_probability_weighs_vocal_against_no_vocal() {
        let mut tags = vec![0.0f32; TAGS];
        assert_eq!(voice_probability(&tags), 0.0);
        tags[13] = 0.6; // vocal
        tags[21] = 0.2; // no vocals
        assert!((voice_probability(&tags) - 0.75).abs() < 1e-6);
    }

    #[test]
    fn curves_cover_each_end() {
        let mut d = VoiceDetector::new(None).unwrap();
        // 50 s of stereo 44.1 kHz: both regions are 35.9 s, the tail's ending at 50 s.
        let mono: Vec<f32> = (0..50 * 44_100)
            .map(|i| 0.3 * (i as f32 * 0.0627).sin())
            .collect();
        let pcm: Vec<f32> = mono.iter().flat_map(|&s| [s, s]).collect();
        let (h, t) = d.head_tail(&pcm, &pcm, 2, 44_100, 50_000).unwrap();
        assert_eq!(h.start_ms, 0);
        assert_eq!(h.hop_ms, VOCAL_HOP_MS);
        assert_eq!(h.voice.len(), 360);
        assert_eq!(t.start_ms, 50_000 - 35_904);
        assert_eq!(t.voice.len(), 360);
        // Steps: constant within a patch.
        assert_eq!(h.voice[0], h.voice[29]);
        // Shorter than a region: the whole buffer.
        let short = &pcm[..44_100 * 2 * 5];
        let (h, t) = d.head_tail(short, short, 2, 44_100, 5_000).unwrap();
        assert_eq!((h.voice.len(), t.start_ms, t.voice.len()), (50, 0, 50));
    }

    fn read_npy(path: &std::path::Path) -> (Vec<usize>, Vec<f32>) {
        let bytes = std::fs::read(path).unwrap();
        let header_len = u16::from_le_bytes([bytes[8], bytes[9]]) as usize;
        let header = std::str::from_utf8(&bytes[10..10 + header_len]).unwrap();
        assert!(header.contains("'<f4'"), "{header}");
        let shape = header
            .split("'shape': (")
            .nth(1)
            .unwrap()
            .split(')')
            .next()
            .unwrap()
            .split(',')
            .filter_map(|s| s.trim().parse().ok())
            .collect::<Vec<usize>>();
        let data: Vec<f32> = bytes[10 + header_len..]
            .as_chunks::<4>()
            .0
            .iter()
            .map(|c| f32::from_le_bytes(*c))
            .collect();
        if header.contains("'fortran_order': True") && shape.len() == 2 {
            let (r, c) = (shape[0], shape[1]);
            let t = (0..r * c).map(|i| data[(i % c) * r + i / c]).collect();
            return (shape, t);
        }
        (shape, data)
    }

    fn max_diff(a: &[f32], b: &[f32]) -> f32 {
        a.iter()
            .zip(b)
            .map(|(x, y)| (x - y).abs())
            .fold(0.0, f32::max)
    }

    /// Mel basis, front end and network against the reference (librosa
    /// 0.10 and TensorFlow; `ref_*.npy` written by the conversion script,
    /// see `NOTICE`). Set `HOCKET_MUSICNN_REF` to their directory.
    #[test]
    #[ignore = "needs HOCKET_MUSICNN_REF"]
    fn parity_with_reference() {
        let dir = std::path::PathBuf::from(std::env::var("HOCKET_MUSICNN_REF").unwrap());
        let (_, basis) = read_npy(&dir.join("ref_melbasis.npy"));
        let (_, wave) = read_npy(&dir.join("ref_wave.npy"));
        let (lshape, logmel) = read_npy(&dir.join("ref_logmel.npy"));
        let (tshape, tags) = read_npy(&dir.join("ref_tags.npy"));
        let d = VoiceDetector::new(None).unwrap();
        let diff = max_diff(d.mel.as_flattened(), &basis);
        assert!(diff < 1e-6, "mel basis differs by {diff}");
        let mine = d.log_mel(&wave);
        assert_eq!(mine.len(), lshape[0] * MEL_BANDS);
        let diff = max_diff(&mine, &logmel);
        assert!(diff < 1e-3, "log-mel differs by {diff}");
        // The reference's patches hop 62 frames from the same start: run those.
        for p in 0..tshape[0] {
            let f = p * 62;
            let slice = &logmel[f * MEL_BANDS..(f + PATCH_FRAMES) * MEL_BANDS];
            let view = ValueView::from_shape(&[1, PATCH_FRAMES, MEL_BANDS][..], slice).unwrap();
            let mut v = d
                .graph
                .run(vec![(d.input, view.into())], &[d.output], None)
                .unwrap();
            let out = v.remove(0).into_tensor::<f32>().unwrap().into_data();
            let diff = max_diff(&out, &tags[p * TAGS..(p + 1) * TAGS]);
            assert!(diff < 1e-4, "patch {p} tags differ by {diff}");
        }
    }
}
