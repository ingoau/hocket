//! The HTTP seam. Everything the client sends goes through [`HttpTransport`]
//! so tests and the simulation harness can substitute an in-memory fake.

use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use bytes::Bytes;
use futures::future::BoxFuture;
use parking_lot::Mutex;
use url::Url;

/// A response as the client sees it. Bodies are fully buffered for API calls;
/// media downloads use [`HttpTransport::download`].
#[derive(Debug, Clone)]
pub struct HttpResponse {
    pub status: u16,
    pub content_type: Option<String>,
    pub body: Bytes,
}

#[derive(Debug, thiserror::Error)]
pub enum TransportError {
    #[error("network error: {0}")]
    Network(String),
    #[error("timed out")]
    Timeout,
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    /// The response body exceeded [`MAX_BODY_BYTES`].
    #[error("response body larger than {MAX_BODY_BYTES} bytes")]
    TooLarge,
}

/// Cap on a buffered API response body (`execute`). `search3` pages are a
/// few MiB at most; anything bigger is a hostile or broken server.
pub const MAX_BODY_BYTES: u64 = 32 * 1024 * 1024;

/// Refuse a download destination that could escape the directory the caller
/// meant: no `..` components, a real file name, and (when the transport has a
/// root) inside that root. Server-supplied strings (track ids, suffixes) are
/// sanitised upstream; this is the last line of defence.
pub fn check_download_dest(dest: &Path, root: Option<&Path>) -> std::io::Result<()> {
    let bad = |why: &str| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("refusing download destination: {why}"),
        )
    };
    if dest.components().any(|c| matches!(c, Component::ParentDir)) {
        return Err(bad("path contains `..`"));
    }
    if dest.file_name().is_none() {
        return Err(bad("no file name"));
    }
    if let Some(root) = root {
        if !dest.starts_with(root) {
            return Err(bad("outside the download root"));
        }
    }
    Ok(())
}

/// A download's part file, removed on drop unless taken (renamed into place).
struct PartFile(Option<PathBuf>);

impl Drop for PartFile {
    fn drop(&mut self) {
        if let Some(p) = self.0.take() {
            let _ = std::fs::remove_file(p);
        }
    }
}

/// A unique sibling temp name for `dest` (`<name>.<random>.part`), so two
/// writers for the same destination never share a temp file.
pub fn part_path(dest: &Path) -> PathBuf {
    let name = dest
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    dest.with_file_name(format!("{name}.{}.part", crate::util::new_id()))
}

/// A request the transport executes. `body` is sent as JSON with `POST`/`PUT`
/// when present.
#[derive(Debug, Clone)]
pub struct HttpRequest {
    pub method: Method,
    pub url: Url,
    pub headers: Vec<(String, String)>,
    pub body: Option<Bytes>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    Get,
    Post,
    Put,
    Delete,
}

impl HttpRequest {
    pub fn get(url: Url) -> Self {
        HttpRequest {
            method: Method::Get,
            url,
            headers: Vec::new(),
            body: None,
        }
    }
    pub fn with_header(mut self, name: &str, value: &str) -> Self {
        self.headers.push((name.to_string(), value.to_string()));
        self
    }
    pub fn with_json_body(mut self, method: Method, json: &str) -> Self {
        self.method = method;
        self.headers
            .push(("content-type".into(), "application/json".into()));
        self.body = Some(Bytes::from(json.to_string()));
        self
    }
}

/// Bytes-downloaded progress callback for [`HttpTransport::download`].
pub type ProgressFn = Arc<dyn Fn(u64, Option<u64>) + Send + Sync>;

