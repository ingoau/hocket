//! LAN room authentication: a mutual HMAC challenge/response keyed from the
//! account password, so a room on the LAN only admits devices that know the
//! Subsonic password for its scope, and a device only follows a LAN leader
//! that can prove the same. Nothing secret ever crosses the wire: each side
//! answers the other's random nonce with `HMAC-SHA256(key, role || nonce ||
//! device_id)`.
//!
//! ```text
//!   joiner                                   leader (room)
//!     │── Challenge{nonce: Nj} ─────────────────▶│
//!     │◀─ Proof{leader_id, mac(leader, Nj)} ─────│
//!     │◀─ Challenge{nonce: Nr} ──────────────────│
//!     │   verify: only then                      │
//!     │── Proof{joiner_id, mac(joiner, Nr)} ────▶│
//!     │── Hello (no credential) ────────────────▶│  verify, then admit
//!     │◀─ Welcome ───────────────────────────────│
//! ```
//!
//! The joiner sends nothing but a nonce until the leader has proved itself,
//! so a rogue listener that won the election learns nothing (not even a MAC
//! sample). A rogue joiner gets one MAC sample from an honest leader and
//! nothing else; the key is stretched ([`derive_lan_key`]) so that sample is
//! not a cheap offline oracle. Roles are bound into the MAC so a proof can
//! never be reflected back at the side that produced it.
//!
//! Known limit: without TLS there is no channel binding, so a rogue that
//! wins the election could relay both handshakes between two honest devices
//! and sit in the middle of that session (it still cannot obtain any
//! credential, and cannot be admitted anywhere itself).
//!
//! # What the actor must supply
//!
//! [`EngineConfig::lan_key`](crate::connect::EngineConfig) must be
//! `Some(derive_lan_key(&scope, &password))` where `scope` is
//! [`scope_key`](crate::connect::wire::scope_key)`(server_url, username)`
//! and `password` the account password the user entered (the same one the
//! Subsonic token is minted from). Without it the engine neither serves nor
//! follows LAN peers.

use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};

type HmacSha256 = Hmac<Sha256>;

/// Key derivation label; bump when the derivation changes.
const KEY_LABEL: &str = "hocket-lan-v1";
/// PBKDF2 rounds for the key stretch. A few milliseconds once per session
/// open; makes a captured MAC sample an expensive password oracle.
pub const KEY_ROUNDS: u32 = 20_000;
/// Nonce length in bytes.
pub const NONCE_LEN: usize = 32;

/// Who is proving: bound into every MAC so proofs cannot be reflected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    /// The side that connected inbound and wants to be admitted.
    Joiner,
    /// The side serving the room.
    Leader,
}

impl Role {
    fn tag(self) -> &'static [u8] {
        match self {
            Role::Joiner => b"joiner",
            Role::Leader => b"leader",
        }
    }
}

/// The shared LAN key for one scope. Never printed.
#[derive(Clone, PartialEq, Eq)]
pub struct LanKey(pub [u8; 32]);

impl std::fmt::Debug for LanKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "LanKey(<redacted>)")
    }
}

impl From<[u8; 32]> for LanKey {
    fn from(b: [u8; 32]) -> Self {
        LanKey(b)
    }
}

/// Derive the shared LAN key for a scope from the account password
/// (PBKDF2-HMAC-SHA256, salt = label || scope).
pub fn derive_lan_key(scope: &str, password: &str) -> LanKey {
    let mut salt = Vec::with_capacity(KEY_LABEL.len() + 1 + scope.len());
    salt.extend_from_slice(KEY_LABEL.as_bytes());
    salt.push(b'|');
    salt.extend_from_slice(scope.as_bytes());
    LanKey(pbkdf2_sha256(password.as_bytes(), &salt, KEY_ROUNDS))
}

