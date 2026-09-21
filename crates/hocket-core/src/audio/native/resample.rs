//! Sample-rate conversion (rubato windowed-sinc) between a source's rate
//! and the output stream's rate, plus channel-count adaptation.
//!
//! The converter is fixed-input-size: interleaved frames are accumulated
//! until a chunk is full, converted, and appended to the output as
//! interleaved frames again. [`Resampler::flush`] pads the last partial chunk
//! so the tail of a track isn't lost at a transition.

use rubato::{Resampler as _, SincFixedIn, SincInterpolationParameters, SincInterpolationType, WindowFunction};

/// Input chunk in frames.
pub const CHUNK_FRAMES: usize = 1024;

#[derive(Debug, Clone, thiserror::Error)]
#[error("resampler: {0}")]
pub struct ResampleError(String);

pub struct Resampler {
    inner: SincFixedIn<f32>,
    channels: usize,
    in_rate: u32,
    out_rate: u32,
    pending: Vec<Vec<f32>>,
    pending_frames: usize,
    out_planar: Vec<Vec<f32>>,
    /// Frames of input accepted but not yet produced as output (latency).
    latency_in_frames: usize,
}

impl std::fmt::Debug for Resampler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Resampler").field("in_rate", &self.in_rate).field("out_rate", &self.out_rate).field("channels", &self.channels).finish()
    }
}

impl Resampler {
    pub fn new(in_rate: u32, out_rate: u32, channels: usize) -> Result<Self, ResampleError> {
        let channels = channels.max(1);
        let params = SincInterpolationParameters {
            sinc_len: 128,
            f_cutoff: 0.95,
            oversampling_factor: 128,
            interpolation: SincInterpolationType::Linear,
            window: WindowFunction::BlackmanHarris2,
        };
        let ratio = f64::from(out_rate) / f64::from(in_rate);
        let inner = SincFixedIn::<f32>::new(ratio, 1.0, params, CHUNK_FRAMES, channels).map_err(|e| ResampleError(e.to_string()))?;
        let out_planar = inner.output_buffer_allocate(true);
        Ok(Self {
            inner,
            channels,
            in_rate,
            out_rate,
            pending: vec![Vec::with_capacity(CHUNK_FRAMES); channels],
            pending_frames: 0,
            out_planar,
            latency_in_frames: 0,
        })
    }

    pub fn ratio(&self) -> f64 {
        f64::from(self.out_rate) / f64::from(self.in_rate)
    }

    /// Input frames buffered inside (pending chunk plus filter delay), for
    /// position accounting.
    pub fn latency_in_frames(&self) -> usize {
        self.pending_frames + self.latency_in_frames
    }

    pub fn reset(&mut self) {
        self.inner.reset();
        for p in &mut self.pending {
            p.clear();
        }
        self.pending_frames = 0;
        self.latency_in_frames = 0;
    }

    /// Feed interleaved frames; converted interleaved frames are appended to
    /// `out`.
    pub fn process(&mut self, interleaved: &[f32], out: &mut Vec<f32>) -> Result<(), ResampleError> {
        for frame in interleaved.chunks_exact(self.channels) {
            for (ch, s) in frame.iter().enumerate() {
                self.pending[ch].push(*s);
            }
            self.pending_frames += 1;
            if self.pending_frames == CHUNK_FRAMES {
                self.run_chunk(out)?;
            }
        }
        Ok(())
    }

    fn run_chunk(&mut self, out: &mut Vec<f32>) -> Result<(), ResampleError> {
        let (used, produced) =
            self.inner.process_into_buffer(&self.pending, &mut self.out_planar, None).map_err(|e| ResampleError(e.to_string()))?;
        debug_assert_eq!(used, CHUNK_FRAMES);
        // The sinc filter delays output by about half its length.
        self.latency_in_frames = 64;
        self.interleave(produced, out);
        for p in &mut self.pending {
            p.clear();
        }
        self.pending_frames = 0;
        Ok(())
    }

    fn interleave(&self, frames: usize, out: &mut Vec<f32>) {
        out.reserve(frames * self.channels);
        for i in 0..frames {
            for ch in 0..self.channels {
                out.push(self.out_planar[ch][i]);
            }
        }
    }

    /// Convert whatever is pending (zero-padded) and drain the filter.
    pub fn flush(&mut self, out: &mut Vec<f32>) -> Result<(), ResampleError> {
        if self.pending_frames > 0 {
            let (_, produced) = self
                .inner
                .process_partial_into_buffer(Some(&self.pending), &mut self.out_planar, None)
                .map_err(|e| ResampleError(e.to_string()))?;
            self.interleave(produced, out);
            for p in &mut self.pending {
                p.clear();
            }
            self.pending_frames = 0;
        }
        let (_, produced) = self
            .inner
            .process_partial_into_buffer(None::<&[Vec<f32>]>, &mut self.out_planar, None)
            .map_err(|e| ResampleError(e.to_string()))?;
        self.interleave(produced, out);
        self.latency_in_frames = 0;
        Ok(())
    }
}

