//! In-process byte-range reader: how a player inside the process (the
//! native backend today, a Media3 `DataSource` over FFI later) reads audio
//! without ever seeing a Subsonic credential. No socket is opened.
//!
//! [`Downloads::resolve_with`] hands such players
//! `hocket-stream://<token>` ([`STREAM_URL_PREFIX`]), where the token is a
//! random 128-bit capability minted per (track, transcoding profile) with a
//! sliding expiry ([`TOKEN_TTL_MS`]). The player calls
//! [`StreamReader::open`] with the URL (or bare token), an offset and an
//! optional length, then [`StreamReader::read`] until it returns an empty
//! chunk (end of input), then [`StreamReader::close`]. Seeking is close +
//! open at the new offset. [`crate::core::Core`] exposes the same calls,
//! plus blocking wrappers for FFI loader threads.
//!
//! An open picks, in order:
//! 1. a completed pin download for the track → read from disk;
//! 2. a complete stream-cache entry → read from disk, held (not evicted or
//!    cleared) until the handle closes;
//! 3. otherwise the server, through a [`StreamUpstream`] that builds the
//!    credentialed URL inside the core. An open at offset 0 with no length
//!    limit is teed into a unique temp file in the stream-cache directory;
//!    when the body ends cleanly at its advertised length and is audio (not
//!    a JSON/XML Subsonic error envelope) the file is registered with
//!    [`Downloads::cache_put`] under (track, profile) and the actor is told
//!    which tracks changed offline state. Closing early, a server error, or
//!    any open at another offset never produces an entry, and the temp file
//!    is removed.
//!
//! Offsets are always honoured: when the server ignores `Range` (on-the-fly
//! transcodes) the reader skips to the offset itself. Open handles are
//! bounded ([`MAX_HANDLES`]); handles left idle for [`HANDLE_IDLE_MS`] are
//! closed on the next open.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use bytes::{Buf, Bytes};
use futures::future::BoxFuture;
use futures::stream::BoxStream;
use futures::StreamExt;
use parking_lot::{Mutex, RwLock};
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};
use tokio_util::sync::CancellationToken;
use url::Url;

use crate::api::TranscodingProfile;
use crate::downloads::{stream_options, Downloads, StreamMinter, StreamTarget, TrackKey};
use crate::subsonic::SubsonicApi;
use crate::util::Clock;

/// URL prefix of media sources served by the reader.
pub const STREAM_URL_PREFIX: &str = "hocket-stream://";
/// A token stays valid this long after it was minted or last used.
pub const TOKEN_TTL_MS: f64 = 12.0 * 60.0 * 60.0 * 1000.0;
/// Live tokens kept at most (the least recently used goes first).
pub const MAX_TOKENS: usize = 1024;
/// Open handles at most.
pub const MAX_HANDLES: usize = 32;
/// A handle nobody read from for this long is closed on the next open.
pub const HANDLE_IDLE_MS: f64 = 10.0 * 60.0 * 1000.0;
/// Largest chunk one read returns.
pub const MAX_READ: usize = 256 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum StreamError {
    #[error("unknown or expired stream token")]
    UnknownToken,
    #[error("unknown stream handle")]
    UnknownHandle,
    #[error("too many open streams")]
    TooManyHandles,
    #[error("no server connection")]
    NoServer,
    #[error("offset is past the end of the stream")]
    RangeNotSatisfiable,
    #[error("server answered HTTP {0}")]
    Status(u16),
    #[error("server sent an error instead of audio")]
    ErrorEnvelope,
    #[error("network: {0}")]
    Network(String),
    #[error("io: {0}")]
    Io(String),
    #[error("stream closed")]
    Closed,
    #[error("core is shut down")]
    ShutDown,
}

/// What an open returned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StreamInfo {
    pub handle: u64,
    /// Where the first byte read comes from (always the requested offset).
    pub offset: u64,
    /// Size of the whole resource, when known (transcodes may not know).
    pub total_length: Option<u64>,
    /// Bytes this handle will return before end of input, when known.
    pub length: Option<u64>,
    pub content_type: Option<String>,
}

// ---------------------------------------------------------------------------
// Upstream seam
// ---------------------------------------------------------------------------

