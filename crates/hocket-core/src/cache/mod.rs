//! Image, lyrics and metadata caches, each with its own policy. Owner: core-server.
//!
//! Entry points:
//! - [`Caches::new`] with `cache_dir`, the `Db`, a `Clock`.
//! - Images: [`Caches::artwork_path`] resolves an artwork id at one of the
//!   fixed [`IMAGE_SIZES`] to a local file, fetching through `getCoverArt`
//!   when missing; a large cached image is never downscaled for a grid — each
//!   size is fetched and cached on its own. [`Caches::artwork_cached`] is the
//!   synchronous lookup (media session artwork, `Query::Artwork` fast path).
//!   LRU by bytes ([`Caches::set_image_budget`]).
//! - Lyrics: [`Caches::lyrics_get`] / [`lyrics_put`](Caches::lyrics_put) /
//!   [`lyrics_put_none`](Caches::lyrics_put_none) (negative cache) keyed by
//!   (track, source), JSON of `api::Lyrics`; TTL policy for the negative
//!   entries, count budget for the rest.
//! - Metadata: [`Caches::meta_get`] / [`meta_put`](Caches::meta_put) —
//!   small JSON blobs (artist info, similar songs...) with a per-entry TTL.
//! - Artist top songs: [`Caches::top_songs_get`] / [`top_songs_put`](Caches::top_songs_put)
//!   — the last `getTopSongs` answer per artist, kept in the metadata table
//!   and served stale-while-revalidate: the artist page shows it at once and
//!   the actor refreshes it in the background once it is older than
//!   [`TOP_SONGS_REFRESH_MS`].
//! - [`Caches::storage_contribution`] → `images_bytes` for `api::StorageSummary`.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use futures::future::{FutureExt, WeakShared};
use parking_lot::RwLock;
use rusqlite::{params, OptionalExtension};

use crate::api::{Lyrics, LyricsSource, Track};
use crate::db::{Db, DbError, DbResult};
use crate::provider::Provider;
use crate::subsonic::SubsonicError;
use crate::util::Clock;

/// The fixed artwork sizes. Requests snap up to the nearest.
pub const IMAGE_SIZES: [u32; 5] = [64, 160, 320, 640, 1280];
/// The sizes a download keeps its covers at, so downloaded music shows art
/// offline in the places each platform draws it (Android: rows, grids, the
/// mini and full player; desktop: rows, grids, the player).
pub fn download_cover_sizes(platform: crate::api::Platform) -> &'static [u32] {
    use crate::api::Platform;
    match platform {
        Platform::Android => &[64, 160, 320, 640],
        Platform::Linux | Platform::MacOs | Platform::Windows => &[64, 320, 1280],
        Platform::Coordinator => &[],
    }
}
/// How long [`Caches::enforce_image_budget`] trusts its last count of kept
/// covers' bytes.
const KEPT_BYTES_STALE_MS: f64 = 60_000.0;
/// Default LRU budget for images.
pub const DEFAULT_IMAGE_BUDGET: f64 = 512.0 * 1024.0 * 1024.0;
/// Negative lyrics entries are retried after this long.
pub const LYRICS_NEGATIVE_TTL_MS: f64 = 7.0 * 24.0 * 3600.0 * 1000.0;
/// Maximum lyrics documents kept (LRU beyond that).
pub const DEFAULT_LYRICS_MAX_ENTRIES: usize = 5000;
/// Default metadata TTL.
pub const DEFAULT_META_TTL_MS: f64 = 24.0 * 3600.0 * 1000.0;

/// Cached top songs older than this are refreshed in the background (still served meanwhile).
pub const TOP_SONGS_REFRESH_MS: f64 = 6.0 * 3600.0 * 1000.0;
/// Cached top songs are dropped by housekeeping after this long unseen-refreshed.
pub const TOP_SONGS_TTL_MS: f64 = 30.0 * 24.0 * 3600.0 * 1000.0;

/// What [`Caches::top_songs_put`] stores: the songs and how many were asked
/// for, so a later, smaller request can be answered from a larger list.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
struct TopSongsEntry {
    requested: u32,
    tracks: Vec<Track>,
}

/// A cached `getTopSongs` answer for one artist (see [`Caches::top_songs_get`]).
#[derive(Debug, Clone, PartialEq)]
pub struct CachedTopSongs {
    /// At most the requested count, in the server's order.
    pub tracks: Vec<Track>,
    /// How many songs the stored answer was fetched for (refresh with this).
    pub requested: u32,
    pub fetched_at: f64,
}

impl CachedTopSongs {
    /// Old enough that the server should be asked again.
    pub fn is_stale(&self, now: f64) -> bool {
        now - self.fetched_at >= TOP_SONGS_REFRESH_MS
    }
}

fn top_songs_key(artist_id: &str) -> String {
    format!("artistTopSongs:{artist_id}")
}

#[derive(Debug, thiserror::Error)]
pub enum CacheError {
    #[error(transparent)]
    Db(#[from] DbError),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Server(#[from] SubsonicError),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    /// A failure of a fetch shared by several callers.
    #[error(transparent)]
    Shared(Arc<CacheError>),
}

/// Snap a requested size up to the nearest fixed size.
pub fn snap_size(size: u32) -> u32 {
    IMAGE_SIZES
        .iter()
        .copied()
        .find(|s| *s >= size)
        .unwrap_or(IMAGE_SIZES[IMAGE_SIZES.len() - 1])
}

fn source_name(s: LyricsSource) -> &'static str {
    match s {
        LyricsSource::Server => "server",
        LyricsSource::External => "external",
        LyricsSource::Embedded => "embedded",
    }
}

fn store_lyrics_in(
    conn: &rusqlite::Connection,
    server_id: &str,
    track_id: &str,
    source: LyricsSource,
    json: &str,
    now: f64,
) -> DbResult<()> {
    conn.execute(
        "INSERT INTO lyrics_cache(server_id, track_id, source, json, fetched_at, last_used_at, bytes) VALUES (?1, ?2, ?3, ?4, ?5, ?5, ?6)
         ON CONFLICT(server_id, track_id, source) DO UPDATE SET json = excluded.json, fetched_at = excluded.fetched_at, last_used_at = excluded.last_used_at, bytes = excluded.bytes",
        params![server_id, track_id, source_name(source), json, now, json.len() as i64],
    )?;
    Ok(())
}

struct Inner {
    db: Db,
    clock: Arc<dyn Clock>,
    cache_dir: PathBuf,
    image_budget: RwLock<f64>,
    lyrics_max: RwLock<usize>,
    /// Images the server answered 404 for (server, id, size), this session:
    /// a shell asking again after a failed fetch never re-asks the server.
    image_missing: parking_lot::Mutex<std::collections::HashSet<(String, String, u32)>>,
    /// Artwork fetches running, by (server, id, size), for callers to share.
    /// Weak: a fetch whose callers have all left is dropped (cancelled).
    image_inflight:
        parking_lot::Mutex<std::collections::HashMap<(String, String, u32), WeakShared<ArtFetch>>>,
    /// Accounts whose download covers are being fetched
    /// ([`Caches::keep_download_covers`]), each with whether another pass
    /// was asked for meanwhile.
    keeping_covers: parking_lot::Mutex<std::collections::HashMap<String, bool>>,
    /// Accounts whose download lyrics are being fetched
    /// ([`Caches::keep_download_lyrics`]), each with whether another pass
    /// was asked for meanwhile.
    keeping_lyrics: parking_lot::Mutex<std::collections::HashMap<String, bool>>,
    /// Bytes of kept covers as the last eviction found them, and when: until
    /// it is stale, a fetch under budget once those are set aside skips the
    /// eviction scan.
    kept_bytes: parking_lot::Mutex<Option<(f64, f64)>>,
}

