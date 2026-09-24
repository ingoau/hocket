//! The stream cache's index: complete and partial entries, their spans,
//! the server metadata they were fetched against, validation against the
//! files on disk, scored eviction and traffic counters.
//!
//! A row per (server, track, transcoding profile) points at one file. A
//! partial row's file is sparse: `spans` says which byte ranges hold data
//! (the stream reader writes whatever it reads at its offset and merges the
//! spans in here); when they cover `[0, total_bytes)` the row is promoted to
//! complete in place. A row is only trusted while its file exists, is long
//! enough for what the row claims, and the track's server metadata still
//! matches `source_tag`; anything else is a miss that removes the row
//! (Android may clear the cache directory at any time).

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};

use super::spans::SpanSet;
use super::{profile_key, remove_if_exists, Doomed, Downloads, TrackKey};
use crate::api::TranscodingProfile;
use crate::db::DbResult;

/// The track's server metadata a cache entry depends on, as one string:
/// size, suffix, content type, and the change markers the mirror has.
const SOURCE_TAG_SQL: &str = "COALESCE(CAST(t.size_bytes AS TEXT), '') || '|' || COALESCE(t.suffix, '') || '|' || COALESCE(t.content_type, '') || '|' || COALESCE(CAST(t.changed AS TEXT), '') || '|' || COALESCE(CAST(t.created AS TEXT), '')";

const ROW_COLUMNS: &str = "server_id, track_id, profile, path, content_type, total_bytes, spans, complete, bytes, source_tag, last_used_at";

/// One stream-cache row, complete or partial.
#[derive(Debug, Clone, PartialEq)]
pub struct CacheRow {
    pub server_id: String,
    pub track_id: String,
    /// `profile_key` of the transcoding profile (`""` = original).
    pub profile: String,
    pub path: PathBuf,
    pub content_type: Option<String>,
    /// Length of the whole stream, when the server said.
    pub total: Option<u64>,
    pub spans: SpanSet,
    pub complete: bool,
    pub bytes: u64,
    pub source_tag: Option<String>,
    pub last_used_at: f64,
}

impl CacheRow {
    fn from_row(r: &rusqlite::Row) -> rusqlite::Result<CacheRow> {
        let path: String = r.get(3)?;
        let total: Option<f64> = r.get(5)?;
        let spans: String = r.get(6)?;
        let complete = r.get::<_, i64>(7)? != 0;
        let bytes: f64 = r.get(8)?;
        let total = total.map(|t| t.max(0.0) as u64);
        let mut spans = SpanSet::decode(&spans);
        if complete && spans.is_empty() {
            spans = SpanSet::of(0, total.unwrap_or(bytes.max(0.0) as u64));
        }
        Ok(CacheRow {
            server_id: r.get(0)?,
            track_id: r.get(1)?,
            profile: r.get(2)?,
            path: PathBuf::from(path),
            content_type: r.get(4)?,
            total,
            spans,
            complete,
            bytes: bytes.max(0.0) as u64,
            source_tag: r.get(9)?,
            last_used_at: r.get(10)?,
        })
    }

    /// The (server, track, profile, path) identity writers merge into.
    pub fn key(&self) -> EntryKey {
        EntryKey {
            server_id: self.server_id.clone(),
            track_id: self.track_id.clone(),
            profile: self.profile.clone(),
            path: self.path.clone(),
        }
    }
}

/// Identifies one cache file: merges only land while the row still points
/// at `path` (a replaced or dropped entry never takes a stale writer's
/// bytes).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct EntryKey {
    pub server_id: String,
    pub track_id: String,
    pub profile: String,
    pub path: PathBuf,
}

/// What a span merge did.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MergeOutcome {
    /// The row still exists and took the spans.
    pub accepted: bool,
    /// The row covers the whole stream now (promoted by this merge or before).
    pub complete: bool,
    /// Tracks whose offline state changed (a promotion, evictions,
    /// redundant variants dropped).
    pub changed: Vec<TrackKey>,
}

/// Per-track facts eviction weighs, recorded by the actor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CacheSignal {
    /// Queued by autoplay.
    Autoplay,
    /// Left early by the user (before the scrobble-ish threshold).
    Skipped,
    /// Its first seconds were primed ahead of a possible play.
    Primed,
    /// It was played: no longer a primed-but-never-played entry.
    Played,
}

