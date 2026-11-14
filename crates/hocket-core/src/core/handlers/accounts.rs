//! Accounts (the registry, credentials, the capability probe, priority
//! order), library sync scheduling, job runners and the outbox flusher.

use std::collections::HashMap;
use std::sync::Arc;

use crate::api::*;
use crate::connect::auth::{derive_lan_key, LanKey};
use crate::connect::engine::Input;
use crate::connect::wire::{scope_key, Credential as WireCredential};
use crate::core::actor::Actor;
use crate::core::state::AccountState;
use crate::core::{ActorMsg, Internal, PendingAccount, PresetServer};
use crate::db::sync::{LibrarySync, LibrarySyncRunner, SyncCursor, SyncJobPayload};
use crate::downloads::DownloadRunner;
use crate::jobs::{JobSpec, RetryAction};
use crate::outbox::OutboxFlushRunner;
use crate::provider::{NavidromeProvider, Provider};
use crate::subsonic::auth::{self, AuthMode};
use crate::subsonic::{Client, ClientConfig, ReqwestTransport, SubsonicApi};

/// Deterministic server id from the identity the device authenticated with.
pub fn server_id_for(url: &str, username: &str) -> String {
    crate::connect::wire::account_id_for_scope(&scope_key(url, username))
}

/// A server URL from `AddAccount`: `http(s)://` with a host and no embedded
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
    pub(crate) fn add_account(&mut self, setup: AccountSetup) {
        match setup {
            AccountSetup::Navidrome {
                url,
                username,
                password,
                name,
            } => self.add_navidrome(url, username, password, name),
        }
    }

    fn add_navidrome(
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
        // An account this device already authenticated with (persisted row,
        // capabilities known): install it now so downloads play offline, and
        // verify in the background. A failed probe keeps it installed.
        let persisted = match self.accounts.get(&id) {
            Some(s) => Some(s.info.clone()),
            None => self
                .db
                .accounts()
                .ok()
                .and_then(|list| list.into_iter().find(|s| s.id == id)),
        };
        if let Some(mut info) = persisted {
            info.name = name;
            info.reachable = false;
            self.pending_accounts.remove(&id);
            self.install_account(
                info,
                client.clone() as Arc<dyn SubsonicApi>,
                Some(client),
                credential,
                lan_key,
                false,
            );
            self.probe_account(&id);
            return;
        }
        let info = AccountInfo {
            id: id.clone(),
            kind: AccountKind::Navidrome,
            priority: self.accounts.next_priority(),
            source: id.clone(),
            url: url.clone(),
            username: username.clone(),
            name,
            ..Default::default()
        };
        // Verify first: a new account is only persisted, announced and synced
        // once the probe accepts the credentials.
        self.pending_accounts.insert(
            id.clone(),
            PendingAccount {
                info,
                client: client.clone(),
                credential,
                lan_key,
            },
        );
        self.spawn_probe(id, client);
    }

    fn spawn_probe(&mut self, account_id: AccountId, client: Arc<Client>) {
        let tx = self.tx.clone();
        self.spawn(async move {
            let result = client.probe().await.map_err(|e| (e.kind(), e.to_string()));
            let _ = tx.send(ActorMsg::Internal(Internal::Probed { account_id, result }));
        });
    }

    /// A server supplied at construction (tests): already authenticated.
    pub(crate) fn attach_preset_server(&mut self, preset: PresetServer) {
        let id = preset.api.server_id().to_string();
        let username = preset
            .api
            .username()
            .unwrap_or_else(|| preset.username.clone());
        let priority = self
            .accounts
            .get(&id)
            .map(|a| a.info.priority)
            .unwrap_or_else(|| self.accounts.next_priority());
        let info = AccountInfo {
            id: id.clone(),
            kind: AccountKind::Navidrome,
            priority,
            source: id,
            url: preset.url.clone(),
            username: username.clone(),
            name: "Test server".into(),
            capabilities: preset.api.capabilities(),
            reachable: true,
            ..Default::default()
        };
        let credential = wire_credential(&preset.url, &username, &preset.password);
        let lan_key = derive_lan_key(&scope_key(&preset.url, &username), &preset.password);
        self.install_account(info, preset.api, None, credential, lan_key, true);
    }

    /// The scope key of the account sessions belong to.
    fn primary_scope(&self) -> Option<String> {
        self.accounts
            .primary()
            .map(|a| scope_key(&a.info.url, &a.info.username))
    }

    /// Close the session (the engine, and everything keyed on its scope).
    fn close_session(&mut self, flush: bool) {
        if flush {
            self.flush_persistence(true);
            self.save_position();
        } else {
            self.unload();
        }
        self.engine = None;
        self.scope = None;
        self.drop_stash();
        self.drop_snoozes();
        self.drop_album_resumes();
        self.last_doc = None;
    }

    /// After the accounts changed: when the session's account (the
    /// highest-priority Navidrome one) is no longer the one the open
    /// session belongs to, close it and open the new one's.
    fn rescope_session(&mut self) {
        let want = self.primary_scope();
        let open = self.scope.clone().filter(|_| self.engine.is_some());
        if open.is_some() && open != want {
            self.close_session(false);
        }
        if self.engine.is_none() && want.is_some() {
            self.open_session();
        }
    }

    fn install_account(
        &mut self,
        mut info: AccountInfo,
        api: Arc<dyn SubsonicApi>,
        client: Option<Arc<Client>>,
        credential: WireCredential,
        lan_key: LanKey,
        reachable: bool,
    ) {
        let provider: Arc<dyn Provider> = Arc::new(NavidromeProvider::new(api.clone()));
        info.features = provider.capabilities();
        let is_primary_now = self
            .accounts
            .primary()
            .is_some_and(|p| p.info.id == info.id);
        // The LAN key is fixed at engine construction: a new password for the
        // session's account rebuilds the engine (after flushing the document).
        let rekeyed = is_primary_now
            && self.engine.is_some()
            && self
                .accounts
                .get(&info.id)
                .is_some_and(|s| s.lan_key.as_ref() != Some(&lan_key));
        if rekeyed {
            self.close_session(true);
        }
        if let Err(e) = self.db.upsert_account(&info, self.now()) {
            self.error(ErrorKind::Storage, "save account", Some(e.to_string()));
        }
        let mut state = self
            .accounts
            .take(&info.id)
            .unwrap_or_else(|| AccountState::new(info.clone()));
        state.info = info;
        state.info.reachable = reachable;
        state.provider = Some(provider);
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
        let id = state.info.id.clone();
        self.accounts.insert(state);
        self.providers_changed();
        self.rescope_session();
        if self.scope_account().as_deref() == Some(id.as_str()) && self.engine.is_some() {
            self.engine_input(Input::SetCredential(Some(credential)));
        }
        let accounts = self.account_infos();
        self.emit(Event::AccountsChanged { accounts });
        if self.started {
            self.register_job_runners();
            self.maybe_start_sync(false, true);
            self.schedule_flush();
            if !self.playback.loaded && self.playback.want_playing {
                self.ensure_current_loaded();
            }
        }
    }

    /// The set of providers changed: hand it to the background readers.
    fn providers_changed(&mut self) {
        if let Some(p) = &self.stream_reader {
            p.set_providers(self.accounts.providers());
        }
        self.mark_prefetch_check();
    }

    fn ensure_current_loaded(&mut self) {
        if self.owns_transport() {
            if let Some(item) = self.doc().and_then(|d| d.current.clone()) {
                tracing::debug!(target: "hocket::player", key = %item.key, "a server is reachable again: loading the current item");
                let play = self.playback.want_playing;
                let pos = self.playback.position_ms;
                self.load_item(&item, pos, play, None);
            }
        }
    }

    pub(crate) fn remove_account(&mut self, account_id: AccountId) {
        let Some(removed) = self.accounts.take(&account_id) else {
            return;
        };
        let was_primary = self.engine.is_some()
            && self.scope.as_deref()
                == Some(scope_key(&removed.info.url, &removed.info.username).as_str());
        if was_primary {
            self.close_session(false);
            for key in ["session", "connect", "position"] {
                let scope = scope_key(&removed.info.url, &removed.info.username);
                let _ = self.db.saved_state_delete(&format!("{key}:{scope}"));
            }
        } else if self
            .playback
            .track
            .as_ref()
            .is_some_and(|t| t.server_id == account_id)
        {
            // What plays came from the account going away.
            self.unload();
        }
        // Its syncs (and their problems) must not read as another account's.
        if let Some(job) = removed.sync_job.clone() {
            let _ = self.jobs.cancel(&job);
        }
        if self.accounts.is_empty() {
            if let Err(e) = self.jobs.forget_kind(JobKind::LibrarySync) {
                self.log("warn", format!("forget sync jobs: {e}"));
            }
        }
        self.providers_changed();
        self.prefetch_tick(self.now());
        if let Err(e) = self.db.delete_account(&account_id) {
            self.error(ErrorKind::Storage, "remove account", Some(e.to_string()));
        }
        self.persist_priorities();
        self.rescope_session();
        if self.started {
            self.register_job_runners();
        }
        self.emit_everything(false);
    }

    pub(crate) fn reorder_accounts(&mut self, ids: Vec<AccountId>) {
        self.accounts.reorder(&ids);
        self.persist_priorities();
        self.providers_changed();
        self.rescope_session();
        if self.started {
            self.register_job_runners();
        }
        let accounts = self.account_infos();
        self.emit(Event::AccountsChanged { accounts });
    }

    fn persist_priorities(&mut self) {
        let order = self.accounts.ids();
        if let Err(e) = self.db.set_account_priorities(&order) {
            self.error(
                ErrorKind::Storage,
                "save account order",
                Some(e.to_string()),
            );
        }
    }

    pub(crate) fn probe_account(&mut self, account_id: &str) {
        let Some(s) = self.accounts.get_mut(account_id) else {
            return;
        };
        if s.probing {
            return;
        }
        let Some(client) = s.client.clone() else {
            return;
        };
        s.probing = true;
        self.spawn_probe(account_id.to_string(), client);
    }

    pub(crate) fn on_probed(
        &mut self,
        account_id: AccountId,
        result: Result<ServerCapabilities, (ErrorKind, String)>,
    ) {
        if let Some(pending) = self.pending_accounts.remove(&account_id) {
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
                    self.install_account(
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
        let Some(s) = self.accounts.get_mut(&account_id) else {
            return;
        };
        s.probing = false;
        match result {
            Ok(caps) => {
                s.info.capabilities = caps;
                s.info.reachable = true;
                if let Some(p) = &s.provider {
                    s.info.features = p.capabilities();
                }
                let info = s.info.clone();
                if let Err(e) = self.db.upsert_account(&info, self.now()) {
                    self.error(ErrorKind::Storage, "save account", Some(e.to_string()));
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
        let accounts = self.account_infos();
        self.emit(Event::AccountsChanged { accounts });
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

    pub(crate) fn account_infos(&self) -> Vec<AccountInfo> {
        self.accounts
            .iter()
            .map(|s| {
                let mut info = s.info.clone();
                info.reachable = s.provider.is_some() && s.info.reachable;
                if let Ok(Some(c)) = self
                    .db
                    .saved_state_get::<SyncCursor>(&SyncCursor::key(&info.id))
                {
                    info.last_sync_tracks = c.last_track_count;
                }
                info
            })
            .collect()
    }

    // -- jobs -----------------------------------------------------------------------

    pub(crate) fn register_job_runners(&mut self) {
        let providers = self.accounts.providers();
        if providers.is_empty() {
            return;
        }
        let syncs: HashMap<AccountId, Arc<LibrarySync>> = self
            .accounts
            .iter()
            .filter_map(|a| a.sync.clone().map(|s| (a.info.id.clone(), s)))
            .collect();
        self.jobs.register(
            JobKind::LibrarySync,
            1,
            Arc::new(LibrarySyncRunner::new(syncs)),
        );
        self.jobs.register(
            JobKind::OutboxFlush,
            1,
            Arc::new(OutboxFlushRunner::new(
                self.outbox.clone(),
                providers.clone(),
            )),
        );
        self.jobs.register(
            JobKind::Download,
            2,
            Arc::new(DownloadRunner::new(self.downloads.clone(), providers)),
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
        // A finished sync frees its account for the next; a finished
        // download updates the pins.
        let mut sync_finished = false;
        for s in self.accounts.iter_mut() {
            if let Some(id) = s.sync_job.clone() {
                if let Some(j) = jobs.iter().find(|j| j.id == id) {
                    if matches!(
                        j.state,
                        JobState::Done | JobState::Failed | JobState::Cancelled
                    ) {
                        s.sync_job = None;
                        sync_finished = true;
                    }
                }
            }
        }
        if sync_finished {
            // Accounts waiting behind it (syncs run one at a time).
            self.maybe_start_sync(false, false);
        }
        if jobs.iter().any(|j| {
            j.kind == JobKind::Download && matches!(j.state, JobState::Done | JobState::Failed)
        }) {
            let pins = self.pins();
            self.emit(Event::PinsChanged { pins });
            let storage = self.storage_summary();
            self.emit(Event::StorageChanged { storage });
            for sid in self.accounts.ids() {
                self.emit(Event::LibraryChanged {
                    server_id: sid,
                    tables: vec!["tracks".into()],
                    ids: vec![],
                });
            }
        }
    }

    // -- library sync ----------------------------------------------------------------

    /// Start a sync for every account that is due (or all, when forced) and
    /// has none running. Syncs run one at a time; the rest queue.
    pub(crate) fn maybe_start_sync(&mut self, full: bool, force: bool) {
        for id in self.accounts.ids() {
            self.maybe_start_sync_for(&id, full, force);
        }
    }

    pub(crate) fn maybe_start_sync_for(&mut self, account_id: &str, full: bool, force: bool) {
        let Some(s) = self.accounts.get(account_id) else {
            return;
        };
        if s.provider.is_none() || s.sync.is_none() || s.sync_job.is_some() {
            return;
        }
        // A sync recovered from before a restart (no account holds it) runs
        // first.
        let tracked = self.accounts.iter().any(|a| a.sync_job.is_some());
        if !tracked && self.jobs.has_active(JobKind::LibrarySync).unwrap_or(false) {
            return;
        }
        let now = self.now();
        let cursor: SyncCursor = self
            .db
            .saved_state_get(&SyncCursor::key(account_id))
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
        let payload = serde_json::to_string(&SyncJobPayload {
            full,
            account_id: Some(account_id.to_string()),
        })
        .unwrap_or_default();
        let label = match (full, self.accounts.len() > 1) {
            (true, false) => "Full library sync".to_string(),
            (false, false) => "Library sync".to_string(),
            (true, true) => format!("Full library sync: {}", s.info.name),
            (false, true) => format!("Library sync: {}", s.info.name),
        };
        match self
            .jobs
            .submit(JobSpec::new(JobKind::LibrarySync, label).payload(payload))
        {
            Ok(id) => {
                if let Some(s) = self.accounts.get_mut(account_id) {
                    s.sync_job = Some(id);
                    s.last_sync_started = Some(now);
                }
            }
            Err(e) => self.error(ErrorKind::Storage, "start sync", Some(e.to_string())),
        }
    }

    pub(crate) fn on_sync_progress(&mut self, p: SyncProgress) {
        let newly_ready: Vec<String> = match self.accounts.get(&p.server_id) {
            Some(s) => p
                .ready_tables
                .iter()
                .filter(|t| !s.ready_tables.contains(t))
                .cloned()
                .collect(),
            None => vec![],
        };
        if let Some(s) = self.accounts.get_mut(&p.server_id) {
            for t in &newly_ready {
                s.ready_tables.push(t.clone());
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
            if let Some(s) = self.accounts.get_mut(&p.server_id) {
                s.info.last_sync = Some(now);
                let _ = self.db.set_account_last_sync(&s.info.id, now);
            }
            self.emit(Event::LibraryChanged {
                server_id: p.server_id.clone(),
                tables: vec![],
                ids: vec![],
            });
            let accounts = self.account_infos();
            self.emit(Event::AccountsChanged { accounts });
            self.reconcile_playlist_pins();
            // Cached audio of tracks whose file changed on the server is stale.
            match self.downloads.invalidate_changed_sources() {
                Ok(changed) if !changed.is_empty() => self.on_stream_cache_changed(changed),
                Ok(_) => {}
                Err(e) => self.log("warn", format!("stream cache invalidation: {e}")),
            }
            // The queue view may have been rendered from bare summaries.
            self.emit_queue();
        }
        self.sync_progress = Some(p.clone());
        self.emit(Event::SyncProgress { progress: p });
    }

    pub(crate) fn reconcile_playlist_pins(&mut self) {
        let mut changed = false;
        for sid in self.accounts.ids() {
            let pins = self.downloads.pins(&sid).unwrap_or_default();
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
        }
        if changed {
            let pins = self.pins();
            self.emit(Event::PinsChanged { pins });
        }
    }

    // -- outbox -----------------------------------------------------------------------

    /// Flush the outbox now (bounded to one in flight); pending entries are
    /// retried from the tick. Each account's entries go to its provider; an
    /// account that is offline keeps its entries for a later flush.
    pub(crate) fn schedule_flush(&mut self) {
        if self.flush_in_flight {
            return;
        }
        let providers = self.accounts.providers();
        if providers.is_empty() {
            return;
        }
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
            let mut total: Option<crate::outbox::FlushReport> = None;
            for p in providers.iter() {
                if let Ok(r) = outbox.flush(p.as_ref(), 100).await {
                    total.get_or_insert_with(Default::default).absorb(r);
                }
            }
            let _ = tx.send(ActorMsg::Internal(Internal::OutboxFlushed {
                report: total,
            }));
        });
    }

    pub(crate) fn on_outbox_flushed(&mut self, report: Option<crate::outbox::FlushReport>) {
        self.flush_in_flight = false;
        let Some(report) = report else { return };
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
            for sid in &report.servers {
                self.emit(Event::LibraryChanged {
                    server_id: sid.clone(),
                    tables: tables.clone(),
                    ids: vec![],
                });
            }
        }
        if !report.playlists.is_empty() {
            // The server's answer (its order after a rebase, a created
            // playlist's id) is the truth: show it and share it.
            self.announce_playlists(&report.playlists, true);
        }
        if report.deferred == 0
            && self
                .outbox
                .pending_count_for(&self.accounts.providers().accounts())
                .unwrap_or(0)
                > 0
        {
            // More was queued while flushing.
            self.schedule_flush();
        }
    }

    pub(crate) fn set_network_state(&mut self, state: NetworkState) {
        let was_offline = self
            .network
            .as_ref()
            .is_some_and(|n| n.kind == NetworkKind::Offline);
        let changed = self.network.as_ref() != Some(&state);
        // Only what playback cares about: the kind, and whether it is metered.
        if self
            .network
            .as_ref()
            .is_none_or(|n| n.kind != state.kind || n.metered != state.metered)
        {
            let from = self.network.as_ref().map(|n| format!("{:?}", n.kind));
            self.log(
                "info",
                format!(
                    "network: {} -> {:?}{}",
                    from.as_deref().unwrap_or("unknown"),
                    state.kind,
                    if state.metered { " (metered)" } else { "" }
                ),
            );
        }
        self.downloads.set_network(Some(state.clone()));
        self.network = Some(state.clone());
        if changed {
            self.emit(Event::NetworkChanged {
                network: self.network.clone(),
            });
        }
        self.mark_prefetch_check();
        if state.kind != NetworkKind::Offline && (was_offline || self.network.is_some()) {
            // Prefetches that failed on the old network are due now.
            self.prefetch_retry_now();
            self.last_outbox_retry = self.now();
            self.schedule_flush();
            self.maybe_start_sync(false, false);
            // A server installed offline (or whose last probe failed) is
            // verified again now that there is a network.
            let stale: Vec<AccountId> = self
                .accounts
                .iter()
                .filter(|s| !s.info.reachable && !s.probing && s.client.is_some())
                .map(|s| s.info.id.clone())
                .collect();
            for id in stale {
                self.probe_account(&id);
            }
        }
        // The stream URL may change with the network's transcoding profile.
        if self.playback.loaded && self.owns_transport() {
            self.refresh_next();
        }
        if state.kind != NetworkKind::Offline {
            self.clear_offline_skips();
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
