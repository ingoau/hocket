//! Library scenarios: sync, search, mutations through the outbox, undo tier
//! 2, downloads, filters, lyrics and settings.

use std::sync::Arc;

use futures::future::BoxFuture;
use hocket_core::api::*;
use hocket_core::core::test_support::{seeded_server, TestBackend, TestCore, TestOptions};
use hocket_core::lyrics::{LyricsError, LyricsHttp};
use hocket_core::sim::SimTime;
use hocket_core::subsonic::fake::FakeServer;
use hocket_core::subsonic::types::{CueLine, LyricCue, LyricLineBody, StructuredLyrics};
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
    let servers = t.snapshot().await.servers;
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
        targets: vec![RatingTarget::Track { id: "t0".into() }],
        rating: 4,
    })
    .await;
    // Optimistic locally, then flushed.
    match t.query(Query::Track { id: "t0".into() }).await {
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
        targets: vec![RatingTarget::Track { id: "t0".into() }],
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
        targets: vec![RatingTarget::Track { id: "t1".into() }],
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
        targets: vec![RatingTarget::Track { id: "t2".into() }],
        loved: true,
    })
    .await;
    t.run(Command::CreatePlaylist {
        server_id: t.server_id.clone(),
        name: "Road trip".into(),
        track_ids: vec!["t0".into(), "t1".into()],
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
    match t.query(Query::Track { id: "t2".into() }).await {
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
            RatingTarget::Track { id: "t1".into() },
            RatingTarget::Track { id: "t2".into() },
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
    let clock = SimTime::new(1_700_000_000_000.0);
    let dir = tempfile::tempdir().unwrap();
    let config = CoreConfig {
        data_dir: dir.path().join("d").to_string_lossy().into_owned(),
        cache_dir: dir.path().join("c").to_string_lossy().into_owned(),
        device_id: "dev-lyrics".into(),
        device_name: "Lyrics".into(),
        platform: Platform::Linux,
        app_version: "test".into(),
        audio: AudioMode::None,
        coordinator_listen: None,
    };
    let (backend, scripted) = TestBackend::scripted(clock.clone());
    let external = r#"{"plainLyrics":"From outside","syncedLyrics":"[00:01.00] From outside\n[00:03.00] Synced too"}"#;
    let core = Core::new_for_test_with(
        config,
        clock.clone(),
        TestOptions {
            backend,
            api: Arc::new(server.clone()),
            server_url: "https://music.example/".into(),
            password: "pw".into(),
            net: None,
            lyrics_http: Some(Arc::new(FakeLyricsHttp(external.into()))),
            seed: 7,
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
        clock,
        server: server.clone(),
        events,
        device_id: "dev-lyrics".into(),
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
    t.run(Command::ImportConfig { document: doc }).await;
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
        QueryResult::Actions(a) => assert!(a.iter().any(|a| a.id == "navigateFilters")),
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
            ids: vec!["t0".into()],
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
    match t.query(Query::Track { id: "t0".into() }).await {
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
    t.run(Command::Unpin {
        target: PinTarget::Album { id: "al0".into() },
    })
    .await;
    match t.query(Query::Pins).await {
        QueryResult::Pins(p) => assert!(p.is_empty()),
        other => panic!("{other:?}"),
    }
}
