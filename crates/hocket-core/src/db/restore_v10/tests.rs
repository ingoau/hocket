//! A real version-10 database (the old schema and migrations, `v10_schema.sql`)
//! with a user's data in it, opened with this build.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use rusqlite::{params, Connection};
use serde_json::{json, Value};

use super::*;
use crate::api::{
    ContextKind, PinTarget, QueueContext, QueueItem, QueueSource, SortOrder, TrackRef,
};
use crate::connect::wire::{account_id_for_scope, scope_key};
use crate::db::SCHEMA_VERSION;
use crate::outbox::Mutation;
use crate::util::WallClock;

const V10_SCHEMA: &str = include_str!("v10_schema.sql");

const URL_A: &str = "https://music.example/";
const URL_B: &str = "http://nas.local:4533";

fn account(url: &str, user: &str) -> String {
    account_id_for_scope(&scope_key(url, user))
}

fn a() -> String {
    account(URL_A, "alice")
}

fn b() -> String {
    account(URL_B, "bob")
}

/// What the old build kept per session scope.
fn scope_a() -> String {
    scope_key(URL_A, "alice")
}

fn scope_b() -> String {
    scope_key(URL_B, "bob")
}

/// An empty version-10 database at `path`.
fn v10_db(path: &Path) -> Connection {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let c = Connection::open(path).unwrap();
    c.execute_batch(V10_SCHEMA).unwrap();
    for v in 1..=10 {
        c.execute(
            "INSERT INTO schema_version(version, applied_at) VALUES (?1, 0)",
            [v],
        )
        .unwrap();
    }
    c
}

fn state(c: &Connection, key: &str, json: &Value, at: f64) {
    c.execute(
        "INSERT INTO saved_state(key, json, updated_at) VALUES (?1, ?2, ?3)",
        params![key, json.to_string(), at],
    )
    .unwrap();
}

/// A current-format value in the version-10 shape: track references back
/// to bare ids, a context's `source` back to `serverId`.
fn downgrade(v: &mut Value) {
    match v {
        Value::Object(o) => {
            if let Some(Value::Object(t)) = o.remove("track") {
                o.insert("trackId".into(), t["id"].clone());
            }
            o.remove("alts");
            if o.contains_key("kind") && o.contains_key("sort") {
                if let Some(s) = o.remove("source") {
                    o.insert("serverId".into(), s);
                }
                if let Some(Value::Array(ts)) = o.get_mut("tracks") {
                    for t in ts.iter_mut() {
                        *t = t["id"].clone();
                    }
                }
            }
            for (_, x) in o.iter_mut() {
                downgrade(x);
            }
        }
        Value::Array(xs) => xs.iter_mut().for_each(downgrade),
        _ => {}
    }
}

fn item(key: &str, track: &str, index: u32) -> QueueItem {
    QueueItem {
        key: key.into(),
        track: TrackRef::new(a(), track),
        source: QueueSource::Context { index },
        unavailable: false,
        added: false,
        queued: false,
        ordinal: index,
        alts: vec![],
    }
}

/// The old session document: album `al0` from server A, `t1` playing.
fn old_session() -> Value {
    let mut doc = crate::session::new_document(scope_a(), "sess-1".into(), 1000.0);
    doc.context = Some(QueueContext {
        source: a(),
        kind: ContextKind::Album { id: "al0".into() },
        label: "Album 0".into(),
        sort: SortOrder::Default,
        tracks: ["t0", "t1", "t2", "t3"]
            .iter()
            .map(|t| TrackRef::new(a(), *t))
            .collect(),
    });
    doc.history = vec![item("k0", "t0", 0)];
    doc.current = Some(item("k1", "t1", 1));
    doc.upcoming = vec![item("k2", "t2", 2), item("k3", "t3", 3)];
    let mut v = serde_json::to_value(&doc).unwrap();
    downgrade(&mut v);
    assert!(v["current"]["trackId"] == "t1" && v["context"]["serverId"] == a());
    v
}

struct Fixture {
    dir: tempfile::TempDir,
}

