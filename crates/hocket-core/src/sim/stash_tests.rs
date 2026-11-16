//! The song stash across devices: live edits fan out through the room, concurrent edits merge
//! (both adds survive, a removal sticks), edits made offline or before a crash land on reconnect,
//! and the coordinator's replica keeps the stash for a device that joins later.

use super::device::StashEdit;
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

fn edit(w: &mut World, i: usize, e: StashEdit) {
    w.devices[i].edit_stash(e);
    w.run_for(0.0);
    w.run_for(1_000.0);
}

fn all_list(w: &World, expected: &[&str]) {
    for d in &w.devices {
        assert_eq!(d.stash_tracks(), ids(expected), "{}", d.id);
    }
}

fn live_edits(mut w: World) {
    edit(&mut w, 0, StashEdit::Add(ids(&["t1", "t2"])));
    all_list(&w, &["t1", "t2"]);
    edit(&mut w, 1, StashEdit::Add(ids(&["t3"])));
    all_list(&w, &["t1", "t2", "t3"]);
    edit(&mut w, 2, StashEdit::Move(2, 0));
    all_list(&w, &["t3", "t1", "t2"]);
    edit(&mut w, 0, StashEdit::Remove(1));
    all_list(&w, &["t3", "t2"]);
    edit(&mut w, 1, StashEdit::Clear);
    all_list(&w, &[]);
    w.finish();
    w.assert_ok();
}

#[test]
fn stash_edits_reach_every_device_through_the_coordinator() {
    live_edits(world(41, Topology::Coordinator, 3));
}

#[test]
fn stash_edits_reach_every_device_through_a_lan_room() {
    live_edits(world(42, Topology::Lan, 3));
}

#[test]
fn concurrent_stash_edits_on_partitioned_devices_merge_on_heal() {
    let mut w = world(43, Topology::Coordinator, 3);
    edit(&mut w, 0, StashEdit::Add(ids(&["t1", "t2", "t3"])));
    all_list(&w, &["t1", "t2", "t3"]);
    w.perform(Action::Partition {
        device: 1,
        duration_ms: 20_000.0,
    });
    w.run_for(500.0);
    // Offline on device 1: add one, remove t1. Meanwhile device 0 adds another and moves t3 up.
    edit(&mut w, 1, StashEdit::Add(ids(&["t4"])));
    edit(&mut w, 1, StashEdit::Remove(0));
    edit(&mut w, 0, StashEdit::Add(ids(&["t5"])));
    edit(&mut w, 0, StashEdit::Move(2, 0));
    assert_eq!(w.devices[2].stash_tracks(), ids(&["t3", "t1", "t2", "t5"]));
    w.perform(Action::Heal { device: 1 });
    w.run_for(10_000.0);
    // Both adds survive, the removal sticks, the move stands.
    let expected = w.devices[0].stash_tracks();
    assert!(expected.contains(&"t4".to_string()) && expected.contains(&"t5".to_string()));
    assert!(!expected.contains(&"t1".to_string()));
    assert_eq!(expected[0], "t3");
    for d in &w.devices {
        assert_eq!(d.stash_tracks(), expected, "{}", d.id);
    }
    w.finish();
    w.assert_ok();
}

#[test]
fn a_clear_and_a_concurrent_add_keep_the_add() {
    let mut w = world(44, Topology::Coordinator, 2);
    edit(&mut w, 0, StashEdit::Add(ids(&["t1", "t2"])));
    w.perform(Action::Partition {
        device: 1,
        duration_ms: 20_000.0,
    });
    w.run_for(500.0);
    edit(&mut w, 0, StashEdit::Clear);
    edit(&mut w, 1, StashEdit::Add(ids(&["t3"])));
    w.perform(Action::Heal { device: 1 });
    w.run_for(10_000.0);
    all_list(&w, &["t3"]);
    w.finish();
    w.assert_ok();
}

#[test]
fn the_stash_survives_a_crash_and_is_kept_by_the_coordinator() {
    let mut w = world(45, Topology::Coordinator, 2);
    edit(&mut w, 0, StashEdit::Add(ids(&["t1", "t2"])));
    // Device 1 goes away; device 0 edits; device 1 comes back and catches up from the replica.
    w.perform(Action::Crash {
        device: 1,
        duration_ms: 5_000.0,
    });
    w.run_for(500.0);
    edit(&mut w, 0, StashEdit::Add(ids(&["t3"])));
    w.run_for(10_000.0);
    assert!(w.devices[1].engine.is_connected());
    all_list(&w, &["t1", "t2", "t3"]);
    // And the other way round: a crash with an edit that never left the device.
    w.perform(Action::Partition {
        device: 0,
        duration_ms: 60_000.0,
    });
    w.run_for(500.0);
    edit(&mut w, 0, StashEdit::Remove(0));
    w.perform(Action::Crash {
        device: 0,
        duration_ms: 2_000.0,
    });
    w.run_for(3_000.0);
    w.perform(Action::Heal { device: 0 });
    w.run_for(10_000.0);
    all_list(&w, &["t2", "t3"]);
    let replica = &w
        .coordinator
        .as_ref()
        .expect("coordinator")
        .room
        .replica()
        .stash;
    assert_eq!(crate::stash::track_ids(replica), ids(&["t2", "t3"]));
    w.finish();
    w.assert_ok();
}
