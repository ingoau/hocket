//! Save to Coordinator (`docs/reports.md`): grants handed out with the
//! diagnostics answer, the HTTP upload API, storage under
//! `<data-dir>/reports/`, quota, retention and the `reports` CLI.
//!
//! Nothing a client sends is ever used as a path: report directories are
//! named by [`new_id`] and checked with [`valid_id`], and files get names the
//! coordinator picks. The grant token and the upload key are never logged,
//! nor is the note.

use std::collections::{HashMap, VecDeque};
use std::io;
use std::path::{Path as FsPath, PathBuf};
use std::sync::Arc;

use axum::body::Body;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use futures::StreamExt;
use parking_lot::Mutex;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncSeekExt, AsyncWriteExt};
use tracing::{info, warn};

use hocket_core::connect::wire::{scope_key, ReportGrant};

use crate::{App, READ_TIMEOUT};

const MIB: u64 = 1024 * 1024;
const HOUR_MS: u64 = 3_600_000;
const DAY_MS: u64 = 24 * HOUR_MS;

/// Directory under `--data-dir` that holds the reports.
pub const REPORTS_DIR: &str = "reports";
/// How long a grant is good for.
pub const GRANT_TTL_MS: u64 = 6 * HOUR_MS;
/// Grants kept in memory at most (the soonest to expire go first).
pub const MAX_GRANTS: usize = 256;
/// New reports per account per hour.
pub const CREATES_PER_HOUR: usize = 20;
/// Largest `POST /reports` body.
pub const MAX_CREATE_BODY: usize = 64 * 1024;
/// Largest `PUT` body.
pub const MAX_PUT_BYTES: u64 = 8 * MIB;
pub const MAX_NOTE_CHARS: usize = 8000;
/// `client`, `device`.
pub const MAX_LABEL_CHARS: usize = 200;
/// A file's `name`.
pub const MAX_NAME_CHARS: usize = 255;
pub const MAX_SCREENSHOTS: usize = 5;
pub const MAX_BUNDLE_BYTES: u64 = 16 * MIB;
pub const MAX_SCREENSHOT_BYTES: u64 = 25 * MIB;
pub const MAX_RECORDING_BYTES: u64 = 1024 * MIB;
/// Recordings are deleted this long after their report was made.
pub const RECORDING_RETENTION_MS: u64 = 30 * DAY_MS;
/// Whole reports are deleted this long after they were made.
pub const REPORT_RETENTION_MS: u64 = 90 * DAY_MS;
/// Reports never finished are deleted this long after they were made.
pub const INCOMPLETE_RETENTION_MS: u64 = 48 * HOUR_MS;
/// An unfinished report idle this long may be deleted to make room.
pub const STALE_INCOMPLETE_MS: u64 = HOUR_MS;

/// The characters of an id's random part (no `i`, `l`, `o`, `u`).
const ID_ALPHABET: &[u8; 32] = b"0123456789abcdefghjkmnpqrstvwxyz";
const META_FILE: &str = "report.json";
const LOG_FILE: &str = "coordinator.log";

/// `report.json`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ReportMeta {
    pub id: String,
    /// Epoch ms (UTC).
    pub created_at: u64,
    /// When the last byte of the last file arrived.
    pub completed_at: Option<u64>,
    pub username: String,
    pub server_url: String,
    pub note: String,
    pub client: String,
    pub device: String,
    pub files: Vec<StoredFile>,
    /// SHA-256 (hex) of the upload key; the key itself is never stored.
    pub upload_key_sha256: String,
    /// Epoch ms of the last bytes received (or of creation).
    #[serde(default)]
    pub updated_at: u64,
}

/// One file of a report.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StoredFile {
    pub index: u32,
    /// `bundle`, `screenshot` or `recording`.
    pub kind: String,
    /// The client's name for it (informational only).
    pub name: String,
    /// The name it is stored under, in the report's directory.
    pub file: String,
    pub size: u64,
    pub received: u64,
    /// Set when retention deleted it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deleted_at: Option<u64>,
}

impl ReportMeta {
    fn complete(&self) -> bool {
        self.files
            .iter()
            .all(|f| f.received >= f.size || f.deleted_at.is_some())
    }

    fn declared(&self) -> u64 {
        self.files.iter().map(|f| f.size).sum()
    }

    /// When bytes last arrived for it.
    fn last_activity(&self) -> u64 {
        self.updated_at.max(self.created_at)
    }

    /// Unfinished and untouched for [`STALE_INCOMPLETE_MS`]: its client gave
    /// up on it (dismissed it, or made it again after losing the answer), so
    /// it may be deleted to make room.
    fn abandoned(&self, now: u64) -> bool {
        !self.complete() && now.saturating_sub(self.last_activity()) > STALE_INCOMPLETE_MS
    }
}

#[derive(Debug, Clone)]
struct Grant {
    server_url: String,
    username: String,
    expires_at: u64,
}

