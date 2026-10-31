//! Two full cores on the in-memory Connect network (LAN tier): shared
//! queue, handoff end to end, resume offers, settings sync.

#![cfg(feature = "sim")]

use hocket_core::api::*;
use hocket_core::core::io::memory::MemoryNet;
use hocket_core::core::test_support::{seeded_server, TestCore};
use hocket_core::sim::SimTime;

async fn pair() -> (TestCore, TestCore) {
    let (a, b, _) = pair_on().await;
    (a, b)
}

/// [`pair`], also handing back the network so a core can be restarted onto it.
async fn pair_on() -> (TestCore, TestCore, std::sync::Arc<MemoryNet>) {
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
    (a, b, net)
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

/// A stall mid-track (the owner's backend runs dry and buffers) freezes the
/// owner's position, and resuming tells the room: other devices must not
/// keep counting through the stall and stay ahead until the next pause.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_buffering_stall_on_the_owner_does_not_leave_others_ahead() {
    let (a, b) = pair().await;
    a.run(Command::PlayTracks {
        server_id: a.server_id.clone(),
        track_ids: vec!["t0".into(), "t1".into()],
        start_index: 0,
        label: "Sel".into(),
        shuffle: false,
    })
    .await;
    TestCore::run_all_for(&[&a, &b], 10_000.0).await;
    let key = a.queue().await.current.unwrap().item.key;
    a.run(Command::BackendReport {
        report: BackendReport::Buffering {
            key: key.clone(),
            buffering: true,
        },
    })
    .await;
    let frozen = a.transport().await.position.position_ms;
    assert!(frozen >= 9_000, "{frozen}");
    // Five seconds with no audio. The player's position holds meanwhile, as
    // a real one does: its reports re-stamp the room when they stray.
    a.backend.stall(5_000.0);
    a.clock.advance(5_000.0);
    assert_eq!(
        a.transport().await.position.position_ms,
        frozen,
        "a stalled play does not advance"
    );
    a.run(Command::BackendReport {
        report: BackendReport::Buffering {
            key,
            buffering: false,
        },
    })
    .await;
    TestCore::run_all_for(&[&a, &b], 500.0).await;
    let seen = b.transport().await.position.position_ms;
    assert!(
        seen >= frozen && seen < frozen + 1_500,
        "b resumed from the stall point, not 5 s ahead: {seen} (stalled at {frozen})"
    );

    // Android's order out of a stall is Ready before Buffering(false): the
    // position still counts on from the resume, not from before the stall.
    let key = a.queue().await.current.unwrap().item.key;
    a.run(Command::BackendReport {
        report: BackendReport::Buffering {
            key: key.clone(),
            buffering: true,
        },
    })
    .await;
    let frozen = a.transport().await.position.position_ms;
    a.clock.advance(3_000.0);
    a.run(Command::BackendReport {
        report: BackendReport::Ready {
            key: key.clone(),
            duration_ms: None,
        },
    })
    .await;
    let t = a.transport().await;
    assert!(!t.buffering);
    assert_eq!(
        t.position.position_ms, frozen,
        "no jump by the stall's length"
    );

    // A seek ends a stall (the native engine clears it without a report).
    a.run(Command::BackendReport {
        report: BackendReport::Buffering {
            key,
            buffering: true,
        },
    })
    .await;
    a.run(Command::SeekTo {
        position_ms: 20_000,
    })
    .await;
    a.clock.advance(1_000.0);
    let t = a.transport().await;
    assert!(!t.buffering);
    assert!(
        t.position.position_ms >= 20_900,
        "{}",
        t.position.position_ms
    );
}

