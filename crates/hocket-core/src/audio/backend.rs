//! The `PlaybackBackend` seam.
//!
//! The actor drives playback through this trait and never learns whether the
//! audio comes out of cpal in this process or out of ExoPlayer on the other
//! side of a JNI bridge. All methods are fire-and-forget and return quickly:
//! a backend hands the request to its own thread (native), to the platform
//! (external) or to a virtual timeline (scripted) and reports progress
//! asynchronously as [`BackendReport`]s through the [`ReportSink`] it was
//! constructed with. The actor wires that sink to its internal channel.
//!
//! Method semantics mirror [`crate::api::BackendCommand`] one to one so the
//! external bridge is a trivial translation.

use std::fmt;
use std::sync::Arc;

use crate::api::{BackendReport, MediaSource, OutputDevice};

/// Where a backend posts its reports. Must be cheap and non-blocking; the
/// actor's implementation pushes onto an unbounded channel.
pub type ReportSink = Arc<dyn Fn(BackendReport) + Send + Sync>;

/// Errors a backend can raise synchronously. Asynchronous playback failures
/// arrive as [`BackendReport::Error`] instead.
#[derive(Debug, Clone, thiserror::Error, PartialEq, Eq)]
pub enum BackendError {
    /// The backend's worker is gone (thread exited, platform detached).
    #[error("playback backend is not running")]
    NotRunning,
    /// The requested output device does not exist.
    #[error("unknown output device: {0}")]
    UnknownDevice(String),
    /// Feature not available on this backend (e.g. device selection on the
    /// external backend, where the platform owns routing).
    #[error("unsupported on this backend: {0}")]
    Unsupported(&'static str),
    /// Anything else, already logged in detail.
    #[error("{0}")]
    Other(String),
}

/// The seam. See the module docs for the contract.
pub trait PlaybackBackend: Send + Sync {
    /// Replace what is loaded with `source`, optionally preloading `next` for
    /// a gapless follow-up, positioned at `position_ms`, playing immediately
    /// when `play` is set. Reports `Ready`, then `Playing`/`Paused`.
    fn load(
        &self,
        source: MediaSource,
        next: Option<MediaSource>,
        position_ms: u32,
        play: bool,
    ) -> Result<(), BackendError>;

    /// Update only the preloaded follow-up. `None` cancels the preload.
    fn set_next(&self, next: Option<MediaSource>) -> Result<(), BackendError>;

    fn play(&self) -> Result<(), BackendError>;
    fn pause(&self) -> Result<(), BackendError>;

    /// Unload everything, including the preloaded next and any pre-buffer.
    fn stop(&self) -> Result<(), BackendError>;

    /// Seek within the current item. Reports `Position` when the new position
    /// is audible.
    fn seek(&self, position_ms: u32) -> Result<(), BackendError>;

    /// Linear volume 0.0–1.0, applied post-DSP with a short ramp.
    fn set_volume(&self, volume: f64) -> Result<(), BackendError>;

    /// Handoff pre-buffering: fetch and decode the first seconds at
    /// `position_ms` without opening the output. Reports `PreBufferReady`.
    /// A later `load` of the same key at (about) the same position reuses it.
    fn pre_buffer(&self, source: MediaSource, position_ms: u32) -> Result<(), BackendError>;

    /// Drop the pre-buffer (picker closed, timeout, another device chosen).
    fn discard_pre_buffer(&self) -> Result<(), BackendError>;

    /// Gapless on/off. On is the default. Off skips preloading and padding
    /// trimming so the transition behaves like a plain stop/start.
    fn set_gapless(&self, enabled: bool) -> Result<(), BackendError>;

    /// Select an output device by id; `None` restores the system default.
    fn set_output_device(&self, id: Option<String>) -> Result<(), BackendError>;

    /// Currently available output devices. Empty where the platform owns
    /// routing (Android) or there is no audio at all.
    fn output_devices(&self) -> Vec<OutputDevice>;

    /// Exclusive / bit-perfect output: reconfigure the stream to the source
    /// rate where the device supports it. Ignored by backends that can't.
    fn set_exclusive(&self, _exclusive: bool) -> Result<(), BackendError> {
        Ok(())
    }

    /// Human-readable backend name for diagnostics.
    fn name(&self) -> &'static str;
}

impl fmt::Debug for dyn PlaybackBackend {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "PlaybackBackend({})", self.name())
    }
}

/// A backend that plays nothing and reports nothing. Used by the coordinator
/// (`AudioMode::None`) and by tests that don't care about audio.
#[derive(Debug, Default, Clone, Copy)]
pub struct NullBackend;

impl NullBackend {
    pub fn new() -> Self {
        NullBackend
    }
}

impl PlaybackBackend for NullBackend {
    fn load(&self, _: MediaSource, _: Option<MediaSource>, _: u32, _: bool) -> Result<(), BackendError> {
        Ok(())
    }
    fn set_next(&self, _: Option<MediaSource>) -> Result<(), BackendError> {
        Ok(())
    }
    fn play(&self) -> Result<(), BackendError> {
        Ok(())
    }
    fn pause(&self) -> Result<(), BackendError> {
        Ok(())
    }
    fn stop(&self) -> Result<(), BackendError> {
        Ok(())
    }
    fn seek(&self, _: u32) -> Result<(), BackendError> {
        Ok(())
    }
    fn set_volume(&self, _: f64) -> Result<(), BackendError> {
        Ok(())
    }
    fn pre_buffer(&self, _: MediaSource, _: u32) -> Result<(), BackendError> {
        Ok(())
    }
    fn discard_pre_buffer(&self) -> Result<(), BackendError> {
        Ok(())
    }
    fn set_gapless(&self, _: bool) -> Result<(), BackendError> {
        Ok(())
    }
    fn set_output_device(&self, _: Option<String>) -> Result<(), BackendError> {
        Ok(())
    }
    fn output_devices(&self) -> Vec<OutputDevice> {
        Vec::new()
    }
    fn name(&self) -> &'static str {
        "null"
    }
}

/// Clamp a volume into the linear 0–1 range, mapping NaN to 1.0.
pub fn clamp_volume(volume: f64) -> f64 {
    if volume.is_nan() {
        1.0
    } else {
        volume.clamp(0.0, 1.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn null_backend_accepts_everything() {
        let b = NullBackend::new();
        assert!(b.play().is_ok());
        assert!(b.set_output_device(Some("x".into())).is_ok());
        assert!(b.output_devices().is_empty());
        assert_eq!(b.name(), "null");
    }

    #[test]
    fn volume_clamps() {
        assert_eq!(clamp_volume(2.0), 1.0);
        assert_eq!(clamp_volume(-1.0), 0.0);
        assert_eq!(clamp_volume(f64::NAN), 1.0);
        assert_eq!(clamp_volume(0.4), 0.4);
    }
}
