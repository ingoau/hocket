//! Key estimation from chroma: spectral peaks of the mono mix between C2
//! and C7 folded onto 12 pitch classes (after estimating the track's tuning),
//! correlated with the 24 rotations of a major and a minor key profile.
//!
//! Streaming and cheap: one 16k-point FFT every ~0.19 s of audio at 44.1 or
//! 48 kHz (~1,300 per four-minute track, ~0.25 s of CPU in all, ~1 % of the
//! whole analysis). Memory is a 120-bin (10-cent)
//! histogram per [`BLOCK_MS`] block, ~0.5 KB per block, so the head and tail
//! estimates come from the same pass as the track's.
//!
//! The profiles are Krumhansl & Kessler's probe-tone ratings; see
//! `docs/automix.md` → *Analysis* for how this was checked on the corpus.

use std::sync::Arc;

use realfft::{RealFftPlanner, RealToComplex};

use crate::api::{Ms, MusicalKey};

/// Krumhansl–Kessler major and minor profiles, tonic first.
const MAJOR: [f64; 12] = [
    6.35, 2.23, 3.48, 2.33, 4.38, 4.09, 2.52, 5.19, 2.39, 3.66, 2.29, 2.88,
];
const MINOR: [f64; 12] = [
    6.33, 2.68, 3.52, 5.38, 2.60, 3.53, 2.54, 4.75, 3.98, 2.69, 3.34, 3.17,
];

/// Lowest and highest frequency whose peaks count (C2, C7).
const F_LO: f64 = 65.4;
const F_HI: f64 = 2_093.0;
/// Sub-bins per semitone of the fine histogram.
const FINE: usize = 10;
const FINE_BINS: usize = 12 * FINE;
/// Frames whose RMS is below this are skipped (silence, fades).
const MIN_RMS: f32 = 1e-3;
/// Peaks below this fraction of the frame's strongest are ignored (−50 dB).
const PEAK_FLOOR: f32 = 3e-3;
/// Histogram block length: head and tail estimates are made of whole blocks.
pub const BLOCK_MS: Ms = 2_000;
/// Head and tail estimates cover this much of each end.
pub const KEY_REGION_MS: Ms = 60_000;

/// Estimator settings (the defaults are what analysis uses; `hocket-mixlab
/// keytest` compares others).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct KeyParams {
    /// Each peak also counts towards the pitch classes of the notes it could
    /// be the 2nd..`harmonics`-th harmonic of, weighted `decay^(h−1)`: a
    /// note's third harmonic otherwise votes for its fifth, pulling the
    /// estimate a fifth up.
    pub harmonics: u32,
    pub decay: f32,
}

impl Default for KeyParams {
    /// Chosen on the corpus against Essentia's KeyExtractor (`docs/automix.md`
    /// → *Analysis*): without harmonics a quarter of tracks came out a fifth off.
    fn default() -> Self {
        Self {
            harmonics: 6,
            decay: 0.9,
        }
    }
}

/// Streaming key estimator over mono PCM at the source rate.
pub struct KeyEstimator {
    params: KeyParams,
    rate: u32,
    n: usize,
    hop: usize,
    fft: Arc<dyn RealToComplex<f32>>,
    window: Vec<f32>,
    input: Vec<f32>,
    spectrum: Vec<realfft::num_complex::Complex<f32>>,
    mag: Vec<f32>,
    /// Mono samples not yet consumed by a frame (starts at `pending_start`).
    pending: Vec<f32>,
    /// Frames analysed so far.
    frame: u64,
    /// One histogram per block, in order.
    blocks: Vec<[f32; FINE_BINS]>,
}

impl KeyEstimator {
    pub fn new(rate: u32) -> Self {
        Self::with_params(rate, KeyParams::default())
    }

    pub fn with_params(rate: u32, params: KeyParams) -> Self {
        // The largest power of two within 0.4 s: 16 384 points at 44.1 and
        // 48 kHz (0.37 / 0.34 s, 2.7 / 2.9 Hz bins; a semitone at C2 is 3.9 Hz).
        let n = (((f64::from(rate) * 0.4) as usize).max(2048) + 1).next_power_of_two() / 2;
        let fft = RealFftPlanner::<f32>::new().plan_fft_forward(n);
        let window = (0..n)
            .map(|i| (0.5 - 0.5 * (2.0 * std::f64::consts::PI * i as f64 / n as f64).cos()) as f32)
            .collect();
        Self {
            params,
            rate,
            n,
            hop: n / 2,
            input: fft.make_input_vec(),
            spectrum: fft.make_output_vec(),
            mag: vec![0.0; n / 2 + 1],
            fft,
            window,
            pending: Vec::with_capacity(n * 2),
            frame: 0,
            blocks: Vec::new(),
        }
    }

