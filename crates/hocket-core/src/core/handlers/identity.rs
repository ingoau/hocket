//! The device's identity: loading and persisting it, pairing (both sides),
//! removing devices, credential sync and the coordinator's claim and
//! invites. The member list itself and its rules live in
//! [`crate::identity`]; the Connect engine shares the same
//! [`crate::identity::SharedIdentity`] for its handshakes.

use std::time::Duration;

use crate::api::*;
use crate::connect::engine::Input;
use crate::connect::wire::{Msg, WireMessage};
use crate::core::actor::Actor;
use crate::core::{ActorMsg, Internal};
use crate::db::Db;
use crate::identity::pairing::{Invite, InviteAccount, Joiner, Rendezvous};
use crate::identity::{
    CoordinatorPin, Identity, SecretKind, SyncedSecret, SyncedSecrets, STORAGE_KEY,
};

/// How long a one-shot exchange (pairing, a claim) waits for its answer.
const EXCHANGE_TIMEOUT: Duration = Duration::from_secs(60);
/// How long one LAN rendezvous address gets to connect.
const LAN_CONNECT_TIMEOUT: Duration = Duration::from_secs(4);
/// The claim link `hocket-coordinator pair --url` prints as a QR code.
pub const CLAIM_LINK_PREFIX: &str = "hocket://claim?";

/// This device's identity from the database, or `fresh` / a new one (first
/// start, or a database from another device).
pub(crate) fn load_identity(
    db: &Db,
    device_id: &str,
    device_name: &str,
    now: EpochMs,
    fresh: Option<Identity>,
) -> Identity {
    match db.saved_state_get::<Identity>(STORAGE_KEY) {
        Ok(Some(i)) if i.device_id == device_id && i.shared.identity() == i.id => i,
        _ => {
            let i = fresh
                .filter(|i| i.device_id == device_id)
                .unwrap_or_else(|| Identity::create(device_id, device_name, now));
            let _ = db.saved_state_set(STORAGE_KEY, &i, &crate::util::WallClock);
            i
        }
    }
}

/// `(url, code)` from a `hocket://claim?url=…&code=…` link, or `None`.
pub fn parse_claim_link(link: &str) -> Option<(String, String)> {
    let q = link.trim().strip_prefix(CLAIM_LINK_PREFIX)?;
    let parsed = url::Url::parse(&format!("http://x/?{q}")).ok()?;
    let mut url = None;
    let mut code = None;
    for (k, v) in parsed.query_pairs() {
        match k.as_ref() {
            "url" => url = Some(v.to_string()),
            "code" => code = Some(v.to_string()),
            _ => {}
        }
    }
    Some((url.unwrap_or_default(), code?))
}

/// The outcome of a one-shot exchange.
pub enum ExchangeResult {
    Paired(Box<Result<(Identity, Vec<PairingAccount>), String>>),
    Coordinator(Result<(String, crate::identity::crypto::PublicKeys, bool), String>),
}

impl Actor {
    pub(crate) fn persist_identity(&mut self) {
        let id = self.identity.lock().clone();
        if let Err(e) = self
            .db
            .saved_state_set(STORAGE_KEY, &id, self.clock.as_ref())
        {
            self.error(ErrorKind::Storage, "save identity", Some(e.to_string()));
        }
    }

    pub(crate) fn identity_info(&self) -> IdentityInfo {
        let mut id = self.identity.lock();
        let state = id.state().clone();
        let me = id.device_id.clone();
        let mut devices: Vec<IdentityDevice> = state
            .members
            .values()
            .map(|d| IdentityDevice {
                id: d.id.clone(),
                name: d.name.clone(),
                is_self: d.id == me,
                last_synced: id.shared.last_synced.get(&d.id).copied(),
            })
            .collect();
        devices.sort_by(|a, b| b.is_self.cmp(&a.is_self).then(a.name.cmp(&b.name)));
        let sync = id.shared.credential_sync_on();
        IdentityInfo {
            id: id.id.clone(),
            device_id: me.clone(),
            devices,
            removed: state.removed.contains(&me),
            credential_sync: sync,
            coordinator: id.shared.coordinator_pin().map(|p| CoordinatorInfo {
                url: p.url.clone(),
                owner: p.owner,
                holds_key: sync,
            }),
        }
    }

