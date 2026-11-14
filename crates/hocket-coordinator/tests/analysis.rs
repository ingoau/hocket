//! AutoMix analysis records through the coordinator: real engines over real
//! WebSockets learn from the Welcome that the store exists, upload and
//! fetch records shared by every verified user of a server (and nobody
//! else), uploads are validated and only preferred records replace stored
//! ones; and the opt-in worker walks a fake library, downloads a
//! synthesised WAV, analyses it and stores the record.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use axum::routing::get;
use axum::Router;
use futures::{SinkExt, StreamExt};

use hocket_coordinator::analysis::AnalysisStore;
use hocket_coordinator::worker::Worker;
use hocket_coordinator::{serve, App, Args};
use hocket_core::api::{
    AnalysisProducer, AnalysisRecord, DeviceInfo, LoudnessCurve, Platform, SourceFingerprint,
};
use hocket_core::automix::ANALYSIS_VERSION;
use hocket_core::connect::engine::{Engine, EngineConfig, Input, Output};
use hocket_core::connect::session_adapter::RealReducer;
use hocket_core::connect::transport::{connect_first, Connection, PeerIds, WsTransport};
use hocket_core::connect::wire::{
    scope_key, server_key, AnalysisEntry, Credential, Msg, WireMessage, PROTOCOL, PROTOCOL_MIN,
};
use hocket_core::subsonic::fake::FakeServer;
use hocket_core::util::WallClock;

/// A Subsonic that accepts the token `good-token` for any user.
async fn fake_subsonic() -> SocketAddr {
    async fn ping(
        q: axum::extract::Query<std::collections::HashMap<String, String>>,
    ) -> axum::Json<serde_json::Value> {
        let ok = q.get("t").map(|t| t == "good-token").unwrap_or(false);
        let status = if ok { "ok" } else { "failed" };
        axum::Json(
            serde_json::json!({ "subsonic-response": { "status": status, "version": "1.16.1" } }),
        )
    }
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, Router::new().route("/rest/ping.view", get(ping)))
            .await
            .unwrap();
    });
    addr
}

fn credential(server: SocketAddr, user: &str, token: &str) -> Credential {
    Credential {
        server_url: format!("http://{server}"),
        username: user.into(),
        token: Some(token.into()),
        salt: Some("s".into()),
        api_key: None,
        client: "hocket-test".into(),
        api_version: "1.16.1".into(),
    }
}

async fn start(args: Args) -> (Arc<App>, String, tokio::sync::oneshot::Sender<()>) {
    std::env::set_var("NO_PROXY", "127.0.0.1,localhost");
    std::env::set_var("no_proxy", "127.0.0.1,localhost");
    let app = App::new(args).unwrap();
    let (stop_tx, stop_rx) = tokio::sync::oneshot::channel::<()>();
    let (addr, _server) = serve(app.clone(), "127.0.0.1:0".parse().unwrap(), async {
        let _ = stop_rx.await;
    })
    .await
    .unwrap();
    (app, format!("ws://{addr}/ws"), stop_tx)
}

fn device(id: &str) -> DeviceInfo {
    DeviceInfo {
        id: id.into(),
        name: id.into(),
        platform: Platform::Linux,
        app_version: "test".into(),
        playing: false,
        ready: false,
        last_seen: 0.0,
        is_self: true,
    }
}

/// One engine, one upstream socket, a tick every 100 ms.
struct Host {
    engine: Engine,
    conn: Option<Connection>,
    transport: WsTransport,
    ids: Arc<PeerIds>,
    outputs: Vec<Output>,
    /// The scope's account: the source its track references name.
    account: String,
}

