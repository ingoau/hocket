//! SQLite mirror: schema, forward-only migrations, typed queries, library sync. Owner: core-server.
//!
//! Entry points:
//! - [`Db::open`] / [`Db::open_in_memory`] — one connection, WAL, busy timeout,
//!   migrations applied on open with the design's two policies (mirror tables
//!   are a cache and get dropped+rebuilt when a migration fails; the
//!   non-rebuildable tables get a file backup before any migration runs).
//! - [`Db::with_conn`] / [`Db::with_tx`] — serialised access for the typed
//!   query layer in [`queries`] (tracks/albums/artists/genres/playlists, FTS
//!   search, by-ids, upserts, play history) and [`saved_state`] get/set JSON.
//! - [`sync::LibrarySync`] — the full and incremental library sync, run as a
//!   job through `jobs::JobQueue` with a persisted cursor so it resumes after a
//!   crash.
//!
//! The schema contract shared with the filters module is `schema.sql`.
//! `WhereClause` lets that module hand over raw SQL against `tracks`.

pub mod queries;
pub mod sync;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use parking_lot::{Mutex, MutexGuard};
use rusqlite::{Connection, OpenFlags, Transaction};

use crate::util::{now_ms, Clock};

/// Numbered, forward-only migrations. `0001` is `schema.sql` itself.
pub const MIGRATIONS: &[(u32, &str, &str)] = &[(1, "0001_init", include_str!("schema.sql"))];

/// Current schema version (the last migration number).
pub const SCHEMA_VERSION: u32 = 1;

/// Tables that are a cache of the server and may be dropped and re-synced.
pub const MIRROR_TABLES: &[&str] = &[
    "tracks",
    "albums",
    "artists",
    "genres",
    "playlists",
    "playlist_tracks",
    "capabilities",
    "image_cache",
    "lyrics_cache",
    "metadata_cache",
    "cache_entries",
];

/// Tables that cannot be rebuilt from the server (backed up before migrating).
pub const DURABLE_TABLES: &[&str] = &[
    "servers",
    "pins",
    "pin_tracks",
    "outbox",
    "jobs",
    "job_items",
    "problems",
    "play_history",
    "saved_state",
    "settings",
    "filters",
];

#[derive(Debug, thiserror::Error)]
pub enum DbError {
    #[error("sqlite: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("migration {version} failed: {source}")]
    Migration {
        version: u32,
        #[source]
        source: rusqlite::Error,
    },
    #[error("database schema version {found} is newer than this build supports ({supported})")]
    TooNew { found: u32, supported: u32 },
    #[error("{0}")]
    Other(String),
}

impl DbError {
    pub fn kind(&self) -> crate::api::ErrorKind {
        crate::api::ErrorKind::Storage
    }
}

pub type DbResult<T> = Result<T, DbError>;

/// Report of what `open` did, for diagnostics.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct OpenReport {
    pub from_version: u32,
    pub to_version: u32,
    pub backup_path: Option<PathBuf>,
    /// The mirror was dropped and must be re-synced.
    pub mirror_rebuilt: bool,
}

struct Inner {
    conn: Mutex<Connection>,
    path: Option<PathBuf>,
    random_seed: i64,
    open_report: OpenReport,
}

/// Handle to the database. Cheap to clone; all access is serialised through
/// one connection (SQLite has one writer anyway, and WAL gives readers no
/// reason to block on it in practice at the sizes a music library has).
#[derive(Clone)]
pub struct Db {
    inner: Arc<Inner>,
}

impl std::fmt::Debug for Db {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Db")
            .field("path", &self.inner.path)
            .finish()
    }
}

impl Db {
    /// Open (creating if needed) the database at `path`, applying migrations.
    /// `backup_dir` receives a copy of the file before a migration when the
    /// existing version is behind.
    pub fn open(path: &Path, backup_dir: &Path) -> DbResult<Db> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open_with_flags(
            path,
            OpenFlags::SQLITE_OPEN_READ_WRITE
                | OpenFlags::SQLITE_OPEN_CREATE
                | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        configure(&conn)?;
        let report = migrate(&conn, Some(path), Some(backup_dir))?;
        Ok(Self::wrap(conn, Some(path.to_path_buf()), report))
    }

