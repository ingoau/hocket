//! Dedup across accounts (docs/providers.md, Phase 3): a song on two
//! accounts lists once, plays from the first account, and a queue item from
//! an account another device lacks plays that device's linked copy.

#![cfg(feature = "sim")]

use hocket_core::api::*;
use hocket_core::core::io::memory::MemoryNet;
use hocket_core::core::test_support::TestCore;
use hocket_core::sim::SimTime;
use hocket_core::subsonic::fake::FakeServer;

const A: &str = "srv-a";
const B: &str = "srv-b";

/// A server with the shared song (`<prefix>-shared`, 200 s, or 200.8 s on
/// B), a live version of it, and one song of its own.
fn server(id: &str, user: &str, prefix: &str) -> FakeServer {
    let s = FakeServer::new(id, user);
    let dur = if id == B { 200.8 } else { 200.0 };
    let songs = [
        (format!("{prefix}-shared"), "Shared Song", "alx", dur),
        (format!("{prefix}-live"), "Shared Song (Live)", "alx", 230.0),
        (
            format!("{prefix}-own"),
            &*format!("Only on {prefix}"),
            "alo",
            180.0,
        ),
    ];
    for (i, (song_id, title, album, d)) in songs.iter().enumerate() {
        let mut c = FakeServer::song(song_id, title, album, "arx", *d);
        c.track = Some(i as u32 + 1);
        s.add_song(c);
        s.set_media(song_id, vec![7u8; 4096]);
    }
    s
}

async fn wait_synced(t: &TestCore, accounts: &[&str]) {
    let want: Vec<String> = accounts.iter().map(|s| s.to_string()).collect();
    t.wait_until(60_000.0, |t| {
        let done: Vec<String> = t
            .events
            .all()
            .iter()
            .filter_map(|e| match e {
                Event::SyncProgress { progress } if progress.finished => {
                    Some(progress.server_id.clone())
                }
                _ => None,
            })
            .collect();
        want.iter().all(|w| done.contains(w))
    })
    .await;
}

