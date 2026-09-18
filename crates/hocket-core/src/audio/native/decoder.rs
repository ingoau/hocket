//! Symphonia decoding: opening sources, sample-accurate seeking, encoder
//! padding trims, and the per-source decode thread that fills a PCM queue
//! the engine drains.
//!
//! Gapless trimming: for MP3, Symphonia applies the LAME delay/padding
//! itself when `enable_gapless` is set (it adds the 529-sample decoder delay
//! and honours the trims per packet). For AAC in MP4, Symphonia does not
//! read `iTunSMPB`, so the priming/padding is parsed here
//! ([`crate::audio::padding::parse_itunsmpb`]) and applied as frame trims.
//! Lossless formats carry exact frame counts and need nothing.

use std::collections::VecDeque;
use std::path::Path;
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use parking_lot::{Condvar, Mutex};
use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::{Decoder, DecoderOptions, CODEC_TYPE_AAC, CODEC_TYPE_MP3, CODEC_TYPE_NULL};
use symphonia::core::errors::Error as SymError;
use symphonia::core::formats::{FormatOptions, FormatReader, SeekMode, SeekTo};
use symphonia::core::io::{MediaSource, MediaSourceStream, MediaSourceStreamOptions};
use symphonia::core::meta::{MetadataOptions, Value};
use symphonia::core::probe::{Hint, ProbedMetadata};
use symphonia::core::units::{Time, TimeBase};

use crate::audio::padding::{find_lame_header, parse_itunsmpb, LameInfo, Trim};

#[derive(Debug, Clone, thiserror::Error, PartialEq, Eq)]
pub enum DecodeError {
    #[error("io: {0}")]
    Io(String),
    #[error("unsupported: {0}")]
    Unsupported(String),
    #[error("no audio track")]
    NoAudio,
    #[error("decode: {0}")]
    Decode(String),
    #[error("seek: {0}")]
    Seek(String),
}

impl From<std::io::Error> for DecodeError {
    fn from(e: std::io::Error) -> Self {
        DecodeError::Io(e.to_string())
    }
}

impl From<SymError> for DecodeError {
    fn from(e: SymError) -> Self {
        match e {
            SymError::IoError(io) => DecodeError::Io(io.to_string()),
            SymError::Unsupported(s) => DecodeError::Unsupported(s.to_string()),
            SymError::SeekError(k) => DecodeError::Seek(format!("{k:?}")),
            other => DecodeError::Decode(other.to_string()),
        }
    }
}

/// PCM format of a decoded stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Spec {
    pub sample_rate: u32,
    pub channels: usize,
}

/// An opened, probed source ready to decode.
pub struct Opened {
    pub format: Box<dyn FormatReader>,
    pub decoder: Box<dyn Decoder>,
    pub track_id: u32,
    pub spec: Spec,
    pub time_base: TimeBase,
    /// Total frames after trimming, when the container knows.
    pub duration_frames: Option<u64>,
    /// Trims this module applies itself (AAC); zero when Symphonia handles
    /// gapless (MP3) or there is nothing to trim.
    pub trim: Trim,
    /// LAME header of an MP3, for its ReplayGain fields.
    pub lame: Option<LameInfo>,
    pub probed_metadata: ProbedMetadata,
}

impl std::fmt::Debug for Opened {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Opened").field("track_id", &self.track_id).field("spec", &self.spec).field("trim", &self.trim).finish()
    }
}

/// Open a local file.
pub fn open_file(path: &Path) -> Result<Opened, DecodeError> {
    let file = std::fs::File::open(path)?;
    let hint = path.extension().and_then(|e| e.to_str()).map(str::to_string);
    open_media_source(Box::new(file), hint.as_deref(), None, true)
}

