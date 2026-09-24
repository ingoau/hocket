//! Pins (never evicted) and the evictable stream cache. Owner: core-server.
//!
//! Entry points:
//! - [`Downloads::new`] with `data_dir`/`cache_dir` from `CoreConfig`, the
//!   `Db`, a `Clock` and a [`StorageProbe`] (free-space source; the platform
//!   or `fs2`-less default).
//! - Pins: [`Downloads::pin`] / [`Downloads::unpin`] / [`Downloads::pins`]
//!   (→ `Vec<api::Pin>`), [`Downloads::plan`] (→ the track ids a pin still
//!   needs, as a `JobSpec` for `JobKind::Download`), [`DownloadRunner`] which
//!   fetches them through a [`SubsonicApi`] with bounded concurrency and files
//!   out-of-space as a loud `Problem`. [`Downloads::reconcile_playlist`] keeps
//!   a playlist pin in step when the playlist changes (enqueues fetches and
//!   removals). [`Downloads::set_gain`] stores gain computed at download time.
//! - Stream cache: [`Downloads::cache_put`] / [`cache_get`](Downloads::cache_get)
//!   / [`Downloads::enforce_cache_budget`] (LRU by bytes) /
//!   [`Downloads::clear_stream_cache`]. Separate directory, separate policy.
//! - [`Downloads::resolve`] → `api::MediaSource` for a track: downloaded file →
//!   cached file → stream URL with the transcoding profile chosen per network
//!   and per platform `cannot_decode` list.
//! - [`Downloads::storage_summary`] → `api::StorageSummary`.
//!
//! Layout: `<data_dir>/downloads/<server_id>/<track_id>.<suffix>` and
//! `<cache_dir>/stream/<server_id>/<track_id>[.<profile hash>].<suffix>`.

pub mod spans;
mod stream_cache;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub use stream_cache::{
    eviction_score, CacheRow, CacheSignal, EntryKey, EvictFacts, MergeOutcome, Traffic,
    TrafficTotals,
};

use futures::future::BoxFuture;
use parking_lot::RwLock;
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};

use crate::api::{
    self, MediaSource, NetworkKind, OfflineState, Pin, PinTarget, Platform, StorageSummary,
    TranscodingProfile,
};
use crate::db::{Db, DbError, DbResult};
use crate::jobs::{JobContext, JobError, JobResult, JobRunner, JobSpec, RetryAction};
use crate::subsonic::client::StreamOptions;
use crate::subsonic::{SubsonicApi, SubsonicError};
use crate::util::Clock;

/// Free-space source, injected so tests can simulate a full disk.
pub trait StorageProbe: Send + Sync + 'static {
    /// Free bytes on the volume holding `path`, if known.
    fn free_bytes(&self, path: &Path) -> Option<f64>;
}

/// Default probe: unknown free space (platform layers supply a real one).
pub struct UnknownStorage;
impl StorageProbe for UnknownStorage {
    fn free_bytes(&self, _: &Path) -> Option<f64> {
        None
    }
}

/// Simulated volume (tests): `capacity` minus the bytes of files under the
/// queried directory; `None` capacity means unknown.
pub struct FixedStorage(pub parking_lot::Mutex<Option<f64>>);
impl StorageProbe for FixedStorage {
    fn free_bytes(&self, path: &Path) -> Option<f64> {
        let capacity = (*self.0.lock())?;
        Some((capacity - dir_bytes(path)).max(0.0))
    }
}

fn dir_bytes(path: &Path) -> f64 {
    let Ok(rd) = std::fs::read_dir(path) else {
        return 0.0;
    };
    rd.flatten()
        .map(|e| {
            let p = e.path();
            if p.is_dir() {
                dir_bytes(&p)
            } else {
                e.metadata().map(|m| m.len() as f64).unwrap_or(0.0)
            }
        })
        .sum()
}

#[derive(Debug, thiserror::Error)]
pub enum DownloadError {
    #[error(transparent)]
    Db(#[from] DbError),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Server(#[from] SubsonicError),
    #[error("out of space: need {needed} bytes, {free} free")]
    OutOfSpace { needed: f64, free: f64 },
    #[error("unknown track {0}")]
    UnknownTrack(String),
    /// No pin wants the track any more (unpinned while the job ran).
    #[error("track {0} is no longer pinned")]
    NotPinned(String),
}

impl From<DownloadError> for JobError {
    fn from(e: DownloadError) -> Self {
        match e {
            DownloadError::Db(d) => JobError::Db(d),
            other => JobError::Failed(other.to_string()),
        }
    }
}

/// Minimum free space kept after a download (out-of-space threshold).
pub const MIN_FREE_BYTES: f64 = 200.0 * 1024.0 * 1024.0;
/// Default LRU budget for the stream cache.
pub const DEFAULT_CACHE_BUDGET: f64 = 2.0 * 1024.0 * 1024.0 * 1024.0;

/// Per-network transcoding choice. `None` profile = original.
#[derive(Debug, Clone, Default)]
pub struct TranscodingPolicy {
    /// Keyed by `NetworkState.network_id`; `None` key is the default for the kind.
    pub by_network_id: HashMap<String, TranscodingProfile>,
    pub by_kind: HashMap<NetworkKind, TranscodingProfile>,
    pub default: Option<TranscodingProfile>,
}

impl TranscodingPolicy {
    pub fn profile_for(&self, network: Option<&api::NetworkState>) -> Option<TranscodingProfile> {
        if let Some(n) = network {
            if let Some(id) = &n.network_id {
                if let Some(p) = self.by_network_id.get(id) {
                    return Some(p.clone());
                }
            }
            if let Some(p) = self.by_kind.get(&n.kind) {
                return Some(p.clone());
            }
        }
        self.default.clone()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DownloadJobPayload {
    pub server_id: String,
    pub target: PinTarget,
    pub transcode: bool,
}

fn kind_name(t: &PinTarget) -> &'static str {
    match t {
        PinTarget::Track { .. } => "track",
        PinTarget::Album { .. } => "album",
        PinTarget::Playlist { .. } => "playlist",
    }
}

fn target_id(t: &PinTarget) -> &str {
    match t {
        PinTarget::Track { id } | PinTarget::Album { id } | PinTarget::Playlist { id } => id,
    }
}

fn target_from(kind: &str, id: &str) -> PinTarget {
    match kind {
        "album" => PinTarget::Album { id: id.into() },
        "playlist" => PinTarget::Playlist { id: id.into() },
        _ => PinTarget::Track { id: id.into() },
    }
}

/// Which codecs a platform can't decode natively (design: desktop ≈ APE/DSD,
/// Android ExoPlayer has its own constraints).
pub fn platform_cannot_decode(platform: Platform) -> Vec<String> {
    match platform {
        Platform::Android => vec![
            "ape".into(),
            "dsf".into(),
            "dff".into(),
            "wv".into(),
            "tak".into(),
            "shn".into(),
        ],
        Platform::Linux | Platform::MacOs | Platform::Windows => vec![
            "ape".into(),
            "dsf".into(),
            "dff".into(),
            "tak".into(),
            "shn".into(),
        ],
        Platform::Coordinator => vec![],
    }
}

struct Inner {
    db: Db,
    clock: Arc<dyn Clock>,
    storage: Arc<dyn StorageProbe>,
    data_dir: PathBuf,
    cache_dir: PathBuf,
    platform: Platform,
    cache_budget: RwLock<f64>,
    warn_threshold: RwLock<Option<f64>>,
    policy: RwLock<TranscodingPolicy>,
    network: RwLock<Option<api::NetworkState>>,
    cache_use: parking_lot::Mutex<CacheUse>,
    traffic: Traffic,
    /// The traffic totals last persisted.
    traffic_saved: parking_lot::Mutex<Option<TrafficTotals>>,
}

/// `(server_id, track_id)`: a track whose offline state may have changed.
pub type TrackKey = (String, String);

/// Which stream-cache files are in use right now, and which removals wait
/// for them. A file is in use while a reader (the loopback proxy serving
/// it) holds it open, or while its track is loaded or preloaded in the
/// player ("protected"): eviction skips it and `ClearStreamCache` defers it.
#[derive(Default)]
struct CacheUse {
    readers: HashMap<PathBuf, usize>,
    protected: std::collections::HashSet<TrackKey>,
    /// Removals deferred until the file is no longer in use. The row is
    /// removed with the file only while it still points at `path`.
    doomed: Vec<Doomed>,
}

struct Doomed {
    server_id: String,
    track_id: String,
    profile: String,
    path: PathBuf,
}

impl CacheUse {
    fn in_use(&self, server_id: &str, track_id: &str, path: &Path) -> bool {
        self.readers.get(path).is_some_and(|n| *n > 0)
            || self
                .protected
                .contains(&(server_id.to_string(), track_id.to_string()))
    }
}

#[derive(Clone)]
pub struct Downloads {
    inner: Arc<Inner>,
}

impl Downloads {
    pub fn new(
        db: Db,
        clock: Arc<dyn Clock>,
        storage: Arc<dyn StorageProbe>,
        data_dir: &Path,
        cache_dir: &Path,
        platform: Platform,
    ) -> Self {
        // A previous process may have died mid-fetch: those rows would
        // otherwise never be re-planned, and a fetch waiting on the
        // (now nonexistent) writer would wait forever.
        if let Err(e) = db.with_conn(|c| {
            c.execute(
                "UPDATE pin_tracks SET state = 'wanted' WHERE state = 'downloading'",
                [],
            )?;
            Ok(())
        }) {
            tracing::warn!(error = %e, "resetting interrupted downloads");
        }
        Downloads {
            inner: Arc::new(Inner {
                db,
                clock,
                storage,
                data_dir: data_dir.to_path_buf(),
                cache_dir: cache_dir.to_path_buf(),
                platform,
                cache_budget: RwLock::new(DEFAULT_CACHE_BUDGET),
                warn_threshold: RwLock::new(None),
                policy: RwLock::new(TranscodingPolicy::default()),
                network: RwLock::new(None),
                cache_use: parking_lot::Mutex::new(CacheUse::default()),
                traffic: Traffic::default(),
                traffic_saved: parking_lot::Mutex::new(None),
            }),
        }
        .sweep_stream_cache()
        .reconcile_at_start()
    }

    /// Startup: drop temp files a previous process left mid-write, and
    /// files no cache row references (the rebuildable `cache_entries` table
    /// may have been dropped by a migration).
    fn sweep_stream_cache(self) -> Self {
        let root = self.inner.cache_dir.join("stream");
        // Unknown index (a database error): only temp files go.
        let known: Option<std::collections::HashSet<String>> = self
            .inner
            .db
            .with_conn(|c| {
                let mut st = c.prepare_cached("SELECT path FROM cache_entries")?;
                let rows = st.query_map([], |r| r.get::<_, String>(0))?;
                Ok(rows.collect::<Result<_, _>>()?)
            })
            .ok();
        let Ok(servers) = std::fs::read_dir(&root) else {
            return self;
        };
        for dir in servers.flatten().filter(|e| e.path().is_dir()) {
            let Ok(files) = std::fs::read_dir(dir.path()) else {
                continue;
            };
            for f in files.flatten() {
                let p = f.path();
                let part = p.extension().is_some_and(|e| e == "part");
                let orphan = known
                    .as_ref()
                    .is_some_and(|k| !k.contains(p.to_string_lossy().as_ref()));
                if p.is_file() && (part || orphan) {
                    if let Err(e) = std::fs::remove_file(&p) {
                        tracing::warn!(error = %e, "removing stale stream-cache file");
                    }
                }
            }
        }
        self
    }

