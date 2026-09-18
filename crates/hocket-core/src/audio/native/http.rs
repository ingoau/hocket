//! HTTP sources for the decoder: range requests streamed into a sparse
//! in-memory buffer that implements Symphonia's `MediaSource`, so seeking
//! works before the download finishes.
//!
//! The network is behind [`RangeFetcher`] so tests (and the simulation
//! harness) can serve bytes from memory with controlled pacing;
//! [`ReqwestFetcher`] is the real one. A downloader task on the tokio
//! runtime streams sequentially from wherever the reader wants to read; when
//! the reader seeks into an unbuffered region the current response is
//! abandoned and a new `Range` request starts there. Servers that ignore
//! `Range` (Navidrome's on-the-fly transcodes have no length and no ranges)
//! degrade to a sequential download the reader waits on.
//!
//! The whole file is held in memory for the life of the source; the stream
//! cache module is responsible for persisting anything across plays.

use std::collections::{BTreeMap, HashMap};
use std::io::{self, Read, Seek, SeekFrom};
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use futures::{Future, Stream, StreamExt};
use parking_lot::{Condvar, Mutex};
use symphonia::core::io::MediaSource;
use tokio::sync::Notify;

/// Boxed future returned by fetchers.
pub type FetchFuture = Pin<Box<dyn Future<Output = Result<FetchResponse, FetchError>> + Send>>;
/// Boxed body stream.
pub type BodyStream = Pin<Box<dyn Stream<Item = Result<Bytes, FetchError>> + Send>>;

#[derive(Debug, Clone, thiserror::Error, PartialEq, Eq)]
pub enum FetchError {
    #[error("http status {0}")]
    Status(u16),
    #[error("network: {0}")]
    Network(String),
    #[error("cancelled")]
    Cancelled,
}

/// One response to a `bytes=start-` request.
pub struct FetchResponse {
    /// Byte offset the body actually starts at: `start` for a 206, `0` when
    /// the server ignored the range.
    pub range_start: u64,
    /// Total resource length when known (`Content-Range` total or
    /// `Content-Length` for a full response).
    pub total_len: Option<u64>,
    /// Whether the server honours ranges (206 or `Accept-Ranges: bytes`).
    pub supports_ranges: bool,
    pub body: BodyStream,
}

/// The network seam for streamed audio.
pub trait RangeFetcher: Send + Sync + 'static {
    fn fetch(&self, url: String, headers: HashMap<String, String>, start: u64) -> FetchFuture;
}

/// Real fetcher over `reqwest`.
pub struct ReqwestFetcher {
    client: reqwest::Client,
}

impl ReqwestFetcher {
    pub fn new(client: reqwest::Client) -> Self {
        Self { client }
    }
}

impl Default for ReqwestFetcher {
    fn default() -> Self {
        Self::new(reqwest::Client::new())
    }
}

impl RangeFetcher for ReqwestFetcher {
    fn fetch(&self, url: String, headers: HashMap<String, String>, start: u64) -> FetchFuture {
        let client = self.client.clone();
        Box::pin(async move {
            let mut req = client.get(&url).header(reqwest::header::RANGE, format!("bytes={start}-"));
            for (k, v) in &headers {
                req = req.header(k.as_str(), v.as_str());
            }
            let resp = req.send().await.map_err(|e| FetchError::Network(e.to_string()))?;
            let status = resp.status();
            if !status.is_success() {
                return Err(FetchError::Status(status.as_u16()));
            }
            let h = resp.headers();
            let content_range = h.get(reqwest::header::CONTENT_RANGE).and_then(|v| v.to_str().ok()).map(str::to_string);
            let content_length = resp.content_length();
            let accept_ranges =
                h.get(reqwest::header::ACCEPT_RANGES).and_then(|v| v.to_str().ok()).map(|s| s.contains("bytes")).unwrap_or(false);
            let (range_start, total_len, supports_ranges) = if status.as_u16() == 206 {
                let (s, total) = content_range.as_deref().and_then(parse_content_range).unwrap_or((start, None));
                (s, total, true)
            } else {
                (0, content_length, accept_ranges)
            };
            let body = resp.bytes_stream().map(|r| r.map_err(|e| FetchError::Network(e.to_string())));
            Ok(FetchResponse { range_start, total_len, supports_ranges, body: Box::pin(body) })
        })
    }
}

