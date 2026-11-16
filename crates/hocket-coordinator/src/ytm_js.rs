//! The coordinator's YouTube Music JS host (`docs/providers.md`, Phase 4
//! follow-up): yt-dlp's EJS solver, the same vendored script the platforms
//! run, in an embedded QuickJS (`rquickjs`). It answers the core's
//! [`JsHost`] requests for the TV app, the one client that streams with the
//! coordinator's own session and needs no PO token:
//!
//! - `Prepare`: the current player's signature timestamp (`iframe_api` names
//!   the player, `base.js` is fetched once per player). No PO token: minting
//!   one needs BotGuard in a browser, which the coordinator doesn't have, so
//!   this host offers only [`ClientKind::Tv`].
//! - `Decipher`: each format's signature (`s` → `sp`) and `n`, through the
//!   player's own functions. The first solve parses the player (seconds in
//!   QuickJS) and keeps EJS's preprocessed form; later ones reuse it.
//!
//! Each solve runs on a short-lived thread of its own (QuickJS isn't `Send`,
//! and the parser wants a deep stack) with a memory cap and a deadline.

use std::sync::Arc;
use std::time::{Duration, Instant};

use futures::future::BoxFuture;
use serde_json::{json, Value};
use tracing::debug;

use hocket_core::util::Clock;
use hocket_core::ytm::clients::{ClientKind, TV};
use hocket_core::ytm::http::{HttpMethod, HttpRequest, YtmHttp};
use hocket_core::ytm::player::{JsAnswer, JsHost, JsRequest};

const ORIGIN: &str = "https://www.youtube.com";
/// How long a player is used before `iframe_api` is asked again.
pub const PLAYER_TTL_MS: f64 = 6.0 * 3_600_000.0;
/// QuickJS's heap cap for one solve (parsing a ~2.5 MB player needs a few hundred MB).
pub const MEMORY_LIMIT: usize = 1 << 30;
/// The longest one solve may run.
pub const DEADLINE: Duration = Duration::from_secs(120);
/// The solving thread's stack (meriyah recurses deeply on the player).
const STACK: usize = 64 << 20;

#[derive(Clone)]
struct Player {
    id: String,
    sts: Option<u32>,
    /// The player's source until EJS has preprocessed it.
    source: Option<Arc<String>>,
    prepared: Option<Arc<String>>,
    at: f64,
}

struct Inner {
    http: Arc<dyn YtmHttp>,
    clock: Arc<dyn Clock>,
    player: tokio::sync::Mutex<Option<Player>>,
}

/// The coordinator's [`JsHost`]: EJS in QuickJS, for the TV app.
#[derive(Clone)]
pub struct EmbeddedJsHost(Arc<Inner>);

impl EmbeddedJsHost {
    pub fn new(http: Arc<dyn YtmHttp>, clock: Arc<dyn Clock>) -> EmbeddedJsHost {
        EmbeddedJsHost(Arc::new(Inner {
            http,
            clock,
            player: tokio::sync::Mutex::new(None),
        }))
    }

    /// Start from an already preprocessed player (`id` as `iframe_api`
    /// names it): tests, without a real `base.js`.
    pub fn with_preprocessed(self, id: &str, sts: u32, preprocessed: &str) -> EmbeddedJsHost {
        let at = self.0.clock.now_ms();
        *self.0.player.try_lock().expect("fresh host") = Some(Player {
            id: id.into(),
            sts: Some(sts),
            source: None,
            prepared: Some(Arc::new(preprocessed.into())),
            at,
        });
        self
    }
}

impl JsHost for EmbeddedJsHost {
    fn solve(&self, req: JsRequest) -> BoxFuture<'static, Result<JsAnswer, String>> {
        let inner = self.0.clone();
        Box::pin(async move {
            match req {
                JsRequest::Prepare { .. } => {
                    let p = inner.player().await?;
                    Ok(JsAnswer {
                        signature_timestamp: p.sts,
                        po_token: None,
                        urls: vec![],
                    })
                }
                JsRequest::Decipher { formats, .. } => inner.decipher(&formats).await,
            }
        })
    }

    fn clients(&self) -> &'static [ClientKind] {
        &[ClientKind::Tv]
    }
}

