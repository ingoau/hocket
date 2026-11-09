//! The opt-in AutoMix analysis worker (`docs/automix.md` → *Coordinator
//! worker*). Off unless `--analysis-server` is given. With its own non-admin
//! Navidrome account it walks the whole library (`search3` with an empty
//! query, page by page), skips every track that already has a usable record
//! with a beat grid, and for the rest downloads the original file
//! (`download`) to a temporary file, analyses it, stores the record
//! (produced by `Worker`, fingerprinted from the song's metadata) and
//! deletes the file. One track at a time on one thread at a lowered
//! priority, with a pause between tracks; the library is walked again every
//! `--analysis-rescan-hours`. Progress is logged and shown in `/health`.
//!
//! The password comes from `--analysis-password-file` or
//! `HOCKET_ANALYSIS_PASSWORD` and is never logged.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, warn};

use hocket_core::api::{AnalysisProducer, SourceFingerprint, Track};
use hocket_core::automix::analysis::Analyser;
use hocket_core::automix::record::compact;
use hocket_core::automix::store::fingerprint_of;
use hocket_core::subsonic::convert::track_from_child;
use hocket_core::subsonic::{Search3Page, SubsonicApi};

use crate::analysis::{AnalysisStore, PutOutcome, MAX_DURATION_MS};

/// Songs per `search3` page (Navidrome's cap).
pub const PAGE_SIZE: u32 = 500;
/// Wait after a failed library walk before trying again.
const RETRY_AFTER_FAILED_WALK: Duration = Duration::from_secs(600);
/// A progress line at most this often (and after every pass).
const PROGRESS_LOG_INTERVAL: Duration = Duration::from_secs(300);
/// The environment variable the password may come from.
pub const PASSWORD_ENV: &str = "HOCKET_ANALYSIS_PASSWORD";

#[derive(clap::Args, Debug, Clone, Default)]
pub struct WorkerArgs {
    /// Run the AutoMix analysis worker against this Navidrome server: the URL
    /// your clients use (records are keyed by it). Off when not given.
    #[arg(long, value_name = "URL")]
    pub analysis_server: Option<String>,
    /// The worker's Navidrome account (create a dedicated non-admin user).
    #[arg(long, value_name = "NAME", requires = "analysis_server")]
    pub analysis_user: Option<String>,
    /// File holding that account's password (or set HOCKET_ANALYSIS_PASSWORD).
    #[arg(long, value_name = "PATH", requires = "analysis_server")]
    pub analysis_password_file: Option<PathBuf>,
    /// Nice value of the worker thread (0..=19; Linux only, 0 leaves it).
    #[arg(long, default_value_t = 10, value_parser = clap::value_parser!(i32).range(0..=19))]
    pub analysis_nice: i32,
    /// Pause between tracks, in seconds.
    #[arg(long, default_value_t = 2)]
    pub analysis_pause_secs: u64,
    /// Walk the library again this many hours after a pass ends.
    #[arg(long, default_value_t = 6)]
    pub analysis_rescan_hours: u64,
}

impl WorkerArgs {
    /// The worker's settings, or `None` when it is off. Reads the password.
    pub fn resolve(&self, tmp_dir: PathBuf) -> anyhow::Result<Option<WorkerSettings>> {
        let Some(server) = self.analysis_server.clone() else {
            return Ok(None);
        };
        let user = self
            .analysis_user
            .clone()
            .ok_or_else(|| anyhow::anyhow!("--analysis-server needs --analysis-user"))?;
        let password = match &self.analysis_password_file {
            Some(path) => std::fs::read_to_string(path)
                .map_err(|e| anyhow::anyhow!("--analysis-password-file: {e}"))?
                .trim_end_matches(['\r', '\n'])
                .to_string(),
            None => std::env::var(PASSWORD_ENV).map_err(|_| {
                anyhow::anyhow!(
                    "--analysis-server needs --analysis-password-file or {PASSWORD_ENV}"
                )
            })?,
        };
        if password.is_empty() {
            anyhow::bail!("the analysis worker's password is empty");
        }
        Ok(Some(WorkerSettings {
            server,
            user,
            password: hocket_core::subsonic::Credential::new(password),
            nice: self.analysis_nice,
            pause: Duration::from_secs(self.analysis_pause_secs),
            rescan: Duration::from_secs(self.analysis_rescan_hours.max(1) * 3600),
            tmp_dir,
        }))
    }
}