/// Everything eviction knows about one entry.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct EvictFacts {
    pub complete: bool,
    pub last_used_at: f64,
    pub loved: bool,
    pub rating: u32,
    /// Local plays (`play_history` rows).
    pub plays: u32,
    pub autoplay: bool,
    pub skips: u32,
    pub primed: bool,
}

const DAY_MS: f64 = 86_400_000.0;

/// Eviction order: lower goes first. The base is the last use (LRU); what
/// the listener cares about shifts it later, throwaways earlier:
/// - loved or rated 4+: 30 days later;
/// - each local play: 3 days later (up to 10 plays);
/// - an autoplay pick played at most once: 7 days earlier;
/// - skipped early and never played through: 14 days earlier;
/// - a partial entry: a day earlier, and a partial entry of a skipped or
///   primed-but-never-played track goes before everything else.
///
/// Pins are not in the stream cache; files in use are skipped by the caller.
pub fn eviction_score(f: &EvictFacts) -> f64 {
    let mut s = f.last_used_at;
    if f.loved || f.rating >= 4 {
        s += 30.0 * DAY_MS;
    }
    s += f64::from(f.plays.min(10)) * 3.0 * DAY_MS;
    if f.autoplay && f.plays <= 1 {
        s -= 7.0 * DAY_MS;
    }
    if f.skips > 0 && f.plays == 0 {
        s -= 14.0 * DAY_MS;
    }
    if !f.complete {
        s -= DAY_MS;
        if f.skips > 0 || f.primed {
            s -= 3650.0 * DAY_MS;
        }
    }
    s
}

/// Bytes served and fetched by the stream reader, for the "data saved"
/// figure. Persisted in `saved_state("storage:traffic")`.
#[derive(Debug, Default)]
pub struct Traffic {
    /// Bytes players read from pins or the stream cache.
    pub from_disk: AtomicU64,
    /// Bytes fetched from the server for players.
    pub fetched: AtomicU64,
    /// Bytes fetched from the server in the background (prefetch, primer).
    pub fetched_background: AtomicU64,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrafficTotals {
    pub from_disk: u64,
    pub fetched: u64,
    pub fetched_background: u64,
}

impl TrafficTotals {
    /// Bytes the network did not have to carry: player reads served from
    /// disk, less what background fetches spent filling the cache.
    pub fn saved(&self) -> u64 {
        self.from_disk.saturating_sub(self.fetched_background)
    }
}

impl Traffic {
    pub fn totals(&self) -> TrafficTotals {
        TrafficTotals {
            from_disk: self.from_disk.load(Ordering::Relaxed),
            fetched: self.fetched.load(Ordering::Relaxed),
            fetched_background: self.fetched_background.load(Ordering::Relaxed),
        }
    }

    fn set(&self, t: TrafficTotals) {
        self.from_disk.store(t.from_disk, Ordering::Relaxed);
        self.fetched.store(t.fetched, Ordering::Relaxed);
        self.fetched_background
            .store(t.fetched_background, Ordering::Relaxed);
    }
}

const TRAFFIC_KEY: &str = "storage:traffic";

impl Downloads {
    // -- rows -----------------------------------------------------------------

    /// The track's server metadata tag now (`None` when the mirror does not
    /// know the track).
    pub fn source_tag(&self, server_id: &str, track_id: &str) -> DbResult<Option<String>> {
        self.inner.db.with_conn(|c| {
            Ok(c.query_row(
                &format!(
                    "SELECT {SOURCE_TAG_SQL} FROM tracks t WHERE t.server_id = ?1 AND t.id = ?2"
                ),
                params![server_id, track_id],
                |r| r.get::<_, String>(0),
            )
            .optional()?)
        })
    }

    fn rows_where(&self, clause: &str, args: &[&dyn rusqlite::ToSql]) -> DbResult<Vec<CacheRow>> {
        self.inner.db.with_conn(|c| {
            let mut st = c.prepare_cached(&format!(
                "SELECT {ROW_COLUMNS} FROM cache_entries WHERE {clause}"
            ))?;
            let rows = st.query_map(args, CacheRow::from_row)?;
            Ok(rows.collect::<Result<Vec<_>, _>>()?)
        })
    }

