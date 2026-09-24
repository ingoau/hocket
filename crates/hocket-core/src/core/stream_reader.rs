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
//! 2. a complete stream-cache entry (the requested profile, or the original
//!    when it can be decoded: a full original serves every quality
//!    setting) → read from disk, held (not evicted or cleared) until the
//!    handle closes;
//! 3. otherwise a *partial* entry: whatever byte spans of (track, profile)
//!    earlier reads left behind are served from disk and only the missing
//!    ranges are fetched from the server (`Range` requests), through a
//!    [`StreamUpstream`] that builds the credentialed URL inside the core.
//!
//! Everything fetched is written into the entry's sparse file at its
//! offset, whatever the open's offset and however the read ends (a seek, a
//! skip, a dropped connection keep their bytes), and the spans are merged
//! into the index ([`Downloads::cache_merge`]); when they cover the whole
//! stream the entry is promoted to complete in place and the actor is told
//! which tracks changed offline state. Fetched bytes must be audio (not a
//! JSON/XML Subsonic error envelope) and every range must agree on the
//! stream's total length and type; a change discards the spans. A cache
//! file the OS removed or truncated is a miss (the row goes, the read falls
//! back to the server), never an error to the player.
//!
//! Offsets are always honoured: when the server ignores `Range` (on-the-fly
//! transcodes) the reader skips to the offset itself. Open handles are
//! bounded ([`MAX_HANDLES`]); handles left idle for [`HANDLE_IDLE_MS`] are
//! closed on the next open.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::Arc;

use bytes::Bytes;
use futures::future::BoxFuture;
use futures::stream::BoxStream;
use futures::StreamExt;
use parking_lot::{Mutex, RwLock};
use tokio::io::{AsyncReadExt, AsyncSeekExt};
use tokio_util::sync::CancellationToken;
use url::Url;

