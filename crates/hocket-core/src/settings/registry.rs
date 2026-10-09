//! The settings registry: every key with its scope, type, default and
//! validation. Adding a setting means adding a row here (and its text in
//! [`super::strings`]); everything else is generic.

use serde_json::{json, Value};

use crate::api::{AutoplaySettings, SettingScope};

/// Value type of a setting, used for validation and for the settings UI.
#[derive(Debug, Clone, PartialEq)]
pub enum SettingKind {
    Bool,
    /// Integer in `[min, max]`.
    Int {
        min: i64,
        max: i64,
    },
    /// Float in `[min, max]`; `null` allowed when `nullable`.
    Float {
        min: f64,
        max: f64,
        nullable: bool,
    },
    /// Free text; `None` allowed when `nullable`.
    Text {
        nullable: bool,
        max_len: usize,
    },
    /// One of a fixed set.
    Enum(&'static [&'static str]),
    /// Opaque JSON validated by `validate`.
    Json,
}

/// Extra validation hook for a setting value.
pub type Validator = fn(&Value) -> Result<(), String>;

/// Registry row.
pub struct SettingDef {
    pub key: &'static str,
    pub scope: SettingScope,
    pub kind: SettingKind,
    /// Default value (JSON).
    pub default: fn() -> Value,
    /// Extra validation for `Json` kinds (and any kind that needs more than
    /// the shape check). Returns a reason on failure.
    pub validate: Option<Validator>,
}

/// Well-known keys. Keep in step with [`REGISTRY`] and [`super::strings`].
pub mod keys {
    pub const QUEUE_MODE: &str = "queue.mode";
    pub const QUEUE_SAVED_CAP: &str = "queue.savedCap";
    pub const QUEUE_HISTORY_CAP: &str = "queue.historyCap";
    pub const AUTOPLAY_SETTINGS: &str = "autoplay.settings";
    pub const AUTOPLAY_CONTEXT_OVERRIDES: &str = "autoplay.contextOverrides";
    pub const AUDIO_SETTINGS: &str = "audio.settings";
    pub const TRANSCODING_PROFILES: &str = "transcoding.profiles";
    pub const LYRICS_EXTERNAL_ENABLED: &str = "lyrics.external.enabled";
    pub const LYRICS_EXTERNAL_PROVIDER: &str = "lyrics.external.provider";
    pub const LYRICS_DEFAULT_OFFSET_MS: &str = "lyrics.defaultOffsetMs";
    pub const LYRICS_SHOW_TRANSLATIONS: &str = "lyrics.showTranslations";
    pub const RATINGS_LOVE_BRIDGE_ENABLED: &str = "ratings.loveBridge.enabled";
    pub const RATINGS_LOVE_BRIDGE_THRESHOLD: &str = "ratings.loveBridge.threshold";
    pub const BATTERY_AUTO_ENGAGE: &str = "battery.autoEngage";
    /// Whether other apps (Android Auto, Wear, media browsers) may browse the library and control
    /// playback. The system's own controls (notification, lock screen, Bluetooth) always work.
    pub const MEDIA_EXTERNAL_CONTROL: &str = "media.externalControl";
    pub const BATTERY_LYRICS_FPS: &str = "battery.lyricsFps";
    pub const BATTERY_SMALL_ARTWORK: &str = "battery.smallArtwork";
    pub const BATTERY_PAUSE_PREFETCH: &str = "battery.pausePrefetch";
    pub const DISPLAY_ANIMATED_BACKGROUND: &str = "display.animatedBackground";
    pub const DISPLAY_LYRICS_FPS: &str = "display.lyricsFps";
    pub const DISPLAY_THEME: &str = "display.theme";
    pub const DISPLAY_ACCENT: &str = "display.accent";
    pub const DISPLAY_DYNAMIC_COLOUR: &str = "display.dynamicColour";
    pub const DISPLAY_QUEUE_PANEL_SPLIT: &str = "display.queuePanelSplit";
    pub const ACTIONS_ORDER_CONTEXT_MENU: &str = "actions.order.contextMenu";
    pub const ACTIONS_ORDER_SIDEBAR: &str = "actions.order.sidebar";
    pub const ACTIONS_ORDER_MEDIA_SESSION: &str = "actions.order.mediaSession";
    /// The phone's bottom bar: ordered place ids (the app's own, not action ids), [] = its default.
    /// Synced between phones; separate from the desktop sidebar's `actions.order.sidebar`.
    pub const NAV_MOBILE_BAR: &str = "nav.mobileBar";
    pub const SWIPE_QUEUE_START_TO_END: &str = "swipe.queue.startToEnd";
    pub const SWIPE_QUEUE_END_TO_START: &str = "swipe.queue.endToStart";
    pub const SWIPE_LIST_START_TO_END: &str = "swipe.list.startToEnd";
    pub const SWIPE_LIST_END_TO_START: &str = "swipe.list.endToStart";
    pub const SHORTCUTS: &str = "shortcuts";
    pub const SYNC_ENABLED: &str = "sync.enabled";
    pub const CONNECT_COORDINATOR_URL: &str = "connect.coordinatorUrl";
    pub const CONNECT_LAN_DISCOVERY: &str = "connect.lanDiscovery";
    pub const CONNECT_ALLOW_INSECURE_COORDINATOR: &str = "connect.allowInsecureCoordinator";
    pub const STORAGE_WARN_THRESHOLD_BYTES: &str = "storage.warnThresholdBytes";
    pub const STORAGE_CACHE_MAX_BYTES: &str = "storage.cacheMaxBytes";
    pub const STORAGE_PREFETCH_ON_MOBILE_DATA: &str = "storage.prefetchOnMobileData";
    pub const DOWNLOADS_TRANSCODE: &str = "downloads.transcode";
    pub const DOWNLOADS_WIFI_ONLY: &str = "downloads.wifiOnly";
    pub const SLEEP_DEFAULT_MINUTES: &str = "sleep.defaultMinutes";
    pub const SLEEP_STOP_AT_END_OF_TRACK: &str = "sleep.stopAtEndOfTrack";
    pub const SCROBBLE_ENABLED: &str = "scrobble.enabled";
    pub const SCROBBLE_NOW_PLAYING: &str = "scrobble.nowPlaying";
    pub const LIBRARY_SYNC_INTERVAL_MINUTES: &str = "library.syncIntervalMinutes";
    pub const LIBRARY_FULL_RECONCILE_DAYS: &str = "library.fullReconcileDays";
    pub const SEARCH_INCLUDE_SERVER: &str = "search.includeServer";

