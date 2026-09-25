//! The album primer: the first seconds of an album's first track are
//! fetched into the stream cache (a partial span) on the device that owns
//! playback, only on an unmetered network and outside battery saver,
//! deduplicated and rate-limited; pressing play then starts from disk.

#![cfg(feature = "sim")]

mod stream_common;

use hocket_core::api::*;
use hocket_core::core::io::memory::MemoryNet;
use hocket_core::core::test_support::{seeded_server, TestCore};
use hocket_core::sim::SimTime;
use stream_common::*;

/// 256 KiB: the floor (the fake songs claim 1000 bytes over 100 s).
const PRIMED: u64 = 256 * 1024;

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
    t.run(network(NetworkKind::Wifi, false)).await;
    t
}

/// Real time for the runtime to work, virtual time ticking.
async fn run_real(t: &TestCore, ms: f64) {
    let steps = (ms / 250.0).ceil() as u32;
    for _ in 0..steps {
        t.run_for(250.0).await;
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
}

fn primed(t: &TestCore, id: &str) -> i64 {
    let db = t.open_db();
    db.with_conn(|c| {
        use rusqlite::OptionalExtension;
        Ok(c.query_row(
            "SELECT primed FROM cache_signals WHERE track_id = ?1",
            [id],
            |r| r.get(0),
        )
        .optional()?
        .unwrap_or(0))
    })
    .unwrap()
}

async fn wait_requests(t: &TestCore, id: &str, n: usize) {
    for _ in 0..200 {
        if t.upstream.stream_requests(id) >= n {
            wait_handles_closed(t).await;
            return;
        }
        run_real(t, 250.0).await;
    }
    panic!("{id}: {:?}", t.upstream.calls());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_local_prime_fetches_the_first_seconds_and_play_starts_from_disk() {
    let t = started("prime-local", 8).await;
    let media: Vec<u8> = (0..3_000_000u32).map(|i| (i % 233) as u8).collect();
    t.upstream.set_media("t0", media.clone(), "audio/flac");
    t.run(Command::PrimeAlbum {
        album_id: "al0".into(),
    })
    .await;
    wait_requests(&t, "t0", 1).await;
    let calls = t.upstream.calls();
    assert_eq!(calls.len(), 1, "{calls:?}");
    assert_eq!(calls[0].range.as_deref(), Some("bytes=0-262143"));
    let row = row_of(&t, "t0");
    assert_eq!(
        (row.complete, row.spans.as_str(), row.total),
        (false, "0-262144", Some(3_000_000.0))
    );
    assert_eq!(offline(&t, "t0").await, OfflineState::None);
    assert_eq!(primed(&t, "t0"), 1, "evicted first until played");
    // Asked again: already primed, nothing fetched.
    t.run(Command::PrimeAlbum {
        album_id: "al0".into(),
    })
    .await;
    t.run(Command::PrimeTrack {
        track_id: "t0".into(),
    })
    .await;
    run_real(&t, 2000.0).await;
    assert_eq!(t.upstream.calls().len(), 1);

    // Pressing play: the first span comes from disk, no request...
    let s = source(&t, "t0").await;
    let info = t.core.stream_open(&s.url, 0, None).await.unwrap();
    assert_eq!(info.total_length, Some(3_000_000));
    let mut out = vec![];
    while (out.len() as u64) < PRIMED {
        let want = (PRIMED as usize - out.len()).min(64 * 1024);
        out.extend_from_slice(&t.core.stream_read(info.handle, want).await.unwrap());
    }
    assert_eq!(
        t.upstream.calls().len(),
        1,
        "the primed span came from disk"
    );
    // ...and the rest is fetched from where it ends.
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
    assert_eq!(calls[1].range.as_deref(), Some("bytes=262144-"));
    wait_offline(&t, "t0", OfflineState::Cached).await;
    // Playing it clears the primed mark.
    t.run(Command::PlayTracks {
        server_id: t.server_id.clone(),
        track_ids: vec!["t0".into()],
        start_index: 0,
        label: "One".into(),
        shuffle: false,
    })
    .await;
    assert_eq!(primed(&t, "t0"), 0);
    t.core.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn priming_needs_an_unmetered_network_and_no_battery_saver() {
    let t = started("prime-gating", 8).await;
    for (cmd, why) in [
        (network(NetworkKind::Wifi, true), "metered wifi"),
        (network(NetworkKind::Cellular, false), "cellular"),
        (network(NetworkKind::Offline, false), "offline"),
    ] {
        t.run(cmd).await;
        // No mobile-data override for the primer.
        t.run(Command::SetSetting {
            key: "storage.prefetchOnMobileData".into(),
            value: "true".into(),
        })
        .await;
        t.run(Command::PrimeTrack {
            track_id: "t0".into(),
        })
        .await;
        run_real(&t, 1000.0).await;
        assert!(
            t.upstream.calls().is_empty(),
            "{why}: {:?}",
            t.upstream.calls()
        );
    }
    t.run(network(NetworkKind::Wifi, false)).await;
    t.run(Command::SetBatterySaver { enabled: true }).await;
    t.run(Command::SetSetting {
        key: "battery.pausePrefetch".into(),
        value: "false".into(),
    })
    .await;
    t.run(Command::PrimeTrack {
        track_id: "t0".into(),
    })
    .await;
    run_real(&t, 1000.0).await;
    assert!(t.upstream.calls().is_empty(), "battery saver");
    t.run(Command::SetBatterySaver { enabled: false }).await;
    t.run(Command::PrimeTrack {
        track_id: "t0".into(),
    })
    .await;
    wait_requests(&t, "t0", 1).await;
    t.core.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn one_prime_at_a_time_newest_waiting_wins_and_the_rate_is_limited() {
    let t = started("prime-rate", 30).await;
    // t1's body is held: it stays in flight.
    t.upstream.hold("t1");
    t.run(Command::PrimeTrack {
        track_id: "t1".into(),
    })
    .await;
    for _ in 0..100 {
        if t.upstream.stream_requests("t1") > 0 {
            break;
        }
        run_real(&t, 250.0).await;
    }
    assert_eq!(t.upstream.stream_requests("t1"), 1);
    // Two more while it runs: the newer replaces the older unstarted one.
    for id in ["t2", "t3"] {
        t.run(Command::PrimeTrack {
            track_id: id.into(),
        })
        .await;
    }
    run_real(&t, 1000.0).await;
    assert_eq!(t.upstream.calls().len(), 1, "one at a time");
    t.upstream.release("t1");
    wait_requests(&t, "t3", 1).await;
    run_real(&t, 1000.0).await;
    assert_eq!(
        t.upstream.stream_requests("t2"),
        0,
        "replaced before it started"
    );
    // Two started so far; eighteen more fill the window of twenty.
    for i in 4..22 {
        let id = format!("t{i}");
        t.run(Command::PrimeTrack {
            track_id: id.clone(),
        })
        .await;
        wait_requests(&t, &id, 1).await;
    }
    t.run(Command::PrimeTrack {
        track_id: "t22".into(),
    })
    .await;
    run_real(&t, 2000.0).await;
    assert_eq!(t.upstream.stream_requests("t22"), 0, "rate-limited");
    // Ten minutes on, it may prime again.
    t.clock.advance(10.0 * 60_000.0);
    t.run(Command::PrimeTrack {
        track_id: "t22".into(),
    })
    .await;
    wait_requests(&t, "t22", 1).await;
    t.core.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_device_that_owns_playback_primes_with_its_own_state() {
    let server = seeded_server(8, 200.0);
    let net = MemoryNet::new();
    let clock = SimTime::new(1_700_000_000_000.0);
    let a = TestCore::start_on("a", server.clone(), Some(net.clone()), 1, clock.clone()).await;
    let b = TestCore::start_on("b", server.clone(), Some(net.clone()), 2, clock.clone()).await;
    TestCore::run_all_for(&[&a, &b], 8_000.0).await;
    for c in [&a, &b] {
        c.run(Command::SetBackendCapabilities { core_stream: true })
            .await;
        c.run(network(NetworkKind::Wifi, false)).await;
    }
    // b plays: it owns transport.
    b.run(Command::PlayTracks {
        server_id: b.server_id.clone(),
        track_ids: vec!["t0".into()],
        start_index: 0,
        label: "One".into(),
        shuffle: false,
    })
    .await;
    TestCore::run_all_for(&[&a, &b], 2_000.0).await;
    assert!(b.backend.is_playing());
    // a's album page asks; b primes the album's first track.
    a.run(Command::PrimeAlbum {
        album_id: "al1".into(),
    })
    .await;
    let mut primed = false;
    for _ in 0..80 {
        TestCore::run_all_for(&[&a, &b], 250.0).await;
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        if b.upstream.stream_requests("t4") > 0 {
            primed = true;
            break;
        }
    }
    assert!(primed, "b: {:?}", b.upstream.calls());
    // a only fetches what b is playing (the watcher's prefetch).
    assert!(
        a.upstream.calls().iter().all(|c| c.track_id == "t0"),
        "a does not prime: {:?}",
        a.upstream.calls()
    );
    assert_eq!(
        b.upstream
            .calls()
            .iter()
            .find(|c| c.track_id == "t4")
            .unwrap()
            .range
            .as_deref(),
        Some("bytes=0-262143"),
        "only the first seconds"
    );
    // b is on a metered network now: it declines, and a does not step in.
    b.run(network(NetworkKind::Wifi, true)).await;
    a.run(Command::PrimeTrack {
        track_id: "t5".into(),
    })
    .await;
    for _ in 0..20 {
        TestCore::run_all_for(&[&a, &b], 250.0).await;
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
    assert_eq!(b.upstream.stream_requests("t5"), 0);
    assert_eq!(a.upstream.stream_requests("t5"), 0);
    a.core.shutdown().await;
    b.core.shutdown().await;
}
