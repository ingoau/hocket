//! Room authentication: a mutual device-signature handshake over nonces.
//!
//! Every device in an identity has an ed25519 key on the identity's member
//! list ([`crate::identity`]). A room (a LAN leader, or the hosted
//! coordinator with its own key, pinned when it was claimed) and a joiner
//! each sign the other's nonce, together with the identity, their own
//! device id and the digest of their member-list heads:
//!
//! ```text
//!   joiner                                         leader (room)
//!     │── Challenge{Nj, identity, heads} ──────────────▶│
//!     │◀─ Proof{leader, sig(leader, Nj), heads, ops} ───│  ops: what our heads lack
//!     │◀─ Challenge{Nr} ────────────────────────────────│
//!     │   merge ops, then verify: only then             │
//!     │── Proof{joiner, sig(joiner, Nr), heads, ops} ──▶│  merge, verify membership
//!     │── Hello ───────────────────────────────────────▶│  admit
//!     │◀─ Welcome ──────────────────────────────────────│
//! ```
//!
//! The joiner sends nothing but a nonce and its heads until the leader has
//! proved it is a current member (or the pinned coordinator). Each side
//! merges the ops the other sent before checking, so a peer with an older
//! list catches up before it admits or follows anyone; the list is a
//! grow-only union, so an old or replayed list never undoes a removal.
//! Roles are bound into the signature so a proof can never be reflected.
//!
//! On the LAN (plain `ws://`) both sides also send an ephemeral X25519 key
//! and sign the transcript with it ([`bind_nonce`]), and every frame after
//! the handshake is sealed with keys from that exchange
//! ([`crate::connect::channel`]), so a relay in the middle can neither read
//! nor inject. The coordinator is reached over TLS and pinned by key.

use crate::identity::{self, log::MemberState};

/// Nonce length in bytes.
pub const NONCE_LEN: usize = 32;

/// Who is proving: bound into every signature so proofs cannot be reflected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    /// The side that connected inbound and wants to be admitted.
    Joiner,
    /// The side serving the room.
    Leader,
}

impl Role {
    pub fn tag(self) -> &'static str {
        match self {
            Role::Joiner => "joiner",
            Role::Leader => "leader",
        }
    }
}

/// The digest a proof signs for a list of heads.
pub fn heads_digest(heads: &[String]) -> String {
    let mut c = identity::crypto::Canon::new("hocket-heads");
    c.list(heads);
    identity::crypto::sha256_hex(c.finish())
}

/// The nonce a proof signs when it also binds a LAN channel transcript
/// ([`crate::connect::channel`]): the nonce and the binding together.
pub fn bind_nonce(nonce: &str, binding: &str) -> String {
    format!("{nonce}.{binding}")
}

/// Sign a proof: `secret` is the prover's device (or coordinator) key.
pub fn prove(
    secret: &identity::crypto::DeviceSecret,
    role: Role,
    identity_id: &str,
    nonce: &str,
    device_id: &str,
    heads: &[String],
) -> String {
    secret.sign(&identity::proof_bytes(
        role.tag(),
        identity_id,
        nonce,
        device_id,
        &heads_digest(heads),
    ))
}

/// Check a proof against a known signing key.
pub fn verify_with_key(
    sign_pk: &str,
    role: Role,
    identity_id: &str,
    nonce: &str,
    device_id: &str,
    heads: &[String],
    sig: &str,
) -> bool {
    identity::crypto::verify(
        sign_pk,
        &identity::proof_bytes(
            role.tag(),
            identity_id,
            nonce,
            device_id,
            &heads_digest(heads),
        ),
        sig,
    )
}

/// Check a member's proof: `device_id` must be a current member of
/// `state`, and the proof signed with its key.
pub fn verify_member(
    state: &MemberState,
    role: Role,
    identity_id: &str,
    nonce: &str,
    device_id: &str,
    heads: &[String],
    sig: &str,
) -> bool {
    state
        .sign_key(device_id)
        .is_some_and(|k| verify_with_key(k, role, identity_id, nonce, device_id, heads, sig))
}

