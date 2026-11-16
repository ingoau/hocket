//! Choosing a stream: the `player` client fallback chain, the format choice
//! (AAC: itag 141 for Premium through the JS host, else 140) and the memory
//! of which client last worked (`docs/providers.md`, Decisions).
//!
//! Order: `VISIONOS`, `ANDROID_VR`, `IOS`, then the JS clients the
//! [`JsHost`] offers: the web client on the platforms' hidden pages
//! (signature, `n` and PO token), the TV app on the coordinator's embedded
//! engine (signature and `n`; it needs no PO token). A Premium account tries
//! the web client first, for 141, and falls back to the chain for 140; a
//! signed-in session tries the TV app first, since it streams with the
//! account (uploads, region- and account-only tracks), and falls back to the
//! anonymous chain.
//! The first client that answers a usable AAC URL wins and is tried first
//! next time; a client whose URL was refused (403 mid-stream) is skipped for
//! [`BROKEN_FOR_MS`].

use std::collections::HashMap;
use std::sync::Arc;

use futures::future::BoxFuture;
use parking_lot::Mutex;

use super::client::{InnerTube, WebPlayerParams};
use super::clients::{ClientKind, ClientProfile, PLAYER_CHAIN};
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
/// Set (to anything) to skip every client but the JS host's (the web one on
/// the platforms, the TV one on the coordinator): a developer switch for
/// checking the JS path live.
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

/// The callback for the JS clients' puzzles: the platform's hidden page, or
/// the coordinator's embedded engine.
pub trait JsHost: Send + Sync + 'static {
    fn solve(&self, req: JsRequest) -> BoxFuture<'static, Result<JsAnswer, String>>;

    /// The JS clients this host can serve. The platforms' pages mint PO
    /// tokens, which the web client needs; an engine that can't mint them
    /// serves the TV app only.
    fn clients(&self) -> &'static [ClientKind] {
        &[ClientKind::WebRemix]
    }
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

    /// The order to try clients in now. `js`: the JS clients the host
    /// serves (none without a host); `signed_in`: a session is there.
    pub fn order(&self, premium: bool, js: &[ClientKind], signed_in: bool) -> Vec<ClientProfile> {
        let offered = |c: &ClientProfile| !c.needs_js || js.contains(&c.kind);
        if !js.is_empty() && std::env::var_os(FORCE_WEB_ENV).is_some() {
            // The live check of the JS path (desktop e2e `ytm-js-live`, the
            // coordinator's `ytm_live`).
            return PLAYER_CHAIN
                .iter()
                .copied()
                .filter(|c| c.needs_js && offered(c))
                .collect();
        }
        let now = self.clock.now_ms();
        let mut m = self.memory.lock();
        m.broken.retain(|_, until| *until > now);
        let mut order: Vec<ClientProfile> = PLAYER_CHAIN
            .iter()
            .copied()
            .filter(offered)
            .filter(|c| !m.broken.contains_key(&c.kind))
            .collect();
        let has = |kind: ClientKind| order.iter().any(|c| c.kind == kind);
        let front = if premium && has(ClientKind::WebRemix) {
            // 141 comes only through the web client.
            Some(ClientKind::WebRemix)
        } else if signed_in && has(ClientKind::Tv) {
            // The session streams through the TV app; the rest stay anonymous.
            Some(ClientKind::Tv)
        } else {
            m.last_good
        };
        if let Some(i) = front.and_then(|k| order.iter().position(|c| c.kind == k)) {
            let c = order.remove(i);
            order.insert(0, c);
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
        let js_clients = js.map_or(&[][..], |j| j.clients());
        for profile in self.order(premium, js_clients, it.signed_in()) {
            let r = if profile.needs_js {
                match js {
                    Some(js) => {
                        self.try_js(it, js.as_ref(), profile, premium, video_id)
                            .await
                    }
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

    /// A JS client: the player's signature timestamp (and, for the web
    /// client, a PO token) first, then the format's signature and `n`.
    async fn try_js(
        &self,
        it: &InnerTube,
        js: &dyn JsHost,
        profile: ClientProfile,
        premium: bool,
        video_id: &str,
    ) -> ProviderResult<ChosenStream> {
        let web = profile.kind == ClientKind::WebRemix;
        let visitor_data = it.visitor_data().await?;
        let prep = js
            .solve(JsRequest::Prepare {
                video_id: video_id.into(),
                visitor_data,
            })
            .await
            .map_err(ProviderError::Unsupported)?;
        // Only the web client's URLs need the PO token.
        let po_token = prep.po_token.filter(|_| web);
        let params = WebPlayerParams {
            signature_timestamp: prep.signature_timestamp,
            po_token: po_token.clone(),
        };
        let v = it.player(profile, video_id, Some(&params)).await?;
        let info = parse_player(&v);
        check_playable(&info, it)?;
        // 141 only through the web client: everything else (and the
        // coordinator's analysis key) is 140.
        let f = pick_format(&info.formats, premium && web)
            .ok_or_else(|| ProviderError::Unsupported(format!("{}: no AAC format", profile.name)))?
            .clone();
        let raw =
            f.url.clone().or_else(|| f.cipher.clone()).ok_or_else(|| {
                ProviderError::Unsupported(format!("{}: SABR only", profile.name))
            })?;
        let answer = js
            .solve(JsRequest::Decipher {
                video_id: video_id.into(),
                formats: vec![raw],
            })
            .await
            .map_err(ProviderError::Unsupported)?;
        let mut url = answer.urls.into_iter().next().ok_or_else(|| {
            ProviderError::Unsupported(format!("{}: the JS host signed nothing", profile.name))
        })?;
        if let Some(pot) = po_token.as_deref().filter(|_| !url.contains("&pot=")) {
            url.push_str("&pot=");
            url.push_str(&urlencode(pot));
        }
        Ok(self.chosen(profile, url, &f, info))
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