/// One fetch from the server. `range` is a `Range` header value.
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
    pub body: BoxStream<'static, Result<Bytes, String>>,
}

/// How the reader reaches the server: reqwest in production, an in-memory
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
                body,
            })
        })
    }
}

// ---------------------------------------------------------------------------
// Reader
// ---------------------------------------------------------------------------

/// Called with the tracks whose offline state changed (a new cache entry,
/// evictions, deferred removals). Runs on whatever thread closed or
/// finished the stream.
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

struct Handle {
    state: tokio::sync::Mutex<Source>,
    cancel: CancellationToken,
    last_used: Mutex<f64>,
    /// Prefetch: reads yield to busy playback streams.
    background: bool,
    /// Still pulling from the server (cleared at end of input or error).
    network: std::sync::atomic::AtomicBool,
}

/// A playback stream that read from the server this recently still has
/// priority over background prefetch.
const FOREGROUND_BUSY_MS: f64 = 10_000.0;

#[derive(Default)]
struct Handles {
    open: HashMap<u64, Arc<Handle>>,
    /// Opens in progress (counted against [`MAX_HANDLES`]).
    opening: usize,
    next_id: u64,
}

struct Shared {
    downloads: Downloads,
    clock: Arc<dyn Clock>,
    upstream: Arc<dyn StreamUpstream>,
    notify: CacheNotify,
    api: RwLock<Option<Arc<dyn SubsonicApi>>>,
    tokens: Mutex<HashMap<String, Token>>,
    handles: Mutex<Handles>,
    stopped: CancellationToken,
}

/// Handle to the reader. Cheap to clone.
#[derive(Clone)]
pub struct StreamReader {
    shared: Arc<Shared>,
}

