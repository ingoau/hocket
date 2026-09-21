//! The world: devices, an optional hosted coordinator, the network, a fake
//! Navidrome that records scrobbles, a scheduler over virtual time, and the
//! invariants checked after every event.
//!
//! # Invariants
//!
//! 1. **Never two transport owners.** Among devices attached to the same
//!    room and not detached, at most one owns transport at any instant.
//! 2. **Revisions monotonic.** Every room's revision never decreases; a
//!    device's confirmed revision never decreases while it stays attached.
//! 3. **No scrobble lost or duplicated.** At quiescence, every
//!    `(trackId, startedAt)` that reached the threshold anywhere reached the
//!    server exactly once.
//! 4. **Queue reducer total.** Nothing here ever panics on any op.
//! 5. **Converged documents after heal.** At quiescence every attached
//!    device's document equals the room's.
//! 6. **Fenced stale writer never overwrites.** Every stamp the coordinator
//!    stores came from the live lease owner at that instant.
//! 7. **Returning diverged state becomes a saved queue, not a merge.** After
//!    a device files its state, it carries no unsynced ops and its document
//!    is the room's; the filed queue reaches the other devices.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use rand::seq::IndexedRandom;
use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;

use crate::api::{DeviceId, EpochMs, TrackId};
use crate::connect::engine::Input;
use crate::connect::replica::{MemoryReplicaStore, ReplicaStore};
use crate::connect::room::{Room, RoomConfig, RoomInput, RoomOutput};
use crate::connect::session_adapter::RealReducer;
use crate::connect::wire::{scope_key, TransportCommand};
use crate::connect::{same_session_state, SessionOp};
use crate::sim::clock::{quantize, to_micros, DeviceClock, SimTime};
use crate::sim::device::{DeviceEffect, Library, Persisted, SimDevice};
use crate::sim::network::{Conditions, NetEvent, Network};

pub const COORDINATOR_NODE: &str = "coordinator";
/// How long a device may keep believing it owns transport after the room
/// handed the lease elsewhere. The news needs one delivery plus a tick and a
/// lossy link may drop it repeatedly, but without any answer the owner
/// declares itself detached after `LEASE_DURATION - OWNER_LAPSE_MARGIN`
/// (18 s) plus a tick, so 20 s is the true bound.
pub const STALE_OWNER_GRACE_MS: f64 = 20_000.0;
pub const COORDINATOR_URL: &str = "wss://coordinator.example/";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Topology {
    /// Every device talks to the hosted coordinator.
    Coordinator,
    /// No coordinator: LAN election among the devices.
    Lan,
}

#[derive(Debug, Clone)]
pub struct WorldConfig {
    pub seed: u64,
    pub devices: usize,
    pub topology: Topology,
    pub conditions: Conditions,
    /// Max clock skew per device, ±ms.
    pub max_skew_ms: f64,
    pub library_size: usize,
    pub keep_logs: bool,
}

impl WorldConfig {
    pub fn new(seed: u64) -> Self {
        WorldConfig {
            seed,
            devices: 2,
            topology: Topology::Coordinator,
            conditions: Conditions::default(),
            max_skew_ms: 2_000.0,
            library_size: 24,
            keep_logs: false,
        }
    }
}

/// The hosted coordinator as a network node.
pub struct SimCoordinator {
    pub room: Room,
    pub store: MemoryReplicaStore,
    next_tick_at: EpochMs,
    /// (device, key) of the last stored stamp: a position-only stamp is not
    /// persisted and must not look like a new writer later.
    last_stamp_key: Option<(DeviceId, Option<String>)>,
    pub max_revision: u32,
}

/// The fake Navidrome: records what was scrobbled.
#[derive(Debug, Default, Clone)]
pub struct FakeServer {
    pub scrobbles: Vec<(TrackId, EpochMs, DeviceId)>,
}

impl FakeServer {
    pub fn count(&self, track_id: &str, started_at: EpochMs) -> usize {
        self.scrobbles
            .iter()
            .filter(|(t, s, _)| t == track_id && (s - started_at).abs() < 1000.0)
            .count()
    }
}

/// A scheduled user or fault action.
#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    PlayTracks {
        device: usize,
        tracks: Vec<TrackId>,
    },
    Next {
        device: usize,
    },
    Previous {
        device: usize,
    },
    PlayNext {
        device: usize,
        tracks: Vec<TrackId>,
    },
    Shuffle {
        device: usize,
        enabled: bool,
    },
    TogglePlay {
        device: usize,
    },
    Seek {
        device: usize,
        position_ms: u32,
    },
    ClaimTransport {
        device: usize,
        takeover: bool,
    },
    OpenPicker {
        device: usize,
    },
    HandoffTo {
        device: usize,
        target: usize,
    },
    ClosePicker {
        device: usize,
    },
    ResumeHere {
        device: usize,
    },
    Partition {
        device: usize,
        duration_ms: f64,
    },
    Heal {
        device: usize,
    },
    Cut {
        device: usize,
    },
    Crash {
        device: usize,
        duration_ms: f64,
    },
    Restart {
        device: usize,
    },
    Sleep {
        device: usize,
        duration_ms: f64,
    },
    Wake {
        device: usize,
    },
    Lossy {
        device: usize,
        drop: f64,
        duration_ms: f64,
    },
    Clean {
        device: usize,
    },
}

