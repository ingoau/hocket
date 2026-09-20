use serde_json::json;

use super::*;
use crate::api::{AutoplaySettings, Filter, FilterNode, ServerInfo, Shortcut};

#[test]
fn registry_is_complete_and_consistent() {
    let mut seen = std::collections::HashSet::new();
    for def in REGISTRY {
        assert!(seen.insert(def.key), "duplicate key {}", def.key);
        let d = (def.default)();
        validate(def, &d).unwrap_or_else(|e| panic!("default for {} is invalid: {e}", def.key));
        assert!(
            strings::text_for(def.key).is_some(),
            "no strings for {}",
            def.key
        );
        assert!(!strings::label(def.key).is_empty());
        assert!(!strings::description(def.key).is_empty());
    }
    for t in strings::TEXTS {
        assert!(lookup(t.key).is_some(), "strings for unknown key {}", t.key);
    }
    // The master toggle must never itself be synced.
    assert_eq!(
        lookup(keys::SYNC_ENABLED).unwrap().scope,
        SettingScope::DeviceLocal
    );
    assert_eq!(
        lookup(keys::CONNECT_COORDINATOR_URL).unwrap().scope,
        SettingScope::DeviceLocal
    );
    assert_eq!(keys::action_order("sidebar"), keys::ACTIONS_ORDER_SIDEBAR);
}

#[test]
fn defaults_match_the_design() {
    let s = Settings::new();
    assert!(s.sync_enabled());
    assert!(!s.get_bool(keys::LYRICS_EXTERNAL_ENABLED));
    assert!(!s.get_bool(keys::RATINGS_LOVE_BRIDGE_ENABLED));
    assert_eq!(s.get_i64(keys::RATINGS_LOVE_BRIDGE_THRESHOLD), 4);
    assert!(s.get_bool(keys::BATTERY_AUTO_ENGAGE));
    assert_eq!(s.get_i64(keys::BATTERY_LYRICS_FPS), 30);
    assert!(s.get_bool(keys::DISPLAY_ANIMATED_BACKGROUND));
    assert_eq!(s.get_i64(keys::QUEUE_SAVED_CAP), 10);
    assert_eq!(s.get_string(keys::QUEUE_MODE).as_deref(), Some("apple"));
    assert_eq!(s.get_string(keys::CONNECT_COORDINATOR_URL), None);
    let ap: AutoplaySettings = s.get_typed(keys::AUTOPLAY_SETTINGS).unwrap();
    assert_eq!(ap, crate::autoplay::default_settings());
    let audio: crate::api::AudioSettings = s.get_typed(keys::AUDIO_SETTINGS).unwrap();
    assert!(audio.gapless);
    assert_eq!(s.get("nope"), serde_json::Value::Null);
    assert_eq!(s.to_api().len(), REGISTRY.len());
    assert!(s.to_api().iter().all(|x| x.updated_at == 0.0));
}