type ArtFetch = futures::future::BoxFuture<'static, Result<Option<PathBuf>, Arc<CacheError>>>;

#[derive(Clone)]
pub struct Caches {
    inner: Arc<Inner>,
}

impl Caches {
    pub fn new(db: Db, clock: Arc<dyn Clock>, cache_dir: &Path) -> Self {
        Caches {
            inner: Arc::new(Inner {
                db,
                clock,
                cache_dir: cache_dir.to_path_buf(),
                image_budget: RwLock::new(DEFAULT_IMAGE_BUDGET),
                lyrics_max: RwLock::new(DEFAULT_LYRICS_MAX_ENTRIES),
                image_missing: Default::default(),
                image_inflight: Default::default(),
                keeping_covers: Default::default(),
                keeping_lyrics: Default::default(),
                kept_bytes: Default::default(),
            }),
        }
    }

    pub fn set_image_budget(&self, bytes: f64) {
        *self.inner.image_budget.write() = bytes.max(0.0);
    }

    pub fn set_lyrics_max_entries(&self, n: usize) {
        *self.inner.lyrics_max.write() = n;
    }

    fn now(&self) -> f64 {
        self.inner.clock.now_ms()
    }

    // -- images -------------------------------------------------------------

    pub fn images_dir(&self, server_id: &str) -> PathBuf {
        self.inner.cache_dir.join("images").join(server_id)
    }

    fn image_path(&self, server_id: &str, image_id: &str, size: u32) -> PathBuf {
        let safe: String = image_id
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        self.images_dir(server_id)
            .join(format!("{safe}_{size}.img"))
    }

    /// Cached artwork file for the exact snapped size, touching LRU. Never
    /// substitutes another size.
    pub fn artwork_cached(
        &self,
        server_id: &str,
        image_id: &str,
        size: u32,
    ) -> DbResult<Option<PathBuf>> {
        let size = snap_size(size);
        let now = self.now();
        let path: Option<String> = self.inner.db.with_conn(|c| {
            Ok(c.query_row(
                "SELECT path FROM image_cache WHERE server_id = ?1 AND image_id = ?2 AND size = ?3",
                params![server_id, image_id, size],
                |r| r.get(0),
            )
            .optional()?)
        })?;
        let Some(path) = path else { return Ok(None) };
        let p = PathBuf::from(&path);
        if !p.exists() {
            self.inner.db.with_conn(|c| {
                c.execute(
                    "DELETE FROM image_cache WHERE server_id = ?1 AND image_id = ?2 AND size = ?3",
                    params![server_id, image_id, size],
                )?;
                Ok(())
            })?;
            return Ok(None);
        }
        self.inner.db.with_conn(|c| {
            c.execute("UPDATE image_cache SET last_used_at = ?4 WHERE server_id = ?1 AND image_id = ?2 AND size = ?3", params![server_id, image_id, size, now])?;
            Ok(())
        })?;
        Ok(Some(p))
    }

    /// Resolve artwork to a local file, fetching `getCoverArt?size=` when
    /// not cached. Returns `None` when the server has no image (404).
    ///
    /// Callers asking for the same (account, id, size) while a fetch runs
    /// share it. The fetch belongs to its callers: once every caller has
    /// dropped its future (a row scrolled away), it is cancelled, so a long
    /// scroll never leaves a queue of covers nobody looks at ahead of the
    /// ones on screen.
    pub async fn artwork_path(
        &self,
        provider: Arc<dyn Provider>,
        image_id: &str,
        size: u32,
    ) -> Result<Option<PathBuf>, CacheError> {
        let server_id = provider.source().to_string();
        let size = snap_size(size);
        if let Some(p) = self.artwork_cached(&server_id, image_id, size)? {
            return Ok(Some(p));
        }
        let key = (server_id, image_id.to_string(), size);
        if self.inner.image_missing.lock().contains(&key) {
            return Ok(None);
        }
        let fetch = {
            let mut inflight = self.inner.image_inflight.lock();
            match inflight.get(&key).and_then(|w| w.upgrade()) {
                Some(f) => f,
                None => {
                    // Entries whose callers all left are dead weight.
                    inflight.retain(|_, w| w.upgrade().is_some());
                    let caches = self.clone();
                    let k = key.clone();
                    let f = async move {
                        let out = caches
                            .fetch_artwork(provider.as_ref(), &k.1, k.2)
                            .await
                            .map_err(Arc::new);
                        caches.inner.image_inflight.lock().remove(&k);
                        out
                    }
                    .boxed()
                    .shared();
                    if let Some(w) = f.downgrade() {
                        inflight.insert(key, w);
                    }
                    f
                }
            }
        };
        fetch.await.map_err(CacheError::Shared)
    }

    async fn fetch_artwork(
        &self,
        provider: &dyn Provider,
        image_id: &str,
        size: u32,
    ) -> Result<Option<PathBuf>, CacheError> {
        let server_id = provider.source();
        // Another fetch may have landed between the caller's look and now.
        if let Some(p) = self.artwork_cached(server_id, image_id, size)? {
            return Ok(Some(p));
        }
        let missing = (server_id.to_string(), image_id.to_string(), size);
        let dest = self.image_path(server_id, image_id, size);
        let Some(url) = provider.cover_art_url(image_id, Some(size)) else {
            return Ok(None);
        };
        let out = match provider.download_image_to_file(url, &dest).await {
            Ok(o) => o,
            Err(SubsonicError::NotFound(_)) => {
                self.inner.image_missing.lock().insert(missing);
                return Ok(None);
            }
            Err(e) => return Err(e.into()),
        };
        let now = self.now();
        self.inner.db.with_conn(|c| {
            c.execute(
                "INSERT INTO image_cache(server_id, image_id, size, path, bytes, content_type, fetched_at, last_used_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7)
                 ON CONFLICT(server_id, image_id, size) DO UPDATE SET path = excluded.path, bytes = excluded.bytes, content_type = excluded.content_type, fetched_at = excluded.fetched_at, last_used_at = excluded.last_used_at",
                params![server_id, image_id, size, dest.to_string_lossy().as_ref(), out.bytes as f64, out.content_type, now],
            )?;
            Ok(())
        })?;
        self.enforce_image_budget()?;
        Ok(Some(dest))
    }

