//! Identities in the simulation (`docs/providers.md`, Phase 2 "Done when"):
//! pairing over the LAN and through the coordinator, removing devices
//! (including the first one, and one that is offline), the removed device
//! being turned away by a LAN peer and by the coordinator, two devices
//! removing each other at once, and the group key rotating with credential
//! sync on and off.

use super::world::{Action, PairVia, Topology, World, WorldConfig, COORDINATOR_NODE};
use crate::identity::{SecretKind, SyncedSecret, SyncedSecrets};

fn world(seed: u64, devices: usize, newcomers: usize, topology: Topology) -> World {
    let mut cfg = WorldConfig::new(seed);
    cfg.devices = devices;
    cfg.newcomers = newcomers;
    cfg.topology = topology;
    cfg.keep_logs = true;
    let mut w = World::new(cfg);
    w.run_for(5_000.0);
    w
}

fn key(w: &World, i: usize) -> Option<[u8; 32]> {
    w.identities[i].lock().group_key()
}

fn members(w: &World, i: usize) -> Vec<String> {
    w.identities[i]
        .lock()
        .state()
        .members
        .keys()
        .cloned()
        .collect()
}

/// Everyone in `devs` shares one document, attached somewhere.
fn assert_in_one_session(w: &World, devs: &[usize]) {
    let doc = w.devices[devs[0]].engine.document().clone();
    for &i in devs {
        assert!(
            w.devices[i].engine.is_connected() || w.devices[i].engine.is_serving(),
            "{} is attached nowhere",
            w.devices[i].id
        );
        assert_eq!(
            w.devices[i].engine.document().session_id,
            doc.session_id,
            "{} is in another session",
            w.devices[i].id
        );
    }
}

fn play_somewhere(w: &mut World, device: usize) {
    let tracks: Vec<String> = w.library.ids().into_iter().take(3).collect();
    w.perform(Action::PlayTracks { device, tracks });
    w.run_for(3_000.0);
}

#[test]
fn a_new_device_pairs_over_the_lan() {
    let mut w = world(301, 1, 1, Topology::Lan);
    assert!(!w.is_member(1));
    let before = w.identities[1].lock().id.clone();
    w.pair(1, 0, PairVia::Lan);
    w.run_for(10_000.0);
    assert_eq!(w.pair_results.get(&1), Some(&Ok(())));
    let after = w.identities[1].lock().id.clone();
    assert_ne!(before, after);
    assert_eq!(after, w.identities[0].lock().id);
    assert_eq!(key(&w, 1), key(&w, 0));
    play_somewhere(&mut w, 0);
    w.run_for(10_000.0);
    assert_in_one_session(&w, &[0, 1]);
    assert_eq!(
        w.devices[1].engine.document().current,
        w.devices[0].engine.document().current
    );
    w.finish();
    w.assert_ok();
}

#[test]
fn a_new_device_pairs_through_the_coordinator() {
    let mut w = world(302, 2, 1, Topology::Coordinator);
    w.pair(2, 1, PairVia::Coordinator);
    w.run_for(10_000.0);
    assert_eq!(w.pair_results.get(&2), Some(&Ok(())));
    assert_eq!(members(&w, 2), members(&w, 0));
    assert_eq!(key(&w, 2), key(&w, 0));
    // The coordinator heard of it before the new device knocked.
    let coordinator_knows = w
        .coordinator
        .as_ref()
        .unwrap()
        .room
        .identity()
        .unwrap()
        .clone()
        .state()
        .is_member("dev-c");
    assert!(coordinator_knows);
    play_somewhere(&mut w, 2);
    w.run_for(5_000.0);
    assert_in_one_session(&w, &[0, 1, 2]);
    w.finish();
    w.assert_ok();
}

#[test]
fn pairing_fails_clearly_when_nothing_is_reachable() {
    let mut w = world(303, 1, 1, Topology::Lan);
    // The newcomer can't reach the inviter's network at all.
    let a = w.devices[0].id.clone();
    w.net.partition("dev-b-pair", &a);
    w.pair(1, 0, PairVia::Lan);
    w.run_for(5_000.0);
    assert!(matches!(w.pair_results.get(&1), Some(Err(_))));
    assert!(!w.is_member(1));
}

