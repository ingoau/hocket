//! The opt-in AutoMix analysis worker (`docs/automix.md` → *Coordinator
//! worker*). Off unless `--analysis-server` is given. With its own non-admin
//! Navidrome account it walks the whole library (`search3` with an empty
//! query, page by page), skips every track that already has a usable record
//! with a beat grid (or its own record), and for the rest downloads the original file
//! (`download`) to a temporary file, analyses it, stores the record
//! (produced by `Worker`, fingerprinted from the song's metadata) and
//! deletes the file. One track at a time on one thread at a lowered
//! priority, with a pause between tracks; the library is walked again every
//! `--analysis-rescan-hours`. Progress is logged and shown in `/health`.
//!
//! Tracks over [`WORKER_MAX_DURATION_MS`] or `--analysis-max-file-mb` are
//! left alone (memory: the analyser holds the whole track's spectrogram). A
//! file that can't be decoded or analysed isn't tried again until it
//! changes; a failed download is tried again next pass, and
//! [`TRANSPORT_FAILURES_TO_ABORT`] in a row end the pass (retried after
//! [`RETRY_AFTER_FAILED_WALK`]).
//!
//! Tracks a client asked the store for and missed (`AnalysisFetch`) are
//! pushed onto a bounded [`PriorityQueue`] that the worker drains before
//! each library track and while it waits between passes; it looks their
//! metadata up with `getSong`, so it needn't have walked them yet.
//!
//! Records are keyed by `--verify-url` when it is set (that's what clients'
//! records are keyed by), otherwise by `--analysis-server`.
//!
//! The password comes from `--analysis-password-file` or
//! `HOCKET_ANALYSIS_PASSWORD` and is never logged.

use std::collections::{HashSet, VecDeque};
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
use hocket_core::connect::wire::server_key;
use hocket_core::provider::{NavidromeProvider, Provider, ResolvedStream};
use hocket_core::subsonic::SubsonicApi;

use crate::analysis::{AnalysisStore, PutOutcome};

/// Songs per `search3` page (Navidrome's cap).
pub const PAGE_SIZE: u32 = 500;
/// Wait after a failed library walk before trying again.
const RETRY_AFTER_FAILED_WALK: Duration = Duration::from_secs(600);
/// A progress line at most this often (and after every pass).
const PROGRESS_LOG_INTERVAL: Duration = Duration::from_secs(300);
/// Longest track the worker analyses (~135 MB of spectrogram; uploads may
/// be longer, see [`crate::analysis::MAX_DURATION_MS`]).
pub const WORKER_MAX_DURATION_MS: u32 = 90 * 60_000;
/// Default `--analysis-max-file-mb`.
pub const DEFAULT_MAX_FILE_MB: u64 = 500;
/// Failed downloads (or store writes) in a row that end a pass.
pub const TRANSPORT_FAILURES_TO_ABORT: u32 = 3;
/// Track ids the priority queue holds; past it the oldest are dropped.
pub const PRIORITY_MAX: usize = 256;
/// Longest track id the priority queue takes (ids are the client's word).
pub const PRIORITY_ID_MAX_LEN: usize = 256;
/// The environment variable the password may come from.
pub const PASSWORD_ENV: &str = "HOCKET_ANALYSIS_PASSWORD";

#[derive(clap::Args, Debug, Clone, Default)]
pub struct WorkerArgs {
    /// Run the AutoMix analysis worker against this Navidrome server. Off when
    /// not given. Records are keyed by `--verify-url` when set, otherwise by
    /// this URL, so it must be the one your clients use.
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
    /// Skip files larger than this many MiB.
    #[arg(long, default_value_t = DEFAULT_MAX_FILE_MB)]
    pub analysis_max_file_mb: u64,
}

