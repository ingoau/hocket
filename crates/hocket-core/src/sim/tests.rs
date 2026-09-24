//! Scripted scenarios for each design rule, then hundreds of seeded random
//! ones. Every scenario runs the real engine, room and reducer.

use super::world::{Action, Topology, World, WorldConfig};
use crate::api::TrackId;
use crate::sim::network::Conditions;

fn tracks(w: &World, n: usize) -> Vec<TrackId> {
    w.library.ids().into_iter().take(n).collect()
}

fn owner(w: &World) -> Option<String> {
    w.devices.iter().find(|d| d.owns()).map(|d| d.id.clone())
}

fn coordinator_world(seed: u64, devices: usize) -> World {
    let mut cfg = WorldConfig::new(seed);
    cfg.devices = devices;
    cfg.keep_logs = true;
    let mut w = World::new(cfg);
    w.run_for(3_000.0);
    for d in &w.devices {
        assert!(
            d.engine.is_connected(),
            "{} should be attached after 3 s",
            d.id
        );
    }
    w
}

#[test]
fn two_devices_share_a_queue_and_one_plays() {
    let mut w = coordinator_world(1, 2);
    let t = tracks(&w, 4);
    w.perform(Action::PlayTracks {
        device: 0,
        tracks: t.clone(),
    });
    w.perform(Action::ClaimTransport {
        device: 0,
        takeover: false,
    });
    w.run_for(2_000.0);
    assert_eq!(owner(&w).as_deref(), Some("dev-a"));
    assert_eq!(
        w.devices[1]
            .engine
            .document()
            .current
            .as_ref()
            .unwrap()
            .track_id,
        t[0]
    );
    // the other device presses next: the queue moves for both, transport stays
    w.perform(Action::Next { device: 1 });
    w.run_for(1_000.0);
    assert_eq!(
        w.devices[0]
            .engine
            .document()
            .current
            .as_ref()
            .unwrap()
            .track_id,
        t[1]
    );
    assert_eq!(
        w.devices[0].playback.track_id.as_deref(),
        Some(t[1].as_str())
    );
    assert_eq!(owner(&w).as_deref(), Some("dev-a"));
    w.finish();
    w.assert_ok();
}

#[test]
fn two_people_pressing_next_produce_one_skip() {
    let mut w = coordinator_world(2, 2);
    let t = tracks(&w, 6);
    w.perform(Action::PlayTracks {
        device: 0,
        tracks: t.clone(),
    });
    w.perform(Action::ClaimTransport {
        device: 0,
        takeover: false,
    });
    w.run_for(1_000.0);
    // both press next within the same instant
    w.perform(Action::Next { device: 0 });
    w.perform(Action::Next { device: 1 });
    w.run_for(1_000.0);
    assert_eq!(
        w.devices[0]
            .engine
            .document()
            .current
            .as_ref()
            .unwrap()
            .track_id,
        t[1]
    );
    assert_eq!(
        w.devices[1]
            .engine
            .document()
            .current
            .as_ref()
            .unwrap()
            .track_id,
        t[1]
    );
    w.finish();
    w.assert_ok();
}

#[test]
fn handoff_moves_transport_with_position_and_played_ms() {
    let mut w = coordinator_world(3, 3);
    // long tracks (t4 is ~172 s) so the position is still inside the first one
    let t = vec!["t4".to_string(), "t3".to_string(), "t2".to_string()];
    w.perform(Action::PlayTracks {
        device: 0,
        tracks: t.clone(),
    });
    w.perform(Action::ClaimTransport {
        device: 0,
        takeover: false,
    });
    w.run_for(30_000.0);
    w.perform(Action::OpenPicker { device: 0 });
    w.run_for(1_500.0);
    assert!(w.devices[0].picker_open);
    assert!(
        w.devices[0].picker_targets.iter().all(|d| d.ready),
        "targets pre-buffered: {:?}",
        w.devices[0].picker_targets
    );
    // the source keeps playing while the picker is open
    assert!(w.devices[0].playback.playing);
    w.perform(Action::HandoffTo {
        device: 0,
        target: 1,
    });
    w.run_for(1_000.0);
    assert_eq!(owner(&w).as_deref(), Some("dev-b"));
    assert!(!w.devices[0].playback.playing);
    assert!(w.devices[1].playback.playing);
    let pos = w.devices[1].position_ms();
    assert!(
        (30_000..36_000).contains(&pos),
        "target continues near the source position: {pos}\n{}\n{}",
        w.devices[0].log.join("\n"),
        w.devices[1].log.join("\n")
    );
    assert!(w.devices[1].playback.played_ms >= 30_000);
    assert!(!w.devices[0].picker_open);
    // the third device discarded its pre-buffer (takeover went elsewhere)
    w.finish();
    w.assert_ok();
}