pub struct World {
    pub cfg: WorldConfig,
    pub time: Arc<SimTime>,
    pub rng: ChaCha8Rng,
    pub net: Network,
    pub devices: Vec<SimDevice>,
    pub coordinator: Option<SimCoordinator>,
    pub server: FakeServer,
    pub library: Arc<Library>,
    pub scope: String,
    schedule: BTreeMap<(i64, u64), Action>,
    schedule_seq: u64,
    pub actions_run: Vec<(EpochMs, Action)>,
    crashed: BTreeMap<usize, Persisted>,
    device_rev_watermark: Vec<Option<u32>>,
    /// Since when a device has held an epoch the room already moved past.
    stale_owner_since: Vec<Option<EpochMs>>,
    pub violations: Vec<String>,
    pub events: u64,
    lan_adverts: BTreeMap<usize, crate::connect::discovery::PeerAdvert>,
}

impl World {
    pub fn new(cfg: WorldConfig) -> World {
        let time = SimTime::new(1_000_000.0);
        let mut rng = ChaCha8Rng::seed_from_u64(cfg.seed);
        let scope = scope_key("https://music.example", "user");
        let library = Arc::new(Library::synthetic(cfg.library_size));
        let mut net = Network::new(cfg.conditions);
        let coordinator = match cfg.topology {
            Topology::Coordinator => {
                net.bind(COORDINATOR_URL, COORDINATOR_NODE);
                let mut rc = RoomConfig::new(scope.clone()).verified();
                rc.session_id = Some("coordinator-session".into());
                Some(SimCoordinator {
                    room: Room::new(rc, time.clone(), RealReducer::shared(), None),
                    store: MemoryReplicaStore::new(),
                    next_tick_at: time.now_ms(),
                    last_stamp_key: None,
                    max_revision: 0,
                })
            }
            Topology::Lan => None,
        };
        let mut w = World {
            time: time.clone(),
            rng: ChaCha8Rng::seed_from_u64(cfg.seed ^ 0xfeed),
            net,
            devices: vec![],
            coordinator,
            server: FakeServer::default(),
            library: library.clone(),
            scope: scope.clone(),
            schedule: BTreeMap::new(),
            schedule_seq: 0,
            actions_run: vec![],
            crashed: BTreeMap::new(),
            device_rev_watermark: vec![None; cfg.devices],
            stale_owner_since: vec![None; cfg.devices],
            violations: vec![],
            events: 0,
            lan_adverts: BTreeMap::new(),
            cfg: cfg.clone(),
        };
        for i in 0..cfg.devices {
            let skew = rng.random_range(-cfg.max_skew_ms..=cfg.max_skew_ms);
            let clock = Arc::new(DeviceClock::new(time.clone(), skew));
            let url =
                matches!(cfg.topology, Topology::Coordinator).then(|| COORDINATOR_URL.to_string());
            let mut d = SimDevice::new(
                &device_name(i),
                &scope,
                clock,
                library.clone(),
                url,
                cfg.topology == Topology::Lan,
                None,
            );
            d.keep_log = cfg.keep_logs;
            w.devices.push(d);
        }
        w.pump_effects();
        w
    }

    pub fn now(&self) -> EpochMs {
        self.time.now_ms()
    }

    pub fn device_index(&self, id: &str) -> Option<usize> {
        self.devices.iter().position(|d| d.id == id)
    }

    // -- scheduling ---------------------------------------------------------

    pub fn schedule(&mut self, at: EpochMs, action: Action) {
        self.schedule_seq += 1;
        self.schedule
            .insert((to_micros(at), self.schedule_seq), action);
    }

    pub fn schedule_in(&mut self, delay_ms: f64, action: Action) {
        let at = self.now() + delay_ms;
        self.schedule(at, action);
    }

    fn next_event_at(&self) -> EpochMs {
        let mut t = f64::MAX;
        if let Some(n) = self.net.next_at() {
            t = t.min(n);
        }
        for d in &self.devices {
            if !d.asleep {
                t = t.min(d.next_tick_at());
            }
        }
        if let Some(c) = &self.coordinator {
            t = t.min(c.next_tick_at);
        }
        if let Some(((k, _), _)) = self.schedule.iter().next() {
            t = t.min(*k as f64 / 1000.0);
        }
        t
    }