impl Host {
    fn new(id: &str, server: SocketAddr, user: &str, ws_url: &str) -> Host {
        let scope = scope_key(&format!("http://{server}"), user);
        let mut cfg = EngineConfig::new(device(id), &scope);
        cfg.credential = Some(credential(server, user, "good-token"));
        cfg.coordinator_url = Some(ws_url.to_string());
        cfg.lan_enabled = false;
        let doc = hocket_core::session::new_document(&scope, format!("session-{id}"), 0.0);
        let engine = Engine::new(cfg, Arc::new(WallClock), RealReducer::shared(), doc, None);
        Host {
            engine,
            conn: None,
            transport: WsTransport,
            ids: Arc::new(PeerIds::default()),
            outputs: vec![],
            account: hocket_core::connect::wire::account_id_for_scope(&scope),
        }
    }

    async fn handle(&mut self, input: Input) {
        let outs = self.engine.handle(input);
        for o in outs {
            if let Output::Connect { candidates } = &o {
                let peer = self.ids.next("up");
                if let Ok(c) = connect_first(&self.transport, peer.clone(), candidates).await {
                    let url = c.url.clone();
                    self.conn = Some(c);
                    let outs = self.engine.handle(Input::Connected { peer, url });
                    for o in outs {
                        self.perform(&o);
                        self.outputs.push(o);
                    }
                }
            } else {
                self.perform(&o);
            }
            self.outputs.push(o);
        }
    }

    fn perform(&mut self, o: &Output) {
        if let Output::WireOut { peer, msg } = o {
            if let Some(c) = &self.conn {
                if &c.peer == peer {
                    let _ = c.tx.send(msg.clone());
                }
            }
        }
    }

    /// Pump for `ms` of wall time.
    async fn run(&mut self, ms: u64) {
        let deadline = tokio::time::Instant::now() + Duration::from_millis(ms);
        let mut tick = tokio::time::interval(Duration::from_millis(100));
        loop {
            let peer = self.conn.as_ref().map(|c| c.peer.clone());
            tokio::select! {
                _ = tokio::time::sleep_until(deadline) => break,
                _ = tick.tick() => self.handle(Input::Tick).await,
                msg = async {
                    match &mut self.conn {
                        Some(c) => c.rx.recv().await,
                        None => std::future::pending().await,
                    }
                }, if peer.is_some() => {
                    let peer = peer.unwrap_or_default();
                    match msg {
                        Some(m) => self.handle(Input::WireIn { peer, msg: m }).await,
                        None => {
                            self.conn = None;
                            self.handle(Input::Disconnected { peer }).await;
                        }
                    }
                }
            }
        }
    }

    fn store_available(&self) -> Option<bool> {
        self.outputs.iter().rev().find_map(|o| match o {
            Output::AnalysisStore(on) => Some(*on),
            _ => None,
        })
    }

    /// Fetch and return what came back (records from every answer).
    async fn fetch(&mut self, ids: &[&str]) -> Vec<AnalysisEntry> {
        self.outputs.clear();
        self.handle(Input::FetchAnalysis {
            track_ids: ids
                .iter()
                .map(|s| hocket_core::api::TrackRef::new(self.account.clone(), *s))
                .collect(),
        })
        .await;
        self.run(500).await;
        self.outputs
            .iter()
            .filter_map(|o| match o {
                Output::AnalysisRecords(r) => Some(r.clone()),
                _ => None,
            })
            .flatten()
            .collect()
    }

    async fn upload(&mut self, records: Vec<(&str, AnalysisRecord)>) {
        let account = self.account.clone();
        self.handle(Input::UploadAnalysis {
            records: records
                .into_iter()
                .map(|(id, record)| AnalysisEntry {
                    track_id: hocket_core::api::TrackRef::new(account.clone(), id),
                    record,
                })
                .collect(),
        })
        .await;
        self.run(300).await;
    }
}