/// Output latency: the owner's player holds its position until the sound
/// is heard, ~200 ms after it reported playing over Bluetooth, so its
/// reports lag the stamp. The correction re-stamps the room too, so a device
/// following along (lyrics on another screen) is not left ahead.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_output_latency_correction_reaches_the_room() {
    let (a, b) = pair().await;
    a.run(Command::PlayTracks {
        server_id: a.server_id.clone(),
        track_ids: vec!["t0".into(), "t1".into()],
        start_index: 0,
        label: "Sel".into(),
        shuffle: false,
    })
    .await;
    TestCore::run_all_for(&[&a, &b], 10_000.0).await;
    let before = a.transport().await.position.position_ms;
    a.backend.stall(200.0);
    TestCore::run_all_for(&[&a, &b], 2_000.0).await;
    let here = a.transport().await.position.position_ms;
    let there = b.transport().await.position.position_ms;
    assert!(
        here < before + 1_900,
        "a took its player's lag: {here} from {before}"
    );
    assert!(
        here.abs_diff(there) <= 50,
        "b follows the corrected position: a at {here}, b at {there}"
    );
}

/// The Subsonic `time` of a scrobble is the play's ORIGINAL start, not when
/// the submitting device resumed it: whichever device ends up submitting a
/// play sends the same timestamp, so duplicates (a device judging alone
/// after the grace) collapse into one downstream.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_handed_off_play_is_scrobbled_with_its_original_start_time() {
    let (a, b) = pair().await;
    let started = a.clock.now_ms();
    a.run(Command::PlayTracks {
        server_id: a.server_id.clone(),
        track_ids: vec!["t0".into(), "t1".into()],
        start_index: 0,
        label: "Sel".into(),
        shuffle: false,
    })
    .await;
    TestCore::run_all_for(&[&a, &b], 2_000.0).await;
    assert!(a.backend.is_playing());
    // A long pause: "resumed at minus played" would land 20 s late.
    a.run(Command::Pause).await;
    TestCore::run_all_for(&[&a, &b], 20_000.0).await;
    a.run(Command::Play).await;
    TestCore::run_all_for(&[&a, &b], 30_000.0).await;
    a.run(Command::OpenHandoffPicker).await;
    TestCore::run_all_for(&[&a, &b], 2_000.0).await;
    a.run(Command::HandoffTo {
        device_id: b.device_id.clone(),
    })
    .await;
    TestCore::run_all_for(&[&a, &b], 2_000.0).await;
    assert!(b.backend.is_playing(), "b took over");
    TestCore::run_all_for(&[&a, &b], 90_000.0).await;
    let subs: Vec<_> = a
        .server
        .scrobbles()
        .into_iter()
        .filter(|s| s.submission && s.id == "t0")
        .collect();
    assert_eq!(subs.len(), 1, "{subs:?}");
    let time = subs[0].time_ms.expect("a submission carries its time");
    // Session clock vs the shared test clock: a LAN offset estimate of a
    // few ms at most.
    assert!(
        (time - started).abs() < 1_000.0,
        "submitted with the original start {started}, got {time}"
    );
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

async fn play_on(owner: &TestCore, other: &TestCore) {
    owner
        .run(Command::PlayTracks {
            server_id: owner.server_id.clone(),
            track_ids: vec!["t0".into(), "t1".into()],
            start_index: 0,
            label: "Sel".into(),
            shuffle: false,
        })
        .await;
    TestCore::run_all_for(&[owner, other], 2_000.0).await;
    assert!(owner.backend.is_playing());
}