impl WorkerArgs {
    /// The worker's settings, or `None` when it is off. Reads the password.
    /// With `verify_url` (`--verify-url`) the worker's records are keyed by
    /// it, as every client's are.
    pub fn resolve(
        &self,
        tmp_dir: PathBuf,
        verify_url: Option<&str>,
    ) -> anyhow::Result<Option<WorkerSettings>> {
        let Some(server) = self.analysis_server.clone() else {
            return Ok(None);
        };
        let key_url = verify_url.unwrap_or(&server).to_string();
        if server_key(&key_url) != server_key(&server) {
            warn!(
                "--analysis-server differs from --verify-url: the worker's records are keyed by \
                 --verify-url (what clients' records are keyed by); make sure both name the same server"
            );
        }
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
            max_file_bytes: self.analysis_max_file_mb.saturating_mul(1 << 20),
            key_url,
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
    pub max_file_bytes: u64,
    /// The URL the store keys this server's records by.
    pub key_url: String,
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
    /// Tracks clients asked for that wait in the priority queue.
    pub priority_queue: u64,
}

/// Tracks clients asked for and the store missed, analysed before the rest
/// of the library. Shared by the coordinator's sockets (which push, never
/// waiting on the worker) and the worker (which pops). Only ids are taken
/// from a request, deduplicated, at most [`PRIORITY_MAX`] (oldest dropped),
/// and only for the server the worker analyses.
pub struct PriorityQueue {
    /// The store's key for the worker's server.
    server_key: String,
    inner: Mutex<PriorityInner>,
    wake: tokio::sync::Notify,
}

#[derive(Default)]
struct PriorityInner {
    queue: VecDeque<String>,
    /// Ids the worker could not or will not analyse (bad file, too long or
    /// large, unknown to the server): not queued again (the library walk
    /// retries a bad file when it changes).
    failed: HashSet<String>,
}

impl PriorityQueue {
    /// A queue for the server the store keys by `server_url`.
    pub fn new(server_url: &str) -> PriorityQueue {
        PriorityQueue {
            server_key: server_key(server_url),
            inner: Mutex::new(PriorityInner::default()),
            wake: tokio::sync::Notify::new(),
        }
    }

    /// Queue these ids if `server` (a store key) is the worker's; wakes the
    /// worker. Never blocks beyond a short lock. Returns how many were added.
    pub fn push<'a>(&self, server: &str, ids: impl IntoIterator<Item = &'a str>) -> usize {
        if server != self.server_key {
            return 0;
        }
        let mut added = 0;
        {
            let mut inner = self.inner.lock();
            for id in ids {
                if id.is_empty()
                    || id.len() > PRIORITY_ID_MAX_LEN
                    || inner.failed.contains(id)
                    || inner.queue.iter().any(|q| q == id)
                {
                    continue;
                }
                if inner.queue.len() >= PRIORITY_MAX {
                    inner.queue.pop_front();
                }
                inner.queue.push_back(id.to_string());
                added += 1;
            }
        }
        if added > 0 {
            self.wake.notify_one();
        }
        added
    }

    /// The oldest queued id.
    pub fn pop(&self) -> Option<String> {
        self.inner.lock().queue.pop_front()
    }

    pub fn len(&self) -> usize {
        self.inner.lock().queue.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Put an id back at the head (its lookup failed for want of the
    /// server); dropped if the queue filled up meanwhile.
    fn requeue(&self, id: String) {
        let mut inner = self.inner.lock();
        if inner.queue.len() < PRIORITY_MAX && !inner.queue.contains(&id) {
            inner.queue.push_front(id);
        }
    }

    fn mark_failed(&self, id: &str) {
        let mut inner = self.inner.lock();
        inner.failed.insert(id.to_string());
        inner.queue.retain(|q| q != id);
    }

    /// Resolves once something was pushed since the last time it resolved.
    async fn pushed(&self) {
        self.wake.notified().await
    }
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
    /// The worker's account, behind the provider seam.
    api: Arc<dyn Provider>,
    store: Arc<AnalysisStore>,
    /// The store's key for this server.
    server_key: String,
    pause: Duration,
    tmp_dir: PathBuf,
    status: Arc<Mutex<Option<WorkerStatus>>>,
    analyser: Option<Analyser>,
    max_file_bytes: u64,
    /// Files that could not be decoded or analysed, by (track id,
    /// fingerprint): retried only when the file changes.
    failed: HashSet<(String, String)>,
    busy: Duration,
    last_progress: Instant,
    priority: Arc<PriorityQueue>,
}

/// What became of one track.
enum Outcome {
    Analysed,
    /// Bad file: remembered.
    Failed,
    /// Download or store failed: not remembered.
    TransportFailed(anyhow::Error),
}

impl Worker {
    /// `server_url` is what the store keys records by (see the module docs).
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
            api: Arc::new(NavidromeProvider::new(api)),
            store,
            server_key: server_key(server_url),
            pause,
            tmp_dir,
            status,
            analyser: None,
            max_file_bytes: DEFAULT_MAX_FILE_MB << 20,
            failed: HashSet::new(),
            busy: Duration::ZERO,
            last_progress: Instant::now(),
            priority: Arc::new(PriorityQueue::new(server_url)),
        }
    }