/// Report state shared by the HTTP handlers: grants, the per-account create
/// log, and a lock per report so two uploads to it never interleave.
pub(crate) struct Reports {
    dir: PathBuf,
    quota_bytes: u64,
    grants: Mutex<HashMap<String, Grant>>,
    creates: Mutex<HashMap<String, VecDeque<u64>>>,
    locks: Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>,
    /// Creates (quota check, eviction, id choice) happen one at a time.
    create_lock: tokio::sync::Mutex<()>,
}

impl Reports {
    /// `dir` is `<data-dir>/reports`; created if missing.
    pub(crate) fn new(dir: PathBuf, quota_mb: u64) -> io::Result<Reports> {
        std::fs::create_dir_all(&dir)?;
        Ok(Reports {
            dir,
            quota_bytes: quota_mb.saturating_mul(MIB),
            grants: Mutex::new(HashMap::new()),
            creates: Mutex::new(HashMap::new()),
            locks: Mutex::new(HashMap::new()),
            create_lock: tokio::sync::Mutex::new(()),
        })
    }

    /// A new grant for this (verified) account.
    pub(crate) fn mint_grant(&self, server_url: &str, username: &str, now: u64) -> ReportGrant {
        let token = random_hex(32);
        let mut grants = self.grants.lock();
        grants.retain(|_, g| g.expires_at > now);
        while grants.len() >= MAX_GRANTS {
            let Some(soonest) = grants
                .iter()
                .min_by_key(|(_, g)| g.expires_at)
                .map(|(k, _)| k.clone())
            else {
                break;
            };
            grants.remove(&soonest);
        }
        grants.insert(
            token.clone(),
            Grant {
                server_url: server_url.to_string(),
                username: username.to_string(),
                expires_at: now + GRANT_TTL_MS,
            },
        );
        ReportGrant {
            token,
            expires_in_ms: GRANT_TTL_MS,
        }
    }

    fn grant(&self, token: &str, now: u64) -> Option<Grant> {
        self.grants
            .lock()
            .get(token)
            .filter(|g| g.expires_at > now)
            .cloned()
    }

    /// Whether the account may create another report now.
    fn rate_ok(&self, account: &str, now: u64) -> bool {
        let mut creates = self.creates.lock();
        creates.retain(|_, q| {
            while q.front().is_some_and(|t| t + HOUR_MS <= now) {
                q.pop_front();
            }
            !q.is_empty()
        });
        creates.get(account).map_or(0, |q| q.len()) < CREATES_PER_HOUR
    }

    fn record_create(&self, account: &str, now: u64) {
        self.creates
            .lock()
            .entry(account.to_string())
            .or_default()
            .push_back(now);
    }

    fn lock_for(&self, id: &str) -> Arc<tokio::sync::Mutex<()>> {
        let mut locks = self.locks.lock();
        if locks.len() > 256 {
            locks.retain(|_, l| Arc::strong_count(l) > 1);
        }
        locks.entry(id.to_string()).or_default().clone()
    }

    /// Make room for `needed` bytes (deleting the oldest finished reports),
    /// then create the report's directory, `report.json` and `coordinator.log`.
    fn create_blocking(
        &self,
        mut meta: ReportMeta,
        log: String,
        now: u64,
    ) -> Result<ReportMeta, ApiError> {
        let needed = meta.declared() + log.len() as u64;
        self.make_room(needed, now)?;
        let date = &hocket_core::devlog::format_timestamp(now)[..10].replace('-', "");
        let mut made = None;
        for _ in 0..32 {
            let id = new_id(date);
            match std::fs::create_dir(self.dir.join(&id)) {
                Ok(()) => {
                    made = Some(id);
                    break;
                }
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(ApiError::io(e)),
            }
        }
        let id = made.ok_or_else(|| ApiError::io("no free report id"))?;
        meta.id = id.clone();
        let dir = self.dir.join(&id);
        let written = save_meta(&dir, &meta).and_then(|()| std::fs::write(dir.join(LOG_FILE), log));
        if let Err(e) = written {
            let _ = std::fs::remove_dir_all(&dir);
            return Err(ApiError::io(e));
        }
        Ok(meta)
    }

    /// Delete the oldest finished (or abandoned, [`ReportMeta::abandoned`])
    /// reports until `needed` more bytes fit under the quota; `tooLarge`
    /// (deleting nothing) when they cannot.
    fn make_room(&self, needed: u64, now: u64) -> Result<(), ApiError> {
        let too_large = || {
            ApiError::new(
                StatusCode::PAYLOAD_TOO_LARGE,
                "tooLarge",
                "this report does not fit in the coordinator's reports quota",
            )
        };
        let mut all = self.all();
        all.sort_by_key(|(_, m)| m.created_at);
        let usage: Vec<u64> = all.iter().map(|(dir, m)| usage(dir, m)).collect();
        let mut total: u64 = usage.iter().sum();
        if total + needed <= self.quota_bytes {
            return Ok(());
        }
        let evictable: u64 = all
            .iter()
            .zip(&usage)
            .filter(|((_, m), _)| m.complete() || m.abandoned(now))
            .map(|(_, u)| u)
            .sum();
        if total - evictable + needed > self.quota_bytes {
            return Err(too_large());
        }
        for ((dir, meta), used) in all.iter().zip(&usage) {
            if total + needed <= self.quota_bytes {
                break;
            }
            if !(meta.complete() || meta.abandoned(now)) {
                continue;
            }
            let lock = self.lock_for(&meta.id);
            let Ok(_held) = lock.try_lock() else {
                continue;
            };
            match std::fs::remove_dir_all(dir) {
                Ok(()) => {
                    total -= used;
                    info!(id = meta.id, bytes = used, "report deleted to make room");
                }
                Err(e) => warn!(id = meta.id, error = %e, "could not delete report"),
            }
        }
        if total + needed > self.quota_bytes {
            return Err(too_large());
        }
        Ok(())
    }

