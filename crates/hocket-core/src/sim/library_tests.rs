//! Library edits (ratings, loves, playlists) relayed through the room: every
//! other device in the room applies them, nobody hears their own back, and a
//! device that is away simply misses them (its next library sync is what
//! catches it up).

use super::world::{Action, Topology, World, WorldConfig};
use crate::api::{LibraryItemKind, LibraryItemState, Playlist};
use crate::connect::wire::PlaylistEdit;

fn rated(id: &str, rating: u32, loved: bool) -> LibraryItemState {
    LibraryItemState {
        kind: LibraryItemKind::Track,
        id: id.into(),
        rating,
        loved,
    }
}

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

/// A user edit on device `i`, then a second of world time.
fn edit(w: &mut World, i: usize, items: Vec<LibraryItemState>, playlists: Vec<PlaylistEdit>) {
    w.devices[i].edit_library(items, playlists);
    // Send what the edit produced now (a user action through `perform` does the same).
    w.run_for(0.0);
    w.run_for(1_000.0);
}

fn check_relayed(mut w: World) {
    let n = w.devices.len();
    edit(&mut w, 1, vec![rated("t3", 4, true)], vec![]);
    for d in &w.devices {
        assert_eq!(
            d.library_items.get("t3"),
            Some(&rated("t3", 4, true)),
            "{}",
            d.id
        );
    }
    let from = w.devices[1].id.clone();
    assert!(
        w.devices[1].library_edits_from.is_empty(),
        "the sender never hears its own edit back"
    );
    for (i, d) in w.devices.iter().enumerate().filter(|(i, _)| *i != 1) {
        assert_eq!(d.library_edits_from, vec![from.clone()], "device {i}");
    }

    // A playlist edit from another device, with a later rating that wins
    // everywhere (absolute values: the last one relayed stands).
    let pl = Playlist {
        id: "p1".into(),
        name: "Mix".into(),
        ..Default::default()
    };
    edit(
        &mut w,
        n - 1,
        vec![rated("t3", 2, true)],
        vec![PlaylistEdit {
            playlist_id: "p1".into(),
            playlist: Some(pl),
            track_ids: Some(vec!["t1".into(), "t3".into()]),
        }],
    );
    for d in &w.devices {
        assert_eq!(d.library_items["t3"].rating, 2, "{}", d.id);
        assert_eq!(
            d.playlists.get("p1").map(|v| v.as_slice()),
            Some(&["t1".to_string(), "t3".to_string()][..]),
            "{}",
            d.id
        );
    }
    // Deleted on one device: gone everywhere.
    edit(
        &mut w,
        0,
        vec![],
        vec![PlaylistEdit {
            playlist_id: "p1".into(),
            playlist: None,
            track_ids: None,
        }],
    );
    for d in &w.devices {
        assert!(!d.playlists.contains_key("p1"), "{}", d.id);
    }
    w.finish();
    w.assert_ok();
}

#[test]
fn library_edits_reach_every_other_device_through_the_coordinator() {
    check_relayed(world(31, Topology::Coordinator, 3));
}

#[test]
fn library_edits_reach_every_other_device_through_a_lan_room() {
    check_relayed(world(32, Topology::Lan, 3));
}

#[test]
fn a_device_away_from_the_room_misses_library_edits_and_nothing_else_breaks() {
    let mut w = world(33, Topology::Coordinator, 3);
    w.perform(Action::Crash {
        device: 2,
        duration_ms: 5_000.0,
    });
    w.run_for(500.0);
    edit(&mut w, 0, vec![rated("t5", 5, false)], vec![]);
    assert_eq!(w.devices[1].library_items["t5"].rating, 5);
    w.run_for(10_000.0);
    assert!(
        w.devices[2].engine.is_connected(),
        "the restarted device is back"
    );
    assert!(
        !w.devices[2].library_items.contains_key("t5"),
        "not replayed on rejoin: the library sync catches it up"
    );
    // Back in the room: the next edit reaches it.
    edit(&mut w, 0, vec![rated("t5", 1, false)], vec![]);
    assert_eq!(w.devices[2].library_items["t5"].rating, 1);
    w.finish();
    w.assert_ok();
}
