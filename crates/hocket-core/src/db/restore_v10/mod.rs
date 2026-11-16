//! One-time restore from the pre-squash backup. Delete this module (and its
//! two call sites: `migrate` and `Actor::new`) once no device runs a
//! version-10 database.
//!
//! Devices released before the schema squash are on version 10. `migrate`
//! backs such a database up (`<backups>/<file>.v10.<ms>.bak`) and recreates
//! it. [`restore_in`] copies back what the server can't give back, from that
//! backup, in the same transaction as the drop and the new schema (so an
//! interrupted upgrade leaves the version-10 file as it was) and before
//! anything else reads the new database (the download and stream-cache
//! sweeps in particular):
//!
//! - **accounts**: each old `servers` row, under the id `AddAccount` derives
//!   for its URL and username (the same md5 of `scope_key` as before, so the
//!   ids are unchanged). The platforms replay their logins at start; finding
//!   the row, `AddAccount` installs the account at once, as it did before.
//!   The account whose session was used last comes first.
//! - **settings**: the settings document (`saved_state("settings")`; the
//!   `settings` table was never used, its rows are copied too), minus keys
//!   the registry doesn't know, values that no longer validate and
//!   `connect.coordinatorUrl` (a coordinator now admits paired identities
//!   only; claiming or redeeming an invite sets it again). Synced settings are
//!   not signed: they ride the session, which broadcasts them on open.
//! - **pins, pin_tracks, cache_entries, cache_signals, automix_records**:
//!   unchanged columns and keys (`server_id` is the account id, the source of
//!   a Navidrome track reference), and the file layout is the same
//!   (`<data>/downloads/<account>/…`, `<cache>/stream/<account>/…`), so the
//!   paths in the rows match the files and the startup sweeps keep them.
//! - **play_history** (ids kept: a pending scrobble names its row),
//!   **filters**, the **outbox** (pending, in-flight and failed rows; a track
//!   rating gains the `source` the new mutation carries). Download jobs are
//!   not: run before the platform signs the account in and its mirror is
//!   synced, every item would fail. A flag per account ([`REPLAN_PREFIX`])
//!   has its first finished sync plan the restored pins' wanted tracks
//!   (`Downloads::replan_wanted`), once.
//! - **device-local saved state**: volume, mute, renderer formats, traffic
//!   totals, autoplay exclusions, lyric offsets.
//! - **scope-keyed saved state** (the queue document, resume position,
//!   stash, snoozes, album resume points, plays awaiting a scrobble verdict):
//!   the session now belongs to the device's identity, which doesn't exist
//!   until the actor starts. These are converted to track references here
//!   and parked under [`PENDING_KEY`]; [`finish`] writes them under the
//!   identity's scope once it exists, and deletes the parked row in the same
//!   transaction.
//!
//! Not restored: the mirror and metadata caches (they resync), sync cursors
//! (the mirror is empty, so the next sync is a full one), the room's sync
//! base (`connect:`; a new identity has a new room), jobs and problems.
//!
//! Every part is best effort: a part that fails is rolled back to its
//! savepoint, logged and reported, and the rest carries on. When SQLite rolls
//! the whole transaction back instead (a full disk, an I/O error), the
//! restore is [`Aborted`]: nothing of it is kept, and the recreation is
//! redone without it, writing [`RETRY_KEY`] in the same transaction. The
//! next open retries once ([`after_open`]); that database has served a
//! session meanwhile, so rows already there win and the old outbox is left
//! out (stale writes sent over newer changes are worse than lost ones). A
//! retry that aborts again is recorded as done with its errors. Nothing here
//! can fail opening the database. A current database with neither record (a
//! fresh install, or one recreated by a build without this restore, such as
//! v0.1.605, and in use since) is recorded as done with nothing restored:
//! its version-10 backup is not restored into it.

use std::collections::{BTreeMap, HashMap};
use std::path::Path;

use rusqlite::types::Value as SqlValue;
use rusqlite::{params, Connection, OpenFlags, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::{Db, DbResult};
use crate::api::{AlbumResumeEntry, SessionDocument, Snooze, StashEntry};
use crate::util::Clock;

/// The only version restored: what released devices run.
pub const FROM_VERSION: u32 = 10;

/// `saved_state` key of the scope-keyed state waiting for the identity.
pub const PENDING_KEY: &str = "restore:v10:pending";

/// `saved_state` key recording that the restore ran (its report).
pub const DONE_KEY: &str = "restore:v10:done";

/// `saved_state` key asking the next open to retry an aborted restore (the
/// backup's path), written with the recreation that went ahead without it.
pub const RETRY_KEY: &str = "restore:v10:retry";

/// Prefix of the `saved_state` keys (one per account) asking the account's
/// next finished sync to plan its restored pins' wanted tracks, once.
pub const REPLAN_PREFIX: &str = "restore:v10:replan:";

/// The one-shot replan flag of `account` ([`REPLAN_PREFIX`]).
pub fn replan_key(account: &str) -> String {
    format!("{REPLAN_PREFIX}{account}")
}

/// Settings that are not restored although the registry knows them.
const DROPPED_SETTINGS: &[&str] = &[crate::settings::keys::CONNECT_COORDINATOR_URL];

/// Device-local `saved_state` keys copied as they are.
const LOCAL_STATE_KEYS: &[&str] = &[
    "volumeSlider",
    "volumeMuted",
    "volume",
    crate::downloads::receivers::STATE_KEY,
    "storage:traffic",
    "autoplay:exclusion",
];

/// Prefixes of device-local `saved_state` keys copied as they are.
const LOCAL_STATE_PREFIXES: &[&str] = &["lyricsOffset:"];

/// What [`restore`] did, per part (`OpenReport::restore`).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct RestoreReport {
    /// Rows (or documents) restored, by part.
    pub restored: BTreeMap<String, usize>,
    /// Rows left behind, by part (an unknown account, a format that no
    /// longer parses, a finished outbox row...).
    pub skipped: BTreeMap<String, usize>,
    /// Parts that failed, with the error.
    pub errors: Vec<String>,
}