    /// Every readable report.
    fn all(&self) -> Vec<(PathBuf, ReportMeta)> {
        all_reports(&self.dir)
    }

    /// Apply retention at `now`: recordings after 30 days, whole reports
    /// after 90, unfinished ones after 48 hours. Dates come from
    /// `report.json`. A report being uploaded to is left for the next round.
    pub(crate) fn purge(&self, now: u64) -> PurgeStats {
        let mut stats = PurgeStats::default();
        let Ok(entries) = std::fs::read_dir(&self.dir) else {
            return stats;
        };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if !valid_id(&name) {
                continue;
            }
            let dir = entry.path();
            let lock = self.lock_for(&name);
            let Ok(_held) = lock.try_lock() else {
                continue;
            };
            let meta = match load_meta(&dir) {
                Ok(m) => m,
                Err(e) if e.kind() == io::ErrorKind::NotFound => {
                    // A create that died before writing report.json: nothing to date it by
                    // but the directory itself.
                    let old = entry
                        .metadata()
                        .and_then(|m| m.modified())
                        .ok()
                        .and_then(|t| t.elapsed().ok())
                        .is_some_and(|age| age.as_millis() as u64 >= INCOMPLETE_RETENTION_MS);
                    if old && std::fs::remove_dir_all(&dir).is_ok() {
                        stats.reports += 1;
                    }
                    continue;
                }
                Err(e) => {
                    warn!(id = name, error = %e, "unreadable report.json skipped");
                    continue;
                }
            };
            let age = now.saturating_sub(meta.created_at);
            if age >= REPORT_RETENTION_MS
                || (meta.completed_at.is_none() && age >= INCOMPLETE_RETENTION_MS)
            {
                match std::fs::remove_dir_all(&dir) {
                    Ok(()) => {
                        stats.reports += 1;
                        info!(
                            id = name,
                            complete = meta.completed_at.is_some(),
                            "report deleted (retention)"
                        );
                    }
                    Err(e) => warn!(id = name, error = %e, "could not delete report"),
                }
                continue;
            }
            if age >= RECORDING_RETENTION_MS {
                let mut meta = meta;
                let mut changed = false;
                for f in meta
                    .files
                    .iter_mut()
                    .filter(|f| f.kind == "recording" && f.deleted_at.is_none())
                {
                    match std::fs::remove_file(dir.join(&f.file)) {
                        Ok(()) => {}
                        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                        Err(e) => {
                            warn!(id = name, error = %e, "could not delete recording");
                            continue;
                        }
                    }
                    f.deleted_at = Some(now);
                    changed = true;
                    stats.recordings += 1;
                }
                if changed {
                    if let Err(e) = save_meta(&dir, &meta) {
                        warn!(id = name, error = %e, "could not update report.json");
                    }
                    info!(id = name, "recording deleted (retention)");
                }
            }
        }
        stats
    }
}

/// What a retention pass deleted.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct PurgeStats {
    pub reports: usize,
    pub recordings: usize,
}

/// `R-YYYYMMDD-xxxx` for a `YYYYMMDD` date.
fn new_id(date: &str) -> String {
    let mut bytes = [0u8; 4];
    rand::rng().fill_bytes(&mut bytes);
    let tail: String = bytes
        .iter()
        .map(|b| ID_ALPHABET[(b & 31) as usize] as char)
        .collect();
    format!("R-{date}-{tail}")
}

/// Whether `s` has the shape of a report id (and so is safe as a path segment).
pub fn valid_id(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 15
        && s.starts_with("R-")
        && b[2..10].iter().all(u8::is_ascii_digit)
        && b[10] == b'-'
        && b[11..].iter().all(|c| ID_ALPHABET.contains(c))
}

fn random_hex(bytes: usize) -> String {
    let mut buf = vec![0u8; bytes];
    rand::rng().fill_bytes(&mut buf);
    hex::encode(buf)
}

fn sha256_hex(s: &str) -> String {
    hex::encode(Sha256::digest(s.as_bytes()))
}

fn load_meta(dir: &FsPath) -> io::Result<ReportMeta> {
    let bytes = std::fs::read(dir.join(META_FILE))?;
    serde_json::from_slice(&bytes).map_err(io::Error::other)
}