    /// Startup: the sweep above, then rows whose files the OS removed (or
    /// truncated) behind our back go, and the traffic counters are loaded.
    fn reconcile_at_start(self) -> Self {
        if let Err(e) = self.reconcile_stream_cache() {
            tracing::warn!(error = %e, "reconciling the stream cache");
        }
        self.load_traffic();
        self
    }

    pub fn db(&self) -> &Db {
        &self.inner.db
    }

    pub fn downloads_dir(&self, server_id: &str) -> PathBuf {
        self.inner.data_dir.join("downloads").join(server_id)
    }

    pub fn stream_cache_dir(&self, server_id: &str) -> PathBuf {
        self.inner.cache_dir.join("stream").join(server_id)
    }

    pub fn set_cache_budget(&self, bytes: f64) {
        *self.inner.cache_budget.write() = bytes.max(0.0);
    }

    pub fn cache_budget(&self) -> f64 {
        *self.inner.cache_budget.read()
    }

    pub fn set_warn_threshold(&self, bytes: Option<f64>) {
        *self.inner.warn_threshold.write() = bytes;
    }

    pub fn set_transcoding_policy(&self, policy: TranscodingPolicy) {
        *self.inner.policy.write() = policy;
    }

    /// `Command::SetTranscodingProfile { network_id, profile }`.
    pub fn set_transcoding_profile(&self, network_id: Option<String>, profile: TranscodingProfile) {
        let mut p = self.inner.policy.write();
        match network_id {
            Some(id) => {
                p.by_network_id.insert(id, profile);
            }
            None => p.default = Some(profile),
        }
    }

    pub fn set_network(&self, network: Option<api::NetworkState>) {
        *self.inner.network.write() = network;
    }

    fn now(&self) -> f64 {
        self.inner.clock.now_ms()
    }

    // -- pins ---------------------------------------------------------------

    /// Create (or re-affirm) a pin and materialise its wanted tracks. Returns
    /// the `JobSpec` to submit for fetching, or `None` when nothing is missing.
    pub fn pin(
        &self,
        server_id: &str,
        target: &PinTarget,
        transcode: bool,
    ) -> Result<Option<JobSpec>, DownloadError> {
        let (label, cover) = self.describe(target)?;
        let profile = if transcode {
            self.profile_for_downloads()
                .map(|p| serde_json::to_string(&p).unwrap_or_default())
        } else {
            None
        };
        let now = self.now();
        self.inner.db.with_tx(|tx| {
            tx.execute(
                "INSERT INTO pins(server_id, target_kind, target_id, label, cover_art, created_at, transcoded, profile) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
                 ON CONFLICT(server_id, target_kind, target_id) DO UPDATE SET label = excluded.label, cover_art = excluded.cover_art",
                params![server_id, kind_name(target), target_id(target), label, cover, now, transcode as i64, profile],
            )?;
            Ok(())
        })?;
        self.materialise(server_id, target)?;
        self.plan(server_id, target, transcode)
    }

    /// Remove a pin and delete files no other pin needs.
    pub fn unpin(&self, server_id: &str, target: &PinTarget) -> Result<usize, DownloadError> {
        let kind = kind_name(target);
        let id = target_id(target);
        let orphans: Vec<(String, Option<String>)> = self.inner.db.with_tx(|tx| {
            tx.execute("DELETE FROM pins WHERE server_id = ?1 AND target_kind = ?2 AND target_id = ?3", params![server_id, kind, id])?;
            // Another pin that merely *wants* one of these tracks inherits the
            // finished file instead of re-downloading it later (L3).
            satisfy_wanted_from_done(tx, server_id, None)?;
            let mut st = tx.prepare_cached(
                "SELECT track_id, path FROM pin_tracks WHERE server_id = ?1 AND target_kind = ?2 AND target_id = ?3 AND track_id NOT IN (SELECT track_id FROM pin_tracks WHERE server_id = ?1 AND NOT (target_kind = ?2 AND target_id = ?3))",
            )?;
            let rows = st.query_map(params![server_id, kind, id], |r| Ok((r.get(0)?, r.get(1)?)))?;
            let orphans = rows.collect::<Result<Vec<_>, _>>()?;
            tx.execute("DELETE FROM pin_tracks WHERE server_id = ?1 AND target_kind = ?2 AND target_id = ?3", params![server_id, kind, id])?;
            Ok(orphans)
        })?;
        let mut removed = 0;
        let mut errors = 0;
        for (track_id, path) in orphans {
            if let Some(p) = path {
                match remove_if_exists(&p) {
                    Ok(true) => removed += 1,
                    Ok(false) => {}
                    Err(e) => {
                        errors += 1;
                        tracing::warn!(error = %e, path = %p, "removing unpinned file");
                    }
                }
            }
            self.refresh_offline(server_id, &track_id)?;
        }
        if errors > 0 {
            tracing::warn!(errors, "unpin left files behind");
        }
        Ok(removed)
    }

    pub fn pins(&self, server_id: &str) -> DbResult<Vec<Pin>> {
        self.inner.db.with_conn(|c| {
            let mut st = c.prepare_cached(
                "SELECT p.target_kind, p.target_id, p.label, p.cover_art, p.created_at, p.transcoded,
                        (SELECT count(*) FROM pin_tracks t WHERE t.server_id = p.server_id AND t.target_kind = p.target_kind AND t.target_id = p.target_id),
                        (SELECT count(*) FROM pin_tracks t WHERE t.server_id = p.server_id AND t.target_kind = p.target_kind AND t.target_id = p.target_id AND t.state = 'done'),
                        (SELECT COALESCE(SUM(bytes), 0) FROM pin_tracks t WHERE t.server_id = p.server_id AND t.target_kind = p.target_kind AND t.target_id = p.target_id AND t.state = 'done')
                 FROM pins p WHERE p.server_id = ?1 ORDER BY p.created_at DESC",
            )?;
            let rows = st.query_map([server_id], |r| {
                let kind: String = r.get(0)?;
                let id: String = r.get(1)?;
                Ok(Pin {
                    target: target_from(&kind, &id),
                    label: r.get(2)?,
                    cover_art: r.get(3)?,
                    created_at: r.get(4)?,
                    transcoded: r.get::<_, i64>(5)? != 0,
                    track_count: r.get::<_, i64>(6)?.max(0) as u32,
                    downloaded_count: r.get::<_, i64>(7)?.max(0) as u32,
                    bytes: r.get::<_, f64>(8)?,
                })
            })?;
            Ok(rows.collect::<Result<Vec<_>, _>>()?)
        })
    }

    pub fn is_pinned(&self, server_id: &str, target: &PinTarget) -> DbResult<bool> {
        self.inner.db.with_conn(|c| {
            let n: i64 = c.query_row(
                "SELECT count(*) FROM pins WHERE server_id = ?1 AND target_kind = ?2 AND target_id = ?3",
                params![server_id, kind_name(target), target_id(target)],
                |r| r.get(0),
            )?;
            Ok(n > 0)
        })
    }

    fn describe(&self, target: &PinTarget) -> Result<(String, Option<String>), DownloadError> {
        Ok(match target {
            PinTarget::Track { id } => {
                let t = self
                    .inner
                    .db
                    .track(id)?
                    .ok_or_else(|| DownloadError::UnknownTrack(id.clone()))?;
                (t.title, t.cover_art)
            }
            PinTarget::Album { id } => match self.inner.db.album(id)? {
                Some(a) => (a.name, a.cover_art),
                None => (id.clone(), None),
            },
            PinTarget::Playlist { id } => match self.inner.db.playlist(id)? {
                Some(p) => (p.name, p.cover_art),
                None => (id.clone(), None),
            },
        })
    }

    /// The tracks a target currently resolves to (from the mirror).
    fn member_ids(&self, target: &PinTarget) -> DbResult<Vec<String>> {
        Ok(match target {
            PinTarget::Track { id } => vec![id.clone()],
            PinTarget::Album { id } => self
                .inner
                .db
                .album_tracks(id)?
                .into_iter()
                .map(|t| t.id)
                .collect(),
            PinTarget::Playlist { id } => self.inner.db.playlist_track_ids(id)?,
        })
    }

    /// Insert `wanted` rows for members not yet tracked. For album/track pins
    /// the set is fixed at pin time; playlists re-run this on change.
    fn materialise(&self, server_id: &str, target: &PinTarget) -> Result<(), DownloadError> {
        let members = self.member_ids(target)?;
        let kind = kind_name(target);
        let id = target_id(target);
        self.inner.db.with_tx(|tx| {
            let mut st = tx.prepare_cached(
                "INSERT OR IGNORE INTO pin_tracks(server_id, target_kind, target_id, track_id, state) VALUES (?1, ?2, ?3, ?4, 'wanted')",
            )?;
            for m in &members {
                st.execute(params![server_id, kind, id, m])?;
            }
            // A file another pin already fetched satisfies this one too.
            satisfy_wanted_from_done(tx, server_id, Some((kind, id)))?;
            Ok(())
        })?;
        Ok(())
    }

    /// Job spec fetching whatever the pin still needs.
    pub fn plan(
        &self,
        server_id: &str,
        target: &PinTarget,
        transcode: bool,
    ) -> Result<Option<JobSpec>, DownloadError> {
        let missing: Vec<String> = self.inner.db.with_conn(|c| {
            let mut st = c.prepare_cached(
                "SELECT track_id FROM pin_tracks WHERE server_id = ?1 AND target_kind = ?2 AND target_id = ?3 AND state IN ('wanted','failed') ORDER BY rowid",
            )?;
            let rows = st.query_map(params![server_id, kind_name(target), target_id(target)], |r| r.get::<_, String>(0))?;
            Ok(rows.collect::<Result<Vec<_>, _>>()?)
        })?;
        if missing.is_empty() {
            return Ok(None);
        }
        for m in &missing {
            self.inner
                .db
                .set_track_offline(m, OfflineState::Downloading)?;
        }
        let (label, _) = self.describe(target)?;
        let payload = serde_json::to_string(&DownloadJobPayload {
            server_id: server_id.into(),
            target: target.clone(),
            transcode,
        })
        .map_err(DbError::from)?;
        Ok(Some(
            JobSpec::new(api::JobKind::Download, format!("Download {label}"))
                .payload(payload)
                .items(missing),
        ))
    }