impl RestoreReport {
    fn count(&mut self, part: &str, restored: usize, skipped: usize) {
        if restored > 0 {
            *self.restored.entry(part.into()).or_default() += restored;
        }
        if skipped > 0 {
            *self.skipped.entry(part.into()).or_default() += skipped;
        }
    }
}

/// Scope-keyed state parked until the identity exists ([`finish`]).
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Pending {
    session: Option<SessionDocument>,
    position: Option<Value>,
    stash: Vec<StashEntry>,
    snoozes: Vec<Snooze>,
    album_resumes: Vec<AlbumResumeEntry>,
    /// `core::state::PendingScrobble`s; their format did not change.
    pending_scrobbles: Vec<Value>,
}

impl Pending {
    fn is_empty(&self) -> bool {
        self.session.is_none()
            && self.position.is_none()
            && self.stash.is_empty()
            && self.snoozes.is_empty()
            && self.album_resumes.is_empty()
            && self.pending_scrobbles.is_empty()
    }
}

/// An old server, under its new account id.
struct OldServer {
    old_id: String,
    account_id: String,
    url: String,
    username: String,
    name: String,
    capabilities: String,
    created_at: f64,
}

struct Ctx<'a> {
    old: Connection,
    new: &'a Connection,
    /// Old server id → account id.
    accounts: HashMap<String, String>,
    /// Old session scope (`url|user`) → account id.
    scopes: HashMap<String, String>,
    /// The scope whose session document is restored (the one used last).
    session_scope: Option<String>,
    /// A retry on a database already used since its recreation.
    retry: bool,
}

impl Ctx<'_> {
    fn account(&self, old_server_id: &str) -> Option<&String> {
        self.accounts.get(old_server_id)
    }
}

/// The restore stopped because SQLite rolled the whole transaction back
/// under it (a full disk, an I/O error): nothing of it is kept and
/// [`DONE_KEY`] isn't written, so the next open tries again. Carries what
/// was reported up to then.
pub(super) struct Aborted(pub RestoreReport);

#[cfg(test)]
thread_local! {
    /// Tests: the part during which SQLite "rolls the transaction back".
    pub(super) static ABORT_AT: std::cell::Cell<Option<&'static str>> =
        const { std::cell::Cell::new(None) };
}

