//! Loopback streaming proxy: the only way a platform player reaches the
//! Subsonic server for audio.
//!
//! [`StreamProxy::start`] binds `127.0.0.1` on an ephemeral port inside the
//! core runtime. Resolution ([`Downloads::resolve_with`]) hands platforms
//! `http://127.0.0.1:<port>/s/<token>.<ext>`, where the token is a random
//! 128-bit capability minted per (track, transcoding profile) with a
//! sliding expiry. Unknown or expired tokens get `404`; no Subsonic
//! credential (`t`/`s`/`u`/`apiKey`) ever leaves the core, since the proxy
//! builds the upstream URL itself on each request.
//!
//! Per request, in order:
//! 1. a completed pin download for the track → served from disk;
//! 2. a complete stream-cache entry → served from disk (held open so it is
//!    not evicted or cleared under the reader);
//! 3. otherwise fetched from the server through a [`StreamUpstream`]. A
//!    `GET` that starts at byte 0 (no `Range`, or `bytes=0-`) is teed into a
//!    unique temp file in the stream-cache directory; when the body ends
//!    cleanly with the advertised length and is audio (not a JSON/XML
//!    Subsonic error envelope) the file is registered with
//!    [`Downloads::cache_put`] under (track, profile) and the actor is told
//!    which tracks changed offline state. Aborted reads and mid-file
//!    `Range` requests are passed through and never produce an entry.
//!
//! Files on disk honour single `Range` requests (`bytes=a-b`, `a-`, `-n`).

use std::collections::HashMap;
use std::convert::Infallible;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use bytes::Bytes;
use futures::future::BoxFuture;
use futures::stream::BoxStream;
use futures::StreamExt;
use http_body_util::combinators::UnsyncBoxBody;
use http_body_util::{BodyExt, Empty, StreamBody};
use hyper::body::{Frame, Incoming};
use hyper::header::{self, HeaderValue};
use hyper::{Method, Request, Response, StatusCode};
use parking_lot::{Mutex, RwLock};
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};
use tokio_util::sync::CancellationToken;
use url::Url;

use crate::api::TranscodingProfile;
use crate::downloads::{stream_options, Downloads, StreamMinter, StreamTarget, TrackKey};
use crate::subsonic::SubsonicApi;
use crate::util::Clock;

/// A token stays valid this long after it was minted or last used.
pub const TOKEN_TTL_MS: f64 = 12.0 * 60.0 * 60.0 * 1000.0;
/// Live tokens kept at most (the least recently used goes first).
pub const MAX_TOKENS: usize = 1024;
/// Chunk size for files served from disk.
const FILE_CHUNK: usize = 64 * 1024;
/// Chunks buffered between the upstream reader and a slow player.
const TEE_BUFFER: usize = 16;

type Body = UnsyncBoxBody<Bytes, std::io::Error>;

// ---------------------------------------------------------------------------
// Upstream seam
// ---------------------------------------------------------------------------

/// One fetch from the server. `range` is the player's `Range` header,
/// forwarded verbatim.
#[derive(Debug, Clone)]
pub struct UpstreamRequest {
    pub url: Url,
    pub range: Option<String>,
}

/// The server's answer, body streamed.
pub struct UpstreamResponse {
    pub status: u16,
    pub content_type: Option<String>,
    pub content_length: Option<u64>,
    pub content_range: Option<String>,
    pub accept_ranges: Option<String>,
    pub body: BoxStream<'static, Result<Bytes, String>>,
}

/// How the proxy reaches the server: reqwest in production, an in-memory
/// fake in tests. Errors must not carry the URL (it holds credentials).
pub trait StreamUpstream: Send + Sync + 'static {
    fn fetch(&self, request: UpstreamRequest)
        -> BoxFuture<'static, Result<UpstreamResponse, String>>;
}

/// Production upstream: reqwest + rustls, no redirects (a redirect would
/// carry the auth query elsewhere), no overall timeout (a track streams for
/// as long as it plays) but connect and read timeouts.
pub struct ReqwestUpstream {
    client: reqwest::Client,
}

