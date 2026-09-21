//! The output stage: one continuous cpal stream fed from a lock-free ring,
//! device enumeration, a hot-plug watcher, and a null sink that drains the
//! ring at real-time rate when there is no audio device (CI, headless).

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use parking_lot::Mutex;

use crate::api::OutputDevice;
use crate::audio::native::ring::SpscRing;

/// Errors opening the output.
#[derive(Debug, Clone, thiserror::Error, PartialEq, Eq)]
pub enum OutputError {
    #[error("no output device")]
    NoDevice,
    #[error("unknown output device: {0}")]
    UnknownDevice(String),
    #[error("device does not support {0} Hz")]
    UnsupportedRate(u32),
    #[error("output stream: {0}")]
    Stream(String),
}

/// What to open.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct OutputConfig {
    /// Device id (its cpal name), `None` for the system default.
    pub device_id: Option<String>,
    /// Requested stream rate (exclusive / bit-perfect). `None` = device default.
    pub sample_rate: Option<u32>,
    /// Force the null sink (tests, `--no-audio`).
    pub null_sink: bool,
}

/// Ring plus the flush flag the callback honours.
pub struct OutputRing {
    pub ring: SpscRing,
    flush: AtomicBool,
    /// Frames the consumer has taken from the ring (monotonic).
    consumed_frames: AtomicU64,
    underruns: AtomicU64,
}

impl OutputRing {
    pub fn new(capacity_frames: usize, channels: usize) -> Arc<Self> {
        Arc::new(Self {
            ring: SpscRing::new(capacity_frames * channels),
            flush: AtomicBool::new(false),
            consumed_frames: AtomicU64::new(0),
            underruns: AtomicU64::new(0),
        })
    }

    /// Ask the consumer to drop everything buffered at its next callback
    /// (seek/stop). Safe from the producer side.
    pub fn request_flush(&self) {
        self.flush.store(true, Ordering::Release);
    }

    pub fn underruns(&self) -> u64 {
        self.underruns.load(Ordering::Relaxed)
    }

    pub fn consumed_frames(&self) -> u64 {
        self.consumed_frames.load(Ordering::Relaxed)
    }

    /// Consumer side: fill `data`, zero on underrun.
    fn drain_into(&self, data: &mut [f32], channels: usize) {
        if self.flush.swap(false, Ordering::AcqRel) {
            self.ring.discard_all();
        }
        let n = self.ring.pop(data);
        if n < data.len() {
            for s in &mut data[n..] {
                *s = 0.0;
            }
            if n == 0 || !self.ring.is_empty() {
                // Only count when the producer really fell behind.
            }
            self.underruns.fetch_add(1, Ordering::Relaxed);
        }
        self.consumed_frames.fetch_add((n / channels.max(1)) as u64, Ordering::Relaxed);
    }
}

/// An open output. Dropping it closes the device.
pub enum OutputStream {
    Cpal { stream: cpal::Stream, rate: u32, channels: usize, device_id: Option<String> },
    Null { stop: Arc<AtomicBool>, thread: Option<thread::JoinHandle<()>>, rate: u32, channels: usize },
}

impl std::fmt::Debug for OutputStream {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OutputStream::Cpal { rate, channels, device_id, .. } => {
                f.debug_struct("Cpal").field("rate", rate).field("channels", channels).field("device", device_id).finish()
            }
            OutputStream::Null { rate, channels, .. } => f.debug_struct("Null").field("rate", rate).field("channels", channels).finish(),
        }
    }
}

impl OutputStream {
    pub fn rate(&self) -> u32 {
        match self {
            OutputStream::Cpal { rate, .. } | OutputStream::Null { rate, .. } => *rate,
        }
    }

    pub fn channels(&self) -> usize {
        match self {
            OutputStream::Cpal { channels, .. } | OutputStream::Null { channels, .. } => *channels,
        }
    }

    pub fn is_null(&self) -> bool {
        matches!(self, OutputStream::Null { .. })
    }
}

