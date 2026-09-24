//! The evictable stream cache, filled by the in-process stream reader: a
//! whole read is cached and replays from disk; offset reads, aborted reads
//! and dropped connections keep the bytes they fetched as partial spans,
//! and a later read fetches only the gaps (a partial entry becomes
//! complete in place); error envelopes never produce an entry; a server
//! stream that changes length discards its spans; a cache file the OS
//! removed or truncated is a miss, never an error; the budget never touches
//! pins; `ClearStreamCache` defers files in use; tokens are opaque and
//! expire; a backend without the capability keeps the direct server URL.

#![cfg(feature = "sim")]

mod stream_common;

use hocket_core::api::*;
use hocket_core::core::stream_reader::{StreamError, STREAM_URL_PREFIX};
use hocket_core::core::test_support::{seeded_server, TestCore, UpstreamBehaviour};
use stream_common::*;

/// A metered network turns background prefetch off, so these tests see
/// only the reads they make (no transcoding profile applies to it).
async fn no_prefetch(t: &TestCore) {
    t.run(Command::SetNetworkState {
        state: NetworkState {
            kind: NetworkKind::Wifi,
            metered: true,
            network_id: None,
        },
    })
    .await;
}

async fn started(name: &str, n: usize) -> TestCore {
    let t = TestCore::start(name, seeded_server(n, 100.0)).await;
    core_stream(&t).await;
    no_prefetch(&t).await;
    t
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_whole_read_is_cached_and_the_second_play_never_reaches_the_server() {
    let t = started("replay", 3).await;
    let media: Vec<u8> = (0..300_000u32).map(|i| (i % 251) as u8).collect();
    t.upstream.set_media("t0", media.clone(), "audio/flac");
    let s = source(&t, "t0").await;
    assert!(s.url.starts_with(STREAM_URL_PREFIX), "{}", s.url);
    assert!(no_credentials(&s.url), "{}", s.url);
    assert!(!s.url.contains('?'), "{}", s.url);
    assert_eq!(offline(&t, "t0").await, OfflineState::None);

    t.events.clear();
    let (info, bytes) = read_from(&t, &s.url, 0).await.unwrap();
    assert_eq!(bytes, media);
    assert_eq!(info.total_length, Some(media.len() as u64));
    assert_eq!(info.content_type.as_deref(), Some("audio/flac"));
    assert_eq!(t.upstream.stream_requests("t0"), 1);
    wait_offline(&t, "t0", OfflineState::Cached).await;
    assert!(t.events.all().iter().any(|e| matches!(
        e,
        Event::LibraryChanged { tables, ids, .. } if tables == &["tracks"] && ids == &["t0"]
    )));
    assert!(t.events.all().iter().any(
        |e| matches!(e, Event::StorageChanged { storage } if storage.cache_bytes == media.len() as f64)
    ));
    assert!(temp_files(&t).is_empty());

    // Second play: from disk, no server request, same bytes.
    let s2 = source(&t, "t0").await;
    let (_, again) = read_from(&t, &s2.url, 0).await.unwrap();
    assert_eq!(again, media);
    assert_eq!(t.upstream.stream_requests("t0"), 1, "served from the cache");
    // A mid-file open of the cached copy is served from disk too.
    let (info, tail) = read_from(&t, &s2.url, 100_000).await.unwrap();
    assert_eq!(tail, &media[100_000..]);
    assert_eq!(info.offset, 100_000);
    assert_eq!(info.total_length, Some(media.len() as u64));
    let info = t.core.stream_open(&s2.url, 10, Some(5)).await.unwrap();
    assert_eq!(info.length, Some(5));
    let b = t.core.stream_read(info.handle, 1024).await.unwrap();
    assert_eq!(&b[..], &media[10..15]);
    assert!(t
        .core
        .stream_read(info.handle, 1024)
        .await
        .unwrap()
        .is_empty());
    t.core.stream_close(info.handle);
    assert_eq!(t.upstream.stream_requests("t0"), 1);

    // The upstream URL (inside the core) is the only place credentials go.
    assert!(t.upstream.calls()[0].url.as_str().contains("stream"));
    t.core.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_offset_open_keeps_its_bytes_and_a_later_read_fetches_only_the_gap() {
    let t = started("offset", 2).await;
    let media: Vec<u8> = (0..100_000u32).map(|i| (i % 13) as u8).collect();
    t.upstream.set_media("t1", media.clone(), "audio/flac");
    let s = source(&t, "t1").await;
    let (info, bytes) = read_from(&t, &s.url, 1000).await.unwrap();
    assert_eq!(bytes, &media[1000..]);
    assert_eq!(info.offset, 1000);
    assert_eq!(info.total_length, Some(100_000));
    let calls = t.upstream.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].range.as_deref(), Some("bytes=1000-"));
    wait_handles_closed(&t).await;
    // The seek's bytes stay, as a partial entry: not "Cached" yet.
    let row = row_of(&t, "t1");
    assert!(!row.complete, "{row:?}");
    assert_eq!(row.spans, "1000-100000");
    assert_eq!(row.bytes, 99_000.0);
    assert_eq!(row.total, Some(100_000.0));
    assert_eq!(offline(&t, "t1").await, OfflineState::None);
    assert!(temp_files(&t).is_empty());
    // Past the end is refused.
    assert_eq!(
        t.core.stream_open(&s.url, 200_000, None).await.unwrap_err(),
        StreamError::RangeNotSatisfiable
    );
    // A read from the start fetches only the missing head, then serves the
    // rest from disk, and the entry is complete.
    let (_, whole) = read_from(&t, &s.url, 0).await.unwrap();
    assert_eq!(whole, media);
    let calls = t.upstream.calls();
    assert_eq!(calls.len(), 2, "{calls:?}");
    assert_eq!(calls[1].range.as_deref(), Some("bytes=0-999"));
    wait_offline(&t, "t1", OfflineState::Cached).await;
    let row = row_of(&t, "t1");
    assert!(row.complete && row.spans == "0-100000", "{row:?}");
    let (_, again) = read_from(&t, &s.url, 500).await.unwrap();
    assert_eq!(again, &media[500..]);
    assert_eq!(t.upstream.stream_requests("t1"), 2, "complete: from disk");
    assert_eq!(stream_files(&t).len(), 1, "one file, promoted in place");
    t.core.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn aborted_reads_and_dropped_connections_keep_their_bytes_and_resume_with_a_range() {
    let t = started("abort", 3).await;
    // Closed by the player mid-body (the server stalls after 40 KB).
    let media: Vec<u8> = (0..200_000u32).map(|i| (i % 241) as u8).collect();
    t.upstream.set_media("t0", media.clone(), "audio/flac");
    t.upstream
        .set_behaviour("t0", UpstreamBehaviour::StallAfter(40_000));
    let s = source(&t, "t0").await;
    let info = t.core.stream_open(&s.url, 0, None).await.unwrap();
    let mut got = 0;
    while got < 40_000 {
        got += t
            .core
            .stream_read(info.handle, 64 * 1024)
            .await
            .unwrap()
            .len();
    }
    // A read blocked on the stalled server is released by the close.
    let core = t.core.clone();
    let blocked = tokio::spawn(async move { core.stream_read(info.handle, 1024).await });
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    t.core.stream_close(info.handle);
    assert_eq!(blocked.await.unwrap().unwrap_err(), StreamError::Closed);
    wait_handles_closed(&t).await;
    let row = row_of(&t, "t0");
    assert_eq!((row.complete, row.spans.as_str()), (false, "0-40000"));
    assert_eq!(offline(&t, "t0").await, OfflineState::None);
    assert!(temp_files(&t).is_empty());
    // The next play serves the first 40 KB from disk and asks only for the rest.
    t.upstream.set_behaviour("t0", UpstreamBehaviour::Serve);
    let (_, bytes) = read_from(&t, &s.url, 0).await.unwrap();
    assert_eq!(bytes, media);
    let calls = t.upstream.calls();
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[1].range.as_deref(), Some("bytes=40000-"));
    wait_offline(&t, "t0", OfflineState::Cached).await;

    // The server drops the connection mid-body: what arrived stays.
    t.upstream.set_media("t1", vec![9u8; 200_000], "audio/flac");
    t.upstream
        .set_behaviour("t1", UpstreamBehaviour::FailAfter(50_000));
    let s = source(&t, "t1").await;
    let err = read_from(&t, &s.url, 0).await.unwrap_err();
    assert!(matches!(err, StreamError::Network(_)), "{err:?}");
    wait_handles_closed(&t).await;
    assert_eq!(row_of(&t, "t1").spans, "0-50000");
    assert_eq!(offline(&t, "t1").await, OfflineState::None);

    // Closed after one read on a healthy server: the chunk that arrived stays.
    let s = source(&t, "t2").await;
    let info = t.core.stream_open(&s.url, 0, None).await.unwrap();
    let first = t.core.stream_read(info.handle, 1024).await.unwrap();
    assert_eq!(first.len(), 1024);
    t.core.stream_close(info.handle);
    wait_handles_closed(&t).await;
    let row = row_of(&t, "t2");
    assert!(!row.complete && row.spans.starts_with("0-"), "{row:?}");
    assert_eq!(offline(&t, "t2").await, OfflineState::None);
    // Partial bytes count against the budget.
    match t.query(Query::Storage).await {
        QueryResult::Storage(s) => {
            assert_eq!(s.cache_bytes, 200_000.0 + 50_000.0 + row.bytes, "{s:?}")
        }
        other => panic!("{other:?}"),
    }
    t.core.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_stream_whose_length_changes_on_the_server_discards_its_spans() {
    let t = started("changed", 2).await;
    let old: Vec<u8> = vec![1u8; 100_000];
    t.upstream.set_media("t0", old.clone(), "audio/flac");
    let s = source(&t, "t0").await;
    read_from(&t, &s.url, 50_000).await.unwrap();
    wait_handles_closed(&t).await;
    assert_eq!(row_of(&t, "t0").spans, "50000-100000");
    // Re-encoded on the server: another length.
    let new: Vec<u8> = (0..120_000u32).map(|i| (i % 7) as u8).collect();
    t.upstream.set_media("t0", new.clone(), "audio/flac");
    let (info, bytes) = read_from(&t, &s.url, 0).await.unwrap();
    assert_eq!(info.total_length, Some(120_000));
    assert_eq!(bytes, new, "none of the old spans were served");
    wait_offline(&t, "t0", OfflineState::Cached).await;
    let row = row_of(&t, "t0");
    assert_eq!((row.complete, row.total), (true, Some(120_000.0)));
    let (_, again) = read_from(&t, &s.url, 0).await.unwrap();
    assert_eq!(again, new);
    t.core.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_cache_file_the_os_removed_or_truncated_is_a_miss_not_an_error() {
    let t = started("os-cleared", 3).await;
    let media: Vec<u8> = (0..300_000u32).map(|i| (i % 253) as u8).collect();
    t.upstream.set_media("t0", media.clone(), "audio/flac");
    let s = source(&t, "t0").await;
    read_from(&t, &s.url, 0).await.unwrap();
    wait_offline(&t, "t0", OfflineState::Cached).await;
    // Removed behind our back: the next read goes to the server.
    std::fs::remove_file(&row_of(&t, "t0").path).unwrap();
    let (_, bytes) = read_from(&t, &s.url, 0).await.unwrap();
    assert_eq!(bytes, media);
    assert_eq!(t.upstream.stream_requests("t0"), 2);
    wait_offline(&t, "t0", OfflineState::Cached).await;
    // Truncated while a read is under way: the read carries on from the
    // server at the same offset.
    let info = t.core.stream_open(&s.url, 0, None).await.unwrap();
    let mut out = t
        .core
        .stream_read(info.handle, 64 * 1024)
        .await
        .unwrap()
        .to_vec();
    let path = row_of(&t, "t0").path;
    std::fs::OpenOptions::new()
        .write(true)
        .open(&path)
        .unwrap()
        .set_len(10)
        .unwrap();
    loop {
        let b = t.core.stream_read(info.handle, 64 * 1024).await.unwrap();
        if b.is_empty() {
            break;
        }
        out.extend_from_slice(&b);
    }
    t.core.stream_close(info.handle);
    assert_eq!(out, media);
    let calls = t.upstream.calls();
    assert_eq!(calls.len(), 3);
    assert_eq!(calls[2].range.as_deref(), Some("bytes=65536-"));
    wait_handles_closed(&t).await;
    // At startup, rows whose files are gone are reconciled away.
    read_from(&t, &source(&t, "t1").await.url, 0).await.unwrap();
    wait_offline(&t, "t1", OfflineState::Cached).await;
    let gone = row_of(&t, "t1").path;
    t.core.shutdown().await;
    std::fs::remove_file(&gone).unwrap();
    let t = t.restart("os-cleared-2").await;
    assert_eq!(offline(&t, "t1").await, OfflineState::None);
    assert!(cache_rows(&t).iter().all(|r| r.track_id != "t1"));
    t.core.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_json_error_envelope_is_refused_and_not_cached() {
    let t = started("envelope", 2).await;
    t.upstream
        .set_behaviour("t0", UpstreamBehaviour::ErrorEnvelope);
    let s = source(&t, "t0").await;
    assert_eq!(
        t.core.stream_open(&s.url, 0, None).await.unwrap_err(),
        StreamError::ErrorEnvelope
    );
    t.core.settle().await;
    assert!(stream_files(&t).is_empty());
    assert_eq!(offline(&t, "t0").await, OfflineState::None);
    t.core.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unknown_and_expired_tokens_are_refused() {
    let t = started("tokens", 2).await;
    for bad in [
        format!("{STREAM_URL_PREFIX}{}", "0".repeat(32)),
        format!("{STREAM_URL_PREFIX}nope"),
        "".to_string(),
        "https://srv.fake/rest/stream?id=t0".to_string(),
    ] {
        assert_eq!(
            t.core.stream_open(&bad, 0, None).await.unwrap_err(),
            StreamError::UnknownToken,
            "{bad}"
        );
    }
    let s = source(&t, "t0").await;
    // The same track keeps its token while it is live.
    assert_eq!(source(&t, "t0").await.url, s.url);
    assert_ne!(source(&t, "t1").await.url, s.url);
    let info = t.core.stream_open(&s.url, 0, None).await.unwrap();
    t.core.stream_close(info.handle);
    assert_eq!(
        t.core.stream_read(info.handle, 10).await.unwrap_err(),
        StreamError::UnknownHandle
    );
    // Thirteen hours idle: expired.
    t.clock.advance(13.0 * 3600.0 * 1000.0);
    assert_eq!(
        t.core.stream_open(&s.url, 0, None).await.unwrap_err(),
        StreamError::UnknownToken
    );
    assert_eq!(t.upstream.stream_requests("t0"), 1, "only the live open");
    t.core.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_budget_evicts_least_recently_used_and_never_touches_pins() {
    let t = started("budget", 4).await;
    let mib = 1024 * 1024;
    t.run(Command::SetSetting {
        key: "storage.cacheMaxBytes".into(),
        value: (64 * mib).to_string(),
    })
    .await;
    // A pinned download (fetched through the server API, not the reader).
    t.run(Command::Pin {
        target: PinTarget::Track { id: "t3".into() },
        transcode: false,
    })
    .await;
    wait_offline(&t, "t3", OfflineState::Downloaded).await;
    let pinned = match t
        .query(Query::MediaSource {
            track_id: "t3".into(),
        })
        .await
    {
        QueryResult::Source(Some(s)) => s.url,
        other => panic!("{other:?}"),
    };
    assert!(pinned.starts_with("file://"), "{pinned}");
    for id in ["t0", "t1", "t2"] {
        t.upstream.set_media(id, vec![1u8; 30 * mib], "audio/flac");
    }
    read_whole(&t, "t0").await;
    wait_offline(&t, "t0", OfflineState::Cached).await;
    t.clock.advance(1000.0);
    read_whole(&t, "t1").await;
    wait_offline(&t, "t1", OfflineState::Cached).await;
    t.clock.advance(1000.0);
    // Replaying t0 makes t1 the least recently used.
    read_whole(&t, "t0").await;
    assert_eq!(t.upstream.stream_requests("t0"), 1);
    t.clock.advance(1000.0);
    read_whole(&t, "t2").await;
    wait_offline(&t, "t2", OfflineState::Cached).await;
    wait_offline(&t, "t1", OfflineState::None).await;
    assert_eq!(offline(&t, "t0").await, OfflineState::Cached);
    assert_eq!(offline(&t, "t3").await, OfflineState::Downloaded);
    let pinned_path = url::Url::parse(&pinned).unwrap().to_file_path().unwrap();
    assert!(pinned_path.exists(), "the pin is untouched");
    match t.query(Query::Storage).await {
        QueryResult::Storage(s) => {
            assert_eq!(s.cache_bytes, (60 * mib) as f64);
            assert!(s.downloads_bytes > 0.0);
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(stream_files(&t).len(), 2);
    t.core.shutdown().await;
}

async fn read_whole(t: &TestCore, id: &str) {
    let s = source(t, id).await;
    read_from(t, &s.url, 0).await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn clear_stream_cache_defers_files_in_use() {
    let t = started("clear", 4).await;
    for id in ["t0", "t1", "t2"] {
        let s = source(&t, id).await;
        read_from(&t, &s.url, 0).await.unwrap();
        wait_offline(&t, id, OfflineState::Cached).await;
    }
    assert_eq!(stream_files(&t).len(), 3);
    // t1 is open in a reader, t0 is loaded in the player.
    let s1 = source(&t, "t1").await;
    let held = t.core.stream_open(&s1.url, 0, None).await.unwrap();
    let first = t.core.stream_read(held.handle, 1000).await.unwrap();
    t.run(Command::PlayTracks {
        server_id: t.server_id.clone(),
        track_ids: vec!["t0".into()],
        start_index: 0,
        label: "One".into(),
        shuffle: false,
    })
    .await;
    t.run_for(500.0).await;
    assert!(t.backend.is_playing());

    t.run(Command::ClearStreamCache).await;
    assert_eq!(
        offline(&t, "t2").await,
        OfflineState::None,
        "not in use: gone"
    );
    assert_eq!(
        offline(&t, "t1").await,
        OfflineState::Cached,
        "open: deferred"
    );
    assert_eq!(
        offline(&t, "t0").await,
        OfflineState::Cached,
        "loaded: deferred"
    );
    assert_eq!(stream_files(&t).len(), 2);
    // The held reader keeps reading the whole file.
    let mut rest = first.len();
    loop {
        let b = t.core.stream_read(held.handle, 64 * 1024).await.unwrap();
        if b.is_empty() {
            break;
        }
        rest += b.len();
    }
    assert_eq!(rest, 64 * 1024);
    t.events.clear();
    t.core.stream_close(held.handle);
    wait_offline(&t, "t1", OfflineState::None).await;
    assert!(t.events.all().iter().any(|e| matches!(
        e,
        Event::LibraryChanged { ids, .. } if ids.contains(&"t1".to_string())
    )));
    assert_eq!(stream_files(&t).len(), 1);
    // The player moves on: t0 goes too.
    t.run(Command::PlayTracks {
        server_id: t.server_id.clone(),
        track_ids: vec!["t3".into()],
        start_index: 0,
        label: "Other".into(),
        shuffle: false,
    })
    .await;
    t.run_for(500.0).await;
    wait_offline(&t, "t0", OfflineState::None).await;
    assert!(stream_files(&t).is_empty(), "{:?}", stream_files(&t));
    match t.query(Query::Storage).await {
        QueryResult::Storage(s) => assert_eq!(s.cache_bytes, 0.0),
        other => panic!("{other:?}"),
    }
    t.core.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn backend_sources_carry_no_credentials_and_gapless_preload_reads_through_the_core() {
    let t = started("gapless", 3).await;
    t.run(Command::PlayTracks {
        server_id: t.server_id.clone(),
        track_ids: vec!["t0".into(), "t1".into(), "t2".into()],
        start_index: 0,
        label: "Three".into(),
        shuffle: false,
    })
    .await;
    t.run_for(500.0).await;
    let sources = t.sources.lock().clone();
    assert!(sources.iter().any(|s| s.track.id == "t0"));
    let next = sources
        .iter()
        .find(|s| s.track.id == "t1")
        .expect("the next item was preloaded")
        .clone();
    for s in &sources {
        assert!(s.url.starts_with(STREAM_URL_PREFIX), "{}", s.url);
        assert!(no_credentials(&s.url), "{}", s.url);
    }
    // The preloaded item reads through the reader and is cached.
    read_from(&t, &next.url, 0).await.unwrap();
    wait_offline(&t, "t1", OfflineState::Cached).await;
    t.core.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_external_backend_without_the_capability_keeps_the_direct_url() {
    let t = TestCore::start("external", seeded_server(2, 100.0)).await;
    t.wait_until(30_000.0, |t| {
        t.events
            .all()
            .iter()
            .any(|e| matches!(e, Event::SyncProgress { progress } if progress.finished))
    })
    .await;
    let s = source(&t, "t0").await;
    assert!(
        s.url.starts_with("https://srv.fake/rest/stream?id=t0"),
        "{}",
        s.url
    );
    // Turning it on switches to the reader; off again restores the URL.
    t.run(Command::SetBackendCapabilities { core_stream: true })
        .await;
    assert!(source(&t, "t0").await.url.starts_with(STREAM_URL_PREFIX));
    t.run(Command::SetBackendCapabilities { core_stream: false })
        .await;
    assert!(source(&t, "t0").await.url.starts_with("https://"));
    t.core.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_library_sync_that_sees_a_track_change_drops_its_cached_audio() {
    let t = started("server-change", 3).await;
    let s0 = source(&t, "t0").await;
    read_from(&t, &s0.url, 0).await.unwrap();
    wait_offline(&t, "t0", OfflineState::Cached).await;
    let s1 = source(&t, "t1").await;
    read_from(&t, &s1.url, 1000).await.unwrap();
    let s2 = source(&t, "t2").await;
    read_from(&t, &s2.url, 0).await.unwrap();
    wait_offline(&t, "t2", OfflineState::Cached).await;
    wait_handles_closed(&t).await;
    assert_eq!(cache_rows(&t).len(), 3);
    let old0 = row_of(&t, "t0").path;
    // t0 and t1 are replaced on the server (another size and suffix); t2
    // is untouched.
    for id in ["t0", "t1"] {
        let mut c = hocket_core::subsonic::fake::FakeServer::song(
            id,
            &format!("Track {id}"),
            "al0",
            "ar0",
            100.0,
        );
        c.track = Some(1);
        c.size = Some(5000.0);
        c.suffix = Some("mp3".into());
        c.content_type = Some("audio/mpeg".into());
        t.server.add_song(c);
    }
    t.events.clear();
    t.run(Command::SyncLibrary {
        server_id: t.server_id.clone(),
        full: true,
    })
    .await;
    t.wait_until(30_000.0, |t| {
        t.events
            .all()
            .iter()
            .any(|e| matches!(e, Event::SyncProgress { progress } if progress.finished))
    })
    .await;
    wait_offline(&t, "t0", OfflineState::None).await;
    let rows = cache_rows(&t);
    assert_eq!(
        rows.iter().map(|r| r.track_id.as_str()).collect::<Vec<_>>(),
        vec!["t2"],
        "{rows:?}"
    );
    assert!(!std::path::Path::new(&old0).exists());
    assert_eq!(offline(&t, "t2").await, OfflineState::Cached);
    // The next play fetches the new file.
    read_from(&t, &source(&t, "t0").await.url, 0).await.unwrap();
    assert_eq!(t.upstream.stream_requests("t0"), 2);
    t.core.shutdown().await;
}