/// Open any seekable [`MediaSource`]. `extension` / `mime_type` hint the
/// probe; `gapless` enables padding trims.
pub fn open_media_source(
    mut source: Box<dyn MediaSource>,
    extension: Option<&str>,
    mime_type: Option<&str>,
    gapless: bool,
) -> Result<Opened, DecodeError> {
    // Peek the head for an MP3 LAME tag before Symphonia consumes it.
    let lame = if source.is_seekable() {
        let mut head = vec![0u8; 16 * 1024];
        let mut read = 0;
        while read < head.len() {
            match std::io::Read::read(&mut source, &mut head[read..]) {
                Ok(0) => break,
                Ok(n) => read += n,
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(e.into()),
            }
        }
        std::io::Seek::seek(&mut source, std::io::SeekFrom::Start(0))?;
        find_lame_header(&head[..read])
    } else {
        None
    };
    let mut hint = Hint::new();
    if let Some(ext) = extension {
        hint.with_extension(ext);
    }
    if let Some(mime) = mime_type {
        hint.mime_type(mime);
    }
    let mss = MediaSourceStream::new(source, MediaSourceStreamOptions::default());
    let fmt_opts = FormatOptions { enable_gapless: gapless, ..Default::default() };
    let probed = symphonia::default::get_probe().format(&hint, mss, &fmt_opts, &MetadataOptions::default())?;
    let mut format = probed.format;
    let track = format
        .tracks()
        .iter()
        .find(|t| t.codec_params.codec != CODEC_TYPE_NULL)
        .ok_or(DecodeError::NoAudio)?;
    let track_id = track.id;
    let params = track.codec_params.clone();
    let sample_rate = params.sample_rate.ok_or_else(|| DecodeError::Unsupported("unknown sample rate".into()))?;
    let channels = params.channels.map(|c| c.count()).or_else(|| params.channel_layout.map(|l| l.into_channels().count())).unwrap_or(2);
    let time_base = params.time_base.unwrap_or_else(|| TimeBase::new(1, sample_rate));
    let decoder = symphonia::default::get_codecs().make(&params, &DecoderOptions::default())?;
    // AAC priming/padding from iTunSMPB.
    let mut trim = Trim::default();
    if gapless && params.codec == CODEC_TYPE_AAC {
        let mut meta = format.metadata();
        meta.skip_to_latest();
        if let Some(rev) = meta.current() {
            for tag in rev.tags() {
                if tag.key.to_ascii_lowercase().ends_with("itunsmpb") {
                    if let Value::String(s) = &tag.value {
                        if let Some(p) = parse_itunsmpb(s) {
                            trim = p.trim_for_aac();
                        }
                    }
                }
            }
        }
        // Without the tag, trim at least the codec's known priming.
        if trim == Trim::default() {
            if let Some(d) = params.delay {
                trim.skip_start = d;
            }
        }
    }
    let raw_frames = params.n_frames;
    let duration_frames = raw_frames.map(|n| n.saturating_sub(u64::from(trim.skip_start)).saturating_sub(u64::from(trim.skip_end)));
    let _ = CODEC_TYPE_MP3; // MP3 trims are Symphonia's job (see module docs).
    Ok(Opened {
        format,
        decoder,
        track_id,
        spec: Spec { sample_rate, channels },
        time_base,
        duration_frames,
        trim,
        lame,
        probed_metadata: probed.metadata,
    })
}

fn ts_to_frames(tb: TimeBase, ts: u64, rate: u32) -> u64 {
    let t = tb.calc_time(ts);
    t.seconds * u64::from(rate) + (t.frac * f64::from(rate)).round() as u64
}

fn frames_to_time(frames: u64, rate: u32) -> Time {
    let secs = frames / u64::from(rate);
    let frac = (frames % u64::from(rate)) as f64 / f64::from(rate);
    Time::new(secs, frac)
}

/// Decode a whole file, handing interleaved `f32` chunks to `sink`.
pub fn decode_file(path: &Path, mut sink: impl FnMut(Spec, &[f32])) -> Result<Spec, DecodeError> {
    let mut opened = open_file(path)?;
    let mut reader = PcmReader::new(&mut opened, 0)?;
    let spec = opened.spec;
    let mut buf = Vec::new();
    loop {
        buf.clear();
        let n = reader.next_chunk(&mut opened, &mut buf)?;
        if n == 0 {
            break;
        }
        sink(spec, &buf);
    }
    Ok(spec)
}

/// Pulls trimmed, sample-accurate PCM out of an [`Opened`] source.
struct PcmReader {
    sample_buf: Option<SampleBuffer<f32>>,
    /// Raw stream frame index of the next decoded frame.
    raw_pos: u64,
    /// Frames still to drop for sample-accurate seeking.
    skip: u64,
    raw_end: Option<u64>,
    eof: bool,
}

impl PcmReader {
    /// Position `frame` is in trimmed frames.
    fn new(opened: &mut Opened, frame: u64) -> Result<Self, DecodeError> {
        let mut r = Self { sample_buf: None, raw_pos: 0, skip: 0, raw_end: None, eof: false };
        r.raw_end = opened.duration_frames.map(|d| d + u64::from(opened.trim.skip_start));
        r.seek(opened, frame)?;
        Ok(r)
    }

