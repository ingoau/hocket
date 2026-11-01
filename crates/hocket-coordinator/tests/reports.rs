//! Save to Coordinator (`docs/reports.md`): grants on the diagnostics answer
//! for report users only, the upload API with its limits, quota, retention
//! and the `reports` CLI.

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use axum::routing::get;
use axum::Router;
use futures::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

use hocket_coordinator::{reports, serve, App, Args};
use hocket_core::api::{DeviceInfo, Platform};
use hocket_core::connect::diagnostics::{COORDINATOR, HELLO_FLAG, ROOM};
use hocket_core::connect::wire::{scope_key, Credential, Msg, ReportGrant, WireMessage};
use hocket_core::connect::wire::{PROTOCOL, PROTOCOL_MIN};

type Ws = WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>;

const T0: u64 = 1_793_541_600_000; // 2026-11-01T14:00:00Z
const HOUR: u64 = 3_600_000;
const DAY: u64 = 24 * HOUR;

/// A Subsonic that accepts the token `good-token` for anyone.
async fn fake_subsonic() -> SocketAddr {
    let ping = |q: axum::extract::Query<std::collections::HashMap<String, String>>| async move {
        let ok = q.get("t").map(|t| t == "good-token").unwrap_or(false);
        let status = if ok { "ok" } else { "failed" };
        axum::Json(json!({ "subsonic-response": { "status": status } }))
    };
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, Router::new().route("/rest/ping.view", get(ping)))
            .await
            .unwrap();
    });
    addr
}

/// A running coordinator with a clock the test moves.
struct Coord {
    ws: String,
    http: String,
    app: Arc<App>,
    clock: Arc<AtomicU64>,
    _stop: tokio::sync::oneshot::Sender<()>,
}

impl Coord {
    fn advance(&self, ms: u64) {
        self.clock.fetch_add(ms, Ordering::SeqCst);
    }
}

async fn start(args: Args) -> Coord {
    hocket_core::devlog::install("info", None, false);
    std::env::set_var("NO_PROXY", "127.0.0.1,localhost");
    std::env::set_var("no_proxy", "127.0.0.1,localhost");
    let clock = Arc::new(AtomicU64::new(T0));
    let c = clock.clone();
    let app = App::with_clock(args, Arc::new(move || c.load(Ordering::SeqCst))).unwrap();
    let (stop_tx, stop_rx) = tokio::sync::oneshot::channel::<()>();
    let (addr, _server) = serve(app.clone(), "127.0.0.1:0".parse().unwrap(), async {
        let _ = stop_rx.await;
    })
    .await
    .unwrap();
    Coord {
        ws: format!("ws://{addr}/ws"),
        http: format!("http://{addr}"),
        app,
        clock,
        _stop: stop_tx,
    }
}

/// Args for a coordinator that keeps reports for `alice` on `server`.
fn report_args(server: &str, data: &Path) -> Args {
    let mut args = Args::for_tests();
    args.diagnostics_users = vec![format!("alice@{server}")];
    args.data_dir = Some(data.to_path_buf());
    args
}

fn send(m: Msg) -> Message {
    Message::text(WireMessage::new(m).encode().unwrap())
}

async fn join(url: &str, server: &str, user: &str, device: &str) -> Ws {
    let (mut ws, _) = tokio_tungstenite::connect_async(url).await.unwrap();
    ws.send(send(Msg::Hello {
        device: DeviceInfo {
            id: device.into(),
            name: format!("Device {device}"),
            platform: Platform::Linux,
            app_version: "test".into(),
            playing: false,
            ready: false,
            last_seen: 0.0,
            is_self: true,
        },
        protocol_min: PROTOCOL_MIN,
        protocol_max: PROTOCOL,
        scope: scope_key(server, user),
        credential: Some(Credential {
            server_url: server.into(),
            username: user.into(),
            token: Some("good-token".into()),
            salt: Some("s".into()),
            api_key: None,
            client: "hocket-test".into(),
            api_version: "1.16.1".into(),
        }),
        session_id: None,
        session_revision: 0,
        held_epoch: None,
        extra: [(HELLO_FLAG.to_string(), Value::Bool(true))]
            .into_iter()
            .collect(),
    }))
    .await
    .unwrap();
    let got = until(&mut ws, |m| matches!(m, Msg::Welcome { .. })).await;
    assert!(matches!(got.last(), Some(Msg::Welcome { .. })), "{got:?}");
    ws
}

