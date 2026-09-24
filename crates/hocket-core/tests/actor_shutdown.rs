//! Awaitable shutdown: `Core::shutdown()` resolves only after the actor has
//! flushed the session document and position, a second call is a no-op,
//! and `shutdown_blocking` works from a plain thread.

#![cfg(feature = "sim")]

use hocket_core::api::*;
use hocket_core::audio::scripted::ScriptedCall;
use hocket_core::core::test_support::{seeded_server, TestCore};

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
async fn shutdown_resolves_after_flush_and_restart_restores_state() {
    let t = synced_core("a").await;
    let sid = t.server_id.clone();
    t.run(Command::PlayContext {
        args: album_args(&sid, "al0"),
    })
    .await;
    t.run_for(500.0).await;
    t.run(Command::SeekTo {
        position_ms: 42_000,
    })
    .await;
    // Move the queue on without letting the document debounce (250 ms) or
    // the position heartbeat (30 s) run: only the shutdown flush can write.
    t.run(Command::Next).await;
    t.run(Command::SeekTo {
        position_ms: 17_000,
    })
    .await;
    assert_eq!(t.current_track_id().await.as_deref(), Some("t1"));

    // Restart on the same data dir: `restart` awaits `Core::shutdown()`.
    let t = t.restart("a").await;
    let q = t.queue().await;
    assert_eq!(
        q.current.as_ref().map(|c| c.track.id.as_str()),
        Some("t1"),
        "the session document written by the shutdown flush is restored"
    );
    assert!(!t.backend.is_playing(), "never auto-resumes on start");
    // The resume point is dormant until Play: the load then starts where
    // the shutdown flush left off.
    t.run(Command::Play).await;
    t.run_for(500.0).await;
    assert!(t.backend.is_playing());
    let load = t
        .backend
        .log()
        .into_iter()
        .find_map(|c| match c {
            ScriptedCall::Load { position_ms, .. } => Some(position_ms),
            _ => None,
        })
        .expect("play loads the restored item");
    assert_eq!(
        load, 17_000,
        "the position written by the shutdown flush is restored"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn second_shutdown_is_a_noop_and_dispatch_fails_after() {
    let t = TestCore::start("b", seeded_server(2, 100.0)).await;
    t.core.shutdown().await;
    // Already stopped: resolves immediately instead of hanging.
    tokio::time::timeout(std::time::Duration::from_secs(5), t.core.shutdown())
        .await
        .expect("second shutdown resolves");
    assert!(matches!(
        t.core.dispatch(Command::RequestSnapshot),
        Err(hocket_core::CoreError::ShutDown)
    ));
    assert!(t.core.query(Query::Snapshot).await.is_err());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn shutdown_after_command_shutdown_resolves() {
    let t = TestCore::start("c", seeded_server(2, 100.0)).await;
    t.core.dispatch(Command::Shutdown).unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(5), t.core.shutdown())
        .await
        .expect("shutdown after Command::Shutdown resolves");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn shutdown_blocking_from_a_plain_thread() {
    let t = TestCore::start("d", seeded_server(2, 100.0)).await;
    let core = t.core.clone();
    let flushed = tokio::task::spawn_blocking(move || {
        core.shutdown_blocking(std::time::Duration::from_secs(5))
    })
    .await
    .unwrap();
    assert!(flushed);
    // And again: still true, still immediate.
    let core = t.core.clone();
    let again = tokio::task::spawn_blocking(move || {
        core.shutdown_blocking(std::time::Duration::from_secs(5))
    })
    .await
    .unwrap();
    assert!(again);
}