impl StreamReader {
    pub fn new(
        downloads: Downloads,
        clock: Arc<dyn Clock>,
        upstream: Arc<dyn StreamUpstream>,
        notify: CacheNotify,
    ) -> StreamReader {
        StreamReader {
            shared: Arc::new(Shared {
                downloads,
                clock,
                upstream,
                notify,
                api: RwLock::new(None),
                tokens: Mutex::new(HashMap::new()),
                handles: Mutex::new(Handles::default()),
                stopped: CancellationToken::new(),
            }),
        }
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

    /// Refuse further opens, forget every token and close every handle.
    pub fn stop(&self) {
        self.shared.stopped.cancel();
        self.shared.tokens.lock().clear();
        let open: Vec<Arc<Handle>> = {
            let mut h = self.shared.handles.lock();
            h.open.drain().map(|(_, v)| v).collect()
        };
        for h in open {
            h.cancel.cancel();
        }
    }

    /// Live tokens (tests, diagnostics).
    pub fn token_count(&self) -> usize {
        self.shared.tokens.lock().len()
    }

    /// Open handles (tests, diagnostics).
    pub fn open_handles(&self) -> usize {
        self.shared.handles.lock().open.len()
    }

    /// Open the stream a `hocket-stream://` URL (or its bare token) stands
    /// for, from `offset`, returning at most `length` bytes when given.
    pub async fn open(
        &self,
        url_or_token: &str,
        offset: u64,
        length: Option<u64>,
    ) -> Result<StreamInfo, StreamError> {
        self.open_with(url_or_token, offset, length, false).await
    }

    /// [`open`](Self::open) for background prefetch: its reads wait while
    /// a playback stream is busy pulling from the server.
    pub async fn open_background(&self, url_or_token: &str) -> Result<StreamInfo, StreamError> {
        self.open_with(url_or_token, 0, None, true).await
    }

    async fn open_with(
        &self,
        url_or_token: &str,
        offset: u64,
        length: Option<u64>,
        background: bool,
    ) -> Result<StreamInfo, StreamError> {
        let sh = &self.shared;
        if sh.stopped.is_cancelled() {
            return Err(StreamError::ShutDown);
        }
        let token = self.lookup(url_or_token)?;
        self.reserve()?;
        let opened = self.open_source(&token, offset, length).await;
        let mut handles = sh.handles.lock();
        handles.opening = handles.opening.saturating_sub(1);
        let (source, info) = opened?;
        if sh.stopped.is_cancelled() {
            return Err(StreamError::ShutDown);
        }
        handles.next_id += 1;
        let id = handles.next_id;
        let network = matches!(source, Source::Upstream(_));
        handles.open.insert(
            id,
            Arc::new(Handle {
                state: tokio::sync::Mutex::new(source),
                cancel: CancellationToken::new(),
                last_used: Mutex::new(sh.clock.now_ms()),
                background,
                network: std::sync::atomic::AtomicBool::new(network),
            }),
        );
        Ok(StreamInfo { handle: id, ..info })
    }

    /// Up to `max_bytes` (capped at [`MAX_READ`]) from the handle; an empty
    /// chunk is end of input.
    pub async fn read(&self, handle: u64, max_bytes: usize) -> Result<Bytes, StreamError> {
        let h = self
            .shared
            .handles
            .lock()
            .open
            .get(&handle)
            .cloned()
            .ok_or(StreamError::UnknownHandle)?;
        if h.background {
            while self.foreground_busy() {
                tokio::select! {
                    _ = h.cancel.cancelled() => return Err(StreamError::Closed),
                    _ = tokio::time::sleep(std::time::Duration::from_millis(100)) => {}
                }
            }
        }
        *h.last_used.lock() = self.shared.clock.now_ms();
        let max = max_bytes.clamp(1, MAX_READ);
        let mut source = tokio::select! {
            _ = h.cancel.cancelled() => return Err(StreamError::Closed),
            s = h.state.lock() => s,
        };
        let r = source.read(max, &h.cancel).await;
        if !matches!(&r, Ok(b) if !b.is_empty()) {
            h.network.store(false, std::sync::atomic::Ordering::Relaxed);
        }
        r
    }

    /// Whether a playback (non-background) stream is still pulling from
    /// the server and was read from recently.
    fn foreground_busy(&self) -> bool {
        let now = self.shared.clock.now_ms();
        self.shared.handles.lock().open.values().any(|h| {
            !h.background
                && h.network.load(std::sync::atomic::Ordering::Relaxed)
                && now - *h.last_used.lock() < FOREGROUND_BUSY_MS
        })
    }

    /// Close the handle. A read in progress returns [`StreamError::Closed`];
    /// an unfinished cache write is discarded. Unknown handles are ignored.
    pub fn close(&self, handle: u64) {
        let h = self.shared.handles.lock().open.remove(&handle);
        if let Some(h) = h {
            h.cancel.cancel();
        }
    }

    fn lookup(&self, url_or_token: &str) -> Result<Token, StreamError> {
        let id = url_or_token
            .strip_prefix(STREAM_URL_PREFIX)
            .unwrap_or(url_or_token)
            .trim_end_matches('/');
        let now = self.shared.clock.now_ms();
        let mut tokens = self.shared.tokens.lock();
        match tokens.get_mut(id) {
            Some(t) if t.expires_at > now => {
                t.expires_at = now + TOKEN_TTL_MS;
                Ok(t.clone())
            }
            Some(_) => {
                tokens.remove(id);
                Err(StreamError::UnknownToken)
            }
            None => Err(StreamError::UnknownToken),
        }
    }

    /// Count an open against the bound, closing idle handles first.
    fn reserve(&self) -> Result<(), StreamError> {
        let now = self.shared.clock.now_ms();
        let mut handles = self.shared.handles.lock();
        let idle: Vec<u64> = handles
            .open
            .iter()
            .filter(|(_, h)| now - *h.last_used.lock() > HANDLE_IDLE_MS)
            .map(|(k, _)| *k)
            .collect();
        for k in idle {
            if let Some(h) = handles.open.remove(&k) {
                h.cancel.cancel();
            }
        }
        if handles.open.len() + handles.opening >= MAX_HANDLES {
            return Err(StreamError::TooManyHandles);
        }
        handles.opening += 1;
        Ok(())
    }

    async fn open_source(
        &self,
        token: &Token,
        offset: u64,
        length: Option<u64>,
    ) -> Result<(Source, StreamInfo), StreamError> {
        let sh = &self.shared;
        let dl = &sh.downloads;
        // 1. A pin finished since the token was minted.
        if let Ok(Some(p)) = dl.downloaded_path(&token.server_id, &token.track_id) {
            return open_file(p, token.mime_type.clone(), offset, length, None).await;
        }
        // 2. A complete stream-cache copy, held while open.
        match dl.cache_lookup(
            &token.server_id,
            &token.track_id,
            token.profile.as_ref(),
            true,
        ) {
            Ok(Some(e)) => {
                dl.acquire_reader(&e.path);
                let guard = ReaderGuard {
                    downloads: dl.clone(),
                    path: e.path.clone(),
                    notify: sh.notify.clone(),
                };
                let ct = e.content_type.or(token.mime_type.clone());
                return open_file(e.path, ct, offset, length, Some(guard)).await;
            }
            Ok(None) => {}
            Err(e) => tracing::warn!(target: "hocket_core", error = %e, "stream cache lookup"),
        }
        // 3. The server.
        let api = sh.api.read().clone();
        let Some(api) = api.filter(|a| a.server_id() == token.server_id) else {
            return Err(StreamError::NoServer);
        };
        self.open_upstream(api.as_ref(), token, offset, length)
            .await
    }

    async fn open_upstream(
        &self,
        api: &dyn SubsonicApi,
        token: &Token,
        offset: u64,
        length: Option<u64>,
    ) -> Result<(Source, StreamInfo), StreamError> {
        let sh = &self.shared;
        let whole_read = offset == 0 && length.is_none();
        let range = match (offset, length) {
            (0, None) => None,
            (o, None) => Some(format!("bytes={o}-")),
            (_, Some(0)) => None,
            (o, Some(n)) => Some(format!("bytes={o}-{}", o + n - 1)),
        };
        let url = api.stream_url(&token.track_id, &stream_options(token.profile.as_ref()));
        let up = sh
            .upstream
            .fetch(UpstreamRequest { url, range })
            .await
            .map_err(|e| {
                tracing::warn!(target: "hocket_core", error = %e, track = %token.track_id, "stream upstream");
                StreamError::Network(e)
            })?;
        if up.status == 416 {
            return Err(StreamError::RangeNotSatisfiable);
        }
        if !(200..300).contains(&up.status) {
            tracing::warn!(target: "hocket_core", status = up.status, track = %token.track_id, "stream upstream status");
            return Err(StreamError::Status(up.status));
        }
        if is_error_envelope(up.content_type.as_deref()) {
            tracing::warn!(target: "hocket_core", track = %token.track_id, content_type = ?up.content_type, "server answered a stream with an error envelope");
            return Err(StreamError::ErrorEnvelope);
        }
        // Where the body starts and how big the whole resource is.
        let (body_start, total) = if up.status == 206 {
            match up.content_range.as_deref().and_then(parse_content_range) {
                Some((s, t)) => (s, t),
                None => (offset, None),
            }
        } else {
            (0, up.content_length)
        };
        if body_start > offset {
            return Err(StreamError::Status(up.status));
        }
        if total.is_some_and(|t| offset > t) {
            return Err(StreamError::RangeNotSatisfiable);
        }
        let skip = offset - body_start;
        let length = match (length, total) {
            (Some(n), Some(t)) => Some(n.min(t - offset)),
            (Some(n), None) => Some(n),
            (None, Some(t)) => Some(t - offset),
            (None, None) => None,
        };
        let whole_body = body_start == 0 && total.is_some_and(|t| up.content_length == Some(t))
            || (up.status == 200 && body_start == 0);
        let budget = sh.downloads.cache_budget();
        let cacheable = whole_read
            && whole_body
            && up
                .content_length
                .is_none_or(|n| n > 0 && (n as f64) <= budget)
            && sh
                .downloads
                .cache_has_room(&token.server_id, up.content_length.unwrap_or(0) as f64);
        let content_type = up.content_type.clone().or(token.mime_type.clone());
        let tee = if cacheable {
            Tee::start(sh, token, up.content_length, content_type.clone()).await
        } else {
            None
        };
        let info = StreamInfo {
            handle: 0,
            offset,
            total_length: total,
            length,
            content_type,
        };
        Ok((
            Source::Upstream(UpstreamSource {
                body: up.body,
                pending: Bytes::new(),
                skip,
                remaining: length,
                eof: false,
                failed: None,
                tee,
            }),
            info,
        ))
    }
}

impl StreamMinter for StreamReader {
    fn mint(&self, target: StreamTarget) -> Option<String> {
        if self.shared.stopped.is_cancelled() {
            return None;
        }
        let now = self.shared.clock.now_ms();
        let mut tokens = self.shared.tokens.lock();
        tokens.retain(|_, t| t.expires_at > now);
        if let Some((id, t)) = tokens.iter_mut().find(|(_, t)| t.same_target(&target)) {
            t.expires_at = now + TOKEN_TTL_MS;
            t.suffix = target.suffix.clone();
            t.mime_type = target.mime_type.clone();
            return Some(format!("{STREAM_URL_PREFIX}{id}"));
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
                suffix: target.suffix,
                mime_type: target.mime_type,
                expires_at: now + TOKEN_TTL_MS,
            },
        );
        Some(format!("{STREAM_URL_PREFIX}{id}"))
    }
}