/// Restore from `backup` (a version-10 copy) into the current schema on
/// `conn`, inside the caller's open transaction, and record [`DONE_KEY`] in
/// it. A backup that can't be read is reported (and recorded as done: there
/// is nothing to retry); a transaction SQLite rolled back is [`Aborted`].
pub(super) fn restore_in(
    conn: &Connection,
    backup: &Path,
    retry: bool,
) -> Result<RestoreReport, Aborted> {
    let mut report = RestoreReport::default();
    let opened = open_backup(backup)
        .map_err(|e| format!("open backup: {e}"))
        .and_then(|old| {
            old_servers(&old)
                .map(|servers| (old, servers))
                .map_err(|e| format!("servers: {e}"))
        });
    let (old, servers) = match opened {
        Ok(x) => x,
        Err(e) => {
            tracing::warn!(error = %e, ?backup, "backup unreadable; nothing restored");
            report.errors.push(e);
            mark_done(conn, &report)?;
            return Ok(report);
        }
    };
    let mut ctx = Ctx {
        old,
        new: conn,
        accounts: servers
            .iter()
            .map(|s| (s.old_id.clone(), s.account_id.clone()))
            .collect(),
        scopes: servers
            .iter()
            .map(|s| {
                (
                    crate::connect::wire::scope_key(&s.url, &s.username),
                    s.account_id.clone(),
                )
            })
            .collect(),
        session_scope: None,
        retry,
    };
    ctx.session_scope = last_session_scope(&ctx).unwrap_or_else(|e| {
        report.errors.push(format!("session scope: {e}"));
        None
    });

    type Part = fn(&Ctx, &[OldServer]) -> DbResult<(usize, usize)>;
    let parts: &[(&str, Part)] = &[
        ("accounts", |c, s| restore_accounts(c, s)),
        ("settings", |c, _| restore_settings(c)),
        ("settingsRows", |c, _| restore_settings_rows(c)),
        ("localState", |c, _| restore_local_state(c)),
        ("pins", |c, _| copy_by_account(c, "pins", PINS)),
        ("pinTracks", |c, _| {
            copy_by_account(c, "pin_tracks", PIN_TRACKS)
        }),
        ("replanFlags", |c, _| flag_replans(c)),
        ("cacheEntries", |c, _| {
            copy_by_account(c, "cache_entries", CACHE_ENTRIES)
        }),
        ("cacheSignals", |c, _| {
            copy_by_account(c, "cache_signals", CACHE_SIGNALS)
        }),
        ("automixRecords", |c, _| {
            copy_by_account(c, "automix_records", AUTOMIX_RECORDS)
        }),
        ("playHistory", |c, _| restore_play_history(c)),
        ("filters", |c, _| restore_filters(c)),
        ("outbox", |c, _| restore_outbox(c)),
        ("deferred", |c, _| restore_deferred(c)),
    ];
    for (name, part) in parts {
        // Outside a transaction a savepoint would open (and its release
        // commit) one of its own: the rest would land piecemeal.
        if conn.is_autocommit() {
            return Err(abort(report, name, "the transaction is gone"));
        }
        let result = conn
            .execute_batch("SAVEPOINT restore_part")
            .map_err(super::DbError::from)
            .and_then(|()| {
                #[cfg(test)]
                if ABORT_AT.with(|a| a.get()) == Some(*name) {
                    let _ = conn.execute_batch("ROLLBACK");
                    return Err(super::DbError::Other(
                        "simulated: database or disk is full".into(),
                    ));
                }
                part(&ctx, &servers)
            });
        match result {
            Ok((restored, skipped)) => {
                if let Err(e) = conn.execute_batch("RELEASE restore_part") {
                    return Err(abort(report, name, &e.to_string()));
                }
                report.count(name, restored, skipped);
            }
            Err(e) if conn.is_autocommit() => {
                // SQLite rolled the whole transaction back (SQLITE_FULL,
                // an I/O error...): there is no savepoint to return to.
                return Err(abort(report, name, &e.to_string()));
            }
            Err(e) => {
                if let Err(e2) =
                    conn.execute_batch("ROLLBACK TO restore_part; RELEASE restore_part")
                {
                    return Err(abort(report, name, &format!("{e}; {e2}")));
                }
                tracing::warn!(part = name, error = %e, "restore: part failed; carrying on");
                report.errors.push(format!("{name}: {e}"));
            }
        }
    }
    mark_done(conn, &report)?;
    tracing::info!(
        restored = ?report.restored,
        skipped = ?report.skipped,
        errors = ?report.errors,
        "restored user data from the version-10 backup"
    );
    Ok(report)
}

fn abort(mut report: RestoreReport, part: &str, error: &str) -> Aborted {
    tracing::warn!(
        part,
        error,
        "restore aborted: the transaction was rolled back; next open retries"
    );
    report.errors.push(format!(
        "{part}: {error} (transaction rolled back; restore aborted)"
    ));
    Aborted(report)
}

fn mark_done(conn: &Connection, report: &RestoreReport) -> Result<(), Aborted> {
    if conn.is_autocommit() {
        return Err(abort(report.clone(), "done", "the transaction is gone"));
    }
    conn.execute(
        "INSERT OR REPLACE INTO saved_state(key, json, updated_at) VALUES (?1, ?2, ?3)",
        params![
            DONE_KEY,
            serde_json::to_string(report).unwrap_or_default(),
            crate::util::now_ms()
        ],
    )
    .map(|_| ())
    .map_err(|e| abort(report.clone(), "done", &e.to_string()))
}

/// Ask the next open to retry: in the caller's transaction, the one that
/// recreated the database without the restore.
pub(super) fn mark_retry(conn: &Connection, backup: &Path) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT OR REPLACE INTO saved_state(key, json, updated_at) VALUES (?1, ?2, ?3)",
        params![
            RETRY_KEY,
            json!(backup.to_string_lossy()).to_string(),
            crate::util::now_ms()
        ],
    )
    .map(|_| ())
}

/// Every open of a current database after `migrate`: retry the restore
/// [`mark_retry`] asked for, once. A database without a record either way
/// (a fresh install, or one recreated by a build without this restore and
/// used since: restoring the old data into it would bring back pins,
/// accounts and outbox writes the user has moved on from) is recorded as
/// done with nothing restored.
pub(super) fn after_open(conn: &Connection) -> Option<RestoreReport> {
    let get = |key: &str| -> Option<String> {
        conn.query_row("SELECT json FROM saved_state WHERE key = ?1", [key], |r| {
            r.get(0)
        })
        .optional()
        .ok()
        .flatten()
    };
    if get(DONE_KEY).is_some() {
        return None;
    }
    let Some(backup) = get(RETRY_KEY).and_then(|j| serde_json::from_str::<String>(&j).ok()) else {
        let _ = conn.execute(
            "INSERT OR IGNORE INTO saved_state(key, json, updated_at) VALUES (?1, '{}', ?2)",
            params![DONE_KEY, crate::util::now_ms()],
        );
        return None;
    };
    tracing::warn!(backup, "retrying the restore from the version-10 backup");
    Some(retry(conn, Path::new(&backup)))
}

