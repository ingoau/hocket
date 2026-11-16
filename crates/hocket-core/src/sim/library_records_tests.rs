//! The identity's library across devices (`docs/providers.md`, Phase 3): a
//! manual link made on one device reaches the others, through the
//! coordinator and through a LAN room; a Mixed playlist syncs; and
//! concurrent edits of one record settle the same way everywhere.

use super::world::{Action, Topology, World, WorldConfig};
use crate::api::TrackRef;
use crate::library::{self, LibraryRecord, MixedPlaylist};

fn world(seed: u64, topology: Topology, devices: usize) -> World {
    let mut cfg = WorldConfig::new(seed);
    cfg.devices = devices;
    cfg.topology = topology;
    cfg.keep_logs = true;
    let mut w = World::new(cfg);
    w.run_for(5_000.0);
    w
}

fn put(w: &mut World, i: usize, record: LibraryRecord) {
    w.devices[i].put_library_record(record);
    w.run_for(0.0);
    w.run_for(1_000.0);
}

fn r(source: &str, id: &str) -> TrackRef {
    TrackRef::new(source, id)
}

fn links(w: &World, i: usize) -> Vec<(TrackRef, TrackRef, bool)> {
    library::manual_links(&w.devices[i].library_records)
        .into_iter()
        .map(|l| (l.a, l.b, l.same))
        .collect()
}

fn check_links(topology: Topology, seed: u64) {
    let mut w = world(seed, topology, 3);
    put(
        &mut w,
        0,
        LibraryRecord::link(r("navi", "1"), r("ytm", "v1"), true, 0.0),
    );
    for i in 0..3 {
        assert_eq!(
            links(&w, i),
            vec![(r("navi", "1"), r("ytm", "v1"), true)],
            "device {i}"
        );
    }
    // "Not the same song" on another device overrides it everywhere.
    put(
        &mut w,
        2,
        LibraryRecord::link(r("ytm", "v1"), r("navi", "1"), false, 0.0),
    );
    for i in 0..3 {
        assert_eq!(
            links(&w, i),
            vec![(r("ytm", "v1"), r("navi", "1"), false)],
            "device {i}"
        );
    }
    w.finish();
    w.assert_ok();
}

#[test]
fn a_manual_link_reaches_every_device_through_the_coordinator() {
    check_links(Topology::Coordinator, 301);
}

#[test]
fn a_manual_link_reaches_every_device_through_a_lan_room() {
    check_links(Topology::Lan, 302);
}

#[test]
fn a_mixed_playlist_syncs_and_its_edits_follow() {
    let mut w = world(303, Topology::Coordinator, 3);
    let mut p = MixedPlaylist {
        id: "mix-1".into(),
        name: "Road trip".into(),
        tracks: vec![r("navi", "1"), r("other", "7")],
        created: 1.0,
        ..Default::default()
    };
    put(&mut w, 0, LibraryRecord::playlist(p.clone(), 0.0));
    for i in 0..3 {
        assert_eq!(
            library::playlists(&w.devices[i].library_records),
            vec![p.clone()],
            "device {i}"
        );
    }
    p.tracks.push(r("ytm", "v2"));
    p.name = "Long road trip".into();
    put(&mut w, 1, LibraryRecord::playlist(p.clone(), 0.0));
    for i in 0..3 {
        assert_eq!(
            library::playlists(&w.devices[i].library_records),
            vec![p.clone()],
            "device {i}"
        );
    }
    // Deleted on one device, gone on all.
    let mut gone = LibraryRecord::playlist(p, 0.0);
    gone.removed = true;
    put(&mut w, 2, gone);
    for i in 0..3 {
        assert!(
            library::playlists(&w.devices[i].library_records).is_empty(),
            "device {i}"
        );
    }
    w.finish();
    w.assert_ok();
}

#[test]
fn the_records_survive_on_the_coordinator_for_a_late_device() {
    let mut w = world(304, Topology::Coordinator, 2);
    // Device 1 is away while device 0 links; it catches up from the replica.
    w.perform(Action::Crash {
        device: 1,
        duration_ms: 5_000.0,
    });
    w.run_for(500.0);
    put(
        &mut w,
        0,
        LibraryRecord::link(r("a", "1"), r("b", "2"), true, 0.0),
    );
    w.run_for(10_000.0);
    assert!(w.devices[1].engine.is_connected());
    assert_eq!(links(&w, 1), vec![(r("a", "1"), r("b", "2"), true)]);
    w.finish();
    w.assert_ok();
}