async fn tracks(t: &TestCore, server_id: &str) -> Vec<(String, String)> {
    match t
        .query(Query::Tracks {
            server_id: server_id.into(),
            filter: None,
            sort: SortOrder::Title,
            descending: false,
            page: Page {
                offset: 0,
                limit: 100,
            },
        })
        .await
    {
        QueryResult::Tracks(p) => p.items.into_iter().map(|t| (t.server_id, t.id)).collect(),
        other => panic!("{other:?}"),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_song_on_two_accounts_lists_once_and_plays_from_the_first() {
    let t = TestCore::start_accounts(
        "dedup",
        server(A, "alice", "a"),
        vec![server(B, "bob", "b")],
    )
    .await;
    wait_synced(&t, &[A, B]).await;

    // One row for the shared song and one for its live version (A's: A
    // comes first), which stays apart from it; each account's own song.
    let mut all = tracks(&t, "").await;
    all.sort();
    let want: Vec<(String, String)> = [(A, "a-live"), (A, "a-own"), (A, "a-shared"), (B, "b-own")]
        .iter()
        .map(|(s, i)| (s.to_string(), i.to_string()))
        .collect();
    assert_eq!(all, want);
    // Filtered to one account, its own copy shows.
    assert!(tracks(&t, B).await.contains(&(B.into(), "b-shared".into())));

    // B's copy queued: A's plays (the higher-priority account has it).
    t.run(Command::PlayTracks {
        tracks: TrackRef::list(B, ["b-shared"]),
        start_index: 0,
        label: "B".into(),
        shuffle: false,
        start_position_ms: 0,
    })
    .await;
    t.run_for(1_000.0).await;
    let key = t.queue().await.current.expect("current").item.key;
    let loaded = t
        .sources
        .lock()
        .iter()
        .rev()
        .find(|s| s.key == key)
        .cloned()
        .expect("loaded");
    assert_eq!(
        (loaded.track.server_id.as_str(), loaded.track.id.as_str()),
        (A, "a-shared")
    );
    // The play is scrobbled to the account whose copy played.
    t.run_for(110_000.0).await;
    let a_submitted: Vec<String> = t
        .server
        .scrobbles()
        .into_iter()
        .filter(|s| s.submission)
        .map(|s| s.id)
        .collect();
    assert_eq!(a_submitted, vec!["a-shared".to_string()]);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_queue_item_from_an_account_the_device_lacks_plays_its_linked_copy() {
    let net = MemoryNet::new();
    let clock = SimTime::new(1_700_000_000_000.0);
    // Device one has both accounts; device two only B.
    let one = TestCore::start_accounts_on(
        "one",
        server(A, "alice", "a"),
        vec![server(B, "bob", "b")],
        Some(net.clone()),
        1,
        clock.clone(),
    )
    .await;
    let two = TestCore::start_on(
        "two",
        server(B, "bob", "b"),
        Some(net.clone()),
        2,
        clock.clone(),
    )
    .await;
    TestCore::run_all_for(&[&one, &two], 8_000.0).await;
    wait_synced(&one, &[A, B]).await;
    wait_synced(&two, &[B]).await;
    TestCore::run_all_for(&[&one, &two], 2_000.0).await;
    assert!(one.snapshot().await.connection.connected);

    // A's copy (and A's own song, which two can't play) queued on one.
    one.run(Command::PlayTracks {
        tracks: TrackRef::list(A, ["a-shared", "a-own"]),
        start_index: 0,
        label: "A".into(),
        shuffle: false,
        start_position_ms: 0,
    })
    .await;
    TestCore::run_all_for(&[&one, &two], 2_000.0).await;
    let q = two.queue().await;
    let current = q.current.clone().expect("two follows the queue");
    assert_eq!(current.item.track, TrackRef::new(A, "a-shared"));
    assert_eq!(
        current.item.alts,
        vec![TrackRef::new(B, "b-shared")],
        "one noted the copy"
    );
    assert_eq!(current.track.title, "Shared Song", "two shows its own copy");

    // Handed to two, the item plays from B.
    one.run(Command::HandoffTo {
        device_id: two.device_id.clone(),
    })
    .await;
    TestCore::run_all_for(&[&one, &two], 3_000.0).await;
    assert!(two.backend.is_playing(), "two took over");
    let key = two.queue().await.current.expect("current").item.key;
    let loaded = two
        .sources
        .lock()
        .iter()
        .rev()
        .find(|s| s.key == key)
        .cloned()
        .expect("two loaded the item");
    assert_eq!(
        (loaded.track.server_id.as_str(), loaded.track.id.as_str()),
        (B, "b-shared")
    );
    assert!(!two.queue().await.current.unwrap().item.unavailable);
}

async fn sources(t: &TestCore, track: TrackRef) -> Vec<(String, String, bool, bool)> {
    match t.query(Query::TrackSources { track }).await {
        QueryResult::TrackSources(list) => list
            .into_iter()
            .map(|s| (s.track.server_id, s.track.id, s.manual, s.preferred))
            .collect(),
        other => panic!("{other:?}"),
    }
}

async fn two_accounts(name: &str, b: FakeServer) -> TestCore {
    let t = TestCore::start_accounts(name, server(A, "alice", "a"), vec![b]).await;
    wait_synced(&t, &[A, B]).await;
    t
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn starring_stars_every_copy_and_links_can_be_made_and_undone_by_hand() {
    let b = server(B, "bob", "b");
    let t = two_accounts("dedup-star", b.clone()).await;

    // The details list both copies; play picks A's.
    let shared = TrackRef::new(A, "a-shared");
    assert_eq!(
        sources(&t, shared.clone()).await,
        vec![
            (A.into(), "a-shared".into(), false, true),
            (B.into(), "b-shared".into(), false, false),
        ]
    );

    // One star, both accounts.
    t.run(Command::SetLoved {
        targets: vec![RatingTarget::Track {
            id: "a-shared".into(),
            source: A.into(),
        }],
        loved: true,
    })
    .await;
    t.wait_until(30_000.0, |t| {
        t.server.starred("a-shared") && b.starred("b-shared")
    })
    .await;
    assert!(t.server.starred("a-shared") && b.starred("b-shared"));

    // "Not the same song": B's copy lists on its own again, and stays apart.
    t.run(Command::UnlinkTrack {
        track: TrackRef::new(B, "b-shared"),
    })
    .await;
    assert_eq!(sources(&t, shared.clone()).await.len(), 1);
    assert!(tracks(&t, "")
        .await
        .contains(&(B.into(), "b-shared".into())));

    // "Link to…" joins two songs the matcher keeps apart.
    t.run(Command::LinkTracks {
        track: TrackRef::new(A, "a-own"),
        to: TrackRef::new(B, "b-own"),
    })
    .await;
    assert_eq!(
        sources(&t, TrackRef::new(B, "b-own")).await,
        vec![
            (A.into(), "a-own".into(), true, true),
            (B.into(), "b-own".into(), true, false),
        ]
    );
    // The candidates for a link are the other account's songs.
    match t
        .query(Query::LinkCandidates {
            track: TrackRef::new(A, "a-own"),
            query: "only".into(),
        })
        .await
    {
        QueryResult::TrackList(list) => {
            assert_eq!(list.len(), 1);
            assert_eq!(
                (list[0].server_id.as_str(), list[0].id.as_str()),
                (B, "b-own")
            );
        }
        other => panic!("{other:?}"),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_account_without_stars_keeps_them_in_the_hocket_library() {
    let b = server(B, "bob", "b");
    b.state.lock().feature_overrides = Some(ProviderCapabilities {
        library_sync: true,
        search: true,
        browse: true,
        stream: true,
        cover_art: true,
        playlists: true,
        report_play: true,
        star: false,
        ..Default::default()
    });
    let t = two_accounts("dedup-hocket-lib", b.clone()).await;
    t.run(Command::SetLoved {
        targets: vec![RatingTarget::Track {
            id: "b-own".into(),
            source: B.into(),
        }],
        loved: true,
    })
    .await;
    t.run_for(5_000.0).await;
    assert!(
        !b.starred("b-own"),
        "never sent to an account that can't keep it"
    );
    match t
        .query(Query::Track {
            track: TrackRef::new(B, "b-own"),
        })
        .await
    {
        QueryResult::TrackDetail(Some(track)) => {
            assert!(track.loved, "starred in the Hocket library")
        }
        other => panic!("{other:?}"),
    }
    // A library sync of B doesn't lose it.
    t.run(Command::SyncLibrary {
        account_id: B.into(),
        full: true,
    })
    .await;
    t.run_for(10_000.0).await;
    match t
        .query(Query::Track {
            track: TrackRef::new(B, "b-own"),
        })
        .await
    {
        QueryResult::TrackDetail(Some(track)) => assert!(track.loved),
        other => panic!("{other:?}"),
    }
}

async fn playlists(t: &TestCore) -> Vec<Playlist> {
    match t
        .query(Query::Playlists {
            server_id: String::new(),
        })
        .await
    {
        QueryResult::Playlists(list) => list,
        other => panic!("{other:?}"),
    }
}

async fn playlist_refs(t: &TestCore, id: &str) -> Vec<(String, String)> {
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
        QueryResult::Tracks(p) => p.items.into_iter().map(|t| (t.server_id, t.id)).collect(),
        other => panic!("{other:?}"),
    }
}

fn pair(s: &str, id: &str) -> (String, String) {
    (s.to_string(), id.to_string())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn mixed_playlists_hold_any_account_s_songs() {
    let b = server(B, "bob", "b");
    let a = server(A, "alice", "a");
    a.add_playlist("pa", "On A", "alice", &["a-own"], false);
    let t = TestCore::start_accounts("dedup-mixed", a.clone(), vec![b]).await;
    wait_synced(&t, &[A, B]).await;

    // A Mixed playlist: one song from each account.
    t.run(Command::CreatePlaylist {
        account_id: "hocket".into(),
        name: "Mix".into(),
        tracks: vec![TrackRef::new(A, "a-own"), TrackRef::new(B, "b-own")],
    })
    .await;
    let mix = playlists(&t)
        .await
        .into_iter()
        .find(|p| p.name == "Mix")
        .expect("the Mixed playlist lists");
    assert_eq!(mix.server_id, "hocket");
    assert_eq!(mix.song_count, 2);
    assert_eq!(
        playlist_refs(&t, &mix.id).await,
        vec![pair(A, "a-own"), pair(B, "b-own")]
    );
    // Edits go to the record, not to a server.
    t.run(Command::PlaylistAdd {
        playlist_id: mix.id.clone(),
        tracks: vec![TrackRef::new(B, "b-shared")],
        at_index: Some(0),
    })
    .await;
    t.run(Command::PlaylistRemove {
        playlist_id: mix.id.clone(),
        indices: vec![1],
    })
    .await;
    assert_eq!(
        playlist_refs(&t, &mix.id).await,
        vec![pair(B, "b-shared"), pair(B, "b-own")]
    );

    // Another account's song with a copy on A goes in as A's copy.
    t.run(Command::PlaylistAdd {
        playlist_id: "pa".into(),
        tracks: vec![TrackRef::new(B, "b-shared")],
        at_index: None,
    })
    .await;
    t.wait_until(30_000.0, |t| {
        t.server
            .state
            .lock()
            .playlist_songs
            .get("pa")
            .is_some_and(|s| s.len() == 2)
    })
    .await;
    assert_eq!(
        t.server.state.lock().playlist_songs["pa"],
        vec!["a-own".to_string(), "a-shared".to_string()]
    );

    // One without a copy: an offer to convert, nothing added meanwhile.
    t.events.clear();
    t.run(Command::PlaylistAdd {
        playlist_id: "pa".into(),
        tracks: vec![TrackRef::new(B, "b-own")],
        at_index: None,
    })
    .await;
    let offer = t
        .events
        .all()
        .into_iter()
        .find_map(|e| match e {
            Event::Toast { toast } if toast.action_label.as_deref() == Some("Convert to Mixed") => {
                Some(toast)
            }
            _ => None,
        })
        .expect("a Convert to Mixed offer");
    let cmd: Command = serde_json::from_str(offer.action_command.as_deref().unwrap()).unwrap();
    t.run(cmd).await;
    t.run_for(5_000.0).await;
    let lists = playlists(&t).await;
    let converted = lists
        .iter()
        .find(|p| p.name == "On A" && p.server_id == "hocket")
        .expect("On A is Mixed now");
    assert_eq!(
        playlist_refs(&t, &converted.id).await,
        vec![pair(A, "a-own"), pair(A, "a-shared"), pair(B, "b-own")]
    );
    assert!(
        !lists.iter().any(|p| p.id == "pa"),
        "the server's copy is gone"
    );
}

/// An artist or album on two accounts (linked) shows what either account
/// has: the album's songs from both copies (each song once), and the
/// artist's albums from both accounts.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_linked_album_and_artist_show_both_accounts_songs() {
    let song = |id: &str, title: &str, album: &str, album_name: &str, artist: &str, track: u32| {
        let mut c = FakeServer::song(id, title, album, artist, 200.0 + track as f64 * 10.0);
        c.album = Some(album_name.into());
        c.artist = Some("Shared Artist".into());
        c.track = Some(track);
        c
    };
    let a = FakeServer::new(A, "alice");
    a.add_song(song("a1", "Song One", "xa", "Shared Album", "ra", 1));
    a.add_song(song("a2", "Song Two", "xa", "Shared Album", "ra", 2));
    let b = FakeServer::new(B, "bob");
    b.add_song(song("b2", "Song Two", "xb", "Shared Album", "rb", 2));
    b.add_song(song("b3", "Song Three", "xb", "Shared Album", "rb", 3));
    b.add_song(song("b9", "Elsewhere", "yb", "Only on B", "rb", 1));
    let t = TestCore::start_accounts("dedup-linked-pages", a, vec![b]).await;
    wait_synced(&t, &[A, B]).await;
    t.run_for(2_000.0).await;
    let ids = |l: Vec<Track>| l.into_iter().map(|t| t.id).collect::<Vec<_>>();
    match t.query(Query::AlbumTracks { id: "xa".into() }).await {
        QueryResult::TrackList(l) => assert_eq!(ids(l), ["a1", "a2", "b3"]),
        other => panic!("{other:?}"),
    }
    match t
        .query(Query::Albums {
            server_id: String::new(),
            artist_id: Some("ra".into()),
            genre: None,
            sort: SortOrder::Title,
            descending: false,
            page: Page {
                offset: 0,
                limit: 50,
            },
        })
        .await
    {
        QueryResult::Albums(p) => {
            let names: Vec<String> = p.items.into_iter().map(|a| a.name).collect();
            assert_eq!(names, ["Only on B", "Shared Album"]);
        }
        other => panic!("{other:?}"),
    }
}

/// Converting deletes the provider's playlist: one whose songs haven't
/// loaded yet is refused, not converted empty.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_playlist_not_yet_loaded_is_not_converted() {
    let a = server(A, "alice", "a");
    a.add_playlist("pa", "On A", "alice", &["a-own", "a-shared"], false);
    let t = TestCore::start_accounts("dedup-convert-unsynced", a.clone(), vec![]).await;
    wait_synced(&t, &[A]).await;
    t.open_db()
        .with_conn(|c| {
            c.execute("UPDATE playlists SET tracks_synced = 0 WHERE id = 'pa'", [])?;
            Ok(())
        })
        .unwrap();
    t.events.clear();
    t.run(Command::ConvertPlaylistToMixed {
        playlist_id: "pa".into(),
        add: vec![],
    })
    .await;
    t.run_for(5_000.0).await;
    let lists = playlists(&t).await;
    assert!(lists.iter().any(|p| p.id == "pa"), "still on the server");
    assert!(!lists.iter().any(|p| p.server_id == "hocket"));
    assert_eq!(t.server.state.lock().playlist_songs["pa"].len(), 2);
    assert!(t.events.all().iter().any(
        |e| matches!(e, Event::Toast { toast } if toast.message.contains("hasn't finished loading"))
    ));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn home_has_continue_listening_and_each_account_s_shelves() {
    let t = two_accounts("dedup-home", server(B, "bob", "b")).await;
    let shelves = match t.query(Query::HomeShelves).await {
        QueryResult::HomeShelves(s) => s,
        other => panic!("{other:?}"),
    };
    let ids: Vec<&str> = shelves.iter().map(|s| s.id.as_str()).collect();
    assert_eq!(
        ids,
        vec![
            "hocket/continueListening",
            "srv-a/recentlyAdded",
            "srv-a/mostPlayed",
            "srv-a/recentlyPlayed",
            "srv-b/recentlyAdded",
            "srv-b/mostPlayed",
            "srv-b/recentlyPlayed",
        ]
    );
    let added = &shelves[1];
    assert!(added.default_on && !shelves[2].default_on);
    assert!(
        added.title.starts_with("Recently added \u{b7} "),
        "{}",
        added.title
    );
    assert!(added.albums.iter().all(|a| a.server_id == A) && !added.albums.is_empty());
}
