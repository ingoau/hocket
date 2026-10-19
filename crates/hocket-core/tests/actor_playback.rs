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
        .any(|e| matches!(e, Event::PlayerNotice { message: Some(m), code: Some(PlayerNoticeCode::CouldNotPlaySkipped), detail: Some(title) } if m.contains("skipped") && m.contains(title.as_str()) && !title.is_empty())));
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

/// A gapless auto-advance must move the session to the item the backend is
/// now playing, whether or not the backend reports `Ended` for the finished
/// one first (Media3 reports only the transition), and playback must carry on
/// through the queue rather than stop after the second item.
async fn auto_advance_follows_the_backend(name: &str, transition_only: bool) {
    let t = synced_core(name).await;
    t.backend.set_transition_only(transition_only);
    let sid = t.server_id.clone();
    t.run(Command::PlayTracks {
        server_id: sid,
        track_ids: vec!["t0".into(), "t1".into(), "t2".into(), "t3".into()],
        start_index: 0,
        label: "Sel".into(),
        shuffle: false,
    })
    .await;
    t.run_for(500.0).await;
    assert_eq!(t.current_track_id().await.as_deref(), Some("t0"));
    t.backend.clear_log();

    t.run_for(201_000.0).await;
    assert_eq!(t.current_track_id().await.as_deref(), Some("t1"));
    // Gapless: the backend was not reloaded, the session followed it.
    assert!(!t
        .backend
        .log()
        .iter()
        .any(|c| matches!(c, ScriptedCall::Load { .. })));
    let snap = t.snapshot().await;
    assert_eq!(
        snap.media_session.metadata.as_ref().unwrap().title,
        "Track 1"
    );
    assert!(snap.transport.position.is_playing);
    assert!(
        snap.transport.position.position_ms < 5_000,
        "position restarted with the new item: {}",
        snap.transport.position.position_ms
    );

    t.run_for(200_000.0).await;
    assert_eq!(t.current_track_id().await.as_deref(), Some("t2"));
    assert!(
        t.backend.is_playing(),
        "playback carries on past the second item"
    );
    assert!(!t
        .backend
        .log()
        .iter()
        .any(|c| matches!(c, ScriptedCall::Load { .. })));

    // Next moves on to what follows; it does not restart the playing item.
    t.run(Command::Next).await;
    t.run_for(500.0).await;
    assert_eq!(t.current_track_id().await.as_deref(), Some("t3"));
    assert_eq!(
        t.snapshot().await.media_session.metadata.unwrap().title,
        "Track 3"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn gapless_auto_advance_with_ended_moves_the_session() {
    auto_advance_follows_the_backend("g", false).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn gapless_auto_advance_without_ended_moves_the_session() {
    auto_advance_follows_the_backend("h", true).await;
}

/// A transient focus loss (a call, a navigation prompt) must not pause the
/// platform player: it resumes by itself when focus returns and reports
/// `Playing`. A pause the user asks for meanwhile does reach it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn transient_focus_loss_resumes_when_focus_returns() {
    let t = synced_core("i").await;
    let sid = t.server_id.clone();
    t.run(Command::PlayTracks {
        server_id: sid,
        track_ids: vec!["t0".into(), "t1".into()],
        start_index: 0,
        label: "Sel".into(),
        shuffle: false,
    })
    .await;
    t.run_for(1_500.0).await;
    t.backend.clear_log();
    t.run(Command::BackendReport {
        report: BackendReport::AudioFocusLost { transient: true },
    })
    .await;
    t.run_for(100.0).await;
    assert!(!t.transport().await.position.is_playing, "shown as paused");
    assert!(
        !t.backend.log().contains(&ScriptedCall::Pause),
        "the platform player is not paused"
    );
    let (key, position_ms) = t.backend.current().unwrap();
    t.run(Command::BackendReport {
        report: BackendReport::Playing { key, position_ms },
    })
    .await;
    t.run_for(100.0).await;
    assert!(t.transport().await.position.is_playing, "focus came back");

    t.run(Command::BackendReport {
        report: BackendReport::AudioFocusLost { transient: true },
    })
    .await;
    t.run(Command::Pause).await;
    t.run_for(100.0).await;
    assert!(
        t.backend.log().contains(&ScriptedCall::Pause),
        "a user pause during the loss reaches the player"
    );
    assert!(!t.backend.is_playing());
}

/// A backend that ends an item without moving on to the follow-up it was
/// given (a `SetNext` it had not applied yet, a preload it lost): the
/// session still adopts the follow-up at the boundary, and when no
/// `TransitionedToNext` confirms it, the core loads it explicitly instead of
/// sitting on an item nothing plays.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_adopted_item_the_backend_never_starts_is_loaded_explicitly() {
    let t = synced_core("i").await;
    t.backend.set_drop_next(true);
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
    assert_eq!(t.current_track_id().await.as_deref(), Some("t0"));
    t.backend.clear_log();

    // t0 (200 s) ends; the backend reports `Ended` and nothing after it.
    t.run_for(200_500.0).await;
    assert_eq!(t.current_track_id().await.as_deref(), Some("t1"));
    assert!(
        !t.backend
            .log()
            .iter()
            .any(|c| matches!(c, ScriptedCall::Load { .. })),
        "adopted first, not reloaded at once"
    );
    assert!(!t.backend.is_playing(), "the backend has nothing loaded");

    // After the adoption timeout the core loads what the document says.
    t.run_for(3_000.0).await;
    // The explicit load carries the document's key for t1 (the preload was
    // keyed before the item was materialised).
    let t1_keys: Vec<QueueKey> = t
        .sources
        .lock()
        .iter()
        .filter(|s| s.track.id == "t1")
        .map(|s| s.key.clone())
        .collect();
    assert!(!t1_keys.is_empty(), "t1 was resolved");
    assert!(
        t.backend.log().iter().any(
            |c| matches!(c, ScriptedCall::Load { key, play: true, .. } if t1_keys.contains(key))
        ),
        "{:?}",
        t.backend.log()
    );
    assert!(t.backend.is_playing());
    assert_eq!(t.current_track_id().await.as_deref(), Some("t1"));
    let snap = t.snapshot().await;
    assert!(snap.transport.position.is_playing);
    assert!(snap.transport.position.position_ms < 5_000);

    // And playback carries on the same way through the next boundary.
    t.run_for(205_000.0).await;
    assert_eq!(t.current_track_id().await.as_deref(), Some("t2"));
    assert!(t.backend.is_playing());
}

/// Play next on a song already in "continue playing" moves it up to play next instead of adding
/// it a second time (#37).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn play_next_on_an_upcoming_item_moves_it() {
    let t = synced_core("play-next-move").await;
    let sid = t.server_id.clone();
    t.run(Command::PlayContext {
        args: album_args(&sid, "al0"),
    })
    .await;
    t.run_for(500.0).await;
    let ids = |q: &QueueView| {
        q.playing_next
            .iter()
            .chain(q.upcoming.iter())
            .map(|e| e.track.id.clone())
            .collect::<Vec<_>>()
    };
    let q = t.queue().await;
    assert_eq!(ids(&q), ["t1", "t2", "t3"]);
    let last = q.upcoming.last().unwrap().clone();
    t.run(Command::RunAction {
        action_id: "playNext".into(),
        target: ActionTarget::QueueItems {
            keys: vec![last.item.key.clone()],
        },
    })
    .await;
    t.run_for(500.0).await;
    let q = t.queue().await;
    assert_eq!(ids(&q), ["t3", "t1", "t2"], "moved, not added twice");
    // A played song (here the current one) is still queued again.
    let current = q.current.clone().unwrap();
    t.run(Command::RunAction {
        action_id: "playNext".into(),
        target: ActionTarget::QueueItems {
            keys: vec![current.item.key.clone()],
        },
    })
    .await;
    t.run_for(500.0).await;
    assert_eq!(ids(&t.queue().await), ["t0", "t3", "t1", "t2"]);
}