impl ReqwestUpstream {
    pub fn new() -> Result<Self, String> {
        let client = reqwest::Client::builder()
            .user_agent(concat!("hocket/", env!("CARGO_PKG_VERSION")))
            .connect_timeout(std::time::Duration::from_secs(15))
            .read_timeout(std::time::Duration::from_secs(60))
            .redirect(reqwest::redirect::Policy::none())
            .use_rustls_tls()
            .build()
            .map_err(|e| e.without_url().to_string())?;
        Ok(ReqwestUpstream { client })
    }
}

impl StreamUpstream for ReqwestUpstream {
    fn fetch(
        &self,
        request: UpstreamRequest,
    ) -> BoxFuture<'static, Result<UpstreamResponse, String>> {
        let client = self.client.clone();
        Box::pin(async move {
            let mut b = client.get(request.url);
            if let Some(r) = &request.range {
                b = b.header(reqwest::header::RANGE, r);
            }
            let resp = b.send().await.map_err(|e| e.without_url().to_string())?;
            let h = resp.headers();
            let text = |name: reqwest::header::HeaderName| {
                h.get(name)
                    .and_then(|v| v.to_str().ok())
                    .map(str::to_string)
            };
            let content_type = text(reqwest::header::CONTENT_TYPE);
            let content_range = text(reqwest::header::CONTENT_RANGE);
            let accept_ranges = text(reqwest::header::ACCEPT_RANGES);
            let status = resp.status().as_u16();
            let content_length = resp.content_length();
            let body = resp
                .bytes_stream()
                .map(|r| r.map_err(|e| e.without_url().to_string()))
                .boxed();
            Ok(UpstreamResponse {
                status,
                content_type,
                content_length,
                content_range,
                accept_ranges,
                body,
            })
        })
    }
}

// ---------------------------------------------------------------------------
// Proxy
// ---------------------------------------------------------------------------

/// Called with the tracks whose offline state changed (a new cache entry,
/// evictions, deferred removals). Runs on a runtime thread.
pub type CacheNotify = Arc<dyn Fn(Vec<TrackKey>) + Send + Sync>;

#[derive(Debug, Clone)]
struct Token {
    server_id: String,
    track_id: String,
    profile: Option<TranscodingProfile>,
    suffix: Option<String>,
    mime_type: Option<String>,
    expires_at: f64,
}

impl Token {
    fn same_target(&self, t: &StreamTarget) -> bool {
        self.server_id == t.server_id && self.track_id == t.track_id && self.profile == t.profile
    }
}

struct Shared {
    port: u16,
    downloads: Downloads,
    clock: Arc<dyn Clock>,
    upstream: Arc<dyn StreamUpstream>,
    notify: CacheNotify,
    api: RwLock<Option<Arc<dyn SubsonicApi>>>,
    tokens: Mutex<HashMap<String, Token>>,
    stop: CancellationToken,
}

/// Handle to the running proxy. Cheap to clone.
#[derive(Clone)]
pub struct StreamProxy {
    shared: Arc<Shared>,
}

impl StreamProxy {
    /// Bind `127.0.0.1:0` and serve on `rt` until [`stop`](Self::stop).
    pub fn start(
        rt: &tokio::runtime::Handle,
        downloads: Downloads,
        clock: Arc<dyn Clock>,
        upstream: Arc<dyn StreamUpstream>,
        notify: CacheNotify,
    ) -> std::io::Result<StreamProxy> {
        let std_listener = std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))?;
        std_listener.set_nonblocking(true)?;
        let port = std_listener.local_addr()?.port();
        let listener = {
            let _guard = rt.enter();
            tokio::net::TcpListener::from_std(std_listener)?
        };
        let shared = Arc::new(Shared {
            port,
            downloads,
            clock,
            upstream,
            notify,
            api: RwLock::new(None),
            tokens: Mutex::new(HashMap::new()),
            stop: CancellationToken::new(),
        });
        rt.spawn(accept_loop(listener, shared.clone()));
        Ok(StreamProxy { shared })
    }

    pub fn port(&self) -> u16 {
        self.shared.port
    }

    /// The server client upstream fetches use. A different server (or
    /// none) revokes every token minted for another one.
    pub fn set_api(&self, api: Option<Arc<dyn SubsonicApi>>) {
        let sid = api.as_ref().map(|a| a.server_id().to_string());
        *self.shared.api.write() = api;
        self.shared
            .tokens
            .lock()
            .retain(|_, t| Some(&t.server_id) == sid.as_ref());
    }

    /// Stop accepting and drop open connections.
    pub fn stop(&self) {
        self.shared.stop.cancel();
        self.shared.tokens.lock().clear();
    }

    /// Number of live tokens (tests, diagnostics).
    pub fn token_count(&self) -> usize {
        self.shared.tokens.lock().len()
    }

    fn url(&self, token: &str, suffix: Option<&str>) -> String {
        let ext = crate::downloads::safe_ext(suffix);
        if ext == "bin" {
            format!("http://127.0.0.1:{}/s/{token}", self.shared.port)
        } else {
            format!("http://127.0.0.1:{}/s/{token}.{ext}", self.shared.port)
        }
    }
}