    /// A pinned playlist changed: add new members, drop removed ones (deleting
    /// files no other pin needs). Returns a fetch job for additions, if any.
    pub fn reconcile_playlist(
        &self,
        server_id: &str,
        playlist_id: &str,
    ) -> Result<Option<JobSpec>, DownloadError> {
        let target = PinTarget::Playlist {
            id: playlist_id.into(),
        };
        if !self.is_pinned(server_id, &target)? {
            return Ok(None);
        }
        let members = self.member_ids(&target)?;
        let tracked: Vec<(String, Option<String>)> = self.inner.db.with_conn(|c| {
            let mut st = c.prepare_cached("SELECT track_id, path FROM pin_tracks WHERE server_id = ?1 AND target_kind = 'playlist' AND target_id = ?2")?;
            let rows = st.query_map(params![server_id, playlist_id], |r| Ok((r.get(0)?, r.get(1)?)))?;
            Ok(rows.collect::<Result<Vec<_>, _>>()?)
        })?;
        let transcoded: bool = self.inner.db.with_conn(|c| {
            Ok(c.query_row(
                "SELECT transcoded FROM pins WHERE server_id = ?1 AND target_kind = 'playlist' AND target_id = ?2",
                params![server_id, playlist_id],
                |r| r.get::<_, i64>(0),
            )
            .optional()?
            .unwrap_or(0)
                != 0)
        })?;
        for (track_id, _) in tracked.iter().filter(|(t, _)| !members.contains(t)) {
            let still_needed: bool = self.inner.db.with_tx(|tx| {
                tx.execute(
                    "DELETE FROM pin_tracks WHERE server_id = ?1 AND target_kind = 'playlist' AND target_id = ?2 AND track_id = ?3",
                    params![server_id, playlist_id, track_id],
                )?;
                let n: i64 = tx.query_row("SELECT count(*) FROM pin_tracks WHERE server_id = ?1 AND track_id = ?2 AND state = 'done'", params![server_id, track_id], |r| r.get(0))?;
                Ok(n > 0)
            })?;
            if !still_needed {
                if let Some(p) = tracked
                    .iter()
                    .find(|(t, _)| t == track_id)
                    .and_then(|(_, p)| p.clone())
                {
                    if let Err(e) = remove_if_exists(&p) {
                        tracing::warn!(error = %e, path = %p, "removing dropped playlist track");
                    }
                }
            }
            self.refresh_offline(server_id, track_id)?;
        }
        self.materialise(server_id, &target)?;
        self.plan(server_id, &target, transcoded)
    }

    /// Gain computed at download time (from the audio module's tag reader),
    /// stored on every pin row for the track.
    pub fn set_gain(
        &self,
        server_id: &str,
        track_id: &str,
        gain_db: Option<f64>,
        peak: Option<f64>,
    ) -> DbResult<bool> {
        self.inner.db.with_conn(|c| {
            Ok(c.execute(
                "UPDATE pin_tracks SET gain_db = ?3, peak = ?4 WHERE server_id = ?1 AND track_id = ?2",
                params![server_id, track_id, gain_db, peak],
            )? > 0)
        })
    }

    /// Stored download gain for a track, if any.
    pub fn gain(&self, server_id: &str, track_id: &str) -> DbResult<Option<f64>> {
        self.inner.db.with_conn(|c| {
            Ok(c.query_row(
                "SELECT gain_db FROM pin_tracks WHERE server_id = ?1 AND track_id = ?2 AND gain_db IS NOT NULL LIMIT 1",
                params![server_id, track_id],
                |r| r.get::<_, Option<f64>>(0),
            )
            .optional()?
            .flatten())
        })
    }

    /// Path of a completed download for the track, if the file exists.
    pub fn downloaded_path(&self, server_id: &str, track_id: &str) -> DbResult<Option<PathBuf>> {
        let p: Option<String> = self.inner.db.with_conn(|c| {
            Ok(c.query_row(
                "SELECT path FROM pin_tracks WHERE server_id = ?1 AND track_id = ?2 AND state = 'done' AND path IS NOT NULL LIMIT 1",
                params![server_id, track_id],
                |r| r.get(0),
            )
            .optional()?)
        })?;
        Ok(p.map(PathBuf::from).filter(|p| p.exists()))
    }

    /// Recompute `tracks.offline` from pins and cache.
    pub fn refresh_offline(&self, server_id: &str, track_id: &str) -> DbResult<()> {
        let state = self.inner.db.with_conn(|c| {
            let done: i64 = c.query_row("SELECT count(*) FROM pin_tracks WHERE server_id = ?1 AND track_id = ?2 AND state = 'done'", params![server_id, track_id], |r| r.get(0))?;
            if done > 0 {
                return Ok(OfflineState::Downloaded);
            }
            let wanted: i64 = c.query_row("SELECT count(*) FROM pin_tracks WHERE server_id = ?1 AND track_id = ?2 AND state IN ('wanted','downloading')", params![server_id, track_id], |r| r.get(0))?;
            if wanted > 0 {
                return Ok(OfflineState::Downloading);
            }
            let cached: i64 = c.query_row("SELECT count(*) FROM cache_entries WHERE server_id = ?1 AND track_id = ?2 AND complete = 1", params![server_id, track_id], |r| r.get(0))?;
            Ok(if cached > 0 { OfflineState::Cached } else { OfflineState::None })
        })?;
        self.inner.db.set_track_offline(track_id, state)?;
        Ok(())
    }

    /// Where a download for this track goes. Both halves of the file name
    /// are sanitised: the id through [`safe_name`], the server-supplied
    /// suffix through [`safe_ext`] (a hostile suffix cannot leave the
    /// downloads directory).
    pub fn download_path(&self, server_id: &str, track_id: &str, suffix: Option<&str>) -> PathBuf {
        self.downloads_dir(server_id)
            .join(format!("{}.{}", safe_name(track_id), safe_ext(suffix)))
    }

    fn profile_for_downloads(&self) -> Option<TranscodingProfile> {
        self.inner.policy.read().default.clone()
    }

    /// Fetch one track for a pin. Checks free space first; records the
    /// result on every pin row for the track.
    ///
    /// Exactly one fetch runs per `(server, track)` at a time: the caller
    /// claims the track by flipping its rows `wanted`/`failed` →
    /// `downloading` (compare-and-set). When another job holds the claim
    /// this call waits for it and returns its outcome, so two pins sharing
    /// a track never write the same file concurrently.
    pub async fn fetch_track(
        &self,
        api: &dyn SubsonicApi,
        server_id: &str,
        track_id: &str,
        transcode: bool,
    ) -> Result<u64, DownloadError> {
        let track = self
            .inner
            .db
            .track(track_id)?
            .ok_or_else(|| DownloadError::UnknownTrack(track_id.into()))?;
        let profile = if transcode {
            self.profile_for_downloads()
                .filter(|p| p.format.is_some() || p.max_bit_rate.is_some())
        } else {
            None
        };
        let needed = track.size_bytes.unwrap_or(0.0);
        let dir = self.downloads_dir(server_id);
        std::fs::create_dir_all(&dir)?;
        if let Some(free) = self.inner.storage.free_bytes(&dir) {
            if free - needed < MIN_FREE_BYTES {
                return Err(DownloadError::OutOfSpace { needed, free });
            }
        }
        let (url, suffix) = match &profile {
            Some(p) => (
                api.stream_url(
                    track_id,
                    &StreamOptions {
                        format: p.format.clone(),
                        max_bit_rate: p.max_bit_rate,
                        ..Default::default()
                    },
                ),
                p.format.clone().or(track.suffix.clone()),
            ),
            None => (api.download_url(track_id), track.suffix.clone()),
        };
        let dest = self.download_path(server_id, track_id, suffix.as_deref());
        debug_assert!(dest.starts_with(self.downloads_dir(server_id)));
        loop {
            if self.claim(server_id, track_id)? {
                break;
            }
            match self.track_state(server_id, track_id)? {
                None => return Err(DownloadError::NotPinned(track_id.into())),
                Some((state, bytes)) if state == "done" => return Ok(bytes.max(0.0) as u64),
                Some((state, _)) if state == "downloading" => {
                    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                }
                Some(_) => {} // wanted/failed again: retry the claim
            }
        }
        // From here on this call owns the fetch. The transport only ever
        // creates `dest` through a completed rename, so on failure there is
        // nothing of ours to remove (a file already there belongs to an
        // earlier complete download and is left alone).
        let result = api.download_to_file(url, &dest).await;
        match result {
            Ok(out) => {
                let rg = track.replay_gain.as_ref();
                self.inner.db.with_conn(|c| {
                    c.execute(
                        "UPDATE pin_tracks SET state = 'done', path = ?3, bytes = ?4, suffix = ?5, content_type = ?6, gain_db = COALESCE(gain_db, ?7), peak = COALESCE(peak, ?8), downloaded_at = ?9, error = NULL WHERE server_id = ?1 AND track_id = ?2",
                        params![
                            server_id,
                            track_id,
                            dest.to_string_lossy().as_ref(),
                            out.bytes as f64,
                            suffix,
                            out.content_type,
                            rg.and_then(|g| g.track_gain_db),
                            rg.and_then(|g| g.track_peak),
                            self.now()
                        ],
                    )?;
                    Ok(())
                })?;
                self.refresh_offline(server_id, track_id)?;
                Ok(out.bytes)
            }
            Err(e) => {
                self.mark_state(
                    server_id,
                    track_id,
                    "failed",
                    Some(&e.to_string()),
                    0.0,
                    None,
                )?;
                self.refresh_offline(server_id, track_id)?;
                Err(e.into())
            }
        }
    }

    /// Compare-and-set claim of every `wanted`/`failed` row for the track.
    /// `false` when nothing was claimable (another fetch holds it, it is
    /// done, or it is no longer pinned).
    fn claim(&self, server_id: &str, track_id: &str) -> DbResult<bool> {
        self.inner.db.with_conn(|c| {
            Ok(c.execute(
                "UPDATE pin_tracks SET state = 'downloading', error = NULL WHERE server_id = ?1 AND track_id = ?2 AND state IN ('wanted','failed')",
                params![server_id, track_id],
            )? > 0)
        })
    }