/// Parse `bytes 100-199/1000` (or `bytes 100-199/*`) → (start, total).
pub fn parse_content_range(v: &str) -> Option<(u64, Option<u64>)> {
    let rest = v.trim().strip_prefix("bytes")?.trim();
    let (range, total) = rest.split_once('/')?;
    let (start, _end) = range.split_once('-')?;
    let start: u64 = start.trim().parse().ok()?;
    let total = total.trim().parse::<u64>().ok();
    Some((start, total))
}

/// An in-memory fetcher for tests: serves `data` in `chunk` byte pieces
/// with an optional delay per chunk, honouring ranges or not.
pub struct MemoryFetcher {
    pub data: Arc<Vec<u8>>,
    pub chunk: usize,
    pub delay: Duration,
    pub ranges: bool,
    pub fail_status: Option<u16>,
    pub requests: Arc<Mutex<Vec<u64>>>,
}

impl MemoryFetcher {
    pub fn new(data: Vec<u8>) -> Self {
        Self {
            data: Arc::new(data),
            chunk: 4096,
            delay: Duration::ZERO,
            ranges: true,
            fail_status: None,
            requests: Arc::new(Mutex::new(Vec::new())),
        }
    }
}

impl RangeFetcher for MemoryFetcher {
    fn fetch(&self, _url: String, _headers: HashMap<String, String>, start: u64) -> FetchFuture {
        let data = self.data.clone();
        let chunk = self.chunk.max(1);
        let delay = self.delay;
        let ranges = self.ranges;
        let fail = self.fail_status;
        self.requests.lock().push(start);
        Box::pin(async move {
            if let Some(code) = fail {
                return Err(FetchError::Status(code));
            }
            let from = if ranges { (start as usize).min(data.len()) } else { 0 };
            let total = data.len() as u64;
            let stream = futures::stream::unfold(from, move |pos| {
                let data = data.clone();
                async move {
                    if pos >= data.len() {
                        return None;
                    }
                    if !delay.is_zero() {
                        tokio::time::sleep(delay).await;
                    }
                    let end = (pos + chunk).min(data.len());
                    Some((Ok(Bytes::copy_from_slice(&data[pos..end])), end))
                }
            });
            Ok(FetchResponse {
                range_start: from as u64,
                total_len: Some(total),
                supports_ranges: ranges,
                body: Box::pin(stream),
            })
        })
    }
}

/// How far ahead of the reader the downloader keeps streaming before it is
/// willing to abandon a response for a seek elsewhere.
const READAHEAD_TOLERANCE: u64 = 4 * 1024 * 1024;

#[derive(Default)]
struct State {
    /// Non-overlapping, non-adjacent-merged segments keyed by start offset.
    segments: BTreeMap<u64, Vec<u8>>,
    total: Option<u64>,
    /// Where the reader wants data next.
    want: u64,
    supports_ranges: Option<bool>,
    /// First response headers seen (or failed).
    opened: bool,
    error: Option<FetchError>,
    closed: bool,
    /// Downloader finished with nothing left to fetch.
    complete: bool,
}

impl State {
    fn segment_at(&self, pos: u64) -> Option<(u64, &Vec<u8>)> {
        let (start, data) = self.segments.range(..=pos).next_back()?;
        if start + data.len() as u64 > pos {
            Some((*start, data))
        } else {
            None
        }
    }

    fn has(&self, pos: u64) -> bool {
        self.segment_at(pos).is_some()
    }

    /// First byte at or after `pos` that isn't buffered, or `None` when
    /// everything from `pos` to the end is present.
    fn first_gap_from(&self, pos: u64) -> Option<u64> {
        let mut p = pos;
        loop {
            match self.total {
                Some(t) if p >= t => return None,
                _ => {}
            }
            match self.segment_at(p) {
                Some((start, data)) => p = start + data.len() as u64,
                None => return Some(p),
            }
        }
    }

    fn append(&mut self, pos: u64, bytes: &[u8]) {
        if bytes.is_empty() {
            return;
        }
        // Extend an adjacent previous segment if there is one.
        if let Some((start, data)) = self.segments.range_mut(..=pos).next_back() {
            let end = *start + data.len() as u64;
            if end == pos {
                data.extend_from_slice(bytes);
                let new_end = end + bytes.len() as u64;
                let start = *start;
                self.merge_following(start, new_end);
                return;
            }
            if end > pos {
                // Overlap with existing data: keep what we have, add the rest.
                let skip = (end - pos) as usize;
                if skip >= bytes.len() {
                    return;
                }
                let start = *start;
                self.append(end, &bytes[skip..]);
                let _ = start;
                return;
            }
        }
        self.segments.insert(pos, bytes.to_vec());
        self.merge_following(pos, pos + bytes.len() as u64);
    }

