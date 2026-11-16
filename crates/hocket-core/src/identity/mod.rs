//! Hocket identities: what sync is scoped to (`docs/providers.md`, Phase 2).
//!
//! An identity is a random 128-bit id created on the first device. Every
//! device in it has its own keys ([`crypto::DeviceSecret`]) and holds a copy
//! of the member list ([`log::MemberLog`]), which every peer and the
//! coordinator validate the same way. On top of the list ride:
//!
//! - **The group key**, rotated whenever a device is removed (and when
//!   credential sync is turned off). It reaches each member as an X25519
//!   envelope ([`Grant`]); envelopes for members that are offline wait in
//!   every other member's (and the coordinator's) copy of [`SharedState`].
//! - **Credential sync** (off by default): Navidrome secrets as one blob
//!   sealed with the group key ([`CredentialBlob`]). Only kinds in
//!   [`SecretKind`] can go in it; YouTube Music sign-ins have no kind, so
//!   Google cookies can never be synced. While it is on, the coordinator
//!   holds the group key too (it gets an envelope) so it can hand the key to
//!   a member no other device is online for.
//! - **The coordinator pin**: which coordinator (URL and key) the identity
//!   claimed or was invited to.
//! - **Last synced** per device, for the device list.
//!
//! Everything a device keeps lives in [`Identity`] (persisted in the core's
//! database under `saved_state` key [`STORAGE_KEY`], device secret
//! included: there is no platform keystore integration yet).

pub mod crypto;
pub mod log;
pub mod pairing;

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::api::{DeviceId, EpochMs};
use crypto::{Canon, DeviceSecret, Envelope, Sealed};
use log::{LogError, MemberDevice, MemberLog, MemberState, OpBody};

pub use log::MemberOp;

/// The identity as the actor and the Connect engine share it.
pub type SharedIdentity = std::sync::Arc<parking_lot::Mutex<Identity>>;

/// `saved_state` key of this device's [`Identity`].
pub const STORAGE_KEY: &str = "identity";
/// The device id grants to the coordinator are addressed to.
pub const COORDINATOR_ID: &str = "coordinator";

/// A new identity id: 128 random bits, hex.
pub fn new_identity_id() -> String {
    crypto::random_hex(16)
}

/// Whether a string is an identity id (32 hex digits).
pub fn is_identity_id(s: &str) -> bool {
    s.len() == 32 && s.bytes().all(|b| b.is_ascii_hexdigit())
}

/// The id of a group key: a hash of it, so anyone can check a key they were
/// handed is the one the member list committed to.
pub fn key_id(key: &[u8; 32]) -> String {
    let mut c = Canon::new("hocket-group-key-id");
    c.bytes(key);
    crypto::sha256_hex(c.finish())[..32].to_string()
}

fn grant_context(identity: &str, key_id: &str, to: &str) -> Vec<u8> {
    let mut c = Canon::new("hocket-group-key-grant");
    c.str(identity).str(key_id).str(to);
    c.finish().to_vec()
}

/// The group key sealed for one device (or the coordinator).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Grant {
    pub key_id: String,
    pub to: DeviceId,
    pub env: Envelope,
}

/// What credential sync may carry. YouTube Music (Google cookies) is
/// deliberately not a kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SecretKind {
    Navidrome,
}

/// One account's secret in the credential blob.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncedSecret {
    pub account_id: String,
    pub kind: SecretKind,
    pub url: String,
    pub username: String,
    pub secret: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

impl std::fmt::Debug for SyncedSecrets {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "SyncedSecrets({} entries)", self.entries.len())
    }
}

/// The plaintext of a [`CredentialBlob`].
#[derive(Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncedSecrets {
    pub entries: Vec<SyncedSecret>,
}

/// The synced credentials, sealed with the group key `keyId`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CredentialBlob {
    pub key_id: String,
    pub author: DeviceId,
    pub at: EpochMs,
    pub sealed: Sealed,
}

fn blob_aad(identity: &str, key_id: &str, author: &str, at: EpochMs) -> Vec<u8> {
    let mut c = Canon::new("hocket-credential-blob");
    c.str(identity).str(key_id).str(author).num(at);
    c.finish().to_vec()
}

/// A last-writer-wins value a member signed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Signed<T> {
    pub value: T,
    pub author: DeviceId,
    pub at: EpochMs,
    pub sig: String,
}

/// The coordinator this identity uses: its URL and keys (pinned when it
/// was claimed or an invite was redeemed).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CoordinatorPin {
    pub url: String,
    pub keys: crypto::PublicKeys,
    /// This identity claimed it (the owner identity).
    #[serde(default)]
    pub owner: bool,
}

/// How a signed record's value is encoded for signing.
pub trait Canonical {
    fn canon(&self, c: &mut Canon);
}

impl Canonical for bool {
    fn canon(&self, c: &mut Canon) {
        c.str(if *self { "on" } else { "off" });
    }
}

impl Canonical for Option<CoordinatorPin> {
    fn canon(&self, c: &mut Canon) {
        match self {
            Some(p) => {
                c.str(&p.url)
                    .str(&p.keys.sign)
                    .str(&p.keys.kx)
                    .str(if p.owner { "owner" } else { "guest" });
            }
            None => {
                c.str("none");
            }
        }
    }
}

