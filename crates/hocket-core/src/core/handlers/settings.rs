//! Settings (scope + LWW sync), config backup, audio settings, the action
//! registry customisation and `RunAction`.

use crate::actions::canonical_id;
use crate::api::*;
use crate::connect::engine::Input;
use crate::connect::wire::SessionOp;
use crate::core::actor::Actor;
use crate::core::handlers::session::DbResolver;
use crate::settings::{keys, ConfigInputs};

impl Actor {
    // -- settings ------------------------------------------------------------------

    pub(crate) fn set_setting_json(&mut self, key: &str, value: &str) {
        let now = self.now();
        match self.settings.set_json(key, value, now) {
            Ok(setting) => self.after_setting_changed(setting, true),
            Err(e) => self.toast(format!("Couldn't change {key}: {e}"), None),
        }
    }

    pub(crate) fn set_setting_value(&mut self, key: &str, value: serde_json::Value) {
        let now = self.now();
        match self.settings.set_value(key, value, now) {
            Ok(setting) => self.after_setting_changed(setting, true),
            Err(e) => self.toast(format!("Couldn't change {key}: {e}"), None),
        }
    }

    pub(crate) fn reset_setting(&mut self, key: &str) {
        let now = self.now();
        match self.settings.reset(key, now) {
            Ok(setting) => self.after_setting_changed(setting, true),
            Err(e) => self.toast(format!("Couldn't reset {key}: {e}"), None),
        }
    }

    fn after_setting_changed(&mut self, setting: Setting, local: bool) {
        self.save_settings();
        self.emit(Event::SettingChanged {
            setting: setting.clone(),
        });
        self.apply_setting_side_effects(&setting.key, local);
        if local && setting.scope == SettingScope::AccountSynced && self.settings.sync_enabled() {
            self.engine_input(Input::SettingChanged(setting));
        }
    }

    /// What a changed key means for the running subsystems.
    pub(crate) fn apply_setting_side_effects(&mut self, key: &str, local: bool) {
        match key {
            keys::QUEUE_MODE => {
                let mode = match self.settings.get_string(key).as_deref() {
                    Some("youTube") => QueueMode::YouTube,
                    _ => QueueMode::Apple,
                };
                if local && self.doc().is_some_and(|d| d.mode != mode) {
                    self.local_op(SessionOp::SetQueueMode { mode });
                }
            }
            keys::QUEUE_SAVED_CAP => {
                let cap = self.settings.get_i64(key).max(0) as u32;
                let (_, mut saved) = self.reducer_policy.get();
                saved = saved.with_cap(cap);
                self.reducer_policy.set_saved(saved);
            }
            keys::QUEUE_HISTORY_CAP => {
                self.reducer_policy
                    .set_history_cap(self.settings.get_i64(key).max(10) as usize);
            }
            keys::AUTOPLAY_SETTINGS => {
                if let (Some(a), Some(s)) = (
                    self.autoplay.as_mut(),
                    self.settings.get_typed::<AutoplaySettings>(key),
                ) {
                    a.set_settings(s);
                }
            }
            keys::AUTOPLAY_CONTEXT_OVERRIDES => {
                let overrides = parse_overrides(&self.settings.get(key));
                if let Some(a) = self.autoplay.as_mut() {
                    a.set_overrides(overrides);
                }
            }
            keys::AUDIO_SETTINGS => {
                if let Some(a) = self.settings.get_typed::<AudioSettings>(key) {
                    if a != self.audio {
                        self.audio = a;
                        self.apply_audio_to_backend();
                        self.emit(Event::AudioSettingsChanged {
                            settings: self.audio.clone(),
                        });
                    }
                }
            }
            keys::TRANSCODING_PROFILES => {
                self.apply_transcoding_settings();
                self.mark_prefetch_check();
            }
            keys::BATTERY_PAUSE_PREFETCH | keys::STORAGE_PREFETCH_ON_MOBILE_DATA => {
                self.mark_prefetch_check();
            }
            keys::STORAGE_WARN_THRESHOLD_BYTES | keys::STORAGE_CACHE_MAX_BYTES => {
                self.apply_storage_settings();
                self.mark_prefetch_check();
                // A smaller budget applies now, not at the next cache write.
                match self.downloads.evict_over_budget() {
                    Ok(evicted) if !evicted.is_empty() => self.on_stream_cache_changed(evicted),
                    Ok(_) => {
                        let storage = self.storage_summary();
                        self.emit(Event::StorageChanged { storage });
                    }
                    Err(e) => {
                        self.error(ErrorKind::Storage, "stream cache", Some(e.to_string()));
                        let storage = self.storage_summary();
                        self.emit(Event::StorageChanged { storage });
                    }
                }
            }
            keys::CONNECT_COORDINATOR_URL => {
                let url = self.settings.get_string(key);
                self.engine_input(Input::SetCoordinatorUrl(url));
            }
            keys::CONNECT_LAN_DISCOVERY => {
                let on = self.settings.get_bool(key) && self.cfg.coordinator_listen.is_none();
                self.engine_input(Input::SetLanDiscovery(on));
            }
            keys::SHORTCUTS => {
                let mut c = self.registry.customisation();
                c.shortcuts.clear();
                if let Some(map) = self.settings.get(key).as_object() {
                    for (id, v) in map {
                        c.shortcuts
                            .insert(id.clone(), v.as_str().map(str::to_string));
                    }
                }
                self.registry.apply_customisation(&c);
                let shortcuts = self.registry.shortcuts();
                self.emit(Event::ShortcutsChanged { shortcuts });
            }
            k if k.starts_with("actions.order.") => {
                let surface = k.trim_start_matches("actions.order.").to_string();
                let ids: Vec<String> = self
                    .settings
                    .get(key)
                    .as_array()
                    .map(|a| {
                        a.iter()
                            .filter_map(|v| v.as_str().map(str::to_string))
                            .collect()
                    })
                    .unwrap_or_default();
                if let Some(s) = crate::actions::Surface::parse(&surface) {
                    if ids.is_empty() {
                        let d = self.registry.default_order(s);
                        self.registry.set_order(s, d);
                    } else {
                        self.registry.set_order(s, ids);
                    }
                }
                self.emit(Event::ActionsChanged { surface });
                self.emit_media_session();
            }
            keys::SYNC_ENABLED => {
                if self.settings.sync_enabled() && self.settings_corrupt.is_none() {
                    for s in self.settings.synced_settings() {
                        self.queue_input(Input::SettingChanged(s));
                    }
                    self.engine_input(Input::Tick);
                }
            }
            keys::BATTERY_SMALL_ARTWORK => {
                self.media_art = None;
                self.emit_media_session();
            }
            keys::LIBRARY_SYNC_INTERVAL_MINUTES | keys::LIBRARY_FULL_RECONCILE_DAYS => {
                self.last_sync_check = 0.0;
            }
            _ => {}
        }
    }