/// [`WorkerArgs`], checked.
#[derive(Debug, Clone)]
pub struct WorkerSettings {
    pub server: String,
    pub user: String,
    pub password: hocket_core::subsonic::Credential,
    pub nice: i32,
    pub pause: Duration,
    pub rescan: Duration,
    /// Downloads go here; emptied when the worker starts.
    pub tmp_dir: PathBuf,
}

impl WorkerSettings {
    /// A Subsonic client for the worker's account.
    pub fn client(&self) -> anyhow::Result<Arc<dyn SubsonicApi>> {
        use hocket_core::subsonic::{AuthMode, Client, ClientConfig, ReqwestTransport};
        let url = url::Url::parse(self.server.trim())?;
        let config = ClientConfig::new(
            "analysis-worker",
            url,
            AuthMode::Password {
                username: self.user.clone(),
                password: self.password.clone(),
            },
        );
        // Downloads of long lossless files take a while on a slow link.
        let transport = ReqwestTransport::new(
            concat!("hocket-coordinator/", env!("CARGO_PKG_VERSION")),
            Duration::from_secs(1800),
        )
        .map_err(|e| anyhow::anyhow!("http client: {e}"))?;
        Ok(Arc::new(Client::try_new(config, Arc::new(transport))?))
    }
}

/// What `/health` shows under `analysis`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkerStatus {
    /// `starting`, `walking`, `analysing`, `waiting` or `failing`.
    pub state: String,
    /// Library walks finished.
    pub passes: u64,
    /// Songs in the library at the last walk.
    pub library_tracks: u64,
    /// Tracks still to analyse in this pass.
    pub remaining: u64,
    /// Since the worker started.
    pub analysed: u64,
    pub failed: u64,
    /// Tracks analysed per hour over this process's analysing time.
    pub per_hour: f64,
}

/// One pass's outcome.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PassStats {
    pub library: usize,
    pub todo: usize,
    pub analysed: usize,
    pub failed: usize,
}

pub struct Worker {
    api: Arc<dyn SubsonicApi>,
    store: Arc<AnalysisStore>,
    /// The store's key for this server.
    server_key: String,
    pause: Duration,
    tmp_dir: PathBuf,
    status: Arc<Mutex<Option<WorkerStatus>>>,
    analyser: Option<Analyser>,
    /// Files that failed, by (track id, fingerprint): retried only when the file changes.
    failed: HashSet<(String, String)>,
    busy: Duration,
    last_progress: Instant,
}

impl Worker {
    pub fn new(
        api: Arc<dyn SubsonicApi>,
        store: Arc<AnalysisStore>,
        server_url: &str,
        pause: Duration,
        tmp_dir: PathBuf,
        status: Arc<Mutex<Option<WorkerStatus>>>,
    ) -> Worker {
        *status.lock() = Some(WorkerStatus {
            state: "starting".into(),
            ..Default::default()
        });
        Worker {
            api,
            store,
            server_key: hocket_core::connect::wire::server_key(server_url),
            pause,
            tmp_dir,
            status,
            analyser: None,
            failed: HashSet::new(),
            busy: Duration::ZERO,
            last_progress: Instant::now(),
        }
    }

    fn set_state(&self, state: &str) {
        if let Some(s) = self.status.lock().as_mut() {
            s.state = state.into();
        }
    }

    fn update(&self, f: impl FnOnce(&mut WorkerStatus)) {
        if let Some(s) = self.status.lock().as_mut() {
            f(s);
        }
    }

    /// Every song in the library.
    pub async fn walk(&self) -> anyhow::Result<Vec<Track>> {
        let mut out = vec![];
        let mut offset = 0;
        loop {
            let page = self
                .api
                .search3("", Search3Page::songs(PAGE_SIZE, offset))
                .await
                .map_err(|e| anyhow::anyhow!("search3 at {offset}: {e}"))?;
            let n = page.song.len() as u32;
            out.extend(
                page.song
                    .iter()
                    .filter(|c| !c.is_dir)
                    .map(|c| track_from_child("", c)),
            );
            if n < PAGE_SIZE {
                return Ok(out);
            }
            offset += n;
        }
    }