impl Fixture {
    fn data(&self) -> PathBuf {
        self.dir.path().join("data")
    }
    fn cache(&self) -> PathBuf {
        self.dir.path().join("cache")
    }
    fn db_path(&self) -> PathBuf {
        self.data().join("hocket.sqlite")
    }
    fn download(&self, track: &str) -> PathBuf {
        self.data()
            .join("downloads")
            .join(a())
            .join(format!("{track}.flac"))
    }
    fn cached(&self, name: &str) -> PathBuf {
        self.cache().join("stream").join(a()).join(name)
    }
    fn open(&self) -> Db {
        Db::open(&self.db_path(), &self.data().join("backups")).unwrap()
    }
}

fn write_file(p: &Path) {
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, b"abcd").unwrap();
}

/// A version-10 data directory: two servers and the user's data, with the
/// downloaded and cached files on disk.
fn fixture() -> Fixture {
    let f = Fixture {
        dir: tempfile::tempdir().unwrap(),
    };
    let c = v10_db(&f.db_path());
    // Old ids were derived the same way: the md5 of the scope key.
    for (id, url, user, name, at) in [
        (a(), URL_A, "alice", "Home", 1.0),
        (b(), URL_B, "bob", "NAS", 2.0),
    ] {
        c.execute(
            "INSERT INTO servers(id, url, username, name, capabilities, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![id, url, user, name, r#"{"openSubsonic":true,"serverVersion":"0.55","extensions":[],"transcodeOffset":false,"formPost":true,"songLyrics":false,"sonicSimilarity":false,"apiKeyAuthentication":false,"transcodingExtension":false,"nativeApi":false,"meetsFloor":true}"#, at],
        )
        .unwrap();
    }
    // Mirror rows that resync (not restored).
    c.execute(
        "INSERT INTO tracks(id, server_id, title) VALUES ('t1', ?1, 'Track 1')",
        [a()],
    )
    .unwrap();

    // Settings: a known key, a key the registry dropped, the coordinator URL.
    state(
        &c,
        "settings",
        &json!({"version": 2, "entries": {
            "queue.savedCap": {"value": 7, "updated_at": 5.0},
            "old.removedKey": {"value": 1, "updated_at": 5.0},
            "connect.coordinatorUrl": {"value": "wss://coord.example", "updated_at": 5.0},
        }}),
        5.0,
    );
    state(&c, "volumeSlider", &json!(0.5), 5.0);
    state(&c, "lyricsOffset:t1", &json!({"offsetMs": 250}), 5.0);
    state(&c, &format!("sync:{}", a()), &json!({"lastFullAt": 9}), 5.0);
    state(&c, &format!("connect:{}", scope_a()), &json!({}), 5.0);

    // The queue: server A's session saved last; B's is older.
    state(&c, &format!("session:{}", scope_a()), &old_session(), 900.0);
    let mut older = old_session();
    older["scope"] = json!(scope_b());
    state(&c, &format!("session:{}", scope_b()), &older, 100.0);
    state(
        &c,
        &format!("position:{}", scope_a()),
        &json!({"key": "k1", "positionMs": 42_000}),
        900.0,
    );
    state(
        &c,
        &format!("stash:{}", scope_a()),
        &json!([{"entryId": "e1", "trackId": "t2", "serverId": a(), "sortKey": 1.0,
                 "addedAt": 5.0, "updatedAt": 5.0, "title": "Track 2"}]),
        5.0,
    );
    state(
        &c,
        &format!("snooze:{}", scope_b()),
        &json!([{"trackId": "x9", "until": null, "updatedAt": 5.0}]),
        5.0,
    );
    state(
        &c,
        &format!("album-resume:{}", scope_a()),
        &json!([{"albumId": "al0", "trackId": "t2", "trackIndex": 2, "positionMs": 1000,
                 "durationMs": 180000, "updatedAt": 5.0}]),
        5.0,
    );

    // Downloads: album al0 pinned, two tracks on disk; a cached stream.
    c.execute(
        "INSERT INTO pins(server_id, target_kind, target_id, label, created_at) VALUES (?1, 'album', 'al0', 'Album 0', 1.0)",
        [a()],
    )
    .unwrap();
    c.execute(
        "INSERT INTO pins(server_id, target_kind, target_id, label, created_at) VALUES ('gone', 'album', 'x', 'Gone', 1.0)",
        [],
    )
    .unwrap();
    for t in ["t1", "t2"] {
        write_file(&f.download(t));
        c.execute(
            "INSERT INTO pin_tracks(server_id, target_kind, target_id, track_id, state, path, bytes, suffix, downloaded_at)
             VALUES (?1, 'album', 'al0', ?2, 'done', ?3, 4, 'flac', 2.0)",
            params![a(), t, f.download(t).to_string_lossy()],
        )
        .unwrap();
    }
    c.execute(
        "INSERT INTO pin_tracks(server_id, target_kind, target_id, track_id, state) VALUES (?1, 'album', 'al0', 't3', 'wanted')",
        [a()],
    )
    .unwrap();
    write_file(&f.cached("t3.mp3"));
    write_file(&f.cached("orphan.mp3"));
    c.execute(
        "INSERT INTO cache_entries(server_id, track_id, path, bytes, content_type, profile, created_at, last_used_at, complete, total_bytes, spans)
         VALUES (?1, 't3', ?2, 4, 'audio/mpeg', '', 1.0, 1.0, 1, 4, '0-4')",
        params![a(), f.cached("t3.mp3").to_string_lossy()],
    )
    .unwrap();
    let payload = json!({"serverId": a(), "target": PinTarget::Album { id: "al0".into() }, "transcode": false});
    c.execute(
        "INSERT INTO jobs(id, kind, label, state, payload, total, created_at, updated_at) VALUES ('j1', 'download', 'Download Album 0', 'running', ?1, 1, 1.0, 1.0)",
        [payload.to_string()],
    )
    .unwrap();
    c.execute(
        "INSERT INTO job_items(job_id, seq, item) VALUES ('j1', 0, 't3')",
        [],
    )
    .unwrap();
    c.execute(
        "INSERT INTO jobs(id, kind, label, state, created_at, updated_at) VALUES ('j2', 'librarySync', 'Sync', 'queued', 1.0, 1.0)",
        [],
    )
    .unwrap();

    // History, a smart playlist, unsent writes.
    c.execute(
        "INSERT INTO play_history(id, server_id, track_id, played_at, played_ms, scrobbled, device_id) VALUES (7, ?1, 't1', 1000.0, 180000, 0, 'dev')",
        [a()],
    )
    .unwrap();
    c.execute(
        "INSERT INTO play_history(id, server_id, track_id, played_at, played_ms, scrobbled, device_id) VALUES (8, ?1, 'x9', 2000.0, 90000, 1, 'dev')",
        [b()],
    )
    .unwrap();
    let mut filter = crate::filters::default_filters().remove(0);
    filter.id = "mine".into();
    filter.name = "Mine".into();
    c.execute(
        "INSERT INTO filters(id, name, json, created_at, updated_at) VALUES ('mine', 'Mine', ?1, 1.0, 1.0)",
        [serde_json::to_string(&filter).unwrap()],
    )
    .unwrap();
    let mut rating = serde_json::to_value(Mutation::SetRating {
        target: crate::api::RatingTarget::Track {
            id: "t1".into(),
            source: String::new(),
        },
        rating: 4,
    })
    .unwrap();
    rating["data"]["target"]["data"]
        .as_object_mut()
        .unwrap()
        .remove("source");
    let scrobble = serde_json::to_value(Mutation::Scrobble {
        track_id: "t1".into(),
        played_at: 1000.0,
        submission: true,
        history_id: Some(7),
    })
    .unwrap();
    for (id, m, status) in [
        ("o1", &rating, "pending"),
        ("o2", &scrobble, "inflight"),
        ("o3", &scrobble, "done"),
    ] {
        c.execute(
            "INSERT INTO outbox(id, server_id, mutation, status, created_at) VALUES (?1, ?2, ?3, ?4, 1.0)",
            params![id, a(), m.to_string(), status],
        )
        .unwrap();
    }
    f
}