/// Write `report.json` through a temporary file and a rename.
fn save_meta(dir: &FsPath, meta: &ReportMeta) -> io::Result<()> {
    let json = serde_json::to_vec_pretty(meta).map_err(io::Error::other)?;
    let tmp = dir.join(format!("{META_FILE}.tmp-{}", random_hex(4)));
    std::fs::write(&tmp, json)?;
    std::fs::rename(&tmp, dir.join(META_FILE)).inspect_err(|_| {
        let _ = std::fs::remove_file(&tmp);
    })
}

async fn load_meta_async(dir: &FsPath) -> Result<ReportMeta, ApiError> {
    match tokio::fs::read(dir.join(META_FILE)).await {
        Ok(bytes) => serde_json::from_slice(&bytes).map_err(ApiError::io),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Err(ApiError::not_found()),
        Err(e) => Err(ApiError::io(e)),
    }
}

async fn save_meta_async(dir: &FsPath, meta: &ReportMeta) -> Result<(), ApiError> {
    let json = serde_json::to_vec_pretty(meta).map_err(ApiError::io)?;
    let tmp = dir.join(format!("{META_FILE}.tmp-{}", random_hex(4)));
    tokio::fs::write(&tmp, json).await.map_err(ApiError::io)?;
    if let Err(e) = tokio::fs::rename(&tmp, dir.join(META_FILE)).await {
        let _ = tokio::fs::remove_file(&tmp).await;
        return Err(ApiError::io(e));
    }
    Ok(())
}

/// Every report directory under `root` with a readable `report.json`.
fn all_reports(root: &FsPath) -> Vec<(PathBuf, ReportMeta)> {
    let Ok(entries) = std::fs::read_dir(root) else {
        return vec![];
    };
    entries
        .flatten()
        .filter(|e| valid_id(&e.file_name().to_string_lossy()))
        .filter_map(|e| {
            let dir = e.path();
            load_meta(&dir).ok().map(|m| (dir, m))
        })
        .collect()
}

/// Bytes a report holds on disk, plus what its unfinished files will still take.
fn usage(dir: &FsPath, meta: &ReportMeta) -> u64 {
    let on_disk: u64 = std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .flatten()
                .filter_map(|e| e.metadata().ok())
                .filter(|m| m.is_file())
                .map(|m| m.len())
                .sum()
        })
        .unwrap_or(0);
    let pending: u64 = meta
        .files
        .iter()
        .filter(|f| f.deleted_at.is_none())
        .map(|f| f.size.saturating_sub(f.received))
        .sum();
    on_disk + pending
}

/// The stored name for a file: fixed per kind, with the extension taken from
/// the client's name only when it is a known image or video type.
fn stored_name(kind: &str, nth: usize, client_name: &str) -> String {
    if kind == "bundle" {
        return "bundle.txt".into();
    }
    let ext = client_name
        .rsplit_once('.')
        .map(|(_, e)| e.to_ascii_lowercase())
        .unwrap_or_default();
    let known: &[&str] = if kind == "screenshot" {
        &["jpg", "jpeg", "png", "webp", "gif", "heic"]
    } else {
        &["mp4", "webm", "mov", "mkv", "m4v", "3gp"]
    };
    let ext = match ext.as_str() {
        "jpeg" => "jpg",
        e if known.contains(&e) => e,
        _ => "bin",
    };
    if kind == "screenshot" {
        format!("screenshot-{nth}.{ext}")
    } else {
        format!("recording.{ext}")
    }
}

// --- HTTP -------------------------------------------------------------------

/// An error answer: `{"error": code, "detail": ...}` (plus `received` on 409).
#[derive(Debug)]
pub(crate) struct ApiError {
    status: StatusCode,
    code: &'static str,
    detail: String,
    received: Option<u64>,
}

impl ApiError {
    fn new(status: StatusCode, code: &'static str, detail: impl Into<String>) -> ApiError {
        ApiError {
            status,
            code,
            detail: detail.into(),
            received: None,
        }
    }
    fn unauthorized(detail: &str) -> ApiError {
        ApiError::new(StatusCode::UNAUTHORIZED, "unauthorized", detail)
    }
    fn not_found() -> ApiError {
        ApiError::new(StatusCode::NOT_FOUND, "notFound", "no such report")
    }
    fn bad(detail: impl Into<String>) -> ApiError {
        ApiError::new(StatusCode::BAD_REQUEST, "badRequest", detail)
    }
    fn too_large(detail: impl Into<String>) -> ApiError {
        ApiError::new(StatusCode::PAYLOAD_TOO_LARGE, "tooLarge", detail)
    }
    fn disabled() -> ApiError {
        ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "disabled",
            "this coordinator does not keep reports",
        )
    }
    /// A server-side failure; the detail goes to the log, not the client.
    fn io(e: impl std::fmt::Display) -> ApiError {
        warn!(error = %e, "report storage failed");
        ApiError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal",
            "the coordinator could not store the report",
        )
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let mut body = serde_json::json!({ "error": self.code, "detail": self.detail });
        if let Some(r) = self.received {
            body["received"] = r.into();
        }
        (self.status, axum::Json(body)).into_response()
    }
}

