//! Offline skip marks are not failures: items skipped because this device
//! was offline (neither downloaded nor cached) play again once the network
//! returns, on this device and its peers, while a real load failure keeps
//! its mark.

#![cfg(feature = "sim")]

mod stream_common;

use hocket_core::api::*;
use hocket_core::core::io::memory::MemoryNet;
use hocket_core::core::test_support::{seeded_server, TestCore};
use hocket_core::sim::SimTime;
use stream_common::*;

fn network(kind: NetworkKind) -> Command {
    Command::SetNetworkState {
        state: NetworkState {
            kind,
            // Metered: no background prefetch.
            metered: true,
            network_id: None,
        },
    }
}

async fn started(name: &str) -> TestCore {
    let t = TestCore::start(name, seeded_server(6, 100.0)).await;
    core_stream(&t).await;
    t.run(network(NetworkKind::Wifi)).await;
    t.run(Command::SetAutoplay { enabled: false }).await;
    t
}

/// `(track, marked)` for every stored item, in queue order.
async fn marks(t: &TestCore) -> Vec<(String, bool)> {
    let q = t.queue().await;
    q.history
        .iter()
        .chain(q.current.iter())
        .chain(q.playing_next.iter())
        .map(|e| (e.track.id.clone(), e.item.unavailable))
        .collect()
}

fn marked(m: &[(String, bool)]) -> Vec<&str> {
    m.iter()
        .filter(|(_, u)| *u)
        .map(|(id, _)| id.as_str())
        .collect()
}

async fn key_of(t: &TestCore, id: &str) -> QueueKey {
    let q = t.queue().await;
    q.history
        .iter()
        .chain(q.current.iter())
        .chain(q.playing_next.iter())
        .find(|e| e.track.id == id)
        .map(|e| e.item.key.clone())
        .unwrap_or_else(|| panic!("{id} not in the queue"))
}

/// Play `first`, then queue `later` (insertions keep their marks, unlike
/// derived context items).
async fn queue(t: &TestCore, first: &str, later: &[&str]) {
    t.run(Command::PlayTracks {
        server_id: t.server_id.clone(),
        track_ids: vec![first.into()],
        start_index: 0,
        label: "Mixed".into(),
        shuffle: false,
    })
    .await;
    t.run(Command::PlayLater {
        server_id: t.server_id.clone(),
        track_ids: later.iter().map(|s| s.to_string()).collect(),
    })
    .await;
    t.run_for(1000.0).await;
}