fn count(db: &Db, sql: &str) -> i64 {
    db.with_conn(|c| Ok(c.query_row(sql, [], |r| r.get(0))?))
        .unwrap()
}

#[test]
fn a_v10_database_is_restored_once() {
    let f = fixture();
    let db = f.open();
    let report = db.open_report().clone();
    assert_eq!(report.from_version, 10);
    assert!(report.mirror_rebuilt);
    let r = report.restore.expect("restored");
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    assert_eq!(r.restored["accounts"], 2);
    assert_eq!(r.restored["pins"], 1);
    assert_eq!(r.skipped["pins"], 1, "a server that's gone");
    assert_eq!(r.skipped["outbox"], 1, "the finished row");
    assert!(db.saved_state_get_raw(DONE_KEY).unwrap().is_some());

    // Accounts: same ids, the last-used session's first, awaiting logins.
    let accounts = db.accounts().unwrap();
    assert_eq!(
        accounts.iter().map(|x| x.id.clone()).collect::<Vec<_>>(),
        vec![a(), b()]
    );
    assert_eq!(accounts[0].source, a());
    assert!(accounts[0].capabilities.meets_floor);
    assert_eq!(accounts[1].name, "NAS");
    assert_eq!(
        count(&db, "SELECT count(*) FROM tracks"),
        0,
        "mirror resyncs"
    );

    // Settings: the known key only.
    let settings: Value = db.saved_state_get("settings").unwrap().unwrap();
    let keys: Vec<&String> = settings["entries"].as_object().unwrap().keys().collect();
    assert_eq!(keys, vec!["queue.savedCap"]);
    let s = crate::settings::Settings::from_json(&settings.to_string()).unwrap();
    assert_eq!(s.get("queue.savedCap"), json!(7));
    assert_eq!(
        db.saved_state_get::<f64>("volumeSlider").unwrap(),
        Some(0.5)
    );
    assert!(db.saved_state_get_raw("lyricsOffset:t1").unwrap().is_some());
    assert!(db
        .saved_state_get_raw(&format!("sync:{}", a()))
        .unwrap()
        .is_none());

    // Durable rows.
    assert_eq!(count(&db, "SELECT count(*) FROM pin_tracks"), 3);
    assert_eq!(count(&db, "SELECT count(*) FROM cache_entries"), 1);
    assert_eq!(
        count(&db, "SELECT count(*) FROM play_history WHERE id IN (7, 8)"),
        2
    );
    assert_eq!(count(&db, "SELECT count(*) FROM filters"), 1);
    assert_eq!(
        count(&db, "SELECT count(*) FROM outbox WHERE status = 'pending'"),
        2
    );
    let rating: String = db
        .with_conn(|c| {
            Ok(
                c.query_row("SELECT mutation FROM outbox WHERE id = 'o1'", [], |r| {
                    r.get(0)
                })?,
            )
        })
        .unwrap();
    assert_eq!(
        serde_json::from_str::<Mutation>(&rating).unwrap(),
        Mutation::SetRating {
            target: crate::api::RatingTarget::Track {
                id: "t1".into(),
                source: a()
            },
            rating: 4
        }
    );
    // Jobs aren't restored: they would run before the account is signed in
    // and synced. Wanted tracks are planned after the first sync.
    assert_eq!(count(&db, "SELECT count(*) FROM jobs"), 0);

    // The downloads layer's startup sweep keeps the files the rows name.
    let downloads = crate::downloads::Downloads::new(
        db.clone(),
        Arc::new(WallClock),
        Arc::new(crate::downloads::UnknownStorage),
        &f.data(),
        &f.cache(),
        crate::api::Platform::Linux,
    );
    for t in ["t1", "t2"] {
        assert!(f.download(t).exists());
        assert_eq!(
            downloads.downloaded_path(&a(), t).unwrap(),
            Some(f.download(t))
        );
    }
    assert!(f.cached("t3.mp3").exists(), "cached stream kept");
    assert!(!f.cached("orphan.mp3").exists(), "the sweep did run");
    assert_eq!(count(&db, "SELECT count(*) FROM cache_entries"), 1);

    // Scope-keyed state waits for the identity, then lands once.
    assert!(db.saved_state_get_raw(PENDING_KEY).unwrap().is_some());
    assert!(finish(&db, "ident", &WallClock) >= 5);
    assert_eq!(finish(&db, "ident", &WallClock), 0, "only once");
    let doc =
        crate::session::load(&db.saved_state_get_raw("session:ident").unwrap().unwrap()).unwrap();
    assert_eq!(doc.scope, "ident");
    assert_eq!(doc.session_id, "sess-1");
    assert_eq!(doc.current.unwrap().track, TrackRef::new(a(), "t1"));
    assert_eq!(doc.upcoming.len(), 2);
    let ctx = doc.context.unwrap();
    assert_eq!(ctx.source, a());
    assert_eq!(ctx.tracks[3], TrackRef::new(a(), "t3"));
    let pos: Value = db.saved_state_get("position:ident").unwrap().unwrap();
    assert_eq!(pos["positionMs"], 42_000);
    let stash: Vec<StashEntry> = db.saved_state_get("stash:ident").unwrap().unwrap();
    assert_eq!(stash[0].track, TrackRef::new(a(), "t2"));
    let snoozes: Vec<Snooze> = db.saved_state_get("snooze:ident").unwrap().unwrap();
    assert_eq!(snoozes[0].track, TrackRef::new(b(), "x9"));
    let resumes: Vec<AlbumResumeEntry> = db.saved_state_get("album-resume:ident").unwrap().unwrap();
    assert_eq!(resumes[0].source, a());
    drop(downloads);
    drop(db);

    // A second open is a plain open.
    let db = f.open();
    assert_eq!(db.open_report().from_version, SCHEMA_VERSION);
    assert!(db.open_report().restore.is_none());
    assert_eq!(count(&db, "SELECT count(*) FROM accounts"), 2);
    assert_eq!(count(&db, "SELECT count(*) FROM pins"), 1);
    assert_eq!(finish(&db, "other", &WallClock), 0);
}

