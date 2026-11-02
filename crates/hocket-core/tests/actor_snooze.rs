//! Snooze through full cores: the song menu's command, global undo, which queues leave a snoozed
//! song out, expiry, and two devices sharing the set.

#![cfg(feature = "sim")]

use hocket_core::api::*;
use hocket_core::core::io::memory::MemoryNet;
use hocket_core::core::test_support::{seeded_server, TestCore};
use hocket_core::sim::SimTime;

fn ids(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

async fn snoozed(t: &TestCore) -> Vec<String> {
    match t.query(Query::Snoozes).await {
        QueryResult::SnoozeItems(items) => items.into_iter().map(|i| i.track_id).collect(),
        other => panic!("{other:?}"),
    }
}

fn queue_ids(q: &QueueView) -> Vec<String> {
    q.current
        .iter()
        .chain(q.playing_next.iter())
        .chain(q.upcoming.iter())
        .map(|e| e.track.id.clone())
        .collect()
}

fn context(server: &str, kind: ContextKind, tracks: &[&str]) -> QueueContext {
    QueueContext {
        server_id: server.into(),
        kind,
        label: "queue".into(),
        sort: SortOrder::Default,
        tracks: ids(tracks),
    }
}

async fn play(t: &TestCore, kind: ContextKind, tracks: &[&str], start: Option<u32>) {
    t.run(Command::PlayContext {
        args: PlayContextArgs {
            context: context(&t.server_id, kind, tracks),
            start_index: start,
            shuffle: false,
            save_outgoing: false,
            start_position_ms: None,
        },
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn snooze_undo_and_each_queue() {
    let t = TestCore::start("snooze", seeded_server(6, 200.0)).await;
    t.run(Command::Snooze {
        track_ids: ids(&["b"]),
        duration: SnoozeFor::Week,
    })
    .await;
    let items = match t.query(Query::Snoozes).await {
        QueryResult::SnoozeItems(items) => items,
        other => panic!("{other:?}"),
    };
    assert_eq!(items[0].track_id, "b");
    let week = 7.0 * 24.0 * 3600.0 * 1000.0;
    let until = items[0].until.expect("a week ends");
    assert!((until - (t.clock.now_ms() + week)).abs() < 1.0);

    // The song that's playing keeps playing. Snooze applies from the next queue on.
    play(
        &t,
        ContextKind::Album { id: "al".into() },
        &["a", "b", "c"],
        None,
    )
    .await;
    assert_eq!(queue_ids(&t.queue().await), ids(&["a", "b", "c"]));
    t.run(Command::Snooze {
        track_ids: ids(&["a"]),
        duration: SnoozeFor::Day,
    })
    .await;
    assert_eq!(
        t.queue().await.current.map(|e| e.track.id),
        Some("a".into()),
        "snoozing the current song does not skip it"
    );

    play(
        &t,
        ContextKind::Playlist { id: "p".into() },
        &["a", "b", "c"],
        None,
    )
    .await;
    assert_eq!(
        queue_ids(&t.queue().await),
        ids(&["c"]),
        "playlist drops a and b"
    );

    play(
        &t,
        ContextKind::Artist { id: "ar".into() },
        &["a", "b", "c"],
        None,
    )
    .await;
    assert_eq!(queue_ids(&t.queue().await), ids(&["c"]));

    play(
        &t,
        ContextKind::Genre { name: "g".into() },
        &["b", "c"],
        None,
    )
    .await;
    assert_eq!(queue_ids(&t.queue().await), ids(&["c"]));

    play(&t, ContextKind::Autoplay, &["a", "c"], None).await;
    assert_eq!(queue_ids(&t.queue().await), ids(&["c"]));

    // Picking the snoozed song plays it; the other snoozed song stays out.
    play(
        &t,
        ContextKind::Playlist { id: "p".into() },
        &["a", "b", "c"],
        Some(0),
    )
    .await;
    assert_eq!(queue_ids(&t.queue().await), ids(&["a", "c"]));

    // An album plays the snoozed songs, including one the listener starts on.
    play(
        &t,
        ContextKind::Album { id: "al".into() },
        &["a", "b"],
        None,
    )
    .await;
    assert_eq!(queue_ids(&t.queue().await), ids(&["a", "b"]));
    play(
        &t,
        ContextKind::Album { id: "al".into() },
        &["a", "b"],
        Some(1),
    )
    .await;
    assert_eq!(
        t.queue().await.current.map(|e| e.track.id),
        Some("b".into())
    );

    // A list the listener picked (search) plays as it stands.
    play(
        &t,
        ContextKind::AdHoc {
            label: "search".into(),
        },
        &["a", "b"],
        None,
    )
    .await;
    assert_eq!(queue_ids(&t.queue().await), ids(&["a", "b"]));

    // Every song snoozed: play them anyway.
    t.run(Command::Snooze {
        track_ids: ids(&["c"]),
        duration: SnoozeFor::Indefinite,
    })
    .await;
    play(
        &t,
        ContextKind::Playlist { id: "p".into() },
        &["a", "b", "c"],
        None,
    )
    .await;
    assert_eq!(queue_ids(&t.queue().await), ids(&["a", "b", "c"]));

    // Undo brings the last one back (c), and the toast's Undo is the global one.
    let toast = t
        .events
        .all()
        .into_iter()
        .rev()
        .find_map(|e| match e {
            Event::Toast { toast } => Some(toast),
            _ => None,
        })
        .expect("toast");
    assert_eq!(toast.message, "Snoozed indefinitely");
    let undo: Command = serde_json::from_str(toast.action_command.as_deref().unwrap()).unwrap();
    assert!(matches!(undo, Command::Undo));
    // The play sits above the snooze on the stack; undo both.
    t.run(Command::Undo).await;
    t.run(Command::Undo).await;
    let left = snoozed(&t).await;
    assert!(!left.contains(&"c".to_string()));
    assert!(left.contains(&"a".to_string()) && left.contains(&"b".to_string()));

    t.run(Command::Unsnooze {
        track_ids: ids(&["a", "b"]),
    })
    .await;
    assert!(snoozed(&t).await.is_empty());
    t.run(Command::Undo).await;
    let back = snoozed(&t).await;
    assert!(back.contains(&"a".to_string()) && back.contains(&"b".to_string()));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn shuffle_and_play_leave_out_a_snoozed_first_song() {
    let t = TestCore::start("shuffle", seeded_server(4, 200.0)).await;
    t.run(Command::Snooze {
        track_ids: ids(&["a"]),
        duration: SnoozeFor::Week,
    })
    .await;
    play(
        &t,
        ContextKind::Playlist { id: "p".into() },
        &["a", "b", "c"],
        None,
    )
    .await;
    assert_eq!(queue_ids(&t.queue().await), ids(&["b", "c"]));
    t.run(Command::PlayContext {
        args: PlayContextArgs {
            context: context(
                &t.server_id,
                ContextKind::Playlist { id: "p".into() },
                &["a", "b", "c"],
            ),
            start_index: None,
            shuffle: true,
            save_outgoing: false,
            start_position_ms: None,
        },
    })
    .await;
    let q = t.queue().await;
    assert!(q.shuffle);
    let played = queue_ids(&q);
    assert!(!played.contains(&"a".to_string()), "{played:?}");
    assert_eq!(played.len(), 2);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_snooze_expires_when_its_end_passes() {
    let t = TestCore::start("expire", seeded_server(4, 200.0)).await;
    t.run(Command::Snooze {
        track_ids: ids(&["t0"]),
        duration: SnoozeFor::Day,
    })
    .await;
    assert_eq!(snoozed(&t).await, ids(&["t0"]));
    t.clock.advance(24.0 * 3600.0 * 1000.0 + 1.0);
    t.core.tick().expect("tick");
    t.core.settle().await;
    assert!(snoozed(&t).await.is_empty());
    play(
        &t,
        ContextKind::Playlist { id: "p".into() },
        &["t0", "t1"],
        None,
    )
    .await;
    assert_eq!(queue_ids(&t.queue().await), ids(&["t0", "t1"]));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn two_devices_share_snoozes() {
    let server = seeded_server(4, 200.0);
    let net = MemoryNet::new();
    let clock = SimTime::new(1_700_000_000_000.0);
    let a = TestCore::start_on("a", server.clone(), Some(net.clone()), 1, clock.clone()).await;
    let b = TestCore::start_on("b", server, Some(net), 2, clock).await;
    TestCore::run_all_for(&[&a, &b], 8_000.0).await;
    assert!(a.snapshot().await.connection.connected);

    a.run(Command::Snooze {
        track_ids: ids(&["t0"]),
        duration: SnoozeFor::Month,
    })
    .await;
    TestCore::run_all_for(&[&a, &b], 1_000.0).await;
    assert_eq!(snoozed(&b).await, ids(&["t0"]));

    b.run(Command::Unsnooze {
        track_ids: ids(&["t0"]),
    })
    .await;
    b.run(Command::Snooze {
        track_ids: ids(&["t1"]),
        duration: SnoozeFor::Indefinite,
    })
    .await;
    TestCore::run_all_for(&[&a, &b], 1_000.0).await;
    let mut on_a = snoozed(&a).await;
    on_a.sort();
    assert_eq!(on_a, ids(&["t1"]));
    assert_eq!(snoozed(&b).await, ids(&["t1"]));
}

/// A saved queue keeps no track list for a playlist; restoring it re-resolves the playlist. The
/// cursor, the shuffle order and each item's context index count positions in the queued list,
/// which left the snoozed songs out, so the rebuilt list has to leave them out too.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_restored_playlist_queue_still_leaves_snoozed_songs_out() {
    let server = seeded_server(10, 200.0);
    let all = ["t0", "t1", "t2", "t3", "t4", "t5", "t6", "t7", "t8", "t9"];
    server.add_playlist("pl", "Mine", "alice", &all, false);
    let t = TestCore::start("snooze-restore", server).await;
    t.wait_until(30_000.0, |t| {
        t.events
            .all()
            .iter()
            .any(|e| matches!(e, Event::SyncProgress { progress } if progress.finished))
    })
    .await;
    let out = ids(&["t2", "t5", "t7"]);
    t.run(Command::Snooze {
        track_ids: out.clone(),
        duration: SnoozeFor::Week,
    })
    .await;

    let sid = t.server_id.clone();
    t.run(Command::PlayContext {
        args: PlayContextArgs {
            context: context(&sid, ContextKind::Playlist { id: "pl".into() }, &[]),
            start_index: None,
            shuffle: true,
            save_outgoing: false,
            start_position_ms: None,
        },
    })
    .await;
    t.run(Command::Next).await;
    t.run_for(500.0).await;
    let before = queue_ids(&t.queue().await);
    assert_eq!(before.len(), 6, "{before:?}");
    assert!(before.iter().all(|id| !out.contains(id)), "{before:?}");

    // Playing an album saves the shuffled playlist queue.
    t.run(Command::PlayContext {
        args: PlayContextArgs {
            context: context(&sid, ContextKind::Album { id: "al0".into() }, &[]),
            start_index: None,
            shuffle: false,
            save_outgoing: true,
            start_position_ms: None,
        },
    })
    .await;
    t.run_for(500.0).await;
    let saved = t.snapshot().await.session.unwrap().saved_queues;
    let sq = saved
        .iter()
        .find(|q| matches!(&q.context.kind, ContextKind::Playlist { id } if id == "pl"))
        .expect("the playlist queue was saved")
        .clone();
    assert!(
        sq.context.tracks.is_empty(),
        "snapshots omit the track list"
    );

    // Saving it as a playlist keeps the same songs, in the same order.
    t.run(Command::SaveQueueAsPlaylist {
        saved_queue_id: Some(sq.id.clone()),
        name: "Again".into(),
    })
    .await;
    t.run_for(500.0).await;
    let made = (1..=100)
        .map(|i| t.server.playlist_song_ids(&format!("pl-new-{i}")))
        .find(|songs| !songs.is_empty())
        .expect("a playlist was made");
    let mut played = t
        .snapshot()
        .await
        .session
        .unwrap()
        .saved_queues
        .iter()
        .find(|q| q.id == sq.id)
        .unwrap()
        .history
        .iter()
        .map(|i| i.track_id.clone())
        .collect::<Vec<_>>();
    played.extend(before.iter().cloned());
    assert_eq!(made, played);

    t.run(Command::RestoreSavedQueue { id: sq.id.clone() })
        .await;
    t.run_for(500.0).await;
    assert_eq!(queue_ids(&t.queue().await), before);
}
