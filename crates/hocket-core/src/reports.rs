//! Save to Coordinator, the client side (`docs/reports.md`): the leave a
//! hosted coordinator grants with its diagnostics answer, the upload's
//! staging directory and `job.json`, and the HTTP upload itself (create the
//! report, then send every file in [`CHUNK_BYTES`] pieces, resuming from the
//! offsets the coordinator holds).
//!
//! Nothing here touches the actor: `core/handlers/reports.rs` decides when to
//! upload and feeds [`upload`]'s events back into the core. Nothing here logs
//! the note, the grant or the upload key.

use std::io::SeekFrom;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncReadExt, AsyncSeekExt};

use crate::api::{
    ConnectionState, ConnectionTier, EpochMs, ReportAttachment, ReportAttachmentKind,
    ReportUploadError, ReportUploadState, ReportUploadStatus,
};
use crate::connect::diagnostics::{ReplyPart, COORDINATOR};

/// Bytes per `PUT` (the coordinator takes up to 8 MiB).
pub const CHUNK_BYTES: u64 = 4 << 20;
pub const MAX_SCREENSHOTS: usize = 5;
pub const MAX_RECORDINGS: usize = 1;
pub const MAX_BUNDLE_BYTES: u64 = 16 << 20;
pub const MAX_SCREENSHOT_BYTES: u64 = 25 << 20;
pub const MAX_RECORDING_BYTES: u64 = 1 << 30;
pub const MAX_NOTE_CHARS: usize = 8000;
/// `client` and `device`.
pub const MAX_LABEL_CHARS: usize = 200;
/// The staging directory, under the core's data directory.
pub const DIR: &str = "report-upload";
pub const JOB_FILE: &str = "job.json";
pub const BUNDLE_FILE: &str = "bundle.txt";
/// Waits between automatic retries of a transient failure (network, 5xx);
/// after the last one the upload fails with what went wrong.
pub const BACKOFF: [Duration; 5] = [
    Duration::from_secs(2),
    Duration::from_secs(4),
    Duration::from_secs(8),
    Duration::from_secs(16),
    Duration::from_secs(30),
];
/// Progress goes out at most this often.
pub const PROGRESS_INTERVAL: Duration = Duration::from_millis(250);
/// A grant is treated as spent this long before the coordinator's expiry, so
/// an upload never starts on one about to run out.
const GRANT_MARGIN_MS: f64 = 30_000.0;
/// Pieces of a chunk as they go into the request body (progress granularity).
const PIECE_BYTES: usize = 64 << 10;
const JOB_VERSION: u32 = 1;

// ---------------------------------------------------------------------------
// The grant
// ---------------------------------------------------------------------------

/// Leave to create reports, as the core holds it.
#[derive(Clone, PartialEq)]
pub struct HeldGrant {
    pub token: String,
    /// Epoch ms after which it is not used (a little before the
    /// coordinator's own expiry).
    pub expires_at: EpochMs,
    /// The coordinator URL this device used when it came.
    pub coordinator_url: String,
}

impl std::fmt::Debug for HeldGrant {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HeldGrant")
            .field("token", &"<redacted>")
            .field("expires_at", &self.expires_at)
            .field("coordinator_url", &self.coordinator_url)
            .finish()
    }
}

impl HeldGrant {
    pub fn valid(&self, now: EpochMs) -> bool {
        !self.token.is_empty() && now < self.expires_at
    }
}

/// What a diagnostics answer part does to the held grant.
#[derive(Debug, Clone, PartialEq)]
pub enum GrantUpdate {
    /// Not the coordinator's first part (or not over the coordinator tier):
    /// nothing to say about the grant.
    Ignore,
    /// The coordinator answered without one: this account may not (or no
    /// longer) save reports there.
    Clear,
    Hold(HeldGrant),
}

/// The grant rule: only part 0 of the [`COORDINATOR`] source counts, and only
/// while this device is on the coordinator tier (the URL is recorded with it).
pub fn grant_update(part: &ReplyPart, conn: &ConnectionState, now: EpochMs) -> GrantUpdate {
    if part.from != COORDINATOR || part.part != 0 {
        return GrantUpdate::Ignore;
    }
    let Some(url) = conn
        .coordinator_url
        .as_ref()
        .filter(|_| conn.tier == ConnectionTier::Coordinator)
    else {
        return GrantUpdate::Ignore;
    };
    match &part.report_grant {
        Some(g) if !g.token.is_empty() && g.expires_in_ms > 0 => GrantUpdate::Hold(HeldGrant {
            token: g.token.clone(),
            expires_at: now + g.expires_in_ms as f64 - GRANT_MARGIN_MS,
            coordinator_url: url.clone(),
        }),
        _ => GrantUpdate::Clear,
    }
}