pub trait HttpTransport: Send + Sync + 'static {
    /// Execute a request and buffer the whole response body.
    fn execute(&self, request: HttpRequest) -> BoxFuture<'_, Result<HttpResponse, TransportError>>;

    /// Stream a GET response body to `dest`, creating parent directories.
    /// Returns the response status, content type and bytes written. Non-2xx
    /// responses must not create the file.
    fn download(
        &self,
        url: Url,
        dest: &Path,
        progress: Option<ProgressFn>,
    ) -> BoxFuture<'_, Result<DownloadOutcome, TransportError>>;

    /// [`Self::download`] for a cover: retried in place at most once, as
    /// whoever shows it asks again on its own schedule.
    fn download_image(
        &self,
        url: Url,
        dest: &Path,
    ) -> BoxFuture<'_, Result<DownloadOutcome, TransportError>> {
        self.download(url, dest, None)
    }
}

#[derive(Debug, Clone)]
pub struct DownloadOutcome {
    pub status: u16,
    pub content_type: Option<String>,
    pub bytes: u64,
}

// ---------------------------------------------------------------------------
// reqwest implementation
// ---------------------------------------------------------------------------

/// The rustls config reqwest's `use_rustls_tls()` builds (webpki roots, its
/// default provider, TLS 1.2 and 1.3), made once so every client built on
/// it shares one session cache: a new connection to the server, from the
/// API, download or stream client, resumes a TLS session instead of a full
/// handshake. [http1_only] offers only HTTP/1.1 over ALPN, as a client
/// built `.http1_only()` must.
pub(crate) fn shared_tls(http1_only: bool) -> rustls::ClientConfig {
    static BASE: std::sync::OnceLock<rustls::ClientConfig> = std::sync::OnceLock::new();
    let base = BASE.get_or_init(|| {
        let mut roots = rustls::RootCertStore::empty();
        roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
        let provider = rustls::crypto::CryptoProvider::get_default()
            .cloned()
            .unwrap_or_else(|| Arc::new(rustls::crypto::ring::default_provider()));
        rustls::ClientConfig::builder_with_provider(provider)
            .with_protocol_versions(rustls::ALL_VERSIONS)
            .expect("rustls supports its own versions")
            .with_root_certificates(roots)
            .with_no_client_auth()
    });
    // A clone shares the session cache (`resumption` holds it in an Arc).
    let mut config = base.clone();
    config.alpn_protocols = if http1_only {
        vec![b"http/1.1".to_vec()]
    } else {
        vec![b"h2".to_vec(), b"http/1.1".to_vec()]
    };
    config
}

/// Production transport over reqwest + rustls. Never follows redirects: a
/// redirect would otherwise carry the token+salt query, the native-API JWT
/// header or a login body to another host or down to `http://`.
///
/// API calls run under a total timeout. Media downloads can't (a 10 MB file
/// on a slow link takes minutes), so they get their own client with only a
/// connect timeout and a read timeout (that long without a byte), retry
/// transient failures in place, and keep a failed attempt's partial file to
/// resume with a `Range` request next time.
pub struct ReqwestTransport {
    client: reqwest::Client,
    download_client: reqwest::Client,
    download_root: Option<PathBuf>,
    download_retries: u32,
    retry_backoff: std::time::Duration,
}

impl ReqwestTransport {
    /// `timeout` bounds a whole API call, and how long a download may go
    /// without receiving a byte.
    pub fn new(user_agent: &str, timeout: std::time::Duration) -> Result<Self, TransportError> {
        let base = || {
            reqwest::Client::builder()
                .user_agent(user_agent)
                .connect_timeout(std::time::Duration::from_secs(15))
                .redirect(reqwest::redirect::Policy::none())
                .use_preconfigured_tls(shared_tls(false))
        };
        let build_err = |e: reqwest::Error| TransportError::Network(e.without_url().to_string());
        // API answers (JSON) come compressed when the server can: a sync's
        // pages shrink several times over. Downloads stay byte-exact: their
        // lengths, ranges and resumes count the file's own bytes.
        let client = base().timeout(timeout).build().map_err(build_err)?;
        let download_client = base()
            .read_timeout(timeout)
            .no_gzip()
            .no_brotli()
            .build()
            .map_err(build_err)?;
        Ok(ReqwestTransport {
            client,
            download_client,
            download_root: None,
            download_retries: 3,
            retry_backoff: std::time::Duration::from_secs(1),
        })
    }