impl StreamMinter for StreamProxy {
    fn mint(&self, target: StreamTarget) -> Option<String> {
        if self.shared.stop.is_cancelled() {
            return None;
        }
        let now = self.shared.clock.now_ms();
        let mut tokens = self.shared.tokens.lock();
        tokens.retain(|_, t| t.expires_at > now);
        if let Some((id, t)) = tokens.iter_mut().find(|(_, t)| t.same_target(&target)) {
            t.expires_at = now + TOKEN_TTL_MS;
            t.suffix = target.suffix.clone();
            t.mime_type = target.mime_type.clone();
            let id = id.clone();
            drop(tokens);
            return Some(self.url(&id, target.suffix.as_deref()));
        }
        while tokens.len() >= MAX_TOKENS {
            let oldest = tokens
                .iter()
                .min_by(|a, b| a.1.expires_at.total_cmp(&b.1.expires_at))
                .map(|(k, _)| k.clone());
            match oldest {
                Some(k) => {
                    tokens.remove(&k);
                }
                None => break,
            }
        }
        let id = new_token();
        tokens.insert(
            id.clone(),
            Token {
                server_id: target.server_id,
                track_id: target.track_id,
                profile: target.profile,
                suffix: target.suffix.clone(),
                mime_type: target.mime_type,
                expires_at: now + TOKEN_TTL_MS,
            },
        );
        drop(tokens);
        Some(self.url(&id, target.suffix.as_deref()))
    }
}

/// 128 random bits from the OS-seeded CSPRNG, hex.
fn new_token() -> String {
    use rand::Rng;
    let bytes: [u8; 16] = rand::rng().random();
    hex::encode(bytes)
}

async fn accept_loop(listener: tokio::net::TcpListener, shared: Arc<Shared>) {
    loop {
        let accepted = tokio::select! {
            _ = shared.stop.cancelled() => break,
            r = listener.accept() => r,
        };
        let sock = match accepted {
            Ok((sock, _)) => sock,
            Err(e) => {
                tracing::debug!(target: "hocket_core", error = %e, "stream proxy accept");
                tokio::time::sleep(std::time::Duration::from_millis(20)).await;
                continue;
            }
        };
        let shared = shared.clone();
        tokio::spawn(async move {
            let stop = shared.stop.clone();
            let svc = hyper::service::service_fn(move |req| {
                let shared = shared.clone();
                async move { Ok::<_, Infallible>(handle(shared, req).await) }
            });
            let conn = hyper::server::conn::http1::Builder::new()
                .serve_connection(hyper_util::rt::TokioIo::new(sock), svc);
            tokio::select! {
                _ = stop.cancelled() => {}
                r = conn => {
                    if let Err(e) = r {
                        tracing::debug!(target: "hocket_core", error = %e, "stream proxy connection");
                    }
                }
            }
        });
    }
}

fn empty() -> Body {
    Empty::<Bytes>::new()
        .map_err(|never: Infallible| match never {})
        .boxed_unsync()
}

fn status(code: StatusCode) -> Response<Body> {
    let mut r = Response::new(empty());
    *r.status_mut() = code;
    r
}

fn set_header(r: &mut Response<Body>, name: header::HeaderName, value: &str) {
    if let Ok(v) = HeaderValue::from_str(value) {
        r.headers_mut().insert(name, v);
    }
}

/// `/s/<32 hex>[.<ext>]` → the token.
fn token_of(path: &str) -> Option<&str> {
    let rest = path.strip_prefix("/s/")?;
    let token = rest.split_once('.').map_or(rest, |(t, _)| t);
    (token.len() == 32 && token.bytes().all(|b| b.is_ascii_hexdigit())).then_some(token)
}