fn record(producer: AnalysisProducer, beats: bool) -> AnalysisRecord {
    AnalysisRecord {
        version: ANALYSIS_VERSION,
        analyser: "test".into(),
        produced_by: producer,
        source: SourceFingerprint {
            duration_ms: 200_000,
            size_bytes: Some(1000.0),
            suffix: Some("flac".into()),
            transcoded: false,
        },
        duration_ms: 200_000,
        integrated_lufs: Some(-9.0),
        audible_start_ms: 0,
        audible_end_ms: 199_000,
        head: LoudnessCurve {
            start_ms: 0,
            hop_ms: 100,
            lufs: vec![-10.0; 600],
        },
        tail: LoudnessCurve {
            start_ms: 140_000,
            hop_ms: 100,
            lufs: vec![-10.0; 600],
        },
        head_vocals: Default::default(),
        tail_vocals: Default::default(),
        bpm: beats.then_some(120.0),
        beats_ms: if beats {
            (0..400).map(|i| i * 500).collect()
        } else {
            vec![]
        },
        downbeats_ms: if beats {
            (0..100).map(|i| i * 2000).collect()
        } else {
            vec![]
        },
        beat_confidence: if beats { 0.9 } else { 0.0 },
        created_at: 1.0,
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn records_are_shared_by_the_users_of_a_server_and_nobody_else() {
    let s1 = fake_subsonic().await;
    let s2 = fake_subsonic().await;
    let dir = tempfile::tempdir().unwrap();
    let mut args = Args::for_tests();
    args.data_dir = Some(dir.path().to_path_buf());
    let (app, url, _stop) = start(args).await;

    let mut alice = Host::new("alice-desk", s1, "alice", &url);
    let mut bob = Host::new("bob-phone", s1, "bob", &url);
    let mut carol = Host::new("carol-desk", s2, "carol", &url);
    for h in [&mut alice, &mut bob, &mut carol] {
        h.run(1_000).await;
        assert!(h.engine.is_connected());
        assert_eq!(
            h.store_available(),
            Some(true),
            "Welcome says the store is there"
        );
    }

    // alice's desktop uploads one good record and one from a newer analyser
    let mut future = record(AnalysisProducer::Desktop, true);
    future.version = ANALYSIS_VERSION + 1;
    alice
        .upload(vec![
            ("t1", record(AnalysisProducer::Desktop, true)),
            ("t2", future),
        ])
        .await;
    let server1 = server_key(&format!("http://{s1}"));
    let store = app.analysis_store().unwrap();
    assert!(store.get(&server1, "t1").unwrap().is_some());
    assert!(store.get(&server1, "t2").unwrap().is_none(), "refused");

    // bob, another user of the same server, gets it; misses are left out
    let got = bob.fetch(&["t1", "t2", "t3"]).await;
    assert_eq!(got.len(), 1, "{got:?}");
    assert_eq!(got[0].track_id, "t1");
    assert_eq!(got[0].record.produced_by, AnalysisProducer::Desktop);

    // bob's phone uploads a loudness-only record: the beat grid stays
    bob.upload(vec![("t1", record(AnalysisProducer::Device, false))])
        .await;
    let got = alice.fetch(&["t1"]).await;
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].record.produced_by, AnalysisProducer::Desktop);
    assert!(!got[0].record.beats_ms.is_empty());

    // carol is on another server: nothing for her, and hers stay hers
    assert!(carol.fetch(&["t1"]).await.is_empty());
    carol
        .upload(vec![("t1", record(AnalysisProducer::Device, false))])
        .await;
    let server2 = server_key(&format!("http://{s2}"));
    assert_eq!(
        store.get(&server2, "t1").unwrap().unwrap().produced_by,
        AnalysisProducer::Device
    );
    assert_eq!(
        store.get(&server1, "t1").unwrap().unwrap().produced_by,
        AnalysisProducer::Desktop
    );
    assert!(dir.path().join("automix.sqlite").exists());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn without_the_store_the_welcome_says_so_and_nothing_is_answered() {
    let s1 = fake_subsonic().await;
    let mut args = Args::for_tests();
    args.no_analysis_store = true;
    let (app, url, _stop) = start(args).await;
    assert!(app.analysis_store().is_none());
    let mut h = Host::new("a", s1, "alice", &url);
    h.run(1_000).await;
    assert!(h.engine.is_connected());
    assert_eq!(h.store_available(), None, "never said true");
    assert!(h.fetch(&["t1"]).await.is_empty());
}

/// A socket that was not admitted gets nothing from the store, whatever it sends.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_unverified_socket_cannot_read_or_write_records() {
    let s1 = fake_subsonic().await;
    let (app, url, _stop) = start(Args::for_tests()).await;
    let base = format!("http://{s1}");
    app.analysis_store()
        .unwrap()
        .put(
            &server_key(&base),
            "t1",
            record(AnalysisProducer::Worker, true),
            1,
        )
        .unwrap();
    let hello = WireMessage::new(Msg::Hello {
        device: device("intruder"),
        protocol_min: PROTOCOL_MIN,
        protocol_max: PROTOCOL,
        scope: scope_key(&base, "alice"),
        credential: Some(credential(s1, "alice", "wrong")),
        session_id: None,
        session_revision: 0,
        held_epoch: None,
        extra: Default::default(),
    });
    let fetch = WireMessage::new(Msg::AnalysisFetch {
        track_ids: vec!["t1".into()],
    });
    let upload = WireMessage::new(Msg::AnalysisUpload {
        records: vec![AnalysisEntry {
            track_id: "t9".into(),
            record: record(AnalysisProducer::Worker, true),
        }],
    });
    let (mut ws, _) = tokio_tungstenite::connect_async(&url).await.unwrap();
    use tokio_tungstenite::tungstenite::Message;
    for m in [&hello, &fetch, &upload] {
        let _ = ws.send(Message::text(m.encode().unwrap())).await;
    }
    let mut got = vec![];
    let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
    while let Ok(Some(Ok(Message::Text(t)))) = tokio::time::timeout_at(deadline, ws.next()).await {
        got.push(WireMessage::decode(&t).unwrap().msg);
    }
    assert!(
        !got.iter().any(|m| matches!(m, Msg::AnalysisRecords { .. })),
        "{got:?}"
    );
    assert!(app
        .analysis_store()
        .unwrap()
        .get(&server_key(&base), "t9")
        .unwrap()
        .is_none());
}

