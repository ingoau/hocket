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
//! stream's total length and type; a change discards the spans, except a
//! shorter real length the spans all fit in (an estimate corrected, below),
//! which keeps them. A cache
//! file the OS removed or truncated is a miss (the row goes, the read falls
//! back to the server), never an error to the player.
//!
//! Offsets are always honoured: when the server ignores `Range` (on-the-fly
//! transcodes) the reader skips to the offset itself. An offset at or past
//! the known end is an empty stream, not an error. A transcode Navidrome
//! has not cached yet is served unseekable with an estimated length
//! (`estimateContentLength`) that can overshoot, so a player told the
//! estimate resumes or seeks past the real end. Once cached, the server's
//! 416 says how long it really is; uncached, the stream sent again ends
//! exactly where the reader was asked to start. Either is the end of the
//! stream, at its real length. Open handles are bounded ([`MAX_HANDLES`]); handles left idle for [`HANDLE_IDLE_MS`] are
//! closed on the next open.
//!
//! A playback handle (not background prefetch) whose stream is being cached
//! fetches the rest of it in one burst, ahead of the player
//! ([`StreamReader::pump`]): the player reads it back from disk, so the
//! radio sleeps for the rest of the track instead of waking for each of the
//! player's buffer refills.
//!
//! A playback read does not wait out a dead connection: when the server
//! has sent nothing for [`STALL_WAIT`] (or the body fails or ends short)
//! the fetch is dropped and asked again from the read position, until the
//! read has gone [`UPSTREAM_WAIT`] without a byte; then it fails with a
//! network error the player reports. A fetch that can't be opened at all
//! fails the read at once: the player retries. [`StreamReader::reconnect_stalled`] asks again at once
//! (play pressed on a starved track). Background reads keep the long wait.

use std::collections::HashMap;
use std::future::Future;
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
    CacheRow, Downloads, EntryKey, MergeOutcome, StreamMinter, StreamTarget, TrackKey,
};
use crate::provider::{Provider, ProviderError, ProviderSet, StreamRequest};
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
/// How long a read waits for the server's next bytes (or a fetch for its
/// response) before the handle fails with a network error. Counted only
/// while a read is waiting: a player that has filled its buffer and stops
/// reading for minutes (Media3 loads in bursts) must not find the stream
/// timed out behind its back, which the HTTP client's own read timeout did.
pub const UPSTREAM_WAIT: std::time::Duration = std::time::Duration::from_secs(60);
/// A playback read whose fetch has sent nothing for this long drops it and
/// asks the server again from the read position (a stalled connection
/// would otherwise sit out the whole [`UPSTREAM_WAIT`]).
pub const STALL_WAIT: std::time::Duration = std::time::Duration::from_secs(8);
/// How long a playback read waits for the server to answer a fetch.
pub const RESPONSE_WAIT: std::time::Duration = std::time::Duration::from_secs(20);
/// Before asking again after a playback read's fetch failed at once.
const RETRY_PAUSE: std::time::Duration = std::time::Duration::from_secs(1);
/// [`StreamReader::reconnect_stalled`] leaves reads that only just started
/// waiting alone.
const KICK_MIN_WAIT: std::time::Duration = std::time::Duration::from_secs(1);

/// The reader's timeouts (shortened in tests).
#[derive(Debug, Clone, Copy)]
pub struct Waits {
    /// [`UPSTREAM_WAIT`].
    pub give_up: std::time::Duration,
    /// [`STALL_WAIT`].
    pub stall: std::time::Duration,
    /// [`RESPONSE_WAIT`].
    pub response: std::time::Duration,
    /// [`RETRY_PAUSE`].
    pub retry: std::time::Duration,
}

impl Default for Waits {
    fn default() -> Self {
        Waits {
            give_up: UPSTREAM_WAIT,
            stall: STALL_WAIT,
            response: RESPONSE_WAIT,
            retry: RETRY_PAUSE,
        }
    }
}

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
    /// Headers the provider's URL needs (googlevideo: the client's User-Agent).
    pub headers: Vec<(String, String)>,
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
/// as long as it plays) and no read timeout either: the reader applies
/// [`UPSTREAM_WAIT`] only while it is actually waiting for bytes, whereas
/// the client's read timeout also ran while nobody read the body.
pub struct ReqwestUpstream {
    client: reqwest::Client,
}

impl ReqwestUpstream {
    pub fn new() -> Result<Self, String> {
        let client = reqwest::Client::builder()
            .user_agent(concat!("hocket/", env!("CARGO_PKG_VERSION")))
            .connect_timeout(std::time::Duration::from_secs(15))
            .redirect(reqwest::redirect::Policy::none())
            .use_rustls_tls()
            // Audio bytes as the server has them: lengths and ranges count those.
            .no_gzip()
            .no_brotli()
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
            for (k, v) in &request.headers {
                b = b.header(k.as_str(), v.as_str());
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
    duration_ms: Option<f64>,
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
    /// The burst pump ([`StreamReader::pump`]) is running for this handle.
    pumping: std::sync::atomic::AtomicBool,
    /// No new pump before this (after a fetch the pump could not open).
    pump_after: Mutex<f64>,
    /// Reads waiting for the source: the pump lets them have it.
    wake: Wake,
}

/// How a read tells the pump it wants the source.
#[derive(Default)]
struct Wake {
    readers: std::sync::atomic::AtomicUsize,
    notify: tokio::sync::Notify,
}

impl Wake {
    /// Resolves once a read is waiting (at once if one already is).
    async fn wanted(&self) {
        loop {
            let n = self.notify.notified();
            tokio::pin!(n);
            n.as_mut().enable();
            if self.readers.load(Ordering::SeqCst) > 0 {
                return;
            }
            n.await;
        }
    }

    /// Count a read as waiting until the guard drops.
    fn waiting(&self) -> WaitingRead<'_> {
        self.readers.fetch_add(1, Ordering::SeqCst);
        self.notify.notify_waiters();
        WaitingRead(self)
    }
}

struct WaitingRead<'a>(&'a Wake);

impl Drop for WaitingRead<'_> {
    fn drop(&mut self) {
        self.0.readers.fetch_sub(1, Ordering::SeqCst);
    }
}

