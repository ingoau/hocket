//! Library sync into the mirror.
//!
//! Full sync pages `search3` with an empty query (flat at every offset on
//! Navidrome ≥ 0.63) into `tracks`, with `getArtists` / `getAlbumList2` for
//! the metadata songs don't carry, `getPlaylists`+`getPlaylist` for
//! playlists and `getGenres` for genres, then a mark-and-sweep by
//! `sync_gen` that catches deletions. Incremental sync is cheap: scan status,
//! new albums (`getAlbumList2 type=newest` until we reach what we know),
//! starred state, playlists whose `changed` moved. A full reconcile runs
//! when the last one is older than [`FULL_RECONCILE_INTERVAL_MS`].
//!
//! The cursor lives in `saved_state` under `sync:<serverId>` so a crash
//! resumes at the same page. Progress is reported per phase with the tables
//! that are already complete (`ready_tables`) so the UI switches over
//! table-by-table while the mirror builds. It runs as `JobKind::LibrarySync`
//! via [`LibrarySyncRunner`].

use std::sync::Arc;

use futures::future::BoxFuture;
use serde::{Deserialize, Serialize};

use crate::api::{self, SyncProgress};
use crate::jobs::{JobContext, JobError, JobResult, JobRunner};
use crate::subsonic::client::{AlbumListType, Search3Page};
use crate::subsonic::convert::{
    album_changed_ms, album_from_id3, artist_from_id3, child_changed_ms, genre_from_body,
    playlist_from_body, track_from_child,
};
use crate::subsonic::{SubsonicApi, SubsonicError};
use crate::util::Clock;

use super::queries::{
    set_playlist_tracks_in, sweep, upsert_albums_in, upsert_artists_in, upsert_playlists_in,
    upsert_tracks_in,
};
use super::{Db, DbError};

/// Page size for `search3` / `getAlbumList2` (Navidrome's maximum).
pub const PAGE_SIZE: u32 = 500;
/// Full reconcile cadence: once a day.
pub const FULL_RECONCILE_INTERVAL_MS: f64 = 24.0 * 3600.0 * 1000.0;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum Phase {
    Artists,
    Albums,
    Tracks,
    Playlists,
    Genres,
    Reconcile,
    Done,
}

impl Phase {
    fn name(self) -> &'static str {
        match self {
            Phase::Artists => "artists",
            Phase::Albums => "albums",
            Phase::Tracks => "tracks",
            Phase::Playlists => "playlists",
            Phase::Genres => "genres",
            Phase::Reconcile => "reconcile",
            Phase::Done => "done",
        }
    }
}

/// Persisted sync state per server.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct SyncCursor {
    /// Generation of the sync in progress (rows seen get `sync_gen = gen`).
    pub gen: i64,
    pub phase: Phase,
    pub offset: u32,
    /// Whether the in-progress run is a full reconcile.
    pub full: bool,
    pub ready_tables: Vec<String>,
    pub started_at: Option<f64>,
    pub last_full_at: Option<f64>,
    pub last_incremental_at: Option<f64>,
    /// `getScanStatus.lastScan` observed at the end of the last run.
    pub last_scan: Option<String>,
    pub last_scan_count: Option<f64>,
    /// Newest album `created` we have seen (incremental stop condition).
    pub newest_album_created: Option<f64>,
}

impl Default for SyncCursor {
    fn default() -> Self {
        SyncCursor {
            gen: 0,
            phase: Phase::Done,
            offset: 0,
            full: false,
            ready_tables: vec![],
            started_at: None,
            last_full_at: None,
            last_incremental_at: None,
            last_scan: None,
            last_scan_count: None,
            newest_album_created: None,
        }
    }
}