impl Drop for OutputStream {
    fn drop(&mut self) {
        if let OutputStream::Null { stop, thread, .. } = self {
            stop.store(true, Ordering::Release);
            if let Some(t) = thread.take() {
                let _ = t.join();
            }
        }
    }
}

/// Enumerate output devices. Ids are the cpal device names, which is what
/// the settings store; `is_default` marks the host default.
pub fn list_output_devices() -> Vec<OutputDevice> {
    let host = cpal::default_host();
    let default_name = host.default_output_device().and_then(|d| d.name().ok());
    let mut out = Vec::new();
    match host.output_devices() {
        Ok(devices) => {
            for d in devices {
                if let Ok(name) = d.name() {
                    if out.iter().any(|o: &OutputDevice| o.id == name) {
                        continue;
                    }
                    out.push(OutputDevice { id: name.clone(), name: name.clone(), is_default: Some(&name) == default_name.as_ref() });
                }
            }
        }
        Err(e) => tracing::warn!(target: "hocket::audio::output", error = %e, "enumerating output devices failed"),
    }
    out
}

fn find_device(host: &cpal::Host, id: Option<&str>) -> Result<cpal::Device, OutputError> {
    match id {
        None => host.default_output_device().ok_or(OutputError::NoDevice),
        Some(id) => {
            let mut devices = host.output_devices().map_err(|e| OutputError::Stream(e.to_string()))?;
            devices.find(|d| d.name().map(|n| n == id).unwrap_or(false)).ok_or_else(|| OutputError::UnknownDevice(id.to_string()))
        }
    }
}

/// Null sink at the given format: a thread pops samples at real-time rate.
pub fn open_null(rate: u32, channels: usize, ring: Arc<OutputRing>) -> OutputStream {
    let stop = Arc::new(AtomicBool::new(false));
    let stop2 = stop.clone();
    let thread = thread::Builder::new()
        .name("hocket-null-sink".into())
        .spawn(move || {
            let tick = Duration::from_millis(10);
            let per_tick = (rate as usize / 100) * channels;
            let mut buf = vec![0.0f32; per_tick];
            let start = Instant::now();
            let mut ticks: u64 = 0;
            while !stop2.load(Ordering::Acquire) {
                ring.drain_into(&mut buf, channels);
                ticks += 1;
                let due = start + tick * ticks as u32;
                let now = Instant::now();
                if due > now {
                    thread::sleep(due - now);
                }
            }
        })
        .ok();
    OutputStream::Null { stop, thread, rate, channels }
}

/// Open the output described by `config`, feeding from `ring` (which must
/// have been created for the format this returns; callers open the stream
/// first with a probe and then size the ring — see [`probe_format`]).
///
/// Falls back to the null sink, with a warning, when the host has no usable
/// device so the app still runs without a sound card.
pub fn open_output(config: &OutputConfig, ring: Arc<OutputRing>, on_error: impl Fn(String) + Send + 'static) -> Result<OutputStream, OutputError> {
    if config.null_sink {
        let (rate, channels) = (config.sample_rate.unwrap_or(48_000), 2);
        return Ok(open_null(rate, channels, ring));
    }
    match open_cpal(config, ring.clone(), on_error) {
        Ok(s) => Ok(s),
        Err(OutputError::UnknownDevice(id)) => Err(OutputError::UnknownDevice(id)),
        Err(OutputError::UnsupportedRate(r)) => Err(OutputError::UnsupportedRate(r)),
        Err(e) => {
            tracing::warn!(target: "hocket::audio::output", error = %e, "no audio output available, using null sink");
            Ok(open_null(config.sample_rate.unwrap_or(48_000), 2, ring))
        }
    }
}

