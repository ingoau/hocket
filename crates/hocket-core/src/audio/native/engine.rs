//! The native backend: an engine thread that owns the output stream, the
//! current / next / pre-buffered sources, the DSP chain and the volume
//! ramp, and reports through [`BackendReport`] exactly like the external
//! backend does.
//!
//! Timing model: the output callback (or the null sink) drains the ring at
//! real-time rate; the engine keeps the ring full while playing, so its
//! loop is paced by the device. Loads run on short-lived loader threads
//! (opening an HTTP source blocks until headers arrive; probing reads) and
//! post back with a generation number so a superseded load is ignored.
//! Positions are derived from what has been pushed minus what is still in
//! the ring and the resampler, so they are accurate to about the ring size
//! (~100 ms) and monotonic within a track.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use parking_lot::Mutex;

use crate::api::{AudioSettings, BackendReport, MediaSource, OutputDevice, QueueKey};
use crate::audio::backend::{clamp_volume, BackendError, PlaybackBackend, ReportSink};
use crate::audio::dsp::{DspChain, DspSettings, SmoothGain};
use crate::audio::native::decoder::{
    open_file, open_media_source, DecodeError, DecodeHandle, Spec,
};
use crate::audio::native::http::{HttpSource, RangeFetcher};
use crate::audio::native::output::{
    list_output_devices, open_output, probe_format, DeviceWatcher, OutputConfig, OutputRing,
    OutputStream,
};
use crate::audio::native::resample::{remap_channels, Resampler, CHUNK_FRAMES};

/// Output ring capacity.
pub const RING_MS: u32 = 200;
/// Decoded PCM kept ahead of the engine per source.
pub const QUEUE_SECONDS: u32 = 4;
/// How much a handoff pre-buffer decodes.
pub const PREBUFFER_SECONDS: u32 = 10;
/// A pre-buffer is reused by a `load` of the same key within this distance.
pub const PREBUFFER_REUSE_TOLERANCE_MS: u32 = 2_000;
/// Play/pause fade and volume ramp.
pub const FADE_MS: f64 = 10.0;
/// Position report cadence.
pub const POSITION_INTERVAL: Duration = Duration::from_millis(1000);
/// Waiting for HTTP headers.
pub const OPEN_TIMEOUT: Duration = Duration::from_secs(20);

/// Construction parameters.
pub struct NativeConfig {
    pub fetcher: Arc<dyn RangeFetcher>,
    pub runtime: tokio::runtime::Handle,
    pub output: OutputConfig,
    pub audio: AudioSettings,
    /// Poll for device hot-plug (off in tests).
    pub watch_devices: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Role {
    Current,
    Next,
    PreBuffer,
}

struct Loaded {
    media: MediaSource,
    handle: DecodeHandle,
}

/// Engine commands. `Load` carries whole `MediaSource`s and dwarfs the unit
/// variants; the channel carries a handful of these per track, so boxing
/// would buy nothing.
#[allow(clippy::large_enum_variant)]
enum Cmd {
    Load {
        source: MediaSource,
        next: Option<MediaSource>,
        position_ms: u32,
        play: bool,
    },
    SetNext(Option<MediaSource>),
    Play,
    Pause,
    Stop,
    Seek(u32),
    SetVolume(f64),
    PreBuffer {
        source: MediaSource,
        position_ms: u32,
    },
    DiscardPreBuffer,
    SetGapless(bool),
    SetOutputDevice(Option<String>),
    SetExclusive(bool),
    SetDsp(DspSettings),
    Loaded {
        role: Role,
        generation: u64,
        key: QueueKey,
        result: Result<Box<Loaded>, DecodeError>,
    },
    Shutdown,
}

struct Source {
    media: MediaSource,
    handle: DecodeHandle,
    spec: Spec,
    duration_ms: Option<u32>,
    resampler: Option<Resampler>,
    resampler_for: Option<(u32, usize)>,
    /// Source frame where the current run started (load position / seek).
    start_frame: u64,
    /// Source frames pushed towards the output since `start_frame`.
    pushed_frames: u64,
    buffering: bool,
    /// Decoder hit end-of-stream and the resampler has been flushed.
    finished: bool,
}

impl Source {
    fn new(l: Box<Loaded>) -> Self {
        let l = *l;
        let spec = l.handle.spec;
        let duration_ms = l
            .handle
            .duration_frames
            .map(|f| (f * 1000 / u64::from(spec.sample_rate)).min(u64::from(u32::MAX)) as u32)
            .or(Some(l.media.track.duration_ms).filter(|d| *d > 0));
        let start_frame = l.handle.queue.head_frame();
        Self {
            media: l.media,
            handle: l.handle,
            spec,
            duration_ms,
            resampler: None,
            resampler_for: None,
            start_frame,
            pushed_frames: 0,
            buffering: false,
            finished: false,
        }
    }

    fn key(&self) -> &QueueKey {
        &self.media.key
    }

    fn frames_to_ms(&self, frames: u64) -> u32 {
        (frames * 1000 / u64::from(self.spec.sample_rate)).min(u64::from(u32::MAX)) as u32
    }

    fn ms_to_frames(&self, ms: u32) -> u64 {
        u64::from(ms) * u64::from(self.spec.sample_rate) / 1000
    }