    /// Run until `until` (virtual ms), processing everything in order and
    /// checking invariants after every step.
    pub fn run_until(&mut self, until: EpochMs) {
        let trace = std::env::var("HOCKET_SIM_TRACE").is_ok();
        loop {
            let next = self.next_event_at();
            if trace {
                let devs: Vec<String> = self
                    .devices
                    .iter()
                    .map(|d| {
                        format!(
                            "{}:{}{}",
                            d.id,
                            d.next_tick_at(),
                            if d.asleep { "(asleep)" } else { "" }
                        )
                    })
                    .collect();
                eprintln!(
                    "TRACE now={:.3} next={:.3} inflight={} net_next={:?} sched={:?} coord={:?} devs={:?} events={}",
                    self.now(),
                    next,
                    self.net.in_flight(),
                    self.net.next_at(),
                    self.schedule.keys().next(),
                    self.coordinator.as_ref().map(|c| c.next_tick_at),
                    devs,
                    self.events,
                );
            }
            if next > until || next == f64::MAX {
                self.time.set(until);
                self.step();
                break;
            }
            self.time.set(next.max(self.now()));
            self.step();
        }
    }

    pub fn run_for(&mut self, ms: f64) {
        let until = self.now() + ms;
        self.run_until(until);
    }

    fn step(&mut self) {
        let now = self.now();
        // scheduled actions
        let due: Vec<(i64, u64)> = self
            .schedule
            .range(..=(to_micros(now), u64::MAX))
            .map(|(k, _)| *k)
            .collect();
        for k in due {
            if let Some(a) = self.schedule.remove(&k) {
                self.perform(a);
            }
        }
        // network deliveries
        for (node, ev) in self.net.due(now) {
            self.deliver(&node, ev);
        }
        // device steps
        for i in 0..self.devices.len() {
            self.devices[i].step();
        }
        self.pump_effects();
        // coordinator tick
        if let Some(c) = &mut self.coordinator {
            if now >= c.next_tick_at {
                c.next_tick_at = quantize(now + 1000.0);
                let outs = c.room.handle(RoomInput::Tick);
                self.coordinator_outputs(outs);
            }
        }
        self.events += 1;
        self.check_invariants();
        if self.net.in_flight() > 20_000 {
            self.violations.push(format!(
                "[{now:.0}] message storm: {}",
                self.net.pending_summary()
            ));
            self.assert_ok();
        }
    }

    fn deliver(&mut self, node: &str, ev: NetEvent) {
        if node == COORDINATOR_NODE {
            let input = match ev {
                NetEvent::Accepted { peer } => RoomInput::Connected(peer),
                NetEvent::Message { peer, msg } => RoomInput::Message(peer, msg),
                NetEvent::Closed { peer } => RoomInput::Disconnected(peer),
                NetEvent::Connected { .. } | NetEvent::ConnectFailed { .. } => return,
            };
            let outs = match &mut self.coordinator {
                Some(c) => c.room.handle(input),
                None => return,
            };
            self.coordinator_outputs(outs);
            return;
        }
        let Some(i) = self.device_index(node) else {
            return;
        };
        if self.devices[i].asleep {
            return;
        }
        let input = match ev {
            NetEvent::Connected { peer, url } => Input::Connected { peer, url },
            NetEvent::ConnectFailed { error } => Input::ConnectFailed { error },
            NetEvent::Accepted { peer } => Input::PeerConnected { peer },
            NetEvent::Message { peer, msg } => Input::WireIn { peer, msg },
            NetEvent::Closed { peer } => Input::Disconnected { peer },
        };
        self.devices[i].handle(input);
    }

    fn coordinator_outputs(&mut self, outs: Vec<RoomOutput>) {
        let now = self.now();
        for o in outs {
            match o {
                RoomOutput::Send(peer, msg) => {
                    self.net
                        .send(now, &mut self.rng, COORDINATOR_NODE, &peer, msg)
                }
                RoomOutput::Close(peer) => self.net.close(now, COORDINATOR_NODE, &peer),
                RoomOutput::Verify { peer, credential } => {
                    // The fake Navidrome accepts the one credential the devices carry.
                    let ok =
                        credential.username == "user" && credential.token.as_deref() == Some("tok");
                    let outs = match &mut self.coordinator {
                        Some(c) => c.room.handle(RoomInput::Verified { peer, ok }),
                        None => vec![],
                    };
                    self.coordinator_outputs(outs);
                }
                RoomOutput::ReplicaChanged => {
                    if let Some(c) = &mut self.coordinator {
                        let _ = c.store.save(c.room.scope(), c.room.replica());
                        // Invariant 6: the stored stamp came from the live owner.
                        let stamp = c
                            .room
                            .replica()
                            .last_stamp
                            .as_ref()
                            .map(|s| (s.device_id.clone(), s.key.clone()));
                        if stamp != c.last_stamp_key {
                            if let Some((dev, _)) = &stamp {
                                let owner = c.room.live_owner();
                                if owner.as_deref() != Some(dev.as_str()) {
                                    self.violations.push(format!(
                                        "[{now:.0}] stamp from {dev} stored while live owner is {owner:?}"
                                    ));
                                }
                            }
                            c.last_stamp_key = stamp;
                        }
                        // Invariant 2: room revision monotonic.
                        let rev = c.room.revision();
                        if rev < c.max_revision {
                            self.violations.push(format!(
                                "[{now:.0}] coordinator revision went from {} to {rev}",
                                c.max_revision
                            ));
                        }
                        c.max_revision = c.max_revision.max(rev);
                    }
                }
            }
        }
    }

