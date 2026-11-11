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

use crate::api::TransitionPlan;
use crate::api::{AudioSettings, BackendReport, MediaSource, OutputDevice, QueueKey};
use crate::audio::backend::{clamp_volume, BackendError, PlaybackBackend, ReportSink};
use crate::audio::decoder::{open_file, open_media_source, DecodeError, DecodeHandle, Spec};
use crate::audio::dsp::{DspChain, DspSettings, SmoothGain, SoftLimiter};
use crate::audio::native::http::{HttpSource, RangeFetcher};
use crate::audio::native::mix::{
    render_tail_job, RenderedTail, TailJob, TailState, Transition, SWITCH_XFADE_MS, TAIL_LEAD_MS,
};
use crate::audio::native::output::{
    list_output_devices, open_output, probe_format, DeviceWatcher, OutputConfig, OutputRing,
    OutputStream,
};
use crate::audio::native::resample::{remap_channels, Resampler, CHUNK_FRAMES};
use crate::automix::render::ms_to_frames;
use crate::automix::IncomingShaper;

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
    CompleteTransition,
    TailRendered {
        id: u64,
        result: Result<Vec<f32>, String>,
    },
    Loaded {
        role: Role,
        generation: u64,
        key: QueueKey,
        result: Result<Box<Loaded>, DecodeError>,
    },
    Shutdown,
}

pub(super) struct Source {
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
    /// Output-format frames converted from this source since `start_frame`.
    out_emitted: u64,
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
            out_emitted: 0,
            buffering: false,
            finished: false,
        }
    }

    fn key(&self) -> &QueueKey {
        &self.media.key
    }

    /// Restart decoding at `frame` (seek, or repositioning a follow-up).
    fn rewind(&mut self, frame: u64) {
        self.handle.seek(frame);
        self.start_frame = frame;
        self.pushed_frames = 0;
        self.out_emitted = 0;
        self.finished = false;
        self.buffering = false;
        if let Some(r) = self.resampler.as_mut() {
            r.reset();
        }
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
    /// Bumped by every `SetNext` and `Load`: two `SetNext`s can carry the same
    /// key for different tracks (an upcoming context item's key is its
    /// index), so the key alone does not tell a superseded follow-up load.
    next_generation: u64,
    pre_buffer_generation: u64,
    /// Output-format samples converted but not yet pushed.
    pending_out: Vec<f32>,
    scratch_src: Vec<f32>,
    scratch_map: Vec<f32>,
    last_position_report: Instant,
    error_flag: Arc<AtomicBool>,
    /// AutoMix: the tail for current → next, and the transition playing.
    tail: TailState,
    /// Stops the render job of the tail in `tail` when it is dropped.
    tail_cancel: Arc<AtomicBool>,
    /// Bumped whenever a tail is dropped, so a late render is ignored.
    tail_id: u64,
    transition: Option<Transition>,
    /// The follow-up's plan was given up on (tail late, seek past it):
    /// join it gaplessly. Cleared by `set_next`, `load` and seeks.
    mix_skipped: bool,
    /// On the summed signal while two sources sound.
    limiter: SoftLimiter,
}

/// Pop up to a chunk of `src`, run it through `dsp`, adapt channels and
/// resample to the output format, appending to `out`. Returns the source
/// frames taken (0: nothing queued).
#[allow(clippy::too_many_arguments)]
fn convert_chunk(
    src: &mut Source,
    dsp: &mut DspChain,
    out_rate: u32,
    out_ch: usize,
    scratch_src: &mut Vec<f32>,
    scratch_map: &mut Vec<f32>,
    out: &mut Vec<f32>,
) -> usize {
    scratch_src.clear();
    let (n, _) = src.handle.queue.pop(CHUNK_FRAMES, scratch_src);
    if n == 0 {
        return 0;
    }
    src.pushed_frames += n as u64;
    dsp.process(scratch_src);
    scratch_map.clear();
    remap_channels(scratch_src, src.spec.channels, out_ch, scratch_map);
    let before = out.len();
    resample_into(src, out_rate, out_ch, scratch_map, out);
    src.out_emitted += ((out.len() - before) / out_ch) as u64;
    n
}

/// Resample `input` (output channel count, source rate) into `out` with
/// the source's own resampler, created on first use for this format.
fn resample_into(
    src: &mut Source,
    out_rate: u32,
    out_ch: usize,
    input: &[f32],
    out: &mut Vec<f32>,
) {
    let src_rate = src.spec.sample_rate;
    if src_rate == out_rate {
        out.extend_from_slice(input);
        return;
    }
    let fmt = (out_rate, out_ch);
    if src.resampler_for != Some(fmt) {
        src.resampler = Resampler::new(src_rate, out_rate, out_ch).ok();
        src.resampler_for = Some(fmt);
    }
    match src.resampler.as_mut() {
        Some(r) => {
            let before = out.len();
            if let Err(e) = r.process(input, out) {
                tracing::warn!(target: "hocket::audio::native", error = %e, "resampling failed, passing through");
                out.truncate(before);
                out.extend_from_slice(input);
            }
        }
        None => out.extend_from_slice(input),
    }
}

