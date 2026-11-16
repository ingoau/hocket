//! The coordinator address follows the identity's pin: claiming one sets it,
//! and when the pin goes (this device starts over or leaves the identity) the
//! address goes too, so nothing keeps dialling a coordinator Settings no
//! longer shows. (A pin dropped by a peer's member-log change takes the same
//! path, `apply_coordinator_pin`; its decision is unit-tested in
//! `core::handlers::identity`.)

#![cfg(feature = "sim")]

use std::sync::Arc;

use hocket_core::api::*;
use hocket_core::connect::transport::PeerIds;
use hocket_core::connect::wire::{Msg, WireMessage};
use hocket_core::core::io::memory::MemoryNet;
use hocket_core::core::io::ConnectIo;
use hocket_core::core::test_support::{seeded_server, TestCore};
use hocket_core::sim::SimTime;

/// A coordinator that admits any claim (owner) and answers nothing else; its URL.
async fn claimable_coordinator(net: &Arc<MemoryNet>) -> String {
    let io = net.io("coordinator");
    let secret = hocket_core::identity::crypto::DeviceSecret::from_seed(b"pin-coordinator");
    let mut listener = io.listen(Arc::new(PeerIds::default())).await.unwrap();
    let url = MemoryNet::coordinator_url_for(listener.port());
    tokio::spawn(async move {
        while let Some(mut conn) = listener.accept().await {
            let keys = secret.public();
            tokio::spawn(async move {
                while let Some(msg) = conn.rx.recv().await {
                    if let Msg::CoordClaim { .. } = msg.msg {
                        let _ = conn.tx.send(WireMessage::new(Msg::CoordAdmitted {
                            keys: keys.clone(),
                            owner: true,
                        }));
                    }
                }
            });
        }
    });
    url
}

fn coordinator_setting(s: &Snapshot) -> Option<String> {
    s.settings
        .iter()
        .find(|x| x.key == "connect.coordinatorUrl")
        .and_then(|x| {
            serde_json::from_str::<Option<String>>(&x.value)
                .ok()
                .flatten()
        })
}

async fn claim(t: &TestCore, url: &str) {
    t.run(Command::ClaimCoordinator {
        url: url.into(),
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
    let s = t.snapshot().await;
    assert_eq!(
        s.identity.coordinator.as_ref().map(|c| c.url.as_str()),
        Some(url)
    );
    assert_eq!(coordinator_setting(&s).as_deref(), Some(url));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn starting_over_forgets_the_pinned_coordinators_address() {
    let net = MemoryNet::new();
    let clock = SimTime::new(1_700_000_000_000.0);
    let t = TestCore::start_on("pin", seeded_server(2, 200.0), Some(net.clone()), 1, clock).await;
    let url = claimable_coordinator(&net).await;
    claim(&t, &url).await;

    t.run(Command::ResetIdentity).await;
    t.run_for(1_000.0).await;
    let s = t.snapshot().await;
    assert!(s.identity.coordinator.is_none());
    assert_eq!(
        coordinator_setting(&s),
        None,
        "the address went with the pin"
    );
    assert_eq!(s.connection.coordinator_url, None);
    assert_ne!(s.connection.tier, ConnectionTier::Coordinator);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_address_set_without_a_pin_is_left_alone() {
    let t = TestCore::start("nopin", seeded_server(2, 200.0)).await;
    t.run(Command::SetCoordinatorUrl {
        url: Some("wss://elsewhere.example/ws".into()),
    })
    .await;
    t.run(Command::ResetIdentity).await;
    let s = t.snapshot().await;
    assert_eq!(
        coordinator_setting(&s).as_deref(),
        Some("wss://elsewhere.example/ws")
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn leaving_the_identity_forgets_the_pinned_coordinators_address() {
    let net = MemoryNet::new();
    let clock = SimTime::new(1_700_000_000_000.0);
    let t = TestCore::start_on(
        "leaver",
        seeded_server(2, 200.0),
        Some(net.clone()),
        1,
        clock,
    )
    .await;
    let url = claimable_coordinator(&net).await;
    claim(&t, &url).await;

    // Removing itself: this device starts over with an identity of its own, unpinned.
    t.run(Command::RemoveDevice {
        device_id: t.device_id.clone(),
    })
    .await;
    t.run_for(1_000.0).await;
    let s = t.snapshot().await;
    assert!(s.identity.coordinator.is_none());
    assert_eq!(coordinator_setting(&s), None);
    assert_eq!(s.connection.coordinator_url, None);
}