/// Six seconds of 120 BPM clicks as a 16-bit mono WAV.
fn click_wav(seconds: f64) -> Vec<u8> {
    let rate = 22_050u32;
    let n = (seconds * f64::from(rate)) as usize;
    let mut pcm = vec![0i16; n];
    let beat = rate as usize / 2;
    for (k, start) in (0..n).step_by(beat).enumerate() {
        let amp = if k % 4 == 0 { 0.9 } else { 0.4 };
        for i in 0..(rate as usize / 20) {
            if start + i >= n {
                break;
            }
            let env = (-(i as f64) / (0.01 * f64::from(rate))).exp();
            let s = amp
                * env
                * (2.0 * std::f64::consts::PI * 1_000.0 * i as f64 / f64::from(rate)).sin();
            pcm[start + i] = (s * 32_000.0) as i16;
        }
    }
    let data_len = (n * 2) as u32;
    let mut out = Vec::with_capacity(44 + n * 2);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes()); // PCM
    out.extend_from_slice(&1u16.to_le_bytes()); // mono
    out.extend_from_slice(&rate.to_le_bytes());
    out.extend_from_slice(&(rate * 2).to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    for s in pcm {
        out.extend_from_slice(&s.to_le_bytes());
    }
    out
}

#[tokio::test]
async fn the_worker_walks_the_library_and_stores_what_it_analysed() {
    let fake = Arc::new(FakeServer::new("srv", "worker"));
    let wav = click_wav(6.0);
    let mut song = FakeServer::song("good", "Clicks", "al1", "ar1", 6.0);
    song.suffix = Some("wav".into());
    song.content_type = Some("audio/wav".into());
    song.size = Some(wav.len() as f64);
    fake.add_song(song);
    fake.set_media("good", wav);
    // a file that isn't audio: fails, and isn't tried again
    let mut broken = FakeServer::song("broken", "Broken", "al1", "ar1", 10.0);
    broken.suffix = Some("wav".into());
    fake.add_song(broken);
    fake.set_media("broken", b"not audio at all".to_vec());
    // already covered by a desktop's beat grid: skipped
    let mut covered = FakeServer::song("covered", "Covered", "al1", "ar1", 200.0);
    covered.size = Some(1000.0);
    fake.add_song(covered);

    let store = Arc::new(AnalysisStore::in_memory().unwrap());
    let server = "https://music.example/";
    store
        .put(
            &server_key(server),
            "covered",
            record(AnalysisProducer::Desktop, true),
            1,
        )
        .unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let status = Arc::new(parking_lot::Mutex::new(None));
    let mut worker = Worker::new(
        fake.clone(),
        store.clone(),
        server,
        Duration::ZERO,
        tmp.path().to_path_buf(),
        status.clone(),
    );
    let cancel = tokio_util::sync::CancellationToken::new();
    let stats = worker.pass(&cancel).await.unwrap();
    assert_eq!(stats.library, 3);
    assert_eq!(stats.todo, 2);
    assert_eq!(stats.analysed, 1);
    assert_eq!(stats.failed, 1);

    let r = store.get(&server_key(server), "good").unwrap().unwrap();
    assert_eq!(r.produced_by, AnalysisProducer::Worker);
    assert_eq!(r.source.suffix.as_deref(), Some("wav"));
    assert_eq!(r.source.size_bytes, Some(click_wav(6.0).len() as f64));
    assert!(!r.source.transcoded);
    assert!(r.duration_ms.abs_diff(6_000) < 100, "{}", r.duration_ms);
    assert!(r.integrated_lufs.is_some());
    // downloads are gone
    assert_eq!(std::fs::read_dir(tmp.path()).unwrap().count(), 0);
    let s = status.lock().clone().unwrap();
    assert_eq!((s.analysed, s.failed, s.remaining, s.passes), (1, 1, 0, 1));

    // the next walk finds nothing to do
    let stats = worker.pass(&cancel).await.unwrap();
    assert_eq!((stats.library, stats.todo), (3, 0));
}