/// Flush the source's resampler at end of stream (once).
fn flush_source(src: &mut Source, out: &mut Vec<f32>) {
    if src.finished {
        return;
    }
    src.finished = true;
    if let Some(r) = src.resampler.as_mut() {
        let mut tail = Vec::new();
        if r.flush(&mut tail).is_ok() {
            out.extend_from_slice(&tail);
        }
    }
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
            next_generation: 0,
            pre_buffer_generation: 0,
            pending_out: Vec::new(),
            scratch_src: Vec::with_capacity(CHUNK_FRAMES * 8),
            scratch_map: Vec::with_capacity(CHUNK_FRAMES * 8),
            last_position_report: Instant::now(),
            error_flag: Arc::new(AtomicBool::new(false)),
            tail: TailState::None,
            tail_cancel: Arc::new(AtomicBool::new(false)),
            tail_id: 0,
            transition: None,
            mix_skipped: false,
            limiter: SoftLimiter::default(),
        }
    }

    fn report(&self, r: BackendReport) {
        tracing::trace!(target: "hocket::audio::native", report = ?r);
        (self.sink)(r);
    }

    // -- output -----------------------------------------------------------

    fn open_output(&mut self, config: OutputConfig) -> Result<(), String> {
        // A tail rendered for the old format can't be mixed into the new one.
        self.cut_transition("output changed");
        self.drop_tail();
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
                if generation != self.next_generation || self.next_key.as_ref() != Some(&key) {
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
        let Some(mut next) = self.next.take() else {
            return;
        };
        self.next_key = None;
        self.drop_tail();
        self.mix_skipped = false;
        // A gapless join starts B at 0, but a tail made ready may have moved
        // it to its entry point (the mix then didn't happen: exclusive output
        // turned on, the plan dropped).
        if next.start_frame != 0 || next.pushed_frames != 0 {
            next.rewind(0);
        }
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
        let ring_out_frames = self.queued_frames() as u64
            + self
                .transition
                .as_ref()
                .map_or(0, |t| (t.incoming.len() / self.out_channels.max(1)) as u64);
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

    /// Output frames pushed but not yet played.
    fn queued_frames(&self) -> usize {
        (self.ring.ring.len() + self.pending_out.len()) / self.out_channels.max(1)
    }

    /// The item the listener hears as "now playing" and its position: A
    /// until a transition's handover, then the current source.
    fn now_playing(&self) -> Option<(QueueKey, u32)> {
        if let Some(t) = self.transition.as_ref().filter(|t| !t.handed_over) {
            let heard = t.heard(self.queued_frames()).max(0);
            let ms = heard as f64 * 1000.0 / f64::from(self.out_rate.max(1));
            let plan = &t.tail.plan;
            let pos = f64::from(plan.tail_start_ms)
                + crate::automix::stretch::source_ms(plan.tempo.as_ref(), ms);
            return Some((t.from.clone(), pos as u32));
        }
        self.current
            .as_ref()
            .map(|c| (c.key().clone(), self.current_position()))
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
            Cmd::SetNext(next) => self.set_next(next),
            Cmd::Play => {
                self.ensure_output();
                if self.playing && !self.fading_out {
                    return true;
                }
                self.playing = true;
                self.fading_out = false;
                self.volume.set_target(self.user_volume);
                if self.current.as_ref().is_some_and(|c| c.finished) {
                    // Replaying a finished track from its end: restart it.
                    self.seek(0);
                }
                if let Some((key, pos)) = self.now_playing() {
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
                self.next_generation += 1;
                self.loading_current = false;
                self.current = None;
                self.next = None;
                self.next_key = None;
                self.awaiting_next = false;
                self.pre_buffer = None;
                self.pre_buffer_key = None;
                self.transition = None;
                self.drop_tail();
                self.mix_skipped = false;
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
            Cmd::SetDsp(s) => {
                if &s != self.dsp.settings() {
                    self.dsp.set_settings(s);
                    // A tail rendered (or rendering) with the old EQ and
                    // gain would play them for up to its length: render
                    // it again (a running transition keeps its own).
                    self.drop_tail();
                }
            }
            Cmd::CompleteTransition => self.cut_transition("completed"),
            Cmd::TailRendered { id, result } => self.on_tail_rendered(id, result),
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
        self.transition = None;
        self.drop_tail();
        self.mix_skipped = false;
        self.pending_out.clear();
        self.ring.request_flush();
        self.dsp.reset();
        self.playing = play;
        self.fading_out = false;
        self.volume
            .set_target(if play { self.user_volume } else { 0.0 });
        self.next_key = next.as_ref().map(|n| n.key.clone());
        self.next_generation += 1;
        if let Some(n) = next {
            self.spawn_load(Role::Next, self.next_generation, n, 0);
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
                pre.rewind(frame);
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
        // A seek cancels a transition: before the handover A plays on from
        // the seek (B goes back to being the follow-up), after it B does.
        if self.transition.as_ref().is_some_and(|t| t.handed_over) {
            self.transition = None;
        } else {
            self.cancel_transition();
        }
        self.mix_skipped = false;
        let Some(cur) = self.current.as_mut() else {
            return;
        };
        let ms = cur.duration_ms.map(|d| ms.min(d)).unwrap_or(ms);
        let frame = cur.ms_to_frames(ms);
        cur.rewind(frame);
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
                if let Some((key, pos)) = self.now_playing() {
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
        if self.transition.is_some() {
            return self.feed_transition(out_ch);
        }
        let src_rate = self
            .current
            .as_ref()
            .expect("caller checked")
            .spec
            .sample_rate;
        // Room for one chunk at the worst-case ratio.
        let ratio = f64::from(self.out_rate) / f64::from(src_rate);
        let need = ((CHUNK_FRAMES as f64 * ratio) as usize + 64) * out_ch;
        if self.ring.ring.free() < need {
            return Step::Wait;
        }
        let mut reports: Vec<BackendReport> = Vec::new();
        let mut converted = std::mem::take(&mut self.pending_out);
        let start = converted.len();
        let (n, before_out) = {
            let cur = self.current.as_mut().expect("caller checked");
            let before_out = cur.out_emitted;
            let n = convert_chunk(
                cur,
                &mut self.dsp,
                self.out_rate,
                out_ch,
                &mut self.scratch_src,
                &mut self.scratch_map,
                &mut converted,
            );
            (n, before_out)
        };
        if n == 0 {
            self.pending_out = converted;
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
                    let mut tail = Vec::new();
                    flush_source(cur, &mut tail);
                    self.volume.process(&mut tail, out_ch);
                    self.pending_out.extend_from_slice(&tail);
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
        }
        // AutoMix: does this chunk cross the tail start of a pending mix?
        let mut live = None;
        if let Some(switch) = self.switch_frame() {
            let produced = ((converted.len() - start) / out_ch) as i64;
            let before = before_out as i64;
            if before > switch {
                self.skip_mix("already past the tail start");
            } else if before + produced > switch {
                if self.tail_is_ready() {
                    let split = start + (switch - before) as usize * out_ch;
                    live = Some(converted.split_off(split));
                } else {
                    self.skip_mix("the tail isn't rendered in time");
                }
            }
        }
        self.volume.process(&mut converted[start..], out_ch);
        self.pending_out = converted;
        for r in reports {
            self.report(r);
        }
        if let Some(live) = live {
            self.begin_transition(live, out_ch);
        }
        Step::Continue
    }

    // -- AutoMix ----------------------------------------------------------

    fn drop_tail(&mut self) {
        self.tail = TailState::None;
        self.tail_id += 1;
        // A render still running stops early rather than decode up to
        // a minute of audio for nothing.
        self.tail_cancel.store(true, Ordering::Release);
    }

    /// The follow-up's plan, when a transition into it may still happen.
    fn pending_plan(&self) -> Option<&TransitionPlan> {
        if self.exclusive || self.mix_skipped || self.transition.is_some() {
            return None;
        }
        self.next.as_ref()?.media.mix_in.as_ref()
    }

    /// Output frame (counted from the current source's start frame, as
    /// `out_emitted` is) where the pending plan's tail starts.
    fn switch_frame(&self) -> Option<i64> {
        let plan = self.pending_plan()?;
        let cur = self.current.as_ref()?;
        let tail_src = cur.ms_to_frames(plan.tail_start_ms) as i64 - cur.start_frame as i64;
        let ratio = f64::from(self.out_rate) / f64::from(cur.spec.sample_rate);
        Some((tail_src as f64 * ratio).round() as i64)
    }

    /// The rendered tail matches the follow-up's plan and the output, and
    /// the follow-up sits at its entry point.
    fn tail_is_ready(&self) -> bool {
        let (Some(plan), Some(next)) = (self.pending_plan(), self.next.as_ref()) else {
            return false;
        };
        let TailState::Ready(t) = &self.tail else {
            return false;
        };
        t.next_generation == self.next_generation
            && t.rate == self.out_rate
            && t.channels == self.out_channels
            && &t.plan == plan
            && next.start_frame == next.ms_to_frames(plan.incoming_from_ms)
            && next.pushed_frames == 0
    }

    /// Give up on the pending plan: A plays to its end and B follows from
    /// its start, gaplessly. Never stalls playback.
    fn skip_mix(&mut self, why: &str) {
        if !self.mix_skipped {
            tracing::info!(target: "hocket::audio::native", reason = why, "AutoMix: plain cut instead of the planned transition");
        }
        self.mix_skipped = true;
        if let Some(n) = self.next.as_mut() {
            if n.start_frame != 0 || n.pushed_frames != 0 {
                n.rewind(0);
            }
        }
    }

    fn set_next(&mut self, next: Option<MediaSource>) {
        if let Some(t) = self.transition.as_mut().filter(|t| !t.handed_over) {
            // During the overlap: for the transition after this one.
            t.held_next = Some(next);
            return;
        }
        if let (Some(n), Some(cur)) = (next.as_ref(), self.next.as_ref()) {
            if &cur.media == n {
                return; // the same follow-up: keep its load and tail
            }
        }
        self.drop_tail();
        self.mix_skipped = false;
        self.next = None;
        self.next_key = next.as_ref().map(|n| n.key.clone());
        self.awaiting_next = false;
        self.next_generation += 1;
        if let Some(n) = next {
            self.spawn_load(Role::Next, self.next_generation, n, 0);
        }
    }

    /// Start rendering the tail once current and follow-up are both known
    /// and the tail start is near; position the follow-up once it's ready.
    fn maybe_prepare_tail(&mut self) {
        if self.output.is_none() || self.current.is_none() {
            return;
        }
        let Some(plan) = self.pending_plan().cloned() else {
            return;
        };
        match &self.tail {
            TailState::Rendering { .. } => return,
            TailState::Ready(t) => {
                if t.next_generation == self.next_generation
                    && t.rate == self.out_rate
                    && t.channels == self.out_channels
                    && t.plan == plan
                {
                    let next = self.next.as_mut().expect("pending_plan checked");
                    let frame = next.ms_to_frames(plan.incoming_from_ms);
                    if next.start_frame != frame || next.pushed_frames != 0 {
                        next.rewind(frame);
                    }
                    return;
                }
                self.drop_tail();
            }
            TailState::None => {}
        }
        let pos = self.current_position();
        if pos >= plan.tail_start_ms {
            self.skip_mix("already past the tail start");
            return;
        }
        if pos.saturating_add(TAIL_LEAD_MS) < plan.tail_start_ms {
            return;
        }
        let cur = self.current.as_ref().expect("checked");
        self.tail_id += 1;
        let id = self.tail_id;
        self.tail_cancel = Arc::new(AtomicBool::new(false));
        self.tail = TailState::Rendering {
            next_generation: self.next_generation,
            plan: plan.clone(),
            rate: self.out_rate,
            channels: self.out_channels,
        };
        let job = TailJob {
            source: cur.media.clone(),
            plan,
            dsp: self.dsp.clone(),
            gapless: self.gapless,
            out_rate: self.out_rate,
            out_channels: self.out_channels,
            fetcher: self.fetcher.clone(),
            runtime: self.runtime.clone(),
            cancel: self.tail_cancel.clone(),
        };
        let tx = self.tx.clone();
        let spawned = thread::Builder::new()
            .name("hocket-tail".into())
            .spawn(move || {
                let result = render_tail_job(job);
                let _ = tx.send(Cmd::TailRendered { id, result });
            });
        if let Err(e) = spawned {
            self.drop_tail();
            self.skip_mix(&e.to_string());
        }
    }

    fn on_tail_rendered(&mut self, id: u64, result: Result<Vec<f32>, String>) {
        if id != self.tail_id {
            return;
        }
        let TailState::Rendering {
            next_generation,
            plan,
            rate,
            channels,
        } = std::mem::replace(&mut self.tail, TailState::None)
        else {
            return;
        };
        match result {
            Ok(pcm) => {
                self.tail = TailState::Ready(RenderedTail {
                    plan,
                    next_generation,
                    rate,
                    channels,
                    pcm,
                });
                // Position the follow-up now rather than at the next tick.
                self.maybe_prepare_tail();
            }
            Err(e) => {
                tracing::warn!(target: "hocket::audio::native", error = %e, "AutoMix: rendering the tail failed");
                self.skip_mix("the tail couldn't be rendered");
            }
        }
    }

    /// At A's tail start: A's live output stops (after `live`, its first
    /// frames past the switch, crossfaded into the tail), B becomes the
    /// current source with its own DSP chain, and the mix begins.
    fn begin_transition(&mut self, mut live: Vec<f32>, out_ch: usize) {
        let TailState::Ready(mut tail) = std::mem::replace(&mut self.tail, TailState::None) else {
            return;
        };
        let (Some(mut a), Some(b)) = (self.current.take(), self.next.take()) else {
            return;
        };
        self.next_key = None;
        let plan = tail.plan.clone();
        let mut b_dsp = self.dsp.clone();
        b_dsp.set_format(f64::from(b.spec.sample_rate), b.spec.channels);
        b_dsp.begin_track_gain_db(b.media.gain_db);
        // As at a gapless join, the normaliser carries on at its gain; only
        // A's filter memory is dropped (B enters on its own).
        b_dsp.reset_filters();
        let mut a_dsp = std::mem::replace(&mut self.dsp, b_dsp);
        // The tail was rendered without the normaliser: hold live A's gain
        // over it, so the level carries on from the switch.
        let held_db = a_dsp.normaliser_gain_db();
        if held_db != 0.0 {
            let g = 10f64.powf(held_db / 20.0) as f32;
            tail.pcm.iter_mut().for_each(|v| *v *= g);
        }
        let rate = self.out_rate;
        let xfade = ms_to_frames(f64::from(SWITCH_XFADE_MS), rate);
        while live.len() / out_ch < xfade {
            let n = convert_chunk(
                &mut a,
                &mut a_dsp,
                rate,
                out_ch,
                &mut self.scratch_src,
                &mut self.scratch_map,
                &mut live,
            );
            if n == 0 {
                break;
            }
        }
        live.truncate(xfade * out_ch);
        tracing::debug!(target: "hocket::audio::native", from = %a.key(), to = %b.key(), kind = ?plan.kind, "AutoMix: transition started");
        self.transition = Some(Transition {
            from: a.key().clone(),
            to: b.key().clone(),
            frame: 0,
            incoming_at: ms_to_frames(f64::from(plan.incoming_at_ms.min(plan.tail_len_ms)), rate),
            handover: ms_to_frames(f64::from(plan.handover_ms), rate),
            live,
            outgoing: Some((a, a_dsp)),
            shaper: IncomingShaper::new(&plan, out_ch, rate),
            incoming: Vec::new(),
            incoming_done: false,
            started_reported: false,
            handed_over: false,
            held_next: None,
            tail,
        });
        self.current = Some(b);
    }

    /// One block of a running transition: tail (+ live A at the switch) +
    /// shaped B, limited, then volume.
    fn feed_transition(&mut self, out_ch: usize) -> Step {
        const BLOCK: usize = 1024;
        if self.ring.ring.free() < BLOCK * out_ch {
            return Step::Wait;
        }
        let mut t = self.transition.take().expect("caller checked");
        let start = t.frame;
        let end = start + BLOCK;
        let b_need = end.saturating_sub(start.max(t.incoming_at));
        // Buffering reports go out under the item the listener hears.
        let heard_key = if t.handed_over {
            t.to.clone()
        } else {
            t.from.clone()
        };
        while t.incoming.len() / out_ch < b_need && !t.incoming_done {
            let cur = self.current.as_mut().expect("B is current");
            let before = t.incoming.len();
            let n = convert_chunk(
                cur,
                &mut self.dsp,
                self.out_rate,
                out_ch,
                &mut self.scratch_src,
                &mut self.scratch_map,
                &mut t.incoming,
            );
            if n == 0 {
                if cur.handle.queue.error().is_some() {
                    // B failed: hand over so its error lands on the item
                    // now playing; the plain path reports it.
                    self.transition = Some(t);
                    self.hand_over(None, true);
                    self.transition = None;
                    return Step::Continue;
                }
                if !cur.handle.queue.is_eof() {
                    // B is late: wait for it rather than drop its audio, and
                    // say so, as the plain path does.
                    let report = !cur.buffering;
                    cur.buffering = true;
                    self.transition = Some(t);
                    if report {
                        self.report(BackendReport::Buffering {
                            key: heard_key,
                            buffering: true,
                        });
                    }
                    return Step::Wait;
                }
                flush_source(cur, &mut t.incoming);
                t.incoming_done = true;
            } else if cur.buffering {
                cur.buffering = false;
                self.report(BackendReport::Buffering {
                    key: heard_key.clone(),
                    buffering: false,
                });
            }
            t.shaper.process(&mut t.incoming[before..]);
        }

        let tail_frames = t.tail.frames();
        let live_frames = t.live.len() / out_ch;
        let mut block = vec![0.0f32; BLOCK * out_ch];
        let mut b_used = 0usize;
        for (i, o) in block.chunks_exact_mut(out_ch).enumerate() {
            let f = start + i;
            if f < tail_frames {
                o.copy_from_slice(&t.tail.pcm[f * out_ch..(f + 1) * out_ch]);
            }
            if f < live_frames {
                let w = (f + 1) as f32 / (live_frames + 1) as f32;
                for (c, s) in o.iter_mut().enumerate() {
                    *s = t.live[f * out_ch + c] * (1.0 - w) + *s * w;
                }
            }
            if f >= t.incoming_at && (b_used + 1) * out_ch <= t.incoming.len() {
                for (c, s) in o.iter_mut().enumerate() {
                    *s += t.incoming[b_used * out_ch + c];
                }
                b_used += 1;
            }
        }
        t.incoming.drain(..b_used * out_ch);
        self.limiter.process(&mut block);
        self.volume.process(&mut block, out_ch);
        self.pending_out = block;
        t.frame = end;

        let b_out = t.incoming_done && t.incoming.is_empty();
        if t.frame >= tail_frames && (t.shaper.is_done() || b_out) {
            if !t.handed_over && b_out {
                // B ran out inside the overlap: hand over now.
                self.transition = Some(t);
                self.hand_over(None, true);
                t = self.transition.take().expect("still there");
            }
            if t.handed_over {
                // Done: B carries on through the plain path.
                let mut rest = std::mem::take(&mut t.incoming);
                self.volume.process(&mut rest, out_ch);
                self.pending_out.extend_from_slice(&rest);
                tracing::debug!(target: "hocket::audio::native", "AutoMix: transition finished");
                return Step::Continue;
            }
        }
        self.transition = Some(t);
        Step::Continue
    }

    /// Reports that follow what the listener hears: `TransitionStarted`
    /// when the tail's first frame plays, the handover when its frame does.
    fn transition_marks(&mut self) {
        let queued = self.queued_frames();
        let Some(t) = self.transition.as_mut() else {
            return;
        };
        let heard = t.heard(queued);
        if !t.started_reported && heard >= 0 {
            t.started_reported = true;
            let plan = &t.tail.plan;
            let r = BackendReport::TransitionStarted {
                from: t.from.clone(),
                to: t.to.clone(),
                tail_len_ms: plan.tail_len_ms,
                incoming_at_ms: plan.incoming_at_ms,
                handover_ms: plan.handover_ms,
            };
            self.report(r);
        }
        let t = self.transition.as_ref().expect("checked");
        if !t.handed_over && heard >= t.handover as i64 {
            self.hand_over(Some(heard), true);
        }
    }

    /// Report the handover (`Ended(A)`, `TransitionedToNext(B)` and, when
    /// asked, `Position(B, p)`) and let A go. Returns B's position.
    fn hand_over(&mut self, heard: Option<i64>, report_position: bool) -> u32 {
        let queued = self.queued_frames();
        let rate = f64::from(self.out_rate.max(1));
        let b_duration = self.current.as_ref().and_then(|c| c.duration_ms);
        let Some(t) = self.transition.as_mut().filter(|t| !t.handed_over) else {
            return 0;
        };
        let mut reports = Vec::new();
        let plan = &t.tail.plan;
        if !t.started_reported {
            t.started_reported = true;
            reports.push(BackendReport::TransitionStarted {
                from: t.from.clone(),
                to: t.to.clone(),
                tail_len_ms: plan.tail_len_ms,
                incoming_at_ms: plan.incoming_at_ms,
                handover_ms: plan.handover_ms,
            });
        }
        let heard = heard.unwrap_or_else(|| t.heard(queued)).max(0);
        let mut p = plan.incoming_position_at((heard as f64 * 1000.0 / rate) as u32);
        if let Some(d) = b_duration {
            p = p.min(d);
        }
        t.handed_over = true;
        t.outgoing = None;
        let held = t.held_next.take();
        reports.push(BackendReport::Ended {
            key: t.from.clone(),
        });
        reports.push(BackendReport::TransitionedToNext { key: t.to.clone() });
        if report_position {
            reports.push(BackendReport::Position {
                key: t.to.clone(),
                position_ms: p,
            });
            self.last_position_report = Instant::now();
        }
        for r in reports {
            self.report(r);
        }
        if let Some(n) = held {
            self.set_next(n);
        }
        p
    }

    /// End a transition at once (Next pressed, output changed): hand over
    /// if that hasn't happened and play B on alone from where it is.
    fn cut_transition(&mut self, why: &str) {
        let Some(t) = self.transition.as_ref() else {
            return;
        };
        tracing::debug!(target: "hocket::audio::native", reason = why, "AutoMix: transition cut short");
        if t.handed_over {
            // B's decoded frames not yet mixed go with the transition: take
            // B up from where the listener is, flushed and ramped in.
            let p = self.current_position();
            self.transition = None;
            self.seek(p);
            return;
        }
        let p = self.hand_over(None, false);
        self.transition = None;
        self.seek(p);
    }

    /// A seek before the handover: A becomes current again and B goes back
    /// to being the follow-up (mixed into again if A reaches its tail start).
    fn cancel_transition(&mut self) {
        let Some(mut t) = self.transition.take() else {
            return;
        };
        if t.handed_over {
            return;
        }
        let Some((a, a_dsp)) = t.outgoing.take() else {
            return;
        };
        if let Some(mut b) = self.current.take() {
            b.rewind(0);
            self.next_key = Some(b.key().clone());
            self.next = Some(b);
        }
        // Settings changed during the transition reached only B's chain:
        // carry them over, and re-render the tail made with the old ones.
        let settings = self.dsp.settings().clone();
        self.dsp = a_dsp;
        self.current = Some(a);
        self.tail = TailState::Ready(t.tail);
        if &settings != self.dsp.settings() {
            self.dsp.set_settings(settings);
            self.drop_tail();
        }
        if let Some(n) = t.held_next {
            self.set_next(n);
        }
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
        self.transition_marks();
        self.maybe_prepare_tail();
        if self.playing
            && !self.fading_out
            && self.last_position_report.elapsed() >= POSITION_INTERVAL
        {
            if let Some((key, pos)) = self.now_playing() {
                self.report(BackendReport::Position {
                    key,
                    position_ms: pos,
                });
                self.last_position_report = Instant::now();
            }
        }
    }

    fn run(mut self) {
        self.ensure_output();
        loop {
            let wait = if self.playing || self.fading_out {
                Duration::from_millis(5)
            } else {
                Duration::from_millis(50)
            };
            match self.rx.recv_timeout(wait) {
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
pub(super) fn load_source(
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

/// What a second engine needs in order to play a preview on the same device.
struct PreviewKit {
    fetcher: Arc<dyn RangeFetcher>,
    runtime: tokio::runtime::Handle,
    output: OutputConfig,
    audio: AudioSettings,
}

/// The native backend handle. See the module docs.
pub struct NativeBackend {
    tx: Sender<Cmd>,
    thread: Mutex<Option<thread::JoinHandle<()>>>,
    devices: Arc<Mutex<Vec<OutputDevice>>>,
    watcher: Mutex<Option<DeviceWatcher>>,
    reports: ReportSink,
    /// Current output and DSP, so a preview opened later matches the main engine.
    kit: Mutex<PreviewKit>,
    /// Last gain sent to the engine, mirrored onto the preview player.
    gain: Mutex<f64>,
    /// A second engine for song previews. The main item stays loaded.
    preview: Mutex<Option<Box<NativeBackend>>>,
    /// Key of the preview currently loaded, so a late end from the previous
    /// song is not taken as the end of this one.
    preview_key: Arc<Mutex<String>>,
    /// Whether the preview was last asked to play, for position reports.
    preview_playing: Arc<AtomicBool>,
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
        let kit = PreviewKit {
            fetcher: config.fetcher.clone(),
            runtime: config.runtime.clone(),
            output: OutputConfig {
                device_id: config.audio.output_device.clone(),
                ..config.output
            },
            audio: config.audio.clone(),
        };
        let params = EngineParams {
            rx,
            tx: tx.clone(),
            sink: sink.clone(),
            fetcher: config.fetcher,
            runtime: config.runtime,
            output: kit.output.clone(),
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
            reports: sink,
            kit: Mutex::new(kit),
            gain: Mutex::new(1.0),
            preview: Mutex::new(None),
            preview_key: Arc::new(Mutex::new(String::new())),
            preview_playing: Arc::new(AtomicBool::new(false)),
        })
    }

    /// The preview engine, opened on the same device the first time a
    /// preview is asked for. Its reports are rewritten as preview reports
    /// so the core never mistakes them for the main item.
    fn ensure_preview(&self) -> Result<(), BackendError> {
        if self.preview.lock().is_some() {
            return Ok(());
        }
        let gain = *self.gain.lock();
        let kit = {
            let kit = self.kit.lock();
            PreviewKit {
                fetcher: kit.fetcher.clone(),
                runtime: kit.runtime.clone(),
                output: kit.output.clone(),
                audio: kit.audio.clone(),
            }
        };
        let parent = self.reports.clone();
        let current_key = self.preview_key.clone();
        let wanted = self.preview_playing.clone();
        let sink: ReportSink = Arc::new(move |report| {
            let key = match &report {
                BackendReport::Ready { key, .. }
                | BackendReport::Playing { key, .. }
                | BackendReport::Paused { key, .. }
                | BackendReport::Position { key, .. }
                | BackendReport::Ended { key }
                | BackendReport::Error { key, .. } => Some(key),
                _ => None,
            };
            if key.is_some_and(|k| k != &*current_key.lock()) {
                return;
            }
            let mapped = match report {
                BackendReport::Ready { duration_ms, .. } => {
                    Some(BackendReport::PreviewReady { duration_ms })
                }
                BackendReport::Playing { position_ms, .. } => {
                    Some(BackendReport::PreviewProgress {
                        position_ms,
                        playing: true,
                    })
                }
                BackendReport::Paused { position_ms, .. } => Some(BackendReport::PreviewProgress {
                    position_ms,
                    playing: false,
                }),
                // A position report says nothing about play/pause: it may
                // have been queued before a pause, so it carries what was
                // last asked of the preview, not "playing".
                BackendReport::Position { position_ms, .. } => {
                    Some(BackendReport::PreviewProgress {
                        position_ms,
                        playing: wanted.load(Ordering::Relaxed),
                    })
                }
                BackendReport::Ended { .. } => Some(BackendReport::PreviewEnded),
                BackendReport::Error { .. } => Some(BackendReport::PreviewFailed),
                _ => None,
            };
            if let Some(mapped) = mapped {
                parent(mapped);
            }
        });
        let cfg = NativeConfig {
            fetcher: kit.fetcher.clone(),
            runtime: kit.runtime.clone(),
            output: OutputConfig {
                device_id: kit.output.device_id.clone(),
                sample_rate: None,
                null_sink: kit.output.null_sink,
                tap: None,
            },
            audio: kit.audio.clone(),
            watch_devices: false,
        };
        let child = NativeBackend::new(cfg, sink)?;
        let _ = child.set_volume(gain);
        *self.preview.lock() = Some(Box::new(child));
        Ok(())
    }

    fn send(&self, cmd: Cmd) -> Result<(), BackendError> {
        self.tx.send(cmd).map_err(|_| BackendError::NotRunning)
    }

    /// Apply new DSP / gapless / exclusive settings.
    pub fn set_audio_settings(&self, settings: &AudioSettings) -> Result<(), BackendError> {
        self.kit.lock().audio = settings.clone();
        self.send(Cmd::SetDsp(DspSettings::from(settings)))?;
        self.send(Cmd::SetGapless(settings.gapless))?;
        self.send(Cmd::SetExclusive(settings.exclusive))?;
        if let Some(preview) = self.preview.lock().as_ref() {
            let _ = preview.set_audio_settings(settings);
        }
        Ok(())
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
        let volume = clamp_volume(volume);
        *self.gain.lock() = volume;
        if let Some(preview) = self.preview.lock().as_ref() {
            let _ = preview.set_volume(volume);
        }
        self.send(Cmd::SetVolume(volume))
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
        self.kit.lock().audio.gapless = enabled;
        if let Some(preview) = self.preview.lock().as_ref() {
            let _ = preview.set_gapless(enabled);
        }
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
        self.kit.lock().output.device_id = id.clone();
        if let Some(preview) = self.preview.lock().as_ref() {
            let _ = preview.set_output_device(id.clone());
        }
        self.send(Cmd::SetOutputDevice(id))
    }
    fn output_devices(&self) -> Vec<OutputDevice> {
        self.devices.lock().clone()
    }
    fn set_exclusive(&self, exclusive: bool) -> Result<(), BackendError> {
        self.kit.lock().audio.exclusive = exclusive;
        if let Some(preview) = self.preview.lock().as_ref() {
            let _ = preview.set_exclusive(exclusive);
        }
        self.send(Cmd::SetExclusive(exclusive))
    }
    fn set_audio_settings(&self, settings: &AudioSettings) -> Result<(), BackendError> {
        NativeBackend::set_audio_settings(self, settings)
    }
    fn complete_transition(&self) -> Result<(), BackendError> {
        self.send(Cmd::CompleteTransition)
    }

    fn name(&self) -> &'static str {
        "native"
    }

    fn preview_load(&self, source: MediaSource, position_ms: u32) -> Result<(), BackendError> {
        self.ensure_preview()?;
        *self.preview_key.lock() = source.key.clone();
        self.preview_playing.store(true, Ordering::Relaxed);
        let loaded = {
            let slot = self.preview.lock();
            match slot.as_ref() {
                Some(preview) => preview.load(source, None, position_ms, true),
                None => Err(BackendError::NotRunning),
            }
        };
        if loaded.is_err() {
            *self.preview_key.lock() = String::new();
        }
        loaded
    }

    fn preview_play(&self) -> Result<(), BackendError> {
        self.preview_playing.store(true, Ordering::Relaxed);
        let slot = self.preview.lock();
        let Some(preview) = slot.as_ref() else {
            return Ok(());
        };
        preview.play()
    }

    fn preview_pause(&self) -> Result<(), BackendError> {
        self.preview_playing.store(false, Ordering::Relaxed);
        let slot = self.preview.lock();
        let Some(preview) = slot.as_ref() else {
            return Ok(());
        };
        preview.pause()
    }

    fn preview_seek(&self, position_ms: u32) -> Result<(), BackendError> {
        let slot = self.preview.lock();
        let Some(preview) = slot.as_ref() else {
            return Ok(());
        };
        preview.seek(position_ms)
    }

    fn preview_stop(&self) -> Result<(), BackendError> {
        self.preview_playing.store(false, Ordering::Relaxed);
        *self.preview_key.lock() = String::new();
        let slot = self.preview.lock();
        let Some(preview) = slot.as_ref() else {
            return Ok(());
        };
        preview.stop()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::TrackSummary;
    use crate::audio::decoder::tests::write_test_wav;
    use crate::audio::native::http::MemoryFetcher;
    use crate::audio::native::output::OutputTap;
    use crate::core::default_audio_settings;

    type Reports = Arc<Mutex<Vec<BackendReport>>>;

    struct Rig {
        backend: NativeBackend,
        reports: Reports,
        _rt: tokio::runtime::Runtime,
        dir: tempfile::TempDir,
        tap: OutputTap,
    }

    fn rig_with(fetcher: Arc<dyn RangeFetcher>) -> Rig {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()
            .unwrap();
        let reports: Reports = Default::default();
        let r2 = reports.clone();
        let tap = OutputTap::default();
        let backend = NativeBackend::new(
            NativeConfig {
                fetcher,
                runtime: rt.handle().clone(),
                output: OutputConfig {
                    null_sink: true,
                    tap: Some(tap.clone()),
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
            tap,
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
                local: false,
                artwork_url: None,
                mix_in: None,
                mix_out_ms: None,
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
        // Discarded pre-buffers never report. The load may finish before the
        // discard is even sent, so only reports after it count: a seek
        // reports its position synchronously, right after the engine has
        // handled the discard.
        rig.backend
            .load(rig.wav("a.wav", 48_000, 48_000 * 2), None, 0, false)
            .unwrap();
        rig.wait_for(Duration::from_secs(5), |r| {
            matches!(r, BackendReport::Paused { .. })
        });
        let b = rig.wav("b.wav", 48_000, 48_000);
        rig.take();
        rig.backend.pre_buffer(b, 0).unwrap();
        rig.backend.discard_pre_buffer().unwrap();
        rig.backend.seek(1234).unwrap();
        let is_barrier = |r: &BackendReport| {
            matches!(
                r,
                BackendReport::Position {
                    position_ms: 1234,
                    ..
                }
            )
        };
        rig.wait_for(Duration::from_secs(5), is_barrier);
        thread::sleep(Duration::from_millis(300));
        let reports = rig.reports.lock();
        let barrier = reports.iter().position(is_barrier).unwrap();
        assert!(!reports[barrier..]
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
            local: false,
            artwork_url: None,
            mix_in: None,
            mix_out_ms: None,
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

    // -- AutoMix ----------------------------------------------------------

    use crate::api::{FadeCurve, TransitionKind};

    /// A stereo 16-bit sine, both channels alike.
    fn write_sine(path: &std::path::Path, rate: u32, ms: u32, freq: f64) {
        use std::io::Write;
        let frames = rate * ms / 1000;
        let data_len = frames * 4;
        let mut body = Vec::with_capacity(44 + data_len as usize);
        body.extend_from_slice(b"RIFF");
        body.extend_from_slice(&(36 + data_len).to_le_bytes());
        body.extend_from_slice(b"WAVEfmt ");
        body.extend_from_slice(&16u32.to_le_bytes());
        body.extend_from_slice(&1u16.to_le_bytes());
        body.extend_from_slice(&2u16.to_le_bytes());
        body.extend_from_slice(&rate.to_le_bytes());
        body.extend_from_slice(&(rate * 4).to_le_bytes());
        body.extend_from_slice(&4u16.to_le_bytes());
        body.extend_from_slice(&16u16.to_le_bytes());
        body.extend_from_slice(b"data");
        body.extend_from_slice(&data_len.to_le_bytes());
        for i in 0..frames {
            let v = (0.4
                * (2.0 * std::f64::consts::PI * freq * f64::from(i) / f64::from(rate)).sin()
                * 32767.0) as i16;
            body.extend_from_slice(&v.to_le_bytes());
            body.extend_from_slice(&v.to_le_bytes());
        }
        std::fs::File::create(path)
            .unwrap()
            .write_all(&body)
            .unwrap();
    }

    fn fade_plan(
        tail_start: u32,
        tail_len: u32,
        incoming_at: u32,
        handover: u32,
    ) -> TransitionPlan {
        TransitionPlan {
            kind: TransitionKind::Fade,
            tail_start_ms: tail_start,
            tail_source_ms: tail_len,
            tail_len_ms: tail_len,
            incoming_at_ms: incoming_at,
            incoming_from_ms: 0,
            handover_ms: handover,
            out_curve: FadeCurve::EqualPower,
            in_curve: FadeCurve::EqualPower,
            tempo: None,
            bass_swap: None,
            high_cut: None,
            incoming_gain_db: 0.0,
        }
    }

    impl Rig {
        fn sine(&self, name: &str, rate: u32, ms: u32, freq: f64) -> MediaSource {
            let mut m = self.wav(name, rate, 1);
            write_sine(&self.dir.path().join(name), rate, ms, freq);
            m.track.duration_ms = ms;
            m
        }

        /// The left channel of what the sink played, from the first to the
        /// last audible sample.
        fn played(&self) -> Vec<f32> {
            // Reports go out as audio is queued: wait until the sink has
            // played the ring out (300 ms of silence at the end).
            let deadline = Instant::now() + Duration::from_secs(5);
            loop {
                thread::sleep(Duration::from_millis(50));
                let t = self.tap.0.lock();
                let quiet = 48 * 300 * 2;
                if t.len() > quiet && t[t.len() - quiet..].iter().all(|v| v.abs() < 1e-3) {
                    break;
                }
                assert!(Instant::now() < deadline, "the sink never went quiet");
            }
            let all = self.tap.0.lock().clone();
            let left: Vec<f32> = all.chunks_exact(2).map(|f| f[0]).collect();
            let first = left.iter().position(|v| v.abs() > 1e-3).unwrap_or(0);
            let last = left.iter().rposition(|v| v.abs() > 1e-3).unwrap_or(0);
            left[first..=last].to_vec()
        }

        /// Load paused and give the tail time to render (debug builds on a
        /// loaded host are slow), then play.
        fn start(&self, a: MediaSource, b: MediaSource) {
            self.backend.load(a, Some(b), 0, false).unwrap();
            thread::sleep(Duration::from_millis(1_500));
            self.backend.play().unwrap();
        }

        fn count(&self, pred: impl Fn(&BackendReport) -> bool) -> usize {
            self.reports.lock().iter().filter(|r| pred(r)).count()
        }
    }

    fn pos_of(reports: &[BackendReport], pred: impl Fn(&BackendReport) -> bool) -> usize {
        reports
            .iter()
            .position(pred)
            .unwrap_or_else(|| panic!("missing in {reports:?}"))
    }

    /// No silence inside (5 ms windows all audible) and no click (no step
    /// much larger than the sines' own).
    fn assert_continuous(x: &[f32]) {
        let start = 48 * 20; // past the 10 ms fade-in
        for (i, w) in x[start..].chunks(240).enumerate() {
            let peak = w.iter().fold(0.0f32, |m, v| m.max(v.abs()));
            assert!(
                peak > 0.05 || w.len() < 240,
                "gap at {} ms",
                (start + i * 240) / 48
            );
        }
        let worst = x[start..]
            .windows(2)
            .map(|w| (w[1] - w[0]).abs())
            .enumerate()
            .fold((0, 0.0f32), |m, (i, d)| if d > m.1 { (i, d) } else { m });
        assert!(
            worst.1 < 0.08,
            "click of {} at {} ms",
            worst.1,
            (start + worst.0) / 48
        );
    }

    fn is_started(r: &BackendReport) -> bool {
        matches!(r, BackendReport::TransitionStarted { .. })
    }

    #[test]
    fn mixes_into_the_follow_up_and_hands_over_at_the_plan_time() {
        let rig = rig();
        let a = rig.sine("a.wav", 48_000, 3_000, 441.0);
        let mut b = rig.sine("b.wav", 48_000, 3_000, 660.0);
        b.mix_in = Some(fade_plan(1_500, 1_000, 300, 600));
        rig.start(a, b);
        let reports = rig.wait_for(
            Duration::from_secs(15),
            |r| matches!(r, BackendReport::Ended { key } if key == "b.wav"),
        );
        let started = pos_of(&reports, |r| {
            *r == BackendReport::TransitionStarted {
                from: "a.wav".into(),
                to: "b.wav".into(),
                tail_len_ms: 1_000,
                incoming_at_ms: 300,
                handover_ms: 600,
            }
        });
        let ended_a = pos_of(
            &reports,
            |r| matches!(r, BackendReport::Ended { key } if key == "a.wav"),
        );
        let next = pos_of(
            &reports,
            |r| matches!(r, BackendReport::TransitionedToNext { key } if key == "b.wav"),
        );
        assert!(started < ended_a && ended_a + 1 == next, "{reports:?}");
        // B's true position at the handover: 600 − 300 ms in.
        match &reports[next + 1] {
            BackendReport::Position { key, position_ms } if key == "b.wav" => {
                assert!((300..=330).contains(position_ms), "{position_ms}")
            }
            r => panic!("expected B's position, got {r:?}"),
        }
        // Positions of A before the handover never exceed the tail.
        assert!(reports[..ended_a].iter().all(|r| match r {
            BackendReport::Position { key, position_ms } if key == "a.wav" => *position_ms <= 2_500,
            _ => true,
        }));
        assert!(!reports
            .iter()
            .any(|r| matches!(r, BackendReport::Error { .. })));
        // 1.5 s of A, the 1 s tail, then B's remaining 2.3 s: 4.8 s, with
        // no gap and no click at the switch, B's entry or the tail's end.
        let x = rig.played();
        let ms = x.len() as f64 / 48.0;
        assert!((ms - 4_800.0).abs() < 15.0, "played {ms} ms");
        assert_continuous(&x);
    }

    #[test]
    fn mixes_across_sample_rates() {
        let rig = rig();
        let a = rig.sine("a.wav", 44_100, 2_000, 441.0);
        let mut b = rig.sine("b.wav", 44_100, 2_000, 660.0);
        b.mix_in = Some(fade_plan(1_000, 800, 200, 400));
        rig.start(a, b);
        rig.wait_for(
            Duration::from_secs(15),
            |r| matches!(r, BackendReport::Ended { key } if key == "b.wav"),
        );
        assert_eq!(rig.count(is_started), 1, "{:?}", rig.reports.lock());
        let x = rig.played();
        // 1 s + 0.8 s + (2 − 0.6) s.
        let ms = x.len() as f64 / 48.0;
        assert!((ms - 3_200.0).abs() < 15.0, "played {ms} ms");
        assert_continuous(&x);
    }

    #[test]
    fn a_late_tail_falls_back_to_a_gapless_cut() {
        let rig = rig();
        // The test build sleeps 3 s before rendering "slow-tail" sources.
        let a = rig.sine("slow-tail-a.wav", 48_000, 1_500, 441.0);
        let mut b = rig.sine("b.wav", 48_000, 1_500, 660.0);
        b.mix_in = Some(fade_plan(700, 500, 100, 300));
        // Paused first: a load's ring flush lands at the sink's next tick and,
        // on a loaded host, can eat the first frames pushed after it.
        rig.backend.load(a, Some(b), 0, false).unwrap();
        thread::sleep(Duration::from_millis(300));
        rig.backend.play().unwrap();
        let reports = rig.wait_for(
            Duration::from_secs(15),
            |r| matches!(r, BackendReport::Ended { key } if key == "b.wav"),
        );
        assert_eq!(rig.count(is_started), 0, "{reports:?}");
        let next = pos_of(
            &reports,
            |r| matches!(r, BackendReport::TransitionedToNext { key } if key == "b.wav"),
        );
        assert_eq!(
            reports[next + 1],
            BackendReport::Position {
                key: "b.wav".into(),
                position_ms: 0
            }
        );
        let x = rig.played();
        let ms = x.len() as f64 / 48.0;
        assert!((ms - 3_000.0).abs() < 15.0, "played {ms} ms");
        assert_continuous(&x);
    }

    #[test]
    fn complete_transition_hands_over_at_once() {
        let rig = rig();
        let a = rig.sine("a.wav", 48_000, 3_000, 441.0);
        let mut b = rig.sine("b.wav", 48_000, 3_000, 660.0);
        b.mix_in = Some(fade_plan(800, 1_500, 200, 1_200));
        rig.start(a, b);
        rig.wait_for(Duration::from_secs(10), is_started);
        thread::sleep(Duration::from_millis(400));
        rig.backend.complete_transition().unwrap();
        let reports = rig.wait_for(
            Duration::from_secs(10),
            |r| matches!(r, BackendReport::TransitionedToNext { key } if key == "b.wav"),
        );
        let next = pos_of(
            &reports,
            |r| matches!(r, BackendReport::TransitionedToNext { key } if key == "b.wav"),
        );
        assert!(
            matches!(&reports[next - 1], BackendReport::Ended { key } if key == "a.wav"),
            "{reports:?}"
        );
        thread::sleep(Duration::from_millis(50));
        let reports = rig.reports.lock().clone();
        match &reports[next + 1] {
            // ~400 ms into the tail, B entered at 200 ms.
            BackendReport::Position { key, position_ms } if key == "b.wav" => {
                assert!((120..=400).contains(position_ms), "{position_ms}")
            }
            r => panic!("expected B's position, got {r:?}"),
        }
        rig.wait_for(
            Duration::from_secs(10),
            |r| matches!(r, BackendReport::Ended { key } if key == "b.wav"),
        );
        assert_eq!(rig.count(is_started), 1);
        assert_eq!(
            rig.count(|r| matches!(r, BackendReport::TransitionedToNext { .. })),
            1
        );
    }

    #[test]
    fn a_seek_cancels_the_transition_and_it_mixes_again() {
        let rig = rig();
        let a = rig.sine("a.wav", 48_000, 2_500, 441.0);
        let mut b = rig.sine("b.wav", 48_000, 1_500, 660.0);
        b.mix_in = Some(fade_plan(1_000, 1_000, 200, 800));
        rig.start(a, b);
        rig.wait_for(Duration::from_secs(10), is_started);
        thread::sleep(Duration::from_millis(300));
        rig.backend.seek(500).unwrap();
        let reports = rig.wait_for(
            Duration::from_secs(15),
            |r| matches!(r, BackendReport::Ended { key } if key == "b.wav"),
        );
        let seek = pos_of(&reports, |r| {
            *r == BackendReport::Position {
                key: "a.wav".into(),
                position_ms: 500,
            }
        });
        let ended_a = pos_of(
            &reports,
            |r| matches!(r, BackendReport::Ended { key } if key == "a.wav"),
        );
        assert!(seek < ended_a, "{reports:?}");
        assert_eq!(rig.count(is_started), 2, "{reports:?}");
        assert_eq!(
            rig.count(|r| matches!(r, BackendReport::TransitionedToNext { .. })),
            1
        );
    }

    #[test]
    fn pause_holds_both_and_resume_carries_on() {
        let rig = rig();
        let a = rig.sine("a.wav", 48_000, 2_500, 441.0);
        let mut b = rig.sine("b.wav", 48_000, 1_500, 660.0);
        b.mix_in = Some(fade_plan(1_000, 1_200, 100, 900));
        rig.start(a, b);
        rig.wait_for(Duration::from_secs(10), is_started);
        thread::sleep(Duration::from_millis(200));
        rig.take();
        rig.backend.pause().unwrap();
        let reports = rig.wait_for(Duration::from_secs(5), |r| {
            matches!(r, BackendReport::Paused { .. })
        });
        match reports.last().unwrap() {
            BackendReport::Paused { key, position_ms } => {
                assert_eq!(key, "a.wav");
                assert!((1_100..1_600).contains(position_ms), "{position_ms}");
            }
            _ => unreachable!(),
        }
        thread::sleep(Duration::from_millis(500));
        assert_eq!(
            rig.count(|r| matches!(r, BackendReport::TransitionedToNext { .. })),
            0,
            "paused before the handover"
        );
        rig.backend.play().unwrap();
        rig.wait_for(
            Duration::from_secs(10),
            |r| matches!(r, BackendReport::Ended { key } if key == "b.wav"),
        );
        assert_eq!(
            rig.count(|r| matches!(r, BackendReport::TransitionedToNext { key } if key == "b.wav")),
            1
        );
    }

    #[test]
    fn set_next_during_the_overlap_waits_for_the_handover() {
        let rig = rig();
        let a = rig.sine("a.wav", 48_000, 2_000, 441.0);
        let mut b = rig.sine("b.wav", 48_000, 1_500, 660.0);
        b.mix_in = Some(fade_plan(800, 1_000, 200, 800));
        let c = rig.sine("c.wav", 48_000, 500, 550.0);
        rig.start(a, b);
        rig.wait_for(Duration::from_secs(10), is_started);
        rig.backend.set_next(Some(c)).unwrap();
        let reports = rig.wait_for(
            Duration::from_secs(15),
            |r| matches!(r, BackendReport::Ended { key } if key == "c.wav"),
        );
        let to_b = pos_of(
            &reports,
            |r| matches!(r, BackendReport::TransitionedToNext { key } if key == "b.wav"),
        );
        let to_c = pos_of(
            &reports,
            |r| matches!(r, BackendReport::TransitionedToNext { key } if key == "c.wav"),
        );
        assert!(to_b < to_c, "{reports:?}");
        let x = rig.played();
        // 0.8 + 1.0 + (1.5 − 0.8) + 0.5 s.
        let ms = x.len() as f64 / 48.0;
        assert!((ms - 3_000.0).abs() < 15.0, "played {ms} ms");
    }

    #[test]
    fn an_output_change_mid_transition_cuts_to_the_follow_up() {
        let rig = rig();
        let a = rig.sine("a.wav", 48_000, 3_000, 441.0);
        let mut b = rig.sine("b.wav", 48_000, 1_500, 660.0);
        b.mix_in = Some(fade_plan(800, 1_500, 200, 1_200));
        rig.start(a, b);
        rig.wait_for(Duration::from_secs(10), is_started);
        thread::sleep(Duration::from_millis(300));
        rig.backend.set_output_device(None).unwrap();
        let reports = rig.wait_for(
            Duration::from_millis(500),
            |r| matches!(r, BackendReport::TransitionedToNext { key } if key == "b.wav"),
        );
        let next = pos_of(
            &reports,
            |r| matches!(r, BackendReport::TransitionedToNext { key } if key == "b.wav"),
        );
        assert!(
            matches!(&reports[next - 1], BackendReport::Ended { key } if key == "a.wav"),
            "{reports:?}"
        );
        rig.wait_for(
            Duration::from_secs(10),
            |r| matches!(r, BackendReport::Ended { key } if key == "b.wav"),
        );
    }

    #[test]
    fn exclusive_output_never_mixes() {
        let rig = rig();
        rig.backend.set_exclusive(true).unwrap();
        let a = rig.sine("a.wav", 48_000, 1_200, 441.0);
        let mut b = rig.sine("b.wav", 48_000, 800, 660.0);
        b.mix_in = Some(fade_plan(500, 400, 100, 200));
        // Paused first: a load's ring flush lands at the sink's next tick and,
        // on a loaded host, can eat the first frames pushed after it.
        rig.backend.load(a, Some(b), 0, false).unwrap();
        thread::sleep(Duration::from_millis(300));
        rig.backend.play().unwrap();
        let reports = rig.wait_for(
            Duration::from_secs(10),
            |r| matches!(r, BackendReport::Ended { key } if key == "b.wav"),
        );
        assert_eq!(rig.count(is_started), 0, "{reports:?}");
        assert!(reports.contains(&BackendReport::Position {
            key: "b.wav".into(),
            position_ms: 0
        }));
    }

    /// Peak of `x` (48 kHz) between `from` and `to` ms.
    fn peak(x: &[f32], from: usize, to: usize) -> f32 {
        x[from * 48..to * 48]
            .iter()
            .fold(0.0f32, |m, v| m.max(v.abs()))
    }

    /// With Normalisation on, the tail carries on at live A's AGC gain: no
    /// level jump at the switch.
    #[test]
    fn the_tail_keeps_the_normaliser_gain() {
        let rig = rig();
        let audio = AudioSettings {
            normalisation: true,
            ..default_audio_settings()
        };
        rig.backend.set_audio_settings(&audio).unwrap();
        let a = rig.sine("a.wav", 48_000, 3_000, 441.0);
        let mut b = rig.sine("b.wav", 48_000, 1_500, 660.0);
        b.mix_in = Some(fade_plan(2_000, 800, 500, 600));
        rig.start(a, b);
        rig.wait_for(
            Duration::from_secs(15),
            |r| matches!(r, BackendReport::Ended { key } if key == "b.wav"),
        );
        assert_eq!(rig.count(is_started), 1, "{:?}", rig.reports.lock());
        let x = rig.played();
        let before = peak(&x, 1_950, 1_990);
        let after = peak(&x, 2_015, 2_055);
        assert!(before < 0.35, "the AGC turned A down: {before}");
        assert!(
            (after / before - 1.0).abs() < 0.06,
            "level jump at the switch: {before} → {after}"
        );
    }

    /// A settings change drops a tail rendered with the old ones: the tail
    /// plays with the new EQ, at the level live A had.
    #[test]
    fn new_dsp_settings_render_the_tail_again() {
        let rig = rig();
        let a = rig.sine("a.wav", 48_000, 3_000, 441.0);
        let mut b = rig.sine("b.wav", 48_000, 1_500, 660.0);
        b.mix_in = Some(fade_plan(2_000, 800, 500, 600));
        rig.backend.load(a, Some(b), 0, false).unwrap();
        thread::sleep(Duration::from_millis(1_500));
        let mut audio = default_audio_settings();
        audio.eq = crate::audio::dsp::eq::flat();
        audio.eq.enabled = true;
        audio.eq.preamp_db = -6.0;
        // Through the seam the core drives (the trait method, which the
        // LoggedBackend wrapper forwards), not the inherent one.
        PlaybackBackend::set_audio_settings(&rig.backend, &audio).unwrap();
        thread::sleep(Duration::from_millis(1_500));
        rig.backend.play().unwrap();
        rig.wait_for(
            Duration::from_secs(15),
            |r| matches!(r, BackendReport::Ended { key } if key == "b.wav"),
        );
        assert_eq!(rig.count(is_started), 1, "{:?}", rig.reports.lock());
        let x = rig.played();
        let before = peak(&x, 1_950, 1_990);
        let after = peak(&x, 2_015, 2_055);
        assert!(before < 0.25, "the EQ turned A down: {before}");
        assert!(
            (after / before - 1.0).abs() < 0.06,
            "level jump at the switch: {before} → {after}"
        );
    }

    /// Completing a transition after its handover takes B up from where the
    /// listener is (a fresh position, no frames lost).
    #[test]
    fn complete_after_the_handover_carries_on_from_b_heard() {
        let rig = rig();
        let a = rig.sine("a.wav", 48_000, 3_000, 441.0);
        let mut b = rig.sine("b.wav", 48_000, 3_000, 660.0);
        b.mix_in = Some(fade_plan(800, 1_500, 200, 400));
        rig.start(a, b);
        rig.wait_for(
            Duration::from_secs(10),
            |r| matches!(r, BackendReport::TransitionedToNext { key } if key == "b.wav"),
        );
        thread::sleep(Duration::from_millis(300));
        rig.take();
        rig.backend.complete_transition().unwrap();
        let reports = rig.wait_for(
            Duration::from_millis(500),
            |r| matches!(r, BackendReport::Position { key, .. } if key == "b.wav"),
        );
        match &reports[0] {
            // ~700 ms into the tail, B entered at 200 ms.
            BackendReport::Position { key, position_ms } if key == "b.wav" => {
                assert!((350..=700).contains(position_ms), "{position_ms}")
            }
            r => panic!("expected B's position, got {r:?}"),
        }
        rig.wait_for(
            Duration::from_secs(10),
            |r| matches!(r, BackendReport::Ended { key } if key == "b.wav"),
        );
        assert_eq!(
            rig.count(|r| matches!(r, BackendReport::TransitionedToNext { .. })),
            0
        );
    }

    /// A follow-up that can't keep up during the overlap reports buffering
    /// (under the item heard) and its end, as the plain path does.
    #[test]
    fn a_late_follow_up_reports_buffering_during_the_overlap() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("slow.wav");
        write_sine(&p, 48_000, 2_000, 660.0);
        // ~0.2× real time.
        let fetcher = MemoryFetcher {
            chunk: 4096,
            delay: Duration::from_millis(100),
            ..MemoryFetcher::new(std::fs::read(&p).unwrap())
        };
        let rig = rig_with(Arc::new(fetcher));
        let a = rig.sine("a.wav", 48_000, 2_000, 441.0);
        let mut b = rig.sine("b.wav", 48_000, 2_000, 660.0);
        b.url = "https://navidrome.example/rest/stream?id=slow&format=wav".into();
        b.mix_in = Some(fade_plan(1_000, 1_500, 0, 1_200));
        rig.start(a, b);
        let buffering = |on: bool| move |r: &BackendReport| matches!(r, BackendReport::Buffering { key, buffering } if key == "a.wav" && *buffering == on);
        rig.wait_for(Duration::from_secs(10), is_started);
        rig.wait_for(Duration::from_secs(10), buffering(true));
        rig.wait_for(Duration::from_secs(10), buffering(false));
    }

    /// A follow-up the tail had moved to its entry point plays from its
    /// start when the mix doesn't happen after all.
    #[test]
    fn a_gapless_join_after_a_dropped_mix_starts_b_at_zero() {
        let rig = rig();
        let a = rig.sine("a.wav", 48_000, 1_200, 441.0);
        let mut b = rig.sine("b.wav", 48_000, 1_000, 660.0);
        let mut plan = fade_plan(600, 400, 100, 200);
        plan.incoming_from_ms = 500;
        b.mix_in = Some(plan);
        rig.backend.load(a, Some(b), 0, false).unwrap();
        // The tail renders and B moves to 500 ms; then exclusive output
        // rules the mix out.
        thread::sleep(Duration::from_millis(1_500));
        rig.backend.set_exclusive(true).unwrap();
        thread::sleep(Duration::from_millis(100));
        rig.backend.play().unwrap();
        rig.wait_for(
            Duration::from_secs(10),
            |r| matches!(r, BackendReport::Ended { key } if key == "b.wav"),
        );
        assert_eq!(rig.count(is_started), 0);
        let x = rig.played();
        let ms = x.len() as f64 / 48.0;
        assert!((ms - 2_200.0).abs() < 15.0, "played {ms} ms");
    }

    /// Dropping a tail that is still rendering stops its job.
    #[test]
    fn a_dropped_tail_stops_rendering() {
        let rig = rig();
        // The test build waits 3 s before rendering "slow-tail" sources.
        let a = rig.sine("slow-tail-cancel.wav", 48_000, 1_500, 441.0);
        let mut b = rig.sine("b.wav", 48_000, 1_500, 660.0);
        b.mix_in = Some(fade_plan(700, 500, 100, 300));
        let running = || {
            super::super::mix::RUNNING_TAILS
                .lock()
                .iter()
                .any(|k| k == "slow-tail-cancel.wav")
        };
        rig.backend.load(a, Some(b), 0, false).unwrap();
        thread::sleep(Duration::from_millis(500));
        assert!(running());
        rig.backend.set_next(None).unwrap();
        thread::sleep(Duration::from_millis(500));
        assert!(!running());
    }
}