    pub(crate) fn apply_storage_settings(&mut self) {
        let warn = self.settings.get_f64(keys::STORAGE_WARN_THRESHOLD_BYTES);
        self.downloads
            .set_warn_threshold(if warn > 0.0 { Some(warn) } else { None });
        let budget = self.settings.get_f64(keys::STORAGE_CACHE_MAX_BYTES);
        if budget > 0.0 {
            self.downloads.set_cache_budget(budget);
        }
    }

    pub(crate) fn apply_transcoding_settings(&mut self) {
        let mut policy = crate::downloads::TranscodingPolicy::default();
        if let Some(map) = self.settings.get(keys::TRANSCODING_PROFILES).as_object() {
            for (k, v) in map {
                let Ok(p) = serde_json::from_value::<TranscodingProfile>(v.clone()) else {
                    continue;
                };
                match k.as_str() {
                    "default" => policy.default = Some(p),
                    "cellular" => {
                        policy.by_kind.insert(NetworkKind::Cellular, p);
                    }
                    "wifi" => {
                        policy.by_kind.insert(NetworkKind::Wifi, p);
                    }
                    "wired" => {
                        policy.by_kind.insert(NetworkKind::Wired, p);
                    }
                    other => {
                        policy.by_network_id.insert(other.to_string(), p);
                    }
                }
            }
        }
        self.downloads.set_transcoding_policy(policy);
    }

    pub(crate) fn set_transcoding_profile(
        &mut self,
        network_id: Option<String>,
        profile: TranscodingProfile,
    ) {
        let mut map = self
            .settings
            .get(keys::TRANSCODING_PROFILES)
            .as_object()
            .cloned()
            .unwrap_or_default();
        let key = network_id.unwrap_or_else(|| "default".into());
        match serde_json::to_value(&profile) {
            Ok(v) => {
                map.insert(key, v);
            }
            Err(e) => {
                self.error(ErrorKind::Internal, "profile", Some(e.to_string()));
                return;
            }
        }
        self.set_setting_value(keys::TRANSCODING_PROFILES, serde_json::Value::Object(map));
    }

