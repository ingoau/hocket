//! Servers, credentials, the capability probe, library sync scheduling, job
//! runners and the outbox flusher.

use std::sync::Arc;

use crate::api::*;
use crate::connect::auth::{derive_lan_key, LanKey};
use crate::connect::engine::Input;
use crate::connect::wire::{scope_key, Credential as WireCredential};
use crate::core::actor::Actor;
use crate::core::state::ServerState;
use crate::core::{ActorMsg, Internal, PendingServer, PresetServer};
use crate::db::sync::{LibrarySync, LibrarySyncRunner, SyncCursor, SyncJobPayload};
use crate::downloads::DownloadRunner;
use crate::jobs::{JobSpec, RetryAction};
use crate::outbox::OutboxFlushRunner;
use crate::subsonic::auth::{self, AuthMode};
use crate::subsonic::{Client, ClientConfig, ReqwestTransport, SubsonicApi};

/// Deterministic server id from the identity the device authenticated with.
pub fn server_id_for(url: &str, username: &str) -> String {
    use md5::{Digest, Md5};
    let d = Md5::digest(scope_key(url, username).as_bytes());
    hex::encode(&d[..8])
}

/// A server URL from `AddServer`: `http(s)://` with a host and no embedded
/// credentials. The error is the reason, for the toast.
pub(crate) fn validate_server_url(url: &str) -> Result<url::Url, String> {
    let url = url.trim();
    let base = url::Url::parse(url).map_err(|e| e.to_string())?;
    if base.cannot_be_a_base() {
        return Err("not an http(s) address".into());
    }
    match base.scheme() {
        "http" | "https" => {}
        other => return Err(format!("unsupported scheme {other:?}; use http or https")),
    }
    // The parser folds `http:///rest` into `http://rest/`; the host must be
    // what the user typed, not a path segment promoted to one.
    let authority = url
        .get(base.scheme().len()..)
        .and_then(|rest| rest.strip_prefix("://"))
        .unwrap_or("");
    if authority.is_empty() || authority.starts_with(['/', '\\']) {
        return Err("no host".into());
    }
    if base.host_str().is_none_or(str::is_empty) {
        return Err("no host".into());
    }
    if !base.username().is_empty() || base.password().is_some() {
        return Err("credentials belong in the username and password fields, not the URL".into());
    }
    Ok(base)
}

impl Actor {
    pub(crate) fn add_server(
        &mut self,
        url: String,
        username: String,
        password: Secret,
        name: Option<String>,
    ) {
        let base = match validate_server_url(&url) {
            Ok(u) => u,
            Err(e) => {
                self.toast(format!("Invalid server URL: {e}"), None);
                return;
            }
        };
        let id = server_id_for(&url, &username);
        let name = name
            .filter(|n| !n.is_empty())
            .or_else(|| base.host_str().map(str::to_string))
            .unwrap_or_else(|| url.clone());
        let transport = match ReqwestTransport::new(
            &format!("hocket/{}", self.cfg.app_version),
            std::time::Duration::from_secs(30),
        ) {
            Ok(t) => Arc::new(t),
            Err(e) => {
                self.error(ErrorKind::Internal, "http client", Some(e.to_string()));
                return;
            }
        };
        let cfg = ClientConfig::new(
            id.clone(),
            base,
            AuthMode::Password {
                username: username.clone(),
                password: crate::subsonic::Credential::new(password.expose()),
            },
        );
        let client = Arc::new(Client::new(cfg, transport));
        let credential = wire_credential(&url, &username, password.expose());
        // Mutual LAN auth: peers of this scope prove knowledge of a key
        // stretched from the same password (see `connect::auth`).
        let lan_key = derive_lan_key(&scope_key(&url, &username), password.expose());
        // A server this device already authenticated with (persisted row,
        // capabilities known): install it now so downloads play offline, and
        // verify in the background. A failed probe keeps it installed.
        let persisted = match &self.server {
            Some(s) if s.info.id == id => Some(s.info.clone()),
            _ => self
                .db
                .servers()
                .ok()
                .and_then(|list| list.into_iter().find(|s| s.id == id)),
        };
        if let Some(mut info) = persisted {
            info.name = name;
            info.reachable = false;
            self.pending_servers.remove(&id);
            self.install_server(
                info,
                client.clone() as Arc<dyn SubsonicApi>,
                Some(client),
                credential,
                lan_key,
                false,
            );
            self.probe_server(&id);
            return;
        }
        let info = ServerInfo {
            id: id.clone(),
            url: url.clone(),
            username: username.clone(),
            name,
            capabilities: ServerCapabilities::default(),
            last_sync: None,
            reachable: false,
        };
        // Verify first: a new server is only persisted, announced and synced
        // once the probe accepts the credentials.
        self.pending_servers.insert(
            id.clone(),
            PendingServer {
                info,
                client: client.clone(),
                credential,
                lan_key,
            },
        );
        self.spawn_probe(id, client);
    }