    fn seek(&mut self, opened: &mut Opened, frame: u64) -> Result<(), DecodeError> {
        let rate = opened.spec.sample_rate;
        let raw_target = frame + u64::from(opened.trim.skip_start);
        self.eof = false;
        if raw_target == 0 {
            // Fresh start: no seek needed unless we've already read.
            if self.raw_pos != 0 || self.sample_buf.is_some() {
                let seeked = opened.format.seek(SeekMode::Accurate, SeekTo::Time { time: Time::new(0, 0.0), track_id: Some(opened.track_id) })?;
                opened.decoder.reset();
                self.raw_pos = ts_to_frames(opened.time_base, seeked.actual_ts, rate);
            }
            self.skip = raw_target.saturating_sub(self.raw_pos);
            return Ok(());
        }
        let time = frames_to_time(raw_target, rate);
        match opened.format.seek(SeekMode::Accurate, SeekTo::Time { time, track_id: Some(opened.track_id) }) {
            Ok(seeked) => {
                opened.decoder.reset();
                self.raw_pos = ts_to_frames(opened.time_base, seeked.actual_ts, rate);
                self.skip = raw_target.saturating_sub(self.raw_pos);
            }
            Err(SymError::SeekError(k)) => {
                // Unseekable (unknown length): restart and skip forward.
                tracing::debug!(target: "hocket::audio::decoder", kind = ?k, "accurate seek unsupported, decoding forward");
                let seeked = opened.format.seek(SeekMode::Coarse, SeekTo::Time { time: Time::new(0, 0.0), track_id: Some(opened.track_id) })?;
                opened.decoder.reset();
                self.raw_pos = ts_to_frames(opened.time_base, seeked.actual_ts, rate);
                self.skip = raw_target.saturating_sub(self.raw_pos);
            }
            Err(e) => return Err(e.into()),
        }
        Ok(())
    }

    /// Append the next chunk of interleaved frames to `out`; returns frames
    /// appended, 0 at end of stream.
    fn next_chunk(&mut self, opened: &mut Opened, out: &mut Vec<f32>) -> Result<usize, DecodeError> {
        loop {
            if self.eof {
                return Ok(0);
            }
            let packet = match opened.format.next_packet() {
                Ok(p) => p,
                Err(SymError::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                    self.eof = true;
                    return Ok(0);
                }
                Err(SymError::ResetRequired) => {
                    opened.decoder.reset();
                    continue;
                }
                Err(e) => return Err(e.into()),
            };
            if packet.track_id() != opened.track_id {
                continue;
            }
            let decoded = match opened.decoder.decode(&packet) {
                Ok(d) => d,
                Err(SymError::DecodeError(msg)) => {
                    tracing::debug!(target: "hocket::audio::decoder", msg, "skipping corrupt packet");
                    continue;
                }
                Err(SymError::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                    self.eof = true;
                    return Ok(0);
                }
                Err(e) => return Err(e.into()),
            };
            let frames = decoded.frames();
            if frames == 0 {
                continue;
            }
            let spec = *decoded.spec();
            let channels = spec.channels.count();
            let buf = self.sample_buf.get_or_insert_with(|| SampleBuffer::new(frames as u64 * 2, spec));
            if buf.capacity() < frames * channels {
                *buf = SampleBuffer::new(frames as u64 * 2, spec);
            }
            buf.copy_interleaved_ref(decoded);
            let mut samples = buf.samples();
            let mut start_frame = self.raw_pos;
            self.raw_pos += frames as u64;
            // Sample-accurate seek: drop what precedes the target.
            if self.skip > 0 {
                let drop = (self.skip as usize).min(frames);
                samples = &samples[drop * channels..];
                self.skip -= drop as u64;
                start_frame += drop as u64;
                if samples.is_empty() {
                    continue;
                }
            }
            // End trim.
            if let Some(end) = self.raw_end {
                let n = samples.len() / channels;
                if start_frame >= end {
                    self.eof = true;
                    return Ok(0);
                }
                let keep = ((end - start_frame) as usize).min(n);
                samples = &samples[..keep * channels];
                if keep < n {
                    self.eof = true;
                }
            }
            if samples.is_empty() {
                continue;
            }
            out.extend_from_slice(samples);
            return Ok(samples.len() / channels);
        }
    }
}

