//! Stream-cache policy through a full core: what plays offline (downloads
//! and complete cache entries; the rest is skipped, autoplay picks from
//! it, and the "Available offline" filter lists it), the signals eviction
//! weighs, pinning a cached track without downloading it, a full original
//! serving a transcoded request, the auto-sized budget and the "data
//! saved" counters.

#![cfg(feature = "sim")]

mod stream_common;

use hocket_core::api::*;
use hocket_core::core::test_support::{seeded_server, TestCore};
use stream_common::*;

fn network(kind: NetworkKind, metered: bool) -> Command {
    Command::SetNetworkState {
        state: NetworkState {
            kind,
            metered,
            network_id: None,
        },
    }
}

async fn started(name: &str, n: usize) -> TestCore {
    let t = TestCore::start(name, seeded_server(n, 100.0)).await;
    core_stream(&t).await;
    // Metered: no background prefetch, only the reads the test makes.
    t.run(network(NetworkKind::Wifi, true)).await;
    t
}

async fn read_whole(t: &TestCore, id: &str) {
    let s = source(t, id).await;
    read_from(t, &s.url, 0).await.unwrap();
    wait_offline(t, id, OfflineState::Cached).await;
}

async fn pin(t: &TestCore, id: &str) {
    t.run(Command::Pin {
        target: PinTarget::Track { id: id.into() },
        transcode: false,
    })
    .await;
    wait_offline(t, id, OfflineState::Downloaded).await;
}