    /// Position given how many source frames are still queued downstream.
    fn position_ms(&self, pending_src_frames: u64) -> u32 {
        let played = self.pushed_frames.saturating_sub(pending_src_frames);
        let mut pos = self.frames_to_ms(self.start_frame + played);
        if let Some(d) = self.duration_ms {
            pos = pos.min(d);
        }
        pos
    }
}

enum Step {
    Continue,
    Wait,
}

/// Everything the engine thread needs to build itself (all `Send`; the
/// cpal stream is created on the thread because it isn't).
struct EngineParams {
    rx: Receiver<Cmd>,
    tx: Sender<Cmd>,
    sink: ReportSink,
    fetcher: Arc<dyn RangeFetcher>,
    runtime: tokio::runtime::Handle,
    output: OutputConfig,
    dsp: DspSettings,
    gapless: bool,
    exclusive: bool,
}

struct Engine {
    rx: Receiver<Cmd>,
    tx: Sender<Cmd>,
    sink: ReportSink,
    fetcher: Arc<dyn RangeFetcher>,
    runtime: tokio::runtime::Handle,
    output_config: OutputConfig,
    output: Option<OutputStream>,
    ring: Arc<OutputRing>,
    out_rate: u32,
    out_channels: usize,
    dsp: DspChain,
    volume: SmoothGain,
    user_volume: f64,
    playing: bool,
    fading_out: bool,
    gapless: bool,
    exclusive: bool,
    current: Option<Source>,
    next: Option<Source>,
    next_key: Option<QueueKey>,
    awaiting_next: bool,
    /// A `Current` load is in flight; `playing` must survive until it lands.
    loading_current: bool,
    pre_buffer: Option<Source>,
    pre_buffer_key: Option<QueueKey>,
    pre_buffer_position_ms: u32,
    generation: u64,
    pre_buffer_generation: u64,
    /// Output-format samples converted but not yet pushed.
    pending_out: Vec<f32>,
    scratch_src: Vec<f32>,
    scratch_map: Vec<f32>,
    last_position_report: Instant,
    error_flag: Arc<AtomicBool>,
}

impl Engine {
    fn new(p: EngineParams) -> Self {
        Engine {
            rx: p.rx,
            tx: p.tx,
            sink: p.sink,
            fetcher: p.fetcher,
            runtime: p.runtime,
            output_config: p.output,
            output: None,
            ring: OutputRing::new(16, 2),
            out_rate: 48_000,
            out_channels: 2,
            dsp: DspChain::new(48_000.0, 2, p.dsp),
            volume: SmoothGain::new(1.0, 480),
            user_volume: 1.0,
            playing: false,
            fading_out: false,
            gapless: p.gapless,
            exclusive: p.exclusive,
            current: None,
            next: None,
            next_key: None,
            awaiting_next: false,
            loading_current: false,
            pre_buffer: None,
            pre_buffer_key: None,
            pre_buffer_position_ms: 0,
            generation: 0,
            pre_buffer_generation: 0,
            pending_out: Vec::new(),
            scratch_src: Vec::with_capacity(CHUNK_FRAMES * 8),
            scratch_map: Vec::with_capacity(CHUNK_FRAMES * 8),
            last_position_report: Instant::now(),
            error_flag: Arc::new(AtomicBool::new(false)),
        }
    }

    fn report(&self, r: BackendReport) {
        tracing::trace!(target: "hocket::audio::native", report = ?r);
        (self.sink)(r);
    }

    // -- output -----------------------------------------------------------

    fn open_output(&mut self, config: OutputConfig) -> Result<(), String> {
        let (rate, channels) = probe_format(&config).map_err(|e| e.to_string())?;
        let ring = OutputRing::new((rate * RING_MS / 1000) as usize, channels);
        let flag = self.error_flag.clone();
        let stream = open_output(&config, ring.clone(), move |_| {
            flag.store(true, Ordering::Release)
        })
        .map_err(|e| e.to_string())?;
        let (rate, channels) = (stream.rate(), stream.channels());
        self.output = None; // close the old one before keeping the new
        self.output = Some(stream);
        self.ring = ring;
        self.out_rate = rate;
        self.out_channels = channels;
        self.output_config = config;
        let ramp = ((f64::from(rate) * FADE_MS / 1000.0) as usize).max(1);
        let v = f64::from(self.volume.target());
        self.volume = SmoothGain::new(v, ramp);
        self.pending_out.clear();
        for s in [&mut self.current, &mut self.next].into_iter().flatten() {
            s.resampler = None;
            s.resampler_for = None;
        }
        Ok(())
    }

    fn ensure_output(&mut self) {
        if self.output.is_some() {
            return;
        }
        let cfg = self.output_config.clone();
        if let Err(e) = self.open_output(cfg) {
            tracing::warn!(target: "hocket::audio::native", error = %e, "opening output failed, using null sink");
            let cfg = OutputConfig {
                null_sink: true,
                ..self.output_config.clone()
            };
            if let Err(e) = self.open_output(cfg) {
                tracing::error!(target: "hocket::audio::native", error = %e, "null sink failed");
            }
        }
    }

    /// Reopen the output at the source's rate when exclusive mode asks for
    /// it and the device can; otherwise keep resampling.
    fn maybe_switch_rate(&mut self) {
        let Some(rate) = self.current.as_ref().map(|c| c.spec.sample_rate) else {
            return;
        };
        let wanted = if self.exclusive { Some(rate) } else { None };
        if self.output_config.sample_rate == wanted {
            return;
        }
        let cfg = OutputConfig {
            sample_rate: wanted,
            ..self.output_config.clone()
        };
        match self.open_output(cfg) {
            Ok(()) => {
                tracing::info!(target: "hocket::audio::native", rate = self.out_rate, "output rate switched")
            }
            Err(e) => {
                tracing::info!(target: "hocket::audio::native", error = %e, "device can't follow the source rate, resampling")
            }
        }
    }

    // -- loading ----------------------------------------------------------

    fn spawn_load(&self, role: Role, generation: u64, source: MediaSource, position_ms: u32) {
        let tx = self.tx.clone();
        let fetcher = self.fetcher.clone();
        let runtime = self.runtime.clone();
        let gapless = self.gapless;
        let key = source.key.clone();
        let seconds = if role == Role::PreBuffer {
            PREBUFFER_SECONDS
        } else {
            QUEUE_SECONDS
        };
        let name = format!("{role:?}").to_ascii_lowercase();
        let key2 = key.clone();
        let spawn = thread::Builder::new()
            .name(format!("hocket-load-{name}"))
            .spawn(move || {
                let result = load_source(
                    &source,
                    position_ms,
                    gapless,
                    seconds,
                    fetcher,
                    runtime,
                    &name,
                )
                .map(|handle| {
                    Box::new(Loaded {
                        media: source,
                        handle,
                    })
                });
                let _ = tx.send(Cmd::Loaded {
                    role,
                    generation,
                    key: key2,
                    result,
                });
            });
        if let Err(e) = spawn {
            let _ = self.tx.send(Cmd::Loaded {
                role,
                generation,
                key,
                result: Err(DecodeError::Io(e.to_string())),
            });
        }
    }