#[test]
fn a_partial_backup_restores_what_it_has() {
    // Only some tables, and documents that don't parse.
    let f = Fixture {
        dir: tempfile::tempdir().unwrap(),
    };
    {
        std::fs::create_dir_all(f.data()).unwrap();
        let c = Connection::open(f.db_path()).unwrap();
        c.execute_batch(
            "CREATE TABLE schema_version(version INTEGER NOT NULL, applied_at REAL NOT NULL);
             INSERT INTO schema_version VALUES (10, 0);
             CREATE TABLE servers(id TEXT PRIMARY KEY, url TEXT NOT NULL, username TEXT NOT NULL,
                 name TEXT NOT NULL, last_sync REAL, capabilities TEXT NOT NULL DEFAULT '{}', created_at REAL NOT NULL);
             CREATE TABLE saved_state(key TEXT PRIMARY KEY, json TEXT NOT NULL, updated_at REAL NOT NULL);
             CREATE TABLE filters(id TEXT PRIMARY KEY, name TEXT NOT NULL, json TEXT NOT NULL,
                 created_at REAL NOT NULL, updated_at REAL NOT NULL);
             INSERT INTO filters VALUES ('f', 'F', 'not json', 1, 1);",
        )
        .unwrap();
        c.execute(
            "INSERT INTO servers(id, url, username, name, created_at) VALUES (?1, ?2, 'alice', 'Home', 1)",
            params![a(), URL_A],
        )
        .unwrap();
        state(&c, "volumeSlider", &json!(0.25), 1.0);
        c.execute(
            "INSERT INTO saved_state VALUES ('settings', '{nope', 1), (?1, '[1, 2', 1)",
            [format!("session:{}", scope_a())],
        )
        .unwrap();
    }
    let db = f.open();
    assert_eq!(db.schema_version().unwrap(), SCHEMA_VERSION);
    let r = db.open_report().restore.clone().unwrap();
    assert!(r.errors.iter().any(|e| e.starts_with("pins:")), "{r:?}");
    assert!(r.errors.iter().any(|e| e.starts_with("settings:")), "{r:?}");
    assert_eq!(r.restored["accounts"], 1);
    assert_eq!(r.skipped["filters"], 1);
    assert_eq!(r.skipped["deferred"], 1, "the session that doesn't parse");
    assert_eq!(db.accounts().unwrap().len(), 1);
    assert_eq!(
        db.saved_state_get::<f64>("volumeSlider").unwrap(),
        Some(0.25)
    );
    assert!(db.saved_state_get_raw("settings").unwrap().is_none());
    assert_eq!(finish(&db, "ident", &WallClock), 0);
}

