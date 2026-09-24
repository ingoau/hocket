//! Minimal UniFFI surface: JSON in, JSON out, events via a callback interface.
//! Kotlin types come from `typeshare` (see `android/core-api`), not from UniFFI.

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::Arc;
use std::time::Duration;

use hocket_core::api::Event;
use hocket_core::core::stream_reader;
use hocket_core::{Core, CoreError};

uniffi::setup_scaffolding!();

#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum HocketError {
    /// `reason`, not `message`: UniFFI turns the variant into a Kotlin exception and a field called
    /// `message` collides with `Throwable.message`.
    #[error("{reason}")]
    Failed { reason: String },
}

impl From<CoreError> for HocketError {
    fn from(e: CoreError) -> Self {
        HocketError::Failed {
            reason: e.to_string(),
        }
    }
}

/// A failed stream call, mirroring the core's `stream_reader::StreamError`. Payloads never carry a
/// URL (the core strips it from network errors), so the Kotlin exception is safe to log.
#[derive(Debug, PartialEq, Eq, thiserror::Error, uniffi::Error)]
pub enum StreamError {
    /// The `hocket-stream://` token is unknown or expired: the player should ask the core to
    /// resolve the queue item again.
    #[error("unknown or expired stream token")]
    UnknownToken,
    #[error("unknown stream handle")]
    UnknownHandle,
    #[error("too many open streams")]
    TooManyHandles,
    #[error("no server connection")]
    NoServer,
    #[error("offset is past the end of the stream")]
    RangeNotSatisfiable,
    #[error("server answered HTTP {code}")]
    Status { code: u16 },
    #[error("server sent an error instead of audio")]
    ErrorEnvelope,
    #[error("network: {reason}")]
    Network { reason: String },
    #[error("io: {reason}")]
    Io { reason: String },
    #[error("stream closed")]
    Closed,
    #[error("core is shut down")]
    ShutDown,
}

impl From<stream_reader::StreamError> for StreamError {
    fn from(e: stream_reader::StreamError) -> Self {
        use stream_reader::StreamError as E;
        match e {
            E::UnknownToken => StreamError::UnknownToken,
            E::UnknownHandle => StreamError::UnknownHandle,
            E::TooManyHandles => StreamError::TooManyHandles,
            E::NoServer => StreamError::NoServer,
            E::RangeNotSatisfiable => StreamError::RangeNotSatisfiable,
            E::Status(code) => StreamError::Status { code },
            E::ErrorEnvelope => StreamError::ErrorEnvelope,
            E::Network(reason) => StreamError::Network { reason },
            E::Io(reason) => StreamError::Io { reason },
            E::Closed => StreamError::Closed,
            E::ShutDown => StreamError::ShutDown,
        }
    }
}

/// What `stream_open` returned.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct StreamInfo {
    pub handle: u64,
    /// Where the first byte read comes from (always the requested offset).
    pub offset: u64,
    /// Size of the whole resource, when known (a transcode may not know it).
    pub total_length: Option<u64>,
    /// Bytes this handle returns before end of input, when known.
    pub length: Option<u64>,
    pub content_type: Option<String>,
}

impl From<stream_reader::StreamInfo> for StreamInfo {
    fn from(i: stream_reader::StreamInfo) -> Self {
        StreamInfo {
            handle: i.handle,
            offset: i.offset,
            total_length: i.total_length,
            length: i.length,
            content_type: i.content_type,
        }
    }
}

/// Implemented on the Kotlin side. Called on a core thread; hop to Main before touching UI.
#[uniffi::export(with_foreign)]
pub trait EventListener: Send + Sync {
    fn on_event(&self, event_json: String);
}

struct Bridge(Arc<dyn EventListener>);

impl hocket_core::EventSink for Bridge {
    fn on_event(&self, event: Event) {
        let Ok(json) = serde_json::to_string(&event) else {
            return;
        };
        // The sink is called synchronously on the actor task. UniFFI turns an unexpected exception
        // from the Kotlin listener into a panic here, which would kill the actor; contain it so one
        // bad event on the app side cannot take the core down with it.
        if catch_unwind(AssertUnwindSafe(|| self.0.on_event(json))).is_err() {
            tracing::error!(target: "hocket_android", "event listener failed; event dropped");
        }
    }
}

#[derive(uniffi::Object)]
pub struct HocketCore {
    core: Core,
}

