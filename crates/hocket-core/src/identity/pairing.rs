//! Pairing: a two-way exchange that admits a new device into an identity.
//!
//! ```text
//!   inviter (member)                      new device
//!   Invite ──── QR or hocket://pair link ────▶ (identity, inviter key, secret,
//!                                               expiry, rendezvous, accounts)
//!                      rendezvous: the inviter's LAN listener, or the
//!                      coordinator relaying to the inviter's socket
//!          ◀── PairJoin {inviteId, device keys, mac(secret)} ──
//!   check mac, expiry, one use; sign Add(device)
//!          ── PairAccept {member list, grant, mac(secret)} ──▶
//!                                         check mac, the list chains to the
//!                                         inviter's key, it admits us, the
//!                                         grant opens to the committed key
//! ```
//!
//! The secret never travels: both sides prove it with an HMAC over what they
//! send. The link carries no password; account configs ride along so a new
//! device with credential sync off only needs to type passwords.

use base64::Engine as _;
use serde::{Deserialize, Serialize};

use crate::api::{DeviceId, EpochMs};
use crate::identity::crypto::{self, Canon, DeviceSecret};
use crate::identity::log::{MemberDevice, OpBody};
use crate::identity::{Grant, Identity, IdentitySync, PendingInvite};

/// How long an invite stays good.
pub const INVITE_TTL_MS: f64 = 10.0 * 60_000.0;
/// The link scheme and path.
pub const LINK_PREFIX: &str = "hocket://pair?d=";

/// Where the new device can reach the inviter.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Rendezvous {
    /// The inviter's LAN listener(s), `ws://ip:port`.
    #[serde(default)]
    pub lan: Vec<String>,
    /// The identity's coordinator, which relays to the inviter.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub coordinator: Option<String>,
}

/// A non-secret account config (what to add once a password is typed).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InviteAccount {
    pub kind: String,
    pub url: String,
    pub username: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

/// What the QR code or link carries.
#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Invite {
    pub identity: String,
    pub inviter: DeviceId,
    /// The inviter's ed25519 key: the member list the new device receives
    /// must have been signed by it.
    pub inviter_key: String,
    /// 16 random bytes, hex. Proven, never sent.
    pub secret: String,
    pub expires_at: EpochMs,
    pub rendezvous: Rendezvous,
    #[serde(default)]
    pub accounts: Vec<InviteAccount>,
    /// Synced settings riding along (the old setup link's payload).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub settings: Option<serde_json::Value>,
}

impl std::fmt::Debug for Invite {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Invite")
            .field("identity", &self.identity)
            .field("inviter", &self.inviter)
            .field("expires_at", &self.expires_at)
            .field("rendezvous", &self.rendezvous)
            .finish_non_exhaustive()
    }
}

impl Invite {
    pub fn invite_id(&self) -> String {
        invite_id(&self.secret)
    }

    /// `hocket://pair?d=<base64url(json)>`.
    pub fn to_link(&self) -> String {
        let json = serde_json::to_vec(self).unwrap_or_default();
        format!(
            "{LINK_PREFIX}{}",
            base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(json)
        )
    }

    pub fn from_link(link: &str) -> Result<Invite, String> {
        let d = link
            .trim()
            .strip_prefix(LINK_PREFIX)
            .ok_or("not a Hocket pairing link")?;
        let d = d.split('&').next().unwrap_or(d);
        let json = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(d)
            .map_err(|_| "the pairing link is damaged".to_string())?;
        let inv: Invite =
            serde_json::from_slice(&json).map_err(|_| "the pairing link is damaged".to_string())?;
        if !super::is_identity_id(&inv.identity) || inv.secret.len() != 32 {
            return Err("the pairing link is damaged".into());
        }
        Ok(inv)
    }
}

pub fn invite_id(secret: &str) -> String {
    let mut c = Canon::new("hocket-invite-id");
    c.str(secret);
    crypto::sha256_hex(c.finish())[..32].to_string()
}

fn join_bytes(invite_id: &str, d: &MemberDevice) -> Vec<u8> {
    let mut c = Canon::new("hocket-pair-join");
    c.str(invite_id)
        .str(&d.id)
        .str(&d.name)
        .str(&d.keys.sign)
        .str(&d.keys.kx);
    c.finish().to_vec()
}