/// Check a peer's proof against our member list, merging the ops it sent
/// only when needed: when it is a member here and we know its heads, our
/// list decides; otherwise its proof must first verify with a key it could
/// have (its current one, or one an `Add` it sent names) before the ops are
/// merged into a scratch copy, so a stranger can't make us do the merge.
#[allow(clippy::too_many_arguments)]
pub fn verify_member_with_sync(
    log: &identity::log::MemberLog,
    sync: Option<&identity::log::MemberLog>,
    role: Role,
    identity_id: &str,
    nonce: &str,
    device_id: &str,
    heads: &[String],
    sig: &str,
) -> bool {
    let state = log.compute_state();
    if state.is_member(device_id) && log.knows(heads) {
        return verify_member(&state, role, identity_id, nonce, device_id, heads, sig);
    }
    let Some(sync) = sync.filter(|s| !s.is_empty()) else {
        return false;
    };
    let mut keys: Vec<String> = state
        .sign_key(device_id)
        .map(str::to_string)
        .into_iter()
        .collect();
    for op in sync.ops() {
        if let identity::log::OpBody::Add { device, .. } = &op.body {
            if device.id == device_id && !keys.contains(&device.keys.sign) {
                keys.push(device.keys.sign.clone());
            }
        }
    }
    if !keys
        .iter()
        .any(|k| verify_with_key(k, role, identity_id, nonce, device_id, heads, sig))
    {
        return false;
    }
    let mut scratch = log.clone();
    let _ = scratch.merge(sync.ops());
    scratch.knows(heads)
        && verify_member(
            scratch.state(),
            role,
            identity_id,
            nonce,
            device_id,
            heads,
            sig,
        )
}

/// A fresh random nonce, hex encoded (what travels in `Msg::Challenge`).
pub fn new_nonce() -> String {
    use rand::RngCore;
    let mut b = [0u8; NONCE_LEN];
    rand::rng().fill_bytes(&mut b);
    hex::encode(b)
}

/// A well-formed nonce: exactly [`NONCE_LEN`] bytes of hex.
pub fn nonce_valid(nonce: &str) -> bool {
    nonce.len() == NONCE_LEN * 2 && nonce.bytes().all(|b| b.is_ascii_hexdigit())
}

/// Whether a coordinator URL may be used: `wss://`
/// always; `ws://` only to the loopback host, or to a private/link-local
/// host when `allow_insecure` (the `connect.allowInsecureCoordinator`
/// setting) is on. Anything else, or any URL with userinfo or an unknown
/// scheme, is rejected with the reason.
pub fn coordinator_url_check(url: &str, allow_insecure: bool) -> Result<(), String> {
    let u = url::Url::parse(url).map_err(|e| e.to_string())?;
    if !u.username().is_empty() || u.password().is_some() {
        return Err("coordinator URL must not contain credentials".into());
    }
    let Some(host) = u.host() else {
        return Err("coordinator URL needs a host".into());
    };
    match u.scheme() {
        "wss" => Ok(()),
        "ws" => {
            if host_is_loopback(&host) {
                Ok(())
            } else if host_is_private(&host) {
                if allow_insecure {
                    Ok(())
                } else {
                    Err("ws:// to a private host needs connect.allowInsecureCoordinator".into())
                }
            } else {
                Err("coordinator URL must be wss:// (ws:// only on the local network)".into())
            }
        }
        _ => Err("expected a wss:// URL".into()),
    }
}

fn host_is_loopback(host: &url::Host<&str>) -> bool {
    match host {
        url::Host::Ipv4(a) => a.is_loopback(),
        url::Host::Ipv6(a) => a.is_loopback(),
        url::Host::Domain(d) => {
            let d = d.trim_end_matches('.').to_ascii_lowercase();
            d == "localhost" || d.ends_with(".localhost")
        }
    }
}

/// Private, link-local or LAN-only names (including loopback).
pub fn host_is_private(host: &url::Host<&str>) -> bool {
    match host {
        url::Host::Ipv4(a) => ipv4_is_private(*a),
        url::Host::Ipv6(a) => ipv6_is_private(*a),
        url::Host::Domain(d) => {
            let d = d.trim_end_matches('.').to_ascii_lowercase();
            d == "localhost"
                || [".localhost", ".local", ".lan", ".home.arpa", ".internal"]
                    .iter()
                    .any(|s| d.ends_with(s))
        }
    }
}

/// Loopback, RFC 1918, link-local, CGNAT, unspecified, broadcast, multicast.
pub fn ipv4_is_private(a: std::net::Ipv4Addr) -> bool {
    let o = a.octets();
    a.is_loopback()
        || a.is_private()
        || a.is_link_local()
        || a.is_unspecified()
        || a.is_broadcast()
        || a.is_multicast()
        || (o[0] == 100 && (64..=127).contains(&o[1]))
        || o[0] == 0
}