    pub fn push(&mut self, mono: &[f32]) {
        self.pending.extend_from_slice(mono);
        let mut used = 0;
        while self.pending.len() - used >= self.n {
            self.analyse_frame(used);
            used += self.hop;
        }
        self.pending.drain(..used);
    }

    fn analyse_frame(&mut self, at: usize) {
        let frame = &self.pending[at..at + self.n];
        // Frame start in ms decides its block.
        let start_ms = self.frame as f64 * self.hop as f64 * 1000.0 / f64::from(self.rate);
        self.frame += 1;
        let block = (start_ms / f64::from(BLOCK_MS)) as usize;
        let rms = (frame.iter().map(|x| x * x).sum::<f32>() / self.n as f32).sqrt();
        if !(rms.is_finite() && rms >= MIN_RMS) {
            return;
        }
        for ((o, &x), &w) in self.input.iter_mut().zip(frame).zip(&self.window) {
            *o = x * w;
        }
        if self
            .fft
            .process(&mut self.input, &mut self.spectrum)
            .is_err()
        {
            return;
        }
        for (m, c) in self.mag.iter_mut().zip(&self.spectrum) {
            *m = c.norm();
        }
        let res = f64::from(self.rate) / self.n as f64;
        let lo = ((F_LO / res).ceil() as usize).max(1);
        let hi = ((F_HI / res).floor() as usize).min(self.mag.len() - 2);
        if lo > hi {
            // A rate too low to hold C2.
            return;
        }
        let max = self.mag[lo..=hi].iter().copied().fold(0.0f32, f32::max);
        if max <= 0.0 {
            return;
        }
        let mut hist = [0.0f32; FINE_BINS];
        let mut total = 0.0f32;
        for k in lo..=hi {
            let m = self.mag[k];
            if m < max * PEAK_FLOOR || m <= self.mag[k - 1] || m < self.mag[k + 1] {
                continue;
            }
            // Parabolic interpolation on log magnitude for the peak's frequency.
            let (a, b, c) = (
                (self.mag[k - 1] + 1e-12).ln(),
                (m + 1e-12).ln(),
                (self.mag[k + 1] + 1e-12).ln(),
            );
            let denom = a - 2.0 * b + c;
            let shift = if denom.abs() > 1e-12 {
                (0.5 * (a - c) / denom).clamp(-0.5, 0.5)
            } else {
                0.0
            };
            let f = (k as f64 + f64::from(shift)) * res;
            // Log-compressed weight: quiet partials still count, loud bass doesn't drown them.
            let w = (1.0 + 100.0 * m / max).ln();
            let mut hw = w;
            for h in 1..=self.params.harmonics.max(1) {
                let midi = 69.0 + 12.0 * (f / f64::from(h) / 440.0).log2();
                let fine = (midi * FINE as f64).round() as i64;
                let bin = fine.rem_euclid(FINE_BINS as i64) as usize;
                hist[bin] += hw;
                total += hw;
                hw *= self.params.decay;
            }
        }
        if total <= 0.0 {
            return;
        }
        if self.blocks.len() <= block {
            self.blocks.resize(block + 1, [0.0; FINE_BINS]);
        }
        let out = &mut self.blocks[block];
        for (o, h) in out.iter_mut().zip(hist) {
            *o += h / total;
        }
    }

    /// Keys of the whole track, its first and its last [`KEY_REGION_MS`]
    /// (`duration_ms` places the tail). `None` where nothing tonal was heard.
    pub fn finish(
        self,
        duration_ms: Ms,
    ) -> (Option<MusicalKey>, Option<MusicalKey>, Option<MusicalKey>) {
        let sum = |range: &mut dyn Iterator<Item = &[f32; FINE_BINS]>| {
            let mut s = [0.0f32; FINE_BINS];
            for b in range {
                for (o, v) in s.iter_mut().zip(b) {
                    *o += v;
                }
            }
            s
        };
        let all = sum(&mut self.blocks.iter());
        // One tuning for the whole track: regions are too short to estimate it.
        let tuning = tuning_offset(&all);
        let head_blocks = (KEY_REGION_MS / BLOCK_MS) as usize;
        let tail_from = (duration_ms.saturating_sub(KEY_REGION_MS) / BLOCK_MS) as usize;
        let head = sum(&mut self.blocks.iter().take(head_blocks));
        let tail = sum(&mut self.blocks.iter().skip(tail_from));
        let key = |h: &[f32; FINE_BINS]| estimate(&fold(h, tuning));
        (key(&all), key(&head), key(&tail))
    }
}

