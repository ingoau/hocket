//! YouTube Music through the actor (docs/providers.md, Phase 4), with the
//! InnerTube transport answered from recorded fixtures: anonymous and
//! signed-in accounts, the catalogue, the mirror sync, writes through the
//! outbox, home shelves and an expired sign-in.

#![cfg(feature = "sim")]

use std::sync::Arc;

use hocket_core::api::*;
use hocket_core::core::test_support::TestCore;
use hocket_core::subsonic::fake::FakeServer;
use hocket_core::ytm::fake::{endpoint, fake, players};
use hocket_core::ytm::http::{FakeYtmHttp, HttpResponse};

fn navidrome() -> FakeServer {
    let s = FakeServer::new("srv", "alice");
    s.add_song(FakeServer::song(
        "n1",
        "Navidrome Song",
        "al1",
        "ar1",
        200.0,
    ));
    s
}

fn ytm() -> FakeYtmHttp {
    fake(players(&[("VISIONOS", "player_visionos")]))
}

async fn add_ytm(t: &TestCore, cookies: Option<&str>) {
    t.run(Command::AddAccount {
        setup: AccountSetup::YoutubeMusic {
            cookies: cookies.map(|c| Secret(c.into())),
            js_host: false,
        },
    })
    .await;
}

async fn accounts(t: &TestCore) -> Vec<AccountInfo> {
    match t.query(Query::Accounts).await {
        QueryResult::Accounts(a) => a,
        other => panic!("{other:?}"),
    }
}