impl<T: Canonical> Signed<T> {
    fn bytes(identity: &str, kind: &str, value: &T, author: &str, at: EpochMs) -> Vec<u8> {
        let mut c = Canon::new("hocket-signed-record");
        c.str(identity).str(kind).str(author).num(at);
        value.canon(&mut c);
        c.finish().to_vec()
    }

    fn new(
        identity: &str,
        kind: &str,
        value: T,
        author: &str,
        at: EpochMs,
        s: &DeviceSecret,
    ) -> Self {
        let sig = s.sign(&Self::bytes(identity, kind, &value, author, at));
        Signed {
            value,
            author: author.into(),
            at,
            sig,
        }
    }

    fn check(&self, identity: &str, kind: &str, state: &MemberState) -> bool {
        state.sign_key(&self.author).is_some_and(|k| {
            crypto::verify(
                k,
                &Self::bytes(identity, kind, &self.value, &self.author, self.at),
                &self.sig,
            )
        })
    }

    /// Newer wins; equal times by signature.
    fn newer_than(&self, other: &Self) -> bool {
        (self.at, &self.sig) > (other.at, &other.sig)
    }
}

/// How far ahead of our clock a peer's unsigned time may be.
pub const MAX_SKEW_MS: EpochMs = 5.0 * 60_000.0;

/// Everything about an identity that devices and the coordinator share.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SharedState {
    pub log: MemberLog,
    #[serde(default)]
    pub grants: Vec<Grant>,
    #[serde(default)]
    pub blob: Option<CredentialBlob>,
    #[serde(default)]
    pub credential_sync: Option<Signed<bool>>,
    #[serde(default)]
    pub coordinator: Option<Signed<Option<CoordinatorPin>>>,
    #[serde(default)]
    pub last_synced: BTreeMap<DeviceId, EpochMs>,
}

/// What travels between peers: a [`SharedState`] or the part of it the
/// other side lacks (ops), always whole for the small parts.
pub type IdentitySync = SharedState;

/// What a merge changed.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Changed {
    /// The member list (new ops).
    pub members: bool,
    /// Anything else worth persisting and passing on.
    pub other: bool,
}

impl Changed {
    pub fn any(self) -> bool {
        self.members || self.other
    }
}

/// Who holds keys: a device, or the coordinator.
pub struct Holder<'a> {
    pub id: &'a str,
    pub secret: &'a DeviceSecret,
}

impl SharedState {
    pub fn new(log: MemberLog) -> SharedState {
        SharedState {
            log,
            grants: vec![],
            blob: None,
            credential_sync: None,
            coordinator: None,
            last_synced: BTreeMap::new(),
        }
    }

    pub fn identity(&self) -> &str {
        &self.log.identity
    }

    pub fn state(&mut self) -> &MemberState {
        self.log.state()
    }

    pub fn credential_sync_on(&self) -> bool {
        self.credential_sync.as_ref().is_some_and(|s| s.value)
    }

    pub fn coordinator_pin(&self) -> Option<&CoordinatorPin> {
        self.coordinator.as_ref().and_then(|s| s.value.as_ref())
    }

    /// The part a peer whose member-list heads are `heads` is missing.
    pub fn sync_for(&self, heads: &[String]) -> IdentitySync {
        let mut s = self.clone();
        s.log = MemberLog::partial(self.identity(), self.log.missing_for(heads));
        s
    }

    /// Union in a peer's state (one that proved it is a member). Times from
    /// beyond `now` (plus [`MAX_SKEW_MS`]) are refused: an unsigned blob or
    /// sync time from the future would otherwise win forever.
    pub fn merge(&mut self, other: IdentitySync, now: EpochMs) -> Result<Changed, LogError> {
        let mut ch = Changed::default();
        if other.identity() != self.identity() {
            return Err(LogError::WrongIdentity);
        }
        let added = self.log.merge(other.log.ops())?;
        ch.members = added > 0;
        let state = self.log.state().clone();
        let id = self.identity().to_string();
        let coordinator_holds = self.credential_sync_on() || other.credential_sync_on();
        for g in other.grants {
            let wanted = Some(&g.key_id) == state.key_id.as_ref()
                && (state.is_member(&g.to) || (coordinator_holds && g.to == COORDINATOR_ID))
                && !self
                    .grants
                    .iter()
                    .any(|o| o.key_id == g.key_id && o.to == g.to);
            if wanted && self.grants.len() < 4 * log::MAX_OPS {
                self.grants.push(g);
                ch.other = true;
            }
        }
        if let Some(theirs) = other.credential_sync {
            if theirs.check(&id, "credentialSync", &state)
                && self
                    .credential_sync
                    .as_ref()
                    .is_none_or(|ours| theirs.newer_than(ours))
            {
                self.credential_sync = Some(theirs);
                ch.other = true;
            }
        }
        if let Some(theirs) = other.coordinator {
            if theirs.check(&id, "coordinator", &state)
                && self
                    .coordinator
                    .as_ref()
                    .is_none_or(|ours| theirs.newer_than(ours))
            {
                self.coordinator = Some(theirs);
                ch.other = true;
            }
        }
        if let Some(theirs) = other.blob.filter(|b| b.at <= now + MAX_SKEW_MS) {
            // Only a key holder can seal one under the current key, and
            // those rank first; the author is informational.
            let better = match &self.blob {
                None => true,
                Some(ours) => blob_rank(&theirs, &state) > blob_rank(ours, &state),
            };
            if better {
                self.blob = Some(theirs);
                ch.other = true;
            }
        }
        for (d, t) in other.last_synced {
            if (!state.is_member(&d) && !state.removed.contains(&d)) || t > now + MAX_SKEW_MS {
                continue;
            }
            let e = self.last_synced.entry(d).or_insert(0.0);
            if t > *e {
                *e = t;
                ch.other = true;
            }
        }
        if self.prune() {
            ch.other = true;
        }
        Ok(ch)
    }