/// What the decode thread is told.
enum Cmd {
    /// Seek to a trimmed frame; bumps the queue generation so stale samples
    /// are discarded.
    Seek { frame: u64, generation: u32 },
    Stop,
}

struct QueueState {
    samples: VecDeque<f32>,
    /// Source frame index of the first sample in `samples`.
    head_frame: u64,
    generation: u32,
    eof: bool,
    error: Option<DecodeError>,
}

/// Bounded PCM queue between a decode thread and the engine.
pub struct PcmQueue {
    state: Mutex<QueueState>,
    space: Condvar,
    channels: usize,
    capacity_samples: usize,
}

impl PcmQueue {
    fn new(channels: usize, capacity_frames: usize) -> Self {
        Self {
            state: Mutex::new(QueueState { samples: VecDeque::new(), head_frame: 0, generation: 0, eof: false, error: None }),
            space: Condvar::new(),
            channels,
            capacity_samples: capacity_frames.max(1) * channels,
        }
    }

    /// Frames buffered.
    pub fn buffered_frames(&self) -> usize {
        self.state.lock().samples.len() / self.channels
    }

    pub fn capacity_frames(&self) -> usize {
        self.capacity_samples / self.channels
    }

    pub fn is_eof(&self) -> bool {
        let st = self.state.lock();
        st.eof && st.samples.is_empty()
    }

    /// Decoder finished (successfully) and everything has been consumed, or
    /// it failed.
    pub fn error(&self) -> Option<DecodeError> {
        self.state.lock().error.clone()
    }

    /// Source frame index of the next frame [`Self::pop`] returns.
    pub fn head_frame(&self) -> u64 {
        self.state.lock().head_frame
    }

    /// Consumer: take up to `frames` frames into `out`. Returns frames taken
    /// and the source frame index of the first one.
    pub fn pop(&self, frames: usize, out: &mut Vec<f32>) -> (usize, u64) {
        let mut st = self.state.lock();
        let n = (frames * self.channels).min(st.samples.len());
        let n = n - n % self.channels;
        let start = st.head_frame;
        out.extend(st.samples.drain(..n));
        let taken = n / self.channels;
        st.head_frame += taken as u64;
        if taken > 0 {
            self.space.notify_one();
        }
        (taken, start)
    }
}

/// Handle to a running decode thread. Dropping it stops the thread.
pub struct DecodeHandle {
    cmd: Sender<Cmd>,
    pub queue: Arc<PcmQueue>,
    pub spec: Spec,
    pub duration_frames: Option<u64>,
    generation: Mutex<u32>,
    thread: Option<thread::JoinHandle<Opened>>,
}

impl std::fmt::Debug for DecodeHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DecodeHandle").field("spec", &self.spec).field("duration_frames", &self.duration_frames).finish()
    }
}

impl DecodeHandle {
    /// Start decoding `opened` from `start_frame` on its own thread into a
    /// queue of `capacity_frames`.
    pub fn spawn(mut opened: Opened, start_frame: u64, capacity_frames: usize, name: &str) -> Result<Self, DecodeError> {
        let spec = opened.spec;
        let duration_frames = opened.duration_frames;
        let queue = Arc::new(PcmQueue::new(spec.channels, capacity_frames));
        let (tx, rx) = mpsc::channel();
        let q = queue.clone();
        let mut reader = PcmReader::new(&mut opened, start_frame)?;
        {
            let mut st = queue.state.lock();
            st.head_frame = start_frame;
        }
        let thread = thread::Builder::new()
            .name(format!("hocket-decode-{name}"))
            .spawn(move || {
                run_decoder(&mut opened, &mut reader, &q, &rx);
                opened
            })
            .map_err(|e| DecodeError::Io(e.to_string()))?;
        Ok(Self { cmd: tx, queue, spec, duration_frames, generation: Mutex::new(0), thread: Some(thread) })
    }

    /// Reposition. Buffered samples are discarded; the queue reports the
    /// new `head_frame` once the decoder has repositioned.
    pub fn seek(&self, frame: u64) {
        let mut g = self.generation.lock();
        *g = g.wrapping_add(1);
        {
            let mut st = self.queue.state.lock();
            st.generation = *g;
            st.samples.clear();
            st.eof = false;
            st.error = None;
            st.head_frame = frame;
        }
        let _ = self.cmd.send(Cmd::Seek { frame, generation: *g });
        self.queue.space.notify_all();
    }