/// Several upcoming songs at once move up in the order chosen, even though each context item's key
/// is its index and moving one renumbers the rest (#37).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn play_next_on_several_upcoming_items_moves_each_of_them() {
    let t = synced_core("play-next-move-many").await;
    let sid = t.server_id.clone();
    t.run(Command::PlayContext {
        args: album_args(&sid, "al0"),
    })
    .await;
    t.run_for(500.0).await;
    let ids = |q: &QueueView| {
        q.playing_next
            .iter()
            .chain(q.upcoming.iter())
            .map(|e| e.track.id.clone())
            .collect::<Vec<_>>()
    };
    let q = t.queue().await;
    assert_eq!(ids(&q), ["t1", "t2", "t3"]);
    t.run(Command::RunAction {
        action_id: "playNext".into(),
        target: ActionTarget::QueueItems {
            keys: vec![
                q.upcoming[2].item.key.clone(),
                q.upcoming[0].item.key.clone(),
            ],
        },
    })
    .await;
    t.run_for(500.0).await;
    assert_eq!(ids(&t.queue().await), ["t3", "t1", "t2"]);
}

async fn snapshot_volume(t: &TestCore) -> f64 {
    match t.query(Query::Snapshot).await {
        QueryResult::SnapshotResult(s) => s.transport.volume,
        other => panic!("unexpected {other:?}"),
    }
}