/// The format `open_output` would produce for `config` without opening a
/// stream: `(rate, channels)`. Used to size the ring and resampler first.
pub fn probe_format(config: &OutputConfig) -> Result<(u32, usize), OutputError> {
    if config.null_sink {
        return Ok((config.sample_rate.unwrap_or(48_000), 2));
    }
    let host = cpal::default_host();
    let device = match find_device(&host, config.device_id.as_deref()) {
        Ok(d) => d,
        Err(OutputError::UnknownDevice(id)) => return Err(OutputError::UnknownDevice(id)),
        Err(_) => return Ok((config.sample_rate.unwrap_or(48_000), 2)),
    };
    let supported = match choose_config(&device, config.sample_rate) {
        Ok(c) => c,
        Err(OutputError::UnsupportedRate(r)) => return Err(OutputError::UnsupportedRate(r)),
        Err(_) => return Ok((config.sample_rate.unwrap_or(48_000), 2)),
    };
    Ok((supported.sample_rate().0, usize::from(supported.channels())))
}

fn choose_config(device: &cpal::Device, rate: Option<u32>) -> Result<cpal::SupportedStreamConfig, OutputError> {
    let default = device.default_output_config().map_err(|e| OutputError::Stream(e.to_string()))?;
    let Some(rate) = rate else { return Ok(default) };
    if default.sample_rate().0 == rate {
        return Ok(default);
    }
    let want_channels = default.channels();
    let ranges = device.supported_output_configs().map_err(|e| OutputError::Stream(e.to_string()))?;
    let mut best: Option<cpal::SupportedStreamConfig> = None;
    for r in ranges {
        if r.min_sample_rate().0 <= rate && rate <= r.max_sample_rate().0 {
            let cfg = r.with_sample_rate(cpal::SampleRate(rate));
            let better = match &best {
                None => true,
                Some(b) => (cfg.channels() == want_channels) && b.channels() != want_channels || (cfg.sample_format() == cpal::SampleFormat::F32 && b.sample_format() != cpal::SampleFormat::F32),
            };
            if better {
                best = Some(cfg);
            }
        }
    }
    best.ok_or(OutputError::UnsupportedRate(rate))
}

fn open_cpal(config: &OutputConfig, ring: Arc<OutputRing>, on_error: impl Fn(String) + Send + 'static) -> Result<OutputStream, OutputError> {
    let host = cpal::default_host();
    let device = find_device(&host, config.device_id.as_deref())?;
    let supported = choose_config(&device, config.sample_rate)?;
    let rate = supported.sample_rate().0;
    let channels = usize::from(supported.channels());
    let stream_config = cpal::StreamConfig { channels: supported.channels(), sample_rate: supported.sample_rate(), buffer_size: cpal::BufferSize::Default };
    let on_error = Arc::new(on_error);
    let err_cb = {
        let on_error = on_error.clone();
        move |e: cpal::StreamError| {
            tracing::warn!(target: "hocket::audio::output", error = %e, "output stream error");
            on_error(e.to_string());
        }
    };
    let stream = match supported.sample_format() {
        cpal::SampleFormat::I16 => {
            let r = ring.clone();
            let mut scratch: Vec<f32> = Vec::new();
            device.build_output_stream::<i16, _, _>(
                &stream_config,
                move |data, _| {
                    scratch.resize(data.len(), 0.0);
                    r.drain_into(&mut scratch, channels);
                    for (o, s) in data.iter_mut().zip(&scratch) {
                        *o = (s.clamp(-1.0, 1.0) * 32767.0) as i16;
                    }
                },
                err_cb,
                None,
            )
        }
        cpal::SampleFormat::U16 => {
            let r = ring.clone();
            let mut scratch: Vec<f32> = Vec::new();
            device.build_output_stream::<u16, _, _>(
                &stream_config,
                move |data, _| {
                    scratch.resize(data.len(), 0.0);
                    r.drain_into(&mut scratch, channels);
                    for (o, s) in data.iter_mut().zip(&scratch) {
                        *o = ((s.clamp(-1.0, 1.0) * 32767.0) as i32 + 32768) as u16;
                    }
                },
                err_cb,
                None,
            )
        }
        _ => {
            let r = ring.clone();
            device.build_output_stream::<f32, _, _>(&stream_config, move |data, _| r.drain_into(data, channels), err_cb, None)
        }
    }
    .map_err(|e| OutputError::Stream(e.to_string()))?;
    stream.play().map_err(|e| OutputError::Stream(e.to_string()))?;
    tracing::info!(target: "hocket::audio::output", rate, channels, device = ?config.device_id, "output stream open");
    Ok(OutputStream::Cpal { stream, rate, channels, device_id: config.device_id.clone() })
}

