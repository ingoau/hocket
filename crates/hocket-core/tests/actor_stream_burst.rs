//! A playing stream is fetched to its end in one burst, ahead of a player
//! that reads in bursts of its own (the radio sleeps for the rest of the
//! track): on a metered network too, from where the player opened it,
//! skipping what the cache already has, and only when the stream is being
//! cached; the player reads the pump's bytes back from disk, which saves
//! no data.

#![cfg(feature = "sim")]

mod stream_common;

use hocket_core::api::*;
use hocket_core::core::test_support::{seeded_server, TestCore, UpstreamBehaviour};
use stream_common::*;

/// On cellular: background prefetch is off, so the reads are the test's.
/// How long a stream is open before it is fetched ahead (`PUMP_START_DELAY_MS`).
const PUMP_DELAY_MS: f64 = 20_000.0;

async fn started(name: &str, n: usize) -> TestCore {
    let t = TestCore::start(name, seeded_server(n, 100.0)).await;
    core_stream(&t).await;
    t.run(Command::SetNetworkState {
        state: NetworkState {
            kind: NetworkKind::Cellular,
            metered: true,
            network_id: None,
        },
    })
    .await;
    t
}

async fn storage(t: &TestCore) -> StorageSummary {
    match t.query(Query::Storage).await {
        QueryResult::Storage(s) => s,
        other => panic!("{other:?}"),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_playing_stream_is_fetched_to_its_end_while_the_player_reads_only_its_start() {
    let t = started("burst", 2).await;
    let media: Vec<u8> = (0..2_000_000u32).map(|i| (i % 251) as u8).collect();
    t.upstream.set_media("t0", media.clone(), "audio/flac");
    let s = source(&t, "t0").await;
    assert!(!s.local, "nothing cached yet");
    let info = t.core.stream_open(&s.url, 0, None).await.unwrap();
    let mut out = t
        .core
        .stream_read(info.handle, 64 * 1024)
        .await
        .unwrap()
        .to_vec();
    // Once the stream has been open a while, the player's next read starts the pump.
    t.clock.advance(PUMP_DELAY_MS);
    out.extend_from_slice(&t.core.stream_read(info.handle, 64 * 1024).await.unwrap());
    // The player reads nothing more; the whole track lands in the cache.
    wait_offline(&t, "t0", OfflineState::Cached).await;
    assert_eq!(t.upstream.stream_requests("t0"), 1);
    let row = row_of(&t, "t0");
    assert!(row.complete && row.spans == "0-2000000", "{row:?}");
    assert!(source(&t, "t0").await.local, "complete: no network needed");
    // The player's later bursts come from disk.
    loop {
        let b = t.core.stream_read(info.handle, 64 * 1024).await.unwrap();
        if b.is_empty() {
            break;
        }
        out.extend_from_slice(&b);
    }
    t.core.stream_close(info.handle);
    assert_eq!(out, media);
    assert_eq!(t.upstream.stream_requests("t0"), 1);
    let s = storage(&t).await;
    assert_eq!(s.fetched_bytes, 2_000_000.0);
    assert_eq!(s.served_from_disk_bytes, 0.0, "fetched for this play");
    t.core.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_seek_fetches_from_its_offset_and_a_later_play_fills_only_the_head() {
    let t = started("burst-seek", 2).await;
    let media: Vec<u8> = (0..2_000_000u32).map(|i| (i % 241) as u8).collect();
    t.upstream.set_media("t0", media.clone(), "audio/flac");
    let s = source(&t, "t0").await;
    let info = t.core.stream_open(&s.url, 1_000_000, None).await.unwrap();
    let first = t.core.stream_read(info.handle, 1024).await.unwrap();
    assert_eq!(&first[..], &media[1_000_000..1_001_024]);
    t.clock.advance(PUMP_DELAY_MS);
    t.core.stream_read(info.handle, 1024).await.unwrap();
    wait_rows(&t, "the tail fetched ahead", |rows| {
        rows.iter().any(|r| r.spans == "1000000-2000000")
    })
    .await;
    t.core.stream_close(info.handle);
    wait_handles_closed(&t).await;
    assert!(!row_of(&t, "t0").complete);
    assert!(!source(&t, "t0").await.local);
    // Played from the start: only the head is fetched (once, ahead of the
    // player), and the entry completes.
    let (_, whole) = read_from(&t, &s.url, 0).await.unwrap();
    assert_eq!(whole, media);
    let calls = t.upstream.calls();
    assert_eq!(calls.len(), 2, "{calls:?}");
    assert_eq!(calls[0].range.as_deref(), Some("bytes=1000000-"));
    assert_eq!(calls[1].range.as_deref(), Some("bytes=0-999999"));
    wait_offline(&t, "t0", OfflineState::Cached).await;
    t.core.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_short_listen_is_not_fetched_ahead() {
    // A song preview, or a track skipped at once: closed before the delay.
    let t = started("burst-short", 2).await;
    let media: Vec<u8> = (0..2_000_000u32).map(|i| (i % 227) as u8).collect();
    t.upstream.set_media("t0", media, "audio/flac");
    let s = source(&t, "t0").await;
    let info = t.core.stream_open(&s.url, 0, None).await.unwrap();
    t.core.stream_read(info.handle, 64 * 1024).await.unwrap();
    t.clock.advance(PUMP_DELAY_MS / 2.0);
    t.core.stream_read(info.handle, 64 * 1024).await.unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    assert!(storage(&t).await.fetched_bytes < 1_000_000.0);
    t.core.stream_close(info.handle);
    t.core.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_stream_too_big_for_the_cache_is_not_fetched_ahead() {
    let t = started("burst-budget", 2).await;
    t.run(Command::SetSetting {
        key: "storage.cacheMaxBytes".into(),
        value: (64 * 1024 * 1024).to_string(),
    })
    .await;
    let media: Vec<u8> = (0..70_000_000u32).map(|i| (i % 239) as u8).collect();
    t.upstream.set_media("t0", media.clone(), "audio/flac");
    let s = source(&t, "t0").await;
    let info = t.core.stream_open(&s.url, 0, None).await.unwrap();
    t.core.stream_read(info.handle, 64 * 1024).await.unwrap();
    t.clock.advance(PUMP_DELAY_MS);
    t.core.stream_read(info.handle, 64 * 1024).await.unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    // Nowhere to put it: the stream is fetched only as fast as it is read.
    assert!(cache_rows(&t).is_empty(), "{:?}", cache_rows(&t));
    assert!(storage(&t).await.fetched_bytes < 1_000_000.0);
    t.core.stream_close(info.handle);
    t.core.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_connection_the_pump_lost_fails_the_read_where_it_dropped() {
    let t = started("burst-drop", 2).await;
    let media: Vec<u8> = (0..300_000u32).map(|i| (i % 233) as u8).collect();
    t.upstream.set_media("t0", media.clone(), "audio/flac");
    t.upstream
        .set_behaviour("t0", UpstreamBehaviour::FailAfter(120_000));
    let s = source(&t, "t0").await;
    let info = t.core.stream_open(&s.url, 0, None).await.unwrap();
    let mut out = t
        .core
        .stream_read(info.handle, 1024)
        .await
        .unwrap()
        .to_vec();
    t.clock.advance(PUMP_DELAY_MS);
    out.extend_from_slice(&t.core.stream_read(info.handle, 1024).await.unwrap());
    wait_rows(&t, "what arrived before the drop", |rows| {
        rows.iter().any(|r| r.spans == "0-120000")
    })
    .await;
    // The player gets every byte that arrived, then the error.
    let err = loop {
        match t.core.stream_read(info.handle, 64 * 1024).await {
            Ok(b) => {
                assert!(!b.is_empty(), "end of input before the error");
                out.extend_from_slice(&b);
            }
            Err(e) => break e,
        }
    };
    assert!(
        matches!(
            err,
            hocket_core::core::stream_reader::StreamError::Network(_)
        ),
        "{err:?}"
    );
    assert_eq!(out, &media[..120_000]);
    t.core.stream_close(info.handle);
    assert_eq!(t.upstream.stream_requests("t0"), 1);
    t.core.shutdown().await;
}