#[test]
fn an_unreadable_backup_restores_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let junk = dir.path().join("hocket.sqlite.v10.1.bak");
    std::fs::write(&junk, b"this is not a database").unwrap();
    let c = Connection::open_in_memory().unwrap();
    for (_, _, sql) in super::super::MIGRATIONS {
        c.execute_batch(sql).unwrap();
    }
    let r = retry(&c, &junk);
    assert_eq!(r.errors.len(), 1, "{r:?}");
    assert!(r.restored.is_empty());
    // Recorded as done: there's nothing to retry.
    let done: i64 = c
        .query_row(
            "SELECT count(*) FROM saved_state WHERE key = ?1",
            [DONE_KEY],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(done, 1);
}

/// Copy the fixture's database to `backups/<file>.v10.<ms>.bak`.
fn back_up(f: &Fixture, ms: i64) -> PathBuf {
    let dir = f.data().join("backups");
    std::fs::create_dir_all(&dir).unwrap();
    let dest = dir.join(format!("hocket.sqlite.v10.{ms}.bak"));
    Connection::open(f.db_path())
        .unwrap()
        .execute("VACUUM INTO ?1", [dest.to_string_lossy()])
        .unwrap();
    dest
}

#[test]
fn the_upgrade_restores_from_the_first_backup() {
    // An older build backed the database up, then crashed while dropping
    // tables one by one: the file is still version 10 but half emptied.
    // This open backs that up too, and restores from the first copy.
    let f = fixture();
    back_up(&f, 1_000);
    Connection::open(f.db_path())
        .unwrap()
        .execute_batch("DELETE FROM servers; DROP TABLE pins; DROP TABLE pin_tracks;")
        .unwrap();
    let db = f.open();
    assert_eq!(db.open_report().from_version, 10);
    let r = db.open_report().restore.clone().expect("restored");
    assert!(r.errors.is_empty(), "{r:?}");
    assert_eq!(r.restored["accounts"], 2, "from the first backup");
    assert_eq!(count(&db, "SELECT count(*) FROM pin_tracks"), 3);
}