impl SyncCursor {
    pub fn key(server_id: &str) -> String {
        format!("sync:{server_id}")
    }
    pub fn in_progress(&self) -> bool {
        self.phase != Phase::Done
    }
    pub fn has_completed_once(&self) -> bool {
        self.last_full_at.is_some()
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct SyncStats {
    pub tracks: usize,
    pub albums: usize,
    pub artists: usize,
    pub playlists: usize,
    pub deleted: usize,
    pub full: bool,
    pub skipped: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum SyncError {
    #[error("cancelled")]
    Cancelled,
    #[error(transparent)]
    Db(#[from] DbError),
    #[error(transparent)]
    Server(#[from] SubsonicError),
}

impl From<SyncError> for JobError {
    fn from(e: SyncError) -> Self {
        match e {
            SyncError::Cancelled => JobError::Cancelled,
            SyncError::Db(d) => JobError::Db(d),
            SyncError::Server(s) => JobError::Failed(s.to_string()),
        }
    }
}

impl From<JobError> for SyncError {
    fn from(e: JobError) -> Self {
        match e {
            JobError::Cancelled => SyncError::Cancelled,
            JobError::Db(d) => SyncError::Db(d),
            other => SyncError::Db(DbError::Other(other.to_string())),
        }
    }
}

/// How the sync reports progress and yields for pause/cancel.
pub trait SyncObserver: Send + Sync {
    fn progress(&self, progress: SyncProgress);
    fn checkpoint(&self) -> BoxFuture<'_, Result<(), SyncError>>;
}

/// No-op observer (tests, one-off runs).
pub struct NoopObserver;
impl SyncObserver for NoopObserver {
    fn progress(&self, _: SyncProgress) {}
    fn checkpoint(&self) -> BoxFuture<'_, Result<(), SyncError>> {
        Box::pin(async { Ok(()) })
    }
}

impl SyncObserver for JobContext {
    fn progress(&self, p: SyncProgress) {
        self.set_progress(p.done, p.total);
    }
    fn checkpoint(&self) -> BoxFuture<'_, Result<(), SyncError>> {
        Box::pin(async move { JobContext::checkpoint(self).await.map_err(SyncError::from) })
    }
}

pub type ProgressFn = Arc<dyn Fn(SyncProgress) + Send + Sync>;

/// The sync engine for one server.
pub struct LibrarySync {
    db: Db,
    api: Arc<dyn SubsonicApi>,
    clock: Arc<dyn Clock>,
    on_progress: ProgressFn,
    page_size: u32,
}

impl LibrarySync {
    pub fn new(
        db: Db,
        api: Arc<dyn SubsonicApi>,
        clock: Arc<dyn Clock>,
        on_progress: ProgressFn,
    ) -> Self {
        LibrarySync {
            db,
            api,
            clock,
            on_progress,
            page_size: PAGE_SIZE,
        }
    }

    /// Smaller pages (tests).
    pub fn with_page_size(mut self, n: u32) -> Self {
        self.page_size = n.max(1);
        self
    }

    pub fn server_id(&self) -> &str {
        self.api.server_id()
    }

    pub fn cursor(&self) -> Result<SyncCursor, DbError> {
        Ok(self
            .db
            .saved_state_get(&SyncCursor::key(self.server_id()))?
            .unwrap_or_default())
    }

    fn save_cursor(&self, c: &SyncCursor) -> Result<(), DbError> {
        self.db
            .saved_state_set(&SyncCursor::key(self.server_id()), c, self.clock.as_ref())
    }

    fn report(
        &self,
        obs: &dyn SyncObserver,
        c: &SyncCursor,
        done: u32,
        total: Option<u32>,
        finished: bool,
    ) {
        let p = SyncProgress {
            server_id: self.server_id().to_string(),
            phase: c.phase.name().to_string(),
            done,
            total,
            ready_tables: c.ready_tables.clone(),
            finished,
        };
        (self.on_progress)(p.clone());
        obs.progress(p);
    }

    /// Run a sync. `full` forces a reconcile; otherwise it is incremental
    /// unless a full one is due or never happened. Resumes an interrupted run.
    pub async fn run(&self, full: bool, obs: &dyn SyncObserver) -> Result<SyncStats, SyncError> {
        let mut c = self.cursor()?;
        let now = self.clock.now_ms();
        let mut stats = SyncStats::default();
        if c.in_progress() {
            tracing::info!(server = self.server_id(), phase = ?c.phase, offset = c.offset, "resuming interrupted sync");
        } else {
            let due = c
                .last_full_at
                .is_none_or(|t| now - t >= FULL_RECONCILE_INTERVAL_MS);
            c.full = full || due;
            c.gen += 1;
            c.phase = if c.full {
                Phase::Artists
            } else {
                Phase::Albums
            };
            c.offset = 0;
            c.started_at = Some(now);
            if c.full {
                c.ready_tables.clear();
            }
            self.save_cursor(&c)?;
        }
        stats.full = c.full;

        // Scan status first: total for progress, and a cheap "nothing changed" signal.
        let scan = match self.api.scan_status().await {
            Ok(s) => Some(s),
            Err(e) => {
                tracing::debug!(error = %e, "getScanStatus unavailable");
                None
            }
        };
        let total_tracks = scan
            .as_ref()
            .and_then(|s| s.count)
            .map(|n| n.max(0.0) as u32);
        if !c.full && !c.in_progress_resumed() {
            let unchanged = scan.as_ref().is_some_and(|s| {
                !s.scanning
                    && s.last_scan.is_some()
                    && s.last_scan == c.last_scan
                    && s.count == c.last_scan_count
            });
            if unchanged {
                // Library unchanged; only playlists and stars can have moved.
                c.phase = Phase::Playlists;
                self.save_cursor(&c)?;
            }
        }

        loop {
            obs.checkpoint().await?;
            match c.phase {
                Phase::Artists => {
                    let n = self.sync_artists(c.gen).await?;
                    stats.artists += n;
                    c.ready_tables.push("artists".into());
                    c.phase = Phase::Albums;
                    c.offset = 0;
                    self.save_cursor(&c)?;
                    self.report(obs, &c, 0, total_tracks, false);
                }
                Phase::Albums => {
                    if c.full {
                        let (n, done) = self.sync_albums_page(c.gen, c.offset).await?;
                        stats.albums += n;
                        c.offset += n as u32;
                        if done {
                            c.ready_tables.push("albums".into());
                            c.phase = Phase::Tracks;
                            c.offset = 0;
                        }
                    } else {
                        let (n, tracks) =
                            self.sync_new_albums(c.gen, c.newest_album_created).await?;
                        stats.albums += n;
                        stats.tracks += tracks;
                        self.sync_starred(c.gen).await?;
                        c.phase = Phase::Playlists;
                        c.offset = 0;
                    }
                    self.save_cursor(&c)?;
                    self.report(obs, &c, c.offset, total_tracks, false);
                }
                Phase::Tracks => {
                    let (n, done) = self.sync_tracks_page(c.gen, c.offset).await?;
                    stats.tracks += n;
                    c.offset += n as u32;
                    if done {
                        c.ready_tables.push("tracks".into());
                        c.phase = Phase::Playlists;
                        c.offset = 0;
                    }
                    self.save_cursor(&c)?;
                    self.report(obs, &c, c.offset, total_tracks, false);
                }
                Phase::Playlists => {
                    let n = self.sync_playlists(c.gen, c.full).await?;
                    stats.playlists += n;
                    if !c.ready_tables.iter().any(|t| t == "playlists") {
                        c.ready_tables.push("playlists".into());
                    }
                    c.phase = Phase::Genres;
                    c.offset = 0;
                    self.save_cursor(&c)?;
                    self.report(obs, &c, 0, None, false);
                }
                Phase::Genres => {
                    self.sync_genres(c.gen).await?;
                    if !c.ready_tables.iter().any(|t| t == "genres") {
                        c.ready_tables.push("genres".into());
                    }
                    c.phase = if c.full {
                        Phase::Reconcile
                    } else {
                        Phase::Done
                    };
                    self.save_cursor(&c)?;
                    self.report(obs, &c, 0, None, false);
                }
                Phase::Reconcile => {
                    stats.deleted += self.sweep(c.gen)?;
                    c.phase = Phase::Done;
                    self.save_cursor(&c)?;
                }
                Phase::Done => break,
            }
        }

        let finished_at = self.clock.now_ms();
        if c.full {
            c.last_full_at = Some(finished_at);
        }
        c.last_incremental_at = Some(finished_at);
        if let Some(s) = &scan {
            c.last_scan = s.last_scan.clone();
            c.last_scan_count = s.count;
        }
        c.newest_album_created = self.newest_album_created()?;
        c.offset = 0;
        self.save_cursor(&c)?;
        self.db
            .set_server_last_sync(self.server_id(), finished_at)?;
        self.report(obs, &c, c.offset, total_tracks, true);
        Ok(stats)
    }

    async fn sync_artists(&self, gen: i64) -> Result<usize, SyncError> {
        let index = self.api.artists().await?;
        let sid = self.server_id();
        let artists: Vec<api::Artist> = index
            .index
            .iter()
            .flat_map(|i| i.artist.iter())
            .map(|a| artist_from_id3(sid, a))
            .collect();
        let n = artists.len();
        self.db.with_tx(|tx| upsert_artists_in(tx, &artists, gen))?;
        Ok(n)
    }

    /// One page of `getAlbumList2 alphabeticalByName`. Returns (count, done).
    async fn sync_albums_page(&self, gen: i64, offset: u32) -> Result<(usize, bool), SyncError> {
        let page = self
            .api
            .album_list2(AlbumListType::AlphabeticalByName, self.page_size, offset)
            .await?;
        let sid = self.server_id();
        let albums: Vec<api::Album> = page.iter().map(|a| album_from_id3(sid, a)).collect();
        let changed: Vec<Option<f64>> = page.iter().map(album_changed_ms).collect();
        let n = albums.len();
        self.db
            .with_tx(|tx| upsert_albums_in(tx, &albums, &changed, gen))?;
        Ok((n, (n as u32) < self.page_size))
    }

    /// One page of `search3` with an empty query. Returns (count, done).
    async fn sync_tracks_page(&self, gen: i64, offset: u32) -> Result<(usize, bool), SyncError> {
        let r = self
            .api
            .search3("", Search3Page::songs(self.page_size, offset))
            .await?;
        let sid = self.server_id();
        let tracks: Vec<api::Track> = r
            .song
            .iter()
            .filter(|c| !c.is_dir)
            .map(|c| track_from_child(sid, c))
            .collect();
        let changed: Vec<Option<f64>> = r
            .song
            .iter()
            .filter(|c| !c.is_dir)
            .map(child_changed_ms)
            .collect();
        let n = tracks.len();
        self.db
            .with_tx(|tx| upsert_tracks_in(tx, &tracks, &changed, gen))?;
        Ok((n, (r.song.len() as u32) < self.page_size))
    }

    /// Incremental: walk `newest` albums until one we already knew about.
    async fn sync_new_albums(
        &self,
        gen: i64,
        newest_known: Option<f64>,
    ) -> Result<(usize, usize), SyncError> {
        let sid = self.server_id();
        let mut offset = 0;
        let mut new_albums = vec![];
        'outer: loop {
            let page = self
                .api
                .album_list2(AlbumListType::Newest, self.page_size, offset)
                .await?;
            let len = page.len() as u32;
            for a in page {
                let created = a
                    .created
                    .as_deref()
                    .and_then(crate::subsonic::types::parse_iso_ms);
                let known = self.db.album(&a.id)?.is_some();
                if known && newest_known.is_some_and(|nk| created.is_some_and(|c| c <= nk)) {
                    break 'outer;
                }
                if !known || newest_known.is_none() {
                    new_albums.push(a);
                }
            }
            if len < self.page_size {
                break;
            }
            offset += len;
            if newest_known.is_none() && offset >= self.page_size * 4 {
                // First incremental after an import with no cursor: bounded effort, the full pass will get the rest.
                break;
            }
        }
        let mut tracks = 0;
        let mut albums = 0;
        for a in new_albums {
            let full = match self.api.album(&a.id).await {
                Ok(f) => f,
                Err(SubsonicError::NotFound(_)) => continue,
                Err(e) => return Err(e.into()),
            };
            let album = album_from_id3(sid, &full.album);
            let changed = album_changed_ms(&full.album);
            let songs: Vec<api::Track> =
                full.song.iter().map(|c| track_from_child(sid, c)).collect();
            let song_changed: Vec<Option<f64>> = full.song.iter().map(child_changed_ms).collect();
            let artists: Vec<api::Artist> = full
                .song
                .iter()
                .filter_map(|c| c.artist_id.as_ref().zip(c.artist.as_ref()))
                .map(|(id, name)| api::Artist {
                    id: id.clone(),
                    server_id: sid.to_string(),
                    name: name.clone(),
                    ..Default::default()
                })
                .collect();
            self.db.with_tx(|tx| {
                // Artists first (only fills in unknown ones: album_count 0 keeps the existing value via the upsert's CASE).
                for ar in &artists {
                    let exists: i64 = tx.query_row(
                        "SELECT count(*) FROM artists WHERE server_id = ?1 AND id = ?2",
                        [sid, &ar.id],
                        |r| r.get(0),
                    )?;
                    if exists == 0 {
                        upsert_artists_in(tx, std::slice::from_ref(ar), gen)?;
                    }
                }
                upsert_albums_in(tx, std::slice::from_ref(&album), &[changed], gen)?;
                upsert_tracks_in(tx, &songs, &song_changed, gen)?;
                Ok(())
            })?;
            albums += 1;
            tracks += songs.len();
        }
        Ok((albums, tracks))
    }

    /// Reconcile loved flags from `getStarred2` (cheap and catches changes
    /// made on other clients; ratings have no such endpoint and wait for the
    /// full pass).
    async fn sync_starred(&self, _gen: i64) -> Result<(), SyncError> {
        let starred = self.api.starred2().await?;
        let sid = self.server_id();
        self.db.with_tx(|tx| {
            for (table, ids) in [
                (
                    "tracks",
                    starred
                        .song
                        .iter()
                        .map(|s| s.id.clone())
                        .collect::<Vec<_>>(),
                ),
                (
                    "albums",
                    starred
                        .album
                        .iter()
                        .map(|a| a.id.clone())
                        .collect::<Vec<_>>(),
                ),
                (
                    "artists",
                    starred
                        .artist
                        .iter()
                        .map(|a| a.id.clone())
                        .collect::<Vec<_>>(),
                ),
            ] {
                tx.execute(
                    &format!("UPDATE {table} SET loved = 0 WHERE server_id = ?1 AND loved = 1"),
                    [sid],
                )?;
                let mut st = tx.prepare_cached(&format!(
                    "UPDATE {table} SET loved = 1 WHERE server_id = ?1 AND id = ?2"
                ))?;
                for id in ids {
                    st.execute([sid, &id])?;
                }
            }
            Ok(())
        })?;
        Ok(())
    }

    async fn sync_playlists(&self, gen: i64, full: bool) -> Result<usize, SyncError> {
        let sid = self.server_id();
        let username = self.api.username();
        let list = self.api.playlists().await?;
        let existing = self.db.playlists(sid)?;
        let playlists: Vec<api::Playlist> = list
            .iter()
            .map(|p| playlist_from_body(sid, username.as_deref(), p))
            .collect();
        self.db.with_tx(|tx| {
            upsert_playlists_in(tx, &playlists, gen)?;
            sweep(tx, "playlists", sid, gen)?;
            Ok(())
        })?;
        let mut refreshed = 0;
        for p in &playlists {
            let prev = existing.iter().find(|e| e.id == p.id);
            let synced = self.db.playlist_tracks_synced(&p.id)?;
            let stale = !synced
                || full
                || prev.is_none_or(|e| e.changed != p.changed || e.song_count != p.song_count);
            if !stale {
                continue;
            }
            let detail = match self.api.playlist(&p.id).await {
                Ok(d) => d,
                Err(SubsonicError::NotFound(_)) => continue,
                Err(e) => return Err(e.into()),
            };
            let tracks: Vec<api::Track> = detail
                .entry
                .iter()
                .map(|c| track_from_child(sid, c))
                .collect();
            let changed: Vec<Option<f64>> = detail.entry.iter().map(child_changed_ms).collect();
            let ids: Vec<String> = detail.entry.iter().map(|c| c.id.clone()).collect();
            self.db.with_tx(|tx| {
                // Entries may reference tracks not yet paged in on a first run; keep them.
                upsert_tracks_in(tx, &tracks, &changed, gen)?;
                set_playlist_tracks_in(tx, sid, &p.id, &ids)?;
                Ok(())
            })?;
            refreshed += 1;
        }
        Ok(refreshed)
    }

    async fn sync_genres(&self, gen: i64) -> Result<(), SyncError> {
        let genres: Vec<api::Genre> = self
            .api
            .genres()
            .await?
            .iter()
            .map(genre_from_body)
            .collect();
        self.db.replace_genres(self.server_id(), &genres, gen)?;
        Ok(())
    }

    /// Mark-and-sweep plus derived counts.
    fn sweep(&self, gen: i64) -> Result<usize, SyncError> {
        let sid = self.server_id();
        let n = self.db.with_tx(|tx| {
            let mut n = 0;
            n += sweep(tx, "tracks", sid, gen)?;
            n += sweep(tx, "albums", sid, gen)?;
            n += sweep(tx, "artists", sid, gen)?;
            tx.execute(
                "UPDATE artists SET song_count = (SELECT count(*) FROM tracks WHERE tracks.server_id = artists.server_id AND tracks.artist_id = artists.id) WHERE server_id = ?1",
                [sid],
            )?;
            tx.execute(
                "UPDATE albums SET song_count = (SELECT count(*) FROM tracks WHERE tracks.server_id = albums.server_id AND tracks.album_id = albums.id) WHERE server_id = ?1 AND song_count = 0",
                [sid],
            )?;
            Ok(n)
        })?;
        Ok(n)
    }

    fn newest_album_created(&self) -> Result<Option<f64>, DbError> {
        self.db.with_conn(|c| {
            Ok(c.query_row(
                "SELECT MAX(created) FROM albums WHERE server_id = ?1",
                [self.server_id()],
                |r| r.get::<_, Option<f64>>(0),
            )?)
        })
    }
}

impl SyncCursor {
    fn in_progress_resumed(&self) -> bool {
        // set by `run` before this is consulted: a resumed run has an offset or a phase past the first
        self.offset > 0 || !matches!(self.phase, Phase::Artists | Phase::Albums)
    }
}

/// Payload of a `JobKind::LibrarySync` job.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct SyncJobPayload {
    pub full: bool,
}

/// `JobRunner` adapter: `payload = {"full": bool}`.
pub struct LibrarySyncRunner {
    sync: Arc<LibrarySync>,
}

impl LibrarySyncRunner {
    pub fn new(sync: Arc<LibrarySync>) -> Self {
        LibrarySyncRunner { sync }
    }
}

impl JobRunner for LibrarySyncRunner {
    fn run(&self, ctx: JobContext) -> BoxFuture<'static, JobResult<()>> {
        let sync = self.sync.clone();
        Box::pin(async move {
            let payload: SyncJobPayload = serde_json::from_str(&ctx.payload).unwrap_or_default();
            sync.run(payload.full, &ctx)
                .await
                .map(|_| ())
                .map_err(JobError::from)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::queries::TrackQuery;
    use crate::jobs::{JobQueue, JobSpec};
    use crate::subsonic::fake::FakeServer;
    use crate::subsonic::types::Child;
    use crate::util::WallClock;
    use parking_lot::Mutex;

    fn server() -> FakeServer {
        let s = FakeServer::new("srv", "alice");
        for i in 0..12 {
            let mut c = FakeServer::song(
                &format!("s{i:02}"),
                &format!("Song {i}"),
                &format!("al{}", i / 4),
                &format!("ar{}", i / 8),
                100.0 + i as f64,
            );
            c.created = Some(format!("2024-01-{:02}T00:00:00Z", 1 + i / 4));
            c.genre = Some(if i % 2 == 0 {
                "Even".into()
            } else {
                "Odd".into()
            });
            c.user_rating = Some((i % 6) as u32);
            s.add_song(c);
        }
        s.add_playlist("pl1", "Mix", "alice", &["s00", "s05"], false);
        s.add_playlist("pl2", "Smart", "bob", &["s01"], true);
        s
    }

    fn sync_for(db: &Db, s: &FakeServer, progress: Arc<Mutex<Vec<SyncProgress>>>) -> LibrarySync {
        let p = progress.clone();
        LibrarySync::new(
            db.clone(),
            Arc::new(s.clone()),
            Arc::new(WallClock),
            Arc::new(move |sp| p.lock().push(sp)),
        )
        .with_page_size(5)
    }

    #[tokio::test]
    async fn full_sync_populates_everything_with_progress() {
        let db = Db::open_in_memory().unwrap();
        let s = server();
        let progress = Arc::new(Mutex::new(vec![]));
        let sync = sync_for(&db, &s, progress.clone());
        let stats = sync.run(true, &NoopObserver).await.unwrap();
        assert!(stats.full);
        assert_eq!(stats.tracks, 12);
        assert_eq!(stats.albums, 3);
        assert_eq!(stats.artists, 2);
        assert_eq!(stats.playlists, 2);
        assert_eq!(db.track_count("srv", &None).unwrap(), 12);
        assert_eq!(db.artists("srv", None).unwrap().len(), 2);
        assert_eq!(
            db.artist("ar0").unwrap().unwrap().song_count,
            8,
            "derived at reconcile"
        );
        assert_eq!(db.genres("srv").unwrap().len(), 2);
        let pls = db.playlists("srv").unwrap();
        assert_eq!(pls.len(), 2);
        assert!(pls.iter().find(|p| p.id == "pl2").unwrap().is_smart);
        assert!(pls.iter().find(|p| p.id == "pl1").unwrap().is_mine);
        assert_eq!(db.playlist_track_ids("pl1").unwrap(), vec!["s00", "s05"]);
        // paging happened (12 songs / 5 per page = 3 pages)
        assert_eq!(s.calls_to("search3"), 3);
        // progress: phases in order, ready tables accumulate, last is finished
        let p = progress.lock();
        let phases: Vec<&str> = p.iter().map(|x| x.phase.as_str()).collect();
        assert!(phases.starts_with(&["albums"]), "{phases:?}");
        assert!(
            phases.contains(&"tracks")
                && phases.contains(&"playlists")
                && phases.contains(&"genres")
        );
        assert!(p.last().unwrap().finished);
        assert_eq!(
            p.last().unwrap().ready_tables,
            vec!["artists", "albums", "tracks", "playlists", "genres"]
        );
        assert_eq!(
            p.iter().find(|x| x.phase == "tracks").unwrap().total,
            Some(12)
        );
        let c = sync.cursor().unwrap();
        assert!(!c.in_progress() && c.has_completed_once());
        assert!(
            db.servers().unwrap().is_empty(),
            "sync doesn't create server rows"
        );
    }

    #[tokio::test]
    async fn resumes_after_crash_mid_tracks() {
        let db = Db::open_in_memory().unwrap();
        let s = server();
        let sync = sync_for(&db, &s, Arc::new(Mutex::new(vec![])));
        // Fail on the 2nd search3 page (after artists, one albums page, first tracks page).
        s.fail_next(SubsonicError::Network("drop".into()), 0);
        struct FailAt(std::sync::atomic::AtomicUsize);
        impl SyncObserver for FailAt {
            fn progress(&self, _: SyncProgress) {}
            fn checkpoint(&self) -> BoxFuture<'_, Result<(), SyncError>> {
                Box::pin(async move {
                    let n = self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    if n == 4 {
                        Err(SyncError::Cancelled)
                    } else {
                        Ok(())
                    }
                })
            }
        }
        let e = sync
            .run(true, &FailAt(Default::default()))
            .await
            .unwrap_err();
        assert!(matches!(e, SyncError::Cancelled));
        let c = sync.cursor().unwrap();
        assert!(c.in_progress());
        let before = db.track_count("srv", &None).unwrap();
        assert!(before > 0 && before < 12, "partial: {before}");
        let calls_before = s.calls_to("search3");
        let stats = sync.run(false, &NoopObserver).await.unwrap();
        assert!(stats.full, "resumed run keeps its full flag");
        assert_eq!(db.track_count("srv", &None).unwrap(), 12);
        assert!(
            s.calls_to("search3") - calls_before < 3,
            "did not restart from page 0"
        );
        assert!(!sync.cursor().unwrap().in_progress());
    }

    #[tokio::test]
    async fn full_reconcile_catches_deletions_and_changes() {
        let db = Db::open_in_memory().unwrap();
        let s = server();
        let sync = sync_for(&db, &s, Arc::new(Mutex::new(vec![])));
        sync.run(true, &NoopObserver).await.unwrap();
        db.set_track_offline("s03", api::OfflineState::Downloaded)
            .unwrap();
        {
            let mut st = s.state.lock();
            st.songs.remove("s11");
            st.songs.get_mut("s03").unwrap().title = "Renamed".into();
            st.playlists.remove("pl2");
        }
        let stats = sync.run(true, &NoopObserver).await.unwrap();
        assert_eq!(stats.deleted, 1);
        assert!(db.track("s11").unwrap().is_none());
        let t = db.track("s03").unwrap().unwrap();
        assert_eq!(t.title, "Renamed");
        assert_eq!(
            t.offline,
            api::OfflineState::Downloaded,
            "local state survives"
        );
        assert_eq!(db.playlists("srv").unwrap().len(), 1);
    }

    #[tokio::test]
    async fn incremental_skips_when_scan_unchanged_and_picks_up_new_albums() {
        let db = Db::open_in_memory().unwrap();
        let s = server();
        s.state.lock().scan_status.last_scan = Some("2024-06-01T00:00:00Z".into());
        let sync = sync_for(&db, &s, Arc::new(Mutex::new(vec![])));
        sync.run(true, &NoopObserver).await.unwrap();
        let calls = s.calls_to("search3");
        // unchanged scan → no album/track work, playlists still refreshed
        let stats = sync.run(false, &NoopObserver).await.unwrap();
        assert!(!stats.full);
        assert_eq!(stats.tracks, 0);
        assert_eq!(s.calls_to("search3"), calls);
        assert_eq!(
            s.calls_to("getAlbumList2"),
            1,
            "only the full run listed albums"
        );
        // a new album appears and the scan moves
        {
            let mut st = s.state.lock();
            st.scan_status.last_scan = Some("2024-06-02T00:00:00Z".into());
        }
        let mut c = FakeServer::song("s99", "New Song", "al9", "ar0", 90.0);
        c.created = Some("2024-05-01T00:00:00Z".into());
        s.add_song(c);
        s.state.lock().albums.get_mut("al9").unwrap().created = Some("2024-05-01T00:00:00Z".into());
        let stats = sync.run(false, &NoopObserver).await.unwrap();
        assert_eq!(stats.albums, 1);
        assert_eq!(stats.tracks, 1);
        assert!(db.track("s99").unwrap().is_some());
        assert!(db.album("al9").unwrap().is_some());
        assert_eq!(
            s.calls_to("search3"),
            calls,
            "incremental never pages search3"
        );
    }

    #[tokio::test]
    async fn incremental_reconciles_stars_and_changed_playlists() {
        let db = Db::open_in_memory().unwrap();
        let s = server();
        s.state.lock().scan_status.last_scan = Some("2024-06-01T00:00:00Z".into());
        let sync = sync_for(&db, &s, Arc::new(Mutex::new(vec![])));
        sync.run(true, &NoopObserver).await.unwrap();
        s.state.lock().scan_status.last_scan = Some("2024-06-03T00:00:00Z".into());
        s.star(&[crate::subsonic::StarTarget::Song("s02".into())])
            .await
            .unwrap();
        s.update_playlist(
            "pl1",
            crate::subsonic::PlaylistUpdate {
                song_ids_to_add: vec!["s07".into()],
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let getpl = s.calls_to("getPlaylist");
        sync.run(false, &NoopObserver).await.unwrap();
        assert!(db.track("s02").unwrap().unwrap().loved);
        assert_eq!(
            db.playlist_track_ids("pl1").unwrap(),
            vec!["s00", "s05", "s07"]
        );
        assert_eq!(
            s.calls_to("getPlaylist") - getpl,
            1,
            "only the changed playlist was refetched"
        );
    }

    #[tokio::test]
    async fn runs_as_a_job_and_reports_errors_as_problems() {
        let db = Db::open_in_memory().unwrap();
        let s = server();
        let sync = Arc::new(sync_for(&db, &s, Arc::new(Mutex::new(vec![]))));
        let q = JobQueue::new(db.clone(), Arc::new(WallClock));
        q.register(
            api::JobKind::LibrarySync,
            1,
            Arc::new(LibrarySyncRunner::new(sync.clone())),
        );
        let id = q
            .submit(JobSpec::new(api::JobKind::LibrarySync, "Sync").payload(r#"{"full":true}"#))
            .unwrap();
        q.run_until_idle().await.unwrap();
        let j = q.job(&id).unwrap().unwrap();
        assert_eq!(j.state, api::JobState::Done);
        assert_eq!(j.total, Some(12));
        assert_eq!(db.track_count("srv", &None).unwrap(), 12);

        s.set_offline(true);
        let id = q
            .submit(JobSpec::new(api::JobKind::LibrarySync, "Sync").payload(r#"{"full":true}"#))
            .unwrap();
        q.run_until_idle().await.unwrap();
        assert_eq!(q.job(&id).unwrap().unwrap().state, api::JobState::Failed);
        let p = q.problems().unwrap();
        assert_eq!(p.len(), 1);
        assert!(p[0].retryable);
        // retry after the network returns resumes the interrupted run
        s.set_offline(false);
        q.retry_problem(&p[0].id).unwrap();
        q.run_until_idle().await.unwrap();
        assert_eq!(q.job(&id).unwrap().unwrap().state, api::JobState::Done);
        assert!(q.problems().unwrap().is_empty());
    }

    #[tokio::test]
    async fn playlist_entries_unknown_to_tracks_are_kept() {
        let db = Db::open_in_memory().unwrap();
        let s = FakeServer::new("srv", "alice");
        s.add_song(FakeServer::song("a", "A", "al", "ar", 10.0));
        s.add_playlist("pl", "P", "alice", &["a", "ghost"], false);
        // "ghost" is in the playlist but not in the songs table of the fake; the fake drops it from entries.
        let sync = sync_for(&db, &s, Arc::new(Mutex::new(vec![])));
        sync.run(true, &NoopObserver).await.unwrap();
        assert_eq!(db.playlist_track_ids("pl").unwrap(), vec!["a"]);
        let q = TrackQuery {
            server_id: "srv".into(),
            ..Default::default()
        };
        assert_eq!(db.tracks(&q).unwrap().len(), 1);
        let _ = Child::default();
    }
}