    fn emit_identity(&mut self) {
        let identity = self.identity_info();
        self.emit(Event::IdentityChanged { identity });
    }

    /// A change made here: persist, show, pass on to the engine.
    pub(crate) fn identity_changed_locally(&mut self) {
        self.persist_identity();
        self.emit_identity();
        self.apply_coordinator_pin();
        if self.engine.is_some() {
            self.engine_input(Input::IdentityChanged);
        }
    }

    /// The engine merged something from a peer.
    pub(crate) fn identity_changed_by_peer(&mut self) {
        self.persist_identity();
        self.emit_identity();
        self.apply_coordinator_pin();
        self.adopt_synced_credentials();
    }

    pub(crate) fn on_paired(&mut self, device_id: DeviceId, name: String) {
        self.persist_identity();
        self.emit_identity();
        self.emit(Event::DevicePaired { device_id, name });
    }

    /// The pinned coordinator is the one to use.
    fn apply_coordinator_pin(&mut self) {
        let pinned = self
            .identity
            .lock()
            .shared
            .coordinator_pin()
            .map(|p| p.url.clone());
        let Some(url) = pinned else { return };
        let current = self
            .settings
            .get_string(crate::settings::keys::CONNECT_COORDINATOR_URL);
        if current.as_deref() != Some(url.as_str()) {
            self.set_setting_value(
                crate::settings::keys::CONNECT_COORDINATOR_URL,
                serde_json::Value::String(url),
            );
        }
    }

    // -- pairing ------------------------------------------------------------

    pub(crate) fn create_pairing_invite(&mut self) {
        let lan = self
            .listener_port
            .map(|port| self.io.lan_urls(port))
            .unwrap_or_default();
        let coordinator = self
            .identity
            .lock()
            .shared
            .coordinator_pin()
            .map(|p| p.url.clone());
        if lan.is_empty() && coordinator.is_none() {
            self.emit(Event::PairingFailed {
                message: "This device isn't on a network the other device could reach, and no coordinator is set up. Connect both devices to the same network and try again.".into(),
            });
            return;
        }
        let accounts: Vec<InviteAccount> = self
            .accounts
            .iter()
            .map(|a| a.info.clone())
            .map(|a| InviteAccount {
                kind: a.kind.as_str().into(),
                url: a.url,
                username: a.username,
                name: Some(a.name).filter(|n| !n.is_empty()),
            })
            .collect();
        let now = self.now();
        let invite =
            self.identity
                .lock()
                .invite(Rendezvous { lan, coordinator }, accounts, None, now);
        self.persist_identity();
        if self.engine.is_some() {
            self.engine_input(Input::IdentityChanged);
        }
        self.emit(Event::PairingInvite {
            link: invite.to_link(),
            expires_at: invite.expires_at,
        });
    }