#[test]
fn scrobble_survives_a_mid_track_handoff_exactly_once() {
    let mut w = coordinator_world(4, 2);
    // t0 is 20 s long: threshold 10 s
    let t = vec!["t0".to_string(), "t1".to_string(), "t2".to_string()];
    w.perform(Action::PlayTracks {
        device: 0,
        tracks: t,
    });
    w.perform(Action::ClaimTransport {
        device: 0,
        takeover: false,
    });
    w.run_for(6_000.0);
    w.perform(Action::OpenPicker { device: 0 });
    w.run_for(1_000.0);
    w.perform(Action::HandoffTo {
        device: 0,
        target: 1,
    });
    // target crosses the threshold with the source's 6 s counted
    w.run_for(6_000.0);
    assert_eq!(w.server.count("t0", w.devices[1].playback.started_at), 1);
    w.finish();
    w.assert_ok();
    assert_eq!(
        w.server
            .scrobbles
            .iter()
            .filter(|(t, _, _)| t == "t0")
            .count(),
        1
    );
}

#[test]
fn partitioned_owner_keeps_playing_then_is_fenced_and_files_its_state() {
    let mut w = coordinator_world(5, 2);
    let t = tracks(&w, 8);
    w.perform(Action::PlayTracks {
        device: 0,
        tracks: t.clone(),
    });
    w.perform(Action::ClaimTransport {
        device: 0,
        takeover: false,
    });
    w.run_for(2_000.0);
    // a loses the network but keeps playing and editing its queue
    w.perform(Action::Partition {
        device: 0,
        duration_ms: 120_000.0,
    });
    w.run_for(5_000.0);
    w.perform(Action::Next { device: 0 });
    w.run_for(30_000.0);
    assert!(
        w.devices[0].owns() && w.devices[0].engine.is_detached(),
        "a plays on, detached"
    );
    assert!(w.devices[0].playback.playing);
    // b sees the lease lapse and gets a dormant resume offer; it never auto-resumes
    assert!(!w.devices[1].owns());
    assert!(
        w.devices[1].resume_offer.is_some(),
        "b should be offered a resume"
    );
    w.perform(Action::ResumeHere { device: 1 });
    w.run_for(1_000.0);
    assert!(w.devices[1].owns());
    // b also changes the queue while a is away
    w.perform(Action::Next { device: 1 });
    w.run_for(1_000.0);
    let rev_before = w.coordinator.as_ref().unwrap().room.revision();
    // a comes back: fenced, stops, and its diverged queue becomes a saved queue
    w.perform(Action::Heal { device: 0 });
    w.run_for(40_000.0);
    assert!(w.devices[0].engine.is_connected());
    assert!(!w.devices[0].owns(), "a was fenced");
    assert!(!w.devices[0].playback.playing);
    assert!(w.devices[1].owns(), "b keeps transport");
    assert_eq!(
        w.devices[0].filed_count, 1,
        "a filed its state exactly once"
    );
    assert!(w.coordinator.as_ref().unwrap().room.revision() >= rev_before);
    // the filed queue synced to b through the LWW set
    assert!(
        w.devices[1]
            .saved_queues
            .iter()
            .any(|q| q.label.contains("from dev-a")),
        "saved queues: {:?}",
        w.devices[1]
            .saved_queues
            .iter()
            .map(|q| &q.label)
            .collect::<Vec<_>>()
    );
    w.finish();
    w.assert_ok();
}

#[test]
fn offline_edits_fast_forward_when_nobody_else_moved() {
    let mut w = coordinator_world(6, 2);
    let t = tracks(&w, 8);
    w.perform(Action::PlayTracks {
        device: 1,
        tracks: t.clone(),
    });
    w.run_for(1_000.0);
    w.perform(Action::Partition {
        device: 0,
        duration_ms: 40_000.0,
    });
    w.run_for(2_000.0);
    w.perform(Action::PlayNext {
        device: 0,
        tracks: vec!["t20".into()],
    });
    w.perform(Action::Next { device: 0 });
    // heal at +40 s; reconnect backoff may take a few more tens of seconds
    w.run_for(90_000.0);
    // a's two ops replayed; nobody filed anything
    assert_eq!(
        w.devices[0].filed_count,
        0,
        "{}",
        w.devices[0].log.join("\n")
    );
    assert_eq!(
        w.devices[1]
            .engine
            .document()
            .current
            .as_ref()
            .unwrap()
            .track_id,
        "t20",
        "{}\n{}",
        w.devices[0].log.join("\n"),
        w.devices[1].log.join("\n")
    );
    w.finish();
    w.assert_ok();
}