    fn spawn_probe(&mut self, server_id: ServerId, client: Arc<Client>) {
        let tx = self.tx.clone();
        self.spawn(async move {
            let result = client.probe().await.map_err(|e| (e.kind(), e.to_string()));
            let _ = tx.send(ActorMsg::Internal(Internal::Probed { server_id, result }));
        });
    }

    /// A server supplied at construction (tests): already authenticated.
    pub(crate) fn attach_preset_server(&mut self, preset: PresetServer) {
        let id = preset.api.server_id().to_string();
        let username = preset
            .api
            .username()
            .unwrap_or_else(|| preset.username.clone());
        let info = ServerInfo {
            id,
            url: preset.url.clone(),
            username: username.clone(),
            name: "Test server".into(),
            capabilities: preset.api.capabilities(),
            last_sync: None,
            reachable: true,
        };
        let credential = wire_credential(&preset.url, &username, &preset.password);
        let lan_key = derive_lan_key(&scope_key(&preset.url, &username), &preset.password);
        self.install_server(info, preset.api, None, credential, lan_key, true);
    }

    fn install_server(
        &mut self,
        info: ServerInfo,
        api: Arc<dyn SubsonicApi>,
        client: Option<Arc<Client>>,
        credential: WireCredential,
        lan_key: LanKey,
        reachable: bool,
    ) {
        let replaced = self.server.as_ref().is_some_and(|s| s.info.id != info.id);
        // The LAN key is fixed at engine construction: a new password for the
        // same server rebuilds the engine (after flushing the document).
        let rekeyed = !replaced
            && self.engine.is_some()
            && self
                .server
                .as_ref()
                .is_some_and(|s| s.lan_key.as_ref() != Some(&lan_key));
        if replaced {
            // One server for now: switching identity closes the old session.
            self.unload();
            self.engine = None;
            self.scope = None;
            self.last_doc = None;
        } else if rekeyed {
            self.flush_persistence(true);
            self.save_position();
            self.engine = None;
            self.scope = None;
            self.last_doc = None;
        }
        if let Err(e) = self.db.upsert_server(&info, self.now()) {
            self.error(ErrorKind::Storage, "save server", Some(e.to_string()));
        }
        let mut state = match self.server.take() {
            Some(s) if s.info.id == info.id => s,
            _ => ServerState::new(info.clone()),
        };
        state.info = info;
        state.info.reachable = reachable;
        state.api = Some(api.clone());
        if let Some(p) = &self.stream_reader {
            p.set_api(Some(api.clone()));
        }
        self.mark_prefetch_check();
        state.client = client;
        state.credential = Some(credential.clone());
        state.lan_key = Some(lan_key);
        let tx = self.tx.clone();
        let sync = LibrarySync::new(
            self.db.clone(),
            api.clone(),
            self.clock.clone(),
            Arc::new(move |p: SyncProgress| {
                let _ = tx.send(ActorMsg::Internal(Internal::SyncProgress(p)));
            }),
        );
        state.sync = Some(Arc::new(sync));
        state.ready_tables = self
            .db
            .saved_state_get::<SyncCursor>(&SyncCursor::key(&state.info.id))
            .ok()
            .flatten()
            .map(|c| {
                if c.has_completed_once() {
                    vec![
                        "artists".into(),
                        "albums".into(),
                        "tracks".into(),
                        "playlists".into(),
                        "genres".into(),
                    ]
                } else {
                    c.ready_tables
                }
            })
            .unwrap_or_default();
        self.server = Some(state);
        if self.engine.is_none() {
            self.open_session();
        } else {
            self.engine_input(Input::SetCredential(Some(credential)));
        }
        let servers = self.server_infos();
        self.emit(Event::ServersChanged { servers });
        if self.started {
            self.register_job_runners();
            self.maybe_start_sync(false, true);
            self.schedule_flush();
            if !self.playback.loaded && self.playback.want_playing {
                self.ensure_current_loaded();
            }
        }
    }

