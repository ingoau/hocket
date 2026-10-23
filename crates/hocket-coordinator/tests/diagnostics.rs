//! Developer diagnostics through the hosted coordinator: the room answers for
//! itself and routes the request and the answers under each member's real
//! identity; the coordinator adds its own section, with its whole log only
//! for a `--diagnostics-user` account.

use std::net::SocketAddr;
use std::time::Duration;

use axum::routing::get;
use axum::Router;
use futures::{SinkExt, StreamExt};
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

use hocket_coordinator::{serve, App, Args};
use hocket_core::api::{DeviceInfo, Platform};
use hocket_core::connect::diagnostics::{COORDINATOR, ROOM};
use hocket_core::connect::wire::{scope_key, Credential, Msg, WireMessage};
use hocket_core::connect::wire::{PROTOCOL, PROTOCOL_MIN};

type Ws = WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>;

/// A Subsonic that accepts the token `good-token` for anyone.
async fn fake_subsonic() -> SocketAddr {
    let ping = |q: axum::extract::Query<std::collections::HashMap<String, String>>| async move {
        let ok = q.get("t").map(|t| t == "good-token").unwrap_or(false);
        let status = if ok { "ok" } else { "failed" };
        axum::Json(serde_json::json!({ "subsonic-response": { "status": status } }))
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

async fn start(args: Args) -> (String, tokio::sync::oneshot::Sender<()>) {
    // The capture the coordinator binary installs, so there is a log to send.
    hocket_core::devlog::install("info", None, false);
    std::env::set_var("NO_PROXY", "127.0.0.1,localhost");
    std::env::set_var("no_proxy", "127.0.0.1,localhost");
    let app = App::new(args).unwrap();
    let (stop_tx, stop_rx) = tokio::sync::oneshot::channel::<()>();
    let (addr, _server) = serve(app, "127.0.0.1:0".parse().unwrap(), async {
        let _ = stop_rx.await;
    })
    .await
    .unwrap();
    (format!("ws://{addr}/ws"), stop_tx)
}

fn send(m: Msg) -> Message {
    Message::text(WireMessage::new(m).encode().unwrap())
}

/// A device of `user` on `server`, admitted (its `Welcome` read).
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
        extra: Default::default(),
    }))
    .await
    .unwrap();
    let got = until(&mut ws, |m| matches!(m, Msg::Welcome { .. })).await;
    assert!(matches!(got.last(), Some(Msg::Welcome { .. })), "{got:?}");
    ws
}

/// Read until `stop` matches (or 5 s pass); returns everything read.
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

fn replies(got: &[Msg], source: &str) -> Vec<(u32, u32, String, Vec<String>)> {
    got.iter()
        .filter_map(|m| match m {
            Msg::DiagnosticsReply {
                from,
                part,
                parts,
                text,
                asked,
                ..
            } if from == source => Some((
                *part,
                *parts,
                text.clone(),
                asked.iter().map(|d| d.id.clone()).collect(),
            )),
            _ => None,
        })
        .collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn room_and_coordinator_answer_and_devices_are_routed_under_their_identity() {
    let subsonic = fake_subsonic().await;
    let server = format!("http://{subsonic}");
    let mut args = Args::for_tests();
    args.diagnostics_users = vec![format!("alice@{server}/")];
    let (url, _stop) = start(args).await;

    let mut a = join(&url, &server, "alice", "a").await;
    let mut b = join(&url, &server, "alice", "b").await;
    a.send(send(Msg::DiagnosticsRequest {
        request_id: "r1".into(),
        from: "a".into(),
    }))
    .await
    .unwrap();

    let got = until(
        &mut a,
        |m| matches!(m, Msg::DiagnosticsReply { from, .. } if from == ROOM),
    )
    .await;
    let coordinator = replies(&got, COORDINATOR);
    assert!(
        !coordinator.is_empty(),
        "the coordinator's first part comes ahead of the room's: {got:?}"
    );
    assert!(coordinator[0].2.contains("hocket-coordinator"));
    assert!(
        coordinator[0].2.contains("Process log"),
        "{}",
        coordinator[0].2
    );
    let room = replies(&got, ROOM);
    assert_eq!(
        room[0].3,
        vec!["b".to_string()],
        "the room names whom it asked"
    );
    assert!(room[0].2.contains("members (2)"), "{}", room[0].2);

    // b is asked on a's behalf, and its answer reaches a as b's, whatever it claims
    let got_b = until(&mut b, |m| matches!(m, Msg::DiagnosticsRequest { .. })).await;
    assert!(
        matches!(got_b.last(), Some(Msg::DiagnosticsRequest { request_id, from }) if request_id == "r1" && from == "a"),
        "{got_b:?}"
    );
    b.send(send(Msg::DiagnosticsReply {
        request_id: "r1".into(),
        from: COORDINATOR.into(),
        from_name: "Not really".into(),
        to: "a".into(),
        part: 0,
        parts: 1,
        text: "hello from b".into(),
        declined: None,
        asked: vec![],
    }))
    .await
    .unwrap();
    let got = until(
        &mut a,
        |m| matches!(m, Msg::DiagnosticsReply { text, .. } if text == "hello from b"),
    )
    .await;
    let from_b = replies(&got, "b");
    assert_eq!(from_b.len(), 1, "{got:?}");

    // asking again at once is turned down by the room
    a.send(send(Msg::DiagnosticsRequest {
        request_id: "r2".into(),
        from: "a".into(),
    }))
    .await
    .unwrap();
    let got = until(&mut a, |m| matches!(m, Msg::DiagnosticsReply { from, request_id, .. } if from == ROOM && request_id == "r2")).await;
    assert!(
        matches!(
            got.last(),
            Some(Msg::DiagnosticsReply {
                declined: Some(_),
                ..
            })
        ),
        "{got:?}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn other_accounts_get_the_version_only() {
    let subsonic = fake_subsonic().await;
    let server = format!("http://{subsonic}");
    let mut args = Args::for_tests();
    args.diagnostics_users = vec![format!("alice@{server}")];
    let (url, _stop) = start(args).await;

    let mut bob = join(&url, &server, "bob", "x").await;
    bob.send(send(Msg::DiagnosticsRequest {
        request_id: "r".into(),
        from: "x".into(),
    }))
    .await
    .unwrap();
    let got = until(
        &mut bob,
        |m| matches!(m, Msg::DiagnosticsReply { from, .. } if from == ROOM),
    )
    .await;
    let coordinator = replies(&got, COORDINATOR);
    assert_eq!(coordinator.len(), 1, "{got:?}");
    assert!(coordinator[0].2.contains("hocket-coordinator"));
    assert!(
        !coordinator[0].2.contains("Process log"),
        "{}",
        coordinator[0].2
    );
    assert!(coordinator[0].2.contains("--diagnostics-user"));
}

#[test]
fn diagnostics_user_flags_are_checked() {
    let mut args = Args::for_tests();
    args.diagnostics_users = vec!["alice".into()];
    assert!(
        App::new(args.clone()).is_err(),
        "a bare user needs --verify-url"
    );
    args.verify_url = Some("https://music.example".into());
    assert!(App::new(args.clone()).is_ok());
    args.no_verify = true;
    assert!(App::new(args).is_err(), "never with --no-verify");
}
