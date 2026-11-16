//! Listening history is shared between devices: a play on one shows in the
//! others' history and stats, a play made offline arrives once the devices
//! reach each other again, and nothing is counted or scrobbled twice.

#![cfg(feature = "sim")]

use std::sync::Arc;

use hocket_core::api::*;
use hocket_core::core::io::memory::MemoryNet;
use hocket_core::core::test_support::{seeded_server, TestCore};
use hocket_core::sim::SimTime;

/// Two cores on a LAN, paired and synced.
async fn pair() -> (TestCore, TestCore, Arc<MemoryNet>) {
    let server = seeded_server(6, 200.0);
    let net = MemoryNet::new();
    let clock = SimTime::new(1_700_000_000_000.0);
    let a = TestCore::start_on("a", server.clone(), Some(net.clone()), 1, clock.clone()).await;
    let b = TestCore::start_on("b", server.clone(), Some(net.clone()), 2, clock.clone()).await;
    TestCore::run_all_for(&[&a, &b], 8_000.0).await;
    let (ca, cb) = (a.snapshot().await.connection, b.snapshot().await.connection);
    assert!(ca.connected && cb.connected, "{ca:?} {cb:?}");
    (a, b, net)
}

async fn history(t: &TestCore) -> Vec<PlayHistoryEntry> {
    match t.query(Query::RecentlyPlayed { limit: 50 }).await {
        QueryResult::History(h) => h,
        other => panic!("{other:?}"),
    }
}

async fn stats(t: &TestCore) -> ListeningStats {
    match t.query(Query::Stats { period_days: 7 }).await {
        QueryResult::Stats(s) => s,
        other => panic!("{other:?}"),
    }
}

fn local_play_count(t: &TestCore, track: &str) -> i64 {
    t.open_db()
        .with_conn(|c| {
            Ok(c.query_row(
                "SELECT local_play_count FROM tracks WHERE id = ?1",
                [track],
                |r| r.get(0),
            )?)
        })
        .unwrap()
}

fn submissions(t: &TestCore, id: &str) -> usize {
    t.server
        .scrobbles()
        .iter()
        .filter(|s| s.submission && s.id == id)
        .count()
}

/// `t` plays `track` from the start, well past its scrobble threshold (the
/// tracks are 200 s; the threshold is 100 s), then pauses.
async fn play_through(t: &TestCore, others: &[&TestCore], track: &str) {
    t.run(Command::PlayTracks {
        tracks: TrackRef::list(&t.server_id, [track]),
        start_index: 0,
        label: "Sel".into(),
        shuffle: false,
        start_position_ms: 0,
    })
    .await;
    let mut all = vec![t];
    all.extend_from_slice(others);
    TestCore::run_all_for(&all, 2_000.0).await;
    assert!(t.backend.is_playing(), "{} plays {track}", t.device_id);
    TestCore::run_all_for(&all, 110_000.0).await;
    t.run(Command::Pause).await;
    TestCore::run_all_for(&all, 2_000.0).await;
}

/// Shut `t` down and start it again on the same data directory.
async fn restart_on(t: TestCore, name: &str, net: Option<Arc<MemoryNet>>, seed: u64) -> TestCore {
    t.core.shutdown().await;
    let TestCore {
        clock, server, dir, ..
    } = t;
    TestCore::start_in(name, server, net, seed, clock, dir).await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_play_on_one_device_shows_in_the_others_history_and_stats() {
    let (a, b, _net) = pair().await;
    play_through(&a, &[&b], "t0").await;
    TestCore::run_all_for(&[&a, &b], 5_000.0).await;

    let ha = history(&a).await;
    assert_eq!(ha.len(), 1, "{ha:?}");
    let hb = history(&b).await;
    assert_eq!(hb.len(), 1, "b sees a's play once: {hb:?}");
    assert_eq!(hb[0].track.id, "t0");
    assert_eq!(
        hb[0].device_id, a.device_id,
        "attributed to the device that played it"
    );
    assert_eq!(hb[0].played_at, ha[0].played_at, "the same play");
    assert_eq!(stats(&b).await.total_plays, 1);
    assert_eq!(local_play_count(&b, "t0"), 1);
    assert_eq!(
        submissions(&a, "t0"),
        1,
        "scrobbled once, by the device that played it"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_play_made_offline_arrives_when_the_devices_meet_again_and_only_once() {
    let (a, b, net) = pair().await;
    // a goes offline (no network at all) and plays.
    let a = restart_on(a, "a", None, 1).await;
    TestCore::run_all_for(&[&a, &b], 5_000.0).await;
    play_through(&a, &[&b], "t1").await;
    TestCore::run_all_for(&[&a, &b], 5_000.0).await;
    assert_eq!(history(&a).await.len(), 1);
    assert!(history(&b).await.is_empty(), "not shared while offline");

    // Back on the network: the queued play is delivered.
    let a = restart_on(a, "a", Some(net.clone()), 1).await;
    TestCore::run_all_for(&[&a, &b], 30_000.0).await;
    let hb = history(&b).await;
    assert_eq!(hb.len(), 1, "delivered after the partition heals: {hb:?}");
    assert_eq!(hb[0].track.id, "t1");
    assert_eq!(hb[0].device_id, a.device_id);
    assert_eq!(local_play_count(&b, "t1"), 1);
    // The receiving device never scrobbles another device's play.
    assert_eq!(submissions(&a, "t1"), 1);

    // b plays too: a learns it, and neither counts anything twice.
    play_through(&b, &[&a], "t2").await;
    TestCore::run_all_for(&[&a, &b], 5_000.0).await;
    let ha = history(&a).await;
    let hb = history(&b).await;
    assert_eq!(ha.len(), 2, "{ha:?}");
    assert_eq!(hb.len(), 2, "{hb:?}");
    assert_eq!(stats(&a).await.total_plays, 2);
    assert_eq!(stats(&b).await.total_plays, 2);

    // A restart re-reads the log from its mark: still no duplicates.
    let b = restart_on(b, "b", Some(net.clone()), 2).await;
    TestCore::run_all_for(&[&a, &b], 5_000.0).await;
    assert_eq!(history(&b).await.len(), 2);
    assert_eq!(local_play_count(&b, "t1"), 1);
}