async fn handle(shared: Arc<Shared>, req: Request<Incoming>) -> Response<Body> {
    let head = match *req.method() {
        Method::GET => false,
        Method::HEAD => true,
        _ => return status(StatusCode::METHOD_NOT_ALLOWED),
    };
    let Some(token_id) = token_of(req.uri().path()) else {
        return status(StatusCode::NOT_FOUND);
    };
    let now = shared.clock.now_ms();
    let token = {
        let mut tokens = shared.tokens.lock();
        match tokens.get_mut(token_id) {
            Some(t) if t.expires_at > now => {
                t.expires_at = now + TOKEN_TTL_MS;
                t.clone()
            }
            Some(_) => {
                tokens.remove(token_id);
                return status(StatusCode::NOT_FOUND);
            }
            None => return status(StatusCode::NOT_FOUND),
        }
    };
    let range = req
        .headers()
        .get(header::RANGE)
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);
    let dl = &shared.downloads;
    // 1. A pin finished since the token was minted.
    if let Ok(Some(p)) = dl.downloaded_path(&token.server_id, &token.track_id) {
        return serve_file(&shared, p, token.mime_type.clone(), range, head, false).await;
    }
    // 2. A complete stream-cache copy.
    match dl.cache_lookup(
        &token.server_id,
        &token.track_id,
        token.profile.as_ref(),
        true,
    ) {
        Ok(Some(e)) => {
            let ct = e.content_type.or(token.mime_type.clone());
            return serve_file(&shared, e.path, ct, range, head, true).await;
        }
        Ok(None) => {}
        Err(e) => tracing::warn!(target: "hocket_core", error = %e, "stream cache lookup"),
    }
    // 3. The server.
    let api = shared.api.read().clone();
    let Some(api) = api.filter(|a| a.server_id() == token.server_id) else {
        return status(StatusCode::SERVICE_UNAVAILABLE);
    };
    serve_upstream(&shared, api.as_ref(), &token, range, head).await
}

// -- disk ---------------------------------------------------------------------

#[derive(Debug, PartialEq)]
enum ByteRange {
    Full,
    /// Inclusive.
    Part(u64, u64),
    Unsatisfiable,
}

/// A single `bytes=` range against a resource of `len` bytes. Anything
/// malformed or multi-range is served whole.
fn parse_range(h: Option<&str>, len: u64) -> ByteRange {
    let Some(spec) = h.and_then(|h| h.trim().strip_prefix("bytes=")) else {
        return ByteRange::Full;
    };
    if spec.contains(',') {
        return ByteRange::Full;
    }
    let Some((a, b)) = spec.split_once('-') else {
        return ByteRange::Full;
    };
    let (a, b) = (a.trim(), b.trim());
    if a.is_empty() {
        let Ok(n) = b.parse::<u64>() else {
            return ByteRange::Full;
        };
        if n == 0 || len == 0 {
            return ByteRange::Unsatisfiable;
        }
        return ByteRange::Part(len.saturating_sub(n), len - 1);
    }
    let Ok(start) = a.parse::<u64>() else {
        return ByteRange::Full;
    };
    let end = if b.is_empty() {
        None
    } else {
        match b.parse::<u64>() {
            Ok(e) if e >= start => Some(e),
            _ => return ByteRange::Full,
        }
    };
    if start >= len {
        return ByteRange::Unsatisfiable;
    }
    ByteRange::Part(start, end.map_or(len - 1, |e| e.min(len - 1)))
}

/// Releases a cache file's reader hold when the response body is done or
/// dropped, running deferred removals.
struct ReaderGuard {
    downloads: Downloads,
    path: PathBuf,
    notify: CacheNotify,
}

impl Drop for ReaderGuard {
    fn drop(&mut self) {
        let changed = self.downloads.release_reader(&self.path);
        if !changed.is_empty() {
            (self.notify)(changed);
        }
    }
}