    fn ensure_current_loaded(&mut self) {
        if self.owns_transport() {
            if let Some(item) = self.doc().and_then(|d| d.current.clone()) {
                let play = self.playback.want_playing;
                let pos = self.playback.position_ms;
                self.load_item(&item, pos, play, None);
            }
        }
    }

    pub(crate) fn remove_server(&mut self, server_id: ServerId) {
        if self.server_id().as_deref() != Some(server_id.as_str()) {
            return;
        }
        self.unload();
        self.engine = None;
        self.scope = None;
        self.last_doc = None;
        self.server = None;
        if let Some(p) = &self.stream_reader {
            p.set_api(None);
        }
        self.mark_prefetch_check();
        self.prefetch_tick(self.now());
        if let Err(e) = self.db.delete_server(&server_id) {
            self.error(ErrorKind::Storage, "remove server", Some(e.to_string()));
        }
        for key in ["session", "connect", "position"] {
            let _ = self.db.saved_state_keys(&format!("{key}:")).map(|keys| {
                for k in keys {
                    let _ = self.db.saved_state_delete(&k);
                }
            });
        }
        self.emit_everything(false);
    }

    pub(crate) fn probe_server(&mut self, server_id: &str) {
        let Some(s) = &mut self.server else { return };
        if s.info.id != server_id || s.probing {
            return;
        }
        let Some(client) = s.client.clone() else {
            return;
        };
        s.probing = true;
        self.spawn_probe(server_id.to_string(), client);
    }

    pub(crate) fn on_probed(
        &mut self,
        server_id: ServerId,
        result: Result<ServerCapabilities, (ErrorKind, String)>,
    ) {
        if let Some(pending) = self.pending_servers.remove(&server_id) {
            match result {
                Ok(caps) => {
                    let mut info = pending.info;
                    info.capabilities = caps;
                    info.reachable = true;
                    if !info.capabilities.meets_floor {
                        self.log(
                            "warn",
                            format!(
                                "server version {:?} is below the Navidrome 0.63 floor",
                                info.capabilities.server_version
                            ),
                        );
                    }
                    let client = pending.client;
                    self.install_server(
                        info,
                        client.clone() as Arc<dyn SubsonicApi>,
                        Some(client),
                        pending.credential,
                        pending.lan_key,
                        true,
                    );
                }
                Err((kind, e)) => self.probe_failed(kind, e),
            }
            return;
        }
        let Some(s) = &mut self.server else { return };
        if s.info.id != server_id {
            return;
        }
        s.probing = false;
        match result {
            Ok(caps) => {
                s.info.capabilities = caps;
                s.info.reachable = true;
                let info = s.info.clone();
                if let Err(e) = self.db.upsert_server(&info, self.now()) {
                    self.error(ErrorKind::Storage, "save server", Some(e.to_string()));
                }
                if !info.capabilities.meets_floor {
                    self.log(
                        "warn",
                        format!(
                            "server version {:?} is below the Navidrome 0.63 floor",
                            info.capabilities.server_version
                        ),
                    );
                }
            }
            Err((kind, e)) => {
                s.info.reachable = false;
                self.probe_failed(kind, e);
            }
        }
        let servers = self.server_infos();
        self.emit(Event::ServersChanged { servers });
    }

