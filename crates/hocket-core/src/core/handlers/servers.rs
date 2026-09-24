//! Servers, credentials, the capability probe, library sync scheduling, job
//! runners and the outbox flusher.

use std::sync::Arc;

use crate::api::*;
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
pub(crate) fn server_id_for(url: &str, username: &str) -> String {
    use md5::{Digest, Md5};
    let d = Md5::digest(scope_key(url, username).as_bytes());
    hex::encode(&d[..8])
}

impl Actor {
    pub(crate) fn add_server(
        &mut self,
        url: String,
        username: String,
        password: String,
        name: Option<String>,
    ) {
        let base = match url::Url::parse(&url) {
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
                password: crate::subsonic::Credential::new(password.clone()),
            },
        );
        let client = Arc::new(Client::new(cfg, transport));
        let info = match &self.server {
            Some(s) if s.info.id == id => {
                let mut i = s.info.clone();
                i.name = name;
                i
            }
            _ => ServerInfo {
                id: id.clone(),
                url: url.clone(),
                username: username.clone(),
                name,
                capabilities: ServerCapabilities::default(),
                last_sync: None,
                reachable: false,
            },
        };
        let credential = wire_credential(&url, &username, &password);
        // Verify first: the server is only persisted, announced and synced
        // once the probe accepts the credentials.
        self.pending_servers.insert(
            id.clone(),
            PendingServer {
                info,
                client: client.clone(),
                credential,
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
        self.install_server(info, preset.api, None, credential);
    }

    fn install_server(
        &mut self,
        info: ServerInfo,
        api: Arc<dyn SubsonicApi>,
        client: Option<Arc<Client>>,
        credential: WireCredential,
    ) {
        let replaced = self.server.as_ref().is_some_and(|s| s.info.id != info.id);
        if replaced {
            // One server for now: switching identity closes the old session.
            self.unload();
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
        state.info.reachable = true;
        state.api = Some(api.clone());
        state.client = client;
        state.credential = Some(credential.clone());
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
        self.emit_everything();
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
        if state.kind != NetworkKind::Offline && (was_offline || self.network.is_some()) {
            self.last_outbox_retry = self.now();
            self.schedule_flush();
            self.maybe_start_sync(false, false);
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