/// The retry [`after_open`] runs, in a transaction of its own. The
/// database has served a session since it was recreated: rows already
/// there win, and the outbox is left out (old writes sent over newer
/// changes are worse than lost ones). It is tried once: if it aborts
/// again, it is recorded as done with the errors.
pub(super) fn retry(conn: &Connection, backup: &Path) -> RestoreReport {
    let attempt = conn
        .execute_batch("BEGIN IMMEDIATE")
        .and_then(|()| conn.execute("DELETE FROM saved_state WHERE key = ?1", [RETRY_KEY]))
        .map_err(|e| RestoreReport {
            errors: vec![format!("begin: {e}")],
            ..Default::default()
        })
        .and_then(|_| restore_in(conn, backup, true).map_err(|Aborted(r)| r))
        .and_then(|report| {
            conn.execute_batch("COMMIT")
                .map(|()| report)
                .map_err(|e| RestoreReport {
                    errors: vec![format!("commit: {e}")],
                    ..Default::default()
                })
        });
    match attempt {
        Ok(report) => report,
        Err(report) => {
            if !conn.is_autocommit() {
                let _ = conn.execute_batch("ROLLBACK");
            }
            tracing::warn!(errors = ?report.errors, "the restore retry failed too; giving up");
            let _ = conn.execute_batch("BEGIN IMMEDIATE").and_then(|()| {
                conn.execute("DELETE FROM saved_state WHERE key = ?1", [RETRY_KEY])?;
                conn.execute(
                    "INSERT OR REPLACE INTO saved_state(key, json, updated_at) VALUES (?1, ?2, ?3)",
                    params![
                        DONE_KEY,
                        serde_json::to_string(&report).unwrap_or_default(),
                        crate::util::now_ms()
                    ],
                )?;
                conn.execute_batch("COMMIT")
            });
            if !conn.is_autocommit() {
                let _ = conn.execute_batch("ROLLBACK");
            }
            report
        }
    }
}

/// The first version-10 backup of `db_path` in `dir`: the copy taken before
/// the upgrade. A later one (an upgrade interrupted by an older build, which
/// dropped tables one by one, and retried) may be of a half-emptied file.
pub(super) fn first_backup(dir: &Path, db_path: &Path) -> Option<std::path::PathBuf> {
    let prefix = format!(
        "{}.v{FROM_VERSION}.",
        db_path.file_name()?.to_string_lossy()
    );
    std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            let ms: i64 = name
                .strip_prefix(&prefix)?
                .strip_suffix(".bak")?
                .parse()
                .ok()?;
            Some((ms, e.path()))
        })
        .min_by_key(|(ms, _)| *ms)
        .map(|(_, p)| p)
}

/// Write the parked scope-keyed state under `scope` (this device's identity)
/// and drop the parked row, in one transaction. Keys that already hold
/// something are left alone. Returns how many documents were written; `0`
/// when nothing was parked.
pub fn finish(db: &Db, scope: &str, clock: &dyn Clock) -> usize {
    let result = db.with_tx(|tx| {
        let json: Option<String> = tx
            .query_row(
                "SELECT json FROM saved_state WHERE key = ?1",
                [PENDING_KEY],
                |r| r.get(0),
            )
            .optional()?;
        let Some(json) = json else { return Ok(0) };
        tx.execute("DELETE FROM saved_state WHERE key = ?1", [PENDING_KEY])?;
        let pending: Pending = match serde_json::from_str(&json) {
            Ok(p) => p,
            Err(e) => {
                tracing::warn!(error = %e, "parked restore state unreadable; dropped");
                return Ok(0);
            }
        };
        let now = clock.now_ms();
        let mut docs: Vec<(String, String)> = vec![];
        if let Some(mut doc) = pending.session {
            doc.scope = scope.to_string();
            docs.push((format!("session:{scope}"), serde_json::to_string(&doc)?));
        }
        if let Some(pos) = pending.position {
            docs.push((format!("position:{scope}"), pos.to_string()));
        }
        if !pending.stash.is_empty() {
            docs.push((
                crate::stash::state_key(scope),
                serde_json::to_string(&pending.stash)?,
            ));
        }
        if !pending.snoozes.is_empty() {
            docs.push((
                crate::snooze::state_key(scope),
                serde_json::to_string(&pending.snoozes)?,
            ));
        }
        if !pending.album_resumes.is_empty() {
            docs.push((
                crate::album_resume::state_key(scope),
                serde_json::to_string(&pending.album_resumes)?,
            ));
        }
        if !pending.pending_scrobbles.is_empty() {
            // `core::state::pending_scrobbles_key`.
            docs.push((
                format!("pendingScrobbles:{scope}"),
                serde_json::to_string(&pending.pending_scrobbles)?,
            ));
        }
        let mut written = 0;
        for (key, json) in docs {
            written += tx.execute(
                "INSERT INTO saved_state(key, json, updated_at) VALUES (?1, ?2, ?3)
                 ON CONFLICT(key) DO NOTHING",
                params![key, json, now],
            )?;
        }
        Ok(written)
    });
    match result {
        Ok(0) => 0,
        Ok(n) => {
            tracing::info!(
                documents = n,
                "restored the queue and session state from the version-10 backup"
            );
            n
        }
        Err(e) => {
            tracing::warn!(error = %e, "could not restore the parked session state");
            0
        }
    }
}