/// `RoomDiagnosticsStatus::report_upload`: a good grant, and this device is
/// connected to the coordinator it came from.
pub fn grant_usable(grant: Option<&HeldGrant>, conn: &ConnectionState, now: EpochMs) -> bool {
    grant.is_some_and(|g| {
        g.valid(now)
            && conn.tier == ConnectionTier::Coordinator
            && conn.connected
            && conn.coordinator_url.as_deref() == Some(g.coordinator_url.as_str())
    })
}

/// The HTTP base for a coordinator URL: `wss://` → `https://` (`ws://` →
/// `http://`), a trailing `/ws` segment removed, no query or fragment and no
/// trailing slash. `None` for anything else.
pub fn base_url(coordinator_url: &str) -> Option<String> {
    let mut u = url::Url::parse(coordinator_url.trim()).ok()?;
    let scheme = match u.scheme() {
        "wss" | "https" => "https",
        "ws" | "http" => "http",
        _ => return None,
    };
    u.host_str()?;
    u.set_scheme(scheme).ok()?;
    let path = u.path().trim_end_matches('/');
    let path = path.strip_suffix("/ws").unwrap_or(path).to_string();
    u.set_path(&path);
    u.set_query(None);
    u.set_fragment(None);
    Some(u.as_str().trim_end_matches('/').to_string())
}

// ---------------------------------------------------------------------------
// The job
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum FileKind {
    Bundle,
    Screenshot,
    Recording,
}

impl From<ReportAttachmentKind> for FileKind {
    fn from(k: ReportAttachmentKind) -> Self {
        match k {
            ReportAttachmentKind::Screenshot => FileKind::Screenshot,
            ReportAttachmentKind::Recording => FileKind::Recording,
        }
    }
}

impl FileKind {
    pub fn max_bytes(self) -> u64 {
        match self {
            FileKind::Bundle => MAX_BUNDLE_BYTES,
            FileKind::Screenshot => MAX_SCREENSHOT_BYTES,
            FileKind::Recording => MAX_RECORDING_BYTES,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct JobFile {
    pub kind: FileKind,
    pub path: String,
    /// The file's own name, as the coordinator is told it.
    pub name: String,
    pub size: u64,
    /// What the coordinator holds of it (as last heard).
    #[serde(default)]
    pub received: u64,
    /// The platform's copy: deleted once the upload is done or dismissed.
    #[serde(default)]
    pub delete_after: bool,
}

/// Everything needed to carry an upload on, across restarts (`job.json`).
#[derive(Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Job {
    pub version: u32,
    pub base_url: String,
    pub note: String,
    pub client: String,
    pub device: String,
    pub started_at: EpochMs,
    pub state: ReportUploadState,
    #[serde(default)]
    pub report_id: Option<String>,
    #[serde(default)]
    pub upload_key: Option<String>,
    #[serde(default)]
    pub error: Option<ReportUploadError>,
    #[serde(default)]
    pub error_detail: Option<String>,
    /// The bundle first, then the attachments in the platform's order.
    pub files: Vec<JobFile>,
}

impl std::fmt::Debug for Job {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Job")
            .field("base_url", &self.base_url)
            .field(
                "note",
                &format_args!("<{} chars>", self.note.chars().count()),
            )
            .field("state", &self.state)
            .field("report_id", &self.report_id)
            .field(
                "upload_key",
                &self.upload_key.as_ref().map(|_| "<redacted>"),
            )
            .field("error", &self.error)
            .field("files", &self.files)
            .finish()
    }
}

impl Job {
    pub fn total_bytes(&self) -> u64 {
        self.files.iter().map(|f| f.size).sum()
    }

    pub fn sent_bytes(&self) -> u64 {
        self.files.iter().map(|f| f.received.min(f.size)).sum()
    }

    pub fn status(&self) -> ReportUploadStatus {
        ReportUploadStatus {
            state: self.state,
            sent_bytes: self.sent_bytes() as f64,
            total_bytes: self.total_bytes() as f64,
            report_id: self.report_id.clone(),
            error: self.error,
            error_detail: self.error_detail.clone(),
            started_at: self.started_at,
        }
    }