async fn until(ws: &mut Ws, stop: impl Fn(&Msg) -> bool) -> Vec<Msg> {
    let mut got = vec![];
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while let Ok(Some(Ok(frame))) = tokio::time::timeout_at(deadline, ws.next()).await {
        if let Message::Text(t) = frame {
            if let Ok(m) = WireMessage::decode(&t) {
                let done = stop(&m.msg);
                got.push(m.msg);
                if done {
                    break;
                }
            }
        }
    }
    got
}

/// Ask for diagnostics as `user`; every coordinator part's `(part, grant)`.
async fn coordinator_grants(
    c: &Coord,
    server: &str,
    user: &str,
    device: &str,
) -> Vec<(u32, Option<ReportGrant>)> {
    let mut ws = join(&c.ws, server, user, device).await;
    ws.send(send(Msg::DiagnosticsRequest {
        request_id: "r".into(),
        from: device.into(),
    }))
    .await
    .unwrap();
    let mut out = vec![];
    let mut total = None;
    let mut room_seen = false;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    while total.is_none_or(|t| out.len() < t as usize) || !room_seen {
        let Ok(Some(Ok(Message::Text(t)))) = tokio::time::timeout_at(deadline, ws.next()).await
        else {
            break;
        };
        match WireMessage::decode(&t).map(|m| m.msg) {
            Ok(Msg::DiagnosticsReply {
                from,
                part,
                parts,
                report_grant,
                ..
            }) if from == COORDINATOR => {
                total = Some(parts);
                out.push((part, report_grant));
            }
            Ok(Msg::DiagnosticsReply { from, .. }) if from == ROOM => room_seen = true,
            _ => {}
        }
    }
    assert!(!out.is_empty(), "no coordinator answer");
    out
}

async fn grant_for_alice(c: &Coord, server: &str) -> String {
    let parts = coordinator_grants(c, server, "alice", "a").await;
    parts[0].1.clone().expect("alice gets a grant").token
}

fn client() -> reqwest::Client {
    reqwest::Client::builder().no_proxy().build().unwrap()
}

async fn create(c: &Coord, token: &str, body: Value) -> (u16, Value) {
    let r = client()
        .post(format!("{}/reports", c.http))
        .bearer_auth(token)
        .json(&body)
        .send()
        .await
        .unwrap();
    let status = r.status().as_u16();
    (status, r.json().await.unwrap_or(Value::Null))
}

async fn put(
    c: &Coord,
    id: &str,
    key: &str,
    index: u32,
    offset: u64,
    data: Vec<u8>,
) -> (u16, Value) {
    let r = client()
        .put(format!("{}/reports/{id}/files/{index}", c.http))
        .bearer_auth(key)
        .header("Upload-Offset", offset.to_string())
        .body(data)
        .send()
        .await
        .unwrap();
    let status = r.status().as_u16();
    (status, r.json().await.unwrap_or(Value::Null))
}

async fn status(c: &Coord, id: &str, key: &str) -> (u16, Value) {
    let r = client()
        .get(format!("{}/reports/{id}", c.http))
        .bearer_auth(key)
        .send()
        .await
        .unwrap();
    let status = r.status().as_u16();
    (status, r.json().await.unwrap_or(Value::Null))
}

fn bundle_only(size: u64) -> Value {
    json!({ "note": "n", "client": "test", "device": "d",
            "files": [{ "kind": "bundle", "size": size, "name": "b.txt" }] })
}

/// Create a report with one bundle of `size` bytes and upload it whole.
async fn complete_report(c: &Coord, token: &str, size: usize) -> (String, String) {
    let (s, v) = create(c, token, bundle_only(size as u64)).await;
    assert_eq!(s, 201, "{v}");
    let id = v["id"].as_str().unwrap().to_string();
    let key = v["uploadKey"].as_str().unwrap().to_string();
    let (s, v) = put(c, &id, &key, 0, 0, vec![b'x'; size]).await;
    assert_eq!(s, 200, "{v}");
    assert_eq!(v["complete"], true);
    (id, key)
}

fn report_dir(data: &Path, id: &str) -> PathBuf {
    data.join("reports").join(id)
}