async fn serve_file(
    shared: &Shared,
    path: PathBuf,
    content_type: Option<String>,
    range: Option<String>,
    head: bool,
    cache_entry: bool,
) -> Response<Body> {
    // Hold the file before opening it, so a concurrent clear defers.
    let guard = cache_entry.then(|| {
        shared.downloads.acquire_reader(&path);
        ReaderGuard {
            downloads: shared.downloads.clone(),
            path: path.clone(),
            notify: shared.notify.clone(),
        }
    });
    let mut file = match tokio::fs::File::open(&path).await {
        Ok(f) => f,
        Err(e) => {
            tracing::warn!(target: "hocket_core", error = %e, "stream proxy open");
            return status(StatusCode::NOT_FOUND);
        }
    };
    let len = match file.metadata().await {
        Ok(m) => m.len(),
        Err(_) => return status(StatusCode::INTERNAL_SERVER_ERROR),
    };
    let (code, start, count) = match parse_range(range.as_deref(), len) {
        ByteRange::Full => (StatusCode::OK, 0, len),
        ByteRange::Part(a, b) => (StatusCode::PARTIAL_CONTENT, a, b - a + 1),
        ByteRange::Unsatisfiable => {
            let mut r = status(StatusCode::RANGE_NOT_SATISFIABLE);
            set_header(&mut r, header::CONTENT_RANGE, &format!("bytes */{len}"));
            return r;
        }
    };
    if start > 0 && file.seek(std::io::SeekFrom::Start(start)).await.is_err() {
        return status(StatusCode::INTERNAL_SERVER_ERROR);
    }
    let body = if head {
        empty()
    } else {
        let stream = futures::stream::unfold(
            (file, count, guard),
            |(mut file, remaining, guard)| async move {
                if remaining == 0 {
                    return None;
                }
                let want = remaining.min(FILE_CHUNK as u64) as usize;
                let mut buf = vec![0u8; want];
                match file.read(&mut buf).await {
                    Ok(0) => Some((
                        Err(std::io::Error::new(
                            std::io::ErrorKind::UnexpectedEof,
                            "file shrank while serving",
                        )),
                        (file, 0, guard),
                    )),
                    Ok(n) => {
                        buf.truncate(n);
                        Some((
                            Ok(Frame::data(Bytes::from(buf))),
                            (file, remaining - n as u64, guard),
                        ))
                    }
                    Err(e) => Some((Err(e), (file, 0, guard))),
                }
            },
        );
        BodyExt::boxed_unsync(StreamBody::new(stream))
    };
    let mut r = Response::new(body);
    *r.status_mut() = code;
    set_header(&mut r, header::ACCEPT_RANGES, "bytes");
    set_header(&mut r, header::CONTENT_LENGTH, &count.to_string());
    if code == StatusCode::PARTIAL_CONTENT {
        set_header(
            &mut r,
            header::CONTENT_RANGE,
            &format!("bytes {start}-{}/{len}", start + count - 1),
        );
    }
    if let Some(ct) = content_type {
        set_header(&mut r, header::CONTENT_TYPE, &ct);
    }
    r
}

// -- server -------------------------------------------------------------------

/// A body the server sends instead of audio when `stream` fails with
/// HTTP 200 (Subsonic's JSON/XML error envelope) or any text.
fn is_error_envelope(content_type: Option<&str>) -> bool {
    let Some(ct) = content_type else {
        return false;
    };
    let ct = ct.trim().to_ascii_lowercase();
    ct.starts_with("application/json")
        || ct.starts_with("application/xml")
        || ct.starts_with("text/")
}

/// Whether a player request reads the resource from its first byte.
fn from_start(range: Option<&str>) -> bool {
    match range {
        None => true,
        Some(r) => r.trim().replace(' ', "") == "bytes=0-",
    }
}

/// `bytes 0-999/1000` covers the whole resource of that total.
fn covers_whole(content_range: Option<&str>, content_length: Option<u64>) -> bool {
    let Some((start, total)) = content_range.and_then(parse_content_range) else {
        return false;
    };
    start == 0 && total.is_some() && total == content_length
}

/// `bytes 100-199/1000` (or `/*`) → (start, total).
fn parse_content_range(v: &str) -> Option<(u64, Option<u64>)> {
    let rest = v.trim().strip_prefix("bytes")?.trim();
    let (range, total) = rest.split_once('/')?;
    let (start, _end) = range.split_once('-')?;
    Some((start.trim().parse().ok()?, total.trim().parse().ok()))
}