/// A playback stream that read from the server this recently still has
/// priority over background prefetch.
const FOREGROUND_BUSY_MS: f64 = 10_000.0;
/// After the pump failed to open a fetch, a read restarts it no sooner.
const PUMP_RETRY_MS: f64 = 30_000.0;
/// A stream is fetched ahead only once it has been open this long: a song
/// preview (held for a few seconds) or a stream skipped at once never
/// downloads the whole track. The player's own first buffer covers the
/// wait, and the pump starts at its next read.
const PUMP_START_DELAY_MS: f64 = 20_000.0;
/// How much playing time the pump fetches past the read position when
/// [`Downloads::fetch_ahead_limited`] (a metered network): enough for the
/// radio to sleep for minutes, without pulling a whole track the listener
/// may skip, pause or never reach (Media3 preloads the next item).
const METERED_AHEAD_MS: f64 = 5.0 * 60.0 * 1000.0;
/// The bit rate assumed for that horizon when neither the stream's length
/// nor a transcoding bit rate says (320 kbps).
const ASSUMED_BYTES_PER_MS: f64 = 320.0 / 8.0;

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
    providers: RwLock<ProviderSet>,
    tokens: Mutex<HashMap<String, Token>>,
    handles: Mutex<Handles>,
    stopped: CancellationToken,
    waits: RwLock<Waits>,
    /// [`StreamReader::reconnect_stalled`] wakes reads waiting on the server.
    kick: tokio::sync::Notify,
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
                providers: RwLock::new(ProviderSet::default()),
                tokens: Mutex::new(HashMap::new()),
                handles: Mutex::new(Handles::default()),
                stopped: CancellationToken::new(),
                waits: RwLock::new(Waits::default()),
                kick: tokio::sync::Notify::new(),
            }),
        }
    }

    /// Replace the timeouts (tests).
    pub fn set_waits(&self, waits: Waits) {
        *self.shared.waits.write() = waits;
    }

    /// Playback reads that have been waiting on the server for more than a
    /// moment drop their fetch and ask again from where they are now,
    /// instead of waiting for [`STALL_WAIT`]: the listener pressed play on
    /// a track that is starved, so the connection is probably dead.
    pub fn reconnect_stalled(&self) {
        self.shared.kick.notify_waiters();
    }

    /// The providers upstream fetches use, by source. Tokens minted for a
    /// source no provider serves any more are revoked.
    pub fn set_providers(&self, providers: ProviderSet) {
        let sources = providers.sources();
        *self.shared.providers.write() = providers;
        self.shared
            .tokens
            .lock()
            .retain(|_, t| sources.contains(&t.server_id));
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
        let token = match self.lookup(url_or_token) {
            Ok(t) => t,
            Err(e) => {
                tracing::info!(target: "hocket::stream", offset, error = %e, "stream open refused");
                return Err(e);
            }
        };
        if let Err(e) = self.reserve() {
            tracing::info!(target: "hocket::stream", track = %token.track_id, offset, error = %e, "stream open refused");
            return Err(e);
        }
        // Counted against the bound until it settles, however it ends: an
        // open given up on (its future dropped) frees its slot too.
        let mut reserved = Reserved(Some(sh.as_ref()));
        let cancel = CancellationToken::new();
        let opened = self
            .open_source(&token, offset, length, background, &cancel)
            .await;
        if let Err(e) = &opened {
            if matches!(e, StreamError::Closed | StreamError::ShutDown) {
                tracing::debug!(target: "hocket::stream", track = %token.track_id, offset, error = %e, "stream open failed");
            } else {
                tracing::info!(target: "hocket::stream", track = %token.track_id, offset, error = %e, "stream open failed");
            }
        }
        reserved.0 = None;
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
                pumping: std::sync::atomic::AtomicBool::new(false),
                pump_after: Mutex::new(sh.clock.now_ms() + PUMP_START_DELAY_MS),
                wake: Wake::default(),
            }),
        );
        let h = handles.open.get(&id).cloned();
        drop(handles);
        if let Some(h) = h {
            if let Ok(src) = h.state.try_lock() {
                self.maybe_pump(&h, &src);
            }
        }
        // Where the first read comes from: a fetch already under way, or
        // local bytes (cached, or a download). A later fetch logs itself.
        tracing::debug!(
            target: "hocket::stream",
            track = %token.track_id,
            offset,
            handle = id,
            background,
            first_read = if network { "server" } else { "local" },
            "stream opened"
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
        // A pump waiting on the server lets go of the source.
        let waiting = h.wake.waiting();
        let mut source = tokio::select! {
            _ = h.cancel.cancelled() => return Err(StreamError::Closed),
            s = h.state.lock() => s,
        };
        drop(waiting);
        let r = source.read(max, &h.cancel).await;
        let pulling = matches!(&r, Ok(b) if !b.is_empty()) && source.pulling();
        h.network.store(pulling, Ordering::Relaxed);
        if r.is_ok() {
            self.maybe_pump(&h, &source);
        }
        r
    }

    /// Start the burst pump for a playback handle that has more of its
    /// stream to fetch into the cache (see [`pump`](Self::pump)).
    fn maybe_pump(&self, h: &Arc<Handle>, source: &Source) {
        let Source::Cache(c) = source else {
            return;
        };
        if h.background
            || !c.wants_pump(true)
            || *h.pump_after.lock() > self.shared.clock.now_ms()
            || h.pumping.swap(true, Ordering::AcqRel)
        {
            return;
        }
        tokio::spawn(self.clone().pump(h.clone()));
    }

    /// Fetch the rest of a playing stream into the cache in one go, ahead
    /// of the player: a player that reads in bursts (Media3 buffers a
    /// couple of minutes) would otherwise wake the radio for each one, and
    /// the mobile radio stays in its high-power state for seconds after
    /// every transfer. The player then reads what the pump wrote from disk.
    ///
    /// Only for an open-ended playback (not background) handle whose
    /// stream is being cached (it fits the budget): nothing is held in
    /// memory beyond the chunk in flight. It runs whatever the network
    /// (the bytes are the track being played, which the player would fetch
    /// anyway), from the handle's position to the end of the stream (or of
    /// the server's body when the length is unknown), skipping spans the
    /// cache already has. A read always goes first: the pump gives the
    /// source up while it waits on the server. Closing the handle (a skip,
    /// a seek) stops it; what it fetched stays cached. A failure the reader
    /// would have reported (a dropped connection, a body ending short) is
    /// left for the reader to meet at the byte where it happened.
    async fn pump(self, h: Arc<Handle>) {
        tracing::debug!(target: "hocket::stream", "fetching the rest of the stream ahead of the player");
        // Where a refused URL was last resolved again: a second refusal
        // there waits for the retry.
        let mut refused_at: Option<u64> = None;
        loop {
            let mut src = tokio::select! {
                _ = h.cancel.cancelled() => break,
                s = h.state.lock() => s,
            };
            let Source::Cache(c) = &mut *src else {
                break;
            };
            let step = c.pump_step(&h.cancel, &h.wake).await;
            let step = match step {
                PumpStep::Open(open) => {
                    // The fetch opens without the source: reads go on from
                    // disk meanwhile, and none of them aborts it.
                    let track = c.token.track_id.clone();
                    drop(src);
                    let wait = self.shared.waits.read().give_up;
                    let up =
                        fetch_upstream(&self.shared, open.request, &h.cancel, &track, wait).await;
                    let mut src = tokio::select! {
                        _ = h.cancel.cancelled() => break,
                        s = h.state.lock() => s,
                    };
                    let Source::Cache(c) = &mut *src else {
                        break;
                    };
                    match up {
                        Err(StreamError::Closed) => PumpStep::Done,
                        Err(e) => {
                            tracing::debug!(target: "hocket::stream", track = %track, at = open.from, error = %e, "could not fetch ahead of the player");
                            PumpStep::Retry
                        }
                        // The reader opened this gap itself meanwhile (it
                        // caught up): its fetch serves it.
                        Ok(_) if c.seg.is_some() || c.fill_from() != Some(open.from) => {
                            PumpStep::More
                        }
                        // An expired URL: forgotten, so the next step
                        // resolves the same gap again.
                        Ok(up)
                            if refused_at != Some(open.from)
                                && c.forget_refused_url(up.status, open.from) =>
                        {
                            refused_at = Some(open.from);
                            PumpStep::More
                        }
                        Ok(up) => match c.attach_segment(open.from, open.gap_end, up) {
                            Ok(()) => PumpStep::More,
                            Err(e) => {
                                tracing::debug!(target: "hocket::stream", track = %track, at = open.from, error = %e, "could not fetch ahead of the player");
                                PumpStep::Retry
                            }
                        },
                    }
                }
                step => {
                    drop(src);
                    step
                }
            };
            match step {
                PumpStep::More | PumpStep::Yield | PumpStep::Open(_) => {}
                PumpStep::Done => break,
                PumpStep::Retry => {
                    *h.pump_after.lock() = self.shared.clock.now_ms() + PUMP_RETRY_MS;
                    break;
                }
            }
            tokio::task::yield_now().await;
        }
        h.pumping.store(false, Ordering::Release);
    }

    /// Whether every byte of the stream a `hocket-stream://` URL stands for
    /// is on this device: a download, or the complete cache entry an open
    /// would serve for its profile. Playing it needs no network.
    pub fn is_local(&self, url_or_token: &str) -> bool {
        let Ok(token) = self.lookup(url_or_token) else {
            return false;
        };
        let dl = &self.shared.downloads;
        if let Ok(Some(_)) = dl.downloaded_path(&token.server_id, &token.track_id) {
            return true;
        }
        match dl.cache_complete(
            &token.server_id,
            &token.track_id,
            token.profile.as_ref(),
            false,
        ) {
            Ok((row, changed)) => {
                if !changed.is_empty() {
                    (self.shared.notify)(changed);
                }
                row.is_some()
            }
            Err(_) => false,
        }
    }

    /// Whether a playback (non-background) stream is still pulling from
    /// the server and was read from recently, or is fetching ahead of its
    /// player (the playing track finishes first).
    fn foreground_busy(&self) -> bool {
        let now = self.shared.clock.now_ms();
        self.shared.handles.lock().open.values().any(|h| {
            !h.background
                && (h.pumping.load(Ordering::Relaxed)
                    || h.network.load(Ordering::Relaxed)
                        && now - *h.last_used.lock() < FOREGROUND_BUSY_MS)
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
        if !idle.is_empty() {
            tracing::debug!(target: "hocket::stream", closed = idle.len(), "closing stream handles left idle");
        }
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
        let api = sh.providers.read().for_source(&token.server_id).cloned();
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
            t.duration_ms = target.duration_ms;
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
                duration_ms: target.duration_ms,
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
    // Past the end: an empty stream (see the module docs).
    let start = offset.min(len);
    if start > 0 {
        file.seek(std::io::SeekFrom::Start(start))
            .await
            .map_err(|e| StreamError::Io(e.to_string()))?;
    }
    let remaining = length.map_or(len - start, |n| n.min(len - start));
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
    /// Asked for from here, but the server ignored the `Range` and sent the
    /// whole stream (`Some` only then, for an offset past 0).
    skip_to: Option<u64>,
    /// The body failed while the pump read it ahead: the reader reports it
    /// once it gets here.
    error: Option<String>,
}

impl Segment {
    /// The body ended (cleanly or not) exactly where the range it ignored
    /// began: a transcode served with an estimated length, asked again at
    /// the real end where the last one stopped (see the module docs). A
    /// dropped connection landing on that very byte is not plausible.
    fn ended_at_skip(&self) -> bool {
        self.skip_to.is_some_and(|p| self.next_abs == p)
    }
}

/// An open in progress, released from [`Handles::opening`] when dropped
/// unsettled (the open's future was dropped).
struct Reserved<'a>(Option<&'a Shared>);

impl Drop for Reserved<'_> {
    fn drop(&mut self) {
        if let Some(sh) = self.0 {
            let mut h = sh.handles.lock();
            h.opening = h.opening.saturating_sub(1);
        }
    }
}