    fn pump_effects(&mut self) {
        let now = self.now();
        let mut guard = 0;
        loop {
            let mut any = false;
            for i in 0..self.devices.len() {
                let effects = self.devices[i].take_effects();
                if effects.is_empty() {
                    continue;
                }
                any = true;
                let id = self.devices[i].id.clone();
                for e in effects {
                    match e {
                        DeviceEffect::Connect { candidates } => {
                            self.net.connect(now, &mut self.rng, &id, &candidates)
                        }
                        DeviceEffect::Send { peer, msg } => {
                            self.net.send(now, &mut self.rng, &id, &peer, msg)
                        }
                        DeviceEffect::Close { peer } => self.net.close(now, &id, &peer),
                        DeviceEffect::StartListener => {
                            let port = 4000 + i as u16;
                            self.devices[i].listener_port = Some(port);
                            self.net
                                .bind(&format!("ws://10.0.0.{}:{port}/", i + 1), &id);
                            self.devices[i].handle(Input::ListenerStarted { port });
                        }
                        DeviceEffect::StopListener => {
                            if let Some(port) = self.devices[i].listener_port.take() {
                                self.net.unbind(&format!("ws://10.0.0.{}:{port}/", i + 1));
                            }
                            self.devices[i].handle(Input::ListenerStopped);
                        }
                        DeviceEffect::Advertise(advert) => {
                            match advert {
                                Some(mut a) => {
                                    a.addresses = vec![format!("10.0.0.{}", i + 1)];
                                    self.lan_adverts.insert(i, a);
                                }
                                None => {
                                    self.lan_adverts.remove(&i);
                                }
                            }
                            self.broadcast_adverts();
                        }
                        DeviceEffect::Verify { peer, .. } => {
                            self.devices[i].handle(Input::CredentialVerified { peer, ok: true });
                        }
                        DeviceEffect::Scrobble {
                            track_id,
                            started_at,
                        } => {
                            self.server
                                .scrobbles
                                .push((track_id, started_at, id.clone()));
                        }
                    }
                }
            }
            guard += 1;
            if !any || guard > 100 {
                break;
            }
        }
    }

    /// mDNS: everyone on the LAN sees everyone's current record.
    fn broadcast_adverts(&mut self) {
        if self.cfg.topology != Topology::Lan {
            return;
        }
        let adverts: Vec<(usize, crate::connect::discovery::PeerAdvert)> = self
            .lan_adverts
            .iter()
            .map(|(i, a)| (*i, a.clone()))
            .collect();
        let present: BTreeSet<usize> = adverts.iter().map(|(i, _)| *i).collect();
        for i in 0..self.devices.len() {
            if self.devices[i].asleep || self.net.is_down(&self.devices[i].id) {
                continue;
            }
            for (j, a) in &adverts {
                if *j == i || self.net.is_down(&self.devices[*j].id) {
                    continue;
                }
                self.devices[i].handle(Input::PeerDiscovered(a.clone()));
            }
            let lost: Vec<String> = (0..self.devices.len())
                .filter(|j| {
                    *j != i && (!present.contains(j) || self.net.is_down(&self.devices[*j].id))
                })
                .map(|j| self.devices[j].id.clone())
                .collect();
            for device_id in lost {
                self.devices[i].handle(Input::PeerLost { device_id });
            }
        }
    }

    // -- actions ------------------------------------------------------------

