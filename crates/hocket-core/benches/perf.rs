//! Isolated hot-path benches for the work that dominates both apps.
//!
//! Setup (document build, filter design, buffer allocation) lives outside
//! `iter`. Criterion reports mean ± CI; JSON sizes are also printed so a
//! time comparison is never mistaken for a payload comparison.
//!
//! ```text
//! cargo bench -p hocket-core --bench perf
//! cargo bench -p hocket-core --bench perf -- --save-baseline main
//! cargo bench -p hocket-core --bench perf -- --baseline main
//! ```

use std::time::Duration;

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use hocket_core::api::*;
use hocket_core::audio::dsp::biquad::{Biquad, BiquadKind, Coefficients};
use hocket_core::audio::dsp::eq::{preset, EqOptions, Equalizer};
use hocket_core::lyrics::LyricsCursor;
use hocket_core::session::{
    derive, new_document, reduce, DeterministicEntropy, QueueOp, ReduceCtx, SavedQueuePolicy,
    DEFAULT_HISTORY_CAP,
};

fn criterion_config() -> Criterion {
    Criterion::default()
        .warm_up_time(Duration::from_secs(1))
        .measurement_time(Duration::from_secs(3))
        .sample_size(60)
        .noise_threshold(0.03)
}

fn playing_doc(n: usize) -> SessionDocument {
    let e = DeterministicEntropy::new(1);
    let ctx = ReduceCtx {
        now: 1.0,
        position_ms: 0,
        history_cap: DEFAULT_HISTORY_CAP,
        saved: SavedQueuePolicy::default(),
        entropy: &e,
    };
    let context = QueueContext {
        server_id: "srv".into(),
        kind: ContextKind::Album { id: "a".into() },
        label: "Album".into(),
        sort: SortOrder::Default,
        tracks: (0..n).map(|i| format!("t{i:05}")).collect(),
    };
    reduce(
        &new_document("s", "id".into(), 0.0),
        QueueOp::PlayContext {
            args: PlayContextArgs {
                context,
                start_index: Some(0),
                shuffle: false,
                save_outgoing: true,
            },
        },
        &ctx,
    )
    .unwrap()
    .0
}

fn resolve(id: &TrackId) -> TrackSummary {
    TrackSummary {
        id: id.clone(),
        title: id.clone(),
        artist: Some("Artist".into()),
        album: Some("Album".into()),
        duration_ms: 180_000,
        ..Default::default()
    }
}

fn queue_event(doc: &SessionDocument, window: bool) -> Event {
    let mut derived = derive(doc);
    if window {
        derived.window_for_view();
    }
    Event::QueueChanged {
        queue: derived.into_view(resolve),
    }
}

fn queue_changed(c: &mut Criterion) {
    println!("QueueChanged JSON payload sizes (bytes, realistic summaries):");
    for n in [100usize, 1_000, 10_000] {
        let doc = playing_doc(n);
        let full = serde_json::to_vec(&queue_event(&doc, false)).unwrap();
        let win = serde_json::to_vec(&queue_event(&doc, true)).unwrap();
        println!(
            "  n={n:>5}  full={:>9} B  windowed={:>8} B  ratio={:.1}×",
            full.len(),
            win.len(),
            full.len() as f64 / win.len() as f64
        );
    }

    let mut group = c.benchmark_group("queue_changed_json");
    for n in [100usize, 1_000, 10_000] {
        let doc = playing_doc(n);
        let full_event = queue_event(&doc, false);
        let win_event = queue_event(&doc, true);
        let full_bytes = serde_json::to_vec(&full_event).unwrap().len() as u64;
        let win_bytes = serde_json::to_vec(&win_event).unwrap().len() as u64;

        // What the actor actually does on QueueChanged: derive + window + summaries + JSON.
        group.throughput(Throughput::Bytes(full_bytes));
        group.bench_with_input(BenchmarkId::new("emit_full", n), &n, |b, _| {
            b.iter(|| black_box(serde_json::to_string(&queue_event(&doc, false)).unwrap()));
        });
        group.throughput(Throughput::Bytes(win_bytes));
        group.bench_with_input(BenchmarkId::new("emit_windowed", n), &n, |b, _| {
            b.iter(|| black_box(serde_json::to_string(&queue_event(&doc, true)).unwrap()));
        });

        // Serialize-only of an already-built view: isolates JSON from derive.
        group.throughput(Throughput::Bytes(full_bytes));
        group.bench_with_input(BenchmarkId::new("serialize_full", n), &n, |b, _| {
            b.iter(|| black_box(serde_json::to_string(&full_event).unwrap()));
        });
        group.throughput(Throughput::Bytes(win_bytes));
        group.bench_with_input(BenchmarkId::new("serialize_windowed", n), &n, |b, _| {
            b.iter(|| black_box(serde_json::to_string(&win_event).unwrap()));
        });
    }
    group.finish();
}

