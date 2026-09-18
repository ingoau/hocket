//! Config backup: a versioned [`ConfigDocument`] with forward migrations.
//!
//! Secrets never enter the document unless `include_secrets`, and even then
//! only as opaque keystore references (`keystore://hocket/server/<id>/password`)
//! that the platform keystore resolves on import; the core never sees
//! plaintext.

use std::collections::HashMap;

use serde_json::{json, Map, Value};

use crate::api::{AudioSettings, AutoplaySettings, ConfigDocument, Filter, ServerInfo, Setting, Shortcut};

/// Current document version.
pub const CONFIG_VERSION: u32 = 1;

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum ConfigError {
    #[error("not a config document: {0}")]
    Invalid(String),
    #[error("config document version {0} is newer than this app understands ({CONFIG_VERSION})")]
    TooNew(u32),
}

/// Everything that goes into a backup.
#[derive(Debug, Clone, Default)]
pub struct ConfigInputs {
    pub settings: Vec<Setting>,
    pub filters: Vec<Filter>,
    pub shortcuts: Vec<Shortcut>,
    pub servers: Vec<ServerInfo>,
    pub audio: Option<AudioSettings>,
    pub autoplay: Option<AutoplaySettings>,
}

/// Opaque keystore reference for a server's password.
pub fn secret_reference(server_id: &str) -> String {
    format!("keystore://hocket/server/{server_id}/password")
}

/// Secret map key for a server's password.
pub fn secret_key(server_id: &str) -> String {
    format!("server:{server_id}:password")
}

/// Builds a document. With `include_secrets`, the `secrets` map holds one
/// keystore reference per server; without, it is absent.
pub fn build_document(inputs: ConfigInputs, include_secrets: bool, now_ms: f64) -> ConfigDocument {
    let secrets = include_secrets.then(|| inputs.servers.iter().map(|s| (secret_key(&s.id), secret_reference(&s.id))).collect::<HashMap<_, _>>());
    ConfigDocument {
        version: CONFIG_VERSION,
        exported_at: now_ms,
        settings: inputs.settings,
        filters: inputs.filters,
        shortcuts: inputs.shortcuts,
        servers: inputs.servers,
        secrets,
        audio: inputs.audio.unwrap_or_else(crate::core::default_audio_settings),
        autoplay: inputs.autoplay.unwrap_or_else(crate::autoplay::default_settings),
        extra: HashMap::new(),
    }
}

/// Serialises a document.
pub fn to_json(doc: &ConfigDocument) -> Result<String, ConfigError> {
    serde_json::to_string_pretty(doc).map_err(|e| ConfigError::Invalid(e.to_string()))
}

/// Parses and migrates a document of any supported version to
/// [`CONFIG_VERSION`]. Unknown top-level fields are kept in `extra`.
pub fn parse_document(json: &str) -> Result<ConfigDocument, ConfigError> {
    let value: Value = serde_json::from_str(json).map_err(|e| ConfigError::Invalid(e.to_string()))?;
    let mut obj = match value {
        Value::Object(o) => o,
        _ => return Err(ConfigError::Invalid("top level must be an object".into())),
    };
    let version = obj.get("version").and_then(Value::as_u64).unwrap_or(0) as u32;
    if version > CONFIG_VERSION {
        return Err(ConfigError::TooNew(version));
    }
    let mut v = version;
    while v < CONFIG_VERSION {
        obj = match v {
            0 => migrate_v0_to_v1(obj)?,
            _ => return Err(ConfigError::Invalid(format!("no migration from version {v}"))),
        };
        v += 1;
    }
    // Preserve unknown top-level fields verbatim.
    let known = ["version", "exportedAt", "settings", "filters", "shortcuts", "servers", "secrets", "audio", "autoplay", "extra"];
    let mut extra: HashMap<String, String> = obj
        .get("extra")
        .and_then(Value::as_object)
        .map(|m| m.iter().map(|(k, v)| (k.clone(), v.as_str().map(str::to_string).unwrap_or_else(|| v.to_string()))).collect())
        .unwrap_or_default();
    let unknown: Vec<String> = obj.keys().filter(|k| !known.contains(&k.as_str())).cloned().collect();
    for k in unknown {
        if let Some(v) = obj.remove(&k) {
            extra.insert(k, v.to_string());
        }
    }
    obj.insert("extra".into(), json!(extra));
    fill_defaults(&mut obj);
    serde_json::from_value(Value::Object(obj)).map_err(|e| ConfigError::Invalid(e.to_string()))
}

fn fill_defaults(obj: &mut Map<String, Value>) {
    obj.entry("exportedAt").or_insert(json!(0.0));
    for k in ["settings", "filters", "shortcuts", "servers"] {
        obj.entry(k).or_insert(json!([]));
    }
    if !obj.get("audio").is_some_and(|a| serde_json::from_value::<AudioSettings>(a.clone()).is_ok()) {
        obj.insert("audio".into(), serde_json::to_value(crate::core::default_audio_settings()).unwrap_or(Value::Null));
    }
    if !obj.get("autoplay").is_some_and(|a| serde_json::from_value::<AutoplaySettings>(a.clone()).is_ok()) {
        obj.insert("autoplay".into(), serde_json::to_value(crate::autoplay::default_settings()).unwrap_or(Value::Null));
    }
}

/// v0 (pre-release) shape: `settings` was a flat `{key: value}` map with no
/// scope or timestamp, `shortcuts` a `{actionId: chord}` map, and there was
/// no `autoplay` block. Scopes are re-derived from the registry.
fn migrate_v0_to_v1(mut obj: Map<String, Value>) -> Result<Map<String, Value>, ConfigError> {
    if let Some(Value::Object(map)) = obj.remove("settings") {
        let settings: Vec<Value> = map
            .into_iter()
            .map(|(key, value)| {
                let scope = super::registry::lookup(&key).map(|d| d.scope).unwrap_or(crate::api::SettingScope::DeviceLocal);
                json!({ "key": key, "value": value.to_string(), "scope": scope, "updatedAt": 0.0 })
            })
            .collect();
        obj.insert("settings".into(), Value::Array(settings));
    }
    if let Some(Value::Object(map)) = obj.remove("shortcuts") {
        let shortcuts: Vec<Value> = map
            .into_iter()
            .map(|(action_id, chord)| json!({ "actionId": action_id, "shortcut": chord, "defaultShortcut": null }))
            .collect();
        obj.insert("shortcuts".into(), Value::Array(shortcuts));
    }
    obj.insert("version".into(), json!(1));
    Ok(obj)
}
