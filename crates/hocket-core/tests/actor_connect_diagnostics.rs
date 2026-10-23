//! Developer diagnostics across devices: three full cores on the in-memory
//! Connect network (LAN tier). One asks; the room answers for itself; a
//! device with developer mode on sends its platform's bundle (or, when the
//! platform stays silent, the core's own); a device with it off declines.

#![cfg(feature = "sim")]

use hocket_core::api::*;
use hocket_core::core::io::memory::MemoryNet;
use hocket_core::core::test_support::{seeded_server, TestCore};
use hocket_core::sim::SimTime;

async fn trio() -> (TestCore, TestCore, TestCore) {
    let server = seeded_server(4, 200.0);
    let net = MemoryNet::new();
    let clock = SimTime::new(1_700_000_000_000.0);
    let a = TestCore::start_on("a", server.clone(), Some(net.clone()), 1, clock.clone()).await;
    let b = TestCore::start_on("b", server.clone(), Some(net.clone()), 2, clock.clone()).await;
    let c = TestCore::start_on("c", server.clone(), Some(net.clone()), 3, clock.clone()).await;
    TestCore::run_all_for(&[&a, &b, &c], 8_000.0).await;
    for t in [&a, &b, &c] {
        let s = t.snapshot().await.connection;
        assert!(s.connected && s.peer_count >= 2, "{s:?}");
    }
    (a, b, c)
}

async fn developer_mode(t: &TestCore, on: bool) {
    t.run(Command::SetSetting {
        key: "developer.mode".into(),
        value: on.to_string(),
    })
    .await;
}

fn requested(t: &TestCore) -> Option<(String, String)> {
    t.events.all().into_iter().find_map(|e| match e {
        Event::DiagnosticsRequested {
            request_id,
            from_device,
        } => Some((request_id, from_device)),
        _ => None,
    })
}

/// Ask `t` for the room's diagnostics while `all` keep running; the text it answers.
async fn collect(t: &TestCore, all: &[&TestCore], answer: impl Fn(&[&TestCore])) -> String {
    let core = t.core.clone();
    let pending = tokio::spawn(async move { core.query(Query::RoomDiagnostics).await });
    for _ in 0..240 {
        TestCore::run_all_for(all, 250.0).await;
        answer(all);
        if pending.is_finished() {
            break;
        }
    }
    match pending.await.unwrap().unwrap() {
        QueryResult::Text(text) => text,
        other => panic!("{other:?}"),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn every_device_answers_or_declines_by_its_own_developer_mode() {
    let (a, b, c) = trio().await;
    developer_mode(&a, true).await;
    developer_mode(&b, true).await;
    for t in [&a, &b, &c] {
        t.events.clear();
    }

    let b_core = b.core.clone();
    let text = collect(&a, &[&a, &b, &c], |_| {
        if let Some((request_id, _)) = requested(&b) {
            let _ = b_core.dispatch(Command::AnswerDiagnosticsRequest {
                request_id,
                text: "the bundle b's platform built".into(),
            });
            b.events.clear();
        }
    })
    .await;

    assert!(text.contains("1 of 2 sent a bundle"), "{text}");
    assert!(text.contains("the bundle b's platform built"), "{text}");
    assert!(
        text.contains("(declined: developer mode is off on this device)"),
        "{text}"
    );
    assert!(text.contains("Room\n----"), "{text}");
    assert!(text.contains("members (3)"), "{text}");
    assert!(
        requested(&c).is_none(),
        "c was never asked to build anything"
    );
    assert!(requested(&a).is_none(), "nobody asks the asking device");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_silent_platform_gets_the_cores_own_bundle_sent() {
    let (a, b, c) = trio().await;
    developer_mode(&a, true).await;
    developer_mode(&b, true).await;
    developer_mode(&c, true).await;
    let text = collect(&a, &[&a, &b, &c], |_| {}).await;
    assert!(text.contains("2 of 2 sent a bundle"), "{text}");
    assert_eq!(
        text.matches("the app did not build its bundle in time")
            .count(),
        2,
        "{text}"
    );
    assert!(
        text.contains("Hocket core"),
        "the core's diagnostics: {text}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn nothing_is_asked_while_developer_mode_is_off_here() {
    let (a, b, c) = trio().await;
    developer_mode(&b, true).await;
    developer_mode(&c, true).await;
    let text = collect(&a, &[&a, &b, &c], |_| {}).await;
    assert!(
        text.contains("developer mode is off on this device"),
        "{text}"
    );
    TestCore::run_all_for(&[&a, &b, &c], 1_000.0).await;
    assert!(requested(&b).is_none() && requested(&c).is_none());
}
