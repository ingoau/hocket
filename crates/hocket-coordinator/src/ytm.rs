//! AutoMix analysis of YouTube Music tracks on the coordinator
//! (`docs/providers.md`, Phase 4 follow-up). Off unless `--analysis-ytm`.
//!
//! - **On demand only.** There's no library to walk: tracks clients asked
//!   the store for and missed (`AnalysisKey::ytm(videoId, 140)`) are queued,
//!   and the worker analyses them one at a time with a pause between.
//! - **Its own URLs.** googlevideo URLs are bound to the address that
//!   resolved them, so the worker resolves every stream itself (the visionOS
//!   client and the rest of the chain, as a device does) and never takes one
//!   from a device: a request names only the video id.
//! - **Anonymous by default.** The owner identity's devices can sign it in
//!   with a session of its own (`Msg::CoordYtmSignIn`): the `Cookie:`
//!   header arrives sealed to the coordinator's key (the Phase 2 envelope),
//!   is checked against the account menu, and is kept in
//!   `<data-dir>/ytm-session.json` (owner-only permissions).
//! - **Streams with the account.** Signed in, the worker resolves through
//!   the TV app first, with the session (cookies, `SAPISIDHASH`), its
//!   signature and `n` solved by yt-dlp's EJS in an embedded QuickJS
//!   ([`crate::ytm_js`]), so uploads and account- or region-only tracks can
//!   be analysed; when that's refused it falls back to the anonymous chain.
//!   The no-JS clients never send the session.
//! - **Priming.** Records are of the decoded audio with the MP4 edit list
//!   applied (the 1600-frame AAC priming trimmed), as both platforms play it,
//!   so they line up with what devices hear.

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use parking_lot::{Mutex, RwLock};
use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, warn};

use hocket_core::api::{AnalysisProducer, SourceFingerprint};
use hocket_core::automix::analysis::Analyser;
use hocket_core::automix::record::compact;
use hocket_core::connect::wire::{AnalysisKey, COORD_YTM_CONTEXT};
use hocket_core::identity::crypto::{DeviceSecret, Envelope};
use hocket_core::provider::{Provider, StreamRequest};
use hocket_core::util::Clock;
use hocket_core::ytm::auth::Cookies;
use hocket_core::ytm::client::InnerTube;
use hocket_core::ytm::http::YtmHttp;
use hocket_core::ytm::player::JsHost;
use hocket_core::ytm::YtmProvider;

use crate::analysis::AnalysisStore;

/// The analysis store's provider key for YouTube Music.
pub const PROVIDER: &str = "ytm";
/// Where the coordinator's own session is kept.
pub const SESSION_FILE: &str = "ytm-session.json";
/// Video ids the queue holds; past it the oldest are dropped.
pub const QUEUE_MAX: usize = 256;
/// Failed video ids remembered; past it the oldest are forgotten.
pub const FAILED_MAX: usize = 4096;
/// First wait after a failure that may pass (a network error, a refused
/// stream); doubled per repeat, up to [`RETRY_MAX_MS`].
pub const RETRY_BASE_MS: u64 = 10 * 60_000;
pub const RETRY_MAX_MS: u64 = 24 * 3_600_000;

/// Why one analysis failed: worth another try later, or never (the stream
/// is too large or can't be analysed).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Failure {
    Transient(String),
    Permanent(String),
}

impl std::fmt::Display for Failure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Failure::Transient(e) | Failure::Permanent(e) => f.write_str(e),
        }
    }
}

/// Video ids that failed: a bounded map (oldest forgotten first) with a
/// retry time per id (never, for a permanent failure).
#[derive(Default)]
struct Failed {
    /// id → (retry at, ms or `u64::MAX`; transient failures so far; order).
    by_id: HashMap<String, (u64, u32, u64)>,
    seq: u64,
}

impl Failed {
    fn blocks(&self, id: &str, now: u64) -> bool {
        self.by_id.get(id).is_some_and(|(until, _, _)| now < *until)
    }

