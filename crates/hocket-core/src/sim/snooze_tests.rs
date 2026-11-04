//! Snoozes across devices: an edit reaches every peer through the coordinator and through a LAN
//! room, concurrent edits of different songs merge, and a removal sticks.

use super::world::{Action, Topology, World, WorldConfig};

fn world(seed: u64, topology: Topology, devices: usize) -> World {
    let mut cfg = WorldConfig::new(seed);
    cfg.devices = devices;
    cfg.topology = topology;
    cfg.keep_logs = true;
    let mut w = World::new(cfg);
    w.run_for(5_000.0);
    for d in &w.devices {
        assert!(
            d.engine.is_connected() || d.engine.is_serving(),
            "{} should be in a room",
            d.id
        );
    }
    w
}

fn ids(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

fn snooze(w: &mut World, i: usize, track: &str) {
    let until = w.devices[i].now() + 86_400_000.0;
    w.devices[i].edit_snooze(track, Some(until), false);
    w.run_for(0.0);
    w.run_for(1_000.0);
}

fn unsnooze(w: &mut World, i: usize, track: &str) {
    w.devices[i].edit_snooze(track, None, true);
    w.run_for(0.0);
    w.run_for(1_000.0);
}

fn all_have(w: &World, expected: &[&str]) {
    let mut want = ids(expected);
    want.sort();
    for d in &w.devices {
        assert_eq!(d.snoozed_tracks(), want, "{}", d.id);
    }
}

#[test]
fn snoozes_reach_every_device_through_the_coordinator() {
    let mut w = world(157, Topology::Coordinator, 3);
    snooze(&mut w, 0, "t1");
    all_have(&w, &["t1"]);
    snooze(&mut w, 1, "t2");
    all_have(&w, &["t1", "t2"]);
    unsnooze(&mut w, 2, "t1");
    all_have(&w, &["t2"]);
    w.finish();
    w.assert_ok();
}

#[test]
fn snoozes_reach_every_device_through_a_lan_room() {
    let mut w = world(158, Topology::Lan, 3);
    snooze(&mut w, 0, "t1");
    snooze(&mut w, 1, "t2");
    all_have(&w, &["t1", "t2"]);
    w.finish();
    w.assert_ok();
}

#[test]
fn concurrent_snoozes_on_partitioned_devices_merge_on_heal() {
    let mut w = world(159, Topology::Coordinator, 2);
    snooze(&mut w, 0, "t1");
    all_have(&w, &["t1"]);
    w.perform(Action::Partition {
        device: 1,
        duration_ms: 20_000.0,
    });
    w.run_for(500.0);
    snooze(&mut w, 1, "t2");
    unsnooze(&mut w, 1, "t1");
    snooze(&mut w, 0, "t3");
    w.run_for(25_000.0);
    all_have(&w, &["t2", "t3"]);
    w.finish();
    w.assert_ok();
}