    /// The track's effective state across its pin rows (`downloading` wins
    /// over `done` over the rest) with the stored byte count.
    fn track_state(&self, server_id: &str, track_id: &str) -> DbResult<Option<(String, f64)>> {
        self.inner.db.with_conn(|c| {
            Ok(c.query_row(
                "SELECT state, bytes FROM pin_tracks WHERE server_id = ?1 AND track_id = ?2
                 ORDER BY CASE state WHEN 'downloading' THEN 0 WHEN 'done' THEN 1 ELSE 2 END LIMIT 1",
                params![server_id, track_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?)
        })
    }

    fn mark_state(
        &self,
        server_id: &str,
        track_id: &str,
        state: &str,
        error: Option<&str>,
        bytes: f64,
        path: Option<&str>,
    ) -> DbResult<()> {
        self.inner.db.with_conn(|c| {
            c.execute(
                "UPDATE pin_tracks SET state = ?3, error = ?4, bytes = CASE WHEN ?5 > 0 THEN ?5 ELSE bytes END, path = COALESCE(?6, path) WHERE server_id = ?1 AND track_id = ?2",
                params![server_id, track_id, state, error, bytes, path],
            )?;
            Ok(())
        })
    }

    // -- stream cache -------------------------------------------------------

    /// Path a cached stream for (track, profile) lives at.
    pub fn cache_path(
        &self,
        server_id: &str,
        track_id: &str,
        profile: Option<&TranscodingProfile>,
        suffix: Option<&str>,
    ) -> PathBuf {
        let ext = safe_ext(suffix);
        let key = profile_key(profile);
        let name = if key.is_empty() {
            format!("{}.{}", safe_name(track_id), ext)
        } else {
            format!("{}.{}.{}", safe_name(track_id), key, ext)
        };
        self.stream_cache_dir(server_id).join(name)
    }

    /// A fresh, never-reused path for a new cache file of (track, profile):
    /// [`cache_path`](Self::cache_path) with a random component, so a new
    /// copy never replaces a file a reader still has open (and a deferred
    /// removal of the old copy can never hit the new one).
    pub fn cache_path_unique(
        &self,
        server_id: &str,
        track_id: &str,
        profile: Option<&TranscodingProfile>,
        suffix: Option<&str>,
    ) -> PathBuf {
        let base = self.cache_path(server_id, track_id, profile, suffix);
        let ext = safe_ext(suffix);
        let stem = base
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let stem = stem.strip_suffix(&format!(".{ext}")).unwrap_or(&stem);
        let id = crate::util::new_id();
        base.with_file_name(format!("{stem}.{}.{ext}", &id[..12]))
    }

    /// Whether the cache volume has room for `bytes` more (plus the
    /// out-of-space margin). Unknown free space counts as room.
    pub fn cache_has_room(&self, server_id: &str, bytes: f64) -> bool {
        let dir = self.stream_cache_dir(server_id);
        let probe = if dir.exists() {
            dir
        } else {
            self.inner.cache_dir.clone()
        };
        match self.inner.storage.free_bytes(&probe) {
            Some(free) => free - bytes >= MIN_FREE_BYTES,
            None => true,
        }
    }

    /// Register a fully written cache file and enforce the budget. A copy
    /// it replaces is removed (or, while in use, once it is released).
    /// Returns the tracks whose offline state changed (the new entry and
    /// anything evicted to make room).
    pub fn cache_put(
        &self,
        server_id: &str,
        track_id: &str,
        profile: Option<&TranscodingProfile>,
        path: &Path,
        bytes: f64,
        content_type: Option<&str>,
    ) -> DbResult<Vec<TrackKey>> {
        let now = self.now();
        let key = profile_key(profile);
        let new_path = path.to_string_lossy().into_owned();
        let tag = self.source_tag(server_id, track_id)?;
        let old: Option<String> = self.inner.db.with_tx(|tx| {
            let old: Option<String> = tx
                .query_row(
                    "SELECT path FROM cache_entries WHERE server_id = ?1 AND track_id = ?2 AND profile = ?3",
                    params![server_id, track_id, key],
                    |r| r.get(0),
                )
                .optional()?;
            tx.execute(
                "INSERT INTO cache_entries(server_id, track_id, path, bytes, content_type, profile, created_at, last_used_at, complete, total_bytes, spans, source_tag) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7, 1, ?4, ?8, ?9)
                 ON CONFLICT(server_id, track_id, profile) DO UPDATE SET path = excluded.path, bytes = excluded.bytes, content_type = excluded.content_type, last_used_at = excluded.last_used_at, complete = 1, total_bytes = excluded.total_bytes, spans = excluded.spans, source_tag = excluded.source_tag",
                params![server_id, track_id, new_path, bytes, content_type, key, now, spans::SpanSet::of(0, bytes.max(0.0) as u64).encode(), tag],
            )?;
            Ok(old)
        })?;
        {
            // A deferred removal only deletes the row while it still points
            // at the doomed file, so the new copy survives one of the old.
            let mut u = self.inner.cache_use.lock();
            if let Some(old) = old.filter(|o| *o != new_path) {
                let old = PathBuf::from(old);
                if u.in_use(server_id, track_id, &old) {
                    u.doomed.push(Doomed {
                        server_id: server_id.into(),
                        track_id: track_id.into(),
                        profile: key.clone(),
                        path: old,
                    });
                } else if let Err(e) = remove_if_exists(&old.to_string_lossy()) {
                    tracing::warn!(error = %e, "removing replaced cache file");
                }
            }
        }
        self.refresh_offline(server_id, track_id)?;
        let mut changed = vec![(server_id.to_string(), track_id.to_string())];
        changed.extend(self.evict_over_budget()?);
        Ok(changed)
    }

    /// Cached file for the track (any profile, preferring the requested one);
    /// touches its LRU stamp.
    pub fn cache_get(
        &self,
        server_id: &str,
        track_id: &str,
        profile: Option<&TranscodingProfile>,
    ) -> DbResult<Option<(PathBuf, Option<String>)>> {
        Ok(self
            .cache_lookup(server_id, track_id, profile, true)?
            .map(|e| (e.path, e.content_type)))
    }

    /// The complete entry that serves (track, profile) — see
    /// [`cache_complete`](Self::cache_complete) — with its size. `touch`
    /// stamps it as used. A file the OS removed or truncated is a miss.
    pub fn cache_lookup(
        &self,
        server_id: &str,
        track_id: &str,
        profile: Option<&TranscodingProfile>,
        touch: bool,
    ) -> DbResult<Option<CacheEntry>> {
        let (row, _) = self.cache_complete(server_id, track_id, profile, touch)?;
        Ok(row.map(|r| CacheEntry {
            bytes: r.total.unwrap_or(r.bytes),
            path: r.path,
            content_type: r.content_type,
            profile: r.profile,
        }))
    }

    pub fn cache_bytes(&self) -> DbResult<f64> {
        self.inner.db.with_conn(|c| {
            Ok(c.query_row(
                "SELECT COALESCE(SUM(bytes), 0) FROM cache_entries",
                [],
                |r| r.get(0),
            )?)
        })
    }

    /// Evict cache entries (see [`eviction_score`]) until under budget.
    /// Never touches downloads, and skips files in use. Returns the number
    /// of files removed.
    pub fn enforce_cache_budget(&self) -> DbResult<usize> {
        Ok(self.evict_over_budget()?.len())
    }

    /// `Command::ClearStreamCache`. Files in use are removed once released
    /// (see [`release_reader`](Self::release_reader) and
    /// [`set_protected`](Self::set_protected)). Returns the number of files
    /// removed now.
    pub fn clear_stream_cache(&self) -> DbResult<usize> {
        let entries: Vec<(String, String, String, String)> = self.inner.db.with_conn(|c| {
            let mut st =
                c.prepare_cached("SELECT server_id, track_id, profile, path FROM cache_entries")?;
            let rows = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?;
            Ok(rows.collect::<Result<Vec<_>, _>>()?)
        })?;
        let mut n = 0;
        for (sid, tid, prof, path) in entries {
            let p = PathBuf::from(&path);
            {
                let mut u = self.inner.cache_use.lock();
                if u.in_use(&sid, &tid, &p) {
                    let already = u.doomed.iter().any(|d| d.path == p);
                    if !already {
                        u.doomed.push(Doomed {
                            server_id: sid,
                            track_id: tid,
                            profile: prof,
                            path: p,
                        });
                    }
                    continue;
                }
            }
            match remove_if_exists(&path) {
                Ok(true) => n += 1,
                Ok(false) => {}
                Err(e) => {
                    tracing::warn!(error = %e, path, "clearing cache file");
                    continue;
                }
            }
            self.inner.db.with_conn(|c| {
                c.execute(
                    "DELETE FROM cache_entries WHERE server_id = ?1 AND track_id = ?2 AND profile = ?3",
                    params![sid, tid, prof],
                )?;
                Ok(())
            })?;
            self.refresh_offline(&sid, &tid)?;
        }
        Ok(n)
    }

    /// A reader (the loopback proxy) opened a cache file: it is not evicted
    /// or removed until [`release_reader`](Self::release_reader).
    pub fn acquire_reader(&self, path: &Path) {
        *self
            .inner
            .cache_use
            .lock()
            .readers
            .entry(path.to_path_buf())
            .or_insert(0) += 1;
    }

    /// The reader of `path` is done. Runs deferred removals and the budget;
    /// returns the tracks whose offline state changed.
    pub fn release_reader(&self, path: &Path) -> Vec<TrackKey> {
        {
            let mut u = self.inner.cache_use.lock();
            if let Some(n) = u.readers.get_mut(path) {
                *n = n.saturating_sub(1);
                if *n == 0 {
                    u.readers.remove(path);
                }
            }
        }
        self.reap()
    }

    /// The tracks the player has loaded or preloaded: their cache files are
    /// kept (a seek re-reads them) until they leave this set. Returns the
    /// tracks whose offline state changed from deferred removals.
    pub fn set_protected(&self, tracks: Vec<TrackKey>) -> Vec<TrackKey> {
        {
            let mut u = self.inner.cache_use.lock();
            let next: std::collections::HashSet<TrackKey> = tracks.into_iter().collect();
            if next == u.protected {
                return vec![];
            }
            u.protected = next;
        }
        self.reap()
    }

    /// Carry out deferred removals whose files are no longer in use, then
    /// the budget (entries skipped while in use may now go).
    fn reap(&self) -> Vec<TrackKey> {
        let ready: Vec<Doomed> = {
            let mut u = self.inner.cache_use.lock();
            let (ready, keep): (Vec<Doomed>, Vec<Doomed>) = std::mem::take(&mut u.doomed)
                .into_iter()
                .partition(|d| !u.in_use(&d.server_id, &d.track_id, &d.path));
            u.doomed = keep;
            ready
        };
        let mut changed = vec![];
        for d in ready {
            if let Err(e) = remove_if_exists(&d.path.to_string_lossy()) {
                tracing::warn!(error = %e, "removing released cache file");
            }
            let r = self.inner.db.with_conn(|c| {
                c.execute(
                    "DELETE FROM cache_entries WHERE server_id = ?1 AND track_id = ?2 AND profile = ?3 AND path = ?4",
                    params![d.server_id, d.track_id, d.profile, d.path.to_string_lossy().as_ref()],
                )?;
                Ok(())
            });
            if let Err(e) = r.and_then(|()| self.refresh_offline(&d.server_id, &d.track_id)) {
                tracing::warn!(error = %e, "deferred cache removal");
            }
            changed.push((d.server_id, d.track_id));
        }
        match self.evict_over_budget() {
            Ok(evicted) => changed.extend(evicted),
            Err(e) => tracing::warn!(error = %e, "cache budget"),
        }
        changed
    }

    // -- resolution ---------------------------------------------------------

    /// Effective transcoding profile for streaming now: the per-network
    /// profile, with the platform's `cannot_decode` list merged in, forced
    /// to a transcode when the track's container can't be decoded.
    pub fn effective_profile(&self, suffix: Option<&str>) -> Option<TranscodingProfile> {
        let network = self.inner.network.read().clone();
        let mut profile = self
            .inner
            .policy
            .read()
            .profile_for(network.as_ref())
            .unwrap_or(TranscodingProfile {
                format: None,
                max_bit_rate: None,
                cannot_decode: vec![],
            });
        for c in platform_cannot_decode(self.inner.platform) {
            if !profile
                .cannot_decode
                .iter()
                .any(|x| x.eq_ignore_ascii_case(&c))
            {
                profile.cannot_decode.push(c);
            }
        }
        let undecodable = suffix.is_some_and(|s| {
            profile
                .cannot_decode
                .iter()
                .any(|c| c.eq_ignore_ascii_case(s))
        });
        if undecodable && profile.format.is_none() {
            profile.format = Some(default_transcode_format(self.inner.platform).into());
        }
        if profile.format.is_none() && profile.max_bit_rate.is_none() && !undecodable {
            return None;
        }
        Some(profile)
    }

    /// Resolve how to play a track without the loopback proxy: downloaded
    /// file → cached file → stream URL (credentials in its query). The core
    /// uses [`resolve_with`](Self::resolve_with) and a proxy.
    pub fn resolve(
        &self,
        api: &dyn SubsonicApi,
        key: &str,
        track: &api::Track,
    ) -> DbResult<MediaSource> {
        self.resolve_with(api, key, track, None)
    }

    /// Resolve how to play a track: a completed download is a `file://`
    /// URL; anything else goes through `proxy` (which serves a complete
    /// stream-cache copy from disk or fetches from the server, caching a
    /// complete read). Without a proxy (or when it declines) this falls back
    /// to a cached `file://` or the server's stream URL.
    pub fn resolve_with(
        &self,
        api: &dyn SubsonicApi,
        key: &str,
        track: &api::Track,
        proxy: Option<&dyn StreamMinter>,
    ) -> DbResult<MediaSource> {
        let sid = &track.server_id;
        let summary = crate::subsonic::convert::summary_of(track);
        let gain_db = self.gain(sid, &track.id)?.unwrap_or(0.0);
        if let Some(p) = self.downloaded_path(sid, &track.id)? {
            let ct = self.inner.db.with_conn(|c| {
                Ok(c.query_row("SELECT content_type FROM pin_tracks WHERE server_id = ?1 AND track_id = ?2 AND state = 'done' LIMIT 1", params![sid, track.id], |r| r.get::<_, Option<String>>(0)).optional()?.flatten())
            })?;
            return Ok(MediaSource {
                key: key.into(),
                track: summary,
                url: file_url(&p),
                headers: HashMap::new(),
                mime_type: ct.or(track.content_type.clone()),
                gain_db,
                transcoded: false,
            });
        }
        let profile = self.effective_profile(track.suffix.as_deref());
        let transcoded = profile
            .as_ref()
            .is_some_and(|p| p.format.is_some() || p.max_bit_rate.is_some());
        let mime = if transcoded {
            profile
                .as_ref()
                .and_then(|p| p.format.as_deref())
                .map(mime_for)
        } else {
            track.content_type.clone()
        };
        if let Some(proxy) = proxy {
            let cached_type = self
                .cache_lookup(sid, &track.id, profile.as_ref(), false)?
                .and_then(|e| e.content_type);
            let suffix = if transcoded {
                profile.as_ref().and_then(|p| p.format.clone())
            } else {
                track.suffix.clone()
            };
            let mime_type = cached_type.or(mime.clone());
            if let Some(url) = proxy.mint(StreamTarget {
                server_id: sid.clone(),
                track_id: track.id.clone(),
                profile: profile.clone(),
                suffix,
                mime_type: mime_type.clone(),
            }) {
                return Ok(MediaSource {
                    key: key.into(),
                    track: summary,
                    url,
                    headers: HashMap::new(),
                    mime_type,
                    gain_db,
                    transcoded,
                });
            }
        }
        if let Some((p, ct)) = self.cache_get(sid, &track.id, profile.as_ref())? {
            return Ok(MediaSource {
                key: key.into(),
                track: summary,
                url: file_url(&p),
                headers: HashMap::new(),
                mime_type: ct.or(track.content_type.clone()),
                gain_db,
                transcoded: profile.is_some(),
            });
        }
        let opts = stream_options(profile.as_ref());
        Ok(MediaSource {
            key: key.into(),
            track: summary,
            url: api.stream_url(&track.id, &opts).to_string(),
            headers: HashMap::new(),
            mime_type: mime,
            gain_db,
            transcoded,
        })
    }

    // -- storage ------------------------------------------------------------

    pub fn downloads_bytes(&self) -> DbResult<f64> {
        self.inner.db.with_conn(|c| {
            Ok(c.query_row(
                "SELECT COALESCE(SUM(b), 0) FROM (SELECT MAX(bytes) AS b FROM pin_tracks WHERE state = 'done' GROUP BY server_id, track_id)",
                [],
                |r| r.get(0),
            )?)
        })
    }

    /// Storage summary (images_bytes is filled in by the image cache).
    pub fn storage_summary(&self, images_bytes: f64) -> DbResult<StorageSummary> {
        Ok(StorageSummary {
            downloads_bytes: self.downloads_bytes()?,
            cache_bytes: self.cache_bytes()?,
            images_bytes,
            warn_threshold_bytes: *self.inner.warn_threshold.read(),
            free_bytes: self.inner.storage.free_bytes(&self.inner.data_dir),
        })
    }

    /// True when downloads exceed the configured warn threshold.
    pub fn over_warn_threshold(&self) -> DbResult<bool> {
        let Some(t) = *self.inner.warn_threshold.read() else {
            return Ok(false);
        };
        Ok(self.downloads_bytes()? > t)
    }
}

/// `stream` options for an effective profile (`None` = original).
pub fn stream_options(profile: Option<&TranscodingProfile>) -> StreamOptions {
    match profile {
        Some(p) => StreamOptions {
            format: p.format.clone(),
            max_bit_rate: p.max_bit_rate,
            estimate_content_length: true,
            ..Default::default()
        },
        None => StreamOptions::default(),
    }
}

/// What a stream URL stands for, handed to a [`StreamMinter`].
#[derive(Debug, Clone, PartialEq)]
pub struct StreamTarget {
    pub server_id: String,
    pub track_id: String,
    /// Effective transcoding profile (`None` = original).
    pub profile: Option<TranscodingProfile>,
    /// File extension of what will be served (URL hint, cache file name).
    pub suffix: Option<String>,
    pub mime_type: Option<String>,
}

/// Turns a [`StreamTarget`] into a URL a platform player can open (the
/// core's loopback proxy). `None` declines (resolution falls back).
pub trait StreamMinter {
    fn mint(&self, target: StreamTarget) -> Option<String>;
}

/// A complete stream-cache entry.
#[derive(Debug, Clone, PartialEq)]
pub struct CacheEntry {
    pub path: PathBuf,
    pub content_type: Option<String>,
    pub bytes: u64,
    /// `profile_key` of what the file holds (`""` = the original).
    pub profile: String,
}

fn default_transcode_format(platform: Platform) -> &'static str {
    match platform {
        Platform::Android => "opus",
        _ => "flac",
    }
}

fn mime_for(format: &str) -> String {
    match format.to_ascii_lowercase().as_str() {
        "mp3" => "audio/mpeg",
        "opus" | "ogg" | "oga" => "audio/ogg",
        "aac" | "m4a" => "audio/mp4",
        "flac" => "audio/flac",
        "wav" => "audio/wav",
        _ => "application/octet-stream",
    }
    .into()
}

/// The transcoding profile a [`profile_key`] stands for, as far as the
/// server request goes (`None` = the original).
pub(crate) fn profile_from_key(key: &str) -> Option<TranscodingProfile> {
    if key.is_empty() {
        return None;
    }
    let (format, rate) = key.rsplit_once('-').unwrap_or((key, "0"));
    let rate: u32 = rate.parse().unwrap_or(0);
    Some(TranscodingProfile {
        format: (format != "raw").then(|| format.to_string()),
        max_bit_rate: (rate > 0).then_some(rate),
        cannot_decode: vec![],
    })
}

pub(crate) fn profile_key(p: Option<&TranscodingProfile>) -> String {
    match p {
        Some(p) if p.format.is_some() || p.max_bit_rate.is_some() => {
            format!(
                "{}-{}",
                p.format.as_deref().unwrap_or("raw"),
                p.max_bit_rate.unwrap_or(0)
            )
        }
        _ => String::new(),
    }
}

/// Copy the finished file of one pin onto the `wanted` rows of others for
/// the same track (all of the server's targets, or just one).
fn satisfy_wanted_from_done(
    tx: &rusqlite::Connection,
    server_id: &str,
    target: Option<(&str, &str)>,
) -> DbResult<()> {
    let (kind, id) = target.unwrap_or(("", ""));
    tx.execute(
        "UPDATE pin_tracks SET state = 'done', path = (SELECT o.path FROM pin_tracks o WHERE o.server_id = pin_tracks.server_id AND o.track_id = pin_tracks.track_id AND o.state = 'done' AND o.path IS NOT NULL LIMIT 1),
                bytes = (SELECT o.bytes FROM pin_tracks o WHERE o.server_id = pin_tracks.server_id AND o.track_id = pin_tracks.track_id AND o.state = 'done' LIMIT 1),
                gain_db = (SELECT o.gain_db FROM pin_tracks o WHERE o.server_id = pin_tracks.server_id AND o.track_id = pin_tracks.track_id AND o.state = 'done' LIMIT 1)
         WHERE server_id = ?1 AND (?4 = 0 OR (target_kind = ?2 AND target_id = ?3)) AND state = 'wanted'
           AND EXISTS (SELECT 1 FROM pin_tracks o WHERE o.server_id = pin_tracks.server_id AND o.track_id = pin_tracks.track_id AND o.state = 'done')",
        params![server_id, kind, id, target.is_some() as i64],
    )?;
    Ok(())
}

/// `remove_file` that treats "already gone" as success. `Ok(true)` when a
/// file was removed.
fn remove_if_exists(path: &str) -> std::io::Result<bool> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(true),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e),
    }
}