#[tokio::test]
async fn the_volume_slider_reaches_the_backend_on_a_db_scale() {
    let t = TestCore::start("vol", seeded_server(1, 200.0)).await;
    t.run(Command::SetVolume { volume: 0.5 }).await;
    // The slider position is what the UI sees; the backend gets the gain.
    assert_eq!(snapshot_volume(&t).await, 0.5);
    let want = hocket_core::audio::backend::volume_to_gain(0.5);
    assert!((t.backend.volume() - want).abs() < 1e-12);
    // Halfway is -25 dB, not -6 dB.
    assert!((20.0 * t.backend.volume().log10() + 25.0).abs() < 1e-9);

    // The slider position survives a restart, and so does the gain.
    let t = t.restart("vol").await;
    assert_eq!(snapshot_volume(&t).await, 0.5);
    assert!((t.backend.volume() - want).abs() < 1e-12);
}

#[tokio::test]
async fn a_volume_saved_as_a_linear_gain_keeps_its_loudness() {
    let t = TestCore::start("legacy", seeded_server(1, 200.0)).await;
    t.core.shutdown().await;
    let db = t.open_db();
    db.saved_state_delete("volumeSlider").unwrap();
    db.saved_state_set("volume", &0.25, &hocket_core::util::WallClock)
        .unwrap();
    drop(db);

    let t = t.restart("legacy").await;
    // Same loudness as before the upgrade: a gain of 0.25 (about -12 dB).
    assert!(
        (t.backend.volume() - 0.25).abs() < 1e-9,
        "{}",
        t.backend.volume()
    );
    let slider = snapshot_volume(&t).await;
    assert!((slider - hocket_core::audio::backend::gain_to_volume(0.25)).abs() < 1e-12);
    // Migrated once: a later change is saved and read back under the new key.
    t.run(Command::SetVolume { volume: 0.9 }).await;
    let t = t.restart("legacy").await;
    assert_eq!(snapshot_volume(&t).await, 0.9);
}

#[tokio::test]
async fn mute_survives_clearing_the_queue() {
    let t = synced_core("mute").await;
    t.run(Command::PlayContext {
        args: album_args(&t.server_id, "al0"),
    })
    .await;
    t.run(Command::SetVolume { volume: 0.6 }).await;
    t.run(Command::SetMuted { muted: true }).await;
    assert_eq!(t.backend.volume(), 0.0);
    t.run(Command::Stop).await;
    t.run(Command::ClearQueue).await;
    let tr = match t.query(Query::Snapshot).await {
        QueryResult::SnapshotResult(s) => s.transport,
        other => panic!("unexpected {other:?}"),
    };
    assert!(tr.muted, "still muted");
    assert_eq!(tr.volume, 0.6);
    assert_eq!(t.backend.volume(), 0.0, "and still silent");
    t.run(Command::SetMuted { muted: false }).await;
    assert!((t.backend.volume() - hocket_core::audio::backend::volume_to_gain(0.6)).abs() < 1e-12);
}