    /// Share this priority queue (the coordinator pushes onto it).
    pub fn with_priority(mut self, priority: Arc<PriorityQueue>) -> Worker {
        self.priority = priority;
        self
    }

    /// The worker's priority queue.
    pub fn priority(&self) -> Arc<PriorityQueue> {
        self.priority.clone()
    }

    /// Skip files larger than this (default [`DEFAULT_MAX_FILE_MB`] MiB).
    pub fn with_max_file_bytes(mut self, bytes: u64) -> Worker {
        self.max_file_bytes = bytes;
        self
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
                .library_page(offset, PAGE_SIZE)
                .await
                .map_err(|e| anyhow::anyhow!("library page at {offset}: {e}"))?;
            out.extend(page.tracks.into_iter().map(|mut t| {
                // The store keys records by server, not by account.
                t.server_id = String::new();
                t
            }));
            match page.next_offset {
                Some(next) => offset = next,
                None => return Ok(out),
            }
        }
    }

    /// Whether the worker would ever analyse this track (length and size).
    fn analysable(&self, t: &Track) -> bool {
        t.duration_ms > 0
            && t.duration_ms <= WORKER_MAX_DURATION_MS
            && !t.size_bytes.is_some_and(|b| b > self.max_file_bytes as f64)
    }

    fn wanted(&self, t: &Track) -> bool {
        if !self.analysable(t) {
            return false;
        }
        let fp = fingerprint_of(t);
        if self.failed.contains(&(t.id.clone(), fp_key(&fp))) {
            return false;
        }
        match self.store.covers(&self.server_key, &t.id, &fp) {
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
        self.analyse_library(&library, cancel).await
    }

    /// The analysing part of a [`Worker::pass`], over a walked `library`.
    pub async fn analyse_library(
        &mut self,
        library: &[Track],
        cancel: &CancellationToken,
    ) -> anyhow::Result<PassStats> {
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
        let mut transport_failures = 0;
        for (i, track) in todo.iter().enumerate() {
            if cancel.is_cancelled() {
                break;
            }
            // What clients are waiting for comes first.
            self.drain_priority(cancel).await;
            if cancel.is_cancelled() {
                break;
            }
            let remaining = todo.len() - i - 1;
            // Analysed meanwhile (asked for by a client, or uploaded).
            if !self.wanted(track) {
                self.update(|s| s.remaining = remaining as u64);
                continue;
            }
            match self.analyse_counted(track).await {
                Outcome::Analysed => {
                    transport_failures = 0;
                    stats.analysed += 1;
                }
                Outcome::Failed => {
                    transport_failures = 0;
                    stats.failed += 1;
                }
                Outcome::TransportFailed(e) => {
                    transport_failures += 1;
                    stats.failed += 1;
                    if transport_failures >= TRANSPORT_FAILURES_TO_ABORT {
                        anyhow::bail!("{transport_failures} downloads in a row failed; last: {e}");
                    }
                }
            }
            self.update(|s| s.remaining = remaining as u64);
            let per_hour = self.per_hour();
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
        let cancelled = cancel.is_cancelled();
        if !cancelled {
            self.update(|s| s.passes += 1);
        }
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
            "{}",
            if cancelled {
                "analysis pass stopped"
            } else {
                "analysis pass finished"
            }
        );
        Ok(stats)
    }

    fn per_hour(&self) -> f64 {
        self.status.lock().as_ref().map_or(0, |s| s.analysed) as f64
            / self.busy.as_secs_f64().max(1.0)
            * 3600.0
    }

    /// [`Worker::analyse_one`], counted in the status, logged, and a bad
    /// file remembered.
    async fn analyse_counted(&mut self, track: &Track) -> Outcome {
        self.set_state("analysing");
        let started = Instant::now();
        let outcome = match self.analyse_one(track).await {
            Ok(()) => {
                self.update(|s| s.analysed += 1);
                Outcome::Analysed
            }
            Err(OneError::File(e)) => {
                self.failed
                    .insert((track.id.clone(), fp_key(&fingerprint_of(track))));
                self.priority.mark_failed(&track.id);
                self.update(|s| s.failed += 1);
                warn!(track = %track.id, title = %track.title, error = %e, "analysis failed");
                Outcome::Failed
            }
            Err(OneError::Transport(e)) => {
                // Not remembered: tried again next pass.
                self.update(|s| s.failed += 1);
                warn!(track = %track.id, title = %track.title, error = %e, "analysis download failed");
                Outcome::TransportFailed(e)
            }
        };
        self.busy += started.elapsed();
        let per_hour = self.per_hour();
        self.update(|s| s.per_hour = (per_hour * 10.0).round() / 10.0);
        outcome
    }

    /// Analyse what clients asked for and missed, oldest first, until the
    /// queue is empty, a lookup or download fails for want of the server, or
    /// `cancel`. Each id's metadata comes from `getSong`. Ids the server
    /// doesn't know and tracks the worker never analyses (too long, too
    /// large) are dropped and not queued again; tracks covered meanwhile or
    /// failed before are dropped. Pauses `--analysis-pause-secs` after each
    /// track analysed. Returns how many were analysed.
    pub async fn drain_priority(&mut self, cancel: &CancellationToken) -> usize {
        use hocket_core::subsonic::SubsonicError;
        let mut analysed = 0;
        while !cancel.is_cancelled() {
            let Some(id) = self.priority.pop() else { break };
            let song = match self.api.track(&id).await {
                Ok(mut song) => {
                    song.server_id = String::new();
                    song
                }
                Err(
                    e @ (SubsonicError::NotFound(_)
                    | SubsonicError::Forbidden(_)
                    | SubsonicError::Protocol(_)),
                ) => {
                    debug!(track = %id, error = %e, "priority track unknown");
                    self.priority.mark_failed(&id);
                    continue;
                }
                Err(e) => {
                    // The server is in trouble: leave the queue for later.
                    debug!(track = %id, error = %e, "priority track lookup failed");
                    self.priority.requeue(id);
                    break;
                }
            };
            let track = song;
            if !self.analysable(&track) {
                self.priority.mark_failed(&track.id);
                continue;
            }
            if !self.wanted(&track) {
                continue;
            }
            info!(track = %track.id, title = %track.title, "analysing a track a client asked for");
            match self.analyse_counted(&track).await {
                Outcome::Analysed => analysed += 1,
                Outcome::Failed => {}
                // The server is likely in trouble: leave the rest for later.
                Outcome::TransportFailed(_) => break,
            }
            // Clients don't get to skip the pause, before the next queued
            // track or the walk's next one.
            if !self.pause.is_zero() {
                self.set_state("waiting");
                tokio::select! {
                    _ = tokio::time::sleep(self.pause) => {}
                    _ = cancel.cancelled() => break,
                }
            }
        }
        analysed
    }

    /// Wait `wait` (or until `cancel`), analysing what clients ask for
    /// meanwhile.
    async fn idle(&mut self, wait: Duration, cancel: &CancellationToken) {
        let deadline = tokio::time::Instant::now() + wait;
        let priority = self.priority.clone();
        loop {
            tokio::select! {
                _ = tokio::time::sleep_until(deadline) => return,
                _ = cancel.cancelled() => return,
                _ = priority.pushed() => {}
            }
            let state = self.status.lock().as_ref().map(|s| s.state.clone());
            self.drain_priority(cancel).await;
            if let Some(state) = state {
                self.set_state(&state);
            }
        }
    }

    /// Download, analyse, store, delete.
    async fn analyse_one(&mut self, track: &Track) -> Result<(), OneError> {
        let path = self.tmp_dir.join(temp_name(track));
        let result = self.download_and_analyse(track, &path).await;
        let _ = tokio::fs::remove_file(&path).await;
        let record = result?;
        let outcome = self
            .store
            .put(
                &self.server_key,
                &track.id,
                record,
                hocket_core::util::now_ms() as u64,
            )
            .map_err(|e| OneError::Transport(anyhow::anyhow!("store: {e}")))?;
        debug!(track = %track.id, stored = outcome == PutOutcome::Stored, "analysed");
        Ok(())
    }

    async fn download_and_analyse(
        &mut self,
        track: &Track,
        path: &Path,
    ) -> Result<hocket_core::api::AnalysisRecord, OneError> {
        let url = self.api.download_url(&track.id).ok_or_else(|| {
            OneError::Transport(anyhow::anyhow!("download: the account offers no files"))
        })?;
        let stream = ResolvedStream {
            url,
            headers: Default::default(),
            expires_at: None,
            container: track.suffix.clone(),
            codec: None,
            gain_db: None,
            range_capable: true,
            transcoded: false,
        };
        self.api
            .download_to_file(&stream, path)
            .await
            .map_err(|e| OneError::Transport(anyhow::anyhow!("download: {e}")))?;
        // The song's size was checked before; the server may have lied.
        let len = tokio::fs::metadata(path)
            .await
            .map_err(|e| OneError::Transport(anyhow::anyhow!("download: {e}")))?
            .len();
        if len > self.max_file_bytes {
            return Err(OneError::File(anyhow::anyhow!(
                "file of {len} bytes is too large"
            )));
        }
        if self.analyser.is_none() {
            // Not the file's fault: the pass ends and is tried again later.
            self.analyser = Some(
                Analyser::new(1)
                    .map_err(|e| OneError::Transport(anyhow::anyhow!("analyser: {e}")))?,
            );
        }
        let analyser = self.analyser.as_mut().expect("just set");
        // Blocking on purpose: the worker's runtime is its own, on its own
        // (niced) thread, and does nothing else meanwhile.
        let mut record = analyser
            .analyse_file(path, AnalysisProducer::Worker)
            .map_err(|e| OneError::File(anyhow::anyhow!("analyse: {e}")))?;
        record.source = fingerprint_of(track);
        Ok(compact(record))
    }

    /// Passes until cancelled: a pass, then a wait of `rescan` (or a retry
    /// after a failed walk or a pass ended by failed downloads).
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
                    warn!(error = %e, "analysis pass failed; retrying later");
                    RETRY_AFTER_FAILED_WALK
                }
            };
            self.idle(wait, &cancel).await;
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
    priority: Arc<PriorityQueue>,
    cancel: CancellationToken,
) -> anyhow::Result<std::thread::JoinHandle<()>> {
    let api = settings.client()?;
    info!(
        server = %settings.server,
        key = %server_key(&settings.key_url),
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
                &settings.key_url,
                settings.pause,
                settings.tmp_dir.clone(),
                status,
            )
            .with_max_file_bytes(settings.max_file_bytes)
            .with_priority(priority);
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

/// Why one track was not analysed.
enum OneError {
    /// The file can't be decoded or analysed (or is too large): not tried
    /// again until it changes.
    File(anyhow::Error),
    /// The download or the store failed: tried again next pass.
    Transport(anyhow::Error),
}

fn fp_key(fp: &SourceFingerprint) -> String {
    format!("{}:{:?}:{:?}", fp.duration_ms, fp.size_bytes, fp.suffix)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_are_keyed_by_the_verify_url_when_set() {
        let dir = tempfile::tempdir().unwrap();
        let pw = dir.path().join("pw");
        std::fs::write(&pw, "secret\n").unwrap();
        let args = WorkerArgs {
            analysis_server: Some("http://navidrome:4533".into()),
            analysis_user: Some("worker".into()),
            analysis_password_file: Some(pw),
            analysis_max_file_mb: 10,
            ..Default::default()
        };
        let tmp = dir.path().join("tmp");
        let s = args.resolve(tmp.clone(), None).unwrap().unwrap();
        assert_eq!(s.key_url, "http://navidrome:4533");
        assert_eq!(s.max_file_bytes, 10 << 20);
        let s = args
            .resolve(tmp, Some("https://music.example"))
            .unwrap()
            .unwrap();
        assert_eq!(s.key_url, "https://music.example");
        assert_eq!(s.server, "http://navidrome:4533", "still fetched from here");
    }
}

#[cfg(test)]
mod priority_tests {
    use super::*;

    #[test]
    fn the_priority_queue_is_bounded_deduplicated_and_for_one_server() {
        let q = PriorityQueue::new("https://music.example");
        let key = server_key("https://music.example");
        assert_eq!(q.push("another", ["a"]), 0);
        assert_eq!(
            q.push(&key, ["a", "a", "", &"x".repeat(PRIORITY_ID_MAX_LEN + 1)]),
            1
        );
        q.mark_failed("a");
        assert!(q.is_empty(), "a failed track leaves the queue");
        assert_eq!(q.push(&key, ["a"]), 0, "and isn't queued again");
        let ids: Vec<String> = (0..PRIORITY_MAX + 10).map(|i| i.to_string()).collect();
        q.push(&key, ids.iter().map(String::as_str));
        assert_eq!(q.len(), PRIORITY_MAX);
        assert_eq!(q.pop().as_deref(), Some("10"), "the oldest were dropped");
    }
}
