//! Song preview: a second player that gives this device's playback back
//! and leaves a Connect peer alone.

#![cfg(feature = "sim")]

use hocket_core::api::*;
use hocket_core::audio::scripted::ScriptedCall;
use hocket_core::core::test_support::{seeded_server, TestCore};
use hocket_core::sim::SimTime;

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

fn preview_of(events: &[Event]) -> Option<PreviewState> {
    events.iter().rev().find_map(|e| match e {
        Event::PreviewChanged { preview } => Some(preview.clone()),
        _ => None,
    })?
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn preview_pauses_here_and_resumes_at_the_same_position() {
    let t = synced_core("preview").await;
    let sid = t.server_id.clone();
    t.run(Command::PlayTracks {
        server_id: sid,
        track_ids: vec!["t0".into(), "t1".into()],
        start_index: 0,
        label: "Sel".into(),
        shuffle: false,
    })
    .await;
    t.run_for(3_000.0).await;
    let before = t.transport().await;
    let position = before.position.position_ms;
    assert!(before.position.is_playing);
    assert!(position >= 2_000, "playback moved, at {position}");

    t.events.clear();
    t.run(Command::PreviewStart {
        track_id: "t3".into(),
    })
    .await;
    let transport = t.transport().await;
    assert!(!transport.position.is_playing, "main playback pauses");
    assert!(
        transport.position.position_ms.abs_diff(position) < 500,
        "position held: was {position}, now {}",
        transport.position.position_ms
    );
    let preview = preview_of(&t.events.all()).expect("preview started");
    assert_eq!(preview.track_id, "t3");
    assert!(preview.playing);
    // 200 s song, opened 30% in.
    assert_eq!(preview.position_ms, 60_000);
    assert!(!t.backend.is_playing(), "the main player is paused");
    assert!(
        t.backend.log().iter().any(|c| matches!(
            c,
            ScriptedCall::PreviewLoad { position_ms: 60_000, .. }
        )),
        "preview loads at 30%"
    );
    // The main item is still the one that was playing, not the preview.
    let (key, _) = t.backend.current().expect("main item stays loaded");
    assert_ne!(key, "preview:t3");

    let history: i64 = t
        .open_db()
        .with_conn(|c| {
            Ok(c.query_row(
                "SELECT count(*) FROM play_history WHERE track_id = 't3'",
                [],
                |r| r.get(0),
            )?)
        })
        .unwrap();
    assert_eq!(history, 0, "a preview is not a play");

    t.run(Command::PreviewToggle).await;
    let paused = preview_of(&t.events.all()).expect("still previewing");
    assert!(!paused.playing);

    t.run(Command::PreviewSeek { position_ms: 80_000 })
        .await;
    let sought = preview_of(&t.events.all()).expect("seek");
    assert_eq!(sought.position_ms, 80_000);

    t.run(Command::PreviewStop).await;
    assert!(preview_of(&t.events.all()).is_none(), "preview closed");
    let after = t.transport().await;
    assert!(after.position.is_playing, "main playback resumes");
    assert!(
        after.position.position_ms.abs_diff(position) < 800,
        "resumed at {position}, got {}",
        after.position.position_ms
    );
    assert_eq!(t.current_track_id().await.as_deref(), Some("t0"));
    assert!(t.queue().await.history.is_empty(), "the queue did not move");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn preview_on_a_peer_leaves_the_player_alone() {
    let server = seeded_server(6, 200.0);
    let net = hocket_core::core::io::memory::MemoryNet::new();
    let clock = SimTime::new(1_700_000_000_000.0);
    let a = TestCore::start_on("a", server.clone(), Some(net.clone()), 1, clock.clone()).await;
    let b = TestCore::start_on("b", server, Some(net), 2, clock).await;
    TestCore::run_all_for(&[&a, &b], 8_000.0).await;
    a.run(Command::PlayTracks {
        server_id: a.server_id.clone(),
        track_ids: vec!["t0".into()],
        start_index: 0,
        label: "Sel".into(),
        shuffle: false,
    })
    .await;
    TestCore::run_all_for(&[&a, &b], 2_000.0).await;
    assert!(a.backend.is_playing());
    assert!(!b.backend.is_playing());
    let owner = a.snapshot().await.transport.lease.owner.clone();
    assert_eq!(owner.as_deref(), Some(a.device_id.as_str()));

    b.events.clear();
    b.run(Command::PreviewStart {
        track_id: "t2".into(),
    })
    .await;
    TestCore::run_all_for(&[&a, &b], 1_000.0).await;

    assert!(a.backend.is_playing(), "the peer keeps playing");
    assert_eq!(
        a.snapshot().await.transport.lease.owner.as_deref(),
        Some(a.device_id.as_str())
    );
    let preview = preview_of(&b.events.all()).expect("b previews locally");
    assert_eq!(preview.track_id, "t2");
    assert!(preview.playing);
    assert!(
        b.backend.log().iter().any(|c| matches!(c, ScriptedCall::PreviewLoad { .. })),
        "the preview is played on this device"
    );
    // Nothing was asked of the owner's player beyond what it was already doing.
    assert!(
        !a.backend
            .log()
            .iter()
            .any(|c| matches!(c, ScriptedCall::Pause | ScriptedCall::Stop)),
        "the peer was not paused"
    );
}