fn open_backup(path: &Path) -> rusqlite::Result<Connection> {
    let c = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    let version: i64 = c.query_row(
        "SELECT COALESCE(MAX(version), 0) FROM schema_version",
        [],
        |r| r.get(0),
    )?;
    if version != FROM_VERSION as i64 {
        return Err(rusqlite::Error::InvalidParameterName(format!(
            "backup is version {version}, not {FROM_VERSION}"
        )));
    }
    Ok(c)
}

fn old_servers(old: &Connection) -> rusqlite::Result<Vec<OldServer>> {
    let mut st = old.prepare(
        "SELECT id, url, username, name, capabilities, created_at FROM servers ORDER BY created_at",
    )?;
    let rows = st.query_map([], |r| {
        let url: String = r.get(1)?;
        let username: String = r.get(2)?;
        Ok(OldServer {
            old_id: r.get(0)?,
            account_id: crate::connect::wire::account_id_for_scope(
                &crate::connect::wire::scope_key(&url, &username),
            ),
            url,
            username,
            name: r.get(3)?,
            capabilities: r.get(4)?,
            created_at: r.get(5)?,
        })
    })?;
    rows.collect()
}

/// The scope of the session document saved last (an account's).
fn last_session_scope(ctx: &Ctx) -> rusqlite::Result<Option<String>> {
    let mut st = ctx.old.prepare(
        "SELECT substr(key, 9) FROM saved_state WHERE key LIKE 'session:%' ORDER BY updated_at DESC",
    )?;
    let scopes = st.query_map([], |r| r.get::<_, String>(0))?;
    for s in scopes {
        let s = s?;
        if ctx.scopes.contains_key(&s) {
            return Ok(Some(s));
        }
    }
    Ok(None)
}

fn restore_accounts(ctx: &Ctx, servers: &[OldServer]) -> DbResult<(usize, usize)> {
    let first = ctx
        .session_scope
        .as_ref()
        .and_then(|s| ctx.scopes.get(s))
        .cloned();
    let mut ordered: Vec<&OldServer> = servers.iter().collect();
    ordered.sort_by_key(|s| Some(&s.account_id) != first.as_ref());
    let mut n = 0;
    for (priority, s) in ordered.into_iter().enumerate() {
        n += ctx.new.execute(
            "INSERT INTO accounts(id, kind, url, username, name, last_sync, priority, server_capabilities, capabilities, created_at)
             VALUES (?1, 'navidrome', ?2, ?3, ?4, NULL, ?5, ?6, '{}', ?7)
             ON CONFLICT(id) DO NOTHING",
            params![s.account_id, s.url, s.username, s.name, priority as i64, s.capabilities, s.created_at],
        )?;
    }
    Ok((n, servers.len() - n))
}

/// The settings document, minus keys the registry doesn't know, values it
/// no longer accepts and [`DROPPED_SETTINGS`].
fn restore_settings(ctx: &Ctx) -> DbResult<(usize, usize)> {
    let Some(json) = old_state(ctx, "settings")? else {
        return Ok((0, 0));
    };
    let mut doc: Value = serde_json::from_str(&json)?;
    let Some(entries) = doc.get_mut("entries").and_then(Value::as_object_mut) else {
        return Ok((0, 1));
    };
    let before = entries.len();
    entries.retain(|key, entry| setting_kept(key, entry.get("value")));
    let kept = entries.len();
    // Parse as the actor will (it migrates older document versions).
    crate::settings::Settings::from_json(&doc.to_string())
        .map_err(|e| super::DbError::Other(e.to_string()))?;
    ctx.new.execute(
        "INSERT INTO saved_state(key, json, updated_at) VALUES ('settings', ?1, ?2)
         ON CONFLICT(key) DO NOTHING",
        params![doc.to_string(), crate::util::now_ms()],
    )?;
    Ok((kept, before - kept))
}

fn setting_kept(key: &str, value: Option<&Value>) -> bool {
    if DROPPED_SETTINGS.contains(&key) {
        return false;
    }
    match (crate::settings::lookup(key), value) {
        (Some(def), Some(v)) => crate::settings::validate(def, v).is_ok(),
        _ => false,
    }
}

