//! AutoMix's order (`docs/automix.md` → *Order*): with AutoMix on and a
//! shuffled queue, the entry after the next one is the candidate that blends
//! best with it; an in-order queue is never touched, and skipping through
//! tracks, back and forth, never moves anything.

#![cfg(feature = "sim")]

use hocket_core::api::*;
use hocket_core::automix::order::SETTLE_MS;
use hocket_core::automix::store::fingerprint_of;
use hocket_core::automix::ANALYSIS_VERSION;
use hocket_core::core::io::memory::MemoryNet;
use hocket_core::core::test_support::{seeded_server, TestCore};
use hocket_core::sim::SimTime;

async fn synced_core(name: &str) -> TestCore {
    let t = TestCore::start(name, seeded_server(8, 200.0)).await;
    t.wait_until(30_000.0, |t| {
        t.events
            .all()
            .iter()
            .any(|e| matches!(e, Event::SyncProgress { progress } if progress.finished))
    })
    .await;
    t
}

/// Even tracks at 120 BPM, odd ones at 150: a pair of the same tempo blends,
/// a pair across tempos only fades.
fn bpm_of(id: &str) -> f64 {
    let n: u32 = id.trim_start_matches('t').parse().unwrap();
    if n % 2 == 0 {
        120.0
    } else {
        150.0
    }
}

async fn store_records(t: &TestCore) {
    for n in 0..8 {
        store_record(t, &format!("t{n}")).await;
    }
}

async fn store_record(t: &TestCore, id: &str) {
    {
        let track = t.open_db().track(&id).unwrap().unwrap();
        let beat = 60_000.0 / bpm_of(id);
        let mut beats = vec![];
        let mut downbeats = vec![];
        let mut at = 250.0;
        let mut i = 0;
        while at < f64::from(track.duration_ms) - 1_000.0 {
            beats.push(at.round() as Ms);
            if i % 4 == 0 {
                downbeats.push(at.round() as Ms);
            }
            at += beat;
            i += 1;
        }
        let record = AnalysisRecord {
            version: ANALYSIS_VERSION,
            analyser: "test".into(),
            produced_by: AnalysisProducer::Desktop,
            source: fingerprint_of(&track),
            duration_ms: track.duration_ms,
            audible_end_ms: track.duration_ms - 1_000,
            bpm: Some(bpm_of(id) as f32),
            beats_ms: beats,
            downbeats_ms: downbeats,
            beat_confidence: 0.9,
            ..Default::default()
        };
        t.run(Command::StoreAnalysisRecord {
            track_id: id.into(),
            record: Box::new(record),
        })
        .await;
    }
}

async fn start(t: &TestCore, shuffle: bool) {
    t.run(Command::SetTransitionSettings {
        settings: TransitionSettings {
            mode: TransitionMode::AutoMix,
            crossfade_seconds: 6.0,
        },
    })
    .await;
    store_records(t).await;
    play_all(t, shuffle).await;
    t.run_for(SETTLED).await;
}

/// Long enough for the current track to settle and the order to be decided.
const SETTLED: f64 = SETTLE_MS as f64 + 2_000.0;

async fn play_all(t: &TestCore, shuffle: bool) {
    t.run(Command::PlayTracks {
        tracks: TrackRef::list(&t.server_id, (0..8).map(|n| format!("t{n}"))),
        start_index: 0,
        label: "Sel".into(),
        shuffle,
        start_position_ms: 0,
    })
    .await;
}

async fn upcoming(t: &TestCore) -> Vec<String> {
    t.snapshot()
        .await
        .session
        .expect("a session")
        .upcoming
        .iter()
        .map(|i| i.track.id.to_string())
        .collect()
}

/// The whole list, history to the end: what navigation must never change.
async fn whole_list(t: &TestCore) -> Vec<String> {
    let doc = t.snapshot().await.session.expect("a session");
    doc.history
        .iter()
        .chain(doc.current.as_ref())
        .chain(doc.upcoming.iter())
        .map(|i| i.track.id.to_string())
        .collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn nothing_moves_before_the_current_track_settles() {
    let t = synced_core("unsettled").await;
    t.run(Command::SetTransitionSettings {
        settings: TransitionSettings {
            mode: TransitionMode::AutoMix,
            crossfade_seconds: 6.0,
        },
    })
    .await;
    store_records(&t).await;
    play_all(&t, true).await;
    t.run_for(500.0).await;
    let shuffled = whole_list(&t).await;
    t.run_for(f64::from(SETTLE_MS) - 3_000.0).await;
    assert_eq!(whole_list(&t).await, shuffled, "not before it settles");
}

/// Skipping through tracks, forward and back, leaves the list as it was.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn skipping_back_and_forth_never_reorders() {
    let t = synced_core("skips").await;
    start(&t, true).await;
    let list = whole_list(&t).await;
    for cmd in [
        Command::Next,
        Command::Next,
        Command::Next,
        Command::PreviousTrack,
        Command::PreviousTrack,
        Command::Next,
        Command::PreviousTrack,
        Command::PreviousTrack,
    ] {
        t.run(cmd).await;
        t.run_for(1_500.0).await;
        assert_eq!(whole_list(&t).await, list);
    }
}