    /// Restrict [`HttpTransport::download`] to destinations under `root`
    /// (the app's data + cache directories' common parent, typically).
    pub fn with_download_root(mut self, root: impl Into<PathBuf>) -> Self {
        self.download_root = Some(root.into());
        self
    }

    /// In-place retries of a download after a network error or timeout, the
    /// first after `backoff`, doubling after that.
    pub fn with_download_retries(mut self, retries: u32, backoff: std::time::Duration) -> Self {
        self.download_retries = retries;
        self.retry_backoff = backoff;
        self
    }

    fn build(&self, request: &HttpRequest) -> reqwest::RequestBuilder {
        let mut b = match request.method {
            Method::Get => self.client.get(request.url.clone()),
            Method::Post => self.client.post(request.url.clone()),
            Method::Put => self.client.put(request.url.clone()),
            Method::Delete => self.client.delete(request.url.clone()),
        };
        for (k, v) in &request.headers {
            b = b.header(k, v);
        }
        if let Some(body) = &request.body {
            b = b.body(body.clone());
        }
        b
    }

    /// One GET of `url` into a temp file, resuming `partial` when the server
    /// answers its range request with the matching `206`. On a failure
    /// mid-body, the partial file comes back with the error (when it can be
    /// resumed); otherwise every temp file is removed.
    async fn download_once(
        &self,
        url: &Url,
        dest: &Path,
        partial: Option<Partial>,
        progress: Option<&ProgressFn>,
    ) -> Result<DownloadOutcome, (TransportError, Option<Partial>)> {
        use futures::StreamExt;
        use reqwest::header;
        use tokio::io::AsyncWriteExt;
        let mut req = self.download_client.get(url.clone());
        if let Some(p) = &partial {
            req = req.header(header::RANGE, format!("bytes={}-", p.len));
            if let Some(v) = &p.validator {
                req = req.header(header::IF_RANGE, v);
            }
        }
        let resp = match req.send().await {
            Ok(r) => r,
            Err(e) => return Err((map_reqwest(e), partial)),
        };
        let status = resp.status().as_u16();
        let headers = resp.headers();
        let text = |name: header::HeaderName| {
            headers
                .get(name)
                .and_then(|v| v.to_str().ok())
                .map(String::from)
        };
        let content_type = text(header::CONTENT_TYPE);
        let validator = text(header::ETAG).or_else(|| text(header::LAST_MODIFIED));
        let content_range = text(header::CONTENT_RANGE);
        let resumed = match (&partial, status) {
            (Some(p), 206) => {
                if content_range.as_deref().and_then(parse_content_range) != Some((p.len, p.total))
                {
                    // Not the bytes we asked for: start over.
                    discard(partial).await;
                    return Err((
                        TransportError::Network("unexpected partial response".into()),
                        None,
                    ));
                }
                partial
            }
            (Some(_), 416) => {
                discard(partial).await;
                return Err((
                    TransportError::Network("range not satisfiable".into()),
                    None,
                ));
            }
            (None, 206) => {
                return Err((
                    TransportError::Network("unexpected partial response".into()),
                    None,
                ));
            }
            (_, s) if !(200..300).contains(&s) => {
                // The caller judges the status; a partial waits for a later
                // attempt (an expired token, a server restarting).
                if let Some(p) = partial {
                    stash_partial(dest, p).await;
                }
                return Ok(DownloadOutcome {
                    status,
                    content_type,
                    bytes: 0,
                });
            }
            _ => {
                // A full body (the server ignored or refused the range).
                discard(partial).await;
                None
            }
        };
        let (tmp, base, total, validator) = match resumed {
            Some(p) => (p.tmp, p.len, Some(p.total), p.validator),
            None => (part_path(dest), 0, resp.content_length(), validator),
        };
        // Removes the part file if this future is dropped mid-transfer (a
        // cover nobody waits for any more is cancelled).
        let mut part = PartFile(Some(tmp.clone()));
        let file = if base > 0 {
            tokio::fs::OpenOptions::new().append(true).open(&tmp).await
        } else {
            tokio::fs::File::create(&tmp).await
        };
        let mut file = match file {
            Ok(f) => f,
            Err(e) => {
                let _ = tokio::fs::remove_file(&tmp).await;
                return Err((e.into(), None));
            }
        };
        let mut written = base;
        let result: Result<(), TransportError> = async {
            let mut stream = resp.bytes_stream();
            while let Some(chunk) = stream.next().await {
                let chunk = chunk.map_err(map_reqwest)?;
                file.write_all(&chunk).await?;
                written += chunk.len() as u64;
                if let Some(p) = progress {
                    p(written, total);
                }
            }
            file.flush().await?;
            Ok(())
        }
        .await;
        drop(file);
        if let Err(e) = result {
            let resumable = matches!(e, TransportError::Network(_) | TransportError::Timeout)
                && written > 0
                && total.is_some_and(|t| written < t);
            if resumable {
                let p = Partial {
                    tmp,
                    len: written,
                    total: total.unwrap_or(0),
                    validator,
                };
                part.0 = None;
                return Err((e, Some(p)));
            }
            let _ = tokio::fs::remove_file(&tmp).await;
            return Err((e, None));
        }
        if let Err(e) = tokio::fs::rename(&tmp, dest).await {
            let _ = tokio::fs::remove_file(&tmp).await;
            return Err((e.into(), None));
        }
        part.0 = None;
        Ok(DownloadOutcome {
            status: if base > 0 { 200 } else { status },
            content_type,
            bytes: written,
        })
    }
}