/// The `settings` table (unused by either version, copied for completeness).
fn restore_settings_rows(ctx: &Ctx) -> DbResult<(usize, usize)> {
    let mut st = ctx
        .old
        .prepare("SELECT key, value, scope, updated_at FROM settings")?;
    let rows = st.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, f64>(3)?,
        ))
    })?;
    let (mut kept, mut skipped) = (0, 0);
    for row in rows {
        let (key, value, scope, at) = row?;
        let parsed: Option<Value> = serde_json::from_str(&value).ok();
        if !setting_kept(&key, parsed.as_ref()) {
            skipped += 1;
            continue;
        }
        kept += ctx.new.execute(
            "INSERT OR IGNORE INTO settings(key, value, scope, updated_at) VALUES (?1, ?2, ?3, ?4)",
            params![key, value, scope, at],
        )?;
    }
    Ok((kept, skipped))
}

fn restore_local_state(ctx: &Ctx) -> DbResult<(usize, usize)> {
    let mut st = ctx
        .old
        .prepare("SELECT key, json, updated_at FROM saved_state")?;
    let rows = st.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, f64>(2)?,
        ))
    })?;
    let mut n = 0;
    for row in rows {
        let (key, json, at) = row?;
        let local = LOCAL_STATE_KEYS.contains(&key.as_str())
            || LOCAL_STATE_PREFIXES.iter().any(|p| key.starts_with(p));
        if local {
            n += ctx.new.execute(
                "INSERT OR IGNORE INTO saved_state(key, json, updated_at) VALUES (?1, ?2, ?3)",
                params![key, json, at],
            )?;
        }
    }
    Ok((n, 0))
}

/// Columns shared by the version-10 and current tables, `server_id` first.
const PINS: &[&str] = &[
    "server_id",
    "target_kind",
    "target_id",
    "label",
    "cover_art",
    "created_at",
    "transcoded",
    "profile",
];
const PIN_TRACKS: &[&str] = &[
    "server_id",
    "target_kind",
    "target_id",
    "track_id",
    "state",
    "path",
    "bytes",
    "suffix",
    "content_type",
    "gain_db",
    "peak",
    "downloaded_at",
    "error",
];
const CACHE_ENTRIES: &[&str] = &[
    "server_id",
    "track_id",
    "path",
    "bytes",
    "content_type",
    "profile",
    "created_at",
    "last_used_at",
    "complete",
    "total_bytes",
    "spans",
    "source_tag",
    "gain_db",
    "peak",
];
const CACHE_SIGNALS: &[&str] = &[
    "server_id",
    "track_id",
    "autoplay",
    "skips",
    "primed",
    "updated_at",
];
const AUTOMIX_RECORDS: &[&str] = &[
    "server_id",
    "track_id",
    "version",
    "produced_by",
    "json",
    "updated_at",
];
const PLAY_HISTORY: &[&str] = &[
    "server_id",
    "id",
    "track_id",
    "played_at",
    "played_ms",
    "scrobbled",
    "device_id",
];

/// Copy `table`'s rows whose `server_id` (`cols[0]`) is a restored account,
/// under that account's id.
fn copy_by_account(ctx: &Ctx, table: &str, cols: &[&str]) -> DbResult<(usize, usize)> {
    copy_rows(ctx, table, cols, "1 = 1", |row| {
        let SqlValue::Text(sid) = &row[0] else {
            return false;
        };
        match ctx.account(sid) {
            Some(account) => {
                row[0] = SqlValue::Text(account.clone());
                true
            }
            None => false,
        }
    })
}

fn copy_rows(
    ctx: &Ctx,
    table: &str,
    cols: &[&str],
    filter: &str,
    mut keep: impl FnMut(&mut Vec<SqlValue>) -> bool,
) -> DbResult<(usize, usize)> {
    let list = cols.join(", ");
    let mut read = ctx
        .old
        .prepare(&format!("SELECT {list} FROM {table} WHERE {filter}"))?;
    let marks = vec!["?"; cols.len()].join(", ");
    let mut write = ctx.new.prepare(&format!(
        "INSERT OR IGNORE INTO {table}({list}) VALUES ({marks})"
    ))?;
    let mut rows = read.query([])?;
    let (mut kept, mut skipped) = (0, 0);
    while let Some(r) = rows.next()? {
        let mut row = (0..cols.len())
            .map(|i| r.get::<_, SqlValue>(i))
            .collect::<Result<Vec<_>, _>>()?;
        if keep(&mut row) {
            kept += write.execute(rusqlite::params_from_iter(row.iter()))?;
        } else {
            skipped += 1;
        }
    }
    Ok((kept, skipped))
}

/// One flag per account with restored wanted tracks: its next finished
/// sync plans them (`Downloads::replan_wanted`), once.
fn flag_replans(ctx: &Ctx) -> DbResult<(usize, usize)> {
    let n = ctx.new.execute(
        "INSERT OR IGNORE INTO saved_state(key, json, updated_at)
         SELECT DISTINCT ?1 || server_id, 'true', ?2 FROM pin_tracks
         WHERE state IN ('wanted', 'downloading')",
        params![REPLAN_PREFIX, crate::util::now_ms()],
    )?;
    Ok((n, 0))
}

fn restore_play_history(ctx: &Ctx) -> DbResult<(usize, usize)> {
    copy_by_account(ctx, "play_history", PLAY_HISTORY)
}