fn bearer(headers: &HeaderMap) -> Option<&str> {
    let v = headers
        .get(axum::http::header::AUTHORIZATION)?
        .to_str()
        .ok()?;
    let (scheme, token) = v.split_once(' ')?;
    let token = token.trim();
    (scheme.eq_ignore_ascii_case("bearer") && !token.is_empty()).then_some(token)
}

fn content_length(headers: &HeaderMap) -> Option<u64> {
    headers
        .get(axum::http::header::CONTENT_LENGTH)?
        .to_str()
        .ok()?
        .parse()
        .ok()
}

fn reports_of(app: &App) -> Result<&Reports, ApiError> {
    app.reports.as_ref().ok_or_else(ApiError::disabled)
}

fn check_key(meta: &ReportMeta, headers: &HeaderMap) -> Result<(), ApiError> {
    let key = bearer(headers).ok_or_else(|| ApiError::unauthorized("upload key missing"))?;
    // Comparing digests: timing reveals nothing about the key.
    if sha256_hex(key) != meta.upload_key_sha256 {
        return Err(ApiError::unauthorized("wrong upload key"));
    }
    Ok(())
}

#[derive(Deserialize)]
struct CreateRequest {
    #[serde(default)]
    note: String,
    #[serde(default)]
    client: String,
    #[serde(default)]
    device: String,
    files: Vec<FileDecl>,
}

#[derive(Deserialize)]
struct FileDecl {
    kind: String,
    size: u64,
    #[serde(default)]
    name: String,
}

/// Check a create request against the limits; the files as they will be stored.
fn validate(req: &CreateRequest) -> Result<Vec<StoredFile>, ApiError> {
    if req.note.chars().count() > MAX_NOTE_CHARS {
        return Err(ApiError::bad(format!(
            "note is longer than {MAX_NOTE_CHARS} characters"
        )));
    }
    if req.client.chars().count() > MAX_LABEL_CHARS || req.device.chars().count() > MAX_LABEL_CHARS
    {
        return Err(ApiError::bad(format!(
            "client and device are at most {MAX_LABEL_CHARS} characters"
        )));
    }
    if req.files.first().map(|f| f.kind.as_str()) != Some("bundle") {
        return Err(ApiError::bad("the first file must be the bundle"));
    }
    let (mut bundles, mut screenshots, mut recordings) = (0, 0, 0);
    let mut out = Vec::with_capacity(req.files.len());
    for (i, f) in req.files.iter().enumerate() {
        let (count, max) = match f.kind.as_str() {
            "bundle" => {
                bundles += 1;
                (bundles, MAX_BUNDLE_BYTES)
            }
            "screenshot" => {
                screenshots += 1;
                (screenshots, MAX_SCREENSHOT_BYTES)
            }
            "recording" => {
                recordings += 1;
                (recordings, MAX_RECORDING_BYTES)
            }
            _ => {
                return Err(ApiError::bad(
                    "file kind must be bundle, screenshot or recording",
                ))
            }
        };
        if bundles > 1 || screenshots > MAX_SCREENSHOTS || recordings > 1 {
            return Err(ApiError::bad(format!(
                "one bundle, at most {MAX_SCREENSHOTS} screenshots and one recording"
            )));
        }
        if f.size == 0 {
            return Err(ApiError::bad("empty files are refused"));
        }
        if f.size > max {
            return Err(ApiError::too_large(format!(
                "a {} is at most {} MiB",
                f.kind,
                max / MIB
            )));
        }
        if f.name.chars().count() > MAX_NAME_CHARS {
            return Err(ApiError::bad(format!(
                "file names are at most {MAX_NAME_CHARS} characters"
            )));
        }
        out.push(StoredFile {
            index: i as u32,
            kind: f.kind.clone(),
            name: f.name.clone(),
            file: stored_name(&f.kind, count, &f.name),
            size: f.size,
            received: 0,
            deleted_at: None,
        });
    }
    Ok(out)
}

fn files_json(files: &[StoredFile]) -> Vec<serde_json::Value> {
    files
        .iter()
        .map(|f| serde_json::json!({ "index": f.index, "size": f.size, "received": f.received }))
        .collect()
}

/// `POST /reports`.
pub(crate) async fn create(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    body: Body,
) -> Response {
    create_inner(app, headers, body)
        .await
        .unwrap_or_else(IntoResponse::into_response)
}