fn report_json(data: &Path, id: &str) -> Value {
    serde_json::from_slice(&std::fs::read(report_dir(data, id).join("report.json")).unwrap())
        .unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn report_users_get_a_grant_on_part_zero_and_nobody_else_does() {
    let subsonic = fake_subsonic().await;
    let server = format!("http://{subsonic}");
    let data = tempfile::tempdir().unwrap();
    let c = start(report_args(&server, data.path())).await;
    assert!(c.app.reports_enabled());

    let alice = coordinator_grants(&c, &server, "alice", "a").await;
    let grant = alice
        .iter()
        .find(|(p, _)| *p == 0)
        .and_then(|(_, g)| g.clone())
        .expect("a grant on part 0");
    assert_eq!(grant.token.len(), 64);
    assert_eq!(grant.expires_in_ms, 6 * HOUR);
    assert!(
        alice
            .iter()
            .filter(|(p, _)| *p != 0)
            .all(|(_, g)| g.is_none()),
        "{alice:?}"
    );

    let bob = coordinator_grants(&c, &server, "bob", "b").await;
    assert!(bob.iter().all(|(_, g)| g.is_none()), "{bob:?}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn no_grant_without_a_data_dir_or_with_no_reports() {
    let subsonic = fake_subsonic().await;
    let server = format!("http://{subsonic}");
    let data = tempfile::tempdir().unwrap();

    let mut args = report_args(&server, data.path());
    args.no_reports = true;
    let c = start(args).await;
    assert!(!c.app.reports_enabled());
    let parts = coordinator_grants(&c, &server, "alice", "a").await;
    assert!(parts.iter().all(|(_, g)| g.is_none()), "{parts:?}");
    let (s, v) = create(&c, "whatever", bundle_only(1)).await;
    assert_eq!((s, v["error"].as_str()), (503, Some("disabled")));
    assert!(!data.path().join("reports").exists());

    let mut args = report_args(&server, data.path());
    args.data_dir = None;
    let c = start(args).await;
    assert!(!c.app.reports_enabled());
    let parts = coordinator_grants(&c, &server, "alice", "a").await;
    assert!(parts.iter().all(|(_, g)| g.is_none()), "{parts:?}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_report_uploads_in_chunks_resumes_and_completes() {
    let subsonic = fake_subsonic().await;
    let server = format!("http://{subsonic}");
    let data = tempfile::tempdir().unwrap();
    let c = start(report_args(&server, data.path())).await;
    let token = grant_for_alice(&c, &server).await;

    let bundle: Vec<u8> = (0..300_000u32).map(|i| (i % 251) as u8).collect();
    let shot: Vec<u8> = vec![7; 1000];
    let rec: Vec<u8> = (0..50_000u32).map(|i| (i % 13) as u8).collect();
    let (s, v) = create(
        &c,
        &token,
        json!({
            "note": "Queue jumped back two songs\nafter unlocking",
            "client": "Hocket Test 1.0",
            "device": "Pixel 7 Pro",
            "files": [
                { "kind": "bundle", "size": bundle.len(), "name": "../../hocket-diagnostics.txt" },
                { "kind": "screenshot", "size": shot.len(), "name": "screenshot-1.JPEG" },
                { "kind": "recording", "size": rec.len(), "name": "screen.mp4" },
            ]
        }),
    )
    .await;
    assert_eq!(s, 201, "{v}");
    let id = v["id"].as_str().unwrap().to_string();
    let key = v["uploadKey"].as_str().unwrap().to_string();
    assert!(id.starts_with("R-20261101-"), "{id}");
    assert!(reports::valid_id(&id));
    assert_eq!(key.len(), 64);
    assert!(key.chars().all(|c| c.is_ascii_hexdigit()));
    assert_eq!(v["files"].as_array().unwrap().len(), 3);
    assert_eq!(
        v["files"][2],
        json!({ "index": 2, "size": rec.len(), "received": 0 })
    );

    // the bundle in three chunks
    let (s, v) = put(&c, &id, &key, 0, 0, bundle[..100_000].to_vec()).await;
    assert_eq!((s, v["received"].as_u64()), (200, Some(100_000)), "{v}");
    assert_eq!(v["complete"], false);
    // a wrong offset is refused with what the coordinator has
    let (s, v) = put(&c, &id, &key, 0, 0, bundle[..10].to_vec()).await;
    assert_eq!(s, 409, "{v}");
    assert_eq!(v["error"], "conflict");
    assert_eq!(v["received"], 100_000);
    let (s, v) = put(&c, &id, &key, 0, 100_000, bundle[100_000..200_000].to_vec()).await;
    assert_eq!((s, v["received"].as_u64()), (200, Some(200_000)), "{v}");

    // a request that breaks off: what arrived is kept
    let first = bundle[200_000..250_000].to_vec();
    let body = futures::stream::iter(vec![
        Ok::<_, std::io::Error>(first),
        Err(std::io::Error::other("connection lost")),
    ]);
    let _ = client()
        .put(format!("{}/reports/{id}/files/0", c.http))
        .bearer_auth(&key)
        .header("Upload-Offset", "200000")
        .body(reqwest::Body::wrap_stream(body))
        .send()
        .await;
    // resume: ask what arrived
    let mut received = 0;
    for _ in 0..50 {
        let (s, v) = status(&c, &id, &key).await;
        assert_eq!(s, 200, "{v}");
        assert_eq!(v["id"], id.as_str());
        assert_eq!(v["complete"], false);
        received = v["files"][0]["received"].as_u64().unwrap();
        if received > 200_000 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(
        (200_000..=250_000).contains(&received),
        "a broken request keeps what arrived: {received}"
    );
    let (s, v) = put(
        &c,
        &id,
        &key,
        0,
        received,
        bundle[received as usize..].to_vec(),
    )
    .await;
    assert_eq!((s, v["received"].as_u64()), (200, Some(300_000)), "{v}");
    assert_eq!(v["complete"], false);

    let (s, _) = put(&c, &id, &key, 1, 0, shot.clone()).await;
    assert_eq!(s, 200);
    let (s, v) = put(&c, &id, &key, 2, 0, rec[..20_000].to_vec()).await;
    assert_eq!((s, v["complete"].as_bool()), (200, Some(false)));
    let (s, v) = put(&c, &id, &key, 2, 20_000, rec[20_000..].to_vec()).await;
    assert_eq!((s, v["complete"].as_bool()), (200, Some(true)), "{v}");
    let (s, v) = status(&c, &id, &key).await;
    assert_eq!((s, v["complete"].as_bool()), (200, Some(true)), "{v}");

    // on disk under the names the coordinator picked
    let dir = report_dir(data.path(), &id);
    assert_eq!(std::fs::read(dir.join("bundle.txt")).unwrap(), bundle);
    assert_eq!(std::fs::read(dir.join("screenshot-1.jpg")).unwrap(), shot);
    assert_eq!(std::fs::read(dir.join("recording.mp4")).unwrap(), rec);
    let log = std::fs::read_to_string(dir.join("coordinator.log")).unwrap();
    assert!(log.contains("hocket-coordinator"), "{log}");
    let meta = report_json(data.path(), &id);
    assert_eq!(meta["id"], id.as_str());
    assert_eq!(meta["createdAt"], T0);
    assert!(meta["completedAt"].as_u64().is_some());
    assert_eq!(meta["username"], "alice");
    assert_eq!(meta["serverUrl"], server.as_str());
    assert_eq!(meta["note"], "Queue jumped back two songs\nafter unlocking");
    assert_eq!(meta["client"], "Hocket Test 1.0");
    assert_eq!(meta["device"], "Pixel 7 Pro");
    assert_eq!(meta["files"][0]["file"], "bundle.txt");
    assert_eq!(meta["files"][0]["name"], "../../hocket-diagnostics.txt");
    assert_eq!(meta["files"][2]["kind"], "recording");
    assert_eq!(meta["files"][2]["received"], rec.len());
    assert!(
        !meta.to_string().contains(&key),
        "the upload key is never stored"
    );

    // a finished file takes no more
    let (s, v) = put(&c, &id, &key, 1, shot.len() as u64, vec![1]).await;
    assert_eq!((s, v["error"].as_str()), (413, Some("tooLarge")), "{v}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn bad_tokens_and_keys_are_refused() {
    let subsonic = fake_subsonic().await;
    let server = format!("http://{subsonic}");
    let data = tempfile::tempdir().unwrap();
    let c = start(report_args(&server, data.path())).await;
    let token = grant_for_alice(&c, &server).await;

    let (s, v) = create(&c, &"0".repeat(64), bundle_only(10)).await;
    assert_eq!((s, v["error"].as_str()), (401, Some("unauthorized")), "{v}");
    let r = client()
        .post(format!("{}/reports", c.http))
        .json(&bundle_only(10))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status().as_u16(), 401);

    let (s, v) = create(&c, &token, bundle_only(10)).await;
    assert_eq!(s, 201);
    let id = v["id"].as_str().unwrap().to_string();
    let key = v["uploadKey"].as_str().unwrap().to_string();
    // the grant is not an upload key, nor is anything else
    for wrong in [token.as_str(), &"f".repeat(64)] {
        let (s, v) = put(&c, &id, wrong, 0, 0, vec![1; 10]).await;
        assert_eq!((s, v["error"].as_str()), (401, Some("unauthorized")), "{v}");
        let (s, _) = status(&c, &id, wrong).await;
        assert_eq!(s, 401);
    }
    let (s, _) = status(&c, "R-20261101-zzzz", &key).await;
    assert_eq!(s, 404);
    let (s, _) = status(&c, "..%2F..%2Fetc", &key).await;
    assert_eq!(s, 404);
    let (s, _) = put(&c, &id, &key, 9, 0, vec![1]).await;
    assert_eq!(s, 404);

    // expired
    c.advance(6 * HOUR + 1);
    let (s, v) = create(&c, &token, bundle_only(10)).await;
    assert_eq!((s, v["error"].as_str()), (401, Some("unauthorized")), "{v}");
    // the upload key outlives the grant
    let (s, _) = put(&c, &id, &key, 0, 0, vec![1; 10]).await;
    assert_eq!(s, 200);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn limits_are_enforced() {
    let subsonic = fake_subsonic().await;
    let server = format!("http://{subsonic}");
    let data = tempfile::tempdir().unwrap();
    let c = start(report_args(&server, data.path())).await;
    let token = grant_for_alice(&c, &server).await;
    let bundle = json!({ "kind": "bundle", "size": 10 });
    let shot = json!({ "kind": "screenshot", "size": 10, "name": "s.jpg" });
    let rec = json!({ "kind": "recording", "size": 10, "name": "r.mp4" });

    let cases: Vec<(Value, u16)> = vec![
        (
            json!({ "files": [bundle, shot, shot, shot, shot, shot, shot] }),
            400,
        ),
        (
            json!({ "files": [bundle, shot, shot, shot, shot, shot, rec] }),
            201,
        ),
        (json!({ "files": [bundle, rec, rec] }), 400),
        (json!({ "files": [shot, bundle] }), 400),
        (json!({ "files": [bundle, bundle] }), 400),
        (json!({ "files": [] }), 400),
        (
            json!({ "files": [bundle, { "kind": "video", "size": 1 }] }),
            400,
        ),
        (json!({ "files": [{ "kind": "bundle", "size": 0 }] }), 400),
        (
            json!({ "files": [{ "kind": "bundle", "size": 16 * 1024 * 1024 + 1 }] }),
            413,
        ),
        (
            json!({ "files": [bundle, { "kind": "screenshot", "size": 25 * 1024 * 1024 + 1 }] }),
            413,
        ),
        (
            json!({ "files": [bundle, { "kind": "recording", "size": 1024u64 * 1024 * 1024 + 1 }] }),
            413,
        ),
        (json!({ "note": "x".repeat(8001), "files": [bundle] }), 400),
        (json!({ "note": "é".repeat(8000), "files": [bundle] }), 201),
        (json!({ "device": "x".repeat(201), "files": [bundle] }), 400),
        (
            json!({ "files": [{ "kind": "bundle", "size": 1, "name": "x".repeat(256) }] }),
            400,
        ),
        (json!({ "note": "no files" }), 400),
    ];
    for (body, want) in cases {
        let (s, v) = create(&c, &token, body.clone()).await;
        assert_eq!(s, want, "{body}: {v}");
    }
    // a body over 64 KiB
    let (s, v) = create(
        &c,
        &token,
        json!({ "note": "x".repeat(70_000), "files": [bundle] }),
    )
    .await;
    assert_eq!((s, v["error"].as_str()), (413, Some("tooLarge")), "{v}");
    let (s, v) = create(&c, &token, json!("not an object")).await;
    assert_eq!((s, v["error"].as_str()), (400, Some("badRequest")), "{v}");

    // a body beyond the declared size, and beyond 8 MiB
    let (s, v) = create(
        &c,
        &token,
        json!({ "files": [{ "kind": "bundle", "size": 100 }, { "kind": "recording", "size": 10 * 1024 * 1024 }] }),
    )
    .await;
    assert_eq!(s, 201);
    let id = v["id"].as_str().unwrap().to_string();
    let key = v["uploadKey"].as_str().unwrap().to_string();
    let (s, v) = put(&c, &id, &key, 0, 0, vec![1; 101]).await;
    assert_eq!((s, v["error"].as_str()), (413, Some("tooLarge")), "{v}");
    // the same without a Content-Length: caught while streaming
    let chunks = futures::stream::iter(vec![
        Ok::<_, std::io::Error>(vec![1u8; 60]),
        Ok(vec![1u8; 60]),
    ]);
    let r = client()
        .put(format!("{}/reports/{id}/files/0", c.http))
        .bearer_auth(&key)
        .header("Upload-Offset", "0")
        .body(reqwest::Body::wrap_stream(chunks))
        .send()
        .await;
    if let Ok(r) = r {
        assert_eq!(r.status().as_u16(), 413);
    }
    // (the coordinator answers before reading the body, so the client may see the
    // connection close instead of the answer)
    let r = client()
        .put(format!("{}/reports/{id}/files/1", c.http))
        .bearer_auth(&key)
        .header("Upload-Offset", "0")
        .body(vec![1; 8 * 1024 * 1024 + 1])
        .send()
        .await;
    if let Ok(r) = r {
        assert_eq!(r.status().as_u16(), 413);
    }
    // nothing of a refused request is kept
    let (_, v) = status(&c, &id, &key).await;
    assert_eq!(v["files"][0]["received"], 0);
    assert_eq!(v["files"][1]["received"], 0);
    let (s, v) = put(&c, &id, &key, 1, 0, vec![1; 8 * 1024 * 1024]).await;
    assert_eq!(
        (s, v["received"].as_u64()),
        (200, Some(8 * 1024 * 1024)),
        "{v}"
    );
    // a missing offset
    let r = client()
        .put(format!("{}/reports/{id}/files/0", c.http))
        .bearer_auth(&key)
        .body(vec![1; 10])
        .send()
        .await
        .unwrap();
    assert_eq!(r.status().as_u16(), 400);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn twenty_reports_an_hour_per_account() {
    let subsonic = fake_subsonic().await;
    let server = format!("http://{subsonic}");
    let data = tempfile::tempdir().unwrap();
    let c = start(report_args(&server, data.path())).await;
    let token = grant_for_alice(&c, &server).await;
    for i in 0..20 {
        let (s, v) = create(&c, &token, bundle_only(10)).await;
        assert_eq!(s, 201, "report {i}: {v}");
        c.advance(1000);
    }
    let (s, v) = create(&c, &token, bundle_only(10)).await;
    assert_eq!((s, v["error"].as_str()), (429, Some("rateLimited")), "{v}");
    // a fresh grant is the same account
    let other = grant_for_alice(&c, &server).await;
    let (s, _) = create(&c, &other, bundle_only(10)).await;
    assert_eq!(s, 429);
    c.advance(HOUR);
    let (s, _) = create(&c, &token, bundle_only(10)).await;
    assert_eq!(s, 201);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_quota_deletes_the_oldest_finished_reports() {
    let subsonic = fake_subsonic().await;
    let server = format!("http://{subsonic}");
    let data = tempfile::tempdir().unwrap();
    let mut args = report_args(&server, data.path());
    args.reports_quota_mb = 1;
    let c = start(args).await;
    let token = grant_for_alice(&c, &server).await;

    let (a, _) = complete_report(&c, &token, 300_000).await;
    c.advance(1000);
    let (b, _) = complete_report(&c, &token, 300_000).await;
    c.advance(1000);
    // 700 KB more does not fit next to both: the oldest goes
    let (s, v) = create(&c, &token, bundle_only(500_000)).await;
    assert_eq!(s, 201, "{v}");
    let unfinished = v["id"].as_str().unwrap().to_string();
    assert!(
        !report_dir(data.path(), &a).exists(),
        "the oldest was deleted"
    );
    assert!(report_dir(data.path(), &b).exists());
    // more than the quota at all
    let (s, v) = create(&c, &token, bundle_only(1024 * 1024 + 1)).await;
    assert_eq!((s, v["error"].as_str()), (413, Some("tooLarge")), "{v}");
    assert!(
        report_dir(data.path(), &b).exists(),
        "nothing deleted in vain"
    );
    // only an unfinished report would have to go: refused
    let (s, _) = create(&c, &token, bundle_only(600_000)).await;
    assert_eq!(s, 413);
    assert!(report_dir(data.path(), &b).exists());
    assert!(report_dir(data.path(), &unfinished).exists());
    // … until it has sat untouched for over an hour: then it counts as abandoned
    c.advance(61 * 60 * 1000);
    let (s, v) = create(&c, &token, bundle_only(600_000)).await;
    assert_eq!(s, 201, "{v}");
    assert!(!report_dir(data.path(), &unfinished).exists());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_client_deletes_an_unfinished_report_with_its_key_only() {
    let subsonic = fake_subsonic().await;
    let server = format!("http://{subsonic}");
    let data = tempfile::tempdir().unwrap();
    let c = start(report_args(&server, data.path())).await;
    let token = grant_for_alice(&c, &server).await;
    let delete = |id: String, key: String| {
        let url = format!("{}/reports/{id}", c.http);
        async move {
            client()
                .delete(url)
                .bearer_auth(key)
                .send()
                .await
                .unwrap()
                .status()
                .as_u16()
        }
    };

    let (s, v) = create(&c, &token, bundle_only(10)).await;
    assert_eq!(s, 201, "{v}");
    let id = v["id"].as_str().unwrap().to_string();
    let key = v["uploadKey"].as_str().unwrap().to_string();
    assert_eq!(delete(id.clone(), "0".repeat(64)).await, 401);
    assert!(report_dir(data.path(), &id).exists());
    assert_eq!(delete(id.clone(), key.clone()).await, 204);
    assert!(!report_dir(data.path(), &id).exists());
    assert_eq!(delete(id, key).await, 404);

    // a finished one stays
    let (done, done_key) = complete_report(&c, &token, 10).await;
    assert_eq!(delete(done.clone(), done_key).await, 409);
    assert!(report_dir(data.path(), &done).exists());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn retention_deletes_recordings_then_reports_and_unfinished_ones_early() {
    let subsonic = fake_subsonic().await;
    let server = format!("http://{subsonic}");
    let data = tempfile::tempdir().unwrap();
    let c = start(report_args(&server, data.path())).await;
    let token = grant_for_alice(&c, &server).await;

    let (s, v) = create(
        &c,
        &token,
        json!({ "files": [{ "kind": "bundle", "size": 5 }, { "kind": "recording", "size": 5, "name": "r.webm" }] }),
    )
    .await;
    assert_eq!(s, 201);
    let done = v["id"].as_str().unwrap().to_string();
    let key = v["uploadKey"].as_str().unwrap().to_string();
    put(&c, &done, &key, 0, 0, vec![1; 5]).await;
    let (_, v) = put(&c, &done, &key, 1, 0, vec![2; 5]).await;
    assert_eq!(v["complete"], true);
    let (s, v) = create(&c, &token, bundle_only(5)).await;
    assert_eq!(s, 201);
    let unfinished = v["id"].as_str().unwrap().to_string();
    let dir = report_dir(data.path(), &done);

    c.advance(47 * HOUR);
    assert_eq!(c.app.purge_reports(), Default::default());
    c.advance(2 * HOUR);
    let stats = c.app.purge_reports();
    assert_eq!((stats.reports, stats.recordings), (1, 0));
    assert!(!report_dir(data.path(), &unfinished).exists());
    assert!(dir.join("recording.webm").exists());

    c.advance(29 * DAY);
    let stats = c.app.purge_reports();
    assert_eq!((stats.reports, stats.recordings), (0, 1));
    assert!(!dir.join("recording.webm").exists());
    assert!(dir.join("bundle.txt").exists());
    let meta = report_json(data.path(), &done);
    assert!(meta["files"][1]["deletedAt"].as_u64().is_some(), "{meta}");
    assert!(meta["files"][0].get("deletedAt").is_none(), "{meta}");
    // the report reads as complete still
    let (s, v) = status(&c, &done, &key).await;
    assert_eq!((s, v["complete"].as_bool()), (200, Some(true)));
    assert_eq!(c.app.purge_reports(), Default::default(), "once only");

    c.advance(60 * DAY);
    assert_eq!(c.app.purge_reports().reports, 1);
    assert!(!dir.exists());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_cli_lists_and_shows_reports() {
    let subsonic = fake_subsonic().await;
    let server = format!("http://{subsonic}");
    let data = tempfile::tempdir().unwrap();
    let c = start(report_args(&server, data.path())).await;
    let token = grant_for_alice(&c, &server).await;

    assert_eq!(reports::list(data.path()).unwrap(), "no reports\n");
    let (old, _) = complete_report(&c, &token, 10).await;
    c.advance(HOUR);
    let (s, v) = create(
        &c,
        &token,
        json!({
            "note": format!("{}\nsecond line", "Long first line ".repeat(10)),
            "files": [
                { "kind": "bundle", "size": 2048 },
                { "kind": "screenshot", "size": 10, "name": "a.png" },
                { "kind": "screenshot", "size": 10, "name": "b.png" },
                { "kind": "recording", "size": 10, "name": "c.mov" },
            ]
        }),
    )
    .await;
    assert_eq!(s, 201);
    let new = v["id"].as_str().unwrap().to_string();

    let list = reports::list(data.path()).unwrap();
    let lines: Vec<&str> = list.lines().collect();
    assert_eq!(lines.len(), 2, "{list}");
    assert!(lines[0].starts_with(&new), "newest first: {list}");
    assert!(lines[0].contains("2026-11-01 15:00Z"), "{list}");
    assert!(lines[0].contains("alice"));
    assert!(
        lines[0].contains("bundle, 2 screenshots, recording"),
        "{list}"
    );
    assert!(lines[0].contains("incomplete"));
    assert!(lines[0].contains("Long first line"));
    assert!(lines[0].contains('…'), "truncated: {list}");
    assert!(!lines[0].contains("second line"));
    assert!(lines[1].starts_with(&old));
    assert!(lines[1].contains(" complete"));

    let show = reports::show(data.path(), &new).unwrap();
    assert!(show.contains("second line"), "the whole note: {show}");
    let root = std::fs::canonicalize(data.path()).unwrap();
    for f in [
        "bundle.txt",
        "screenshot-1.png",
        "screenshot-2.png",
        "recording.mov",
        "coordinator.log",
        "report.json",
    ] {
        let path = root.join("reports").join(&new).join(f);
        assert!(show.contains(&path.display().to_string()), "{f}: {show}");
    }
    assert!(reports::show(data.path(), "R-20261101-zzzz").is_err());
    assert!(reports::show(data.path(), "../etc").is_err());
}

/// The core's own uploader against this coordinator, end to end: the grant
/// from a diagnostics answer, the base URL derived from the `/ws` URL, a
/// multi-chunk recording and a screenshot, then the files on disk.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_core_uploader_saves_a_report_here() {
    use hocket_core::api::{ReportAttachment, ReportAttachmentKind};
    use hocket_core::reports as client_reports;

    let subsonic = fake_subsonic().await;
    let server = format!("http://{subsonic}");
    let data = tempfile::tempdir().unwrap();
    let c = start(report_args(&server, data.path())).await;
    let token = grant_for_alice(&c, &server).await;

    let files = tempfile::tempdir().unwrap();
    let shot = files.path().join("shot.jpg");
    std::fs::write(&shot, vec![7u8; 1234]).unwrap();
    // Over two 4 MiB chunks.
    let recording: Vec<u8> = (0..(9u32 << 20)).map(|i| (i % 251) as u8).collect();
    let rec = files.path().join("screen.mp4");
    std::fs::write(&rec, &recording).unwrap();
    let attachments = [
        ReportAttachment {
            path: shot.to_string_lossy().into(),
            kind: ReportAttachmentKind::Screenshot,
            delete_after: false,
        },
        ReportAttachment {
            path: rec.to_string_lossy().into(),
            kind: ReportAttachmentKind::Recording,
            delete_after: false,
        },
    ];

    let staging = tempfile::tempdir().unwrap();
    let base = client_reports::base_url(&c.ws).expect("base url from the ws url");
    assert_eq!(base, c.http);
    let mut job = client_reports::stage(
        staging.path(),
        client_reports::NewReport {
            note: "queue jumped back",
            bundle: "Hocket diagnostics\nall good\n",
            attachments: &attachments,
            base_url: base,
            client: "Hocket test".into(),
            device: "laptop".into(),
            started_at: 0.0,
        },
    )
    .unwrap();
    let http = client_reports::http_client("hocket-test").unwrap();
    client_reports::upload(
        &http,
        staging.path(),
        &mut job,
        Some(&token),
        &[],
        Arc::new(|_| {}),
    )
    .await
    .unwrap();

    let id = job.report_id.clone().expect("report id");
    let meta = report_json(data.path(), &id);
    assert!(meta["completedAt"].is_number(), "{meta}");
    assert_eq!(meta["note"], "queue jumped back");
    assert_eq!(meta["username"], "alice");
    let dir = report_dir(data.path(), &id);
    assert_eq!(
        std::fs::read_to_string(dir.join("bundle.txt")).unwrap(),
        "Hocket diagnostics\nall good\n"
    );
    assert_eq!(
        std::fs::read(dir.join("screenshot-1.jpg")).unwrap(),
        vec![7u8; 1234]
    );
    assert_eq!(std::fs::read(dir.join("recording.mp4")).unwrap(), recording);
    assert!(dir.join("coordinator.log").exists());
}