#[test]
fn set_validate_reset_and_persist() {
    let store = MemoryStore::default();
    let mut s = Settings::load(&store).unwrap();
    let ev = s.set_json(keys::QUEUE_SAVED_CAP, "25", 100.0).unwrap();
    assert_eq!(
        (ev.key.as_str(), ev.value.as_str(), ev.scope, ev.updated_at),
        (
            keys::QUEUE_SAVED_CAP,
            "25",
            SettingScope::AccountSynced,
            100.0
        )
    );
    assert!(matches!(
        s.set_json(keys::QUEUE_SAVED_CAP, "51", 101.0),
        Err(SettingsError::Invalid { .. })
    ));
    assert!(matches!(
        s.set_json(keys::QUEUE_SAVED_CAP, "\"ten\"", 101.0),
        Err(SettingsError::Invalid { .. })
    ));
    assert!(matches!(
        s.set_json(keys::QUEUE_SAVED_CAP, "not json", 101.0),
        Err(SettingsError::Json(_))
    ));
    assert!(matches!(
        s.set_json("made.up", "1", 101.0),
        Err(SettingsError::UnknownKey(_))
    ));
    assert!(matches!(
        s.set_json(keys::QUEUE_MODE, "\"spotify\"", 101.0),
        Err(SettingsError::Invalid { .. })
    ));
    assert!(s.set_json(keys::QUEUE_MODE, "\"youTube\"", 101.0).is_ok());
    assert!(s
        .set_json(keys::DISPLAY_ACCENT, "\"#ff8800\"", 101.0)
        .is_ok());
    assert!(s
        .set_json(keys::DISPLAY_ACCENT, "\"orange\"", 101.0)
        .is_err());
    assert!(s.set_json(keys::DISPLAY_ACCENT, "null", 101.0).is_ok());
    assert!(s
        .set_json(
            keys::CONNECT_COORDINATOR_URL,
            "\"wss://hocket.example/ws\"",
            101.0
        )
        .is_ok());
    assert!(s
        .set_json(keys::CONNECT_COORDINATOR_URL, "\"ftp://x\"", 101.0)
        .is_err());
    assert!(s.set_json(keys::AUTOPLAY_SETTINGS, r#"{"chain":["random"],"seedWindow":3,"minSimilarity":null,"exclusionWindow":10,"filterId":null}"#, 102.0).is_ok());
    assert!(s
        .set_json(keys::AUTOPLAY_SETTINGS, r#"{"chain":"random"}"#, 102.0)
        .is_err());
    assert!(s
        .set_json(
            keys::AUTOPLAY_CONTEXT_OVERRIDES,
            r#"{"album":["random"]}"#,
            102.0
        )
        .is_ok());
    assert!(s
        .set_json(
            keys::AUTOPLAY_CONTEXT_OVERRIDES,
            r#"{"podcast":["random"]}"#,
            102.0
        )
        .is_err());
    assert!(s
        .set_json(
            keys::TRANSCODING_PROFILES,
            r#"{"cellular":{"format":"opus","maxBitRate":96,"cannotDecode":["dsf"]}}"#,
            102.0
        )
        .is_ok());
    assert!(s
        .set_json(keys::SHORTCUTS, r#"{"play":"Space","next":null}"#, 102.0)
        .is_ok());
    assert!(s.set_json(keys::SHORTCUTS, r#"{"play":1}"#, 102.0).is_err());
    assert!(s
        .set_json(keys::ACTIONS_ORDER_SIDEBAR, r#"["home","albums"]"#, 102.0)
        .is_ok());
    assert!(s
        .set_json(keys::ACTIONS_ORDER_SIDEBAR, r#"[1]"#, 102.0)
        .is_err());
    assert!(s
        .set_typed(keys::DISPLAY_QUEUE_PANEL_SPLIT, &0.7, 103.0)
        .is_ok());
    assert!(s
        .set_typed(keys::DISPLAY_QUEUE_PANEL_SPLIT, &1.7, 103.0)
        .is_err());

    let ev = s.reset(keys::QUEUE_SAVED_CAP, 200.0).unwrap();
    assert_eq!((ev.value.as_str(), ev.updated_at), ("10", 200.0));
    assert!(s.is_set(keys::QUEUE_SAVED_CAP));

    s.save(&store).unwrap();
    let back = Settings::load(&store).unwrap();
    assert_eq!(back.to_api(), s.to_api());
    assert_eq!(
        back.get_string(keys::QUEUE_MODE).as_deref(),
        Some("youTube")
    );
    let ap: AutoplaySettings = back.get_typed(keys::AUTOPLAY_SETTINGS).unwrap();
    assert_eq!(ap.seed_window, 3);

    // Garbage in the store falls back to defaults rather than failing start-up.
    store.save("{{{").unwrap();
    assert_eq!(
        Settings::load(&store).unwrap().to_api(),
        Settings::new().to_api()
    );

    // Unknown keys from a newer app survive a round trip, with their scope.
    let doc = r#"{"version":2,"entries":{"future.key":{"value":42,"updatedAt":5.0,"scope":"accountSynced"},"queue.savedCap":{"value":999,"updatedAt":1.0}}}"#;
    let s = Settings::from_json(doc).unwrap();
    assert_eq!(s.get("future.key"), json!(42));
    assert_eq!(s.scope_of("future.key"), Some(SettingScope::AccountSynced));
    assert_eq!(
        s.get_i64(keys::QUEUE_SAVED_CAP),
        10,
        "out-of-range stored value dropped"
    );
    let api = s.to_api();
    assert_eq!(api.last().unwrap().key, "future.key");
    let again = Settings::from_json(&s.to_json().unwrap()).unwrap();
    assert_eq!(again.get("future.key"), json!(42));
}

fn setting(key: &str, value: &str, scope: SettingScope, at: f64) -> Setting {
    Setting {
        key: key.into(),
        value: value.into(),
        scope,
        updated_at: at,
    }
}

#[test]
fn lww_merge_is_pure_and_scoped() {
    let local = vec![
        setting(
            keys::QUEUE_SAVED_CAP,
            "10",
            SettingScope::AccountSynced,
            100.0,
        ),
        setting(
            keys::SCROBBLE_ENABLED,
            "true",
            SettingScope::AccountSynced,
            300.0,
        ),
    ];
    let remote = vec![
        setting(
            keys::QUEUE_SAVED_CAP,
            "20",
            SettingScope::AccountSynced,
            200.0,
        ), // newer → wins
        setting(
            keys::SCROBBLE_ENABLED,
            "false",
            SettingScope::AccountSynced,
            250.0,
        ), // older → loses
        setting(
            keys::SLEEP_DEFAULT_MINUTES,
            "45",
            SettingScope::AccountSynced,
            50.0,
        ), // new key → added
        setting(
            keys::SYNC_ENABLED,
            "false",
            SettingScope::DeviceLocal,
            999.0,
        ), // device-local → ignored
        setting(
            keys::BATTERY_AUTO_ENGAGE,
            "false",
            SettingScope::AccountSynced,
            999.0,
        ), // registry says local → ignored
        setting(
            keys::QUEUE_HISTORY_CAP,
            "99999",
            SettingScope::AccountSynced,
            999.0,
        ), // invalid → ignored
        setting("unknown.key", "1", SettingScope::AccountSynced, 999.0), // unknown → ignored
    ];
    let (merged, changed) = merge(&local, &remote);
    assert_eq!(
        changed,
        vec![keys::QUEUE_SAVED_CAP, keys::SLEEP_DEFAULT_MINUTES]
    );
    assert_eq!(merged.len(), 3);
    assert_eq!(merged[0].value, "20");
    assert_eq!(merged[0].updated_at, 200.0);
    assert_eq!(merged[1].value, "true");
    assert_eq!(merged[2].key, keys::SLEEP_DEFAULT_MINUTES);
    // Merge is idempotent.
    let (again, changed2) = merge(&merged, &remote);
    assert!(changed2.is_empty());
    assert_eq!(again, merged);
    // Same value with a newer stamp: no change reported, stamp advances.
    let (m, c) = merge(
        &[setting(
            keys::SCROBBLE_ENABLED,
            "true",
            SettingScope::AccountSynced,
            1.0,
        )],
        &[setting(
            keys::SCROBBLE_ENABLED,
            "true",
            SettingScope::AccountSynced,
            2.0,
        )],
    );
    assert!(c.is_empty());
    assert_eq!(m[0].updated_at, 2.0);
}

#[test]
fn merge_remote_respects_master_toggle() {
    let mut s = Settings::new();
    s.set_json(keys::QUEUE_SAVED_CAP, "5", 100.0).unwrap();
    s.set_json(keys::DISPLAY_THEME, "\"dark\"", 100.0).unwrap();
    let synced = s.synced_settings();
    assert_eq!(synced.len(), 1);
    assert_eq!(synced[0].key, keys::QUEUE_SAVED_CAP);

    let remote = vec![
        setting(
            keys::QUEUE_SAVED_CAP,
            "30",
            SettingScope::AccountSynced,
            500.0,
        ),
        setting(
            keys::SCROBBLE_ENABLED,
            "false",
            SettingScope::AccountSynced,
            500.0,
        ),
        setting(
            keys::DISPLAY_THEME,
            "\"light\"",
            SettingScope::AccountSynced,
            500.0,
        ),
    ];
    let changed = s.merge_remote(&remote);
    assert_eq!(changed, vec![keys::QUEUE_SAVED_CAP, keys::SCROBBLE_ENABLED]);
    assert_eq!(s.get_i64(keys::QUEUE_SAVED_CAP), 30);
    assert!(!s.get_bool(keys::SCROBBLE_ENABLED));
    assert_eq!(s.get_string(keys::DISPLAY_THEME).as_deref(), Some("dark"));
    assert_eq!(s.setting(keys::SCROBBLE_ENABLED).unwrap().updated_at, 500.0);

    s.set_json(keys::SYNC_ENABLED, "false", 600.0).unwrap();
    assert!(s.synced_settings().is_empty());
    assert!(s
        .merge_remote(&[setting(
            keys::QUEUE_SAVED_CAP,
            "40",
            SettingScope::AccountSynced,
            700.0
        )])
        .is_empty());
    assert_eq!(s.get_i64(keys::QUEUE_SAVED_CAP), 30);
}

#[test]
fn config_document_builds_and_round_trips() {
    let mut s = Settings::new();
    s.set_json(keys::QUEUE_SAVED_CAP, "7", 10.0).unwrap();
    let server = ServerInfo {
        id: "srv1".into(),
        url: "https://music.example".into(),
        username: "me".into(),
        name: "Home".into(),
        ..Default::default()
    };
    let filter = Filter {
        id: "f1".into(),
        name: "F".into(),
        root: FilterNode::All(vec![]),
        sort: Default::default(),
        descending: false,
        limit: None,
    };
    let inputs = ConfigInputs {
        settings: s.to_api(),
        filters: vec![filter.clone()],
        shortcuts: vec![Shortcut {
            action_id: "play".into(),
            shortcut: Some("Space".into()),
            default_shortcut: Some("Space".into()),
        }],
        servers: vec![server.clone()],
        audio: None,
        autoplay: None,
    };
    let doc = build_document(inputs.clone(), false, 1234.0);
    assert_eq!(doc.version, CONFIG_VERSION);
    assert_eq!(doc.exported_at, 1234.0);
    assert!(doc.secrets.is_none());
    let json = to_json(&doc).unwrap();
    assert!(!json.contains("password"));
    let back = parse_document(&json).unwrap();
    assert_eq!(back, doc);

    let with_secrets = build_document(inputs, true, 1234.0);
    let secrets = with_secrets.secrets.clone().unwrap();
    assert_eq!(
        secrets.get(&secret_key("srv1")).map(String::as_str),
        Some("keystore://hocket/server/srv1/password")
    );
    assert!(!to_json(&with_secrets).unwrap().contains("hunter2"));

    let mut fresh = Settings::new();
    let (applied, skipped) = fresh.apply_settings(&back.settings, 999.0);
    assert_eq!(applied.len(), REGISTRY.len());
    assert!(skipped.is_empty());
    assert_eq!(fresh.get_i64(keys::QUEUE_SAVED_CAP), 7);
    assert_eq!(
        fresh.setting(keys::QUEUE_SAVED_CAP).unwrap().updated_at,
        10.0
    );
    // Unset keys in the export carry updatedAt 0 and get the import time.
    assert_eq!(
        fresh.setting(keys::SCROBBLE_ENABLED).unwrap().updated_at,
        999.0
    );
    let (applied, skipped) = fresh.apply_settings(
        &[
            setting("bogus", "1", SettingScope::DeviceLocal, 1.0),
            setting(keys::QUEUE_SAVED_CAP, "x", SettingScope::AccountSynced, 1.0),
        ],
        1.0,
    );
    assert!(applied.is_empty());
    assert_eq!(skipped, vec!["bogus", keys::QUEUE_SAVED_CAP]);
}

#[test]
fn v0_document_migrates_forward() {
    let v0 = r#"{
        "exportedAt": 5.0,
        "settings": { "queue.savedCap": 3, "display.theme": "dark", "mystery": true },
        "shortcuts": { "play": "Space", "next": null },
        "servers": [],
        "customBlock": { "a": 1 }
    }"#;
    let doc = parse_document(v0).unwrap();
    assert_eq!(doc.version, 1);
    assert_eq!(doc.exported_at, 5.0);
    let cap = doc
        .settings
        .iter()
        .find(|s| s.key == "queue.savedCap")
        .unwrap();
    assert_eq!(
        (cap.value.as_str(), cap.scope, cap.updated_at),
        ("3", SettingScope::AccountSynced, 0.0)
    );
    let theme = doc
        .settings
        .iter()
        .find(|s| s.key == "display.theme")
        .unwrap();
    assert_eq!(
        (theme.value.as_str(), theme.scope),
        ("\"dark\"", SettingScope::DeviceLocal)
    );
    assert!(doc
        .settings
        .iter()
        .any(|s| s.key == "mystery" && s.scope == SettingScope::DeviceLocal));
    assert_eq!(doc.shortcuts.len(), 2);
    assert_eq!(
        doc.shortcuts
            .iter()
            .find(|s| s.action_id == "play")
            .unwrap()
            .shortcut
            .as_deref(),
        Some("Space")
    );
    assert_eq!(doc.autoplay, crate::autoplay::default_settings());
    assert_eq!(doc.audio, crate::core::default_audio_settings());
    assert_eq!(
        doc.extra.get("customBlock").map(String::as_str),
        Some(r#"{"a":1}"#)
    );
    // Re-exporting keeps the extra block.
    let again = parse_document(&to_json(&doc).unwrap()).unwrap();
    assert_eq!(again, doc);

    let mut s = Settings::new();
    let (applied, skipped) = s.apply_settings(&doc.settings, 1.0);
    assert_eq!(applied, vec!["queue.savedCap", "display.theme"]);
    assert_eq!(skipped, vec!["mystery"]);
}

#[test]
fn config_errors() {
    assert!(matches!(parse_document("[]"), Err(ConfigError::Invalid(_))));
    assert!(matches!(
        parse_document("nope"),
        Err(ConfigError::Invalid(_))
    ));
    assert_eq!(
        parse_document(r#"{"version": 99}"#),
        Err(ConfigError::TooNew(99))
    );
    // A minimal v1 document with everything else defaulted parses.
    let d = parse_document(r#"{"version": 1}"#).unwrap();
    assert!(d.settings.is_empty() && d.servers.is_empty());
}