/// The track's tuning relative to A440, in semitones (−0.5..0.5): the
/// weighted circular mean of where peaks fall within a semitone.
pub fn tuning_offset(fine: &[f32; FINE_BINS]) -> f64 {
    let (mut x, mut y) = (0.0f64, 0.0f64);
    for (i, &w) in fine.iter().enumerate() {
        let phase = 2.0 * std::f64::consts::PI * (i % FINE) as f64 / FINE as f64;
        x += f64::from(w) * phase.cos();
        y += f64::from(w) * phase.sin();
    }
    if x == 0.0 && y == 0.0 {
        return 0.0;
    }
    y.atan2(x) / (2.0 * std::f64::consts::PI)
}

/// Fold a fine histogram onto 12 pitch classes centred `tuning` semitones off A440.
pub fn fold(fine: &[f32; FINE_BINS], tuning: f64) -> [f64; 12] {
    let mut chroma = [0.0f64; 12];
    for (i, &w) in fine.iter().enumerate() {
        let s = i as f64 / FINE as f64 - tuning;
        let nearest = s.round();
        let d = s - nearest;
        let pc = (nearest as i64).rem_euclid(12) as usize;
        chroma[pc] += f64::from(w) * (std::f64::consts::PI * d).cos().powi(2);
    }
    chroma
}

fn pearson(x: &[f64; 12], y: &[f64; 12]) -> f64 {
    let mx = x.iter().sum::<f64>() / 12.0;
    let my = y.iter().sum::<f64>() / 12.0;
    let (mut sxy, mut sxx, mut syy) = (0.0, 0.0, 0.0);
    for i in 0..12 {
        let (a, b) = (x[i] - mx, y[i] - my);
        sxy += a * b;
        sxx += a * a;
        syy += b * b;
    }
    if sxx <= 0.0 || syy <= 0.0 {
        0.0
    } else {
        sxy / (sxx * syy).sqrt()
    }
}