#[uniffi::export]
impl HocketCore {
    /// `config_json` is a serialised `CoreConfig`.
    #[uniffi::constructor]
    pub fn new(config_json: String) -> Result<Arc<Self>, HocketError> {
        let config = serde_json::from_str(&config_json).map_err(|e| HocketError::Failed {
            reason: e.to_string(),
        })?;
        let core = Core::new(config)?;
        Ok(Arc::new(Self { core }))
    }

    pub fn set_listener(&self, listener: Arc<dyn EventListener>) {
        self.core.add_sink(Arc::new(Bridge(listener)));
    }

    /// Serialised `Command`. Never blocks.
    pub fn dispatch(&self, command_json: String) -> Result<(), HocketError> {
        Ok(self.core.dispatch_json(&command_json)?)
    }

    /// Serialised `Query` → serialised `QueryResult`.
    pub async fn query(&self, query_json: String) -> Result<String, HocketError> {
        Ok(self.core.query_json(&query_json).await?)
    }

    // -- stream reader (ExoPlayer's `HocketStreamDataSource`) -------------------------------------
    //
    // All three BLOCK the calling thread: call them from ExoPlayer's loader threads, never from the
    // main thread and never from inside a tokio runtime. The work runs on the core's own runtime.

    /// Open a `hocket-stream://<token>` media source at `offset`, returning at most `length` bytes
    /// when given.
    pub fn stream_open(
        &self,
        url: String,
        offset: u64,
        length: Option<u64>,
    ) -> Result<StreamInfo, StreamError> {
        Ok(self.core.stream_open_blocking(&url, offset, length)?.into())
    }

    /// Up to `max_bytes` (capped by the core at 256 KiB) from an open stream. Empty = end of input.
    pub fn stream_read(&self, handle: u64, max_bytes: u32) -> Result<Vec<u8>, StreamError> {
        Ok(self
            .core
            .stream_read_blocking(handle, max_bytes as usize)?
            .to_vec())
    }

    /// Close a stream; a read in progress on it fails with `Closed`. Never blocks. Idempotent.
    pub fn stream_close(&self, handle: u64) {
        self.core.stream_close(handle);
    }

    /// Flush and stop the core (session document, position, settings, sync base), blocking the
    /// calling thread for at most `timeout_ms`. Returns `true` when the flush completed in time.
    /// Idempotent. Blocks: call it from a worker thread, never from the main thread, and free the
    /// object afterwards.
    pub fn shutdown(&self, timeout_ms: u64) -> bool {
        // `Core::shutdown_blocking` waits on a task that holds only the flush acknowledgement, never
        // a `Core`, so freeing this object right afterwards (as `NativeCore.close()` does) can never
        // leave the last handle, and with it the runtime, on one of the runtime's own workers.
        self.core
            .shutdown_blocking(Duration::from_millis(timeout_ms))
    }
}

/// Installs a tracing subscriber that forwards to logcat-friendly stderr. Call once.
#[uniffi::export]
pub fn init_logging(level: String) {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(level)
        .with_ansi(false)
        .try_init();
}

#[uniffi::export]
pub fn core_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

#[cfg(test)]
mod fixtures;