    /// Every row for the track (any profile), unvalidated.
    pub fn cache_rows(&self, server_id: &str, track_id: &str) -> DbResult<Vec<CacheRow>> {
        self.rows_where(
            "server_id = ?1 AND track_id = ?2 ORDER BY last_used_at DESC, rowid DESC",
            &[&server_id, &track_id],
        )
    }

    /// Whether a removal of `path` is pending (it must not be served).
    pub(crate) fn is_doomed(&self, path: &Path) -> bool {
        self.inner
            .cache_use
            .lock()
            .doomed
            .iter()
            .any(|d| d.path == path)
    }

    /// Check a row against the disk and the mirror. A missing or short file,
    /// or a track whose server metadata changed since, is a miss: the row
    /// goes (the file too, once nobody reads it) and `changed` learns the
    /// track when its offline state may have moved.
    fn validate(&self, row: &CacheRow, changed: &mut Vec<TrackKey>) -> DbResult<bool> {
        if self.is_doomed(&row.path) {
            return Ok(false);
        }
        let len = std::fs::metadata(&row.path).ok().map(|m| m.len());
        let needed = if row.complete {
            row.total.unwrap_or(row.bytes)
        } else {
            row.spans.max_end()
        };
        let file_ok = len.is_some_and(|l| l >= needed);
        let tag_ok = match &row.source_tag {
            None => true,
            Some(tag) => match self.source_tag(&row.server_id, &row.track_id)? {
                Some(now) => &now == tag,
                None => true,
            },
        };
        if file_ok && tag_ok {
            return Ok(true);
        }
        tracing::debug!(
            track = %row.track_id,
            file_ok,
            tag_ok,
            "stream cache entry is stale; dropping it"
        );
        self.drop_row(row, changed)?;
        Ok(false)
    }

    /// Remove a row and its file; while the file is in use the removal is
    /// deferred (the row is detached from new readers at once).
    pub(crate) fn drop_row(&self, row: &CacheRow, changed: &mut Vec<TrackKey>) -> DbResult<()> {
        let in_use = {
            let mut u = self.inner.cache_use.lock();
            let busy = u.in_use(&row.server_id, &row.track_id, &row.path);
            if busy && !u.doomed.iter().any(|d| d.path == row.path) {
                u.doomed.push(Doomed {
                    server_id: row.server_id.clone(),
                    track_id: row.track_id.clone(),
                    profile: row.profile.clone(),
                    path: row.path.clone(),
                });
            }
            busy
        };
        if !in_use {
            if let Err(e) = remove_if_exists(&row.path.to_string_lossy()) {
                tracing::warn!(error = %e, "removing stale cache file");
            }
        }
        self.inner.db.with_conn(|c| {
            c.execute(
                "DELETE FROM cache_entries WHERE server_id = ?1 AND track_id = ?2 AND profile = ?3 AND path = ?4",
                params![row.server_id, row.track_id, row.profile, row.path.to_string_lossy().as_ref()],
            )?;
            Ok(())
        })?;
        if row.complete {
            self.refresh_offline(&row.server_id, &row.track_id)?;
            changed.push((row.server_id.clone(), row.track_id.clone()));
        }
        Ok(())
    }

    /// Whether the track's original file can be played where `profile` is
    /// what playback asks for: always for the original itself, otherwise
    /// unless the profile only transcodes because the platform cannot decode
    /// the track's format.
    pub fn original_decodable(
        &self,
        server_id: &str,
        track_id: &str,
        profile: Option<&TranscodingProfile>,
    ) -> DbResult<bool> {
        let Some(p) = profile else {
            return Ok(true);
        };
        let suffix: Option<String> = self.inner.db.with_conn(|c| {
            Ok(c.query_row(
                "SELECT suffix FROM tracks WHERE server_id = ?1 AND id = ?2",
                params![server_id, track_id],
                |r| r.get::<_, Option<String>>(0),
            )
            .optional()?
            .flatten())
        })?;
        Ok(!suffix.is_some_and(|s| p.cannot_decode.iter().any(|c| c.eq_ignore_ascii_case(&s))))
    }