/// Loopback, unique-local, link-local, unspecified, multicast, and v4-mapped
/// forms of the same.
pub fn ipv6_is_private(a: std::net::Ipv6Addr) -> bool {
    if let Some(v4) = a.to_ipv4_mapped() {
        return ipv4_is_private(v4);
    }
    let s = a.segments();
    a.is_loopback()
        || a.is_unspecified()
        || a.is_multicast()
        || (s[0] & 0xfe00) == 0xfc00
        || (s[0] & 0xffc0) == 0xfe80
}

/// Whether an address should never be a verification target chosen by an
/// unauthenticated client.
pub fn ip_is_private(ip: std::net::IpAddr) -> bool {
    match ip {
        std::net::IpAddr::V4(a) => ipv4_is_private(a),
        std::net::IpAddr::V6(a) => ipv6_is_private(a),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn proofs_bind_role_nonce_device_and_heads() {
        let a = identity::Identity::create("a", "A", 1.0);
        let heads = a.heads();
        let nonce = new_nonce();
        assert!(nonce_valid(&nonce));
        let sig = prove(a.secret(), Role::Joiner, &a.id, &nonce, "a", &heads);
        let st = a.member_state();
        assert!(verify_member(
            &st,
            Role::Joiner,
            &a.id,
            &nonce,
            "a",
            &heads,
            &sig
        ));
        assert!(!verify_member(
            &st,
            Role::Leader,
            &a.id,
            &nonce,
            "a",
            &heads,
            &sig
        ));
        assert!(!verify_member(
            &st,
            Role::Joiner,
            &a.id,
            &new_nonce(),
            "a",
            &heads,
            &sig
        ));
        assert!(!verify_member(
            &st,
            Role::Joiner,
            "other",
            &nonce,
            "a",
            &heads,
            &sig
        ));
        assert!(!verify_member(
            &st,
            Role::Joiner,
            &a.id,
            &nonce,
            "b",
            &heads,
            &sig
        ));
        assert!(!verify_member(
            &st,
            Role::Joiner,
            &a.id,
            &nonce,
            "a",
            &[],
            &sig
        ));
        assert!(!verify_member(
            &st,
            Role::Joiner,
            &a.id,
            &nonce,
            "a",
            &heads,
            "zz"
        ));
    }

    #[test]
    fn nonces_are_unique_and_well_formed() {
        assert_ne!(new_nonce(), new_nonce());
        assert!(!nonce_valid("abc"));
        assert!(!nonce_valid(&"g".repeat(64)));
    }

    #[test]
    fn coordinator_url_rules() {
        assert!(coordinator_url_check("wss://c.example/ws", false).is_ok());
        assert!(coordinator_url_check("ws://127.0.0.1:7373/ws", false).is_ok());
        assert!(coordinator_url_check("ws://localhost:7373/ws", false).is_ok());
        assert!(coordinator_url_check("ws://[::1]:7373/ws", false).is_ok());
        assert!(coordinator_url_check("ws://192.168.1.10:7373/ws", false).is_err());
        assert!(coordinator_url_check("ws://192.168.1.10:7373/ws", true).is_ok());
        assert!(coordinator_url_check("ws://nas.local:7373/ws", true).is_ok());
        assert!(coordinator_url_check("ws://c.example/ws", true).is_err());
        assert!(coordinator_url_check("wss://user:pw@c.example/ws", false).is_err());
        assert!(coordinator_url_check("https://c.example/ws", false).is_err());
        assert!(coordinator_url_check("not a url", false).is_err());
    }

    #[test]
    fn private_ip_classification() {
        use std::net::IpAddr;
        for s in [
            "127.0.0.1",
            "10.1.2.3",
            "172.16.5.5",
            "192.168.0.1",
            "169.254.169.254",
            "100.64.0.1",
            "0.0.0.0",
            "::1",
            "fe80::1",
            "fd00::1",
            "::ffff:10.0.0.1",
        ] {
            assert!(ip_is_private(s.parse::<IpAddr>().unwrap()), "{s}");
        }
        for s in ["8.8.8.8", "1.1.1.1", "2606:4700::1111", "172.32.0.1"] {
            assert!(!ip_is_private(s.parse::<IpAddr>().unwrap()), "{s}");
        }
    }
}