    fn note(&mut self, id: &str, failure: &Failure, now: u64) {
        let tries = self.by_id.get(id).map_or(0, |e| e.1) + 1;
        let until = match failure {
            Failure::Permanent(_) => u64::MAX,
            Failure::Transient(_) => {
                let wait = RETRY_BASE_MS.saturating_mul(1 << (tries - 1).min(16));
                now.saturating_add(wait.min(RETRY_MAX_MS))
            }
        };
        self.seq += 1;
        self.by_id.insert(id.to_string(), (until, tries, self.seq));
        while self.by_id.len() > FAILED_MAX {
            let oldest = self
                .by_id
                .iter()
                .min_by_key(|(_, e)| e.2)
                .map(|(k, _)| k.clone())
                .expect("not empty");
            self.by_id.remove(&oldest);
        }
    }
}
/// The longest stream the worker downloads.
pub const MAX_BYTES: u64 = 200 << 20;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
struct Session {
    cookies: String,
    channel: String,
    name: String,
}

/// What `Msg::CoordYtmStatus` says.
#[derive(Debug, Clone, PartialEq)]
pub struct Status {
    pub signed_in: bool,
    pub account: Option<String>,
}

pub struct YtmAnalysis {
    http: Arc<dyn YtmHttp>,
    clock: Arc<dyn Clock>,
    js: Arc<dyn JsHost>,
    dir: Option<PathBuf>,
    session: Mutex<Option<Session>>,
    provider: RwLock<Arc<YtmProvider>>,
    queue: Mutex<(VecDeque<String>, Failed)>,
    wake: tokio::sync::Notify,
}

fn provider_for(
    http: &Arc<dyn YtmHttp>,
    clock: &Arc<dyn Clock>,
    js: &Arc<dyn JsHost>,
    session: Option<&Session>,
) -> Arc<YtmProvider> {
    let cookies = session.and_then(|s| Cookies::parse(&s.cookies));
    let id = session.map_or_else(|| "ytm:coordinator".to_string(), |s| s.channel.clone());
    let it = Arc::new(InnerTube::new(http.clone(), clock.clone(), cookies));
    let p = YtmProvider::new(id, it, clock.clone());
    p.set_js_host(Some(js.clone()));
    Arc::new(p)
}

