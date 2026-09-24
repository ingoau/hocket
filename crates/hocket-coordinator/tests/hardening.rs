//! The unauthenticated surface: refused sockets persist nothing, the verify
//! proxy cannot be pointed at this host's own network and never follows a
//! redirect, oversized frames end the socket, and one address cannot open
//! connections without bound.

use std::net::SocketAddr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use axum::routing::get;
use axum::Router;
use futures::{SinkExt, StreamExt};
use tokio_tungstenite::tungstenite::Message;

use hocket_coordinator::{serve, App, Args, MAX_FRAME_BYTES};
use hocket_core::api::{DeviceInfo, Platform};
use hocket_core::connect::wire::{scope_key, Credential, Msg, RefuseReason, WireMessage};
use hocket_core::connect::wire::{PROTOCOL, PROTOCOL_MIN};

/// A fake Subsonic that counts what reaches it. `/rest/ping.view` answers
/// ok for the token `good-token`, or redirects to `/elsewhere` when
/// `redirect` is set; `/elsewhere` always answers ok.
struct FakeSubsonic {
    addr: SocketAddr,
    pings: Arc<AtomicUsize>,
    elsewhere: Arc<AtomicUsize>,
}

async fn fake_subsonic(redirect: bool) -> FakeSubsonic {
    let pings = Arc::new(AtomicUsize::new(0));
    let elsewhere = Arc::new(AtomicUsize::new(0));
    let p = pings.clone();
    let e = elsewhere.clone();
    let ping = move |q: axum::extract::Query<std::collections::HashMap<String, String>>| {
        let p = p.clone();
        async move {
            p.fetch_add(1, Ordering::SeqCst);
            if redirect {
                return axum::response::Redirect::temporary("/elsewhere").into_response();
            }
            let ok = q.get("t").map(|t| t == "good-token").unwrap_or(false);
            let status = if ok { "ok" } else { "failed" };
            axum::Json(serde_json::json!({ "subsonic-response": { "status": status } }))
                .into_response()
        }
    };
    let other = move || {
        let e = e.clone();
        async move {
            e.fetch_add(1, Ordering::SeqCst);
            axum::Json(serde_json::json!({ "subsonic-response": { "status": "ok" } }))
        }
    };
    use axum::response::IntoResponse;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(
            listener,
            Router::new()
                .route("/rest/ping.view", get(ping))
                .route("/elsewhere", get(other)),
        )
        .await
        .unwrap();
    });
    FakeSubsonic {
        addr,
        pings,
        elsewhere,
    }
}