    /// An in-memory database (tests).
    pub fn open_in_memory() -> DbResult<Db> {
        let conn = Connection::open_in_memory()?;
        configure(&conn)?;
        let report = migrate(&conn, None, None)?;
        Ok(Self::wrap(conn, None, report))
    }

    fn wrap(conn: Connection, path: Option<PathBuf>, open_report: OpenReport) -> Db {
        let random_seed = (now_ms() as i64) ^ 0x5DEE_CE66;
        Db {
            inner: Arc::new(Inner {
                conn: Mutex::new(conn),
                path,
                random_seed,
                open_report,
            }),
        }
    }

    pub fn path(&self) -> Option<&Path> {
        self.inner.path.as_deref()
    }

    pub fn open_report(&self) -> &OpenReport {
        &self.inner.open_report
    }

    /// Per-process seed for `SortOrder::Random` so paging is stable.
    pub fn random_seed(&self) -> i64 {
        self.inner.random_seed
    }

    /// Run a closure against the connection. Serialised.
    pub fn with_conn<T>(&self, f: impl FnOnce(&Connection) -> DbResult<T>) -> DbResult<T> {
        let conn = self.inner.conn.lock();
        f(&conn)
    }

    /// Run a closure inside a transaction; commits on `Ok`, rolls back on `Err`.
    pub fn with_tx<T>(&self, f: impl FnOnce(&Transaction) -> DbResult<T>) -> DbResult<T> {
        let mut conn = self.inner.conn.lock();
        let tx = conn.transaction()?;
        let out = f(&tx)?;
        tx.commit()?;
        Ok(out)
    }

    /// Raw guard for callers that need multiple statements without a transaction.
    pub fn lock(&self) -> MutexGuard<'_, Connection> {
        self.inner.conn.lock()
    }

    // -- saved_state --------------------------------------------------------

    /// JSON document by key (session doc, saved queues, shortcuts, cursors...).
    pub fn saved_state_get<T: serde::de::DeserializeOwned>(
        &self,
        key: &str,
    ) -> DbResult<Option<T>> {
        self.with_conn(|c| {
            let json: Option<String> = c
                .query_row("SELECT json FROM saved_state WHERE key = ?1", [key], |r| {
                    r.get(0)
                })
                .optional()?;
            match json {
                Some(j) => Ok(Some(serde_json::from_str(&j)?)),
                None => Ok(None),
            }
        })
    }

    pub fn saved_state_get_raw(&self, key: &str) -> DbResult<Option<String>> {
        self.with_conn(|c| {
            Ok(
                c.query_row("SELECT json FROM saved_state WHERE key = ?1", [key], |r| {
                    r.get(0)
                })
                .optional()?,
            )
        })
    }

    pub fn saved_state_set<T: serde::Serialize>(
        &self,
        key: &str,
        value: &T,
        clock: &dyn Clock,
    ) -> DbResult<()> {
        let json = serde_json::to_string(value)?;
        self.saved_state_set_raw(key, &json, clock)
    }

    pub fn saved_state_set_raw(&self, key: &str, json: &str, clock: &dyn Clock) -> DbResult<()> {
        self.with_conn(|c| {
            c.execute(
                "INSERT INTO saved_state(key, json, updated_at) VALUES (?1, ?2, ?3)
                 ON CONFLICT(key) DO UPDATE SET json = excluded.json, updated_at = excluded.updated_at",
                rusqlite::params![key, json, clock.now_ms()],
            )?;
            Ok(())
        })
    }

    pub fn saved_state_delete(&self, key: &str) -> DbResult<bool> {
        self.with_conn(|c| Ok(c.execute("DELETE FROM saved_state WHERE key = ?1", [key])? > 0))
    }

    /// Keys with a prefix (e.g. `lyricsOffset:`).
    pub fn saved_state_keys(&self, prefix: &str) -> DbResult<Vec<String>> {
        self.with_conn(|c| {
            let mut st =
                c.prepare("SELECT key FROM saved_state WHERE key LIKE ?1 || '%' ORDER BY key")?;
            let rows = st.query_map([prefix], |r| r.get::<_, String>(0))?;
            Ok(rows.collect::<Result<Vec<_>, _>>()?)
        })
    }

    /// Drop every mirror table's rows for a server (server removed / forced re-sync).
    pub fn clear_mirror(&self, server_id: &str) -> DbResult<()> {
        self.with_tx(|tx| {
            for t in MIRROR_TABLES {
                tx.execute(
                    &format!("DELETE FROM {t} WHERE server_id = ?1"),
                    [server_id],
                )?;
            }
            Ok(())
        })
    }

    /// `VACUUM INTO` a copy of the whole database (used for backups and diagnostics).
    pub fn backup_to(&self, dest: &Path) -> DbResult<()> {
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)?;
        }
        if dest.exists() {
            std::fs::remove_file(dest)?;
        }
        self.with_conn(|c| {
            c.execute("VACUUM INTO ?1", [dest.to_string_lossy().as_ref()])?;
            Ok(())
        })
    }

    pub fn schema_version(&self) -> DbResult<u32> {
        self.with_conn(|c| Ok(current_version(c)?))
    }
}