async fn create_inner(app: Arc<App>, headers: HeaderMap, body: Body) -> Result<Response, ApiError> {
    let reports = reports_of(&app)?;
    let now = app.now_ms();
    let token = bearer(&headers).ok_or_else(|| ApiError::unauthorized("grant missing"))?;
    let grant = reports
        .grant(token, now)
        .ok_or_else(|| ApiError::unauthorized("the grant is unknown or has expired"))?;
    if content_length(&headers).is_some_and(|n| n > MAX_CREATE_BODY as u64) {
        return Err(ApiError::too_large("the request is larger than 64 KiB"));
    }
    let bytes = axum::body::to_bytes(body, MAX_CREATE_BODY)
        .await
        .map_err(|_| ApiError::too_large("the request is larger than 64 KiB"))?;
    let req: CreateRequest =
        serde_json::from_slice(&bytes).map_err(|e| ApiError::bad(format!("bad request: {e}")))?;
    let files = validate(&req)?;
    let account = scope_key(&grant.server_url, &grant.username);
    let key = random_hex(32);
    let meta = ReportMeta {
        id: String::new(),
        created_at: now,
        completed_at: None,
        username: grant.username.clone(),
        server_url: grant.server_url.clone(),
        note: req.note,
        client: req.client,
        device: req.device,
        files,
        upload_key_sha256: sha256_hex(&key),
        updated_at: now,
    };

    let _serial = reports.create_lock.lock().await;
    if !reports.rate_ok(&account, now) {
        return Err(ApiError::new(
            StatusCode::TOO_MANY_REQUESTS,
            "rateLimited",
            format!("at most {CREATES_PER_HOUR} reports per hour"),
        ));
    }
    let log = app.diagnostics_text(true);
    let worker = app.clone();
    let meta =
        tokio::task::spawn_blocking(move || reports_of(&worker)?.create_blocking(meta, log, now))
            .await
            .map_err(ApiError::io)??;
    reports.record_create(&account, now);
    info!(
        id = meta.id,
        user = meta.username,
        files = meta.files.len(),
        bytes = meta.declared(),
        "report created"
    );
    Ok((
        StatusCode::CREATED,
        axum::Json(serde_json::json!({
            "id": meta.id,
            "uploadKey": key,
            "files": files_json(&meta.files),
        })),
    )
        .into_response())
}

/// What of a file is really on disk: never more than `report.json` says, nor
/// than the file holds.
async fn on_disk(dir: &FsPath, f: &StoredFile) -> u64 {
    let len = tokio::fs::metadata(dir.join(&f.file))
        .await
        .map(|m| m.len())
        .unwrap_or(0);
    f.received.min(len)
}

/// `DELETE /reports/{id}`: the client gave up on an unfinished report.
pub(crate) async fn remove(
    State(app): State<Arc<App>>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Response {
    remove_inner(app, id, headers)
        .await
        .unwrap_or_else(IntoResponse::into_response)
}

async fn remove_inner(app: Arc<App>, id: String, headers: HeaderMap) -> Result<Response, ApiError> {
    let reports = reports_of(&app)?;
    if !valid_id(&id) {
        return Err(ApiError::not_found());
    }
    let dir = reports.dir.join(&id);
    check_key(&load_meta_async(&dir).await?, &headers)?;
    let lock = reports.lock_for(&id);
    let _held = lock.lock().await;
    let meta = load_meta_async(&dir).await?;
    if meta.completed_at.is_some() {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            "conflict",
            "a finished report stays until retention removes it",
        ));
    }
    tokio::fs::remove_dir_all(&dir)
        .await
        .map_err(ApiError::io)?;
    info!(
        id = meta.id,
        user = meta.username,
        "unfinished report deleted by its client"
    );
    Ok(StatusCode::NO_CONTENT.into_response())
}

