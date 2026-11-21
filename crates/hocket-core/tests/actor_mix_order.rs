//! AutoMix's order (`docs/automix.md` → *Order*): with AutoMix on and a
//! shuffled queue, the entry after the next one is the candidate that blends
//! best with it; an in-order queue is never touched.

#![cfg(feature = "sim")]

use hocket_core::api::*;
use hocket_core::automix::store::fingerprint_of;
use hocket_core::automix::ANALYSIS_VERSION;
use hocket_core::core::test_support::{seeded_server, TestCore};

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
        let id = format!("t{n}");
        let track = t.open_db().track(&id).unwrap().unwrap();
        let beat = 60_000.0 / bpm_of(&id);
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
            bpm: Some(bpm_of(&id) as f32),
            beats_ms: beats,
            downbeats_ms: downbeats,
            beat_confidence: 0.9,
            ..Default::default()
        };
        t.run(Command::StoreAnalysisRecord {
            track_id: id.as_str().into(),
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
    t.run(Command::PlayTracks {
        tracks: TrackRef::list(&t.server_id, (0..8).map(|n| format!("t{n}"))),
        start_index: 0,
        label: "Sel".into(),
        shuffle,
        start_position_ms: 0,
    })
    .await;
    t.run_for(500.0).await;
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