    fn on_loaded(
        &mut self,
        role: Role,
        generation: u64,
        key: QueueKey,
        result: Result<Box<Loaded>, DecodeError>,
    ) {
        match role {
            Role::Current => {
                if generation != self.generation || self.current.is_some() {
                    return;
                }
                self.loading_current = false;
                match result {
                    Ok(l) => {
                        let src = Source::new(l);
                        self.begin_source(src);
                        let cur = self.current.as_ref().expect("just set");
                        self.report(BackendReport::Ready {
                            key: key.clone(),
                            duration_ms: cur.duration_ms,
                        });
                        let pos = self.current_position();
                        if self.playing {
                            self.report(BackendReport::Playing {
                                key,
                                position_ms: pos,
                            });
                        } else {
                            self.report(BackendReport::Paused {
                                key,
                                position_ms: pos,
                            });
                        }
                    }
                    Err(e) => {
                        self.playing = false;
                        self.report(BackendReport::Error {
                            key,
                            message: e.to_string(),
                            fatal: true,
                        });
                    }
                }
            }
            Role::Next => {
                if generation != self.generation || self.next_key.as_ref() != Some(&key) {
                    return;
                }
                match result {
                    Ok(l) => {
                        self.next = Some(Source::new(l));
                        if self.awaiting_next {
                            self.awaiting_next = false;
                            self.promote_next();
                        }
                    }
                    Err(e) => {
                        self.next_key = None;
                        self.awaiting_next = false;
                        self.report(BackendReport::Error {
                            key,
                            message: e.to_string(),
                            fatal: true,
                        });
                        if self.current.is_none() {
                            self.playing = false;
                        }
                    }
                }
            }
            Role::PreBuffer => {
                if generation != self.pre_buffer_generation
                    || self.pre_buffer_key.as_ref() != Some(&key)
                {
                    return;
                }
                match result {
                    Ok(l) => {
                        self.pre_buffer = Some(Source::new(l));
                        self.report(BackendReport::PreBufferReady { key });
                    }
                    Err(e) => {
                        self.pre_buffer_key = None;
                        self.report(BackendReport::Error {
                            key,
                            message: e.to_string(),
                            fatal: false,
                        });
                    }
                }
            }
        }
    }

    /// Install `src` as current: DSP format and gain, exclusive rate.
    fn begin_source(&mut self, src: Source) {
        self.dsp
            .set_format(f64::from(src.spec.sample_rate), src.spec.channels);
        self.dsp.begin_track_gain_db(src.media.gain_db);
        self.current = Some(src);
        self.maybe_switch_rate();
        // Fade in from silence so a hard start never clicks.
        self.volume.snap(0.0);
        if !self.fading_out {
            self.volume.set_target(self.user_volume);
        }
    }

    fn promote_next(&mut self) {
        let Some(next) = self.next.take() else { return };
        self.next_key = None;
        let key = next.key().clone();
        self.dsp
            .set_format(f64::from(next.spec.sample_rate), next.spec.channels);
        self.dsp.begin_track_gain_db(next.media.gain_db);
        self.current = Some(next);
        self.maybe_switch_rate();
        self.report(BackendReport::TransitionedToNext { key: key.clone() });
        self.report(BackendReport::Position {
            key,
            position_ms: 0,
        });
        self.last_position_report = Instant::now();
    }

    // -- positions --------------------------------------------------------

    fn pending_src_frames(&self, src: &Source) -> u64 {
        let ring_out_frames = (self.ring.ring.len() / self.out_channels.max(1)) as u64
            + (self.pending_out.len() / self.out_channels.max(1)) as u64;
        let ratio = f64::from(src.spec.sample_rate) / f64::from(self.out_rate.max(1));
        let ring_src = (ring_out_frames as f64 * ratio) as u64;
        ring_src
            + src
                .resampler
                .as_ref()
                .map(|r| r.latency_in_frames() as u64)
                .unwrap_or(0)
    }

    fn current_position(&self) -> u32 {
        match &self.current {
            Some(c) => c.position_ms(self.pending_src_frames(c)),
            None => 0,
        }
    }

    // -- commands ---------------------------------------------------------