    pub(crate) fn pair_with_invite(&mut self, link: String) {
        let invite = match Invite::from_link(&link) {
            Ok(i) => i,
            Err(message) => {
                self.emit(Event::PairingFailed { message });
                return;
            }
        };
        if invite.identity == self.identity.lock().id {
            self.emit(Event::PairingFailed {
                message: "This device is already part of that identity.".into(),
            });
            return;
        }
        let joiner = match Joiner::new(
            invite.clone(),
            &self.cfg.device_id,
            &self.cfg.device_name,
            self.now(),
        ) {
            Ok(j) => j,
            Err(message) => {
                self.emit(Event::PairingFailed { message });
                return;
            }
        };
        let allow_insecure = self
            .settings
            .get_bool(crate::settings::keys::CONNECT_ALLOW_INSECURE_COORDINATOR);
        let mut routes: Vec<Vec<String>> = invite
            .rendezvous
            .lan
            .iter()
            .map(|u| vec![u.clone()])
            .collect();
        if let Some(c) = invite.rendezvous.coordinator.clone() {
            if crate::connect::auth::coordinator_url_check(&c, allow_insecure).is_ok() {
                routes.push(vec![c]);
            }
        }
        let have: Vec<(String, String)> = self
            .accounts
            .iter()
            .map(|a| a.info.clone())
            .map(|a| (crate::connect::wire::server_key(&a.url), a.username))
            .collect();
        let accounts: Vec<PairingAccount> = invite
            .accounts
            .iter()
            .filter(|a| a.kind == "navidrome")
            .filter(|a| {
                !have.contains(&(crate::connect::wire::server_key(&a.url), a.username.clone()))
            })
            .map(|a| PairingAccount {
                kind: AccountKind::Navidrome,
                url: a.url.clone(),
                username: a.username.clone(),
                name: a.name.clone(),
            })
            .collect();
        let frame = Msg::PairJoin {
            identity: invite.identity.clone(),
            join: joiner.join.clone(),
            relay: None,
        };
        let io = self.io.clone();
        let tx = self.tx.clone();
        self.spawn(async move {
            let mut last = String::new();
            let mut answer = None;
            for (i, candidates) in routes.iter().enumerate() {
                let lan = i < invite.rendezvous.lan.len();
                match exchange(io.as_ref(), candidates.clone(), frame.clone(), lan).await {
                    Ok(m) => {
                        answer = Some(m);
                        break;
                    }
                    Err(e) => last = e,
                }
            }
            let result = match answer {
                Some(Msg::PairAccept { accept, .. }) => joiner.finish(*accept).map(|id| (id, accounts)),
                Some(Msg::Refuse { message, .. }) => Err(format!("The other device said no: {message}")),
                Some(_) => Err("pairing failed: unexpected answer".into()),
                None if routes.is_empty() => Err("The invite has no way to reach the other device.".into()),
                None => {
                    tracing::debug!(error = %last, "pairing: no rendezvous reachable");
                    Err("Couldn't reach the other device. Put both devices on the same network (or set up a coordinator) and try again while the code is still showing.".into())
                }
            };
            let _ = tx.send(ActorMsg::Internal(Internal::Exchange(ExchangeResult::Paired(
                Box::new(result),
            ))));
        });
    }

    pub(crate) fn on_exchange(&mut self, r: ExchangeResult) {
        match r {
            ExchangeResult::Paired(result) => match *result {
                Ok((identity, accounts)) => {
                    self.log("info", format!("joined identity {}", identity.id));
                    *self.identity.lock() = identity;
                    self.persist_identity();
                    self.rescope_session();
                    self.emit_identity();
                    self.apply_coordinator_pin();
                    self.adopt_synced_credentials();
                    self.emit(Event::PairingFinished { accounts });
                }
                Err(message) => self.emit(Event::PairingFailed { message }),
            },
            ExchangeResult::Coordinator(result) => match result {
                Ok((url, keys, owner)) => {
                    let now = self.now();
                    self.identity.lock().set_coordinator(
                        Some(CoordinatorPin {
                            url: url.clone(),
                            keys,
                            owner,
                        }),
                        now,
                    );
                    self.identity_changed_locally();
                    if self.engine.is_some() {
                        self.engine_input(Input::SetCoordinatorUrl(Some(url.clone())));
                        self.engine_input(Input::ConnectCoordinator);
                    }
                    self.emit(Event::CoordinatorJoined { url, owner });
                }
                Err(message) => self.emit(Event::CoordinatorFailed { message }),
            },
        }
    }

    // -- membership ---------------------------------------------------------

