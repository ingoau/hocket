//! The LAN channel: an ephemeral X25519 exchange bound into the handshake
//! proofs, and ChaCha20-Poly1305 on every frame after it.
//!
//! LAN sockets are plain `ws://`, so without this a device that relays both
//! handshakes between two members could read and inject everything after
//! them. Each side adds a fresh X25519 key to the handshake and signs a
//! transcript naming both keys, both nonces and the device ids
//! ([`auth`](crate::connect::auth)):
//!
//! ```text
//!   joiner                                         leader (room)
//!     │── Challenge{Nj, Ej} ───────────────────────────▶│
//!     │◀─ Proof{sig over Nj + H(Ej, El, Nj, L)}, El ────│
//!     │◀─ Challenge{Nr} ────────────────────────────────│
//!     │── Proof{sig over Nr + H(Ej, El, Nj, Nr, L, J)} ▶│
//!     │══ Sealed{Hello} ═══════════════════════════════▶│  keys from DH(Ej, El)
//! ```
//!
//! A relay that swaps either ephemeral key breaks a signature; one that
//! passes them through never learns the shared secret. Each direction has
//! its own key (HKDF over the shared secret, salted with the full
//! transcript) and a frame counter in the AEAD's associated data, so a
//! frame can't be replayed, reordered, dropped unnoticed or reflected.
//!
//! The coordinator is reached over TLS and pinned by key, so its rooms
//! don't require this ([`RoomConfig::require_channel`](crate::connect::room::RoomConfig)).

use hkdf::Hkdf;
use sha2::Sha256;
use x25519_dalek::{PublicKey as XPublic, StaticSecret};

use crate::connect::wire::{Msg, WireMessage};
use crate::identity::crypto::{self, Canon, Sealed};

/// One side's ephemeral X25519 key for one handshake.
#[derive(Clone)]
pub struct Ephemeral {
    secret: StaticSecret,
    /// Hex.
    pub public: String,
}

impl std::fmt::Debug for Ephemeral {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Ephemeral({})", self.public)
    }
}

impl Ephemeral {
    pub fn new() -> Ephemeral {
        let secret = StaticSecret::from(crypto::random32());
        let public = hex::encode(XPublic::from(&secret).to_bytes());
        Ephemeral { secret, public }
    }

    /// The shared secret with `theirs` (hex), unless it is malformed or a
    /// low-order point (an all-zero result).
    fn agree(&self, theirs: &str) -> Option<[u8; 32]> {
        if !valid_key(theirs) {
            return None;
        }
        let shared = self
            .secret
            .diffie_hellman(&XPublic::from(crypto::decode32(theirs)?));
        shared.was_contributory().then_some(*shared.as_bytes())
    }
}

/// Equal when the public keys are (a handshake compares, never the secret).
impl PartialEq for Ephemeral {
    fn eq(&self, other: &Self) -> bool {
        self.public == other.public
    }
}

impl Default for Ephemeral {
    fn default() -> Self {
        Ephemeral::new()
    }
}

/// A well-formed ephemeral key: 32 bytes of lowercase hex.
pub fn valid_key(k: &str) -> bool {
    k.len() == 64 && crypto::is_lower_hex(k)
}

/// What the leader's proof binds: both keys, the joiner's nonce and the
/// leader's id (the joiner's id isn't known yet).
pub fn leader_binding(
    joiner_eph: &str,
    leader_eph: &str,
    joiner_nonce: &str,
    leader: &str,
) -> String {
    let mut c = Canon::new("hocket-lan-leader-v1");
    c.str(joiner_eph)
        .str(leader_eph)
        .str(joiner_nonce)
        .str(leader);
    crypto::sha256_hex(c.finish())
}

/// What the joiner's proof binds (and what salts the keys): the whole
/// transcript.
pub fn joiner_binding(
    joiner_eph: &str,
    leader_eph: &str,
    joiner_nonce: &str,
    leader_nonce: &str,
    leader: &str,
    joiner: &str,
) -> String {
    let mut c = Canon::new("hocket-lan-joiner-v1");
    c.str(joiner_eph)
        .str(leader_eph)
        .str(joiner_nonce)
        .str(leader_nonce)
        .str(leader)
        .str(joiner);
    crypto::sha256_hex(c.finish())
}

/// Which end of the channel this is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum End {
    Joiner,
    Leader,
}

/// One established channel: a key and a counter per direction.
#[derive(Clone)]
pub struct Channel {
    send_key: [u8; 32],
    recv_key: [u8; 32],
    send_seq: u64,
    recv_seq: u64,
}

impl std::fmt::Debug for Channel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Channel(sent {}, received {})",
            self.send_seq, self.recv_seq
        )
    }
}

