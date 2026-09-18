//! Navidrome's native (UI) API — undocumented and unstable, reverse-engineered
//! from Navidrome's `server/nativeapi` and Feishin. Used only for what the
//! Subsonic API can't do: reading and writing smart-playlist rules. Every
//! caller must check `capabilities().native_api` first.
//!
//! Auth: `POST /auth/login {username,password}` → `{token, ...}`; the JWT
//! goes in `x-nd-authorization: Bearer <token>` (Navidrome refreshes it in the
//! response header of every call). `GET /api/keepalive/keepalive` is the ping.

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use url::Url;

use super::auth::AuthMode;
use super::client::Client;
use super::transport::{HttpRequest, HttpResponse, Method};
use super::types::{NativeLogin, NativePlaylist};
use super::{SubsonicError, SubsonicResult};

pub const AUTH_HEADER: &str = "x-nd-authorization";

/// In-memory JWT session for the native API.
#[derive(Default)]
pub struct NativeSession {
    inner: RwLock<Option<NativeLogin>>,
}

impl NativeSession {
    pub fn token(&self) -> Option<String> {
        self.inner.read().as_ref().map(|l| l.token.clone())
    }
    pub fn username(&self) -> Option<String> {
        self.inner.read().as_ref().map(|l| l.username.clone()).filter(|u| !u.is_empty())
    }
    pub fn is_logged_in(&self) -> bool {
        self.inner.read().is_some()
    }
    pub fn clear(&self) {
        *self.inner.write() = None;
    }
}

/// Body for `POST /api/playlist` and `PUT /api/playlist/:id`. `rules` is the
/// smart-playlist criteria object (NSP shape) as produced by the filters
/// module; `None` leaves the field alone / makes a static playlist.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct NativePlaylistUpdate {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub public: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub owner_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rules: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sync: Option<bool>,
}

pub(super) fn native_url(client: &Client, path: &str) -> Url {
    let mut url = client.base_url().clone();
    {
        let mut segs = url.path_segments_mut().expect("base url is not cannot-be-a-base");
        segs.pop_if_empty();
        for s in path.trim_start_matches('/').split('/') {
            segs.push(s);
        }
    }
    url
}

/// Log in with the password credential. Not possible in `apiKey` mode (the
/// native API has no key auth), in which case `native_api` stays false.
pub(super) async fn login(client: &Client) -> SubsonicResult<NativeLogin> {
    let (username, password) = match client.auth_mode() {
        AuthMode::Password { username, password } => (username, password),
        AuthMode::ApiKey { .. } => return Err(SubsonicError::Unsupported("native api needs a password".into())),
    };
    let body = serde_json::json!({ "username": username, "password": password.expose() }).to_string();
    let req = HttpRequest::get(native_url(client, "auth/login")).with_json_body(Method::Post, &body);
    let resp = client.execute_raw(req).await?;
    match resp.status {
        200..=299 => {
            let login: NativeLogin = serde_json::from_slice(&resp.body)
                .map_err(|e| SubsonicError::Protocol(format!("native login: {e}")))?;
            if login.token.is_empty() {
                return Err(SubsonicError::Protocol("native login: empty token".into()));
            }
            *client.native.inner.write() = Some(login.clone());
            Ok(login)
        }
        401 | 403 => Err(SubsonicError::Auth("native login rejected".into())),
        404 => Err(SubsonicError::NotFound("native api absent".into())),
        s => Err(SubsonicError::Server { code: s as u32, message: format!("native login http {s}") }),
    }
}

/// `GET /api/keepalive/keepalive`. True when the native API answered 200.
pub(super) async fn keepalive(client: &Client) -> SubsonicResult<bool> {
    let resp = authed(client, HttpRequest::get(native_url(client, "api/keepalive/keepalive"))).await?;
    Ok((200..300).contains(&resp.status))
}

/// Execute with the JWT, logging in first (and once more on 401).
async fn authed(client: &Client, request: HttpRequest) -> SubsonicResult<HttpResponse> {
    if !client.native.is_logged_in() {
        login(client).await?;
    }
    let send = |token: String| {
        let mut r = request.clone();
        r.headers.push((AUTH_HEADER.into(), format!("Bearer {token}")));
        r.headers.push(("accept".into(), "application/json".into()));
        r
    };
    let token = client.native.token().unwrap_or_default();
    let resp = client.execute_raw(send(token)).await?;
    if resp.status == 401 {
        client.native.clear();
        let login = login(client).await?;
        return Ok(client.execute_raw(send(login.token)).await?);
    }
    Ok(resp)
}

fn parse_native<T: serde::de::DeserializeOwned>(resp: HttpResponse, what: &str) -> SubsonicResult<T> {
    match resp.status {
        200..=299 => serde_json::from_slice(&resp.body).map_err(|e| SubsonicError::Protocol(format!("native {what}: {e}"))),
        401 | 403 => Err(SubsonicError::Auth(format!("native {what}: http {}", resp.status))),
        404 => Err(SubsonicError::NotFound(format!("native {what}"))),
        s => Err(SubsonicError::Server { code: s as u32, message: format!("native {what} http {s}") }),
    }
}

pub(super) async fn get_playlist(client: &Client, id: &str) -> SubsonicResult<NativePlaylist> {
    let resp = authed(client, HttpRequest::get(native_url(client, &format!("api/playlist/{id}")))).await?;
    parse_native(resp, "playlist")
}

pub(super) async fn update_playlist(
    client: &Client,
    id: &str,
    update: &NativePlaylistUpdate,
) -> SubsonicResult<NativePlaylist> {
    let body = serde_json::to_string(update).map_err(|e| SubsonicError::Protocol(e.to_string()))?;
    let req = HttpRequest::get(native_url(client, &format!("api/playlist/{id}"))).with_json_body(Method::Put, &body);
    let resp = authed(client, req).await?;
    // Navidrome answers PUT with the updated resource, but be lenient: on an
    // empty body re-read it.
    if (200..300).contains(&resp.status) && resp.body.is_empty() {
        return get_playlist(client, id).await;
    }
    parse_native(resp, "playlist update")
}

#[derive(Deserialize)]
struct Created {
    id: String,
}

pub(super) async fn create_playlist(client: &Client, update: &NativePlaylistUpdate) -> SubsonicResult<String> {
    let body = serde_json::to_string(update).map_err(|e| SubsonicError::Protocol(e.to_string()))?;
    let req = HttpRequest::get(native_url(client, "api/playlist")).with_json_body(Method::Post, &body);
    let resp = authed(client, req).await?;
    let created: Created = parse_native(resp, "playlist create")?;
    Ok(created.id)
}