    fn probe_failed(&mut self, kind: ErrorKind, detail: String) {
        let message = match kind {
            ErrorKind::Auth => "Wrong username or password",
            ErrorKind::Network => "Couldn't reach the server",
            _ => "The server refused the request",
        };
        self.toast(format!("{message}: {detail}"), None);
        self.emit(Event::Error {
            kind,
            message: message.into(),
            detail: Some(detail),
        });
    }

    // -- jobs -----------------------------------------------------------------------

    pub(crate) fn register_job_runners(&mut self) {
        let Some(s) = &self.server else { return };
        let Some(api) = s.api.clone() else { return };
        if let Some(sync) = s.sync.clone() {
            self.jobs.register(
                JobKind::LibrarySync,
                1,
                Arc::new(LibrarySyncRunner::new(sync)),
            );
        }
        self.jobs.register(
            JobKind::OutboxFlush,
            1,
            Arc::new(OutboxFlushRunner::new(self.outbox.clone(), api.clone())),
        );
        self.jobs.register(
            JobKind::Download,
            2,
            Arc::new(DownloadRunner::new(self.downloads.clone(), api)),
        );
    }

    pub(crate) fn job_op(&mut self, op: &str, id: &str) {
        let r = match op {
            "cancel" => self.jobs.cancel(id),
            "pause" => self.jobs.pause(id),
            "resume" => self.jobs.resume(id),
            _ => self.jobs.retry(id),
        };
        match r {
            Ok(false) => self.toast("That job is no longer running", None),
            Ok(true) => {}
            Err(e) => self.error(ErrorKind::Storage, "job", Some(e.to_string())),
        }
    }

    pub(crate) fn retry_problem(&mut self, id: &str) {
        match self.jobs.retry_problem(id) {
            Ok(Some(_)) => {}
            Ok(None) => self.toast("Nothing to retry", None),
            Err(e) => self.error(ErrorKind::Storage, "retry", Some(e.to_string())),
        }
    }

    pub(crate) fn on_jobs_changed(&mut self, jobs: &[Job]) {
        // A finished sync updates the server row; a finished download the pins.
        if let Some(s) = &mut self.server {
            if let Some(id) = s.sync_job.clone() {
                if let Some(j) = jobs.iter().find(|j| j.id == id) {
                    if matches!(
                        j.state,
                        JobState::Done | JobState::Failed | JobState::Cancelled
                    ) {
                        s.sync_job = None;
                    }
                }
            }
        }
        if jobs.iter().any(|j| {
            j.kind == JobKind::Download && matches!(j.state, JobState::Done | JobState::Failed)
        }) {
            let pins = self.pins();
            self.emit(Event::PinsChanged { pins });
            let storage = self.storage_summary();
            self.emit(Event::StorageChanged { storage });
            if let Some(sid) = self.server_id() {
                self.emit(Event::LibraryChanged {
                    server_id: sid,
                    tables: vec!["tracks".into()],
                    ids: vec![],
                });
            }
        }
    }

    // -- library sync ----------------------------------------------------------------