    pub fn perform(&mut self, action: Action) {
        let now = self.now();
        self.actions_run.push((now, action.clone()));
        let dev = |a: &Action| -> usize {
            match a {
                Action::PlayTracks { device, .. }
                | Action::Next { device }
                | Action::Previous { device }
                | Action::PlayNext { device, .. }
                | Action::Shuffle { device, .. }
                | Action::TogglePlay { device }
                | Action::Seek { device, .. }
                | Action::ClaimTransport { device, .. }
                | Action::OpenPicker { device }
                | Action::HandoffTo { device, .. }
                | Action::ClosePicker { device }
                | Action::ResumeHere { device }
                | Action::Partition { device, .. }
                | Action::Heal { device }
                | Action::Cut { device }
                | Action::Crash { device, .. }
                | Action::Restart { device }
                | Action::Sleep { device, .. }
                | Action::Wake { device }
                | Action::Lossy { device, .. }
                | Action::Clean { device } => *device,
            }
        };
        let i = dev(&action);
        let id = self.devices[i].id.clone();
        let user_action = !matches!(
            action,
            Action::Partition { .. }
                | Action::Heal { .. }
                | Action::Cut { .. }
                | Action::Crash { .. }
                | Action::Restart { .. }
                | Action::Sleep { .. }
                | Action::Wake { .. }
                | Action::Lossy { .. }
                | Action::Clean { .. }
        );
        if user_action && (self.devices[i].asleep || self.crashed.contains_key(&i)) {
            return;
        }
        match action {
            Action::PlayTracks { tracks, .. } => {
                let op = SessionOp::PlayTracks {
                    server_id: "srv".into(),
                    track_ids: tracks,
                    start_index: 0,
                    label: "selection".into(),
                    shuffle: false,
                    save_outgoing: true,
                };
                self.devices[i].handle(Input::LocalOp { op });
            }
            Action::Next { .. } => self.devices[i].handle(Input::LocalOp {
                op: SessionOp::Next,
            }),
            Action::Previous { .. } => self.devices[i].handle(Input::LocalOp {
                op: SessionOp::Previous,
            }),
            Action::PlayNext { tracks, .. } => self.devices[i].handle(Input::LocalOp {
                op: SessionOp::PlayNext {
                    server_id: "srv".into(),
                    track_ids: tracks,
                },
            }),
            Action::Shuffle { enabled, .. } => self.devices[i].handle(Input::LocalOp {
                op: SessionOp::SetShuffle { enabled },
            }),
            Action::TogglePlay { .. } => {
                self.devices[i].handle(Input::TransportRequest(TransportCommand::TogglePlay))
            }
            Action::Seek { position_ms, .. } => {
                self.devices[i].handle(Input::TransportRequest(TransportCommand::SeekTo {
                    position_ms,
                }))
            }
            Action::ClaimTransport { takeover, .. } => {
                self.devices[i].handle(Input::ClaimTransport { takeover })
            }
            Action::OpenPicker { .. } => self.devices[i].handle(Input::OpenPicker),
            Action::HandoffTo { target, .. } => {
                let t = self.devices[target].id.clone();
                self.devices[i].handle(Input::HandoffTo { device_id: t });
            }
            Action::ClosePicker { .. } => self.devices[i].handle(Input::ClosePicker),
            Action::ResumeHere { .. } => self.devices[i].handle(Input::ResumeHere),
            Action::Partition { duration_ms, .. } => {
                self.partition_device(i);
                self.schedule_in(duration_ms, Action::Heal { device: i });
            }
            Action::Heal { .. } => self.heal_device(i),
            Action::Cut { .. } => {
                let others: Vec<String> = self.other_nodes(i);
                for o in others {
                    self.net.cut(now, &id, &o);
                }
            }
            Action::Crash { duration_ms, .. } => {
                if !self.crashed.contains_key(&i) {
                    let p = self.devices[i].persisted();
                    self.crashed.insert(i, p);
                    self.net.node_down(now, &id);
                    self.devices[i].asleep = true;
                    self.schedule_in(duration_ms, Action::Restart { device: i });
                }
            }
            Action::Restart { .. } => {
                if let Some(p) = self.crashed.remove(&i) {
                    self.net.node_up(&id);
                    let library = self.library.clone();
                    let scope = self.scope.clone();
                    let clock = self.devices[i].clock.clone();
                    let url = self
                        .coordinator
                        .is_some()
                        .then(|| COORDINATOR_URL.to_string());
                    let lan = self.cfg.topology == Topology::Lan;
                    let keep_log = self.devices[i].keep_log;
                    let mut fresh = SimDevice::new(&id, &scope, clock, library, url, lan, Some(p));
                    fresh.keep_log = keep_log;
                    fresh.log = std::mem::take(&mut self.devices[i].log);
                    fresh.scrobbles_reached =
                        std::mem::take(&mut self.devices[i].scrobbles_reached);
                    fresh.filed_count = self.devices[i].filed_count;
                    self.devices[i] = fresh;
                    self.device_rev_watermark[i] = None;
                    self.lan_adverts.remove(&i);
                }
            }
            Action::Sleep { duration_ms, .. } => {
                if !self.devices[i].asleep {
                    self.devices[i].asleep = true;
                    self.net.node_down(now, &id);
                    self.schedule_in(duration_ms, Action::Wake { device: i });
                }
            }
            Action::Wake { .. } => {
                if self.devices[i].asleep && !self.crashed.contains_key(&i) {
                    self.devices[i].asleep = false;
                    self.net.node_up(&id);
                    self.lan_adverts.remove(&i);
                    self.devices[i].handle(Input::Tick);
                }
            }
            Action::Lossy {
                drop, duration_ms, ..
            } => {
                let c = Conditions {
                    drop,
                    ..self.cfg.conditions
                };
                for o in self.other_nodes(i) {
                    self.net.set_conditions(&id, &o, c);
                }
                self.schedule_in(duration_ms, Action::Clean { device: i });
            }
            Action::Clean { .. } => {
                for o in self.other_nodes(i) {
                    self.net.set_conditions(&id, &o, self.cfg.conditions);
                }
            }
        }
        self.pump_effects();
        if self.cfg.topology == Topology::Lan {
            self.broadcast_adverts();
        }
    }