fn undo_len(s: &Snapshot) -> usize {
    s.undo.history.len()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn offline_skips_are_cleared_when_the_network_returns() {
    let t = started("offline-skips-clear").await;
    t.run(Command::Pin {
        target: PinTarget::Track { id: "t0".into() },
        transcode: false,
    })
    .await;
    wait_offline(&t, "t0", OfflineState::Downloaded).await;
    read_from(&t, &source(&t, "t2").await.url, 0).await.unwrap();
    wait_offline(&t, "t2", OfflineState::Cached).await;

    // [downloaded, not cached, cached, not cached], offline.
    t.run(network(NetworkKind::Offline)).await;
    queue(&t, "t0", &["t1", "t2", "t3"]).await;
    assert_eq!(t.current_track_id().await.as_deref(), Some("t0"));
    t.run(Command::Next).await;
    t.run_for(1000.0).await;
    assert_eq!(t.current_track_id().await.as_deref(), Some("t2"));
    assert!(t.backend.is_playing());
    t.run(Command::Next).await;
    t.run_for(1000.0).await;
    let m = marks(&t).await;
    assert_eq!(marked(&m), ["t1", "t3"], "{m:?}");
    // An offline skip is not reported as a failure.
    assert!(!t.events.all().iter().any(
        |e| matches!(e, Event::PlayerNotice { message: Some(m), .. } if m.starts_with("Couldn't play"))
    ));
    // Announced by its stable code, once.
    let offline_notices = t
        .events
        .all()
        .iter()
        .filter(|e| {
            matches!(
                e,
                Event::PlayerNotice {
                    code: Some(PlayerNoticeCode::OfflineSkipping),
                    ..
                }
            )
        })
        .count();
    assert!(offline_notices >= 1);

    // Back online: the marks go, without an undo entry of their own.
    let before = undo_len(&t.snapshot().await);
    t.run(network(NetworkKind::Wifi)).await;
    t.run_for(500.0).await;
    let m = marks(&t).await;
    assert!(marked(&m).is_empty(), "{m:?}");
    let snap = t.snapshot().await;
    assert_eq!(undo_len(&snap), before);
    let doc = snap.session.expect("a session");
    assert!(!doc.extra.contains_key("offlineSkipped"), "{:?}", doc.extra);

    // Undo restores a snapshot taken offline: its marks are cleared again.
    t.run(Command::Undo).await;
    t.run_for(500.0).await;
    assert_eq!(t.current_track_id().await.as_deref(), Some("t2"));
    let m = marks(&t).await;
    assert!(marked(&m).is_empty(), "{m:?}");

    // Next plays the not-cached track, from the server.
    let k0 = key_of(&t, "t0").await;
    t.run(Command::JumpToQueueItem { key: k0 }).await;
    t.run_for(500.0).await;
    t.sources.lock().clear();
    t.run(Command::Next).await;
    t.run_for(1000.0).await;
    assert_eq!(t.current_track_id().await.as_deref(), Some("t1"));
    let k1 = key_of(&t, "t1").await;
    assert_eq!(t.backend.current().map(|(k, _)| k), Some(k1));
    assert!(t.backend.is_playing());
    assert!(
        t.sources.lock().iter().any(|s| s.track.id == "t1"),
        "t1 loaded"
    );
    t.core.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_load_failure_stays_marked_across_an_offline_spell() {
    let t = started("offline-skips-failure").await;
    t.backend.fail_track("t1");
    queue(&t, "t0", &["t1", "t2"]).await;
    t.run(Command::Next).await;
    t.run_for(1000.0).await;
    assert_eq!(t.current_track_id().await.as_deref(), Some("t2"));
    assert_eq!(marked(&marks(&t).await), ["t1"]);
    t.backend.unfail_track("t1");

    // Offline, something else is skipped for being unavailable here.
    t.run(network(NetworkKind::Offline)).await;
    t.run(Command::PlayLater {
        server_id: t.server_id.clone(),
        track_ids: vec!["t3".into()],
    })
    .await;
    t.run(Command::Next).await;
    t.run_for(1000.0).await;
    assert_eq!(marked(&marks(&t).await), ["t1", "t3"]);

    t.run(network(NetworkKind::Wifi)).await;
    t.run_for(500.0).await;
    assert_eq!(
        marked(&marks(&t).await),
        ["t1"],
        "the failure keeps its mark"
    );
    t.core.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_clear_reaches_a_peer() {
    let server = seeded_server(6, 200.0);
    let net = MemoryNet::new();
    let clock = SimTime::new(1_700_000_000_000.0);
    let a = TestCore::start_on("a", server.clone(), Some(net.clone()), 1, clock.clone()).await;
    let b = TestCore::start_on("b", server.clone(), Some(net.clone()), 2, clock.clone()).await;
    TestCore::run_all_for(&[&a, &b], 8_000.0).await;
    assert!(a.snapshot().await.connection.connected);
    a.run(Command::SetAutoplay { enabled: false }).await;

    // Nothing is downloaded or cached on a: offline, everything is skipped.
    a.run(network(NetworkKind::Offline)).await;
    a.run(Command::PlayTracks {
        server_id: a.server_id.clone(),
        track_ids: vec!["t0".into(), "t1".into(), "t2".into()],
        start_index: 0,
        label: "Sel".into(),
        shuffle: false,
    })
    .await;
    TestCore::run_all_for(&[&a, &b], 3_000.0).await;
    let on_a = marks(&a).await;
    assert_eq!(marked(&on_a), ["t0", "t1", "t2"], "{on_a:?}");
    assert_eq!(marks(&b).await, on_a, "b follows the marks");

    a.run(network(NetworkKind::Wifi)).await;
    TestCore::run_all_for(&[&a, &b], 2_000.0).await;
    let on_a = marks(&a).await;
    assert!(marked(&on_a).is_empty(), "{on_a:?}");
    let on_b = marks(&b).await;
    assert_eq!(on_b, on_a, "the clear reached b");
    let doc = b.snapshot().await.session.expect("a session");
    assert!(!doc.extra.contains_key("offlineSkipped"));
    a.core.shutdown().await;
    b.core.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_repeating_queue_with_nothing_offline_stops_with_a_coded_notice() {
    let t = started("offline-nothing").await;
    t.run(network(NetworkKind::Offline)).await;
    t.run(Command::SetRepeat {
        mode: RepeatMode::All,
    })
    .await;
    t.run(Command::PlayTracks {
        server_id: t.server_id.clone(),
        track_ids: vec!["t0".into(), "t1".into()],
        start_index: 0,
        label: "Loop".into(),
        shuffle: false,
    })
    .await;
    t.run_for(2000.0).await;
    assert!(
        t.events.all().iter().any(|e| matches!(
            e,
            Event::PlayerNotice {
                code: Some(PlayerNoticeCode::NothingAvailableOffline),
                message: Some(_),
                detail: None
            }
        )),
        "{:?}",
        t.events
            .all()
            .iter()
            .filter(|e| matches!(e, Event::PlayerNotice { .. }))
            .collect::<Vec<_>>()
    );
    assert!(!t.backend.is_playing());
    t.core.shutdown().await;
}

/// The clear is a whole-document `Replace`: when a peer's op lands in the
/// room first, the room refuses it as stale. It is re-run once against the
/// new document instead of leaving the marks until the next reconnect. Run
/// with each device as the one that was offline, so one run has it
/// following the other's room.
#[tokio::test(flavor = "current_thread")]
async fn a_clear_refused_as_stale_is_run_again() {
    let mut peer_op_won = false;
    for offline_is_first in [true, false] {
        let server = seeded_server(6, 200.0);
        let net = MemoryNet::new();
        let clock = SimTime::new(1_700_000_000_000.0);
        let a = TestCore::start_on("a", server.clone(), Some(net.clone()), 1, clock.clone()).await;
        let b = TestCore::start_on("b", server.clone(), Some(net.clone()), 2, clock.clone()).await;
        TestCore::run_all_for(&[&a, &b], 8_000.0).await;
        assert!(a.snapshot().await.connection.connected);
        let (off, peer) = if offline_is_first { (&a, &b) } else { (&b, &a) };
        off.run(Command::SetAutoplay { enabled: false }).await;
        off.run(network(NetworkKind::Offline)).await;
        off.run(Command::PlayTracks {
            server_id: off.server_id.clone(),
            track_ids: vec!["t0".into(), "t1".into(), "t2".into()],
            start_index: 0,
            label: "Sel".into(),
            shuffle: false,
        })
        .await;
        TestCore::run_all_for(&[&a, &b], 3_000.0).await;
        assert_eq!(marked(&marks(off).await), ["t0", "t1", "t2"]);
        assert_eq!(marks(peer).await, marks(off).await);

        // The peer's op and the network's return race for the same revision.
        peer.dispatch(Command::PlayLater {
            server_id: peer.server_id.clone(),
            track_ids: vec!["t4".into()],
        });
        off.dispatch(network(NetworkKind::Wifi));
        // Well inside a lease heartbeat (which would also clear them, later).
        TestCore::run_all_for(&[&a, &b], 500.0).await;

        // Whichever op the room took first, the marks are gone everywhere.
        // (When the offline device serves the room, its clear wins and the
        // peer's stale op is the one refused.)
        for (name, t) in [("offline device", off), ("peer", peer)] {
            let m = marks(t).await;
            assert!(
                marked(&m).is_empty(),
                "{name} (offline_is_first={offline_is_first}): {m:?}"
            );
        }
        assert_eq!(marks(off).await, marks(peer).await);
        peer_op_won |= marks(off).await.iter().any(|(id, _)| id == "t4");
        a.core.shutdown().await;
        b.core.shutdown().await;
    }
    assert!(
        peer_op_won,
        "in one run the peer's op won the race, so the clear was refused and run again"
    );
}