#[test]
fn an_unknown_invite_is_refused_by_the_coordinator() {
    let mut w = world(304, 1, 1, Topology::Coordinator);
    // An invite the inviter never offered upstream (made, then forgotten).
    w.pair(1, 0, PairVia::Coordinator);
    w.identities[0].lock().invites.clear();
    w.run_for(5_000.0);
    assert!(matches!(w.pair_results.get(&1), Some(Err(_))));
}

/// Three devices; the second removes the first. The other two carry on
/// together, and the first is turned away by `topology`'s room.
fn remove_the_first(seed: u64, topology: Topology) {
    let mut w = world(seed, 3, 0, topology);
    play_somewhere(&mut w, 1);
    let old_key = key(&w, 0);
    w.remove(1, 0).unwrap();
    w.run_for(30_000.0);
    // b and c: one identity, a new key, one session.
    assert_eq!(members(&w, 1), vec!["dev-b", "dev-c"]);
    assert_eq!(members(&w, 2), vec!["dev-b", "dev-c"]);
    assert_ne!(key(&w, 1), old_key);
    assert_eq!(key(&w, 1), key(&w, 2));
    assert_in_one_session(&w, &[1, 2]);
    w.perform(Action::Next { device: 2 });
    w.run_for(3_000.0);
    assert_eq!(
        w.devices[1].engine.document().current,
        w.devices[2].engine.document().current
    );
    // a: learned it was removed, holds no new key, is in nobody's room.
    assert!(w.identities[0].lock().was_removed());
    assert!(key(&w, 0) != key(&w, 1));
    assert!(!w.devices[0].engine.is_connected());
    for i in [1, 2] {
        assert!(!w.devices[i].engine.room().has_member("dev-a"));
    }
    if let Some(c) = &w.coordinator {
        assert!(!c.room.has_member("dev-a"));
    }
    w.finish();
    w.assert_ok();
}

#[test]
fn removing_the_first_device_leaves_the_others_working_with_a_coordinator() {
    remove_the_first(311, Topology::Coordinator);
}

#[test]
fn removing_the_first_device_leaves_the_others_working_on_the_lan() {
    remove_the_first(312, Topology::Lan);
}

#[test]
fn a_removed_device_is_rejected_by_a_lan_peer_and_by_the_coordinator() {
    // LAN first, then the coordinator comes back: both refuse it.
    let mut cfg = WorldConfig::new(313);
    cfg.devices = 3;
    cfg.topology = Topology::LanThenCoordinator;
    cfg.coordinator_returns_ms = 40_000.0;
    cfg.keep_logs = true;
    let mut w = World::new(cfg);
    w.run_for(5_000.0);
    w.remove(2, 0).unwrap();
    w.run_for(20_000.0);
    // On the LAN: nobody follows or admits dev-a.
    assert!(!w.devices[0].engine.is_connected());
    for i in [1, 2] {
        assert!(!w.devices[i].engine.room().has_member("dev-a"));
        assert_ne!(
            w.devices[i].engine.lan_leader().map(String::as_str),
            Some("dev-a")
        );
    }
    w.run_for(60_000.0);
    // And the coordinator, once it's back, doesn't either.
    let c = w.coordinator.as_ref().unwrap();
    assert!(!c.room.has_member("dev-a"));
    assert!(
        c.room.has_member("dev-b") || c.room.has_member("dev-c"),
        "{}\n{:?}\n{:?}",
        c.room.diagnostics_text(),
        w.devices[1].engine.connection_state(),
        w.devices[1].log.iter().rev().take(30).collect::<Vec<_>>()
    );
    assert!(!w.devices[0].engine.is_connected());
    w.finish();
    w.assert_ok();
}

#[test]
fn removal_works_while_the_removed_device_is_offline() {
    let mut w = world(314, 3, 0, Topology::Coordinator);
    w.perform(Action::Crash {
        device: 0,
        duration_ms: 60_000.0,
    });
    w.run_for(1_000.0);
    w.remove(1, 0).unwrap();
    w.run_for(10_000.0);
    assert_eq!(members(&w, 2), vec!["dev-b", "dev-c"]);
    // a comes back with its old list and an old key: the coordinator's
    // proof hands it the removal, and it is never admitted.
    w.run_for(90_000.0);
    assert!(w.identities[0].lock().was_removed());
    assert!(!w.coordinator.as_ref().unwrap().room.has_member("dev-a"));
    assert!(!w.devices[0].engine.is_connected());
    w.finish();
    w.assert_ok();
}