/// An entry's follow-up is decided once: going back to the track before it
/// and letting it settle again doesn't move anything, even when the records
/// that arrived since would now choose differently.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_revisited_track_keeps_the_order_it_had() {
    let t = synced_core("revisit").await;
    t.run(Command::SetTransitionSettings {
        settings: TransitionSettings {
            mode: TransitionMode::AutoMix,
            crossfade_seconds: 6.0,
        },
    })
    .await;
    play_all(&t, true).await;
    t.run(Command::Next).await;
    // Only the next track's record: its follow-up is decided blind (kept).
    let next = upcoming(&t).await[0].clone();
    store_record(&t, &next).await;
    t.run_for(SETTLED).await;
    store_records(&t).await;
    t.run(Command::Next).await;
    t.run_for(SETTLED).await;
    let list = whole_list(&t).await;
    let here = t.current_track_id().await;
    t.run(Command::PreviousTrack).await;
    t.run_for(SETTLED).await;
    assert_ne!(t.current_track_id().await, here, "went back");
    assert_eq!(whole_list(&t).await, list, "back a track");
    t.run(Command::Next).await;
    t.run_for(SETTLED).await;
    assert_eq!(t.current_track_id().await, here);
    assert_eq!(whole_list(&t).await, list, "and forward again");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn shuffle_puts_a_blendable_track_after_the_next() {
    let t = synced_core("order").await;
    start(&t, true).await;
    let up = upcoming(&t).await;
    assert_eq!(up.len(), 7, "{up:?}");
    // Every other candidate shares the next track's tempo: one of them follows it.
    assert_eq!(bpm_of(&up[0]), bpm_of(&up[1]), "{up:?}");
    // And the preloaded next track's mix-out is planned against it: a blend.
    t.run_for(200.0).await;
    let next = t.backend.next_source().expect("preloaded");
    assert_eq!(next.track.id.to_string(), up[0]);
    assert!(next.mix_out_ms.is_some());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_in_order_queue_is_left_alone() {
    let t = synced_core("inorder").await;
    start(&t, false).await;
    assert_eq!(
        upcoming(&t).await,
        ["t1", "t2", "t3", "t4", "t5", "t6", "t7"]
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_users_group_is_never_reordered() {
    let t = synced_core("group").await;
    start(&t, true).await;
    let before = upcoming(&t).await;
    // Queue two songs that don't blend with each other (150 → 120 BPM).
    let (a, b) = if bpm_of(&before[0]) == 120.0 {
        (
            before[0].clone(),
            before.iter().find(|i| bpm_of(i) == 150.0).unwrap().clone(),
        )
    } else {
        (
            before[0].clone(),
            before.iter().find(|i| bpm_of(i) == 120.0).unwrap().clone(),
        )
    };
    t.run(Command::PlayNext {
        tracks: TrackRef::list(&t.server_id, [a.clone(), b.clone()]),
        keys: vec![],
    })
    .await;
    t.run_for(500.0).await;
    let up = upcoming(&t).await;
    assert_eq!(up[..2], [a, b], "{up:?}");
}

/// Over a room: the promotion follows the op that changed the next entry, so
/// the room takes both, every peer sees the same order and the skips stand.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn promotions_reach_the_room_after_the_skip_that_asked_for_them() {
    let server = seeded_server(8, 200.0);
    let net = MemoryNet::new();
    let clock = SimTime::new(1_700_000_000_000.0);
    let room = TestCore::start_on("a", server.clone(), Some(net.clone()), 1, clock.clone()).await;
    let a = TestCore::start_on("b", server.clone(), Some(net.clone()), 2, clock.clone()).await;
    TestCore::run_all_for(&[&room, &a], 8_000.0).await;
    assert!(a.snapshot().await.connection.connected);
    a.run(Command::SetAutoplay { enabled: false }).await;
    a.run(Command::SetTransitionSettings {
        settings: TransitionSettings {
            mode: TransitionMode::AutoMix,
            crossfade_seconds: 6.0,
        },
    })
    .await;
    store_records(&a).await;
    play_all(&a, true).await;
    TestCore::run_all_for(&[&room, &a], SETTLED).await;
    for _ in 0..3 {
        let before = upcoming(&a).await;
        a.run(Command::Next).await;
        TestCore::run_all_for(&[&room, &a], SETTLED).await;
        let cur = a.current_track_id().await.expect("playing");
        assert_eq!(cur, before[0], "the skip stood");
        let up = upcoming(&a).await;
        if up.len() >= 3 && up[1..].iter().any(|t| bpm_of(t) == bpm_of(&up[0])) {
            assert_eq!(bpm_of(&up[0]), bpm_of(&up[1]), "{up:?}");
        }
        assert_eq!(
            upcoming(&room).await,
            up,
            "the room's peer has the same order"
        );
    }
    a.core.shutdown().await;
    room.core.shutdown().await;
}