    fn handle(&mut self, cmd: Cmd) -> bool {
        match cmd {
            Cmd::Shutdown => return false,
            Cmd::Load {
                source,
                next,
                position_ms,
                play,
            } => self.load(source, next, position_ms, play),
            Cmd::SetNext(next) => {
                self.next = None;
                self.next_key = next.as_ref().map(|n| n.key.clone());
                self.awaiting_next = false;
                if let Some(n) = next {
                    self.spawn_load(Role::Next, self.generation, n, 0);
                }
            }
            Cmd::Play => {
                self.ensure_output();
                if self.playing && !self.fading_out {
                    return true;
                }
                self.playing = true;
                self.fading_out = false;
                self.volume.set_target(self.user_volume);
                if let Some(c) = &self.current {
                    let key = c.key().clone();
                    if c.finished {
                        // Replaying a finished track from its end: restart it.
                        self.seek(0);
                    }
                    let pos = self.current_position();
                    self.report(BackendReport::Playing {
                        key,
                        position_ms: pos,
                    });
                }
            }
            Cmd::Pause => {
                if !self.playing {
                    return true;
                }
                if self.current.is_none() {
                    self.playing = false;
                    return true;
                }
                self.fading_out = true;
                self.volume.set_target(0.0);
            }
            Cmd::Stop => {
                self.generation += 1;
                self.loading_current = false;
                self.current = None;
                self.next = None;
                self.next_key = None;
                self.awaiting_next = false;
                self.pre_buffer = None;
                self.pre_buffer_key = None;
                self.playing = false;
                self.fading_out = false;
                self.pending_out.clear();
                self.ring.request_flush();
                self.dsp.reset();
            }
            Cmd::Seek(ms) => self.seek(ms),
            Cmd::SetVolume(v) => {
                self.user_volume = v;
                if !self.fading_out {
                    self.volume.set_target(v);
                }
            }
            Cmd::PreBuffer {
                source,
                position_ms,
            } => {
                self.pre_buffer = None;
                self.pre_buffer_generation += 1;
                self.pre_buffer_key = Some(source.key.clone());
                self.pre_buffer_position_ms = position_ms;
                self.spawn_load(
                    Role::PreBuffer,
                    self.pre_buffer_generation,
                    source,
                    position_ms,
                );
            }
            Cmd::DiscardPreBuffer => {
                self.pre_buffer = None;
                self.pre_buffer_key = None;
                self.pre_buffer_generation += 1;
            }
            Cmd::SetGapless(g) => self.gapless = g,
            Cmd::SetOutputDevice(id) => {
                let cfg = OutputConfig {
                    device_id: id,
                    ..self.output_config.clone()
                };
                if let Err(e) = self.open_output(cfg) {
                    tracing::warn!(target: "hocket::audio::native", error = %e, "switching output device failed, keeping default");
                    let cfg = OutputConfig {
                        device_id: None,
                        ..self.output_config.clone()
                    };
                    if self.open_output(cfg).is_err() {
                        self.output = None;
                        self.ensure_output();
                    }
                }
            }
            Cmd::SetExclusive(x) => {
                self.exclusive = x;
                self.maybe_switch_rate();
            }
            Cmd::SetDsp(s) => self.dsp.set_settings(s),
            Cmd::Loaded {
                role,
                generation,
                key,
                result,
            } => self.on_loaded(role, generation, key, result),
        }
        true
    }

    fn load(
        &mut self,
        source: MediaSource,
        next: Option<MediaSource>,
        position_ms: u32,
        play: bool,
    ) {
        self.ensure_output();
        self.generation += 1;
        self.current = None;
        self.next = None;
        self.awaiting_next = false;
        self.pending_out.clear();
        self.ring.request_flush();
        self.dsp.reset();
        self.playing = play;
        self.fading_out = false;
        self.volume
            .set_target(if play { self.user_volume } else { 0.0 });
        self.next_key = next.as_ref().map(|n| n.key.clone());
        if let Some(n) = next {
            self.spawn_load(Role::Next, self.generation, n, 0);
        }
        // Reuse the handoff pre-buffer when it is the same item nearby.
        let reuse = self.pre_buffer_key.as_ref() == Some(&source.key)
            && self.pre_buffer.is_some()
            && self.pre_buffer_position_ms.abs_diff(position_ms) <= PREBUFFER_REUSE_TOLERANCE_MS;
        if reuse {
            let mut pre = self.pre_buffer.take().expect("checked");
            self.pre_buffer_key = None;
            pre.media = source;
            if self.pre_buffer_position_ms != position_ms {
                let frame = pre.ms_to_frames(position_ms);
                pre.handle.seek(frame);
                pre.start_frame = frame;
                pre.pushed_frames = 0;
            }
            let key = pre.key().clone();
            let duration = pre.duration_ms;
            self.begin_source(pre);
            self.report(BackendReport::Ready {
                key: key.clone(),
                duration_ms: duration,
            });
            if play {
                self.report(BackendReport::Playing { key, position_ms });
            } else {
                self.report(BackendReport::Paused { key, position_ms });
            }
            return;
        }
        self.loading_current = true;
        self.spawn_load(Role::Current, self.generation, source, position_ms);
    }

    fn seek(&mut self, ms: u32) {
        let Some(cur) = self.current.as_mut() else {
            return;
        };
        let ms = cur.duration_ms.map(|d| ms.min(d)).unwrap_or(ms);
        let frame = cur.ms_to_frames(ms);
        cur.handle.seek(frame);
        cur.start_frame = frame;
        cur.pushed_frames = 0;
        cur.finished = false;
        cur.buffering = false;
        if let Some(r) = cur.resampler.as_mut() {
            r.reset();
        }
        let key = cur.key().clone();
        self.pending_out.clear();
        self.ring.request_flush();
        self.dsp.reset();
        // Ramp back in so the discontinuity doesn't click.
        self.volume.snap(0.0);
        self.volume.set_target(if self.fading_out {
            0.0
        } else if self.playing {
            self.user_volume
        } else {
            0.0
        });
        if !self.playing {
            self.volume.snap(self.user_volume);
        }
        self.report(BackendReport::Position {
            key,
            position_ms: ms,
        });
        self.last_position_report = Instant::now();
    }

    // -- feeding ----------------------------------------------------------

    /// Push converted audio while there is room. Returns when the ring is
    /// full, the source is starved, or playback stopped.
    fn feed(&mut self) {
        if self.output.is_none() {
            return;
        }
        let out_ch = self.out_channels.max(1);
        loop {
            // Flush leftovers first.
            if !self.pending_out.is_empty() {
                let n = self.ring.ring.push(&self.pending_out);
                self.pending_out.drain(..n);
                if !self.pending_out.is_empty() {
                    return;
                }
            }
            if !(self.playing || self.fading_out) {
                return;
            }
            if self.fading_out && !self.volume.is_ramping() {
                // Fade finished: now actually pause.
                self.fading_out = false;
                self.playing = false;
                if let Some(key) = self.current.as_ref().map(|c| c.key().clone()) {
                    let pos = self.current_position();
                    self.report(BackendReport::Paused {
                        key,
                        position_ms: pos,
                    });
                }
                return;
            }
            if self.current.is_none() {
                if !self.awaiting_next && !self.loading_current {
                    self.playing = false;
                    self.fading_out = false;
                }
                return;
            }
            match self.feed_one_chunk(out_ch) {
                Step::Continue => {}
                Step::Wait => return,
            }
        }
    }

