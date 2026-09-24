//! Settings: a registry of scoped keys, a values document persisted as JSON,
//! last-write-wins merge for account-synced keys, and the config backup.
//!
//! Entry points for the actor:
//!
//! | Need | Call |
//! |---|---|
//! | Load at start | [`Settings::load`] `(&dyn SettingsStore)`; [`Settings::save`] after every change |
//! | Read a value | [`Settings::get`] (JSON), [`Settings::get_bool`], [`get_i64`](Settings::get_i64), [`get_f64`](Settings::get_f64), [`get_string`](Settings::get_string), [`get_typed`](Settings::get_typed) |
//! | `Command::SetSetting` / `ResetSetting` | [`Settings::set_json`] / [`Settings::reset`] → the `api::Setting` to emit |
//! | `Query::Settings` / snapshot | [`Settings::to_api`] |
//! | Sync: what to send, what to accept | [`Settings::synced_settings`], [`Settings::merge_remote`]; pure [`merge`] |
//! | Config backup | [`build_document`], [`to_json`], [`parse_document`] (migrates), [`Settings::import_settings`] (→ [`ImportOutcome`]; changed keys restamped to now, device-local keys opt-in, changed synced keys to broadcast) |
//! | Key constants | [`keys`] |
//! | Labels/descriptions | [`strings`] |
//!
//! `sync.enabled` is the master toggle: device-local, on by default. When
//! off, [`Settings::synced_settings`] is empty and [`Settings::merge_remote`]
//! is a no-op.

pub mod config;
pub mod registry;
pub mod strings;

#[cfg(test)]
mod tests;

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub use config::{
    build_document, parse_document, secret_key, secret_reference, to_json, ConfigError,
    ConfigInputs, CONFIG_VERSION,
};
pub use registry::{keys, lookup, validate, SettingDef, SettingKind, REGISTRY};

use crate::api::{Setting, SettingScope};

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum SettingsError {
    #[error("unknown setting '{0}'")]
    UnknownKey(String),
    #[error("'{key}': {reason}")]
    Invalid { key: String, reason: String },
    #[error("value is not valid JSON: {0}")]
    Json(String),
    #[error("store: {0}")]
    Store(String),
}

/// Persistence for the settings document (one JSON blob, e.g. the
/// `saved_state` row `"settings"`). The actor adapts the db to this.
pub trait SettingsStore: Send + Sync {
    /// The last saved document, or `None` on first run.
    fn load(&self) -> Result<Option<String>, SettingsError>;
    fn save(&self, json: &str) -> Result<(), SettingsError>;
}

/// In-memory store for tests and the coordinator.
#[derive(Debug, Default)]
pub struct MemoryStore {
    doc: parking_lot::Mutex<Option<String>>,
}

impl SettingsStore for MemoryStore {
    fn load(&self) -> Result<Option<String>, SettingsError> {
        Ok(self.doc.lock().clone())
    }
    fn save(&self, json: &str) -> Result<(), SettingsError> {
        *self.doc.lock() = Some(json.to_string());
        Ok(())
    }
}

/// Persisted schema version of the settings document.
///
/// - 2: `storage.cacheMaxBytes` is `null` for an automatic stream-cache
///   budget. Version 1 read its old default (2 GiB) as automatic, so a
///   stored 2 GiB migrates to `null`.
pub const SETTINGS_DOC_VERSION: u32 = 2;

/// The 2 GiB that version 1 stored for an automatic stream-cache budget.
const V1_CACHE_MAX_DEFAULT: f64 = 2.0 * 1024.0 * 1024.0 * 1024.0;