async fn start(args: Args) -> (Arc<App>, String, tokio::sync::oneshot::Sender<()>) {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(std::env::var("RUST_LOG").unwrap_or_else(|_| "info".into()))
        .try_init();
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

fn hello(scope: &str, credential: Option<Credential>) -> Message {
    let m = WireMessage::new(Msg::Hello {
        device: DeviceInfo {
            id: "raw".into(),
            name: "raw".into(),
            platform: Platform::Linux,
            app_version: "test".into(),
            playing: false,
            ready: false,
            last_seen: 0.0,
            is_self: true,
        },
        protocol_min: PROTOCOL_MIN,
        protocol_max: PROTOCOL,
        scope: scope.into(),
        credential,
        session_id: None,
        session_revision: 0,
        held_epoch: None,
        extra: Default::default(),
    });
    Message::text(m.encode().unwrap())
}

fn credential(server_url: &str, token: &str) -> Credential {
    Credential {
        server_url: server_url.into(),
        username: "alice".into(),
        token: Some(token.into()),
        salt: Some("s".into()),
        api_key: None,
        client: "hocket-test".into(),
        api_version: "1.16.1".into(),
    }
}

/// Send one frame and collect what comes back until the socket closes (or
/// 3 s pass).
async fn exchange(url: &str, frames: Vec<Message>) -> Vec<Msg> {
    let (mut ws, _) = tokio_tungstenite::connect_async(url).await.unwrap();
    for f in frames {
        ws.send(f).await.unwrap();
    }
    let mut got = vec![];
    let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
    loop {
        match tokio::time::timeout_at(deadline, ws.next()).await {
            Ok(Some(Ok(Message::Text(t)))) => {
                if let Ok(m) = WireMessage::decode(&t) {
                    got.push(m.msg);
                }
            }
            Ok(Some(Ok(Message::Close(_)))) | Ok(None) | Ok(Some(Err(_))) | Err(_) => break,
            Ok(Some(Ok(_))) => {}
        }
    }
    got
}

fn replica_files(dir: &std::path::Path) -> usize {
    std::fs::read_dir(dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_string_lossy().ends_with(".json"))
        .count()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn refused_sockets_persist_nothing_and_open_no_lasting_room() {
    let dir = tempfile::tempdir().unwrap();
    let mut args = Args::for_tests();
    args.data_dir = Some(dir.path().to_path_buf());
    let (app, url, _stop) = start(args).await;

    // a frame before Hello
    let got = exchange(&url, vec![Message::text("{\"protocolVersion\":2,\"msg\":{\"type\":\"syncRequest\",\"data\":null}}")]).await;
    assert!(matches!(
        got.first(),
        Some(Msg::Refuse {
            reason: RefuseReason::Unauthorised,
            ..
        })
    ), "{got:?}");

    // a Hello without a credential, for a hundred different scopes
    for i in 0..100 {
        let got = exchange(&url, vec![hello(&format!("https://x{i}|alice"), None)]).await;
        assert!(
            matches!(
                got.first(),
                Some(Msg::Refuse {
                    reason: RefuseReason::Unauthorised,
                    ..
                })
            ),
            "{got:?}"
        );
    }
    // the refused Hello opened a room for the duration of the socket only,
    // and the closing Disconnected did not resurrect it from disk
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(app.room_count(), 0);
    assert_eq!(replica_files(dir.path()), 0, "nothing on disk for refused scopes");
    app.flush_all();
    assert_eq!(replica_files(dir.path()), 0);
    assert_eq!(app.connection_count(), 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_verify_proxy_never_fetches_private_addresses_by_default() {
    let subsonic = fake_subsonic(false).await;
    let mut args = Args::for_tests();
    args.allow_private_servers = false;
    let (app, url, _stop) = start(args).await;
    let scope = scope_key(&format!("http://{}", subsonic.addr), "alice");
    for named in [
        format!("http://{}", subsonic.addr),
        format!("http://localhost:{}", subsonic.addr.port()),
        "http://169.254.169.254/latest/meta-data".into(),
        format!("http://user:pw@{}", subsonic.addr),
        "ftp://music.example/".into(),
    ] {
        let got = exchange(&url, vec![hello(&scope, Some(credential(&named, "good-token")))]).await;
        assert!(
            matches!(
                got.first(),
                Some(Msg::Refuse {
                    reason: RefuseReason::Unauthorised,
                    ..
                })
            ),
            "{named}: {got:?}"
        );
    }
    assert_eq!(app.verification_count(), 0, "no ping was ever sent");
    assert_eq!(subsonic.pings.load(Ordering::SeqCst), 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_verify_proxy_does_not_follow_redirects() {
    let subsonic = fake_subsonic(true).await;
    let (app, url, _stop) = start(Args::for_tests()).await;
    let base = format!("http://{}", subsonic.addr);
    let scope = scope_key(&base, "alice");
    let got = exchange(&url, vec![hello(&scope, Some(credential(&base, "good-token")))]).await;
    assert!(
        matches!(
            got.first(),
            Some(Msg::Refuse {
                reason: RefuseReason::Unauthorised,
                ..
            })
        ),
        "{got:?}"
    );
    assert_eq!(app.verification_count(), 1);
    assert_eq!(subsonic.pings.load(Ordering::SeqCst), 1);
    assert_eq!(
        subsonic.elsewhere.load(Ordering::SeqCst),
        0,
        "the credential was not forwarded to the redirect target"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_good_credential_is_admitted_and_the_room_persists_only_then() {
    let subsonic = fake_subsonic(false).await;
    let dir = tempfile::tempdir().unwrap();
    let mut args = Args::for_tests();
    args.data_dir = Some(dir.path().to_path_buf());
    let (app, url, _stop) = start(args).await;
    let base = format!("http://{}", subsonic.addr);
    let scope = scope_key(&base, "alice");
    // wrong token: refused, nothing on disk
    let got = exchange(&url, vec![hello(&scope, Some(credential(&base, "wrong")))]).await;
    assert!(matches!(got.first(), Some(Msg::Refuse { .. })), "{got:?}");
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(replica_files(dir.path()), 0);
    // right token: welcomed; the room is on disk once the socket goes
    let (mut ws, _) = tokio_tungstenite::connect_async(&url).await.unwrap();
    ws.send(hello(&scope, Some(credential(&base, "good-token"))))
        .await
        .unwrap();
    let first = tokio::time::timeout(Duration::from_secs(3), ws.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    let Message::Text(t) = first else {
        panic!("{first:?}")
    };
    assert!(matches!(
        WireMessage::decode(&t).unwrap().msg,
        Msg::Welcome { .. }
    ));
    assert!(app.has_member(&scope, "raw"));
    ws.close(None).await.unwrap();
    drop(ws);
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(app.room_count(), 0);
    assert_eq!(replica_files(dir.path()), 1);
    let stored = std::fs::read_to_string(
        std::fs::read_dir(dir.path())
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path(),
    )
    .unwrap();
    assert!(!stored.contains("good-token"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn oversized_frames_end_the_socket() {
    let (app, url, _stop) = start(Args::for_tests()).await;
    let (mut ws, _) = tokio_tungstenite::connect_async(&url).await.unwrap();
    let big = format!(
        r#"{{"protocolVersion":2,"msg":{{"type":"bye","data":{{"reason":"{}"}}}}}}"#,
        "x".repeat(MAX_FRAME_BYTES + 1024)
    );
    // The server may close before the whole frame is read: sending can fail too.
    let _ = ws.send(Message::text(big)).await;
    let ended = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            match ws.next().await {
                Some(Ok(Message::Close(_))) | Some(Err(_)) | None => break,
                _ => {}
            }
        }
    })
    .await;
    assert!(ended.is_ok(), "the socket was closed");
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(app.room_count(), 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn one_address_cannot_open_connections_without_bound() {
    let mut args = Args::for_tests();
    args.per_ip_connections = 2;
    args.per_ip_rate = 3;
    let (_app, url, _stop) = start(args).await;
    let a = tokio_tungstenite::connect_async(&url).await.unwrap();
    let b = tokio_tungstenite::connect_async(&url).await.unwrap();
    // a third live one from the same address is refused before the upgrade
    let refused = tokio_tungstenite::connect_async(&url).await;
    match refused {
        Err(tokio_tungstenite::tungstenite::Error::Http(resp)) => {
            assert_eq!(resp.status(), 429);
        }
        other => panic!("expected an HTTP 429, got {:?}", other.map(|_| ())),
    }
    drop(a);
    drop(b);
    tokio::time::sleep(Duration::from_millis(300)).await;
    // the live count freed up, but the minute's tokens are spent
    let refused = tokio_tungstenite::connect_async(&url).await;
    match refused {
        Err(tokio_tungstenite::tungstenite::Error::Http(resp)) => {
            assert_eq!(resp.status(), 429);
        }
        other => panic!("expected an HTTP 429, got {:?}", other.map(|_| ())),
    }
}
