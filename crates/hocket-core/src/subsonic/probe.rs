//! Capability probe: `ping` + `getOpenSubsonicExtensions` + native-API
//! login/keepalive, folded into [`api::ServerCapabilities`]. Run on connect
//! and stored per server; nothing is ever feature-detected by trying an
//! action and catching the failure.

use crate::api::ServerCapabilities;

use super::auth::AuthMode;
use super::client::Client;
use super::{SubsonicApi, SubsonicError, SubsonicResult};

/// Minimum Navidrome version (design: 0.63.0).
pub const FLOOR: (u32, u32, u32) = (0, 63, 0);

/// Parse a Navidrome `serverVersion` such as `0.63.1 (abcd1234)` or
/// `v0.63.1-SNAPSHOT` into `(major, minor, patch)`.
pub fn parse_version(s: &str) -> Option<(u32, u32, u32)> {
    let s = s.trim().trim_start_matches('v');
    let core: String = s
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '.')
        .collect();
    let mut it = core
        .split('.')
        .filter(|p| !p.is_empty())
        .map(|p| p.parse::<u32>().ok());
    let major = it.next()??;
    let minor = it.next().flatten().unwrap_or(0);
    let patch = it.next().flatten().unwrap_or(0);
    Some((major, minor, patch))
}

pub fn meets_floor(version: Option<&str>) -> bool {
    version.and_then(parse_version).is_some_and(|v| v >= FLOOR)
}

impl Client {
    /// Probe the server. Fails only when `ping` fails (auth/network); missing
    /// extensions or an absent native API just leave flags false.
    pub async fn probe(&self) -> SubsonicResult<ServerCapabilities> {
        let ping = self.ping().await?;
        let mut caps = ServerCapabilities {
            server_version: ping.server_version.clone().filter(|v| !v.is_empty()),
            open_subsonic: ping.open_subsonic,
            ..Default::default()
        };
        caps.meets_floor = meets_floor(caps.server_version.as_deref());
        if !ping
            .server_type
            .as_deref()
            .is_some_and(|t| t.eq_ignore_ascii_case("navidrome"))
        {
            tracing::warn!(server_type = ?ping.server_type, "server is not navidrome; floor check may be wrong");
        }

        if caps.open_subsonic {
            match self.open_subsonic_extensions().await {
                Ok(exts) => {
                    caps.extensions = exts.iter().map(|e| e.name.clone()).collect();
                    let has = |n: &str| exts.iter().any(|e| e.name == n);
                    caps.transcode_offset = has("transcodeOffset");
                    caps.form_post = has("formPost");
                    caps.song_lyrics = has("songLyrics");
                    caps.sonic_similarity = has("sonicSimilarity");
                    caps.api_key_authentication = has("apiKeyAuthentication");
                    caps.transcoding_extension = has("transcoding");
                }
                Err(e) if !matches!(e, SubsonicError::Auth(_)) => {
                    tracing::warn!(error = %e, "getOpenSubsonicExtensions failed; assuming none");
                }
                Err(e) => return Err(e),
            }
        }

        caps.native_api = match self.auth_mode() {
            AuthMode::Password { .. } => match super::native::login(self).await {
                Ok(_) => super::native::keepalive(self).await.unwrap_or(false),
                Err(e) => {
                    tracing::info!(error = %e, "native api unavailable");
                    false
                }
            },
            AuthMode::ApiKey { .. } => false,
        };

        self.set_capabilities(caps.clone());
        Ok(caps)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_parsing_and_floor() {
        assert_eq!(parse_version("0.63.1 (abcd1234)"), Some((0, 63, 1)));
        assert_eq!(
            parse_version("v0.64.0-SNAPSHOT (deadbeef)"),
            Some((0, 64, 0))
        );
        assert_eq!(parse_version("0.62.9"), Some((0, 62, 9)));
        assert_eq!(parse_version(""), None);
        assert!(meets_floor(Some("0.63.0")));
        assert!(meets_floor(Some("1.0.0")));
        assert!(!meets_floor(Some("0.62.9")));
        assert!(!meets_floor(None));
    }
}
