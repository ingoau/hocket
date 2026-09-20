//! Authentication material and the per-request auth parameters.
//!
//! Only two schemes are ever used: token+salt (`t`/`s`, MD5 of password and a
//! fresh random salt per request) and OpenSubsonic `apiKey`. Plaintext `p=`
//! and Navidrome's `jwt=` are deliberately not representable here.

use std::fmt;

use md5::{Digest, Md5};
use rand::RngCore;

/// A secret held in memory (password or API key). Its bytes are overwritten
/// with zeros when dropped so a password from the platform keystore does not
/// linger on the heap.
#[derive(Clone, PartialEq, Eq)]
pub struct Credential {
    bytes: Vec<u8>,
}

impl Credential {
    pub fn new(secret: impl Into<String>) -> Self {
        Credential {
            bytes: secret.into().into_bytes(),
        }
    }

    /// Expose the secret. Callers must not copy it into long-lived storage.
    pub fn expose(&self) -> &str {
        // Constructed from a String, so always valid UTF-8.
        std::str::from_utf8(&self.bytes).unwrap_or("")
    }

    pub fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }
}

impl Drop for Credential {
    fn drop(&mut self) {
        // Volatile writes so the optimiser can't elide the wipe.
        for b in self.bytes.iter_mut() {
            // SAFETY: `b` is a valid, aligned, exclusively borrowed u8.
            unsafe { std::ptr::write_volatile(b, 0) };
        }
        std::sync::atomic::compiler_fence(std::sync::atomic::Ordering::SeqCst);
    }
}

impl fmt::Debug for Credential {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Credential(<redacted>)")
    }
}

/// How a request proves who it is.
#[derive(Debug, Clone)]
pub enum AuthMode {
    /// Token+salt computed per request from the password held in memory.
    Password {
        username: String,
        password: Credential,
    },
    /// OpenSubsonic `apiKeyAuthentication`: no password retained at all.
    ApiKey { api_key: Credential },
}

impl AuthMode {
    pub fn username(&self) -> Option<&str> {
        match self {
            AuthMode::Password { username, .. } => Some(username),
            AuthMode::ApiKey { .. } => None,
        }
    }
}

/// Subsonic protocol version we speak.
pub const API_VERSION: &str = "1.16.1";
/// Client name sent as `c=`.
pub const CLIENT_NAME: &str = "hocket";

/// Compute `md5(password + salt)` as lowercase hex, exactly as the server does.
pub fn token(password: &str, salt: &str) -> String {
    let mut h = Md5::new();
    h.update(password.as_bytes());
    h.update(salt.as_bytes());
    hex::encode(h.finalize())
}

/// Fresh random salt: 8 bytes of entropy as 16 hex chars.
pub fn random_salt() -> String {
    let mut buf = [0u8; 8];
    rand::rng().fill_bytes(&mut buf);
    hex::encode(buf)
}

/// Build the auth + protocol query parameters for one request. A new salt is
/// generated every call for the password scheme.
pub fn auth_params(mode: &AuthMode) -> Vec<(String, String)> {
    auth_params_with_salt(mode, &random_salt())
}

/// Same as [`auth_params`] with an explicit salt (tests).
pub fn auth_params_with_salt(mode: &AuthMode, salt: &str) -> Vec<(String, String)> {
    let mut p = vec![
        ("v".to_string(), API_VERSION.to_string()),
        ("c".to_string(), CLIENT_NAME.to_string()),
        ("f".to_string(), "json".to_string()),
    ];
    match mode {
        AuthMode::Password { username, password } => {
            p.push(("u".to_string(), username.clone()));
            p.push(("t".to_string(), token(password.expose(), salt)));
            p.push(("s".to_string(), salt.to_string()));
        }
        AuthMode::ApiKey { api_key } => {
            p.push(("apiKey".to_string(), api_key.expose().to_string()));
        }
    }
    p
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_matches_subsonic_reference() {
        // From the Subsonic API docs: password "sesame", salt "c19b2d" -> 26719a1196d2a940705a59634eb18eab
        assert_eq!(
            token("sesame", "c19b2d"),
            "26719a1196d2a940705a59634eb18eab"
        );
    }

    #[test]
    fn password_mode_never_sends_plaintext() {
        let mode = AuthMode::Password {
            username: "alice".into(),
            password: Credential::new("sesame"),
        };
        let params = auth_params_with_salt(&mode, "c19b2d");
        let map: std::collections::HashMap<_, _> = params.into_iter().collect();
        assert_eq!(map["u"], "alice");
        assert_eq!(map["t"], "26719a1196d2a940705a59634eb18eab");
        assert_eq!(map["s"], "c19b2d");
        assert_eq!(map["v"], "1.16.1");
        assert_eq!(map["c"], "hocket");
        assert_eq!(map["f"], "json");
        assert!(!map.contains_key("p"));
        assert!(!map.contains_key("jwt"));
        assert!(!map.contains_key("apiKey"));
    }

    #[test]
    fn api_key_mode_sends_only_key() {
        let mode = AuthMode::ApiKey {
            api_key: Credential::new("k123"),
        };
        let params = auth_params(&mode);
        let map: std::collections::HashMap<_, _> = params.into_iter().collect();
        assert_eq!(map["apiKey"], "k123");
        assert!(!map.contains_key("u"));
        assert!(!map.contains_key("t"));
        assert!(!map.contains_key("s"));
    }

    #[test]
    fn salt_is_fresh_per_call() {
        assert_ne!(random_salt(), random_salt());
        assert_eq!(random_salt().len(), 16);
    }

    #[test]
    fn credential_debug_is_redacted() {
        let c = Credential::new("hunter2");
        assert_eq!(format!("{c:?}"), "Credential(<redacted>)");
        assert_eq!(c.expose(), "hunter2");
    }
}
