//! Google sign-in as InnerTube sees it: the cookies a platform captured from
//! its sign-in WebView / BrowserWindow, and the `SAPISIDHASH` authorization
//! derived from them.

use sha1::{Digest, Sha1};

/// A captured cookie jar for `music.youtube.com`, as one `Cookie:` header.
#[derive(Clone, PartialEq, Eq)]
pub struct Cookies(String);

impl std::fmt::Debug for Cookies {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Cookies({} names)", self.names().len())
    }
}

/// The cookies a usable sign-in must include.
pub const REQUIRED: [&str; 1] = ["SAPISID"];

impl Cookies {
    /// From a `Cookie:` header (or a `name=value; …` string). `None` when it
    /// lacks `SAPISID` (`__Secure-3PAPISID` stands in for it): not signed in.
    pub fn parse(header: &str) -> Option<Cookies> {
        let c = Cookies(
            header
                .split(';')
                .map(str::trim)
                .filter(|p| p.contains('='))
                .collect::<Vec<_>>()
                .join("; "),
        );
        c.sapisid().is_some().then_some(c)
    }

    pub fn header(&self) -> &str {
        &self.0
    }

    pub fn get(&self, name: &str) -> Option<&str> {
        self.0.split("; ").find_map(|p| {
            let (k, v) = p.split_once('=')?;
            (k == name).then_some(v)
        })
    }

    pub fn names(&self) -> Vec<&str> {
        self.0
            .split("; ")
            .filter_map(|p| p.split_once('=').map(|(k, _)| k))
            .collect()
    }

    fn sapisid(&self) -> Option<&str> {
        self.get("SAPISID")
            .or_else(|| self.get("__Secure-3PAPISID"))
            .filter(|v| !v.is_empty())
    }

    /// `Authorization: SAPISIDHASH <ts>_<sha1(ts SAPISID origin)>`, followed
    /// by the `SAPISID1PHASH` / `SAPISID3PHASH` forms when the jar has
    /// `__Secure-1PAPISID` / `__Secure-3PAPISID` (as the web app and yt-dlp send).
    pub fn authorization(&self, origin: &str, now_s: u64) -> Option<String> {
        let sapisid = self.sapisid()?;
        let mut parts = vec![format!(
            "SAPISIDHASH {now_s}_{}",
            sapisid_hash(now_s, sapisid, origin)
        )];
        for (scheme, name) in [
            ("SAPISID1PHASH", "__Secure-1PAPISID"),
            ("SAPISID3PHASH", "__Secure-3PAPISID"),
        ] {
            if let Some(v) = self.get(name).filter(|v| !v.is_empty()) {
                parts.push(format!(
                    "{scheme} {now_s}_{}",
                    sapisid_hash(now_s, v, origin)
                ));
            }
        }
        Some(parts.join(" "))
    }
}

pub fn sapisid_hash(ts: u64, sapisid: &str, origin: &str) -> String {
    let mut h = Sha1::new();
    h.update(format!("{ts} {sapisid} {origin}").as_bytes());
    hex::encode(h.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_cookies_and_hashes_like_the_web_client() {
        assert!(Cookies::parse("PREF=f6=8; YSC=abc").is_none());
        let c = Cookies::parse(" SID=x ;SAPISID=abc123/def; HSID=y;junk").unwrap();
        assert_eq!(c.header(), "SID=x; SAPISID=abc123/def; HSID=y");
        assert_eq!(c.get("SAPISID"), Some("abc123/def"));
        // sha1("1700000000 abc123/def https://music.youtube.com")
        let auth = c
            .authorization("https://music.youtube.com", 1_700_000_000)
            .unwrap();
        assert_eq!(
            auth,
            format!(
                "SAPISIDHASH 1700000000_{}",
                sapisid_hash(1_700_000_000, "abc123/def", "https://music.youtube.com")
            )
        );
        assert_eq!(auth.len(), "SAPISIDHASH 1700000000_".len() + 40);
        assert!(!format!("{c:?}").contains("abc123"));
        let secure = Cookies::parse("__Secure-3PAPISID=zz").unwrap();
        assert_eq!(
            secure.authorization("o", 1).unwrap(),
            format!(
                "SAPISIDHASH 1_{h} SAPISID3PHASH 1_{h}",
                h = sapisid_hash(1, "zz", "o")
            )
        );
    }
}