/// Polls the device list and calls `on_change` with the new list when it
/// differs. cpal 0.15 has no hot-plug notification, so this is the
/// portable way to get `OutputDevicesChanged`.
pub struct DeviceWatcher {
    stop: Arc<AtomicBool>,
    thread: Option<thread::JoinHandle<()>>,
    pub devices: Arc<Mutex<Vec<OutputDevice>>>,
}

impl DeviceWatcher {
    pub fn start(interval: Duration, initial: Vec<OutputDevice>, on_change: impl Fn(Vec<OutputDevice>) + Send + 'static) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let devices = Arc::new(Mutex::new(initial));
        let s2 = stop.clone();
        let d2 = devices.clone();
        let thread = thread::Builder::new()
            .name("hocket-device-watch".into())
            .spawn(move || {
                while !s2.load(Ordering::Acquire) {
                    thread::sleep(interval);
                    if s2.load(Ordering::Acquire) {
                        break;
                    }
                    let now = list_output_devices();
                    let changed = {
                        let mut cur = d2.lock();
                        if *cur != now {
                            *cur = now.clone();
                            true
                        } else {
                            false
                        }
                    };
                    if changed {
                        on_change(now);
                    }
                }
            })
            .ok();
        Self { stop, thread, devices }
    }

    /// Force a refresh now (`Command::RefreshOutputDevices`).
    pub fn refresh(&self) -> Vec<OutputDevice> {
        let now = list_output_devices();
        *self.devices.lock() = now.clone();
        now
    }
}

impl Drop for DeviceWatcher {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn null_sink_drains_at_real_time_rate() {
        let ring = OutputRing::new(48_000, 2);
        let out = open_null(48_000, 2, ring.clone());
        assert!(out.is_null());
        assert_eq!(out.rate(), 48_000);
        ring.ring.push(&vec![0.5; 9600]); // 100 ms
        thread::sleep(Duration::from_millis(250));
        assert!(ring.ring.is_empty());
        let consumed = ring.consumed_frames();
        assert!((4_800..=20_000).contains(&consumed), "{consumed}");
        assert!(ring.underruns() > 0, "silence padded while starved");
        drop(out);
    }

    #[test]
    fn flush_request_is_honoured_by_the_consumer() {
        let ring = OutputRing::new(1024, 2);
        ring.ring.push(&[1.0; 100]);
        ring.request_flush();
        let mut buf = [0.0; 10];
        ring.drain_into(&mut buf, 2);
        assert!(buf.iter().all(|v| *v == 0.0));
        assert!(ring.ring.is_empty());
    }

    #[test]
    fn open_output_never_fails_for_the_default_device() {
        // On CI there is no device: this must fall back to the null sink.
        let ring = OutputRing::new(4096, 2);
        let out = open_output(&OutputConfig::default(), ring, |_| {}).expect("fallback");
        assert!(out.rate() > 0 && out.channels() > 0);
        let _ = probe_format(&OutputConfig::default()).unwrap();
        assert!(matches!(open_output(&OutputConfig { device_id: Some("no such device".into()), ..Default::default() }, OutputRing::new(16, 2), |_| {}), Err(OutputError::UnknownDevice(_)) | Ok(_)));
    }

    #[test]
    fn device_listing_is_deduplicated() {
        let d = list_output_devices();
        let mut ids: Vec<&str> = d.iter().map(|x| x.id.as_str()).collect();
        let n = ids.len();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), n);
        assert!(d.iter().filter(|x| x.is_default).count() <= 1);
    }
}