fn restore_filters(ctx: &Ctx) -> DbResult<(usize, usize)> {
    copy_rows(
        ctx,
        "filters",
        &["json", "id", "name", "created_at", "updated_at"],
        "1 = 1",
        |row| {
            matches!(&row[0], SqlValue::Text(j)
                if serde_json::from_str::<crate::api::Filter>(j).is_ok())
        },
    )
}

/// Unsent rows (pending, in flight when the app stopped, failed and
/// retryable by hand), their mutation in the current format.
fn restore_outbox(ctx: &Ctx) -> DbResult<(usize, usize)> {
    if ctx.retry {
        let unsent: i64 = ctx.old.query_row(
            "SELECT count(*) FROM outbox WHERE status IN ('pending', 'inflight', 'failed')",
            [],
            |r| r.get(0),
        )?;
        tracing::warn!(unsent, "restore retry: the old outbox is left out");
        return Ok((0, unsent.max(0) as usize));
    }
    let cols = [
        "server_id",
        "mutation",
        "id",
        "status",
        "created_at",
        "attempts",
        "next_attempt_at",
        "last_error",
        "target",
        "expected_prior",
        "finished_at",
        "server_ref",
    ];
    let (kept, skipped) = copy_rows(
        ctx,
        "outbox",
        &cols,
        "status IN ('pending', 'inflight', 'failed')",
        |row| {
            let (SqlValue::Text(sid), SqlValue::Text(m)) = (&row[0], &row[1]) else {
                return false;
            };
            let Some(account) = ctx.account(sid).cloned() else {
                return false;
            };
            let Some(m) = map_mutation(m, &account) else {
                return false;
            };
            row[0] = SqlValue::Text(account);
            row[1] = SqlValue::Text(m);
            if matches!(&row[3], SqlValue::Text(s) if s == "inflight") {
                row[3] = SqlValue::Text("pending".into());
            }
            true
        },
    )?;
    if skipped > 0 {
        tracing::warn!(
            skipped,
            "restore: outbox rows that could not be carried over"
        );
    }
    let finished: i64 = ctx.old.query_row(
        "SELECT count(*) FROM outbox WHERE status NOT IN ('pending', 'inflight', 'failed')",
        [],
        |r| r.get(0),
    )?;
    Ok((kept, skipped + finished.max(0) as usize))
}

/// An old outbox mutation in the current format, or `None` when it doesn't
/// convert. Only a track rating changed: it names the track's source.
fn map_mutation(json: &str, source: &str) -> Option<String> {
    let mut v: Value = serde_json::from_str(json).ok()?;
    if v.get("type").and_then(Value::as_str) == Some("setRating") {
        if let Some(target) = v.pointer_mut("/data/target") {
            if target.get("type").and_then(Value::as_str) == Some("track") {
                if let Some(data) = target.get_mut("data").and_then(Value::as_object_mut) {
                    data.insert("source".into(), json!(source));
                }
            }
        }
    }
    serde_json::from_value::<crate::outbox::Mutation>(v.clone()).ok()?;
    Some(v.to_string())
}

fn old_state(ctx: &Ctx, key: &str) -> rusqlite::Result<Option<String>> {
    ctx.old
        .query_row("SELECT json FROM saved_state WHERE key = ?1", [key], |r| {
            r.get(0)
        })
        .optional()
}

/// Every `saved_state` row under `prefix:<scope>` with the scope's account.
fn old_scoped(ctx: &Ctx, prefix: &str) -> rusqlite::Result<Vec<(String, String, String)>> {
    let mut st = ctx
        .old
        .prepare("SELECT key, json FROM saved_state WHERE substr(key, 1, ?2) = ?1")?;
    let head = format!("{prefix}:");
    let rows = st.query_map(params![head, head.len() as i64], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
    })?;
    let mut out = vec![];
    for row in rows {
        let (key, json) = row?;
        let scope = key[head.len()..].to_string();
        if let Some(account) = ctx.scopes.get(&scope) {
            out.push((scope, account.clone(), json));
        }
    }
    Ok(out)
}

