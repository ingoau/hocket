//! The unauthenticated surface: sockets of unknown identities (or of
//! devices their identity doesn't list) are refused and persist nothing,
//! claims need the current code, oversized frames end the socket, and one
//! address cannot open connections without bound.

mod common;

use std::time::Duration;

use futures::{SinkExt, StreamExt};
use tokio_tungstenite::tungstenite::Message;

use common::{exchange, frame, join, owner, pair, start, try_join};
use hocket_coordinator::{Args, MAX_FRAME_BYTES};
use hocket_core::connect::auth;
use hocket_core::connect::wire::{Msg, RefuseReason};
use hocket_core::identity::Identity;

fn replica_files(dir: &std::path::Path) -> usize {
    std::fs::read_dir(dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().to_string())
        .filter(|n| {
            n.ends_with(".json")
                && n != hocket_coordinator::identities::KEY_FILE
                && n != hocket_coordinator::identities::IDENTITIES_FILE
                && n != hocket_coordinator::identities::CLAIM_FILE
        })
        .count()
}

fn refused(got: &[Msg]) -> bool {
    matches!(
        got.first(),
        Some(Msg::Refuse {
            reason: RefuseReason::Unauthorised,
            ..
        })
    )
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn refused_sockets_persist_nothing_and_open_no_lasting_room() {
    let dir = tempfile::tempdir().unwrap();
    let mut args = Args::for_tests();
    args.data_dir = Some(dir.path().to_path_buf());
    let (app, url, _stop) = start(args).await;

    // a frame that names no identity
    let got = exchange(
        &url,
        vec![Message::text(
            "{\"protocolVersion\":5,\"msg\":{\"type\":\"syncRequest\",\"data\":null}}",
        )],
    )
    .await;
    assert!(refused(&got), "{got:?}");

    // a hundred identities it never admitted
    for _ in 0..100 {
        let stranger = Identity::create("dev", "Dev", 1.0);
        let got = exchange(
            &url,
            vec![frame(Msg::Challenge {
                nonce: auth::new_nonce(),
                identity: Some(stranger.id.clone()),
                heads: stranger.heads(),
            })],
        )
        .await;
        assert!(refused(&got), "{got:?}");
    }
    // a claim with a made-up code
    let stranger = Identity::create("dev", "Dev", 1.0);
    let got = exchange(
        &url,
        vec![frame(Msg::CoordClaim {
            code: "ABCD-EFGH".into(),
            sync: Box::new(stranger.sync()),
        })],
    )
    .await;
    assert!(refused(&got), "{got:?}");
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(app.room_count(), 0);
    assert_eq!(
        replica_files(dir.path()),
        0,
        "nothing on disk for refused sockets"
    );
    app.flush_all();
    assert_eq!(replica_files(dir.path()), 0);
    assert_eq!(app.connection_count(), 0);
    assert_eq!(app.identities().count(), 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn claiming_needs_the_current_code_and_an_expired_one_fails() {
    let (app, url, _stop) = start(Args::for_tests()).await;
    let a = Identity::create("a", "A", 1.0);
    let now = app.now_ms();
    let expired = app.identities().issue_claim(now - 120_000, 60_000).unwrap();
    let got = exchange(
        &url,
        vec![frame(Msg::CoordClaim {
            code: expired.code,
            sync: Box::new(a.sync()),
        })],
    )
    .await;
    assert!(refused(&got), "{got:?}");
    assert!(app.identities().owner().is_none());
    let mut a = owner(&app, &url, "a").await;
    assert_eq!(app.identities().owner().as_deref(), Some(a.id.as_str()));
    join(&app, &url, &mut a).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_member_is_admitted_and_the_room_persists_only_then() {
    let dir = tempfile::tempdir().unwrap();
    let mut args = Args::for_tests();
    args.data_dir = Some(dir.path().to_path_buf());
    let (app, url, _stop) = start(args).await;
    let mut a = owner(&app, &url, "a").await;
    // A device the identity doesn't list (its own keys, claiming a's identity): refused.
    let mut forged = Identity::create_with(
        &a.id,
        "x",
        "X",
        hocket_core::identity::crypto::DeviceSecret::generate(),
        1.0,
    );
    let got = try_join(&app, &url, &mut forged, false)
        .await
        .err()
        .unwrap();
    assert!(
        got.iter().any(|m| matches!(m, Msg::Refuse { .. })),
        "{got:?}"
    );
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(replica_files(dir.path()), 0);
    // A paired device the coordinator hasn't heard of yet brings its own add.
    let mut b = pair(&mut a, "b");
    let mut ws = join(&app, &url, &mut b).await;
    assert!(app.has_member(&b.id, "b"));
    ws.close(None).await.unwrap();
    drop(ws);
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(app.room_count(), 0);
    assert_eq!(replica_files(dir.path()), 1);
    let mut shared = app.identities().shared(&a.id).unwrap();
    assert!(shared.state().is_member("b"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_removed_device_is_turned_away() {
    let (app, url, _stop) = start(Args::for_tests()).await;
    let mut a = owner(&app, &url, "a").await;
    let mut b = pair(&mut a, "b");
    let mut c = pair(&mut a, "c");
    let mut wa = join(&app, &url, &mut a).await;
    let mut wb = join(&app, &url, &mut b).await;
    // a removes b and tells the room.
    a.remove_device("b", 5.0).unwrap();
    wa.send(frame(Msg::IdentitySync {
        sync: Box::new(a.sync()),
    }))
    .await
    .unwrap();
    // b is dropped at once.
    let got = common::until(&mut wb, |m| matches!(m, Msg::Refuse { .. })).await;
    assert!(
        got.iter().any(|m| matches!(m, Msg::Refuse { .. })),
        "{got:?}"
    );
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(!app.has_member(&a.id, "b"));
    // and can't come back with its old list.
    let got = try_join(&app, &url, &mut b, false).await.err().unwrap();
    assert!(
        got.iter().any(|m| matches!(m, Msg::Refuse { .. })),
        "{got:?}"
    );
    assert!(b.was_removed(), "the coordinator's proof told it");
    // c, which was offline, still gets in.
    join(&app, &url, &mut c).await;
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
    args.per_ip_rate = 2;
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

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_socket_that_never_speaks_is_dropped() {
    let (app, url, _stop) = start(Args::for_tests()).await;
    let (mut ws, _) = tokio_tungstenite::connect_async(&url).await.unwrap();
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(app.connection_count(), 1);
    // silent: the server hangs up after HELLO_TIMEOUT, no room was opened
    let ended = tokio::time::timeout(
        hocket_coordinator::HELLO_TIMEOUT + Duration::from_secs(3),
        async {
            loop {
                match ws.next().await {
                    None | Some(Err(_)) | Some(Ok(Message::Close(_))) => break,
                    Some(Ok(_)) => {}
                }
            }
        },
    )
    .await;
    assert!(ended.is_ok(), "the silent socket was never closed");
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(app.connection_count(), 0);
    assert_eq!(app.room_count(), 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_owner_invites_and_revokes_over_the_socket() {
    let (app, url, _stop) = start(Args::for_tests()).await;
    let mut a = owner(&app, &url, "a").await;
    let mut ws = join(&app, &url, &mut a).await;
    ws.send(frame(Msg::CoordInviteCreate)).await.unwrap();
    let got = common::until(&mut ws, |m| matches!(m, Msg::CoordInvites { .. })).await;
    let Some(Msg::CoordInvites {
        created: Some(code),
        invites,
    }) = got.last()
    else {
        panic!("{got:?}")
    };
    assert_eq!(invites.len(), 1);
    ws.send(frame(Msg::CoordInviteRevoke { code: code.clone() }))
        .await
        .unwrap();
    let got = common::until(&mut ws, |m| matches!(m, Msg::CoordInvites { .. })).await;
    assert!(
        matches!(got.last(), Some(Msg::CoordInvites { invites, .. }) if invites.is_empty()),
        "{got:?}"
    );
    // A revoked code admits nobody.
    let guest = Identity::create("g", "G", 1.0);
    let got = exchange(
        &url,
        vec![frame(Msg::CoordRedeem {
            code: code.clone(),
            sync: Box::new(guest.sync()),
        })],
    )
    .await;
    assert!(refused(&got), "{got:?}");
    assert!(!app.identities().is_admitted(&guest.id));
    // A guest identity's devices can't make invites.
    let mut g = common::invited(&app, &url, &a.id, "g").await;
    let mut gws = join(&app, &url, &mut g).await;
    gws.send(frame(Msg::CoordInviteCreate)).await.unwrap();
    let got = common::until(&mut gws, |m| matches!(m, Msg::CoordInvites { .. })).await;
    assert!(
        !got.iter().any(|m| matches!(m, Msg::CoordInvites { .. })),
        "{got:?}"
    );
}