    /// The complete entry to serve for (track, profile): the exact profile,
    /// else the original when it can be decoded (a full original serves any
    /// quality setting without a fetch), else another complete variant.
    /// Stale rows found on the way are dropped. `touch` stamps it as used.
    pub fn cache_complete(
        &self,
        server_id: &str,
        track_id: &str,
        profile: Option<&TranscodingProfile>,
        touch: bool,
    ) -> DbResult<(Option<CacheRow>, Vec<TrackKey>)> {
        let want = profile_key(profile);
        let mut rows: Vec<CacheRow> = self
            .cache_rows(server_id, track_id)?
            .into_iter()
            .filter(|r| r.complete)
            .collect();
        let original_ok = self.original_decodable(server_id, track_id, profile)?;
        rows.retain(|r| !r.profile.is_empty() || want.is_empty() || original_ok);
        rows.sort_by_key(|r| (r.profile != want, !r.profile.is_empty()));
        let mut changed = vec![];
        for row in rows {
            if self.validate(&row, &mut changed)? {
                if touch {
                    self.touch(&row)?;
                }
                return Ok((Some(row), changed));
            }
        }
        Ok((None, changed))
    }

    /// The partial entry for exactly (track, profile), if a valid one exists.
    pub fn cache_partial(
        &self,
        server_id: &str,
        track_id: &str,
        profile: Option<&TranscodingProfile>,
    ) -> DbResult<(Option<CacheRow>, Vec<TrackKey>)> {
        let want = profile_key(profile);
        let mut changed = vec![];
        let row = self
            .cache_rows(server_id, track_id)?
            .into_iter()
            .find(|r| !r.complete && r.profile == want);
        match row {
            Some(r) if self.validate(&r, &mut changed)? => Ok((Some(r), changed)),
            _ => Ok((None, changed)),
        }
    }

    fn touch(&self, row: &CacheRow) -> DbResult<()> {
        let now = self.now();
        self.inner.db.with_conn(|c| {
            c.execute(
                "UPDATE cache_entries SET last_used_at = ?4 WHERE server_id = ?1 AND track_id = ?2 AND profile = ?3",
                params![row.server_id, row.track_id, row.profile, now],
            )?;
            Ok(())
        })
    }

    /// Start (or join) the partial entry a reader writes into: the existing
    /// row for (track, profile), or a new one at a fresh file. A row whose
    /// total differs from `total` is reset (the server's stream changed).
    /// The file exists (sparse, `total` long when known) on return.
    #[allow(clippy::too_many_arguments)]
    pub fn cache_begin(
        &self,
        server_id: &str,
        track_id: &str,
        profile: Option<&TranscodingProfile>,
        suffix: Option<&str>,
        total: Option<u64>,
        content_type: Option<&str>,
        source_tag: Option<&str>,
    ) -> DbResult<Option<CacheRow>> {
        let key = profile_key(profile);
        let now = self.now();
        // A doomed row still holds the key: detach it (its file goes when
        // released) so the new entry gets a file of its own.
        let doomed: Vec<PathBuf> = self
            .inner
            .cache_use
            .lock()
            .doomed
            .iter()
            .map(|d| d.path.clone())
            .collect();
        for p in doomed {
            self.inner.db.with_conn(|c| {
                c.execute(
                    "DELETE FROM cache_entries WHERE server_id = ?1 AND track_id = ?2 AND profile = ?3 AND path = ?4",
                    params![server_id, track_id, key, p.to_string_lossy().as_ref()],
                )?;
                Ok(())
            })?;
        }
        let fresh = self.cache_path_unique(server_id, track_id, profile, suffix);
        self.inner.db.with_conn(|c| {
            c.execute(
                "INSERT OR IGNORE INTO cache_entries(server_id, track_id, path, bytes, content_type, profile, created_at, last_used_at, complete, total_bytes, spans, source_tag)
                 VALUES (?1, ?2, ?3, 0, ?4, ?5, ?6, ?6, 0, ?7, '', ?8)",
                params![server_id, track_id, fresh.to_string_lossy().as_ref(), content_type, key, now, total.map(|t| t as f64), source_tag],
            )?;
            Ok(())
        })?;
        let Some(mut row) = self
            .cache_rows(server_id, track_id)?
            .into_iter()
            .find(|r| r.profile == key)
        else {
            return Ok(None);
        };
        if row.complete {
            return Ok(None);
        }
        if row.total.is_some() && total.is_some() && row.total != total {
            self.cache_reset(&row.key(), total, content_type)?;
            row.total = total;
            row.spans.clear();
            row.bytes = 0;
        }
        if let Some(parent) = row.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let f = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(&row.path)?;
        if let Some(t) = row.total {
            if f.metadata()?.len() < t {
                f.set_len(t)?;
            }
        }
        Ok(Some(row))
    }

