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
//!   `<data-dir>/ytm-session.json` (owner-only permissions). The player
//!   clients that need no JS never send a session, so signing in changes the
//!   catalogue calls, not which streams resolve (see the Phase 4 follow-up
//!   notes).
//! - **Priming.** Records are of the decoded audio with the MP4 edit list
//!   applied (the 1600-frame AAC priming trimmed), as both platforms play it,
//!   so they line up with what devices hear.

use std::collections::{HashSet, VecDeque};
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
use hocket_core::ytm::YtmProvider;

use crate::analysis::AnalysisStore;

/// The analysis store's provider key for YouTube Music.
pub const PROVIDER: &str = "ytm";
/// Where the coordinator's own session is kept.
pub const SESSION_FILE: &str = "ytm-session.json";
/// Video ids the queue holds; past it the oldest are dropped.
pub const QUEUE_MAX: usize = 256;
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
    dir: Option<PathBuf>,
    session: Mutex<Option<Session>>,
    provider: RwLock<Arc<YtmProvider>>,
    queue: Mutex<(VecDeque<String>, HashSet<String>)>,
    wake: tokio::sync::Notify,
}

fn provider_for(
    http: &Arc<dyn YtmHttp>,
    clock: &Arc<dyn Clock>,
    session: Option<&Session>,
) -> Arc<YtmProvider> {
    let cookies = session.and_then(|s| Cookies::parse(&s.cookies));
    let id = session.map_or_else(|| "ytm:coordinator".to_string(), |s| s.channel.clone());
    let it = Arc::new(InnerTube::new(http.clone(), clock.clone(), cookies));
    Arc::new(YtmProvider::new(id, it, clock.clone()))
}

/// A YouTube Music video id as clients send it (11 URL-safe characters).
fn video_id_ok(id: &str) -> bool {
    id.len() == 11
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

impl YtmAnalysis {
    /// Anonymous, or signed in with the session kept in `dir`.
    pub fn open(http: Arc<dyn YtmHttp>, clock: Arc<dyn Clock>, dir: Option<&Path>) -> YtmAnalysis {
        let session = dir.and_then(|d| {
            let text = std::fs::read_to_string(d.join(SESSION_FILE)).ok()?;
            serde_json::from_str::<Session>(&text)
                .map_err(|e| warn!(error = %e, "ytm-session.json unreadable; anonymous"))
                .ok()
        });
        let provider = provider_for(&http, &clock, session.as_ref());
        YtmAnalysis {
            http,
            clock,
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
        {
            let mut q = self.queue.lock();
            for k in keys {
                if k.provider != PROVIDER {
                    continue;
                }
                let Some(id) = k.fingerprint.split('|').next().filter(|id| video_id_ok(id)) else {
                    continue;
                };
                if q.1.contains(id) || q.0.iter().any(|x| x == id) {
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

    fn mark_failed(&self, id: &str) {
        self.queue.lock().1.insert(id.to_string());
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
        *self.provider.write() = provider_for(&self.http, &self.clock, session.as_ref());
        *self.session.lock() = session;
        Ok(())
    }

    /// Resolve, download, analyse and store one video's itag 140 record.
    pub async fn analyse_one(
        &self,
        analyser: &mut Option<Analyser>,
        store: &AnalysisStore,
        video_id: &str,
        tmp_dir: &Path,
    ) -> Result<(), String> {
        let provider = self.provider.read().clone();
        // Resolved here, for this address: never a device's URL.
        let stream = provider
            .resolve_stream(video_id, &StreamRequest::default())
            .await
            .map_err(|e| format!("resolve: {e}"))?;
        if stream.content_length.is_some_and(|l| l > MAX_BYTES) {
            return Err("stream too large".into());
        }
        let itag: u32 = stream.variant.parse().map_err(|_| "no itag")?;
        std::fs::create_dir_all(tmp_dir).map_err(|e| format!("tmp: {e}"))?;
        let path = tmp_dir.join(format!("ytm-{video_id}.m4a"));
        let result = async {
            provider
                .download_to_file(&stream, &path)
                .await
                .map_err(|e| format!("download: {e}"))?;
            if analyser.is_none() {
                *analyser = Some(Analyser::new(1).map_err(|e| format!("analyser: {e}"))?);
            }
            let a = analyser.as_mut().expect("just set");
            a.analyse_file(&path, AnalysisProducer::Worker)
                .map_err(|e| format!("analyse: {e}"))
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
            .map_err(|e| format!("store: {e}"))?;
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
                    self.mark_failed(&id);
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
    let tmp = path.with_extension("tmp");
    {
        use std::io::Write;
        let mut opts = std::fs::OpenOptions::new();
        opts.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o600);
        }
        let mut f = opts.open(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
    }
    std::fs::rename(tmp, path)
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
