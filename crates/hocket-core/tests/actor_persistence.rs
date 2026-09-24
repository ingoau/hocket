//! What survives a restart, and what the actor writes on its own: corrupt
//! settings are backed up, the resume point is clamped, housekeeping prunes,
//! and `Started` is emitted exactly once per core.

#![cfg(feature = "sim")]

use hocket_core::api::*;
use hocket_core::audio::scripted::ScriptedCall;
use hocket_core::core::test_support::{seeded_server, TestCore};
use hocket_core::util::WallClock;

fn album_args(server_id: &str, album: &str) -> PlayContextArgs {
    PlayContextArgs {
        context: QueueContext {
            server_id: server_id.into(),
            kind: ContextKind::Album { id: album.into() },
            label: String::new(),
            sort: SortOrder::Default,
            tracks: vec![],
        },
        start_index: Some(0),
        shuffle: false,
        save_outgoing: true,
    }
}

async fn synced_core(name: &str) -> TestCore {
    let t = TestCore::start(name, seeded_server(8, 200.0)).await;
    t.wait_until(30_000.0, |t| {
        t.events
            .all()
            .iter()
            .any(|e| matches!(e, Event::SyncProgress { progress } if progress.finished))
    })
    .await;
    t
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn corrupt_settings_are_backed_up_reported_and_not_broadcast() {
    let t = TestCore::start("settings", seeded_server(2, 100.0)).await;
    t.run(Command::SetSetting {
        key: "scrobble.enabled".into(),
        value: "false".into(),
    })
    .await;
    t.core.shutdown().await;
    // A partial write: the stored document no longer parses.
    let db = t.open_db();
    let junk = r#"{"version":1,"entries":{"scrobble.enabled":"#;
    db.saved_state_set_raw("settings", junk, &WallClock)
        .unwrap();
    drop(db);
    let TestCore {
        clock, server, dir, ..
    } = t;
    let t = TestCore::start_in("settings", server, None, 1, clock, dir).await;

    // Reported as a storage error at start, not silently defaulted.
    assert!(
        t.events.all().iter().any(|e| matches!(
            e,
            Event::Error { kind: ErrorKind::Storage, message, .. } if message.contains("Settings")
        )),
        "{:?}",
        t.events.all()
    );
    // The original bytes are kept under a backup key.
    let db = t.open_db();
    let backups = db.saved_state_keys("settings.corrupt-").unwrap();
    assert_eq!(backups.len(), 1, "{backups:?}");
    assert_eq!(
        db.saved_state_get_raw(&backups[0]).unwrap().as_deref(),
        Some(junk)
    );
    // Defaults are in effect and the session document is intact.
    let snap = t.snapshot().await;
    let scrobble = snap
        .settings
        .iter()
        .find(|s| s.key == "scrobble.enabled")
        .expect("setting");
    assert_eq!(scrobble.value, "true");
    assert!(snap.session.is_some());
    // A user edit still saves and is announced.
    t.run(Command::SetSetting {
        key: "scrobble.enabled".into(),
        value: "false".into(),
    })
    .await;
    assert!(t
        .events
        .all()
        .iter()
        .any(|e| matches!(e, Event::SettingChanged { setting } if setting.key == "scrobble.enabled" && setting.value == "false")));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn started_is_emitted_once_and_snapshot_requests_emit_snapshot() {
    let t = TestCore::start("once", seeded_server(2, 100.0)).await;
    let started = |t: &TestCore| {
        t.events
            .all()
            .iter()
            .filter(|e| matches!(e, Event::Started { .. }))
            .count()
    };
    let snapshots = |t: &TestCore| {
        t.events
            .all()
            .iter()
            .filter(|e| matches!(e, Event::Snapshot { .. }))
            .count()
    };
    assert_eq!(started(&t), 1);
    assert_eq!(snapshots(&t), 0);

    t.run(Command::RequestSnapshot).await;
    assert_eq!(started(&t), 1, "RequestSnapshot never repeats Started");
    assert_eq!(snapshots(&t), 1);
    // The replay still carries the state.
    let events = t.events.all();
    let snap_idx = events
        .iter()
        .rposition(|e| matches!(e, Event::Snapshot { .. }))
        .unwrap();
    assert!(
        events[snap_idx..]
            .iter()
            .any(|e| matches!(e, Event::ServersChanged { servers } if servers.len() == 1)),
        "ServersChanged follows the Snapshot"
    );

    // A repeated Start and a server removal are replays too.
    t.run(Command::Start).await;
    assert_eq!(started(&t), 1);
    assert_eq!(snapshots(&t), 2);
    let sid = t.server_id.clone();
    t.run(Command::RemoveServer { server_id: sid }).await;
    assert_eq!(started(&t), 1);
    assert_eq!(snapshots(&t), 3);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn restored_position_past_the_end_is_clamped_before_the_backend_sees_it() {
    let t = synced_core("clamp").await;
    let sid = t.server_id.clone();
    t.run(Command::PlayContext {
        args: album_args(&sid, "al0"),
    })
    .await;
    t.run_for(500.0).await;
    t.run(Command::Pause).await;
    t.core.shutdown().await;
    // Tamper with the persisted resume point (a peer's saved queue or an
    // old device can carry one that is not clamped).
    let db = t.open_db();
    let key = db.saved_state_keys("position:").unwrap().remove(0);
    let mut pos: serde_json::Value = db.saved_state_get(&key).unwrap().unwrap();
    pos["positionMs"] = serde_json::json!(4_000_000_000u32);
    db.saved_state_set(&key, &pos, &WallClock).unwrap();
    drop(db);
    let TestCore {
        clock, server, dir, ..
    } = t;
    let t = TestCore::start_in("clamp", server, None, 1, clock, dir).await;
    t.run(Command::Play).await;
    t.run_for(500.0).await;
    let loads: Vec<u32> = t
        .backend
        .log()
        .into_iter()
        .filter_map(|c| match c {
            ScriptedCall::Load { position_ms, .. } => Some(position_ms),
            _ => None,
        })
        .collect();
    assert!(!loads.is_empty());
    assert!(
        loads.iter().all(|p| *p <= 200_000),
        "positions handed to the backend: {loads:?}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn housekeeping_prunes_finished_outbox_entries_and_old_jobs() {
    let t = synced_core("prune").await;
    let sid = t.server_id.clone();
    t.run(Command::PlayTracks {
        server_id: sid.clone(),
        track_ids: vec!["t0".into()],
        start_index: 0,
        label: "Selection".into(),
        shuffle: false,
    })
    .await;
    // Past the scrobble threshold: now-playing and submission are flushed
    // and sit in the outbox as done rows.
    t.run_for(110_000.0).await;
    let db = t.open_db();
    let done = |db: &hocket_core::db::Db| {
        db.with_conn(|c| {
            Ok(c.query_row(
                "SELECT COUNT(*) FROM outbox WHERE status = 'done'",
                [],
                |r| r.get::<_, i64>(0),
            )?)
        })
        .unwrap()
    };
    let old_jobs = |db: &hocket_core::db::Db| {
        db.with_conn(|c| {
            Ok(c.query_row(
                "SELECT COUNT(*) FROM jobs WHERE state IN ('done','cancelled','failed')",
                [],
                |r| r.get::<_, i64>(0),
            )?)
        })
        .unwrap()
    };
    assert!(done(&db) >= 2, "done outbox rows: {}", done(&db));
    assert!(old_jobs(&db) >= 1, "finished jobs: {}", old_jobs(&db));

    // Eight days later, the hourly housekeeping runs on the next tick.
    t.clock.advance(8.0 * 86_400_000.0);
    t.core.tick().unwrap();
    t.core.settle().await;
    assert_eq!(
        done(&db),
        0,
        "finished outbox entries older than a week are gone"
    );
    let remaining = db
        .with_conn(|c| {
            Ok(c.query_row(
                "SELECT COUNT(*) FROM jobs WHERE state IN ('done','cancelled','failed') AND updated_at < ?1",
                [t.clock.now_ms() - 24.0 * 3_600_000.0],
                |r| r.get::<_, i64>(0),
            )?)
        })
        .unwrap();
    assert_eq!(
        remaining, 0,
        "terminal jobs older than the retention are gone"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn session_document_and_sync_base_are_persisted_together() {
    let t = synced_core("tx").await;
    let sid = t.server_id.clone();
    t.run(Command::PlayContext {
        args: album_args(&sid, "al0"),
    })
    .await;
    t.run_for(500.0).await;
    let db = t.open_db();
    let sessions = db.saved_state_keys("session:").unwrap();
    let connects = db.saved_state_keys("connect:").unwrap();
    assert_eq!(sessions.len(), 1);
    assert_eq!(connects.len(), 1);
    let (a, b) = db
        .with_conn(|c| {
            let a: f64 = c.query_row(
                "SELECT updated_at FROM saved_state WHERE key = ?1",
                [&sessions[0]],
                |r| r.get(0),
            )?;
            let b: f64 = c.query_row(
                "SELECT updated_at FROM saved_state WHERE key = ?1",
                [&connects[0]],
                |r| r.get(0),
            )?;
            Ok((a, b))
        })
        .unwrap();
    assert_eq!(a, b, "one transaction, one timestamp");
}