    pub(crate) fn remove_device(&mut self, device_id: DeviceId) {
        let now = self.now();
        let leaving = device_id == self.cfg.device_id;
        let result = self.identity.lock().remove_device(&device_id, now);
        match result {
            Ok(_) => {
                self.identity_changed_locally();
                if leaving {
                    self.reset_identity();
                }
            }
            Err(e) => self.toast(format!("Couldn't remove that device: {e}"), None),
        }
    }

    pub(crate) fn reset_identity(&mut self) {
        let now = self.now();
        let name = self.cfg.device_name.clone();
        self.identity.lock().reset(&name, now);
        self.persist_identity();
        self.rescope_session();
        self.emit_identity();
    }

    // -- credential sync ----------------------------------------------------

    fn local_secrets(&self) -> SyncedSecrets {
        let mut entries: Vec<SyncedSecret> = self
            .accounts
            .iter()
            .filter(|a| a.info.kind == AccountKind::Navidrome)
            .filter_map(|a| {
                Some(SyncedSecret {
                    account_id: a.info.id.clone(),
                    kind: SecretKind::Navidrome,
                    url: a.info.url.clone(),
                    username: a.info.username.clone(),
                    secret: a.password.as_ref()?.expose().to_string(),
                    name: Some(a.info.name.clone()).filter(|n| !n.is_empty()),
                })
            })
            .collect();
        entries.sort_by(|a, b| a.account_id.cmp(&b.account_id));
        SyncedSecrets { entries }
    }

    pub(crate) fn set_credential_sync(&mut self, on: bool) {
        let now = self.now();
        let mut secrets = self.local_secrets();
        // Keep what other devices synced and this one lacks.
        if let Some(existing) = self.identity.lock().secrets() {
            for e in existing.entries {
                if !secrets.entries.iter().any(|s| s.account_id == e.account_id) {
                    secrets.entries.push(e);
                }
            }
        }
        let result = self
            .identity
            .lock()
            .set_credential_sync(on, Some(&secrets), now);
        match result {
            Ok(()) => self.identity_changed_locally(),
            Err(e) => self.toast(format!("Couldn't change credential sync: {e}"), None),
        }
    }

    /// The accounts or passwords changed: refresh the synced blob (when on).
    pub(crate) fn credentials_changed(&mut self) {
        if !self.identity.lock().shared.credential_sync_on() {
            return;
        }
        let mine = self.local_secrets();
        let now = self.now();
        let changed = {
            let mut id = self.identity.lock();
            let mut merged = id.secrets().unwrap_or_default();
            for e in mine.entries {
                merged.entries.retain(|s| s.account_id != e.account_id);
                merged.entries.push(e);
            }
            // Accounts removed here leave the blob too.
            let ids: Vec<String> = self.accounts.ids();
            merged.entries.retain(|s| ids.contains(&s.account_id));
            merged
                .entries
                .sort_by(|a, b| a.account_id.cmp(&b.account_id));
            id.put_secrets(&merged, now)
        };
        if changed {
            self.identity_changed_locally();
        }
    }

    /// Add the accounts credential sync carries that this device lacks (or
    /// lacks a password for), and hand their passwords to the platform.
    pub(crate) fn adopt_synced_credentials(&mut self) {
        let Some(secrets) = self.identity.lock().secrets() else {
            return;
        };
        let mut credentials = vec![];
        for e in secrets.entries {
            if e.kind != SecretKind::Navidrome {
                continue;
            }
            let has_password = self
                .accounts
                .get(&e.account_id)
                .is_some_and(|a| a.password.is_some());
            if has_password {
                continue;
            }
            credentials.push(SyncedAccountCredential {
                account_id: e.account_id.clone(),
                url: e.url.clone(),
                username: e.username.clone(),
                password: Secret(e.secret.clone()),
            });
            self.add_account(AccountSetup::Navidrome {
                url: e.url,
                username: e.username,
                password: Secret(e.secret),
                name: e.name,
            });
        }
        if !credentials.is_empty() {
            self.emit(Event::CredentialsSynced { credentials });
        }
    }