    /// The platform's copies (`delete_after`), to delete when the upload ends.
    pub fn platform_copies(&self) -> impl Iterator<Item = &str> {
        self.files
            .iter()
            .filter(|f| f.delete_after && f.kind != FileKind::Bundle)
            .map(|f| f.path.as_str())
    }
}

/// Write `job.json` atomically (a temporary file renamed over it). The
/// directory must exist: a dismissed upload's late write fails quietly.
pub fn save_job(dir: &Path, job: &Job) -> std::io::Result<()> {
    let tmp = dir.join(format!("{JOB_FILE}.tmp"));
    let json = serde_json::to_vec_pretty(job).map_err(std::io::Error::other)?;
    {
        use std::io::Write;
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(&json)?;
        f.sync_all()?;
    }
    std::fs::rename(&tmp, dir.join(JOB_FILE))
}

/// The staged job, if there is one. A file that does not parse is an error.
pub fn load_job(dir: &Path) -> std::io::Result<Option<Job>> {
    match std::fs::read(dir.join(JOB_FILE)) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .map(Some)
            .map_err(std::io::Error::other),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

/// Delete the platform's copies and the staging directory.
pub fn clean_up(dir: &Path, job: Option<&Job>) {
    if let Some(job) = job {
        for path in job.platform_copies() {
            remove_file(path);
        }
    }
    if let Err(e) = std::fs::remove_dir_all(dir) {
        if e.kind() != std::io::ErrorKind::NotFound {
            tracing::warn!(target: "hocket::reports", error = %e, "removing the report staging directory");
        }
    }
}

/// Delete a platform's copy, logging (not failing) when that does not work.
pub fn remove_file(path: &str) {
    if let Err(e) = std::fs::remove_file(path) {
        if e.kind() != std::io::ErrorKind::NotFound {
            tracing::warn!(target: "hocket::reports", error = %e, "deleting an attachment copy");
        }
    }
}

/// Why an upload stopped (or never started).
#[derive(Debug, Clone, PartialEq)]
pub struct Failure {
    pub error: ReportUploadError,
    pub detail: String,
    /// Worth trying again on its own (network trouble, 5xx).
    pub transient: bool,
}

impl Failure {
    pub fn new(error: ReportUploadError, detail: impl Into<String>) -> Self {
        Failure {
            error,
            detail: detail.into(),
            transient: false,
        }
    }

    fn network(detail: impl Into<String>) -> Self {
        Failure {
            error: ReportUploadError::Network,
            detail: detail.into(),
            transient: true,
        }
    }
}

/// A new report, as the platform sent it.
pub struct NewReport<'a> {
    pub note: &'a str,
    pub bundle: &'a str,
    pub attachments: &'a [ReportAttachment],
    pub base_url: String,
    pub client: String,
    pub device: String,
    pub started_at: EpochMs,
}

/// Check the attachments against the coordinator's limits (count, size,
/// readable, not empty) and their files as they are now.
pub fn validate(note: &str, attachments: &[ReportAttachment]) -> Result<Vec<JobFile>, Failure> {
    use ReportUploadError::{TooLarge, Unreadable};
    if note.chars().count() > MAX_NOTE_CHARS {
        return Err(Failure::new(
            TooLarge,
            format!("the note is over {MAX_NOTE_CHARS} characters"),
        ));
    }
    let count = |k| attachments.iter().filter(|a| a.kind == k).count();
    if count(ReportAttachmentKind::Screenshot) > MAX_SCREENSHOTS {
        return Err(Failure::new(
            Unreadable,
            format!("more than {MAX_SCREENSHOTS} screenshots"),
        ));
    }
    if count(ReportAttachmentKind::Recording) > MAX_RECORDINGS {
        return Err(Failure::new(
            Unreadable,
            format!("more than {MAX_RECORDINGS} recording"),
        ));
    }
    let mut files = vec![];
    for a in attachments {
        let kind = FileKind::from(a.kind);
        let path = Path::new(&a.path);
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .filter(|n| !n.is_empty())
            .ok_or_else(|| Failure::new(Unreadable, format!("{}: no file name", a.path)))?;
        let unreadable = |e: std::io::Error| Failure::new(Unreadable, format!("{name}: {e}"));
        let meta = std::fs::metadata(path).map_err(unreadable)?;
        if !meta.is_file() {
            return Err(Failure::new(Unreadable, format!("{name}: not a file")));
        }
        std::fs::File::open(path).map_err(unreadable)?;
        if meta.len() == 0 {
            return Err(Failure::new(Unreadable, format!("{name}: empty")));
        }
        if meta.len() > kind.max_bytes() {
            return Err(Failure::new(
                TooLarge,
                format!(
                    "{name}: {} bytes, over the {} bytes a {kind:?} may have",
                    meta.len(),
                    kind.max_bytes()
                ),
            ));
        }
        files.push(JobFile {
            kind,
            path: a.path.clone(),
            name: truncate(&name, MAX_LABEL_CHARS),
            size: meta.len(),
            received: 0,
            delete_after: a.delete_after,
        });
    }
    Ok(files)
}

/// Validate and stage a new report into `dir` (created; whatever was there is
/// replaced): `bundle.txt` and `job.json`, state `Uploading`.
pub fn stage(dir: &Path, r: NewReport<'_>) -> Result<Job, Failure> {
    let attachments = validate(r.note, r.attachments)?;
    let storage = |e: std::io::Error| {
        Failure::new(
            ReportUploadError::Unreadable,
            format!("staging the report: {e}"),
        )
    };
    let _ = std::fs::remove_dir_all(dir);
    std::fs::create_dir_all(dir).map_err(storage)?;
    let bundle = if r.bundle.is_empty() {
        "(the app sent no diagnostics bundle)\n".to_string()
    } else {
        crate::connect::diagnostics::cap(r.bundle, MAX_BUNDLE_BYTES as usize)
    };
    let bundle_path = dir.join(BUNDLE_FILE);
    std::fs::write(&bundle_path, bundle.as_bytes()).map_err(storage)?;
    let mut files = vec![JobFile {
        kind: FileKind::Bundle,
        path: bundle_path.to_string_lossy().into_owned(),
        name: bundle_name(r.started_at),
        size: bundle.len() as u64,
        received: 0,
        delete_after: false,
    }];
    files.extend(attachments);
    let job = Job {
        version: JOB_VERSION,
        base_url: r.base_url,
        note: r.note.to_string(),
        client: truncate(&r.client, MAX_LABEL_CHARS),
        device: truncate(&r.device, MAX_LABEL_CHARS),
        started_at: r.started_at,
        state: ReportUploadState::Uploading,
        report_id: None,
        upload_key: None,
        error: None,
        error_detail: None,
        files,
    };
    save_job(dir, &job).map_err(storage)?;
    Ok(job)
}

/// `hocket-diagnostics-20261101-1403.txt` (UTC).
pub fn bundle_name(at: EpochMs) -> String {
    let ts = crate::devlog::format_timestamp(at.max(0.0) as u64);
    // 2026-11-01T14:03:05.678Z
    let digits = |r: std::ops::Range<usize>| ts.get(r).unwrap_or("").replace(['-', ':'], "");
    format!(
        "hocket-diagnostics-{}-{}.txt",
        digits(0..10),
        digits(11..16)
    )
}

fn truncate(s: &str, max: usize) -> String {
    s.chars().take(max).collect()
}

/// The staging directory under a data directory.
pub fn staging_dir(data_dir: &Path) -> PathBuf {
    data_dir.join(DIR)
}

// ---------------------------------------------------------------------------
// HTTP
// ---------------------------------------------------------------------------

/// The upload's HTTP client: rustls, no redirects, 15 s to connect and five
/// minutes per request (4 MiB on a slow mobile link).
pub fn http_client(user_agent: &str) -> reqwest::Result<reqwest::Client> {
    reqwest::Client::builder()
        .user_agent(user_agent)
        .connect_timeout(Duration::from_secs(15))
        .timeout(Duration::from_secs(300))
        .redirect(reqwest::redirect::Policy::none())
        .use_rustls_tls()
        .build()
}

/// What [`upload`] reports along the way.
#[derive(Debug, Clone, PartialEq)]
pub enum UploadEvent {
    /// The coordinator made the report (`job.json` already has its id and
    /// key).
    Created { report_id: String },
    /// Bytes sent so far, all files together (at most every
    /// [`PROGRESS_INTERVAL`]).
    Sent { sent: u64 },
}

pub type OnEvent = Arc<dyn Fn(UploadEvent) + Send + Sync>;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CreateBody<'a> {
    note: &'a str,
    client: &'a str,
    device: &'a str,
    files: Vec<CreateFile<'a>>,
}

