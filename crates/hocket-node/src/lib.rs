//! Minimal napi surface: JSON in, JSON out, events via a threadsafe callback.
//! TypeScript types come from `typeshare` (see `desktop/src/core/api.ts`).

use std::sync::Arc;

use hocket_core::api::Event;
use hocket_core::Core;
use napi::bindgen_prelude::*;
use napi::threadsafe_function::{ThreadsafeFunction, ThreadsafeFunctionCallMode};
use napi_derive::napi;

struct Bridge(ThreadsafeFunction<String, (), String, false>);

impl hocket_core::EventSink for Bridge {
    fn on_event(&self, event: Event) {
        if let Ok(json) = serde_json::to_string(&event) {
            self.0.call(json, ThreadsafeFunctionCallMode::NonBlocking);
        }
    }
}

#[napi]
pub struct HocketCore {
    core: Core,
}

#[napi]
impl HocketCore {
    /// `configJson` is a serialised `CoreConfig`.
    #[napi(constructor)]
    pub fn new(config_json: String) -> Result<Self> {
        let config = serde_json::from_str(&config_json).map_err(|e| Error::from_reason(e.to_string()))?;
        let core = Core::new(config).map_err(|e| Error::from_reason(e.to_string()))?;
        Ok(Self { core })
    }

    /// Subscribe to events; `callback(eventJson)`.
    #[napi]
    pub fn set_listener(&self, callback: ThreadsafeFunction<String, (), String, false>) {
        self.core.add_sink(Arc::new(Bridge(callback)));
    }

    #[napi]
    pub fn dispatch(&self, command_json: String) -> Result<()> {
        self.core.dispatch_json(&command_json).map_err(|e| Error::from_reason(e.to_string()))
    }

    #[napi]
    pub async fn query(&self, query_json: String) -> Result<String> {
        self.core.query_json(&query_json).await.map_err(|e| Error::from_reason(e.to_string()))
    }
}

#[napi]
pub fn init_logging(level: String) {
    let _ = tracing_subscriber::fmt().with_env_filter(level).try_init();
}

#[napi]
pub fn core_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}