/// `GET /reports/{id}`.
pub(crate) async fn status(
    State(app): State<Arc<App>>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Response {
    status_inner(app, id, headers)
        .await
        .unwrap_or_else(IntoResponse::into_response)
}

async fn status_inner(app: Arc<App>, id: String, headers: HeaderMap) -> Result<Response, ApiError> {
    let reports = reports_of(&app)?;
    if !valid_id(&id) {
        return Err(ApiError::not_found());
    }
    let dir = reports.dir.join(&id);
    // The key first, so nobody without it waits on an upload's lock.
    check_key(&load_meta_async(&dir).await?, &headers)?;
    let lock = reports.lock_for(&id);
    let _held = lock.lock().await;
    let mut meta = load_meta_async(&dir).await?;
    for f in meta.files.iter_mut() {
        if f.deleted_at.is_none() {
            f.received = on_disk(&dir, f).await;
        }
    }
    Ok(axum::Json(serde_json::json!({
        "id": meta.id,
        "files": files_json(&meta.files),
        "complete": meta.completed_at.is_some(),
    }))
    .into_response())
}

/// `PUT /reports/{id}/files/{index}`.
pub(crate) async fn upload(
    State(app): State<Arc<App>>,
    Path((id, index)): Path<(String, String)>,
    headers: HeaderMap,
    body: Body,
) -> Response {
    upload_inner(app, id, index, headers, body)
        .await
        .unwrap_or_else(IntoResponse::into_response)
}

async fn upload_inner(
    app: Arc<App>,
    id: String,
    index: String,
    headers: HeaderMap,
    body: Body,
) -> Result<Response, ApiError> {
    let reports = reports_of(&app)?;
    if !valid_id(&id) {
        return Err(ApiError::not_found());
    }
    let dir = reports.dir.join(&id);
    // The key first, so nobody without it waits on an upload's lock.
    check_key(&load_meta_async(&dir).await?, &headers)?;
    let lock = reports.lock_for(&id);
    let _held = lock.lock().await;
    let mut meta = load_meta_async(&dir).await?;
    let index: usize = index.parse().map_err(|_| ApiError::bad("bad file index"))?;
    let f = match meta.files.get(index) {
        Some(f) if f.deleted_at.is_none() => f.clone(),
        _ => {
            return Err(ApiError::new(
                StatusCode::NOT_FOUND,
                "notFound",
                "no such file",
            ))
        }
    };
    let offset: u64 = headers
        .get("upload-offset")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.trim().parse().ok())
        .ok_or_else(|| ApiError::bad("Upload-Offset missing"))?;
    let received = on_disk(&dir, &f).await;
    if offset != received {
        let mut e = ApiError::new(
            StatusCode::CONFLICT,
            "conflict",
            "Upload-Offset is not what the coordinator has",
        );
        e.received = Some(received);
        return Err(e);
    }
    let cap = (f.size - received).min(MAX_PUT_BYTES);
    let too_large = || {
        ApiError::too_large(format!(
            "at most {cap} bytes fit in this request (8 MiB, and no further than the declared size)"
        ))
    };
    if content_length(&headers).is_some_and(|n| n > cap) {
        return Err(too_large());
    }

    let path = dir.join(&f.file);
    let mut file = tokio::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(&path)
        .await
        .map_err(ApiError::io)?;
    // Anything past what was recorded is a write that was never acknowledged.
    file.set_len(received).await.map_err(ApiError::io)?;
    file.seek(io::SeekFrom::Start(received))
        .await
        .map_err(ApiError::io)?;
    let mut stream = body.into_data_stream();
    let mut written = 0u64;
    let mut over = false;
    let mut broken = false;
    loop {
        match tokio::time::timeout(READ_TIMEOUT, stream.next()).await {
            Ok(Some(Ok(chunk))) => {
                if written + chunk.len() as u64 > cap {
                    over = true;
                    break;
                }
                file.write_all(&chunk).await.map_err(ApiError::io)?;
                written += chunk.len() as u64;
            }
            Ok(None) => break,
            Ok(Some(Err(_))) | Err(_) => {
                broken = true;
                break;
            }
        }
    }
    if over {
        // The whole request is refused: nothing of it is kept.
        let _ = file.set_len(received).await;
        return Err(too_large());
    }
    file.flush().await.map_err(ApiError::io)?;
    drop(file);
    let now_received = received + written;
    meta.files[index].received = now_received;
    meta.updated_at = app.now_ms();
    let complete = meta.complete();
    let finished_now = complete && meta.completed_at.is_none();
    if finished_now {
        meta.completed_at = Some(app.now_ms());
    }
    save_meta_async(&dir, &meta).await?;
    if finished_now {
        info!(
            id = meta.id,
            user = meta.username,
            bytes = meta.declared(),
            "report complete"
        );
    }
    if broken {
        // The client is most likely gone; what arrived is kept for the resume.
        return Err(ApiError::new(
            StatusCode::REQUEST_TIMEOUT,
            "timeout",
            "the request body stalled or ended early; what arrived is kept",
        ));
    }
    Ok(
        axum::Json(serde_json::json!({ "received": now_received, "complete": complete }))
            .into_response(),
    )
}

// --- CLI --------------------------------------------------------------------

/// What a client wrote, safe to print on the operator's terminal: control
/// characters (escape sequences included) are shown escaped; newlines stay
/// when `multiline`.
fn printable(s: &str, multiline: bool) -> String {
    s.chars()
        .map(|c| {
            if (c == '\n' && multiline) || !c.is_control() {
                c.to_string()
            } else {
                c.escape_unicode().to_string()
            }
        })
        .collect()
}

fn human_bytes(n: u64) -> String {
    if n < 1024 {
        return format!("{n} B");
    }
    let mut v = n as f64 / 1024.0;
    for unit in ["KiB", "MiB", "GiB"] {
        if v < 1024.0 {
            return format!("{v:.1} {unit}");
        }
        v /= 1024.0;
    }
    format!("{v:.1} TiB")
}

fn utc(ms: u64) -> String {
    let t = hocket_core::devlog::format_timestamp(ms);
    format!("{} {}Z", &t[..10], &t[11..16])
}

fn file_summary(meta: &ReportMeta) -> String {
    let screenshots = meta.files.iter().filter(|f| f.kind == "screenshot").count();
    let mut parts = vec!["bundle".to_string()];
    match screenshots {
        0 => {}
        1 => parts.push("1 screenshot".into()),
        n => parts.push(format!("{n} screenshots")),
    }
    if let Some(r) = meta.files.iter().find(|f| f.kind == "recording") {
        parts.push(if r.deleted_at.is_some() {
            "recording (deleted)".into()
        } else {
            "recording".into()
        });
    }
    parts.join(", ")
}