    /// `actions.order.<surface>` for a customisable surface.
    pub fn action_order(surface: &str) -> String {
        format!("actions.order.{surface}")
    }
}

use keys::*;
use SettingScope::{AccountSynced as Synced, DeviceLocal as Local};

const GIB: f64 = 1024.0 * 1024.0 * 1024.0;

const fn def(
    key: &'static str,
    scope: SettingScope,
    kind: SettingKind,
    default: fn() -> Value,
) -> SettingDef {
    SettingDef {
        key,
        scope,
        kind,
        default,
        validate: None,
    }
}

const fn json_def(
    key: &'static str,
    scope: SettingScope,
    default: fn() -> Value,
    validate: Validator,
) -> SettingDef {
    SettingDef {
        key,
        scope,
        kind: SettingKind::Json,
        default,
        validate: Some(validate),
    }
}

fn autoplay_default() -> Value {
    serde_json::to_value(crate::autoplay::default_settings()).unwrap_or(Value::Null)
}

fn autoplay_valid(v: &Value) -> Result<(), String> {
    serde_json::from_value::<AutoplaySettings>(v.clone())
        .map(|_| ())
        .map_err(|e| e.to_string())
}

fn audio_default() -> Value {
    serde_json::to_value(crate::core::default_audio_settings()).unwrap_or(Value::Null)
}

fn audio_valid(v: &Value) -> Result<(), String> {
    serde_json::from_value::<crate::api::AudioSettings>(v.clone())
        .map(|_| ())
        .map_err(|e| e.to_string())
}

