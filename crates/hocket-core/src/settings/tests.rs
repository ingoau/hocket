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
    // ws:// is only for the local network; https is not a WebSocket
    assert!(s
        .set_json(
            keys::CONNECT_COORDINATOR_URL,
            "\"ws://hocket.example/ws\"",
            101.0
        )
        .is_err());
    assert!(s
        .set_json(
            keys::CONNECT_COORDINATOR_URL,
            "\"ws://192.168.1.4:7373/ws\"",
            101.0
        )
        .is_ok());
    assert!(s
        .set_json(
            keys::CONNECT_COORDINATOR_URL,
            "\"https://hocket.example/ws\"",
            101.0
        )
        .is_err());
    assert!(s
        .set_json(
            keys::CONNECT_COORDINATOR_URL,
            "\"wss://u:p@hocket.example/ws\"",
            101.0
        )
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
    let doc = r#"{"version":3,"entries":{"future.key":{"value":42,"updatedAt":5.0,"scope":"accountSynced"},"queue.savedCap":{"value":999,"updatedAt":1.0}}}"#;
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
    // A changed key is restamped to the import time, not the document's 10.0.
    assert_eq!(
        fresh.setting(keys::QUEUE_SAVED_CAP).unwrap().updated_at,
        999.0
    );
    // A key already at the imported value is not restamped.
    assert!(fresh
        .setting(keys::SCROBBLE_ENABLED)
        .is_none_or(|s| s.updated_at < 999.0));
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
    assert_eq!(doc.version, CONFIG_VERSION);
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

/// M6: an imported document can't plant timestamps in the future, device-
/// local keys are opt-in, and the synced keys that changed are reported.
#[test]
fn import_of_an_older_backup_is_not_reverted_by_the_next_merge() {
    let mut s = Settings::new();
    // a peer set savedCap=40 at t=5_000 and we merged it
    s.merge_remote(&[setting(
        keys::QUEUE_SAVED_CAP,
        "40",
        SettingScope::AccountSynced,
        5_000.0,
    )]);
    // the user restores a backup written at t=100 that says 30
    let doc = vec![setting(
        keys::QUEUE_SAVED_CAP,
        "30",
        SettingScope::AccountSynced,
        100.0,
    )];
    let out = s.import_settings(&doc, 6_000.0, false);
    assert_eq!(out.changed_synced, vec![keys::QUEUE_SAVED_CAP]);
    assert_eq!(
        s.setting(keys::QUEUE_SAVED_CAP).unwrap().updated_at,
        6_000.0
    );
    // the peer's old value arriving again must not revert the restore
    let changed = s.merge_remote(&[setting(
        keys::QUEUE_SAVED_CAP,
        "40",
        SettingScope::AccountSynced,
        5_000.0,
    )]);
    assert!(changed.is_empty());
    assert_eq!(s.get_i64(keys::QUEUE_SAVED_CAP), 30);
}

#[test]
fn import_restamps_changed_keys_and_scopes_device_local_keys() {
    let mut s = Settings::new();
    s.set_json(keys::QUEUE_SAVED_CAP, "20", 100.0).unwrap();
    let doc = vec![
        setting(
            keys::QUEUE_SAVED_CAP,
            "30",
            SettingScope::AccountSynced,
            9e15, // hand-edited: would win every future merge
        ),
        setting(
            keys::SCROBBLE_ENABLED,
            "true",
            SettingScope::AccountSynced,
            50.0,
        ),
        setting(keys::SYNC_ENABLED, "false", SettingScope::DeviceLocal, 50.0),
        setting(
            keys::DISPLAY_THEME,
            "\"light\"",
            SettingScope::DeviceLocal,
            50.0,
        ),
        setting("bogus", "1", SettingScope::DeviceLocal, 1.0),
    ];
    let out = s.import_settings(&doc, 1_000.0, false);
    assert_eq!(
        out.applied,
        vec![keys::QUEUE_SAVED_CAP, keys::SCROBBLE_ENABLED]
    );
    assert_eq!(
        out.skipped_device_local,
        vec![keys::SYNC_ENABLED, keys::DISPLAY_THEME]
    );
    assert_eq!(out.skipped, vec!["bogus"]);
    assert_eq!(
        out.changed_synced,
        vec![keys::QUEUE_SAVED_CAP],
        "scrobble.enabled was already true (default): no change to broadcast"
    );
    assert_eq!(s.get_i64(keys::QUEUE_SAVED_CAP), 30);
    assert_eq!(
        s.setting(keys::QUEUE_SAVED_CAP).unwrap().updated_at,
        1_000.0,
        "a changed key is restamped to the import time, never the document's"
    );
    assert!(
        s.setting(keys::SCROBBLE_ENABLED)
            .is_none_or(|x| x.updated_at < 1_000.0),
        "an unchanged value is not restamped"
    );
    assert!(s.sync_enabled(), "device-local key untouched");
    // a peer's honest later write still wins after the import
    let changed = s.merge_remote(&[setting(
        keys::QUEUE_SAVED_CAP,
        "25",
        SettingScope::AccountSynced,
        2_000.0,
    )]);
    assert_eq!(changed, vec![keys::QUEUE_SAVED_CAP]);
    assert_eq!(s.get_i64(keys::QUEUE_SAVED_CAP), 25);

    // same-device restore includes device-local keys, restamped to now
    let out = s.import_settings(&doc, 3_000.0, true);
    assert!(out.skipped_device_local.is_empty());
    assert!(!s.sync_enabled());
    assert_eq!(
        s.setting(keys::QUEUE_SAVED_CAP).unwrap().updated_at,
        3_000.0
    );
    // the compatibility wrapper clamps too
    let mut t = Settings::new();
    t.apply_settings(&doc, 10.0);
    assert_eq!(t.setting(keys::QUEUE_SAVED_CAP).unwrap().updated_at, 10.0);
}

/// L4: the external-lyrics toggle is a per-device privacy choice.
#[test]
fn external_lyrics_toggle_is_device_local() {
    assert_eq!(
        lookup(keys::LYRICS_EXTERNAL_ENABLED).unwrap().scope,
        SettingScope::DeviceLocal
    );
    assert_eq!(
        lookup(keys::LYRICS_EXTERNAL_PROVIDER).unwrap().scope,
        SettingScope::AccountSynced
    );
    let mut s = Settings::new();
    assert!(s
        .merge_remote(&[setting(
            keys::LYRICS_EXTERNAL_ENABLED,
            "true",
            SettingScope::AccountSynced,
            5.0
        )])
        .is_empty());
    assert!(!s.get_bool(keys::LYRICS_EXTERNAL_ENABLED));
}

/// Version 1 read `storage.cacheMaxBytes` equal to its old 2 GiB default as
/// "automatic"; version 2 keeps automatic as `null`, so a user's own 2 GiB
/// is a budget like any other.
#[test]
fn cache_budget_automatic_is_null_and_v1_documents_migrate() {
    const GIB: f64 = 1024.0 * 1024.0 * 1024.0;
    let mut s = Settings::new();
    assert_eq!(s.get(keys::STORAGE_CACHE_MAX_BYTES), Value::Null, "automatic by default");
    s.set_json(keys::STORAGE_CACHE_MAX_BYTES, &(2.0 * GIB).to_string(), 1.0)
        .unwrap();
    assert_eq!(s.get_f64(keys::STORAGE_CACHE_MAX_BYTES), 2.0 * GIB);
    // Survives a save and load (no migration at the current version).
    let again = Settings::from_json(&s.to_json().unwrap()).unwrap();
    assert_eq!(again.get(keys::STORAGE_CACHE_MAX_BYTES), json!(2.0 * GIB));
    s.set_json(keys::STORAGE_CACHE_MAX_BYTES, "null", 2.0).unwrap();
    assert_eq!(s.get(keys::STORAGE_CACHE_MAX_BYTES), Value::Null);
    s.set_json(keys::STORAGE_CACHE_MAX_BYTES, "1e9", 3.0).unwrap();
    s.reset(keys::STORAGE_CACHE_MAX_BYTES, 4.0).unwrap();
    assert_eq!(s.get(keys::STORAGE_CACHE_MAX_BYTES), Value::Null, "reset is automatic");
    assert!(s.set_json(keys::STORAGE_CACHE_MAX_BYTES, "1", 5.0).is_err(), "below the minimum");
    assert!(s.set_json(keys::DISPLAY_QUEUE_PANEL_SPLIT, "null", 5.0).is_err(), "not nullable");

    // A v1 document: 2 GiB (set, or reset to the old default) was automatic; other sizes stay.
    let v1 = |bytes: f64| {
        format!(r#"{{"version":1,"entries":{{"storage.cacheMaxBytes":{{"value":{bytes},"updatedAt":7.0,"scope":"deviceLocal"}}}}}}"#)
    };
    assert_eq!(Settings::document_version(&v1(1.0)), Some(1));
    let migrated = Settings::from_json(&v1(2.0 * GIB)).unwrap();
    assert_eq!(migrated.get(keys::STORAGE_CACHE_MAX_BYTES), Value::Null);
    let kept = Settings::from_json(&v1(5e8)).unwrap();
    assert_eq!(kept.get_f64(keys::STORAGE_CACHE_MAX_BYTES), 5e8);
    let saved = migrated.to_json().unwrap();
    assert_eq!(Settings::document_version(&saved), Some(SETTINGS_DOC_VERSION));
    // A current document's 2 GiB is not migrated again.
    let current = v1(2.0 * GIB).replace(r#""version":1"#, &format!(r#""version":{SETTINGS_DOC_VERSION}"#));
    assert_eq!(
        Settings::from_json(&current).unwrap().get_f64(keys::STORAGE_CACHE_MAX_BYTES),
        2.0 * GIB
    );

    // Config backups: a v1 export carried every key, the automatic budget as 2 GiB.
    let backup = |version: u32, value: &str| {
        format!(r#"{{"version":{version},"settings":[{{"key":"storage.cacheMaxBytes","value":"{value}","scope":"deviceLocal","updatedAt":1.0}}]}}"#)
    };
    let budget = |doc: &crate::api::ConfigDocument| {
        doc.settings
            .iter()
            .find(|s| s.key == keys::STORAGE_CACHE_MAX_BYTES)
            .map(|s| s.value.clone())
    };
    let d = parse_document(&backup(1, "2147483648.0")).unwrap();
    assert_eq!(d.version, CONFIG_VERSION);
    assert_eq!(budget(&d).as_deref(), Some("null"));
    let d = parse_document(&backup(1, "2147483648")).unwrap();
    assert_eq!(budget(&d).as_deref(), Some("null"));
    let d = parse_document(&backup(1, "500000000.0")).unwrap();
    assert_eq!(budget(&d).as_deref(), Some("500000000.0"));
    let d = parse_document(&backup(2, "2147483648.0")).unwrap();
    assert_eq!(budget(&d).as_deref(), Some("2147483648.0"), "a v2 2 GiB is the user's");
    let mut s = Settings::new();
    s.set_json(keys::STORAGE_CACHE_MAX_BYTES, "1e9", 1.0).unwrap();
    let d = parse_document(&backup(1, "2147483648.0")).unwrap();
    let out = s.import_settings(&d.settings, 2.0, true);
    assert_eq!(out.applied, vec![keys::STORAGE_CACHE_MAX_BYTES]);
    assert_eq!(s.get(keys::STORAGE_CACHE_MAX_BYTES), Value::Null);
}
