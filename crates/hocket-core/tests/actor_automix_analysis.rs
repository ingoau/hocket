//! AutoMix analysis on a miss (`docs/automix.md` → *Platforms*, desktop):
//! with the mode on AutoMix, a track around the current one whose whole
//! file is in the stream cache is analysed in the background and its
//! record stored under the library's fingerprint; with the mode off,
//! nothing is analysed.

#![cfg(all(feature = "sim", feature = "automix-analysis"))]

mod stream_common;

use std::time::Duration;

use hocket_core::api::*;
use hocket_core::automix::store::fingerprint_of;
use hocket_core::core::test_support::{seeded_server, TestCore};
use stream_common::*;

/// A 20 s stereo 16-bit WAV of 120 bpm clicks.
fn click_wav() -> Vec<u8> {
    let rate = 44_100u32;
    let frames = rate * 20;
    let mut pcm = vec![0i16; frames as usize];
    let beat = (rate / 2) as usize;
    for (k, start) in (0..frames as usize).step_by(beat).enumerate() {
        let f = if k % 4 == 0 { 1_000.0 } else { 2_000.0 };
        for i in 0..(rate as usize / 20) {
            if start + i >= pcm.len() {
                break;
            }
            let env = (-(i as f64) / (0.01 * f64::from(rate))).exp();
            let s = 0.5 * env * (2.0 * std::f64::consts::PI * f * i as f64 / f64::from(rate)).sin();
            pcm[start + i] = (s * 32_767.0) as i16;
        }
    }
    let data_len = frames * 4;
    let mut out = Vec::with_capacity(44 + data_len as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&rate.to_le_bytes());
    out.extend_from_slice(&(rate * 4).to_le_bytes());
    out.extend_from_slice(&4u16.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    for s in pcm {
        out.extend_from_slice(&s.to_le_bytes());
        out.extend_from_slice(&s.to_le_bytes());
    }
    out
}

async fn set_mode(t: &TestCore, mode: TransitionMode) {
    t.run(Command::SetTransitionSettings {
        settings: TransitionSettings {
            mode,
            crossfade_seconds: 6.0,
        },
    })
    .await;
}

async fn record(t: &TestCore, id: &str) -> Option<AnalysisRecord> {
    match t
        .query(Query::TrackAnalysis {
            track_id: id.into(),
        })
        .await
    {
        QueryResult::Analysis(r) => r,
        other => panic!("{other:?}"),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_cached_follow_up_is_analysed_in_automix_mode_only() {
    let t = TestCore::start("analysis", seeded_server(3, 200.0)).await;
    core_stream(&t).await;
    // Metered: no prefetch, so only t1 is cached.
    t.run(Command::SetNetworkState {
        state: NetworkState {
            kind: NetworkKind::Wifi,
            metered: true,
            network_id: None,
        },
    })
    .await;
    t.upstream.set_media("t1", click_wav(), "audio/wav");
    let s = source(&t, "t1").await;
    read_from(&t, &s.url, 0).await.unwrap();
    wait_offline(&t, "t1", OfflineState::Cached).await;

    t.run(Command::PlayTracks {
        server_id: t.server_id.clone(),
        track_ids: vec!["t0".into(), "t1".into(), "t2".into()],
        start_index: 0,
        label: "Sel".into(),
        shuffle: false,
        start_position_ms: 0,
    })
    .await;
    set_mode(&t, TransitionMode::Crossfade).await;
    t.run_for(6_000.0).await;
    assert!(
        record(&t, "t1").await.is_none(),
        "not analysed outside AutoMix"
    );

    set_mode(&t, TransitionMode::AutoMix).await;
    let mut got = None;
    for _ in 0..600 {
        t.run_for(250.0).await;
        got = record(&t, "t1").await;
        if got.is_some() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let r = got.expect("t1 analysed within a minute");
    let track = t.open_db().track("t1").unwrap().unwrap();
    assert_eq!(r.produced_by, AnalysisProducer::Desktop);
    assert_eq!(
        r.source,
        fingerprint_of(&track),
        "the library's fingerprint"
    );
    assert!((r.bpm.unwrap() - 120.0).abs() < 2.0, "{:?}", r.bpm);
    // Not on disk: never analysed.
    assert!(record(&t, "t0").await.is_none());
    assert!(record(&t, "t2").await.is_none());
}