/// A server-supplied suffix as a file extension: `[A-Za-z0-9]{1,8}`, else
/// `bin`. Anything else (`..`, separators, dots) is not an extension.
pub fn safe_ext(suffix: Option<&str>) -> &str {
    match suffix {
        Some(s)
            if !s.is_empty() && s.len() <= 8 && s.bytes().all(|b| b.is_ascii_alphanumeric()) =>
        {
            s
        }
        _ => "bin",
    }
}

fn safe_name(id: &str) -> String {
    id.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// `file://` URL for a local path.
pub fn file_url(p: &Path) -> String {
    match url::Url::from_file_path(p) {
        Ok(u) => u.to_string(),
        Err(_) => format!("file://{}", p.to_string_lossy()),
    }
}

/// Runs `JobKind::Download` jobs: each item is a track id of the pin in the
/// payload. Out-of-space aborts the job with a loud problem instead of
/// silently unpinning.
pub struct DownloadRunner {
    downloads: Downloads,
    api: Arc<dyn SubsonicApi>,
}

impl DownloadRunner {
    pub fn new(downloads: Downloads, api: Arc<dyn SubsonicApi>) -> Self {
        DownloadRunner { downloads, api }
    }
}

impl JobRunner for DownloadRunner {
    fn run(&self, ctx: JobContext) -> BoxFuture<'static, JobResult<()>> {
        let dl = self.downloads.clone();
        let api = self.api.clone();
        Box::pin(async move {
            let payload: DownloadJobPayload = serde_json::from_str(&ctx.payload)
                .map_err(|e| JobError::Failed(format!("bad payload: {e}")))?;
            let items = ctx.pending_items()?;
            let total = items.len() as u32;
            let mut done = 0u32;
            ctx.set_progress(0, Some(total));
            for it in items {
                ctx.checkpoint().await?;
                // Pin removed meanwhile → stop quietly.
                if !dl.is_pinned(&payload.server_id, &payload.target)? {
                    return Err(JobError::Cancelled);
                }
                match dl
                    .fetch_track(
                        api.as_ref(),
                        &payload.server_id,
                        &it.item,
                        payload.transcode,
                    )
                    .await
                {
                    Ok(_) => {
                        done += 1;
                        ctx.item_done(it.seq)?;
                    }
                    Err(DownloadError::OutOfSpace { needed, free }) => {
                        ctx.item_failed(it.seq, "out of space")?;
                        ctx.queue().add_problem(
                            Some(&ctx.job_id),
                            "Not enough space to finish downloading",
                            Some(&format!(
                                "Needed about {:.0} MB, {:.0} MB free. Free up space and retry.",
                                needed / 1e6,
                                free / 1e6
                            )),
                            Some(RetryAction::RetryJob {
                                job_id: ctx.job_id.clone(),
                            }),
                        )?;
                        return Err(JobError::Failed("out of space".into()));
                    }
                    Err(e) => {
                        ctx.item_failed(it.seq, &e.to_string())?;
                    }
                }
                ctx.set_progress(done, Some(total));
            }
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jobs::JobQueue;
    use crate::subsonic::fake::FakeServer;
    use crate::util::WallClock;

    struct Fixture {
        dir: tempfile::TempDir,
        db: Db,
        server: FakeServer,
        dl: Downloads,
        storage: Arc<FixedStorage>,
    }

    fn fixture() -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open_in_memory().unwrap();
        let server = FakeServer::new("srv", "alice");
        let mut tracks = vec![];
        for i in 0..6 {
            let mut c = FakeServer::song(
                &format!("t{i}"),
                &format!("Track {i}"),
                &format!("al{}", i / 3),
                "ar",
                100.0,
            );
            c.size = Some(1000.0);
            c.replay_gain = Some(crate::subsonic::types::ReplayGainBody {
                track_gain: Some(-3.5),
                track_peak: Some(0.9),
                ..Default::default()
            });
            server.add_song(c.clone());
            server.set_media(&format!("t{i}"), vec![b'x'; 1000]);
            tracks.push(crate::subsonic::convert::track_from_child("srv", &c));
        }
        server.add_playlist("pl", "Mix", "alice", &["t0", "t4"], false);
        db.upsert_tracks(&tracks, &[], 1).unwrap();
        db.upsert_albums(
            &[
                api::Album {
                    id: "al0".into(),
                    server_id: "srv".into(),
                    name: "Album 0".into(),
                    cover_art: Some("c0".into()),
                    ..Default::default()
                },
                api::Album {
                    id: "al1".into(),
                    server_id: "srv".into(),
                    name: "Album 1".into(),
                    ..Default::default()
                },
            ],
            &[],
            1,
        )
        .unwrap();
        db.upsert_playlists(
            &[api::Playlist {
                id: "pl".into(),
                server_id: "srv".into(),
                name: "Mix".into(),
                ..Default::default()
            }],
            1,
        )
        .unwrap();
        db.set_playlist_tracks("srv", "pl", &["t0".into(), "t4".into()])
            .unwrap();
        let storage = Arc::new(FixedStorage(parking_lot::Mutex::new(None)));
        let dl = Downloads::new(
            db.clone(),
            Arc::new(WallClock),
            storage.clone(),
            &dir.path().join("data"),
            &dir.path().join("cache"),
            Platform::Linux,
        );
        Fixture {
            dir,
            db,
            server,
            dl,
            storage,
        }
    }

    async fn run_job(f: &Fixture, spec: JobSpec) -> api::Job {
        let q = JobQueue::new(f.db.clone(), Arc::new(WallClock));
        q.register(
            api::JobKind::Download,
            2,
            Arc::new(DownloadRunner::new(
                f.dl.clone(),
                Arc::new(f.server.clone()),
            )),
        );
        let id = q.submit(spec).unwrap();
        q.run_until_idle().await.unwrap();
        q.job(&id).unwrap().unwrap()
    }

    #[tokio::test]
    async fn pin_album_downloads_all_tracks_and_stores_gain() {
        let f = fixture();
        let target = PinTarget::Album { id: "al0".into() };
        let spec = f.dl.pin("srv", &target, false).unwrap().unwrap();
        assert_eq!(spec.items, vec!["t0", "t1", "t2"]);
        assert_eq!(
            f.db.track("t1").unwrap().unwrap().offline,
            OfflineState::Downloading
        );
        let job = run_job(&f, spec).await;
        assert_eq!(job.state, api::JobState::Done);
        let pins = f.dl.pins("srv").unwrap();
        assert_eq!(pins.len(), 1);
        assert_eq!(pins[0].label, "Album 0");
        assert_eq!(pins[0].cover_art.as_deref(), Some("c0"));
        assert_eq!((pins[0].track_count, pins[0].downloaded_count), (3, 3));
        assert_eq!(pins[0].bytes, 3000.0);
        assert!(!pins[0].transcoded);
        let p = f.dl.downloaded_path("srv", "t1").unwrap().unwrap();
        assert!(p.starts_with(f.dir.path().join("data").join("downloads").join("srv")));
        assert_eq!(p.extension().unwrap(), "flac");
        assert_eq!(std::fs::read(&p).unwrap().len(), 1000);
        assert_eq!(
            f.db.track("t1").unwrap().unwrap().offline,
            OfflineState::Downloaded
        );
        assert_eq!(
            f.dl.gain("srv", "t1").unwrap(),
            Some(-3.5),
            "server-reported gain stored at download time"
        );
        assert!(f.dl.set_gain("srv", "t1", Some(-4.0), Some(0.8)).unwrap());
        assert_eq!(f.dl.gain("srv", "t1").unwrap(), Some(-4.0));
        assert!(
            f.dl.plan("srv", &target, false).unwrap().is_none(),
            "nothing left to fetch"
        );
        assert_eq!(f.dl.downloads_bytes().unwrap(), 3000.0);
        // download URL, never a stream/transcode
        assert!(f.server.calls().iter().all(|c| c != "stream"));
    }

    #[tokio::test]
    async fn unpin_keeps_files_other_pins_need() {
        let f = fixture();
        let album = PinTarget::Album { id: "al0".into() };
        let song = PinTarget::Track { id: "t0".into() };
        run_job(&f, f.dl.pin("srv", &album, false).unwrap().unwrap()).await;
        assert!(
            f.dl.pin("srv", &song, false).unwrap().is_none(),
            "already downloaded by the album pin"
        );
        assert_eq!(f.dl.pins("srv").unwrap().len(), 2);
        assert_eq!(
            f.dl.downloads_bytes().unwrap(),
            3000.0,
            "shared files counted once"
        );
        let removed = f.dl.unpin("srv", &album).unwrap();
        assert_eq!(removed, 2, "t1 and t2 deleted, t0 kept for the song pin");
        assert!(f.dl.downloaded_path("srv", "t0").unwrap().is_some());
        assert!(f.dl.downloaded_path("srv", "t1").unwrap().is_none());
        assert_eq!(
            f.db.track("t1").unwrap().unwrap().offline,
            OfflineState::None
        );
        assert_eq!(
            f.db.track("t0").unwrap().unwrap().offline,
            OfflineState::Downloaded
        );
        f.dl.unpin("srv", &song).unwrap();
        assert!(f.dl.pins("srv").unwrap().is_empty());
        assert_eq!(f.dl.downloads_bytes().unwrap(), 0.0);
    }

    #[tokio::test]
    async fn playlist_pin_tracks_changes() {
        let f = fixture();
        let pl = PinTarget::Playlist { id: "pl".into() };
        run_job(&f, f.dl.pin("srv", &pl, false).unwrap().unwrap()).await;
        assert!(f.dl.downloaded_path("srv", "t4").unwrap().is_some());
        // playlist changes: t4 out, t5 in
        f.db.set_playlist_tracks("srv", "pl", &["t0".into(), "t5".into()])
            .unwrap();
        let spec = f.dl.reconcile_playlist("srv", "pl").unwrap().unwrap();
        assert_eq!(spec.items, vec!["t5"]);
        assert!(
            f.dl.downloaded_path("srv", "t4").unwrap().is_none(),
            "removal deleted the file"
        );
        assert_eq!(
            f.db.track("t4").unwrap().unwrap().offline,
            OfflineState::None
        );
        run_job(&f, spec).await;
        assert!(f.dl.downloaded_path("srv", "t5").unwrap().is_some());
        assert!(f.dl.reconcile_playlist("srv", "pl").unwrap().is_none());
        assert!(f
            .dl
            .reconcile_playlist("srv", "unpinned")
            .unwrap()
            .is_none());
        // album pins are fixed: changing album contents doesn't matter (no reconcile API for them)
    }

    #[tokio::test]
    async fn out_of_space_is_a_loud_problem_not_a_silent_unpin() {
        let f = fixture();
        *f.storage.0.lock() = Some(MIN_FREE_BYTES + 1500.0);
        let target = PinTarget::Album { id: "al0".into() };
        let spec = f.dl.pin("srv", &target, false).unwrap().unwrap();
        let q = JobQueue::new(f.db.clone(), Arc::new(WallClock));
        q.register(
            api::JobKind::Download,
            1,
            Arc::new(DownloadRunner::new(
                f.dl.clone(),
                Arc::new(f.server.clone()),
            )),
        );
        let id = q.submit(spec).unwrap();
        q.run_until_idle().await.unwrap();
        let job = q.job(&id).unwrap().unwrap();
        assert_eq!(job.state, api::JobState::Failed);
        assert_eq!(job.done, 1, "first fit, second didn't");
        let problems = q.problems().unwrap();
        assert!(problems
            .iter()
            .any(|p| p.summary.contains("Not enough space") && p.retryable));
        assert!(f.dl.is_pinned("srv", &target).unwrap(), "pin kept");
        assert_eq!(f.dl.pins("srv").unwrap()[0].downloaded_count, 1);
        // space frees up → retry finishes the pin
        *f.storage.0.lock() = None;
        q.retry(&id).unwrap();
        q.run_until_idle().await.unwrap();
        assert_eq!(q.job(&id).unwrap().unwrap().state, api::JobState::Done);
        assert_eq!(f.dl.pins("srv").unwrap()[0].downloaded_count, 3);
    }

    #[tokio::test]
    async fn server_failure_marks_item_failed_and_is_retryable() {
        let f = fixture();
        f.server.fail_next(
            SubsonicError::Server {
                code: 500,
                message: "x".into(),
            },
            1,
        );
        let target = PinTarget::Track { id: "t3".into() };
        let job = run_job(&f, f.dl.pin("srv", &target, false).unwrap().unwrap()).await;
        assert_eq!(job.state, api::JobState::Failed);
        assert_eq!(
            f.db.track("t3").unwrap().unwrap().offline,
            OfflineState::None
        );
        let spec = f.dl.plan("srv", &target, false).unwrap().unwrap();
        assert_eq!(spec.items, vec!["t3"]);
        let job = run_job(&f, spec).await;
        assert_eq!(job.state, api::JobState::Done);
    }

    #[tokio::test]
    async fn unpinning_mid_job_stops_it() {
        let f = fixture();
        let target = PinTarget::Album { id: "al1".into() };
        let spec = f.dl.pin("srv", &target, false).unwrap().unwrap();
        f.dl.unpin("srv", &target).unwrap();
        let job = run_job(&f, spec).await;
        assert_eq!(job.state, api::JobState::Cancelled);
        assert!(f.dl.downloaded_path("srv", "t3").unwrap().is_none());
    }

    #[tokio::test]
    async fn transcoded_pin_uses_stream_with_profile() {
        let f = fixture();
        f.dl.set_transcoding_profile(
            None,
            TranscodingProfile {
                format: Some("opus".into()),
                max_bit_rate: Some(128),
                cannot_decode: vec![],
            },
        );
        let target = PinTarget::Track { id: "t2".into() };
        run_job(&f, f.dl.pin("srv", &target, true).unwrap().unwrap()).await;
        let p = f.dl.downloaded_path("srv", "t2").unwrap().unwrap();
        assert_eq!(p.extension().unwrap(), "opus");
        assert!(f.dl.pins("srv").unwrap()[0].transcoded);
        assert!(f.server.calls().contains(&"download".to_string()));
    }

    #[test]
    fn stream_cache_lru_budget_and_separation() {
        let f = fixture();
        f.dl.set_cache_budget(2500.0);
        let cdir = f.dl.stream_cache_dir("srv");
        std::fs::create_dir_all(&cdir).unwrap();
        let mut paths = vec![];
        for i in 0..3 {
            let p = f.dl.cache_path("srv", &format!("t{i}"), None, Some("flac"));
            std::fs::write(&p, vec![0u8; 1000]).unwrap();
            f.dl.cache_put(
                "srv",
                &format!("t{i}"),
                None,
                &p,
                1000.0,
                Some("audio/flac"),
            )
            .unwrap();
            paths.push(p);
        }
        assert!(paths[0].starts_with(f.dir.path().join("cache").join("stream")));
        assert_eq!(
            f.dl.cache_bytes().unwrap(),
            2000.0,
            "oldest evicted to stay under 2500"
        );
        assert!(!paths[0].exists() && paths[1].exists() && paths[2].exists());
        assert_eq!(
            f.db.track("t0").unwrap().unwrap().offline,
            OfflineState::None
        );
        assert_eq!(
            f.db.track("t1").unwrap().unwrap().offline,
            OfflineState::Cached
        );
        // touching t1 makes t2 the next victim (the LRU clock is the wall
        // clock in ms; make sure the touch lands after the puts)
        std::thread::sleep(std::time::Duration::from_millis(2));
        assert!(f.dl.cache_get("srv", "t1", None).unwrap().is_some());
        let p = f.dl.cache_path("srv", "t5", None, Some("flac"));
        std::fs::write(&p, vec![0u8; 1000]).unwrap();
        f.dl.cache_put("srv", "t5", None, &p, 1000.0, None).unwrap();
        assert!(paths[1].exists() && !paths[2].exists());
        // a file deleted behind our back is dropped from the index
        std::fs::remove_file(&paths[1]).unwrap();
        assert!(f.dl.cache_get("srv", "t1", None).unwrap().is_none());
        assert_eq!(
            f.db.track("t1").unwrap().unwrap().offline,
            OfflineState::None
        );
        // profile-specific entries have their own key
        let prof = TranscodingProfile {
            format: Some("opus".into()),
            max_bit_rate: Some(96),
            cannot_decode: vec![],
        };
        let pp = f.dl.cache_path("srv", "t5", Some(&prof), Some("opus"));
        assert!(pp.to_string_lossy().contains("opus-96"));
        std::fs::write(&pp, vec![0u8; 500]).unwrap();
        f.dl.cache_put("srv", "t5", Some(&prof), &pp, 500.0, Some("audio/ogg"))
            .unwrap();
        assert_eq!(
            f.dl.cache_get("srv", "t5", Some(&prof)).unwrap().unwrap().0,
            pp
        );
        assert_eq!(f.dl.cache_get("srv", "t5", None).unwrap().unwrap().0, p);
        assert_eq!(f.dl.clear_stream_cache().unwrap(), 2);
        assert_eq!(f.dl.cache_bytes().unwrap(), 0.0);
        assert!(!pp.exists());
        // downloads directory untouched by any of this
        assert_eq!(f.dl.downloads_bytes().unwrap(), 0.0);
    }

    /// Files a reader holds, or whose track the player has loaded, are
    /// skipped by eviction and deferred by a clear; a replaced copy goes
    /// once released; the startup sweep drops temp and orphan files.
    #[test]
    fn stream_cache_files_in_use_are_kept_until_released() {
        let f = fixture();
        let off = |id: &str| f.db.track(id).unwrap().unwrap().offline;
        let put = |id: &str| {
            let p = f.dl.cache_path_unique("srv", id, None, Some("flac"));
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(&p, vec![0u8; 1000]).unwrap();
            f.dl.cache_put("srv", id, None, &p, 1000.0, None).unwrap();
            p
        };
        let p0 = put("t0");
        let p1 = put("t1");
        assert_ne!(p0, f.dl.cache_path_unique("srv", "t0", None, Some("flac")));
        f.dl.acquire_reader(&p0);
        assert!(f
            .dl
            .set_protected(vec![("srv".into(), "t1".into())])
            .is_empty());
        f.dl.set_cache_budget(500.0);
        assert_eq!(f.dl.enforce_cache_budget().unwrap(), 0, "both in use");
        assert_eq!(f.dl.clear_stream_cache().unwrap(), 0, "both deferred");
        assert!(p0.exists() && p1.exists());
        assert_eq!(off("t0"), OfflineState::Cached);
        let changed = f.dl.release_reader(&p0);
        assert!(
            changed.contains(&("srv".into(), "t0".into())),
            "{changed:?}"
        );
        assert!(!p0.exists());
        assert_eq!(off("t0"), OfflineState::None);
        assert!(p1.exists(), "still protected");
        let changed = f.dl.set_protected(vec![]);
        assert!(
            changed.contains(&("srv".into(), "t1".into())),
            "{changed:?}"
        );
        assert!(!p1.exists());
        assert_eq!(off("t1"), OfflineState::None);
        assert_eq!(f.dl.cache_bytes().unwrap(), 0.0);

        // A new copy replaces an old one; the old file goes once released.
        f.dl.set_cache_budget(1e9);
        let a = put("t2");
        f.dl.acquire_reader(&a);
        let b = put("t2");
        assert!(a.exists() && b.exists());
        assert_eq!(f.dl.cache_get("srv", "t2", None).unwrap().unwrap().0, b);
        f.dl.release_reader(&a);
        assert!(!a.exists() && b.exists());
        assert_eq!(off("t2"), OfflineState::Cached);
        let c = put("t2");
        assert!(!b.exists() && c.exists(), "not in use: removed at once");

        // Startup: temp files and files no row references are dropped.
        let dir = f.dl.stream_cache_dir("srv");
        let part = dir.join("t3.flac.abc.part");
        let orphan = dir.join("t4.flac");
        std::fs::write(&part, b"x").unwrap();
        std::fs::write(&orphan, b"x").unwrap();
        let _again = Downloads::new(
            f.db.clone(),
            Arc::new(WallClock),
            f.storage.clone(),
            &f.dir.path().join("data"),
            &f.dir.path().join("cache"),
            Platform::Linux,
        );
        assert!(!part.exists() && !orphan.exists());
        assert!(c.exists());
    }

    #[tokio::test]
    async fn resolve_prefers_download_then_cache_then_stream() {
        let f = fixture();
        let track = f.db.track("t0").unwrap().unwrap();
        let s = f.dl.resolve(&f.server, "k1", &track).unwrap();
        assert!(
            s.url.starts_with("https://srv.fake/rest/stream?id=t0"),
            "{}",
            s.url
        );
        assert!(!s.transcoded);
        assert_eq!(s.mime_type.as_deref(), Some("audio/flac"));
        assert_eq!(s.key, "k1");
        assert_eq!(s.track.id, "t0");
        // cellular profile → transcode
        f.dl.set_transcoding_profile(
            None,
            TranscodingProfile {
                format: None,
                max_bit_rate: None,
                cannot_decode: vec![],
            },
        );
        let mut policy = TranscodingPolicy::default();
        policy.by_kind.insert(
            NetworkKind::Cellular,
            TranscodingProfile {
                format: Some("opus".into()),
                max_bit_rate: Some(96),
                cannot_decode: vec![],
            },
        );
        f.dl.set_transcoding_policy(policy);
        f.dl.set_network(Some(api::NetworkState {
            kind: NetworkKind::Cellular,
            metered: true,
            network_id: None,
        }));
        let s = f.dl.resolve(&f.server, "k1", &track).unwrap();
        assert!(s.transcoded && s.url.contains("format=opus") && s.url.contains("maxBitRate=96"));
        assert_eq!(s.mime_type.as_deref(), Some("audio/ogg"));
        // wifi with no profile → original
        f.dl.set_network(Some(api::NetworkState {
            kind: NetworkKind::Wifi,
            metered: false,
            network_id: Some("home".into()),
        }));
        assert!(!f.dl.resolve(&f.server, "k1", &track).unwrap().transcoded);
        // per-network-id override wins
        f.dl.set_transcoding_profile(
            Some("home".into()),
            TranscodingProfile {
                format: Some("mp3".into()),
                max_bit_rate: Some(320),
                cannot_decode: vec![],
            },
        );
        assert!(f
            .dl
            .resolve(&f.server, "k1", &track)
            .unwrap()
            .url
            .contains("format=mp3"));
        f.dl.set_transcoding_profile(
            Some("home".into()),
            TranscodingProfile {
                format: None,
                max_bit_rate: None,
                cannot_decode: vec![],
            },
        );
        // platform can't decode APE → forced transcode even on wifi
        let mut ape = track.clone();
        ape.suffix = Some("ape".into());
        let s = f.dl.resolve(&f.server, "k1", &ape).unwrap();
        assert!(s.transcoded && s.url.contains("format=flac"), "{}", s.url);
        // cached file wins over streaming
        let p = f.dl.cache_path("srv", "t0", None, Some("flac"));
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, b"abc").unwrap();
        f.dl.cache_put("srv", "t0", None, &p, 3.0, Some("audio/flac"))
            .unwrap();
        let s = f.dl.resolve(&f.server, "k1", &track).unwrap();
        assert!(
            s.url.starts_with("file://") && s.url.ends_with("t0.flac"),
            "{}",
            s.url
        );
        // a download wins over everything and carries the stored gain
        run_job(
            &f,
            f.dl.pin("srv", &PinTarget::Track { id: "t0".into() }, false)
                .unwrap()
                .unwrap(),
        )
        .await;
        f.dl.set_gain("srv", "t0", Some(-6.0), None).unwrap();
        let track = f.db.track("t0").unwrap().unwrap();
        let s = f.dl.resolve(&f.server, "k1", &track).unwrap();
        assert!(s.url.contains("/downloads/srv/"), "{}", s.url);
        assert_eq!(s.gain_db, -6.0);
        assert_eq!(track.offline, OfflineState::Downloaded);
    }

    /// H1: a server-supplied suffix is never a path; the file lands under
    /// the downloads directory with a sanitised extension.
    #[tokio::test]
    async fn hostile_suffix_cannot_escape_downloads_dir() {
        assert_eq!(safe_ext(Some("flac")), "flac");
        assert_eq!(safe_ext(Some("MP3")), "MP3");
        assert_eq!(safe_ext(Some("m4a")), "m4a");
        assert_eq!(safe_ext(None), "bin");
        assert_eq!(safe_ext(Some("")), "bin");
        assert_eq!(safe_ext(Some("../../evil")), "bin");
        assert_eq!(safe_ext(Some("a.b")), "bin");
        assert_eq!(safe_ext(Some("fl ac")), "bin");
        assert_eq!(safe_ext(Some("toolongext")), "bin");
        assert_eq!(safe_ext(Some("flac/../x")), "bin");

        let f = fixture();
        let mut c = FakeServer::song("evil", "Evil", "al0", "ar", 100.0);
        c.suffix = Some("../../../../escaped".into());
        f.server.add_song(c.clone());
        f.server.set_media("evil", b"payload".to_vec());
        f.db.upsert_tracks(
            &[crate::subsonic::convert::track_from_child("srv", &c)],
            &[],
            1,
        )
        .unwrap();
        assert_eq!(
            f.db.track("evil").unwrap().unwrap().suffix.as_deref(),
            Some("../../../../escaped")
        );
        let target = PinTarget::Track { id: "evil".into() };
        let spec = f.dl.pin("srv", &target, false).unwrap().unwrap();
        let job = run_job(&f, spec).await;
        assert_eq!(job.state, api::JobState::Done);
        let p = f.dl.downloaded_path("srv", "evil").unwrap().unwrap();
        let dir = f.dir.path().join("data").join("downloads").join("srv");
        assert!(p.starts_with(&dir), "{p:?}");
        assert_eq!(p.file_name().unwrap(), "evil.bin");
        assert!(!p
            .components()
            .any(|c| matches!(c, std::path::Component::ParentDir)));
        assert_eq!(std::fs::read(&p).unwrap(), b"payload");
        // nothing was written anywhere above the downloads directory
        for up in [f.dir.path().to_path_buf(), f.dir.path().join("data")] {
            let names: Vec<_> = std::fs::read_dir(&up)
                .unwrap()
                .flatten()
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect();
            assert!(!names.iter().any(|n| n.contains("escaped")), "{names:?}");
        }
        let escaped = std::path::Path::new("/").join("escaped");
        assert!(!escaped.exists());
    }

    /// A transport whose downloads take a while and which counts concurrent
    /// writers per destination (a second writer would corrupt the file).
    struct SlowTransport {
        inflight: parking_lot::Mutex<HashMap<PathBuf, usize>>,
        max_per_dest: std::sync::atomic::AtomicUsize,
        downloads: std::sync::atomic::AtomicUsize,
    }

    impl crate::subsonic::HttpTransport for SlowTransport {
        fn execute(
            &self,
            _r: crate::subsonic::transport::HttpRequest,
        ) -> BoxFuture<
            '_,
            Result<
                crate::subsonic::transport::HttpResponse,
                crate::subsonic::transport::TransportError,
            >,
        > {
            Box::pin(async move {
                Ok(crate::subsonic::transport::HttpResponse {
                    status: 404,
                    content_type: None,
                    body: bytes::Bytes::new(),
                })
            })
        }

        fn download(
            &self,
            _url: url::Url,
            dest: &Path,
            _progress: Option<crate::subsonic::transport::ProgressFn>,
        ) -> BoxFuture<
            '_,
            Result<
                crate::subsonic::transport::DownloadOutcome,
                crate::subsonic::transport::TransportError,
            >,
        > {
            use std::sync::atomic::Ordering;
            let dest = dest.to_path_buf();
            Box::pin(async move {
                self.downloads.fetch_add(1, Ordering::SeqCst);
                let n = {
                    let mut m = self.inflight.lock();
                    let e = m.entry(dest.clone()).or_insert(0);
                    *e += 1;
                    *e
                };
                self.max_per_dest.fetch_max(n, Ordering::SeqCst);
                tokio::time::sleep(std::time::Duration::from_millis(60)).await;
                if let Some(parent) = dest.parent() {
                    tokio::fs::create_dir_all(parent).await?;
                }
                let tmp = crate::subsonic::transport::part_path(&dest);
                tokio::fs::write(&tmp, b"0123456789").await?;
                tokio::fs::rename(&tmp, &dest).await?;
                *self.inflight.lock().get_mut(&dest).unwrap() -= 1;
                Ok(crate::subsonic::transport::DownloadOutcome {
                    status: 200,
                    content_type: Some("audio/flac".into()),
                    bytes: 10,
                })
            })
        }
    }

    /// H3: an album pin and a playlist pin sharing `t0`, run concurrently,
    /// fetch it once; the second waits for the first instead of writing the
    /// same file, and both pins end up complete.
    #[tokio::test]
    async fn concurrent_pins_of_the_same_track_download_it_once() {
        use std::sync::atomic::Ordering;
        let f = fixture();
        let transport = Arc::new(SlowTransport {
            inflight: parking_lot::Mutex::new(HashMap::new()),
            max_per_dest: std::sync::atomic::AtomicUsize::new(0),
            downloads: std::sync::atomic::AtomicUsize::new(0),
        });
        let mut cfg = crate::subsonic::ClientConfig::new(
            "srv",
            url::Url::parse("https://music.example.org/").unwrap(),
            crate::subsonic::AuthMode::ApiKey {
                api_key: crate::subsonic::Credential::new("k"),
            },
        );
        cfg.max_concurrent = 4;
        let api: Arc<dyn SubsonicApi> =
            Arc::new(crate::subsonic::Client::new(cfg, transport.clone()));
        let q = JobQueue::new(f.db.clone(), Arc::new(WallClock));
        q.register(
            api::JobKind::Download,
            2,
            Arc::new(DownloadRunner::new(f.dl.clone(), api)),
        );
        let album = PinTarget::Album { id: "al0".into() };
        let playlist = PinTarget::Playlist { id: "pl".into() };
        let a = f.dl.pin("srv", &album, false).unwrap().unwrap();
        let b = f.dl.pin("srv", &playlist, false).unwrap().unwrap();
        assert!(a.items.contains(&"t0".to_string()) && b.items.contains(&"t0".to_string()));
        let ida = q.submit(a).unwrap();
        let idb = q.submit(b).unwrap();
        q.run_until_idle().await.unwrap();
        assert_eq!(q.job(&ida).unwrap().unwrap().state, api::JobState::Done);
        assert_eq!(q.job(&idb).unwrap().unwrap().state, api::JobState::Done);
        assert_eq!(
            transport.max_per_dest.load(Ordering::SeqCst),
            1,
            "never two writers on one file"
        );
        assert_eq!(
            transport.downloads.load(Ordering::SeqCst),
            4,
            "t0 fetched once, t1 t2 t4 once each"
        );
        let pins = f.dl.pins("srv").unwrap();
        assert_eq!(pins.len(), 2);
        for p in &pins {
            assert_eq!(p.track_count, p.downloaded_count, "{:?}", p.target);
        }
        let p = f.dl.downloaded_path("srv", "t0").unwrap().unwrap();
        assert_eq!(std::fs::read(&p).unwrap(), b"0123456789");
        // no temp files left behind
        let dir = f.dir.path().join("data/downloads/srv");
        let names: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        assert!(names.iter().all(|n| !n.ends_with(".part")), "{names:?}");
        assert_eq!(names.len(), 4);
        // the shared track's rows all point at the one file
        let paths: Vec<Option<String>> =
            f.db.with_conn(|c| {
                let mut st = c.prepare("SELECT path FROM pin_tracks WHERE track_id = 't0'")?;
                let rows = st.query_map([], |r| r.get(0))?;
                Ok(rows.collect::<Result<Vec<_>, _>>()?)
            })
            .unwrap();
        assert_eq!(paths.len(), 2);
        assert!(paths
            .iter()
            .all(|p| p.as_deref() == Some(p.as_deref().unwrap())));
        assert_eq!(f.dl.downloads_bytes().unwrap(), 40.0);
    }

    /// M8: a failed fetch leaves an earlier complete download alone and
    /// leaves no temp file; a retry re-claims the `failed` row.
    #[tokio::test]
    async fn failed_fetch_keeps_existing_file_and_is_reclaimable() {
        let f = fixture();
        let target = PinTarget::Track { id: "t1".into() };
        let spec = f.dl.pin("srv", &target, false).unwrap().unwrap();
        assert_eq!(run_job(&f, spec).await.state, api::JobState::Done);
        let p = f.dl.downloaded_path("srv", "t1").unwrap().unwrap();
        // force the row back to failed (as a re-plan of a failed row would) and fail the fetch
        f.db.with_conn(|c| {
            c.execute(
                "UPDATE pin_tracks SET state = 'failed' WHERE track_id = 't1'",
                [],
            )?;
            Ok(())
        })
        .unwrap();
        f.server.fail_next(
            SubsonicError::Server {
                code: 0,
                message: "boom".into(),
            },
            1,
        );
        let e =
            f.dl.fetch_track(&f.server, "srv", "t1", false)
                .await
                .unwrap_err();
        assert!(matches!(e, DownloadError::Server(_)), "{e}");
        assert!(p.exists(), "the earlier complete download is not removed");
        assert_eq!(
            f.dl.fetch_track(&f.server, "srv", "t1", false)
                .await
                .unwrap(),
            1000
        );
        assert_eq!(
            f.db.track("t1").unwrap().unwrap().offline,
            OfflineState::Downloaded
        );
        // an unpinned track is reported as such rather than fetched
        f.dl.unpin("srv", &target).unwrap();
        assert!(matches!(
            f.dl.fetch_track(&f.server, "srv", "t1", false)
                .await
                .unwrap_err(),
            DownloadError::NotPinned(_)
        ));
    }

    /// L3: unpinning A while B merely wants the same track hands B the file.
    #[tokio::test]
    async fn unpin_hands_the_file_to_a_pin_that_still_wants_it() {
        let f = fixture();
        let album = PinTarget::Album { id: "al0".into() };
        let spec = f.dl.pin("srv", &album, false).unwrap().unwrap();
        assert_eq!(run_job(&f, spec).await.state, api::JobState::Done);
        let p = f.dl.downloaded_path("srv", "t0").unwrap().unwrap();
        // B wants t0 (materialised as done via A) — make it a plain 'wanted'
        // row as if B were pinned before A finished.
        let playlist = PinTarget::Playlist { id: "pl".into() };
        let _ = f.dl.pin("srv", &playlist, false).unwrap();
        f.db.with_conn(|c| {
            c.execute(
                "UPDATE pin_tracks SET state = 'wanted', path = NULL, bytes = 0 WHERE target_kind = 'playlist' AND track_id = 't0'",
                [],
            )?;
            Ok(())
        })
        .unwrap();
        let removed = f.dl.unpin("srv", &album).unwrap();
        assert_eq!(removed, 2, "t1 and t2 were orphans; t0 was handed over");
        assert!(p.exists());
        assert_eq!(f.dl.downloaded_path("srv", "t0").unwrap().unwrap(), p);
        assert!(
            f.dl.plan("srv", &playlist, false)
                .unwrap()
                .map(|s| s.items)
                .unwrap_or_default()
                == vec!["t4".to_string()],
            "t0 is not re-downloaded"
        );
        // L2: a file that vanished under us does not abort the loop
        std::fs::remove_file(&p).unwrap();
        assert_eq!(f.dl.unpin("srv", &playlist).unwrap(), 0);
        assert_eq!(
            f.db.track("t0").unwrap().unwrap().offline,
            OfflineState::None
        );
    }

    #[test]
    fn storage_summary_and_thresholds() {
        let f = fixture();
        *f.storage.0.lock() = Some(1e9);
        f.dl.set_warn_threshold(Some(500.0));
        let s = f.dl.storage_summary(42.0).unwrap();
        assert_eq!(s.free_bytes, Some(1e9));
        assert_eq!(s.warn_threshold_bytes, Some(500.0));
        assert_eq!(s.images_bytes, 42.0);
        assert!(!f.dl.over_warn_threshold().unwrap());
        f.db.with_conn(|c| {
            c.execute("INSERT INTO pin_tracks(server_id, target_kind, target_id, track_id, state, bytes) VALUES ('srv','track','t0','t0','done',600)", [])?;
            Ok(())
        })
        .unwrap();
        assert!(f.dl.over_warn_threshold().unwrap());
        assert_eq!(platform_cannot_decode(Platform::Coordinator).len(), 0);
        assert!(platform_cannot_decode(Platform::Android).contains(&"wv".to_string()));
        assert!(
            !platform_cannot_decode(Platform::Linux).contains(&"wv".to_string()),
            "Symphonia decodes WavPack on desktop"
        );
        assert!(file_url(Path::new("/a/b c.flac")).starts_with("file:///a/b%20c.flac"));
    }
}