    fn other_nodes(&self, i: usize) -> Vec<String> {
        let mut v: Vec<String> = self
            .devices
            .iter()
            .enumerate()
            .filter(|(j, _)| *j != i)
            .map(|(_, d)| d.id.clone())
            .collect::<Vec<_>>();
        if self.coordinator.is_some() {
            v.push(COORDINATOR_NODE.into());
        }
        v
    }

    fn partition_device(&mut self, i: usize) {
        let id = self.devices[i].id.clone();
        for o in self.other_nodes(i) {
            self.net.partition(&id, &o);
        }
    }

    fn heal_device(&mut self, i: usize) {
        let id = self.devices[i].id.clone();
        for o in self.other_nodes(i) {
            self.net.heal(&id, &o);
        }
    }

    /// Heal every fault and wake/restart everyone.
    pub fn heal_everything(&mut self) {
        self.net.heal_all();
        let sleeping: Vec<usize> = self
            .devices
            .iter()
            .enumerate()
            .filter(|(_, d)| d.asleep)
            .map(|(i, _)| i)
            .collect();
        for i in sleeping {
            if self.crashed.contains_key(&i) {
                self.perform(Action::Restart { device: i });
            } else {
                self.perform(Action::Wake { device: i });
            }
        }
        let pending: Vec<(i64, u64)> = self.schedule.keys().copied().collect();
        for k in pending {
            if let Some(a) = self.schedule.get(&k) {
                if matches!(
                    a,
                    Action::Heal { .. }
                        | Action::Wake { .. }
                        | Action::Restart { .. }
                        | Action::Clean { .. }
                ) {
                    let a = a.clone();
                    self.schedule.remove(&k);
                    self.perform(a);
                }
            }
        }
        for i in 0..self.devices.len() {
            self.perform(Action::Clean { device: i });
        }
        self.schedule.clear();
    }

    // -- invariants ---------------------------------------------------------

    /// Which room a device is attached to, if any.
    pub fn room_key(&self, i: usize) -> Option<String> {
        let d = &self.devices[i];
        if d.asleep {
            return None;
        }
        match self.cfg.topology {
            Topology::Coordinator => d
                .engine
                .is_connected()
                .then(|| COORDINATOR_NODE.to_string()),
            Topology::Lan => {
                if d.engine.is_serving() {
                    Some(d.id.clone())
                } else if d.engine.is_connected() {
                    d.engine.lan_leader().map(|l| l.to_string())
                } else {
                    None
                }
            }
        }
    }

    /// The current transport epoch of a room (the coordinator's, or the
    /// serving LAN device's own room).
    fn room_epoch(&self, room: &str) -> Option<u32> {
        if room == COORDINATOR_NODE {
            return self.coordinator.as_ref().map(|c| c.room.lease().epoch);
        }
        self.devices
            .iter()
            .find(|d| d.id == room)
            .map(|d| d.engine.room().lease().epoch)
    }

    fn check_invariants(&mut self) {
        let now = self.now();
        // 1. never two owners per room: at most one device holds the room's
        //    current epoch, and a device holding a stale epoch (the room moved
        //    on and the news is still in flight) stands down within a bounded
        //    time. Detached devices know they cannot write.
        let mut current_holders: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for i in 0..self.devices.len() {
            let d = &self.devices[i];
            let mut stale = false;
            if d.owns() && !d.engine.is_detached() {
                if let Some(room) = self.room_key(i) {
                    let epoch = self.room_epoch(&room);
                    // A lease counts for a room only if that room granted it: the
                    // serving device's own room, or the remote room a client follows.
                    let granted_by_room = d.engine.is_serving() || d.engine.held_from_remote();
                    if granted_by_room && epoch == d.engine.held_epoch() {
                        current_holders.entry(room).or_default().push(d.id.clone());
                    } else {
                        stale = true;
                    }
                }
            }
            if stale {
                let since = *self.stale_owner_since[i].get_or_insert(now);
                if now - since > STALE_OWNER_GRACE_MS {
                    self.violations.push(format!(
                        "[{now:.0}] {} still owns transport {:.0} ms after the room moved to another epoch",
                        d.id,
                        now - since
                    ));
                    self.stale_owner_since[i] = None;
                }
            } else {
                self.stale_owner_since[i] = None;
            }
        }
        for (room, list) in current_holders {
            if list.len() > 1 {
                self.violations.push(format!(
                    "[{now:.0}] two transport owners in room {room}: {list:?}"
                ));
            }
        }
        // 2. device confirmed revision monotonic while attached
        for i in 0..self.devices.len() {
            let d = &self.devices[i];
            if self.room_key(i).is_some() && d.engine.pending_count() == 0 {
                let rev = d.engine.document().revision;
                if let Some(w) = self.device_rev_watermark[i] {
                    if rev < w {
                        self.violations.push(format!(
                            "[{now:.0}] {} revision went from {w} to {rev} while attached",
                            d.id
                        ));
                    }
                }
                self.device_rev_watermark[i] = Some(rev);
            } else {
                self.device_rev_watermark[i] = None;
            }
        }
        // 7. filing never leaves local ops behind
        for d in &self.devices {
            if d.filed_count > 0 && d.engine.has_unsynced() && d.engine.is_connected() {
                self.violations.push(format!(
                    "[{now:.0}] {} kept unsynced ops after filing its state",
                    d.id
                ));
            }
        }
    }