    // -- the coordinator ----------------------------------------------------

    pub(crate) fn join_coordinator(&mut self, url: String, code: String, claim: bool) {
        let (url, code) = match parse_claim_link(&code) {
            Some((u, c)) if url.trim().is_empty() => (u, c),
            Some((_, c)) => (url, c),
            None => (url, code),
        };
        let url = url.trim().to_string();
        let allow_insecure = self
            .settings
            .get_bool(crate::settings::keys::CONNECT_ALLOW_INSECURE_COORDINATOR);
        if let Err(e) = crate::connect::auth::coordinator_url_check(&url, allow_insecure) {
            self.emit(Event::CoordinatorFailed {
                message: format!("That coordinator address can't be used: {e}"),
            });
            return;
        }
        let sync = Box::new(self.identity.lock().sync());
        let code = code.trim().to_string();
        let frame = if claim {
            Msg::CoordClaim { code, sync }
        } else {
            Msg::CoordRedeem { code, sync }
        };
        let io = self.io.clone();
        let tx = self.tx.clone();
        self.spawn(async move {
            let result = match exchange(io.as_ref(), vec![url.clone()], frame, false).await {
                Ok(Msg::CoordAdmitted { keys, owner }) => Ok((url, keys, owner)),
                Ok(Msg::Refuse { message, .. }) => Err(message),
                Ok(_) => Err("The coordinator gave an unexpected answer.".into()),
                Err(e) => Err(format!("Couldn't reach the coordinator: {e}")),
            };
            let _ = tx.send(ActorMsg::Internal(Internal::Exchange(
                ExchangeResult::Coordinator(result),
            )));
        });
    }

    pub(crate) fn coordinator_request(&mut self, msg: Msg) {
        let owner = self
            .identity
            .lock()
            .shared
            .coordinator_pin()
            .is_some_and(|p| p.owner);
        if !owner {
            self.emit(Event::CoordinatorFailed {
                message: "Only the identity that claimed the coordinator can invite others.".into(),
            });
            return;
        }
        if self.engine.is_none() {
            return;
        }
        self.engine_input(Input::ToCoordinator(msg));
    }
}

/// Connect, send one frame, wait for the answer that ends the exchange.
async fn exchange(
    io: &dyn crate::core::io::ConnectIo,
    candidates: Vec<String>,
    frame: Msg,
    lan: bool,
) -> Result<Msg, String> {
    let connect = io.connect(format!("pair-{}", crate::util::new_id()), candidates);
    let conn = if lan {
        tokio::time::timeout(LAN_CONNECT_TIMEOUT, connect)
            .await
            .map_err(|_| "timed out".to_string())?
    } else {
        connect.await
    }
    .map_err(|e| e.to_string())?;
    let mut rx = conn.rx;
    conn.tx
        .send(WireMessage::new(frame))
        .map_err(|_| "connection closed".to_string())?;
    let deadline = tokio::time::Instant::now() + EXCHANGE_TIMEOUT;
    loop {
        let m = tokio::time::timeout_at(deadline, rx.recv())
            .await
            .map_err(|_| "no answer".to_string())?
            .ok_or_else(|| "connection closed".to_string())?;
        match m.msg {
            Msg::PairAccept { .. } | Msg::CoordAdmitted { .. } | Msg::Refuse { .. } => {
                return Ok(m.msg)
            }
            _ => continue,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claim_links_parse() {
        assert_eq!(
            parse_claim_link("hocket://claim?url=wss%3A%2F%2Fc.example%2Fws&code=ABCD-1234"),
            Some(("wss://c.example/ws".into(), "ABCD-1234".into()))
        );
        assert_eq!(
            parse_claim_link("hocket://claim?code=XY"),
            Some((String::new(), "XY".into()))
        );
        assert!(parse_claim_link("ABCD").is_none());
    }
}
