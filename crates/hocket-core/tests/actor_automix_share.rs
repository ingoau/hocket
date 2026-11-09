//! AutoMix records shared through the coordinator, the client side
//! (`docs/automix.md` → *Sharing records*): a full core attached to a small
//! fake coordinator (a real `Room` on the in-memory network that says it
//! keeps records) fetches the window's missing records in one batch, stores
//! what comes back, doesn't ask again for a while, and uploads the records
//! the platform hands it.

#![cfg(feature = "sim")]

use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::Mutex;

use hocket_core::api::*;
use hocket_core::automix::store::fingerprint_of;
use hocket_core::automix::ANALYSIS_VERSION;
use hocket_core::connect::room::{Room, RoomConfig, RoomInput, RoomOutput};
use hocket_core::connect::session_adapter::RealReducer;
use hocket_core::connect::transport::PeerIds;
use hocket_core::connect::wire::{AnalysisEntry, Msg, WireMessage};
use hocket_core::core::io::memory::MemoryNet;
use hocket_core::core::io::ConnectIo;
use hocket_core::core::test_support::{seeded_server, TestCore};
use hocket_core::sim::SimTime;

#[derive(Default)]
struct Fake {
    /// Served to fetches.
    records: HashMap<String, AnalysisRecord>,
    fetches: Vec<Vec<String>>,
    uploads: Vec<AnalysisEntry>,
}

type Shared = Arc<Mutex<Fake>>;

/// A coordinator with an analysis store on the in-memory network; its URL.
async fn fake_coordinator(net: &Arc<MemoryNet>, clock: Arc<SimTime>, fake: Shared) -> String {
    let io = net.io("coordinator", true);
    let mut listener = io.listen(Arc::new(PeerIds::default())).await.unwrap();
    let url = MemoryNet::coordinator_url_for(listener.port());
    let room: Arc<Mutex<Option<Room>>> = Arc::default();
    let senders: Arc<Mutex<HashMap<String, tokio::sync::mpsc::UnboundedSender<WireMessage>>>> =
        Arc::default();
    tokio::spawn(async move {
        while let Some(mut conn) = listener.accept().await {
            let (room, senders, fake, clock) =
                (room.clone(), senders.clone(), fake.clone(), clock.clone());
            senders.lock().insert(conn.peer.clone(), conn.tx.clone());
            tokio::spawn(async move {
                let peer = conn.peer.clone();
                let perform = |outs: Vec<RoomOutput>| {
                    let senders = senders.lock();
                    for o in outs {
                        if let RoomOutput::Send(p, m) = o {
                            if let Some(tx) = senders.get(&p) {
                                let _ = tx.send(m);
                            }
                        }
                    }
                };
                while let Some(msg) = conn.rx.recv().await {
                    match msg.msg {
                        Msg::AnalysisFetch { track_ids } => {
                            let records = {
                                let mut f = fake.lock();
                                f.fetches.push(track_ids.clone());
                                track_ids
                                    .iter()
                                    .filter_map(|id| {
                                        f.records.get(id).map(|r| AnalysisEntry {
                                            track_id: id.clone(),
                                            record: r.clone(),
                                        })
                                    })
                                    .collect()
                            };
                            let _ = conn
                                .tx
                                .send(WireMessage::new(Msg::AnalysisRecords { records }));
                        }
                        Msg::AnalysisUpload { records } => fake.lock().uploads.extend(records),
                        other => {
                            let outs = {
                                let mut room = room.lock();
                                if room.is_none() {
                                    if let Msg::Hello { scope, .. } = &other {
                                        let mut cfg = RoomConfig::new(scope.clone());
                                        cfg.analysis_store = true;
                                        *room = Some(Room::new(
                                            cfg,
                                            clock.clone(),
                                            RealReducer::shared(),
                                            None,
                                        ));
                                    }
                                }
                                let Some(r) = room.as_mut() else { continue };
                                let mut outs = vec![];
                                if matches!(other, Msg::Hello { .. }) {
                                    outs.extend(r.handle(RoomInput::Connected(peer.clone())));
                                }
                                outs.extend(r.handle(RoomInput::Message(
                                    peer.clone(),
                                    WireMessage::new(other),
                                )));
                                outs
                            };
                            perform(outs);
                        }
                    }
                }
            });
        }
    });
    url
}

