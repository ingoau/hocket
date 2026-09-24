//! Library edits across devices: two full cores on the in-memory Connect
//! network (LAN tier). A rating, love or playlist edit on one device lands
//! in the other's mirror at once (without waiting for its library sync) and
//! both emit the same events for their screens.

#![cfg(feature = "sim")]

use hocket_core::api::*;
use hocket_core::core::io::memory::MemoryNet;
use hocket_core::core::test_support::{seeded_server, TestCore};
use hocket_core::sim::SimTime;

async fn pair() -> (TestCore, TestCore) {
    let server = seeded_server(6, 200.0);
    server.add_playlist("pl", "Mine", "alice", &["t0", "t1"], false);
    let net = MemoryNet::new();
    let clock = SimTime::new(1_700_000_000_000.0);
    let a = TestCore::start_on("a", server.clone(), Some(net.clone()), 1, clock.clone()).await;
    let b = TestCore::start_on("b", server.clone(), Some(net.clone()), 2, clock.clone()).await;
    TestCore::run_all_for(&[&a, &b], 8_000.0).await;
    let ca = a.snapshot().await.connection;
    let cb = b.snapshot().await.connection;
    assert!(ca.connected && cb.connected, "{ca:?} {cb:?}");
    (a, b)
}

async fn track(t: &TestCore, id: &str) -> Track {
    match t.query(Query::Track { id: id.into() }).await {
        QueryResult::TrackDetail(Some(tr)) => tr,
        other => panic!("{other:?}"),
    }
}

async fn mirror_playlist(t: &TestCore, id: &str) -> Vec<String> {
    match t
        .query(Query::PlaylistTracks {
            id: id.into(),
            page: Page {
                offset: 0,
                limit: 100,
            },
        })
        .await
    {
        QueryResult::Tracks(p) => p.items.into_iter().map(|t| t.id).collect(),
        other => panic!("{other:?}"),
    }
}

/// The `LibraryItemsChanged` states a core emitted for `id`, with their origin.
fn item_events(t: &TestCore, id: &str) -> Vec<(LibraryItemState, Option<String>)> {
    t.events
        .all()
        .into_iter()
        .filter_map(|e| match e {
            Event::LibraryItemsChanged {
                items, from_device, ..
            } => Some(
                items
                    .into_iter()
                    .filter(|i| i.id == id)
                    .map(|i| (i, from_device.clone()))
                    .collect::<Vec<_>>(),
            ),
            _ => None,
        })
        .flatten()
        .collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ratings_and_loves_reach_the_other_device_and_its_now_playing() {
    let (a, b) = pair().await;
    a.run(Command::PlayTracks {
        server_id: a.server_id.clone(),
        track_ids: vec!["t0".into(), "t1".into()],
        start_index: 0,
        label: "Sel".into(),
        shuffle: false,
    })
    .await;
    TestCore::run_all_for(&[&a, &b], 2_000.0).await;
    a.events.clear();
    b.events.clear();

    a.run(Command::SetRating {
        targets: vec![RatingTarget::Track { id: "t0".into() }],
        rating: 3,
    })
    .await;
    a.run(Command::SetLoved {
        targets: vec![RatingTarget::Track { id: "t0".into() }],
        loved: true,
    })
    .await;
    TestCore::run_all_for(&[&a, &b], 1_000.0).await;

    // This device: told at once, from nobody else.
    let here = item_events(&a, "t0");
    assert_eq!(here.first().map(|(i, _)| i.rating), Some(3), "{here:?}");
    assert!(here.iter().all(|(_, from)| from.is_none()));
    // The other device: its mirror, its events and its now playing follow.
    let tr = track(&b, "t0").await;
    assert_eq!((tr.rating, tr.loved), (3, true));
    let there = item_events(&b, "t0");
    assert!(
        there.iter().any(|(i, from)| i.rating == 3
            && i.loved
            && i.kind == LibraryItemKind::Track
            && from.as_deref() == Some(a.device_id.as_str())),
        "{there:?}"
    );
    let np = b
        .events
        .all()
        .into_iter()
        .rev()
        .find_map(|e| match e {
            Event::NowPlayingChanged { entry } => entry,
            _ => None,
        })
        .expect("b re-emitted now playing");
    assert_eq!((np.track.rating, np.track.loved), (3, true));
    // The server write is the sender's alone.
    assert_eq!(a.server.rating_of("t0"), 3);
    assert!(a.server.starred("t0"));

    // Undo on the sender: the other device follows back.
    a.run(Command::Undo).await;
    TestCore::run_all_for(&[&a, &b], 1_000.0).await;
    assert!(!track(&b, "t0").await.loved, "undo reached b");
    assert!(!a.server.starred("t0"));

    // And the other way round, for an artist.
    b.run(Command::SetArtistLoved {
        artist_id: "ar0".into(),
        loved: true,
    })
    .await;
    TestCore::run_all_for(&[&a, &b], 1_000.0).await;
    match a.query(Query::Artist { id: "ar0".into() }).await {
        QueryResult::ArtistDetail(Some(ar)) => assert!(ar.loved),
        other => panic!("{other:?}"),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn playlist_edits_reach_the_other_device() {
    let (a, b) = pair().await;
    assert_eq!(mirror_playlist(&b, "pl").await, ["t0", "t1"]);
    b.events.clear();

    a.run(Command::PlaylistAdd {
        playlist_id: "pl".into(),
        track_ids: vec!["t4".into()],
        at_index: None,
    })
    .await;
    TestCore::run_all_for(&[&a, &b], 1_000.0).await;
    assert_eq!(a.server.playlist_song_ids("pl"), ["t0", "t1", "t4"]);
    assert_eq!(mirror_playlist(&b, "pl").await, ["t0", "t1", "t4"]);
    assert!(b.events.all().iter().any(|e| matches!(
        e,
        Event::PlaylistChanged { playlist_id, playlist: Some(p), from_device: Some(from), .. }
            if playlist_id == "pl" && p.song_count == 3 && from == &a.device_id
    )));

    // Removed on the other device.
    b.run(Command::PlaylistRemove {
        playlist_id: "pl".into(),
        indices: vec![0],
    })
    .await;
    TestCore::run_all_for(&[&a, &b], 1_000.0).await;
    assert_eq!(mirror_playlist(&a, "pl").await, ["t1", "t4"]);

    // A new playlist shows up once the server has given it an id.
    a.run(Command::CreatePlaylist {
        server_id: a.server_id.clone(),
        name: "Road trip".into(),
        track_ids: vec!["t2".into(), "t3".into()],
    })
    .await;
    TestCore::run_all_for(&[&a, &b], 1_000.0).await;
    let created = match b
        .query(Query::Playlists {
            server_id: b.server_id.clone(),
        })
        .await
    {
        QueryResult::Playlists(list) => list.into_iter().find(|p| p.name == "Road trip"),
        other => panic!("{other:?}"),
    }
    .expect("b has the new playlist");
    assert_eq!(created.server_id, b.server_id);
    assert_eq!(mirror_playlist(&b, &created.id).await, ["t2", "t3"]);

    // Deleted: gone on both.
    b.run(Command::DeletePlaylist {
        playlist_id: created.id.clone(),
    })
    .await;
    TestCore::run_all_for(&[&a, &b], 1_000.0).await;
    match a.query(Query::Playlist { id: created.id }).await {
        QueryResult::PlaylistDetail(p) => assert!(p.is_none(), "{p:?}"),
        other => panic!("{other:?}"),
    }
}