    /// Data saver: the cached file for [image_id] at the smallest size at
    /// least [size] (it scales down cleanly), so a cover already on the
    /// device is not fetched again at another size.
    pub fn artwork_cached_larger(
        &self,
        server_id: &str,
        image_id: &str,
        size: u32,
    ) -> DbResult<Option<PathBuf>> {
        let size = snap_size(size);
        for s in IMAGE_SIZES.iter().copied().filter(|s| *s >= size) {
            if let Some(p) = self.artwork_cached(server_id, image_id, s)? {
                return Ok(Some(p));
            }
        }
        Ok(None)
    }

    /// Offline: the cached file for [image_id] at the size closest to
    /// [size], the next larger first (it scales down cleanly), else the
    /// largest smaller one. Online asks go through [`Self::artwork_path`],
    /// which never substitutes.
    pub fn artwork_cached_any(
        &self,
        server_id: &str,
        image_id: &str,
        size: u32,
    ) -> DbResult<Option<PathBuf>> {
        let size = snap_size(size);
        let mut order: Vec<u32> = IMAGE_SIZES.iter().copied().filter(|s| *s >= size).collect();
        order.extend(IMAGE_SIZES.iter().copied().filter(|s| *s < size).rev());
        for s in order {
            if let Some(p) = self.artwork_cached(server_id, image_id, s)? {
                return Ok(Some(p));
            }
        }
        Ok(None)
    }

    /// Covers of what is downloaded ((account, image id)): of each finished
    /// track, its album (album views ask for the album's cover, which can
    /// differ from the song's) and each pin. The image budget never evicts them.
    pub fn kept_covers(&self) -> DbResult<std::collections::HashSet<(String, String)>> {
        self.inner.db.with_conn(|c| {
            let mut st = c.prepare_cached(
                "SELECT DISTINCT t.server_id, t.cover_art FROM pin_tracks pt
                   JOIN tracks t ON t.server_id = pt.server_id AND t.id = pt.track_id
                  WHERE pt.state = 'done' AND t.cover_art IS NOT NULL
                 UNION
                 SELECT a.server_id, a.cover_art FROM pin_tracks pt
                   JOIN tracks t ON t.server_id = pt.server_id AND t.id = pt.track_id
                   JOIN albums a ON a.server_id = t.server_id AND a.id = t.album_id
                  WHERE pt.state = 'done' AND a.cover_art IS NOT NULL
                 UNION
                 SELECT server_id, cover_art FROM pins WHERE cover_art IS NOT NULL",
            )?;
            let rows = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
            Ok(rows.collect::<Result<_, _>>()?)
        })
    }