#[cfg(test)]
mod ffi_tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex, Once};

    use super::*;

    /// Messages of panics on any thread (the runtime's workers included), from a process-wide hook.
    static PANICS: Mutex<Vec<String>> = Mutex::new(Vec::new());
    static HOOK: Once = Once::new();

    fn record_panics() {
        HOOK.call_once(|| {
            let previous = std::panic::take_hook();
            std::panic::set_hook(Box::new(move |info| {
                PANICS.lock().unwrap().push(info.to_string());
                previous(info);
            }));
        });
    }

    fn panics_matching(needle: &str) -> usize {
        PANICS
            .lock()
            .unwrap()
            .iter()
            .filter(|m| m.contains(needle))
            .count()
    }

    fn core(dir: &std::path::Path) -> Arc<HocketCore> {
        let config = serde_json::json!({
            "dataDir": dir.join("data").to_string_lossy(),
            "cacheDir": dir.join("cache").to_string_lossy(),
            "deviceId": "ffi-test",
            "deviceName": "test",
            "platform": "android",
            "appVersion": "0.0.0-test",
            "audio": "external",
        });
        HocketCore::new(config.to_string()).expect("core")
    }

    /// `NativeCore.close()`: shut down, then free the handle straight away. The last reference to
    /// the core (and its tokio runtime) must never be dropped on one of the runtime's own workers.
    /// The race is timing-dependent, hence the repetitions.
    #[test]
    fn shutdown_then_free_never_drops_the_runtime_on_its_own_worker() {
        record_panics();
        for _ in 0..24 {
            let dir = tempfile::tempdir().unwrap();
            let c = core(dir.path());
            c.dispatch(r#"{"type":"start"}"#.into()).unwrap();
            assert!(c.shutdown(10_000), "the flush completes in time");
            assert!(c.shutdown(10_000), "a second shutdown returns at once");
            drop(c);
            // Give a worker that would have dropped the runtime the chance to do (and panic) so.
            std::thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(panics_matching("Cannot drop a runtime"), 0);
    }

    /// The blocking stream surface from a plain (non-runtime) thread, the way ExoPlayer's loader
    /// threads call it: an unknown token is `UnknownToken` (the DataSource turns that into a
    /// re-resolve), an unknown handle is `UnknownHandle`, and close is idempotent.
    #[test]
    fn stream_calls_block_off_runtime_and_map_errors() {
        let dir = tempfile::tempdir().unwrap();
        let c = core(dir.path());
        c.dispatch(r#"{"type":"start"}"#.into()).unwrap();
        let worker = {
            let c = c.clone();
            std::thread::spawn(move || {
                let open = c.stream_open("hocket-stream://no-such-token".into(), 0, None);
                let read = c.stream_read(4242, 1024);
                c.stream_close(4242);
                c.stream_close(4242);
                (open, read)
            })
        };
        let (open, read) = worker.join().expect("no panic on the loader thread");
        assert_eq!(open, Err(StreamError::UnknownToken));
        assert_eq!(read, Err(StreamError::UnknownHandle));
        assert!(c.shutdown(10_000));
    }

    #[test]
    fn every_core_stream_error_maps_one_to_one() {
        use hocket_core::core::stream_reader::StreamError as E;
        let cases = [
            (E::UnknownToken, StreamError::UnknownToken),
            (E::UnknownHandle, StreamError::UnknownHandle),
            (E::TooManyHandles, StreamError::TooManyHandles),
            (E::NoServer, StreamError::NoServer),
            (E::RangeNotSatisfiable, StreamError::RangeNotSatisfiable),
            (E::Status(503), StreamError::Status { code: 503 }),
            (E::ErrorEnvelope, StreamError::ErrorEnvelope),
            (
                E::Network("reset".into()),
                StreamError::Network {
                    reason: "reset".into(),
                },
            ),
            (
                E::Io("disk".into()),
                StreamError::Io {
                    reason: "disk".into(),
                },
            ),
            (E::Closed, StreamError::Closed),
            (E::ShutDown, StreamError::ShutDown),
        ];
        for (core_err, ffi_err) in cases {
            assert_eq!(StreamError::from(core_err), ffi_err);
        }
    }

    struct FailingListener(AtomicUsize);

    impl EventListener for FailingListener {
        fn on_event(&self, _event_json: String) {
            self.0.fetch_add(1, Ordering::SeqCst);
            // What UniFFI does with an unexpected exception from the Kotlin callback.
            panic!("listener failure (expected in this test)");
        }
    }

    /// A listener that throws must not kill the actor: every later event is still offered to it
    /// and the core keeps answering.
    #[test]
    fn a_failing_listener_never_takes_the_core_down() {
        record_panics();
        let dir = tempfile::tempdir().unwrap();
        let c = core(dir.path());
        let listener = Arc::new(FailingListener(AtomicUsize::new(0)));
        c.set_listener(listener.clone());
        c.dispatch(r#"{"type":"start"}"#.into()).unwrap();
        let first = futures::executor::block_on(c.query(r#"{"type":"servers"}"#.into()));
        assert!(
            first.is_ok(),
            "the actor answers after a failing callback: {first:?}"
        );
        let seen = listener.0.load(Ordering::SeqCst);
        assert!(seen > 0, "Start emitted events to the listener");
        c.dispatch(r#"{"type":"requestSnapshot"}"#.into()).unwrap();
        let second = futures::executor::block_on(c.query(r#"{"type":"snapshot"}"#.into()));
        assert!(second.is_ok());
        assert!(
            listener.0.load(Ordering::SeqCst) > seen,
            "later events still reach it"
        );
        assert!(c.shutdown(10_000));
    }
}