fn lyrics_doc(lines: usize, syllable: bool) -> Lyrics {
    Lyrics {
        track_id: "t".into(),
        tier: if syllable {
            LyricsTier::Syllable
        } else {
            LyricsTier::Line
        },
        lang: None,
        display_artist: None,
        display_title: None,
        agents: vec![],
        lines: (0..lines)
            .map(|i| {
                let start = (i as u32) * 500;
                LyricLine {
                    start_ms: Some(start),
                    end_ms: Some(start + 480),
                    text: format!("line {i}"),
                    syllables: if syllable {
                        (0..4)
                            .map(|s| {
                                let s0 = start + s * 110;
                                LyricSyllable {
                                    text: "w".into(),
                                    start_ms: s0,
                                    end_ms: s0 + 100,
                                    joined: false,
                                }
                            })
                            .collect()
                    } else {
                        vec![]
                    },
                    agent: None,
                    background: false,
                    translation: None,
                }
            })
            .collect(),
        source: LyricsSource::Server,
        offset_ms: 0,
    }
}

fn lyrics_cursor(c: &mut Criterion) {
    let mut group = c.benchmark_group("lyrics_cursor");
    // One locate per sample: the 30 fps renderer path (reused cursor, +33 ms)
    // versus a cold seek (new cursor, mid-track).
    for n in [50usize, 200, 800] {
        let lyrics = lyrics_doc(n, false);
        let mid = (n as f64) * 250.0;
        group.throughput(Throughput::Elements(1));
        group.bench_with_input(BenchmarkId::new("playing_forward", n), &n, |b, _| {
            let mut cursor = LyricsCursor::new();
            let mut pos = 10.0_f64;
            let max = n as f64 * 500.0;
            b.iter(|| {
                pos += 33.0;
                if pos > max {
                    pos = 10.0;
                    cursor.reset();
                }
                black_box(cursor.locate(&lyrics, pos).line)
            });
        });
        group.bench_with_input(BenchmarkId::new("cold_mid", n), &n, |b, _| {
            b.iter(|| {
                let mut cursor = LyricsCursor::new();
                black_box(cursor.locate(&lyrics, mid).line)
            });
        });
    }
    let syllables = lyrics_doc(200, true);
    group.bench_function("playing_forward_syllable_200", |b| {
        let mut cursor = LyricsCursor::new();
        let mut pos = 10.0_f64;
        b.iter(|| {
            pos += 33.0;
            if pos > 100_000.0 {
                pos = 10.0;
                cursor.reset();
            }
            black_box(cursor.locate(&syllables, pos).syllable)
        });
    });
    group.finish();
}

fn dsp(c: &mut Criterion) {
    const RATE: f64 = 48_000.0;
    const CALLBACK_FRAMES: usize = 512; // ~10.67 ms at 48 kHz
    const SECOND_FRAMES: usize = 48_000;
    let coeffs = Coefficients::design(BiquadKind::Peaking, RATE, 1000.0, 1.0, 6.0);

    let mut group = c.benchmark_group("dsp");
    group.throughput(Throughput::Elements((CALLBACK_FRAMES * 2) as u64));
    group.bench_function("peaking_stereo_512", |b| {
        let mut bq = Biquad::new(coeffs, 2);
        let mut x = vec![0.1f32; CALLBACK_FRAMES * 2];
        b.iter(|| {
            bq.process_interleaved(&mut x);
            black_box(x[0])
        });
    });

    group.throughput(Throughput::Elements((SECOND_FRAMES * 2) as u64));
    group.bench_function("peaking_stereo_1s", |b| {
        let mut bq = Biquad::new(coeffs, 2);
        let mut x = vec![0.1f32; SECOND_FRAMES * 2];
        b.iter(|| {
            bq.process_interleaved(&mut x);
            black_box(x[0])
        });
    });

    let settings = preset("rock").expect("rock preset");
    group.throughput(Throughput::Elements((CALLBACK_FRAMES * 2) as u64));
    group.bench_function("eq_rock_stereo_512", |b| {
        let mut eq = Equalizer::new(&settings, RATE, 2, EqOptions::default());
        let mut x = vec![0.1f32; CALLBACK_FRAMES * 2];
        b.iter(|| {
            eq.process(&mut x);
            black_box(x[0])
        });
    });
    group.finish();
}

criterion_group! {
    name = benches;
    config = criterion_config();
    targets = queue_changed, lyrics_cursor, dsp
}
criterion_main!(benches);