/// 128 random bits from the OS-seeded CSPRNG, hex.
fn new_token() -> String {
    use rand::Rng;
    let bytes: [u8; 16] = rand::rng().random();
    hex::encode(bytes)
}

// -- sources ------------------------------------------------------------------

enum Source {
    File {
        file: tokio::fs::File,
        remaining: u64,
        /// Held for a stream-cache file; released on close.
        _guard: Option<ReaderGuard>,
    },
    Upstream(UpstreamSource),
}

impl Source {
    async fn read(&mut self, max: usize, cancel: &CancellationToken) -> Result<Bytes, StreamError> {
        match self {
            Source::File {
                file, remaining, ..
            } => {
                if *remaining == 0 {
                    return Ok(Bytes::new());
                }
                let want = (*remaining).min(max as u64) as usize;
                let mut buf = vec![0u8; want];
                let n = tokio::select! {
                    _ = cancel.cancelled() => return Err(StreamError::Closed),
                    r = file.read(&mut buf) => r.map_err(|e| StreamError::Io(e.to_string()))?,
                };
                if n == 0 {
                    return Err(StreamError::Io("file shrank while reading".into()));
                }
                buf.truncate(n);
                *remaining -= n as u64;
                Ok(Bytes::from(buf))
            }
            Source::Upstream(u) => u.read(max, cancel).await,
        }
    }
}

