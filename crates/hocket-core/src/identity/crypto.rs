//! The primitives identities are built from: device keys (ed25519 to sign,
//! X25519 to agree), a canonical byte encoding to sign and hash, sealed
//! boxes (ChaCha20-Poly1305) and envelopes (an ephemeral X25519 exchange,
//! HKDF-SHA256, then a sealed box) that hand a secret to one device.
//!
//! Everything public travels as lowercase hex (keys, signatures, hashes) or
//! base64 (ciphertext).

use base64::Engine as _;
use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{ChaCha20Poly1305, Key, Nonce};
use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
use hkdf::Hkdf;
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use x25519_dalek::{PublicKey as XPublic, StaticSecret};

/// 32 random bytes from the OS-seeded thread RNG.
pub fn random32() -> [u8; 32] {
    use rand::RngCore;
    let mut b = [0u8; 32];
    rand::rng().fill_bytes(&mut b);
    b
}

/// `n` random bytes, hex.
pub fn random_hex(n: usize) -> String {
    use rand::RngCore;
    let mut b = vec![0u8; n];
    rand::rng().fill_bytes(&mut b);
    hex::encode(b)
}

pub fn sha256(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    hex::encode(sha256(bytes))
}

/// `HMAC-SHA256(key, msg)`, hex.
pub fn hmac_hex(key: &[u8], msg: &[u8]) -> String {
    let mut m = <Hmac<Sha256> as Mac>::new_from_slice(key).expect("hmac accepts any key length");
    m.update(msg);
    hex::encode(m.finalize().into_bytes())
}

/// Constant-time check of [`hmac_hex`].
pub fn hmac_verify(key: &[u8], msg: &[u8], mac_hex: &str) -> bool {
    let Ok(given) = hex::decode(mac_hex) else {
        return false;
    };
    let mut m = <Hmac<Sha256> as Mac>::new_from_slice(key).expect("hmac accepts any key length");
    m.update(msg);
    m.verify_slice(&given).is_ok()
}

/// A deterministic, unambiguous byte encoding for signing and hashing:
/// every field is a label and a length-prefixed value, so no two different
/// field lists encode alike.
#[derive(Default)]
pub struct Canon(Vec<u8>);

impl Canon {
    pub fn new(domain: &str) -> Canon {
        let mut c = Canon(Vec::with_capacity(256));
        c.bytes(domain.as_bytes());
        c
    }

    pub fn bytes(&mut self, b: &[u8]) -> &mut Self {
        self.0.extend_from_slice(&(b.len() as u64).to_be_bytes());
        self.0.extend_from_slice(b);
        self
    }

    pub fn str(&mut self, s: &str) -> &mut Self {
        self.bytes(s.as_bytes())
    }

    pub fn num(&mut self, n: f64) -> &mut Self {
        self.bytes(&n.to_bits().to_be_bytes())
    }

    pub fn list<'a>(&mut self, items: impl IntoIterator<Item = &'a String>) -> &mut Self {
        let items: Vec<&String> = items.into_iter().collect();
        self.bytes(&(items.len() as u64).to_be_bytes());
        for i in items {
            self.str(i);
        }
        self
    }

    pub fn finish(&self) -> &[u8] {
        &self.0
    }
}

/// A device's public keys, hex.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PublicKeys {
    /// ed25519 verifying key.
    pub sign: String,
    /// X25519 public key.
    pub kx: String,
}

/// A device's secret keys. Never printed, never on the wire.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceSecret {
    sign: String,
    kx: String,
}

impl std::fmt::Debug for DeviceSecret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "DeviceSecret(<redacted>)")
    }
}

impl DeviceSecret {
    pub fn generate() -> DeviceSecret {
        DeviceSecret {
            sign: hex::encode(random32()),
            kx: hex::encode(random32()),
        }
    }

    /// Deterministic keys from a seed: tests and the simulation only.
    pub fn from_seed(seed: &[u8]) -> DeviceSecret {
        DeviceSecret {
            sign: sha256_hex(&[b"sign|".as_slice(), seed].concat()),
            kx: sha256_hex(&[b"kx|".as_slice(), seed].concat()),
        }
    }

    fn signing(&self) -> SigningKey {
        SigningKey::from_bytes(&decode32(&self.sign).unwrap_or([0; 32]))
    }

    fn agreement(&self) -> StaticSecret {
        StaticSecret::from(decode32(&self.kx).unwrap_or([0; 32]))
    }

    pub fn public(&self) -> PublicKeys {
        PublicKeys {
            sign: hex::encode(self.signing().verifying_key().to_bytes()),
            kx: hex::encode(XPublic::from(&self.agreement()).to_bytes()),
        }
    }

    /// ed25519 signature over `msg`, hex.
    pub fn sign(&self, msg: &[u8]) -> String {
        hex::encode(self.signing().sign(msg).to_bytes())
    }

    /// Open an envelope addressed to this device's X25519 key.
    pub fn open_envelope(&self, context: &[u8], env: &Envelope) -> Option<Vec<u8>> {
        let eph = XPublic::from(decode32(&env.eph)?);
        let shared = self.agreement().diffie_hellman(&eph);
        let key = envelope_key(shared.as_bytes(), &env.eph, &self.public().kx, context);
        open(&key, context, &env.sealed)
    }
}