/// The bytes of an interrupted download, owned by one caller: a temp file
/// holding the first `len` of `total` bytes, and the response's `ETag` (or
/// `Last-Modified`) to send as `If-Range` so a changed file restarts.
struct Partial {
    tmp: PathBuf,
    len: u64,
    total: u64,
    validator: Option<String>,
}

async fn discard(partial: Option<Partial>) {
    if let Some(p) = partial {
        let _ = tokio::fs::remove_file(&p.tmp).await;
    }
}

/// Where an interrupted download of `dest` waits to be resumed (its total
/// size and validator sit beside it in `<that>.meta`).
pub fn resume_path(dest: &Path) -> PathBuf {
    let name = dest
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    dest.with_file_name(format!("{name}.resume"))
}

fn resume_meta_path(dest: &Path) -> PathBuf {
    let mut p = resume_path(dest).into_os_string();
    p.push(".meta");
    PathBuf::from(p)
}

/// Park a partial download next to `dest` for a later call to resume.
async fn stash_partial(dest: &Path, p: Partial) {
    let meta = format!("{}\n{}", p.total, p.validator.unwrap_or_default());
    let parked = async {
        tokio::fs::write(resume_meta_path(dest), meta).await?;
        tokio::fs::rename(&p.tmp, resume_path(dest)).await
    }
    .await;
    if parked.is_err() {
        let _ = tokio::fs::remove_file(&p.tmp).await;
        let _ = tokio::fs::remove_file(resume_meta_path(dest)).await;
    }
}

/// Take ownership of a parked partial download of `dest`, if any: renamed
/// to a unique temp name first, so two callers never both append to it.
async fn claim_partial(dest: &Path) -> Option<Partial> {
    let tmp = part_path(dest);
    tokio::fs::rename(resume_path(dest), &tmp).await.ok()?;
    let meta = tokio::fs::read_to_string(resume_meta_path(dest))
        .await
        .unwrap_or_default();
    let _ = tokio::fs::remove_file(resume_meta_path(dest)).await;
    let len = tokio::fs::metadata(&tmp)
        .await
        .map(|m| m.len())
        .unwrap_or(0);
    let (total, validator) = meta.split_once('\n').unwrap_or(("", ""));
    match total.parse::<u64>() {
        Ok(total) if len > 0 && len < total => Some(Partial {
            tmp,
            len,
            total,
            validator: (!validator.is_empty()).then(|| validator.to_string()),
        }),
        _ => {
            let _ = tokio::fs::remove_file(&tmp).await;
            None
        }
    }
}