/// The key whose profile best matches `chroma`, `None` for a flat or empty chroma.
pub fn estimate(chroma: &[f64; 12]) -> Option<MusicalKey> {
    if chroma.iter().sum::<f64>() <= 0.0 {
        return None;
    }
    let mut best: Option<(f64, u32, bool)> = None;
    for tonic in 0..12 {
        let mut rotated = [0.0; 12];
        for (i, r) in rotated.iter_mut().enumerate() {
            *r = chroma[(tonic + i) % 12];
        }
        for (profile, minor) in [(&MAJOR, false), (&MINOR, true)] {
            let r = pearson(&rotated, profile);
            if best.is_none_or(|(b, _, _)| r > b) {
                best = Some((r, tonic as u32, minor));
            }
        }
    }
    let (r, tonic, minor) = best.filter(|&(r, _, _)| r > 0.0)?;
    Some(MusicalKey {
        tonic,
        minor,
        strength: ((r * 100.0).round() / 100.0) as f32,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: u32 = 44_100;

    /// `seconds` of chords, each a list of MIDI notes with a few harmonics,
    /// a chord a second, `cents` off A440.
    fn chords(seconds: usize, progression: &[&[i32]], cents: f64) -> Vec<f32> {
        let n = RATE as usize;
        let mut out = vec![0.0f32; seconds * n];
        for s in 0..seconds {
            let chord = progression[s % progression.len()];
            for &note in chord {
                let f0 = 440.0 * 2f64.powf((f64::from(note) - 69.0 + cents / 100.0) / 12.0);
                for h in 1..=4 {
                    let f = f0 * h as f64;
                    let amp = 0.08 / h as f64;
                    for i in 0..n {
                        let t = (s * n + i) as f64 / f64::from(RATE);
                        out[s * n + i] += (amp * (2.0 * std::f64::consts::PI * f * t).sin()) as f32;
                    }
                }
            }
        }
        out
    }

    fn run(pcm: &[f32]) -> (Option<MusicalKey>, Option<MusicalKey>, Option<MusicalKey>) {
        let mut k = KeyEstimator::new(RATE);
        for c in pcm.chunks(4096) {
            k.push(c);
        }
        k.finish((pcm.len() as u64 * 1000 / u64::from(RATE)) as Ms)
    }

    // I–IV–V–I in C (bass notes an octave down), and i–iv–V–i in A minor.
    const C_MAJOR: &[&[i32]] = &[
        &[48, 60, 64, 67],
        &[41, 60, 65, 69],
        &[43, 59, 62, 67],
        &[48, 60, 64, 67],
    ];
    const A_MINOR: &[&[i32]] = &[
        &[45, 57, 60, 64],
        &[38, 57, 62, 65],
        &[40, 56, 59, 64],
        &[45, 57, 60, 64],
    ];

    #[test]
    fn finds_major_and_minor_keys() {
        let (key, head, tail) = run(&chords(12, C_MAJOR, 0.0));
        let key = key.unwrap();
        assert_eq!((key.tonic, key.minor), (0, false), "{key:?}");
        assert!(
            key.strength >= crate::automix::key::MIN_KEY_STRENGTH,
            "{key:?}"
        );
        assert_eq!(head.unwrap().tonic, 0);
        assert_eq!(tail.unwrap().tonic, 0);
        let key = run(&chords(12, A_MINOR, 0.0)).0.unwrap();
        assert_eq!((key.tonic, key.minor), (9, true), "{key:?}");
    }

    #[test]
    fn tolerates_detuning() {
        // 40 cents sharp is still C, not C#.
        let key = run(&chords(12, C_MAJOR, 40.0)).0.unwrap();
        assert_eq!((key.tonic, key.minor), (0, false), "{key:?}");
    }

    #[test]
    fn transposes() {
        let up: Vec<Vec<i32>> = C_MAJOR
            .iter()
            .map(|c| c.iter().map(|n| n + 2).collect())
            .collect();
        let up: Vec<&[i32]> = up.iter().map(|c| c.as_slice()).collect();
        let key = run(&chords(12, &up, 0.0)).0.unwrap();
        assert_eq!((key.tonic, key.minor), (2, false), "{key:?}");
    }

    #[test]
    fn head_and_tail_follow_a_key_change() {
        // 70 s in C, then 70 s a semitone up (the last chorus's gear change).
        let up: Vec<Vec<i32>> = C_MAJOR
            .iter()
            .map(|c| c.iter().map(|n| n + 1).collect())
            .collect();
        let up: Vec<&[i32]> = up.iter().map(|c| c.as_slice()).collect();
        let mut pcm = chords(70, C_MAJOR, 0.0);
        pcm.extend(chords(70, &up, 0.0));
        let (_, head, tail) = run(&pcm);
        assert_eq!(head.unwrap().tonic, 0);
        assert_eq!(tail.unwrap().tonic, 1);
    }

    #[test]
    fn other_rates() {
        // 48 kHz uses the same 16k frame as 44.1 kHz.
        assert_eq!(KeyEstimator::new(48_000).n, 16_384);
        assert_eq!(KeyEstimator::new(44_100).n, 16_384);
        let mut k = KeyEstimator::new(48_000);
        let pcm = chords(12, C_MAJOR, 0.0);
        // Resampled by index: 44.1 kHz chords read at 48 kHz are ~1.47 semitones sharp,
        // so only check that a key comes out; a tiny rate must not panic.
        k.push(&pcm);
        assert!(k.finish(12_000).0.is_some());
        let mut tiny = KeyEstimator::new(100);
        tiny.push(&vec![0.5; 5_000]);
        let _ = tiny.finish(50_000);
    }

    #[test]
    fn silence_has_no_key() {
        assert_eq!(run(&vec![0.0; RATE as usize * 5]), (None, None, None));
        assert_eq!(estimate(&[0.0; 12]), None);
    }

    #[test]
    fn tuning_and_fold() {
        let mut fine = [0.0f32; FINE_BINS];
        // Peaks 20 cents sharp of every C and G.
        fine[2] = 1.0;
        fine[7 * FINE + 2] = 1.0;
        let t = tuning_offset(&fine);
        assert!((t - 0.2).abs() < 1e-9, "{t}");
        let c = fold(&fine, t);
        assert!(
            (c[0] - 1.0).abs() < 1e-9 && (c[7] - 1.0).abs() < 1e-9,
            "{c:?}"
        );
    }
}
