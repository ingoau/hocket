//! Minimal napi surface: JSON in, JSON out, events via a threadsafe callback.
//! TypeScript types come from `typeshare` (see `desktop/src/core/api.ts`).
//!
//! Two classes cross the seam:
//!
//! - [`HocketCore`] wraps [`hocket_core::Core`]: `dispatch(commandJson)`,
//!   `query(queryJson) -> Promise<string>`, `setListener(cb)`.
//! - [`media_session::MediaSession`] is the desktop `MediaSessionAdapter`
//!   (docs/design.md "OS media session") over the `playwire` crate. It takes
//!   a serialised `MediaSessionState` and hands back serialised `Command`s.

mod media_session;

use std::sync::Arc;

use hocket_core::api::Event;
use hocket_core::Core;
use napi::bindgen_prelude::*;
use napi::threadsafe_function::{ThreadsafeFunction, ThreadsafeFunctionCallMode};
use napi_derive::napi;

pub use media_session::{media_session_available, MediaSession, MediaSessionOptions};

/// JSON string callback: `(eventJson: string) => void`, never receives an error argument.
pub(crate) type JsonCallback = ThreadsafeFunction<String, (), String, Status, false>;

struct Bridge(JsonCallback);

impl hocket_core::EventSink for Bridge {
    fn on_event(&self, event: Event) {
        match serde_json::to_string(&event) {
            Ok(json) => {
                self.0.call(json, ThreadsafeFunctionCallMode::NonBlocking);
            }
            Err(e) => tracing::error!(target: "hocket_node", "event serialisation failed: {e}"),
        }
    }
}

/// One core per process. Constructed by the Electron main process.
#[napi]
pub struct HocketCore {
    core: Core,
}

#[napi]
impl HocketCore {
    /// `configJson` is a serialised `CoreConfig`.
    #[napi(constructor)]
    pub fn new(config_json: String) -> Result<Self> {
        let config =
            serde_json::from_str(&config_json).map_err(|e| Error::from_reason(e.to_string()))?;
        let core = Core::new(config).map_err(|e| Error::from_reason(e.to_string()))?;
        Ok(Self { core })
    }

    /// Subscribe to events; `callback(eventJson)`. May be called more than once
    /// (UI + media session + tests); every listener receives every event.
    #[napi]
    pub fn set_listener(&self, callback: JsonCallback) {
        self.core.add_sink(Arc::new(Bridge(callback)));
    }

    /// Fire-and-forget `Command`. Errors only on malformed JSON or a shut-down core.
    #[napi]
    pub fn dispatch(&self, command_json: String) -> Result<()> {
        self.core
            .dispatch_json(&command_json)
            .map_err(|e| Error::from_reason(e.to_string()))
    }

    /// `Query` in, `QueryResult` out, both JSON.
    #[napi]
    pub async fn query(&self, query_json: String) -> Result<String> {
        self.core
            .query_json(&query_json)
            .await
            .map_err(|e| Error::from_reason(e.to_string()))
    }
}

/// Install a `tracing` subscriber writing to stderr. `level` is an `EnvFilter`
/// directive such as `info` or `hocket_core=debug`. Idempotent.
#[napi]
pub fn init_logging(level: String) {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(level)
        .with_writer(std::io::stderr)
        .try_init();
}

/// Version of the addon crate (same as the workspace version).
#[napi]
pub fn core_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

/// `API_SCHEMA_VERSION` of the core the addon was built against; the app
/// refuses to load an addon whose schema doesn't match its generated types.
#[napi]
pub fn api_schema_version() -> u32 {
    hocket_core::api::API_SCHEMA_VERSION
}