    /// Checks that only hold once the network is healed and quiet.
    pub fn check_quiescent(&mut self) {
        let now = self.now();
        // 5. convergence: every attached device holds its room's document,
        //    and (LAN) the healed network has settled on one room.
        if self.cfg.topology == Topology::Lan {
            let rooms: BTreeSet<String> = (0..self.devices.len())
                .filter_map(|i| self.room_key(i))
                .collect();
            if rooms.len() > 1 {
                self.violations.push(format!(
                    "[{now:.0}] LAN did not settle on one room after heal: {rooms:?}"
                ));
            }
        }
        for i in 0..self.devices.len() {
            let room_doc = match self.cfg.topology {
                Topology::Coordinator => self
                    .coordinator
                    .as_ref()
                    .map(|c| c.room.replica().document.clone()),
                Topology::Lan => self
                    .room_key(i)
                    .and_then(|leader| self.devices.iter().find(|d| d.id == leader))
                    .map(|d| d.engine.room().replica().document.clone()),
            };
            let d = &self.devices[i];
            if d.asleep {
                continue;
            }
            if self.cfg.topology == Topology::Coordinator && !d.engine.is_connected() {
                self.violations
                    .push(format!("[{now:.0}] {} not connected after heal", d.id));
                continue;
            }
            if d.engine.pending_count() != 0 {
                self.violations
                    .push(format!("[{now:.0}] {} still has pending ops", d.id));
            }
            if let Some(rd) = &room_doc {
                if self.room_key(i).is_some() && !same_session_state(d.engine.document(), rd) {
                    self.violations.push(format!(
                        "[{now:.0}] {} document diverged from the room: rev {} vs {}: {}",
                        d.id,
                        d.engine.document().revision,
                        rd.revision,
                        doc_diff(d.engine.document(), rd)
                    ));
                }
            }
        }
        // 3. scrobbles: every reached pair exactly once
        let mut reached: BTreeSet<(String, i64)> = BTreeSet::new();
        for d in &self.devices {
            for (t, s) in &d.scrobbles_reached {
                reached.insert((t.clone(), *s as i64));
            }
        }
        for (t, s) in &reached {
            let n = self.server.count(t, *s as f64);
            if n != 1 {
                self.violations.push(format!(
                    "[{now:.0}] scrobble ({t}, {s}) submitted {n} times"
                ));
            }
        }
        for (t, s, _) in &self.server.scrobbles {
            if !reached.contains(&(t.clone(), *s as i64)) {
                self.violations.push(format!(
                    "[{now:.0}] scrobble ({t}, {s}) submitted but never reached"
                ));
            }
        }
        for d in &self.devices {
            if !d.outbox.is_empty() && !d.asleep {
                self.violations.push(format!(
                    "[{now:.0}] {} has {} scrobbles without a verdict; engine {:?}",
                    d.id,
                    d.outbox.len(),
                    d.engine
                ));
            }
        }
        // 1b. at quiescence the room's owner is the one device that owns
        if let Some(c) = &self.coordinator {
            let owner = c.room.live_owner();
            let owning: Vec<String> = self
                .devices
                .iter()
                .filter(|d| d.owns() && !d.asleep)
                .map(|d| d.id.clone())
                .collect();
            if owning.len() > 1
                || (owning.len() == 1 && owner.as_deref() != Some(owning[0].as_str()))
            {
                self.violations.push(format!(
                    "[{now:.0}] room owner {owner:?} vs owning devices {owning:?}"
                ));
            }
        }
    }

    /// Run the tail: heal, let everything settle, check at a quiet instant
    /// (nothing in flight, no op awaiting a verdict).
    pub fn finish(&mut self) {
        self.heal_everything();
        self.run_for(120_000.0);
        for _ in 0..240 {
            let quiet = self.net.in_flight() == 0
                && self.devices.iter().all(|d| d.engine.pending_count() == 0);
            if quiet {
                break;
            }
            self.run_for(250.0);
        }
        self.check_quiescent();
    }

    pub fn assert_ok(&self) {
        if !self.violations.is_empty() {
            let mut msg = format!("seed {} violations:\n", self.cfg.seed);
            for v in &self.violations {
                msg.push_str(v);
                msg.push('\n');
            }
            msg.push_str("actions:\n");
            for (t, a) in &self.actions_run {
                msg.push_str(&format!("  [{t:.0}] {a:?}\n"));
            }
            msg.push_str("engines at the end:\n");
            for d in &self.devices {
                msg.push_str(&format!(
                    "  {:?} asleep={} links={}\n",
                    d.engine,
                    d.asleep,
                    self.net.link_count()
                ));
            }
            for d in &self.devices {
                for l in &d.log {
                    msg.push_str(l);
                    msg.push('\n');
                }
            }
            panic!("{msg}");
        }
    }

