//! Pressing Next repeatedly: the UI state (now playing, queue) must follow
//! each press at once, and the backend must not start a stream for every
//! track skipped over on the way.

#![cfg(feature = "sim")]

use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::Mutex;

use hocket_core::api::*;
use hocket_core::audio::scripted::ScriptedCall;
use hocket_core::core::test_support::{seeded_server, TestCore};
use hocket_core::EventSink;

/// Every event as a platform sink sees it: serialised to JSON on the
/// actor thread, stamped with the wall time it arrived.
#[derive(Default)]
struct Timed {
    events: Mutex<Vec<(Instant, String, usize)>>,
}

impl EventSink for Timed {
    fn on_event(&self, event: Event) {
        let json = serde_json::to_string(&event).unwrap();
        // `{"type":"<kind>",...`: no second parse; the platforms' own
        // decode is not the core's cost.
        let kind = json
            .strip_prefix("{\"type\":\"")
            .and_then(|r| r.split('"').next())
            .unwrap_or_default()
            .to_string();
        self.events.lock().push((Instant::now(), kind, json.len()));
    }
}

async fn playing_core(name: &str, n: usize) -> TestCore {
    let t = TestCore::start(name, seeded_server(n, 200.0)).await;
    t.wait_until(60_000.0, |t| {
        t.events
            .all()
            .iter()
            .any(|e| matches!(e, Event::SyncProgress { progress } if progress.finished))
    })
    .await;
    let ids: Vec<String> = (0..n).map(|i| format!("t{i}")).collect();
    t.run(Command::PlayTracks {
        server_id: t.server_id.clone(),
        track_ids: ids,
        start_index: 0,
        label: "All".into(),
        shuffle: false,
    })
    .await;
    t.run_for(1000.0).await;
    t
}

fn loads(t: &TestCore) -> Vec<String> {
    t.backend
        .log()
        .iter()
        .filter_map(|c| match c {
            ScriptedCall::Load { key, .. } => Some(key.clone()),
            _ => None,
        })
        .collect()
}

