//! Album resume: recorded only for an album started from the album, past the first track and
//! before it finishes, then offered on another device.

#![cfg(feature = "sim")]

use hocket_core::api::*;
use hocket_core::core::io::memory::MemoryNet;
use hocket_core::core::test_support::{seeded_server, TestCore};
use hocket_core::sim::SimTime;

fn album(server_id: &str, id: &str, shuffle: bool) -> PlayContextArgs {
    PlayContextArgs {
        context: QueueContext {
            server_id: server_id.into(),
            kind: ContextKind::Album { id: id.into() },
            label: String::new(),
            sort: SortOrder::Default,
            tracks: vec![],
        },
        start_index: Some(0),
        shuffle,
        save_outgoing: false,
    }
}

async fn synced(name: &str) -> TestCore {
    let t = TestCore::start(name, seeded_server(8, 180.0)).await;
    t.wait_until(30_000.0, |t| {
        t.events
            .all()
            .iter()
            .any(|e| matches!(e, Event::SyncProgress { progress } if progress.finished))
    })
    .await;
    t
}

async fn point(t: &TestCore, album_id: &str) -> Option<AlbumResumePoint> {
    match t
        .query(Query::AlbumResume {
            id: album_id.into(),
        })
        .await
    {
        QueryResult::AlbumResumeState(p) => p,
        other => panic!("{other:?}"),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pausing_past_the_first_track_is_resumable() {
    let t = synced("resume").await;
    let sid = t.server_id.clone();
    t.run(Command::PlayContext {
        args: album(&sid, "al0", false),
    })
    .await;
    t.run_for(1_000.0).await;
    t.run(Command::Pause).await;
    assert!(
        point(&t, "al0").await.is_none(),
        "the first track is not a resume point"
    );

    t.run(Command::Play).await;
    t.run(Command::Next).await;
    t.run_for(500.0).await;
    t.run(Command::SeekTo {
        position_ms: 45_000,
    })
    .await;
    t.run(Command::Pause).await;
    let p = point(&t, "al0").await.expect("track 2 is resumable");
    assert_eq!(p.track_id, "t1");
    assert_eq!(p.track_index, 1);
    assert_eq!(p.position_ms, 45_000);

    t.run(Command::ResumeAlbum {
        album_id: "al0".into(),
    })
    .await;
    t.run_for(300.0).await;
    assert_eq!(t.current_track_id().await.as_deref(), Some("t1"));
    let at = t.snapshot().await.transport.position.position_ms;
    assert!((40_000..50_000).contains(&at), "resumes near 45s, got {at}");

    t.run(Command::ClearAlbumResume {
        album_id: "al0".into(),
    })
    .await;
    assert!(
        point(&t, "al0").await.is_none(),
        "start over forgets the point"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn shuffle_playlists_and_the_switch_do_not_offer_resume() {
    let t = synced("skip").await;
    let sid = t.server_id.clone();
    t.run(Command::PlayContext {
        args: album(&sid, "al0", true),
    })
    .await;
    t.run_for(400.0).await;
    t.run(Command::Next).await;
    t.run(Command::Pause).await;
    assert!(
        point(&t, "al0").await.is_none(),
        "a shuffled album is not resumed"
    );

    t.run(Command::PlayContext {
        args: PlayContextArgs {
            context: QueueContext {
                server_id: sid.clone(),
                kind: ContextKind::Playlist { id: "p0".into() },
                label: String::new(),
                sort: SortOrder::Default,
                tracks: vec!["t0".into(), "t1".into(), "t2".into()],
            },
            start_index: Some(1),
            shuffle: false,
            save_outgoing: false,
        },
    })
    .await;
    t.run_for(400.0).await;
    t.run(Command::Pause).await;
    assert!(point(&t, "al0").await.is_none());

    t.run(Command::SetSetting {
        key: "playback.resumeAlbums".into(),
        value: "false".into(),
    })
    .await;
    t.run(Command::PlayContext {
        args: album(&sid, "al0", false),
    })
    .await;
    t.run(Command::Next).await;
    t.run(Command::SeekTo {
        position_ms: 20_000,
    })
    .await;
    t.run(Command::Pause).await;
    assert!(
        point(&t, "al0").await.is_none(),
        "the setting turns resume off"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn finishing_the_album_clears_the_point() {
    let t = synced("done").await;
    let sid = t.server_id.clone();
    t.run(Command::SetAutoplay { enabled: false }).await;
    t.run(Command::PlayContext {
        args: PlayContextArgs {
            start_index: Some(3),
            ..album(&sid, "al0", false)
        },
    })
    .await;
    t.run_for(400.0).await;
    // The last track is past the first, so it is a point until it ends.
    t.run(Command::Pause).await;
    assert!(point(&t, "al0").await.is_some());
    t.run(Command::Play).await;
    t.run(Command::Next).await;
    t.run_for(400.0).await;
    assert!(point(&t, "al0").await.is_none(), "the album finished");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_point_saved_on_one_device_resumes_on_the_other() {
    let server = seeded_server(8, 180.0);
    let net = MemoryNet::new();
    let clock = SimTime::new(1_700_000_000_000.0);
    let a = TestCore::start_on("a", server.clone(), Some(net.clone()), 1, clock.clone()).await;
    let b = TestCore::start_on("b", server, Some(net), 2, clock).await;
    TestCore::run_all_for(&[&a, &b], 8_000.0).await;

    let sid = a.server_id.clone();
    a.run(Command::PlayContext {
        args: album(&sid, "al0", false),
    })
    .await;
    a.run(Command::Next).await;
    TestCore::run_all_for(&[&a, &b], 500.0).await;
    a.run(Command::SeekTo {
        position_ms: 45_000,
    })
    .await;
    a.run(Command::Pause).await;
    TestCore::run_all_for(&[&a, &b], 2_000.0).await;

    let p = point(&b, "al0").await.expect("b learned the point");
    assert_eq!(p.track_id, "t1");
    assert_eq!(p.position_ms, 45_000);
}