async fn serve_upstream(
    shared: &Shared,
    api: &dyn SubsonicApi,
    token: &Token,
    range: Option<String>,
    head: bool,
) -> Response<Body> {
    let url = api.stream_url(&token.track_id, &stream_options(token.profile.as_ref()));
    let request = UpstreamRequest {
        url,
        range: range.clone(),
    };
    let up = match shared.upstream.fetch(request).await {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!(target: "hocket_core", error = %e, track = %token.track_id, "stream proxy upstream");
            return status(StatusCode::BAD_GATEWAY);
        }
    };
    if !(200..300).contains(&up.status) {
        tracing::warn!(target: "hocket_core", status = up.status, track = %token.track_id, "stream proxy upstream status");
        return status(match up.status {
            404 => StatusCode::NOT_FOUND,
            416 => StatusCode::RANGE_NOT_SATISFIABLE,
            _ => StatusCode::BAD_GATEWAY,
        });
    }
    if is_error_envelope(up.content_type.as_deref()) {
        tracing::warn!(target: "hocket_core", track = %token.track_id, content_type = ?up.content_type, "server answered a stream with an error envelope");
        return status(StatusCode::BAD_GATEWAY);
    }
    let whole = match up.status {
        200 => true,
        206 => covers_whole(up.content_range.as_deref(), up.content_length),
        _ => false,
    };
    let budget = shared.downloads.cache_budget();
    let cacheable = !head
        && from_start(range.as_deref())
        && whole
        && up
            .content_length
            .is_none_or(|n| (n as f64) <= budget && n > 0)
        && shared
            .downloads
            .cache_has_room(&token.server_id, up.content_length.unwrap_or(0) as f64);
    let mut r = Response::new(empty());
    *r.status_mut() = StatusCode::from_u16(up.status).unwrap_or(StatusCode::OK);
    if let Some(ct) = up.content_type.as_deref().or(token.mime_type.as_deref()) {
        set_header(&mut r, header::CONTENT_TYPE, ct);
    }
    if let Some(n) = up.content_length {
        set_header(&mut r, header::CONTENT_LENGTH, &n.to_string());
    }
    if let Some(cr) = &up.content_range {
        set_header(&mut r, header::CONTENT_RANGE, cr);
    }
    if let Some(ar) = &up.accept_ranges {
        set_header(&mut r, header::ACCEPT_RANGES, ar);
    }
    if head {
        return r;
    }
    let body = if cacheable {
        tee(shared, token, up)
    } else {
        let stream = up.body.map(|c| c.map(Frame::data).map_err(std::io::Error::other));
        BodyExt::boxed_unsync(StreamBody::new(stream))
    };
    *r.body_mut() = body;
    r
}