    /// One iteration of [`Self::feed`] with a current source present.
    fn feed_one_chunk(&mut self, out_ch: usize) -> Step {
        let (src_rate, src_channels) = {
            let cur = self.current.as_ref().expect("caller checked");
            (cur.spec.sample_rate, cur.spec.channels)
        };
        // Room for one chunk at the worst-case ratio.
        let ratio = f64::from(self.out_rate) / f64::from(src_rate);
        let need = ((CHUNK_FRAMES as f64 * ratio) as usize + 64) * out_ch;
        if self.ring.ring.free() < need {
            return Step::Wait;
        }
        self.scratch_src.clear();
        let mut reports: Vec<BackendReport> = Vec::new();
        let n = {
            let cur = self.current.as_mut().expect("caller checked");
            let (n, _) = cur.handle.queue.pop(CHUNK_FRAMES, &mut self.scratch_src);
            n
        };
        if n == 0 {
            let cur = self.current.as_mut().expect("caller checked");
            if let Some(e) = cur.handle.queue.error() {
                let key = cur.key().clone();
                self.current = None;
                self.playing = false;
                self.fading_out = false;
                self.report(BackendReport::Error {
                    key,
                    message: e.to_string(),
                    fatal: true,
                });
                return Step::Wait;
            }
            if cur.handle.queue.is_eof() {
                if !cur.finished {
                    cur.finished = true;
                    if let Some(r) = cur.resampler.as_mut() {
                        let mut tail = Vec::new();
                        if r.flush(&mut tail).is_ok() {
                            self.volume.process(&mut tail, out_ch);
                            self.pending_out.extend_from_slice(&tail);
                        }
                    }
                }
                if !self.pending_out.is_empty() {
                    return Step::Continue;
                }
                self.finish_current();
                return if self.current.is_some() {
                    Step::Continue
                } else {
                    Step::Wait
                };
            }
            if !cur.buffering {
                cur.buffering = true;
                let key = cur.key().clone();
                self.report(BackendReport::Buffering {
                    key,
                    buffering: true,
                });
            }
            return Step::Wait;
        }
        {
            let cur = self.current.as_mut().expect("caller checked");
            if cur.buffering {
                cur.buffering = false;
                reports.push(BackendReport::Buffering {
                    key: cur.key().clone(),
                    buffering: false,
                });
            }
            cur.pushed_frames += n as u64;
        }
        self.dsp.process(&mut self.scratch_src);
        self.scratch_map.clear();
        remap_channels(
            &self.scratch_src,
            src_channels,
            out_ch,
            &mut self.scratch_map,
        );
        let mut converted = std::mem::take(&mut self.pending_out);
        if src_rate != self.out_rate {
            let fmt = (self.out_rate, out_ch);
            let cur = self.current.as_mut().expect("caller checked");
            if cur.resampler_for != Some(fmt) {
                cur.resampler = Resampler::new(src_rate, self.out_rate, out_ch).ok();
                cur.resampler_for = Some(fmt);
            }
            match cur.resampler.as_mut() {
                Some(r) => {
                    let before = converted.len();
                    if let Err(e) = r.process(&self.scratch_map, &mut converted) {
                        tracing::warn!(target: "hocket::audio::native", error = %e, "resampling failed, passing through");
                        converted.truncate(before);
                        converted.extend_from_slice(&self.scratch_map);
                    }
                }
                None => converted.extend_from_slice(&self.scratch_map),
            }
        } else {
            converted.extend_from_slice(&self.scratch_map);
        }
        self.volume.process(&mut converted, out_ch);
        self.pending_out = converted;
        for r in reports {
            self.report(r);
        }
        Step::Continue
    }

    /// The current source has played out: report and move on.
    fn finish_current(&mut self) {
        let Some(cur) = self.current.take() else {
            return;
        };
        self.report(BackendReport::Ended {
            key: cur.key().clone(),
        });
        drop(cur);
        if self.next.is_some() {
            self.promote_next();
        } else if self.next_key.is_some() {
            // Next is still loading: transition as soon as it lands.
            self.awaiting_next = true;
        } else {
            self.playing = false;
            self.fading_out = false;
        }
    }

    fn tick(&mut self) {
        if self.error_flag.swap(false, Ordering::AcqRel) {
            tracing::warn!(target: "hocket::audio::native", "output stream reported an error, reopening");
            self.output = None;
            self.ensure_output();
        }
        if self.playing && !self.fading_out {
            if let Some(c) = &self.current {
                if self.last_position_report.elapsed() >= POSITION_INTERVAL {
                    let key = c.key().clone();
                    let pos = self.current_position();
                    self.report(BackendReport::Position {
                        key,
                        position_ms: pos,
                    });
                    self.last_position_report = Instant::now();
                }
            }
        }
    }

