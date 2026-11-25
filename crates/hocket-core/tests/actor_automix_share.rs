//! AutoMix records shared through the coordinator, the client side
//! (`docs/automix.md` → *Sharing records*): a full core attached to a small
//! fake coordinator (a real `Room` on the in-memory network that says it
//! keeps records) fetches the window's missing records in batches, stores
//! what comes back, doesn't ask again for a while, and uploads the records
//! the platform hands it.

#![cfg(feature = "sim")]

use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::Mutex;

use hocket_core::api::*;
use hocket_core::automix::store::fingerprint_of;
use hocket_core::automix::ANALYSIS_VERSION;
use hocket_core::connect::room::{Room, RoomAuth, RoomConfig, RoomInput, RoomOutput};
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
    /// The YouTube Music sessions signed in (opened with the coordinator's key).
    ytm_sessions: Vec<Option<String>>,
}

type Shared = Arc<Mutex<Fake>>;

/// A coordinator with an analysis store on the in-memory network; its URL.
async fn fake_coordinator(net: &Arc<MemoryNet>, clock: Arc<SimTime>, fake: Shared) -> String {
    let io = net.io("coordinator");
    let secret = hocket_core::identity::crypto::DeviceSecret::from_seed(b"fake-coordinator");
    let mut listener = io.listen(Arc::new(PeerIds::default())).await.unwrap();
    let url = MemoryNet::coordinator_url_for(listener.port());
    let room: Arc<Mutex<Option<Room>>> = Arc::default();
    let senders: Arc<Mutex<HashMap<String, tokio::sync::mpsc::UnboundedSender<WireMessage>>>> =
        Arc::default();
    tokio::spawn(async move {
        while let Some(mut conn) = listener.accept().await {
            let (room, senders, fake, clock, secret) = (
                room.clone(),
                senders.clone(),
                fake.clone(),
                clock.clone(),
                secret.clone(),
            );
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
                        Msg::AnalysisFetch { wants } => {
                            let records = {
                                let mut f = fake.lock();
                                f.fetches
                                    .push(wants.iter().map(|w| w.track_id.id.clone()).collect());
                                wants
                                    .iter()
                                    .filter_map(|w| {
                                        f.records.get(&w.track_id.id).map(|r| AnalysisEntry {
                                            track_id: w.track_id.clone(),
                                            key: w.key.clone(),
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
                        Msg::CoordYtmSignIn { envelope } => {
                            let opened = envelope.as_ref().map(|e| {
                                let plain = secret
                                    .open_envelope(hocket_core::connect::wire::COORD_YTM_CONTEXT, e)
                                    .expect("sealed to this coordinator");
                                String::from_utf8(plain).unwrap()
                            });
                            fake.lock().ytm_sessions.push(opened.clone());
                            let _ = conn.tx.send(WireMessage::new(Msg::CoordYtmStatus {
                                enabled: true,
                                signed_in: opened.is_some(),
                                account: opened.map(|_| "Test User".into()),
                                error: None,
                            }));
                        }
                        // The device claims it: its identity gets a room.
                        Msg::CoordClaim { sync, .. } => {
                            let mut cfg = RoomConfig::new(sync.identity().to_string());
                            cfg.analysis_store = true;
                            cfg.auth = RoomAuth::Identity {
                                device_id: hocket_core::identity::COORDINATOR_ID.into(),
                                secret: secret.clone(),
                            };
                            let mut r = Room::new(cfg, clock.clone(), RealReducer::shared(), None);
                            let _ = r.set_identity(*sync);
                            *room.lock() = Some(r);
                            let _ = conn.tx.send(WireMessage::new(Msg::CoordAdmitted {
                                keys: secret.public(),
                                owner: true,
                            }));
                        }
                        other => {
                            let outs = {
                                let mut room = room.lock();
                                let Some(r) = room.as_mut() else { continue };
                                let mut outs = vec![];
                                if matches!(other, Msg::Challenge { .. }) {
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
        seeded_server(24, 200.0),
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
    t.run(Command::ClaimCoordinator {
        url: url.clone(),
        code: "ANY".into(),
    })
    .await;
    t.wait_until(5_000.0, |t| {
        t.events
            .all()
            .iter()
            .any(|e| matches!(e, Event::CoordinatorJoined { .. }))
    })
    .await;
    t.run(Command::PlayTracks {
        tracks: TrackRef::list(&t.server_id, (0..24).map(|i| format!("t{i}"))),
        start_index: 0,
        label: "Sel".into(),
        shuffle: false,
        start_position_ms: 0,
    })
    .await;
    t.wait_until(20_000.0, |_| !fake.lock().fetches.is_empty())
        .await;
    t.run_for(3_000.0).await;

    // The current track and the sixteen after it (as far as the order
    // looks), nearest first, in batches of one answer frame each (t6's
    // loudness-only record from a phone still wants a better one).
    let fetches = fake.lock().fetches.clone();
    assert_eq!(fetches.len(), 2, "{fetches:?}");
    let first: Vec<String> = (0..16).map(|i| format!("t{i}")).collect();
    assert_eq!(fetches[0], first);
    assert_eq!(fetches[1], vec!["t16"]);
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
        .map(|e| e.track_id.id.clone())
        .collect();
    assert_eq!(uploaded, vec!["t6", "t5"]);

    // Moving on asks only for what entered the window.
    t.run(Command::Next).await;
    t.run_for(3_000.0).await;
    let fetches = fake.lock().fetches.clone();
    assert_eq!(fetches.len(), 3, "{fetches:?}");
    assert_eq!(fetches[2], vec!["t17"]);
    t.run_for(10_000.0).await;
    assert_eq!(fake.lock().fetches.len(), 3, "misses are remembered");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_owner_signs_the_coordinator_in_to_youtube_music_with_a_sealed_session() {
    let net = MemoryNet::new();
    let clock = SimTime::new(1_700_000_000_000.0);
    let t = TestCore::start_on(
        "coord-ytm",
        seeded_server(1, 200.0),
        Some(net.clone()),
        1,
        clock.clone(),
    )
    .await;
    let fake: Shared = Arc::default();
    // No coordinator yet: nothing to seal it to.
    t.run(Command::SignCoordinatorInToYoutubeMusic {
        cookies: Some(Secret("SAPISID=abc".into())),
    })
    .await;
    assert!(t
        .events
        .all()
        .iter()
        .any(|e| matches!(e, Event::CoordinatorFailed { .. })));

    let url = fake_coordinator(&net, clock.clone(), fake.clone()).await;
    t.run(Command::ClaimCoordinator {
        url,
        code: "ANY".into(),
    })
    .await;
    t.wait_until(5_000.0, |t| {
        t.events
            .all()
            .iter()
            .any(|e| matches!(e, Event::CoordinatorJoined { .. }))
    })
    .await;
    // Attached to it (requests to it need the connection).
    t.wait_until(5_000.0, |t| {
        t.events.all().iter().any(|e| {
            matches!(e, Event::ConnectionChanged { state } if state.connected && state.peer_count > 0)
        })
    })
    .await;
    t.run(Command::SignCoordinatorInToYoutubeMusic {
        cookies: Some(Secret("SAPISID=abc; SID=x".into())),
    })
    .await;
    t.wait_until(5_000.0, |t| {
        t.events.all().iter().any(|e| {
            matches!(e, Event::CoordinatorYtmStatus { signed_in: true, account: Some(a), .. } if a == "Test User")
        })
    })
    .await;
    // Only the coordinator's key opens it, and it carried exactly the session.
    assert_eq!(
        fake.lock().ytm_sessions,
        vec![Some("SAPISID=abc; SID=x".to_string())]
    );
    t.run(Command::SignCoordinatorInToYoutubeMusic { cookies: None })
        .await;
    t.wait_until(5_000.0, |_| fake.lock().ytm_sessions.len() == 2)
        .await;
    assert_eq!(fake.lock().ytm_sessions[1], None);
}
