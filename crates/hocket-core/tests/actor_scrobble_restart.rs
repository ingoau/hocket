//! A play that reached its scrobble threshold while the session could not
//! judge it yet survives a restart: it is asked about again, recorded once
//! and submitted at most once (connect/mod.rs rules 8 and 9).

#![cfg(feature = "sim")]

use std::sync::Arc;

use hocket_core::api::*;
use hocket_core::core::io::memory::MemoryNet;
use hocket_core::core::test_support::{seeded_server, TestCore};
use hocket_core::sim::SimTime;

fn submissions(t: &TestCore, id: &str) -> usize {
    t.server
        .scrobbles()
        .iter()
        .filter(|s| s.submission && s.id == id)
        .count()
}

async fn history(t: &TestCore) -> Vec<PlayHistoryEntry> {
    match t.query(Query::RecentlyPlayed { limit: 50 }).await {
        QueryResult::History(h) => h,
        other => panic!("{other:?}"),
    }
}

/// The persisted connect state (`connect:<scope>`) of a core's database.
fn connect_state(t: &TestCore) -> (String, serde_json::Value) {
    let db = t.open_db();
    let key = db
        .saved_state_keys("connect:")
        .unwrap()
        .into_iter()
        .next()
        .expect("connect state persisted");
    let json = db.saved_state_get_raw(&key).unwrap().unwrap();
    (key, serde_json::from_str(&json).unwrap())
}

fn awaiting(state: &serde_json::Value) -> Vec<serde_json::Value> {
    state["knownScrobbled"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter(|k| k.get("awaitingSince").is_some())
                .cloned()
                .collect()
        })
        .unwrap_or_default()
}

struct Deferred {
    a: TestCore,
    b: TestCore,
    net: Arc<MemoryNet>,
    started: f64,
}