    // -- random scenarios -----------------------------------------------------

    /// Generate and run a randomised scenario: `actions` user/fault actions
    /// spread over the run, then a quiescent tail with the final checks.
    pub fn run_random(cfg: WorldConfig, actions: usize) -> World {
        let mut w = World::new(cfg.clone());
        let mut rng = ChaCha8Rng::seed_from_u64(cfg.seed ^ 0xabcdef);
        let n = cfg.devices;
        let ids = w.library.ids();
        // let everyone connect
        w.run_for(3_000.0);
        // someone starts playing
        let first = rng.random_range(0..n);
        let tracks: Vec<TrackId> = (0..rng.random_range(3..=8))
            .map(|_| ids.choose(&mut rng).cloned().unwrap())
            .collect();
        w.perform(Action::PlayTracks {
            device: first,
            tracks,
        });
        w.perform(Action::ClaimTransport {
            device: first,
            takeover: false,
        });
        w.run_for(2_000.0);
        for _ in 0..actions {
            let gap = rng.random_range(500.0..12_000.0);
            w.run_for(gap);
            let d = rng.random_range(0..n);
            let roll: f64 = rng.random();
            let action = if roll < 0.10 {
                let tracks: Vec<TrackId> = (0..rng.random_range(2..=6))
                    .map(|_| ids.choose(&mut rng).cloned().unwrap())
                    .collect();
                Action::PlayTracks { device: d, tracks }
            } else if roll < 0.28 {
                Action::Next { device: d }
            } else if roll < 0.33 {
                Action::Previous { device: d }
            } else if roll < 0.40 {
                let tracks: Vec<TrackId> = (0..rng.random_range(1..=3))
                    .map(|_| ids.choose(&mut rng).cloned().unwrap())
                    .collect();
                Action::PlayNext { device: d, tracks }
            } else if roll < 0.44 {
                Action::Shuffle {
                    device: d,
                    enabled: rng.random(),
                }
            } else if roll < 0.50 {
                Action::TogglePlay { device: d }
            } else if roll < 0.54 {
                Action::Seek {
                    device: d,
                    position_ms: rng.random_range(0..200_000),
                }
            } else if roll < 0.62 {
                // handoff: the owner opens the picker, then picks after a beat
                if let Some(owner) = w.devices.iter().position(|x| x.owns()) {
                    let target = rng.random_range(0..n);
                    if target != owner {
                        w.perform(Action::OpenPicker { device: owner });
                        let beat = rng.random_range(300.0..4_000.0);
                        if rng.random::<f64>() < 0.85 {
                            w.schedule_in(
                                beat,
                                Action::HandoffTo {
                                    device: owner,
                                    target,
                                },
                            );
                        } else {
                            w.schedule_in(beat, Action::ClosePicker { device: owner });
                        }
                    }
                }
                continue;
            } else if roll < 0.68 {
                if w.devices[d].resume_offer.is_some() {
                    Action::ResumeHere { device: d }
                } else if !w
                    .devices
                    .iter()
                    .any(|x| x.owns() && !x.engine.is_detached())
                {
                    Action::ClaimTransport {
                        device: d,
                        takeover: false,
                    }
                } else {
                    Action::ClaimTransport {
                        device: d,
                        takeover: true,
                    }
                }
            } else if roll < 0.78 {
                Action::Partition {
                    device: d,
                    duration_ms: rng.random_range(2_000.0..90_000.0),
                }
            } else if roll < 0.83 {
                Action::Cut { device: d }
            } else if roll < 0.88 {
                Action::Crash {
                    device: d,
                    duration_ms: rng.random_range(5_000.0..120_000.0),
                }
            } else if roll < 0.93 {
                Action::Sleep {
                    device: d,
                    duration_ms: rng.random_range(5_000.0..120_000.0),
                }
            } else {
                Action::Lossy {
                    device: d,
                    drop: rng.random_range(0.05..0.5),
                    duration_ms: rng.random_range(5_000.0..60_000.0),
                }
            };
            w.perform(action);
        }
        w.finish();
        w
    }
}

/// Top-level fields on which two documents differ, for violation messages.
pub fn doc_diff(a: &crate::api::SessionDocument, b: &crate::api::SessionDocument) -> String {
    let (Ok(serde_json::Value::Object(a)), Ok(serde_json::Value::Object(b))) =
        (serde_json::to_value(a), serde_json::to_value(b))
    else {
        return "unserialisable".into();
    };
    let mut out = vec![];
    for (k, va) in &a {
        if let Some(vb) = b.get(k) {
            if va != vb {
                let sa = va.to_string();
                let sb = vb.to_string();
                out.push(format!(
                    "{k}: {} vs {}",
                    &sa[..sa.len().min(160)],
                    &sb[..sb.len().min(160)]
                ));
            }
        }
    }
    out.join("; ")
}

pub fn device_name(i: usize) -> String {
    format!("dev-{}", (b'a' + i as u8) as char)
}
