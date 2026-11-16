//! Who may use this coordinator: Hocket identities, admitted by claiming it
//! (the owner) or by an invite its owner made (`docs/providers.md`, Phase 2
//! "Coordinator").
//!
//! - **Its key.** The coordinator signs its side of every handshake with
//!   its own ed25519 key (`<data-dir>/coordinator-key.json`); devices pin
//!   it when they claim the coordinator or redeem an invite, and refuse any
//!   room that can't prove it holds it.
//! - **Claiming.** `hocket-coordinator pair` writes a short-lived code to
//!   `<data-dir>/claim.json` (so it works with `docker exec` against a
//!   running server) and prints it with a QR code. The first device to
//!   present it before it expires makes its identity the owner; the code is
//!   then used up. Claiming again (a new `pair`) hands ownership to the new
//!   identity; the old one stays admitted.
//! - **Invites.** The owner's devices create invite codes (good for
//!   [`INVITE_TTL_MS`], one use each) and can revoke them. Redeeming one
//!   admits the redeeming identity.
//! - **The identities' shared state** (member list, key grants, the
//!   credential blob while credential sync is on) is kept per identity in
//!   `<data-dir>/identities.json`, merged from what members sync, so a
//!   removed device is turned away even when no other device is online.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

use hocket_core::connect::wire::CoordInvite;
use hocket_core::identity::crypto::{DeviceSecret, PublicKeys};
use hocket_core::identity::log::MemberLog;
use hocket_core::identity::{is_identity_id, IdentitySync, SharedState};

/// The coordinator's key file in the data dir.
pub const KEY_FILE: &str = "coordinator-key.json";
/// The pending claim code in the data dir.
pub const CLAIM_FILE: &str = "claim.json";
/// The admitted identities in the data dir.
pub const IDENTITIES_FILE: &str = "identities.json";
/// How long a claim code is good for by default.
pub const CLAIM_TTL_MS: u64 = 10 * 60_000;
/// How long an invite is good for.
pub const INVITE_TTL_MS: u64 = 7 * 24 * 3_600_000;
/// Open invites one owner may have at once.
pub const INVITES_MAX: usize = 32;
/// Identities one coordinator admits at most.
pub const IDENTITIES_MAX: usize = 256;

/// A pending claim (`claim.json`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Claim {
    pub code: String,
    pub expires_at: u64,
}

/// A short, typeable code: 8 characters of an unambiguous alphabet, as
/// `XXXX-XXXX`.
pub fn new_code() -> String {
    use rand::Rng;
    const ALPHABET: &[u8] = b"ABCDEFGHJKMNPQRSTUVWXYZ23456789";
    let mut rng = rand::rng();
    let c: String = (0..8)
        .map(|_| ALPHABET[rng.random_range(0..ALPHABET.len())] as char)
        .collect();
    format!("{}-{}", &c[..4], &c[4..])
}

/// Codes compare without case, dashes or spaces.
fn normalise(code: &str) -> String {
    code.chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_uppercase())
        .collect()
}

/// Write `bytes` to `path` atomically, readable by this user only: the
/// temporary file is created 0600 (never with the umask's permissions, not
/// even for a moment), and a failure to set them is an error.
pub(crate) fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    let tmp = path.with_extension("tmp");
    // A leftover could have other permissions; `create_new` won't reuse it.
    match std::fs::remove_file(&tmp) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e),
        _ => {}
    }
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let mut f = opts.open(&tmp)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        f.set_permissions(std::fs::Permissions::from_mode(0o600))?;
    }
    f.write_all(bytes)?;
    f.sync_all()?;
    drop(f);
    std::fs::rename(&tmp, path)
}

/// Write a new claim code to `dir` (what `hocket-coordinator pair` does).
pub fn write_claim(dir: &Path, now: u64, ttl_ms: u64) -> anyhow::Result<Claim> {
    std::fs::create_dir_all(dir)?;
    let claim = Claim {
        code: new_code(),
        expires_at: now + ttl_ms,
    };
    write_atomic(&dir.join(CLAIM_FILE), &serde_json::to_vec(&claim)?)?;
    Ok(claim)
}