impl Inner {
    async fn get(&self, url: &str) -> Result<String, String> {
        let r = self
            .http
            .send(HttpRequest {
                method: HttpMethod::Get,
                url: url.into(),
                headers: vec![("User-Agent".into(), TV.user_agent.into())],
                body: None,
            })
            .await?;
        if !(200..300).contains(&r.status) {
            return Err(format!("player fetch: HTTP {}", r.status));
        }
        Ok(String::from_utf8_lossy(&r.body).into_owned())
    }

    /// The current player, fetched when `iframe_api` names a new one.
    async fn player(&self) -> Result<Player, String> {
        let mut slot = self.player.lock().await;
        let now = self.clock.now_ms();
        if let Some(p) = slot.as_ref().filter(|p| now - p.at < PLAYER_TTL_MS) {
            return Ok(p.clone());
        }
        let api = self.get(&format!("{ORIGIN}/iframe_api")).await?;
        let id = player_id(&api).ok_or("no player id in iframe_api")?;
        if let Some(p) = slot.as_mut().filter(|p| p.id == id) {
            p.at = now;
            return Ok(p.clone());
        }
        let source = self
            .get(&format!(
                "{ORIGIN}/s/player/{id}/player_ias.vflset/en_US/base.js"
            ))
            .await?;
        let p = Player {
            sts: signature_timestamp(&source),
            id,
            source: Some(Arc::new(source)),
            prepared: None,
            at: now,
        };
        debug!(player = %p.id, sts = ?p.sts, "YouTube player fetched");
        *slot = Some(p.clone());
        Ok(p)
    }

    async fn decipher(&self, formats: &[String]) -> Result<JsAnswer, String> {
        let parsed = formats
            .iter()
            .map(|f| parse_format(f))
            .collect::<Result<Vec<_>, _>>()?;
        let mut sigs: Vec<String> = vec![];
        let mut ns: Vec<String> = vec![];
        for f in &parsed {
            if let Some(s) = &f.s {
                if !sigs.contains(s) {
                    sigs.push(s.clone());
                }
            }
            if let Some(n) = query_param(&f.url, "n") {
                if !ns.contains(&n) {
                    ns.push(n);
                }
            }
        }
        let mut requests = vec![];
        if !sigs.is_empty() {
            requests.push(json!({"type": "sig", "challenges": sigs}));
        }
        if !ns.is_empty() {
            requests.push(json!({"type": "n", "challenges": ns}));
        }
        let solved = if requests.is_empty() {
            Value::Null
        } else {
            self.run(requests).await?
        };
        let urls = parsed
            .iter()
            .map(|f| {
                let sig =
                    f.s.as_ref()
                        .map(|s| {
                            solved["sig"][s]
                                .as_str()
                                .map(str::to_string)
                                .ok_or("sig: no answer")
                        })
                        .transpose()?;
                let n = query_param(&f.url, "n")
                    .map(|n| {
                        solved["n"][&n]
                            .as_str()
                            .map(str::to_string)
                            .ok_or("n: no answer")
                    })
                    .transpose()?;
                rebuild(
                    &f.url,
                    sig.as_deref().map(|s| (f.sp.as_str(), s)),
                    n.as_deref(),
                )
            })
            .collect::<Result<Vec<_>, String>>()?;
        Ok(JsAnswer {
            urls,
            ..Default::default()
        })
    }

