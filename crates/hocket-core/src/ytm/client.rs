//! The InnerTube client: one call per endpoint, answering raw JSON that
//! [`super::parse`] turns into items. Requests carry `visitorData` (fetched
//! once and refreshed) and, when signed in, the cookies and `SAPISIDHASH`.
//! `player` calls from the no-JS clients stay anonymous, so a session is
//! only ever used by the clients that carry one in the real apps (the web
//! client and the TV app, [`ClientProfile::session`]).

use std::sync::Arc;

use parking_lot::{Mutex, RwLock};
use serde_json::{json, Value};

use super::auth::Cookies;
use super::clients::{ClientProfile, WEB_REMIX};
use super::http::{HttpMethod, HttpRequest, HttpResponse, YtmHttp};
use crate::provider::{ProviderError, ProviderResult};
use crate::util::Clock;

/// How long a fetched `visitorData` is used before it is fetched again.
pub const VISITOR_TTL_MS: f64 = 6.0 * 3_600_000.0;

/// Search filters (`params` of `search`), as the web app sends them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchFilter {
    Songs,
    Albums,
    Artists,
    Playlists,
}

impl SearchFilter {
    fn params(self) -> &'static str {
        match self {
            SearchFilter::Songs => "EgWKAQIIAWoKEAkQBRAKEAMQBA%3D%3D",
            SearchFilter::Albums => "EgWKAQIYAWoKEAkQChAFEAMQBA%3D%3D",
            SearchFilter::Artists => "EgWKAQIgAWoKEAkQChAFEAMQBA%3D%3D",
            SearchFilter::Playlists => "EgeKAQQoAEABagoQAxAEEAoQCRAF",
        }
    }
}

/// A playlist edit action (`browse/edit_playlist`).
#[derive(Debug, Clone, PartialEq)]
pub enum EditAction {
    Add {
        video_id: String,
    },
    Remove {
        video_id: String,
        set_video_id: String,
    },
    /// Move an entry before another (`None`: to the end).
    Move {
        set_video_id: String,
        before: Option<String>,
    },
    Rename {
        title: String,
    },
    Describe {
        description: String,
    },
}

impl EditAction {
    fn json(&self) -> Value {
        match self {
            EditAction::Add { video_id } => {
                json!({"action": "ACTION_ADD_VIDEO", "addedVideoId": video_id, "dedupeOption": "DEDUPE_OPTION_SKIP"})
            }
            EditAction::Remove {
                video_id,
                set_video_id,
            } => {
                json!({"action": "ACTION_REMOVE_VIDEO", "removedVideoId": video_id, "setVideoId": set_video_id})
            }
            EditAction::Move {
                set_video_id,
                before,
            } => {
                let mut a =
                    json!({"action": "ACTION_MOVE_VIDEO_BEFORE", "setVideoId": set_video_id});
                if let Some(b) = before {
                    a["movedSetVideoIdSuccessor"] = b.clone().into();
                }
                a
            }
            EditAction::Rename { title } => {
                json!({"action": "ACTION_SET_PLAYLIST_NAME", "playlistName": title})
            }
            EditAction::Describe { description } => {
                json!({"action": "ACTION_SET_PLAYLIST_DESCRIPTION", "playlistDescription": description})
            }
        }
    }
}

/// What the web client's `player` call needs from the platform's JS host.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct WebPlayerParams {
    pub signature_timestamp: Option<u32>,
    pub po_token: Option<String>,
}

pub struct InnerTube {
    http: Arc<dyn YtmHttp>,
    clock: Arc<dyn Clock>,
    cookies: RwLock<Option<Cookies>>,
    visitor: Mutex<Option<(String, f64)>>,
    hl: String,
    gl: String,
}

impl InnerTube {
    pub fn new(
        http: Arc<dyn YtmHttp>,
        clock: Arc<dyn Clock>,
        cookies: Option<Cookies>,
    ) -> InnerTube {
        InnerTube {
            http,
            clock,
            cookies: RwLock::new(cookies),
            visitor: Mutex::new(None),
            hl: "en".into(),
            gl: "US".into(),
        }
    }

    pub fn signed_in(&self) -> bool {
        self.cookies.read().is_some()
    }

    pub fn set_cookies(&self, cookies: Option<Cookies>) {
        *self.cookies.write() = cookies;
    }

    pub fn http(&self) -> &Arc<dyn YtmHttp> {
        &self.http
    }

    /// The anonymous visitor id every client needs (without it the no-JS
    /// clients answer "Sign in to confirm you're not a bot").
    pub async fn visitor_data(&self) -> ProviderResult<String> {
        let now = self.clock.now_ms();
        if let Some((v, at)) = self.visitor.lock().clone() {
            if now - at < VISITOR_TTL_MS {
                return Ok(v);
            }
        }
        let r = self
            .send(HttpRequest {
                method: HttpMethod::Get,
                url: "https://www.youtube.com/sw.js_data".into(),
                headers: vec![("User-Agent".into(), WEB_REMIX.user_agent.into())],
                body: None,
            })
            .await?;
        let body = String::from_utf8_lossy(&r.body);
        let v = parse_visitor_data(&body)
            .ok_or_else(|| ProviderError::Protocol("no visitorData in sw.js_data".into()))?;
        *self.visitor.lock() = Some((v.clone(), now));
        Ok(v)
    }

