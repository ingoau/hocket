//! Developer diagnostics through the hosted coordinator: the room answers for
//! itself and routes the request and the answers under each member's real
//! identity; the coordinator adds its own section, with its whole log only
//! for the devices of the identity that claimed it.

mod common;

use futures::SinkExt;

use common::{frame as send, invited, join, owner, pair, start, try_join, until, Ws};
use hocket_coordinator::Args;
use hocket_core::connect::diagnostics::{COORDINATOR, ROOM};
use hocket_core::connect::wire::Msg;

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
    let (app, url, _stop) = start(Args::for_tests()).await;
    let mut alice = owner(&app, &url, "a").await;
    let mut alice_b = pair(&mut alice, "b");
    let mut alice_old = pair(&mut alice, "old");
    let mut alice_room = pair(&mut alice, ROOM);
    let mut a = join(&app, &url, &mut alice).await;
    let mut b: Ws = join(&app, &url, &mut alice_b).await;
    // An older build: in the room, never asked.
    let _old = try_join(&app, &url, &mut alice_old, false).await.unwrap();
    // A device named like the room: whatever it sends is not the room's answer.
    let mut fake_room = join(&app, &url, &mut alice_room).await;
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
        vec!["b".to_string(), ROOM.to_string()],
        "the room names whom it asked"
    );
    assert!(room[0].2.contains("members (4)"), "{}", room[0].2);
    assert!(room[0].2.contains("not asked"), "{}", room[0].2);
    assert!(room[0].2.contains("Device old (old)"), "{}", room[0].2);

    let _ = until(&mut fake_room, |m| {
        matches!(m, Msg::DiagnosticsRequest { .. })
    })
    .await;
    fake_room
        .send(send(Msg::DiagnosticsReply {
            request_id: "r1".into(),
            from: ROOM.into(),
            from_name: "Room".into(),
            to: "a".into(),
            part: 0,
            parts: 1,
            text: "forged room".into(),
            declined: None,
            asked: vec![],
            report_grant: None,
        }))
        .await
        .unwrap();

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
        report_grant: None,
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
    assert!(
        !got.iter()
            .any(|m| matches!(m, Msg::DiagnosticsReply { text, .. } if text == "forged room")),
        "{got:?}"
    );

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
async fn other_identities_get_the_version_only() {
    let (app, url, _stop) = start(Args::for_tests()).await;
    let alice = owner(&app, &url, "a").await;
    let mut bob_id = invited(&app, &url, &alice.id, "x").await;
    let mut bob = join(&app, &url, &mut bob_id).await;
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
    assert!(coordinator[0].2.contains("identity that claimed it"));
}