    /// Drop what no longer applies: grants for old keys or non-members,
    /// records whose author was removed, the blob when sync is off.
    fn prune(&mut self) -> bool {
        let state = self.log.state().clone();
        let before = (
            self.grants.len(),
            self.blob.is_some(),
            self.credential_sync.is_some(),
            self.coordinator.is_some(),
            self.last_synced.len(),
        );
        let key = state.key_id.clone();
        let coordinator_holds = self.credential_sync_on();
        self.grants.retain(|g| {
            Some(&g.key_id) == key.as_ref()
                && (state.is_member(&g.to) || (coordinator_holds && g.to == COORDINATOR_ID))
        });
        let mut seen = std::collections::BTreeSet::new();
        self.grants.retain(|g| seen.insert(g.to.clone()));
        if self
            .credential_sync
            .as_ref()
            .is_some_and(|s| !state.is_member(&s.author))
        {
            self.credential_sync = None;
        }
        if self
            .coordinator
            .as_ref()
            .is_some_and(|s| !state.is_member(&s.author))
        {
            self.coordinator = None;
        }
        if !self.credential_sync_on() {
            self.blob = None;
        }
        self.last_synced
            .retain(|d, _| state.is_member(d) || state.removed.contains(d));
        before
            != (
                self.grants.len(),
                self.blob.is_some(),
                self.credential_sync.is_some(),
                self.coordinator.is_some(),
                self.last_synced.len(),
            )
    }

    /// Note that `device` was in touch now (coarse: at most once a minute
    /// counts as a change).
    pub fn touch(&mut self, device: &str, now: EpochMs) -> bool {
        if !self.log.state().is_member(device) {
            return false;
        }
        let e = self.last_synced.entry(device.into()).or_insert(0.0);
        if now - *e > 60_000.0 {
            *e = now;
            true
        } else {
            false
        }
    }

    /// The group key, if `holder` can open a grant for it.
    pub fn open_key(&self, holder: &Holder, key_id: &str) -> Option<[u8; 32]> {
        let g = self
            .grants
            .iter()
            .find(|g| g.key_id == key_id && g.to == holder.id)?;
        self.open_grant(holder, g)
    }

    /// The key in `g`, if it is `holder`'s and opens to its key id.
    fn open_grant(&self, holder: &Holder, g: &Grant) -> Option<[u8; 32]> {
        let key_id = g.key_id.as_str();
        let key: [u8; 32] = holder
            .secret
            .open_envelope(&grant_context(self.identity(), key_id, holder.id), &g.env)?
            .try_into()
            .ok()?;
        (self::key_id(&key) == key_id).then_some(key)
    }

    /// Seal `key` for `to` (whose X25519 key is `kx`).
    fn grant(&self, key_id: &str, key: &[u8; 32], to: &str, kx: &str) -> Option<Grant> {
        Some(Grant {
            key_id: key_id.into(),
            to: to.into(),
            env: crypto::envelope(kx, &grant_context(self.identity(), key_id, to), key)?,
        })
    }

    /// What a key holder owes everyone else: grants of the current key for
    /// members (and the coordinator while credential sync is on) that have
    /// none, and the credential blob moved to the current key. Returns
    /// whether anything changed.
    pub fn maintain(&mut self, keys: &BTreeMap<String, [u8; 32]>) -> bool {
        let state = self.log.state().clone();
        let Some(kid) = state.key_id.clone() else {
            return false;
        };
        let Some(key) = keys.get(&kid) else {
            return false;
        };
        let mut changed = false;
        let mut targets: Vec<(String, String)> = state
            .members
            .values()
            .map(|d| (d.id.clone(), d.keys.kx.clone()))
            .collect();
        if self.credential_sync_on() {
            if let Some(pin) = self.coordinator_pin() {
                targets.push((COORDINATOR_ID.into(), pin.keys.kx.clone()));
            }
        }
        for (to, kx) in targets {
            if !self.grants.iter().any(|g| g.key_id == kid && g.to == to) {
                if let Some(g) = self.grant(&kid, key, &to, &kx) {
                    self.grants.push(g);
                    changed = true;
                }
            }
        }
        // Re-seal the blob under the current key.
        if let Some(blob) = self.blob.clone() {
            if blob.key_id != kid {
                if let Some(plain) = keys
                    .get(&blob.key_id)
                    .and_then(|old| open_blob(self.identity(), old, &blob))
                {
                    self.blob = Some(seal_blob(
                        self.identity(),
                        &kid,
                        key,
                        &blob.author,
                        blob.at,
                        &plain,
                    ));
                    changed = true;
                }
            }
        }
        changed
    }

