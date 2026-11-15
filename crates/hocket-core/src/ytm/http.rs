//! The InnerTube client's HTTP seam: reqwest in production, recorded
//! fixtures in tests (no live network in CI).

use std::sync::Arc;

use bytes::Bytes;
use futures::future::BoxFuture;
use parking_lot::Mutex;

#[derive(Debug, Clone, PartialEq)]
pub enum HttpMethod {
    Get,
    Post,
}

#[derive(Clone)]
pub struct HttpRequest {
    pub method: HttpMethod,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Option<Vec<u8>>,
}

impl std::fmt::Debug for HttpRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Never the headers: they carry cookies.
        f.debug_struct("HttpRequest")
            .field("method", &self.method)
            .field("url", &crate::api::redact_url(&self.url))
            .finish()
    }
}

impl HttpRequest {
    /// The JSON body, for tests that route on it.
    pub fn json(&self) -> serde_json::Value {
        self.body
            .as_deref()
            .and_then(|b| serde_json::from_slice(b).ok())
            .unwrap_or(serde_json::Value::Null)
    }

    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }
}

#[derive(Debug, Clone)]
pub struct HttpResponse {
    pub status: u16,
    pub body: Bytes,
    pub content_range: Option<String>,
    pub content_type: Option<String>,
}

impl HttpResponse {
    pub fn json(status: u16, body: &str) -> HttpResponse {
        HttpResponse {
            status,
            body: Bytes::copy_from_slice(body.as_bytes()),
            content_range: None,
            content_type: Some("application/json".into()),
        }
    }
}

/// How the client reaches YouTube. Errors must not carry the URL.
pub trait YtmHttp: Send + Sync + 'static {
    fn send(&self, req: HttpRequest) -> BoxFuture<'static, Result<HttpResponse, String>>;
}

/// Production transport over reqwest + rustls.
pub struct ReqwestYtmHttp {
    client: reqwest::Client,
}

impl ReqwestYtmHttp {
    pub fn new() -> Result<ReqwestYtmHttp, String> {
        let client = reqwest::Client::builder()
            .connect_timeout(std::time::Duration::from_secs(15))
            .timeout(std::time::Duration::from_secs(60))
            .use_rustls_tls()
            .build()
            .map_err(|e| e.without_url().to_string())?;
        Ok(ReqwestYtmHttp { client })
    }
}

impl YtmHttp for ReqwestYtmHttp {
    fn send(&self, req: HttpRequest) -> BoxFuture<'static, Result<HttpResponse, String>> {
        let client = self.client.clone();
        Box::pin(async move {
            let mut b = match req.method {
                HttpMethod::Get => client.get(&req.url),
                HttpMethod::Post => client.post(&req.url),
            };
            for (k, v) in &req.headers {
                b = b.header(k, v);
            }
            if let Some(body) = req.body {
                b = b.body(body);
            }
            let resp = b.send().await.map_err(|e| e.without_url().to_string())?;
            let status = resp.status().as_u16();
            let text = |n: reqwest::header::HeaderName| {
                resp.headers()
                    .get(n)
                    .and_then(|v| v.to_str().ok())
                    .map(str::to_string)
            };
            let content_range = text(reqwest::header::CONTENT_RANGE);
            let content_type = text(reqwest::header::CONTENT_TYPE);
            let body = resp
                .bytes()
                .await
                .map_err(|e| e.without_url().to_string())?;
            Ok(HttpResponse {
                status,
                body,
                content_range,
                content_type,
            })
        })
    }
}

type Handler = dyn Fn(&HttpRequest) -> Option<HttpResponse> + Send + Sync;

/// A fake transport: a handler answers each request (`None` is a 404), and
/// every request is logged for assertions.
#[derive(Clone)]
pub struct FakeYtmHttp {
    handler: Arc<Handler>,
    pub log: Arc<Mutex<Vec<HttpRequest>>>,
}

impl FakeYtmHttp {
    pub fn new(
        handler: impl Fn(&HttpRequest) -> Option<HttpResponse> + Send + Sync + 'static,
    ) -> FakeYtmHttp {
        FakeYtmHttp {
            handler: Arc::new(handler),
            log: Arc::default(),
        }
    }

    /// The InnerTube endpoints called, in order (`player`, `browse`…).
    pub fn endpoints(&self) -> Vec<String> {
        self.log
            .lock()
            .iter()
            .filter_map(|r| {
                let path = r.url.split('?').next()?;
                path.rsplit_once("/youtubei/v1/")
                    .map(|(_, e)| e.to_string())
            })
            .collect()
    }
}

impl YtmHttp for FakeYtmHttp {
    fn send(&self, req: HttpRequest) -> BoxFuture<'static, Result<HttpResponse, String>> {
        self.log.lock().push(req.clone());
        let r = (self.handler)(&req).unwrap_or_else(|| HttpResponse::json(404, "{}"));
        Box::pin(async move { Ok(r) })
    }
}
