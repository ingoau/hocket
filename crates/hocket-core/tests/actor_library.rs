//! Library scenarios: sync, search, mutations through the outbox, undo tier
//! 2, downloads, filters, lyrics and settings.

#![cfg(feature = "sim")]

use std::sync::Arc;

use futures::future::BoxFuture;
use hocket_core::api::*;
use hocket_core::core::test_support::{seeded_server, TestBackend, TestCore, TestOptions};
use hocket_core::lyrics::{LyricsError, LyricsHttp};
use hocket_core::sim::SimTime;
use hocket_core::subsonic::fake::FakeServer;
use hocket_core::subsonic::types::{CueLine, LyricCue, LyricLineBody, StructuredLyrics};
use hocket_core::subsonic::SubsonicError;
use hocket_core::Core;

async fn synced(name: &str, server: FakeServer) -> TestCore {
    let t = TestCore::start(name, server).await;
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
async fn library_sync_reports_ready_tables_in_order_and_fills_the_mirror() {
    let t = synced("sync", seeded_server(12, 180.0)).await;
    let progress: Vec<SyncProgress> = t
        .events
        .all()
        .into_iter()
        .filter_map(|e| match e {
            Event::SyncProgress { progress } => Some(progress),
            _ => None,
        })
        .collect();
    assert!(!progress.is_empty());
    let mut last = 0usize;
    for p in &progress {
        assert!(
            p.ready_tables.len() >= last,
            "ready tables never shrink: {:?}",
            p.ready_tables
        );
        last = p.ready_tables.len();
    }
    let done = progress.last().unwrap();
    assert!(done.finished);
    for table in ["artists", "albums", "tracks", "playlists", "genres"] {
        assert!(
            done.ready_tables.iter().any(|t| t == table),
            "{table} ready"
        );
    }
    // LibraryChanged fired as tables became ready and once at the end.
    let changed: Vec<Vec<String>> = t
        .events
        .all()
        .into_iter()
        .filter_map(|e| match e {
            Event::LibraryChanged { tables, .. } => Some(tables),
            _ => None,
        })
        .collect();
    assert!(changed.iter().any(|t| t.iter().any(|x| x == "tracks")));
    assert!(changed.iter().any(|t| t.is_empty()));
    let sid = t.server_id.clone();
    match t
        .query(Query::Tracks {
            server_id: sid.clone(),
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
        QueryResult::Tracks(p) => assert_eq!(p.total, 12),
        other => panic!("{other:?}"),
    }
    match t
        .query(Query::AlbumCount {
            server_id: sid.clone(),
            artist_id: None,
            genre: None,
        })
        .await
    {
        QueryResult::Count(n) => assert_eq!(n, 3),
        other => panic!("{other:?}"),
    }
    let servers = t.snapshot().await.accounts;
    assert!(servers[0].last_sync.is_some());
    assert!(servers[0].reachable);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn search_answers_locally_first_then_from_the_server() {
    let server = seeded_server(4, 180.0);
    // A song the mirror does not have yet (added after the sync).
    let t = synced("search", server.clone()).await;
    server.add_song(FakeServer::song(
        "later",
        "Track later",
        "al9",
        "ar9",
        100.0,
    ));
    let sid = t.server_id.clone();
    let result = t
        .query(Query::Search {
            server_id: sid,
            query: "Track".into(),
            limit: 20,
            include_server: true,
            request_id: "r1".into(),
        })
        .await;
    let QueryResult::Search(local) = result else {
        panic!()
    };
    assert_eq!(local.request_id, "r1");
    assert!(!local.from_server);
    assert_eq!(local.tracks.len(), 4);
    t.core.settle().await;
    let batches: Vec<SearchResults> = t
        .events
        .all()
        .into_iter()
        .filter_map(|e| match e {
            Event::SearchResults { results } if results.request_id == "r1" => Some(results),
            _ => None,
        })
        .collect();
    assert_eq!(batches.len(), 2, "local batch then server batch");
    assert!(!batches[0].from_server);
    assert!(batches[1].from_server);
    assert!(batches[1].tracks.iter().any(|t| t.id == "later"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ratings_go_through_the_outbox_and_undo_is_compare_and_swap() {
    let t = synced("rate", seeded_server(4, 180.0)).await;
    t.run(Command::SetRating {
        targets: vec![RatingTarget::Track {
            id: "t0".into(),
            source: String::new(),
        }],
        rating: 4,
    })
    .await;
    // Optimistic locally, then flushed.
    match t
        .query(Query::Track {
            track: TrackRef::new(t.server_id.clone(), "t0"),
        })
        .await
    {
        QueryResult::TrackDetail(Some(tr)) => assert_eq!(tr.rating, 4),
        other => panic!("{other:?}"),
    }
    t.run_for(500.0).await;
    assert_eq!(t.server.rating_of("t0"), 4);
    assert!(t.snapshot().await.undo.can_undo);

    // Undo: the server still holds what we set, so the inverse applies.
    t.run(Command::Undo).await;
    t.run_for(500.0).await;
    assert_eq!(t.server.rating_of("t0"), 0);
    assert!(t
        .events
        .all()
        .iter()
        .any(|e| matches!(e, Event::Toast { toast } if toast.message == "Undid Rate 4 stars")));

    // Redo, then someone else changes it: undo reports honestly and leaves it.
    t.run(Command::Redo).await;
    t.run_for(500.0).await;
    assert_eq!(t.server.rating_of("t0"), 4);
    t.server
        .state
        .lock()
        .songs
        .get_mut("t0")
        .unwrap()
        .user_rating = Some(2);
    t.run(Command::Undo).await;
    t.run_for(500.0).await;
    assert_eq!(t.server.rating_of("t0"), 2, "changed elsewhere: left alone");
    assert!(t.events.all().iter().any(|e| matches!(
        e,
        Event::Toast { toast } if toast.message == "Undid Rate 4 stars: undid 0 of 1, 1 changed elsewhere"
    )));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rating_bridges_to_love_above_the_threshold_when_enabled() {
    let t = synced("bridge", seeded_server(2, 180.0)).await;
    t.run(Command::SetRating {
        targets: vec![RatingTarget::Track {
            id: "t0".into(),
            source: String::new(),
        }],
        rating: 5,
    })
    .await;
    t.run_for(500.0).await;
    assert!(!t.server.starred("t0"), "bridge is off by default");
    t.run(Command::SetSetting {
        key: "ratings.loveBridge.enabled".into(),
        value: "true".into(),
    })
    .await;
    t.run(Command::SetRating {
        targets: vec![RatingTarget::Track {
            id: "t1".into(),
            source: String::new(),
        }],
        rating: 4,
    })
    .await;
    t.run_for(500.0).await;
    assert!(t.server.starred("t1"));
    assert_eq!(t.server.rating_of("t1"), 4);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn offline_changes_flush_after_the_network_returns() {
    let t = synced("offline", seeded_server(3, 180.0)).await;
    t.server.set_offline(true);
    t.run(Command::SetNetworkState {
        state: NetworkState {
            kind: NetworkKind::Offline,
            metered: false,
            network_id: None,
        },
    })
    .await;
    t.run(Command::SetLoved {
        targets: vec![RatingTarget::Track {
            id: "t2".into(),
            source: String::new(),
        }],
        loved: true,
    })
    .await;
    t.run(Command::CreatePlaylist {
        account_id: t.server_id.clone(),
        name: "Road trip".into(),
        tracks: TrackRef::list(&t.server_id, ["t0", "t1"]),
    })
    .await;
    t.run_for(2_000.0).await;
    assert!(!t.server.starred("t2"));
    assert!(t
        .server
        .state
        .lock()
        .playlists
        .values()
        .all(|p| p.name != "Road trip"));
    // Local view already reflects the love.
    match t
        .query(Query::Track {
            track: TrackRef::new(t.server_id.clone(), "t2"),
        })
        .await
    {
        QueryResult::TrackDetail(Some(tr)) => assert!(tr.loved),
        other => panic!("{other:?}"),
    }
    t.server.set_offline(false);
    t.run(Command::SetNetworkState {
        state: NetworkState {
            kind: NetworkKind::Wifi,
            metered: false,
            network_id: Some("home".into()),
        },
    })
    .await;
    t.run_for(40_000.0).await;
    assert!(t.server.starred("t2"));
    assert!(t
        .server
        .state
        .lock()
        .playlists
        .values()
        .any(|p| p.name == "Road trip"));
    assert!(t
        .events
        .all()
        .iter()
        .any(|e| matches!(e, Event::LibraryChanged { tables, .. } if tables.iter().any(|x| x == "playlists"))));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn filters_preview_export_and_materialise() {
    let t = synced("filters", seeded_server(6, 180.0)).await;
    t.run(Command::SetRating {
        targets: vec![
            RatingTarget::Track {
                id: "t1".into(),
                source: String::new(),
            },
            RatingTarget::Track {
                id: "t2".into(),
                source: String::new(),
            },
        ],
        rating: 5,
    })
    .await;
    let filter = Filter {
        id: "top".into(),
        name: "Top".into(),
        root: FilterNode::All(vec![FilterNode::Rule(FilterRule {
            field: FilterField::Rating,
            op: FilterOp::Gt,
            value: FilterValue::Number(3.0),
        })]),
        sort: SortOrder::Title,
        descending: false,
        limit: None,
    };
    match t
        .query(Query::FilterPreview {
            filter: filter.clone(),
        })
        .await
    {
        QueryResult::Preview(p) => {
            assert_eq!(p.count, 2);
            assert!(p.capability.server_expressible);
            assert_eq!(p.sample.len(), 2);
        }
        other => panic!("{other:?}"),
    }
    let local_only = Filter {
        root: FilterNode::All(vec![FilterNode::Rule(FilterRule {
            field: FilterField::Downloaded,
            op: FilterOp::IsTrue,
            value: FilterValue::Bool(true),
        })]),
        ..filter.clone()
    };
    match t.query(Query::FilterPreview { filter: local_only }).await {
        QueryResult::Preview(p) => {
            assert_eq!(p.count, 0);
            assert!(!p.capability.server_expressible);
            assert_eq!(
                p.capability.local_only_fields,
                vec![FilterField::Downloaded]
            );
        }
        other => panic!("{other:?}"),
    }
    t.run(Command::SaveFilter {
        filter: filter.clone(),
    })
    .await;
    match t.query(Query::Filters).await {
        QueryResult::Filters(f) => assert!(f.iter().any(|f| f.id == "top")),
        other => panic!("{other:?}"),
    }
    t.run(Command::ExportNsp {
        filter: filter.clone(),
        path: None,
    })
    .await;
    let nsp = t
        .events
        .all()
        .into_iter()
        .find_map(|e| match e {
            Event::NspExported {
                document,
                filter_id,
                ..
            } if filter_id == "top" => Some(document),
            _ => None,
        })
        .expect("nsp exported");
    assert!(nsp.contains("rating"), "{nsp}");
    t.run(Command::CreateStaticPlaylistFromFilter {
        server_id: t.server_id.clone(),
        filter,
        name: "Top static".into(),
    })
    .await;
    t.run_for(500.0).await;
    let st = t.server.state.lock();
    let pl = st
        .playlists
        .values()
        .find(|p| p.name == "Top static")
        .expect("created");
    assert_eq!(
        st.playlist_songs[&pl.id],
        vec!["t1".to_string(), "t2".to_string()]
    );
}

/// [`synced`], with this HTTP for external services (lyrics, Deezer) instead of the network.
async fn synced_with_http(name: &str, server: FakeServer, http: Arc<dyn LyricsHttp>) -> TestCore {
    let clock = SimTime::new(1_700_000_000_000.0);
    let dir = tempfile::tempdir().unwrap();
    let config = CoreConfig {
        data_dir: dir.path().join("d").to_string_lossy().into_owned(),
        cache_dir: dir.path().join("c").to_string_lossy().into_owned(),
        device_id: format!("dev-{name}"),
        device_name: name.into(),
        platform: Platform::Linux,
        app_version: "test".into(),
        audio: AudioMode::None,
        coordinator_listen: None,
    };
    let (backend, scripted) = TestBackend::scripted(clock.clone());
    let core = Core::new_for_test_with(
        config,
        clock.clone(),
        TestOptions {
            backend,
            api: Some(Arc::new(server.clone())),
            more_apis: vec![],
            server_url: "https://music.example/".into(),
            password: "pw".into(),
            net: None,
            lyrics_http: Some(http),
            seed: 7,
            reducer: None,
            ytm_http: None,
        },
    )
    .unwrap();
    let events = Arc::new(hocket_core::core::test_support::EventLog::default());
    core.add_sink(events.clone());
    core.dispatch(Command::Start).unwrap();
    core.settle().await;
    let t = TestCore {
        core,
        backend: scripted,
        sources: Default::default(),
        upstream: Default::default(),
        clock,
        server: server.clone(),
        events,
        device_id: format!("dev-{name}"),
        server_id: "srv".into(),
        dir,
    };
    t.wait_until(30_000.0, |t| {
        t.events
            .all()
            .iter()
            .any(|e| matches!(e, Event::SyncProgress { progress } if progress.finished))
    })
    .await;
    t
}

struct FakeLyricsHttp(String);

impl LyricsHttp for FakeLyricsHttp {
    fn get(&self, _url: &str) -> BoxFuture<'_, Result<String, LyricsError>> {
        let body = self.0.clone();
        Box::pin(async move { Ok(body) })
    }
}

fn syllable_entry() -> StructuredLyrics {
    StructuredLyrics {
        lang: "eng".into(),
        synced: true,
        line: vec![
            LyricLineBody {
                start: Some(0),
                value: "Hello world".into(),
            },
            LyricLineBody {
                start: Some(3000),
                value: "Second line".into(),
            },
        ],
        cue_line: vec![CueLine {
            index: 0,
            start: Some(0),
            end: Some(2000),
            value: "Hello world".into(),
            agent_id: None,
            cue: vec![
                LyricCue {
                    start: Some(0),
                    end: Some(1000),
                    value: "Hello ".into(),
                    ..Default::default()
                },
                LyricCue {
                    start: Some(1000),
                    end: Some(2000),
                    value: "world".into(),
                    ..Default::default()
                },
            ],
        }],
        ..Default::default()
    }
}

async fn lyrics_of(t: &TestCore, id: &str) -> Option<Lyrics> {
    let first = t
        .query(Query::Lyrics {
            track_id: id.into(),
        })
        .await;
    if let QueryResult::LyricsResult(Some(l)) = first {
        return Some(l);
    }
    t.core.settle().await;
    t.events.all().into_iter().rev().find_map(|e| match e {
        Event::LyricsChanged { track_id, lyrics } if track_id == id => Some(lyrics),
        _ => None,
    })?
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn lyrics_degrade_honestly_across_tiers_and_external_is_opt_in() {
    let server = seeded_server(4, 180.0);
    {
        let mut st = server.state.lock();
        st.lyrics.insert("t0".into(), vec![syllable_entry()]);
        st.lyrics.insert(
            "t1".into(),
            vec![StructuredLyrics {
                synced: true,
                line: vec![
                    LyricLineBody {
                        start: Some(100),
                        value: "Line one".into(),
                    },
                    LyricLineBody {
                        start: Some(2100),
                        value: "Line two".into(),
                    },
                ],
                ..Default::default()
            }],
        );
        st.lyrics.insert(
            "t2".into(),
            vec![StructuredLyrics {
                synced: false,
                line: vec![LyricLineBody {
                    start: None,
                    value: "Just words".into(),
                }],
                ..Default::default()
            }],
        );
    }
    let external = r#"{"plainLyrics":"From outside","syncedLyrics":"[00:01.00] From outside\n[00:03.00] Synced too"}"#;
    let t = synced_with_http("lyrics", server, Arc::new(FakeLyricsHttp(external.into()))).await;

    let l0 = lyrics_of(&t, "t0").await.expect("syllable lyrics");
    assert_eq!(l0.tier, LyricsTier::Syllable);
    assert_eq!(l0.source, LyricsSource::Server);
    assert_eq!(l0.lines[0].syllables.len(), 2);
    assert!(l0.lines[1].syllables.is_empty(), "never fabricated");
    let l1 = lyrics_of(&t, "t1").await.expect("line lyrics");
    assert_eq!(l1.tier, LyricsTier::Line);
    let l2 = lyrics_of(&t, "t2").await.expect("unsynced lyrics");
    assert_eq!(l2.tier, LyricsTier::Unsynced);
    // Served from the cache the second time: no extra server call.
    let calls = t.server.calls_to("getLyricsBySongId");
    let again = t
        .query(Query::Lyrics {
            track_id: "t0".into(),
        })
        .await;
    assert!(matches!(again, QueryResult::LyricsResult(Some(_))));
    assert_eq!(t.server.calls_to("getLyricsBySongId"), calls);

    // t3 has nothing on the server; external is off by default.
    assert!(lyrics_of(&t, "t3").await.is_none());
    t.run(Command::SetExternalLyricsEnabled { enabled: true })
        .await;
    t.run(Command::FetchLyrics {
        track_id: "t3".into(),
    })
    .await;
    let l3 = lyrics_of(&t, "t3").await.expect("external lyrics");
    assert_eq!(l3.source, LyricsSource::External);
    assert_eq!(l3.tier, LyricsTier::Line);

    // Now-playing drives lyrics without a query: t2 is announced when it
    // starts, and the next item (t3, now external-enabled) is prefetched
    // silently into the cache.
    t.events.clear();
    let calls_before = t.server.calls_to("getLyricsBySongId");
    t.run(Command::PlayTracks {
        tracks: TrackRef::list("srv", ["t2", "t0"]),
        start_index: 0,
        label: "Sel".into(),
        shuffle: false,
        start_position_ms: 0,
    })
    .await;
    t.run_for(500.0).await;
    let announced: Vec<(String, bool)> = t
        .events
        .all()
        .into_iter()
        .filter_map(|e| match e {
            Event::LyricsChanged { track_id, lyrics } => Some((track_id, lyrics.is_some())),
            _ => None,
        })
        .collect();
    assert!(announced.contains(&("t2".into(), true)), "{announced:?}");
    assert!(
        !announced.iter().any(|(id, _)| id == "t0"),
        "prefetch is silent: {announced:?}"
    );
    assert_eq!(
        t.server.calls_to("getLyricsBySongId"),
        calls_before,
        "both were cached already"
    );
    t.run(Command::Next).await;
    t.run_for(500.0).await;
    assert!(t
        .events
        .all()
        .iter()
        .any(|e| matches!(e, Event::LyricsChanged { track_id, lyrics: Some(l) } if track_id == "t0" && l.tier == LyricsTier::Syllable)));

    // A per-track offset rides on top.
    t.run(Command::SetLyricsOffset {
        track_id: "t0".into(),
        offset_ms: 250,
    })
    .await;
    let l0 = lyrics_of(&t, "t0").await.unwrap();
    assert_eq!(l0.offset_ms, 250);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_failed_lyrics_fetch_is_not_retried_on_every_ask() {
    let server = seeded_server(2, 180.0);
    server
        .state
        .lock()
        .lyrics
        .insert("t0".into(), vec![syllable_entry()]);
    let t = synced("lyrics-fail", server).await;
    // The server stops answering (the platform still says online).
    t.server.set_offline(true);
    assert!(lyrics_of(&t, "t0").await.is_none());
    let calls = t.server.calls_to("getLyricsBySongId");
    assert_eq!(calls, 1);
    t.server.set_offline(false);
    for _ in 0..3 {
        assert!(lyrics_of(&t, "t0").await.is_none());
    }
    assert_eq!(
        t.server.calls_to("getLyricsBySongId"),
        calls,
        "the failure is remembered"
    );
    // Asked for explicitly, it is tried again.
    t.run(Command::FetchLyrics {
        track_id: "t0".into(),
    })
    .await;
    t.core.settle().await;
    assert_eq!(t.server.calls_to("getLyricsBySongId"), calls + 1);
    assert!(lyrics_of(&t, "t0").await.is_some());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_forced_lyrics_fetch_asks_a_server_marked_unreachable() {
    let server = seeded_server(2, 180.0);
    server
        .state
        .lock()
        .lyrics
        .insert("t0".into(), vec![syllable_entry()]);
    let t = synced("lyrics-force", server).await;
    // A probe fails: the account is marked unreachable.
    t.core
        .post_internal(hocket_core::core::Internal::Probed {
            account_id: t.server_id.clone(),
            result: Err((ErrorKind::Network, "no route".into())),
        })
        .unwrap();
    t.core.settle().await;
    match t.query(Query::Accounts).await {
        QueryResult::Accounts(a) => assert!(!a[0].reachable),
        other => panic!("{other:?}"),
    }
    // Unasked, it isn't bothered.
    let calls = t.server.calls_to("getLyricsBySongId");
    assert!(lyrics_of(&t, "t0").await.is_none());
    assert_eq!(t.server.calls_to("getLyricsBySongId"), calls);
    // Asked for explicitly, it is.
    t.events.clear();
    t.run(Command::FetchLyrics {
        track_id: "t0".into(),
    })
    .await;
    t.core.settle().await;
    assert_eq!(t.server.calls_to("getLyricsBySongId"), calls + 1);
    assert!(t.events.all().iter().any(
        |e| matches!(e, Event::LyricsChanged { track_id, lyrics: Some(_) } if track_id == "t0")
    ));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_stale_lyrics_answer_leaves_the_newer_fetch_alone() {
    use hocket_core::core::Internal;
    let server = seeded_server(2, 180.0);
    server
        .state
        .lock()
        .lyrics
        .insert("t0".into(), vec![syllable_entry()]);
    let t = synced("lyrics-stale", server).await;
    t.server.set_delay(
        "getLyricsBySongId",
        Some(std::time::Duration::from_millis(300)),
    );
    t.events.clear();
    t.dispatch(Command::FetchLyrics {
        track_id: "t0".into(),
    });
    // What an aborted earlier fetch of the same track had already sent: it
    // must not end (or answer for) the one now running.
    t.core
        .post_internal(Internal::LyricsFetched {
            server_id: t.server_id.clone(),
            track_id: "t0".into(),
            lyrics: None,
            announce: true,
            failed: true,
            generation: 0,
        })
        .unwrap();
    t.core.settle().await;
    let announced: Vec<bool> = t
        .events
        .all()
        .into_iter()
        .filter_map(|e| match e {
            Event::LyricsChanged { track_id, lyrics } if track_id == "t0" => Some(lyrics.is_some()),
            _ => None,
        })
        .collect();
    assert_eq!(announced, vec![true], "only the running fetch answers");
    // Nor is its failure remembered: asked again, it is answered.
    assert!(lyrics_of(&t, "t0").await.is_some());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn settings_persist_export_and_import() {
    let t = synced("settings", seeded_server(2, 180.0)).await;
    t.run(Command::SetSetting {
        key: "queue.savedCap".into(),
        value: "3".into(),
    })
    .await;
    t.run(Command::SetSetting {
        key: "nope.unknown".into(),
        value: "1".into(),
    })
    .await;
    assert!(t
        .events
        .all()
        .iter()
        .any(|e| matches!(e, Event::Toast { toast } if toast.message.contains("nope.unknown"))));
    match t
        .query(Query::Setting {
            key: "queue.savedCap".into(),
        })
        .await
    {
        QueryResult::SettingDetail(Some(s)) => {
            assert_eq!(s.value, "3");
            assert_eq!(s.scope, SettingScope::AccountSynced);
        }
        other => panic!("{other:?}"),
    }
    t.run(Command::ExportConfig {
        include_secrets: false,
    })
    .await;
    let doc = t
        .events
        .all()
        .into_iter()
        .find_map(|e| match e {
            Event::ConfigExported { document } => Some(document),
            _ => None,
        })
        .unwrap();
    assert!(doc.contains("queue.savedCap"));
    t.run(Command::ResetSetting {
        key: "queue.savedCap".into(),
    })
    .await;
    match t
        .query(Query::Setting {
            key: "queue.savedCap".into(),
        })
        .await
    {
        QueryResult::SettingDetail(Some(s)) => assert_eq!(s.value, "10"),
        other => panic!("{other:?}"),
    }
    t.run(Command::ImportConfig {
        document: doc,
        include_device_local: false,
    })
    .await;
    match t
        .query(Query::Setting {
            key: "queue.savedCap".into(),
        })
        .await
    {
        QueryResult::SettingDetail(Some(s)) => assert_eq!(s.value, "3"),
        other => panic!("{other:?}"),
    }
    // Actions: aliases the desktop uses resolve, shortcuts persist.
    match t
        .query(Query::Actions {
            surface: "sidebar".into(),
            target: ActionTarget::None,
        })
        .await
    {
        QueryResult::Actions(a) => assert!(a.iter().any(|a| a.id == "navigateLibrary")),
        other => panic!("{other:?}"),
    }
    t.run(Command::SetShortcut {
        action_id: "transport.volumeUp".into(),
        shortcut: Some("Ctrl+Shift+ArrowUp".into()),
    })
    .await;
    match t.query(Query::Shortcuts).await {
        QueryResult::Shortcuts(s) => {
            assert!(s.iter().any(|s| s.action_id == "volumeUp"
                && s.shortcut.as_deref() == Some("Ctrl+Shift+ArrowUp")))
        }
        other => panic!("{other:?}"),
    }
    t.run(Command::RunAction {
        action_id: "rate.3".into(),
        target: ActionTarget::Tracks {
            tracks: TrackRef::list(&t.server_id, ["t0"]),
        },
    })
    .await;
    t.run_for(500.0).await;
    assert_eq!(t.server.rating_of("t0"), 3);
    let diag = t.query(Query::Diagnostics).await;
    assert!(matches!(diag, QueryResult::Text(s) if s.contains("Hocket core")));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pins_download_through_the_job_queue() {
    let server = seeded_server(3, 180.0);
    for i in 0..3 {
        server.set_media(&format!("t{i}"), vec![0u8; 4096]);
    }
    let t = synced("pins", server).await;
    t.run(Command::Pin {
        target: PinTarget::Album { id: "al0".into() },
        transcode: false,
    })
    .await;
    t.wait_until(20_000.0, |t| {
        t.events.all().iter().any(|e| {
            matches!(e, Event::PinsChanged { pins } if pins.iter().any(|p| p.downloaded_count == 3))
        })
    })
    .await;
    match t
        .query(Query::Track {
            track: TrackRef::new(t.server_id.clone(), "t0"),
        })
        .await
    {
        QueryResult::TrackDetail(Some(tr)) => assert_eq!(tr.offline, OfflineState::Downloaded),
        other => panic!("{other:?}"),
    }
    match t
        .query(Query::MediaSource {
            track_id: "t0".into(),
        })
        .await
    {
        QueryResult::Source(Some(s)) => assert!(s.url.starts_with("file://"), "{}", s.url),
        other => panic!("{other:?}"),
    }
    match t.query(Query::Storage).await {
        QueryResult::Storage(s) => assert!(s.downloads_bytes >= 3.0 * 4096.0),
        other => panic!("{other:?}"),
    }
    // Artwork resolves to a plain local path (no scheme), the same
    // convention as the media session's artwork_path.
    let art = match t
        .query(Query::Artwork {
            id: "al-al0".into(),
            size: 640,
            display: false,
        })
        .await
    {
        QueryResult::Path(Some(p)) => p,
        other => panic!("{other:?}"),
    };
    assert!(!art.starts_with("file://"), "{art}");
    assert!(std::path::Path::new(&art).is_file(), "{art}");
    t.run(Command::PlayTracks {
        tracks: TrackRef::list(&t.server_id, ["t0"]),
        start_index: 0,
        label: "One".into(),
        shuffle: false,
        start_position_ms: 0,
    })
    .await;
    t.run_for(500.0).await;
    let ms = t.snapshot().await.media_session;
    assert_eq!(
        ms.metadata.unwrap().artwork_path.as_deref(),
        Some(art.as_str())
    );
    t.run(Command::Unpin {
        target: PinTarget::Album { id: "al0".into() },
    })
    .await;
    match t.query(Query::Pins).await {
        QueryResult::Pins(p) => assert!(p.is_empty()),
        other => panic!("{other:?}"),
    }
}

/// A download keeps its covers, so offline the album still has art: asked at
/// a kept size or any other, answered from disk at once, the server never
/// asked.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn downloads_keep_their_covers_for_offline() {
    let server = seeded_server(2, 180.0);
    for i in 0..2 {
        server.set_media(&format!("t{i}"), vec![0u8; 4096]);
    }
    server.set_media("al-al0", vec![7u8; 64]);
    let t = synced("pin-covers", server).await;
    t.run(Command::Pin {
        target: PinTarget::Album { id: "al0".into() },
        transcode: false,
    })
    .await;
    t.wait_until(20_000.0, |t| {
        t.events.all().iter().any(|e| {
            matches!(e, Event::JobsChanged { jobs } if jobs.iter().any(|j| j.kind == JobKind::Download && j.state == JobState::Done))
        })
    })
    .await;
    // The covers follow the job, off its slot: 64, 320 and 1280 on Linux.
    for _ in 0..500 {
        if t.server.calls_to("getCoverArt") >= 3 {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    assert_eq!(
        t.server.calls_to("getCoverArt"),
        3,
        "{:?}",
        t.server.calls()
    );
    // Online, a display ask takes a kept copy up to 4× its size instead of
    // a fetch; an ask that reads the pixels gets its exact size.
    for (size, kept) in [(160, 320), (640, 1280), (64, 64)] {
        match t
            .query(Query::Artwork {
                id: "al-al0".into(),
                size,
                display: true,
            })
            .await
        {
            QueryResult::Path(Some(p)) => {
                assert!(p.ends_with(&format!("al-al0_{kept}.img")), "{size}: {p}")
            }
            other => panic!("{size}: {other:?}"),
        }
    }
    assert_eq!(t.server.calls_to("getCoverArt"), 3);
    match t
        .query(Query::Artwork {
            id: "al-al0".into(),
            size: 640,
            display: false,
        })
        .await
    {
        QueryResult::Path(Some(p)) => assert!(p.ends_with("al-al0_640.img"), "{p}"),
        other => panic!("{other:?}"),
    }
    assert_eq!(t.server.calls_to("getCoverArt"), 4);
    t.server.set_offline(true);
    t.run(Command::SetNetworkState {
        state: NetworkState {
            kind: NetworkKind::Offline,
            metered: false,
            network_id: None,
        },
    })
    .await;
    let calls = t.server.calls_to("getCoverArt") + t.server.calls_to("download");
    // Linux keeps 64, 320 and 1280: 320 exactly, 160 from the next size up.
    for size in [320, 160] {
        match t
            .query(Query::Artwork {
                id: "al-al0".into(),
                size,
                display: false,
            })
            .await
        {
            QueryResult::Path(Some(p)) => assert!(p.ends_with("al-al0_320.img"), "{size}: {p}"),
            other => panic!("{size}: {other:?}"),
        }
    }
    match t
        .query(Query::Artwork {
            id: "al-al0".into(),
            size: 1280,
            display: false,
        })
        .await
    {
        QueryResult::Path(Some(p)) => assert!(p.ends_with("al-al0_1280.img"), "{p}"),
        other => panic!("{other:?}"),
    }
    assert_eq!(
        t.server.calls_to("getCoverArt") + t.server.calls_to("download"),
        calls,
        "offline never asks the server"
    );
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

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn playlist_append_undo_removes_and_redo_re_adds_without_new_history() {
    let server = seeded_server(6, 180.0);
    server.add_playlist("pl", "Mine", "alice", &["t0", "t1"], false);
    let t = synced("pl-undo", server).await;

    // Append (no index): the common "Add to playlist" path.
    t.run(Command::PlaylistAdd {
        playlist_id: "pl".into(),
        tracks: TrackRef::list(&t.server_id, ["t2", "t3"]),
        at_index: None,
    })
    .await;
    t.run_for(500.0).await;
    assert_eq!(t.server.playlist_song_ids("pl"), ["t0", "t1", "t2", "t3"]);
    let undo = t.snapshot().await.undo;
    assert!(undo.can_undo && !undo.can_redo);
    assert_eq!(undo.history.len(), 1);

    // Undo really removes the appended tracks (mirror and server).
    t.run(Command::Undo).await;
    t.run_for(500.0).await;
    assert_eq!(mirror_playlist(&t, "pl").await, ["t0", "t1"]);
    assert_eq!(t.server.playlist_song_ids("pl"), ["t0", "t1"]);
    let undo = t.snapshot().await.undo;
    assert!(
        !undo.can_undo && undo.can_redo,
        "undo is a step back, not a new action: {undo:?}"
    );
    assert!(t.events.all().iter().any(
        |e| matches!(e, Event::Toast { toast } if toast.message == "Undid Add to playlist (2)")
    ));

    // Redo puts them back; the redo stack was not wiped by the undo.
    t.run(Command::Redo).await;
    t.run_for(500.0).await;
    assert_eq!(t.server.playlist_song_ids("pl"), ["t0", "t1", "t2", "t3"]);
    let undo = t.snapshot().await.undo;
    assert!(undo.can_undo && !undo.can_redo);
    assert_eq!(undo.history.len(), 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn playlist_remove_and_move_undo_are_compare_and_swap() {
    let server = seeded_server(6, 180.0);
    server.add_playlist("pl", "Mine", "alice", &["t0", "t1", "t2", "t3"], false);
    let t = synced("pl-cas", server).await;

    // Move t0 to the end, undo restores it.
    t.run(Command::PlaylistMove {
        playlist_id: "pl".into(),
        from_index: 0,
        to_index: 3,
    })
    .await;
    t.run_for(500.0).await;
    assert_eq!(t.server.playlist_song_ids("pl"), ["t1", "t2", "t3", "t0"]);
    t.run(Command::Undo).await;
    t.run_for(500.0).await;
    assert_eq!(t.server.playlist_song_ids("pl"), ["t0", "t1", "t2", "t3"]);
    assert!(!t.snapshot().await.undo.can_undo);

    // Remove t2, then someone else deletes t2's neighbour on the server and
    // the mirror re-syncs: the undo (re-add at index 2) still applies since
    // t2 is still absent.
    t.run(Command::PlaylistRemove {
        playlist_id: "pl".into(),
        indices: vec![2],
    })
    .await;
    t.run_for(500.0).await;
    assert_eq!(t.server.playlist_song_ids("pl"), ["t0", "t1", "t3"]);
    t.run(Command::Undo).await;
    t.run_for(500.0).await;
    assert_eq!(t.server.playlist_song_ids("pl"), ["t0", "t1", "t2", "t3"]);

    // Add t4, then t4 is removed elsewhere and the mirror re-syncs: the undo
    // expects t4 present, finds it gone, reports honestly and leaves it.
    t.run(Command::PlaylistAdd {
        playlist_id: "pl".into(),
        tracks: TrackRef::list(&t.server_id, ["t4"]),
        at_index: None,
    })
    .await;
    t.run_for(500.0).await;
    assert_eq!(
        t.server.playlist_song_ids("pl"),
        ["t0", "t1", "t2", "t3", "t4"]
    );
    t.server.state.lock().playlist_songs.insert(
        "pl".into(),
        vec!["t0".into(), "t1".into(), "t2".into(), "t3".into()],
    );
    t.run(Command::SyncLibrary {
        account_id: t.server_id.clone(),
        full: true,
    })
    .await;
    t.wait_until(30_000.0, |t| {
        t.events
            .all()
            .iter()
            .filter(|e| matches!(e, Event::SyncProgress { progress } if progress.finished))
            .count()
            >= 2
    })
    .await;
    assert_eq!(mirror_playlist(&t, "pl").await, ["t0", "t1", "t2", "t3"]);
    t.events.clear();
    t.run(Command::Undo).await;
    t.run_for(500.0).await;
    assert_eq!(t.server.playlist_song_ids("pl"), ["t0", "t1", "t2", "t3"]);
    assert!(t.events.all().iter().any(|e| matches!(
        e,
        Event::Toast { toast } if toast.message == "Undid Add to playlist (1): undid 0 of 1, 1 changed elsewhere"
    )), "{:?}", t.events.all().iter().filter(|e| matches!(e, Event::Toast{..})).collect::<Vec<_>>());
}

async fn setting_of(t: &TestCore, key: &str) -> Setting {
    match t.query(Query::Setting { key: key.into() }).await {
        QueryResult::SettingDetail(Some(s)) => s,
        other => panic!("{key}: {other:?}"),
    }
}

/// `ImportConfig` on two paired cores (LAN): account-synced keys the
/// document changes are broadcast like a local edit (the peer sees them
/// instead of reverting the import on its next merge); a far-future
/// `updatedAt` is clamped to now so it cannot pin the value against later
/// edits; device-local keys stay untouched unless `include_device_local` is
/// set, and even then never travel.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn import_config_broadcasts_synced_keys_and_keeps_device_local_unless_asked() {
    use hocket_core::core::io::memory::MemoryNet;
    let server = seeded_server(2, 180.0);
    let net = MemoryNet::new();
    let clock = SimTime::new(1_700_000_000_000.0);
    let a = TestCore::start_on("a", server.clone(), Some(net.clone()), 1, clock.clone()).await;
    let b = TestCore::start_on("b", server.clone(), Some(net.clone()), 2, clock.clone()).await;
    TestCore::run_all_for(&[&a, &b], 8_000.0).await;
    assert_eq!(a.snapshot().await.devices.len(), 2, "a and b paired");

    // The document comes from a third, unconnected device: a synced key and
    // a device-local key, with the synced key stamped far in the future.
    let c = TestCore::start_on("c", server, None, 3, clock.clone()).await;
    c.run(Command::SetSetting {
        key: "queue.savedCap".into(),
        value: "5".into(),
    })
    .await;
    c.run(Command::SetSetting {
        key: "display.theme".into(),
        value: "\"dark\"".into(),
    })
    .await;
    c.run(Command::ExportConfig {
        include_secrets: false,
    })
    .await;
    let exported = c
        .events
        .all()
        .into_iter()
        .find_map(|e| match e {
            Event::ConfigExported { document } => Some(document),
            _ => None,
        })
        .unwrap();
    let mut parsed: ConfigDocument = serde_json::from_str(&exported).unwrap();
    let far_future = clock.now_ms() + 10.0 * 365.0 * 86_400_000.0;
    parsed
        .settings
        .iter_mut()
        .find(|s| s.key == "queue.savedCap")
        .expect("exported")
        .updated_at = far_future;
    let doc = serde_json::to_string(&parsed).unwrap();

    a.run(Command::ImportConfig {
        document: doc.clone(),
        include_device_local: false,
    })
    .await;
    let cap = setting_of(&a, "queue.savedCap").await;
    assert_eq!(cap.value, "5");
    assert!(
        cap.updated_at <= clock.now_ms(),
        "future updatedAt clamped to now: {} > {}",
        cap.updated_at,
        clock.now_ms()
    );
    assert_eq!(
        setting_of(&a, "display.theme").await.value,
        "\"system\"",
        "device-local keys are left alone by default"
    );
    assert!(
        a.events.all().iter().any(|e| matches!(
            e,
            Event::Toast { toast } if toast.message.contains("device setting")
        )),
        "the import says what it left alone: {:?}",
        a.events.all()
    );
    // The synced key went out through the engine: b learns it.
    TestCore::run_all_for(&[&a, &b], 2_000.0).await;
    assert_eq!(
        setting_of(&b, "queue.savedCap").await.value,
        "5",
        "b got the import"
    );

    // Because the stamp was clamped, a later normal edit on b still wins.
    b.run(Command::SetSetting {
        key: "queue.savedCap".into(),
        value: "7".into(),
    })
    .await;
    TestCore::run_all_for(&[&a, &b], 2_000.0).await;
    assert_eq!(
        setting_of(&a, "queue.savedCap").await.value,
        "7",
        "an imported far-future stamp must not pin the value"
    );

    // Opting in takes the device-local key too, but it never travels.
    a.run(Command::ImportConfig {
        document: doc,
        include_device_local: true,
    })
    .await;
    assert_eq!(setting_of(&a, "display.theme").await.value, "\"dark\"");
    TestCore::run_all_for(&[&a, &b], 2_000.0).await;
    assert_eq!(
        setting_of(&b, "display.theme").await.value,
        "\"system\"",
        "device-local keys never travel"
    );
    assert_eq!(
        setting_of(&b, "queue.savedCap").await.value,
        "5",
        "the re-import (stamped now) is newer than b's edit and reaches b"
    );
}

fn track_ids(r: QueryResult) -> Vec<String> {
    match r {
        QueryResult::TrackList(list) => list.into_iter().map(|t| t.id).collect(),
        other => panic!("{other:?}"),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn artist_top_songs_are_cached_and_refreshed_in_the_background() {
    let server = seeded_server(12, 180.0);
    let t = synced("top-songs", server.clone()).await;
    let top = |count: u32| Query::ArtistTopSongs {
        id: "ar0".into(),
        count,
    };
    let calls = server.calls_to("getTopSongs");

    // A failed first fetch answers from the mirror and caches nothing.
    server.fail_next(
        hocket_core::subsonic::SubsonicError::Network("down".into()),
        1,
    );
    assert_eq!(track_ids(t.query(top(10)).await).len(), 10);
    let calls = calls + 1;
    assert_eq!(server.calls_to("getTopSongs"), calls);

    // First visit asks the server and caches the answer.
    let first = track_ids(t.query(top(10)).await);
    assert_eq!(first.len(), 10);
    assert_eq!(server.calls_to("getTopSongs"), calls + 1);

    // Later visits (and smaller lists) are answered from the cache.
    assert_eq!(track_ids(t.query(top(10)).await), first);
    assert_eq!(track_ids(t.query(top(5)).await), first[..5]);
    t.core.settle().await;
    assert_eq!(server.calls_to("getTopSongs"), calls + 1);

    // Offline, the cached list still answers (not the most-played fallback).
    server.set_offline(true);
    assert_eq!(track_ids(t.query(top(10)).await), first);
    server.set_offline(false);

    // The server's list changes; once the cache is stale the old list is
    // still served at once, refreshed behind it, and UIs are told to refetch.
    server.add_song(FakeServer::song("a-new", "New hit", "al9", "ar0", 100.0));
    t.clock
        .advance(hocket_core::cache::TOP_SONGS_REFRESH_MS + 1.0);
    t.events.clear();
    assert_eq!(track_ids(t.query(top(10)).await), first);
    // A second ask while the refresh runs does not start another. The
    // refresh may already have landed, so either list is a valid answer.
    let second = track_ids(t.query(top(10)).await);
    assert!(second == first || second[0] == "a-new", "{second:?}");
    t.core.settle().await;
    assert_eq!(server.calls_to("getTopSongs"), calls + 2);
    assert!(t.events.all().iter().any(|e| matches!(
        e,
        Event::LibraryChanged { tables, ids, .. }
            if tables == &["artistTopSongs".to_string()] && ids == &["ar0".to_string()]
    )));
    let refreshed = track_ids(t.query(top(10)).await);
    assert_eq!(refreshed[0], "a-new");
    t.core.settle().await;
    assert_eq!(server.calls_to("getTopSongs"), calls + 2);

    // A refresh that finds the same list says nothing.
    t.clock
        .advance(hocket_core::cache::TOP_SONGS_REFRESH_MS + 1.0);
    t.events.clear();
    t.query(top(10)).await;
    t.core.settle().await;
    assert_eq!(server.calls_to("getTopSongs"), calls + 3);
    assert!(!t
        .events
        .all()
        .iter()
        .any(|e| matches!(e, Event::LibraryChanged { .. })));
}

fn external(r: QueryResult) -> Option<ExternalInfo> {
    match r {
        QueryResult::ExternalInfo(info) => info,
        other => panic!("{other:?}"),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn artist_and_album_info_come_from_the_server_and_are_cached() {
    let server = seeded_server(12, 180.0);
    let t = synced("external-info", server.clone()).await;

    let artist = external(t.query(Query::ArtistInfo { id: "ar0".into() }).await).unwrap();
    assert!(artist
        .text
        .as_deref()
        .is_some_and(|s| s.starts_with("About ")));
    assert!(artist.similar_artists.iter().all(|a| a.id != "ar0"));
    let album = external(t.query(Query::AlbumInfo { id: "al0".into() }).await).unwrap();
    assert!(album
        .text
        .as_deref()
        .is_some_and(|s| s.starts_with("About ")));
    assert_eq!(server.calls_to("getArtistInfo2"), 1);
    assert_eq!(server.calls_to("getAlbumInfo2"), 1);

    // Answered from the cache, offline too.
    server.set_offline(true);
    assert_eq!(
        external(t.query(Query::ArtistInfo { id: "ar0".into() }).await),
        Some(artist.clone())
    );
    assert_eq!(server.calls_to("getArtistInfo2"), 1);
    // Past its week, offline: the older answer still stands in.
    t.clock.advance(8.0 * 86_400_000.0);
    assert_eq!(
        external(t.query(Query::ArtistInfo { id: "ar0".into() }).await),
        Some(artist.clone())
    );
    assert_eq!(
        server.calls_to("getArtistInfo2"),
        2,
        "a stale entry asks the server again"
    );
    // Something never fetched, offline: nothing, not an error.
    assert_eq!(
        external(t.query(Query::AlbumInfo { id: "al1".into() }).await),
        None
    );
}

/// Deezer by URL: the answer for the first key the URL contains, else 404. Records each URL.
struct FakeDeezer {
    answers: Vec<(String, String)>,
    asked: Arc<parking_lot::Mutex<Vec<String>>>,
}

impl LyricsHttp for FakeDeezer {
    fn get(&self, url: &str) -> BoxFuture<'_, Result<String, LyricsError>> {
        self.asked.lock().push(url.to_string());
        let hit = self
            .answers
            .iter()
            .find(|(k, _)| url.contains(k.as_str()))
            .map(|(_, v)| v.clone());
        Box::pin(async move {
            hit.ok_or(LyricsError::Http {
                status: 404,
                body: String::new(),
            })
        })
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn song_link_finds_the_song_on_deezer_by_isrc_then_by_search() {
    let server = seeded_server(4, 180.0);
    server.state.lock().songs.get_mut("t0").unwrap().isrc = vec!["GBAYE9200070".into()];
    let asked = Arc::new(parking_lot::Mutex::new(Vec::new()));
    let http = Arc::new(FakeDeezer {
        answers: vec![
            ("track/isrc:GBAYE9200070".into(), r#"{"id":111}"#.into()),
            (
                "/search?".into(),
                r#"{"data":[{"id":222,"title":"Track 1","duration":180,"artist":{"name":"Artist ar0"}}]}"#.into(),
            ),
            (
                "/search/album?".into(),
                r#"{"data":[{"id":333,"title":"Album al0","artist":{"name":"Artist ar0"}}]}"#.into(),
            ),
        ],
        asked: asked.clone(),
    });
    let t = synced_with_http("songlink", server, http).await;
    let link = |track: Option<&str>, album: Option<&str>| {
        let q = Query::SongLink {
            track_id: track.map(Into::into),
            album_id: album.map(Into::into),
        };
        let t = &t;
        async move {
            match t.query(q).await {
                QueryResult::SongLink(url) => url,
                other => panic!("unexpected {other:?}"),
            }
        }
    };

    // The mirror keeps no ISRC: the core asks the server for the song, then Deezer by ISRC.
    let before = t.server.calls_to("getSong");
    assert_eq!(
        link(Some("t0"), None).await.as_deref(),
        Some("https://squigly.link/?link=https%3A%2F%2Fwww.deezer.com%2Ftrack%2F111")
    );
    assert_eq!(t.server.calls_to("getSong"), before + 1);
    // No ISRC: search, and keep the result with this artist and title.
    assert_eq!(
        link(Some("t1"), None).await.as_deref(),
        Some("https://squigly.link/?link=https%3A%2F%2Fwww.deezer.com%2Ftrack%2F222")
    );
    // Track 2 is not in the search answer.
    assert_eq!(link(Some("t2"), None).await, None);
    assert_eq!(
        link(None, Some("al0")).await.as_deref(),
        Some("https://squigly.link/?link=https%3A%2F%2Fwww.deezer.com%2Falbum%2F333")
    );
    assert_eq!(link(Some("nope"), None).await, None);
    assert_eq!(link(None, None).await, None);
    assert!(asked
        .lock()
        .iter()
        .all(|u| u.starts_with("https://api.deezer.com/")));
}

/// Set up another device without a coordinator: `Query::SetupSettings` gives
/// the account-synced settings changed from their defaults (device-local ones
/// stay behind), and `Command::ApplySetupSettings` on the new device takes
/// them, skipping device-local, unknown and invalid keys.
#[tokio::test]
async fn setup_settings_carry_changed_synced_settings_to_a_new_device() {
    let a = TestCore::start("setup-a", seeded_server(2, 180.0)).await;
    a.run(Command::SetSetting {
        key: "queue.savedCap".into(),
        value: "5".into(),
    })
    .await;
    a.run(Command::SetSetting {
        key: "display.theme".into(),
        value: "\"dark\"".into(),
    })
    .await;
    // Set to its default: nothing to carry.
    a.run(Command::SetSetting {
        key: "queue.section".into(),
        value: "true".into(),
    })
    .await;
    let text = match a.query(Query::SetupSettings).await {
        QueryResult::Text(t) => t,
        other => panic!("{other:?}"),
    };
    let carried: serde_json::Map<String, serde_json::Value> = serde_json::from_str(&text).unwrap();
    assert_eq!(
        carried.get("queue.savedCap"),
        Some(&serde_json::json!(5)),
        "{text}"
    );
    assert!(
        !carried.contains_key("display.theme"),
        "device-local stays: {text}"
    );
    assert!(
        !carried.contains_key("queue.section"),
        "defaults stay: {text}"
    );

    let b = TestCore::start("setup-b", seeded_server(2, 180.0)).await;
    let mut incoming = carried.clone();
    incoming.insert("display.theme".into(), serde_json::json!("dark"));
    incoming.insert("no.such.key".into(), serde_json::json!(1));
    incoming.insert("queue.section".into(), serde_json::json!("not a bool"));
    b.run(Command::ApplySetupSettings {
        settings: serde_json::Value::Object(incoming).to_string(),
    })
    .await;
    assert_eq!(setting_of(&b, "queue.savedCap").await.value, "5");
    assert_eq!(
        setting_of(&b, "display.theme").await.value,
        "\"system\"",
        "device-local ignored"
    );
    assert_eq!(
        setting_of(&b, "queue.section").await.value,
        "true",
        "invalid value ignored"
    );
    assert!(b.events.all().iter().any(|e| matches!(e, Event::SettingChanged { setting } if setting.key == "queue.savedCap" && setting.value == "5")));
    // Not a JSON object: nothing happens.
    b.run(Command::ApplySetupSettings {
        settings: "[1]".into(),
    })
    .await;
    assert_eq!(setting_of(&b, "queue.savedCap").await.value, "5");
}

/// Every batch a search with `request_id` produced, local first.
async fn search_batches(
    t: &TestCore,
    query: &str,
    limit: u32,
    include_server: bool,
    request_id: &str,
) -> Vec<SearchResults> {
    t.query(Query::Search {
        server_id: t.server_id.clone(),
        query: query.into(),
        limit,
        include_server,
        request_id: request_id.into(),
    })
    .await;
    t.core.settle().await;
    t.events
        .all()
        .into_iter()
        .filter_map(|e| match e {
            Event::SearchResults { results } if results.request_id == request_id => Some(results),
            _ => None,
        })
        .collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn search_asking_the_server_always_gets_one_server_batch() {
    let t = synced("search-once", seeded_server(4, 180.0)).await;
    let only_server = |b: &[SearchResults]| -> Vec<SearchResults> {
        b.iter().filter(|b| b.from_server).cloned().collect()
    };

    // Not asked: no server batch.
    let b = search_batches(&t, "Track", 20, false, "no-server").await;
    assert_eq!(b.len(), 1);
    assert!(!b[0].from_server);

    // Local results already fill the page: an empty server batch.
    let b = search_batches(&t, "Track", 2, true, "full").await;
    let server = only_server(&b);
    assert_eq!(server.len(), 1, "{b:?}");
    assert!(server[0].tracks.is_empty() && server[0].albums.is_empty());
    assert_eq!(server[0].query, "Track");

    // Every provider failed: an empty server batch.
    t.server.set_offline(true);
    let b = search_batches(&t, "Track", 20, true, "failed").await;
    let server = only_server(&b);
    assert_eq!(server.len(), 1, "{b:?}");
    assert!(server[0].tracks.is_empty());
    t.server.set_offline(false);

    // Offline: an empty server batch.
    t.run(Command::SetNetworkState {
        state: NetworkState {
            kind: NetworkKind::Offline,
            metered: false,
            network_id: None,
        },
    })
    .await;
    let b = search_batches(&t, "Track", 20, true, "offline").await;
    assert_eq!(only_server(&b).len(), 1, "{b:?}");
    t.run(Command::SetNetworkState {
        state: NetworkState {
            kind: NetworkKind::Wifi,
            metered: false,
            network_id: None,
        },
    })
    .await;

    // Server search turned off: an empty server batch.
    t.run(Command::SetSetting {
        key: "search.includeServer".into(),
        value: "false".into(),
    })
    .await;
    let b = search_batches(&t, "Track", 20, true, "off").await;
    let server = only_server(&b);
    assert_eq!(server.len(), 1, "{b:?}");
    assert!(server[0].tracks.is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_newer_search_replaces_the_one_still_asking_the_server() {
    let api = seeded_server(4, 180.0);
    let t = synced("search-typing", api.clone()).await;
    // Typed quickly: the second query arrives before the first is answered
    // (the server takes its time, so the first is still asking).
    api.set_delay("search3", Some(std::time::Duration::from_millis(300)));
    let asked = api.calls_to("search3");
    for (q, id) in [("Tra", "k1"), ("Track", "k2")] {
        t.query(Query::Search {
            server_id: t.server_id.clone(),
            query: q.into(),
            limit: 20,
            include_server: true,
            request_id: id.into(),
        })
        .await;
    }
    t.core.settle().await;
    assert_eq!(api.calls_to("search3"), asked + 2, "both asked the server");
    let server: Vec<SearchResults> = t
        .events
        .all()
        .into_iter()
        .filter_map(|e| match e {
            Event::SearchResults { results } if results.from_server => Some(results),
            _ => None,
        })
        .collect();
    for id in ["k1", "k2"] {
        let mine: Vec<_> = server.iter().filter(|b| b.request_id == id).collect();
        assert_eq!(
            mine.len(),
            1,
            "exactly one server batch for {id}: {server:?}"
        );
    }
    let k1 = server.iter().find(|b| b.request_id == "k1").unwrap();
    assert_eq!(k1.query, "Tra");
    assert!(k1.tracks.is_empty(), "replaced, answered empty: {k1:?}");
    let k2 = server.iter().find(|b| b.request_id == "k2").unwrap();
    assert_eq!(k2.query, "Track");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_newer_search_not_asking_the_server_stops_the_one_still_asking() {
    let api = seeded_server(4, 180.0);
    let t = synced("search-reuse-local", api.clone()).await;
    api.set_delay("search3", Some(std::time::Duration::from_millis(300)));
    // The same request id; the second search doesn't ask the server (the
    // text was cleared), so it answers its server batch at once, empty.
    for q in ["Tra", ""] {
        t.query(Query::Search {
            server_id: t.server_id.clone(),
            query: q.into(),
            limit: 20,
            include_server: true,
            request_id: "same".into(),
        })
        .await;
    }
    // Long enough for the first search's server answer, had it kept on.
    tokio::time::sleep(std::time::Duration::from_millis(600)).await;
    t.core.settle().await;
    let server: Vec<SearchResults> = t
        .events
        .all()
        .into_iter()
        .filter_map(|e| match e {
            Event::SearchResults { results } if results.from_server => Some(results),
            _ => None,
        })
        .collect();
    assert_eq!(server.len(), 1, "one server batch for the id: {server:?}");
    assert_eq!(server[0].query, "", "the newer search's");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_newer_search_reusing_the_request_id_gets_one_server_batch() {
    let api = seeded_server(4, 180.0);
    let t = synced("search-reuse", api.clone()).await;
    api.set_delay("search3", Some(std::time::Duration::from_millis(300)));
    // One request id for both (a caller that keeps it while the source or
    // the text changes): the first is still asking when the second arrives.
    for q in ["Tra", "Track"] {
        t.query(Query::Search {
            server_id: t.server_id.clone(),
            query: q.into(),
            limit: 20,
            include_server: true,
            request_id: "same".into(),
        })
        .await;
    }
    t.core.settle().await;
    let server: Vec<SearchResults> = t
        .events
        .all()
        .into_iter()
        .filter_map(|e| match e {
            Event::SearchResults { results } if results.from_server => Some(results),
            _ => None,
        })
        .collect();
    assert_eq!(server.len(), 1, "one server batch for the id: {server:?}");
    assert_eq!(server[0].query, "Track", "the newer search's");
}

fn net(kind: NetworkKind, metered: bool) -> Command {
    Command::SetNetworkState {
        state: NetworkState {
            kind,
            metered,
            network_id: Some("home".into()),
        },
    }
}

fn downloaded(t: &TestCore) -> bool {
    t.events.all().iter().any(|e| {
        matches!(e, Event::PinsChanged { pins } if pins.iter().any(|p| p.downloaded_count == 1))
    })
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn failed_downloads_are_retried_on_a_network_handover() {
    let t = synced("dl-retry", seeded_server(2, 180.0)).await;
    t.run(net(NetworkKind::Wifi, false)).await;
    // The server stops answering while the track downloads (an outage its
    // probe never notices: the account stays reachable): it fails.
    t.server.set_offline(true);
    t.run(Command::Pin {
        target: PinTarget::Track { id: "t0".into() },
        transcode: false,
    })
    .await;
    t.run_for(1_000.0).await;
    let tries = t.server.calls_to("download");
    assert!(tries >= 1);
    assert!(!downloaded(&t));
    t.server.set_offline(false);
    t.events.clear();
    // Wi-Fi to cellular, with no offline in between: fetched again.
    t.run(net(NetworkKind::Cellular, false)).await;
    t.wait_until(10_000.0, |t| downloaded(t)).await;
    assert_eq!(t.server.calls_to("download"), tries + 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_refused_download_waits_for_the_users_retry() {
    let t = synced("dl-refused", seeded_server(2, 180.0)).await;
    t.run(net(NetworkKind::Wifi, false)).await;
    // The server says the file isn't there: no network change fixes that.
    t.server
        .fail_next(SubsonicError::NotFound("http 404".into()), 1);
    t.run(Command::Pin {
        target: PinTarget::Track { id: "t0".into() },
        transcode: false,
    })
    .await;
    t.run_for(1_000.0).await;
    let tries = t.server.calls_to("download");
    assert_eq!(tries, 1);
    t.run(net(NetworkKind::Cellular, false)).await;
    t.run(net(NetworkKind::Wifi, false)).await;
    t.run_for(1_000.0).await;
    assert_eq!(t.server.calls_to("download"), tries, "not retried unasked");
    // The user's retry fetches it, and clears what made the core skip it.
    t.events.clear();
    t.run(Command::Pin {
        target: PinTarget::Track { id: "t0".into() },
        transcode: false,
    })
    .await;
    t.wait_until(10_000.0, |t| downloaded(t)).await;
    assert_eq!(t.server.calls_to("download"), tries + 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn retrying_a_failed_download_job_clears_its_failure_counts() {
    let t = synced("dl-retry-job", seeded_server(2, 180.0)).await;
    t.run(net(NetworkKind::Wifi, false)).await;
    t.server
        .fail_next(SubsonicError::NotFound("http 404".into()), 1);
    t.run(Command::Pin {
        target: PinTarget::Track { id: "t0".into() },
        transcode: false,
    })
    .await;
    t.run_for(1_000.0).await;
    let failures = |t: &TestCore| -> (i64, bool) {
        t.open_db()
            .with_conn(|c| {
                Ok(c.query_row(
                    "SELECT failures, permanent FROM pin_tracks WHERE track_id = 't0'",
                    [],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )?)
            })
            .unwrap()
    };
    assert_eq!(failures(&t), (1, true));
    let job = match t.query(Query::Jobs).await {
        QueryResult::Jobs(jobs) => {
            jobs.into_iter()
                .find(|j| j.kind == JobKind::Download && j.state == JobState::Failed)
                .expect("the failed download job")
                .id
        }
        other => panic!("{other:?}"),
    };
    // Held back, so the reset is seen before the job runs again.
    t.run(net(NetworkKind::Offline, false)).await;
    t.run(Command::RetryJob { id: job }).await;
    assert_eq!(failures(&t), (0, false));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_finished_download_refreshes_pins_once_not_on_every_jobs_change() {
    use hocket_core::core::Internal;
    use hocket_core::jobs::QueueEvent;
    let t = synced("dl-refresh", seeded_server(2, 180.0)).await;
    let job = |id: &str, state: JobState| Job {
        id: id.into(),
        kind: JobKind::Download,
        label: "Download".into(),
        state,
        done: 1,
        total: Some(1),
        failed: 0,
        created_at: 0.0,
        updated_at: 0.0,
        cancellable: false,
        waiting: None,
    };
    let post = |jobs: Vec<Job>| {
        t.core
            .post_internal(Internal::Jobs(QueueEvent::JobsChanged(jobs)))
            .unwrap();
    };
    let pins_changed = || {
        t.events
            .all()
            .iter()
            .filter(|e| matches!(e, Event::PinsChanged { .. }))
            .count()
    };
    t.core.settle().await;
    t.events.clear();

    post(vec![job("d1", JobState::Running)]);
    t.core.settle().await;
    assert_eq!(pins_changed(), 0, "still running");

    post(vec![job("d1", JobState::Done)]);
    t.core.settle().await;
    assert_eq!(pins_changed(), 1, "newly finished");

    // The queue keeps the finished job and re-sends it on every change.
    post(vec![
        job("d1", JobState::Done),
        job("s1", JobState::Running),
    ]);
    post(vec![job("d1", JobState::Done)]);
    t.core.settle().await;
    assert_eq!(pins_changed(), 1, "same finished job: no refresh");
    let library_changed = t
        .events
        .all()
        .iter()
        .filter(|e| matches!(e, Event::LibraryChanged { .. }))
        .count();
    assert_eq!(library_changed, 1);

    // Another download finishing refreshes again, a failed one too.
    post(vec![job("d1", JobState::Done), job("d2", JobState::Failed)]);
    t.core.settle().await;
    assert_eq!(pins_changed(), 2);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_running_download_refreshes_pins_as_each_track_lands() {
    use hocket_core::core::Internal;
    use hocket_core::jobs::QueueEvent;
    let t = synced("dl-progress", seeded_server(2, 180.0)).await;
    let job = |done: u32| Job {
        id: "d1".into(),
        kind: JobKind::Download,
        label: "Download".into(),
        state: JobState::Running,
        done,
        total: Some(3),
        failed: 0,
        created_at: 0.0,
        updated_at: 0.0,
        cancellable: false,
        waiting: None,
    };
    let post = |jobs: Vec<Job>| {
        t.core
            .post_internal(Internal::Jobs(QueueEvent::JobsChanged(jobs)))
            .unwrap();
    };
    let count = |f: fn(&Event) -> bool| t.events.all().iter().filter(|e| f(e)).count();
    t.core.settle().await;
    t.events.clear();

    post(vec![job(0)]);
    t.core.settle().await;
    assert_eq!(
        count(|e| matches!(e, Event::PinsChanged { .. })),
        0,
        "started"
    );

    post(vec![job(1)]);
    t.core.settle().await;
    assert_eq!(
        count(|e| matches!(e, Event::PinsChanged { .. })),
        1,
        "a track landed"
    );

    // A tick without progress (another job's change) doesn't refresh.
    post(vec![job(1)]);
    t.core.settle().await;
    assert_eq!(count(|e| matches!(e, Event::PinsChanged { .. })), 1);
    // Progress refreshes pins only; the library and storage wait for the end.
    assert_eq!(count(|e| matches!(e, Event::LibraryChanged { .. })), 0);
}