/// `bytes <start>-<end>/<total>` → `(start, total)`.
fn parse_content_range(v: &str) -> Option<(u64, u64)> {
    let rest = v.trim().strip_prefix("bytes ")?;
    let (range, total) = rest.split_once('/')?;
    let (start, end) = range.split_once('-')?;
    let (start, end, total) = (
        start.trim().parse::<u64>().ok()?,
        end.trim().parse::<u64>().ok()?,
        total.trim().parse::<u64>().ok()?,
    );
    // Only a range that runs to the end resumes the file: a server or proxy
    // that caps the range would otherwise leave it short but "complete".
    (start <= end && end + 1 == total).then_some((start, total))
}

/// reqwest errors print their URL (with the auth query) in `Display`;
/// strip it before the message can reach a log or the UI.
fn map_reqwest(e: reqwest::Error) -> TransportError {
    if e.is_timeout() {
        TransportError::Timeout
    } else {
        TransportError::Network(e.without_url().to_string())
    }
}

/// Buffer a response body up to [`MAX_BODY_BYTES`].
async fn read_capped(resp: reqwest::Response) -> Result<Bytes, TransportError> {
    use futures::StreamExt;
    if resp.content_length().is_some_and(|n| n > MAX_BODY_BYTES) {
        return Err(TransportError::TooLarge);
    }
    let mut buf = bytes::BytesMut::new();
    let mut stream = resp.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(map_reqwest)?;
        if (buf.len() + chunk.len()) as u64 > MAX_BODY_BYTES {
            return Err(TransportError::TooLarge);
        }
        buf.extend_from_slice(&chunk);
    }
    Ok(buf.freeze())
}

impl HttpTransport for ReqwestTransport {
    fn execute(&self, request: HttpRequest) -> BoxFuture<'_, Result<HttpResponse, TransportError>> {
        Box::pin(async move {
            let resp = self.build(&request).send().await.map_err(map_reqwest)?;
            let status = resp.status().as_u16();
            let content_type = resp
                .headers()
                .get(reqwest::header::CONTENT_TYPE)
                .and_then(|v| v.to_str().ok())
                .map(String::from);
            let body = read_capped(resp).await?;
            Ok(HttpResponse {
                status,
                content_type,
                body,
            })
        })
    }

    fn download(
        &self,
        url: Url,
        dest: &Path,
        progress: Option<ProgressFn>,
    ) -> BoxFuture<'_, Result<DownloadOutcome, TransportError>> {
        self.download_retrying(url, dest, progress, self.download_retries)
    }

    fn download_image(
        &self,
        url: Url,
        dest: &Path,
    ) -> BoxFuture<'_, Result<DownloadOutcome, TransportError>> {
        self.download_retrying(url, dest, None, self.download_retries.min(1))
    }
}

impl ReqwestTransport {
    /// A download retried in place up to `retries` times after a network
    /// error or timeout.
    fn download_retrying(
        &self,
        url: Url,
        dest: &Path,
        progress: Option<ProgressFn>,
        retries: u32,
    ) -> BoxFuture<'_, Result<DownloadOutcome, TransportError>> {
        let dest = dest.to_path_buf();
        Box::pin(async move {
            check_download_dest(&dest, self.download_root.as_deref())?;
            if let Some(parent) = dest.parent() {
                tokio::fs::create_dir_all(parent).await?;
            }
            let mut partial = claim_partial(&dest).await;
            let mut attempt = 0;
            loop {
                match self
                    .download_once(&url, &dest, partial.take(), progress.as_ref())
                    .await
                {
                    Ok(out) => return Ok(out),
                    Err((e, p)) => {
                        let transient =
                            matches!(e, TransportError::Network(_) | TransportError::Timeout);
                        if transient && attempt < retries {
                            tokio::time::sleep(self.retry_backoff * 2u32.pow(attempt)).await;
                            attempt += 1;
                            partial = p;
                            continue;
                        }
                        if let Some(p) = p {
                            stash_partial(&dest, p).await;
                        }
                        return Err(e);
                    }
                }
            }
        })
    }
}