    /// The stream behind an entry changed (another length or type): forget
    /// its spans, keep the file.
    pub fn cache_reset(
        &self,
        key: &EntryKey,
        total: Option<u64>,
        content_type: Option<&str>,
    ) -> DbResult<()> {
        self.inner.db.with_conn(|c| {
            c.execute(
                "UPDATE cache_entries SET spans = '', bytes = 0, complete = 0, total_bytes = ?5, content_type = COALESCE(?6, content_type) WHERE server_id = ?1 AND track_id = ?2 AND profile = ?3 AND path = ?4",
                params![key.server_id, key.track_id, key.profile, key.path.to_string_lossy().as_ref(), total.map(|t| t as f64), content_type],
            )?;
            Ok(())
        })?;
        if let (Some(t), Ok(f)) = (
            total,
            std::fs::OpenOptions::new().write(true).open(&key.path),
        ) {
            let _ = f.set_len(t);
        }
        Ok(())
    }

    /// Merge spans a reader wrote (and flushed) into its entry. Only lands
    /// while the row still points at the writer's file and agrees on the
    /// total. Promotes the row when the spans cover the whole stream, then
    /// drops variants a complete original makes redundant and enforces the
    /// budget.
    pub fn cache_merge(
        &self,
        key: &EntryKey,
        add: &SpanSet,
        total: Option<u64>,
        content_type: Option<&str>,
    ) -> DbResult<MergeOutcome> {
        let now = self.now();
        let path = key.path.to_string_lossy().into_owned();
        let merged: Option<(bool, bool)> = self.inner.db.with_tx(|tx| {
            let row = tx
                .query_row(
                    &format!("SELECT {ROW_COLUMNS} FROM cache_entries WHERE server_id = ?1 AND track_id = ?2 AND profile = ?3 AND path = ?4"),
                    params![key.server_id, key.track_id, key.profile, path],
                    CacheRow::from_row,
                )
                .optional()?;
            let Some(row) = row else {
                return Ok(None);
            };
            if row.complete {
                return Ok(Some((true, false)));
            }
            let total = match (row.total, total) {
                (Some(a), Some(b)) if a != b => return Ok(None),
                (a, b) => a.or(b),
            };
            let mut spans = row.spans.clone();
            spans.union(add);
            if let Some(t) = total {
                spans.truncate(t);
            }
            let complete = total.is_some_and(|t| spans.covers(t));
            if complete {
                spans = SpanSet::of(0, total.unwrap_or(0));
            }
            tx.execute(
                "UPDATE cache_entries SET spans = ?5, bytes = ?6, complete = ?7, total_bytes = ?8, content_type = COALESCE(content_type, ?9), last_used_at = ?10 WHERE server_id = ?1 AND track_id = ?2 AND profile = ?3 AND path = ?4",
                params![
                    key.server_id,
                    key.track_id,
                    key.profile,
                    path,
                    spans.encode(),
                    spans.bytes() as f64,
                    complete as i64,
                    total.map(|t| t as f64),
                    content_type,
                    now
                ],
            )?;
            Ok(Some((true, complete)))
        })?;
        let Some((accepted, promoted)) = merged else {
            return Ok(MergeOutcome::default());
        };
        let mut out = MergeOutcome {
            accepted,
            complete: promoted,
            changed: vec![],
        };
        if promoted {
            self.refresh_offline(&key.server_id, &key.track_id)?;
            out.changed
                .push((key.server_id.clone(), key.track_id.clone()));
            if key.profile.is_empty() {
                self.drop_redundant_variants(&key.server_id, &key.track_id, &mut out.changed)?;
            }
        } else if let Ok(Some(row)) = self
            .cache_rows(&key.server_id, &key.track_id)
            .map(|rows| rows.into_iter().find(|r| r.path == key.path))
        {
            out.complete = row.complete;
        }
        out.changed.extend(self.evict_over_budget()?);
        Ok(out)
    }