pub fn decode32(h: &str) -> Option<[u8; 32]> {
    let v = hex::decode(h).ok()?;
    v.try_into().ok()
}

/// Check an ed25519 signature (hex) by a verifying key (hex).
pub fn verify(sign_pk: &str, msg: &[u8], sig: &str) -> bool {
    let Some(pk) = decode32(sign_pk) else {
        return false;
    };
    let Ok(pk) = VerifyingKey::from_bytes(&pk) else {
        return false;
    };
    let Ok(sig) = hex::decode(sig) else {
        return false;
    };
    let Ok(sig) = Signature::from_slice(&sig) else {
        return false;
    };
    pk.verify_strict(msg, &sig).is_ok()
}

/// A secret sealed with ChaCha20-Poly1305 under a 32-byte key.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Sealed {
    /// 12 bytes, hex.
    pub nonce: String,
    /// Ciphertext and tag, base64.
    pub ct: String,
}

pub fn seal(key: &[u8; 32], aad: &[u8], plaintext: &[u8]) -> Sealed {
    use rand::RngCore;
    let mut nonce = [0u8; 12];
    rand::rng().fill_bytes(&mut nonce);
    let cipher = ChaCha20Poly1305::new(Key::from_slice(key));
    let ct = cipher
        .encrypt(
            Nonce::from_slice(&nonce),
            Payload {
                msg: plaintext,
                aad,
            },
        )
        .expect("chacha20poly1305 encrypts any length we use");
    Sealed {
        nonce: hex::encode(nonce),
        ct: base64::engine::general_purpose::STANDARD.encode(ct),
    }
}

pub fn open(key: &[u8; 32], aad: &[u8], sealed: &Sealed) -> Option<Vec<u8>> {
    let nonce = hex::decode(&sealed.nonce).ok()?;
    if nonce.len() != 12 {
        return None;
    }
    let ct = base64::engine::general_purpose::STANDARD
        .decode(&sealed.ct)
        .ok()?;
    let cipher = ChaCha20Poly1305::new(Key::from_slice(key));
    cipher
        .decrypt(Nonce::from_slice(&nonce), Payload { msg: &ct, aad })
        .ok()
}

/// A secret for one device: an ephemeral X25519 key, and the secret sealed
/// under `HKDF(shared, eph || recipient || context)`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Envelope {
    pub eph: String,
    pub sealed: Sealed,
}

fn envelope_key(shared: &[u8], eph: &str, recipient: &str, context: &[u8]) -> [u8; 32] {
    let hk = Hkdf::<Sha256>::new(Some(b"hocket-envelope-v1"), shared);
    let mut info = Canon::new("envelope");
    info.str(eph).str(recipient).bytes(context);
    let mut out = [0u8; 32];
    hk.expand(info.finish(), &mut out)
        .expect("32 bytes is a valid HKDF output length");
    out
}

/// Seal `secret` for the device whose X25519 public key is `to_kx`.
pub fn envelope(to_kx: &str, context: &[u8], secret: &[u8]) -> Option<Envelope> {
    let to = XPublic::from(decode32(to_kx)?);
    let eph_secret = StaticSecret::from(random32());
    let eph = hex::encode(XPublic::from(&eph_secret).to_bytes());
    let shared = eph_secret.diffie_hellman(&to);
    let key = envelope_key(shared.as_bytes(), &eph, to_kx, context);
    Some(Envelope {
        eph,
        sealed: seal(&key, context, secret),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sign_and_verify() {
        let s = DeviceSecret::generate();
        let p = s.public();
        let sig = s.sign(b"hello");
        assert!(verify(&p.sign, b"hello", &sig));
        assert!(!verify(&p.sign, b"hellp", &sig));
        let other = DeviceSecret::generate().public();
        assert!(!verify(&other.sign, b"hello", &sig));
        assert!(!verify(&p.sign, b"hello", "zz"));
        assert!(!verify("nothex", b"hello", &sig));
    }

    #[test]
    fn envelopes_open_only_for_their_recipient_and_context() {
        let a = DeviceSecret::generate();
        let b = DeviceSecret::generate();
        let env = envelope(&a.public().kx, b"ctx", b"group key").unwrap();
        assert_eq!(a.open_envelope(b"ctx", &env).unwrap(), b"group key");
        assert!(a.open_envelope(b"other", &env).is_none());
        assert!(b.open_envelope(b"ctx", &env).is_none());
    }

    #[test]
    fn sealed_boxes_bind_their_aad() {
        let k = random32();
        let s = seal(&k, b"aad", b"secret");
        assert_eq!(open(&k, b"aad", &s).unwrap(), b"secret");
        assert!(open(&k, b"bad", &s).is_none());
        assert!(open(&random32(), b"aad", &s).is_none());
    }

    #[test]
    fn canon_is_unambiguous() {
        let mut a = Canon::new("d");
        a.str("ab").str("c");
        let mut b = Canon::new("d");
        b.str("a").str("bc");
        assert_ne!(a.finish(), b.finish());
    }

    #[test]
    fn secrets_are_redacted() {
        assert_eq!(
            format!("{:?}", DeviceSecret::generate()),
            "DeviceSecret(<redacted>)"
        );
    }

    #[test]
    fn seeded_keys_are_stable() {
        assert_eq!(
            DeviceSecret::from_seed(b"x").public(),
            DeviceSecret::from_seed(b"x").public()
        );
    }
}