/// Adapt interleaved `input` with `in_ch` channels to `out_ch` channels,
/// appending to `out`. Mono is duplicated, extra channels are averaged
/// into the first `out_ch` (stereo from 5.1 keeps L/R and folds C/surrounds
/// in at −3 dB), missing channels are zero.
pub fn remap_channels(input: &[f32], in_ch: usize, out_ch: usize, out: &mut Vec<f32>) {
    let in_ch = in_ch.max(1);
    let out_ch = out_ch.max(1);
    if in_ch == out_ch {
        out.extend_from_slice(input);
        return;
    }
    for frame in input.chunks_exact(in_ch) {
        match (in_ch, out_ch) {
            (1, n) => {
                for _ in 0..n {
                    out.push(frame[0]);
                }
            }
            (2, 1) => out.push((frame[0] + frame[1]) * 0.5),
            (i, 2) if i >= 3 => {
                // L R C (LFE) Ls Rs ...: fold centre and surrounds.
                let c = if i >= 3 { frame[2] * std::f32::consts::FRAC_1_SQRT_2 } else { 0.0 };
                let ls = if i >= 5 { frame[i - 2] * std::f32::consts::FRAC_1_SQRT_2 } else { 0.0 };
                let rs = if i >= 6 { frame[i - 1] * std::f32::consts::FRAC_1_SQRT_2 } else { 0.0 };
                out.push(frame[0] + c + ls);
                out.push(frame[1] + c + rs);
            }
            (i, n) if i > n => {
                out.extend_from_slice(&frame[..n]);
            }
            (i, n) => {
                out.extend_from_slice(frame);
                for _ in i..n {
                    out.push(0.0);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::PI;

    fn sine(freq: f64, rate: u32, frames: usize) -> Vec<f32> {
        (0..frames).flat_map(|i| {
            let s = (2.0 * PI * freq * i as f64 / f64::from(rate)).sin() as f32;
            [s, s]
        }).collect()
    }

    #[test]
    fn converts_44k_to_48k_preserving_a_tone() {
        let mut r = Resampler::new(44_100, 48_000, 2).unwrap();
        let input = sine(1000.0, 44_100, 44_100);
        let mut out = Vec::new();
        r.process(&input, &mut out).unwrap();
        r.flush(&mut out).unwrap();
        let frames = out.len() / 2;
        assert!((frames as i64 - 48_000).abs() < 300, "{frames}");
        // Zero crossings of the middle second-half: 1 kHz → ~2000 crossings per second.
        let mid = &out[2 * 10_000..2 * 40_000];
        let crossings = mid.chunks_exact(2).map(|f| f[0]).collect::<Vec<_>>().windows(2).filter(|w| (w[0] < 0.0) != (w[1] < 0.0)).count();
        let seconds = 30_000.0 / 48_000.0;
        let est = crossings as f64 / seconds / 2.0;
        assert!((est - 1000.0).abs() < 15.0, "{est}");
        assert!(out.iter().all(|v| v.abs() <= 1.05));
    }

    #[test]
    fn reset_clears_pending() {
        let mut r = Resampler::new(48_000, 44_100, 1).unwrap();
        r.process(&[0.5; 100], &mut Vec::new()).unwrap();
        assert_eq!(r.latency_in_frames(), 100);
        r.reset();
        assert_eq!(r.latency_in_frames(), 0);
        assert!((r.ratio() - 0.91875).abs() < 1e-9);
    }

    #[test]
    fn channel_remapping() {
        let mut out = Vec::new();
        remap_channels(&[0.1, 0.2], 1, 2, &mut out);
        assert_eq!(out, vec![0.1, 0.1, 0.2, 0.2]);
        out.clear();
        remap_channels(&[0.2, 0.4], 2, 1, &mut out);
        assert!((out[0] - 0.3).abs() < 1e-6);
        out.clear();
        remap_channels(&[1.0, 2.0, 0.0, 9.0, 0.0, 0.0], 6, 2, &mut out);
        assert_eq!(out, vec![1.0, 2.0]);
        out.clear();
        remap_channels(&[1.0, 2.0], 2, 4, &mut out);
        assert_eq!(out, vec![1.0, 2.0, 0.0, 0.0]);
        out.clear();
        remap_channels(&[1.0, 2.0, 3.0], 3, 3, &mut out);
        assert_eq!(out, vec![1.0, 2.0, 3.0]);
        out.clear();
        remap_channels(&[1.0, 2.0, 3.0, 4.0], 4, 3, &mut out);
        assert_eq!(out, vec![1.0, 2.0, 3.0]);
    }
}