    /// A complete original serves every quality setting: transcoded
    /// variants of the track (complete or partial) are dead weight.
    fn drop_redundant_variants(
        &self,
        server_id: &str,
        track_id: &str,
        changed: &mut Vec<TrackKey>,
    ) -> DbResult<()> {
        if !self.original_decodable(server_id, track_id, None)? {
            return Ok(());
        }
        for row in self.cache_rows(server_id, track_id)? {
            if !row.profile.is_empty() {
                self.drop_row(&row, changed)?;
            }
        }
        Ok(())
    }

    /// Drop the entry a writer created when it never landed a byte (a
    /// failed first fetch), so empty rows do not linger.
    pub fn cache_discard_if_empty(&self, key: &EntryKey) -> DbResult<()> {
        let rows = self.cache_rows(&key.server_id, &key.track_id)?;
        if let Some(row) = rows
            .into_iter()
            .find(|r| r.path == key.path && !r.complete && r.spans.is_empty())
        {
            let mut changed = vec![];
            self.drop_row(&row, &mut changed)?;
        }
        Ok(())
    }

    /// Bytes held by partial entries.
    pub fn partial_cache_bytes(&self) -> DbResult<f64> {
        self.inner.db.with_conn(|c| {
            Ok(c.query_row(
                "SELECT COALESCE(SUM(bytes), 0) FROM cache_entries WHERE complete = 0",
                [],
                |r| r.get(0),
            )?)
        })
    }

    // -- server changes and the disk --------------------------------------------

    /// After a library sync: drop every entry whose track's server metadata
    /// (size, suffix, content type, change markers) no longer matches what
    /// it was fetched against. Returns the tracks whose offline state
    /// changed.
    pub fn invalidate_changed_sources(&self) -> DbResult<Vec<TrackKey>> {
        let stale: Vec<CacheRow> = self.inner.db.with_conn(|c| {
            let mut st = c.prepare_cached(&format!(
                "SELECT {} FROM cache_entries c JOIN tracks t ON t.server_id = c.server_id AND t.id = c.track_id
                 WHERE c.source_tag IS NOT NULL AND c.source_tag <> ({SOURCE_TAG_SQL})",
                ROW_COLUMNS
                    .split(", ")
                    .map(|col| format!("c.{col}"))
                    .collect::<Vec<_>>()
                    .join(", ")
            ))?;
            let rows = st.query_map([], CacheRow::from_row)?;
            Ok(rows.collect::<Result<Vec<_>, _>>()?)
        })?;
        let mut changed = vec![];
        for row in stale {
            tracing::info!(track = %row.track_id, "track changed on the server; dropping its cached audio");
            self.drop_row(&row, &mut changed)?;
        }
        Ok(changed)
    }

    /// Reconcile the index with the disk (startup, hourly): rows whose file
    /// is gone or shorter than they claim are removed, and tracks marked
    /// `Cached` without a complete entry are corrected. Returns the tracks
    /// whose offline state changed.
    pub fn reconcile_stream_cache(&self) -> DbResult<Vec<TrackKey>> {
        let rows = self.rows_where("1 = 1", &[])?;
        let mut changed = vec![];
        for row in rows {
            if self.is_doomed(&row.path) {
                continue;
            }
            let len = std::fs::metadata(&row.path).ok().map(|m| m.len());
            let needed = if row.complete {
                row.total.unwrap_or(row.bytes)
            } else {
                row.spans.max_end()
            };
            if len.is_none_or(|l| l < needed) {
                self.drop_row(&row, &mut changed)?;
            }
        }
        let wrong: Vec<(String, String)> = self.inner.db.with_conn(|c| {
            let mut st = c.prepare_cached(
                "SELECT t.server_id, t.id FROM tracks t WHERE t.offline = 1 AND NOT EXISTS (SELECT 1 FROM cache_entries c WHERE c.server_id = t.server_id AND c.track_id = t.id AND c.complete = 1)",
            )?;
            let rows = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
            Ok(rows.collect::<Result<Vec<_>, _>>()?)
        })?;
        for (sid, tid) in wrong {
            self.refresh_offline(&sid, &tid)?;
            changed.push((sid, tid));
        }
        changed.sort();
        changed.dedup();
        Ok(changed)
    }