    /// One `jsc` run for these requests; answers `{sig: {challenge: answer}, n: {…}}`.
    async fn run(&self, requests: Vec<Value>) -> Result<Value, String> {
        let p = self.player().await?;
        let input = match (&p.prepared, &p.source) {
            (Some(prep), _) => json!({
                "type": "preprocessed",
                "preprocessed_player": prep.as_str(),
                "requests": requests,
            }),
            (None, Some(src)) => json!({
                "type": "player",
                "player": src.as_str(),
                "requests": requests,
                "output_preprocessed": true,
            }),
            (None, None) => return Err("no player".into()),
        };
        let started = Instant::now();
        let out = run_jsc(input.to_string()).await?;
        debug!(
            ms = started.elapsed().as_millis() as u64,
            preprocessed = p.prepared.is_some(),
            "EJS solved"
        );
        let out: Value = serde_json::from_str(&out).map_err(|e| format!("EJS output: {e}"))?;
        if out["type"] != "result" {
            return Err(format!(
                "EJS: {}",
                out["error"].as_str().unwrap_or("the solver failed")
            ));
        }
        if let Some(prep) = out["preprocessed_player"].as_str() {
            let mut slot = self.player.lock().await;
            if let Some(cur) = slot.as_mut().filter(|c| c.id == p.id) {
                cur.prepared = Some(Arc::new(prep.to_string()));
                cur.source = None;
            }
        }
        let mut by = serde_json::Map::new();
        for (req, resp) in requests
            .iter()
            .zip(out["responses"].as_array().into_iter().flatten())
        {
            let kind = req["type"].as_str().unwrap_or("");
            if resp["type"] != "result" {
                return Err(format!(
                    "EJS {kind}: {}",
                    resp["error"].as_str().unwrap_or("no answer")
                ));
            }
            by.insert(kind.into(), resp["data"].clone());
        }
        Ok(Value::Object(by))
    }
}

/// Run EJS's `jsc` over `input` (JSON) in a fresh QuickJS on its own thread.
pub async fn run_jsc(input: String) -> Result<String, String> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    std::thread::Builder::new()
        .name("hocket-ytm-ejs".into())
        .stack_size(STACK)
        .spawn(move || {
            let _ = tx.send(run_jsc_blocking(&input));
        })
        .map_err(|e| format!("EJS thread: {e}"))?;
    rx.await.map_err(|_| "EJS thread died".to_string())?
}

fn run_jsc_blocking(input: &str) -> Result<String, String> {
    use rquickjs::{CatchResultExt, Context, Function, Runtime};
    let rt = Runtime::new().map_err(|e| format!("QuickJS: {e}"))?;
    rt.set_memory_limit(MEMORY_LIMIT);
    rt.set_max_stack_size(STACK - (4 << 20));
    let deadline = Instant::now() + DEADLINE;
    rt.set_interrupt_handler(Some(Box::new(move || Instant::now() > deadline)));
    let ctx = Context::full(&rt).map_err(|e| format!("QuickJS: {e}"))?;
    ctx.with(|ctx| {
        let fail = |e: rquickjs::CaughtError| format!("EJS: {e}");
        ctx.eval::<(), _>(hocket_core::ytm::ejs_solver_script())
            .catch(&ctx)
            .map_err(fail)?;
        let jsc: Function = ctx.globals().get("jsc").catch(&ctx).map_err(fail)?;
        let arg = ctx.json_parse(input).catch(&ctx).map_err(fail)?;
        let out: rquickjs::Value = jsc.call((arg,)).catch(&ctx).map_err(fail)?;
        let text = ctx
            .json_stringify(out)
            .catch(&ctx)
            .map_err(fail)?
            .ok_or("EJS answered nothing")?;
        text.to_string().map_err(|e| format!("EJS: {e}"))
    })
}

/// A format: its URL, and the scrambled signature with the parameter it
/// goes in when it came as a `signatureCipher`.
struct Format {
    url: String,
    s: Option<String>,
    sp: String,
}

fn parse_format(raw: &str) -> Result<Format, String> {
    if raw.starts_with("http://") || raw.starts_with("https://") {
        return Ok(Format {
            url: raw.into(),
            s: None,
            sp: "signature".into(),
        });
    }
    let mut url = None;
    let mut s = None;
    let mut sp = None;
    for (k, v) in url::form_urlencoded::parse(raw.as_bytes()) {
        match &*k {
            "url" => url = Some(v.into_owned()),
            "s" => s = Some(v.into_owned()),
            "sp" => sp = Some(v.into_owned()),
            _ => {}
        }
    }
    Ok(Format {
        url: url.ok_or("a format without a URL")?,
        s,
        sp: sp.unwrap_or_else(|| "signature".into()),
    })
}