/// `reports list`: one line per report under `data_dir`, newest first.
pub fn list(data_dir: &FsPath) -> anyhow::Result<String> {
    let mut all = all_reports(&data_dir.join(REPORTS_DIR));
    all.sort_by(|(_, a), (_, b)| b.created_at.cmp(&a.created_at).then(b.id.cmp(&a.id)));
    if all.is_empty() {
        return Ok("no reports\n".into());
    }
    let mut out = String::new();
    for (_, m) in &all {
        let first = m.note.lines().next().unwrap_or("").trim();
        let note = if first.chars().count() > 60 {
            format!("{}…", first.chars().take(59).collect::<String>())
        } else {
            first.to_string()
        };
        let note = printable(&note, false);
        out.push_str(&format!(
            "{}  {}  {}  {}  {}  {}  {}\n",
            m.id,
            utc(m.created_at),
            printable(&m.username, false),
            file_summary(m),
            human_bytes(m.declared()),
            if m.completed_at.is_some() {
                "complete"
            } else {
                "incomplete"
            },
            note
        ));
    }
    Ok(out)
}

/// `reports show <id>`: the metadata, the whole note and every file's absolute path.
pub fn show(data_dir: &FsPath, id: &str) -> anyhow::Result<String> {
    if !valid_id(id) {
        anyhow::bail!("{id:?} is not a report id (R-YYYYMMDD-xxxx)");
    }
    let root = data_dir.join(REPORTS_DIR);
    let root = std::fs::canonicalize(&root).unwrap_or(root);
    let dir = root.join(id);
    let m = load_meta(&dir).map_err(|e| anyhow::anyhow!("report {id}: {e}"))?;
    let mut out = format!(
        "id:        {}\ncreated:   {}\ncompleted: {}\nuser:      {}\nserver:    {}\nclient:    {}\ndevice:    {}\nsize:      {}\n\nnote:\n{}\n\nfiles:\n",
        m.id,
        utc(m.created_at),
        m.completed_at.map(utc).unwrap_or_else(|| "no (incomplete)".into()),
        printable(&m.username, false),
        printable(&m.server_url, false),
        printable(&m.client, false),
        printable(&m.device, false),
        human_bytes(m.declared()),
        if m.note.is_empty() {
            "(none)".to_string()
        } else {
            printable(&m.note, true)
        },
    );
    for f in &m.files {
        let state = match f.deleted_at {
            Some(t) => format!("deleted {}", utc(t)),
            None if f.received >= f.size => human_bytes(f.size),
            None => format!("{} of {}", human_bytes(f.received), human_bytes(f.size)),
        };
        out.push_str(&format!(
            "  {:<10} {}  ({state}; sent as {:?})\n",
            f.kind,
            dir.join(&f.file).display(),
            f.name
        ));
    }
    out.push_str(&format!(
        "  {:<10} {}\n  {:<10} {}\n",
        "log",
        dir.join(LOG_FILE).display(),
        "metadata",
        dir.join(META_FILE).display()
    ));
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_have_their_shape() {
        for _ in 0..100 {
            let id = new_id("20261101");
            assert!(valid_id(&id), "{id}");
        }
        for bad in [
            "R-2026110-7k3f",
            "R-20261101-7k3",
            "R-20261101-7K3F",
            "R-20261101-7k3i",
            "R-20261101/..ab",
            "../../etc/pass",
            "",
        ] {
            assert!(!valid_id(bad), "{bad}");
        }
    }

    #[test]
    fn stored_names_ignore_what_the_client_says_but_a_known_extension() {
        assert_eq!(stored_name("bundle", 1, "../../x.sh"), "bundle.txt");
        assert_eq!(stored_name("screenshot", 2, "a.JPEG"), "screenshot-2.jpg");
        assert_eq!(
            stored_name("screenshot", 1, "a/../b.png"),
            "screenshot-1.png"
        );
        assert_eq!(stored_name("screenshot", 1, "evil.sh"), "screenshot-1.bin");
        assert_eq!(stored_name("recording", 1, "x.mp4"), "recording.mp4");
        assert_eq!(stored_name("recording", 1, "x.png"), "recording.bin");
        assert_eq!(stored_name("recording", 1, "noext"), "recording.bin");
    }

    #[test]
    fn printed_client_text_cannot_drive_the_terminal() {
        assert_eq!(printable("a\u{1b}[2Jb\nc", false), "a\\u{1b}[2Jb\\u{a}c");
        assert_eq!(printable("line\nnext\u{7}", true), "line\nnext\\u{7}");
    }

    #[test]
    fn bearer_parsing() {
        let mut h = HeaderMap::new();
        assert_eq!(bearer(&h), None);
        h.insert("authorization", "Bearer abc".parse().unwrap());
        assert_eq!(bearer(&h), Some("abc"));
        h.insert("authorization", "Basic abc".parse().unwrap());
        assert_eq!(bearer(&h), None);
        h.insert("authorization", "Bearer ".parse().unwrap());
        assert_eq!(bearer(&h), None);
    }
}