    // -- eviction -------------------------------------------------------------

    /// Every entry with its eviction facts, in eviction order.
    pub fn eviction_order(&self) -> DbResult<Vec<(CacheRow, EvictFacts)>> {
        let mut rows: Vec<(CacheRow, EvictFacts, i64)> = self.inner.db.with_conn(|c| {
            let cols = ROW_COLUMNS
                .split(", ")
                .map(|col| format!("c.{col}"))
                .collect::<Vec<_>>()
                .join(", ");
            let mut st = c.prepare_cached(&format!(
                "SELECT {cols}, COALESCE(t.loved, 0), COALESCE(t.rating, 0),
                        (SELECT count(*) FROM play_history h WHERE h.server_id = c.server_id AND h.track_id = c.track_id),
                        COALESCE(s.autoplay, 0), COALESCE(s.skips, 0), COALESCE(s.primed, 0), c.rowid
                 FROM cache_entries c
                 LEFT JOIN tracks t ON t.server_id = c.server_id AND t.id = c.track_id
                 LEFT JOIN cache_signals s ON s.server_id = c.server_id AND s.track_id = c.track_id"
            ))?;
            let rows = st.query_map([], |r| {
                let row = CacheRow::from_row(r)?;
                let facts = EvictFacts {
                    complete: row.complete,
                    last_used_at: row.last_used_at,
                    loved: r.get::<_, i64>(11)? != 0,
                    rating: r.get::<_, i64>(12)?.max(0) as u32,
                    plays: r.get::<_, i64>(13)?.max(0) as u32,
                    autoplay: r.get::<_, i64>(14)? != 0,
                    skips: r.get::<_, i64>(15)?.max(0) as u32,
                    primed: r.get::<_, i64>(16)? != 0,
                };
                Ok((row, facts, r.get::<_, i64>(17)?))
            })?;
            Ok(rows.collect::<Result<Vec<_>, _>>()?)
        })?;
        rows.sort_by(|a, b| {
            eviction_score(&a.1)
                .total_cmp(&eviction_score(&b.1))
                .then(a.2.cmp(&b.2))
        });
        Ok(rows.into_iter().map(|(r, f, _)| (r, f)).collect())
    }

    /// Evict by [`eviction_score`] until under budget. Never touches
    /// downloads, and skips files in use. Returns the tracks evicted.
    pub fn evict_over_budget(&self) -> DbResult<Vec<TrackKey>> {
        let budget = *self.inner.cache_budget.read();
        let mut total = self.cache_bytes()?;
        if total <= budget {
            return Ok(vec![]);
        }
        let mut removed = vec![];
        for (row, _) in self.eviction_order()? {
            if total <= budget {
                break;
            }
            let busy = self
                .inner
                .cache_use
                .lock()
                .in_use(&row.server_id, &row.track_id, &row.path);
            if busy {
                continue;
            }
            if let Err(e) = remove_if_exists(&row.path.to_string_lossy()) {
                tracing::warn!(error = %e, "evicting cache file");
                continue;
            }
            self.inner.db.with_conn(|c| {
                c.execute(
                    "DELETE FROM cache_entries WHERE server_id = ?1 AND track_id = ?2 AND profile = ?3 AND path = ?4",
                    params![row.server_id, row.track_id, row.profile, row.path.to_string_lossy().as_ref()],
                )?;
                Ok(())
            })?;
            self.refresh_offline(&row.server_id, &row.track_id)?;
            total -= row.bytes as f64;
            removed.push((row.server_id, row.track_id));
        }
        Ok(removed)
    }