#[test]
fn crash_and_restart_rejoins_with_persisted_state() {
    let mut w = coordinator_world(7, 2);
    let t = tracks(&w, 5);
    w.perform(Action::PlayTracks {
        device: 0,
        tracks: t.clone(),
    });
    w.perform(Action::ClaimTransport {
        device: 0,
        takeover: false,
    });
    w.run_for(5_000.0);
    w.perform(Action::Crash {
        device: 1,
        duration_ms: 30_000.0,
    });
    w.perform(Action::Next { device: 0 });
    w.run_for(40_000.0);
    assert!(w.devices[1].engine.is_connected());
    assert_eq!(
        w.devices[1]
            .engine
            .document()
            .current
            .as_ref()
            .unwrap()
            .track_id,
        t[1]
    );
    assert_eq!(
        w.devices[1].filed_count, 0,
        "a restarted device that was merely behind files nothing"
    );
    w.finish();
    w.assert_ok();
}

#[test]
fn track_end_advances_only_on_the_owner_and_everyone_follows() {
    let mut w = coordinator_world(8, 3);
    let t = vec!["t0".to_string(), "t1".to_string()]; // t0 is 20 s
    w.perform(Action::PlayTracks {
        device: 2,
        tracks: t,
    });
    w.perform(Action::ClaimTransport {
        device: 2,
        takeover: false,
    });
    w.run_for(25_000.0);
    if std::env::var("HOCKET_SIM_TRACE").is_ok() {
        for l in &w.devices[2].log {
            eprintln!("{l}");
        }
    }
    for d in &w.devices {
        assert_eq!(
            d.engine.document().current.as_ref().unwrap().track_id,
            "t1",
            "{}",
            d.id
        );
    }
    assert_eq!(w.coordinator.as_ref().unwrap().room.revision(), 2);
    w.finish();
    w.assert_ok();
}

#[test]
fn lan_election_serves_a_room_and_peers_converge() {
    let mut cfg = WorldConfig::new(9);
    cfg.devices = 3;
    cfg.topology = Topology::Lan;
    cfg.keep_logs = true;
    let mut w = World::new(cfg);
    w.run_for(5_000.0);
    let serving: Vec<&str> = w
        .devices
        .iter()
        .filter(|d| d.engine.is_serving())
        .map(|d| d.id.as_str())
        .collect();
    assert_eq!(serving.len(), 1, "exactly one LAN coordinator: {serving:?}");
    let clients = w
        .devices
        .iter()
        .filter(|d| d.engine.is_connected() && !d.engine.is_serving())
        .count();
    assert_eq!(clients, 2);
    let t = tracks(&w, 4);
    w.perform(Action::PlayTracks {
        device: 1,
        tracks: t.clone(),
    });
    w.perform(Action::ClaimTransport {
        device: 1,
        takeover: false,
    });
    w.run_for(2_000.0);
    for d in &w.devices {
        assert_eq!(
            d.engine.document().current.as_ref().unwrap().track_id,
            t[0],
            "{}",
            d.id
        );
    }
    w.perform(Action::Next { device: 2 });
    w.run_for(2_000.0);
    for d in &w.devices {
        assert_eq!(
            d.engine.document().current.as_ref().unwrap().track_id,
            t[1],
            "{}",
            d.id
        );
    }
    w.finish();
    w.assert_ok();
}

#[test]
fn lossy_and_reordering_network_still_converges() {
    let mut cfg = WorldConfig::new(10);
    cfg.devices = 3;
    cfg.conditions = Conditions {
        delay_ms: 80.0,
        jitter_ms: 120.0,
        drop: 0.2,
    };
    cfg.keep_logs = true;
    let mut w = World::new(cfg);
    w.run_for(5_000.0);
    let t = tracks(&w, 10);
    w.perform(Action::PlayTracks {
        device: 0,
        tracks: t,
    });
    w.perform(Action::ClaimTransport {
        device: 0,
        takeover: false,
    });
    for i in 0..20 {
        w.run_for(1_500.0);
        w.perform(Action::Next { device: i % 3 });
    }
    w.finish();
    w.assert_ok();
}