    /// The synced credentials, when `keys` holds the blob's key.
    pub fn secrets(&self, keys: &BTreeMap<String, [u8; 32]>) -> Option<SyncedSecrets> {
        let blob = self.blob.as_ref()?;
        let key = keys.get(&blob.key_id)?;
        open_blob(self.identity(), key, blob)
    }
}

/// Current-key blobs first, then newer.
fn blob_rank<'a>(b: &'a CredentialBlob, state: &MemberState) -> (bool, EpochMs, &'a str) {
    (
        state.key_id.as_deref() == Some(b.key_id.as_str()),
        b.at,
        &b.sealed.ct,
    )
}

fn seal_blob(
    identity: &str,
    key_id: &str,
    key: &[u8; 32],
    author: &str,
    at: EpochMs,
    plain: &SyncedSecrets,
) -> CredentialBlob {
    let json = serde_json::to_vec(plain).unwrap_or_default();
    CredentialBlob {
        key_id: key_id.into(),
        author: author.into(),
        at,
        sealed: crypto::seal(key, &blob_aad(identity, key_id, author, at), &json),
    }
}

fn open_blob(identity: &str, key: &[u8; 32], b: &CredentialBlob) -> Option<SyncedSecrets> {
    let plain = crypto::open(
        key,
        &blob_aad(identity, &b.key_id, &b.author, b.at),
        &b.sealed,
    )?;
    serde_json::from_slice(&plain).ok()
}

/// An invite this device made and is waiting on (never synced).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingInvite {
    pub invite_id: String,
    pub secret: String,
    pub expires_at: EpochMs,
}

/// Everything this device keeps about its identity.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Identity {
    pub id: String,
    pub device_id: DeviceId,
    secret: DeviceSecret,
    pub shared: SharedState,
    /// Group keys this device holds, by id (hex).
    #[serde(default)]
    keys: BTreeMap<String, String>,
    #[serde(default)]
    pub invites: Vec<PendingInvite>,
}

/// How a handshake proof is bound: who proves, to whom, over what.
pub fn proof_bytes(
    role: &str,
    identity: &str,
    nonce: &str,
    device_id: &str,
    head_digest: &str,
) -> Vec<u8> {
    let mut c = Canon::new("hocket-room-proof-v1");
    c.str(role)
        .str(identity)
        .str(nonce)
        .str(device_id)
        .str(head_digest);
    c.finish().to_vec()
}

impl Identity {
    /// A brand-new identity with this device as its only member.
    pub fn create(device_id: &str, device_name: &str, now: EpochMs) -> Identity {
        Identity::create_with(
            &new_identity_id(),
            device_id,
            device_name,
            DeviceSecret::generate(),
            now,
        )
    }

    /// [`Identity::create`] with a given id and keys (tests, simulation).
    pub fn create_with(
        id: &str,
        device_id: &str,
        device_name: &str,
        secret: DeviceSecret,
        now: EpochMs,
    ) -> Identity {
        let key = crypto::random32();
        let kid = key_id(&key);
        let me = MemberDevice {
            id: device_id.into(),
            name: truncate(device_name),
            keys: secret.public(),
        };
        let log = MemberLog::genesis(id, me, kid.clone(), now, &secret);
        let mut keys = BTreeMap::new();
        keys.insert(kid, hex::encode(key));
        let mut i = Identity {
            id: id.into(),
            device_id: device_id.into(),
            secret,
            shared: SharedState::new(log),
            keys,
            invites: vec![],
        };
        i.shared.last_synced.insert(device_id.into(), now);
        i.maintain();
        i
    }

    /// A device that has keys but no identity yet (pairing): its own keys
    /// joined to the identity a [`pairing::PairAccept`] hands it.
    pub(crate) fn joined(
        device_id: &str,
        secret: DeviceSecret,
        shared: SharedState,
    ) -> Option<Identity> {
        let mut i = Identity {
            id: shared.identity().to_string(),
            device_id: device_id.into(),
            secret,
            shared,
            keys: BTreeMap::new(),
            invites: vec![],
        };
        i.after_merge();
        i.is_member().then_some(i)
    }

    pub fn secret(&self) -> &DeviceSecret {
        &self.secret
    }

    pub fn public_keys(&self) -> crypto::PublicKeys {
        self.secret.public()
    }