    fn run(mut self) {
        self.ensure_output();
        loop {
            match self.rx.recv_timeout(Duration::from_millis(5)) {
                Ok(cmd) => {
                    if !self.handle(cmd) {
                        break;
                    }
                    while let Ok(cmd) = self.rx.try_recv() {
                        if !self.handle(cmd) {
                            return;
                        }
                    }
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => break,
            }
            self.feed();
            self.tick();
        }
    }
}

/// Open and start decoding one source at `position_ms`.
fn load_source(
    source: &MediaSource,
    position_ms: u32,
    gapless: bool,
    queue_seconds: u32,
    fetcher: Arc<dyn RangeFetcher>,
    runtime: tokio::runtime::Handle,
    name: &str,
) -> Result<DecodeHandle, DecodeError> {
    let url = source.url.trim();
    let opened = if let Some(path) = local_path(url) {
        if gapless {
            open_file(&path)?
        } else {
            let file = std::fs::File::open(&path)?;
            let ext = path
                .extension()
                .and_then(|e| e.to_str())
                .map(str::to_string);
            open_media_source(
                Box::new(file),
                ext.as_deref(),
                source.mime_type.as_deref(),
                false,
            )?
        }
    } else {
        let http = HttpSource::open(
            fetcher,
            runtime,
            url.to_string(),
            source.headers.clone(),
            OPEN_TIMEOUT,
        )?;
        let ext = url_extension(url);
        open_media_source(
            Box::new(http),
            ext.as_deref(),
            source.mime_type.as_deref(),
            gapless,
        )?
    };
    let start_frame = u64::from(position_ms) * u64::from(opened.spec.sample_rate) / 1000;
    let capacity = (opened.spec.sample_rate * queue_seconds) as usize;
    DecodeHandle::spawn(opened, start_frame, capacity, name)
}

/// `file://` URLs and bare paths → a filesystem path.
pub fn local_path(url: &str) -> Option<std::path::PathBuf> {
    if let Ok(u) = url::Url::parse(url) {
        if u.scheme() == "file" {
            return u.to_file_path().ok();
        }
        return None;
    }
    if url.starts_with('/') || url.starts_with("./") || url.chars().nth(1) == Some(':') {
        return Some(std::path::PathBuf::from(url));
    }
    None
}

fn url_extension(url: &str) -> Option<String> {
    let u = url::Url::parse(url).ok()?;
    let seg = u.path_segments()?.next_back()?.to_string();
    let (_, ext) = seg.rsplit_once('.')?;
    if ext.is_empty() || ext.len() > 5 {
        return None;
    }
    Some(ext.to_ascii_lowercase())
}

/// The native backend handle. See the module docs.
pub struct NativeBackend {
    tx: Sender<Cmd>,
    thread: Mutex<Option<thread::JoinHandle<()>>>,
    devices: Arc<Mutex<Vec<OutputDevice>>>,
    watcher: Mutex<Option<DeviceWatcher>>,
}

impl NativeBackend {
    pub fn new(config: NativeConfig, sink: ReportSink) -> Result<Self, BackendError> {
        let (tx, rx) = mpsc::channel();
        let initial_devices = if config.output.null_sink {
            Vec::new()
        } else {
            list_output_devices()
        };
        let devices = Arc::new(Mutex::new(initial_devices.clone()));
        let watcher = if config.watch_devices {
            let s = sink.clone();
            let d = devices.clone();
            Some(DeviceWatcher::start(
                Duration::from_secs(2),
                initial_devices,
                move |list| {
                    *d.lock() = list.clone();
                    s(BackendReport::OutputDevicesChanged { devices: list });
                },
            ))
        } else {
            None
        };
        let params = EngineParams {
            rx,
            tx: tx.clone(),
            sink,
            fetcher: config.fetcher,
            runtime: config.runtime,
            output: OutputConfig {
                device_id: config.audio.output_device.clone(),
                ..config.output
            },
            dsp: DspSettings::from(&config.audio),
            gapless: config.audio.gapless,
            exclusive: config.audio.exclusive,
        };
        let thread = thread::Builder::new()
            .name("hocket-audio-engine".into())
            .spawn(move || Engine::new(params).run())
            .map_err(|e| BackendError::Other(e.to_string()))?;
        Ok(Self {
            tx,
            thread: Mutex::new(Some(thread)),
            devices,
            watcher: Mutex::new(watcher),
        })
    }

    fn send(&self, cmd: Cmd) -> Result<(), BackendError> {
        self.tx.send(cmd).map_err(|_| BackendError::NotRunning)
    }

    /// Apply new DSP / gapless / exclusive settings.
    pub fn set_audio_settings(&self, settings: &AudioSettings) -> Result<(), BackendError> {
        self.send(Cmd::SetDsp(DspSettings::from(settings)))?;
        self.send(Cmd::SetGapless(settings.gapless))?;
        self.send(Cmd::SetExclusive(settings.exclusive))
    }

    /// Re-enumerate devices now (`Command::RefreshOutputDevices`).
    pub fn refresh_devices(&self) -> Vec<OutputDevice> {
        let list = match self.watcher.lock().as_ref() {
            Some(w) => w.refresh(),
            None => list_output_devices(),
        };
        *self.devices.lock() = list.clone();
        list
    }
}

impl Drop for NativeBackend {
    fn drop(&mut self) {
        let _ = self.tx.send(Cmd::Shutdown);
        self.watcher.lock().take();
        if let Some(t) = self.thread.lock().take() {
            let _ = t.join();
        }
    }
}

impl PlaybackBackend for NativeBackend {
    fn load(
        &self,
        source: MediaSource,
        next: Option<MediaSource>,
        position_ms: u32,
        play: bool,
    ) -> Result<(), BackendError> {
        self.send(Cmd::Load {
            source,
            next,
            position_ms,
            play,
        })
    }
    fn set_next(&self, next: Option<MediaSource>) -> Result<(), BackendError> {
        self.send(Cmd::SetNext(next))
    }
    fn play(&self) -> Result<(), BackendError> {
        self.send(Cmd::Play)
    }
    fn pause(&self) -> Result<(), BackendError> {
        self.send(Cmd::Pause)
    }
    fn stop(&self) -> Result<(), BackendError> {
        self.send(Cmd::Stop)
    }
    fn seek(&self, position_ms: u32) -> Result<(), BackendError> {
        self.send(Cmd::Seek(position_ms))
    }
    fn set_volume(&self, volume: f64) -> Result<(), BackendError> {
        self.send(Cmd::SetVolume(clamp_volume(volume)))
    }
    fn pre_buffer(&self, source: MediaSource, position_ms: u32) -> Result<(), BackendError> {
        self.send(Cmd::PreBuffer {
            source,
            position_ms,
        })
    }
    fn discard_pre_buffer(&self) -> Result<(), BackendError> {
        self.send(Cmd::DiscardPreBuffer)
    }
    fn set_gapless(&self, enabled: bool) -> Result<(), BackendError> {
        self.send(Cmd::SetGapless(enabled))
    }
    fn set_output_device(&self, id: Option<String>) -> Result<(), BackendError> {
        if let Some(id) = &id {
            let known = self.devices.lock().iter().any(|d| &d.id == id)
                || self.refresh_devices().iter().any(|d| &d.id == id);
            if !known {
                return Err(BackendError::UnknownDevice(id.clone()));
            }
        }
        self.send(Cmd::SetOutputDevice(id))
    }
    fn output_devices(&self) -> Vec<OutputDevice> {
        self.devices.lock().clone()
    }
    fn set_exclusive(&self, exclusive: bool) -> Result<(), BackendError> {
        self.send(Cmd::SetExclusive(exclusive))
    }
    fn name(&self) -> &'static str {
        "native"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::TrackSummary;
    use crate::audio::native::decoder::tests::write_test_wav;
    use crate::audio::native::http::MemoryFetcher;
    use crate::core::default_audio_settings;