#[test]
fn a_database_recreated_by_a_build_without_the_restore_is_left_alone() {
    // v0.1.605 squashed the schema without restoring: its devices have the
    // version-10 backup, a recreated database in use since, and no record.
    let f = fixture();
    back_up(&f, 1_000);
    for ext in ["", "-wal", "-shm"] {
        let _ = std::fs::remove_file(format!("{}{ext}", f.db_path().display()));
    }
    {
        let c = Connection::open(f.db_path()).unwrap();
        for (_, _, sql) in super::super::MIGRATIONS {
            c.execute_batch(sql).unwrap();
        }
        c.execute(
            "INSERT INTO schema_version(version, applied_at) VALUES (?1, 0)",
            [SCHEMA_VERSION],
        )
        .unwrap();
        // The user signed in to one account again and listened since.
        c.execute(
            "INSERT INTO accounts(id, url, username, name, created_at) VALUES (?1, ?2, 'alice', 'Home', 5)",
            params![a(), URL_A],
        )
        .unwrap();
        c.execute(
            "INSERT INTO play_history(server_id, track_id, played_at, device_id) VALUES (?1, 'new', 9000, 'dev')",
            [a()],
        )
        .unwrap();
    }
    let db = f.open();
    assert_eq!(db.open_report().from_version, SCHEMA_VERSION);
    assert!(db.open_report().restore.is_none(), "nothing restored");
    assert!(
        db.saved_state_get_raw(DONE_KEY).unwrap().is_some(),
        "recorded"
    );
    assert_eq!(count(&db, "SELECT count(*) FROM accounts"), 1);
    assert_eq!(count(&db, "SELECT count(*) FROM play_history"), 1);
    for table in ["pins", "pin_tracks", "outbox", "filters"] {
        assert_eq!(
            count(&db, &format!("SELECT count(*) FROM {table}")),
            0,
            "{table}"
        );
    }
    assert!(db.saved_state_get_raw(PENDING_KEY).unwrap().is_none());
    drop(db);
    assert!(f.open().open_report().restore.is_none());
}