use crate::api::TranscodingProfile;
use crate::downloads::spans::SpanSet;
use crate::downloads::{
    stream_options, CacheRow, Downloads, EntryKey, StreamMinter, StreamTarget, TrackKey,
};
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
    fn fetch(
        &self,
        request: UpstreamRequest,
    ) -> BoxFuture<'static, Result<UpstreamResponse, String>>;
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

    /// [`open_background`](Self::open_background) for a byte range (the
    /// album primer reads only a track's first seconds).
    pub async fn open_background_range(
        &self,
        url_or_token: &str,
        offset: u64,
        length: Option<u64>,
    ) -> Result<StreamInfo, StreamError> {
        self.open_with(url_or_token, offset, length, true).await
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
        let cancel = CancellationToken::new();
        let opened = self
            .open_source(&token, offset, length, background, &cancel)
            .await;
        let mut handles = sh.handles.lock();
        handles.opening = handles.opening.saturating_sub(1);
        let (source, info) = opened?;
        if sh.stopped.is_cancelled() {
            return Err(StreamError::ShutDown);
        }
        handles.next_id += 1;
        let id = handles.next_id;
        let network = source.pulling();
        handles.open.insert(
            id,
            Arc::new(Handle {
                state: tokio::sync::Mutex::new(source),
                cancel,
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
        let pulling = matches!(&r, Ok(b) if !b.is_empty()) && source.pulling();
        h.network.store(pulling, Ordering::Relaxed);
        r
    }

    /// Whether a playback (non-background) stream is still pulling from
    /// the server and was read from recently.
    fn foreground_busy(&self) -> bool {
        let now = self.shared.clock.now_ms();
        self.shared.handles.lock().open.values().any(|h| {
            !h.background
                && h.network.load(Ordering::Relaxed)
                && now - *h.last_used.lock() < FOREGROUND_BUSY_MS
        })
    }

    /// Close the handle. A read in progress returns [`StreamError::Closed`];
    /// what it fetched stays in the cache. Unknown handles are ignored.
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
        let idle: Vec<Arc<Handle>> = {
            let mut handles = self.shared.handles.lock();
            let keys: Vec<u64> = handles
                .open
                .iter()
                .filter(|(_, h)| now - *h.last_used.lock() > HANDLE_IDLE_MS)
                .map(|(k, _)| *k)
                .collect();
            let idle = keys
                .into_iter()
                .filter_map(|k| handles.open.remove(&k))
                .collect();
            if handles.open.len() + handles.opening >= MAX_HANDLES {
                drop(handles);
                for h in &idle {
                    let h: &Arc<Handle> = h;
                    h.cancel.cancel();
                }
                return Err(StreamError::TooManyHandles);
            }
            handles.opening += 1;
            idle
        };
        for h in idle {
            h.cancel.cancel();
        }
        Ok(())
    }

    async fn open_source(
        &self,
        token: &Token,
        offset: u64,
        length: Option<u64>,
        background: bool,
        cancel: &CancellationToken,
    ) -> Result<(Source, StreamInfo), StreamError> {
        let sh = &self.shared;
        let dl = &sh.downloads;
        // 1. A pin finished since the token was minted.
        if let Ok(Some(p)) = dl.downloaded_path(&token.server_id, &token.track_id) {
            let traffic = (!background).then(|| sh.clone());
            return open_file(p, token.mime_type.clone(), offset, length, traffic).await;
        }
        let api = sh
            .api
            .read()
            .clone()
            .filter(|a| a.server_id() == token.server_id);
        let mut src = CacheSource::new(sh.clone(), token.clone(), api, offset, length, background);
        // 2. A complete stream-cache copy (held while open), else 3. the
        // partial spans earlier reads left.
        let mut changed = vec![];
        match dl.cache_complete(
            &token.server_id,
            &token.track_id,
            token.profile.as_ref(),
            true,
        ) {
            Ok((Some(row), c)) => {
                changed.extend(c);
                src.attach(row);
            }
            Ok((None, c)) => {
                changed.extend(c);
                match dl.cache_partial(&token.server_id, &token.track_id, token.profile.as_ref()) {
                    Ok((Some(row), c)) => {
                        changed.extend(c);
                        src.attach(row);
                    }
                    Ok((None, c)) => changed.extend(c),
                    Err(e) => {
                        tracing::warn!(target: "hocket_core", error = %e, "stream cache lookup")
                    }
                }
            }
            Err(e) => tracing::warn!(target: "hocket_core", error = %e, "stream cache lookup"),
        }
        if !changed.is_empty() {
            (sh.notify)(changed);
        }
        if src.api.is_none() && src.entry.is_none() {
            return Err(StreamError::NoServer);
        }
        let info = src.prepare(cancel).await?;
        Ok((Source::Cache(Box::new(src)), info))
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

/// Spans are merged into the index at least this often while writing.
const PERSIST_EVERY: u64 = 4 * 1024 * 1024;

enum Source {
    /// A pinned download.
    File {
        file: tokio::fs::File,
        remaining: u64,
        traffic: Option<Arc<Shared>>,
    },
    /// The stream cache, complete or partial, filled from the server.
    Cache(Box<CacheSource>),
}

impl Source {
    async fn read(&mut self, max: usize, cancel: &CancellationToken) -> Result<Bytes, StreamError> {
        match self {
            Source::File {
                file,
                remaining,
                traffic,
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
                if let Some(sh) = traffic {
                    sh.downloads
                        .traffic()
                        .from_disk
                        .fetch_add(n as u64, Ordering::Relaxed);
                }
                Ok(Bytes::from(buf))
            }
            Source::Cache(c) => c.read(max, cancel).await,
        }
    }

    /// Pulling from the server right now.
    fn pulling(&self) -> bool {
        match self {
            Source::File { .. } => false,
            Source::Cache(c) => c.seg.is_some(),
        }
    }
}

async fn open_file(
    path: PathBuf,
    content_type: Option<String>,
    offset: u64,
    length: Option<u64>,
    traffic: Option<Arc<Shared>>,
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
            traffic,
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

impl ReaderGuard {
    fn new(sh: &Shared, path: &Path) -> ReaderGuard {
        sh.downloads.acquire_reader(path);
        ReaderGuard {
            downloads: sh.downloads.clone(),
            path: path.to_path_buf(),
            notify: sh.notify.clone(),
        }
    }
}

impl Drop for ReaderGuard {
    fn drop(&mut self) {
        let changed = self.downloads.release_reader(&self.path);
        if !changed.is_empty() {
            (self.notify)(changed);
        }
    }
}

/// One fetch from the server covering a gap.
struct Segment {
    body: BoxStream<'static, Result<Bytes, String>>,
    /// Absolute offset of the next byte the body delivers.
    next_abs: u64,
    /// Received and not yet returned, starting at `pending_at`.
    pending: Bytes,
    pending_at: u64,
    /// Stop using this fetch here (the next cached span starts); `None`
    /// reads it to its end.
    stop: Option<u64>,
    eof: bool,
}

/// The cache file a handle reads from and writes into.
struct EntryState {
    key: EntryKey,
    /// Complete (or given up on): read, never written.
    read_only: bool,
    /// Write handle, opened on the first write.
    file: Option<std::fs::File>,
    /// Written (and not yet merged into the index).
    pending: SpanSet,
    pending_bytes: u64,
    /// Nothing was merged and nothing written yet (a failed first fetch
    /// leaves no empty row behind).
    fresh: bool,
    _guard: ReaderGuard,
}

/// A stream served from the cache where it has the bytes and from the
/// server where it does not, writing what it fetches into the cache.
struct CacheSource {
    sh: Arc<Shared>,
    token: Token,
    api: Option<Arc<dyn SubsonicApi>>,
    /// What the bytes are (the token's profile, or the original standing in
    /// for a transcode): gap fetches ask the server for exactly this.
    profile: Option<TranscodingProfile>,
    source_tag: Option<String>,
    background: bool,
    /// Readable from disk.
    known: SpanSet,
    total: Option<u64>,
    content_type: Option<String>,
    pos: u64,
    /// Exclusive end of this handle's range, when known.
    end: Option<u64>,
    requested_length: Option<u64>,
    offset: u64,
    entry: Option<EntryState>,
    /// Whether this handle may create a cache entry.
    may_cache: bool,
    disk: Option<(tokio::fs::File, u64)>,
    seg: Option<Segment>,
    failed: Option<StreamError>,
    /// Bytes already returned (a stream change after that is an error).
    served: u64,
}

impl CacheSource {
    fn new(
        sh: Arc<Shared>,
        token: Token,
        api: Option<Arc<dyn SubsonicApi>>,
        offset: u64,
        length: Option<u64>,
        background: bool,
    ) -> CacheSource {
        let source_tag = sh
            .downloads
            .source_tag(&token.server_id, &token.track_id)
            .ok()
            .flatten();
        CacheSource {
            profile: token.profile.clone(),
            sh,
            token,
            api,
            source_tag,
            background,
            known: SpanSet::new(),
            total: None,
            content_type: None,
            pos: offset,
            end: None,
            requested_length: length,
            offset,
            entry: None,
            may_cache: true,
            disk: None,
            seg: None,
            failed: None,
            served: 0,
        }
    }

    fn attach(&mut self, row: CacheRow) {
        let guard = ReaderGuard::new(&self.sh, &row.path);
        if row.profile != downloads_profile_key(self.profile.as_ref()) {
            self.profile = crate::downloads::profile_from_key(&row.profile);
        }
        self.known = row.spans.clone();
        self.total = row.total;
        self.content_type = row.content_type.clone();
        self.entry = Some(EntryState {
            key: row.key(),
            read_only: row.complete,
            file: None,
            pending: SpanSet::new(),
            pending_bytes: 0,
            fresh: false,
            _guard: guard,
        });
    }

    fn update_end(&mut self) {
        let from_len = self.requested_length.map(|n| self.offset + n);
        self.end = match (from_len, self.total) {
            (Some(a), Some(t)) => Some(a.min(t)),
            (a, t) => a.or(t),
        };
    }

    /// Get ready to read at `offset`: from disk when it is cached,
    /// otherwise open the first fetch now (so a server error surfaces at
    /// open).
    async fn prepare(&mut self, cancel: &CancellationToken) -> Result<StreamInfo, StreamError> {
        if self.total.is_some_and(|t| self.offset > t) {
            return Err(StreamError::RangeNotSatisfiable);
        }
        self.update_end();
        let at_end = self.end.is_some_and(|e| self.pos >= e);
        if !at_end && self.known.end_at(self.pos).is_none() {
            self.open_segment(cancel).await?;
        }
        Ok(StreamInfo {
            handle: 0,
            offset: self.offset,
            total_length: self.total,
            length: self.end.map(|e| e.saturating_sub(self.offset)),
            content_type: self.content_type.clone().or(self.token.mime_type.clone()),
        })
    }

    async fn read(&mut self, max: usize, cancel: &CancellationToken) -> Result<Bytes, StreamError> {
        loop {
            if let Some(e) = &self.failed {
                return Err(e.clone());
            }
            if self.end.is_some_and(|e| self.pos >= e) {
                self.persist();
                return Ok(Bytes::new());
            }
            if let Some(mut seg) = self.seg.take() {
                if seg.stop.is_some_and(|s| self.pos >= s) || seg.pending_at > self.pos {
                    self.persist();
                    continue;
                }
                if !seg.pending.is_empty() {
                    let mut n = seg.pending.len().min(max) as u64;
                    if let Some(e) = self.end {
                        n = n.min(e - self.pos);
                    }
                    if let Some(s) = seg.stop {
                        n = n.min(s - self.pos);
                    }
                    let out = seg.pending.split_to(n as usize);
                    seg.pending_at += n;
                    self.pos += n;
                    self.served += n;
                    self.seg = Some(seg);
                    return Ok(out);
                }
                if seg.eof {
                    let ended_at = seg.next_abs;
                    let short = match (seg.stop, self.total) {
                        (Some(s), _) => ended_at < s,
                        (None, Some(t)) => ended_at < t,
                        (None, None) => false,
                    };
                    if short {
                        self.persist();
                        self.failed = Some(StreamError::Network("stream ended early".into()));
                        continue;
                    }
                    if seg.stop.is_none() && self.total.is_none() {
                        // The server never said how long: now we know.
                        self.total = Some(ended_at);
                        self.update_end();
                    }
                    self.persist();
                    continue;
                }
                let next = tokio::select! {
                    _ = cancel.cancelled() => {
                        self.seg = Some(seg);
                        return Err(StreamError::Closed);
                    }
                    c = seg.body.next() => c,
                };
                match next {
                    Some(Ok(chunk)) => {
                        let at = seg.next_abs;
                        seg.next_abs += chunk.len() as u64;
                        let t = self.sh.downloads.traffic();
                        let counter = if self.background {
                            &t.fetched_background
                        } else {
                            &t.fetched
                        };
                        counter.fetch_add(chunk.len() as u64, Ordering::Relaxed);
                        self.write(at, &chunk);
                        let end = at + chunk.len() as u64;
                        if end > self.pos {
                            let skip = self.pos.saturating_sub(at) as usize;
                            seg.pending = chunk.slice(skip..);
                            seg.pending_at = at + skip as u64;
                        }
                        self.seg = Some(seg);
                    }
                    Some(Err(e)) => {
                        self.persist();
                        self.failed = Some(StreamError::Network(e));
                    }
                    None => {
                        seg.eof = true;
                        self.seg = Some(seg);
                    }
                }
                continue;
            }
            if let Some(span_end) = self.known.end_at(self.pos) {
                match self.read_disk(span_end, max, cancel).await {
                    Ok(b) => return Ok(b),
                    Err(StreamError::Closed) => return Err(StreamError::Closed),
                    Err(e) => {
                        tracing::info!(target: "hocket_core", error = %e, track = %self.token.track_id, "stream cache file unreadable; falling back to the server");
                        self.drop_entry();
                        continue;
                    }
                }
            }
            if let Err(e) = self.open_segment(cancel).await {
                self.failed = Some(e);
            }
        }
    }

    async fn read_disk(
        &mut self,
        span_end: u64,
        max: usize,
        cancel: &CancellationToken,
    ) -> Result<Bytes, StreamError> {
        let io = |e: std::io::Error| StreamError::Io(e.to_string());
        let mut want = (span_end - self.pos).min(max as u64);
        if let Some(e) = self.end {
            want = want.min(e - self.pos);
        }
        let path = match &self.entry {
            Some(e) => e.key.path.clone(),
            None => return Err(StreamError::Io("no cache file".into())),
        };
        let (mut file, at) = match self.disk.take() {
            Some(d) => d,
            None => (tokio::fs::File::open(&path).await.map_err(io)?, u64::MAX),
        };
        if at != self.pos {
            file.seek(std::io::SeekFrom::Start(self.pos))
                .await
                .map_err(io)?;
        }
        let mut buf = vec![0u8; want as usize];
        let n = tokio::select! {
            _ = cancel.cancelled() => return Err(StreamError::Closed),
            r = file.read(&mut buf) => r.map_err(io)?,
        };
        if n == 0 {
            return Err(StreamError::Io("cache file shorter than its index".into()));
        }
        buf.truncate(n);
        self.pos += n as u64;
        self.served += n as u64;
        self.disk = Some((file, self.pos));
        if !self.background {
            self.sh
                .downloads
                .traffic()
                .from_disk
                .fetch_add(n as u64, Ordering::Relaxed);
        }
        Ok(Bytes::from(buf))
    }

    /// The cache file cannot be read: forget the entry (a miss, never an
    /// error to the player) and fetch from the server from here on.
    fn drop_entry(&mut self) {
        self.disk = None;
        self.known.clear();
        if let Some(e) = self.entry.take() {
            let dl = &self.sh.downloads;
            let row = dl
                .cache_rows(&e.key.server_id, &e.key.track_id)
                .ok()
                .and_then(|rows| rows.into_iter().find(|r| r.path == e.key.path));
            if let Some(row) = row {
                let mut changed = vec![];
                if let Err(err) = dl.drop_row(&row, &mut changed) {
                    tracing::warn!(target: "hocket_core", error = %err, "dropping unreadable cache entry");
                }
                if !changed.is_empty() {
                    (self.sh.notify)(changed);
                }
            }
        }
    }

    /// Fetch the gap at `pos` (up to the next cached span or the end).
    async fn open_segment(&mut self, cancel: &CancellationToken) -> Result<(), StreamError> {
        let Some(api) = self.api.clone() else {
            return Err(StreamError::NoServer);
        };
        let pos = self.pos;
        let gap_end = match (self.known.next_start_after(pos), self.end) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        };
        // To the end of the stream: open-ended (what a plain seek sends).
        let bounded = gap_end.filter(|e| Some(*e) != self.total);
        let range = match (pos, bounded) {
            (0, None) => None,
            (p, None) => Some(format!("bytes={p}-")),
            (p, Some(e)) => Some(format!("bytes={p}-{}", e.max(p + 1) - 1)),
        };
        let url = api.stream_url(&self.token.track_id, &stream_options(self.profile.as_ref()));
        let fetch = self.sh.upstream.fetch(UpstreamRequest { url, range });
        let up = tokio::select! {
            _ = cancel.cancelled() => return Err(StreamError::Closed),
            r = fetch => r.map_err(|e| {
                tracing::warn!(target: "hocket_core", error = %e, track = %self.token.track_id, "stream upstream");
                StreamError::Network(e)
            })?,
        };
        if up.status == 416 {
            return Err(StreamError::RangeNotSatisfiable);
        }
        if !(200..300).contains(&up.status) {
            tracing::warn!(target: "hocket_core", status = up.status, track = %self.token.track_id, "stream upstream status");
            return Err(StreamError::Status(up.status));
        }
        if is_error_envelope(up.content_type.as_deref()) {
            tracing::warn!(target: "hocket_core", track = %self.token.track_id, content_type = ?up.content_type, "server answered a stream with an error envelope");
            return Err(StreamError::ErrorEnvelope);
        }
        // Where the body starts and how big the whole resource is.
        let (body_start, total) = if up.status == 206 {
            match up.content_range.as_deref().and_then(parse_content_range) {
                Some((s, t)) => (s, t),
                None => (pos, None),
            }
        } else {
            (0, up.content_length)
        };
        if body_start > pos {
            return Err(StreamError::Status(up.status));
        }
        if total.is_some_and(|t| pos > t) {
            return Err(StreamError::RangeNotSatisfiable);
        }
        // The same stream as the cached spans came from?
        let changed_len = matches!((self.total, total), (Some(a), Some(b)) if a != b);
        let changed_type = match (&self.content_type, &up.content_type) {
            (Some(a), Some(b)) => base_type(a) != base_type(b),
            _ => false,
        };
        if changed_len || changed_type {
            tracing::info!(target: "hocket_core", track = %self.token.track_id, "the server's stream changed; discarding its cached spans");
            self.known.clear();
            self.disk = None;
            if let Some(e) = &mut self.entry {
                e.pending.clear();
                e.pending_bytes = 0;
                if let Err(err) =
                    self.sh
                        .downloads
                        .cache_reset(&e.key, total, up.content_type.as_deref())
                {
                    tracing::warn!(target: "hocket_core", error = %err, "resetting cache entry");
                }
                e.read_only = false;
            }
            self.total = total;
            self.content_type = up.content_type.clone();
            self.update_end();
            if self.served > 0 {
                // Bytes of the old stream already went out: the player
                // must start over.
                return Err(StreamError::Network(
                    "the stream changed on the server".into(),
                ));
            }
        } else {
            self.total = self.total.or(total);
            self.content_type = self.content_type.clone().or(up.content_type.clone());
            self.update_end();
        }
        let whole = up.status == 200;
        self.ensure_entry();
        self.seg = Some(Segment {
            body: up.body,
            next_abs: body_start,
            pending: Bytes::new(),
            pending_at: pos,
            stop: if whole { None } else { gap_end },
            eof: false,
        });
        Ok(())
    }

    /// Start writing into a cache entry when this stream may be cached.
    fn ensure_entry(&mut self) {
        if self.entry.is_some() || !self.may_cache || self.sh.stopped.is_cancelled() {
            return;
        }
        self.may_cache = false;
        let dl = &self.sh.downloads;
        let fits = self
            .total
            .is_none_or(|t| t > 0 && (t as f64) <= dl.cache_budget())
            && dl.cache_has_room(&self.token.server_id, self.total.unwrap_or(0) as f64);
        if !fits {
            return;
        }
        let suffix = if self.profile == self.token.profile {
            self.token.suffix.clone()
        } else {
            None
        };
        match dl.cache_begin(
            &self.token.server_id,
            &self.token.track_id,
            self.profile.as_ref(),
            suffix.as_deref(),
            self.total,
            self.content_type.as_deref(),
            self.source_tag.as_deref(),
        ) {
            Ok(Some(row)) => {
                let guard = ReaderGuard::new(&self.sh, &row.path);
                let fresh = row.spans.is_empty();
                self.entry = Some(EntryState {
                    key: row.key(),
                    read_only: false,
                    file: None,
                    pending: SpanSet::new(),
                    pending_bytes: 0,
                    fresh,
                    _guard: guard,
                });
            }
            Ok(None) => {}
            Err(e) => tracing::warn!(target: "hocket_core", error = %e, "stream cache entry"),
        }
    }

    /// Write fetched bytes at their offset into the entry's file.
    fn write(&mut self, at: u64, chunk: &[u8]) {
        let total = self.total;
        let Some(e) = self.entry.as_mut() else {
            return;
        };
        if e.read_only || chunk.is_empty() {
            return;
        }
        let mut data = chunk;
        if let Some(t) = total {
            if at >= t {
                return;
            }
            if at + data.len() as u64 > t {
                // More than the server announced: not a clean body.
                tracing::warn!(target: "hocket_core", track = %self.token.track_id, "stream body longer than announced; not caching it");
                e.pending.clear();
                e.pending_bytes = 0;
                self.stop_writing();
                return;
            }
            data = &data[..(t - at).min(data.len() as u64) as usize];
        }
        let written = (|| -> std::io::Result<()> {
            use std::io::{Seek, Write};
            if e.file.is_none() {
                e.file = Some(std::fs::OpenOptions::new().write(true).open(&e.key.path)?);
            }
            let f = e.file.as_mut().expect("opened above");
            f.seek(std::io::SeekFrom::Start(at))?;
            f.write_all(data)
        })();
        if let Err(err) = written {
            tracing::warn!(target: "hocket_core", error = %err, "stream cache write");
            self.stop_writing();
            return;
        }
        e.fresh = false;
        e.pending.insert(at, at + data.len() as u64);
        e.pending_bytes += data.len() as u64;
        if e.pending_bytes >= PERSIST_EVERY {
            self.persist();
        }
    }

    /// Keep what was merged, write nothing more.
    fn stop_writing(&mut self) {
        self.persist();
        if let Some(e) = &mut self.entry {
            e.file = None;
            e.read_only = true;
        }
    }

    /// Merge what was written into the index (flushed to disk first).
    fn persist(&mut self) {
        let total = self.total;
        let ct = self.content_type.clone();
        let Some(e) = self.entry.as_mut() else {
            return;
        };
        if e.pending.is_empty() {
            return;
        }
        if let Some(f) = &e.file {
            if let Err(err) = f.sync_data() {
                tracing::warn!(target: "hocket_core", error = %err, "stream cache sync");
                e.pending.clear();
                e.pending_bytes = 0;
                return;
            }
        }
        let add = std::mem::take(&mut e.pending);
        e.pending_bytes = 0;
        match self
            .sh
            .downloads
            .cache_merge(&e.key, &add, total, ct.as_deref())
        {
            Ok(out) => {
                if !out.accepted {
                    // The entry was dropped or replaced meanwhile.
                    e.file = None;
                    e.read_only = true;
                } else {
                    self.known.union(&add);
                    if out.complete {
                        e.read_only = true;
                        e.file = None;
                    }
                }
                if !out.changed.is_empty() {
                    (self.sh.notify)(out.changed);
                }
            }
            Err(err) => tracing::warn!(target: "hocket_core", error = %err, "stream cache index"),
        }
    }
}

impl Drop for CacheSource {
    fn drop(&mut self) {
        self.persist();
        if let Some(e) = &self.entry {
            if e.fresh {
                if let Err(err) = self.sh.downloads.cache_discard_if_empty(&e.key) {
                    tracing::warn!(target: "hocket_core", error = %err, "dropping empty cache entry");
                }
            }
        }
    }
}

fn downloads_profile_key(p: Option<&TranscodingProfile>) -> String {
    crate::downloads::profile_key(p)
}

/// `audio/flac; x=y` → `audio/flac`.
fn base_type(ct: &str) -> String {
    ct.split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase()
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

    fn reader() -> (
        tempfile::TempDir,
        Downloads,
        StreamReader,
        Arc<crate::core::test_support::FakeUpstream>,
    ) {
        let dir = tempfile::tempdir().unwrap();
        let db = crate::db::Db::open_in_memory().unwrap();
        let server = crate::subsonic::fake::FakeServer::new("srv", "alice");
        let c = crate::subsonic::fake::FakeServer::song("t0", "T0", "al0", "ar0", 100.0);
        server.add_song(c.clone());
        db.upsert_tracks(
            &[crate::subsonic::convert::track_from_child("srv", &c)],
            &[],
            1,
        )
        .unwrap();
        let dl = Downloads::new(
            db,
            Arc::new(crate::util::WallClock),
            Arc::new(crate::downloads::UnknownStorage),
            &dir.path().join("data"),
            &dir.path().join("cache"),
            crate::api::Platform::Linux,
        );
        let upstream = Arc::new(crate::core::test_support::FakeUpstream::new());
        let r = StreamReader::new(
            dl.clone(),
            Arc::new(crate::util::WallClock),
            upstream.clone(),
            Arc::new(|_| {}),
        );
        r.set_api(Some(Arc::new(server)));
        (dir, dl, r, upstream)
    }

    fn target() -> StreamTarget {
        StreamTarget {
            server_id: "srv".into(),
            track_id: "t0".into(),
            profile: None,
            suffix: Some("flac".into()),
            mime_type: Some("audio/flac".into()),
        }
    }

    /// The native backend's fetcher reads `hocket-stream://` through the
    /// reader (a whole read caches; an offset read starts there) and
    /// passes anything else to its HTTP fallback.
    #[cfg(feature = "native-audio")]
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn native_fetcher_reads_through_the_reader() {
        use crate::audio::native::{MemoryFetcher, RangeFetcher};
        let (_dir, dl, r, upstream) = reader();
        let media: Vec<u8> = (0..200_000u32).map(|i| (i % 7) as u8).collect();
        upstream.set_media("t0", media.clone(), "audio/flac");
        let url = r.mint(target()).unwrap();
        assert!(url.starts_with(STREAM_URL_PREFIX));
        let fallback = Arc::new(MemoryFetcher::new(vec![1, 2, 3]));
        let f = CoreStreamFetcher {
            reader: r.clone(),
            fallback: fallback.clone(),
        };
        let collect = |resp: crate::audio::native::http::FetchResponse| async move {
            let mut out = vec![];
            let mut body = resp.body;
            while let Some(c) = body.next().await {
                out.extend_from_slice(&c.unwrap());
            }
            out
        };
        let resp = f.fetch(url.clone(), HashMap::new(), 0).await.unwrap();
        assert_eq!(resp.total_len, Some(200_000));
        assert_eq!(collect(resp).await, media);
        assert_eq!(r.open_handles(), 0, "the body's end closed the handle");
        let e = dl.cache_lookup("srv", "t0", None, false).unwrap().unwrap();
        assert_eq!(e.bytes, 200_000);
        let resp = f.fetch(url.clone(), HashMap::new(), 150_000).await.unwrap();
        assert_eq!(resp.range_start, 150_000);
        assert_eq!(collect(resp).await, &media[150_000..]);
        assert_eq!(upstream.stream_requests("t0"), 1, "second read from disk");
        // Dropping a body mid-way closes the handle.
        let resp = f.fetch(url, HashMap::new(), 0).await.unwrap();
        assert_eq!(r.open_handles(), 1);
        drop(resp);
        assert_eq!(r.open_handles(), 0);
        // Other URLs use the HTTP fetcher.
        let resp = f
            .fetch("https://example.test/a.flac".into(), HashMap::new(), 0)
            .await
            .unwrap();
        assert_eq!(collect(resp).await, vec![1, 2, 3]);
        assert_eq!(fallback.requests.lock().len(), 1);
        let err = match f
            .fetch(format!("{STREAM_URL_PREFIX}nope"), HashMap::new(), 0)
            .await
        {
            Err(e) => e,
            Ok(_) => panic!("unknown token opened"),
        };
        assert_eq!(err, crate::audio::native::http::FetchError::Status(404));
    }

    /// Open handles are bounded, and a stopped reader refuses everything.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn handles_are_bounded_and_stop_closes_them() {
        let (_dir, _dl, r, _upstream) = reader();
        let url = r.mint(target()).unwrap();
        let mut open = vec![];
        for _ in 0..MAX_HANDLES {
            open.push(r.open(&url, 5, None).await.unwrap().handle);
        }
        assert_eq!(
            r.open(&url, 5, None).await.unwrap_err(),
            StreamError::TooManyHandles
        );
        r.close(open.pop().unwrap());
        let h = r.open(&url, 5, None).await.unwrap().handle;
        r.stop();
        assert_eq!(r.read(h, 10).await.unwrap_err(), StreamError::UnknownHandle);
        assert_eq!(
            r.open(&url, 0, None).await.unwrap_err(),
            StreamError::ShutDown
        );
        assert!(r.mint(target()).is_none());
    }

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
