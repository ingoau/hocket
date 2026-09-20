//! Opt-in external lyrics sources. Off by default (setting
//! `lyrics.external.enabled`): fetching reveals what you are listening to.
//!
//! Network goes through [`LyricsHttp`] so tests and the simulation harness
//! can fake it. The one implementation is [`LrclibProvider`] against
//! <https://lrclib.net> (`GET /api/get?artist_name&track_name&album_name&duration`
//! → `{plainLyrics, syncedLyrics, instrumental, …}`; 404 `TrackNotFound`).

use futures::future::BoxFuture;
use serde::Deserialize;

use crate::api::{Lyrics, LyricsSource};

use super::adapt::from_plain_text;
use super::lrc::{lrc_to_lyrics, parse_lrc};

/// What we know about the track to look up.
#[derive(Debug, Clone, PartialEq)]
pub struct LyricsRequest {
    pub track_id: String,
    pub title: String,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub duration_ms: Option<u32>,
}

#[derive(Debug, thiserror::Error)]
pub enum LyricsError {
    #[error("network: {0}")]
    Network(String),
    #[error("http {status}: {body}")]
    Http { status: u16, body: String },
    #[error("bad response: {0}")]
    Decode(String),
    #[error("external lyrics are disabled")]
    Disabled,
}

/// HTTP GET returning the body; `Err(Http{404})` for not found.
pub trait LyricsHttp: Send + Sync {
    fn get(&self, url: &str) -> BoxFuture<'_, Result<String, LyricsError>>;
}

/// An external lyrics source. `Ok(None)` means "not found", which callers
/// negative-cache.
pub trait ExternalLyricsProvider: Send + Sync {
    /// Stable id for cache keys and the settings UI (`lrclib`).
    fn id(&self) -> &'static str;
    fn fetch<'a>(
        &'a self,
        req: &'a LyricsRequest,
    ) -> BoxFuture<'a, Result<Option<Lyrics>, LyricsError>>;
}

/// LRCLIB (<https://lrclib.net/docs>). Synced LRC → line tier, otherwise
/// plain text → unsynced. Instrumentals report `None`.
pub struct LrclibProvider<H: LyricsHttp> {
    http: H,
    base_url: String,
}

impl<H: LyricsHttp> LrclibProvider<H> {
    pub const DEFAULT_BASE_URL: &'static str = "https://lrclib.net";

    pub fn new(http: H) -> Self {
        Self::with_base_url(http, Self::DEFAULT_BASE_URL)
    }

    pub fn with_base_url(http: H, base_url: &str) -> Self {
        Self {
            http,
            base_url: base_url.trim_end_matches('/').to_string(),
        }
    }

    /// The exact URL a request maps to (exposed for tests).
    pub fn url_for(&self, req: &LyricsRequest) -> String {
        let mut url =
            url::Url::parse(&format!("{}/api/get", self.base_url)).expect("static base url");
        {
            let mut q = url.query_pairs_mut();
            q.append_pair("track_name", &req.title);
            q.append_pair("artist_name", req.artist.as_deref().unwrap_or(""));
            if let Some(album) = req.album.as_deref().filter(|a| !a.is_empty()) {
                q.append_pair("album_name", album);
            }
            if let Some(d) = req.duration_ms {
                q.append_pair("duration", &(d / 1000).to_string());
            }
        }
        url.to_string()
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LrclibRecord {
    #[serde(default)]
    instrumental: bool,
    #[serde(default)]
    plain_lyrics: Option<String>,
    #[serde(default)]
    synced_lyrics: Option<String>,
}

impl<H: LyricsHttp> ExternalLyricsProvider for LrclibProvider<H> {
    fn id(&self) -> &'static str {
        "lrclib"
    }

    fn fetch<'a>(
        &'a self,
        req: &'a LyricsRequest,
    ) -> BoxFuture<'a, Result<Option<Lyrics>, LyricsError>> {
        Box::pin(async move {
            if req.title.trim().is_empty() {
                return Ok(None);
            }
            let url = self.url_for(req);
            let body = match self.http.get(&url).await {
                Ok(b) => b,
                Err(LyricsError::Http { status: 404, .. }) => return Ok(None),
                Err(e) => return Err(e),
            };
            let record: LrclibRecord =
                serde_json::from_str(&body).map_err(|e| LyricsError::Decode(e.to_string()))?;
            if record.instrumental {
                return Ok(None);
            }
            if let Some(synced) = record
                .synced_lyrics
                .as_deref()
                .filter(|s| !s.trim().is_empty())
            {
                let doc = parse_lrc(synced);
                if let Some(l) = lrc_to_lyrics(&req.track_id, &doc, LyricsSource::External) {
                    return Ok(Some(l));
                }
            }
            Ok(record
                .plain_lyrics
                .as_deref()
                .and_then(|p| from_plain_text(&req.track_id, p, LyricsSource::External)))
        })
    }
}

/// `reqwest`-backed HTTP for production. Sends a descriptive User-Agent as
/// LRCLIB asks.
pub struct ReqwestLyricsHttp {
    client: reqwest::Client,
}

impl ReqwestLyricsHttp {
    pub fn new(app_version: &str) -> Result<Self, LyricsError> {
        let client = reqwest::Client::builder()
            .user_agent(format!(
                "Hocket/{app_version} (https://github.com/ingoau/hocket)"
            ))
            .timeout(std::time::Duration::from_secs(15))
            .build()
            .map_err(|e| LyricsError::Network(e.to_string()))?;
        Ok(Self { client })
    }
}

impl LyricsHttp for ReqwestLyricsHttp {
    fn get(&self, url: &str) -> BoxFuture<'_, Result<String, LyricsError>> {
        let url = url.to_string();
        Box::pin(async move {
            let resp = self
                .client
                .get(&url)
                .send()
                .await
                .map_err(|e| LyricsError::Network(e.to_string()))?;
            let status = resp.status().as_u16();
            let body = resp
                .text()
                .await
                .map_err(|e| LyricsError::Network(e.to_string()))?;
            if (200..300).contains(&status) {
                Ok(body)
            } else {
                Err(LyricsError::Http { status, body })
            }
        })
    }
}