    pub fn holder(&self) -> Holder<'_> {
        Holder {
            id: &self.device_id,
            secret: &self.secret,
        }
    }

    fn key_map(&self) -> BTreeMap<String, [u8; 32]> {
        self.keys
            .iter()
            .filter_map(|(k, v)| Some((k.clone(), crypto::decode32(v)?)))
            .collect()
    }

    pub fn state(&mut self) -> &MemberState {
        self.shared.state()
    }

    pub fn member_state(&self) -> MemberState {
        self.shared.log.compute_state()
    }

    pub fn is_member(&mut self) -> bool {
        let me = self.device_id.clone();
        self.state().is_member(&me)
    }

    /// Whether this device was removed from its identity.
    pub fn was_removed(&mut self) -> bool {
        let me = self.device_id.clone();
        self.state().removed.contains(&me)
    }

    /// The current group key, if this device holds it.
    pub fn group_key(&mut self) -> Option<[u8; 32]> {
        let kid = self.state().key_id.clone()?;
        self.key_map().get(&kid).copied()
    }

    /// Whether this device holds the current group key.
    pub fn holds_current_key(&mut self) -> bool {
        self.group_key().is_some()
    }

    /// Union in a peer's state, then take any key granted to us and do what
    /// a key holder owes the others.
    pub fn merge(&mut self, mut sync: IdentitySync, now: EpochMs) -> Result<Changed, LogError> {
        // What we can check, we do: a blob under a key we hold must open,
        // and a grant to us must open to its key. Ours too, so a bad one
        // that got in before can be replaced.
        let keys = self.key_map();
        let opens = |b: &CredentialBlob| {
            keys.get(&b.key_id)
                .is_none_or(|k| open_blob(&self.id, k, b).is_some())
        };
        if sync.blob.as_ref().is_some_and(|b| !opens(b)) {
            sync.blob = None;
        }
        let bad_ours = self.shared.blob.as_ref().is_some_and(|b| !opens(b));
        let me = self.device_id.clone();
        let grant_ok =
            |g: &Grant| g.to != me || self.shared.open_grant(&self.holder(), g).is_some();
        sync.grants.retain(|g| grant_ok(g));
        let mine_ok: Vec<bool> = self.shared.grants.iter().map(grant_ok).collect();
        let mut i = 0;
        self.shared.grants.retain(|_| {
            i += 1;
            mine_ok[i - 1]
        });
        if bad_ours {
            self.shared.blob = None;
        }
        let mut ch = self.shared.merge(sync, now)?;
        if self.after_merge() {
            ch.other = true;
        }
        // A contested removal left a key the removed devices hold: move
        // everyone left to a new one.
        if self.state().rekey && self.is_member() {
            let at = self
                .shared
                .log
                .ops()
                .iter()
                .map(|o| o.at)
                .fold(0.0, f64::max);
            if self.rotate(at + 1.0).is_ok() {
                ch.members = true;
            }
        }
        Ok(ch)
    }

    fn after_merge(&mut self) -> bool {
        let mut changed = false;
        if let Some(kid) = self.shared.state().key_id.clone() {
            if !self.keys.contains_key(&kid) {
                if let Some(k) = self.shared.open_key(&self.holder(), &kid) {
                    self.keys.insert(kid.clone(), hex::encode(k));
                    changed = true;
                }
            }
            // Keep the current key and the one before it (a blob may still be sealed with it).
            if self.keys.len() > 2 {
                let blob_key = self.shared.blob.as_ref().map(|b| b.key_id.clone());
                self.keys
                    .retain(|k, _| *k == kid || Some(k) == blob_key.as_ref());
                changed = true;
            }
        }
        if self.maintain() {
            changed = true;
        }
        changed
    }

    fn maintain(&mut self) -> bool {
        if !self.is_member() {
            return false;
        }
        let keys = self.key_map();
        self.shared.maintain(&keys)
    }

    /// The whole shared state (what a peer that told us nothing gets).
    pub fn sync(&self) -> IdentitySync {
        self.shared.clone()
    }

    pub fn heads(&self) -> Vec<String> {
        self.shared.log.heads()
    }

    /// Sign a handshake proof over the other side's nonce.
    pub fn prove(&self, role: &str, nonce: &str) -> String {
        self.secret.sign(&proof_bytes(
            role,
            &self.id,
            nonce,
            &self.device_id,
            &self.shared.log.head_digest(),
        ))
    }

    /// Note that `device` was in touch now.
    pub fn touch(&mut self, device: &str, now: EpochMs) -> bool {
        self.shared.touch(device, now)
    }

    fn new_key(&mut self) -> String {
        let key = crypto::random32();
        let kid = key_id(&key);
        self.keys.insert(kid.clone(), hex::encode(key));
        kid
    }

    /// Admit a device (pairing). Returns the op and the device's grant.
    pub fn add_device(
        &mut self,
        device: MemberDevice,
        now: EpochMs,
    ) -> Result<(MemberOp, Option<Grant>), String> {
        if !self.is_member() {
            return Err("this device is no longer part of the identity".into());
        }
        if device.id == COORDINATOR_ID {
            return Err("that device id is reserved".into());
        }
        let state = self.state().clone();
        if state.removed.contains(&device.id) {
            return Err("that device was removed; it needs to be reset first".into());
        }
        if let Some(existing) = state.members.get(&device.id) {
            if existing.keys != device.keys {
                return Err("a device with that id is already a member".into());
            }
        }
        let mut device = device;
        device.name = truncate(&device.name);
        let (to, kx) = (device.id.clone(), device.keys.kx.clone());
        let op = self.shared.log.append(
            &self.device_id.clone(),
            now,
            OpBody::Add {
                device,
                key_id: None,
            },
            &self.secret.clone(),
        );
        self.maintain();
        let kid = self.state().key_id.clone().unwrap_or_default();
        let grant = self
            .shared
            .grants
            .iter()
            .find(|g| g.to == to && g.key_id == kid)
            .cloned()
            .or_else(|| {
                let key = self.group_key()?;
                self.shared.grant(&kid, &key, &to, &kx)
            });
        Ok((op, grant))
    }

    /// Remove a device (possibly this one) and move everyone left to a new
    /// group key.
    pub fn remove_device(&mut self, device_id: &str, now: EpochMs) -> Result<MemberOp, String> {
        if !self.is_member() {
            return Err("this device is no longer part of the identity".into());
        }
        if !self.state().is_member(device_id) {
            return Err("not a member".into());
        }
        // Records the removed device signed stand: the remover vouches for
        // them, so pruning the removed author doesn't drop them.
        let me = self.device_id.clone();
        if let Some(r) = self.shared.coordinator.clone() {
            if r.author == device_id {
                self.shared.coordinator = Some(Signed::new(
                    &self.id,
                    "coordinator",
                    r.value,
                    &me,
                    now.max(r.at),
                    &self.secret,
                ));
            }
        }
        if let Some(r) = self.shared.credential_sync.clone() {
            if r.author == device_id {
                self.shared.credential_sync = Some(Signed::new(
                    &self.id,
                    "credentialSync",
                    r.value,
                    &me,
                    now.max(r.at),
                    &self.secret,
                ));
            }
        }
        let kid = self.new_key();
        let op = self.shared.log.append(
            &self.device_id.clone(),
            now,
            OpBody::Remove {
                device_id: device_id.into(),
                key_id: kid,
            },
            &self.secret.clone(),
        );
        self.shared.last_synced.remove(device_id);
        self.after_merge();
        self.shared.prune();
        Ok(op)
    }

    /// A new group key for everyone, without a membership change.
    pub fn rotate(&mut self, now: EpochMs) -> Result<MemberOp, String> {
        if !self.is_member() {
            return Err("this device is no longer part of the identity".into());
        }
        let kid = self.new_key();
        let op = self.shared.log.append(
            &self.device_id.clone(),
            now,
            OpBody::Rotate { key_id: kid },
            &self.secret.clone(),
        );
        self.after_merge();
        self.shared.prune();
        Ok(op)
    }

    /// Turn credential sync on (with these secrets) or off. Off rotates the
    /// group key, so the coordinator's copy (and the old blob) are dead.
    pub fn set_credential_sync(
        &mut self,
        on: bool,
        secrets: Option<&SyncedSecrets>,
        now: EpochMs,
    ) -> Result<(), String> {
        if !self.is_member() {
            return Err("this device is no longer part of the identity".into());
        }
        let was_on = self.shared.credential_sync_on();
        self.shared.credential_sync = Some(Signed::new(
            &self.id,
            "credentialSync",
            on,
            &self.device_id,
            now,
            &self.secret,
        ));
        if on {
            if let Some(s) = secrets {
                self.put_secrets(s, now);
            }
        } else {
            self.shared.blob = None;
            if was_on {
                self.rotate(now)?;
            }
        }
        self.shared.prune();
        self.maintain();
        Ok(())
    }

    /// Replace the synced credentials (credential sync on).
    pub fn put_secrets(&mut self, secrets: &SyncedSecrets, now: EpochMs) -> bool {
        if !self.shared.credential_sync_on() {
            return false;
        }
        let Some(key) = self.group_key() else {
            return false;
        };
        let kid = key_id(&key);
        if self.secrets().as_ref() == Some(secrets) {
            return false;
        }
        self.shared.blob = Some(seal_blob(
            &self.id,
            &kid,
            &key,
            &self.device_id,
            now,
            secrets,
        ));
        true
    }

    /// The synced credentials, if sync is on and this device can open them.
    pub fn secrets(&self) -> Option<SyncedSecrets> {
        if !self.shared.credential_sync_on() {
            return None;
        }
        self.shared.secrets(&self.key_map())
    }

    /// Record the coordinator this identity uses (`None` forgets it).
    pub fn set_coordinator(&mut self, pin: Option<CoordinatorPin>, now: EpochMs) {
        self.shared.coordinator = Some(Signed::new(
            &self.id,
            "coordinator",
            pin,
            &self.device_id,
            now,
            &self.secret,
        ));
        self.maintain();
    }

    /// Start from scratch: a new identity for this device (after it was
    /// removed, or to leave).
    pub fn reset(&mut self, device_name: &str, now: EpochMs) {
        *self = Identity::create(&self.device_id.clone(), device_name, now);
    }

    /// Drop expired invites.
    pub fn expire_invites(&mut self, now: EpochMs) -> bool {
        let n = self.invites.len();
        self.invites.retain(|i| i.expires_at > now);
        n != self.invites.len()
    }
}