fn accept_bytes(invite_id: &str, add_op: &str, device: &str) -> Vec<u8> {
    let mut c = Canon::new("hocket-pair-accept");
    c.str(invite_id).str(add_op).str(device);
    c.finish().to_vec()
}

/// New device → inviter (through the rendezvous).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PairJoin {
    pub invite_id: String,
    pub device: MemberDevice,
    pub mac: String,
}

/// Inviter → new device.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PairAccept {
    pub invite_id: String,
    /// The hash of the op adding the new device.
    pub add_op: String,
    pub state: IdentitySync,
    pub grant: Option<Grant>,
    pub mac: String,
}

/// The new device's side.
pub struct Joiner {
    pub invite: Invite,
    pub device_id: DeviceId,
    secret: DeviceSecret,
    pub join: PairJoin,
}

impl Joiner {
    /// Start pairing with fresh keys (the device's old identity, if any,
    /// is left behind once this succeeds).
    pub fn new(
        invite: Invite,
        device_id: &str,
        device_name: &str,
        now: EpochMs,
    ) -> Result<Joiner, String> {
        Joiner::with_secret(
            invite,
            device_id,
            device_name,
            DeviceSecret::generate(),
            now,
        )
    }

    pub fn with_secret(
        invite: Invite,
        device_id: &str,
        device_name: &str,
        secret: DeviceSecret,
        now: EpochMs,
    ) -> Result<Joiner, String> {
        if invite.expires_at <= now {
            return Err("this pairing code has expired; make a new one on the other device".into());
        }
        let device = MemberDevice {
            id: device_id.into(),
            name: device_name.chars().take(60).collect(),
            keys: secret.public(),
        };
        let invite_id = invite.invite_id();
        let mac = crypto::hmac_hex(invite.secret.as_bytes(), &join_bytes(&invite_id, &device));
        Ok(Joiner {
            device_id: device_id.into(),
            secret,
            join: PairJoin {
                invite_id,
                device,
                mac,
            },
            invite,
        })
    }

    /// Check the inviter's answer and become a member.
    pub fn finish(self, accept: PairAccept) -> Result<Identity, String> {
        let bad = |why: &str| Err(format!("pairing failed: {why}"));
        if accept.invite_id != self.join.invite_id
            || !crypto::hmac_verify(
                self.invite.secret.as_bytes(),
                &accept_bytes(&accept.invite_id, &accept.add_op, &self.device_id),
                &accept.mac,
            )
        {
            return bad("the other device didn't prove it made this code");
        }
        if accept.state.identity() != self.invite.identity {
            return bad("the other device answered for another identity");
        }
        let mut shared = crate::identity::SharedState::new(crate::identity::log::MemberLog::empty(
            &self.invite.identity,
        ));
        shared
            .merge(accept.state)
            .map_err(|e| format!("pairing failed: {e}"))?;
        let add = shared.log.get(&accept.add_op).cloned();
        let state = shared.state().clone();
        match add {
            Some(op)
                if op.author == self.invite.inviter
                    && state.sign_key(&op.author) == Some(self.invite.inviter_key.as_str())
                    && matches!(&op.body, OpBody::Add { device, .. } if *device == self.join.device)
                    && state.valid.contains(&accept.add_op) => {}
            _ => return bad("the member list doesn't admit this device"),
        }
        if let Some(g) = accept.grant {
            if !shared.grants.contains(&g) {
                shared.grants.push(g);
            }
        }
        Identity::joined(&self.device_id, self.secret, shared)
            .ok_or_else(|| "pairing failed: the member list doesn't admit this device".into())
    }
}

impl Identity {
    /// Make an invite (kept until used or expired).
    pub fn invite(
        &mut self,
        rendezvous: Rendezvous,
        accounts: Vec<InviteAccount>,
        settings: Option<serde_json::Value>,
        now: EpochMs,
    ) -> Invite {
        self.expire_invites(now);
        let secret = crypto::random_hex(16);
        let inv = Invite {
            identity: self.id.clone(),
            inviter: self.device_id.clone(),
            inviter_key: self.public_keys().sign,
            secret: secret.clone(),
            expires_at: now + INVITE_TTL_MS,
            rendezvous,
            accounts,
            settings,
        };
        self.invites.push(PendingInvite {
            invite_id: inv.invite_id(),
            secret,
            expires_at: inv.expires_at,
        });
        inv
    }

