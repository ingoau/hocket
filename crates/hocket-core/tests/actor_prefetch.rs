//! Background prefetch of the next two queue items into the stream cache,
//! on the transport owner only, gated by battery, network and budget, and
//! following queue changes.

#![cfg(feature = "sim")]

mod stream_common;

use hocket_core::api::*;
use hocket_core::core::io::memory::MemoryNet;
use hocket_core::core::test_support::{seeded_server, TestCore, UpstreamBehaviour};
use hocket_core::sim::SimTime;
use hocket_core::subsonic::fake::FakeServer;
use stream_common::*;

async fn started(name: &str, server: FakeServer) -> TestCore {
    let t = TestCore::start(name, server).await;
    core_stream(&t).await;
    t
}

async fn play_album(t: &TestCore, n: usize, start: u32) {
    t.run(Command::PlayTracks {
        server_id: t.server_id.clone(),
        track_ids: (0..n).map(|i| format!("t{i}")).collect(),
        start_index: start,
        label: "Album".into(),
        shuffle: false,
    })
    .await;
}

/// Tick virtual time in steps, letting the runtime work between ticks.
async fn run_real(t: &TestCore, ms: f64) {
    let steps = (ms / 250.0).ceil() as u32;
    for _ in 0..steps {
        t.run_for(250.0).await;
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
}

async fn wait_cached(t: &TestCore, id: &str) {
    for _ in 0..200 {
        if offline(t, id).await == OfflineState::Cached {
            return;
        }
        run_real(t, 250.0).await;
    }
    panic!("{id} was never prefetched: {:?}", t.upstream.calls());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_next_two_items_are_prefetched_and_play_from_disk() {
    let t = started("next2", seeded_server(5, 100.0)).await;
    play_album(&t, 5, 0).await;
    // Nothing before the debounce.
    t.run_for(1000.0).await;
    assert!(t.upstream.calls().is_empty(), "{:?}", t.upstream.calls());
    wait_cached(&t, "t1").await;
    wait_cached(&t, "t2").await;
    run_real(&t, 3000.0).await;
    assert_eq!(offline(&t, "t3").await, OfflineState::None);
    assert_eq!(t.upstream.stream_requests("t3"), 0);
    assert_eq!(
        t.upstream.stream_requests("t0"),
        0,
        "the current item is the player's"
    );
    assert!(temp_files(&t).is_empty());
    // The queue shows them cached.
    let q = t.queue().await;
    let up: Vec<_> = q
        .upcoming
        .iter()
        .map(|e| (e.track.id.clone(), e.track.offline))
        .collect();
    assert_eq!(up[0], ("t1".into(), OfflineState::Cached), "{up:?}");
    assert_eq!(up[1], ("t2".into(), OfflineState::Cached), "{up:?}");

    // Advance: t1 plays from disk (no new request), and t3 is fetched.
    t.run(Command::Next).await;
    t.run_for(500.0).await;
    assert_eq!(t.current_track_id().await.as_deref(), Some("t1"));
    let loaded = t
        .sources
        .lock()
        .iter()
        .rev()
        .find(|s| s.track.id == "t1")
        .cloned()
        .unwrap();
    read_from(&t, &loaded.url, 0).await.unwrap();
    assert_eq!(t.upstream.stream_requests("t1"), 1, "only the prefetch");
    wait_cached(&t, "t3").await;
    run_real(&t, 3000.0).await;
    assert_eq!(t.upstream.stream_requests("t2"), 1);
    assert_eq!(t.upstream.stream_requests("t4"), 0);
    t.core.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_queue_change_cancels_a_stale_prefetch() {
    let t = started("stale", seeded_server(6, 100.0)).await;
    t.upstream.set_media("t1", vec![3u8; 300_000], "audio/flac");
    t.upstream
        .set_behaviour("t1", UpstreamBehaviour::StallAfter(50_000));
    play_album(&t, 3, 0).await;
    for _ in 0..100 {
        if t.core.stream_open_handles() > 0 && t.upstream.stream_requests("t1") > 0 {
            break;
        }
        run_real(&t, 250.0).await;
    }
    assert_eq!(
        t.core.stream_open_handles(),
        1,
        "t1's prefetch is in flight"
    );
    // Another context: t1 is no longer among the next two.
    t.run(Command::PlayTracks {
        server_id: t.server_id.clone(),
        track_ids: vec!["t3".into(), "t4".into(), "t5".into()],
        start_index: 0,
        label: "Other".into(),
        shuffle: false,
    })
    .await;
    wait_cached(&t, "t4").await;
    wait_cached(&t, "t5").await;
    wait_handles_closed(&t).await;
    // The cancelled fetch keeps what it got as a partial entry.
    assert_eq!(offline(&t, "t1").await, OfflineState::None);
    let row = row_of(&t, "t1");
    assert_eq!((row.complete, row.spans.as_str()), (false, "0-50000"));
    assert_eq!(
        t.upstream.stream_requests("t2"),
        0,
        "the old window was abandoned"
    );
    t.core.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn battery_saver_with_pause_prefetch_stops_prefetch() {
    let t = started("battery", seeded_server(4, 100.0)).await;
    t.run(Command::SetBatterySaver { enabled: true }).await;
    play_album(&t, 4, 0).await;
    run_real(&t, 5000.0).await;
    assert!(t.upstream.calls().is_empty(), "{:?}", t.upstream.calls());
    // `battery.pausePrefetch` off lets it run even in battery saver.
    t.run(Command::SetSetting {
        key: "battery.pausePrefetch".into(),
        value: "false".into(),
    })
    .await;
    wait_cached(&t, "t1").await;
    t.core.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_metered_network_prevents_prefetch_unless_allowed() {
    let t = started("metered", seeded_server(4, 100.0)).await;
    t.run(Command::SetNetworkState {
        state: NetworkState {
            kind: NetworkKind::Wifi,
            metered: true,
            network_id: None,
        },
    })
    .await;
    play_album(&t, 4, 0).await;
    run_real(&t, 5000.0).await;
    assert!(t.upstream.calls().is_empty(), "{:?}", t.upstream.calls());
    t.run(Command::SetNetworkState {
        state: NetworkState {
            kind: NetworkKind::Offline,
            metered: false,
            network_id: None,
        },
    })
    .await;
    t.run(Command::SetSetting {
        key: "storage.prefetchOnMobileData".into(),
        value: "true".into(),
    })
    .await;
    run_real(&t, 5000.0).await;
    assert!(t.upstream.calls().is_empty(), "offline: nothing");
    t.run(Command::SetNetworkState {
        state: NetworkState {
            kind: NetworkKind::Wifi,
            metered: true,
            network_id: None,
        },
    })
    .await;
    wait_cached(&t, "t1").await;
    wait_cached(&t, "t2").await;
    t.core.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn prefetch_stays_within_a_quarter_of_the_budget() {
    // 10 MiB tracks against a 64 MiB budget: a quarter (16 MiB) fits one.
    let server = FakeServer::new("srv", "alice");
    for i in 0..4 {
        let mut c = FakeServer::song(&format!("t{i}"), &format!("Track {i}"), "al0", "ar0", 100.0);
        c.track = Some(i + 1);
        c.size = Some(10.0 * 1024.0 * 1024.0);
        server.add_song(c);
    }
    let t = started("budget", server).await;
    for i in 1..4 {
        t.upstream
            .set_media(&format!("t{i}"), vec![5u8; 10 * 1024 * 1024], "audio/flac");
    }
    t.run(Command::SetSetting {
        key: "storage.cacheMaxBytes".into(),
        value: (64 * 1024 * 1024).to_string(),
    })
    .await;
    play_album(&t, 4, 0).await;
    wait_cached(&t, "t1").await;
    run_real(&t, 5000.0).await;
    assert_eq!(
        t.upstream.stream_requests("t2"),
        0,
        "{:?}",
        t.upstream.calls()
    );
    assert_eq!(offline(&t, "t2").await, OfflineState::None);
    t.core.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn only_the_transport_owner_prefetches_and_a_handoff_moves_it() {
    let server = seeded_server(6, 200.0);
    let net = MemoryNet::new();
    let clock = SimTime::new(1_700_000_000_000.0);
    let a = TestCore::start_on("a", server.clone(), Some(net.clone()), 1, clock.clone()).await;
    let b = TestCore::start_on("b", server.clone(), Some(net.clone()), 2, clock.clone()).await;
    TestCore::run_all_for(&[&a, &b], 8_000.0).await;
    for c in [&a, &b] {
        c.run(Command::SetBackendCapabilities { core_stream: true })
            .await;
    }
    // b's second prefetch stalls, so it is in flight at the handoff.
    b.upstream
        .set_behaviour("t2", UpstreamBehaviour::StallAfter(20_000));
    b.run(Command::PlayTracks {
        server_id: b.server_id.clone(),
        track_ids: (0..6).map(|i| format!("t{i}")).collect(),
        start_index: 0,
        label: "Album".into(),
        shuffle: false,
    })
    .await;
    let mut in_flight = false;
    for _ in 0..80 {
        TestCore::run_all_for(&[&a, &b], 250.0).await;
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        if offline(&b, "t1").await == OfflineState::Cached
            && b.upstream.stream_requests("t2") > 0
            && b.core.stream_open_handles() > 0
        {
            in_flight = true;
            break;
        }
    }
    assert!(in_flight, "b prefetches: {:?}", b.upstream.calls());
    assert!(b.backend.is_playing());
    assert!(a.upstream.calls().is_empty(), "a only watches");

    b.run(Command::OpenHandoffPicker).await;
    TestCore::run_all_for(&[&a, &b], 2_000.0).await;
    b.run(Command::HandoffTo {
        device_id: a.device_id.clone(),
    })
    .await;
    let mut moved = false;
    for _ in 0..120 {
        TestCore::run_all_for(&[&a, &b], 250.0).await;
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        if a.upstream.stream_requests("t2") > 0 && b.core.stream_open_handles() == 0 {
            moved = true;
            break;
        }
    }
    assert!(a.backend.is_playing(), "a took over");
    assert!(
        moved,
        "a: {:?} b handles: {}",
        a.upstream.calls(),
        b.core.stream_open_handles()
    );
    assert_eq!(offline(&b, "t2").await, OfflineState::None);
    let b_calls = b.upstream.calls().len();
    TestCore::run_all_for(&[&a, &b], 5_000.0).await;
    assert_eq!(b.upstream.calls().len(), b_calls, "b stopped prefetching");
    a.core.shutdown().await;
    b.core.shutdown().await;
}