// ---------------------------------------------------------------------------
// In-memory fake (tests, simulation)
// ---------------------------------------------------------------------------

/// A canned response or failure the fake returns for a route.
#[derive(Debug, Clone)]
pub enum FakeReply {
    Json(String),
    Bytes { content_type: String, body: Bytes },
    Status(u16),
    Network,
    Timeout,
}

type Matcher = Arc<dyn Fn(&HttpRequest) -> bool + Send + Sync>;

struct Route {
    matcher: Matcher,
    replies: Vec<FakeReply>,
    /// Number of times the route has fired; replies are consumed in order and
    /// the last one repeats.
    hits: usize,
}

/// A scripted transport. Routes are matched most-recently-added first, on a
/// predicate over the full request (path, query, method, body). Every request
/// is recorded so tests can assert on auth parameters.
#[derive(Clone, Default)]
pub struct FakeTransport {
    inner: Arc<Mutex<FakeInner>>,
}

#[derive(Default)]
struct FakeInner {
    routes: Vec<Route>,
    requests: Vec<HttpRequest>,
}

impl FakeTransport {
    pub fn new() -> Self {
        Self::default()
    }

    /// Route on the last path segment (the Subsonic endpoint name, e.g. `ping`)
    /// regardless of `.view` suffix.
    pub fn on_endpoint(&self, endpoint: &str, reply: FakeReply) -> &Self {
        let ep = endpoint.to_string();
        self.on(move |r| endpoint_of(&r.url) == ep, vec![reply])
    }

    /// Route on the endpoint with a sequence of replies (consumed in order, last repeats).
    pub fn on_endpoint_seq(&self, endpoint: &str, replies: Vec<FakeReply>) -> &Self {
        let ep = endpoint.to_string();
        self.on(move |r| endpoint_of(&r.url) == ep, replies)
    }

    /// Route on a path prefix (native API) and optional method.
    pub fn on_path(&self, path_prefix: &str, method: Option<Method>, reply: FakeReply) -> &Self {
        let p = path_prefix.to_string();
        self.on(
            move |r| r.url.path().starts_with(&p) && method.is_none_or(|m| m == r.method),
            vec![reply],
        )
    }

    pub fn on(
        &self,
        matcher: impl Fn(&HttpRequest) -> bool + Send + Sync + 'static,
        replies: Vec<FakeReply>,
    ) -> &Self {
        self.inner.lock().routes.push(Route {
            matcher: Arc::new(matcher),
            replies,
            hits: 0,
        });
        self
    }

    /// Every request received so far.
    pub fn requests(&self) -> Vec<HttpRequest> {
        self.inner.lock().requests.clone()
    }

    /// Requests whose endpoint name matches.
    pub fn requests_to(&self, endpoint: &str) -> Vec<HttpRequest> {
        self.requests()
            .into_iter()
            .filter(|r| endpoint_of(&r.url) == endpoint)
            .collect()
    }

    pub fn clear_requests(&self) {
        self.inner.lock().requests.clear();
    }

    fn reply_for(&self, request: &HttpRequest) -> Option<FakeReply> {
        let mut inner = self.inner.lock();
        inner.requests.push(request.clone());
        let route = inner
            .routes
            .iter_mut()
            .rev()
            .find(|r| (r.matcher)(request))?;
        let idx = route.hits.min(route.replies.len().saturating_sub(1));
        route.hits += 1;
        route.replies.get(idx).cloned()
    }
}

