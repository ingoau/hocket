//! Choosing a stream: the `player` client fallback chain, the format choice
//! (AAC: itag 141 for Premium through the JS host, else 140) and the memory
//! of which client last worked (`docs/providers.md`, Decisions).
//!
//! Order: `VISIONOS`, `ANDROID_VR`, `IOS`, then the web client through the
//! platform's [`JsHost`] (signature, `n` and PO token). A Premium account
//! tries the web client first, for 141, and falls back to the chain for 140.
//! The first client that answers a usable AAC URL wins and is tried first
//! next time; a client whose URL was refused (403 mid-stream) is skipped for
//! [`BROKEN_FOR_MS`].

use std::collections::HashMap;
use std::sync::Arc;

use futures::future::BoxFuture;
use parking_lot::Mutex;

use super::client::{InnerTube, WebPlayerParams};
use super::clients::{ClientKind, ClientProfile, PLAYER_CHAIN, WEB_REMIX};
use super::parse::{parse_player, AudioFormat, PlayerInfo};
use crate::provider::{ProviderError, ProviderResult};
use crate::util::Clock;

/// How long a client whose URLs were refused is skipped.
pub const BROKEN_FOR_MS: f64 = 30.0 * 60_000.0;
/// Re-resolve this long before a URL's stated expiry.
pub const EXPIRY_MARGIN_MS: f64 = 10.0 * 60_000.0;

/// AAC-LC 128 kbit/s: the baseline every client offers.
pub const ITAG_AAC_128: u32 = 140;
/// AAC-LC 256 kbit/s: Premium only.
pub const ITAG_AAC_256: u32 = 141;
/// Set (to anything) to skip every client but the web one when a JS host
/// is there: a developer switch for checking the JS path live.
pub const FORCE_WEB_ENV: &str = "HOCKET_YTM_FORCE_WEB";

/// What the platform's JS host is asked (a hidden WebView on Android, a
/// hidden `BrowserWindow` on desktop).
#[derive(Debug, Clone, PartialEq)]
pub enum JsRequest {
    /// The player's signature timestamp and a PO token for `video_id`.
    Prepare {
        video_id: String,
        visitor_data: String,
    },
    /// Sign (`s`/`sp`) and `n`-transform these formats' URLs. Each entry is
    /// a plain URL with an `n` parameter or a `signatureCipher` query string.
    Decipher {
        video_id: String,
        formats: Vec<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct JsAnswer {
    pub signature_timestamp: Option<u32>,
    pub po_token: Option<String>,
    /// For `Decipher`: one playable URL per format asked, in order.
    pub urls: Vec<String>,
}

/// The platform callback for the web client's puzzles.
pub trait JsHost: Send + Sync + 'static {
    fn solve(&self, req: JsRequest) -> BoxFuture<'static, Result<JsAnswer, String>>;
}

/// A resolved googlevideo URL.
#[derive(Debug, Clone, PartialEq)]
pub struct ChosenStream {
    pub url: String,
    pub client: ClientKind,
    pub user_agent: &'static str,
    pub itag: u32,
    pub mime: String,
    pub content_length: Option<u64>,
    pub loudness_db: Option<f64>,
    pub expires_at: f64,
    pub info: PlayerInfo,
}

#[derive(Default)]
struct ChainMemory {
    /// The client that answered last.
    last_good: Option<ClientKind>,
    broken: HashMap<ClientKind, f64>,
}

/// The chain's memory, shared by every resolve of one account.
pub struct StreamChooser {
    clock: Arc<dyn Clock>,
    memory: Mutex<ChainMemory>,
}

impl StreamChooser {
    pub fn new(clock: Arc<dyn Clock>) -> StreamChooser {
        StreamChooser {
            clock,
            memory: Mutex::default(),
        }
    }

    /// The order to try clients in now.
    pub fn order(&self, premium: bool, js: bool) -> Vec<ClientProfile> {
        if js && std::env::var_os(FORCE_WEB_ENV).is_some() {
            // The live check of the JS path (desktop e2e `ytm-js-live`).
            return vec![WEB_REMIX];
        }
        let now = self.clock.now_ms();
        let mut m = self.memory.lock();
        m.broken.retain(|_, until| *until > now);
        let mut order: Vec<ClientProfile> = PLAYER_CHAIN
            .iter()
            .copied()
            .filter(|c| !c.needs_js || js)
            .filter(|c| !m.broken.contains_key(&c.kind))
            .collect();
        if premium && js && !m.broken.contains_key(&ClientKind::WebRemix) {
            // 141 comes only through the web client.
            order.retain(|c| c.kind != ClientKind::WebRemix);
            order.insert(0, WEB_REMIX);
        } else if let Some(last) = m.last_good {
            if let Some(i) = order.iter().position(|c| c.kind == last) {
                let c = order.remove(i);
                order.insert(0, c);
            }
        }
        order
    }

    /// A client's URL was refused: skip it for a while.
    pub fn mark_broken(&self, kind: ClientKind) {
        let until = self.clock.now_ms() + BROKEN_FOR_MS;
        let mut m = self.memory.lock();
        m.broken.insert(kind, until);
        if m.last_good == Some(kind) {
            m.last_good = None;
        }
    }

    fn mark_good(&self, kind: ClientKind) {
        self.memory.lock().last_good = Some(kind);
    }

    pub fn last_good(&self) -> Option<ClientKind> {
        self.memory.lock().last_good
    }

