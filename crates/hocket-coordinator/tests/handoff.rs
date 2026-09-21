//! Spins the coordinator on an ephemeral port next to a fake Subsonic
//! server, drives two real engines over real WebSockets (the same transport
//! the apps use), and walks them through a handoff.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use axum::routing::get;
use axum::Router;

use hocket_coordinator::{serve, App, Args};
use hocket_core::api::{DeviceInfo, Platform, PositionStamp};
use hocket_core::connect::engine::{Engine, EngineConfig, Input, Output};
use hocket_core::connect::session_adapter::RealReducer;
use hocket_core::connect::transport::{connect_first, Connection, PeerIds, WsTransport};
use hocket_core::connect::wire::{scope_key, Credential};
use hocket_core::connect::SessionOp;
use hocket_core::util::WallClock;

/// A Subsonic that accepts exactly one token.
async fn fake_subsonic() -> SocketAddr {
    async fn ping(
        q: axum::extract::Query<std::collections::HashMap<String, String>>,
    ) -> axum::Json<serde_json::Value> {
        let ok = q.get("u").map(|u| u == "alice").unwrap_or(false)
            && q.get("t").map(|t| t == "good-token").unwrap_or(false);
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

fn credential(subsonic: SocketAddr, token: &str) -> Credential {
    Credential {
        server_url: format!("http://{subsonic}"),
        username: "alice".into(),
        token: Some(token.into()),
        salt: Some("s".into()),
        api_key: None,
        client: "hocket-test".into(),
        api_version: "1.16.1".into(),
    }
}

/// The smallest actor: one engine, one upstream socket, a tick every 100 ms.
struct Host {
    engine: Engine,
    conn: Option<Connection>,
    transport: WsTransport,
    ids: Arc<PeerIds>,
    outputs: Vec<Output>,
}

impl Host {
    fn new(id: &str, scope: &str, credential: Credential, ws_url: &str) -> Host {
        let device = DeviceInfo {
            id: id.into(),
            name: id.into(),
            platform: Platform::Linux,
            app_version: "test".into(),
            playing: false,
            ready: false,
            last_seen: 0.0,
            is_self: true,
        };
        let mut cfg = EngineConfig::new(device, scope);
        cfg.credential = Some(credential);
        cfg.coordinator_url = Some(ws_url.to_string());
        cfg.lan_enabled = false;
        let doc = hocket_core::session::new_document(scope, format!("session-{id}"), 0.0);
        let engine = Engine::new(cfg, Arc::new(WallClock), RealReducer::shared(), doc, None);
        Host {
            engine,
            conn: None,
            transport: WsTransport,
            ids: Arc::new(PeerIds::default()),
            outputs: vec![],
        }
    }

    async fn handle(&mut self, input: Input) {
        let outs = self.engine.handle(input);
        for o in outs {
            match &o {
                Output::Connect { candidates } => {
                    let peer = self.ids.next("up");
                    match connect_first(&self.transport, peer.clone(), candidates).await {
                        Ok(c) => {
                            let url = c.url.clone();
                            self.conn = Some(c);
                            let outs = self.engine.handle(Input::Connected { peer, url });
                            self.outputs.extend(outs.clone());
                            for o in outs {
                                self.perform(o);
                            }
                        }
                        Err(e) => {
                            let outs = self.engine.handle(Input::ConnectFailed {
                                error: e.to_string(),
                            });
                            self.outputs.extend(outs);
                        }
                    }
                }
                other => self.perform(other.clone()),
            }
            self.outputs.push(o);
        }
    }

    fn perform(&mut self, o: Output) {
        match o {
            Output::WireOut { peer, msg } => {
                if let Some(c) = &self.conn {
                    if c.peer == peer {
                        let _ = c.tx.send(msg);
                    }
                }
            }
            Output::Disconnect { peer } => {
                if self.conn.as_ref().map(|c| c.peer == peer).unwrap_or(false) {
                    self.conn = None;
                }
            }
            _ => {}
        }
    }

    /// Pump for `ms` of wall time.
    async fn run(&mut self, ms: u64) {
        let deadline = tokio::time::Instant::now() + Duration::from_millis(ms);
        let mut tick = tokio::time::interval(Duration::from_millis(100));
        loop {
            let (peer, has_conn) = match &self.conn {
                Some(c) => (c.peer.clone(), true),
                None => (String::new(), false),
            };
            tokio::select! {
                _ = tokio::time::sleep_until(deadline) => break,
                _ = tick.tick() => self.handle(Input::Tick).await,
                msg = async {
                    match &mut self.conn {
                        Some(c) => c.rx.recv().await,
                        None => std::future::pending().await,
                    }
                }, if has_conn => {
                    match msg {
                        Some(m) => self.handle(Input::WireIn { peer: peer.clone(), msg: m }).await,
                        None => {
                            self.conn = None;
                            self.handle(Input::Disconnected { peer }).await;
                        }
                    }
                }
            }
        }
    }

    fn took(&self) -> Option<(String, u32)> {
        self.outputs.iter().rev().find_map(|o| match o {
            Output::TakeTransport {
                track_id,
                position_ms,
                ..
            } => Some((track_id.clone(), *position_ms)),
            _ => None,
        })
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn two_engines_hand_off_through_the_coordinator() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(std::env::var("RUST_LOG").unwrap_or_else(|_| "info".into()))
        .try_init();
    // The proxied ping must reach the loopback fake directly, whatever proxy the environment sets.
    std::env::set_var("NO_PROXY", "127.0.0.1,localhost");
    std::env::set_var("no_proxy", "127.0.0.1,localhost");
    let subsonic = fake_subsonic().await;
    let dir = tempfile::tempdir().unwrap();
    let args = Args {
        listen: "127.0.0.1:0".parse().unwrap(),
        data_dir: Some(dir.path().to_path_buf()),
        verify_url: None,
        no_verify: false,
        max_members: 8,
    };
    let app = App::new(args).unwrap();
    let (stop_tx, stop_rx) = tokio::sync::oneshot::channel::<()>();
    let (addr, server) = serve(app.clone(), "127.0.0.1:0".parse().unwrap(), async {
        let _ = stop_rx.await;
    })
    .await
    .unwrap();
    let ws_url = format!("ws://{addr}/ws");
    let scope = scope_key(&format!("http://{subsonic}"), "alice");

    // health
    let health: serde_json::Value = reqwest::get(format!("http://{addr}/health"))
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(health["status"], "ok");

    // a bad credential is refused by the proxied ping
    let mut bad = Host::new("bad", &scope, credential(subsonic, "wrong"), &ws_url);
    bad.handle(Input::Tick).await;
    bad.run(1500).await;
    assert!(!bad.engine.is_connected(), "bad token must not be admitted");
    assert!(!app.has_member(&scope, "bad"));

    let mut a = Host::new(
        "laptop",
        &scope,
        credential(subsonic, "good-token"),
        &ws_url,
    );
    let mut b = Host::new("phone", &scope, credential(subsonic, "good-token"), &ws_url);
    a.handle(Input::Tick).await;
    b.handle(Input::Tick).await;
    a.run(800).await;
    b.run(800).await;
    assert!(a.engine.is_connected(), "laptop attached");
    assert!(b.engine.is_connected(), "phone attached");
    assert!(app.has_member(&scope, "laptop") && app.has_member(&scope, "phone"));

    // laptop plays a selection and takes transport
    a.handle(Input::LocalOp {
        op: SessionOp::PlayTracks {
            server_id: "srv".into(),
            track_ids: vec!["t1".into(), "t2".into(), "t3".into()],
            start_index: 0,
            label: "Selection".into(),
            shuffle: false,
            save_outgoing: false,
        },
    })
    .await;
    a.handle(Input::ClaimTransport { takeover: false }).await;
    a.run(400).await;
    b.run(400).await;
    assert!(a.engine.owns_transport());
    assert_eq!(
        b.engine
            .document()
            .current
            .as_ref()
            .map(|c| c.track_id.as_str()),
        Some("t1")
    );
    assert_eq!(app.live_owner(&scope).as_deref(), Some("laptop"));
    let started_at = a.engine.now_session_ms();
    a.handle(Input::LocalStamp {
        key: a.engine.document().current.as_ref().map(|c| c.key.clone()),
        track_id: Some("t1".into()),
        position: PositionStamp {
            position_ms: 30_000,
            taken_at: a.engine.now_local_ms(),
            rate: 1.0,
            is_playing: true,
        },
        played_ms: 30_000,
        started_at,
        scrobbled: false,
    })
    .await;

    // phone presses next: one skip everywhere
    b.handle(Input::LocalOp {
        op: SessionOp::Next,
    })
    .await;
    b.run(400).await;
    a.run(400).await;
    assert_eq!(
        a.engine
            .document()
            .current
            .as_ref()
            .map(|c| c.track_id.as_str()),
        Some("t2")
    );
    assert_eq!(
        b.engine
            .document()
            .current
            .as_ref()
            .map(|c| c.track_id.as_str()),
        Some("t2")
    );
    let key = a.engine.document().current.as_ref().unwrap().key.clone();
    a.handle(Input::LocalStamp {
        key: Some(key.clone()),
        track_id: Some("t2".into()),
        position: PositionStamp {
            position_ms: 5_000,
            taken_at: a.engine.now_local_ms(),
            rate: 1.0,
            is_playing: true,
        },
        played_ms: 5_000,
        started_at: a.engine.now_session_ms(),
        scrobbled: false,
    })
    .await;

    // handoff: picker → phone pre-buffers → pick → phone owns and plays from the position
    a.handle(Input::OpenPicker).await;
    a.run(400).await;
    b.run(400).await;
    let prepared = b.outputs.iter().find_map(|o| match o {
        Output::PreBuffer { key, track_id, .. } => Some((key.clone(), track_id.clone())),
        _ => None,
    });
    assert_eq!(
        prepared,
        Some((key.clone(), "t2".to_string())),
        "phone asked to pre-buffer"
    );
    b.handle(Input::PreBufferReady { key: key.clone() }).await;
    b.run(300).await;
    a.run(300).await;
    assert!(a.outputs.iter().any(|o| matches!(o, Output::PickerChanged { open: true, targets } if targets.iter().any(|d| d.id == "phone" && d.ready))));
    a.handle(Input::HandoffTo {
        device_id: "phone".into(),
    })
    .await;
    a.run(400).await;
    b.run(400).await;
    assert!(!a.engine.owns_transport(), "laptop released");
    assert!(b.engine.owns_transport(), "phone owns");
    assert_eq!(app.live_owner(&scope).as_deref(), Some("phone"));
    let (track, pos) = b.took().expect("phone took transport");
    assert_eq!(track, "t2");
    assert!((5_000..9_000).contains(&pos), "position travelled: {pos}");

    // the replica survived on disk
    app.flush_all();
    let replica = app
        .store()
        .load(&scope)
        .unwrap()
        .expect("replica persisted");
    assert_eq!(replica.transport_lease.owner.as_deref(), Some("phone"));
    assert_eq!(
        replica
            .document
            .current
            .as_ref()
            .map(|c| c.track_id.as_str()),
        Some("t2")
    );
    let json = serde_json::to_string(&replica).unwrap();
    assert!(
        !json.contains("good-token"),
        "credentials never reach the replica"
    );

    let _ = stop_tx.send(());
    let _ = tokio::time::timeout(Duration::from_secs(5), server).await;
}