    fn wanted(&self, t: &Track) -> bool {
        if t.duration_ms == 0 || t.duration_ms > MAX_DURATION_MS {
            return false;
        }
        let fp = fingerprint_of(t);
        if self.failed.contains(&(t.id.clone(), fp_key(&fp))) {
            return false;
        }
        match self.store.has_beat_record(&self.server_key, &t.id, &fp) {
            Ok(has) => !has,
            Err(e) => {
                warn!(error = %e, "analysis store read failed");
                false
            }
        }
    }

    /// Walk the library and analyse what lacks a record, until done or
    /// cancelled.
    pub async fn pass(&mut self, cancel: &CancellationToken) -> anyhow::Result<PassStats> {
        self.set_state("walking");
        let library = self.walk().await?;
        let todo: Vec<Track> = library.iter().filter(|t| self.wanted(t)).cloned().collect();
        let mut stats = PassStats {
            library: library.len(),
            todo: todo.len(),
            ..Default::default()
        };
        self.update(|s| {
            s.library_tracks = library.len() as u64;
            s.remaining = todo.len() as u64;
        });
        info!(
            library = library.len(),
            to_analyse = todo.len(),
            "analysis pass started"
        );
        for (i, track) in todo.iter().enumerate() {
            if cancel.is_cancelled() {
                break;
            }
            self.set_state("analysing");
            let started = Instant::now();
            match self.analyse_one(track).await {
                Ok(()) => {
                    stats.analysed += 1;
                    self.update(|s| s.analysed += 1);
                }
                Err(e) => {
                    stats.failed += 1;
                    self.failed
                        .insert((track.id.clone(), fp_key(&fingerprint_of(track))));
                    self.update(|s| s.failed += 1);
                    warn!(track = %track.id, title = %track.title, error = %e, "analysis failed");
                }
            }
            self.busy += started.elapsed();
            let per_hour = self.status.lock().as_ref().map_or(0, |s| s.analysed) as f64
                / self.busy.as_secs_f64().max(1.0)
                * 3600.0;
            let remaining = todo.len() - i - 1;
            self.update(|s| {
                s.remaining = remaining as u64;
                s.per_hour = (per_hour * 10.0).round() / 10.0;
            });
            if self.last_progress.elapsed() >= PROGRESS_LOG_INTERVAL {
                self.last_progress = Instant::now();
                info!(
                    analysed = stats.analysed,
                    failed = stats.failed,
                    remaining,
                    per_hour = format!("{per_hour:.0}"),
                    "analysis progress"
                );
            }
            if remaining > 0 && !self.pause.is_zero() {
                self.set_state("waiting");
                tokio::select! {
                    _ = tokio::time::sleep(self.pause) => {}
                    _ = cancel.cancelled() => break,
                }
            }
        }
        self.update(|s| s.passes += 1);
        let counts = self
            .store
            .counts(Some(&self.server_key))
            .unwrap_or_default();
        info!(
            library = stats.library,
            analysed = stats.analysed,
            failed = stats.failed,
            stored = counts.records,
            with_beats = counts.with_beats,
            "analysis pass finished"
        );
        Ok(stats)
    }

    /// Download, analyse, store, delete.
    async fn analyse_one(&mut self, track: &Track) -> anyhow::Result<()> {
        let path = self.tmp_dir.join(temp_name(track));
        let result = self.download_and_analyse(track, &path).await;
        let _ = tokio::fs::remove_file(&path).await;
        let record = result?;
        let outcome = self.store.put(
            &self.server_key,
            &track.id,
            record,
            hocket_core::util::now_ms() as u64,
        )?;
        debug!(track = %track.id, stored = outcome == PutOutcome::Stored, "analysed");
        Ok(())
    }