/// A hostile host on the LAN: right scope, wrong key, adverts that win
/// every election. Nobody follows it, nobody admits it, it sees nothing
/// but challenges, and no `Hello` on the LAN ever carries a credential.
#[test]
fn hostile_lan_peer_is_never_admitted_and_lan_hellos_carry_no_credential() {
    let mut cfg = WorldConfig::new(11);
    cfg.devices = 3;
    cfg.topology = Topology::Lan;
    cfg.hostile = true;
    cfg.keep_logs = true;
    let mut w = World::new(cfg);
    w.run_for(8_000.0);
    let rogue = w.hostile_index().unwrap();
    let serving: Vec<&str> = w.devices[..3]
        .iter()
        .filter(|d| d.engine.is_serving())
        .map(|d| d.id.as_str())
        .collect();
    assert_eq!(serving.len(), 1, "one honest LAN coordinator: {serving:?}");
    assert_eq!(
        w.devices[..3]
            .iter()
            .filter(|d| d.engine.is_connected() && !d.engine.is_serving())
            .count(),
        2,
        "the other two honest devices follow it"
    );
    assert!(!w.devices[rogue].engine.is_connected());
    let t = tracks(&w, 4);
    w.perform(Action::PlayTracks {
        device: 1,
        tracks: t.clone(),
    });
    w.perform(Action::ClaimTransport {
        device: 1,
        takeover: false,
    });
    w.run_for(2_000.0);
    w.perform(Action::Next { device: 2 });
    w.run_for(2_000.0);
    for d in &w.devices[..3] {
        assert_eq!(
            d.engine.document().current.as_ref().unwrap().track_id,
            t[1],
            "{}",
            d.id
        );
    }
    assert!(
        w.devices[rogue].engine.document().current.is_none(),
        "the rogue never saw the session"
    );
    assert_eq!(w.devices[rogue].engine.room().member_count(), 0);
    assert!(w.lan_hellos > 0, "honest devices did join each other");
    w.finish();
    w.assert_ok();
}

/// Devices with a coordinator configured but unreachable form a LAN room,
/// keep playing, and when the coordinator returns the LAN session carries
/// over: nothing is filed as "session moved on", the queue is not reverted.
#[test]
fn lan_session_carries_over_when_the_coordinator_returns() {
    let mut cfg = WorldConfig::new(12);
    cfg.devices = 3;
    cfg.topology = Topology::LanThenCoordinator;
    cfg.coordinator_returns_ms = 40_000.0;
    cfg.keep_logs = true;
    let mut w = World::new(cfg);
    let start = w.now();
    w.run_for(10_000.0);
    let serving = w.devices.iter().filter(|d| d.engine.is_serving()).count();
    assert_eq!(serving, 1, "the LAN elected a room while the coordinator is down");
    let t = tracks(&w, 5);
    w.perform(Action::PlayTracks {
        device: 0,
        tracks: t.clone(),
    });
    w.perform(Action::ClaimTransport {
        device: 0,
        takeover: false,
    });
    w.run_for(2_000.0);
    w.perform(Action::Next { device: 1 });
    w.run_for(2_000.0);
    w.perform(Action::Next { device: 2 });
    w.run_for(2_000.0);
    for d in &w.devices {
        assert_eq!(
            d.engine.document().current.as_ref().unwrap().track_id,
            t[2],
            "{} on the LAN",
            d.id
        );
    }
    // the coordinator returns at 40 s; everyone moves back to it
    w.run_until(start + 41_000.0);
    w.run_for(20_000.0);
    for d in &w.devices {
        assert!(d.engine.is_connected(), "{} back on the coordinator", d.id);
        assert!(d.engine.lan_leader().is_none(), "{} left the LAN room", d.id);
        assert_eq!(
            d.engine.document().current.as_ref().unwrap().track_id,
            t[2],
            "{} kept the LAN session",
            d.id
        );
        assert_eq!(d.filed_count, 0, "{} filed nothing", d.id);
    }
    let room_doc = &w.coordinator.as_ref().unwrap().room.replica().document;
    assert_eq!(room_doc.current.as_ref().unwrap().track_id, t[2]);
    assert_eq!(owner(&w).as_deref(), Some("dev-a"));
    w.finish();
    w.assert_ok();
}

/// Seeds with an open bug, excluded from the batches so the harness stays a
/// gate for everything else. Reproduce one with `random_single_seed_from_env`.
/// Empty at the moment; keep it that way.
const KNOWN_FAILING: &[u64] = &[];

