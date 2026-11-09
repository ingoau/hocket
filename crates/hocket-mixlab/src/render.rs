//! Rendering a planned transition the way a backend does: A plays normally
//! up to `tail_start_ms`, then the rendered tail; B, shaped by
//! `IncomingShaper`, joins at `incoming_at_ms`; the sum goes through the
//! chain's limiter. Output: 30 s before the overlap to 30 s after it.

use std::io::Write;
use std::path::Path;

use anyhow::{bail, Result};

use hocket_core::audio::decoder::decode_file;
use hocket_core::audio::dsp::SoftLimiter;
use hocket_core::automix::render::ms_to_frames;
use hocket_core::automix::{render_tail, IncomingShaper, TransitionPlan};

/// Context either side of the overlap.
const CONTEXT_MS: u32 = 30_000;
/// The rendering is stereo.
const CHANNELS: usize = 2;

struct Pcm {
    samples: Vec<f32>,
    rate: u32,
}

/// Decode to interleaved stereo.
fn decode(path: &Path) -> Result<Pcm> {
    let mut samples = Vec::new();
    let spec = decode_file(path, |spec, pcm| match spec.channels {
        1 => samples.extend(pcm.iter().flat_map(|&s| [s, s])),
        2 => samples.extend_from_slice(pcm),
        n => samples.extend(pcm.chunks_exact(n).flat_map(|f| [f[0], f[1]])),
    })?;
    Ok(Pcm {
        samples,
        rate: spec.sample_rate,
    })
}

/// Linear-interpolation resampling: good enough to audition a transition.
fn resample(p: Pcm, rate: u32) -> Pcm {
    if p.rate == rate {
        return p;
    }
    let frames = p.samples.len() / CHANNELS;
    let out_frames = (frames as f64 * rate as f64 / p.rate as f64) as usize;
    let step = p.rate as f64 / rate as f64;
    let mut out = Vec::with_capacity(out_frames * CHANNELS);
    for i in 0..out_frames {
        let x = i as f64 * step;
        let k = x as usize;
        let f = (x - k as f64) as f32;
        for c in 0..CHANNELS {
            let a = p.samples[k * CHANNELS + c];
            let b = p.samples.get((k + 1) * CHANNELS + c).copied().unwrap_or(a);
            out.push(a + (b - a) * f);
        }
    }
    Pcm { samples: out, rate }
}

fn slice(p: &[f32], from_frame: usize, frames: usize) -> Vec<f32> {
    let lo = (from_frame * CHANNELS).min(p.len());
    let hi = ((from_frame + frames) * CHANNELS).min(p.len());
    let mut v = p[lo..hi].to_vec();
    v.resize(frames * CHANNELS, 0.0);
    v
}

pub struct Rendered {
    pub pcm: Vec<f32>,
    pub rate: u32,
    pub check: Check,
}

/// Numeric sanity of one rendering.
#[derive(Debug, Clone, Default)]
pub struct Check {
    pub peak: f32,
    pub non_finite: usize,
    pub tail_frames: usize,
    pub expected_tail_frames: usize,
    pub frames: usize,
    pub expected_frames: usize,
}

impl Check {
    pub fn peak_dbfs(&self) -> f32 {
        20.0 * self.peak.max(1e-9).log10()
    }

    pub fn ok(&self) -> bool {
        self.non_finite == 0
            && self.peak < 1.0
            && self.tail_frames == self.expected_tail_frames
            && self.frames == self.expected_frames
    }

    pub fn summary(&self) -> String {
        format!(
            "peak {:.2} dBFS, non-finite {}, tail {}/{} frames, total {}/{} frames: {}",
            self.peak_dbfs(),
            self.non_finite,
            self.tail_frames,
            self.expected_tail_frames,
            self.frames,
            self.expected_frames,
            if self.ok() { "ok" } else { "FAIL" }
        )
    }

    pub fn verify(&self) -> Result<()> {
        if !self.ok() {
            bail!("render check failed: {}", self.summary());
        }
        Ok(())
    }
}

#[derive(Debug, Default)]
pub struct CheckTotals {
    n: usize,
    failed: usize,
    max_peak: f32,
    non_finite: usize,
}

