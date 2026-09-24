//! Full-core playback scenarios on the scripted backend and a fake server:
//! play an album, next/previous, undo, saved-queue restore, scrobbling.

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
    // The launch sync runs as a job; wait for the mirror to fill.
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
async fn play_album_then_next_previous_and_undo() {
    let t = synced_core("a").await;
    let sid = t.server_id.clone();
    t.run(Command::PlayContext {
        args: album_args(&sid, "al0"),
    })
    .await;
    t.run_for(500.0).await;

    let q = t.queue().await;
    assert_eq!(q.context_label.as_deref(), Some("Album al0"));
    assert_eq!(q.current.as_ref().unwrap().track.id, "t0");
    assert_eq!(q.upcoming.len(), 3);
    assert!(t.backend.is_playing(), "the owner drives audio");
    let snap = t.snapshot().await;
    assert!(snap.transport.position.is_playing);
    assert_eq!(
        snap.transport.lease.owner.as_deref(),
        Some(t.device_id.as_str())
    );
    assert!(snap.media_session.is_playing);
    assert_eq!(
        snap.media_session.metadata.as_ref().unwrap().title,
        "Track 0"
    );
    // Gapless: the follow-up was preloaded.
    assert!(t
        .backend
        .log()
        .iter()
        .any(|c| matches!(c, ScriptedCall::Load { next: Some(_), .. })));

    t.run(Command::Next).await;
    t.run_for(500.0).await;
    assert_eq!(t.current_track_id().await.as_deref(), Some("t1"));
    assert_eq!(
        t.backend.current().unwrap().0,
        t.queue().await.current.unwrap().item.key
    );

    // Previous within 3 s goes back; nothing was consumed.
    t.run(Command::Previous).await;
    t.run_for(500.0).await;
    let q = t.queue().await;
    assert_eq!(q.current.as_ref().unwrap().track.id, "t0");
    assert_eq!(q.upcoming.len(), 3);
    assert!(q.history.is_empty());

    // Previous after 3 s restarts the track instead.
    t.run(Command::Next).await;
    t.run_for(5_000.0).await;
    assert_eq!(t.current_track_id().await.as_deref(), Some("t1"));
    t.run(Command::Previous).await;
    t.run_for(500.0).await;
    assert_eq!(t.current_track_id().await.as_deref(), Some("t1"));
    assert!(t.backend.current().unwrap().1 < 2_000);

    // Undo the Next: back to t0 with the queue intact.
    let undo = t.snapshot().await.undo;
    assert!(undo.can_undo);
    t.run(Command::Undo).await;
    t.run_for(500.0).await;
    assert_eq!(t.current_track_id().await.as_deref(), Some("t0"));
    assert!(t
        .events
        .all()
        .iter()
        .any(|e| matches!(e, Event::Toast { toast } if toast.message.starts_with("Undid"))));
    assert!(t.snapshot().await.undo.can_redo);
    t.run(Command::Redo).await;
    t.run_for(500.0).await;
    assert_eq!(t.current_track_id().await.as_deref(), Some("t1"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn playing_something_new_saves_the_outgoing_queue_and_restore_lands_mid_track() {
    let t = synced_core("b").await;
    let sid = t.server_id.clone();
    t.run(Command::PlayContext {
        args: album_args(&sid, "al0"),
    })
    .await;
    t.run(Command::Next).await;
    t.run_for(42_000.0).await;
    assert_eq!(t.current_track_id().await.as_deref(), Some("t1"));
    let before = t.snapshot().await.transport.position.position_ms;
    assert!(before >= 40_000, "position {before}");

    t.run(Command::PlayContext {
        args: album_args(&sid, "al1"),
    })
    .await;
    t.run_for(500.0).await;
    assert_eq!(t.current_track_id().await.as_deref(), Some("t4"));
    let saved = t.snapshot().await.session.unwrap().saved_queues;
    assert_eq!(saved.len(), 1, "the outgoing album was kept");
    let sq = &saved[0];
    assert_eq!(sq.label, "Album al0");
    assert!(
        sq.context.tracks.is_empty(),
        "ID-referenced snapshots omit the track list"
    );
    assert_eq!(sq.history.len(), 1);
    assert!(sq.position_ms >= 40_000);
    assert!(t
        .events
        .all()
        .iter()
        .any(|e| matches!(e, Event::SavedQueuesChanged { queues } if queues.len() == 1)));

    t.run(Command::RestoreSavedQueue { id: sq.id.clone() })
        .await;
    t.run_for(500.0).await;
    let q = t.queue().await;
    assert_eq!(q.current.as_ref().unwrap().track.id, "t1");
    assert_eq!(q.history.len(), 1, "history travels with the snapshot");
    assert_eq!(q.upcoming.len(), 2, "context re-resolved on restore");
    let pos = t.backend.current().unwrap().1;
    assert!(pos >= 40_000, "landed mid-track at {pos}");
    // Restoring bumps the entry (dedupe on identity) and al1 was saved.
    let saved = t.snapshot().await.session.unwrap().saved_queues;
    assert_eq!(saved.len(), 2);

    t.run(Command::PinSavedQueue {
        id: sq.id.clone(),
        pinned: true,
    })
    .await;
    assert!(t
        .snapshot()
        .await
        .session
        .unwrap()
        .saved_queues
        .iter()
        .any(|q| q.id == sq.id && q.pinned));
    t.run(Command::DeleteSavedQueue { id: sq.id.clone() }).await;
    assert!(!t
        .snapshot()
        .await
        .session
        .unwrap()
        .saved_queues
        .iter()
        .any(|q| q.id == sq.id));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_play_scrobbles_exactly_once_and_lands_in_history() {
    let t = synced_core("c").await;
    let sid = t.server_id.clone();
    t.run(Command::PlayTracks {
        server_id: sid.clone(),
        track_ids: vec!["t0".into(), "t1".into()],
        start_index: 0,
        label: "Selection".into(),
        shuffle: false,
    })
    .await;
    // 200 s track: threshold is 100 s. Now playing goes out at start.
    t.run_for(2_000.0).await;
    let now_playing = t
        .server
        .scrobbles()
        .iter()
        .filter(|s| !s.submission)
        .count();
    assert_eq!(now_playing, 1);
    t.run_for(90_000.0).await;
    assert_eq!(
        t.server.scrobbles().iter().filter(|s| s.submission).count(),
        0
    );
    t.run_for(15_000.0).await;
    let submitted: Vec<_> = t
        .server
        .scrobbles()
        .into_iter()
        .filter(|s| s.submission)
        .collect();
    assert_eq!(submitted.len(), 1, "exactly one submission");
    assert_eq!(submitted[0].id, "t0");
    // Keep playing past the end: no second submission for t0, and t1 starts.
    t.run_for(100_000.0).await;
    assert_eq!(t.current_track_id().await.as_deref(), Some("t1"));
    assert_eq!(
        t.server
            .scrobbles()
            .iter()
            .filter(|s| s.submission && s.id == "t0")
            .count(),
        1
    );
    match t.query(Query::RecentlyPlayed { limit: 10 }).await {
        QueryResult::History(h) => {
            assert_eq!(h.len(), 1);
            assert_eq!(h[0].track.id, "t0");
            assert!(h[0].played_ms >= 100_000);
        }
        other => panic!("{other:?}"),
    }
    match t.query(Query::Stats { period_days: 30 }).await {
        QueryResult::Stats(s) => assert_eq!(s.total_plays, 1),
        other => panic!("{other:?}"),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn seeking_does_not_count_as_listening() {
    let t = synced_core("d").await;
    let sid = t.server_id.clone();
    t.run(Command::PlayTracks {
        server_id: sid,
        track_ids: vec!["t0".into()],
        start_index: 0,
        label: "One".into(),
        shuffle: false,
    })
    .await;
    t.run_for(10_000.0).await;
    t.run(Command::SeekTo {
        position_ms: 150_000,
    })
    .await;
    t.run_for(20_000.0).await;
    assert_eq!(
        t.server.scrobbles().iter().filter(|s| s.submission).count(),
        0
    );
    let played = t.snapshot().await.transport.played_ms;
    assert!(played < 40_000, "played {played}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unplayable_items_are_skipped_with_a_notice() {
    let t = synced_core("e").await;
    let sid = t.server_id.clone();
    t.run(Command::PlayTracks {
        server_id: sid,
        track_ids: vec!["t0".into(), "t1".into(), "t2".into()],
        start_index: 0,
        label: "Sel".into(),
        shuffle: false,
    })
    .await;
    t.run_for(500.0).await;
    // t1 cannot be decoded here: one retry, then it is marked unavailable
    // and playback moves on. Nothing scrobbles for it.
    t.backend.fail_track("t1");
    t.run(Command::Next).await;
    t.run_for(2_000.0).await;
    let q = t.queue().await;
    assert_eq!(
        q.current.as_ref().unwrap().track.id,
        "t2",
        "skipped to the next playable item"
    );
    assert!(q
        .history
        .iter()
        .any(|e| e.item.track_id == "t1" && e.item.unavailable));
    assert!(t
        .events
        .all()
        .iter()
        .any(|e| matches!(e, Event::PlayerNotice { message: Some(m) } if m.contains("skipped"))));
    assert!(t.backend.is_playing());
    let loads = t
        .backend
        .log()
        .iter()
        .filter(|c| matches!(c, ScriptedCall::Load { .. }))
        .count();
    assert!(loads >= 3, "t0, t1 (+retry), t2: {loads}");
    // Going back lands on t0 (the unavailable item is passed over) and a
    // deliberate jump to t1 clears the flag and tries again.
    t.run(Command::Previous).await;
    t.run_for(500.0).await;
    assert_eq!(t.current_track_id().await.as_deref(), Some("t0"));
    match t.query(Query::Problems).await {
        QueryResult::Problems(p) => assert!(p.iter().any(|p| p.summary.contains("Track 1"))),
        other => panic!("{other:?}"),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn sleep_timer_stops_at_end_of_track() {
    let t = synced_core("f").await;
    let sid = t.server_id.clone();
    t.run(Command::PlayTracks {
        server_id: sid,
        track_ids: vec!["t0".into(), "t1".into()],
        start_index: 0,
        label: "Sel".into(),
        shuffle: false,
    })
    .await;
    t.run(Command::SetSleepTimer {
        timer: Some(SleepTimer {
            ends_at: None,
            stop_at_end_of_track: true,
        }),
    })
    .await;
    assert!(t.snapshot().await.sleep_timer.is_some());
    t.run_for(205_000.0).await;
    assert!(!t.backend.is_playing());
    assert!(t.snapshot().await.sleep_timer.is_none());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_play_with_scrobbling_off_is_recorded_but_not_marked_scrobbled() {
    let t = synced_core("noscrobble").await;
    t.run(Command::SetSetting {
        key: "scrobble.enabled".into(),
        value: "false".into(),
    })
    .await;
    let sid = t.server_id.clone();
    t.run(Command::PlayTracks {
        server_id: sid.clone(),
        track_ids: vec!["t0".into(), "t1".into()],
        start_index: 0,
        label: "Selection".into(),
        shuffle: false,
    })
    .await;
    t.run_for(110_000.0).await;
    t.run(Command::Next).await;
    t.run_for(500.0).await;
    assert!(
        t.server.scrobbles().is_empty(),
        "nothing sent to the server"
    );
    match t.query(Query::RecentlyPlayed { limit: 10 }).await {
        QueryResult::History(h) => {
            assert_eq!(h.len(), 1);
            assert_eq!(h[0].track.id, "t0");
            assert!(
                !h[0].scrobbled,
                "history is honest about what was scrobbled"
            );
        }
        other => panic!("{other:?}"),
    }
    match t.query(Query::Stats { period_days: 30 }).await {
        QueryResult::Stats(s) => assert_eq!(s.total_plays, 1),
        other => panic!("{other:?}"),
    }
}