/// PBKDF2 with HMAC-SHA256, one 32-byte block.
fn pbkdf2_sha256(password: &[u8], salt: &[u8], rounds: u32) -> [u8; 32] {
    let mut mac = HmacSha256::new_from_slice(password).expect("hmac accepts any key length");
    mac.update(salt);
    mac.update(&1u32.to_be_bytes());
    let mut u: [u8; 32] = mac.finalize().into_bytes().into();
    let mut out = u;
    for _ in 1..rounds.max(1) {
        let mut mac = HmacSha256::new_from_slice(password).expect("hmac accepts any key length");
        mac.update(&u);
        u = mac.finalize().into_bytes().into();
        for (o, b) in out.iter_mut().zip(u.iter()) {
            *o ^= b;
        }
    }
    out
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

/// `HMAC-SHA256(key, role || nonce || device_id)`, hex encoded.
pub fn prove(key: &LanKey, role: Role, nonce: &str, device_id: &str) -> String {
    let mut mac = HmacSha256::new_from_slice(&key.0).expect("hmac accepts any key length");
    mac.update(role.tag());
    mac.update(b"|");
    mac.update(nonce.as_bytes());
    mac.update(b"|");
    mac.update(device_id.as_bytes());
    hex::encode(mac.finalize().into_bytes())
}

/// Constant-time check of a proof against the nonce we issued.
pub fn verify(key: &LanKey, role: Role, nonce: &str, device_id: &str, mac: &str) -> bool {
    let Ok(given) = hex::decode(mac) else {
        return false;
    };
    let mut m = HmacSha256::new_from_slice(&key.0).expect("hmac accepts any key length");
    m.update(role.tag());
    m.update(b"|");
    m.update(nonce.as_bytes());
    m.update(b"|");
    m.update(device_id.as_bytes());
    m.verify_slice(&given).is_ok()
}

/// SHA-256 of arbitrary bytes (a small helper for callers that need a
/// stable 32-byte digest, e.g. the simulation deriving a key from its scope).
pub fn sha256(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}

/// A key straight from a digest: tests and the simulation, never the app
/// (which stretches the password with [`derive_lan_key`]).
pub fn test_key(label: &[u8]) -> LanKey {
    LanKey(sha256(label))
}

/// Whether a coordinator URL may carry the account credential: `wss://`
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
    fn key_is_deterministic_and_scope_bound() {
        let a = derive_lan_key("https://music.example|alice", "pw");
        let b = derive_lan_key("https://music.example|alice", "pw");
        let c = derive_lan_key("https://music.example|bob", "pw");
        let d = derive_lan_key("https://music.example|alice", "pw2");
        assert_eq!(a, b);
        assert_ne!(a, c);
        assert_ne!(a, d);
    }

    #[test]
    fn pbkdf2_matches_known_vector() {
        // RFC 6070-style vector for PBKDF2-HMAC-SHA256("password", "salt", 1).
        let out = pbkdf2_sha256(b"password", b"salt", 1);
        assert_eq!(
            hex::encode(out),
            "120fb6cffcf8b32c43e7225256c4f837a86548c92ccc35480805987cb70be17b"
        );
        let out = pbkdf2_sha256(b"password", b"salt", 2);
        assert_eq!(
            hex::encode(out),
            "ae4d0c95af6b46d32d0adff928f06dd02a303f8ef3c251dfd6e2d85a95474c43"
        );
    }

    #[test]
    fn proof_round_trips_and_binds_role_nonce_and_device() {
        let key = derive_lan_key("s", "pw");
        let nonce = new_nonce();
        assert!(nonce_valid(&nonce));
        let mac = prove(&key, Role::Joiner, &nonce, "dev-a");
        assert!(verify(&key, Role::Joiner, &nonce, "dev-a", &mac));
        assert!(!verify(&key, Role::Leader, &nonce, "dev-a", &mac));
        assert!(!verify(&key, Role::Joiner, &nonce, "dev-b", &mac));
        assert!(!verify(&key, Role::Joiner, &new_nonce(), "dev-a", &mac));
        let other = derive_lan_key("s", "other");
        assert!(!verify(&other, Role::Joiner, &nonce, "dev-a", &mac));
        assert!(!verify(&key, Role::Joiner, &nonce, "dev-a", "zz"));
        assert!(!verify(&key, Role::Joiner, &nonce, "dev-a", ""));
    }

    #[test]
    fn nonces_are_unique_and_well_formed() {
        assert_ne!(new_nonce(), new_nonce());
        assert!(!nonce_valid("abc"));
        assert!(!nonce_valid(&"g".repeat(64)));
    }

    #[test]
    fn key_debug_is_redacted() {
        let k = derive_lan_key("s", "pw");
        assert_eq!(format!("{k:?}"), "LanKey(<redacted>)");
        assert!(!format!("{:?}", Some(k)).contains('['));
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