    // -- audio ---------------------------------------------------------------------

    pub(crate) fn set_audio_settings(&mut self, settings: AudioSettings) {
        match serde_json::to_value(&settings) {
            Ok(v) => self.set_setting_value(keys::AUDIO_SETTINGS, v),
            Err(e) => self.error(ErrorKind::Internal, "audio settings", Some(e.to_string())),
        }
    }

    pub(crate) fn apply_audio_to_backend(&mut self) {
        if let Err(e) = self.backend.set_gapless(self.audio.gapless) {
            self.log("debug", format!("set_gapless: {e}"));
        }
        if let Err(e) = self
            .backend
            .set_output_device(self.audio.output_device.clone())
        {
            self.log("debug", format!("set_output_device: {e}"));
        }
        if let Err(e) = self.backend.set_exclusive(self.audio.exclusive) {
            self.log("debug", format!("set_exclusive: {e}"));
        }
        #[cfg(feature = "native-audio")]
        {
            let _ = &self.audio;
        }
        // ReplayGain/preamp/normalisation reach the backend through the
        // per-item gain, so the loaded items are refreshed.
        if self.playback.loaded && self.owns_transport() {
            self.playback.next = None;
            self.refresh_next();
        }
    }

    // -- config backup -----------------------------------------------------------------

    pub(crate) fn config_document(&self, include_secrets: bool) -> String {
        let inputs = ConfigInputs {
            settings: self.settings.to_api(),
            filters: self
                .filters()
                .into_iter()
                .filter(|f| !crate::filters::is_builtin(&f.id))
                .collect(),
            shortcuts: self.registry.shortcuts(),
            servers: self.server_infos(),
            audio: Some(self.audio.clone()),
            autoplay: self.settings.get_typed(keys::AUTOPLAY_SETTINGS),
        };
        let doc = crate::settings::build_document(inputs, include_secrets, self.now());
        crate::settings::to_json(&doc).unwrap_or_default()
    }

    /// Import a config document. Synced keys the document changes are
    /// broadcast like a local edit (otherwise the next merge from a peer
    /// would revert the import); device-local keys, including the `audio`
    /// block, are only taken with `include_device_local`.
    pub(crate) fn import_config(&mut self, document: &str, include_device_local: bool) {
        let doc = match crate::settings::parse_document(document) {
            Ok(d) => d,
            Err(e) => {
                self.toast(format!("Couldn't import: {e}"), None);
                return;
            }
        };
        let now = self.now();
        let mut out = self
            .settings
            .import_settings(&doc.settings, now, include_device_local);
        for f in &doc.filters {
            self.save_filter(f.clone());
        }
        for s in &doc.shortcuts {
            let _ = self
                .registry
                .set_shortcut(&s.action_id, s.shortcut.as_deref());
        }
        self.persist_registry();
        if include_device_local {
            if let Ok(v) = serde_json::to_value(&doc.audio) {
                if self
                    .settings
                    .set_value(keys::AUDIO_SETTINGS, v, now)
                    .is_ok()
                {
                    out.applied.push(keys::AUDIO_SETTINGS.into());
                }
            }
        } else {
            out.skipped_device_local.push(keys::AUDIO_SETTINGS.into());
        }
        if let Ok(v) = serde_json::to_value(&doc.autoplay) {
            let before = self.settings.get(keys::AUTOPLAY_SETTINGS);
            if self
                .settings
                .set_value(keys::AUTOPLAY_SETTINGS, v, now)
                .is_ok()
            {
                out.applied.push(keys::AUTOPLAY_SETTINGS.into());
                if self.settings.get(keys::AUTOPLAY_SETTINGS) != before {
                    out.changed_synced.push(keys::AUTOPLAY_SETTINGS.into());
                }
            }
        }
        self.save_settings();
        let changed_synced: std::collections::HashSet<&str> =
            out.changed_synced.iter().map(String::as_str).collect();
        // Everything is announced to the UI; changed synced keys go through
        // the local-edit path below, which also feeds the engine.
        let all = self.settings.to_api();
        for s in &all {
            if !changed_synced.contains(s.key.as_str()) {
                self.emit(Event::SettingChanged { setting: s.clone() });
            }
        }
        for key in &out.applied {
            if !changed_synced.contains(key.as_str()) {
                self.apply_setting_side_effects(key, true);
            }
        }
        for s in all {
            if changed_synced.contains(s.key.as_str()) {
                self.after_setting_changed(s, true);
            }
        }
        let shortcuts = self.registry.shortcuts();
        self.emit(Event::ShortcutsChanged { shortcuts });
        let mut notes = Vec::new();
        if !out.skipped.is_empty() {
            notes.push(format!("{} setting(s) were skipped", out.skipped.len()));
        }
        if !include_device_local && !out.skipped_device_local.is_empty() {
            notes.push(format!(
                "{} device setting(s) left alone",
                out.skipped_device_local.len()
            ));
        }
        if notes.is_empty() {
            self.toast("Configuration imported", None);
        } else {
            self.toast(format!("Imported; {}", notes.join(", ")), None);
        }
    }