    type Reports = Arc<Mutex<Vec<BackendReport>>>;

    struct Rig {
        backend: NativeBackend,
        reports: Reports,
        _rt: tokio::runtime::Runtime,
        dir: tempfile::TempDir,
    }

    fn rig_with(fetcher: Arc<dyn RangeFetcher>) -> Rig {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()
            .unwrap();
        let reports: Reports = Default::default();
        let r2 = reports.clone();
        let backend = NativeBackend::new(
            NativeConfig {
                fetcher,
                runtime: rt.handle().clone(),
                output: OutputConfig {
                    null_sink: true,
                    ..Default::default()
                },
                audio: default_audio_settings(),
                watch_devices: false,
            },
            Arc::new(move |r| r2.lock().push(r)),
        )
        .unwrap();
        Rig {
            backend,
            reports,
            _rt: rt,
            dir: tempfile::tempdir().unwrap(),
        }
    }

    fn rig() -> Rig {
        rig_with(Arc::new(MemoryFetcher::new(Vec::new())))
    }

    impl Rig {
        fn wav(&self, name: &str, rate: u32, frames: u32) -> MediaSource {
            let p = self.dir.path().join(name);
            write_test_wav(&p, rate, frames);
            MediaSource {
                key: name.into(),
                track: TrackSummary {
                    id: name.into(),
                    duration_ms: frames * 1000 / rate,
                    ..Default::default()
                },
                url: url::Url::from_file_path(&p).unwrap().to_string(),
                headers: Default::default(),
                mime_type: Some("audio/wav".into()),
                gain_db: 0.0,
                transcoded: false,
            }
        }

        fn wait_for(
            &self,
            timeout: Duration,
            pred: impl Fn(&BackendReport) -> bool,
        ) -> Vec<BackendReport> {
            let deadline = Instant::now() + timeout;
            loop {
                {
                    let r = self.reports.lock();
                    if r.iter().any(&pred) {
                        return r.clone();
                    }
                }
                assert!(
                    Instant::now() < deadline,
                    "timed out; reports: {:?}",
                    self.reports.lock()
                );
                thread::sleep(Duration::from_millis(5));
            }
        }

        fn take(&self) -> Vec<BackendReport> {
            std::mem::take(&mut *self.reports.lock())
        }
    }

    #[test]
    fn plays_a_wav_to_the_end_with_positions() {
        let rig = rig();
        let a = rig.wav("a.wav", 44_100, 44_100 * 3 / 2);
        rig.backend.load(a, None, 0, true).unwrap();
        let reports = rig.wait_for(Duration::from_secs(10), |r| {
            matches!(r, BackendReport::Ended { .. })
        });
        assert_eq!(
            reports[0],
            BackendReport::Ready {
                key: "a.wav".into(),
                duration_ms: Some(1500)
            }
        );
        assert!(
            matches!(&reports[1], BackendReport::Playing { key, position_ms: 0 } if key == "a.wav")
        );
        let positions: Vec<u32> = reports
            .iter()
            .filter_map(|r| {
                if let BackendReport::Position { position_ms, .. } = r {
                    Some(*position_ms)
                } else {
                    None
                }
            })
            .collect();
        assert!(
            !positions.is_empty(),
            "at least one 1 Hz position report: {reports:?}"
        );
        assert!(positions.windows(2).all(|w| w[1] >= w[0]));
        assert!(positions.iter().all(|p| *p <= 1500));
        assert!(!reports
            .iter()
            .any(|r| matches!(r, BackendReport::Error { .. })));
    }

    #[test]
    fn gapless_transition_to_next_then_ends() {
        let rig = rig();
        let a = rig.wav("a.wav", 48_000, 48_000 / 2);
        let b = rig.wav("b.wav", 44_100, 44_100 / 2);
        rig.backend.load(a, Some(b), 0, true).unwrap();
        let reports = rig.wait_for(
            Duration::from_secs(10),
            |r| matches!(r, BackendReport::Ended { key } if key == "b.wav"),
        );
        let idx =
            |pred: &dyn Fn(&BackendReport) -> bool| reports.iter().position(pred).expect("present");
        let ended_a = idx(&|r| matches!(r, BackendReport::Ended { key } if key == "a.wav"));
        let trans =
            idx(&|r| matches!(r, BackendReport::TransitionedToNext { key } if key == "b.wav"));
        let pos0 = idx(
            &|r| matches!(r, BackendReport::Position { key, position_ms: 0 } if key == "b.wav"),
        );
        assert!(ended_a < trans && trans < pos0, "{reports:?}");
        assert!(
            !reports
                .iter()
                .any(|r| matches!(r, BackendReport::Error { .. })),
            "{reports:?}"
        );
    }