fn gain(v: f64) -> f64 {
    hocket_core::audio::backend::volume_to_gain(v)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_volume_control_drives_the_device_playing() {
    let (a, b) = pair().await;
    // Nobody plays: each device's control sets its own volume.
    a.run(Command::SetVolume { volume: 0.8 }).await;
    b.run(Command::SetVolume { volume: 0.3 }).await;
    let tb = b.snapshot().await.transport;
    assert_eq!(tb.volume, 0.3);
    assert_eq!(tb.volume_device.as_deref(), Some(b.device_id.as_str()));

    play_on(&a, &b).await;
    // b's control now shows and drives a's volume.
    let tb = b.snapshot().await.transport;
    assert_eq!(tb.volume_device.as_deref(), Some(a.device_id.as_str()));
    assert_eq!(tb.volume, 0.8);
    assert_eq!(b.snapshot().await.media_session.volume, 0.8);
    b.run(Command::SetVolume { volume: 0.5 }).await;
    TestCore::run_all_for(&[&a, &b], 1_000.0).await;
    assert_eq!(a.snapshot().await.transport.volume, 0.5);
    assert!((a.backend.volume() - gain(0.5)).abs() < 1e-12);
    assert!(
        (b.backend.volume() - gain(0.3)).abs() < 1e-12,
        "b's own volume is untouched"
    );
    TestCore::run_all_for(&[&a, &b], 2_000.0).await;
    assert_eq!(b.snapshot().await.transport.volume, 0.5, "a's stamp agrees");

    // Mute from b mutes a and keeps a's level for the unmute.
    b.run(Command::SetMuted { muted: true }).await;
    TestCore::run_all_for(&[&a, &b], 1_000.0).await;
    assert_eq!(a.backend.volume(), 0.0);
    let ta = a.snapshot().await.transport;
    assert!(ta.muted);
    assert_eq!(ta.volume, 0.5);
    b.run(Command::SetMuted { muted: false }).await;
    TestCore::run_all_for(&[&a, &b], 1_000.0).await;
    assert!((a.backend.volume() - gain(0.5)).abs() < 1e-12);

    // A change on a itself shows up on b.
    a.run(Command::SetVolume { volume: 0.9 }).await;
    TestCore::run_all_for(&[&a, &b], 2_000.0).await;
    assert_eq!(b.snapshot().await.transport.volume, 0.9);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn handoff_plays_at_the_new_devices_own_volume() {
    let (a, b) = pair().await;
    a.run(Command::SetVolume { volume: 0.2 }).await;
    b.run(Command::SetVolume { volume: 0.7 }).await;
    play_on(&a, &b).await;
    a.run(Command::HandoffTo {
        device_id: b.device_id.clone(),
    })
    .await;
    TestCore::run_all_for(&[&a, &b], 3_000.0).await;
    assert!(b.backend.is_playing(), "b took over");
    assert!((b.backend.volume() - gain(0.7)).abs() < 1e-12);
    let ta = a.snapshot().await.transport;
    assert_eq!(ta.volume_device.as_deref(), Some(b.device_id.as_str()));
    assert_eq!(ta.volume, 0.7);
    assert!((a.backend.volume() - gain(0.2)).abs() < 1e-12);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_system_volume_is_mirrored_and_driven_remotely() {
    let (a, b) = pair().await;
    // a is a phone: the platform owns its volume.
    a.backend.use_system_volume(0.6, false);
    a.core.settle().await;
    assert_eq!(a.snapshot().await.transport.volume, 0.6);
    assert_eq!(
        a.backend.volume(),
        1.0,
        "no hidden second volume in the player"
    );
    play_on(&a, &b).await;
    assert_eq!(b.snapshot().await.transport.volume, 0.6);

    // The hardware keys on the phone show up on the other device.
    a.backend.change_system_volume(0.4, false);
    TestCore::run_all_for(&[&a, &b], 2_000.0).await;
    assert_eq!(b.snapshot().await.transport.volume, 0.4);

    // The other device's control sets the phone's system volume.
    b.run(Command::SetVolume { volume: 0.9 }).await;
    TestCore::run_all_for(&[&a, &b], 1_000.0).await;
    assert_eq!(a.backend.system_volume(), Some((0.9, false)));
    assert_eq!(a.backend.volume(), 1.0);
    b.run(Command::SetMuted { muted: true }).await;
    TestCore::run_all_for(&[&a, &b], 2_000.0).await;
    assert_eq!(a.backend.system_volume(), Some((0.9, true)));
    // The platform reports a muted stream at 0; the level is kept for the
    // unmute, here and on the other device.
    assert_eq!(a.snapshot().await.transport.volume, 0.9);
    let tb = b.snapshot().await.transport;
    assert!(tb.muted);
    assert_eq!(tb.volume, 0.9);
    b.run(Command::SetMuted { muted: false }).await;
    TestCore::run_all_for(&[&a, &b], 1_000.0).await;
    assert_eq!(a.backend.system_volume(), Some((0.9, false)));

    // Clearing the queue keeps the phone on its system volume.
    a.run(Command::Stop).await;
    a.run(Command::ClearQueue).await;
    TestCore::run_all_for(&[&a, &b], 1_000.0).await;
    a.run(Command::SetVolume { volume: 0.3 }).await;
    a.core.settle().await;
    assert_eq!(a.backend.system_volume(), Some((0.3, false)));
    assert_eq!(a.backend.volume(), 1.0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn volume_steps_do_nothing_while_the_playing_devices_volume_is_unknown() {
    let (a, b) = pair().await;
    play_on(&a, &b).await;
    // The playing device's volume is known: a step moves it.
    b.run(Command::RunAction {
        action_id: "volumeDown".into(),
        target: ActionTarget::None,
    })
    .await;
    TestCore::run_all_for(&[&a, &b], 1_000.0).await;
    assert_eq!(a.snapshot().await.transport.volume, 0.95);
    // Muted: down stays muted, up unmutes from silence.
    a.run(Command::SetMuted { muted: true }).await;
    TestCore::run_all_for(&[&a, &b], 2_000.0).await;
    b.run(Command::RunAction {
        action_id: "volumeDown".into(),
        target: ActionTarget::None,
    })
    .await;
    TestCore::run_all_for(&[&a, &b], 1_000.0).await;
    assert!(a.snapshot().await.transport.muted);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn previous_from_another_device_restarts_a_track_well_under_way() {
    let (a, b) = pair().await;
    play_on(&a, &b).await;
    a.run(Command::Next).await;
    TestCore::run_all_for(&[&a, &b], 1_000.0).await;
    assert_eq!(a.current_track_id().await.as_deref(), Some("t1"));

    // Within 3 s, Previous from b goes back, as on a.
    b.run(Command::Previous).await;
    TestCore::run_all_for(&[&a, &b], 1_000.0).await;
    assert_eq!(a.current_track_id().await.as_deref(), Some("t0"));

    // Further in, it restarts the track playing on a (the room's position
    // tells b how far in it is).
    a.run(Command::Next).await;
    TestCore::run_all_for(&[&a, &b], 6_000.0).await;
    assert_eq!(a.current_track_id().await.as_deref(), Some("t1"));
    b.run(Command::Previous).await;
    TestCore::run_all_for(&[&a, &b], 1_000.0).await;
    assert_eq!(a.current_track_id().await.as_deref(), Some("t1"));
    assert!(a.backend.current().unwrap().1 < 2_000);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn previous_goes_back_once_the_playing_devices_lease_lapses() {
    let (a, b) = pair().await;
    play_on(&b, &a).await;
    b.run(Command::Next).await;
    TestCore::run_all_for(&[&a, &b], 6_000.0).await;
    b.run(Command::Pause).await;
    TestCore::run_all_for(&[&a, &b], 1_000.0).await;
    // b goes quiet 6 s into t1: its lease lapses and nobody plays, though
    // the room's last stamp was well into the track.
    a.run_for(30_000.0).await;
    assert!(a.snapshot().await.transport.lease.owner.is_none());
    a.run(Command::Previous).await;
    a.run_for(1_000.0).await;
    assert_eq!(a.current_track_id().await.as_deref(), Some("t0"));
}

/// `b` plays, following `a`'s room (on a tie the lowest device id leads),
/// then its core restarts and rejoins inside the lease window. A room keeps
/// the lease of an owner that disconnected, so the fresh engine adopts a lease
/// naming `b` while holding nothing. `b` started at `start_index` of three
/// tracks. Returns `(b, a)`.
async fn restarted_while_the_room_still_names_it(start_index: u32) -> (TestCore, TestCore) {
    let (a, b, net) = pair_on().await;
    b.run(Command::PlayTracks {
        server_id: b.server_id.clone(),
        track_ids: vec!["t0".into(), "t1".into(), "t2".into()],
        start_index,
        label: "Sel".into(),
        shuffle: false,
    })
    .await;
    TestCore::run_all_for(&[&a, &b], 2_000.0).await;
    assert!(b.backend.is_playing());
    b.core.shutdown().await;
    let TestCore {
        clock, server, dir, ..
    } = b;
    let b = TestCore::start_in("b", server, Some(net), 2, clock, dir).await;
    TestCore::run_all_for(&[&a, &b], 2_000.0).await;
    let s = b.snapshot().await;
    assert!(s.connection.connected, "b rejoined");
    assert_eq!(
        s.transport.lease.owner.as_deref(),
        Some(b.device_id.as_str()),
        "the room still names b"
    );
    assert!(!b.backend.is_playing(), "the restarted core holds nothing");
    (b, a)
}

/// Play there claims the lease, at once, rather than asking the owner the
/// room names: this device, which would drop the request for want of it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn play_after_a_restart_the_room_outlived_starts_here() {
    let (b, a) = restarted_while_the_room_still_names_it(0).await;
    b.run(Command::Play).await;
    TestCore::run_all_for(&[&a, &b], 1_000.0).await;
    assert!(b.backend.is_playing(), "play took the transport");
}

/// So does starting something new there.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn new_music_after_a_restart_the_room_outlived_plays_here() {
    let (b, a) = restarted_while_the_room_still_names_it(0).await;
    b.run(Command::PlayTracks {
        server_id: b.server_id.clone(),
        track_ids: vec!["t3".into(), "t4".into()],
        start_index: 0,
        label: "New".into(),
        shuffle: false,
    })
    .await;
    TestCore::run_all_for(&[&a, &b], 1_000.0).await;
    assert_eq!(b.current_track_id().await.as_deref(), Some("t3"));
    assert!(b.backend.is_playing(), "the new music plays here");
}

/// And so does Previous. The room's position for the track is this device's
/// own from before the restart: there is no other player to send a restart
/// to, so it goes back a track and plays that here.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn previous_after_a_restart_the_room_outlived_plays_here() {
    let (b, a) = restarted_while_the_room_still_names_it(1).await;
    b.run(Command::Previous).await;
    TestCore::run_all_for(&[&a, &b], 1_000.0).await;
    assert_eq!(b.current_track_id().await.as_deref(), Some("t0"));
    assert!(b.backend.is_playing(), "the previous track plays here");
}

/// What would start the player making sound: `Play`, or a load that plays.
fn starts_playing(log: &[hocket_core::audio::scripted::ScriptedCall]) -> bool {
    use hocket_core::audio::scripted::ScriptedCall;
    log.iter().any(|c| {
        matches!(
            c,
            ScriptedCall::Play | ScriptedCall::Load { play: true, .. }
        )
    })
}

/// A call takes audio focus on the device that plays, and meanwhile another
/// device changes the track. The new track loads paused, and the lease
/// renewals that follow must not start it over the call.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_track_changed_elsewhere_during_a_call_stays_quiet() {
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
    a.run(Command::BackendReport {
        report: BackendReport::AudioFocusLost { transient: true },
    })
    .await;
    a.backend.clear_log();
    b.run(Command::Next).await;
    // Several lease renewals' worth of call.
    TestCore::run_all_for(&[&a, &b], 20_000.0).await;
    assert_eq!(a.current_track_id().await.as_deref(), Some("t1"));
    let log = a.backend.log();
    assert!(
        !starts_playing(&log),
        "nothing started t1 over the call: {log:?}"
    );
}

/// A device that hands the play away during a call has nothing of that call
/// left to wait for: pressing play there later starts at once.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn play_after_handing_off_during_a_call_starts_at_once() {
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
    a.run(Command::BackendReport {
        report: BackendReport::AudioFocusLost { transient: true },
    })
    .await;
    a.run(Command::OpenHandoffPicker).await;
    TestCore::run_all_for(&[&a, &b], 2_000.0).await;
    a.run(Command::HandoffTo {
        device_id: b.device_id.clone(),
    })
    .await;
    TestCore::run_all_for(&[&a, &b], 2_000.0).await;
    assert!(b.backend.is_playing(), "b took over");
    b.run(Command::Stop).await;
    TestCore::run_all_for(&[&a, &b], 2_000.0).await;
    assert!(a.snapshot().await.transport.lease.owner.is_none());

    a.run(Command::Play).await;
    // Less than a lease renewal: the press itself must start it.
    TestCore::run_all_for(&[&a, &b], 1_000.0).await;
    assert!(a.backend.is_playing(), "play started at once");
}

/// Applies ops as a build from before stable context keys (#97) did: an
/// album song made current is keyed `<op>-<n>`, where this build keys it
/// `<op>-<n>.<index>`. Everything else is this build's reducer.
struct OlderReducer;

impl hocket_core::connect::SessionReducer for OlderReducer {
    fn apply(
        &self,
        doc: &SessionDocument,
        op: &hocket_core::connect::wire::SessionOp,
        ctx: &hocket_core::connect::OpContext,
    ) -> Result<SessionDocument, hocket_core::connect::ReduceFailure> {
        let mut next =
            hocket_core::connect::session_adapter::RealReducer::default().apply(doc, op, ctx)?;
        let before = doc.current.as_ref().map(|c| c.key.clone());
        if let Some(c) = next.current.as_mut() {
            if Some(&c.key) != before.as_ref() {
                if let Some((base, _)) = c.key.rsplit_once('.') {
                    c.key = base.to_string();
                }
            }
        }
        Ok(next)
    }
}

/// The room (a leads it) runs an older build that keys the current item
/// differently. When b, playing, rejoins after a network blip and adopts the
/// room's document, the song keeps playing where it was: a new key for the
/// same item is not a new item.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_room_rekeying_the_current_item_does_not_restart_it() {
    use hocket_core::audio::scripted::ScriptedCall;
    let server = seeded_server(6, 200.0);
    let net = MemoryNet::new_partitionable();
    let clock = SimTime::new(1_700_000_000_000.0);
    let a = TestCore::start_with_reducer(
        "a",
        server.clone(),
        Some(net.clone()),
        1,
        clock.clone(),
        std::sync::Arc::new(OlderReducer),
    )
    .await;
    let b = TestCore::start_on("b", server.clone(), Some(net.clone()), 2, clock.clone()).await;
    TestCore::run_all_for(&[&a, &b], 8_000.0).await;
    let (ca, cb) = (a.snapshot().await.connection, b.snapshot().await.connection);
    assert!(ca.connected && cb.connected, "{ca:?} {cb:?}");

    b.run(Command::PlayTracks {
        server_id: b.server_id.clone(),
        track_ids: vec!["t0".into(), "t1".into(), "t2".into()],
        start_index: 1,
        label: "Sel".into(),
        shuffle: false,
    })
    .await;
    TestCore::run_all_for(&[&a, &b], 30_000.0).await;
    assert!(b.backend.is_playing());
    let ours = b.queue().await.current.unwrap().item.key;
    let theirs = a.queue().await.current.unwrap().item.key;
    assert_ne!(ours, theirs, "the room keyed the item its own way");

    let loads = |t: &TestCore| {
        t.backend
            .log()
            .iter()
            .filter(|c| matches!(c, ScriptedCall::Load { .. }))
            .count()
    };
    let loads_before = loads(&b);
    net.set_partitioned(true);
    TestCore::run_all_for(&[&a, &b], 3_000.0).await;
    net.set_partitioned(false);
    TestCore::run_all_for(&[&a, &b], 10_000.0).await;

    assert!(b.snapshot().await.connection.connected, "b rejoined");
    let current = b.queue().await.current.unwrap();
    assert_eq!(current.item.key, theirs, "b adopted the room's document");
    assert_eq!(current.item.track_id, "t1");
    assert_eq!(loads(&b), loads_before, "{:?}", b.backend.log());
    assert!(b.backend.is_playing());
    let (_, pos) = b.backend.current().unwrap();
    assert!(pos >= 40_000, "still where it was, at {pos}");
}

/// The persisted sync point (`connect:<scope>`) of a core's database.
fn persisted_sync_base(t: &TestCore) -> serde_json::Value {
    let db = t.open_db();
    let key = db
        .saved_state_keys("connect:")
        .unwrap()
        .into_iter()
        .next()
        .expect("connect state persisted");
    let json = db.saved_state_get_raw(&key).unwrap().unwrap();
    serde_json::from_str::<serde_json::Value>(&json).unwrap()["syncBase"].clone()
}

/// The room acknowledges a skip only after the document was saved (a slow
/// network): the sync point that ack moves is saved too. Were it not, skips
/// made after a restart and before the device rejoins would look like a fork
/// of a session that moved on without it, and be replaced by the room's
/// state (the queue jumping back to where it was).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn skips_made_before_rejoining_after_a_restart_are_kept() {
    let server = seeded_server(6, 200.0);
    let net = MemoryNet::new_partitionable();
    let clock = SimTime::new(1_700_000_000_000.0);
    let a = TestCore::start_on("a", server.clone(), Some(net.clone()), 1, clock.clone()).await;
    let b = TestCore::start_on("b", server.clone(), Some(net.clone()), 2, clock.clone()).await;
    TestCore::run_all_for(&[&a, &b], 8_000.0).await;
    let ids: Vec<String> = (0..6).map(|i| format!("t{i}")).collect();
    a.run(Command::PlayTracks {
        server_id: a.server_id.clone(),
        track_ids: ids.clone(),
        start_index: 0,
        label: "Sel".into(),
        shuffle: false,
    })
    .await;
    TestCore::run_all_for(&[&a, &b], 2_000.0).await;
    // The follower is the one that records a sync point with the leader.
    let ((leader, _), (follower, slot)) = if persisted_sync_base(&a).is_null() {
        ((a, 1), (b, 2))
    } else {
        ((b, 2), (a, 1))
    };
    let name = if slot == 1 { "a" } else { "b" };
    leader.run(Command::OpenHandoffPicker).await;
    TestCore::run_all_for(&[&leader, &follower], 2_000.0).await;
    leader
        .run(Command::HandoffTo {
            device_id: follower.device_id.clone(),
        })
        .await;
    TestCore::run_all_for(&[&leader, &follower], 2_000.0).await;
    assert!(follower.backend.is_playing(), "the follower plays");

    net.set_held(true);
    follower.run(Command::Next).await;
    // The document is saved; the ack is still on its way.
    follower.run_for(1_000.0).await;
    net.set_held(false);
    TestCore::run_all_for(&[&leader, &follower], 1_000.0).await;
    assert_eq!(leader.current_track_id().await.as_deref(), Some("t1"));
    let acked = follower
        .snapshot()
        .await
        .session
        .expect("a session")
        .revision;
    follower.core.shutdown().await;
    assert_eq!(
        persisted_sync_base(&follower)["revision"],
        serde_json::json!(acked),
        "the acked revision is the saved sync point"
    );

    // Restarted out of reach of the room, the follower skips twice...
    net.set_partitioned(true);
    let TestCore {
        clock, server, dir, ..
    } = follower;
    let follower = TestCore::start_in(name, server, Some(net.clone()), slot, clock, dir).await;
    follower.run_for(1_000.0).await;
    follower.run(Command::Play).await;
    follower.run(Command::Next).await;
    follower.run(Command::Next).await;
    follower.run_for(1_000.0).await;
    assert_eq!(follower.current_track_id().await.as_deref(), Some("t3"));

    // ...and rejoining keeps them: the room never moved on.
    net.set_partitioned(false);
    TestCore::run_all_for(&[&leader, &follower], 10_000.0).await;
    assert!(follower.snapshot().await.connection.connected, "rejoined");
    assert_eq!(follower.current_track_id().await.as_deref(), Some("t3"));
    assert_eq!(leader.current_track_id().await.as_deref(), Some("t3"));
    leader.core.shutdown().await;
    follower.core.shutdown().await;
}