async fn wait_ytm_synced(t: &TestCore) {
    t.wait_until(60_000.0, |t| {
        t.events.all().iter().any(|e| {
            matches!(e, Event::SyncProgress { progress } if progress.finished && progress.server_id == "UCme")
        })
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn anonymous_youtube_music_searches_browses_and_keeps_its_catalogue() {
    let http = ytm();
    let t = TestCore::start_with_ytm("a", navidrome(), Arc::new(http.clone())).await;
    add_ytm(&t, None).await;
    let list = accounts(&t).await;
    let y = list
        .iter()
        .find(|a| a.kind == AccountKind::YoutubeMusic)
        .unwrap();
    assert_eq!(y.id, "ytm:anon");
    assert_eq!(y.source, "ytm");
    assert!(
        y.priority > list.iter().find(|a| a.id == "srv").unwrap().priority,
        "Navidrome stays first"
    );
    assert!(!y.features.star && !y.features.library_sync && y.features.home_shelves);
    assert!(y.features.search && y.features.radio && !y.features.rate);

    // A live search lands as server results, and its songs outlive it.
    let _ = t
        .query(Query::Search {
            server_id: String::new(),
            query: "daft punk get lucky".into(),
            limit: 10,
            include_server: true,
            request_id: "r1".into(),
        })
        .await;
    t.wait_until(10_000.0, |t| {
        t.events.all().iter().any(|e| matches!(e, Event::SearchResults { results } if results.from_server && results.tracks.iter().any(|x| x.server_id == "ytm")))
    })
    .await;
    let track = t
        .query(Query::Track {
            track: TrackRef::new("ytm", "4D7u5KF7SP8"),
        })
        .await;
    match track {
        QueryResult::TrackDetail(Some(tr)) => assert!(tr.title.starts_with("Get Lucky")),
        other => panic!("{other:?}"),
    }
    // Not the library.
    match t
        .query(Query::Tracks {
            server_id: "ytm:anon".into(),
            filter: None,
            sort: SortOrder::Title,
            descending: false,
            page: Page {
                offset: 0,
                limit: 50,
            },
        })
        .await
    {
        QueryResult::Tracks(p) => assert_eq!(p.total, 0),
        other => panic!("{other:?}"),
    }

    // An album page outside the library is fetched live, then kept.
    match t
        .query(Query::Album {
            id: "MPREb_K8qWMWVqXGi".into(),
        })
        .await
    {
        QueryResult::AlbumDetail(Some(a)) => {
            assert_eq!(a.name, "Random Access Memories");
            assert_eq!(a.server_id, "ytm");
        }
        other => panic!("{other:?}"),
    }
    let browses = http.endpoints().iter().filter(|e| *e == "browse").count();
    match t
        .query(Query::AlbumTracks {
            id: "MPREb_K8qWMWVqXGi".into(),
        })
        .await
    {
        QueryResult::TrackList(l) => assert_eq!(l.len(), 6),
        other => panic!("{other:?}"),
    }
    assert_eq!(
        http.endpoints().iter().filter(|e| *e == "browse").count(),
        browses,
        "from the catalogue"
    );
    match t
        .query(Query::Artist {
            id: "UCRr1xG_2WIDs18a6cIiCxeA".into(),
        })
        .await
    {
        QueryResult::ArtistDetail(Some(a)) => assert_eq!(a.name, "Daft Punk"),
        other => panic!("{other:?}"),
    }
    match t
        .query(Query::ArtistTopSongs {
            id: "UCRr1xG_2WIDs18a6cIiCxeA".into(),
            count: 3,
        })
        .await
    {
        QueryResult::TrackList(l) => assert_eq!(l.len(), 3),
        other => panic!("{other:?}"),
    }

    // Anonymous adds go to the Hocket library; nothing is sent to YouTube.
    t.run(Command::SetInLibrary {
        tracks: vec![TrackRef::new("ytm", "4D7u5KF7SP8")],
        added: true,
    })
    .await;
    t.run_for(2_000.0).await;
    assert!(!http.endpoints().iter().any(|e| e.starts_with("like/")));
    // An added song lists in the library (from its snapshot), not loved;
    // removed, it goes again.
    let in_library = |t: &TestCore| {
        let db = t.open_db();
        db.with_conn(|c| {
            Ok(c.query_row(
                "SELECT loved FROM tracks WHERE server_id = 'ytm' AND id = '4D7u5KF7SP8'",
                [],
                |r| r.get::<_, bool>(0),
            )
            .ok())
        })
        .unwrap()
    };
    assert_eq!(in_library(&t), Some(false));
    t.run(Command::SetInLibrary {
        tracks: vec![TrackRef::new("ytm", "4D7u5KF7SP8")],
        added: false,
    })
    .await;
    t.run_for(1_000.0).await;
    assert_eq!(in_library(&t), None);

    // Home gets YouTube Music's shelves, songs only.
    let _ = t.query(Query::HomeShelves).await;
    t.wait_until(10_000.0, |t| {
        t.events.all().iter().any(|e| matches!(e, Event::LibraryChanged { tables, .. } if tables.iter().any(|x| x == "home")))
    })
    .await;
    match t.query(Query::HomeShelves).await {
        QueryResult::HomeShelves(s) => {
            let quick = s
                .iter()
                .find(|s| s.id == "ytm:anon/quick-picks")
                .expect("quick picks");
            assert_eq!(quick.kind, HomeShelfKind::Tracks);
            assert!(!quick.tracks.is_empty());
        }
        other => panic!("{other:?}"),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn signed_in_youtube_music_syncs_and_writes_back_through_the_outbox() {
    let http = ytm();
    let t = TestCore::start_with_ytm("a", navidrome(), Arc::new(http.clone())).await;
    add_ytm(&t, Some("SAPISID=abc; SID=x")).await;
    t.wait_until(10_000.0, |t| t.events.all().iter().any(|e| matches!(e, Event::AccountsChanged { accounts } if accounts.iter().any(|a| a.id == "UCme"))))
        .await;
    let y = accounts(&t)
        .await
        .into_iter()
        .find(|a| a.id == "UCme")
        .unwrap();
    assert_eq!(y.name, "YouTube Music (Test User)");
    assert!(
        y.features.star
            && y.features.library_sync
            && y.features.playlists
            && y.features.library_edit
    );
    t.run(Command::SyncLibrary {
        account_id: "UCme".into(),
        full: true,
    })
    .await;
    wait_ytm_synced(&t).await;
    // The account's library is the `ytm` mirror.
    let lib = match t
        .query(Query::Tracks {
            server_id: "UCme".into(),
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
        QueryResult::Tracks(p) => p.items,
        other => panic!("{other:?}"),
    };
    assert!(lib.len() >= 4, "{}", lib.len());
    assert!(lib.iter().all(|x| x.server_id == "ytm"));
    let liked = lib.iter().find(|x| x.id == "4D7u5KF7SP8").unwrap();
    assert!(liked.loved);
    // The mixed playlist lists its songs only.
    match t
        .query(Query::PlaylistTracks {
            id: "PLmixed".into(),
            page: Page {
                offset: 0,
                limit: 50,
            },
        })
        .await
    {
        QueryResult::Tracks(p) => assert_eq!(p.items.len(), 4),
        other => panic!("{other:?}"),
    }

    // Unlike, then move the 4th listed song to 2nd: both reach YouTube.
    t.run(Command::SetLoved {
        targets: vec![RatingTarget::Track {
            id: "4D7u5KF7SP8".into(),
            source: "ytm".into(),
        }],
        loved: false,
    })
    .await;
    t.run(Command::PlaylistMove {
        playlist_id: "PLmixed".into(),
        from_index: 3,
        to_index: 1,
    })
    .await;
    t.wait_until(30_000.0, |_| {
        let e = http.endpoints();
        e.iter().any(|x| x == "like/removelike") && e.iter().any(|x| x == "browse/edit_playlist")
    })
    .await;
    let log = http.log.lock();
    let unlike = log
        .iter()
        .find(|r| endpoint(r) == "like/removelike")
        .unwrap();
    assert_eq!(unlike.json()["target"]["videoId"], "4D7u5KF7SP8");
    let edit = log
        .iter()
        .find(|r| endpoint(r) == "browse/edit_playlist")
        .unwrap()
        .json();
    assert_eq!(edit["actions"][0]["action"], "ACTION_MOVE_VIDEO_BEFORE");
    assert!(edit["actions"][0]["movedSetVideoIdSuccessor"].is_string());
    drop(log);

    // A new playlist with no account named goes to the first song's
    // account (its source `ytm` names none).
    t.events.clear();
    t.run(Command::CreatePlaylist {
        account_id: String::new(),
        name: "New one".into(),
        tracks: vec![TrackRef::new("ytm", "4D7u5KF7SP8")],
    })
    .await;
    t.wait_until(30_000.0, |_| {
        http.endpoints().iter().any(|x| x == "playlist/create")
    })
    .await;
    assert!(!t
        .events
        .all()
        .iter()
        .any(|e| matches!(e, Event::Toast { toast } if toast.message.contains("Unknown server"))));

    // Removing the account unloads its song that is playing.
    t.run(Command::PlayTracks {
        tracks: vec![TrackRef::new("ytm", "4D7u5KF7SP8")],
        start_index: 0,
        label: "x".into(),
        shuffle: false,
        start_position_ms: 0,
    })
    .await;
    t.wait_until(10_000.0, |t| {
        t.backend
            .log()
            .iter()
            .any(|c| matches!(c, hocket_core::audio::scripted::ScriptedCall::Load { .. }))
    })
    .await;
    let stops = |t: &TestCore| {
        t.backend
            .log()
            .iter()
            .filter(|c| matches!(c, hocket_core::audio::scripted::ScriptedCall::Stop))
            .count()
    };
    let before = stops(&t);
    t.run(Command::RemoveAccount {
        account_id: "UCme".into(),
    })
    .await;
    assert!(stops(&t) > before, "its song was unloaded");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_expired_sign_in_asks_to_sign_in_again() {
    let inner = ytm();
    let http = FakeYtmHttp::new(move |r| {
        if endpoint(r) == "account/account_menu" {
            return Some(HttpResponse::json(401, "{}"));
        }
        inner.answer(r)
    });
    let t = TestCore::start_with_ytm("a", navidrome(), Arc::new(http)).await;
    // A persisted signed-in account (as after a restart).
    add_ytm(&t, Some("SAPISID=abc")).await;
    t.wait_until(10_000.0, |t| {
        t.events
            .all()
            .iter()
            .any(|e| matches!(e, Event::Toast { toast } if toast.message.contains("Sign in again")))
    })
    .await;
    assert!(accounts(&t)
        .await
        .iter()
        .all(|a| a.kind != AccountKind::YoutubeMusic));
}