/// A YouTube Music video id as clients send it (11 URL-safe characters).
fn video_id_ok(id: &str) -> bool {
    id.len() == 11
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

impl YtmAnalysis {
    /// Anonymous, or signed in with the session kept in `dir`; the TV app's
    /// puzzles solved by the embedded EJS ([`crate::ytm_js`]).
    pub fn open(http: Arc<dyn YtmHttp>, clock: Arc<dyn Clock>, dir: Option<&Path>) -> YtmAnalysis {
        let js = Arc::new(crate::ytm_js::EmbeddedJsHost::new(
            http.clone(),
            clock.clone(),
        ));
        YtmAnalysis::open_with_js(http, clock, dir, js)
    }

    /// [`YtmAnalysis::open`] with another JS host (tests).
    pub fn open_with_js(
        http: Arc<dyn YtmHttp>,
        clock: Arc<dyn Clock>,
        dir: Option<&Path>,
        js: Arc<dyn JsHost>,
    ) -> YtmAnalysis {
        let session = dir.and_then(|d| {
            let text = std::fs::read_to_string(d.join(SESSION_FILE)).ok()?;
            serde_json::from_str::<Session>(&text)
                .map_err(|e| warn!(error = %e, "ytm-session.json unreadable; anonymous"))
                .ok()
        });
        let provider = provider_for(&http, &clock, &js, session.as_ref());
        YtmAnalysis {
            http,
            clock,
            js,
            dir: dir.map(Path::to_path_buf),
            session: Mutex::new(session),
            provider: RwLock::new(provider),
            queue: Mutex::default(),
            wake: tokio::sync::Notify::new(),
        }
    }

    pub fn status(&self) -> Status {
        let s = self.session.lock();
        Status {
            signed_in: s.is_some(),
            account: s.as_ref().map(|s| s.name.clone()),
        }
    }

    /// Queue the video ids of these YouTube Music keys (a client missed them).
    pub fn push<'a>(&self, keys: impl IntoIterator<Item = &'a AnalysisKey>) -> usize {
        let mut added = 0;
        let now = self.clock.now_ms() as u64;
        {
            let mut q = self.queue.lock();
            for k in keys {
                if k.provider != PROVIDER {
                    continue;
                }
                let Some(id) = k.fingerprint.split('|').next().filter(|id| video_id_ok(id)) else {
                    continue;
                };
                if q.1.blocks(id, now) || q.0.iter().any(|x| x == id) {
                    continue;
                }
                if q.0.len() >= QUEUE_MAX {
                    q.0.pop_front();
                }
                q.0.push_back(id.to_string());
                added += 1;
            }
        }
        if added > 0 {
            self.wake.notify_one();
        }
        added
    }

    pub fn queued(&self) -> usize {
        self.queue.lock().0.len()
    }

    fn pop(&self) -> Option<String> {
        self.queue.lock().0.pop_front()
    }

    fn mark_failed(&self, id: &str, failure: &Failure) {
        let now = self.clock.now_ms() as u64;
        self.queue.lock().1.note(id, failure, now);
    }

    /// Sign in with the session in `envelope` (sealed to `secret`'s key), or
    /// out (`None`). The session is checked against the account menu first.
    pub async fn sign_in(
        &self,
        secret: &DeviceSecret,
        envelope: Option<&Envelope>,
    ) -> Result<Status, String> {
        let Some(envelope) = envelope else {
            self.set_session(None)?;
            info!("YouTube Music analysis signed out: anonymous");
            return Ok(self.status());
        };
        let plain = secret
            .open_envelope(COORD_YTM_CONTEXT, envelope)
            .ok_or("the session wasn't sealed for this coordinator")?;
        let header = String::from_utf8(plain).map_err(|_| "the session isn't text")?;
        let cookies = Cookies::parse(&header).ok_or("that isn't a signed-in YouTube session")?;
        let it = InnerTube::new(self.http.clone(), self.clock.clone(), Some(cookies));
        let menu = it
            .account_menu()
            .await
            .map_err(|e| format!("YouTube Music refused the session: {e}"))?;
        let (channel, name) = hocket_core::ytm::parse::parse_account(&menu)
            .ok_or("YouTube Music didn't name the account")?;
        self.set_session(Some(Session {
            cookies: header,
            channel,
            name: name.clone(),
        }))?;
        info!(account = %name, "YouTube Music analysis signed in");
        Ok(self.status())
    }

    fn set_session(&self, session: Option<Session>) -> Result<(), String> {
        if let Some(dir) = &self.dir {
            let path = dir.join(SESSION_FILE);
            match &session {
                Some(s) => write_private(&path, &serde_json::to_vec(s).expect("serialises"))
                    .map_err(|e| format!("couldn't keep the session: {e}"))?,
                None => {
                    let _ = std::fs::remove_file(&path);
                }
            }
        }
        *self.provider.write() = provider_for(&self.http, &self.clock, &self.js, session.as_ref());
        *self.session.lock() = session;
        // Another account may stream what this one couldn't.
        self.queue.lock().1 = Failed::default();
        Ok(())
    }

    /// Resolve, download, analyse and store one video's itag 140 record.
    pub async fn analyse_one(
        &self,
        analyser: &mut Option<Analyser>,
        store: &AnalysisStore,
        video_id: &str,
        tmp_dir: &Path,
    ) -> Result<(), Failure> {
        use Failure::{Permanent, Transient};
        let provider = self.provider.read().clone();
        // Resolved here, for this address: never a device's URL.
        let stream = provider
            .resolve_stream(video_id, &StreamRequest::default())
            .await
            .map_err(|e| Transient(format!("resolve: {e}")))?;
        if stream.content_length.is_some_and(|l| l > MAX_BYTES) {
            return Err(Permanent("stream too large".into()));
        }
        let itag: u32 = stream
            .variant
            .parse()
            .map_err(|_| Permanent("no itag".into()))?;
        std::fs::create_dir_all(tmp_dir).map_err(|e| Transient(format!("tmp: {e}")))?;
        let path = tmp_dir.join(format!("ytm-{video_id}.m4a"));
        let result = async {
            provider
                .download_to_file(&stream, &path)
                .await
                .map_err(|e| Transient(format!("download: {e}")))?;
            if analyser.is_none() {
                *analyser =
                    Some(Analyser::new(1).map_err(|e| Transient(format!("analyser: {e}")))?);
            }
            let a = analyser.as_mut().expect("just set");
            a.analyse_file(&path, AnalysisProducer::Worker)
                .map_err(|e| Permanent(format!("analyse: {e}")))
        }
        .await;
        let _ = std::fs::remove_file(&path);
        let mut record = result?;
        // Devices know a YouTube Music track by its duration only.
        record.source = SourceFingerprint {
            duration_ms: record.duration_ms,
            size_bytes: None,
            suffix: None,
            transcoded: false,
        };
        let key = AnalysisKey::ytm(video_id, itag);
        store
            .put(
                &key.provider,
                &key.fingerprint,
                compact(record),
                self.clock.now_ms() as u64,
            )
            .map_err(|e| Transient(format!("store: {e}")))?;
        debug!(video = %video_id, itag, "YouTube Music track analysed");
        Ok(())
    }

    /// Analyse what's queued, one at a time, until `cancel`.
    pub async fn run(
        self: Arc<Self>,
        store: Arc<AnalysisStore>,
        tmp_dir: PathBuf,
        pause: Duration,
        cancel: CancellationToken,
    ) {
        let mut analyser = None;
        while !cancel.is_cancelled() {
            while let Some(id) = self.pop() {
                if cancel.is_cancelled() {
                    return;
                }
                info!(video = %id, "analysing a YouTube Music track a client asked for");
                if let Err(e) = self.analyse_one(&mut analyser, &store, &id, &tmp_dir).await {
                    warn!(video = %id, error = %e, "YouTube Music analysis failed");
                    self.mark_failed(&id, &e);
                }
                tokio::select! {
                    _ = tokio::time::sleep(pause) => {}
                    _ = cancel.cancelled() => return,
                }
            }
            tokio::select! {
                _ = self.wake.notified() => {}
                _ = cancel.cancelled() => return,
            }
        }
    }
}