/// Measure a single Next and a burst of Nexts on a queue of `n` tracks;
/// prints the numbers. Run with `--ignored --nocapture`.
async fn measure(n: usize) {
    let t = playing_core("m", n).await;
    let timed = Arc::new(Timed::default());
    t.core.add_sink(timed.clone());

    // One Next: dispatch to NowPlayingChanged / QueueChanged.
    let start = Instant::now();
    t.dispatch(Command::Next);
    t.core.settle().await;
    let ev = std::mem::take(&mut *timed.events.lock());
    let at = |k: &str| {
        ev.iter()
            .find(|(_, kind, _)| kind == k)
            .map(|(i, _, _)| i.duration_since(start))
    };
    let bytes: usize = ev.iter().map(|(_, _, b)| b).sum();
    println!(
        "n={n} single Next: nowPlaying {:?} queue {:?} backend-load-before-queue {} events {} bytes {} (last {:?})",
        at("nowPlayingChanged"),
        at("queueChanged"),
        ev.iter().position(|(_, k, _)| k == "mediaSession").unwrap_or(usize::MAX)
            < ev.iter().position(|(_, k, _)| k == "queueChanged").unwrap_or(0),
        ev.len(),
        bytes,
        ev.last().map(|(i, _, _)| i.duration_since(start)),
    );

    // A burst of 20, dispatched back to back.
    let before = loads(&t).len();
    let start = Instant::now();
    for _ in 0..20 {
        t.dispatch(Command::Next);
    }
    t.core.settle().await;
    let actor = start.elapsed();
    let ev = std::mem::take(&mut *timed.events.lock());
    let np: Vec<Duration> = ev
        .iter()
        .filter(|(_, k, _)| k == "nowPlayingChanged")
        .map(|(i, _, _)| i.duration_since(start))
        .collect();
    let bytes: usize = ev.iter().map(|(_, _, b)| b).sum();
    let mut kinds: std::collections::BTreeMap<&str, usize> = Default::default();
    for (_, k, _) in &ev {
        *kinds.entry(k.as_str()).or_default() += 1;
    }
    println!(
        "n={n} burst of 20: actor busy {actor:?}, first nowPlaying {:?}, last nowPlaying {:?}, backend loads {} (before settle window), events {} bytes {} {kinds:?}",
        np.first(),
        np.last(),
        loads(&t).len() - before,
        ev.len(),
        bytes,
    );
    t.run_for(1000.0).await;
    println!(
        "n={n} after 1 s: backend loads for the burst {}",
        loads(&t).len() - before
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "timing harness; run with --ignored --nocapture"]
async fn measure_next_latency() {
    for n in [8, 500, 3000] {
        measure(n).await;
    }
}

/// Records, for every now-playing change, how many loads the backend had
/// been given when the UI heard of it.
struct LoadsAtNowPlaying {
    backend: Arc<hocket_core::audio::scripted::ScriptedBackend>,
    seen: Mutex<Vec<(Option<String>, usize)>>,
}

impl EventSink for LoadsAtNowPlaying {
    fn on_event(&self, event: Event) {
        if let Event::NowPlayingChanged { entry } = event {
            let loads = self
                .backend
                .log()
                .iter()
                .filter(|c| matches!(c, ScriptedCall::Load { .. }))
                .count();
            self.seen.lock().push((entry.map(|e| e.track.id), loads));
        }
    }
}

fn backend_on_current(t: &TestCore, q: &QueueView) -> bool {
    t.backend.current().map(|(k, _)| k) == q.current.as_ref().map(|e| e.item.key.clone())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_single_next_reaches_the_ui_before_the_backend_loads() {
    let t = playing_core("single", 12).await;
    let watch = Arc::new(LoadsAtNowPlaying {
        backend: t.backend.clone(),
        seen: Mutex::new(vec![]),
    });
    t.core.add_sink(watch.clone());
    let before = loads(&t).len();
    t.run(Command::Next).await;
    // Not part of a burst: loaded at once, without waiting for a tick.
    assert_eq!(loads(&t).len(), before + 1);
    let seen = watch.seen.lock().clone();
    assert_eq!(seen, vec![(Some("t1".to_string()), before)]);
    assert!(backend_on_current(&t, &t.queue().await));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_burst_of_nexts_updates_the_ui_each_time_and_loads_where_it_lands() {
    let t = playing_core("burst", 12).await;
    let before = loads(&t).len();
    t.events.clear();
    for _ in 0..6 {
        t.dispatch(Command::Next);
    }
    t.core.settle().await;
    // Every press reached the UI at once.
    let now_playing: Vec<String> = t
        .events
        .all()
        .iter()
        .filter_map(|e| match e {
            Event::NowPlayingChanged { entry } => entry.as_ref().map(|e| e.track.id.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(now_playing, ["t1", "t2", "t3", "t4", "t5", "t6"]);
    let queue_changes = t
        .events
        .all()
        .iter()
        .filter(|e| matches!(e, Event::QueueChanged { .. }))
        .count();
    assert!(queue_changes >= 6);
    let q = t.queue().await;
    assert_eq!(q.current.as_ref().unwrap().track.id, "t6");
    // Only the first press loaded; the rest wait for the burst to settle,
    // with the skipped-from item paused and the new one shown as loading.
    assert_eq!(loads(&t).len(), before + 1);
    assert!(!t.backend.is_playing());
    let transport = t.transport().await;
    assert!(transport.buffering);
    assert!(transport.position.is_playing);
    assert_eq!(transport.position.position_ms, 0);

    t.run_for(250.0).await;
    let burst = loads(&t)[before..].to_vec();
    assert_eq!(burst.len(), 2, "the first press and where the burst landed");
    assert!(backend_on_current(&t, &t.queue().await));
    assert!(t.backend.is_playing());

    // Skipped-over tracks never count: no now-playing for t2..t5.
    t.run_for(5_000.0).await;
    let announced: Vec<String> = t
        .server
        .scrobbles()
        .iter()
        .filter(|s| !s.submission)
        .map(|s| s.id.clone())
        .collect();
    for skipped in ["t2", "t3", "t4", "t5"] {
        assert!(!announced.contains(&skipped.to_string()), "{announced:?}");
    }
    assert!(announced.contains(&"t6".to_string()), "{announced:?}");

    // A press after the burst settled loads at once again.
    let before = loads(&t).len();
    t.run(Command::Next).await;
    assert_eq!(loads(&t).len(), before + 1);
    assert!(backend_on_current(&t, &t.queue().await));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pausing_or_seeking_during_a_burst_is_kept_by_the_deferred_load() {
    let t = playing_core("pause", 12).await;
    t.dispatch(Command::Next);
    t.dispatch(Command::Next);
    t.dispatch(Command::SeekTo {
        position_ms: 30_000,
    });
    t.dispatch(Command::Pause);
    t.core.settle().await;
    let before = loads(&t).len();
    t.run_for(250.0).await;
    let last = t
        .backend
        .log()
        .iter()
        .rev()
        .find_map(|c| match c {
            ScriptedCall::Load {
                key,
                position_ms,
                play,
                ..
            } => Some((key.clone(), *position_ms, *play)),
            _ => None,
        })
        .unwrap();
    assert_eq!(loads(&t).len(), before + 1);
    let q = t.queue().await;
    assert_eq!(q.current.as_ref().unwrap().track.id, "t2");
    assert_eq!(last, (q.current.unwrap().item.key, 30_000, false));
    assert!(!t.backend.is_playing());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn previous_during_a_burst_goes_back_through_the_queue() {
    let t = playing_core("prev", 12).await;
    for _ in 0..4 {
        t.dispatch(Command::Next);
    }
    t.dispatch(Command::Previous);
    t.dispatch(Command::Previous);
    t.core.settle().await;
    t.run_for(250.0).await;
    let q = t.queue().await;
    assert_eq!(q.current.as_ref().unwrap().track.id, "t2");
    assert!(backend_on_current(&t, &q));
    assert!(t.backend.is_playing());
}