    /// Forget the visitor id (a "not a bot" answer).
    pub fn reset_visitor(&self) {
        *self.visitor.lock() = None;
    }

    async fn send(&self, req: HttpRequest) -> ProviderResult<HttpResponse> {
        let r = self.http.send(req).await.map_err(ProviderError::Network)?;
        match r.status {
            200..=299 => Ok(r),
            401 | 403 => Err(ProviderError::Auth(format!("HTTP {}", r.status))),
            404 => Err(ProviderError::NotFound("HTTP 404".into())),
            429 => Err(ProviderError::Network("HTTP 429 (rate limited)".into())),
            s @ 500..=599 => Err(ProviderError::Network(format!("HTTP {s}"))),
            s => Err(ProviderError::Server {
                code: s as u32,
                message: String::from_utf8_lossy(&r.body).chars().take(200).collect(),
            }),
        }
    }

    /// One InnerTube call as `profile`. `auth`: send the session (only for a
    /// client that carries one, [`ClientProfile::session`]).
    pub async fn call(
        &self,
        profile: ClientProfile,
        endpoint: &str,
        mut body: Value,
        auth: bool,
    ) -> ProviderResult<Value> {
        let visitor = self.visitor_data().await?;
        body["context"] = profile.context(Some(&visitor), &self.hl, &self.gl);
        let origin = format!("https://{}", profile.host);
        let mut headers = vec![
            ("Content-Type".to_string(), "application/json".to_string()),
            ("User-Agent".into(), profile.user_agent.into()),
            ("X-YouTube-Client-Name".into(), profile.id.to_string()),
            ("X-YouTube-Client-Version".into(), profile.version.into()),
            ("Origin".into(), origin.clone()),
            ("X-Goog-Visitor-Id".into(), visitor),
        ];
        if auth && profile.session {
            if let Some(c) = self.cookies.read().clone() {
                let now_s = (self.clock.now_ms() / 1000.0) as u64;
                if let Some(a) = c.authorization(&origin, now_s) {
                    headers.push(("Authorization".into(), a));
                    headers.push(("X-Origin".into(), origin.clone()));
                }
                headers.push(("Cookie".into(), c.header().to_string()));
                headers.push(("X-Goog-AuthUser".into(), "0".into()));
            }
        }
        let r = self
            .send(HttpRequest {
                method: HttpMethod::Post,
                url: format!(
                    "https://{}/youtubei/v1/{endpoint}?prettyPrint=false",
                    profile.host
                ),
                headers,
                body: Some(serde_json::to_vec(&body).unwrap_or_default()),
            })
            .await?;
        serde_json::from_slice(&r.body)
            .map_err(|e| ProviderError::Protocol(format!("{endpoint}: {e}")))
    }

    async fn web(&self, endpoint: &str, body: Value) -> ProviderResult<Value> {
        self.call(WEB_REMIX, endpoint, body, true).await
    }

    fn need_session(&self, what: &str) -> ProviderResult<()> {
        if self.signed_in() {
            Ok(())
        } else {
            Err(ProviderError::Unsupported(format!(
                "{what} needs a signed-in YouTube Music account"
            )))
        }
    }

    pub async fn search(&self, query: &str, filter: Option<SearchFilter>) -> ProviderResult<Value> {
        let mut body = json!({ "query": query });
        if let Some(f) = filter {
            body["params"] = f.params().into();
        }
        self.web("search", body).await
    }

    pub async fn browse(&self, browse_id: &str, params: Option<&str>) -> ProviderResult<Value> {
        let mut body = json!({ "browseId": browse_id });
        if let Some(p) = params {
            body["params"] = p.into();
        }
        self.web("browse", body).await
    }

    pub async fn continuation(&self, token: &str) -> ProviderResult<Value> {
        self.web("browse", json!({ "continuation": token })).await
    }

    /// The watch queue: radio for a song (`RDAMVM<id>`), plus the lyrics tab.
    pub async fn next(&self, video_id: &str, playlist_id: Option<&str>) -> ProviderResult<Value> {
        let mut body = json!({ "videoId": video_id, "isAudioOnly": true, "enablePersistentPlaylistPanel": true });
        if let Some(p) = playlist_id {
            body["playlistId"] = p.into();
        }
        self.web("next", body).await
    }