/// Removing the song that plays next replaces the gapless preload. Context
/// songs used to be keyed by index (`ctx-<index>`), so the next one inherited
/// the removed one's key and a preload kept by key alone played the removed
/// song at the boundary (#62); now each keeps its own key (#97).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn removing_the_next_song_replaces_the_gapless_preload() {
    let t = synced_core("remove-next-preload").await;
    let sid = t.server_id.clone();
    t.run(Command::PlayContext {
        args: album_args(&sid, "al0"),
    })
    .await;
    t.run_for(500.0).await;
    let q = t.queue().await;
    let next = q.upcoming[0].clone();
    assert_eq!(next.track.id, "t1");
    t.backend.clear_log();

    t.run(Command::RemoveQueueItems {
        keys: vec![next.item.key.clone()],
    })
    .await;
    t.run_for(500.0).await;
    let q = t.queue().await;
    assert_eq!(q.upcoming[0].track.id, "t2");
    let replacement = q.upcoming[0].item.key.clone();
    assert_ne!(replacement, next.item.key, "another song, another key");
    assert!(
        t.backend
            .log()
            .iter()
            .any(|c| matches!(c, ScriptedCall::SetNext { next: Some(k) } if *k == replacement)),
        "the preload was replaced: {:?}",
        t.backend.log()
    );

    // At the boundary the backend moves on to the replaced preload by itself.
    t.backend.clear_log();
    t.run_for(201_000.0).await;
    assert_eq!(t.current_track_id().await.as_deref(), Some("t2"));
    assert!(!t
        .backend
        .log()
        .iter()
        .any(|c| matches!(c, ScriptedCall::Load { .. })));
}

/// Under Repeat All the last song preloads the first one, which the queue
/// wraps to, so the wrap is gapless too (#13).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn repeat_all_preloads_the_wrap() {
    let t = synced_core("wrap-preload").await;
    let sid = t.server_id.clone();
    let mut args = album_args(&sid, "al0");
    args.start_index = Some(3);
    t.run(Command::PlayContext { args }).await;
    t.run(Command::SetRepeat {
        mode: RepeatMode::All,
    })
    .await;
    t.run_for(500.0).await;
    let q = t.queue().await;
    assert_eq!(q.current.as_ref().unwrap().track.id, "t3");
    let wrap = q.wrap_next.clone().expect("wraps to the top");
    assert_eq!(wrap.track.id, "t0");
    assert!(
        t.backend
            .log()
            .iter()
            .any(|c| matches!(c, ScriptedCall::SetNext { next: Some(k) } if *k == wrap.item.key)),
        "the wrap is preloaded: {:?}",
        t.backend.log()
    );
    t.backend.clear_log();
    t.run_for(201_000.0).await;
    assert_eq!(t.current_track_id().await.as_deref(), Some("t0"));
    assert!(!t
        .backend
        .log()
        .iter()
        .any(|c| matches!(c, ScriptedCall::Load { .. })));
}

/// With autoplay on, the last song fetches autoplay while it plays and
/// preloads the first pick, rather than stopping at the end to fetch (#13).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn autoplay_is_fetched_and_preloaded_before_the_queue_ends() {
    let t = synced_core("autoplay-preload").await;
    let sid = t.server_id.clone();
    let mut args = album_args(&sid, "al0");
    args.start_index = Some(3);
    t.run(Command::PlayContext { args }).await;
    t.run(Command::SetAutoplay { enabled: true }).await;
    t.run_for(2000.0).await;
    let q = t.queue().await;
    assert_eq!(q.current.as_ref().unwrap().track.id, "t3", "still playing");
    let pick = q
        .playing_next
        .first()
        .cloned()
        .expect("autoplay joined the queue ahead of the end");
    assert!(matches!(pick.item.source, QueueSource::Autoplay { .. }));
    assert!(
        t.backend
            .log()
            .iter()
            .any(|c| matches!(c, ScriptedCall::SetNext { next: Some(k) } if *k == pick.item.key)),
        "the pick is preloaded: {:?}",
        t.backend.log()
    );
    t.backend.clear_log();
    t.run_for(201_000.0).await;
    assert_eq!(
        t.current_track_id().await.as_deref(),
        Some(pick.track.id.as_str())
    );
    assert!(!t
        .backend
        .log()
        .iter()
        .any(|c| matches!(c, ScriptedCall::Load { .. })));
}