/// How waiting for a segment's next chunk ended.
enum Wait {
    Chunk(Option<Result<Bytes, String>>),
    Closed,
    /// Nothing for too long, or a reconnect was asked for.
    Stalled(&'static str),
}

/// What one [`CacheSource::pump_step`] did.
enum PumpStep {
    /// Fetched (or settled) something; go on.
    More,
    /// A read wants the source.
    Yield,
    /// Nothing more to fetch ahead (or left for the reader to deal with).
    Done,
    /// A fetch could not be opened: try again later.
    Retry,
    /// Open this fetch (outside the source lock), then attach it.
    Open(PendingOpen),
}

/// A gap fetch the pump opens: from `from` up to `gap_end`.
struct PendingOpen {
    from: u64,
    gap_end: Option<u64>,
    request: UpstreamRequest,
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
    /// The last index merge failed: no more merges every
    /// [`PERSIST_EVERY`] (a sync and a transaction per chunk) until the
    /// next one at a segment end or the close.
    merge_failed: bool,
    _guard: ReaderGuard,
}

type SegmentResult = Result<(UpstreamRequest, Option<u64>), StreamError>;

/// A stream served from the cache where it has the bytes and from the
/// server where it does not, writing what it fetches into the cache.
struct CacheSource {
    sh: Arc<Shared>,
    token: Token,
    api: Option<Arc<dyn Provider>>,
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
    /// What this handle fetched and wrote: read back from disk, it saved
    /// no data.
    fetched_here: SpanSet,
    /// Since when a playback read has had no byte from the server (it
    /// gives up at [`Waits::give_up`]).
    starved_since: Option<tokio::time::Instant>,
    /// The last fetch failed at once: wait [`Waits::retry`] before the next.
    retry_due: bool,
}

impl CacheSource {
    fn new(
        sh: Arc<Shared>,
        token: Token,
        api: Option<Arc<dyn Provider>>,
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
            fetched_here: SpanSet::new(),
            starved_since: None,
            retry_due: false,
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
            merge_failed: false,
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
        self.update_end();
        let at_end = self.end.is_some_and(|e| self.pos >= e);
        if !at_end && self.known.end_at(self.pos).is_none() {
            self.open_segment(self.pos, cancel).await?;
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
            // The pump took the segment past us: what it fetched is on disk.
            let ahead =
                self.seg.as_ref().is_some_and(|s| {
                    s.pending_at > self.pos && s.stop.is_none_or(|st| self.pos < st)
                }) && self.known.end_at(self.pos).is_some();
            if let Some(mut seg) = self.seg.take_if(|_| !ahead) {
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
                if let Some(e) = seg.error.take() {
                    self.persist();
                    if self.retry_after(&e) {
                        self.retry_due = true;
                    } else {
                        self.failed = Some(StreamError::Network(e));
                    }
                    continue;
                }
                if seg.eof {
                    let ended_at = seg.next_abs;
                    if seg.ended_at_skip() && self.total.is_some_and(|t| ended_at < t) {
                        tracing::info!(target: "hocket_core", track = %self.token.track_id, "the server's stream ended where it was asked to start; that is its end");
                        self.shorten(ended_at, true);
                        continue;
                    }
                    let short = match (seg.stop, self.total) {
                        (Some(s), _) => ended_at < s,
                        (None, Some(t)) => ended_at < t,
                        (None, None) => false,
                    };
                    if short {
                        tracing::info!(target: "hocket::stream", track = %self.token.track_id, at = ended_at, expected = ?seg.stop.or(self.total), "the server's stream ended early");
                        self.persist();
                        if self.retry_after("stream ended early") {
                            self.retry_due = true;
                        } else {
                            self.failed = Some(StreamError::Network("stream ended early".into()));
                        }
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
                let next = match self.next_chunk(&mut seg, cancel).await {
                    Wait::Chunk(c) => c,
                    Wait::Closed => {
                        self.seg = Some(seg);
                        return Err(StreamError::Closed);
                    }
                    Wait::Stalled(why) => {
                        // Dropped (what it wrote stays cached): the loop
                        // reads on from disk or asks the server again.
                        tracing::info!(target: "hocket::stream", track = %self.token.track_id, at = seg.next_abs, pos = self.pos, why, "the server's stream stalled; asking again");
                        drop(seg);
                        self.persist();
                        if !self.retry_after(why) {
                            self.failed = Some(StreamError::Network(format!(
                                "the server's stream stalled ({why})"
                            )));
                        }
                        continue;
                    }
                };
                match next {
                    Some(Ok(chunk)) => {
                        let at = seg.next_abs;
                        seg.next_abs += chunk.len() as u64;
                        if at + chunk.len() as u64 > self.pos {
                            // Progress where the player is (not a 200's
                            // re-sent prefix before it).
                            self.starved_since = None;
                        }
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
                    Some(Err(_)) if seg.ended_at_skip() => {
                        // Read as the end of the stream (above).
                        seg.eof = true;
                        self.seg = Some(seg);
                    }
                    Some(Err(e)) => {
                        tracing::info!(target: "hocket::stream", track = %self.token.track_id, at = seg.next_abs, error = %e, "the server's stream failed");
                        self.persist();
                        if self.retry_after(&e) {
                            self.retry_due = true;
                        } else {
                            self.failed = Some(StreamError::Network(e));
                        }
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
            if std::mem::take(&mut self.retry_due) {
                // The last fetch failed at once: not again straight away
                // (at once when asked to reconnect).
                let pause = self.sh.waits.read().retry;
                let kick = self.sh.kick.notified();
                tokio::select! {
                    _ = cancel.cancelled() => return Err(StreamError::Closed),
                    _ = tokio::time::sleep(pause) => {}
                    _ = kick => {}
                }
            }
            // A fetch that can't be opened fails the read: the player's own
            // retry (the native downloader, Media3's error handling) takes
            // over, and a blocked read would hold up its seeks and skips.
            if let Err(e) = self.open_segment(self.pos, cancel).await {
                self.failed = Some(e);
            }
        }
    }

    /// A playback read's fetch stalled or failed (`why`): whether to ask
    /// the server again, which it does until it has gone
    /// [`Waits::give_up`] without a byte. Background reads don't.
    fn retry_after(&mut self, why: &str) -> bool {
        if self.background || self.sh.stopped.is_cancelled() || self.api.is_none() {
            return false;
        }
        let now = tokio::time::Instant::now();
        let since = *self.starved_since.get_or_insert(now);
        let give_up = self.sh.waits.read().give_up;
        if now.duration_since(since) >= give_up {
            tracing::info!(target: "hocket::stream", track = %self.token.track_id, pos = self.pos, why, "no data from the server; giving up");
            self.starved_since = None;
            return false;
        }
        true
    }

    /// The segment's next chunk, waiting at most [`Waits::stall`] for it in
    /// a playback read ([`Waits::give_up`] in a background one).
    async fn next_chunk(&mut self, seg: &mut Segment, cancel: &CancellationToken) -> Wait {
        let waits = *self.sh.waits.read();
        let wait = if self.background {
            waits.give_up
        } else {
            waits.stall
        };
        let started = tokio::time::Instant::now();
        // Silence counts from the first wait (a retry resumes the count).
        let since = self.starved_since.unwrap_or(started);
        let deadline = started + wait;
        let next = seg.body.next();
        tokio::pin!(next);
        loop {
            let kick = self.sh.kick.notified();
            tokio::select! {
                _ = cancel.cancelled() => return Wait::Closed,
                c = &mut next => return Wait::Chunk(c),
                _ = tokio::time::sleep_until(deadline) => {
                    if self.background {
                        return Wait::Chunk(Some(Err(format!("no data from the server for {}s", wait.as_secs()))));
                    }
                    self.starved_since = Some(since);
                    return Wait::Stalled("no data");
                }
                _ = kick, if !self.background => {
                    if started.elapsed() >= KICK_MIN_WAIT {
                        self.starved_since = Some(since);
                        return Wait::Stalled("asked to reconnect");
                    }
                }
            }
        }
    }

    /// The first byte at or after the read position the cache lacks, when
    /// it lacks one before the end.
    fn fill_from(&self) -> Option<u64> {
        let mut p = self.pos;
        while let Some(e) = self.known.end_at(p) {
            p = e;
        }
        match self.end {
            Some(e) if p >= e => None,
            _ => Some(p),
        }
    }

    /// Whether [`pump_step`](Self::pump_step) has anything to do: an
    /// open-ended playback read whose bytes go into a writable cache entry,
    /// with some of the stream still to fetch within the horizon and
    /// nothing pending for the reader to resolve. `start`: whether to start
    /// the pump (it starts with half the metered horizon left, so it runs
    /// in few long bursts, and runs to all of it).
    fn wants_pump(&self, start: bool) -> bool {
        !self.background
            && self.requested_length.is_none()
            && self.failed.is_none()
            && self.api.is_some()
            && !self.sh.stopped.is_cancelled()
            && self.entry.as_ref().is_some_and(|e| !e.read_only)
            && self
                .seg
                .as_ref()
                // An ended body is settled by the step (it may give the
                // stream its length); one that failed is the reader's.
                .is_none_or(|s| s.error.is_none() && !(start && s.eof))
            && self.fill_from().is_some_and(|from| {
                self.horizon().is_none_or(|h| {
                    let ahead = from.saturating_sub(self.pos) as f64;
                    ahead < if start { h / 2.0 } else { h }
                })
            })
    }

    /// How far past the read position the pump may fetch, in bytes; `None`
    /// is to the end ([`METERED_AHEAD_MS`] of playing time when the network
    /// is metered).
    fn horizon(&self) -> Option<f64> {
        if !self.sh.downloads.fetch_ahead_limited() {
            return None;
        }
        let from_length = match (self.total, self.token.duration_ms) {
            (Some(t), Some(d)) if d > 0.0 => Some(t as f64 / d),
            _ => None,
        };
        let from_profile = self
            .profile
            .as_ref()
            .and_then(|p| p.max_bit_rate)
            .map(|kbps| kbps as f64 / 8.0);
        let per_ms = from_length.or(from_profile).unwrap_or(ASSUMED_BYTES_PER_MS);
        Some(per_ms * METERED_AHEAD_MS)
    }

    /// One step of the burst pump ([`StreamReader::pump`]): settle what the
    /// reader has not taken from the segment (it is on disk), then pull the
    /// next chunk into the cache, or open a fetch for the next gap.
    async fn pump_step(&mut self, cancel: &CancellationToken, wake: &Wake) -> PumpStep {
        if !self.wants_pump(false) {
            // All fetched (or nothing more to do): into the index now, so
            // the entry completes while the player is still reading.
            self.persist();
            return PumpStep::Done;
        }
        let Some(mut seg) = self.seg.take() else {
            let Some(from) = self.fill_from() else {
                return PumpStep::Done;
            };
            return match self.segment_request(from).await {
                Ok((request, gap_end)) => PumpStep::Open(PendingOpen {
                    from,
                    gap_end,
                    request,
                }),
                Err(_) => PumpStep::Retry,
            };
        };
        if seg.stop.is_some_and(|s| seg.next_abs >= s) && !seg.eof {
            // A bounded fetch has what it was asked for.
            seg.eof = true;
        }
        if !seg.pending.is_empty() {
            let (a, b) = (seg.pending_at, seg.pending_at + seg.pending.len() as u64);
            if self.known.end_at(a).is_none_or(|e| e < b) {
                // Not on disk: the reader takes it from memory.
                self.seg = Some(seg);
                return PumpStep::Done;
            }
            seg.pending = Bytes::new();
            seg.pending_at = b;
        }
        if seg.eof {
            let ended_at = seg.next_abs;
            let short = match (seg.stop, self.total) {
                (Some(s), _) => ended_at < s,
                (None, Some(t)) => ended_at < t,
                (None, None) => false,
            };
            if short || ended_at < seg.pending_at {
                // An estimate, a body ending early: the reader decides
                // what that means when it gets here.
                self.seg = Some(seg);
                return PumpStep::Done;
            }
            if seg.stop.is_none() && self.total.is_none() {
                self.total = Some(ended_at);
                self.update_end();
            }
            self.persist();
            return PumpStep::More;
        }
        let give_up = self.sh.waits.read().give_up;
        let next = tokio::select! {
            _ = cancel.cancelled() => {
                self.seg = Some(seg);
                return PumpStep::Done;
            }
            _ = wake.wanted() => {
                self.seg = Some(seg);
                return PumpStep::Yield;
            }
            c = tokio::time::timeout(give_up, seg.body.next()) => match c {
                Ok(c) => c,
                Err(_) => Some(Err(format!("no data from the server for {}s", give_up.as_secs()))),
            },
        };
        match next {
            Some(Ok(chunk)) => {
                let at = seg.next_abs;
                let end = at + chunk.len() as u64;
                seg.next_abs = end;
                self.sh
                    .downloads
                    .traffic()
                    .fetched
                    .fetch_add(chunk.len() as u64, Ordering::Relaxed);
                self.write(at, &chunk);
                if end > seg.pending_at {
                    let from = seg.pending_at.max(at);
                    if self.known.end_at(from).is_none_or(|e| e < end) {
                        // Not stored (the write failed): hand it to the
                        // reader from memory and stop here.
                        let skip = (from - at) as usize;
                        seg.pending = chunk.slice(skip..);
                        seg.pending_at = from;
                        self.seg = Some(seg);
                        return PumpStep::Done;
                    }
                    seg.pending_at = end;
                }
                self.seg = Some(seg);
                PumpStep::More
            }
            Some(Err(_)) if seg.ended_at_skip() => {
                seg.eof = true;
                self.seg = Some(seg);
                PumpStep::More
            }
            Some(Err(e)) => {
                tracing::info!(target: "hocket::stream", track = %self.token.track_id, at = seg.next_abs, error = %e, "the server's stream failed ahead of the player");
                seg.error = Some(e);
                self.persist();
                self.seg = Some(seg);
                PumpStep::Done
            }
            None => {
                seg.eof = true;
                self.seg = Some(seg);
                PumpStep::More
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
        let (from, to) = (self.pos, self.pos + n as u64);
        self.pos = to;
        self.served += n as u64;
        self.disk = Some((file, self.pos));
        if !self.background {
            let ours: u64 = self
                .fetched_here
                .spans()
                .iter()
                .map(|&(a, b)| b.min(to).saturating_sub(a.max(from)))
                .sum();
            self.sh
                .downloads
                .traffic()
                .from_disk
                .fetch_add(n as u64 - ours, Ordering::Relaxed);
        }
        Ok(Bytes::from(buf))
    }

    /// The cache file cannot be read: forget the entry (a miss, never an
    /// error to the player) and fetch from the server from here on.
    fn drop_entry(&mut self) {
        self.disk = None;
        self.known.clear();
        self.fetched_here.clear();
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

    /// Fetch the gap at `pos` (up to the next cached span or the end): the
    /// read position, or further on for the pump.
    async fn open_segment(
        &mut self,
        pos: u64,
        cancel: &CancellationToken,
    ) -> Result<(), StreamError> {
        let waits = *self.sh.waits.read();
        let wait = if self.background {
            waits.give_up
        } else {
            waits.response
        };
        let (request, gap_end) = self.segment_request(pos).await?;
        let mut up = fetch_upstream(&self.sh, request, cancel, &self.token.track_id, wait).await?;
        let mut gap_end = gap_end;
        if self.forget_refused_url(up.status, pos) {
            let (request, end) = self.segment_request(pos).await?;
            gap_end = end;
            up = fetch_upstream(&self.sh, request, cancel, &self.token.track_id, wait).await?;
        }
        self.attach_segment(pos, gap_end, up)
    }

    /// A refused stream URL (403/410: googlevideo's expire after ~6 h and
    /// are bound to this IP): the provider forgets it, so the next request
    /// for the stream resolves a fresh one. Whether it was one. Shared by
    /// the reader and the pump.
    fn forget_refused_url(&self, status: u16, at: u64) -> bool {
        if !matches!(status, 403 | 410) {
            return false;
        }
        let Some(api) = &self.api else {
            return false;
        };
        tracing::info!(target: "hocket::stream", track = %self.token.track_id, status, at, "stream URL refused; resolving it again");
        api.stream_failed(&self.token.track_id, status);
        true
    }

    /// The request for the gap at `pos`, and where the gap ends.
    /// The request for the gap at `pos`, and where the gap ends. Resolving
    /// the provider's stream happens outside `self` (a handle isn't `Sync`).
    fn segment_request(&self, pos: u64) -> impl Future<Output = SegmentResult> + Send + 'static {
        let api = self.api.clone();
        let gap_end = match (self.known.next_start_after(pos), self.end) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        };
        let total = self.total;
        let first = self.served == 0 && pos == 0;
        let req = StreamRequest {
            profile: self.profile.clone(),
            offset_s: None,
            suffix: self.token.suffix.clone(),
        };
        let track = self.token.track_id.clone();
        let source = self.token.server_id.clone();
        let downloads = self.sh.downloads.clone();
        async move {
            let Some(api) = api else {
                return Err(StreamError::NoServer);
            };
            let stream = api
                .resolve_stream(&track, &req)
                .await
                .map_err(|e| match e {
                    ProviderError::Network(m) => StreamError::Network(m),
                    other => StreamError::Io(other.to_string()),
                })?;
            if let Some(g) = stream.gain_db {
                downloads.note_provider_gain(&source, &track, g);
            }
            // A provider that throttles big requests (googlevideo) gets
            // bounded ranges: a small first one so playback starts fast.
            let gap_end = match stream.max_range {
                Some(max) => {
                    let max = if first {
                        max.min(crate::ytm::provider::FIRST_RANGE)
                    } else {
                        max
                    };
                    let cap = pos + max.max(1);
                    let end = gap_end.map_or(cap, |e| e.min(cap));
                    Some(match total.or(stream.content_length) {
                        Some(t) => end.min(t),
                        None => end,
                    })
                }
                None => gap_end,
            };
            // To the end of the stream: open-ended (what a plain seek sends).
            let bounded = gap_end.filter(|e| stream.max_range.is_some() || Some(*e) != total);
            let range = match (pos, bounded) {
                (0, None) => None,
                (p, None) => Some(format!("bytes={p}-")),
                (p, Some(e)) => Some(format!("bytes={p}-{}", e.max(p + 1) - 1)),
            };
            tracing::debug!(target: "hocket::stream", track = %track, ?range, "fetching from the server");
            let headers = stream.headers.into_iter().collect();
            Ok((
                UpstreamRequest {
                    url: stream.url,
                    range,
                    headers,
                },
                gap_end,
            ))
        }
    }

    /// Start reading the server's answer to the gap fetch at `pos`.
    fn attach_segment(
        &mut self,
        pos: u64,
        gap_end: Option<u64>,
        up: UpstreamResponse,
    ) -> Result<(), StreamError> {
        if up.status == 416 {
            // Past the end. When the server says how long the stream really
            // is (`bytes */N`), that is the end of input, as Media3's own
            // HTTP source treats it: the length the player was told may have
            // been an estimate (see the module docs).
            let real = up.content_range.as_deref().and_then(content_range_total);
            let Some(real) = real.filter(|t| pos >= *t) else {
                return Err(StreamError::RangeNotSatisfiable);
            };
            self.shorten(real, false);
            return Ok(());
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
            self.discard_spans(total, up.content_type.as_deref());
            self.total = total;
            self.content_type = up.content_type.clone();
            self.update_end();
            if self.served > 0 {
                // Bytes of the old stream already went out: the player
                // must start over (also when the pump found out).
                let e = StreamError::Network("the stream changed on the server".into());
                self.failed = Some(e.clone());
                return Err(e);
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
            stop: if whole {
                None
            } else {
                gap_end.map(|g| self.total.map_or(g, |t| g.min(t)))
            },
            eof: false,
            skip_to: (whole && pos > 0).then_some(pos),
            error: None,
        });
        Ok(())
    }

    /// The stream is `real` bytes, less than it was announced with (an
    /// estimate): end there. The cached spans stay when they all fit (the
    /// same bytes; only the length was a guess), and may now complete the
    /// entry. `alone`: keep them only while no other handle holds the
    /// entry, whose writes may have come from another run of the transcode.
    fn shorten(&mut self, real: u64, alone: bool) {
        if self.total != Some(real) {
            let kept = match &self.entry {
                None => Some(MergeOutcome::default()),
                Some(e) if alone && self.sh.downloads.readers_of(&e.key.path) > 1 => None,
                Some(e) => self
                    .sh
                    .downloads
                    .cache_shorten(&e.key, real)
                    .unwrap_or_else(|err| {
                        tracing::warn!(target: "hocket_core", error = %err, "shortening cache entry");
                        None
                    }),
            };
            match kept {
                Some(out) => {
                    tracing::info!(target: "hocket_core", track = %self.token.track_id, real, "the server's stream is shorter than announced");
                    if let Some(e) = &mut self.entry {
                        if out.complete || !out.accepted {
                            e.read_only = true;
                            e.file = None;
                        }
                        if !out.accepted {
                            e.pending.clear();
                            e.pending_bytes = 0;
                        }
                    }
                    if !out.changed.is_empty() {
                        (self.sh.notify)(out.changed);
                    }
                }
                None => {
                    tracing::info!(target: "hocket_core", track = %self.token.track_id, real, "the server's stream is shorter than announced; discarding its cached spans");
                    self.discard_spans(Some(real), None);
                }
            }
        }
        self.total = Some(real);
        self.update_end();
        // What this handle wrote and has not merged lands under the real
        // length (and may complete the entry).
        self.persist();
    }

    /// Forget the cached spans: the server's stream is not the one they came
    /// from (another length or type). The entry starts over at `total`.
    fn discard_spans(&mut self, total: Option<u64>, content_type: Option<&str>) {
        self.known.clear();
        self.fetched_here.clear();
        self.disk = None;
        if let Some(e) = &mut self.entry {
            e.pending.clear();
            e.pending_bytes = 0;
            let reset = self
                .sh
                .downloads
                .cache_reset(&e.key, total, content_type)
                .unwrap_or_else(|err| {
                    tracing::warn!(target: "hocket_core", error = %err, "resetting cache entry");
                    false
                });
            // Not reset (complete already, or gone): write nothing into it.
            e.read_only = !reset;
            if !reset {
                e.file = None;
            }
        }
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
                    merge_failed: false,
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
        let full = e.pending_bytes >= PERSIST_EVERY && !e.merge_failed;
        // Readable from disk at once (another handle on the file sees the
        // write): the pump's bytes are served from there.
        self.known.insert(at, at + data.len() as u64);
        self.fetched_here.insert(at, at + data.len() as u64);
        if full {
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
                e.merge_failed = false;
                if !out.accepted {
                    // The entry was dropped or replaced meanwhile: none of
                    // its spans is ours to read any more.
                    e.file = None;
                    e.read_only = true;
                    self.known.clear();
                    self.disk = None;
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
            Err(err) => {
                // Still written: merged by the next persist (end of the
                // segment, the handle closing) instead of lost to the index.
                tracing::warn!(target: "hocket_core", error = %err, "stream cache index");
                e.pending.union(&add);
                e.pending_bytes += add.bytes();
                e.merge_failed = true;
            }
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

/// Send a gap fetch and wait (at most `wait`) for the answer.
async fn fetch_upstream(
    sh: &Shared,
    request: UpstreamRequest,
    cancel: &CancellationToken,
    track: &str,
    wait: std::time::Duration,
) -> Result<UpstreamResponse, StreamError> {
    let fetch = sh.upstream.fetch(request);
    tokio::select! {
        _ = cancel.cancelled() => Err(StreamError::Closed),
        r = tokio::time::timeout(wait, fetch) => r
            .unwrap_or_else(|_| Err(format!("no response from the server for {}s", wait.as_secs())))
            .map_err(|e| {
                tracing::warn!(target: "hocket_core", error = %e, track = %track, "stream upstream");
                StreamError::Network(e)
            }),
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

/// The total of a `Content-Range` (`bytes a-b/N` or a 416's `bytes */N`).
fn content_range_total(v: &str) -> Option<u64> {
    v.trim()
        .strip_prefix("bytes")?
        .split_once('/')?
        .1
        .trim()
        .parse()
        .ok()
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
        r.set_providers(crate::provider::ProviderSet::single(Arc::new(
            server.provider(),
        )));
        (dir, dl, r, upstream)
    }

    fn target() -> StreamTarget {
        StreamTarget {
            server_id: "srv".into(),
            track_id: "t0".into(),
            profile: None,
            suffix: Some("flac".into()),
            mime_type: Some("audio/flac".into()),
            duration_ms: None,
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

    fn quick_waits(stall_ms: u64, give_up_ms: u64) -> Waits {
        Waits {
            give_up: std::time::Duration::from_millis(give_up_ms),
            stall: std::time::Duration::from_millis(stall_ms),
            response: std::time::Duration::from_millis(give_up_ms),
            retry: std::time::Duration::from_millis(50),
        }
    }

    async fn read_all(r: &StreamReader, handle: u64) -> Result<Vec<u8>, StreamError> {
        let mut out = vec![];
        loop {
            let b = r.read(handle, MAX_READ).await?;
            if b.is_empty() {
                return Ok(out);
            }
            out.extend_from_slice(&b);
        }
    }

    /// Issue #215: a playback read whose connection stalls asks the server
    /// again from where it is after the stall wait, not the long one.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_stalled_playback_read_asks_again_from_where_it_is() {
        use crate::core::test_support::UpstreamBehaviour;
        let (_dir, _dl, r, upstream) = reader();
        r.set_waits(quick_waits(200, 30_000));
        let media: Vec<u8> = (0..200_000u32).map(|i| (i % 251) as u8).collect();
        upstream.set_media("t0", media.clone(), "audio/flac");
        upstream.set_behaviour("t0", UpstreamBehaviour::StallOnceAfter(40_000));
        let url = r.mint(target()).unwrap();
        let started = std::time::Instant::now();
        let h = r.open(&url, 0, None).await.unwrap().handle;
        assert_eq!(read_all(&r, h).await.unwrap(), media);
        assert!(started.elapsed() < std::time::Duration::from_secs(5));
        let calls = upstream.calls();
        assert_eq!(calls.len(), 2, "{calls:?}");
        assert_eq!(calls[1].range.as_deref(), Some("bytes=40000-"));
        r.close(h);
    }

    /// [`StreamReader::reconnect_stalled`] (play pressed on a starved
    /// track) asks again at once, long before the stall wait.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn reconnect_stalled_asks_again_at_once() {
        use crate::core::test_support::UpstreamBehaviour;
        let (_dir, _dl, r, upstream) = reader();
        r.set_waits(quick_waits(60_000, 120_000));
        let media: Vec<u8> = (0..100_000u32).map(|i| (i % 249) as u8).collect();
        upstream.set_media("t0", media.clone(), "audio/flac");
        upstream.set_behaviour("t0", UpstreamBehaviour::StallOnceAfter(30_000));
        let url = r.mint(target()).unwrap();
        let h = r.open(&url, 0, None).await.unwrap().handle;
        let r2 = r.clone();
        let rest = tokio::spawn(async move { read_all(&r2, h).await });
        tokio::time::sleep(KICK_MIN_WAIT).await;
        assert!(!rest.is_finished());
        // Kicked until it reconnects (a read only just waiting is left be).
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while !rest.is_finished() {
            assert!(std::time::Instant::now() < deadline, "the kick reconnects");
            r.reconnect_stalled();
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
        let got = rest.await.unwrap().unwrap();
        assert_eq!(got, media);
        assert_eq!(upstream.calls()[1].range.as_deref(), Some("bytes=30000-"));
        r.close(h);
    }

    /// A stalled read whose server can no longer be reached fails as soon
    /// as asking again fails (the player retries), not after the long wait.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn an_unreachable_server_fails_a_starved_read_in_bounded_time() {
        use crate::core::test_support::UpstreamBehaviour;
        let (_dir, _dl, r, upstream) = reader();
        r.set_waits(quick_waits(100, 30_000));
        let media: Vec<u8> = (0..100_000u32).map(|i| (i % 247) as u8).collect();
        upstream.set_media("t0", media, "audio/flac");
        upstream.set_behaviour("t0", UpstreamBehaviour::StallAfter(20_000));
        let url = r.mint(target()).unwrap();
        let h = r.open(&url, 0, None).await.unwrap().handle;
        let mut got = 0;
        while got < 20_000 {
            got += r.read(h, MAX_READ).await.unwrap().len();
        }
        upstream.set_behaviour("t0", UpstreamBehaviour::Unreachable);
        let started = std::time::Instant::now();
        let err = r.read(h, MAX_READ).await.unwrap_err();
        assert!(matches!(err, StreamError::Network(_)), "{err:?}");
        let took = started.elapsed();
        assert!(took < std::time::Duration::from_secs(3), "{took:?}");
        assert_eq!(upstream.calls().len(), 2, "asked again once");
        // Opening it again fails within the response wait too.
        let err = r.open(&url, 20_000, None).await.unwrap_err();
        assert!(matches!(err, StreamError::Network(_)), "{err:?}");
        r.close(h);
    }

    /// A server that answers but never sends fails a starved read once it
    /// has gone the give-up time without a byte, and asking it again is
    /// paced, not a hot loop.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_server_that_keeps_failing_fails_the_read_in_bounded_time() {
        use crate::core::test_support::UpstreamBehaviour;
        let (_dir, _dl, r, upstream) = reader();
        let mut waits = quick_waits(100, 1_000);
        waits.retry = std::time::Duration::from_millis(200);
        r.set_waits(waits);
        let media: Vec<u8> = (0..100_000u32).map(|i| (i % 241) as u8).collect();
        upstream.set_media("t0", media, "audio/flac");
        upstream.set_behaviour("t0", UpstreamBehaviour::StallAfter(20_000));
        let url = r.mint(target()).unwrap();
        let h = r.open(&url, 0, None).await.unwrap().handle;
        let mut got = 0;
        while got < 20_000 {
            got += r.read(h, MAX_READ).await.unwrap().len();
        }
        // From here every answer fails before its first byte.
        upstream.set_behaviour("t0", UpstreamBehaviour::FailAfter(0));
        let started = std::time::Instant::now();
        let err = r.read(h, MAX_READ).await.unwrap_err();
        assert!(matches!(err, StreamError::Network(_)), "{err:?}");
        let took = started.elapsed();
        assert!(
            took >= std::time::Duration::from_millis(900)
                && took < std::time::Duration::from_secs(4),
            "{took:?}"
        );
        let calls = upstream.calls().len();
        assert!((3..=10).contains(&calls), "paced, not a hot loop: {calls}");
        r.close(h);
    }

    /// An open given up on (its future dropped, as the native downloader
    /// does on a reconnect) frees its slot.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_dropped_open_frees_its_slot() {
        let (_dir, _dl, r, upstream) = reader();
        upstream.hold_responses("t0");
        let url = r.mint(target()).unwrap();
        for _ in 0..MAX_HANDLES + 2 {
            let open = r.open(&url, 5, None);
            assert!(
                tokio::time::timeout(std::time::Duration::from_millis(20), open)
                    .await
                    .is_err()
            );
        }
        upstream.release_responses("t0");
        r.open(&url, 5, None).await.unwrap();
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
        assert_eq!(content_range_total("bytes */10"), Some(10));
        assert_eq!(content_range_total("bytes 0-9/10"), Some(10));
        assert_eq!(content_range_total("bytes 0-9/*"), None);
        let a = new_token();
        assert_eq!(a.len(), 32);
        assert_ne!(a, new_token());
    }
}

/// googlevideo through the reader: bounded ranges with the client's
/// User-Agent, and a URL refused partway is resolved again and the read
/// carries on at the same offset with the same bytes.
#[cfg(test)]
mod googlevideo_tests {
    use super::*;
    use crate::ytm::http::{FakeYtmHttp, HttpResponse};
    use crate::ytm::provider::{FIRST_RANGE, MAX_RANGE};

    const LEN: usize = 9 * 1024 * 1024 + 4321;

    /// A fetch: its URL, range and headers.
    type Fetched = (String, Option<String>, Vec<(String, String)>);

    struct Googlevideo {
        bytes: Arc<Vec<u8>>,
        log: Mutex<Vec<Fetched>>,
    }

    impl StreamUpstream for Googlevideo {
        fn fetch(
            &self,
            request: UpstreamRequest,
        ) -> BoxFuture<'static, Result<UpstreamResponse, String>> {
            self.log.lock().push((
                request.url.to_string(),
                request.range.clone(),
                request.headers.clone(),
            ));
            let bytes = self.bytes.clone();
            Box::pin(async move {
                let (a, b) = request
                    .range
                    .as_deref()
                    .and_then(|r| r.strip_prefix("bytes="))
                    .and_then(|r| r.split_once('-'))
                    .map(|(a, b)| (a.parse::<usize>().unwrap(), b.parse::<usize>().ok()))
                    .unwrap_or((0, None));
                // The first URL dies after its first range.
                if request.url.as_str().ends_with("n=1") && a > 0 {
                    return Ok(UpstreamResponse {
                        status: 403,
                        content_type: Some("text/plain".into()),
                        content_length: Some(0),
                        content_range: None,
                        body: futures::stream::empty().boxed(),
                    });
                }
                let b = b.unwrap_or(LEN - 1).min(LEN - 1);
                let chunk = Bytes::copy_from_slice(&bytes[a..=b]);
                Ok(UpstreamResponse {
                    status: 206,
                    content_type: Some("audio/mp4".into()),
                    content_length: Some(chunk.len() as u64),
                    content_range: Some(format!("bytes {a}-{b}/{LEN}")),
                    body: futures::stream::iter(
                        chunk
                            .chunks(256 * 1024)
                            .map(|c| Ok(Bytes::copy_from_slice(c)))
                            .collect::<Vec<_>>(),
                    )
                    .boxed(),
                })
            })
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_refused_url_is_resolved_again_mid_stream_and_ranges_stay_bounded() {
        let resolves = Arc::new(Mutex::new(0u32));
        let http = {
            let resolves = resolves.clone();
            FakeYtmHttp::new(move |r| {
                if r.url.contains("sw.js_data") {
                    return Some(HttpResponse::json(
                        200,
                        include_str!("../ytm/fixtures/sw_js_data.txt"),
                    ));
                }
                let mut n = resolves.lock();
                *n += 1;
                let v = serde_json::json!({"playabilityStatus": {"status": "OK"}, "streamingData": {"expiresInSeconds": "21540", "adaptiveFormats": [
                    {"itag": 140, "mimeType": "audio/mp4; codecs=\"mp4a.40.2\"", "contentLength": LEN.to_string(),
                     "url": format!("https://rr1.googlevideo.com/videoplayback?itag=140&n={}", *n), "loudnessDb": 2.5}]}});
                Some(HttpResponse::json(200, &v.to_string()))
            })
        };
        let clock: Arc<dyn Clock> = Arc::new(crate::util::WallClock);
        let it = Arc::new(crate::ytm::client::InnerTube::new(
            Arc::new(http),
            clock.clone(),
            None,
        ));
        let provider = Arc::new(crate::ytm::YtmProvider::new(
            crate::ytm::ANON,
            it,
            clock.clone(),
        ));
        let dir = tempfile::tempdir().unwrap();
        let dl = Downloads::new(
            crate::db::Db::open_in_memory().unwrap(),
            clock.clone(),
            Arc::new(crate::downloads::UnknownStorage),
            &dir.path().join("data"),
            &dir.path().join("cache"),
            crate::api::Platform::Linux,
        );
        let bytes: Arc<Vec<u8>> = Arc::new((0..LEN).map(|i| (i * 13 % 251) as u8).collect());
        let upstream = Arc::new(Googlevideo {
            bytes: bytes.clone(),
            log: Mutex::default(),
        });
        let r = StreamReader::new(dl.clone(), clock, upstream.clone(), Arc::new(|_| {}));
        r.set_providers(crate::provider::ProviderSet::single(provider));
        let url = r
            .mint(StreamTarget {
                server_id: "ytm".into(),
                track_id: "vid".into(),
                profile: None,
                suffix: Some("m4a".into()),
                mime_type: Some("audio/mp4".into()),
                duration_ms: Some(370_000.0),
            })
            .unwrap();
        let info = r.open(&url, 0, None).await.unwrap();
        let mut got = vec![];
        loop {
            let b = r.read(info.handle, 64 * 1024).await.unwrap();
            if b.is_empty() {
                break;
            }
            got.extend_from_slice(&b);
        }
        r.close(info.handle);
        assert_eq!(got.len(), LEN);
        assert!(got == *bytes, "the same bytes across the re-resolve");
        assert_eq!(*resolves.lock(), 2, "resolved once more after the refusal");
        let log = upstream.log.lock();
        let ranges: Vec<(u64, u64)> = log
            .iter()
            .filter_map(|(_, r, _)| {
                let (a, b) = r.as_deref()?.strip_prefix("bytes=")?.split_once('-')?;
                Some((a.parse().ok()?, b.parse().ok()?))
            })
            .collect();
        assert_eq!(ranges.len(), log.len(), "never an open-ended request");
        assert_eq!(ranges[0], (0, FIRST_RANGE - 1), "a small first range");
        assert!(ranges.iter().all(|(a, b)| b - a < MAX_RANGE));
        let retry = log.iter().find(|(u, _, _)| u.ends_with("n=2")).unwrap();
        assert_eq!(
            retry
                .1
                .as_deref()
                .map(|r| r.starts_with(&format!("bytes={FIRST_RANGE}-"))),
            Some(true),
            "resumed where it was refused"
        );
        // Each URL is fetched as the client that resolved it: visionOS first,
        // then (visionOS's URL refused) the next client in the chain.
        assert!(log.iter().all(|(u, _, h)| h
            .iter()
            .any(|(k, v)| k == "User-Agent" && (v.contains("Safari") == u.ends_with("n=1")))));
        // loudnessDb became the track gain.
        assert_eq!(dl.gain("ytm", "vid").unwrap(), Some(-2.5));
    }

    /// The pump (fetching ahead of the player) meets the refused URL: it is
    /// resolved again there too, and the rest of the stream lands in the
    /// cache, instead of the same refused URL being retried for ever.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn the_pump_resolves_a_refused_url_again() {
        let resolves = Arc::new(Mutex::new(0u32));
        let http = {
            let resolves = resolves.clone();
            FakeYtmHttp::new(move |r| {
                if r.url.contains("sw.js_data") {
                    return Some(HttpResponse::json(
                        200,
                        include_str!("../ytm/fixtures/sw_js_data.txt"),
                    ));
                }
                let mut n = resolves.lock();
                *n += 1;
                let v = serde_json::json!({"playabilityStatus": {"status": "OK"}, "streamingData": {"expiresInSeconds": "21540", "adaptiveFormats": [
                    {"itag": 140, "mimeType": "audio/mp4; codecs=\"mp4a.40.2\"", "contentLength": LEN.to_string(),
                     "url": format!("https://rr1.googlevideo.com/videoplayback?itag=140&n={}", *n), "loudnessDb": 2.5}]}});
                Some(HttpResponse::json(200, &v.to_string()))
            })
        };
        let fixed = Arc::new(crate::util::FixedClock::new(crate::util::now_ms()));
        let clock: Arc<dyn Clock> = fixed.clone();
        let it = Arc::new(crate::ytm::client::InnerTube::new(
            Arc::new(http),
            clock.clone(),
            None,
        ));
        let provider = Arc::new(crate::ytm::YtmProvider::new(
            crate::ytm::ANON,
            it,
            clock.clone(),
        ));
        let dir = tempfile::tempdir().unwrap();
        let dl = Downloads::new(
            crate::db::Db::open_in_memory().unwrap(),
            clock.clone(),
            Arc::new(crate::downloads::UnknownStorage),
            &dir.path().join("data"),
            &dir.path().join("cache"),
            crate::api::Platform::Linux,
        );
        let bytes: Arc<Vec<u8>> = Arc::new((0..LEN).map(|i| (i * 13 % 251) as u8).collect());
        let upstream = Arc::new(Googlevideo {
            bytes,
            log: Mutex::default(),
        });
        let r = StreamReader::new(dl.clone(), clock, upstream.clone(), Arc::new(|_| {}));
        r.set_providers(crate::provider::ProviderSet::single(provider));
        let url = r
            .mint(StreamTarget {
                server_id: "ytm".into(),
                track_id: "vid".into(),
                profile: None,
                suffix: Some("m4a".into()),
                mime_type: Some("audio/mp4".into()),
                duration_ms: Some(370_000.0),
            })
            .unwrap();
        let info = r.open(&url, 0, None).await.unwrap();
        assert!(!r.read(info.handle, 64 * 1024).await.unwrap().is_empty());
        // Past the pump's start delay, a read starts it.
        fixed.advance(PUMP_START_DELAY_MS + 1_000.0);
        assert!(!r.read(info.handle, 64 * 1024).await.unwrap().is_empty());
        let t = std::time::Instant::now();
        while !r.is_local(&url) {
            assert!(
                t.elapsed() < std::time::Duration::from_secs(20),
                "the pump never got past the refused URL ({} resolves)",
                *resolves.lock()
            );
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        assert_eq!(*resolves.lock(), 2);
        r.close(info.handle);
    }
}