fn wav_song(fake: &FakeServer, id: &str) {
    let wav = click_wav(3.0);
    let mut song = FakeServer::song(id, id, "al1", "ar1", 3.0);
    song.suffix = Some("wav".into());
    song.size = Some(wav.len() as f64);
    fake.add_song(song);
    fake.set_media(id, wav);
}

#[tokio::test]
async fn failed_downloads_are_retried_and_end_the_pass_when_they_pile_up() {
    use hocket_core::subsonic::SubsonicError;
    let fake = Arc::new(FakeServer::new("srv", "worker"));
    for id in ["a", "b", "c", "d"] {
        wav_song(&fake, id);
    }
    let store = Arc::new(AnalysisStore::in_memory().unwrap());
    let tmp = tempfile::tempdir().unwrap();
    let status = Arc::new(parking_lot::Mutex::new(None));
    let server = "https://music.example/";
    let mut worker = Worker::new(
        fake.clone(),
        store.clone(),
        server,
        Duration::ZERO,
        tmp.path().to_path_buf(),
        status,
    );
    let cancel = tokio_util::sync::CancellationToken::new();
    let library = worker.walk().await.unwrap();
    assert_eq!(library.len(), 4);

    // one failed download: skipped this pass, not remembered
    fake.fail_next(SubsonicError::Network("reset".into()), 1);
    let stats = worker.analyse_library(&library, &cancel).await.unwrap();
    assert_eq!((stats.analysed, stats.failed), (3, 1));
    let stats = worker.pass(&cancel).await.unwrap();
    assert_eq!((stats.todo, stats.analysed), (1, 1), "tried again");

    // the server goes away: the pass ends after a few in a row, and nothing
    // is remembered
    let key = server_key(server);
    for id in ["a", "b", "c", "d"] {
        let mut r = store.get(&key, id).unwrap().unwrap();
        r.source.size_bytes = Some(1.0); // another file: wanted again
        store.put(&key, id, r, 2).unwrap();
    }
    let downloads = fake.calls_to("download");
    fake.fail_next(SubsonicError::Network("down".into()), 3);
    let err = worker.analyse_library(&library, &cancel).await.unwrap_err();
    assert!(err.to_string().contains("in a row"), "{err}");
    assert_eq!(fake.calls_to("download"), downloads + 3);
    let stats = worker.pass(&cancel).await.unwrap();
    assert_eq!((stats.todo, stats.analysed), (4, 4));
}