    fn merge_following(&mut self, start: u64, end: u64) {
        // Absorb any segment that starts inside [start, end].
        while let Some((&next_start, _)) = self.segments.range((start + 1)..=end).next() {
            let next = self.segments.remove(&next_start).expect("exists");
            let cur = self.segments.get_mut(&start).expect("exists");
            let cur_end = start + cur.len() as u64;
            if next_start + next.len() as u64 > cur_end {
                let skip = (cur_end - next_start) as usize;
                cur.extend_from_slice(&next[skip..]);
            }
        }
    }
}

struct Shared {
    state: Mutex<State>,
    data_ready: Condvar,
    wake_downloader: Notify,
}

/// A seekable, blocking reader over a streamed HTTP resource.
pub struct HttpSource {
    shared: Arc<Shared>,
    pos: u64,
    _task: tokio::task::JoinHandle<()>,
}

impl HttpSource {
    /// Open the resource: starts the downloader on `runtime` and blocks the
    /// calling (decode) thread until the first response headers arrive, so
    /// `byte_len` is known before probing. Fails on a non-2xx first response.
    pub fn open(
        fetcher: Arc<dyn RangeFetcher>,
        runtime: tokio::runtime::Handle,
        url: String,
        headers: HashMap<String, String>,
        timeout: Duration,
    ) -> io::Result<HttpSource> {
        let shared = Arc::new(Shared { state: Mutex::new(State::default()), data_ready: Condvar::new(), wake_downloader: Notify::new() });
        let task = runtime.spawn(downloader(shared.clone(), fetcher, url, headers));
        let deadline = std::time::Instant::now() + timeout;
        {
            let mut st = shared.state.lock();
            while !st.opened {
                if shared.data_ready.wait_until(&mut st, deadline).timed_out() {
                    st.closed = true;
                    shared.wake_downloader.notify_one();
                    return Err(io::Error::new(io::ErrorKind::TimedOut, "no response from server"));
                }
            }
            if let Some(e) = &st.error {
                st.closed = true;
                shared.wake_downloader.notify_one();
                return Err(io::Error::other(e.to_string()));
            }
        }
        Ok(HttpSource { shared, pos: 0, _task: task })
    }

    pub fn total_len(&self) -> Option<u64> {
        self.shared.state.lock().total
    }

    /// Bytes buffered contiguously from `pos`.
    pub fn buffered_from(&self, pos: u64) -> u64 {
        let st = self.shared.state.lock();
        st.segment_at(pos).map(|(s, d)| s + d.len() as u64 - pos).unwrap_or(0)
    }

    pub fn is_complete(&self) -> bool {
        self.shared.state.lock().complete
    }
}

impl Drop for HttpSource {
    fn drop(&mut self) {
        self.shared.state.lock().closed = true;
        self.shared.wake_downloader.notify_one();
        self._task.abort();
    }
}