fn overrides_valid(v: &Value) -> Result<(), String> {
    // {"album": ["similarSongs", ...], "artist": [...]}
    let obj = v.as_object().ok_or("expected an object")?;
    for (k, chain) in obj {
        if !matches!(
            k.as_str(),
            "album" | "artist" | "playlist" | "genre" | "filter" | "adHoc" | "autoplay"
        ) {
            return Err(format!("unknown context '{k}'"));
        }
        serde_json::from_value::<Vec<crate::api::AutoplayProvider>>(chain.clone())
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn profiles_valid(v: &Value) -> Result<(), String> {
    // {"<networkId>|default": TranscodingProfile}
    let obj = v.as_object().ok_or("expected an object")?;
    for (k, p) in obj {
        if k.is_empty() {
            return Err("empty network id".into());
        }
        serde_json::from_value::<crate::api::TranscodingProfile>(p.clone())
            .map_err(|e| format!("{k}: {e}"))?;
    }
    Ok(())
}

fn string_list_valid(v: &Value) -> Result<(), String> {
    let arr = v.as_array().ok_or("expected a list")?;
    if arr.iter().all(Value::is_string) {
        Ok(())
    } else {
        Err("expected a list of strings".into())
    }
}

fn shortcuts_valid(v: &Value) -> Result<(), String> {
    // {"<actionId>": "<chord>" | null}
    let obj = v.as_object().ok_or("expected an object")?;
    for (k, c) in obj {
        if k.is_empty() {
            return Err("empty action id".into());
        }
        if !(c.is_null() || c.is_string()) {
            return Err(format!("{k}: expected a chord string or null"));
        }
    }
    Ok(())
}

fn accent_valid(v: &Value) -> Result<(), String> {
    match v {
        Value::Null => Ok(()),
        Value::String(s) if s.is_empty() => Ok(()),
        Value::String(s) => {
            let hex = s.strip_prefix('#').unwrap_or(s);
            if (hex.len() == 6 || hex.len() == 8) && hex.chars().all(|c| c.is_ascii_hexdigit()) {
                Ok(())
            } else {
                Err("expected #RRGGBB".into())
            }
        }
        _ => Err("expected a colour string".into()),
    }
}

/// `connect.coordinatorUrl`: `wss://` anywhere, `ws://` only to a
/// loopback or private host (and the engine sends the credential over
/// `ws://` to a private host only when `connect.allowInsecureCoordinator`
/// is on: see `connect::auth::coordinator_url_check`).
fn url_valid(v: &Value) -> Result<(), String> {
    match v {
        Value::Null => Ok(()),
        Value::String(s) if s.is_empty() => Ok(()),
        Value::String(s) => crate::connect::auth::coordinator_url_check(s, true),
        _ => Err("expected a URL".into()),
    }
}

/// What a swipe on a queue row can do: an action registry id, or `none`.
/// Kept to actions that make sense for one queue item without a dialog
/// (`addToPlaylist` opens the platform's picker).
pub const SWIPE_QUEUE_ACTIONS: &[&str] = &[
    "none",
    "removeFromQueue",
    "playNext",
    "love",
    "addToPlaylist",
    "download",
];

/// What a swipe on a song row in any other list can do (album, playlist,
/// search results, library songs): an action registry id, or `none`.
pub const SWIPE_LIST_ACTIONS: &[&str] = &[
    "none",
    "playNext",
    "playLater",
    "love",
    "addToPlaylist",
    "download",
];

/// Every setting. Order is the settings screen's order.
pub static REGISTRY: &[SettingDef] = &[
    def(
        QUEUE_MODE,
        Synced,
        SettingKind::Enum(&["apple", "youTube"]),
        || json!("apple"),
    ),
    def(
        QUEUE_SAVED_CAP,
        Synced,
        SettingKind::Int { min: 0, max: 50 },
        || json!(10),
    ),
    def(
        QUEUE_HISTORY_CAP,
        Synced,
        SettingKind::Int { min: 10, max: 1000 },
        || json!(200),
    ),
    json_def(AUTOPLAY_SETTINGS, Synced, autoplay_default, autoplay_valid),
    json_def(
        AUTOPLAY_CONTEXT_OVERRIDES,
        Synced,
        || json!({ "album": ["similarSongs", "sonicSimilarity", "topSongs", "random", "savedFilter"], "artist": ["topSongs", "similarSongs", "sonicSimilarity", "random", "savedFilter"] }),
        overrides_valid,
    ),
    json_def(AUDIO_SETTINGS, Local, audio_default, audio_valid),
    json_def(
        TRANSCODING_PROFILES,
        Local,
        || json!({ "default": { "format": null, "maxBitRate": null, "cannotDecode": [] }, "cellular": { "format": "opus", "maxBitRate": 128, "cannotDecode": [] } }),
        profiles_valid,
    ),
    // Device-local on purpose: fetching external lyrics reveals what you
    // are listening to, so enabling it on one device must not silently
    // enable it on a shared one (the provider choice stays synced).
    def(LYRICS_EXTERNAL_ENABLED, Local, SettingKind::Bool, || {
        json!(false)
    }),
    def(
        LYRICS_EXTERNAL_PROVIDER,
        Synced,
        SettingKind::Enum(&["lrclib"]),
        || json!("lrclib"),
    ),
    def(
        LYRICS_DEFAULT_OFFSET_MS,
        Local,
        SettingKind::Int {
            min: -10_000,
            max: 10_000,
        },
        || json!(0),
    ),
    def(LYRICS_SHOW_TRANSLATIONS, Synced, SettingKind::Bool, || {
        json!(true)
    }),
    def(
        RATINGS_LOVE_BRIDGE_ENABLED,
        Synced,
        SettingKind::Bool,
        || json!(false),
    ),
    def(
        RATINGS_LOVE_BRIDGE_THRESHOLD,
        Synced,
        SettingKind::Int { min: 1, max: 5 },
        || json!(4),
    ),
    def(BATTERY_AUTO_ENGAGE, Local, SettingKind::Bool, || {
        json!(true)
    }),
    // Device-local: letting other apps in is a decision about this device, never synced.
    def(MEDIA_EXTERNAL_CONTROL, Local, SettingKind::Bool, || {
        json!(false)
    }),
    def(
        BATTERY_LYRICS_FPS,
        Local,
        SettingKind::Int { min: 10, max: 60 },
        || json!(30),
    ),
    def(BATTERY_SMALL_ARTWORK, Local, SettingKind::Bool, || {
        json!(true)
    }),
    def(BATTERY_PAUSE_PREFETCH, Local, SettingKind::Bool, || {
        json!(true)
    }),
    def(
        DISPLAY_ANIMATED_BACKGROUND,
        Local,
        SettingKind::Bool,
        || json!(true),
    ),
    def(
        DISPLAY_LYRICS_FPS,
        Local,
        SettingKind::Int { min: 24, max: 144 },
        || json!(60),
    ),
    def(
        DISPLAY_THEME,
        Local,
        SettingKind::Enum(&["system", "light", "dark"]),
        || json!("system"),
    ),
    json_def(DISPLAY_ACCENT, Local, || Value::Null, accent_valid),
    def(DISPLAY_DYNAMIC_COLOUR, Local, SettingKind::Bool, || {
        json!(true)
    }),
    def(
        DISPLAY_QUEUE_PANEL_SPLIT,
        Local,
        SettingKind::Float {
            min: 0.0,
            max: 1.0,
            nullable: false,
        },
        || json!(0.5),
    ),
    json_def(
        ACTIONS_ORDER_CONTEXT_MENU,
        Synced,
        || json!([]),
        string_list_valid,
    ),
    json_def(
        ACTIONS_ORDER_SIDEBAR,
        Synced,
        || json!([]),
        string_list_valid,
    ),
    json_def(
        ACTIONS_ORDER_MEDIA_SESSION,
        Synced,
        || json!([]),
        string_list_valid,
    ),
    json_def(NAV_MOBILE_BAR, Synced, || json!([]), string_list_valid),
    // Swipe actions on song rows. `love` toggles (love / unlove by the row's
    // state); the platform runs the chosen id through `RunAction`.
    def(
        SWIPE_QUEUE_START_TO_END,
        Synced,
        SettingKind::Enum(SWIPE_QUEUE_ACTIONS),
        || json!("removeFromQueue"),
    ),
    def(
        SWIPE_QUEUE_END_TO_START,
        Synced,
        SettingKind::Enum(SWIPE_QUEUE_ACTIONS),
        || json!("removeFromQueue"),
    ),
    def(
        SWIPE_LIST_START_TO_END,
        Synced,
        SettingKind::Enum(SWIPE_LIST_ACTIONS),
        || json!("playNext"),
    ),
    def(
        SWIPE_LIST_END_TO_START,
        Synced,
        SettingKind::Enum(SWIPE_LIST_ACTIONS),
        || json!("playLater"),
    ),
    json_def(SHORTCUTS, Local, || json!({}), shortcuts_valid),
    def(SYNC_ENABLED, Local, SettingKind::Bool, || json!(true)),
    json_def(CONNECT_COORDINATOR_URL, Local, || Value::Null, url_valid),
    def(
        CONNECT_ALLOW_INSECURE_COORDINATOR,
        Local,
        SettingKind::Bool,
        || json!(false),
    ),
    def(CONNECT_LAN_DISCOVERY, Local, SettingKind::Bool, || {
        json!(true)
    }),
    def(
        STORAGE_WARN_THRESHOLD_BYTES,
        Local,
        SettingKind::Float {
            min: 0.0,
            max: 1.0e15,
            nullable: false,
        },
        || json!(4.0 * GIB),
    ),
    // `null` (the default) is automatic: sized from the cache volume's free
    // space. Any number, 2 GiB included, is the user's own budget.
    def(
        STORAGE_CACHE_MAX_BYTES,
        Local,
        SettingKind::Float {
            min: 64.0 * 1024.0 * 1024.0,
            max: 1.0e15,
            nullable: true,
        },
        || Value::Null,
    ),
    def(
        STORAGE_PREFETCH_ON_MOBILE_DATA,
        Local,
        SettingKind::Bool,
        || json!(false),
    ),
    def(DOWNLOADS_TRANSCODE, Local, SettingKind::Bool, || {
        json!(false)
    }),
    def(DOWNLOADS_WIFI_ONLY, Local, SettingKind::Bool, || {
        json!(true)
    }),
    def(
        SLEEP_DEFAULT_MINUTES,
        Synced,
        SettingKind::Int { min: 1, max: 720 },
        || json!(30),
    ),
    def(
        SLEEP_STOP_AT_END_OF_TRACK,
        Synced,
        SettingKind::Bool,
        || json!(true),
    ),
    def(SCROBBLE_ENABLED, Synced, SettingKind::Bool, || json!(true)),
    def(SCROBBLE_NOW_PLAYING, Synced, SettingKind::Bool, || {
        json!(true)
    }),
    def(
        LIBRARY_SYNC_INTERVAL_MINUTES,
        Local,
        SettingKind::Int { min: 5, max: 1440 },
        || json!(60),
    ),
    def(
        LIBRARY_FULL_RECONCILE_DAYS,
        Local,
        SettingKind::Int { min: 1, max: 90 },
        || json!(7),
    ),
    def(SEARCH_INCLUDE_SERVER, Synced, SettingKind::Bool, || {
        json!(true)
    }),
];

/// Registry row for a key.
pub fn lookup(key: &str) -> Option<&'static SettingDef> {
    REGISTRY.iter().find(|d| d.key == key)
}

/// Validates a value against a definition.
pub fn validate(def: &SettingDef, value: &Value) -> Result<(), String> {
    match &def.kind {
        SettingKind::Bool => {
            if !value.is_boolean() {
                return Err("expected true or false".into());
            }
        }
        SettingKind::Int { min, max } => {
            let n = value
                .as_i64()
                .or_else(|| {
                    value
                        .as_f64()
                        .filter(|f| f.fract() == 0.0)
                        .map(|f| f as i64)
                })
                .ok_or("expected an integer")?;
            if n < *min || n > *max {
                return Err(format!("must be between {min} and {max}"));
            }
        }
        SettingKind::Float { nullable, .. } if value.is_null() && *nullable => {}
        SettingKind::Float { min, max, .. } => {
            let n = value.as_f64().ok_or("expected a number")?;
            if !n.is_finite() || n < *min || n > *max {
                return Err(format!("must be between {min} and {max}"));
            }
        }
        SettingKind::Text { nullable, max_len } => match value {
            Value::Null if *nullable => {}
            Value::String(s) if s.len() <= *max_len => {}
            Value::String(_) => return Err(format!("longer than {max_len} characters")),
            _ => return Err("expected text".into()),
        },
        SettingKind::Enum(options) => {
            let s = value.as_str().ok_or("expected one of the options")?;
            if !options.contains(&s) {
                return Err(format!("must be one of {}", options.join(", ")));
            }
        }
        SettingKind::Json => {}
    }
    if let Some(v) = def.validate {
        v(value)?;
    }
    Ok(())
}