    async fn download_and_analyse(
        &mut self,
        track: &Track,
        path: &Path,
    ) -> anyhow::Result<hocket_core::api::AnalysisRecord> {
        let url = self.api.download_url(&track.id);
        self.api
            .download_to_file(url, path)
            .await
            .map_err(|e| anyhow::anyhow!("download: {e}"))?;
        if self.analyser.is_none() {
            self.analyser = Some(Analyser::new(1).map_err(|e| anyhow::anyhow!("{e}"))?);
        }
        let analyser = self.analyser.as_mut().expect("just set");
        // Blocking on purpose: the worker's runtime is its own, on its own
        // (niced) thread, and does nothing else meanwhile.
        let mut record = analyser
            .analyse_file(path, AnalysisProducer::Worker)
            .map_err(|e| anyhow::anyhow!("analyse: {e}"))?;
        record.source = fingerprint_of(track);
        Ok(compact(record))
    }

    /// Passes until cancelled: a pass, then a wait of `rescan` (or a retry
    /// after a failed walk).
    pub async fn run(mut self, rescan: Duration, cancel: CancellationToken) {
        clear_dir(&self.tmp_dir);
        while !cancel.is_cancelled() {
            let wait = match self.pass(&cancel).await {
                Ok(_) => {
                    self.set_state("waiting");
                    rescan
                }
                Err(e) => {
                    self.set_state("failing");
                    warn!(error = %e, "analysis library walk failed; retrying later");
                    RETRY_AFTER_FAILED_WALK
                }
            };
            tokio::select! {
                _ = tokio::time::sleep(wait) => {}
                _ = cancel.cancelled() => {}
            }
        }
        clear_dir(&self.tmp_dir);
        info!("analysis worker stopped");
    }
}

/// Start the worker on its own thread (lowered priority, its own runtime).
pub fn spawn(
    settings: WorkerSettings,
    store: Arc<AnalysisStore>,
    status: Arc<Mutex<Option<WorkerStatus>>>,
    cancel: CancellationToken,
) -> anyhow::Result<std::thread::JoinHandle<()>> {
    let api = settings.client()?;
    info!(
        server = %settings.server,
        user = %settings.user,
        nice = settings.nice,
        pause_s = settings.pause.as_secs(),
        rescan_h = settings.rescan.as_secs() / 3600,
        "analysis worker starting"
    );
    Ok(std::thread::Builder::new()
        .name("hocket-analysis-worker".into())
        .spawn(move || {
            lower_priority(settings.nice);
            let rt = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(rt) => rt,
                Err(e) => {
                    warn!(error = %e, "analysis worker runtime");
                    return;
                }
            };
            let worker = Worker::new(
                api,
                store,
                &settings.server,
                settings.pause,
                settings.tmp_dir.clone(),
                status,
            );
            rt.block_on(worker.run(settings.rescan, cancel));
        })?)
}

/// Lower this thread's priority (Linux: the nice value is per thread, and
/// the inference pool the analyser creates on it inherits it).
fn lower_priority(nice: i32) {
    if nice <= 0 {
        return;
    }
    #[cfg(target_os = "linux")]
    {
        // SAFETY: plain syscalls on the calling thread, no pointers.
        let rc = unsafe {
            let tid = libc::syscall(libc::SYS_gettid) as libc::id_t;
            libc::setpriority(libc::PRIO_PROCESS, tid, nice)
        };
        if rc != 0 {
            warn!(nice, error = %std::io::Error::last_os_error(), "could not lower the analysis worker's priority");
        }
    }
    #[cfg(not(target_os = "linux"))]
    debug!(nice, "--analysis-nice only applies on Linux");
}

fn clear_dir(dir: &Path) {
    let _ = std::fs::remove_dir_all(dir);
    if let Err(e) = std::fs::create_dir_all(dir) {
        warn!(dir = %dir.display(), error = %e, "analysis temp dir");
    }
}

/// A file name for a track's download: a hash of its id (ids are the
/// server's word) and its suffix, which the decoder probes by.
fn temp_name(track: &Track) -> String {
    use sha2::Digest;
    let hash = hex::encode(&sha2::Sha256::digest(track.id.as_bytes())[..8]);
    let suffix: String = track
        .suffix
        .as_deref()
        .unwrap_or("bin")
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .take(8)
        .collect();
    format!("{hash}.{suffix}")
}

fn fp_key(fp: &SourceFingerprint) -> String {
    format!("{}:{:?}:{:?}", fp.duration_ms, fp.size_bytes, fp.suffix)
}