#[test]
fn two_devices_removing_each_other_settle_the_same_way_everywhere() {
    let mut w = world(315, 3, 0, Topology::Coordinator);
    let (a, b) = (w.devices[0].id.clone(), w.devices[1].id.clone());
    w.net.partition(&a, COORDINATOR_NODE);
    w.net.partition(&b, COORDINATOR_NODE);
    w.run_for(1_000.0);
    w.remove(0, 1).unwrap();
    w.remove(1, 0).unwrap();
    w.net.heal_all();
    w.run_for(60_000.0);
    // Everyone that is still in agrees with everyone else.
    let reference = members(&w, 2);
    assert!(reference.contains(&"dev-c".to_string()));
    let survivors: Vec<usize> = [0, 1]
        .into_iter()
        .filter(|&i| reference.contains(&w.devices[i].id))
        .collect();
    assert_eq!(
        survivors.len(),
        1,
        "exactly one of the two stays: {reference:?}"
    );
    let loser = 1 - survivors[0];
    w.set_member(survivors[0], true);
    w.set_member(loser, false);
    assert_eq!(members(&w, survivors[0]), reference);
    assert_eq!(members(&w, loser), reference);
    assert!(w.identities[loser].lock().was_removed());
    assert_eq!(key(&w, survivors[0]), key(&w, 2));
    w.finish();
    w.assert_ok();
}

fn secrets() -> SyncedSecrets {
    SyncedSecrets {
        entries: vec![SyncedSecret {
            account_id: "acc".into(),
            kind: SecretKind::Navidrome,
            url: "https://music.example".into(),
            username: "user".into(),
            secret: "hunter2".into(),
            name: None,
        }],
    }
}

fn rotate_on_removal(seed: u64, credential_sync: bool) {
    let mut w = world(seed, 3, 0, Topology::Coordinator);
    if credential_sync {
        let now = w.now();
        w.identities[0]
            .lock()
            .set_credential_sync(true, Some(&secrets()), now)
            .unwrap();
        w.devices[0].handle(crate::connect::engine::Input::IdentityChanged);
        w.run_for(5_000.0);
        for i in 0..3 {
            assert_eq!(w.identities[i].lock().secrets(), Some(secrets()));
        }
        // The coordinator holds the key while it's on.
        let coordinator_holds = w
            .coordinator
            .as_ref()
            .unwrap()
            .room
            .identity()
            .unwrap()
            .grants
            .iter()
            .any(|g| g.to == crate::identity::COORDINATOR_ID);
        assert!(coordinator_holds);
    }
    let old = key(&w, 0);
    w.remove(0, 2).unwrap();
    w.run_for(10_000.0);
    assert_ne!(key(&w, 0), old);
    assert_eq!(key(&w, 0), key(&w, 1));
    assert!(key(&w, 2) != key(&w, 0));
    if credential_sync {
        // b reads the blob, re-sealed under the new key; c can't.
        assert_eq!(w.identities[1].lock().secrets(), Some(secrets()));
        assert!(w.identities[2].lock().secrets().is_none());
        // Turning it off rotates again and drops the coordinator's copy.
        let now = w.now();
        let before = key(&w, 1);
        w.identities[1]
            .lock()
            .set_credential_sync(false, None, now)
            .unwrap();
        w.devices[1].handle(crate::connect::engine::Input::IdentityChanged);
        w.run_for(5_000.0);
        assert_ne!(key(&w, 1), before);
        assert_eq!(key(&w, 0), key(&w, 1));
        assert!(w.identities[0].lock().secrets().is_none());
        let room = w
            .coordinator
            .as_ref()
            .unwrap()
            .room
            .identity()
            .unwrap()
            .clone();
        assert!(room.blob.is_none());
        assert!(!room
            .grants
            .iter()
            .any(|g| g.to == crate::identity::COORDINATOR_ID));
    } else {
        assert!(w.identities[1].lock().secrets().is_none());
    }
    w.finish();
    w.assert_ok();
}

#[test]
fn key_rotation_with_credential_sync_on() {
    rotate_on_removal(321, true);
}

#[test]
fn key_rotation_with_credential_sync_off() {
    rotate_on_removal(322, false);
}