    /// Start a sync when one is due (or forced) and none is running.
    pub(crate) fn maybe_start_sync(&mut self, full: bool, force: bool) {
        let Some(s) = &self.server else { return };
        if s.api.is_none() || s.sync.is_none() || s.sync_job.is_some() {
            return;
        }
        if self.jobs.has_active(JobKind::LibrarySync).unwrap_or(false) {
            return;
        }
        let now = self.now();
        let cursor: SyncCursor = self
            .db
            .saved_state_get(&SyncCursor::key(&s.info.id))
            .ok()
            .flatten()
            .unwrap_or_default();
        let interval_ms =
            self.settings
                .get_i64(crate::settings::keys::LIBRARY_SYNC_INTERVAL_MINUTES) as f64
                * 60_000.0;
        let reconcile_ms =
            self.settings
                .get_i64(crate::settings::keys::LIBRARY_FULL_RECONCILE_DAYS) as f64
                * 86_400_000.0;
        let last = cursor
            .last_incremental_at
            .or(cursor.last_full_at)
            .unwrap_or(0.0);
        let full_due = cursor
            .last_full_at
            .map(|t| now - t >= reconcile_ms)
            .unwrap_or(true);
        let due = force || cursor.in_progress() || now - last >= interval_ms || full_due;
        if !due {
            return;
        }
        if self
            .network
            .as_ref()
            .is_some_and(|n| n.kind == NetworkKind::Offline)
        {
            return;
        }
        let full = full || full_due;
        let payload = serde_json::to_string(&SyncJobPayload { full }).unwrap_or_default();
        let label = if full {
            "Full library sync"
        } else {
            "Library sync"
        };
        match self
            .jobs
            .submit(JobSpec::new(JobKind::LibrarySync, label).payload(payload))
        {
            Ok(id) => {
                if let Some(s) = &mut self.server {
                    s.sync_job = Some(id);
                    s.last_sync_started = Some(now);
                }
            }
            Err(e) => self.error(ErrorKind::Storage, "start sync", Some(e.to_string())),
        }
    }

    pub(crate) fn on_sync_progress(&mut self, p: SyncProgress) {
        let newly_ready: Vec<String> = match &self.server {
            Some(s) if s.info.id == p.server_id => p
                .ready_tables
                .iter()
                .filter(|t| !s.ready_tables.contains(t))
                .cloned()
                .collect(),
            _ => vec![],
        };
        if let Some(s) = &mut self.server {
            if s.info.id == p.server_id {
                for t in &newly_ready {
                    s.ready_tables.push(t.clone());
                }
            }
        }
        if !newly_ready.is_empty() {
            self.emit(Event::LibraryChanged {
                server_id: p.server_id.clone(),
                tables: newly_ready,
                ids: vec![],
            });
        }
        if p.finished {
            let now = self.now();
            if let Some(s) = &mut self.server {
                s.info.last_sync = Some(now);
                let _ = self.db.set_server_last_sync(&s.info.id, now);
            }
            self.emit(Event::LibraryChanged {
                server_id: p.server_id.clone(),
                tables: vec![],
                ids: vec![],
            });
            let servers = self.server_infos();
            self.emit(Event::ServersChanged { servers });
            self.reconcile_playlist_pins();
            // The queue view may have been rendered from bare summaries.
            self.emit_queue();
        }
        self.sync_progress = Some(p.clone());
        self.emit(Event::SyncProgress { progress: p });
    }

    fn reconcile_playlist_pins(&mut self) {
        let Some(sid) = self.server_id() else { return };
        let pins = self.downloads.pins(&sid).unwrap_or_default();
        let mut changed = false;
        for pin in pins {
            if let PinTarget::Playlist { id } = &pin.target {
                match self.downloads.reconcile_playlist(&sid, id) {
                    Ok(Some(spec)) => {
                        changed = true;
                        if let Err(e) = self.jobs.submit(spec) {
                            self.error(ErrorKind::Storage, "download job", Some(e.to_string()));
                        }
                    }
                    Ok(None) => {}
                    Err(e) => self.log("warn", format!("reconcile playlist pin: {e}")),
                }
            }
        }
        if changed {
            let pins = self.pins();
            self.emit(Event::PinsChanged { pins });
        }
    }

    // -- outbox -----------------------------------------------------------------------