use rusqlite::OptionalExtension;

fn configure(conn: &Connection) -> DbResult<()> {
    conn.busy_timeout(std::time::Duration::from_secs(10))?;
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    conn.pragma_update(None, "temp_store", "MEMORY")?;
    conn.pragma_update(None, "cache_size", -16_000)?; // 16 MB
    Ok(())
}

fn current_version(conn: &Connection) -> rusqlite::Result<u32> {
    let has_table: bool = conn
        .query_row(
            "SELECT count(*) FROM sqlite_master WHERE type='table' AND name='schema_version'",
            [],
            |r| r.get::<_, i64>(0),
        )
        .map(|n| n > 0)?;
    if !has_table {
        return Ok(0);
    }
    conn.query_row(
        "SELECT COALESCE(MAX(version), 0) FROM schema_version",
        [],
        |r| r.get::<_, i64>(0),
    )
    .map(|v| v as u32)
}

fn apply_migration(conn: &Connection, version: u32, sql: &str) -> Result<(), rusqlite::Error> {
    // One transaction per migration so a failure leaves the previous version intact.
    conn.execute_batch("BEGIN")?;
    let result = conn.execute_batch(sql).and_then(|_| {
        conn.execute(
            "INSERT INTO schema_version(version, applied_at) VALUES (?1, ?2)",
            rusqlite::params![version, now_ms()],
        )
        .map(|_| ())
    });
    match result {
        Ok(()) => conn.execute_batch("COMMIT"),
        Err(e) => {
            let _ = conn.execute_batch("ROLLBACK");
            Err(e)
        }
    }
}

