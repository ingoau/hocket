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
    started_with(name, n, 100.0).await
}

async fn started_with(name: &str, n: usize, duration_s: f64) -> TestCore {
    let t = TestCore::start(name, seeded_server(n, duration_s)).await;
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
    assert_eq!(t.upstream.stream_requests("t0"), 1);
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

/// Open at 0, read once, and once more after the start delay (which
/// starts the pump). Returns the handle.
async fn play_past_the_delay(t: &TestCore, url: &str) -> u64 {
    let info = t.core.stream_open(url, 0, None).await.unwrap();
    t.core.stream_read(info.handle, 1024).await.unwrap();
    t.clock.advance(PUMP_DELAY_MS);
    t.core.stream_read(info.handle, 1024).await.unwrap();
    info.handle
}

/// Poll (real time) until the core counts this many fetched bytes.
async fn wait_fetched(t: &TestCore, want: f64) {
    for _ in 0..400 {
        if storage(t).await.fetched_bytes >= want {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    panic!("fetched only {}", storage(t).await.fetched_bytes);
}

/// A head of `n` bytes from an earlier, short listen.
async fn cache_head(t: &TestCore, url: &str, n: u64) {
    let info = t.core.stream_open(url, 0, Some(n)).await.unwrap();
    while !t
        .core
        .stream_read(info.handle, 64 * 1024)
        .await
        .unwrap()
        .is_empty()
    {}
    t.core.stream_close(info.handle);
    wait_handles_closed(t).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn closing_in_the_middle_of_a_burst_keeps_what_was_written() {
    let t = started("burst-close", 2).await;
    let media: Vec<u8> = (0..2_000_000u32).map(|i| (i % 229) as u8).collect();
    t.upstream.set_media("t0", media, "audio/flac");
    // The server stalls part-way: the pump has written up to there.
    t.upstream
        .set_behaviour("t0", UpstreamBehaviour::StallAfter(1_200_000));
    let s = source(&t, "t0").await;
    let handle = play_past_the_delay(&t, &s.url).await;
    wait_fetched(&t, 1_200_000.0).await;
    t.core.stream_close(handle);
    // Merged as the closed handle's source goes (the pump drops it once it
    // sees the close), not before: nothing was due before the end.
    wait_rows(&t, "the burst's bytes", |rows| {
        rows.iter().any(|r| r.spans == "0-1200000" && !r.complete)
    })
    .await;
    t.core.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_transcode_of_unknown_length_is_fetched_to_its_end_which_is_its_length() {
    let t = started("burst-unknown", 2).await;
    let media: Vec<u8> = (0..700_000u32).map(|i| (i % 223) as u8).collect();
    t.upstream.set_media("t0", media.clone(), "audio/ogg");
    t.upstream
        .set_behaviour("t0", UpstreamBehaviour::UnknownLength);
    let s = source(&t, "t0").await;
    let info = t.core.stream_open(&s.url, 0, None).await.unwrap();
    assert_eq!(info.total_length, None);
    let mut out = t
        .core
        .stream_read(info.handle, 1024)
        .await
        .unwrap()
        .to_vec();
    t.clock.advance(PUMP_DELAY_MS);
    out.extend_from_slice(&t.core.stream_read(info.handle, 1024).await.unwrap());
    wait_offline(&t, "t0", OfflineState::Cached).await;
    let row = row_of(&t, "t0");
    assert_eq!((row.complete, row.total), (true, Some(700_000.0)));
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
    t.core.shutdown().await;
}

/// The head is cached, so the pump opens its own fetch for the rest; a
/// server slow to answer it does not hold up reads from disk, and the
/// reads do not abort it (each abort would start another request, and
/// for a transcode another ffmpeg on the server).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reads_from_disk_neither_wait_for_nor_abort_the_pumps_fetch() {
    let t = started("burst-open", 2).await;
    let media: Vec<u8> = (0..2_000_000u32).map(|i| (i % 211) as u8).collect();
    t.upstream.set_media("t0", media.clone(), "audio/flac");
    let s = source(&t, "t0").await;
    cache_head(&t, &s.url, 500_000).await;
    assert_eq!(row_of(&t, "t0").spans, "0-500000");
    t.upstream.hold_responses("t0");
    let handle = play_past_the_delay(&t, &s.url).await;
    // The pump's fetch waits on the server; reads go on from disk.
    for _ in 0..5 {
        tokio::time::timeout(
            std::time::Duration::from_secs(5),
            t.core.stream_read(handle, 16 * 1024),
        )
        .await
        .expect("a read from disk waited on the server")
        .unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    assert_eq!(
        t.upstream.stream_requests("t0"),
        2,
        "{:?}",
        t.upstream.calls()
    );
    t.upstream.release_responses("t0");
    wait_offline(&t, "t0", OfflineState::Cached).await;
    let calls = t.upstream.calls();
    assert_eq!(calls.len(), 2, "{calls:?}");
    assert_eq!(calls[1].range.as_deref(), Some("bytes=500000-"));
    t.core.stream_close(handle);
    t.core.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_fetch_the_pump_could_not_open_is_tried_again_later() {
    let t = started("burst-retry", 2).await;
    let media: Vec<u8> = (0..2_000_000u32).map(|i| (i % 199) as u8).collect();
    t.upstream.set_media("t0", media, "audio/flac");
    let s = source(&t, "t0").await;
    cache_head(&t, &s.url, 500_000).await;
    t.upstream
        .set_behaviour("t0", UpstreamBehaviour::RefuseOnce);
    let handle = play_past_the_delay(&t, &s.url).await;
    for _ in 0..400 {
        if t.upstream.stream_requests("t0") == 2 {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    assert_eq!(t.upstream.stream_requests("t0"), 2, "the pump tried");
    // Reads within the back-off do not start it again...
    t.core.stream_read(handle, 1024).await.unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    assert_eq!(t.upstream.stream_requests("t0"), 2);
    assert!(!row_of(&t, "t0").complete);
    // ...a read after it does, and the rest arrives.
    t.clock.advance(30_000.0);
    t.core.stream_read(handle, 1024).await.unwrap();
    wait_offline(&t, "t0", OfflineState::Cached).await;
    assert_eq!(t.upstream.stream_requests("t0"), 3);
    t.core.stream_close(handle);
    t.core.shutdown().await;
}

/// On a metered network (without prefetch on mobile data) the pump fetches
/// five minutes of playing time past the read position, not the whole
/// track; on an unmetered one, the rest.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn on_a_metered_network_the_pump_fetches_five_minutes_ahead() {
    // 1000 s in 10 MB: 10 bytes a ms, so five minutes is 3 MB.
    let t = started_with("burst-metered", 2, 1000.0).await;
    let media: Vec<u8> = (0..10_000_000u32).map(|i| (i % 197) as u8).collect();
    t.upstream.set_media("t0", media, "audio/flac");
    let s = source(&t, "t0").await;
    let handle = play_past_the_delay(&t, &s.url).await;
    wait_fetched(&t, 3_000_000.0).await;
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    let fetched = storage(&t).await.fetched_bytes;
    assert!(fetched < 3_100_000.0, "{fetched}");
    // Reading a little does not restart it (it waits for half the horizon)...
    t.core.stream_read(handle, 64 * 1024).await.unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    assert_eq!(storage(&t).await.fetched_bytes, fetched);
    // ...on Wi-Fi the next read fetches the rest.
    t.run(Command::SetNetworkState {
        state: NetworkState {
            kind: NetworkKind::Wifi,
            metered: false,
            network_id: None,
        },
    })
    .await;
    t.core.stream_read(handle, 1024).await.unwrap();
    wait_offline(&t, "t0", OfflineState::Cached).await;
    t.core.stream_close(handle);
    t.core.shutdown().await;
}

/// Whether a source needs the network is the core's answer for the very
/// stream (its profile), not the track's offline state: a complete
/// transcode makes the track Cached, but online an original is never
/// served from it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_source_is_local_only_when_its_own_profile_is_cached() {
    let t = started("burst-local", 2).await;
    let media: Vec<u8> = (0..300_000u32).map(|i| (i % 193) as u8).collect();
    t.upstream.set_media("t0", media, "audio/ogg");
    let cellular = source(&t, "t0").await;
    assert!(!t.core.stream_local(&cellular.url));
    read_from(&t, &cellular.url, 0).await.unwrap();
    wait_offline(&t, "t0", OfflineState::Cached).await;
    assert!(t.core.stream_local(&cellular.url));
    assert!(source(&t, "t0").await.local);
    // On Wi-Fi the original plays: not cached, whatever the track shows.
    t.run(Command::SetNetworkState {
        state: NetworkState {
            kind: NetworkKind::Wifi,
            metered: false,
            network_id: None,
        },
    })
    .await;
    let wifi = source(&t, "t0").await;
    assert_ne!(wifi.url, cellular.url);
    assert_eq!(offline(&t, "t0").await, OfflineState::Cached);
    assert!(!wifi.local);
    assert!(!t.core.stream_local(&wifi.url));
    assert!(!t.core.stream_local("hocket-stream://unknown"));
    t.core.shutdown().await;
}