async fn downloader(shared: Arc<Shared>, fetcher: Arc<dyn RangeFetcher>, url: String, headers: HashMap<String, String>) {
    loop {
        // Decide where to fetch from.
        let start = {
            let st = shared.state.lock();
            if st.closed {
                return;
            }
            let ranges_ok = st.supports_ranges.unwrap_or(true);
            let want = if ranges_ok { st.want } else { 0 };
            match st.first_gap_from(want).or_else(|| st.first_gap_from(0)) {
                Some(gap) => {
                    if !ranges_ok && gap != 0 && !st.has(0) {
                        0
                    } else if !ranges_ok {
                        // Sequential only: continue from the end of the prefix.
                        st.first_gap_from(0).unwrap_or(0)
                    } else {
                        gap
                    }
                }
                None => {
                    // Everything present.
                    drop(st);
                    let mut st = shared.state.lock();
                    st.complete = true;
                    st.opened = true;
                    shared.data_ready.notify_all();
                    return;
                }
            }
        };
        let resp = fetcher.fetch(url.clone(), headers.clone(), start).await;
        let mut resp = match resp {
            Ok(r) => r,
            Err(e) => {
                let mut st = shared.state.lock();
                if !st.opened {
                    st.error = Some(e);
                    st.opened = true;
                    shared.data_ready.notify_all();
                    return;
                }
                // Mid-stream failure: retry a few times with backoff, then give up.
                st.error = Some(e.clone());
                shared.data_ready.notify_all();
                drop(st);
                tracing::warn!(target: "hocket::audio::http", error = %e, "fetch failed, retrying");
                tokio::time::sleep(Duration::from_millis(500)).await;
                let st = shared.state.lock();
                if st.closed {
                    return;
                }
                continue;
            }
        };
        let mut pos = resp.range_start;
        {
            let mut st = shared.state.lock();
            if let Some(t) = resp.total_len {
                st.total = Some(t);
            }
            st.supports_ranges = Some(resp.supports_ranges);
            st.opened = true;
            st.error = None;
            shared.data_ready.notify_all();
        }
        loop {
            let chunk = tokio::select! {
                c = resp.body.next() => c,
                _ = shared.wake_downloader.notified() => {
                    // The reader moved. Abandon this response if it wants
                    // something we don't have and won't have soon.
                    let st = shared.state.lock();
                    if st.closed {
                        return;
                    }
                    let want = st.want;
                    let ranges_ok = st.supports_ranges.unwrap_or(true);
                    let far = want < pos || want > pos + READAHEAD_TOLERANCE;
                    if ranges_ok && far && !st.has(want) {
                        break;
                    }
                    continue;
                }
            };
            match chunk {
                Some(Ok(bytes)) => {
                    let mut st = shared.state.lock();
                    if st.closed {
                        return;
                    }
                    st.append(pos, &bytes);
                    pos += bytes.len() as u64;
                    if st.total.is_none() && st.supports_ranges == Some(false) {
                        // Unknown length: grows until the body ends.
                    }
                    shared.data_ready.notify_all();
                }
                Some(Err(e)) => {
                    tracing::warn!(target: "hocket::audio::http", error = %e, "body stream error");
                    break;
                }
                None => {
                    let mut st = shared.state.lock();
                    if st.total.is_none() {
                        st.total = Some(pos);
                    }
                    shared.data_ready.notify_all();
                    break;
                }
            }
        }
    }
}

impl Read for HttpSource {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        let mut st = self.shared.state.lock();
        loop {
            if let Some((start, data)) = st.segment_at(self.pos) {
                let off = (self.pos - start) as usize;
                let n = buf.len().min(data.len() - off);
                buf[..n].copy_from_slice(&data[off..off + n]);
                self.pos += n as u64;
                return Ok(n);
            }
            if let Some(t) = st.total {
                if self.pos >= t {
                    return Ok(0);
                }
            }
            if st.closed {
                return Err(io::Error::new(io::ErrorKind::BrokenPipe, "source closed"));
            }
            if st.complete {
                return Ok(0);
            }
            if st.want != self.pos {
                st.want = self.pos;
                self.shared.wake_downloader.notify_one();
            }
            if let Some(e) = &st.error {
                // Transient errors are being retried; only surface after the
                // downloader gives up (it sets `closed`). Keep waiting.
                tracing::debug!(target: "hocket::audio::http", error = %e, "waiting through fetch error");
            }
            self.shared.data_ready.wait_for(&mut st, Duration::from_millis(200));
        }
    }
}

impl Seek for HttpSource {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        let new = match pos {
            SeekFrom::Start(p) => p,
            SeekFrom::Current(d) => self.pos.checked_add_signed(d).ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "seek before start"))?,
            SeekFrom::End(d) => {
                let total = self.total_len().ok_or_else(|| io::Error::new(io::ErrorKind::Unsupported, "length unknown"))?;
                total.checked_add_signed(d).ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "seek before start"))?
            }
        };
        self.pos = new;
        let mut st = self.shared.state.lock();
        if !st.has(new) && st.want != new {
            st.want = new;
            self.shared.wake_downloader.notify_one();
        }
        Ok(new)
    }
}

impl MediaSource for HttpSource {
    fn is_seekable(&self) -> bool {
        true
    }

    fn byte_len(&self) -> Option<u64> {
        self.total_len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn runtime() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_multi_thread().worker_threads(1).enable_all().build().unwrap()
    }

    fn data(n: usize) -> Vec<u8> {
        (0..n).map(|i| (i % 251) as u8).collect()
    }

    #[test]
    fn reads_sequentially_and_reports_length() {
        let rt = runtime();
        let bytes = data(100_000);
        let fetcher = Arc::new(MemoryFetcher::new(bytes.clone()));
        let mut src = HttpSource::open(fetcher, rt.handle().clone(), "http://x/a".into(), HashMap::new(), Duration::from_secs(5)).unwrap();
        assert_eq!(src.byte_len(), Some(100_000));
        assert!(src.is_seekable());
        let mut out = Vec::new();
        src.read_to_end(&mut out).unwrap();
        assert_eq!(out, bytes);
    }