async fn open_file(
    path: PathBuf,
    content_type: Option<String>,
    offset: u64,
    length: Option<u64>,
    guard: Option<ReaderGuard>,
) -> Result<(Source, StreamInfo), StreamError> {
    let mut file = tokio::fs::File::open(&path)
        .await
        .map_err(|e| StreamError::Io(e.to_string()))?;
    let len = file
        .metadata()
        .await
        .map_err(|e| StreamError::Io(e.to_string()))?
        .len();
    if offset > len {
        return Err(StreamError::RangeNotSatisfiable);
    }
    if offset > 0 {
        file.seek(std::io::SeekFrom::Start(offset))
            .await
            .map_err(|e| StreamError::Io(e.to_string()))?;
    }
    let remaining = length.map_or(len - offset, |n| n.min(len - offset));
    Ok((
        Source::File {
            file,
            remaining,
            _guard: guard,
        },
        StreamInfo {
            handle: 0,
            offset,
            total_length: Some(len),
            length: Some(remaining),
            content_type,
        },
    ))
}

/// Releases a cache file's reader hold when the handle goes, running
/// deferred removals.
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

struct UpstreamSource {
    body: BoxStream<'static, Result<Bytes, String>>,
    /// Received but not yet returned.
    pending: Bytes,
    /// Bytes still to drop before the offset (server ignored `Range`).
    skip: u64,
    /// Bytes still to return, when bounded.
    remaining: Option<u64>,
    eof: bool,
    failed: Option<StreamError>,
    tee: Option<Tee>,
}