    /// Stop the thread and get the source back (for reuse).
    pub fn into_opened(mut self) -> Option<Opened> {
        let _ = self.cmd.send(Cmd::Stop);
        self.queue.space.notify_all();
        self.thread.take().and_then(|t| t.join().ok())
    }
}

impl Drop for DecodeHandle {
    fn drop(&mut self) {
        let _ = self.cmd.send(Cmd::Stop);
        self.queue.space.notify_all();
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

fn run_decoder(opened: &mut Opened, reader: &mut PcmReader, q: &Arc<PcmQueue>, rx: &Receiver<Cmd>) {
    let mut generation = 0u32;
    let mut chunk = Vec::new();
    loop {
        // Commands first.
        loop {
            match rx.try_recv() {
                Ok(Cmd::Stop) => return,
                Ok(Cmd::Seek { frame, generation: g }) => {
                    generation = g;
                    if let Err(e) = reader.seek(opened, frame) {
                        let mut st = q.state.lock();
                        if st.generation == generation {
                            st.error = Some(e);
                            st.eof = true;
                        }
                    }
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => return,
            }
        }
        // Wait for space.
        {
            let mut st = q.state.lock();
            while st.samples.len() >= q.capacity_samples && st.generation == generation {
                q.space.wait_for(&mut st, Duration::from_millis(50));
                if rx_has_cmd(rx) {
                    break;
                }
            }
            if st.generation != generation {
                // A seek is pending; loop to pick it up.
                drop(st);
                match rx.recv_timeout(Duration::from_millis(50)) {
                    Ok(Cmd::Stop) => return,
                    Ok(Cmd::Seek { frame, generation: g }) => {
                        generation = g;
                        if let Err(e) = reader.seek(opened, frame) {
                            let mut st = q.state.lock();
                            st.error = Some(e);
                            st.eof = true;
                        }
                    }
                    Err(mpsc::RecvTimeoutError::Disconnected) => return,
                    Err(mpsc::RecvTimeoutError::Timeout) => {}
                }
                continue;
            }
            if st.samples.len() >= q.capacity_samples {
                continue;
            }
            if st.eof {
                drop(st);
                match rx.recv_timeout(Duration::from_millis(100)) {
                    Ok(Cmd::Stop) | Err(mpsc::RecvTimeoutError::Disconnected) => return,
                    Ok(Cmd::Seek { frame, generation: g }) => {
                        generation = g;
                        if let Err(e) = reader.seek(opened, frame) {
                            let mut st = q.state.lock();
                            st.error = Some(e);
                            st.eof = true;
                        }
                    }
                    Err(mpsc::RecvTimeoutError::Timeout) => {}
                }
                continue;
            }
        }
        chunk.clear();
        match reader.next_chunk(opened, &mut chunk) {
            Ok(0) => {
                let mut st = q.state.lock();
                if st.generation == generation {
                    st.eof = true;
                }
            }
            Ok(_) => {
                let mut st = q.state.lock();
                if st.generation == generation {
                    st.samples.extend(chunk.iter().copied());
                }
            }
            Err(e) => {
                tracing::warn!(target: "hocket::audio::decoder", error = %e, "decode failed");
                let mut st = q.state.lock();
                if st.generation == generation {
                    st.error = Some(e);
                    st.eof = true;
                }
            }
        }
    }
}

fn rx_has_cmd(rx: &Receiver<Cmd>) -> bool {
    // Peek without consuming isn't available on std mpsc; a cheap proxy is
    // to let the outer loop poll after a short wait.
    let _ = rx;
    false
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::io::Write;

    /// Write a 16-bit PCM WAV with a 440 Hz tone in L and a ramp in R.
    pub(crate) fn write_test_wav(path: &Path, rate: u32, frames: u32) {
        let mut f = std::fs::File::create(path).unwrap();
        let data_len = frames * 4;
        f.write_all(b"RIFF").unwrap();
        f.write_all(&(36 + data_len).to_le_bytes()).unwrap();
        f.write_all(b"WAVEfmt ").unwrap();
        f.write_all(&16u32.to_le_bytes()).unwrap();
        f.write_all(&1u16.to_le_bytes()).unwrap(); // PCM
        f.write_all(&2u16.to_le_bytes()).unwrap();
        f.write_all(&rate.to_le_bytes()).unwrap();
        f.write_all(&(rate * 4).to_le_bytes()).unwrap();
        f.write_all(&4u16.to_le_bytes()).unwrap();
        f.write_all(&16u16.to_le_bytes()).unwrap();
        f.write_all(b"data").unwrap();
        f.write_all(&data_len.to_le_bytes()).unwrap();
        let mut body = Vec::with_capacity(data_len as usize);
        for i in 0..frames {
            let l = (0.5 * (2.0 * std::f64::consts::PI * 440.0 * f64::from(i) / f64::from(rate)).sin() * 32767.0) as i16;
            let r = ((i % 1000) as i32 - 500) as i16 * 60;
            body.extend_from_slice(&l.to_le_bytes());
            body.extend_from_slice(&r.to_le_bytes());
        }
        f.write_all(&body).unwrap();
    }

    #[test]
    fn decodes_a_generated_wav_to_pcm() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("tone.wav");
        write_test_wav(&p, 44_100, 44_100);
        let mut pcm = Vec::new();
        let spec = decode_file(&p, |_, chunk| pcm.extend_from_slice(chunk)).unwrap();
        assert_eq!(spec, Spec { sample_rate: 44_100, channels: 2 });
        assert_eq!(pcm.len(), 44_100 * 2);
        let peak = pcm.iter().step_by(2).fold(0.0f32, |m, v| m.max(v.abs()));
        assert!((peak - 0.5).abs() < 0.01, "{peak}");
        // Right channel ramp: frame 100 → (100-500)*60/32768.
        assert!((pcm[100 * 2 + 1] - (-400.0 * 60.0 / 32768.0)).abs() < 1e-3);
    }

    #[test]
    fn seeking_is_sample_accurate_on_pcm() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("tone.wav");
        write_test_wav(&p, 48_000, 96_000);
        let mut opened = open_file(&p).unwrap();
        assert_eq!(opened.duration_frames, Some(96_000));
        let mut reader = PcmReader::new(&mut opened, 12_345).unwrap();
        let mut out = Vec::new();
        reader.next_chunk(&mut opened, &mut out).unwrap();
        // Right channel at frame 12345: (12345 % 1000 - 500) * 60.
        let expected = f32::from(((12_345 % 1000) as i32 - 500) as i16 * 60) / 32768.0;
        assert!((out[1] - expected).abs() < 1e-4, "{} vs {expected}", out[1]);
        assert_eq!(reader.raw_pos - (out.len() / 2) as u64, 12_345);
    }

    #[test]
    fn decode_thread_streams_through_the_queue_and_seeks() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("tone.wav");
        write_test_wav(&p, 48_000, 48_000);
        let opened = open_file(&p).unwrap();
        let handle = DecodeHandle::spawn(opened, 0, 4096, "test").unwrap();
        let mut total = 0usize;
        let mut buf = Vec::new();
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        while total < 48_000 {
            buf.clear();
            let (n, start) = handle.queue.pop(1024, &mut buf);
            if n == 0 {
                assert!(handle.queue.error().is_none());
                assert!(std::time::Instant::now() < deadline, "timed out at {total}");
                thread::sleep(Duration::from_millis(1));
                continue;
            }
            assert_eq!(start as usize, total);
            total += n;
        }
        assert_eq!(total, 48_000);
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while !handle.queue.is_eof() {
            assert!(std::time::Instant::now() < deadline);
            thread::sleep(Duration::from_millis(1));
        }
        // Seek back and read again.
        handle.seek(1000);
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        loop {
            buf.clear();
            let (n, start) = handle.queue.pop(256, &mut buf);
            if n > 0 {
                assert_eq!(start, 1000);
                let expected = f32::from(((1000 % 1000) as i32 - 500) as i16 * 60) / 32768.0;
                assert!((buf[1] - expected).abs() < 1e-4);
                break;
            }
            assert!(std::time::Instant::now() < deadline, "seek never produced data");
            thread::sleep(Duration::from_millis(1));
        }
        let opened = handle.into_opened();
        assert!(opened.is_some());
    }

    #[test]
    fn unreadable_file_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("nope.wav");
        assert!(matches!(open_file(&p), Err(DecodeError::Io(_))));
        std::fs::write(&p, b"not audio at all, just text").unwrap();
        assert!(open_file(&p).is_err());
    }
}