/// Convert the scope-keyed state and park it for [`finish`]. Counts are
/// documents (or entries) parked and entries that didn't convert.
fn restore_deferred(ctx: &Ctx) -> DbResult<(usize, usize)> {
    let mut pending = Pending::default();
    let (mut kept, mut skipped) = (0, 0);
    if let Some(scope) = &ctx.session_scope {
        let account = ctx.scopes[scope].clone();
        if let Some(json) = old_state(ctx, &format!("session:{scope}"))? {
            match map_session(&json, &account, &ctx.accounts) {
                Some(doc) => {
                    pending.session = Some(doc);
                    kept += 1;
                }
                None => skipped += 1,
            }
        }
        if let Some(json) = old_state(ctx, &format!("position:{scope}"))? {
            match serde_json::from_str::<Value>(&json) {
                Ok(v) => {
                    pending.position = Some(v);
                    kept += 1;
                }
                Err(_) => skipped += 1,
            }
        }
    }
    for (_, _, json) in old_scoped(ctx, "stash")? {
        for mut e in entries(&json) {
            let mapped = e.as_object_mut().and_then(|o| {
                let id = o.remove("trackId")?;
                let sid = o.remove("serverId")?;
                let source = ctx.account(sid.as_str()?)?;
                o.insert("track".into(), track_ref(source, id.as_str()?));
                Some(())
            });
            match mapped.and_then(|()| serde_json::from_value::<StashEntry>(e).ok()) {
                Some(e) if !pending.stash.iter().any(|x| x.entry_id == e.entry_id) => {
                    pending.stash.push(e);
                    kept += 1;
                }
                Some(_) => {}
                None => skipped += 1,
            }
        }
    }
    for (_, account, json) in old_scoped(ctx, "snooze")? {
        for mut e in entries(&json) {
            let mapped = e.as_object_mut().and_then(|o| {
                let id = o.remove("trackId")?;
                o.insert("track".into(), track_ref(&account, id.as_str()?));
                Some(())
            });
            match mapped.and_then(|()| serde_json::from_value::<Snooze>(e).ok()) {
                Some(s) => {
                    match pending.snoozes.iter_mut().find(|x| x.track == s.track) {
                        Some(x) if x.updated_at < s.updated_at => *x = s,
                        Some(_) => {}
                        None => pending.snoozes.push(s),
                    }
                    kept += 1;
                }
                None => skipped += 1,
            }
        }
    }
    for (_, account, json) in old_scoped(ctx, "album-resume")? {
        for e in entries(&json) {
            match serde_json::from_value::<AlbumResumeEntry>(e) {
                Ok(mut a) => {
                    a.source = account.clone();
                    match pending
                        .album_resumes
                        .iter_mut()
                        .find(|x| x.source == a.source && x.album_id == a.album_id)
                    {
                        Some(x) if x.updated_at < a.updated_at => *x = a,
                        Some(_) => {}
                        None => pending.album_resumes.push(a),
                    }
                    kept += 1;
                }
                Err(_) => skipped += 1,
            }
        }
    }
    for (_, _, json) in old_scoped(ctx, "pendingScrobbles")? {
        for mut e in entries(&json) {
            let mapped = e
                .get("serverId")
                .and_then(Value::as_str)
                .and_then(|s| ctx.account(s))
                .cloned();
            match mapped {
                Some(account) => {
                    e["serverId"] = json!(account);
                    pending.pending_scrobbles.push(e);
                    kept += 1;
                }
                None => skipped += 1,
            }
        }
    }
    if !pending.is_empty() {
        ctx.new.execute(
            "INSERT OR REPLACE INTO saved_state(key, json, updated_at) VALUES (?1, ?2, ?3)",
            params![
                PENDING_KEY,
                serde_json::to_string(&pending)?,
                crate::util::now_ms()
            ],
        )?;
    }
    Ok((kept, skipped))
}

/// The elements of a JSON array document (nothing when it isn't one).
fn entries(json: &str) -> Vec<Value> {
    match serde_json::from_str(json) {
        Ok(Value::Array(v)) => v,
        _ => vec![],
    }
}

fn track_ref(source: &str, id: &str) -> Value {
    json!({ "source": source, "id": id })
}

/// An old session document in the current format: queue items name a track
/// reference (`track`) instead of a bare `trackId`, valid in the scope's
/// `account`; a context names its `source` instead of its `serverId`, and
/// its tracks are references there. `None` when the result doesn't load.
fn map_session(
    json: &str,
    account: &str,
    accounts: &HashMap<String, String>,
) -> Option<SessionDocument> {
    let mut v: Value = serde_json::from_str(json).ok()?;
    let doc = v.as_object_mut()?;
    map_queue(doc, account, accounts);
    if let Some(Value::Array(saved)) = doc.get_mut("savedQueues") {
        for q in saved.iter_mut().filter_map(Value::as_object_mut) {
            map_queue(q, account, accounts);
        }
    }
    let doc = crate::session::load(&v.to_string()).ok()?;
    crate::session::validate(&doc).ok()?;
    Some(doc)
}

fn map_queue(
    q: &mut serde_json::Map<String, Value>,
    account: &str,
    accounts: &HashMap<String, String>,
) {
    if let Some(ctx) = q.get_mut("context").and_then(Value::as_object_mut) {
        let source = ctx
            .remove("serverId")
            .and_then(|s| s.as_str().and_then(|s| accounts.get(s)).cloned())
            .unwrap_or_else(|| account.to_string());
        if let Some(Value::Array(tracks)) = ctx.get_mut("tracks") {
            for t in tracks.iter_mut() {
                if let Value::String(id) = t {
                    *t = track_ref(&source, id);
                }
            }
        }
        ctx.insert("source".into(), json!(source));
    }
    let item = |v: &mut Value| {
        if let Some(o) = v.as_object_mut() {
            if let Some(Value::String(id)) = o.remove("trackId") {
                o.insert("track".into(), track_ref(account, &id));
            }
        }
    };
    if let Some(cur) = q.get_mut("current") {
        item(cur);
    }
    for list in ["history", "upcoming"] {
        if let Some(Value::Array(items)) = q.get_mut(list) {
            items.iter_mut().for_each(item);
        }
    }
}

#[cfg(test)]
mod tests;