    /// Record a fact eviction weighs about a track.
    pub fn cache_signal(
        &self,
        server_id: &str,
        track_id: &str,
        signal: CacheSignal,
    ) -> DbResult<()> {
        let now = self.now();
        let set = match signal {
            CacheSignal::Autoplay => "autoplay = 1",
            CacheSignal::Skipped => "skips = skips + 1",
            CacheSignal::Primed => "primed = 1",
            CacheSignal::Played => "primed = 0",
        };
        let (autoplay, skips, primed) = match signal {
            CacheSignal::Autoplay => (1, 0, 0),
            CacheSignal::Skipped => (0, 1, 0),
            CacheSignal::Primed => (0, 0, 1),
            CacheSignal::Played => (0, 0, 0),
        };
        self.inner.db.with_conn(|c| {
            c.execute(
                &format!(
                    "INSERT INTO cache_signals(server_id, track_id, autoplay, skips, primed, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                     ON CONFLICT(server_id, track_id) DO UPDATE SET {set}, updated_at = excluded.updated_at"
                ),
                params![server_id, track_id, autoplay, skips, primed, now],
            )?;
            Ok(())
        })
    }

    // -- traffic ----------------------------------------------------------------

    pub fn traffic(&self) -> &Traffic {
        &self.inner.traffic
    }

    pub(crate) fn load_traffic(&self) {
        if let Ok(Some(t)) = self.inner.db.saved_state_get::<TrafficTotals>(TRAFFIC_KEY) {
            self.inner.traffic.set(t);
        }
    }

    /// Persist the traffic counters (when they moved since the last save).
    pub fn save_traffic(&self) {
        let now = self.inner.traffic.totals();
        let mut last = self.inner.traffic_saved.lock();
        if *last == Some(now) {
            return;
        }
        match self
            .inner
            .db
            .saved_state_set(TRAFFIC_KEY, &now, self.inner.clock.as_ref())
        {
            Ok(()) => *last = Some(now),
            Err(e) => tracing::warn!(error = %e, "saving stream traffic counters"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn facts(last: f64) -> EvictFacts {
        EvictFacts {
            complete: true,
            last_used_at: last,
            ..Default::default()
        }
    }

    #[test]
    fn scores_keep_what_the_listener_cares_about() {
        let now = 1000.0 * DAY_MS;
        let plain = facts(now);
        let loved = EvictFacts {
            loved: true,
            ..facts(now - 20.0 * DAY_MS)
        };
        let rated = EvictFacts {
            rating: 4,
            ..facts(now - 20.0 * DAY_MS)
        };
        let frequent = EvictFacts {
            plays: 5,
            ..facts(now - 10.0 * DAY_MS)
        };
        let one_off = EvictFacts {
            autoplay: true,
            plays: 1,
            ..facts(now)
        };
        let skipped = EvictFacts {
            skips: 1,
            ..facts(now)
        };
        let skipped_partial = EvictFacts {
            complete: false,
            skips: 1,
            ..facts(now + 100.0 * DAY_MS)
        };
        let primed_partial = EvictFacts {
            complete: false,
            primed: true,
            ..facts(now + 100.0 * DAY_MS)
        };
        let s = eviction_score;
        // Loved / 4+ outlive a plain entry used later.
        assert!(s(&loved) > s(&plain) && s(&rated) > s(&plain));
        assert!(s(&frequent) > s(&plain));
        // Throwaways go before a plain entry used at the same time.
        assert!(s(&one_off) < s(&plain));
        assert!(s(&skipped) < s(&one_off));
        // Partial entries of skipped or primed-never-played tracks go first.
        for x in [plain, loved, rated, frequent, one_off, skipped] {
            assert!(s(&skipped_partial) < s(&x));
            assert!(s(&primed_partial) < s(&x));
        }
        // A loved track eventually goes (it is a shift, not a pin).
        let ancient_loved = EvictFacts {
            loved: true,
            ..facts(now - 400.0 * DAY_MS)
        };
        assert!(s(&ancient_loved) < s(&plain));
        // Plays count up to ten.
        let many = EvictFacts {
            plays: 50,
            ..facts(now)
        };
        let ten = EvictFacts {
            plays: 10,
            ..facts(now)
        };
        assert_eq!(s(&many), s(&ten));
    }

    #[test]
    fn traffic_saved_is_disk_reads_less_background_fetches() {
        let t = TrafficTotals {
            from_disk: 100,
            fetched: 50,
            fetched_background: 30,
        };
        assert_eq!(t.saved(), 70);
        let t = TrafficTotals {
            from_disk: 10,
            fetched: 0,
            fetched_background: 30,
        };
        assert_eq!(t.saved(), 0);
    }
}
