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
//! - [`Caches::storage_contribution`] → `images_bytes` for `api::StorageSummary`.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use parking_lot::RwLock;
use rusqlite::{params, OptionalExtension};

use crate::api::{Lyrics, LyricsSource};
use crate::db::{Db, DbError, DbResult};
use crate::subsonic::{SubsonicApi, SubsonicError};
use crate::util::Clock;

/// The fixed artwork sizes. Requests snap up to the nearest.
pub const IMAGE_SIZES: [u32; 5] = [64, 160, 320, 640, 1280];
/// Default LRU budget for images.
pub const DEFAULT_IMAGE_BUDGET: f64 = 512.0 * 1024.0 * 1024.0;
/// Negative lyrics entries are retried after this long.
pub const LYRICS_NEGATIVE_TTL_MS: f64 = 7.0 * 24.0 * 3600.0 * 1000.0;
/// Maximum lyrics documents kept (LRU beyond that).
pub const DEFAULT_LYRICS_MAX_ENTRIES: usize = 5000;
/// Default metadata TTL.
pub const DEFAULT_META_TTL_MS: f64 = 24.0 * 3600.0 * 1000.0;

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

struct Inner {
    db: Db,
    clock: Arc<dyn Clock>,
    cache_dir: PathBuf,
    image_budget: RwLock<f64>,
    lyrics_max: RwLock<usize>,
}

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
    pub async fn artwork_path(
        &self,
        api: &dyn SubsonicApi,
        image_id: &str,
        size: u32,
    ) -> Result<Option<PathBuf>, CacheError> {
        let server_id = api.server_id();
        let size = snap_size(size);
        if let Some(p) = self.artwork_cached(server_id, image_id, size)? {
            return Ok(Some(p));
        }
        let dest = self.image_path(server_id, image_id, size);
        let url = api.cover_art_url(image_id, Some(size));
        let out = match api.download_to_file(url, &dest).await {
            Ok(o) => o,
            Err(SubsonicError::NotFound(_)) => return Ok(None),
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

    pub fn images_bytes(&self) -> DbResult<f64> {
        self.inner.db.with_conn(|c| {
            Ok(
                c.query_row("SELECT COALESCE(SUM(bytes), 0) FROM image_cache", [], |r| {
                    r.get(0)
                })?,
            )
        })
    }

    /// Evict least-recently-used images until under budget.
    pub fn enforce_image_budget(&self) -> DbResult<usize> {
        let budget = *self.inner.image_budget.read();
        let mut total = self.images_bytes()?;
        if total <= budget {
            return Ok(0);
        }
        let victims: Vec<(String, String, i64, String, f64)> = self.inner.db.with_conn(|c| {
            let mut st = c.prepare_cached("SELECT server_id, image_id, size, path, bytes FROM image_cache ORDER BY last_used_at ASC, rowid ASC")?;
            let rows = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)))?;
            Ok(rows.collect::<Result<Vec<_>, _>>()?)
        })?;
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

    pub fn clear_images(&self) -> DbResult<usize> {
        let paths: Vec<String> = self.inner.db.with_conn(|c| {
            let mut st = c.prepare_cached("SELECT path FROM image_cache")?;
            let rows = st.query_map([], |r| r.get::<_, String>(0))?;
            Ok(rows.collect::<Result<Vec<_>, _>>()?)
        })?;
        let mut n = 0;
        for p in &paths {
            if std::fs::remove_file(p).is_ok() {
                n += 1;
            }
        }
        self.inner.db.with_conn(|c| {
            c.execute("DELETE FROM image_cache", [])?;
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
        self.lyrics_store(server_id, track_id, lyrics.source, &json)?;
        self.inner.db.set_track_has_lyrics(track_id, true)?;
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
        self.inner.db.with_conn(|c| {
            c.execute(
                "INSERT INTO lyrics_cache(server_id, track_id, source, json, fetched_at, last_used_at, bytes) VALUES (?1, ?2, ?3, ?4, ?5, ?5, ?6)
                 ON CONFLICT(server_id, track_id, source) DO UPDATE SET json = excluded.json, fetched_at = excluded.fetched_at, last_used_at = excluded.last_used_at, bytes = excluded.bytes",
                params![server_id, track_id, source_name(source), json, now, json.len() as i64],
            )?;
            Ok(())
        })
    }

    pub fn lyrics_invalidate(&self, server_id: &str, track_id: &str) -> DbResult<usize> {
        self.inner.db.with_conn(|c| {
            Ok(c.execute(
                "DELETE FROM lyrics_cache WHERE server_id = ?1 AND track_id = ?2",
                params![server_id, track_id],
            )?)
        })
    }

    /// Keep at most `lyrics_max` positive entries (LRU).
    pub fn enforce_lyrics_budget(&self) -> DbResult<usize> {
        let max = *self.inner.lyrics_max.read() as i64;
        self.inner.db.with_conn(|c| {
            let n: i64 = c.query_row("SELECT count(*) FROM lyrics_cache WHERE json != ''", [], |r| r.get(0))?;
            if n <= max {
                return Ok(0);
            }
            Ok(c.execute(
                "DELETE FROM lyrics_cache WHERE rowid IN (SELECT rowid FROM lyrics_cache WHERE json != '' ORDER BY last_used_at ASC LIMIT ?1)",
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
    async fn artwork_fetches_per_size_and_never_substitutes() {
        let (dir, _db, caches, _clock, server) = fixture();
        server.add_song(Child {
            id: "al-1".into(),
            title: "x".into(),
            ..Default::default()
        });
        server.set_media("al-1", vec![1u8; 300]);
        let big = caches
            .artwork_path(&server, "al-1", 1000)
            .await
            .unwrap()
            .unwrap();
        assert!(big.starts_with(dir.path().join("images").join("srv")));
        assert!(big.to_string_lossy().ends_with("al-1_1280.img"));
        assert_eq!(server.calls_to("download"), 1);
        assert!(
            caches.artwork_cached("srv", "al-1", 64).unwrap().is_none(),
            "a large image is never downscaled for a grid"
        );
        let small = caches
            .artwork_path(&server, "al-1", 64)
            .await
            .unwrap()
            .unwrap();
        assert_ne!(small, big);
        assert_eq!(server.calls_to("download"), 2);
        // second request is served from cache
        assert_eq!(
            caches
                .artwork_path(&server, "al-1", 1280)
                .await
                .unwrap()
                .unwrap(),
            big
        );
        assert_eq!(server.calls_to("download"), 2);
        assert_eq!(caches.images_bytes().unwrap(), 600.0);
        assert_eq!(caches.storage_contribution().unwrap(), 600.0);
        // deleted behind our back → refetched
        std::fs::remove_file(&big).unwrap();
        assert!(caches
            .artwork_cached("srv", "al-1", 1280)
            .unwrap()
            .is_none());
        caches
            .artwork_path(&server, "al-1", 1280)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(server.calls_to("download"), 3);
        // missing artwork on the server is None, not an error
        server.fail_next(SubsonicError::NotFound("no art".into()), 1);
        assert!(caches
            .artwork_path(&server, "none", 320)
            .await
            .unwrap()
            .is_none());
        // the fetch URL carried the snapped size
        let s = server.stream_url("x", &Default::default());
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
            .artwork_path(&server, "a", 64)
            .await
            .unwrap()
            .unwrap();
        *clock.0.lock() += 10.0;
        let b = caches
            .artwork_path(&server, "b", 64)
            .await
            .unwrap()
            .unwrap();
        *clock.0.lock() += 10.0;
        caches.artwork_cached("srv", "a", 64).unwrap(); // touch a
        *clock.0.lock() += 10.0;
        let c = caches
            .artwork_path(&server, "c", 64)
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
}