#[tokio::test]
async fn the_worker_leaves_long_and_large_files_alone() {
    let fake = Arc::new(FakeServer::new("srv", "worker"));
    wav_song(&fake, "ok");
    let mut long = FakeServer::song("long", "Long", "al1", "ar1", 91.0 * 60.0);
    long.size = Some(1000.0);
    fake.add_song(long);
    let mut large = FakeServer::song("large", "Large", "al1", "ar1", 300.0);
    large.size = Some(2_000_000.0);
    fake.add_song(large);
    let store = Arc::new(AnalysisStore::in_memory().unwrap());
    let tmp = tempfile::tempdir().unwrap();
    let mut worker = Worker::new(
        fake.clone(),
        store,
        "https://music.example/",
        Duration::ZERO,
        tmp.path().to_path_buf(),
        Arc::new(parking_lot::Mutex::new(None)),
    )
    .with_max_file_bytes(1_000_000);
    let cancel = tokio_util::sync::CancellationToken::new();
    let stats = worker.pass(&cancel).await.unwrap();
    assert_eq!((stats.library, stats.todo, stats.analysed), (3, 1, 1));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn fetch_misses_on_the_workers_server_are_queued_for_it() {
    let s1 = fake_subsonic().await;
    let s2 = fake_subsonic().await;
    let mut args = Args::for_tests();
    args.worker.analysis_server = Some(format!("http://{s1}"));
    let (app, url, _stop) = start(args).await;
    let queue = app.worker_priority().expect("worker on");
    *app.worker_status_slot().lock() = Some(Default::default());

    let server1 = server_key(&format!("http://{s1}"));
    let store = app.analysis_store().unwrap();
    store
        .put(
            &server1,
            "beats",
            record(AnalysisProducer::Desktop, true),
            1,
        )
        .unwrap();
    store
        .put(&server1, "loud", record(AnalysisProducer::Device, false), 1)
        .unwrap();

    let mut bob = Host::new("bob-phone", s1, "bob", &url);
    let mut carol = Host::new("carol-phone", s2, "carol", &url);
    for h in [&mut bob, &mut carol] {
        h.run(1_000).await;
        assert!(h.engine.is_connected());
    }
    // answered as before; the miss and the beat-less record are queued, once
    let got = bob.fetch(&["beats", "loud", "none", "none"]).await;
    assert_eq!(got.len(), 2, "{got:?}");
    assert_eq!(queue.len(), 2);
    bob.fetch(&["none"]).await;
    assert_eq!(queue.len(), 2, "deduplicated");
    assert_eq!(queue.pop().as_deref(), Some("loud"));
    assert_eq!(queue.pop().as_deref(), Some("none"));

    // another server's misses are not the worker's business
    assert!(carol.fetch(&["elsewhere"]).await.is_empty());
    assert!(queue.is_empty());

    bob.fetch(&["x"]).await;
    let status = app.worker_status().unwrap();
    assert_eq!(status.priority_queue, 1);
    assert_eq!(serde_json::to_value(&status).unwrap()["priorityQueue"], 1);
}

#[tokio::test]
async fn the_worker_analyses_what_clients_asked_for_first() {
    use hocket_core::subsonic::SubsonicError;
    let fake = Arc::new(FakeServer::new("srv", "worker"));
    for id in ["a", "b", "c"] {
        wav_song(&fake, id);
    }
    let store = Arc::new(AnalysisStore::in_memory().unwrap());
    let tmp = tempfile::tempdir().unwrap();
    let server = "https://music.example/";
    let key = server_key(server);
    let mut worker = Worker::new(
        fake.clone(),
        store.clone(),
        server,
        Duration::ZERO,
        tmp.path().to_path_buf(),
        Arc::new(parking_lot::Mutex::new(None)),
    );
    let queue = worker.priority();
    let cancel = tokio_util::sync::CancellationToken::new();

    // looked up with getSong, not walked; an unknown id is dropped
    assert_eq!(queue.push(&key, ["c", "nope"]), 2);
    assert_eq!(worker.drain_priority(&cancel).await, 1);
    assert!(store.get(&key, "c").unwrap().is_some());
    assert!(store.get(&key, "a").unwrap().is_none());
    assert_eq!(fake.calls_to("search3"), 0);
    assert_eq!(fake.calls_to("download"), 1);

    // a lookup the server fails keeps the queue; an unknown id isn't queued again
    queue.push(&key, ["a"]);
    fake.fail_next(SubsonicError::Network("down".into()), 1);
    assert_eq!(worker.drain_priority(&cancel).await, 0);
    assert_eq!(queue.len(), 1, "kept for later");
    assert_eq!(queue.pop().as_deref(), Some("a"));
    assert_eq!(queue.push(&key, ["nope"]), 0);

    // the walk doesn't analyse it again
    let library = worker.walk().await.unwrap();
    queue.push(&key, ["b"]);
    let stats = worker.analyse_library(&library, &cancel).await.unwrap();
    assert_eq!(
        (stats.todo, stats.analysed),
        (2, 1),
        "b went first, from the queue"
    );
    assert_eq!(fake.calls_to("download"), 3);
    assert!(store.get(&key, "a").unwrap().is_some());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_queued_track_wakes_the_idle_worker() {
    let fake = Arc::new(FakeServer::new("srv", "worker"));
    wav_song(&fake, "a");
    let store = Arc::new(AnalysisStore::in_memory().unwrap());
    let tmp = tempfile::tempdir().unwrap();
    let server = "https://music.example/";
    let key = server_key(server);
    let status = Arc::new(parking_lot::Mutex::new(None));
    let worker = Worker::new(
        fake.clone(),
        store.clone(),
        server,
        Duration::ZERO,
        tmp.path().join("dl"),
        status.clone(),
    );
    let queue = worker.priority();
    let cancel = tokio_util::sync::CancellationToken::new();
    let run = tokio::spawn(worker.run(Duration::from_secs(3600), cancel.clone()));
    let until = |what: &str, ok: &dyn Fn() -> bool| {
        let deadline = std::time::Instant::now() + Duration::from_secs(30);
        while !ok() {
            assert!(std::time::Instant::now() < deadline, "timed out: {what}");
            std::thread::sleep(Duration::from_millis(20));
        }
    };
    until("first pass", &|| {
        status
            .lock()
            .as_ref()
            .is_some_and(|s: &hocket_coordinator::worker::WorkerStatus| s.passes == 1)
    });
    // a song added after the walk, asked for by a client
    wav_song(&fake, "new");
    assert_eq!(queue.push(&key, ["new"]), 1);
    until("analysed", &|| store.get(&key, "new").unwrap().is_some());
    assert!(queue.is_empty());
    until("waiting again", &|| {
        status.lock().as_ref().unwrap().state == "waiting"
    });
    assert_eq!(status.lock().as_ref().unwrap().analysed, 2);
    cancel.cancel();
    tokio::time::timeout(Duration::from_secs(10), run)
        .await
        .unwrap()
        .unwrap();
}