    /// The inviter's side of the exchange: check the join against an open
    /// invite (one use), admit the device and answer.
    pub fn accept_join(&mut self, join: &PairJoin, now: EpochMs) -> Result<PairAccept, String> {
        self.expire_invites(now);
        let Some(idx) = self
            .invites
            .iter()
            .position(|i| i.invite_id == join.invite_id)
        else {
            return Err("unknown or expired pairing code".into());
        };
        let secret = self.invites[idx].secret.clone();
        if !crypto::hmac_verify(
            secret.as_bytes(),
            &join_bytes(&join.invite_id, &join.device),
            &join.mac,
        ) {
            return Err("the new device didn't prove the pairing code".into());
        }
        self.invites.remove(idx);
        let (op, grant) = self.add_device(join.device.clone(), now)?;
        let add_op = op.hash();
        let mac = crypto::hmac_hex(
            secret.as_bytes(),
            &accept_bytes(&join.invite_id, &add_op, &join.device.id),
        );
        Ok(PairAccept {
            invite_id: join.invite_id.clone(),
            add_op,
            state: self.sync(),
            grant,
            mac,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rendezvous() -> Rendezvous {
        Rendezvous {
            lan: vec!["ws://192.168.1.2:7374".into()],
            coordinator: None,
        }
    }

    #[test]
    fn pairing_round_trip() {
        let mut a = Identity::create("a", "Phone", 1.0);
        let inv = a.invite(
            rendezvous(),
            vec![InviteAccount {
                kind: "navidrome".into(),
                url: "https://music.example".into(),
                username: "u".into(),
                name: None,
            }],
            None,
            1.0,
        );
        let link = inv.to_link();
        assert!(!link.contains(&inv.secret));
        let parsed = Invite::from_link(&link).unwrap();
        assert_eq!(parsed.secret, inv.secret);
        let j = Joiner::new(parsed, "b", "Laptop", 2.0).unwrap();
        let accept = a.accept_join(&j.join, 3.0).unwrap();
        let mut b = j.finish(accept).unwrap();
        assert!(b.is_member());
        assert_eq!(b.id, a.id);
        assert_eq!(b.group_key(), a.group_key());
        // One use only.
        let j2 = Joiner::new(inv.clone(), "c", "Other", 4.0).unwrap();
        assert!(a.accept_join(&j2.join, 4.0).is_err());
    }

    #[test]
    fn wrong_secret_expired_and_forged_answers_fail() {
        let mut a = Identity::create("a", "Phone", 1.0);
        let inv = a.invite(rendezvous(), vec![], None, 1.0);
        // Expired on the joining side.
        assert!(Joiner::new(inv.clone(), "b", "B", inv.expires_at + 1.0).is_err());
        // Expired on the inviter's side.
        let j = Joiner::new(inv.clone(), "b", "B", 2.0).unwrap();
        assert!(a.accept_join(&j.join, inv.expires_at + 1.0).is_err());
        // A join without the secret.
        let inv = a.invite(rendezvous(), vec![], None, 2.0);
        let mut bad = inv.clone();
        bad.secret = crypto::random_hex(16);
        let j = Joiner::new(bad, "b", "B", 3.0).unwrap();
        let mut join = j.join.clone();
        join.invite_id = inv.invite_id();
        assert!(a.accept_join(&join, 3.0).is_err());
        // An answer from a device that doesn't hold the secret.
        let mut rogue = Identity::create("r", "Rogue", 1.0);
        let rinv = rogue.invite(rendezvous(), vec![], None, 1.0);
        let j = Joiner::new(inv.clone(), "b", "B", 3.0).unwrap();
        let mut fake = j.join.clone();
        fake.invite_id = rinv.invite_id();
        fake.mac = crypto::hmac_hex(
            rinv.secret.as_bytes(),
            &join_bytes(&fake.invite_id, &fake.device),
        );
        let mut accept = rogue.accept_join(&fake, 3.0).unwrap();
        accept.invite_id = inv.invite_id();
        assert!(j.finish(accept).is_err());
    }

    #[test]
    fn links_are_validated() {
        assert!(Invite::from_link("hocket://setup?x").is_err());
        assert!(Invite::from_link("hocket://pair?d=!!!").is_err());
    }
}