#[derive(Serialize)]
struct CreateFile<'a> {
    kind: FileKind,
    size: u64,
    name: &'a str,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Created {
    id: String,
    upload_key: String,
    #[serde(default)]
    files: Vec<FileState>,
}

#[derive(Deserialize)]
struct FileState {
    index: usize,
    #[serde(default)]
    received: u64,
}

#[derive(Deserialize)]
struct ReportState {
    #[serde(default)]
    files: Vec<FileState>,
    #[serde(default)]
    complete: bool,
}

#[derive(Deserialize)]
struct PutAnswer {
    received: u64,
    #[serde(default)]
    complete: bool,
}

#[derive(Deserialize, Default)]
struct ErrorBody {
    #[serde(default)]
    error: String,
    #[serde(default)]
    detail: String,
    #[serde(default)]
    received: Option<u64>,
}

/// Sends progress at most every [`PROGRESS_INTERVAL`].
#[derive(Clone)]
struct Throttle {
    on: OnEvent,
    last: Arc<Mutex<Option<tokio::time::Instant>>>,
}

impl Throttle {
    fn sent(&self, sent: u64, force: bool) {
        let now = tokio::time::Instant::now();
        let mut last = self.last.lock();
        if force || last.is_none_or(|l| now.duration_since(l) >= PROGRESS_INTERVAL) {
            *last = Some(now);
            drop(last);
            (self.on)(UploadEvent::Sent { sent });
        }
    }
}