fn run_seeds(range: std::ops::Range<u64>, topology: Topology, actions: usize) {
    run_seeds_with(range, topology, actions, |_| {});
}

fn run_seeds_with(
    range: std::ops::Range<u64>,
    topology: Topology,
    actions: usize,
    tweak: impl Fn(&mut WorldConfig),
) {
    for seed in range {
        if KNOWN_FAILING.contains(&seed) {
            continue;
        }
        let mut cfg = WorldConfig::new(seed);
        cfg.devices = 2 + (seed % 3) as usize;
        cfg.topology = topology;
        cfg.keep_logs = true;
        tweak(&mut cfg);
        if seed.is_multiple_of(4) {
            cfg.conditions = Conditions {
                delay_ms: 60.0,
                jitter_ms: 90.0,
                drop: 0.05,
            };
        }
        let w = World::run_random(cfg, actions);
        w.assert_ok();
    }
}

#[test]
fn random_scenarios_coordinator_batch_1() {
    run_seeds(0..100, Topology::Coordinator, 40);
}

#[test]
fn random_scenarios_coordinator_batch_2() {
    run_seeds(100..200, Topology::Coordinator, 40);
}

#[test]
fn random_scenarios_coordinator_batch_3() {
    run_seeds(200..300, Topology::Coordinator, 60);
}

#[test]
fn random_scenarios_lan() {
    run_seeds(1000..1060, Topology::Lan, 30);
}

#[test]
fn random_scenarios_lan_with_a_hostile_peer() {
    run_seeds_with(2000..2030, Topology::Lan, 30, |cfg| cfg.hostile = true);
}

#[test]
fn random_scenarios_lan_then_coordinator() {
    run_seeds_with(3000..3040, Topology::LanThenCoordinator, 30, |cfg| {
        cfg.coordinator_returns_ms = 30_000.0 + (cfg.seed % 7) as f64 * 20_000.0;
    });
}

/// Debug aid: `HOCKET_SIM_SEED=<n> [HOCKET_SIM_LAN=1 | HOCKET_SIM_LAN_THEN_COORDINATOR=1] [HOCKET_SIM_HOSTILE=1] [HOCKET_SIM_ACTIONS=<n>] cargo test ... random_single_seed -- --nocapture`.
/// The batches use 40 actions for coordinator seeds and 30 for LAN ones (60 for batch 3).
#[test]
fn random_single_seed_from_env() {
    let Ok(seed) = std::env::var("HOCKET_SIM_SEED") else {
        return;
    };
    let seed: u64 = seed.parse().expect("HOCKET_SIM_SEED must be a number");
    let topology = if std::env::var("HOCKET_SIM_LAN").is_ok() {
        Topology::Lan
    } else if std::env::var("HOCKET_SIM_LAN_THEN_COORDINATOR").is_ok() {
        Topology::LanThenCoordinator
    } else {
        Topology::Coordinator
    };
    let mut cfg = WorldConfig::new(seed);
    cfg.devices = 2 + (seed % 3) as usize;
    cfg.topology = topology;
    cfg.keep_logs = true;
    cfg.hostile = std::env::var("HOCKET_SIM_HOSTILE").is_ok();
    if topology == Topology::LanThenCoordinator {
        cfg.coordinator_returns_ms = 30_000.0 + (seed % 7) as f64 * 20_000.0;
    }
    if seed.is_multiple_of(4) {
        cfg.conditions = Conditions {
            delay_ms: 60.0,
            jitter_ms: 90.0,
            drop: 0.05,
        };
    }
    let actions = std::env::var("HOCKET_SIM_ACTIONS")
        .ok()
        .and_then(|a| a.parse().ok())
        .unwrap_or(if topology == Topology::Lan { 30 } else { 40 });
    let w = World::run_random(cfg, actions);
    w.assert_ok();
}

mod props {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        #![proptest_config(ProptestConfig { cases: 24, max_shrink_iters: 0, ..ProptestConfig::default() })]
        #[test]
        fn any_seed_holds_the_invariants(seed in 5000u64..1_000_000u64, devices in 2usize..=4, actions in 10usize..50) {
            let mut cfg = WorldConfig::new(seed);
            cfg.devices = devices;
            let w = World::run_random(cfg, actions);
            prop_assert!(w.violations.is_empty(), "seed {seed}: {:?}", w.violations);
        }
    }
}
