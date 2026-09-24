//! Two full cores on the in-memory Connect network (LAN tier): shared
//! queue, handoff end to end, resume offers, settings sync.

#![cfg(feature = "sim")]

use hocket_core::api::*;
use hocket_core::core::io::memory::MemoryNet;
use hocket_core::core::test_support::{seeded_server, TestCore};
use hocket_core::sim::SimTime;

async fn pair() -> (TestCore, TestCore) {
    let server = seeded_server(6, 200.0);
    let net = MemoryNet::new();
    let clock = SimTime::new(1_700_000_000_000.0);
    let a = TestCore::start_on("a", server.clone(), Some(net.clone()), 1, clock.clone()).await;
    let b = TestCore::start_on("b", server.clone(), Some(net.clone()), 2, clock.clone()).await;
    // Both sync the mirror, discover each other and elect a LAN leader.
    TestCore::run_all_for(&[&a, &b], 8_000.0).await;
    for c in [&a, &b] {
        assert!(
            c.events
                .all()
                .iter()
                .any(|e| matches!(e, Event::SyncProgress { progress } if progress.finished)),
            "{} synced",
            c.device_id
        );
    }
    let ca = a.snapshot().await.connection;
    let cb = b.snapshot().await.connection;
    assert!(ca.connected && cb.connected, "{ca:?} {cb:?}");
    assert_eq!(ca.tier, ConnectionTier::Lan);
    assert_eq!(a.snapshot().await.devices.len(), 2);
    (a, b)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn queue_is_shared_and_handoff_moves_transport_with_position() {
    let (a, b) = pair().await;
    a.run(Command::PlayTracks {
        server_id: a.server_id.clone(),
        track_ids: vec!["t0".into(), "t1".into(), "t2".into()],
        start_index: 0,
        label: "Sel".into(),
        shuffle: false,
    })
    .await;
    TestCore::run_all_for(&[&a, &b], 2_000.0).await;
    assert_eq!(
        b.current_track_id().await.as_deref(),
        Some("t0"),
        "b follows the queue"
    );
    assert!(a.backend.is_playing());
    assert!(!b.backend.is_playing(), "only the owner plays");
    let sb = b.snapshot().await;
    assert_eq!(
        sb.transport.lease.owner.as_deref(),
        Some(a.device_id.as_str())
    );
    assert!(
        sb.transport.position.is_playing,
        "remote stamp extrapolated"
    );
    assert!(!sb.media_session.owns_transport);

    // The other device presses next: one skip, transport stays on a.
    b.run(Command::Next).await;
    TestCore::run_all_for(&[&a, &b], 1_000.0).await;
    assert_eq!(a.current_track_id().await.as_deref(), Some("t1"));
    assert_eq!(b.current_track_id().await.as_deref(), Some("t1"));
    assert!(a.backend.is_playing());
    // A pause from the non-owner is forwarded to the owner.
    b.run(Command::Pause).await;
    TestCore::run_all_for(&[&a, &b], 1_000.0).await;
    assert!(!a.backend.is_playing());
    b.run(Command::Play).await;
    TestCore::run_all_for(&[&a, &b], 1_000.0).await;
    assert!(a.backend.is_playing());

    // Handoff: the picker pre-buffers on b, then b takes over mid-track.
    TestCore::run_all_for(&[&a, &b], 30_000.0).await;
    let pos_before = a.snapshot().await.transport.position.position_ms;
    assert!(pos_before >= 30_000);
    a.run(Command::OpenHandoffPicker).await;
    TestCore::run_all_for(&[&a, &b], 2_000.0).await;
    let picker = a
        .events
        .all()
        .into_iter()
        .rev()
        .find_map(|e| match e {
            Event::HandoffPickerChanged { open, targets } => Some((open, targets)),
            _ => None,
        })
        .unwrap();
    assert!(picker.0);
    assert!(
        picker.1.iter().any(|d| d.id == b.device_id && d.ready),
        "{:?}",
        picker.1
    );
    assert!(b.backend.log().iter().any(|c| matches!(
        c,
        hocket_core::audio::scripted::ScriptedCall::PreBuffer { .. }
    )));
    a.run(Command::HandoffTo {
        device_id: b.device_id.clone(),
    })
    .await;
    TestCore::run_all_for(&[&a, &b], 2_000.0).await;
    assert!(!a.backend.is_playing(), "a released");
    assert!(b.backend.is_playing(), "b took over");
    let (key, pos) = b.backend.current().unwrap();
    assert_eq!(key, b.queue().await.current.unwrap().item.key);
    assert!(
        pos >= pos_before && pos < pos_before + 5_000,
        "landed at {pos} (was {pos_before})"
    );
    let sa = a.snapshot().await;
    assert_eq!(
        sa.transport.lease.owner.as_deref(),
        Some(b.device_id.as_str())
    );
    assert!(!sa.media_session.owns_transport);
    assert!(b.snapshot().await.media_session.owns_transport);

    // Accumulated played time travelled: the scrobble lands exactly once.
    TestCore::run_all_for(&[&a, &b], 80_000.0).await;
    let subs: Vec<_> = a
        .server
        .scrobbles()
        .into_iter()
        .filter(|s| s.submission && s.id == "t1")
        .collect();
    assert_eq!(subs.len(), 1, "one scrobble across the handoff: {subs:?}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn synced_settings_merge_last_write_wins() {
    let (a, b) = pair().await;
    a.run(Command::SetSetting {
        key: "queue.savedCap".into(),
        value: "7".into(),
    })
    .await;
    TestCore::run_all_for(&[&a, &b], 2_000.0).await;
    match b
        .query(Query::Setting {
            key: "queue.savedCap".into(),
        })
        .await
    {
        QueryResult::SettingDetail(Some(s)) => assert_eq!(s.value, "7"),
        other => panic!("{other:?}"),
    }
    // Later write wins, whoever made it.
    TestCore::run_all_for(&[&a, &b], 1_000.0).await;
    b.run(Command::SetSetting {
        key: "queue.savedCap".into(),
        value: "9".into(),
    })
    .await;
    TestCore::run_all_for(&[&a, &b], 2_000.0).await;
    match a
        .query(Query::Setting {
            key: "queue.savedCap".into(),
        })
        .await
    {
        QueryResult::SettingDetail(Some(s)) => assert_eq!(s.value, "9"),
        other => panic!("{other:?}"),
    }
    // Device-local keys never travel.
    a.run(Command::SetSetting {
        key: "display.theme".into(),
        value: "\"dark\"".into(),
    })
    .await;
    TestCore::run_all_for(&[&a, &b], 2_000.0).await;
    match b
        .query(Query::Setting {
            key: "display.theme".into(),
        })
        .await
    {
        QueryResult::SettingDetail(Some(s)) => assert_eq!(s.value, "\"system\""),
        other => panic!("{other:?}"),
    }
    // With sync off, remote writes are ignored.
    b.run(Command::SetSettingsSync { enabled: false }).await;
    a.run(Command::SetSetting {
        key: "queue.savedCap".into(),
        value: "4".into(),
    })
    .await;
    TestCore::run_all_for(&[&a, &b], 2_000.0).await;
    match b
        .query(Query::Setting {
            key: "queue.savedCap".into(),
        })
        .await
    {
        QueryResult::SettingDetail(Some(s)) => assert_eq!(s.value, "9"),
        other => panic!("{other:?}"),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn session_undo_is_shared_but_only_the_author_can_undo() {
    let (a, b) = pair().await;
    a.run(Command::PlayTracks {
        server_id: a.server_id.clone(),
        track_ids: vec!["t0".into(), "t1".into()],
        start_index: 0,
        label: "Sel".into(),
        shuffle: false,
    })
    .await;
    TestCore::run_all_for(&[&a, &b], 2_000.0).await;
    let ub = b.snapshot().await.undo;
    assert!(!ub.can_undo, "b did not do it");
    assert!(ub.history.iter().any(|e| e.device_id == a.device_id));
    assert!(a.snapshot().await.undo.can_undo);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn claiming_after_another_device_moved_on_starts_the_new_track_fresh() {
    let (a, b) = pair().await;
    a.run(Command::PlayTracks {
        server_id: a.server_id.clone(),
        track_ids: vec!["t0".into(), "t1".into(), "t2".into()],
        start_index: 0,
        label: "Sel".into(),
        shuffle: false,
    })
    .await;
    TestCore::run_all_for(&[&a, &b], 2_000.0).await;
    assert!(a.backend.is_playing());

    // a is 90 s into t0 when b takes over.
    TestCore::run_all_for(&[&a, &b], 90_000.0).await;
    a.run(Command::OpenHandoffPicker).await;
    TestCore::run_all_for(&[&a, &b], 2_000.0).await;
    a.run(Command::HandoffTo {
        device_id: b.device_id.clone(),
    })
    .await;
    TestCore::run_all_for(&[&a, &b], 2_000.0).await;
    assert!(b.backend.is_playing(), "b took over");
    // Losing the lease stops a's backend outright: nothing stays loaded
    // (no held output device), and late reports for t0 are not ours.
    assert!(!a.backend.is_playing());
    assert!(
        a.backend.current().is_none(),
        "a's backend was stopped, not just paused"
    );
    assert!(a
        .backend
        .log()
        .iter()
        .any(|c| matches!(c, hocket_core::audio::scripted::ScriptedCall::Stop)));
    let a_before = a.backend.log().len();
    a.run(Command::BackendReport {
        report: BackendReport::Playing {
            key: b.queue().await.current.unwrap().item.key,
            position_ms: 95_000,
        },
    })
    .await;
    assert!(
        !a.transport().await.position.is_playing || a.backend.log().len() == a_before,
        "a stale report for the released item does not make a look like the player"
    );

    // b skips to t1 and stops: nobody owns transport.
    b.run(Command::Next).await;
    TestCore::run_all_for(&[&a, &b], 1_000.0).await;
    assert_eq!(a.current_track_id().await.as_deref(), Some("t1"));
    b.run(Command::Stop).await;
    TestCore::run_all_for(&[&a, &b], 2_000.0).await;
    assert!(a.snapshot().await.transport.lease.owner.is_none());

    // a presses play: t1 starts from the top, not 90 s in (a's stale t0 position).
    a.run(Command::Play).await;
    TestCore::run_all_for(&[&a, &b], 2_000.0).await;
    assert!(a.backend.is_playing());
    let (key, pos) = a.backend.current().unwrap();
    assert_eq!(key, a.queue().await.current.unwrap().item.key);
    assert!(pos < 5_000, "t1 started at {pos} ms");
    let last_load = a
        .backend
        .log()
        .into_iter()
        .rev()
        .find_map(|c| match c {
            hocket_core::audio::scripted::ScriptedCall::Load { position_ms, .. } => {
                Some(position_ms)
            }
            _ => None,
        })
        .unwrap();
    assert_eq!(last_load, 0);
}

/// Mutual LAN auth: the engine's LAN key is derived from the account
/// password (`connect/mod.rs` rule 7), so a device on the same network with
/// a different password for the same account never joins the room.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_lan_peer_with_a_different_password_never_pairs() {
    let server = seeded_server(6, 200.0);
    let net = MemoryNet::new();
    let clock = SimTime::new(1_700_000_000_000.0);
    let start = |name: &'static str, seed: u64, password: &'static str| {
        let server = server.clone();
        let net = net.clone();
        let clock = clock.clone();
        async move {
            let dir = tempfile::tempdir().unwrap();
            TestCore::start_in_with(
                name,
                server,
                Some(net),
                seed,
                clock,
                dir,
                true,
                "https://music.example/",
                password,
            )
            .await
        }
    };
    let a = start("a", 1, "secret").await;
    let b = start("b", 2, "wrong").await;
    TestCore::run_all_for(&[&a, &b], 8_000.0).await;
    for c in [&a, &b] {
        let snap = c.snapshot().await;
        assert_eq!(
            snap.devices.len(),
            1,
            "{} sees only itself: {:?}",
            c.device_id,
            snap.devices
        );
        assert!(
            !(snap.connection.connected && snap.connection.tier == ConnectionTier::Lan),
            "{} joined a LAN room: {:?}",
            c.device_id,
            snap.connection
        );
    }
    // The same password on both sides is what `pair()` relies on.
    let c = start("c", 3, "secret").await;
    TestCore::run_all_for(&[&a, &b, &c], 8_000.0).await;
    assert_eq!(a.snapshot().await.devices.len(), 2, "a and c pair");
    assert_eq!(b.snapshot().await.devices.len(), 1, "b still alone");
}