/// The coordinator's key: loaded from `dir`, or made (and saved) now.
pub fn load_or_create_key(dir: Option<&Path>) -> anyhow::Result<DeviceSecret> {
    let Some(dir) = dir else {
        return Ok(DeviceSecret::generate());
    };
    let path = dir.join(KEY_FILE);
    match std::fs::read(&path) {
        Ok(bytes) => Ok(serde_json::from_slice(&bytes)?),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            std::fs::create_dir_all(dir)?;
            let key = DeviceSecret::generate();
            write_atomic(&path, &serde_json::to_vec(&key)?)?;
            Ok(key)
        }
        Err(e) => Err(e.into()),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Entry {
    shared: SharedState,
    admitted_at: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Registry {
    owner: Option<String>,
    identities: BTreeMap<String, Entry>,
    invites: Vec<CoordInvite>,
}

/// Why a claim or a redemption failed (the message goes to the device).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AdmitError {
    #[error("That code isn't valid for this coordinator (or it was already used).")]
    BadCode,
    #[error("That code has expired. Run `hocket-coordinator pair` again for a new one.")]
    Expired,
    #[error("That invite has expired; ask the coordinator's owner for a new one.")]
    InviteExpired,
    #[error("This device's identity couldn't be read.")]
    BadIdentity,
    #[error("This coordinator admits no more identities.")]
    Full,
}

/// The admitted identities, the owner and the invites.
pub struct Identities {
    dir: Option<PathBuf>,
    key: DeviceSecret,
    inner: Mutex<Registry>,
    /// A claim made in-process (tests, or no data dir).
    claim: Mutex<Option<Claim>>,
}

impl Identities {
    pub fn open(dir: Option<&Path>) -> anyhow::Result<Identities> {
        let key = load_or_create_key(dir)?;
        let inner = match dir {
            Some(d) => match std::fs::read(d.join(IDENTITIES_FILE)) {
                Ok(bytes) => serde_json::from_slice(&bytes)?,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Registry::default(),
                Err(e) => return Err(e.into()),
            },
            None => Registry::default(),
        };
        Ok(Identities {
            dir: dir.map(Path::to_path_buf),
            key,
            inner: Mutex::new(inner),
            claim: Mutex::new(None),
        })
    }

    pub fn keys(&self) -> PublicKeys {
        self.key.public()
    }

    pub fn secret(&self) -> &DeviceSecret {
        &self.key
    }

    fn save(&self, r: &Registry) {
        let Some(dir) = &self.dir else { return };
        let result = serde_json::to_vec(r)
            .map_err(std::io::Error::other)
            .and_then(|bytes| write_atomic(&dir.join(IDENTITIES_FILE), &bytes));
        if let Err(e) = result {
            tracing::warn!(error = %e, "could not save identities");
        }
    }

    /// Issue a claim code (the `pair` subcommand writes the file instead;
    /// this is for a coordinator without a data dir, and tests).
    pub fn issue_claim(&self, now: u64, ttl_ms: u64) -> anyhow::Result<Claim> {
        match &self.dir {
            Some(d) => write_claim(d, now, ttl_ms),
            None => {
                let c = Claim {
                    code: new_code(),
                    expires_at: now + ttl_ms,
                };
                *self.claim.lock() = Some(c.clone());
                Ok(c)
            }
        }
    }

    fn pending_claim(&self) -> Option<Claim> {
        if let Some(c) = self.claim.lock().clone() {
            return Some(c);
        }
        let dir = self.dir.as_ref()?;
        let bytes = std::fs::read(dir.join(CLAIM_FILE)).ok()?;
        serde_json::from_slice(&bytes).ok()
    }

    fn use_claim(&self) {
        *self.claim.lock() = None;
        if let Some(d) = &self.dir {
            let _ = std::fs::remove_file(d.join(CLAIM_FILE));
        }
    }

    pub fn is_admitted(&self, identity: &str) -> bool {
        self.inner.lock().identities.contains_key(identity)
    }

    pub fn is_owner(&self, identity: &str) -> bool {
        self.inner.lock().owner.as_deref() == Some(identity)
    }

    pub fn owner(&self) -> Option<String> {
        self.inner.lock().owner.clone()
    }

    pub fn count(&self) -> usize {
        self.inner.lock().identities.len()
    }

    /// The identity's shared state as this coordinator knows it.
    pub fn shared(&self, identity: &str) -> Option<SharedState> {
        self.inner
            .lock()
            .identities
            .get(identity)
            .map(|e| e.shared.clone())
    }

    /// A room's newer view of an identity's state.
    /// Merged into what is kept, never replacing it: two drives of one
    /// room may hand in their snapshots in either order, and an older one
    /// must not undo a newer one.
    pub fn update(&self, identity: &str, shared: SharedState, now: u64) {
        let mut r = self.inner.lock();
        let Some(e) = r.identities.get_mut(identity) else {
            return;
        };
        if e.shared == shared {
            return;
        }
        match e.shared.merge(shared, now as f64) {
            Ok(ch) if ch.any() => self.save(&r),
            Ok(_) => {}
            Err(err) => tracing::warn!(identity, error = %err, "room identity not kept"),
        }
    }

    /// Admit (or refresh) an identity from what its device sent.
    fn admit(r: &mut Registry, sync: IdentitySync, now: u64) -> Result<String, AdmitError> {
        let id = sync.identity().to_string();
        if !is_identity_id(&id) {
            return Err(AdmitError::BadIdentity);
        }
        match r.identities.get(&id) {
            // Known already: nothing it sent is taken. Nothing in it is
            // proved (a code proves an invite, not membership); its device
            // syncs after its handshake proof, like any other.
            Some(_) => {}
            None => {
                // New: only what signatures prove (the member list and the
                // signed records). The blob, grants and sync times come
                // after a proof.
                let mut sync = sync;
                sync.blob = None;
                sync.grants.clear();
                sync.last_synced.clear();
                if r.identities.len() >= IDENTITIES_MAX {
                    return Err(AdmitError::Full);
                }
                let mut shared = SharedState::new(MemberLog::empty(&id));
                shared
                    .merge(sync, now as f64)
                    .map_err(|_| AdmitError::BadIdentity)?;
                if shared.state().members.is_empty() {
                    return Err(AdmitError::BadIdentity);
                }
                r.identities.insert(
                    id.clone(),
                    Entry {
                        shared,
                        admitted_at: now,
                    },
                );
            }
        }
        Ok(id)
    }

    /// The `pair` code: the identity becomes the owner.
    pub fn claim(&self, code: &str, sync: IdentitySync, now: u64) -> Result<(), AdmitError> {
        let Some(claim) = self.pending_claim() else {
            return Err(AdmitError::BadCode);
        };
        if normalise(&claim.code) != normalise(code) {
            return Err(AdmitError::BadCode);
        }
        if claim.expires_at <= now {
            return Err(AdmitError::Expired);
        }
        let mut r = self.inner.lock();
        let id = Self::admit(&mut r, sync, now)?;
        tracing::info!(identity = %id, "coordinator claimed");
        r.owner = Some(id);
        self.use_claim();
        self.save(&r);
        Ok(())
    }

    /// An owner's invite: the identity is admitted.
    pub fn redeem(&self, code: &str, sync: IdentitySync, now: u64) -> Result<(), AdmitError> {
        let mut r = self.inner.lock();
        let wanted = normalise(code);
        let Some(idx) = r
            .invites
            .iter()
            .position(|i| normalise(&i.code) == wanted && i.redeemed_by.is_none())
        else {
            return Err(AdmitError::BadCode);
        };
        if r.invites[idx].expires_at <= now as f64 {
            return Err(AdmitError::InviteExpired);
        }
        let id = Self::admit(&mut r, sync, now)?;
        tracing::info!(identity = %id, "invite redeemed");
        r.invites[idx].redeemed_by = Some(id);
        self.save(&r);
        Ok(())
    }

    /// A new invite (the owner only). Returns the code.
    pub fn create_invite(&self, by: &str, now: u64) -> Option<String> {
        let mut r = self.inner.lock();
        if r.owner.as_deref() != Some(by) {
            return None;
        }
        r.invites
            .retain(|i| i.redeemed_by.is_some() || i.expires_at > now as f64);
        if r.invites.iter().filter(|i| i.redeemed_by.is_none()).count() >= INVITES_MAX {
            return None;
        }
        let code = new_code();
        r.invites.push(CoordInvite {
            code: code.clone(),
            created_at: now as f64,
            expires_at: (now + INVITE_TTL_MS) as f64,
            redeemed_by: None,
        });
        self.save(&r);
        Some(code)
    }

    /// Revoke an open invite (the owner only).
    pub fn revoke_invite(&self, by: &str, code: &str) -> bool {
        let mut r = self.inner.lock();
        if r.owner.as_deref() != Some(by) {
            return false;
        }
        let wanted = normalise(code);
        let before = r.invites.len();
        r.invites
            .retain(|i| !(normalise(&i.code) == wanted && i.redeemed_by.is_none()));
        let changed = r.invites.len() != before;
        if changed {
            self.save(&r);
        }
        changed
    }

    /// The owner's view of its invites.
    pub fn invites(&self, by: &str, now: u64) -> Option<Vec<CoordInvite>> {
        let r = self.inner.lock();
        if r.owner.as_deref() != Some(by) {
            return None;
        }
        Some(
            r.invites
                .iter()
                .filter(|i| i.redeemed_by.is_some() || i.expires_at > now as f64)
                .cloned()
                .collect(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hocket_core::identity::Identity;

    fn device() -> Identity {
        Identity::create("dev", "Phone", 1.0)
    }

    /// The key file is 0600 even when a world-readable leftover temp file
    /// sits where the new one goes, and whatever the umask.
    #[cfg(unix)]
    #[test]
    fn private_files_are_never_readable_by_others() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("coordinator-key.json");
        let tmp = path.with_extension("tmp");
        std::fs::write(&tmp, b"old").unwrap();
        std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o666)).unwrap();
        write_atomic(&path, b"secret").unwrap();
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        assert_eq!(std::fs::read(&path).unwrap(), b"secret");
        assert!(!tmp.exists());
    }

    #[test]
    fn codes_are_typeable() {
        let c = new_code();
        assert_eq!(c.len(), 9);
        assert_eq!(
            normalise(&c.to_lowercase().replace('-', " ")),
            normalise(&c)
        );
    }

    #[test]
    fn claim_with_a_valid_code_and_not_an_expired_one() {
        let ids = Identities::open(None).unwrap();
        let a = device();
        assert_eq!(ids.claim("NOPE", a.sync(), 0), Err(AdmitError::BadCode));
        let c = ids.issue_claim(1_000, 60_000).unwrap();
        assert_eq!(
            ids.claim(&c.code, a.sync(), 61_001),
            Err(AdmitError::Expired)
        );
        assert!(!ids.is_admitted(&a.id));
        let c = ids.issue_claim(1_000, 60_000).unwrap();
        ids.claim(&c.code.to_lowercase(), a.sync(), 2_000).unwrap();
        assert!(ids.is_owner(&a.id));
        // Used up.
        let b = device();
        assert_eq!(
            ids.claim(&c.code, b.sync(), 2_000),
            Err(AdmitError::BadCode)
        );
    }

    #[test]
    fn invites_admit_identities_and_can_be_revoked() {
        let ids = Identities::open(None).unwrap();
        let owner = device();
        let c = ids.issue_claim(0, 60_000).unwrap();
        ids.claim(&c.code, owner.sync(), 1).unwrap();
        let guest = device();
        // Not the owner: no invites.
        assert!(ids.create_invite(&guest.id, 1).is_none());
        let code = ids.create_invite(&owner.id, 1).unwrap();
        let revoked = ids.create_invite(&owner.id, 1).unwrap();
        assert!(ids.revoke_invite(&owner.id, &revoked));
        let other = device();
        assert_eq!(
            ids.redeem(&revoked, other.sync(), 2),
            Err(AdmitError::BadCode)
        );
        assert!(!ids.is_admitted(&other.id));
        ids.redeem(&code, guest.sync(), 2).unwrap();
        assert!(ids.is_admitted(&guest.id));
        assert!(!ids.is_owner(&guest.id));
        // One use.
        assert_eq!(ids.redeem(&code, other.sync(), 3), Err(AdmitError::BadCode));
        let list = ids.invites(&owner.id, 3).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].redeemed_by.as_deref(), Some(guest.id.as_str()));
        // Expired invites don't work.
        let late = ids.create_invite(&owner.id, 10).unwrap();
        assert_eq!(
            ids.redeem(&late, other.sync(), 10 + INVITE_TTL_MS),
            Err(AdmitError::InviteExpired)
        );
    }

    #[test]
    fn the_registry_and_key_survive_a_restart() {
        let dir = tempfile::tempdir().unwrap();
        let ids = Identities::open(Some(dir.path())).unwrap();
        let keys = ids.keys();
        let a = device();
        let c = write_claim(dir.path(), 0, 60_000).unwrap();
        ids.claim(&c.code, a.sync(), 1).unwrap();
        assert!(!dir.path().join(CLAIM_FILE).exists());
        drop(ids);
        let ids = Identities::open(Some(dir.path())).unwrap();
        assert_eq!(ids.keys(), keys);
        assert!(ids.is_owner(&a.id));
        assert!(ids.shared(&a.id).is_some());
    }

    /// A redeem for a known identity takes nothing from its unproved sync
    /// (not its sync times or blob); a new identity's takes only what
    /// signatures prove.
    #[test]
    fn a_redeem_merges_nothing_unproved() {
        let ids = Identities::open(None).unwrap();
        let owner = device();
        let c = ids.issue_claim(0, 60_000).unwrap();
        ids.claim(&c.code, owner.sync(), 1).unwrap();
        assert!(ids.shared(&owner.id).unwrap().last_synced.is_empty());
        let before = ids.shared(&owner.id).unwrap();
        let code = ids.create_invite(&owner.id, 1).unwrap();
        let mut sync = owner.sync();
        sync.last_synced.insert("dev".into(), 100_000.0);
        ids.redeem(&code, sync, 2).unwrap();
        assert_eq!(ids.shared(&owner.id).unwrap(), before);
    }

    /// Room snapshots are merged in: an older one handed in after a newer
    /// one doesn't undo it.
    #[test]
    fn room_updates_never_go_back() {
        let ids = Identities::open(None).unwrap();
        let mut owner = device();
        let c = ids.issue_claim(0, 60_000).unwrap();
        ids.claim(&c.code, owner.sync(), 1).unwrap();
        let old = ids.shared(&owner.id).unwrap();
        let _b = hocket_core::identity::testing::pair(&mut owner, "b", 2.0);
        let mut newer = old.clone();
        newer.merge(owner.sync(), 10.0).unwrap();
        ids.update(&owner.id, newer, 10);
        ids.update(&owner.id, old, 11);
        let mut kept = ids.shared(&owner.id).unwrap();
        assert!(kept.state().is_member("b"));
    }

    #[test]
    fn a_forged_list_for_a_known_identity_changes_nothing() {
        let ids = Identities::open(None).unwrap();
        let owner = device();
        let c = ids.issue_claim(0, 60_000).unwrap();
        ids.claim(&c.code, owner.sync(), 1).unwrap();
        let code = ids.create_invite(&owner.id, 1).unwrap();
        // Someone with an invite names the owner's identity with a list of its own.
        let forged =
            Identity::create_with(&owner.id, "rogue", "Rogue", DeviceSecret::generate(), 1.0);
        ids.redeem(&code, forged.sync(), 2).unwrap();
        let mut shared = ids.shared(&owner.id).unwrap();
        assert!(shared.state().is_member("dev"));
        assert!(!shared.state().is_member("rogue"));
    }
}