#[test]
fn a_database_that_never_was_version_10_restores_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let data = dir.path().join("data");
    let backups = data.join("backups");
    std::fs::create_dir_all(&backups).unwrap();
    std::fs::write(backups.join("hocket.sqlite.v11.5.bak"), b"x").unwrap();
    let path = data.join("hocket.sqlite");
    for _ in 0..2 {
        let db = Db::open(&path, &backups).unwrap();
        assert!(db.open_report().restore.is_none());
        assert!(db.saved_state_get_raw(DONE_KEY).unwrap().is_some());
        assert!(db.saved_state_get_raw(RETRY_KEY).unwrap().is_none());
        assert!(db.accounts().unwrap().is_empty());
    }
}

/// Simulates SQLite rolling the whole transaction back during `part`.
struct AbortAt;
impl AbortAt {
    fn set(part: &'static str) -> AbortAt {
        ABORT_AT.with(|a| a.set(Some(part)));
        AbortAt
    }
}
impl Drop for AbortAt {
    fn drop(&mut self) {
        ABORT_AT.with(|a| a.set(None));
    }
}

#[test]
fn a_restore_sqlite_rolled_back_is_retried_on_the_next_open() {
    let f = fixture();
    let db = {
        let _abort = AbortAt::set("pinTracks");
        f.open()
    };
    // The upgrade itself went through, without any of the restore.
    assert_eq!(db.schema_version().unwrap(), SCHEMA_VERSION);
    let r = db.open_report().restore.clone().unwrap();
    assert!(
        r.errors.iter().any(|e| e.contains("restore aborted")),
        "{r:?}"
    );
    assert!(db.saved_state_get_raw(DONE_KEY).unwrap().is_none());
    assert!(db.saved_state_get_raw(RETRY_KEY).unwrap().is_some());
    assert_eq!(
        count(&db, "SELECT count(*) FROM accounts"),
        0,
        "no half restore"
    );
    assert_eq!(count(&db, "SELECT count(*) FROM pins"), 0);
    drop(db);
    // The next open restores from the backup, all but the old outbox: the
    // database has served a session meanwhile.
    let db = f.open();
    let r = db.open_report().restore.clone().expect("retried");
    assert!(r.errors.is_empty(), "{r:?}");
    assert_eq!(r.skipped["outbox"], 2, "the unsent rows");
    assert_eq!(count(&db, "SELECT count(*) FROM accounts"), 2);
    assert_eq!(count(&db, "SELECT count(*) FROM pin_tracks"), 3);
    assert_eq!(count(&db, "SELECT count(*) FROM outbox"), 0);
    assert!(db.saved_state_get_raw(DONE_KEY).unwrap().is_some());
    assert!(db.saved_state_get_raw(RETRY_KEY).unwrap().is_none());
    drop(db);
    assert!(f.open().open_report().restore.is_none());
}

#[test]
fn a_retry_sqlite_rolls_back_gives_up_without_writing_anything() {
    // The parts after the rollback don't autocommit one by one, and a
    // second failure is recorded instead of retried on every open.
    let f = fixture();
    let backup = back_up(&f, 1_000);
    let c = Connection::open_in_memory().unwrap();
    for (_, _, sql) in super::super::MIGRATIONS {
        c.execute_batch(sql).unwrap();
    }
    mark_retry(&c, &backup).unwrap();
    let r = {
        let _abort = AbortAt::set("cacheEntries");
        after_open(&c).unwrap()
    };
    assert!(
        r.errors.iter().any(|e| e.starts_with("cacheEntries:")),
        "{r:?}"
    );
    assert!(c.is_autocommit());
    for table in [
        "accounts",
        "pins",
        "pin_tracks",
        "play_history",
        "filters",
        "outbox",
    ] {
        let n: i64 = c
            .query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 0, "{table}");
    }
    let keys: Vec<String> = c
        .prepare("SELECT key FROM saved_state")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(keys, vec![DONE_KEY.to_string()]);
    assert!(after_open(&c).is_none());
}