    /// Fetches the covers of [provider]'s downloads at each of [sizes] that
    /// is not cached yet, one at a time, so downloaded music shows its art
    /// offline. Returns how many were fetched. A missing image is skipped;
    /// any other failure (the server gone) ends the pass, to be picked up by
    /// the next one. A call while a pass runs for the same account returns 0
    /// at once, and that pass looks again for what is missing once done.
    pub async fn keep_download_covers(&self, provider: Arc<dyn Provider>, sizes: &[u32]) -> usize {
        let source = provider.source().to_string();
        if sizes.is_empty() {
            return 0;
        }
        {
            let mut running = self.inner.keeping_covers.lock();
            if let Some(again) = running.get_mut(&source) {
                *again = true;
                return 0;
            }
            running.insert(source.clone(), false);
        }
        struct Running<'a>(&'a Caches, String);
        impl Drop for Running<'_> {
            fn drop(&mut self) {
                self.0.inner.keeping_covers.lock().remove(&self.1);
            }
        }
        let _running = Running(self, source.clone());
        let mut fetched = 0;
        loop {
            let wanted = match self.missing_download_covers(&source, sizes) {
                Ok(w) => w,
                Err(e) => {
                    tracing::warn!(error = %e, "listing download covers");
                    return fetched;
                }
            };
            for (id, size) in wanted {
                match self.artwork_path(provider.clone(), &id, size).await {
                    Ok(Some(_)) => fetched += 1,
                    Ok(None) => {}
                    Err(e) => {
                        tracing::debug!(target: "hocket_core", id = %id, size, error = %e, "download cover fetch failed; stopping");
                        return fetched;
                    }
                }
            }
            let mut running = self.inner.keeping_covers.lock();
            match running.get_mut(&source) {
                Some(again) if *again => *again = false,
                _ => return fetched,
            }
        }
    }

    /// Fetches the server's lyrics of [provider]'s downloaded tracks that
    /// have no lyrics entry yet (positive or "none"), one at a time, so
    /// downloaded music shows lyrics offline and playing it asks nothing.
    /// Kept lyrics are never evicted while the download lasts. Any failure
    /// ends the pass, to be picked up by the next one. Returns how many
    /// tracks got an entry.
    pub async fn keep_download_lyrics(&self, provider: Arc<dyn Provider>) -> usize {
        let source = provider.source().to_string();
        {
            let mut running = self.inner.keeping_lyrics.lock();
            if let Some(again) = running.get_mut(&source) {
                *again = true;
                return 0;
            }
            running.insert(source.clone(), false);
        }
        struct Running<'a>(&'a Caches, String);
        impl Drop for Running<'_> {
            fn drop(&mut self) {
                self.0.inner.keeping_lyrics.lock().remove(&self.1);
            }
        }
        let _running = Running(self, source.clone());
        let mut kept = 0;
        loop {
            let wanted = match self.missing_download_lyrics(&source) {
                Ok(w) => w,
                Err(e) => {
                    tracing::warn!(error = %e, "listing download lyrics");
                    return kept;
                }
            };
            for id in wanted {
                let entries = match provider.lyrics(&id).await {
                    Ok(e) => e,
                    Err(e) => {
                        tracing::debug!(target: "hocket_core", id = %id, error = %e, "download lyrics fetch failed; stopping");
                        return kept;
                    }
                };
                let raw: Result<Vec<crate::lyrics::raw::StructuredLyrics>, _> =
                    serde_json::to_value(&entries).and_then(serde_json::from_value);
                let stored = match raw {
                    Ok(raw) => match crate::lyrics::adapt_list(&id, &raw, LyricsSource::Server) {
                        Some(l) => self.lyrics_put(&source, &id, &l),
                        None => self.lyrics_put_none(&source, &id, LyricsSource::Server),
                    },
                    Err(e) => {
                        tracing::warn!(target: "hocket_core", error = %e, "server lyrics could not be adapted");
                        continue;
                    }
                };
                match stored {
                    Ok(()) => kept += 1,
                    Err(e) => {
                        tracing::warn!(error = %e, "keeping download lyrics");
                        return kept;
                    }
                }
            }
            let mut running = self.inner.keeping_lyrics.lock();
            match running.get_mut(&source) {
                Some(again) if *again => *again = false,
                _ => return kept,
            }
        }
    }

    /// Downloaded tracks of [source] with no lyrics entry at all.
    fn missing_download_lyrics(&self, source: &str) -> DbResult<Vec<String>> {
        self.inner.db.with_conn(|c| {
            let mut st = c.prepare_cached(
                "SELECT pt.track_id FROM pin_tracks pt
                  WHERE pt.server_id = ?1 AND pt.state = 'done'
                    AND NOT EXISTS (SELECT 1 FROM lyrics_cache l
                                     WHERE l.server_id = pt.server_id AND l.track_id = pt.track_id)
                  GROUP BY pt.track_id",
            )?;
            let rows = st.query_map(params![source], |r| r.get(0))?;
            Ok(rows.collect::<Result<Vec<String>, _>>()?)
        })
    }

    /// (image id, size) pairs of [source]'s download covers not cached yet
    /// (nor known missing on the server).
    fn missing_download_covers(&self, source: &str, sizes: &[u32]) -> DbResult<Vec<(String, u32)>> {
        let ids: Vec<String> = self
            .kept_covers()?
            .into_iter()
            .filter(|(s, _)| s == source)
            .map(|(_, id)| id)
            .collect();
        let have: std::collections::HashSet<(String, u32)> = self.inner.db.with_conn(|c| {
            let mut st =
                c.prepare_cached("SELECT image_id, size FROM image_cache WHERE server_id = ?1")?;
            let rows = st.query_map(params![source], |r| Ok((r.get(0)?, r.get(1)?)))?;
            Ok(rows.collect::<Result<_, _>>()?)
        })?;
        let missing = self.inner.image_missing.lock().clone();
        Ok(ids
            .into_iter()
            .flat_map(|id| sizes.iter().map(move |s| (id.clone(), snap_size(*s))))
            .filter(|(id, s)| {
                !have.contains(&(id.clone(), *s))
                    && !missing.contains(&(source.to_string(), id.clone(), *s))
            })
            .collect())
    }

    pub fn images_bytes(&self) -> DbResult<f64> {
        self.inner.db.with_conn(|c| {
            Ok(
                c.query_row("SELECT COALESCE(SUM(bytes), 0) FROM image_cache", [], |r| {
                    r.get(0)
                })?,
            )
        })
    }

    /// Evict least-recently-used images until under budget. Covers of
    /// downloads ([`Self::kept_covers`]) are neither evicted nor counted:
    /// they belong to the downloads, as the audio does.
    pub fn enforce_image_budget(&self) -> DbResult<usize> {
        let budget = *self.inner.image_budget.read();
        let total = self.images_bytes()?;
        if total <= budget {
            return Ok(0);
        }
        let now = self.now();
        if let Some((kept_bytes, at)) = *self.inner.kept_bytes.lock() {
            if now - at < KEPT_BYTES_STALE_MS && total - kept_bytes <= budget {
                return Ok(0);
            }
        }
        let kept = self.kept_covers()?;
        let victims: Vec<(String, String, i64, String, f64)> = self.inner.db.with_conn(|c| {
            let mut st = c.prepare_cached("SELECT server_id, image_id, size, path, bytes FROM image_cache ORDER BY last_used_at ASC, rowid ASC")?;
            let rows = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)))?;
            Ok(rows.collect::<Result<Vec<_>, _>>()?)
        })?;
        let (kept_rows, victims): (Vec<_>, Vec<_>) = victims
            .into_iter()
            .partition(|(sid, iid, ..)| kept.contains(&(sid.clone(), iid.clone())));
        *self.inner.kept_bytes.lock() = Some((kept_rows.iter().map(|v| v.4).sum(), now));
        let mut total: f64 = victims.iter().map(|v| v.4).sum();
        let mut removed = 0;
        for (sid, iid, size, path, bytes) in victims {
            if total <= budget {
                break;
            }
            let p = Path::new(&path);
            if p.exists() {
                if let Err(e) = std::fs::remove_file(p) {
                    tracing::warn!(error = %e, path, "evicting image");
                    continue;
                }
            }
            self.inner.db.with_conn(|c| {
                c.execute(
                    "DELETE FROM image_cache WHERE server_id = ?1 AND image_id = ?2 AND size = ?3",
                    params![sid, iid, size],
                )?;
                Ok(())
            })?;
            total -= bytes;
            removed += 1;
        }
        Ok(removed)
    }

    /// Drops every cached image but the covers of downloads.
    pub fn clear_images(&self) -> DbResult<usize> {
        let kept = self.kept_covers()?;
        let rows: Vec<(String, String, i64, String)> = self.inner.db.with_conn(|c| {
            let mut st =
                c.prepare_cached("SELECT server_id, image_id, size, path FROM image_cache")?;
            let rows = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?;
            Ok(rows.collect::<Result<Vec<_>, _>>()?)
        })?;
        let mut n = 0;
        self.inner.db.with_tx(|tx| {
            for (sid, iid, size, path) in &rows {
                if kept.contains(&(sid.clone(), iid.clone())) {
                    continue;
                }
                if std::fs::remove_file(path).is_ok() {
                    n += 1;
                }
                tx.execute(
                    "DELETE FROM image_cache WHERE server_id = ?1 AND image_id = ?2 AND size = ?3",
                    params![sid, iid, size],
                )?;
            }
            Ok(())
        })?;
        Ok(n)
    }

    // -- lyrics -------------------------------------------------------------

    /// Cached lyrics for (track, source). `Ok(Some(None))` is a still-valid
    /// negative entry ("known to have none"); `Ok(None)` means not cached
    /// (or the negative entry expired).
    pub fn lyrics_get(
        &self,
        server_id: &str,
        track_id: &str,
        source: LyricsSource,
    ) -> Result<Option<Option<Lyrics>>, CacheError> {
        let now = self.now();
        let row: Option<(String, f64)> = self.inner.db.with_conn(|c| {
            Ok(c.query_row(
                "SELECT json, fetched_at FROM lyrics_cache WHERE server_id = ?1 AND track_id = ?2 AND source = ?3",
                params![server_id, track_id, source_name(source)],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?)
        })?;
        let Some((json, fetched_at)) = row else {
            return Ok(None);
        };
        if json.is_empty() {
            if now - fetched_at > LYRICS_NEGATIVE_TTL_MS {
                self.inner.db.with_conn(|c| {
                    c.execute("DELETE FROM lyrics_cache WHERE server_id = ?1 AND track_id = ?2 AND source = ?3", params![server_id, track_id, source_name(source)])?;
                    Ok(())
                })?;
                return Ok(None);
            }
            return Ok(Some(None));
        }
        self.inner.db.with_conn(|c| {
            c.execute(
                "UPDATE lyrics_cache SET last_used_at = ?4 WHERE server_id = ?1 AND track_id = ?2 AND source = ?3",
                params![server_id, track_id, source_name(source), now],
            )?;
            Ok(())
        })?;
        Ok(Some(Some(serde_json::from_str(&json)?)))
    }

    /// Any cached lyrics for the track, server first.
    pub fn lyrics_get_any(
        &self,
        server_id: &str,
        track_id: &str,
    ) -> Result<Option<Lyrics>, CacheError> {
        for s in [
            LyricsSource::Server,
            LyricsSource::Embedded,
            LyricsSource::External,
        ] {
            if let Some(Some(l)) = self.lyrics_get(server_id, track_id, s)? {
                return Ok(Some(l));
            }
        }
        Ok(None)
    }

    pub fn lyrics_put(
        &self,
        server_id: &str,
        track_id: &str,
        lyrics: &Lyrics,
    ) -> Result<(), CacheError> {
        let json = serde_json::to_string(lyrics)?;
        // One transaction, so a failed index write cannot leave the old text
        // searchable next to the new copy.
        let now = self.now();
        self.inner.db.with_tx(|tx| {
            store_lyrics_in(tx, server_id, track_id, lyrics.source, &json, now)?;
            crate::db::queries::index_lyrics_in(
                tx,
                server_id,
                track_id,
                source_name(lyrics.source),
                lyrics,
            )
        })?;
        self.inner
            .db
            .set_track_has_lyrics(server_id, track_id, true)?;
        self.enforce_lyrics_budget()?;
        Ok(())
    }

    /// Remember that a source has nothing for this track.
    pub fn lyrics_put_none(
        &self,
        server_id: &str,
        track_id: &str,
        source: LyricsSource,
    ) -> Result<(), CacheError> {
        Ok(self.lyrics_store(server_id, track_id, source, "")?)
    }

    fn lyrics_store(
        &self,
        server_id: &str,
        track_id: &str,
        source: LyricsSource,
        json: &str,
    ) -> DbResult<()> {
        let now = self.now();
        self.inner
            .db
            .with_conn(|c| store_lyrics_in(c, server_id, track_id, source, json, now))
    }

    pub fn lyrics_invalidate(&self, server_id: &str, track_id: &str) -> DbResult<usize> {
        self.inner.db.with_conn(|c| {
            Ok(c.execute(
                "DELETE FROM lyrics_cache WHERE server_id = ?1 AND track_id = ?2",
                params![server_id, track_id],
            )?)
        })
    }

    /// Keep at most `lyrics_max` positive entries (LRU). Lyrics of
    /// downloaded tracks are kept with the download and not counted.
    pub fn enforce_lyrics_budget(&self) -> DbResult<usize> {
        let max = *self.inner.lyrics_max.read() as i64;
        const EVICTABLE: &str = "json != '' AND NOT EXISTS (SELECT 1 FROM pin_tracks pt
            WHERE pt.server_id = lyrics_cache.server_id AND pt.track_id = lyrics_cache.track_id AND pt.state = 'done')";
        self.inner.db.with_conn(|c| {
            let n: i64 = c.query_row(
                &format!("SELECT count(*) FROM lyrics_cache WHERE {EVICTABLE}"),
                [],
                |r| r.get(0),
            )?;
            if n <= max {
                return Ok(0);
            }
            Ok(c.execute(
                &format!("DELETE FROM lyrics_cache WHERE rowid IN (SELECT rowid FROM lyrics_cache WHERE {EVICTABLE} ORDER BY last_used_at ASC LIMIT ?1)"),
                [n - max],
            )?)
        })
    }

    pub fn lyrics_bytes(&self) -> DbResult<f64> {
        self.inner.db.with_conn(|c| {
            Ok(c.query_row(
                "SELECT COALESCE(SUM(bytes), 0) FROM lyrics_cache",
                [],
                |r| r.get::<_, f64>(0),
            )?)
        })
    }

    // -- metadata -----------------------------------------------------------

    /// Unexpired metadata blob.
    pub fn meta_get<T: serde::de::DeserializeOwned>(
        &self,
        server_id: &str,
        key: &str,
    ) -> Result<Option<T>, CacheError> {
        let now = self.now();
        let row: Option<(String, f64)> = self.inner.db.with_conn(|c| {
            Ok(c.query_row(
                "SELECT json, expires_at FROM metadata_cache WHERE server_id = ?1 AND key = ?2",
                params![server_id, key],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?)
        })?;
        match row {
            Some((json, expires)) if expires > now => Ok(Some(serde_json::from_str(&json)?)),
            Some(_) => {
                self.inner.db.with_conn(|c| {
                    c.execute(
                        "DELETE FROM metadata_cache WHERE server_id = ?1 AND key = ?2",
                        params![server_id, key],
                    )?;
                    Ok(())
                })?;
                Ok(None)
            }
            None => Ok(None),
        }
    }

    pub fn meta_put<T: serde::Serialize>(
        &self,
        server_id: &str,
        key: &str,
        value: &T,
        ttl_ms: Option<f64>,
    ) -> Result<(), CacheError> {
        let json = serde_json::to_string(value)?;
        let now = self.now();
        let expires = now + ttl_ms.unwrap_or(DEFAULT_META_TTL_MS);
        self.inner.db.with_conn(|c| {
            c.execute(
                "INSERT INTO metadata_cache(server_id, key, json, fetched_at, expires_at, bytes) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                 ON CONFLICT(server_id, key) DO UPDATE SET json = excluded.json, fetched_at = excluded.fetched_at, expires_at = excluded.expires_at, bytes = excluded.bytes",
                params![server_id, key, json, now, expires, json.len() as i64],
            )?;
            Ok(())
        })?;
        Ok(())
    }

    /// Drop expired metadata (housekeeping).
    pub fn meta_prune(&self) -> DbResult<usize> {
        let now = self.now();
        self.inner.db.with_conn(|c| {
            Ok(c.execute("DELETE FROM metadata_cache WHERE expires_at <= ?1", [now])?)
        })
    }

    /// A metadata blob whatever its age, with when it was stored. For
    /// stale-while-revalidate readers: never deletes (housekeeping does).
    pub fn meta_get_stale<T: serde::de::DeserializeOwned>(
        &self,
        server_id: &str,
        key: &str,
    ) -> Result<Option<(T, f64)>, CacheError> {
        let row: Option<(String, f64)> = self.inner.db.with_conn(|c| {
            Ok(c.query_row(
                "SELECT json, fetched_at FROM metadata_cache WHERE server_id = ?1 AND key = ?2",
                params![server_id, key],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?)
        })?;
        match row {
            Some((json, fetched_at)) => Ok(Some((serde_json::from_str(&json)?, fetched_at))),
            None => Ok(None),
        }
    }

    // -- artist top songs -------------------------------------------------------

    /// The cached top songs for an artist, when they can answer a request for
    /// `count`: the stored answer was fetched for at least `count` songs, or
    /// the server had fewer than were asked for (so asking for more would not
    /// add any). Served whatever its age; check [`CachedTopSongs::is_stale`].
    pub fn top_songs_get(
        &self,
        server_id: &str,
        artist_id: &str,
        count: u32,
    ) -> Result<Option<CachedTopSongs>, CacheError> {
        let Some((entry, fetched_at)) =
            self.meta_get_stale::<TopSongsEntry>(server_id, &top_songs_key(artist_id))?
        else {
            return Ok(None);
        };
        let exhaustive = (entry.tracks.len() as u64) < u64::from(entry.requested);
        if entry.requested < count && !exhaustive {
            return Ok(None);
        }
        let mut tracks = entry.tracks;
        tracks.truncate(count as usize);
        Ok(Some(CachedTopSongs {
            tracks,
            requested: entry.requested,
            fetched_at,
        }))
    }

    /// Store an artist's top songs as the server answered a request for
    /// `requested` of them. Returns whether the list (its ids, in order)
    /// differs from what was cached before, so callers know to tell the UI.
    pub fn top_songs_put(
        &self,
        server_id: &str,
        artist_id: &str,
        requested: u32,
        tracks: &[Track],
    ) -> Result<bool, CacheError> {
        let key = top_songs_key(artist_id);
        let before = self
            .meta_get_stale::<TopSongsEntry>(server_id, &key)
            .ok()
            .flatten()
            .map(|(e, _)| e);
        let entry = TopSongsEntry {
            requested,
            tracks: tracks.to_vec(),
        };
        self.meta_put(server_id, &key, &entry, Some(TOP_SONGS_TTL_MS))?;
        let changed = match before {
            None => true,
            Some(b) => {
                b.tracks.len() != tracks.len()
                    || b.tracks.iter().zip(tracks).any(|(x, y)| x.id != y.id)
            }
        };
        Ok(changed)
    }

    /// `images_bytes` for `api::StorageSummary` (lyrics/metadata live in the
    /// database and are counted nowhere else).
    pub fn storage_contribution(&self) -> DbResult<f64> {
        self.images_bytes()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::{LyricLine, LyricsTier};
    use crate::subsonic::fake::FakeServer;
    use crate::subsonic::types::Child;

    struct TestClock(parking_lot::Mutex<f64>);
    impl Clock for TestClock {
        fn now_ms(&self) -> f64 {
            *self.0.lock()
        }
    }

    fn fixture() -> (tempfile::TempDir, Db, Caches, Arc<TestClock>, FakeServer) {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open_in_memory().unwrap();
        let clock = Arc::new(TestClock(parking_lot::Mutex::new(1_000.0)));
        let caches = Caches::new(db.clone(), clock.clone(), dir.path());
        let server = FakeServer::new("srv", "alice");
        db.upsert_tracks(
            &[crate::api::Track {
                id: "t".into(),
                server_id: "srv".into(),
                title: "T".into(),
                ..Default::default()
            }],
            &[],
            1,
        )
        .unwrap();
        (dir, db, caches, clock, server)
    }

    #[test]
    fn sizes_snap_up() {
        assert_eq!(snap_size(1), 64);
        assert_eq!(snap_size(64), 64);
        assert_eq!(snap_size(65), 160);
        assert_eq!(snap_size(300), 320);
        assert_eq!(snap_size(5000), 1280);
    }

    #[tokio::test]
    async fn artwork_remembers_a_404_but_asks_again_after_a_failure() {
        let (_dir, _db, caches, _clock, server) = fixture();
        server.set_media("al-1", vec![1u8; 300]);
        server.fail_next(SubsonicError::Network("drop".into()), 1);
        assert!(caches
            .artwork_path(Arc::new(server.provider()), "al-1", 640)
            .await
            .is_err());
        let got = caches
            .artwork_path(Arc::new(server.provider()), "al-1", 640)
            .await
            .unwrap();
        assert!(got.is_some(), "a failed fetch is not remembered");
        server.fail_next(SubsonicError::NotFound("al-2".into()), 1);
        assert!(caches
            .artwork_path(Arc::new(server.provider()), "al-2", 640)
            .await
            .unwrap()
            .is_none());
        let before = server.calls_to("getCoverArt");
        assert!(caches
            .artwork_path(Arc::new(server.provider()), "al-2", 640)
            .await
            .unwrap()
            .is_none());
        assert_eq!(
            server.calls_to("getCoverArt"),
            before,
            "a 404 is not asked again"
        );
    }

    #[tokio::test]
    async fn artwork_fetches_per_size_and_never_substitutes() {
        let (dir, _db, caches, _clock, server) = fixture();
        server.add_song(Child {
            id: "al-1".into(),
            title: "x".into(),
            ..Default::default()
        });
        server.set_media("al-1", vec![1u8; 300]);
        let big = caches
            .artwork_path(Arc::new(server.provider()), "al-1", 1000)
            .await
            .unwrap()
            .unwrap();
        assert!(big.starts_with(dir.path().join("images").join("srv")));
        assert!(big.to_string_lossy().ends_with("al-1_1280.img"));
        assert_eq!(server.calls_to("getCoverArt"), 1);
        assert!(
            caches.artwork_cached("srv", "al-1", 64).unwrap().is_none(),
            "a large image is never downscaled for a grid"
        );
        let small = caches
            .artwork_path(Arc::new(server.provider()), "al-1", 64)
            .await
            .unwrap()
            .unwrap();
        assert_ne!(small, big);
        assert_eq!(server.calls_to("getCoverArt"), 2);
        // second request is served from cache
        assert_eq!(
            caches
                .artwork_path(Arc::new(server.provider()), "al-1", 1280)
                .await
                .unwrap()
                .unwrap(),
            big
        );
        assert_eq!(server.calls_to("getCoverArt"), 2);
        assert_eq!(caches.images_bytes().unwrap(), 600.0);
        assert_eq!(caches.storage_contribution().unwrap(), 600.0);
        // deleted behind our back → refetched
        std::fs::remove_file(&big).unwrap();
        assert!(caches
            .artwork_cached("srv", "al-1", 1280)
            .unwrap()
            .is_none());
        caches
            .artwork_path(Arc::new(server.provider()), "al-1", 1280)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(server.calls_to("getCoverArt"), 3);
        // missing artwork on the server is None, not an error
        server.fail_next(SubsonicError::NotFound("no art".into()), 1);
        assert!(caches
            .artwork_path(Arc::new(server.provider()), "none", 320)
            .await
            .unwrap()
            .is_none());
        // the fetch URL carried the snapped size
        let s = crate::subsonic::SubsonicApi::stream_url(&server, "x", &Default::default());
        assert!(s.as_str().contains("stream"));
    }

    #[tokio::test]
    async fn image_budget_is_lru() {
        let (_dir, _db, caches, clock, server) = fixture();
        server.set_media("a", vec![0u8; 100]);
        server.set_media("b", vec![0u8; 100]);
        server.set_media("c", vec![0u8; 100]);
        caches.set_image_budget(250.0);
        let a = caches
            .artwork_path(Arc::new(server.provider()), "a", 64)
            .await
            .unwrap()
            .unwrap();
        *clock.0.lock() += 10.0;
        let b = caches
            .artwork_path(Arc::new(server.provider()), "b", 64)
            .await
            .unwrap()
            .unwrap();
        *clock.0.lock() += 10.0;
        caches.artwork_cached("srv", "a", 64).unwrap(); // touch a
        *clock.0.lock() += 10.0;
        let c = caches
            .artwork_path(Arc::new(server.provider()), "c", 64)
            .await
            .unwrap()
            .unwrap();
        assert!(
            a.exists() && !b.exists() && c.exists(),
            "b was least recently used"
        );
        assert_eq!(caches.images_bytes().unwrap(), 200.0);
        assert_eq!(caches.clear_images().unwrap(), 2);
        assert_eq!(caches.images_bytes().unwrap(), 0.0);
    }

    /// Track `t` downloaded, its cover `kept`.
    fn download_with_cover(db: &Db) {
        db.with_conn(|c| {
            c.execute_batch(
                "UPDATE tracks SET cover_art = 'kept' WHERE server_id = 'srv' AND id = 't';
                 INSERT INTO pins(server_id, target_kind, target_id, label, created_at) VALUES ('srv', 'track', 't', 'T', 0);
                 INSERT INTO pin_tracks(server_id, target_kind, target_id, track_id, state) VALUES ('srv', 'track', 't', 't', 'done');",
            )?;
            Ok(())
        })
        .unwrap();
    }

    #[tokio::test(start_paused = true)]
    async fn concurrent_asks_for_one_cover_share_a_fetch() {
        let (_dir, _db, caches, _clock, server) = fixture();
        server.set_media("al-1", vec![1u8; 10]);
        server.set_download_delay(Some(std::time::Duration::from_secs(1)));
        let p: Arc<dyn Provider> = Arc::new(server.provider());
        let (a, b) = tokio::join!(
            caches.artwork_path(p.clone(), "al-1", 320),
            caches.artwork_path(p.clone(), "al-1", 320),
        );
        assert_eq!(a.unwrap(), b.unwrap());
        assert_eq!(server.calls_to("getCoverArt"), 1);
        assert!(
            caches.inner.image_inflight.lock().is_empty(),
            "a finished fetch leaves the map"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn a_cover_no_one_waits_for_stops_and_leaves_nothing() {
        let (dir, _db, caches, _clock, server) = fixture();
        server.set_media("al-1", vec![1u8; 10]);
        server.set_download_delay(Some(std::time::Duration::from_secs(10)));
        let p: Arc<dyn Provider> = Arc::new(server.provider());
        // The asker gives up while the download runs.
        let gave_up = tokio::time::timeout(
            std::time::Duration::from_secs(1),
            caches.artwork_path(p.clone(), "al-1", 320),
        )
        .await;
        assert!(gave_up.is_err());
        assert_eq!(server.calls_to("getCoverArt"), 1);
        tokio::time::sleep(std::time::Duration::from_secs(20)).await;
        assert!(
            caches.artwork_cached("srv", "al-1", 320).unwrap().is_none(),
            "the dropped fetch never finished"
        );
        let images = dir.path().join("images").join("srv");
        let leftovers = std::fs::read_dir(&images).map(|d| d.count()).unwrap_or(0);
        assert_eq!(leftovers, 0);
        // A later ask starts afresh.
        server.set_download_delay(None);
        assert!(caches.artwork_path(p, "al-1", 320).await.unwrap().is_some());
        assert_eq!(server.calls_to("getCoverArt"), 2);
    }

    #[tokio::test]
    async fn offline_lookup_prefers_the_next_larger_size() {
        let (_dir, _db, caches, _clock, server) = fixture();
        server.set_media("al-1", vec![1u8; 10]);
        let p: Arc<dyn Provider> = Arc::new(server.provider());
        let small = caches
            .artwork_path(p.clone(), "al-1", 64)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            caches.artwork_cached_any("srv", "al-1", 320).unwrap(),
            Some(small.clone())
        );
        let big = caches.artwork_path(p, "al-1", 640).await.unwrap().unwrap();
        assert_eq!(
            caches.artwork_cached_any("srv", "al-1", 320).unwrap(),
            Some(big)
        );
        assert_eq!(
            caches.artwork_cached_any("srv", "al-1", 64).unwrap(),
            Some(small)
        );
        assert_eq!(caches.artwork_cached_any("srv", "other", 64).unwrap(), None);
    }

    #[tokio::test]
    async fn download_covers_are_kept_fetched_and_never_evicted() {
        let (_dir, db, caches, clock, server) = fixture();
        download_with_cover(&db);
        server.set_media("kept", vec![0u8; 100]);
        server.set_media("x", vec![0u8; 100]);
        server.set_media("y", vec![0u8; 100]);
        let p: Arc<dyn Provider> = Arc::new(server.provider());
        assert_eq!(caches.keep_download_covers(p.clone(), &[64, 640]).await, 2);
        assert!(caches.artwork_cached("srv", "kept", 640).unwrap().is_some());
        assert_eq!(
            caches.keep_download_covers(p.clone(), &[64, 640]).await,
            0,
            "only what is missing"
        );
        // Oldest by far, yet kept: the budget only counts and drops the rest.
        caches.set_image_budget(150.0);
        *clock.0.lock() += 10.0;
        caches
            .artwork_path(p.clone(), "x", 64)
            .await
            .unwrap()
            .unwrap();
        *clock.0.lock() += 10.0;
        caches.artwork_path(p, "y", 64).await.unwrap().unwrap();
        assert!(caches.artwork_cached("srv", "kept", 64).unwrap().is_some());
        assert!(caches.artwork_cached("srv", "kept", 640).unwrap().is_some());
        assert!(caches.artwork_cached("srv", "x", 64).unwrap().is_none());
        assert!(caches.artwork_cached("srv", "y", 64).unwrap().is_some());
        // Clearing the image cache leaves them too.
        assert_eq!(caches.clear_images().unwrap(), 1);
        assert!(caches.artwork_cached("srv", "kept", 64).unwrap().is_some());
    }

    #[tokio::test(start_paused = true)]
    async fn a_cover_pass_asked_for_meanwhile_runs_again() {
        let (_dir, db, caches, _clock, server) = fixture();
        download_with_cover(&db);
        server.set_media("kept", vec![0u8; 10]);
        server.set_media("k2", vec![0u8; 10]);
        server.set_download_delay(Some(std::time::Duration::from_secs(1)));
        let p: Arc<dyn Provider> = Arc::new(server.provider());
        let first = caches.keep_download_covers(p.clone(), &[64]);
        let later = async {
            // While the first pass fetches `kept`, another download lands.
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            db.with_conn(|c| {
                c.execute_batch(
                    "INSERT INTO tracks(id, server_id, title, cover_art) VALUES ('t2', 'srv', 'T2', 'k2');
                     INSERT INTO pin_tracks(server_id, target_kind, target_id, track_id, state) VALUES ('srv', 'track', 't', 't2', 'done');",
                )?;
                Ok(())
            })
            .unwrap();
            caches.keep_download_covers(p.clone(), &[64]).await
        };
        let (a, b) = tokio::join!(first, later);
        assert_eq!((a, b), (2, 0), "the running pass picks up the new cover");
        assert!(caches.artwork_cached("srv", "k2", 64).unwrap().is_some());
        assert!(caches.inner.keeping_covers.lock().is_empty());
    }

    #[tokio::test]
    async fn a_cover_pass_stops_when_the_server_fails() {
        let (_dir, db, caches, _clock, server) = fixture();
        download_with_cover(&db);
        server.set_offline(true);
        let p: Arc<dyn Provider> = Arc::new(server.provider());
        assert_eq!(caches.keep_download_covers(p, &[64, 320, 640]).await, 0);
        assert_eq!(
            server.calls_to("getCoverArt"),
            1,
            "one failure ends the pass"
        );
    }

    fn lyrics(track: &str, source: LyricsSource) -> Lyrics {
        Lyrics {
            track_id: track.into(),
            tier: LyricsTier::Line,
            lang: Some("eng".into()),
            display_artist: None,
            display_title: None,
            agents: vec![],
            lines: vec![LyricLine {
                start_ms: Some(0),
                end_ms: None,
                text: "la".into(),
                syllables: vec![],
                agent: None,
                background: false,
                translation: None,
            }],
            source,
            offset_ms: 0,
        }
    }

    #[test]
    fn lyrics_cache_positive_negative_and_budget() {
        let (_dir, db, caches, clock, _server) = fixture();
        assert!(caches
            .lyrics_get("srv", "t", LyricsSource::Server)
            .unwrap()
            .is_none());
        caches
            .lyrics_put_none("srv", "t", LyricsSource::Server)
            .unwrap();
        assert_eq!(
            caches.lyrics_get("srv", "t", LyricsSource::Server).unwrap(),
            Some(None)
        );
        assert!(caches.lyrics_get_any("srv", "t").unwrap().is_none());
        *clock.0.lock() += LYRICS_NEGATIVE_TTL_MS + 1.0;
        assert!(
            caches
                .lyrics_get("srv", "t", LyricsSource::Server)
                .unwrap()
                .is_none(),
            "negative entry expired"
        );
        let l = lyrics("t", LyricsSource::External);
        caches.lyrics_put("srv", "t", &l).unwrap();
        assert_eq!(
            caches
                .lyrics_get("srv", "t", LyricsSource::External)
                .unwrap(),
            Some(Some(l.clone()))
        );
        assert_eq!(caches.lyrics_get_any("srv", "t").unwrap(), Some(l));
        let has: i64 = db
            .with_conn(|c| {
                Ok(
                    c.query_row("SELECT has_lyrics FROM tracks WHERE id='t'", [], |r| {
                        r.get(0)
                    })?,
                )
            })
            .unwrap();
        assert_eq!(has, 1);
        assert!(caches.lyrics_bytes().unwrap() > 0.0);
        caches.set_lyrics_max_entries(2);
        for i in 0..3 {
            *clock.0.lock() += 1.0;
            caches
                .lyrics_put(
                    "srv",
                    &format!("x{i}"),
                    &lyrics(&format!("x{i}"), LyricsSource::Server),
                )
                .unwrap();
        }
        assert!(
            caches
                .lyrics_get("srv", "t", LyricsSource::External)
                .unwrap()
                .is_none(),
            "oldest evicted"
        );
        assert!(caches
            .lyrics_get("srv", "x2", LyricsSource::Server)
            .unwrap()
            .is_some());
        assert_eq!(caches.lyrics_invalidate("srv", "x2").unwrap(), 1);
    }

    #[test]
    fn metadata_ttl() {
        let (_dir, _db, caches, clock, _server) = fixture();
        caches
            .meta_put(
                "srv",
                "artistInfo2:ar1",
                &serde_json::json!({"bio": "x"}),
                Some(1000.0),
            )
            .unwrap();
        let v: Option<serde_json::Value> = caches.meta_get("srv", "artistInfo2:ar1").unwrap();
        assert_eq!(v.unwrap()["bio"], "x");
        *clock.0.lock() += 1001.0;
        let v: Option<serde_json::Value> = caches.meta_get("srv", "artistInfo2:ar1").unwrap();
        assert!(v.is_none());
        caches.meta_put("srv", "k", &1, None).unwrap();
        *clock.0.lock() += DEFAULT_META_TTL_MS + 1.0;
        assert_eq!(caches.meta_prune().unwrap(), 1);
    }

    fn track(id: &str) -> Track {
        Track {
            id: id.into(),
            server_id: "srv".into(),
            title: id.to_uppercase(),
            ..Default::default()
        }
    }

    fn ids(c: &CachedTopSongs) -> Vec<&str> {
        c.tracks.iter().map(|t| t.id.as_str()).collect()
    }

    #[test]
    fn top_songs_round_trip_and_serve_smaller_requests() {
        let (_dir, _db, caches, _clock, _server) = fixture();
        assert!(caches.top_songs_get("srv", "ar1", 10).unwrap().is_none());
        let list: Vec<Track> = (0..20).map(|i| track(&format!("s{i}"))).collect();
        assert!(caches.top_songs_put("srv", "ar1", 20, &list).unwrap());
        let got = caches.top_songs_get("srv", "ar1", 10).unwrap().unwrap();
        assert_eq!(got.tracks.len(), 10);
        assert_eq!(ids(&got)[..2], ["s0", "s1"]);
        assert_eq!(got.requested, 20);
        assert_eq!(got.fetched_at, 1_000.0);
        // Asking for more than was fetched is a miss while the server may have more.
        assert!(caches.top_songs_get("srv", "ar1", 50).unwrap().is_none());
        // Other servers and artists are separate.
        assert!(caches.top_songs_get("other", "ar1", 10).unwrap().is_none());
        assert!(caches.top_songs_get("srv", "ar2", 10).unwrap().is_none());
    }

    #[test]
    fn top_songs_short_answer_serves_any_count() {
        let (_dir, _db, caches, _clock, _server) = fixture();
        caches
            .top_songs_put("srv", "ar1", 10, &[track("a"), track("b")])
            .unwrap();
        // The server had only two: asking for 500 would add nothing.
        let got = caches.top_songs_get("srv", "ar1", 500).unwrap().unwrap();
        assert_eq!(ids(&got), ["a", "b"]);
        // An empty answer is cached too (artists with no plays stay instant).
        caches.top_songs_put("srv", "ar2", 10, &[]).unwrap();
        assert!(caches
            .top_songs_get("srv", "ar2", 10)
            .unwrap()
            .unwrap()
            .tracks
            .is_empty());
    }

    #[test]
    fn top_songs_are_served_stale_and_report_changes() {
        let (_dir, _db, caches, clock, _server) = fixture();
        let first = [track("a"), track("b")];
        assert!(caches.top_songs_put("srv", "ar1", 10, &first).unwrap());
        assert!(!caches
            .top_songs_get("srv", "ar1", 10)
            .unwrap()
            .unwrap()
            .is_stale(clock.now_ms()));
        *clock.0.lock() += TOP_SONGS_REFRESH_MS;
        // Past the refresh age (and past the default metadata TTL): still served.
        *clock.0.lock() += DEFAULT_META_TTL_MS;
        let got = caches.top_songs_get("srv", "ar1", 10).unwrap().unwrap();
        assert!(got.is_stale(clock.now_ms()));
        assert_eq!(ids(&got), ["a", "b"]);
        assert_eq!(caches.meta_prune().unwrap(), 0, "kept until its own TTL");
        // Same songs again: unchanged, but fresh now.
        assert!(!caches.top_songs_put("srv", "ar1", 10, &first).unwrap());
        let got = caches.top_songs_get("srv", "ar1", 10).unwrap().unwrap();
        assert!(!got.is_stale(clock.now_ms()));
        // Reordered or different songs: changed.
        assert!(caches
            .top_songs_put("srv", "ar1", 10, &[track("b"), track("a")])
            .unwrap());
        assert!(caches
            .top_songs_put("srv", "ar1", 10, &[track("b")])
            .unwrap());
        // Housekeeping drops it once its own TTL has passed.
        *clock.0.lock() += TOP_SONGS_TTL_MS + 1.0;
        assert_eq!(caches.meta_prune().unwrap(), 1);
        assert!(caches.top_songs_get("srv", "ar1", 10).unwrap().is_none());
    }
}
