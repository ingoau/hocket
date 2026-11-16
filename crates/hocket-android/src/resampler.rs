//! Streaming sample-rate conversion for the output of `MixProcessor` (`docs/automix.md` →
//! Android → Output rate).
//!
//! ExoPlayer's sink rebuilds its `AudioTrack` whenever the output rate changes, which leaves a gap
//! between tracks at different rates (and in the middle of an AutoMix transition). So the
//! processor keeps the output rate fixed and converts every stream at another rate with this.
//!
//! What this needs that a chunked converter (`hocket_core::audio::resample`) doesn't give:
//!
//! - **Sample-exact positions.** Output frame `j` of a stream is its input at `j × in / out`,
//!   exactly (the rates are reduced to a ratio of integers): no filter delay to trim, and a stream
//!   of `n` input frames is `ceil(n × out / in)` output frames.
//! - **Low latency.** An output frame is produced as soon as the kernel's lookahead (`half` input
//!   frames, ~1.5 ms) has arrived, not when a 1024-frame chunk is full. Whatever is still waiting
//!   at an item boundary barely moves the sink's clock.
//! - **Seams without clicks.** A stream that ends ([`StreamResampler::finish`]) can take frames
//!   past its end as lookahead only, and a stream that starts ([`StreamResampler::prime`]) can
//!   take frames before its start as history only. With the same audio on both sides (an AutoMix
//!   tail continues A), the switch from one stream to the next is as if nothing were cut.
//!
//! The kernel is a Kaiser-windowed sinc, one row of taps per output phase (`out / gcd` of them:
//! 160 for 44.1 ↔ 48 kHz), so there is no interpolation between rows.

use std::f64::consts::PI;

/// Kernel half-width in input frames when upsampling (scaled up when downsampling).
const HALF_BASE: usize = 64;
/// The widest kernel half-width (192 kHz into 44.1 kHz would want 279).
const HALF_MAX: usize = 256;
/// Phases beyond this (unusual rate pairs) are quantised to this many rows.
const PHASES_MAX: u64 = 1024;
/// Cutoff as a fraction of the lower Nyquist frequency.
const CUTOFF: f64 = 0.92;
/// Kaiser β: about 80 dB of stopband.
const BETA: f64 = 8.0;
/// Consumed input is dropped from the front once this many frames have piled up.
const COMPACT_FRAMES: u64 = 4096;

pub struct StreamResampler {
    channels: usize,
    /// Each output frame advances `step / den` input frames.
    step: u64,
    den: u64,
    half: usize,
    phases: u64,
    /// `phases + 1` rows of `2 × half` taps; row `r` is the kernel at offset `r / phases`.
    table: Vec<f32>,
    /// Input frames from absolute frame `base`, interleaved.
    buf: Vec<f32>,
    base: u64,
    /// Input frames of the stream (primed history included): outputs lie before this.
    end: u64,
    finished: bool,
    /// The next output frame's position in input frames, in units of `1 / den`.
    pos: u64,
    acc: Vec<f32>,
}

impl StreamResampler {
    pub fn new(in_rate: u32, out_rate: u32, channels: usize) -> Self {
        let (in_rate, out_rate) = (u64::from(in_rate.max(1)), u64::from(out_rate.max(1)));
        let g = gcd(in_rate, out_rate);
        let (step, den) = (in_rate / g, out_rate / g);
        let down = in_rate as f64 / out_rate as f64;
        let half = ((HALF_BASE as f64 * down.max(1.0)).ceil() as usize).min(HALF_MAX);
        let phases = den.min(PHASES_MAX);
        // Cycles per input frame, times two: the sinc's zero crossings.
        let fc2 = CUTOFF * (1.0 / down).min(1.0);
        let taps = 2 * half;
        let mut table = vec![0f32; (phases as usize + 1) * taps];
        let i0_beta = bessel_i0(BETA);
        for r in 0..=phases as usize {
            let frac = r as f64 / phases as f64;
            let row = &mut table[r * taps..(r + 1) * taps];
            let mut sum = 0.0;
            let mut w = vec![0f64; taps];
            for (k, wk) in w.iter_mut().enumerate() {
                // Tap k reads input frame `ipos - half + 1 + k`, at `t` frames from the output.
                let t = k as f64 - half as f64 + 1.0 - frac;
                let x = t / half as f64;
                let window = if x.abs() >= 1.0 {
                    0.0
                } else {
                    bessel_i0(BETA * (1.0 - x * x).sqrt()) / i0_beta
                };
                *wk = fc2 * sinc(fc2 * t) * window;
                sum += *wk;
            }
            // Unity gain at DC on every phase.
            for (o, wk) in row.iter_mut().zip(&w) {
                *o = (wk / sum) as f32;
            }
        }
        let channels = channels.max(1);
        Self {
            channels,
            step,
            den,
            half,
            phases,
            table,
            buf: Vec::new(),
            base: 0,
            end: 0,
            finished: false,
            pos: 0,
            acc: vec![0.0; channels],
        }
    }