/// End to end: a platform starts the core on the old data directory.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_core_starts_where_the_old_build_left_off() {
    use crate::core::test_support::{seeded_server_with_id, TestCore};
    let f = fixture();
    let Fixture { dir } = f;
    let server = seeded_server_with_id(&a(), 8, 180.0);
    let clock = crate::sim::SimTime::new(1_700_000_000_000.0);
    let t = TestCore::start_in("a", server, None, 1, clock, dir).await;
    let q = t.queue().await;
    assert_eq!(
        q.current.map(|e| e.item.track),
        Some(TrackRef::new(a(), "t1"))
    );
    assert_eq!(q.history_count, 1);
    assert_eq!(q.total_upcoming, 2);
    match t.query(crate::api::Query::Stash).await {
        crate::api::QueryResult::StashItems(items) => assert_eq!(items.len(), 1),
        other => panic!("{other:?}"),
    }
    match t.query(crate::api::Query::Accounts).await {
        crate::api::QueryResult::Accounts(list) => {
            assert_eq!(list.len(), 2);
            assert_eq!(list[0].id, a());
        }
        other => panic!("{other:?}"),
    }
    let data = t.data_dir();
    for track in ["t1", "t2"] {
        let p = data
            .join("downloads")
            .join(a())
            .join(format!("{track}.flac"));
        assert!(p.exists(), "{p:?} kept");
    }
    let db = t.open_db();
    assert!(db.saved_state_get_raw(PENDING_KEY).unwrap().is_none());
    assert_eq!(db.saved_state_keys("session:").unwrap().len(), 1);

    // The album pin's wanted track downloads once the account has synced.
    t.wait_until(30_000.0, |t| {
        count(
            &t.open_db(),
            "SELECT count(*) FROM pin_tracks WHERE track_id = 't3' AND state = 'done'",
        ) == 1
    })
    .await;
    let db = t.open_db();
    assert!(
        db.saved_state_get_raw(&replan_key(&a())).unwrap().is_none(),
        "planned once"
    );
    assert_eq!(
        count(&db, "SELECT count(*) FROM jobs WHERE state = 'failed'"),
        0,
        "no job ran before the account was ready"
    );
    // The first sync's rows know what was already downloaded.
    assert_eq!(
        db.track("t1").unwrap().unwrap().offline,
        crate::api::OfflineState::Downloaded
    );
    assert_eq!(
        db.track("t3").unwrap().unwrap().offline,
        crate::api::OfflineState::Downloaded
    );
}

/// Wanted tracks whose job the user cancelled stay unfetched: only pins a
/// restore brought back are planned after a sync, and only once.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_sync_does_not_restart_a_cancelled_download() {
    use crate::core::test_support::{seeded_server, TestCore};
    let t = TestCore::start("cancelled", seeded_server(4, 180.0)).await;
    let sid = t.server_id.clone();
    let payload = json!({"serverId": sid, "target": PinTarget::Album { id: "al0".into() }, "transcode": false});
    t.open_db()
        .with_conn(|c| {
            c.execute(
                "INSERT INTO pins(server_id, target_kind, target_id, label, created_at) VALUES (?1, 'album', 'al0', 'Album 0', 1.0)",
                [&sid],
            )?;
            c.execute(
                "INSERT INTO pin_tracks(server_id, target_kind, target_id, track_id, state) VALUES (?1, 'album', 'al0', 't1', 'wanted')",
                [&sid],
            )?;
            c.execute(
                "INSERT INTO jobs(id, kind, label, state, payload, total, created_at, updated_at) VALUES ('cx', 'download', 'Download Album 0', 'cancelled', ?1, 1, 1.0, 1.0)",
                [payload.to_string()],
            )?;
            c.execute("INSERT INTO job_items(job_id, seq, item, state) VALUES ('cx', 0, 't1', 'cancelled')", [])?;
            Ok(())
        })
        .unwrap();
    t.run(crate::api::Command::SyncLibrary {
        account_id: sid.clone(),
        full: true,
    })
    .await;
    t.run_for(5_000.0).await;
    let db = t.open_db();
    assert!(
        count(
            &db,
            "SELECT count(*) FROM jobs WHERE kind = 'librarySync' AND state = 'done'"
        ) >= 1
    );
    assert_eq!(
        count(&db, "SELECT count(*) FROM jobs WHERE kind = 'download'"),
        1,
        "nothing resubmitted"
    );
}
