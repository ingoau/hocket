//! Minimal UniFFI surface: JSON in, JSON out, events via a callback interface.
//! Kotlin types come from `typeshare` (see `android/core-api`), not from UniFFI.

use std::sync::Arc;

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
        if let Ok(json) = serde_json::to_string(&event) {
            self.0.on_event(json);
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