/// Autoplay fetched ahead for the last song is dropped when the queue moved
/// on before it landed: autoplay items join Playing Next, so kept they would
/// play ahead of the album started meanwhile (#13).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn autoplay_fetched_ahead_does_not_jump_a_new_queue() {
    let t = synced_core("autoplay-prefetch-stale").await;
    let sid = t.server_id.clone();
    t.run(Command::PlayContext {
        args: album_args(&sid, "al1"),
    })
    .await;
    t.run(Command::SetAutoplay { enabled: true }).await;
    let mut args = album_args(&sid, "al0");
    args.start_index = Some(3);
    // The last song fetches autoplay ahead; another album starts before the
    // batch lands (both commands are queued ahead of it).
    t.dispatch(Command::PlayContext { args });
    t.dispatch(Command::PlayContext {
        args: album_args(&sid, "al1"),
    });
    t.run_for(2000.0).await;
    let q = t.queue().await;
    assert_eq!(q.current.as_ref().unwrap().track.id, "t4");
    assert!(
        !q.playing_next
            .iter()
            .any(|e| matches!(e.item.source, QueueSource::Autoplay { .. })),
        "the stale batch stayed out: {:?}",
        q.playing_next
    );
    assert_eq!(q.upcoming.len(), 3);
}

/// Autoplay asked for when the queue ran out does not skip a song the user
/// started before the batch landed: its `Next` was for the song that ended.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn autoplay_at_the_end_does_not_skip_a_song_started_meanwhile() {
    let t = synced_core("autoplay-end-stale").await;
    let sid = t.server_id.clone();
    t.run(Command::PlayContext {
        args: album_args(&sid, "al1"),
    })
    .await;
    t.run(Command::SetAutoplay { enabled: true }).await;
    let mut args = album_args(&sid, "al0");
    args.start_index = Some(3);
    // The last song (fetching ahead), Next at its end (the queue ran out),
    // then a single song, all queued ahead of the batch.
    t.dispatch(Command::PlayContext { args });
    t.dispatch(Command::Next);
    t.dispatch(Command::PlayTracks {
        server_id: sid.clone(),
        track_ids: vec!["t5".into()],
        start_index: 0,
        label: "One".into(),
        shuffle: false,
    });
    t.run_for(2000.0).await;
    assert_eq!(t.current_track_id().await.as_deref(), Some("t5"));
}

/// Position reports (~1 Hz) used to emit TransportChanged *and* MediaSession.
/// The latter rebuilt OS notifications, Android binder playlists and the
/// Pebble now-playing push. UIs extrapolate from the stamp on TransportChanged.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn position_ticks_emit_transport_not_media_session() {
    let t = synced_core("pos-ms").await;
    let sid = t.server_id.clone();
    t.run(Command::PlayContext {
        args: album_args(&sid, "al0"),
    })
    .await;
    t.run_for(200.0).await;
    t.events.clear();
    t.run_for(4_500.0).await;
    let evs = t.events.all();
    let transport = evs
        .iter()
        .filter(|e| matches!(e, Event::TransportChanged { .. }))
        .count();
    let session = evs
        .iter()
        .filter(|e| matches!(e, Event::MediaSession { .. }))
        .count();
    assert!(
        transport >= 4,
        "expected ~1 Hz transport stamps over 4.5s, got {transport}"
    );
    assert_eq!(
        session, 0,
        "MediaSession must not follow position reports (got {session} vs {transport} TransportChanged)"
    );
}

/// Rating a track that is not in the session must not rebuild the queue view / OS session.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rating_off_queue_does_not_emit_queue() {
    let t = synced_core("rate-offq").await;
    let sid = t.server_id.clone();
    t.run(Command::PlayContext {
        args: album_args(&sid, "al0"),
    })
    .await;
    t.run_for(500.0).await;
    t.events.clear();
    t.run(Command::SetRating {
        targets: vec![RatingTarget::Track { id: "t7".into() }],
        rating: 5,
    })
    .await;
    let evs = t.events.all();
    assert!(
        evs.iter().any(|e| matches!(
            e,
            Event::LibraryChanged { ids, .. } if ids.iter().any(|id| id == "t7")
        )),
        "the library still announces the rated row"
    );
    let queue = evs
        .iter()
        .filter(|e| matches!(e, Event::QueueChanged { .. }))
        .count();
    let session = evs
        .iter()
        .filter(|e| matches!(e, Event::MediaSession { .. }))
        .count();
    assert_eq!(queue, 0, "off-queue ratings must not emit QueueChanged");
    assert_eq!(
        session, 0,
        "off-queue ratings must not rebuild MediaSession"
    );
}