    #[test]
    fn pause_seek_resume_and_stop() {
        let rig = rig();
        let a = rig.wav("a.wav", 48_000, 48_000 * 4);
        rig.backend.load(a, None, 0, true).unwrap();
        rig.wait_for(Duration::from_secs(5), |r| {
            matches!(r, BackendReport::Playing { .. })
        });
        thread::sleep(Duration::from_millis(300));
        rig.backend.pause().unwrap();
        let reports = rig.wait_for(Duration::from_secs(5), |r| {
            matches!(r, BackendReport::Paused { .. })
        });
        let paused_at = reports
            .iter()
            .find_map(|r| {
                if let BackendReport::Paused { position_ms, .. } = r {
                    Some(*position_ms)
                } else {
                    None
                }
            })
            .unwrap();
        assert!((100..=1000).contains(&paused_at), "{paused_at}");
        rig.take();
        rig.backend.seek(2_500).unwrap();
        let reports = rig.wait_for(Duration::from_secs(5), |r| {
            matches!(
                r,
                BackendReport::Position {
                    position_ms: 2_500,
                    ..
                }
            )
        });
        assert!(reports
            .iter()
            .all(|r| !matches!(r, BackendReport::Playing { .. })));
        rig.take();
        rig.backend.play().unwrap();
        let reports = rig.wait_for(Duration::from_secs(5), |r| {
            matches!(r, BackendReport::Playing { .. })
        });
        let resumed_at = reports
            .iter()
            .find_map(|r| {
                if let BackendReport::Playing { position_ms, .. } = r {
                    Some(*position_ms)
                } else {
                    None
                }
            })
            .unwrap();
        assert!((2_400..=2_700).contains(&resumed_at), "{resumed_at}");
        rig.backend.set_volume(0.5).unwrap();
        rig.backend.stop().unwrap();
        thread::sleep(Duration::from_millis(200));
        rig.take();
        thread::sleep(Duration::from_millis(1200));
        assert!(rig.take().is_empty(), "nothing reports after stop");
    }

    #[test]
    fn pre_buffer_is_reused_by_a_matching_load() {
        let rig = rig();
        let a = rig.wav("a.wav", 48_000, 48_000 * 2);
        rig.backend.pre_buffer(a.clone(), 500).unwrap();
        rig.wait_for(
            Duration::from_secs(5),
            |r| matches!(r, BackendReport::PreBufferReady { key } if key == "a.wav"),
        );
        rig.take();
        rig.backend.load(a, None, 700, true).unwrap();
        let reports = rig.wait_for(Duration::from_secs(5), |r| {
            matches!(r, BackendReport::Playing { .. })
        });
        assert_eq!(
            reports[0],
            BackendReport::Ready {
                key: "a.wav".into(),
                duration_ms: Some(2000)
            }
        );
        assert_eq!(
            reports[1],
            BackendReport::Playing {
                key: "a.wav".into(),
                position_ms: 700
            }
        );
        rig.wait_for(Duration::from_secs(5), |r| {
            matches!(r, BackendReport::Ended { .. })
        });
        // Discarded pre-buffers never report.
        let b = rig.wav("b.wav", 48_000, 48_000);
        rig.backend.pre_buffer(b, 0).unwrap();
        rig.backend.discard_pre_buffer().unwrap();
        thread::sleep(Duration::from_millis(300));
        assert!(!rig
            .reports
            .lock()
            .iter()
            .any(|r| matches!(r, BackendReport::PreBufferReady { key } if key == "b.wav")));
    }

    #[test]
    fn unreadable_source_reports_a_fatal_error_and_never_ends() {
        let rig = rig();
        let mut bad = rig.wav("a.wav", 48_000, 100);
        bad.key = "bad".into();
        bad.url = url::Url::from_file_path(rig.dir.path().join("missing.wav"))
            .unwrap()
            .to_string();
        rig.backend.load(bad, None, 0, true).unwrap();
        let reports = rig.wait_for(Duration::from_secs(5), |r| {
            matches!(r, BackendReport::Error { .. })
        });
        assert!(
            matches!(&reports[0], BackendReport::Error { key, fatal: true, .. } if key == "bad")
        );
        assert!(!reports
            .iter()
            .any(|r| matches!(r, BackendReport::Ended { .. } | BackendReport::Ready { .. })));
    }

    #[test]
    fn streams_over_http_through_the_fetcher_seam() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("h.wav");
        write_test_wav(&p, 48_000, 48_000 / 2);
        let bytes = std::fs::read(&p).unwrap();
        let fetcher = MemoryFetcher {
            chunk: 2048,
            delay: Duration::from_millis(1),
            ..MemoryFetcher::new(bytes)
        };
        let rig = rig_with(Arc::new(fetcher));
        let src = MediaSource {
            key: "h".into(),
            track: TrackSummary {
                id: "h".into(),
                duration_ms: 500,
                ..Default::default()
            },
            url: "https://navidrome.example/rest/stream?id=h&format=wav".into(),
            headers: Default::default(),
            mime_type: Some("audio/wav".into()),
            gain_db: -6.0,
            transcoded: false,
        };
        rig.backend.load(src, None, 100, true).unwrap();
        let reports = rig.wait_for(
            Duration::from_secs(10),
            |r| matches!(r, BackendReport::Ended { key } if key == "h"),
        );
        assert_eq!(
            reports[0],
            BackendReport::Ready {
                key: "h".into(),
                duration_ms: Some(500)
            }
        );
        assert_eq!(
            reports[1],
            BackendReport::Playing {
                key: "h".into(),
                position_ms: 100
            }
        );
        assert!(
            !reports
                .iter()
                .any(|r| matches!(r, BackendReport::Error { .. })),
            "{reports:?}"
        );
    }

    #[test]
    fn unknown_device_is_rejected_synchronously() {
        let rig = rig();
        assert_eq!(
            rig.backend.set_output_device(Some("no such device".into())),
            Err(BackendError::UnknownDevice("no such device".into()))
        );
        assert!(rig.backend.set_output_device(None).is_ok());
        assert_eq!(rig.backend.name(), "native");
        rig.backend
            .set_audio_settings(&default_audio_settings())
            .unwrap();
        rig.backend.set_gapless(false).unwrap();
    }

    #[test]
    fn url_helpers() {
        assert!(local_path("file:///tmp/x.flac").is_some());
        assert!(local_path("/tmp/x.flac").is_some());
        assert!(local_path("https://x/y.mp3").is_none());
        assert_eq!(url_extension("https://x/y/z.MP3?x=1"), Some("mp3".into()));
        assert_eq!(url_extension("https://x/rest/stream?id=1"), None);
    }
}