impl UpstreamSource {
    async fn read(&mut self, max: usize, cancel: &CancellationToken) -> Result<Bytes, StreamError> {
        loop {
            if let Some(e) = &self.failed {
                return Err(e.clone());
            }
            if self.remaining == Some(0) {
                return Ok(Bytes::new());
            }
            if !self.pending.is_empty() {
                let mut n = self.pending.len().min(max);
                if let Some(r) = self.remaining {
                    n = n.min(r as usize);
                }
                let out = self.pending.split_to(n);
                if let Some(r) = &mut self.remaining {
                    *r -= n as u64;
                }
                return Ok(out);
            }
            if self.eof {
                if self.remaining.is_some_and(|r| r > 0) {
                    // The server ended short of what it announced.
                    self.failed = Some(StreamError::Network("stream ended early".into()));
                    continue;
                }
                return Ok(Bytes::new());
            }
            let next = tokio::select! {
                _ = cancel.cancelled() => return Err(StreamError::Closed),
                c = self.body.next() => c,
            };
            match next {
                Some(Ok(mut chunk)) => {
                    if let Some(t) = &mut self.tee {
                        if !t.write(&chunk).await {
                            self.tee = None;
                        }
                    }
                    if self.tee.as_ref().is_some_and(Tee::is_complete) {
                        if let Some(t) = self.tee.take() {
                            t.finish().await;
                        }
                    }
                    if self.skip > 0 {
                        let s = self.skip.min(chunk.len() as u64) as usize;
                        chunk.advance(s);
                        self.skip -= s as u64;
                    }
                    self.pending = chunk;
                }
                Some(Err(e)) => {
                    self.tee = None;
                    self.failed = Some(StreamError::Network(e));
                }
                None => {
                    self.eof = true;
                    if let Some(t) = self.tee.take() {
                        t.finish().await;
                    }
                }
            }
        }
    }
}

/// A cache copy being written alongside a whole read. Dropped unfinished,
/// its temp file is removed.
struct Tee {
    file: Option<tokio::fs::File>,
    tmp: PathBuf,
    dest: PathBuf,
    written: u64,
    expected: Option<u64>,
    token: Token,
    content_type: Option<String>,
    downloads: Downloads,
    notify: CacheNotify,
}

impl Tee {
    async fn start(
        sh: &Shared,
        token: &Token,
        expected: Option<u64>,
        content_type: Option<String>,
    ) -> Option<Tee> {
        let dest = sh.downloads.cache_path_unique(
            &token.server_id,
            &token.track_id,
            token.profile.as_ref(),
            token.suffix.as_deref(),
        );
        let tmp = crate::subsonic::transport::part_path(&dest);
        let created = async {
            if let Some(parent) = tmp.parent() {
                tokio::fs::create_dir_all(parent).await?;
            }
            tokio::fs::File::create(&tmp).await
        }
        .await;
        match created {
            Ok(file) => Some(Tee {
                file: Some(file),
                tmp,
                dest,
                written: 0,
                expected,
                token: token.clone(),
                content_type,
                downloads: sh.downloads.clone(),
                notify: sh.notify.clone(),
            }),
            Err(e) => {
                tracing::warn!(target: "hocket_core", error = %e, "stream cache temp file");
                None
            }
        }
    }

    /// `false` when the copy had to be abandoned.
    async fn write(&mut self, chunk: &[u8]) -> bool {
        let Some(f) = self.file.as_mut() else {
            return false;
        };
        if let Err(e) = f.write_all(chunk).await {
            tracing::warn!(target: "hocket_core", error = %e, "stream cache write");
            return false;
        }
        self.written += chunk.len() as u64;
        if self.expected.is_some_and(|n| self.written > n) {
            // More than the server announced: not a clean body.
            return false;
        }
        true
    }

    /// The announced length has arrived.
    fn is_complete(&self) -> bool {
        self.expected.is_some_and(|n| n == self.written)
    }

    /// Register the copy when it is whole; discard it otherwise.
    async fn finish(mut self) {
        let Some(mut f) = self.file.take() else {
            return;
        };
        let whole = self.written > 0 && self.expected.is_none_or(|n| n == self.written);
        let flushed = whole && f.flush().await.is_ok() && f.sync_all().await.is_ok();
        drop(f);
        if !flushed {
            let _ = tokio::fs::remove_file(&self.tmp).await;
            return;
        }
        if let Err(e) = tokio::fs::rename(&self.tmp, &self.dest).await {
            tracing::warn!(target: "hocket_core", error = %e, "stream cache rename");
            let _ = tokio::fs::remove_file(&self.tmp).await;
            return;
        }
        match self.downloads.cache_put(
            &self.token.server_id,
            &self.token.track_id,
            self.token.profile.as_ref(),
            &self.dest,
            self.written as f64,
            self.content_type.as_deref(),
        ) {
            Ok(changed) => (self.notify)(changed),
            Err(e) => {
                tracing::warn!(target: "hocket_core", error = %e, "stream cache register");
                let _ = tokio::fs::remove_file(&self.dest).await;
            }
        }
    }
}