    pub fn channels(&self) -> usize {
        self.channels
    }

    /// History before the stream's start: `frames` only inform its first outputs. Only before any
    /// input; ignored after.
    pub fn prime(&mut self, frames: &[f32]) {
        if self.end != 0 || self.finished {
            return;
        }
        self.append(frames);
        self.end = self.buffered_end();
        self.pos = self.end * self.den;
    }

    /// The stream's next input frames (interleaved).
    pub fn push(&mut self, frames: &[f32]) {
        if self.finished {
            return;
        }
        self.append(frames);
        self.end = self.buffered_end();
    }

    /// The stream ends here: `lookahead` (the audio that would have followed, if any) only
    /// informs the last outputs; past it, silence. [`Self::pull`] then yields the rest.
    pub fn finish(&mut self, lookahead: &[f32]) {
        if self.finished {
            return;
        }
        self.append(lookahead);
        self.finished = true;
    }

    /// Output frames into `out` (interleaved; whole frames only), as many as are ready and fit.
    /// Returns the samples written. Call again while it fills `out`.
    pub fn pull(&mut self, out: &mut [f32]) -> usize {
        let ch = self.channels;
        let taps = 2 * self.half;
        let have = self.buffered_end();
        let mut written = 0;
        while written + ch <= out.len() {
            let ipos = self.pos / self.den;
            let ready = if self.finished {
                ipos < self.end
            } else {
                ipos + (self.half as u64) < self.end
            };
            if !ready {
                break;
            }
            let frac = self.pos % self.den;
            let r = if self.phases == self.den {
                frac
            } else {
                (frac * self.phases + self.den / 2) / self.den
            } as usize;
            let row = &self.table[r * taps..(r + 1) * taps];
            let first = ipos as i64 - self.half as i64 + 1;
            self.acc.iter_mut().for_each(|a| *a = 0.0);
            if first >= self.base as i64 && first as u64 + taps as u64 <= have {
                let from = (first as u64 - self.base) as usize * ch;
                let frames = &self.buf[from..from + taps * ch];
                if ch == 2 {
                    let (mut l, mut rr) = (0f32, 0f32);
                    for (f, w) in frames.as_chunks::<2>().0.iter().zip(row) {
                        l += f[0] * w;
                        rr += f[1] * w;
                    }
                    self.acc[0] = l;
                    self.acc[1] = rr;
                } else {
                    for (f, w) in frames.chunks_exact(ch).zip(row) {
                        for (a, s) in self.acc.iter_mut().zip(f) {
                            *a += s * w;
                        }
                    }
                }
            } else {
                // At the edges: silence before the first frame and past the last.
                for (k, w) in row.iter().enumerate() {
                    let i = first + k as i64;
                    if i < self.base as i64 || i as u64 >= have {
                        continue;
                    }
                    let at = (i as u64 - self.base) as usize * ch;
                    for (c, a) in self.acc.iter_mut().enumerate() {
                        *a += self.buf[at + c] * w;
                    }
                }
            }
            out[written..written + ch].copy_from_slice(&self.acc);
            written += ch;
            self.pos += self.step;
        }
        self.compact();
        written
    }

    fn append(&mut self, frames: &[f32]) {
        let whole = frames.len() / self.channels * self.channels;
        self.buf.extend_from_slice(&frames[..whole]);
    }