/// Upload `job` from wherever the coordinator has it: create the report if
/// it has no id yet (needs `grant`), otherwise ask how far each file got;
/// then send what is missing. `job.json` in `dir` is rewritten as soon as the
/// report exists. Transient failures are tried again after each wait in
/// `backoff` (the count starts over whenever a chunk goes through).
pub async fn upload(
    client: &reqwest::Client,
    dir: &Path,
    job: &mut Job,
    grant: Option<&str>,
    backoff: &[Duration],
    on: OnEvent,
) -> Result<(), Failure> {
    let throttle = Throttle {
        on,
        last: Arc::default(),
    };
    let mut attempt = 0;
    loop {
        match attempt_once(client, dir, job, grant, &throttle, &mut attempt).await {
            Ok(()) => return Ok(()),
            Err(f) if f.transient && attempt < backoff.len() => {
                tracing::info!(
                    target: "hocket::reports",
                    error = %f.detail,
                    retry_in_s = backoff[attempt].as_secs_f64(),
                    "report upload interrupted; trying again"
                );
                tokio::time::sleep(backoff[attempt]).await;
                attempt += 1;
            }
            Err(f) => return Err(f),
        }
    }
}

async fn attempt_once(
    client: &reqwest::Client,
    dir: &Path,
    job: &mut Job,
    grant: Option<&str>,
    throttle: &Throttle,
    attempt: &mut usize,
) -> Result<(), Failure> {
    let mut complete = false;
    if job.report_id.is_some() {
        match fetch_state(client, job).await? {
            Some(state) => {
                for f in state.files {
                    if let Some(jf) = job.files.get_mut(f.index) {
                        jf.received = f.received.min(jf.size);
                    }
                }
                complete = state.complete;
            }
            None => {
                // Gone (unfinished reports are dropped after 48 hours): make it again.
                tracing::info!(target: "hocket::reports", "the coordinator no longer has the report; creating it again");
                job.report_id = None;
                job.upload_key = None;
                for f in &mut job.files {
                    f.received = 0;
                }
            }
        }
    }
    if job.report_id.is_none() {
        create(client, dir, job, grant, throttle).await?;
    }
    throttle.sent(job.sent_bytes(), true);
    for index in 0..job.files.len() {
        if job.files[index].received >= job.files[index].size {
            continue;
        }
        let path = job.files[index].path.clone();
        let name = job.files[index].name.clone();
        let unreadable =
            |e: std::io::Error| Failure::new(ReportUploadError::Unreadable, format!("{name}: {e}"));
        let mut file = tokio::fs::File::open(&path).await.map_err(unreadable)?;
        while job.files[index].received < job.files[index].size {
            let offset = job.files[index].received;
            let len = (job.files[index].size - offset).min(CHUNK_BYTES) as usize;
            file.seek(SeekFrom::Start(offset))
                .await
                .map_err(unreadable)?;
            let mut buf = vec![0u8; len];
            file.read_exact(&mut buf).await.map_err(|e| {
                Failure::new(
                    ReportUploadError::Unreadable,
                    format!("{name}: changed while it was uploading ({e})"),
                )
            })?;
            let (received, done) = put_chunk(client, job, index, offset, buf, throttle).await?;
            if received > job.files[index].size {
                return Err(Failure::new(
                    ReportUploadError::Server,
                    format!("the coordinator holds {received} bytes of {name}, more than it has"),
                ));
            }
            job.files[index].received = received;
            complete = done;
            *attempt = 0;
            throttle.sent(job.sent_bytes(), false);
        }
    }
    throttle.sent(job.sent_bytes(), true);
    if complete {
        return Ok(());
    }
    // Every byte is there by our count; the coordinator's word decides.
    match fetch_state(client, job).await? {
        Some(s) if s.complete => Ok(()),
        _ => Err(Failure::new(
            ReportUploadError::Server,
            "every file went up but the coordinator does not have the report complete",
        )),
    }
}

async fn create(
    client: &reqwest::Client,
    dir: &Path,
    job: &mut Job,
    grant: Option<&str>,
    throttle: &Throttle,
) -> Result<(), Failure> {
    let Some(grant) = grant else {
        return Err(Failure::new(
            ReportUploadError::NotAllowed,
            "no leave from the coordinator to make the report: open diagnostics again while connected to it",
        ));
    };
    let body = CreateBody {
        note: &job.note,
        client: &job.client,
        device: &job.device,
        files: job
            .files
            .iter()
            .map(|f| CreateFile {
                kind: f.kind,
                size: f.size,
                name: &f.name,
            })
            .collect(),
    };
    let resp = client
        .post(format!("{}/reports", job.base_url))
        .bearer_auth(grant)
        .json(&body)
        .send()
        .await
        .map_err(network)?;
    let status = resp.status();
    if status != reqwest::StatusCode::CREATED && status != reqwest::StatusCode::OK {
        return Err(failure(status, resp).await);
    }
    let created: Created = resp.json().await.map_err(|e| {
        Failure::new(
            ReportUploadError::Server,
            format!("unreadable answer to POST /reports: {}", e.without_url()),
        )
    })?;
    job.report_id = Some(created.id.clone());
    job.upload_key = Some(created.upload_key);
    for f in created.files {
        if let Some(jf) = job.files.get_mut(f.index) {
            jf.received = f.received.min(jf.size);
        }
    }
    if let Err(e) = save_job(dir, job) {
        tracing::warn!(target: "hocket::reports", error = %e, "saving the report upload's job.json");
    }
    tracing::info!(target: "hocket::reports", report_id = %created.id, "the coordinator made the report");
    (throttle.on)(UploadEvent::Created {
        report_id: created.id,
    });
    Ok(())
}

