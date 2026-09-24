//! Minimal UniFFI surface: JSON in, JSON out, events via a callback interface.
//! Kotlin types come from `typeshare` (see `android/core-api`), not from UniFFI.

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::Arc;
use std::time::Duration;

use hocket_core::api::Event;
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

    /// Flush and stop the core (session document, position, settings, sync base), blocking the
    /// calling thread for at most `timeout_ms`. Returns `true` when the flush completed in time.
    /// Idempotent. Blocks: call it from a worker thread, never from the main thread, and free the
    /// object afterwards.
    pub fn shutdown(&self, timeout_ms: u64) -> bool {
        shutdown_off_runtime(&self.core, Duration::from_millis(timeout_ms))
    }
}

/// Awaits [`Core::shutdown`] on a plain helper thread, never on the core's own runtime.
///
/// `Core::shutdown_blocking` awaits on a runtime task that owns a clone of the core. When the caller
/// frees its handle right after the flush (as `NativeCore.close()` does), that task's clone is the
/// last one and drops the tokio runtime from inside one of its own workers, which panics ("Cannot
/// drop a runtime in a context where blocking is not allowed") and leaves the runtime half torn
/// down. Here the only other clone lives on a thread outside any runtime, where dropping the last
/// handle (and with it the runtime) is allowed. The shutdown future needs no runtime context: it
/// sends on the actor's channel and awaits a oneshot.
fn shutdown_off_runtime(core: &Core, timeout: Duration) -> bool {
    let core = core.clone();
    let (done_tx, done_rx) = std::sync::mpsc::channel::<()>();
    let spawned = std::thread::Builder::new()
        .name("hocket-shutdown".into())
        .spawn(move || {
            futures::executor::block_on(core.shutdown());
            let _ = done_tx.send(());
            // `core` drops here, off the runtime.
        });
    if spawned.is_err() {
        return false;
    }
    done_rx.recv_timeout(timeout).is_ok()
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
mod shutdown_tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Once};

    use super::*;

    /// Panics on any thread (the runtime's workers included), counted by a process-wide hook.
    static PANICS: AtomicUsize = AtomicUsize::new(0);
    static HOOK: Once = Once::new();

    fn count_panics() {
        HOOK.call_once(|| {
            let previous = std::panic::take_hook();
            std::panic::set_hook(Box::new(move |info| {
                PANICS.fetch_add(1, Ordering::SeqCst);
                previous(info);
            }));
        });
    }

    fn core(dir: &std::path::Path) -> Arc<HocketCore> {
        let config = serde_json::json!({
            "dataDir": dir.join("data").to_string_lossy(),
            "cacheDir": dir.join("cache").to_string_lossy(),
            "deviceId": "shutdown-test",
            "deviceName": "test",
            "platform": "android",
            "appVersion": "0.0.0-test",
            "audio": "external",
        });
        HocketCore::new(config.to_string()).expect("core")
    }

    /// `NativeCore.close()`: shut down, then free the handle straight away. The last reference to
    /// the core (and its tokio runtime) must never be dropped on one of the runtime's own workers.
    #[test]
    fn shutdown_then_free_never_drops_the_runtime_on_its_own_worker() {
        count_panics();
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
        assert_eq!(
            PANICS.load(Ordering::SeqCst),
            0,
            "a thread panicked during shutdown"
        );
    }
}