fn truncate(name: &str) -> String {
    name.chars().take(60).collect()
}

/// Helpers for tests and the simulation: deterministic devices and an
/// in-process pairing.
#[cfg(any(test, feature = "sim"))]
pub mod testing {
    use super::*;

    pub fn device(id: &str) -> (MemberDevice, DeviceSecret) {
        let s = DeviceSecret::from_seed(id.as_bytes());
        (
            MemberDevice {
                id: id.into(),
                name: id.into(),
                keys: s.public(),
            },
            s,
        )
    }

    /// Pair `id` into `inviter`'s identity the way pairing does.
    pub fn pair(inviter: &mut Identity, id: &str, now: EpochMs) -> Identity {
        let (d, s) = device(id);
        let (_op, grant) = inviter.add_device(d, now).unwrap();
        let mut shared = inviter.sync();
        if let Some(g) = grant {
            if !shared.grants.contains(&g) {
                shared.grants.push(g);
            }
        }
        Identity::joined(id, s, shared).unwrap()
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::testing::*;
    use super::*;

    const T: EpochMs = 1_000_000.0;

    fn exchange(a: &mut Identity, b: &mut Identity) {
        for _ in 0..2 {
            a.merge(b.sync(), T).unwrap();
            b.merge(a.sync(), T).unwrap();
        }
    }

    fn secrets() -> SyncedSecrets {
        SyncedSecrets {
            entries: vec![SyncedSecret {
                account_id: "acc".into(),
                kind: SecretKind::Navidrome,
                url: "https://music.example".into(),
                username: "u".into(),
                secret: "pw".into(),
                name: None,
            }],
        }
    }

    #[test]
    fn pairing_hands_over_the_group_key() {
        let mut a = Identity::create("a", "A", 1.0);
        let mut b = pair(&mut a, "b", 2.0);
        assert!(b.holds_current_key());
        assert_eq!(b.group_key(), a.group_key());
        assert_eq!(b.id, a.id);
    }

    #[test]
    fn removal_rotates_and_shuts_the_removed_device_out() {
        let mut a = Identity::create("a", "A", 1.0);
        let mut b = pair(&mut a, "b", 2.0);
        let mut c = pair(&mut a, "c", 3.0);
        exchange(&mut a, &mut b);
        exchange(&mut a, &mut c);
        let old = a.group_key().unwrap();
        // b removes the first device, a.
        b.remove_device("a", 4.0).unwrap();
        exchange(&mut b, &mut c);
        let new = b.group_key().unwrap();
        assert_ne!(old, new);
        assert_eq!(c.group_key(), Some(new));
        // a learns it was removed and never gets the new key.
        a.merge(c.sync(), T).unwrap();
        assert!(a.was_removed());
        assert!(!a.holds_current_key());
        assert!(!c.sync().grants.iter().any(|g| g.to == "a"));
    }

    /// A blob from the future never wins, and a key holder keeps only a
    /// blob that opens and a grant to it that opens.
    #[test]
    fn unopenable_or_future_blobs_and_grants_are_dropped() {
        let mut a = Identity::create("a", "A", 1.0);
        let mut b = pair(&mut a, "b", 2.0);
        a.set_coordinator(
            Some(CoordinatorPin {
                url: "wss://c".into(),
                keys: DeviceSecret::from_seed(b"coord").public(),
                owner: true,
            }),
            3.0,
        );
        a.set_credential_sync(true, Some(&secrets()), 4.0).unwrap();
        exchange(&mut a, &mut b);
        let kid = a.state().key_id.clone().unwrap();
        // Garbage under the current key, far in the future.
        let mut bad = a.sync();
        bad.blob = Some(CredentialBlob {
            key_id: kid.clone(),
            author: "a".into(),
            at: 1e300,
            sealed: crypto::seal(&[7; 32], b"", b"junk"),
        });
        bad.last_synced.insert("a".into(), 1e300);
        b.merge(bad.clone(), T).unwrap();
        assert_eq!(b.secrets(), Some(secrets()));
        assert!(b.shared.last_synced["a"] < 1e300);
        // Without the key (a coordinator), the time cap still keeps it out.
        let mut shared = a.shared.clone();
        shared.merge(bad, T).unwrap();
        assert!(shared.blob.as_ref().unwrap().at < 1e300);
        // A garbage grant to b is refused, and b keeps its key.
        let mut bad = a.sync();
        let other = DeviceSecret::from_seed(b"other").public();
        bad.grants = vec![Grant {
            key_id: kid.clone(),
            to: "b".into(),
            env: crypto::envelope(&other.kx, b"x", &[1; 32]).unwrap(),
        }];
        let mut fresh = b.clone();
        fresh.shared.grants.retain(|g| g.to != "b");
        fresh.merge(bad, T).unwrap();
        assert!(fresh
            .shared
            .grants
            .iter()
            .all(|g| g.to != "b" || fresh.shared.open_key(&fresh.holder(), &kid).is_some()));
    }

    #[test]
    fn the_coordinator_id_cannot_join() {
        let mut a = Identity::create("a", "A", 1.0);
        let (d, _) = device(COORDINATOR_ID);
        assert!(a.add_device(d, 2.0).is_err());
        assert!(!a.state().is_member(COORDINATOR_ID));
    }

    /// Two devices remove each other: both are out, and the device left
    /// moves to a key neither of them holds.
    #[test]
    fn contested_removal_rotates_away_from_both() {
        let mut a = Identity::create("a", "A", 1.0);
        let mut b = pair(&mut a, "b", 2.0);
        let mut c = pair(&mut a, "c", 3.0);
        exchange(&mut a, &mut b);
        exchange(&mut a, &mut c);
        a.remove_device("b", 4.0).unwrap();
        b.remove_device("a", 4.0).unwrap();
        c.merge(a.sync(), T).unwrap();
        c.merge(b.sync(), T).unwrap();
        assert!(c.is_member());
        assert!(!c.state().is_member("a"));
        assert!(!c.state().is_member("b"));
        assert!(!c.state().rekey);
        let key = c.group_key().unwrap();
        assert_ne!(a.group_key(), Some(key));
        assert_ne!(b.group_key(), Some(key));
        assert!(!c.sync().grants.iter().any(|g| g.to == "a" || g.to == "b"));
    }

    #[test]
    fn credential_sync_on_and_off() {
        let mut a = Identity::create("a", "A", 1.0);
        let mut b = pair(&mut a, "b", 2.0);
        // Off by default: nothing to read.
        assert!(!a.shared.credential_sync_on());
        assert!(!a.put_secrets(&secrets(), 3.0));
        a.set_coordinator(
            Some(CoordinatorPin {
                url: "wss://c".into(),
                keys: DeviceSecret::from_seed(b"coord").public(),
                owner: true,
            }),
            3.0,
        );
        a.set_credential_sync(true, Some(&secrets()), 4.0).unwrap();
        exchange(&mut a, &mut b);
        assert_eq!(b.secrets(), Some(secrets()));
        // The coordinator holds the key while it's on.
        let coord = DeviceSecret::from_seed(b"coord");
        let holder = Holder {
            id: COORDINATOR_ID,
            secret: &coord,
        };
        let kid = a.state().key_id.clone().unwrap();
        assert!(a.shared.open_key(&holder, &kid).is_some());
        // Removal with sync on: the blob moves to the new key.
        let mut c = pair(&mut a, "c", 5.0);
        exchange(&mut a, &mut c);
        a.remove_device("b", 6.0).unwrap();
        exchange(&mut a, &mut c);
        assert_eq!(c.secrets(), Some(secrets()));
        let current = c.state().key_id.clone().unwrap();
        assert_eq!(c.shared.blob.as_ref().unwrap().key_id, current);
        // b, removed, can't read the re-sealed blob.
        b.merge(a.sync(), T).unwrap();
        assert!(b.secrets().is_none());
        // Off: rotates, the blob and the coordinator's grant are gone.
        let before = c.state().key_id.clone();
        c.set_credential_sync(false, None, 7.0).unwrap();
        assert_ne!(c.state().key_id, before);
        assert!(c.shared.blob.is_none());
        assert!(!c.shared.grants.iter().any(|g| g.to == COORDINATOR_ID));
        exchange(&mut a, &mut c);
        assert!(a.secrets().is_none());
        assert_eq!(a.group_key(), c.group_key());
        let kid = a.state().key_id.clone().unwrap();
        assert!(a.shared.open_key(&holder, &kid).is_none());
    }

    #[test]
    fn key_rotation_with_sync_off() {
        let mut a = Identity::create("a", "A", 1.0);
        let mut b = pair(&mut a, "b", 2.0);
        let mut c = pair(&mut a, "c", 3.0);
        exchange(&mut a, &mut b);
        exchange(&mut a, &mut c);
        a.remove_device("c", 4.0).unwrap();
        exchange(&mut a, &mut b);
        assert_eq!(a.group_key(), b.group_key());
        assert!(a.shared.blob.is_none());
        c.merge(a.sync(), T).unwrap();
        assert!(!c.holds_current_key());
    }

    #[test]
    fn records_from_non_members_are_ignored() {
        let mut a = Identity::create("a", "A", 1.0);
        let mut b = pair(&mut a, "b", 2.0);
        exchange(&mut a, &mut b);
        a.remove_device("b", 3.0).unwrap();
        exchange(&mut a, &mut b);
        // b, removed, tries to point everyone at its own coordinator.
        b.shared.coordinator = Some(Signed::new(
            &b.id,
            "coordinator",
            Some(CoordinatorPin {
                url: "wss://evil".into(),
                keys: DeviceSecret::from_seed(b"evil").public(),
                owner: false,
            }),
            "b",
            10.0,
            b.secret(),
        ));
        a.merge(b.sync(), T).unwrap();
        assert!(a.shared.coordinator_pin().is_none());
    }

    #[test]
    fn identity_round_trips_through_json() {
        let a = Identity::create("a", "A", 1.0);
        let json = serde_json::to_string(&a).unwrap();
        let mut back: Identity = serde_json::from_str(&json).unwrap();
        assert_eq!(back.id, a.id);
        assert!(back.holds_current_key());
    }
}