impl Channel {
    /// The channel for `end`, from our ephemeral key, the other side's and
    /// the joiner binding (the transcript). `None` for a bad key.
    pub fn new(end: End, ours: &Ephemeral, theirs: &str, transcript: &str) -> Option<Channel> {
        let shared = ours.agree(theirs)?;
        let hk = Hkdf::<Sha256>::new(Some(transcript.as_bytes()), &shared);
        let mut j2l = [0u8; 32];
        let mut l2j = [0u8; 32];
        hk.expand(b"hocket-lan-v1 joiner to leader", &mut j2l)
            .expect("32 bytes is a valid HKDF output length");
        hk.expand(b"hocket-lan-v1 leader to joiner", &mut l2j)
            .expect("32 bytes is a valid HKDF output length");
        let (send_key, recv_key) = match end {
            End::Joiner => (j2l, l2j),
            End::Leader => (l2j, j2l),
        };
        Some(Channel {
            send_key,
            recv_key,
            send_seq: 0,
            recv_seq: 0,
        })
    }

    fn aad(seq: u64) -> Vec<u8> {
        let mut c = Canon::new("hocket-lan-frame-v1");
        c.num(seq as f64);
        c.finish().to_vec()
    }

    /// `msg` sealed as the next frame.
    pub fn seal(&mut self, msg: &WireMessage) -> WireMessage {
        let seq = self.send_seq;
        self.send_seq += 1;
        let plain = msg.encode().unwrap_or_default();
        let s = crypto::seal(&self.send_key, &Self::aad(seq), plain.as_bytes());
        WireMessage::new(Msg::Sealed {
            seq,
            nonce: s.nonce,
            ct: s.ct,
        })
    }

    /// The frame inside `msg` whatever its place in the order (tests that
    /// read some of a peer's frames and not others).
    #[cfg(test)]
    pub(crate) fn peek(&self, msg: &WireMessage) -> Option<WireMessage> {
        let Msg::Sealed { seq, nonce, ct } = &msg.msg else {
            return None;
        };
        let sealed = Sealed {
            nonce: nonce.clone(),
            ct: ct.clone(),
        };
        let plain = crypto::open(&self.recv_key, &Self::aad(*seq), &sealed)?;
        WireMessage::decode(std::str::from_utf8(&plain).ok()?).ok()
    }

    /// The frame inside `msg`, when it is the next sealed frame and opens.
    /// Anything else (a plain frame, a replay, a forgery) is `None`, and the
    /// caller drops the connection.
    pub fn open(&mut self, msg: &WireMessage) -> Option<WireMessage> {
        let Msg::Sealed { seq, nonce, ct } = &msg.msg else {
            return None;
        };
        if *seq != self.recv_seq {
            return None;
        }
        let sealed = Sealed {
            nonce: nonce.clone(),
            ct: ct.clone(),
        };
        let plain = crypto::open(&self.recv_key, &Self::aad(*seq), &sealed)?;
        let inner = WireMessage::decode(std::str::from_utf8(&plain).ok()?).ok()?;
        if matches!(inner.msg, Msg::Sealed { .. }) {
            return None;
        }
        self.recv_seq += 1;
        Some(inner)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pair() -> (Channel, Channel) {
        let (j, l) = (Ephemeral::new(), Ephemeral::new());
        let t = joiner_binding(&j.public, &l.public, "nj", "nr", "L", "J");
        (
            Channel::new(End::Joiner, &j, &l.public, &t).unwrap(),
            Channel::new(End::Leader, &l, &j.public, &t).unwrap(),
        )
    }

    fn msg() -> WireMessage {
        WireMessage::new(Msg::SyncRequest)
    }

    #[test]
    fn frames_round_trip_in_order_only() {
        let (mut j, mut l) = pair();
        let a = j.seal(&msg());
        let b = j.seal(&msg());
        assert!(!a.encode().unwrap().contains("syncRequest"));
        // Out of order: refused.
        assert!(l.clone().open(&b).is_none());
        assert_eq!(l.open(&a).unwrap().msg, Msg::SyncRequest);
        // Replayed: refused.
        assert!(l.open(&a).is_none());
        assert_eq!(l.open(&b).unwrap().msg, Msg::SyncRequest);
        // The other direction has its own key: a reflected frame fails.
        let c = l.seal(&msg());
        assert!(l.clone().open(&c).is_none());
        assert_eq!(j.open(&c).unwrap().msg, Msg::SyncRequest);
        // Plain frames are never accepted once sealed.
        assert!(j.open(&msg()).is_none());
    }

    #[test]
    fn different_transcripts_give_different_keys() {
        let (j, l) = (Ephemeral::new(), Ephemeral::new());
        let t1 = joiner_binding(&j.public, &l.public, "nj", "nr", "L", "J");
        let t2 = joiner_binding(&j.public, &l.public, "nj", "nr", "L", "K");
        let mut a = Channel::new(End::Joiner, &j, &l.public, &t1).unwrap();
        let mut b = Channel::new(End::Leader, &l, &j.public, &t2).unwrap();
        assert!(b.open(&a.seal(&msg())).is_none());
    }

    #[test]
    fn low_order_and_malformed_keys_are_refused() {
        let e = Ephemeral::new();
        assert!(Channel::new(End::Joiner, &e, &"00".repeat(32), "t").is_none());
        assert!(Channel::new(End::Joiner, &e, "zz", "t").is_none());
        assert!(Channel::new(End::Joiner, &e, &e.public.to_uppercase(), "t").is_none());
    }
}