/// Tell the coordinator an unfinished report was given up on
/// (`DELETE /reports/{id}`), so it stops holding room for it. Best effort:
/// a coordinator that misses it deletes the report once it sits idle.
pub async fn abandon(client: &reqwest::Client, job: &Job) {
    if job.state == ReportUploadState::Done {
        return;
    }
    let Ok((id, key)) = ids(job) else {
        return;
    };
    match client
        .delete(format!("{}/reports/{id}", job.base_url))
        .bearer_auth(key)
        .timeout(std::time::Duration::from_secs(20))
        .send()
        .await
    {
        Ok(r) => {
            tracing::debug!(target: "hocket::reports", status = r.status().as_u16(), "abandoned report deleted")
        }
        Err(e) => {
            tracing::debug!(target: "hocket::reports", error = %e.without_url(), "could not delete the abandoned report")
        }
    }
}

/// `GET /reports/{id}`; `None` when the coordinator does not have it.
async fn fetch_state(client: &reqwest::Client, job: &Job) -> Result<Option<ReportState>, Failure> {
    let (id, key) = ids(job)?;
    let resp = client
        .get(format!("{}/reports/{id}", job.base_url))
        .bearer_auth(key)
        .send()
        .await
        .map_err(network)?;
    let status = resp.status();
    if status == reqwest::StatusCode::NOT_FOUND {
        return Ok(None);
    }
    if !status.is_success() {
        return Err(failure(status, resp).await);
    }
    resp.json().await.map(Some).map_err(|e| {
        Failure::new(
            ReportUploadError::Server,
            format!(
                "unreadable answer to GET /reports/{id}: {}",
                e.without_url()
            ),
        )
    })
}

/// One `PUT`: what the coordinator now holds of the file, and whether the
/// whole report is in. A 409 answers with the coordinator's own offset.
async fn put_chunk(
    client: &reqwest::Client,
    job: &Job,
    index: usize,
    offset: u64,
    buf: Vec<u8>,
    throttle: &Throttle,
) -> Result<(u64, bool), Failure> {
    let (id, key) = ids(job)?;
    let len = buf.len();
    let before = job.sent_bytes();
    let bytes = bytes::Bytes::from(buf);
    let pieces: Vec<bytes::Bytes> = (0..len)
        .step_by(PIECE_BYTES)
        .map(|i| bytes.slice(i..(i + PIECE_BYTES).min(len)))
        .collect();
    let mut sent = 0u64;
    let progress = throttle.clone();
    let stream = futures::stream::iter(pieces.into_iter().map(move |p| {
        sent += p.len() as u64;
        progress.sent(before + sent, false);
        Ok::<_, std::io::Error>(p)
    }));
    let result = client
        .put(format!("{}/reports/{id}/files/{index}", job.base_url))
        .bearer_auth(key)
        .header("Upload-Offset", offset.to_string())
        .header(reqwest::header::CONTENT_TYPE, "application/octet-stream")
        .header(reqwest::header::CONTENT_LENGTH, len.to_string())
        .body(reqwest::Body::wrap_stream(stream))
        .send()
        .await;
    let resp = result.map_err(network)?;
    let status = resp.status();
    if status == reqwest::StatusCode::CONFLICT {
        let text = resp.text().await.unwrap_or_default();
        let body: ErrorBody = serde_json::from_str(&text).unwrap_or_default();
        return match body.received {
            Some(received) => {
                tracing::info!(target: "hocket::reports", index, offset, received, "the coordinator holds a different offset; carrying on from its");
                Ok((received, false))
            }
            None => Err(Failure::new(
                ReportUploadError::Server,
                format!("HTTP 409 without an offset: {}", body.detail),
            )),
        };
    }
    if !status.is_success() {
        return Err(failure(status, resp).await);
    }
    let answer: PutAnswer = resp.json().await.map_err(|e| {
        Failure::new(
            ReportUploadError::Server,
            format!("unreadable answer to PUT: {}", e.without_url()),
        )
    })?;
    Ok((answer.received, answer.complete))
}