/// a starts t0 and hands it to b; the LAN then partitions (both still see
/// each other's adverts), so when b reaches the threshold of this *shared*
/// play its verdict waits for a room both can reach.
async fn deferred_on_b() -> Deferred {
    let server = seeded_server(6, 200.0);
    let net = MemoryNet::new_partitionable();
    let clock = SimTime::new(1_700_000_000_000.0);
    let a = TestCore::start_on("a", server.clone(), Some(net.clone()), 1, clock.clone()).await;
    let b = TestCore::start_on("b", server.clone(), Some(net.clone()), 2, clock.clone()).await;
    TestCore::run_all_for(&[&a, &b], 8_000.0).await;
    let (ca, cb) = (a.snapshot().await.connection, b.snapshot().await.connection);
    assert!(ca.connected && cb.connected, "{ca:?} {cb:?}");

    let started = clock.now_ms();
    a.run(Command::PlayTracks {
        server_id: a.server_id.clone(),
        track_ids: vec!["t0".into(), "t1".into()],
        start_index: 0,
        label: "Sel".into(),
        shuffle: false,
    })
    .await;
    TestCore::run_all_for(&[&a, &b], 30_000.0).await;
    a.run(Command::OpenHandoffPicker).await;
    TestCore::run_all_for(&[&a, &b], 2_000.0).await;
    a.run(Command::HandoffTo {
        device_id: b.device_id.clone(),
    })
    .await;
    TestCore::run_all_for(&[&a, &b], 2_000.0).await;
    assert!(b.backend.is_playing(), "b took over");

    net.set_partitioned(true);
    // 200 s track: the threshold (100 s) passes about 66 s from here.
    TestCore::run_all_for(&[&a, &b], 80_000.0).await;
    assert_eq!(submissions(&b, "t0"), 0, "the shared play's verdict waits");
    assert!(
        history(&b).await.iter().all(|h| h.track.id != "t0"),
        "not recorded before its verdict"
    );
    // The wait is persisted as it starts (not only when the session
    // document next changes): a crash now still resumes it.
    let (_, state) = connect_state(&b);
    let waits = awaiting(&state);
    assert_eq!(waits.len(), 1, "{state}");
    assert_eq!(waits[0]["trackId"], "t0");
    Deferred { a, b, net, started }
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
async fn a_deferred_scrobble_survives_a_restart_and_is_submitted_once() {
    let Deferred { a, b, net, started } = deferred_on_b().await;
    let b = restart_on(b, "b", Some(net.clone()), 2).await;
    // Still partitioned: the restarted device keeps waiting (it is not
    // judged alone just because it restarted).
    TestCore::run_all_for(&[&a, &b], 10_000.0).await;
    assert_eq!(submissions(&b, "t0"), 0);

    net.set_partitioned(false);
    TestCore::run_all_for(&[&a, &b], 20_000.0).await;
    let subs: Vec<_> = b
        .server
        .scrobbles()
        .into_iter()
        .filter(|s| s.submission && s.id == "t0")
        .collect();
    assert_eq!(subs.len(), 1, "submitted exactly once: {subs:?}");
    let time = subs[0].time_ms.expect("a submission carries its time");
    assert!(
        (time - started).abs() < 1_000.0,
        "submitted with the play's original start {started}, got {time}"
    );
    let hb: Vec<_> = history(&b)
        .await
        .into_iter()
        .filter(|h| h.track.id == "t0")
        .collect();
    assert_eq!(hb.len(), 1, "{hb:?}");
    assert!(hb[0].played_ms >= 100_000, "{hb:?}");
    assert!(history(&a).await.iter().all(|h| h.track.id != "t0"));

    // Nothing left to ask about: another restart changes nothing.
    TestCore::run_all_for(&[&a, &b], 5_000.0).await;
    let b = restart_on(b, "b", Some(net.clone()), 2).await;
    TestCore::run_all_for(&[&a, &b], 10_000.0).await;
    assert_eq!(submissions(&b, "t0"), 1);
    assert_eq!(
        history(&b)
            .await
            .iter()
            .filter(|h| h.track.id == "t0")
            .count(),
        1
    );
    assert!(awaiting(&connect_state(&b).1).is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_deferred_scrobble_found_duplicate_after_a_restart_is_recorded_not_submitted() {
    let Deferred { a, b, .. } = deferred_on_b().await;
    b.core.shutdown().await;
    // While b was down, it turns out a scrobbled this very play: b's
    // persisted dedupe log learns it (as it would have from the room).
    let (key, mut state) = connect_state(&b);
    let wait = awaiting(&state).remove(0);
    let mut known = wait.clone();
    known.as_object_mut().unwrap().remove("awaitingSince");
    known["deviceId"] = serde_json::Value::String(a.device_id.clone());
    state["knownScrobbled"].as_array_mut().unwrap().push(known);
    b.open_db()
        .saved_state_set_raw(&key, &state.to_string(), &*b.clock)
        .unwrap();
    drop(a);

    let TestCore {
        clock, server, dir, ..
    } = b;
    let b = TestCore::start_in("b", server, None, 2, clock, dir).await;
    b.run_for(10_000.0).await;
    assert_eq!(submissions(&b, "t0"), 0, "a duplicate is never submitted");
    let hb: Vec<_> = history(&b)
        .await
        .into_iter()
        .filter(|h| h.track.id == "t0")
        .collect();
    assert_eq!(hb.len(), 1, "still a local play, once: {hb:?}");
    assert!(hb[0].scrobbled, "scrobbled by the other device");

    let b = restart_on(b, "b", None, 2).await;
    b.run_for(10_000.0).await;
    assert_eq!(submissions(&b, "t0"), 0);
    assert_eq!(
        history(&b)
            .await
            .iter()
            .filter(|h| h.track.id == "t0")
            .count(),
        1
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_single_device_play_is_submitted_at_its_threshold_and_leaves_nothing_pending() {
    let t = TestCore::start("solo", seeded_server(4, 200.0)).await;
    t.run_for(8_000.0).await;
    t.run(Command::PlayTracks {
        server_id: t.server_id.clone(),
        track_ids: vec!["t0".into(), "t1".into()],
        start_index: 0,
        label: "Sel".into(),
        shuffle: false,
    })
    .await;
    t.run_for(99_000.0).await;
    assert_eq!(submissions(&t, "t0"), 0);
    // The verdict needs no wait: submitted within moments of the threshold.
    t.run_for(3_000.0).await;
    assert_eq!(submissions(&t, "t0"), 1);
    assert!(awaiting(&connect_state(&t).1).is_empty());
    let t = restart_on(t, "solo", None, 1).await;
    t.run_for(10_000.0).await;
    assert_eq!(submissions(&t, "t0"), 1);
    assert_eq!(
        history(&t)
            .await
            .iter()
            .filter(|h| h.track.id == "t0")
            .count(),
        1
    );
}