impl Drop for Tee {
    fn drop(&mut self) {
        if self.file.take().is_some() {
            if let Err(e) = std::fs::remove_file(&self.tmp) {
                if e.kind() != std::io::ErrorKind::NotFound {
                    tracing::warn!(target: "hocket_core", error = %e, "removing stream cache temp file");
                }
            }
        }
    }
}

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

/// `bytes 100-199/1000` (or `/*`) → (start, total).
fn parse_content_range(v: &str) -> Option<(u64, Option<u64>)> {
    let rest = v.trim().strip_prefix("bytes")?.trim();
    let (range, total) = rest.split_once('/')?;
    let (start, _end) = range.split_once('-')?;
    Some((start.trim().parse().ok()?, total.trim().parse().ok()))
}

// ---------------------------------------------------------------------------
// Native backend adapter
// ---------------------------------------------------------------------------

/// The native backend's fetcher: `hocket-stream://` URLs are read through
/// the [`StreamReader`]; anything else goes to `fallback` (HTTP).
#[cfg(feature = "native-audio")]
pub struct CoreStreamFetcher {
    pub reader: StreamReader,
    pub fallback: Arc<dyn crate::audio::native::RangeFetcher>,
}

#[cfg(feature = "native-audio")]
impl crate::audio::native::RangeFetcher for CoreStreamFetcher {
    fn fetch(
        &self,
        url: String,
        headers: HashMap<String, String>,
        start: u64,
    ) -> crate::audio::native::http::FetchFuture {
        use crate::audio::native::http::{FetchError, FetchResponse};
        if !url.starts_with(STREAM_URL_PREFIX) {
            return self.fallback.fetch(url, headers, start);
        }
        let reader = self.reader.clone();
        Box::pin(async move {
            let info = reader.open(&url, start, None).await.map_err(|e| match e {
                StreamError::UnknownToken => FetchError::Status(404),
                StreamError::RangeNotSatisfiable => FetchError::Status(416),
                StreamError::Status(s) => FetchError::Status(s),
                StreamError::ErrorEnvelope => FetchError::Status(502),
                StreamError::Closed | StreamError::ShutDown => FetchError::Cancelled,
                other => FetchError::Network(other.to_string()),
            })?;
            let closer = HandleCloser {
                reader: reader.clone(),
                handle: info.handle,
            };
            let body = futures::stream::unfold(Some(closer), |closer| async move {
                let closer = closer?;
                match closer.reader.read(closer.handle, 64 * 1024).await {
                    Ok(b) if b.is_empty() => None,
                    Ok(b) => Some((Ok(b), Some(closer))),
                    Err(StreamError::Closed) => Some((Err(FetchError::Cancelled), None)),
                    Err(e) => Some((Err(FetchError::Network(e.to_string())), None)),
                }
            });
            Ok(FetchResponse {
                range_start: start,
                total_len: info.total_length,
                supports_ranges: true,
                body: Box::pin(body),
            })
        })
    }
}

/// Closes a reader handle when the native body stream is dropped.
#[cfg(feature = "native-audio")]
struct HandleCloser {
    reader: StreamReader,
    handle: u64,
}

#[cfg(feature = "native-audio")]
impl Drop for HandleCloser {
    fn drop(&mut self) {
        self.reader.close(self.handle);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn envelopes_and_ranges() {
        assert!(is_error_envelope(Some("application/json; charset=utf-8")));
        assert!(is_error_envelope(Some("text/xml")));
        assert!(!is_error_envelope(Some("audio/flac")));
        assert!(!is_error_envelope(None));
        assert_eq!(parse_content_range("bytes 0-9/10"), Some((0, Some(10))));
        assert_eq!(parse_content_range("bytes 5-9/*"), Some((5, None)));
        assert_eq!(parse_content_range("items 5-9/10"), None);
        let a = new_token();
        assert_eq!(a.len(), 32);
        assert_ne!(a, new_token());
    }
}