/// Stream `up` to the player while writing it to a temp file; register
/// the file only when the body ends cleanly at the advertised length. The
/// player going away (or the server failing) mid-body removes the temp
/// file.
fn tee(shared: &Shared, token: &Token, up: UpstreamResponse) -> Body {
    let (tx, rx) = tokio::sync::mpsc::channel::<Result<Bytes, std::io::Error>>(TEE_BUFFER);
    let downloads = shared.downloads.clone();
    let notify = shared.notify.clone();
    let token = token.clone();
    let expected = up.content_length;
    let content_type = up.content_type.clone().or(token.mime_type.clone());
    let mut body = up.body;
    tokio::spawn(async move {
        let dest = downloads.cache_path_unique(
            &token.server_id,
            &token.track_id,
            token.profile.as_ref(),
            token.suffix.as_deref(),
        );
        let tmp = crate::subsonic::transport::part_path(&dest);
        let mut file = match open_temp(&tmp).await {
            Ok(f) => Some(f),
            Err(e) => {
                tracing::warn!(target: "hocket_core", error = %e, "stream cache temp file");
                None
            }
        };
        let mut written: u64 = 0;
        let mut complete = false;
        loop {
            let next = tokio::select! {
                _ = tx.closed() => break,
                c = body.next() => c,
            };
            match next {
                Some(Ok(chunk)) => {
                    if let Some(f) = file.as_mut() {
                        if let Err(e) = f.write_all(&chunk).await {
                            tracing::warn!(target: "hocket_core", error = %e, "stream cache write");
                            file = None;
                            let _ = tokio::fs::remove_file(&tmp).await;
                        }
                    }
                    written += chunk.len() as u64;
                    if tx.send(Ok(chunk)).await.is_err() {
                        break;
                    }
                }
                Some(Err(e)) => {
                    let _ = tx.send(Err(std::io::Error::other(e))).await;
                    break;
                }
                None => {
                    complete = true;
                    break;
                }
            }
        }
        drop(tx);
        let Some(mut f) = file else {
            return;
        };
        let good = complete && written > 0 && expected.is_none_or(|n| n == written);
        let flushed = good && f.flush().await.is_ok() && f.sync_all().await.is_ok();
        drop(f);
        if !flushed {
            let _ = tokio::fs::remove_file(&tmp).await;
            return;
        }
        if let Err(e) = tokio::fs::rename(&tmp, &dest).await {
            tracing::warn!(target: "hocket_core", error = %e, "stream cache rename");
            let _ = tokio::fs::remove_file(&tmp).await;
            return;
        }
        match downloads.cache_put(
            &token.server_id,
            &token.track_id,
            token.profile.as_ref(),
            &dest,
            written as f64,
            content_type.as_deref(),
        ) {
            Ok(changed) => (notify)(changed),
            Err(e) => {
                tracing::warn!(target: "hocket_core", error = %e, "stream cache register");
                let _ = tokio::fs::remove_file(&dest).await;
            }
        }
    });
    let stream = futures::stream::unfold(rx, |mut rx| async move {
        rx.recv().await.map(|c| (c.map(Frame::data), rx))
    });
    BodyExt::boxed_unsync(StreamBody::new(stream))
}

async fn open_temp(tmp: &Path) -> std::io::Result<tokio::fs::File> {
    if let Some(parent) = tmp.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    tokio::fs::File::create(tmp).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ranges() {
        assert_eq!(parse_range(None, 10), ByteRange::Full);
        assert_eq!(parse_range(Some("bytes=0-"), 10), ByteRange::Part(0, 9));
        assert_eq!(parse_range(Some("bytes=2-4"), 10), ByteRange::Part(2, 4));
        assert_eq!(parse_range(Some("bytes=2-40"), 10), ByteRange::Part(2, 9));
        assert_eq!(parse_range(Some("bytes=-3"), 10), ByteRange::Part(7, 9));
        assert_eq!(parse_range(Some("bytes=-30"), 10), ByteRange::Part(0, 9));
        assert_eq!(parse_range(Some("bytes=10-"), 10), ByteRange::Unsatisfiable);
        assert_eq!(parse_range(Some("bytes=-0"), 10), ByteRange::Unsatisfiable);
        assert_eq!(parse_range(Some("bytes=5-2"), 10), ByteRange::Full);
        assert_eq!(parse_range(Some("bytes=0-1,4-5"), 10), ByteRange::Full);
        assert_eq!(parse_range(Some("items=0-1"), 10), ByteRange::Full);
    }

    #[test]
    fn tokens_and_envelopes() {
        assert_eq!(
            token_of("/s/0123456789abcdef0123456789abcdef.flac"),
            Some("0123456789abcdef0123456789abcdef")
        );
        assert_eq!(
            token_of("/s/0123456789abcdef0123456789abcdef"),
            Some("0123456789abcdef0123456789abcdef")
        );
        assert_eq!(token_of("/s/short"), None);
        assert_eq!(token_of("/x/0123456789abcdef0123456789abcdef"), None);
        assert!(from_start(None) && from_start(Some("bytes=0-")));
        assert!(!from_start(Some("bytes=100-")) && !from_start(Some("bytes=0-99")));
        assert!(covers_whole(Some("bytes 0-9/10"), Some(10)));
        assert!(!covers_whole(Some("bytes 0-4/10"), Some(5)));
        assert!(!covers_whole(Some("bytes 0-9/*"), Some(10)));
        assert!(is_error_envelope(Some("application/json; charset=utf-8")));
        assert!(is_error_envelope(Some("text/xml")));
        assert!(!is_error_envelope(Some("audio/flac")));
        assert!(!is_error_envelope(None));
        let a = new_token();
        assert_eq!(a.len(), 32);
        assert_ne!(a, new_token());
    }
}