/// The Subsonic endpoint name of a URL: last path segment without `.view`.
pub fn endpoint_of(url: &Url) -> String {
    let last = url
        .path_segments()
        .and_then(|mut s| s.next_back())
        .unwrap_or("");
    last.trim_end_matches(".view").to_string()
}

/// Read a query parameter from a request URL.
pub fn query_param(request: &HttpRequest, key: &str) -> Option<String> {
    request
        .url
        .query_pairs()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.into_owned())
}

/// All values of a repeated query parameter.
pub fn query_params(request: &HttpRequest, key: &str) -> Vec<String> {
    request
        .url
        .query_pairs()
        .filter(|(k, _)| k == key)
        .map(|(_, v)| v.into_owned())
        .collect()
}

impl HttpTransport for FakeTransport {
    fn execute(&self, request: HttpRequest) -> BoxFuture<'_, Result<HttpResponse, TransportError>> {
        let reply = self.reply_for(&request);
        Box::pin(async move {
            match reply {
                None => Ok(HttpResponse {
                    status: 404,
                    content_type: None,
                    body: Bytes::new(),
                }),
                Some(FakeReply::Json(j)) => Ok(HttpResponse {
                    status: 200,
                    content_type: Some("application/json".into()),
                    body: Bytes::from(j),
                }),
                Some(FakeReply::Bytes { content_type, body }) => Ok(HttpResponse {
                    status: 200,
                    content_type: Some(content_type),
                    body,
                }),
                Some(FakeReply::Status(s)) => Ok(HttpResponse {
                    status: s,
                    content_type: None,
                    body: Bytes::new(),
                }),
                Some(FakeReply::Network) => {
                    Err(TransportError::Network("fake network failure".into()))
                }
                Some(FakeReply::Timeout) => Err(TransportError::Timeout),
            }
        })
    }

    fn download(
        &self,
        url: Url,
        dest: &Path,
        progress: Option<ProgressFn>,
    ) -> BoxFuture<'_, Result<DownloadOutcome, TransportError>> {
        let dest = dest.to_path_buf();
        let request = HttpRequest::get(url);
        let reply = self.reply_for(&request);
        Box::pin(async move {
            let (status, content_type, body) = match reply {
                None => (404, None, Bytes::new()),
                Some(FakeReply::Json(j)) => {
                    (200, Some("application/json".to_string()), Bytes::from(j))
                }
                Some(FakeReply::Bytes { content_type, body }) => (200, Some(content_type), body),
                Some(FakeReply::Status(s)) => (s, None, Bytes::new()),
                Some(FakeReply::Network) => {
                    return Err(TransportError::Network("fake network failure".into()))
                }
                Some(FakeReply::Timeout) => return Err(TransportError::Timeout),
            };
            if !(200..300).contains(&status) {
                return Ok(DownloadOutcome {
                    status,
                    content_type,
                    bytes: 0,
                });
            }
            check_download_dest(&dest, None)?;
            if let Some(parent) = dest.parent() {
                tokio::fs::create_dir_all(parent).await?;
            }
            tokio::fs::write(&dest, &body).await?;
            if let Some(p) = progress {
                p(body.len() as u64, Some(body.len() as u64));
            }
            Ok(DownloadOutcome {
                status,
                content_type,
                bytes: body.len() as u64,
            })
        })
    }
}

/// Convenience: a map of header name → value.
pub fn headers_map(request: &HttpRequest) -> HashMap<String, String> {
    request
        .headers
        .iter()
        .map(|(k, v)| (k.to_ascii_lowercase(), v.clone()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::parse_content_range;

    #[test]
    fn content_range_must_run_to_the_end() {
        assert_eq!(parse_content_range("bytes 100-999/1000"), Some((100, 1000)));
        // A capped range would leave the file short.
        assert_eq!(parse_content_range("bytes 100-499/1000"), None);
        assert_eq!(parse_content_range("bytes 100-1000/1000"), None);
        assert_eq!(parse_content_range("bytes */1000"), None);
    }
}