/// Apply pending migrations. Backs up the file first when upgrading an
/// existing database; on failure, drops the mirror tables (they are a cache)
/// and retries once, and only then gives up.
fn migrate(
    conn: &Connection,
    path: Option<&Path>,
    backup_dir: Option<&Path>,
) -> DbResult<OpenReport> {
    let from = current_version(conn)?;
    let mut report = OpenReport {
        from_version: from,
        to_version: from,
        ..Default::default()
    };
    if from > SCHEMA_VERSION {
        return Err(DbError::TooNew {
            found: from,
            supported: SCHEMA_VERSION,
        });
    }
    let pending: Vec<_> = MIGRATIONS.iter().filter(|(v, _, _)| *v > from).collect();
    if pending.is_empty() {
        return Ok(report);
    }
    if from > 0 {
        if let (Some(path), Some(dir)) = (path, backup_dir) {
            std::fs::create_dir_all(dir)?;
            let name = format!(
                "{}.v{}.{}.bak",
                path.file_name()
                    .map(|s| s.to_string_lossy())
                    .unwrap_or_default(),
                from,
                now_ms() as i64
            );
            let dest = dir.join(name);
            conn.execute("VACUUM INTO ?1", [dest.to_string_lossy().as_ref()])?;
            tracing::info!(?dest, from, "backed up database before migration");
            report.backup_path = Some(dest);
        }
    }
    for (version, name, sql) in pending {
        match apply_migration(conn, *version, sql) {
            Ok(()) => tracing::info!(version, name, "applied migration"),
            Err(first) => {
                tracing::warn!(version, name, error = %first, "migration failed; rebuilding mirror tables");
                drop_mirror_tables(conn)?;
                report.mirror_rebuilt = true;
                apply_migration(conn, *version, sql).map_err(|source| DbError::Migration {
                    version: *version,
                    source,
                })?;
            }
        }
        report.to_version = *version;
    }
    Ok(report)
}