fn ids(job: &Job) -> Result<(&str, &str), Failure> {
    match (&job.report_id, &job.upload_key) {
        (Some(id), Some(key)) => Ok((id, key)),
        _ => Err(Failure::new(
            ReportUploadError::Server,
            "the report has no upload key",
        )),
    }
}

fn network(e: reqwest::Error) -> Failure {
    Failure::network(e.without_url().to_string())
}

/// An error answer as a [`Failure`]: 401/403 not allowed, 413 too large, 5xx
/// transient, anything else the server's.
async fn failure(status: reqwest::StatusCode, resp: reqwest::Response) -> Failure {
    let text = resp.text().await.unwrap_or_default();
    let body: ErrorBody = serde_json::from_str(&text).unwrap_or_default();
    let mut detail = format!("HTTP {}", status.as_u16());
    if !body.error.is_empty() {
        detail.push_str(&format!(" {}", body.error));
    }
    if !body.detail.is_empty() {
        detail.push_str(&format!(": {}", body.detail));
    }
    let error = match status.as_u16() {
        401 | 403 => ReportUploadError::NotAllowed,
        413 => ReportUploadError::TooLarge,
        408 => ReportUploadError::Network,
        _ => ReportUploadError::Server,
    };
    Failure {
        error,
        detail,
        // 408: the coordinator kept what arrived of a stalled request; the
        // next attempt asks how far it got.
        transient: status.is_server_error() || status == reqwest::StatusCode::REQUEST_TIMEOUT,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::connect::wire::ReportGrant;

    #[test]
    fn base_url_follows_the_coordinator_url() {
        let cases = [
            (
                "wss://hocket.example.com/ws",
                Some("https://hocket.example.com"),
            ),
            (
                "wss://hocket.example.com/ws/",
                Some("https://hocket.example.com"),
            ),
            ("ws://10.0.0.2:8080/ws", Some("http://10.0.0.2:8080")),
            (
                "wss://h.example/hocket/ws",
                Some("https://h.example/hocket"),
            ),
            ("wss://h.example/", Some("https://h.example")),
            ("wss://h.example/wsx", Some("https://h.example/wsx")),
            ("wss://h.example/ws?x=1#y", Some("https://h.example")),
            ("https://h.example/ws", Some("https://h.example")),
            ("ws://[::1]:9/ws", Some("http://[::1]:9")),
            ("ftp://h.example/ws", None),
            ("not a url", None),
        ];
        for (url, want) in cases {
            assert_eq!(base_url(url).as_deref(), want, "{url}");
        }
    }

    fn conn(tier: ConnectionTier, connected: bool, url: Option<&str>) -> ConnectionState {
        ConnectionState {
            tier,
            connected,
            coordinator_url: url.map(Into::into),
            ..Default::default()
        }
    }

    fn reply(from: &str, part: u32, grant: Option<ReportGrant>) -> ReplyPart {
        ReplyPart {
            request_id: "r".into(),
            from: from.into(),
            from_name: from.into(),
            part,
            parts: 2,
            text: String::new(),
            declined: None,
            asked: vec![],
            report_grant: grant,
        }
    }

    fn grant(ms: u64) -> Option<ReportGrant> {
        Some(ReportGrant {
            token: "s3cret".into(),
            expires_in_ms: ms,
        })
    }

    #[test]
    fn only_the_coordinators_first_part_grants() {
        let url = "wss://c.example/ws";
        let c = conn(ConnectionTier::Coordinator, true, Some(url));
        let hour = 3_600_000;
        assert_eq!(
            grant_update(&reply("room", 0, grant(hour)), &c, 0.0),
            GrantUpdate::Ignore
        );
        assert_eq!(
            grant_update(&reply("dev-b", 0, grant(hour)), &c, 0.0),
            GrantUpdate::Ignore
        );
        assert_eq!(
            grant_update(&reply(COORDINATOR, 1, grant(hour)), &c, 0.0),
            GrantUpdate::Ignore
        );
        let lan = conn(ConnectionTier::Lan, true, Some(url));
        assert_eq!(
            grant_update(&reply(COORDINATOR, 0, grant(hour)), &lan, 0.0),
            GrantUpdate::Ignore
        );
        assert_eq!(
            grant_update(&reply(COORDINATOR, 0, None), &c, 0.0),
            GrantUpdate::Clear
        );
        let GrantUpdate::Hold(g) = grant_update(&reply(COORDINATOR, 0, grant(hour)), &c, 1_000.0)
        else {
            panic!()
        };
        assert_eq!(g.coordinator_url, url);
        assert_eq!(g.expires_at, 1_000.0 + hour as f64 - GRANT_MARGIN_MS);
        assert!(!format!("{g:?}").contains("s3cret"));

        assert!(grant_usable(Some(&g), &c, 2_000.0));
        assert!(!grant_usable(Some(&g), &c, g.expires_at));
        assert!(!grant_usable(None, &c, 2_000.0));
        assert!(!grant_usable(
            Some(&g),
            &conn(ConnectionTier::Coordinator, false, Some(url)),
            2_000.0
        ));
        assert!(!grant_usable(
            Some(&g),
            &conn(ConnectionTier::Coordinator, true, Some("wss://other/ws")),
            2_000.0
        ));
        assert!(!grant_usable(Some(&g), &lan, 2_000.0));
    }

    fn attach(path: &Path, kind: ReportAttachmentKind) -> ReportAttachment {
        ReportAttachment {
            path: path.to_string_lossy().into_owned(),
            kind,
            delete_after: true,
        }
    }

    #[test]
    fn validation_checks_counts_sizes_and_files() {
        let dir = tempfile::tempdir().unwrap();
        let shot = dir.path().join("shot.jpg");
        std::fs::write(&shot, b"jpeg").unwrap();
        let empty = dir.path().join("empty.png");
        std::fs::write(&empty, b"").unwrap();
        let s = ReportAttachmentKind::Screenshot;
        let r = ReportAttachmentKind::Recording;

        let files = validate("note", &[attach(&shot, s)]).unwrap();
        assert_eq!(files[0].name, "shot.jpg");
        assert_eq!(files[0].size, 4);
        assert!(files[0].delete_after);

        let err = |note: &str, a: &[ReportAttachment]| validate(note, a).unwrap_err().error;
        assert_eq!(
            err("", &vec![attach(&shot, s); 6]),
            ReportUploadError::Unreadable
        );
        assert_eq!(
            err("", &[attach(&shot, r), attach(&shot, r)]),
            ReportUploadError::Unreadable
        );
        assert_eq!(
            err("", &[attach(&dir.path().join("missing.jpg"), s)]),
            ReportUploadError::Unreadable
        );
        assert_eq!(
            err("", &[attach(dir.path(), s)]),
            ReportUploadError::Unreadable
        );
        assert_eq!(err("", &[attach(&empty, s)]), ReportUploadError::Unreadable);
        assert_eq!(
            err(&"x".repeat(MAX_NOTE_CHARS + 1), &[]),
            ReportUploadError::TooLarge
        );
        let big = dir.path().join("big.jpg");
        let f = std::fs::File::create(&big).unwrap();
        f.set_len(MAX_SCREENSHOT_BYTES + 1).unwrap();
        assert_eq!(err("", &[attach(&big, s)]), ReportUploadError::TooLarge);
        // The same size is fine for a recording.
        assert!(validate("", &[attach(&big, r)]).is_ok());
    }

    #[test]
    fn staging_writes_the_bundle_and_a_job_that_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let shot = dir.path().join("screenshot-1.jpg");
        std::fs::write(&shot, b"jpeg").unwrap();
        let staging = dir.path().join("data").join(DIR);
        let job = stage(
            &staging,
            NewReport {
                note: "queue jumped",
                bundle: "bundle text",
                attachments: &[attach(&shot, ReportAttachmentKind::Screenshot)],
                base_url: "https://c.example".into(),
                client: "Hocket Linux 1".into(),
                device: "x".repeat(300),
                started_at: 1_793_541_780_000.0, // 2026-11-01T14:03:00Z
            },
        )
        .unwrap();
        assert_eq!(job.state, ReportUploadState::Uploading);
        assert_eq!(job.device.chars().count(), MAX_LABEL_CHARS);
        assert_eq!(job.files.len(), 2);
        assert_eq!(job.files[0].kind, FileKind::Bundle);
        assert_eq!(job.files[0].name, "hocket-diagnostics-20261101-1403.txt");
        assert_eq!(
            std::fs::read_to_string(&job.files[0].path).unwrap(),
            "bundle text"
        );
        assert_eq!(job.total_bytes(), 11 + 4);
        assert_eq!(load_job(&staging).unwrap().as_ref(), Some(&job));
        assert!(!format!("{job:?}").contains("queue jumped"));

        let mut later = job.clone();
        later.report_id = Some("R-1".into());
        later.upload_key = Some("k".into());
        later.state = ReportUploadState::Failed;
        later.error = Some(ReportUploadError::Network);
        later.files[1].received = 2;
        save_job(&staging, &later).unwrap();
        assert!(!staging.join("job.json.tmp").exists());
        let back = load_job(&staging).unwrap().unwrap();
        assert_eq!(back, later);
        assert_eq!(back.status().sent_bytes, 2.0);
        assert_eq!(back.status().error, Some(ReportUploadError::Network));

        clean_up(&staging, Some(&back));
        assert!(!staging.exists());
        assert!(!shot.exists(), "the platform's copy goes too");
        assert_eq!(load_job(&staging).unwrap(), None);
    }
}