    #[test]
    fn seeking_ahead_of_the_download_issues_a_range_request() {
        let rt = runtime();
        let bytes = data(2_000_000);
        let fetcher = MemoryFetcher { chunk: 1024, delay: Duration::from_millis(2), ..MemoryFetcher::new(bytes.clone()) };
        let requests = fetcher.requests.clone();
        let mut src = HttpSource::open(Arc::new(fetcher), rt.handle().clone(), "http://x/a".into(), HashMap::new(), Duration::from_secs(5)).unwrap();
        // The tail is far beyond what the slow download has reached.
        src.seek(SeekFrom::Start(1_900_000)).unwrap();
        let mut tail = vec![0u8; 4096];
        src.read_exact(&mut tail).unwrap();
        assert_eq!(tail, &bytes[1_900_000..1_904_096]);
        let reqs = requests.lock().clone();
        assert!(reqs.iter().any(|&s| s >= 1_500_000 && s <= 1_900_000), "{reqs:?}");
        // Going back to the start reads what was buffered before the seek.
        src.seek(SeekFrom::Start(0)).unwrap();
        let mut head = vec![0u8; 512];
        src.read_exact(&mut head).unwrap();
        assert_eq!(head, &bytes[..512]);
        assert_eq!(src.seek(SeekFrom::End(-10)).unwrap(), 1_999_990);
        let mut end = Vec::new();
        src.read_to_end(&mut end).unwrap();
        assert_eq!(end, &bytes[1_999_990..]);
    }

    #[test]
    fn servers_without_ranges_degrade_to_sequential_waiting() {
        let rt = runtime();
        let bytes = data(300_000);
        let fetcher = MemoryFetcher { chunk: 8192, delay: Duration::from_millis(1), ranges: false, ..MemoryFetcher::new(bytes.clone()) };
        let requests = fetcher.requests.clone();
        let mut src = HttpSource::open(Arc::new(fetcher), rt.handle().clone(), "http://x/a".into(), HashMap::new(), Duration::from_secs(5)).unwrap();
        src.seek(SeekFrom::Start(250_000)).unwrap();
        let mut buf = vec![0u8; 100];
        src.read_exact(&mut buf).unwrap();
        assert_eq!(buf, &bytes[250_000..250_100]);
        // Everything before was fetched on the way (no extra range requests).
        assert_eq!(src.buffered_from(0), 250_100.max(src.buffered_from(0)));
        assert!(requests.lock().len() <= 2, "{:?}", requests.lock());
    }

    #[test]
    fn http_errors_fail_open() {
        let rt = runtime();
        let fetcher = MemoryFetcher { fail_status: Some(403), ..MemoryFetcher::new(data(10)) };
        let err = HttpSource::open(Arc::new(fetcher), rt.handle().clone(), "http://x/a".into(), HashMap::new(), Duration::from_secs(5)).unwrap_err();
        assert!(err.to_string().contains("403"), "{err}");
    }

    #[test]
    fn content_range_parsing() {
        assert_eq!(parse_content_range("bytes 100-199/1000"), Some((100, Some(1000))));
        assert_eq!(parse_content_range("bytes 5-9/*"), Some((5, None)));
        assert_eq!(parse_content_range("items 1-2/3"), None);
        assert_eq!(parse_content_range("bytes x-2/3"), None);
    }

    #[test]
    fn segment_bookkeeping_merges_adjacent_and_overlapping_data() {
        let mut st = State::default();
        st.append(0, &[1, 2, 3]);
        st.append(3, &[4, 5]);
        assert_eq!(st.segments.len(), 1);
        assert_eq!(st.segments[&0], vec![1, 2, 3, 4, 5]);
        st.append(10, &[7, 8]);
        st.append(5, &[6, 6, 6, 6, 6, 9, 9]);
        assert_eq!(st.segments.len(), 1, "{:?}", st.segments);
        assert_eq!(st.segments[&0], vec![1, 2, 3, 4, 5, 6, 6, 6, 6, 6, 7, 8]);
        st.append(2, &[0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        assert_eq!(st.segments[&0].len(), 15);
        assert_eq!(st.first_gap_from(0), Some(15));
        st.total = Some(15);
        assert_eq!(st.first_gap_from(0), None);
        assert!(st.has(14) && !st.has(15));
    }
}