/// A record for the library's `id` (200 s tracks).
fn record(t: &TestCore, id: &str, producer: AnalysisProducer, beats: bool) -> AnalysisRecord {
    let track = t.open_db().track(id).unwrap().unwrap();
    AnalysisRecord {
        version: ANALYSIS_VERSION,
        analyser: "test".into(),
        produced_by: producer,
        source: fingerprint_of(&track),
        duration_ms: track.duration_ms,
        audible_end_ms: 190_000,
        bpm: beats.then_some(120.0),
        beats_ms: if beats {
            (0..380).map(|i| i * 500).collect()
        } else {
            vec![]
        },
        beat_confidence: if beats { 0.9 } else { 0.0 },
        created_at: 1.0,
        ..Default::default()
    }
}

async fn stored(t: &TestCore, id: &str) -> Option<AnalysisRecord> {
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
async fn the_window_is_fetched_once_and_own_records_are_uploaded() {
    let net = MemoryNet::new();
    let clock = SimTime::new(1_700_000_000_000.0);
    let t = TestCore::start_on(
        "share",
        seeded_server(8, 200.0),
        Some(net.clone()),
        1,
        clock.clone(),
    )
    .await;
    t.wait_until(30_000.0, |t| {
        t.events
            .all()
            .iter()
            .any(|e| matches!(e, Event::SyncProgress { progress } if progress.finished))
    })
    .await;
    let fake: Shared = Arc::default();
    fake.lock().records.insert(
        "t1".into(),
        record(&t, "t1", AnalysisProducer::Worker, true),
    );

    // A record the platform made before any coordinator was there waits for one.
    t.run(Command::StoreAnalysisRecord {
        track_id: "t6".into(),
        record: Box::new(record(&t, "t6", AnalysisProducer::Device, false)),
    })
    .await;

    t.run(Command::SetTransitionSettings {
        settings: TransitionSettings {
            mode: TransitionMode::AutoMix,
            crossfade_seconds: 6.0,
        },
    })
    .await;
    let url = fake_coordinator(&net, clock.clone(), fake.clone()).await;
    t.run(Command::SetCoordinatorUrl { url: Some(url) }).await;
    t.run(Command::PlayTracks {
        server_id: t.server_id.clone(),
        track_ids: (0..8).map(|i| format!("t{i}")).collect(),
        start_index: 0,
        label: "Sel".into(),
        shuffle: false,
        start_position_ms: 0,
    })
    .await;
    t.wait_until(20_000.0, |_| !fake.lock().fetches.is_empty())
        .await;
    t.run_for(3_000.0).await;

    // One batch: the current track and the four after it.
    let fetches = fake.lock().fetches.clone();
    assert_eq!(fetches.len(), 1, "{fetches:?}");
    assert_eq!(fetches[0], vec!["t0", "t1", "t2", "t3", "t4"]);
    // The answer landed in the local store.
    let got = stored(&t, "t1").await.expect("fetched record stored");
    assert_eq!(got.produced_by, AnalysisProducer::Worker);
    // The queued upload went out once the coordinator said it keeps records.
    let uploads = fake.lock().uploads.clone();
    assert_eq!(uploads.len(), 1, "{uploads:?}");
    assert_eq!(uploads[0].track_id, "t6");

    // A record handed over now goes straight up; fetched ones never do.
    t.run(Command::StoreAnalysisRecord {
        track_id: "t5".into(),
        record: Box::new(record(&t, "t5", AnalysisProducer::Device, false)),
    })
    .await;
    t.run_for(1_000.0).await;
    let uploaded: Vec<String> = fake
        .lock()
        .uploads
        .iter()
        .map(|e| e.track_id.clone())
        .collect();
    assert_eq!(uploaded, vec!["t6", "t5"]);

    // Moving on asks only for what entered the window.
    t.run(Command::Next).await;
    t.run_for(3_000.0).await;
    let fetches = fake.lock().fetches.clone();
    assert_eq!(fetches.len(), 2, "{fetches:?}");
    assert_eq!(fetches[1], vec!["t5"]);
    t.run_for(10_000.0).await;
    assert_eq!(fake.lock().fetches.len(), 2, "misses are remembered");
}