fn signal(t: &TestCore, id: &str) -> Option<(i64, i64, i64)> {
    let db = t.open_db();
    db.with_conn(|c| {
        use rusqlite::OptionalExtension;
        Ok(c.query_row(
            "SELECT autoplay, skips, primed FROM cache_signals WHERE track_id = ?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()?)
    })
    .unwrap()
}

async fn storage(t: &TestCore) -> StorageSummary {
    match t.query(Query::Storage).await {
        QueryResult::Storage(s) => s,
        other => panic!("{other:?}"),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn offline_plays_downloads_and_complete_cache_entries_and_skips_the_rest() {
    let t = started("offline-play", 6).await;
    read_whole(&t, "t0").await;
    pin(&t, "t1").await;
    // A partial entry is not enough to play offline.
    read_from(&t, &source(&t, "t4").await.url, 1000)
        .await
        .unwrap();
    // The "Available offline" field lists downloaded and fully cached.
    let rule = FilterNode::All(vec![FilterNode::Rule(FilterRule {
        field: FilterField::AvailableOffline,
        op: FilterOp::IsTrue,
        value: FilterValue::Bool(true),
    })]);
    match t
        .query(Query::Tracks {
            server_id: t.server_id.clone(),
            filter: Some(rule),
            sort: SortOrder::Title,
            descending: false,
            page: Page {
                offset: 0,
                limit: 50,
            },
        })
        .await
    {
        QueryResult::Tracks(p) => {
            let ids: Vec<_> = p.items.iter().map(|t| t.id.as_str()).collect();
            assert_eq!(ids, ["t0", "t1"]);
            assert_eq!(p.items[0].offline, OfflineState::Cached);
        }
        other => panic!("{other:?}"),
    }
    match t.query(Query::Filters).await {
        QueryResult::Filters(f) => assert!(f.iter().any(|f| f.id == "builtin:available-offline")),
        other => panic!("{other:?}"),
    }

    t.run(network(NetworkKind::Offline, false)).await;
    t.events.clear();
    t.sources.lock().clear();
    t.run(Command::PlayTracks {
        server_id: t.server_id.clone(),
        track_ids: ["t2", "t0", "t3", "t4", "t1"]
            .iter()
            .map(|s| s.to_string())
            .collect(),
        start_index: 0,
        label: "Mixed".into(),
        shuffle: false,
    })
    .await;
    t.run_for(1000.0).await;
    assert_eq!(t.current_track_id().await.as_deref(), Some("t0"));
    assert!(t.backend.is_playing());
    let loaded: Vec<String> = t
        .sources
        .lock()
        .iter()
        .map(|s| s.track.id.clone())
        .collect();
    assert!(!loaded.contains(&"t2".to_string()), "{loaded:?}");
    // The gapless preload skips what can't play offline.
    assert_eq!(loaded, ["t0", "t1"], "t0 loaded with t1 preloaded");
    let q = t.queue().await;
    assert!(q
        .history
        .iter()
        .any(|e| e.track.id == "t2" && e.item.unavailable));
    // The queue shows what is available offline.
    let up: Vec<_> = q
        .upcoming
        .iter()
        .map(|e| (e.track.id.as_str(), e.track.offline))
        .collect();
    assert_eq!(
        up,
        [
            ("t3", OfflineState::None),
            ("t4", OfflineState::None),
            ("t1", OfflineState::Downloaded)
        ]
    );
    t.run(Command::Next).await;
    t.run_for(1000.0).await;
    assert_eq!(t.current_track_id().await.as_deref(), Some("t1"));
    assert!(t.backend.is_playing());
    let notices = t
        .events
        .all()
        .iter()
        .filter(|e| matches!(e, Event::PlayerNotice { message: Some(m) } if m.contains("Offline")))
        .count();
    assert!(notices >= 1);
    assert_eq!(
        t.upstream.calls().len(),
        2,
        "only the reads made before going offline"
    );
    t.core.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn offline_autoplay_picks_what_plays_offline() {
    let t = started("offline-autoplay", 6).await;
    read_whole(&t, "t0").await;
    pin(&t, "t3").await;
    t.run(network(NetworkKind::Offline, false)).await;
    t.run(Command::PlayTracks {
        server_id: t.server_id.clone(),
        track_ids: vec!["t0".into()],
        start_index: 0,
        label: "One".into(),
        shuffle: false,
    })
    .await;
    t.run(Command::SetAutoplay { enabled: true }).await;
    t.run_for(500.0).await;
    t.run(Command::Next).await;
    t.run_for(1000.0).await;
    let q = t.queue().await;
    let current = q.current.clone().expect("autoplay continued");
    assert_eq!(current.track.id, "t3");
    match &current.item.source {
        QueueSource::Autoplay { reason, .. } => assert_eq!(reason, "Available offline"),
        other => panic!("{other:?}"),
    }
    assert!(q.upcoming.is_empty(), "nothing else plays offline: {q:?}");
    // An autoplay pick is marked for eviction (a one-off goes early).
    assert_eq!(signal(&t, "t3").map(|s| s.0), Some(1));
    for endpoint in [
        "getSimilarSongs2",
        "getRandomSongs",
        "getTopSongs",
        "getSonicSimilarTracks",
    ] {
        assert_eq!(t.server.calls_to(endpoint), 0, "{:?}", t.server.calls());
    }
    t.core.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn playback_records_skips_and_autoplay_picks_for_eviction() {
    let t = started("signals", 4).await;
    t.run(Command::PlayTracks {
        server_id: t.server_id.clone(),
        track_ids: vec!["t0".into(), "t1".into(), "t2".into()],
        start_index: 0,
        label: "Three".into(),
        shuffle: false,
    })
    .await;
    t.run_for(5_000.0).await;
    // Left five seconds in: an early skip.
    t.run(Command::Next).await;
    t.run_for(40_000.0).await;
    // Left forty seconds in: not early.
    t.run(Command::Next).await;
    t.run_for(1_000.0).await;
    assert_eq!(signal(&t, "t0").map(|s| s.1), Some(1));
    assert_eq!(signal(&t, "t1").map(|s| s.1).unwrap_or(0), 0);
    t.core.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pinning_a_fully_cached_track_keeps_the_cached_file() {
    let t = started("pin-cached", 3).await;
    let media: Vec<u8> = (0..120_000u32).map(|i| (i % 211) as u8).collect();
    t.upstream.set_media("t0", media.clone(), "audio/flac");
    read_whole(&t, "t0").await;
    let cached = row_of(&t, "t0").path;
    t.run(Command::Pin {
        target: PinTarget::Track { id: "t0".into() },
        transcode: false,
    })
    .await;
    wait_offline(&t, "t0", OfflineState::Downloaded).await;
    assert_eq!(t.server.calls_to("download"), 0, "{:?}", t.server.calls());
    assert!(
        cache_rows(&t).is_empty(),
        "the cache entry moved into downloads"
    );
    assert!(!std::path::Path::new(&cached).exists());
    let s = source(&t, "t0").await;
    assert!(s.url.starts_with("file://"), "{}", s.url);
    let path = url::Url::parse(&s.url).unwrap().to_file_path().unwrap();
    assert_eq!(std::fs::read(path).unwrap(), media);
    match t.query(Query::Pins).await {
        QueryResult::Pins(p) => assert_eq!((p[0].downloaded_count, p[0].bytes), (1, 120_000.0)),
        other => panic!("{other:?}"),
    }
    // A partial entry is not enough: that pin downloads.
    read_from(&t, &source(&t, "t1").await.url, 100)
        .await
        .unwrap();
    pin(&t, "t1").await;
    assert_eq!(t.server.calls_to("download"), 1);
    t.core.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_fully_cached_original_serves_a_transcoded_request() {
    let t = started("original", 3).await;
    // Cellular transcodes to 96k Opus.
    t.run(Command::SetSetting {
        key: "transcoding.profiles".into(),
        value: r#"{"cellular":{"format":"opus","maxBitRate":96,"cannotDecode":[]}}"#.into(),
    })
    .await;
    let media: Vec<u8> = (0..150_000u32).map(|i| (i % 199) as u8).collect();
    t.upstream.set_media("t0", media.clone(), "audio/flac");
    read_whole(&t, "t0").await;
    t.run(network(NetworkKind::Cellular, true)).await;
    let s = source(&t, "t0").await;
    assert!(!s.transcoded, "the original is served as it is");
    assert_eq!(s.mime_type.as_deref(), Some("audio/flac"));
    let (info, bytes) = read_from(&t, &s.url, 0).await.unwrap();
    assert_eq!(bytes, media);
    assert_eq!(info.content_type.as_deref(), Some("audio/flac"));
    assert_eq!(t.upstream.stream_requests("t0"), 1, "no transcode fetched");
    // Without an original, the transcode is fetched and cached on its own.
    let s1 = source(&t, "t1").await;
    assert!(s1.transcoded);
    read_from(&t, &s1.url, 0).await.unwrap();
    let call = t.upstream.calls().last().cloned().unwrap();
    assert!(call.url.as_str().contains("format=opus"), "{}", call.url);
    wait_offline(&t, "t1", OfflineState::Cached).await;
    assert_eq!(row_of(&t, "t1").profile, "opus-96");
    // The original arriving later makes the transcode redundant.
    t.run(network(NetworkKind::Wifi, true)).await;
    let o1 = source(&t, "t1").await;
    assert!(!o1.transcoded);
    read_from(&t, &o1.url, 0).await.unwrap();
    wait_rows(&t, "only the original of t1 is kept", |rows| {
        rows.iter()
            .filter(|r| r.track_id == "t1")
            .map(|r| r.profile.as_str())
            .collect::<Vec<_>>()
            == [""]
    })
    .await;
    t.core.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_budget_is_automatic_until_set_and_data_saved_is_counted() {
    let t = started("budget-auto", 3).await;
    let s = storage(&t).await;
    assert!(s.cache_budget_auto);
    assert_eq!(
        s.cache_budget_bytes,
        2.0 * 1024.0 * 1024.0 * 1024.0,
        "unknown free space"
    );
    t.run(Command::SetSetting {
        key: "storage.cacheMaxBytes".into(),
        value: (100 * 1024 * 1024).to_string(),
    })
    .await;
    let s = storage(&t).await;
    assert!(!s.cache_budget_auto);
    assert_eq!(s.cache_budget_bytes, 100.0 * 1024.0 * 1024.0);
    t.run(Command::ResetSetting {
        key: "storage.cacheMaxBytes".into(),
    })
    .await;
    assert!(storage(&t).await.cache_budget_auto);

    // One fetch, two plays from disk.
    t.upstream.set_media("t0", vec![4u8; 100_000], "audio/flac");
    read_whole(&t, "t0").await;
    for _ in 0..2 {
        read_from(&t, &source(&t, "t0").await.url, 0).await.unwrap();
    }
    let s = storage(&t).await;
    assert_eq!(s.fetched_bytes, 100_000.0);
    assert_eq!(s.served_from_disk_bytes, 200_000.0);
    assert_eq!(s.data_saved_bytes, 200_000.0);
    // A partial entry shows up as such.
    read_from(&t, &source(&t, "t1").await.url, 1000)
        .await
        .unwrap();
    wait_handles_closed(&t).await;
    let s = storage(&t).await;
    assert_eq!(s.partial_cache_bytes, row_of(&t, "t1").bytes);
    // The counters survive a restart.
    let t = t.restart("budget-auto-2").await;
    let s = storage(&t).await;
    assert_eq!(s.served_from_disk_bytes, 200_000.0);
    t.core.shutdown().await;
}