impl CheckTotals {
    pub fn add(&mut self, c: &Check) {
        self.n += 1;
        self.failed += usize::from(!c.ok());
        self.max_peak = self.max_peak.max(c.peak);
        self.non_finite += c.non_finite;
    }

    pub fn summary(&self) -> String {
        format!(
            "render checks: {} rendered, {} failed, max peak {:.2} dBFS, non-finite samples {}",
            self.n,
            self.failed,
            20.0 * self.max_peak.max(1e-9).log10(),
            self.non_finite
        )
    }
}

/// Render `plan` from the two files (B resampled to A's rate).
pub fn render_pair(a: &Path, b: &Path, plan: &TransitionPlan) -> Result<Rendered> {
    let pa = decode(a)?;
    let rate = pa.rate;
    let pb = resample(decode(b)?, rate);
    let f = |ms: u32| ms_to_frames(ms as f64, rate);

    // A plain up to the tail start, from 30 s before B comes in.
    let overlap_start = plan.tail_start_ms + plan.incoming_at_ms;
    let pre_from = overlap_start
        .saturating_sub(CONTEXT_MS)
        .min(plan.tail_start_ms);
    let pre_frames = f(plan.tail_start_ms) - f(pre_from);
    let mut out = slice(&pa.samples, f(pre_from), pre_frames);

    // The rendered tail.
    let src = slice(
        &pa.samples,
        f(plan.tail_start_ms),
        f(plan.tail_source_ms) + rate as usize / 10,
    );
    let tail = render_tail(plan, &src, CHANNELS, rate);
    let tail_frames = tail.len() / CHANNELS;
    out.extend_from_slice(&tail);

    // B, from its entry to 30 s after the overlap, shaped and summed.
    let b_frames = f(plan.tail_len_ms.saturating_sub(plan.incoming_at_ms)) + f(CONTEXT_MS);
    let mut bpcm = slice(&pb.samples, f(plan.incoming_from_ms), b_frames);
    let mut shaper = IncomingShaper::new(plan, CHANNELS, rate);
    for block in bpcm.chunks_mut(1024 * CHANNELS) {
        shaper.process(block);
    }
    let at = (pre_frames + f(plan.incoming_at_ms)) * CHANNELS;
    if out.len() < at + bpcm.len() {
        out.resize(at + bpcm.len(), 0.0);
    }
    for (o, s) in out[at..].iter_mut().zip(&bpcm) {
        *o += *s;
    }
    SoftLimiter::default().process(&mut out);

    let check = Check {
        peak: out
            .iter()
            .filter(|s| s.is_finite())
            .fold(0.0f32, |m, s| m.max(s.abs())),
        non_finite: out.iter().filter(|s| !s.is_finite()).count(),
        tail_frames,
        expected_tail_frames: f(plan.tail_len_ms),
        frames: out.len() / CHANNELS,
        expected_frames: pre_frames + f(plan.incoming_at_ms) + b_frames,
    };
    Ok(Rendered {
        pcm: out,
        rate,
        check,
    })
}

/// 32-bit float stereo WAV.
pub fn write_wav(path: &Path, pcm: &[f32], rate: u32) -> Result<()> {
    let data_len = (pcm.len() * 4) as u32;
    let mut w = std::io::BufWriter::new(std::fs::File::create(path)?);
    w.write_all(b"RIFF")?;
    w.write_all(&(36 + data_len).to_le_bytes())?;
    w.write_all(b"WAVEfmt ")?;
    w.write_all(&16u32.to_le_bytes())?;
    w.write_all(&3u16.to_le_bytes())?; // IEEE float
    w.write_all(&(CHANNELS as u16).to_le_bytes())?;
    w.write_all(&rate.to_le_bytes())?;
    w.write_all(&(rate * CHANNELS as u32 * 4).to_le_bytes())?;
    w.write_all(&((CHANNELS * 4) as u16).to_le_bytes())?;
    w.write_all(&32u16.to_le_bytes())?;
    w.write_all(b"data")?;
    w.write_all(&data_len.to_le_bytes())?;
    for s in pcm {
        w.write_all(&s.to_le_bytes())?;
    }
    w.flush()?;
    Ok(())
}
