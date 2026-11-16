//! Shared by the coordinator's integration tests: a served app, identities
//! that claimed it or were invited, and devices that join their identity's
//! room through the real handshake.

#![allow(dead_code)]

use std::sync::Arc;
use std::time::Duration;

use futures::{SinkExt, StreamExt};
use tokio_tungstenite::tungstenite::Message;

use hocket_coordinator::{serve, App, Args};
use hocket_core::api::{DeviceInfo, Platform};
use hocket_core::connect::auth::{self, Role};
use hocket_core::connect::diagnostics::HELLO_FLAG;
use hocket_core::connect::wire::{Msg, WireMessage, PROTOCOL, PROTOCOL_MIN};
use hocket_core::identity::{testing, Identity, COORDINATOR_ID};

pub type Ws =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

pub async fn start(args: Args) -> (Arc<App>, String, tokio::sync::oneshot::Sender<()>) {
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

pub fn frame(m: Msg) -> Message {
    Message::text(WireMessage::new(m).encode().unwrap())
}

pub fn device(id: &str) -> DeviceInfo {
    DeviceInfo {
        id: id.into(),
        name: format!("Device {id}"),
        platform: Platform::Linux,
        app_version: "test".into(),
        playing: false,
        ready: false,
        last_seen: 0.0,
        is_self: true,
    }
}

/// Send frames and collect what comes back until the socket closes (or
/// 3 s pass).
pub async fn exchange(url: &str, frames: Vec<Message>) -> Vec<Msg> {
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

/// Read until `stop` matches (or 5 s pass); returns everything read.
pub async fn until(ws: &mut Ws, stop: impl Fn(&Msg) -> bool) -> Vec<Msg> {
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

/// A new identity (first device `device_id`) that claims the coordinator.
pub async fn owner(app: &App, url: &str, device_id: &str) -> Identity {
    let me = Identity::create(device_id, device_id, 1.0);
    let claim = app.identities().issue_claim(app.now_ms(), 60_000).unwrap();
    let got = exchange(
        url,
        vec![frame(Msg::CoordClaim {
            code: claim.code,
            sync: Box::new(me.sync()),
        })],
    )
    .await;
    assert!(
        matches!(got.first(), Some(Msg::CoordAdmitted { owner: true, .. })),
        "{got:?}"
    );
    me
}

/// A new identity admitted through an invite of `owner_identity`.
pub async fn invited(app: &App, url: &str, owner_identity: &str, device_id: &str) -> Identity {
    let me = Identity::create(device_id, device_id, 1.0);
    let code = app
        .identities()
        .create_invite(owner_identity, app.now_ms())
        .unwrap();
    let got = exchange(
        url,
        vec![frame(Msg::CoordRedeem {
            code,
            sync: Box::new(me.sync()),
        })],
    )
    .await;
    assert!(
        matches!(got.first(), Some(Msg::CoordAdmitted { owner: false, .. })),
        "{got:?}"
    );
    me
}

/// Another device paired into `identity` (the coordinator learns of it
/// from the device's own proof).
pub fn pair(identity: &mut Identity, device_id: &str) -> Identity {
    testing::pair(identity, device_id, 2.0)
}

/// The handshake as a device of `me`; `Ok` once welcomed, else what the
/// coordinator said.
pub async fn try_join(
    app: &App,
    url: &str,
    me: &mut Identity,
    answers: bool,
) -> Result<Ws, Vec<Msg>> {
    let (mut ws, _) = tokio_tungstenite::connect_async(url).await.unwrap();
    let nonce = auth::new_nonce();
    ws.send(frame(Msg::Challenge {
        nonce: nonce.clone(),
        identity: Some(me.id.clone()),
        heads: me.heads(),
        eph: None,
    }))
    .await
    .unwrap();
    let got = until(&mut ws, |m| {
        matches!(m, Msg::Challenge { .. } | Msg::Refuse { .. })
    })
    .await;
    let mut theirs = None;
    let mut leader_heads = vec![];
    for m in &got {
        match m {
            Msg::Proof {
                device_id,
                sig,
                heads,
                sync,
                ..
            } => {
                if let Some(s) = sync {
                    me.merge((**s).clone(), f64::INFINITY).unwrap();
                }
                assert_eq!(device_id, COORDINATOR_ID);
                assert!(
                    auth::verify_with_key(
                        &app.identities().keys().sign,
                        Role::Leader,
                        &me.id,
                        &nonce,
                        device_id,
                        heads,
                        sig
                    ),
                    "the coordinator proves its key"
                );
                leader_heads = heads.clone();
            }
            Msg::Challenge { nonce, .. } => theirs = Some(nonce.clone()),
            _ => {}
        }
    }
    let Some(theirs) = theirs else {
        return Err(got);
    };
    let heads = me.heads();
    ws.send(frame(Msg::Proof {
        device_id: me.device_id.clone(),
        sig: auth::prove(
            me.secret(),
            Role::Joiner,
            &me.id,
            &theirs,
            &me.device_id,
            &heads,
        ),
        heads,
        sync: Some(Box::new(me.shared.sync_for(&leader_heads))),
        eph: None,
    }))
    .await
    .unwrap();
    ws.send(frame(Msg::Hello {
        device: device(&me.device_id),
        protocol_min: PROTOCOL_MIN,
        protocol_max: PROTOCOL,
        scope: me.id.clone(),
        session_id: None,
        session_revision: 0,
        held_epoch: None,
        extra: if answers {
            [(HELLO_FLAG.to_string(), serde_json::Value::Bool(true))]
                .into_iter()
                .collect()
        } else {
            Default::default()
        },
    }))
    .await
    .unwrap();
    let got = until(&mut ws, |m| {
        matches!(m, Msg::Welcome { .. } | Msg::Refuse { .. })
    })
    .await;
    match got.last() {
        Some(Msg::Welcome { .. }) => Ok(ws),
        _ => Err(got),
    }
}

/// [`try_join`] that must succeed.
pub async fn join(app: &App, url: &str, me: &mut Identity) -> Ws {
    try_join(app, url, me, true)
        .await
        .unwrap_or_else(|got| panic!("not admitted: {got:?}"))
}