    /// `player` as `profile`. The JS clients also take the JS host's
    /// signature timestamp (and the web client its PO token), and the
    /// session-carrying ones the session.
    pub async fn player(
        &self,
        profile: ClientProfile,
        video_id: &str,
        web: Option<&WebPlayerParams>,
    ) -> ProviderResult<Value> {
        let mut body = json!({ "videoId": video_id, "contentCheckOk": true, "racyCheckOk": true });
        if let Some(w) = web {
            let mut ctx = json!({"html5Preference": "HTML5_PREF_WANTS"});
            if let Some(sts) = w.signature_timestamp {
                ctx["signatureTimestamp"] = sts.into();
            }
            body["playbackContext"] = json!({ "contentPlaybackContext": ctx });
            if let Some(pot) = &w.po_token {
                body["serviceIntegrityDimensions"] = json!({ "poToken": pot });
            }
        }
        self.call(profile, "player", body, profile.session).await
    }

    pub async fn account_menu(&self) -> ProviderResult<Value> {
        self.need_session("the account menu")?;
        self.web("account/account_menu", json!({})).await
    }

    pub async fn like(&self, video_id: &str, like: bool) -> ProviderResult<()> {
        self.need_session("Like")?;
        let ep = if like { "like/like" } else { "like/removelike" };
        self.web(ep, json!({ "target": { "videoId": video_id } }))
            .await
            .map(|_| ())
    }

    /// Save or unsave a playlist or an album (its `OLAK5uy_…` playlist).
    pub async fn save_playlist(&self, playlist_id: &str, save: bool) -> ProviderResult<()> {
        self.need_session("Save to library")?;
        let ep = if save { "like/like" } else { "like/removelike" };
        self.web(ep, json!({ "target": { "playlistId": playlist_id } }))
            .await
            .map(|_| ())
    }

    pub async fn subscribe(&self, channel_id: &str, subscribe: bool) -> ProviderResult<()> {
        self.need_session("Subscribe")?;
        let ep = if subscribe {
            "subscription/subscribe"
        } else {
            "subscription/unsubscribe"
        };
        self.web(ep, json!({ "channelIds": [channel_id] }))
            .await
            .map(|_| ())
    }

    /// Menu feedback tokens ("Add to library" / "Remove from library").
    pub async fn feedback(&self, tokens: &[String]) -> ProviderResult<()> {
        self.need_session("Library edits")?;
        self.web("feedback", json!({ "feedbackTokens": tokens }))
            .await
            .map(|_| ())
    }

    /// Edit a playlist; answers the `(videoId, setVideoId)` pairs an add created.
    pub async fn edit_playlist(
        &self,
        playlist_id: &str,
        actions: &[EditAction],
    ) -> ProviderResult<Vec<(String, String)>> {
        self.need_session("Playlist edits")?;
        let r = self
            .web(
                "browse/edit_playlist",
                json!({
                    "playlistId": playlist_id.strip_prefix("VL").unwrap_or(playlist_id),
                    "actions": actions.iter().map(EditAction::json).collect::<Vec<_>>(),
                }),
            )
            .await?;
        super::parse::parse_edit_result(&r).map_err(|m| ProviderError::Server {
            code: 0,
            message: m,
        })
    }

    pub async fn create_playlist(
        &self,
        title: &str,
        video_ids: &[String],
    ) -> ProviderResult<String> {
        self.need_session("New playlist")?;
        let r = self
            .web(
                "playlist/create",
                json!({ "title": title, "description": "", "privacyStatus": "PRIVATE", "videoIds": video_ids }),
            )
            .await?;
        r.get("playlistId")
            .and_then(Value::as_str)
            .map(str::to_string)
            .ok_or_else(|| ProviderError::Protocol("playlist/create: no playlistId".into()))
    }

    pub async fn delete_playlist(&self, playlist_id: &str) -> ProviderResult<()> {
        self.need_session("Delete playlist")?;
        self.web(
            "playlist/delete",
            json!({ "playlistId": playlist_id.strip_prefix("VL").unwrap_or(playlist_id) }),
        )
        .await
        .map(|_| ())
    }

    /// A history ping: GET a `playbackTracking` URL with the session.
    pub async fn ping(&self, url: &str) -> ProviderResult<()> {
        self.need_session("History")?;
        let c = self.cookies.read().clone();
        let mut headers = vec![("User-Agent".to_string(), WEB_REMIX.user_agent.to_string())];
        if let Some(c) = c {
            headers.push(("Cookie".into(), c.header().to_string()));
        }
        self.send(HttpRequest {
            method: HttpMethod::Get,
            url: url.to_string(),
            headers,
            body: None,
        })
        .await
        .map(|_| ())
    }
}

/// `)]}'\n[["yt.sw.adr",null,[[["en-GB","AU",…,"<visitorData>",…`: the
/// visitor id is element 13 of the first record.
pub fn parse_visitor_data(body: &str) -> Option<String> {
    let json = body.trim_start_matches(")]}'").trim_start();
    let v: Value = serde_json::from_str(json).ok()?;
    v.get(0)?
        .get(2)?
        .get(0)?
        .get(0)?
        .get(13)?
        .as_str()
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}