    fn buffered_end(&self) -> u64 {
        self.base + (self.buf.len() / self.channels) as u64
    }

    /// Drop input no output will read again (the next one reads from `ipos - half + 1`).
    fn compact(&mut self) {
        let keep_from = (self.pos / self.den).saturating_sub(self.half as u64 - 1);
        if keep_from > self.base + COMPACT_FRAMES {
            let drop = (keep_from - self.base) as usize;
            let drop = drop.min(self.buf.len() / self.channels);
            self.buf.drain(..drop * self.channels);
            self.base += drop as u64;
        }
    }
}

pub(crate) fn gcd(a: u64, b: u64) -> u64 {
    if b == 0 {
        a.max(1)
    } else {
        gcd(b, a % b)
    }
}

fn sinc(x: f64) -> f64 {
    if x.abs() < 1e-12 {
        1.0
    } else {
        (PI * x).sin() / (PI * x)
    }
}

/// The zeroth-order modified Bessel function of the first kind (series; converges fast for β ≤ 20).
fn bessel_i0(x: f64) -> f64 {
    let (mut sum, mut term) = (1.0, 1.0);
    let q = x * x / 4.0;
    for k in 1..64 {
        term *= q / (k * k) as f64;
        sum += term;
        if term < sum * 1e-17 {
            break;
        }
    }
    sum
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(frames: usize, channels: usize, rate: f64, freq: f64, offset: usize) -> Vec<f32> {
        let mut v = Vec::with_capacity(frames * channels);
        for i in 0..frames {
            let s = (2.0 * PI * freq * (i + offset) as f64 / rate).sin() as f32 * 0.5;
            for c in 0..channels {
                v.push(if c == 0 { s } else { -s });
            }
        }
        v
    }

    fn drain(r: &mut StreamResampler, out: &mut Vec<f32>) {
        let mut chunk = vec![0f32; 1000 * r.channels()];
        loop {
            let n = r.pull(&mut chunk);
            out.extend_from_slice(&chunk[..n]);
            if n < chunk.len() {
                break;
            }
        }
    }

    /// The whole of `input`, finished with `lookahead`, in one go.
    fn convert(
        in_rate: u32,
        out_rate: u32,
        ch: usize,
        input: &[f32],
        lookahead: &[f32],
    ) -> Vec<f32> {
        let mut r = StreamResampler::new(in_rate, out_rate, ch);
        r.push(input);
        r.finish(lookahead);
        let mut out = Vec::new();
        drain(&mut r, &mut out);
        out
    }

    fn max_err(a: &[f32], b: &[f32]) -> f32 {
        a.iter().zip(b).fold(0f32, |m, (x, y)| m.max((x - y).abs()))
    }

    #[test]
    fn output_length_is_exact() {
        for (i, o, n) in [
            (44_100, 48_000, 44_100),
            (48_000, 44_100, 12_345),
            (96_000, 48_000, 1_001),
            (22_050, 48_000, 7),
        ] {
            let out = convert(i, o, 2, &vec![0.1; n * 2], &[]);
            let want = (n as u64 * u64::from(o)).div_ceil(u64::from(i)) as usize;
            assert_eq!(out.len(), want * 2, "{i} -> {o}, {n} frames");
        }
    }

    #[test]
    fn a_sine_comes_through_clean_and_in_place() {
        for (i, o) in [
            (44_100u32, 48_000u32),
            (48_000, 44_100),
            (96_000, 44_100),
            (32_000, 48_000),
        ] {
            let input = sine(i as usize, 2, f64::from(i), 1_000.0, 0);
            let out = convert(i, o, 2, &input, &[]);
            // Away from the edges, output frame j is the sine at j / out seconds: no delay.
            let want = sine(out.len() / 2, 2, f64::from(o), 1_000.0, 0);
            let mid = 2 * 1_000..out.len() - 2 * 1_000;
            let err = max_err(&out[mid.clone()], &want[mid]);
            assert!(err < 1e-3, "{i} -> {o}: {err}");
        }
    }

    #[test]
    fn content_above_the_output_band_is_removed() {
        // 23 kHz at 48 kHz would alias to 21.1 kHz at 44.1 kHz.
        let input = sine(48_000, 1, 48_000.0, 23_000.0, 0);
        let out = convert(48_000, 44_100, 1, &input, &[]);
        let peak = out[4_000..out.len() - 4_000]
            .iter()
            .fold(0f32, |m, s| m.max(s.abs()));
        assert!(peak < 0.5 * 1e-3, "{peak}");
    }

    #[test]
    fn chunking_does_not_change_the_output() {
        let input = sine(10_000, 2, 44_100.0, 440.0, 0);
        let whole = convert(44_100, 48_000, 2, &input, &[]);
        let mut r = StreamResampler::new(44_100, 48_000, 2);
        let mut out = Vec::new();
        let mut at = 0;
        for len in [1usize, 7, 300, 2, 1_500, 64, 4_000].iter().cycle() {
            if at >= 10_000 {
                break;
            }
            let end = (at + len).min(10_000);
            r.push(&input[at * 2..end * 2]);
            drain(&mut r, &mut out);
            at = end;
        }
        r.finish(&[]);
        drain(&mut r, &mut out);
        assert_eq!(out, whole);
    }

    #[test]
    fn only_lookahead_is_held_back_until_finish() {
        let mut r = StreamResampler::new(44_100, 48_000, 1);
        r.push(&vec![0.2; 1_000]);
        let mut out = Vec::new();
        drain(&mut r, &mut out);
        let total = (1_000u64 * 48_000).div_ceil(44_100) as usize;
        let held = total - out.len();
        // About `half` input frames' worth (64 × 48/44.1 ≈ 70).
        assert!((60..=80).contains(&held), "{held}");
    }

    #[test]
    fn finish_with_the_real_continuation_matches_an_uncut_stream() {
        let input = sine(6_000, 2, 44_100.0, 3_000.0, 0);
        let uncut = convert(44_100, 48_000, 2, &input, &[]);
        let cut = convert(
            44_100,
            48_000,
            2,
            &input[..4_000 * 2],
            &input[4_000 * 2..4_400 * 2],
        );
        let n = cut.len();
        assert_eq!(n, (4_000u64 * 48_000).div_ceil(44_100) as usize * 2);
        assert!(max_err(&cut, &uncut[..n]) < 1e-6);
        // Without it the last frames are a band-limited cut to silence: well off.
        let bare = convert(44_100, 48_000, 2, &input[..4_000 * 2], &[]);
        assert!(max_err(&bare[n - 20..], &uncut[n - 20..n]) > 0.05);
    }

    #[test]
    fn priming_with_the_audio_before_matches_an_uncut_stream() {
        // The cut at 1470 input frames: an output frame lands exactly on it (× 160 / 147), and the
        // sine isn't at a zero crossing there.
        let input = sine(6_000, 2, 44_100.0, 2_500.0, 0);
        let uncut = convert(44_100, 48_000, 2, &input, &[]);
        let cut_at = 1_470;
        let mut r = StreamResampler::new(44_100, 48_000, 2);
        r.prime(&input[..cut_at * 2]);
        r.push(&input[cut_at * 2..]);
        r.finish(&[]);
        let mut out = Vec::new();
        drain(&mut r, &mut out);
        let skip = cut_at * 48_000 / 44_100 * 2;
        assert_eq!(out.len(), uncut.len() - skip);
        assert!(max_err(&out, &uncut[skip..]) < 1e-6);
        // Unprimed, the first frames see silence before them: audibly off (a click).
        let fresh = convert(44_100, 48_000, 2, &input[cut_at * 2..], &[]);
        let fresh_err = max_err(&fresh[..20], &uncut[skip..skip + 20]);
        assert!(fresh_err > 0.01, "{fresh_err}");
    }

    #[test]
    fn long_streams_keep_memory_bounded() {
        let mut r = StreamResampler::new(48_000, 44_100, 2);
        let block = sine(1_024, 2, 48_000.0, 100.0, 0);
        let mut out = vec![0f32; 4_096];
        for _ in 0..2_000 {
            r.push(&block);
            while r.pull(&mut out) == out.len() {}
        }
        assert!(
            r.buf.len() < (COMPACT_FRAMES as usize + 2_048) * 2,
            "{}",
            r.buf.len()
        );
    }
}
