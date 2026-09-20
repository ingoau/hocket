//! The HTTP seam. Everything the client sends goes through [`HttpTransport`]
//! so tests and the simulation harness can substitute an in-memory fake.

use std::collections::HashMap;
use std::path::Path;
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

/// Production transport over reqwest + rustls.
pub struct ReqwestTransport {
    client: reqwest::Client,
}

impl ReqwestTransport {
    pub fn new(user_agent: &str, timeout: std::time::Duration) -> Result<Self, TransportError> {
        let client = reqwest::Client::builder()
            .user_agent(user_agent)
            .timeout(timeout)
            .connect_timeout(std::time::Duration::from_secs(15))
            .use_rustls_tls()
            .build()
            .map_err(|e| TransportError::Network(e.to_string()))?;
        Ok(ReqwestTransport { client })
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
}

fn map_reqwest(e: reqwest::Error) -> TransportError {
    if e.is_timeout() {
        TransportError::Timeout
    } else {
        TransportError::Network(e.to_string())
    }
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
            let body = resp.bytes().await.map_err(map_reqwest)?;
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
        let dest = dest.to_path_buf();
        Box::pin(async move {
            use futures::StreamExt;
            use tokio::io::AsyncWriteExt;
            let resp = self.client.get(url).send().await.map_err(map_reqwest)?;
            let status = resp.status().as_u16();
            let content_type = resp
                .headers()
                .get(reqwest::header::CONTENT_TYPE)
                .and_then(|v| v.to_str().ok())
                .map(String::from);
            if !(200..300).contains(&status) {
                return Ok(DownloadOutcome {
                    status,
                    content_type,
                    bytes: 0,
                });
            }
            let total = resp.content_length();
            if let Some(parent) = dest.parent() {
                tokio::fs::create_dir_all(parent).await?;
            }
            let tmp = dest.with_extension("part");
            let mut file = tokio::fs::File::create(&tmp).await?;
            let mut stream = resp.bytes_stream();
            let mut written: u64 = 0;
            while let Some(chunk) = stream.next().await {
                let chunk = chunk.map_err(map_reqwest)?;
                file.write_all(&chunk).await?;
                written += chunk.len() as u64;
                if let Some(p) = &progress {
                    p(written, total);
                }
            }
            file.flush().await?;
            drop(file);
            tokio::fs::rename(&tmp, &dest).await?;
            Ok(DownloadOutcome {
                status,
                content_type,
                bytes: written,
            })
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