/// Write `bytes` readable by this user only.
fn write_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    crate::identities::write_atomic(path, bytes)
}

/// The worker on its own thread at a lowered priority, as the Navidrome one.
pub fn spawn(
    ytm: Arc<YtmAnalysis>,
    store: Arc<AnalysisStore>,
    tmp_dir: PathBuf,
    pause: Duration,
    cancel: CancellationToken,
) -> anyhow::Result<std::thread::JoinHandle<()>> {
    info!(
        signed_in = ytm.status().signed_in,
        "YouTube Music analysis starting"
    );
    Ok(std::thread::Builder::new()
        .name("hocket-ytm-analysis".into())
        .spawn(move || {
            crate::worker::lower_priority(10);
            let rt = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(rt) => rt,
                Err(e) => {
                    warn!(error = %e, "YouTube Music analysis runtime");
                    return;
                }
            };
            rt.block_on(ytm.run(store, tmp_dir, pause, cancel));
        })?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use hocket_core::util::FixedClock;
    use hocket_core::ytm::http::FakeYtmHttp;

    const V: &str = "dQw4w9WgXcQ";

    fn ytm(clock: Arc<FixedClock>) -> YtmAnalysis {
        YtmAnalysis::open(Arc::new(FakeYtmHttp::new(|_| None)), clock, None)
    }

    fn key(id: &str) -> AnalysisKey {
        AnalysisKey::ytm(id, 140)
    }

    /// A failure that may pass is retried after a backoff that grows; one
    /// that won't is not; signing in forgets both.
    #[test]
    fn failures_back_off_expire_and_clear_on_sign_in() {
        let clock = Arc::new(FixedClock::new(1_000_000.0));
        let y = ytm(clock.clone());
        assert_eq!(y.push([&key(V)]), 1);
        assert_eq!(y.pop().as_deref(), Some(V));
        y.mark_failed(V, &Failure::Transient("resolve: 503".into()));
        assert_eq!(y.push([&key(V)]), 0);
        clock.advance(RETRY_BASE_MS as f64 + 1.0);
        assert_eq!(y.push([&key(V)]), 1);
        assert_eq!(y.pop().as_deref(), Some(V));
        // Twice: twice the wait.
        y.mark_failed(V, &Failure::Transient("resolve: 503".into()));
        clock.advance(RETRY_BASE_MS as f64 + 1.0);
        assert_eq!(y.push([&key(V)]), 0);
        clock.advance(RETRY_BASE_MS as f64);
        assert_eq!(y.push([&key(V)]), 1);
        assert_eq!(y.pop().as_deref(), Some(V));
        // Permanent: never again, until a sign-in.
        y.mark_failed(V, &Failure::Permanent("stream too large".into()));
        clock.advance(RETRY_MAX_MS as f64 * 10.0);
        assert_eq!(y.push([&key(V)]), 0);
        y.set_session(None).unwrap();
        assert_eq!(y.push([&key(V)]), 1);
    }

    #[test]
    fn the_failed_set_is_bounded() {
        let mut f = Failed::default();
        for i in 0..FAILED_MAX + 10 {
            f.note(&format!("v{i}"), &Failure::Permanent("x".into()), 0);
        }
        assert_eq!(f.by_id.len(), FAILED_MAX);
        assert!(!f.blocks("v0", 0));
        assert!(f.blocks(&format!("v{}", FAILED_MAX + 9), 0));
    }
}