/// A stored `storage.cacheMaxBytes` equal to its version-1 default meant
/// "automatic" then; it is `null` now. Shared by the settings document and
/// config-backup migrations.
pub(crate) fn migrate_cache_budget_v1(value: &mut Value) {
    if value.as_f64() == Some(V1_CACHE_MAX_DEFAULT) {
        *value = Value::Null;
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Entry {
    value: Value,
    #[serde(default)]
    updated_at: f64,
    /// Kept for keys the registry no longer (or not yet) knows.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    scope: Option<SettingScope>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Document {
    version: u32,
    #[serde(default)]
    entries: BTreeMap<String, Entry>,
}

/// The settings state. Unset keys read as their registry default.
#[derive(Debug, Clone, Default)]
pub struct Settings {
    entries: BTreeMap<String, Entry>,
}

impl Settings {
    pub fn new() -> Self {
        Self::default()
    }

    /// Loads from the store; a missing or unreadable document starts empty
    /// (defaults), logging the problem rather than failing start-up.
    pub fn load(store: &dyn SettingsStore) -> Result<Settings, SettingsError> {
        let Some(json) = store.load()? else {
            return Ok(Settings::new());
        };
        Ok(Settings::from_json(&json).unwrap_or_else(|e| {
            tracing::error!(error = %e, "settings document unreadable; starting from defaults");
            Settings::new()
        }))
    }

    pub fn save(&self, store: &dyn SettingsStore) -> Result<(), SettingsError> {
        store.save(&self.to_json()?)
    }

    pub fn to_json(&self) -> Result<String, SettingsError> {
        serde_json::to_string(&Document {
            version: SETTINGS_DOC_VERSION,
            entries: self.entries.clone(),
        })
        .map_err(|e| SettingsError::Json(e.to_string()))
    }

    pub fn from_json(json: &str) -> Result<Settings, SettingsError> {
        let doc: Document =
            serde_json::from_str(json).map_err(|e| SettingsError::Json(e.to_string()))?;
        if doc.version > SETTINGS_DOC_VERSION {
            tracing::warn!(
                version = doc.version,
                "settings document from a newer app; unknown keys preserved"
            );
        }
        let mut s = Settings {
            entries: doc.entries,
        };
        if doc.version < 2 {
            if let Some(e) = s.entries.get_mut(keys::STORAGE_CACHE_MAX_BYTES) {
                migrate_cache_budget_v1(&mut e.value);
            }
        }
        // Drop stored values that no longer validate (older bounds, etc.).
        s.entries.retain(|k, e| match lookup(k) {
            Some(def) => validate(def, &e.value).is_ok(),
            None => true,
        });
        Ok(s)
    }

    /// The schema version of a stored document (`None` when it does not
    /// parse): below [`SETTINGS_DOC_VERSION`], loading it migrates.
    pub fn document_version(json: &str) -> Option<u32> {
        serde_json::from_str::<Document>(json)
            .ok()
            .map(|d| d.version)
    }

    /// Scope of a key: registry first, then whatever the stored entry says.
    pub fn scope_of(&self, key: &str) -> Option<SettingScope> {
        lookup(key)
            .map(|d| d.scope)
            .or_else(|| self.entries.get(key).and_then(|e| e.scope))
    }

    /// The effective value: set value or registry default. `Null` for keys
    /// nobody knows.
    pub fn get(&self, key: &str) -> Value {
        match self.entries.get(key) {
            Some(e) => e.value.clone(),
            None => lookup(key).map(|d| (d.default)()).unwrap_or(Value::Null),
        }
    }

    pub fn is_set(&self, key: &str) -> bool {
        self.entries.contains_key(key)
    }

    pub fn get_bool(&self, key: &str) -> bool {
        self.get(key).as_bool().unwrap_or(false)
    }

    pub fn get_i64(&self, key: &str) -> i64 {
        let v = self.get(key);
        v.as_i64()
            .or_else(|| v.as_f64().map(|f| f as i64))
            .unwrap_or(0)
    }

    pub fn get_f64(&self, key: &str) -> f64 {
        self.get(key).as_f64().unwrap_or(0.0)
    }

    pub fn get_string(&self, key: &str) -> Option<String> {
        self.get(key)
            .as_str()
            .filter(|s| !s.is_empty())
            .map(str::to_string)
    }

    /// Deserialises a JSON-kind setting; falls back to the registry default
    /// when the stored value doesn't parse.
    pub fn get_typed<T: serde::de::DeserializeOwned>(&self, key: &str) -> Option<T> {
        serde_json::from_value(self.get(key))
            .ok()
            .or_else(|| lookup(key).and_then(|d| serde_json::from_value((d.default)()).ok()))
    }

    /// Sets a value from its JSON encoding (what `Command::SetSetting`
    /// carries). Validates against the registry; unknown keys are refused.
    pub fn set_json(
        &mut self,
        key: &str,
        value_json: &str,
        now_ms: f64,
    ) -> Result<Setting, SettingsError> {
        let value: Value =
            serde_json::from_str(value_json).map_err(|e| SettingsError::Json(e.to_string()))?;
        self.set_value(key, value, now_ms)
    }

    /// Sets a value. Returns the `api::Setting` to emit.
    pub fn set_value(
        &mut self,
        key: &str,
        value: Value,
        now_ms: f64,
    ) -> Result<Setting, SettingsError> {
        let def = lookup(key).ok_or_else(|| SettingsError::UnknownKey(key.to_string()))?;
        validate(def, &value).map_err(|reason| SettingsError::Invalid {
            key: key.to_string(),
            reason,
        })?;
        self.entries.insert(
            key.to_string(),
            Entry {
                value,
                updated_at: now_ms,
                scope: Some(def.scope),
            },
        );
        Ok(self.setting(key).expect("just inserted"))
    }

    /// Typed convenience over [`Settings::set_value`].
    pub fn set_typed<T: Serialize>(
        &mut self,
        key: &str,
        value: &T,
        now_ms: f64,
    ) -> Result<Setting, SettingsError> {
        let v = serde_json::to_value(value).map_err(|e| SettingsError::Json(e.to_string()))?;
        self.set_value(key, v, now_ms)
    }

    /// Back to the default. Records the reset time so it syncs like a write.
    pub fn reset(&mut self, key: &str, now_ms: f64) -> Result<Setting, SettingsError> {
        let def = lookup(key).ok_or_else(|| SettingsError::UnknownKey(key.to_string()))?;
        self.entries.insert(
            key.to_string(),
            Entry {
                value: (def.default)(),
                updated_at: now_ms,
                scope: Some(def.scope),
            },
        );
        Ok(self.setting(key).expect("just inserted"))
    }

    /// One key as `api::Setting` (effective value, default when unset).
    pub fn setting(&self, key: &str) -> Option<Setting> {
        let scope = self.scope_of(key)?;
        let updated_at = self.entries.get(key).map(|e| e.updated_at).unwrap_or(0.0);
        Some(Setting {
            key: key.to_string(),
            value: self.get(key).to_string(),
            scope,
            updated_at,
        })
    }

    /// Every registry key (in registry order) plus any stored unknown keys.
    pub fn to_api(&self) -> Vec<Setting> {
        let mut out: Vec<Setting> = REGISTRY
            .iter()
            .filter_map(|d| self.setting(d.key))
            .collect();
        for k in self.entries.keys() {
            if lookup(k).is_none() {
                if let Some(s) = self.setting(k) {
                    out.push(s);
                }
            }
        }
        out
    }

    /// Whether the master sync toggle is on.
    pub fn sync_enabled(&self) -> bool {
        self.get_bool(keys::SYNC_ENABLED)
    }

    /// Account-synced settings that have been explicitly set, for sending
    /// to peers. Empty when sync is off.
    pub fn synced_settings(&self) -> Vec<Setting> {
        if !self.sync_enabled() {
            return Vec::new();
        }
        self.entries
            .keys()
            .filter(|k| self.scope_of(k) == Some(SettingScope::AccountSynced))
            .filter_map(|k| self.setting(k))
            .collect()
    }

    /// Accepts remote synced settings, last-write-wins per key. Returns the
    /// keys that changed. No-op when sync is off. Device-local keys and
    /// invalid values from peers are ignored.
    pub fn merge_remote(&mut self, remote: &[Setting]) -> Vec<String> {
        if !self.sync_enabled() {
            return Vec::new();
        }
        let (merged, changed) = merge(&self.synced_settings_including_defaults(), remote);
        for s in merged {
            if changed.contains(&s.key) {
                if let Ok(value) = serde_json::from_str::<Value>(&s.value) {
                    self.entries.insert(
                        s.key.clone(),
                        Entry {
                            value,
                            updated_at: s.updated_at,
                            scope: Some(SettingScope::AccountSynced),
                        },
                    );
                }
            }
        }
        changed
    }

    fn synced_settings_including_defaults(&self) -> Vec<Setting> {
        REGISTRY
            .iter()
            .filter(|d| d.scope == SettingScope::AccountSynced)
            .filter_map(|d| self.setting(d.key))
            .collect()
    }

    /// Applies settings from a config document (import), device-local keys
    /// included (a same-device restore). Returns `(applied keys, skipped
    /// keys)`. See [`Settings::import_settings`] for the scoped variant the
    /// actor should prefer; timestamps are restamped the same way.
    pub fn apply_settings(
        &mut self,
        settings: &[Setting],
        now_ms: f64,
    ) -> (Vec<String>, Vec<String>) {
        let out = self.import_settings(settings, now_ms, true);
        (out.applied, out.skipped)
    }

    /// Applies settings from a config document (import).
    ///
    /// - An import is a deliberate edit made now, so every valid entry whose
    ///   value differs from the current one is stamped `now_ms`, whatever
    ///   the document says. Keeping the document's timestamp would let an
    ///   older backup lose to any newer peer value and be reverted on the
    ///   next merge, and a hand-edited far-future `updatedAt` would win
    ///   every merge forever. Entries equal to the current value are left
    ///   untouched (counted as applied, not restamped, not broadcast).
    /// - Invalid or unknown entries are skipped and reported.
    /// - Device-local keys (see [`registry::REGISTRY`], scope
    ///   `DeviceLocal`: audio/output device, transcoding profiles, storage
    ///   budgets, `connect.*`, display accent/theme, battery, shortcuts,
    ///   `sync.enabled`, `lyrics.external.enabled`, …) describe *this*
    ///   device and are only imported when `include_device_local`; otherwise
    ///   they are listed in `skipped_device_local`.
    /// - `changed_synced` lists the account-synced keys whose effective value
    ///   changed, for the actor to broadcast (`Input::SettingChanged`) so
    ///   peers learn about the import instead of reverting it on merge.
    pub fn import_settings(
        &mut self,
        settings: &[Setting],
        now_ms: f64,
        include_device_local: bool,
    ) -> ImportOutcome {
        let mut out = ImportOutcome::default();
        for s in settings {
            let scope = lookup(&s.key).map(|d| d.scope);
            if !include_device_local && scope == Some(SettingScope::DeviceLocal) {
                out.skipped_device_local.push(s.key.clone());
                continue;
            }
            let Ok(value) = serde_json::from_str::<Value>(&s.value) else {
                out.skipped.push(s.key.clone());
                continue;
            };
            let before = self.get(&s.key);
            if before == value {
                out.applied.push(s.key.clone());
                continue;
            }
            match self.set_value(&s.key, value, now_ms) {
                Ok(_) => {
                    if scope == Some(SettingScope::AccountSynced) && self.get(&s.key) != before {
                        out.changed_synced.push(s.key.clone());
                    }
                    out.applied.push(s.key.clone());
                }
                Err(_) => out.skipped.push(s.key.clone()),
            }
        }
        out
    }
}

/// What [`Settings::import_settings`] did.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ImportOutcome {
    /// Keys set from the document.
    pub applied: Vec<String>,
    /// Unknown keys and invalid values.
    pub skipped: Vec<String>,
    /// Device-local keys left alone (`include_device_local == false`).
    pub skipped_device_local: Vec<String>,
    /// Account-synced keys whose value changed: broadcast these.
    pub changed_synced: Vec<String>,
}

/// Pure last-write-wins merge of account-synced settings. Local entries are
/// replaced by a remote entry for the same key when the remote is strictly
/// newer and valid; remote keys unknown locally are added when the registry
/// knows them as synced. Returns the merged list and the keys that changed.
pub fn merge(local: &[Setting], remote: &[Setting]) -> (Vec<Setting>, Vec<String>) {
    let mut merged: Vec<Setting> = local.to_vec();
    let mut changed = Vec::new();
    for r in remote {
        let Some(def) = lookup(&r.key) else { continue };
        if def.scope != SettingScope::AccountSynced || r.scope != SettingScope::AccountSynced {
            continue;
        }
        let Ok(value) = serde_json::from_str::<Value>(&r.value) else {
            continue;
        };
        if validate(def, &value).is_err() {
            continue;
        }
        match merged.iter_mut().find(|l| l.key == r.key) {
            Some(l) => {
                if r.updated_at > l.updated_at && l.value != r.value {
                    *l = r.clone();
                    changed.push(r.key.clone());
                } else if r.updated_at > l.updated_at {
                    l.updated_at = r.updated_at;
                }
            }
            None => {
                merged.push(r.clone());
                changed.push(r.key.clone());
            }
        }
    }
    (merged, changed)
}