    /// Walk the chain for `video_id`.
    pub async fn choose(
        &self,
        it: &InnerTube,
        js: Option<&Arc<dyn JsHost>>,
        premium: bool,
        video_id: &str,
    ) -> ProviderResult<ChosenStream> {
        let mut last_err: Option<ProviderError> = None;
        for profile in self.order(premium, js.is_some()) {
            let r = if profile.needs_js {
                match js {
                    Some(js) => self.try_web(it, js.as_ref(), premium, video_id).await,
                    None => continue,
                }
            } else {
                self.try_plain(it, profile, video_id).await
            };
            match r {
                Ok(s) => {
                    tracing::debug!(target: "hocket::ytm", client = ?s.client, itag = s.itag, "stream resolved");
                    self.mark_good(profile.kind);
                    return Ok(s);
                }
                Err(e) => {
                    tracing::debug!(target: "hocket::ytm", client = profile.name, error = %e, "player client gave no usable stream");
                    // A video that isn't playable anywhere stops the walk.
                    let stop = matches!(e, ProviderError::NotFound(_));
                    last_err = Some(e);
                    if stop {
                        break;
                    }
                }
            }
        }
        Err(last_err.unwrap_or_else(|| ProviderError::Unsupported("no player client".into())))
    }

    async fn try_plain(
        &self,
        it: &InnerTube,
        profile: ClientProfile,
        video_id: &str,
    ) -> ProviderResult<ChosenStream> {
        let v = it.player(profile, video_id, None).await?;
        let info = parse_player(&v);
        check_playable(&info, it)?;
        let f = pick_format(&info.formats, false)
            .filter(|f| f.url.is_some())
            .ok_or_else(|| {
                ProviderError::Unsupported(if info.sabr_only {
                    format!("{}: SABR only", profile.name)
                } else {
                    format!("{}: no AAC URL", profile.name)
                })
            })?
            .clone();
        Ok(self.chosen(profile, f.url.clone().unwrap_or_default(), &f, info))
    }

    async fn try_web(
        &self,
        it: &InnerTube,
        js: &dyn JsHost,
        premium: bool,
        video_id: &str,
    ) -> ProviderResult<ChosenStream> {
        let visitor_data = it.visitor_data().await?;
        let prep = js
            .solve(JsRequest::Prepare {
                video_id: video_id.into(),
                visitor_data,
            })
            .await
            .map_err(ProviderError::Unsupported)?;
        let params = WebPlayerParams {
            signature_timestamp: prep.signature_timestamp,
            po_token: prep.po_token.clone(),
        };
        let v = it.player(WEB_REMIX, video_id, Some(&params)).await?;
        let info = parse_player(&v);
        check_playable(&info, it)?;
        let f = pick_format(&info.formats, premium)
            .ok_or_else(|| ProviderError::Unsupported("web: no AAC format".into()))?
            .clone();
        let raw = f
            .url
            .clone()
            .or_else(|| f.cipher.clone())
            .ok_or_else(|| ProviderError::Unsupported("web: SABR only".into()))?;
        let answer = js
            .solve(JsRequest::Decipher {
                video_id: video_id.into(),
                formats: vec![raw],
            })
            .await
            .map_err(ProviderError::Unsupported)?;
        let mut url =
            answer.urls.into_iter().next().ok_or_else(|| {
                ProviderError::Unsupported("web: the JS host signed nothing".into())
            })?;
        if let Some(pot) = prep.po_token.as_deref().filter(|_| !url.contains("&pot=")) {
            url.push_str("&pot=");
            url.push_str(&urlencode(pot));
        }
        Ok(self.chosen(WEB_REMIX, url, &f, info))
    }

    fn chosen(
        &self,
        profile: ClientProfile,
        url: String,
        f: &AudioFormat,
        info: PlayerInfo,
    ) -> ChosenStream {
        let now = self.clock.now_ms();
        let expires_in = info
            .expires_in_s
            .or_else(|| expire_param(&url).map(|e| e.saturating_sub((now / 1000.0) as u64)));
        ChosenStream {
            url,
            client: profile.kind,
            user_agent: profile.user_agent,
            itag: f.itag,
            mime: f.mime.clone(),
            content_length: f.content_length,
            loudness_db: f.loudness_db,
            expires_at: now + expires_in.unwrap_or(6 * 3600) as f64 * 1000.0,
            info,
        }
    }
}

fn check_playable(info: &PlayerInfo, it: &InnerTube) -> ProviderResult<()> {
    match info.status.as_str() {
        "OK" => Ok(()),
        "LOGIN_REQUIRED" => {
            // "Sign in to confirm you're not a bot": a fresh visitor id may help.
            it.reset_visitor();
            Err(ProviderError::Auth(
                info.reason
                    .clone()
                    .unwrap_or_else(|| "login required".into()),
            ))
        }
        "ERROR" => Err(ProviderError::NotFound(
            info.reason
                .clone()
                .unwrap_or_else(|| "video unavailable".into()),
        )),
        other => Err(ProviderError::Forbidden(format!(
            "{other}: {}",
            info.reason.clone().unwrap_or_default()
        ))),
    }
}

/// The AAC format to play: 141 when wanted and offered, else 140 (never a
/// DRC variant). Opus is never chosen: AAC decodes everywhere.
pub fn pick_format(formats: &[AudioFormat], want_141: bool) -> Option<&AudioFormat> {
    let find = |itag: u32| formats.iter().find(|f| f.itag == itag && !f.drc);
    want_141
        .then(|| find(ITAG_AAC_256))
        .flatten()
        .or_else(|| find(ITAG_AAC_128))
}

fn expire_param(url: &str) -> Option<u64> {
    url.split(['?', '&'])
        .find_map(|p| p.strip_prefix("expire="))
        .and_then(|v| v.parse().ok())
}

fn urlencode(s: &str) -> String {
    url::form_urlencoded::byte_serialize(s.as_bytes()).collect()
}