    /// Flush the outbox now (bounded to one in flight); pending entries are
    /// retried from the tick.
    pub(crate) fn schedule_flush(&mut self) {
        if self.flush_in_flight {
            return;
        }
        let Some(api) = self.api() else { return };
        if self
            .network
            .as_ref()
            .is_some_and(|n| n.kind == NetworkKind::Offline)
        {
            return;
        }
        if self.outbox.pending_count().unwrap_or(0) == 0 {
            return;
        }
        self.flush_in_flight = true;
        let outbox = self.outbox.clone();
        let tx = self.tx.clone();
        self.spawn(async move {
            let report = outbox.flush(api.as_ref(), 100).await.ok();
            let _ = tx.send(ActorMsg::Internal(Internal::OutboxFlushed { report }));
        });
    }

    pub(crate) fn on_outbox_flushed(&mut self, report: Option<crate::outbox::FlushReport>) {
        self.flush_in_flight = false;
        let Some(report) = report else { return };
        let Some(sid) = self.server_id() else { return };
        if !report.failed.is_empty() {
            for (entry, error) in &report.failed {
                let _ = self.jobs.add_problem(
                    None,
                    "A change could not be saved to the server",
                    Some(&format!("{error} (outbox entry {entry})")),
                    Some(RetryAction::Resubmit {
                        kind: JobKind::OutboxFlush,
                        label: "Retry pending changes".into(),
                        payload: r#"{"retryFailed":true}"#.into(),
                        items: vec![],
                    }),
                );
            }
        }
        if report.applied > 0
            || !report.conflicts.is_empty()
            || !report.created_playlists.is_empty()
        {
            let mut tables = vec!["tracks".into(), "albums".into()];
            if !report.created_playlists.is_empty() {
                tables.push("playlists".into());
            }
            self.emit(Event::LibraryChanged {
                server_id: sid,
                tables,
                ids: vec![],
            });
        }
        if report.deferred == 0 && self.outbox.pending_count().unwrap_or(0) > 0 {
            // More was queued while flushing.
            self.schedule_flush();
        }
    }

    pub(crate) fn set_network_state(&mut self, state: NetworkState) {
        let was_offline = self
            .network
            .as_ref()
            .is_some_and(|n| n.kind == NetworkKind::Offline);
        self.downloads.set_network(Some(state.clone()));
        self.network = Some(state.clone());
        self.mark_prefetch_check();
        if state.kind != NetworkKind::Offline && (was_offline || self.network.is_some()) {
            self.last_outbox_retry = self.now();
            self.schedule_flush();
            self.maybe_start_sync(false, false);
            // A server installed offline (or whose last probe failed) is
            // verified again now that there is a network.
            if let Some(id) = self
                .server
                .as_ref()
                .filter(|s| !s.info.reachable && !s.probing && s.client.is_some())
                .map(|s| s.info.id.clone())
            {
                self.probe_server(&id);
            }
        }
        // The stream URL may change with the network's transcoding profile.
        if self.playback.loaded && self.owns_transport() {
            self.refresh_next();
        }
    }
}

fn wire_credential(url: &str, username: &str, password: &str) -> WireCredential {
    let salt = auth::random_salt();
    WireCredential {
        server_url: url.to_string(),
        username: username.to_string(),
        token: Some(auth::token(password, &salt)),
        salt: Some(salt),
        api_key: None,
        client: auth::CLIENT_NAME.to_string(),
        api_version: auth::API_VERSION.to_string(),
    }
}

#[cfg(test)]
mod url_tests {
    use super::validate_server_url;

    #[test]
    fn only_plain_http_urls_with_a_host_are_accepted() {
        assert!(validate_server_url("https://music.example/").is_ok());
        assert!(validate_server_url("http://192.168.1.10:4533").is_ok());
        assert!(validate_server_url("https://music.example/navidrome/").is_ok());
        for bad in [
            "mailto:x@y",
            "data:text/plain,x",
            "javascript:alert(1)",
            "ftp://music.example/",
            "ws://music.example/",
            "http://alice:pw@music.example/",
            "http://alice@music.example/",
            "http:///rest",
            "music.example",
            "",
        ] {
            assert!(validate_server_url(bad).is_err(), "{bad}");
        }
    }
}