    // -- actions -----------------------------------------------------------------------

    fn persist_registry(&mut self) {
        let now = self.now();
        let c = self.registry.customisation();
        let shortcuts: serde_json::Map<String, serde_json::Value> = c
            .shortcuts
            .iter()
            .map(|(k, v)| {
                (
                    k.clone(),
                    v.clone()
                        .map(serde_json::Value::String)
                        .unwrap_or(serde_json::Value::Null),
                )
            })
            .collect();
        let _ = self
            .settings
            .set_value(keys::SHORTCUTS, serde_json::Value::Object(shortcuts), now);
        for surface in crate::actions::Surface::ALL {
            let key = keys::action_order(surface.as_str());
            let ids = c.orders.get(surface.as_str()).cloned().unwrap_or_default();
            if crate::settings::lookup(&key).is_some() {
                let _ = self.settings.set_value(&key, serde_json::json!(ids), now);
            }
        }
        self.save_settings();
    }

    pub(crate) fn set_shortcut(&mut self, action_id: &str, shortcut: Option<&str>) {
        match self.registry.set_shortcut(action_id, shortcut) {
            Ok(()) => {
                self.persist_registry();
                let shortcuts = self.registry.shortcuts();
                self.emit(Event::ShortcutsChanged { shortcuts });
            }
            Err(e) => self.toast(format!("Couldn't bind: {e}"), None),
        }
    }

    pub(crate) fn set_action_order(&mut self, surface: &str, action_ids: Vec<String>) {
        match self.registry.set_order_str(surface, action_ids) {
            Ok(()) => {
                self.persist_registry();
                self.emit(Event::ActionsChanged {
                    surface: surface.into(),
                });
                if surface == "mediaSession" {
                    self.emit_media_session();
                }
                let key = keys::action_order(surface);
                if let Some(s) = self.settings.setting(&key) {
                    self.emit(Event::SettingChanged { setting: s.clone() });
                    if self.settings.sync_enabled() {
                        self.engine_input(Input::SettingChanged(s));
                    }
                }
            }
            Err(e) => self.toast(format!("Unknown surface: {e}"), None),
        }
    }

    pub(crate) fn run_action(&mut self, action_id: &str, target: ActionTarget) {
        let id = canonical_id(action_id).to_string();
        let state = self.state_view();
        let commands = {
            let resolver = DbResolver { actor: self };
            self.registry.commands(&id, &target, &state, &resolver)
        };
        match commands {
            Ok(cmds) => {
                if !matches!(target, ActionTarget::None) {
                    self.selection = target;
                }
                for c in cmds {
                    self.handle_command(c);
                }
            }
            Err(e) => self.toast(format!("Can't do that: {e}"), None),
        }
    }
}

fn parse_overrides(v: &serde_json::Value) -> Vec<crate::autoplay::ContextOverride> {
    use crate::autoplay::ContextClass;
    let Some(obj) = v.as_object() else {
        return crate::autoplay::default_overrides();
    };
    obj.iter()
        .filter_map(|(k, chain)| {
            let context = match k.as_str() {
                "album" => ContextClass::Album,
                "artist" => ContextClass::Artist,
                "playlist" => ContextClass::Playlist,
                "genre" => ContextClass::Genre,
                "filter" => ContextClass::Filter,
                "adHoc" => ContextClass::AdHoc,
                "autoplay" => ContextClass::Autoplay,
                _ => return None,
            };
            let chain: Vec<AutoplayProvider> = serde_json::from_value(chain.clone()).ok()?;
            Some(crate::autoplay::ContextOverride { context, chain })
        })
        .collect()
}