fn query_param(url: &str, name: &str) -> Option<String> {
    let u = url::Url::parse(url).ok()?;
    u.query_pairs()
        .find(|(k, _)| k == name)
        .map(|(_, v)| v.into_owned())
}

/// `url` with the signature set (`(sp, sig)`) and `n` replaced.
fn rebuild(url: &str, sig: Option<(&str, &str)>, n: Option<&str>) -> Result<String, String> {
    let mut u = url::Url::parse(url).map_err(|_| "a format URL that doesn't parse")?;
    let mut pairs: Vec<(String, String)> = u
        .query_pairs()
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect();
    if let Some(n) = n {
        for p in pairs.iter_mut().filter(|(k, _)| k == "n") {
            p.1 = n.into();
        }
    }
    if let Some((sp, sig)) = sig {
        pairs.retain(|(k, _)| k != sp);
        pairs.push((sp.into(), sig.into()));
    }
    u.query_pairs_mut().clear().extend_pairs(pairs);
    Ok(u.into())
}

/// The player id in `iframe_api` (`…\/s\/player\/<id>\/…`).
pub fn player_id(api: &str) -> Option<String> {
    let plain = api.replace("\\/", "/");
    let at = plain.find("/s/player/")? + "/s/player/".len();
    let id: String = plain[at..]
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-')
        .collect();
    (!id.is_empty() && plain[at + id.len()..].starts_with('/')).then_some(id)
}

/// `signatureTimestamp:20767` (or `sts:20767`) in the player source.
pub fn signature_timestamp(source: &str) -> Option<u32> {
    for key in ["signatureTimestamp", "sts"] {
        let mut rest = source;
        while let Some(i) = rest.find(key) {
            let before_ok = rest[..i]
                .chars()
                .next_back()
                .is_none_or(|c| !(c.is_ascii_alphanumeric() || c == '_' || c == '$'));
            let after = rest[i + key.len()..].trim_start();
            if before_ok {
                if let Some(v) = after.strip_prefix(':') {
                    let digits: String = v
                        .trim_start()
                        .chars()
                        .take_while(char::is_ascii_digit)
                        .collect();
                    if digits.len() == 5 {
                        return digits.parse().ok();
                    }
                }
            }
            rest = &rest[i + key.len()..];
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_the_player_id_and_the_signature_timestamp() {
        let api = r#"var scriptUrl = 'https:\/\/www.youtube.com\/s\/player\/0c96dfd3\/www-widgetapi.vflset\/www-widgetapi.js';"#;
        assert_eq!(player_id(api).as_deref(), Some("0c96dfd3"));
        assert_eq!(player_id("nothing here"), None);
        assert_eq!(
            signature_timestamp("a,signatureTimestamp:20767,b"),
            Some(20767)
        );
        assert_eq!(signature_timestamp("x={sts: 20401}"), Some(20401));
        assert_eq!(signature_timestamp("ists:12345;sts:123"), None);
    }

    #[test]
    fn rebuilds_a_signature_cipher_url() {
        let f = parse_format("s=AB%3DC&sp=sig&url=https%3A%2F%2Frr1.googlevideo.com%2Fvideoplayback%3Fitag%3D140%26n%3Dxyz").unwrap();
        assert_eq!(f.s.as_deref(), Some("AB=C"));
        assert_eq!(f.sp, "sig");
        assert_eq!(query_param(&f.url, "n").as_deref(), Some("xyz"));
        let u = rebuild(&f.url, Some(("sig", "C=BA")), Some("zyx")).unwrap();
        assert_eq!(
            u,
            "https://rr1.googlevideo.com/videoplayback?itag=140&n=zyx&sig=C%3DBA"
        );
    }
}