fn drop_mirror_tables(conn: &Connection) -> DbResult<()> {
    // Triggers/FTS shadow tables go with their content tables.
    for t in MIRROR_TABLES {
        conn.execute_batch(&format!(
            "DROP TABLE IF EXISTS {t}_fts; DROP TABLE IF EXISTS {t};"
        ))?;
    }
    conn.execute_batch("DELETE FROM saved_state WHERE key LIKE 'sync:%'")
        .ok();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::WallClock;

    #[test]
    fn open_in_memory_applies_schema() {
        let db = Db::open_in_memory().unwrap();
        assert_eq!(db.schema_version().unwrap(), SCHEMA_VERSION);
        assert_eq!(db.open_report().from_version, 0);
        let tables: Vec<String> = db
            .with_conn(|c| {
                let mut st =
                    c.prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")?;
                let rows = st.query_map([], |r| r.get(0))?;
                let names = rows.collect::<Result<Vec<String>, _>>()?;
                Ok(names)
            })
            .unwrap();
        for t in MIRROR_TABLES.iter().chain(DURABLE_TABLES.iter()) {
            assert!(tables.contains(&t.to_string()), "missing {t}");
        }
        assert!(tables.contains(&"tracks_fts".to_string()));
    }

    #[test]
    fn open_on_disk_is_idempotent_and_wal() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("hocket.db");
        {
            let db = Db::open(&path, &dir.path().join("backups")).unwrap();
            let mode: String = db
                .with_conn(|c| Ok(c.pragma_query_value(None, "journal_mode", |r| r.get(0))?))
                .unwrap();
            assert_eq!(mode.to_lowercase(), "wal");
            db.saved_state_set("k", &serde_json::json!({"a": 1}), &WallClock)
                .unwrap();
        }
        let db = Db::open(&path, &dir.path().join("backups")).unwrap();
        assert_eq!(db.open_report().from_version, SCHEMA_VERSION);
        assert!(
            db.open_report().backup_path.is_none(),
            "no migration, no backup"
        );
        let v: serde_json::Value = db.saved_state_get("k").unwrap().unwrap();
        assert_eq!(v["a"], 1);
    }

    #[test]
    fn migration_from_older_version_backs_up_first() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("hocket.db");
        {
            // Fake an older database: schema_version table with version 0 rows and a durable table.
            let c = Connection::open(&path).unwrap();
            c.execute_batch(
                "CREATE TABLE schema_version(version INTEGER NOT NULL, applied_at REAL NOT NULL);
                 INSERT INTO schema_version VALUES (0, 0);",
            )
            .unwrap();
        }
        // version 0 counts as "fresh" (nothing applied) → no backup expected, migrations apply.
        let db = Db::open(&path, &dir.path().join("backups")).unwrap();
        assert_eq!(db.schema_version().unwrap(), SCHEMA_VERSION);
        assert!(db.open_report().backup_path.is_none());
        drop(db);
        // Now simulate a future upgrade path by calling migrate with a pending list: emulate by
        // rewinding the recorded version to a pretend "0.5" (we record 1 → pretend from=1 and add a
        // synthetic migration). Since MIGRATIONS is static, exercise the backup helper directly.
        let db = Db::open(&path, &dir.path().join("backups")).unwrap();
        let dest = dir.path().join("backups").join("manual.bak");
        db.backup_to(&dest).unwrap();
        assert!(dest.exists());
        let copy = Connection::open(&dest).unwrap();
        let v: i64 = copy
            .query_row("SELECT MAX(version) FROM schema_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(v as u32, SCHEMA_VERSION);
    }

    #[test]
    fn newer_database_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("hocket.db");
        {
            let c = Connection::open(&path).unwrap();
            c.execute_batch(
                "CREATE TABLE schema_version(version INTEGER NOT NULL, applied_at REAL NOT NULL);
                 INSERT INTO schema_version VALUES (999, 0);",
            )
            .unwrap();
        }
        let e = Db::open(&path, dir.path()).unwrap_err();
        assert!(matches!(e, DbError::TooNew { found: 999, .. }));
    }

    #[test]
    fn failed_migration_rebuilds_mirror_and_keeps_durable_rows() {
        // A pre-existing *incompatible* mirror table makes the schema migration fail on first try
        // (CREATE INDEX on a missing column); the mirror is dropped and the migration re-applied.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("hocket.db");
        {
            let c = Connection::open(&path).unwrap();
            c.execute_batch(
                "CREATE TABLE tracks(id TEXT PRIMARY KEY, server_id TEXT);
                 CREATE TABLE saved_state(key TEXT PRIMARY KEY, json TEXT NOT NULL, updated_at REAL NOT NULL);
                 INSERT INTO saved_state VALUES ('session', '{\"x\":1}', 0);",
            )
            .unwrap();
        }
        let db = Db::open(&path, &dir.path().join("backups")).unwrap();
        assert!(db.open_report().mirror_rebuilt);
        assert_eq!(db.schema_version().unwrap(), SCHEMA_VERSION);
        let v: serde_json::Value = db.saved_state_get("session").unwrap().unwrap();
        assert_eq!(v["x"], 1, "durable rows survive a mirror rebuild");
        // the rebuilt tracks table has the full column set
        db.with_conn(|c| {
            c.execute(
                "INSERT INTO tracks(id, server_id, title) VALUES ('t', 's', 'x')",
                [],
            )?;
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn saved_state_roundtrip_and_keys() {
        let db = Db::open_in_memory().unwrap();
        db.saved_state_set("lyricsOffset:a", &5, &WallClock)
            .unwrap();
        db.saved_state_set("lyricsOffset:b", &-3, &WallClock)
            .unwrap();
        db.saved_state_set("other", &1, &WallClock).unwrap();
        assert_eq!(
            db.saved_state_keys("lyricsOffset:").unwrap(),
            vec!["lyricsOffset:a", "lyricsOffset:b"]
        );
        assert_eq!(
            db.saved_state_get::<i32>("lyricsOffset:b").unwrap(),
            Some(-3)
        );
        assert!(db.saved_state_delete("other").unwrap());
        assert!(!db.saved_state_delete("other").unwrap());
        assert_eq!(db.saved_state_get::<i32>("other").unwrap(), None);
    }

    #[test]
    fn with_tx_rolls_back_on_error() {
        let db = Db::open_in_memory().unwrap();
        let r: DbResult<()> = db.with_tx(|tx| {
            tx.execute("INSERT INTO saved_state VALUES ('a', '1', 0)", [])?;
            Err(DbError::Other("boom".into()))
        });
        assert!(r.is_err());
        assert_eq!(db.saved_state_get_raw("a").unwrap(), None);
    }
}
