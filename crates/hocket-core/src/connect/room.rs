//! The coordinator role: one room per scope.
//!
//! A room is *transport and replica, never authority*: it orders ops against
//! the document revision, relays what it accepted, arbitrates the transport
//! lease, keeps presence, answers clock pings, holds the resume replica and
//! the scrobble dedupe log. It never plays audio and never keeps a credential
//! beyond the handshake (verification is delegated to the host through
//! [`RoomOutput::Verify`]).
//!
//! The same struct runs inside every app (embedded in the engine, driven over
//! a loopback when the device is alone or is the elected LAN coordinator) and
//! inside the hosted coordinator binary. Pure and clock-injected: the host
//! feeds [`RoomInput`]s and performs every [`RoomOutput`].

use std::sync::Arc;

use crate::api::{DeviceId, DeviceInfo, EpochMs, QueueKey, TransportLease};
use crate::connect::lease::{LeaseError, LeaseEvent, LeaseMachine};
use crate::connect::replica::{new_replica, ReplicaExt, ScrobbleClaim};
use crate::connect::wire::{
    negotiate, scope_key, Credential, LastStamp, Msg, RefuseReason, RejectReason, ReplicaState,
    WireMessage,
};
use crate::connect::{
    apply_op, doc_is_trivial, op_context, PeerId, ReducerHandle, SessionOp, LOOPBACK,
};
use crate::util::Clock;

/// A member that has sent nothing for this long is dropped. Clients ping
/// every 5 s, so this is six missed pings.
pub const MEMBER_TIMEOUT_MS: f64 = 30_000.0;
/// How often the replica's expiring parts are swept.
const SWEEP_INTERVAL_MS: f64 = 60_000.0;

#[derive(Debug, Clone)]
pub struct RoomConfig {
    /// The scope this room serves. `Hello.scope` must match.
    pub scope: String,
    /// Require a credential and delegate a ping to the host before admitting.
    pub verify: bool,
    /// Refuse joins past this many members.
    pub max_members: usize,
    /// Session id for a fresh document (tests pass a fixed one).
    pub session_id: Option<String>,
}

impl RoomConfig {
    pub fn new(scope: impl Into<String>) -> Self {
        RoomConfig {
            scope: scope.into(),
            verify: false,
            max_members: 32,
            session_id: None,
        }
    }

    pub fn verified(mut self) -> Self {
        self.verify = true;
        self
    }
}

/// Variants carry whole frames/documents on purpose: boxing would only move the
/// allocation, and these are handled once each.
#[allow(clippy::large_enum_variant)]
#[derive(Debug)]
pub enum RoomInput {
    /// A socket was accepted. Nothing is known until its `Hello`.
    Connected(PeerId),
    Message(PeerId, WireMessage),
    Disconnected(PeerId),
    /// The host finished verifying a pending `Hello`.
    Verified {
        peer: PeerId,
        ok: bool,
    },
    Tick,
}

#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, PartialEq)]
pub enum RoomOutput {
    Send(PeerId, WireMessage),
    /// Close the socket (after any preceding `Send`s were flushed).
    Close(PeerId),
    /// Proxy a Subsonic `ping` with this credential and answer with
    /// [`RoomInput::Verified`]. The room does not keep the credential.
    Verify {
        peer: PeerId,
        credential: Credential,
    },
    /// The replica changed in a way worth persisting (op, lease, play/pause,
    /// track change, heartbeat). Read it with [`Room::replica`].
    ReplicaChanged,
}

#[derive(Debug, Clone)]
struct Member {
    peer: PeerId,
    device: DeviceInfo,
    protocol: u32,
    last_seen: EpochMs,
    /// Pre-buffered and ready for this key.
    ready_key: Option<QueueKey>,
    picker_open: bool,
}

#[derive(Debug, Clone)]
struct PendingHello {
    device: DeviceInfo,
    protocol: u32,
}

/// The coordinator role for one scope.
pub struct Room {
    cfg: RoomConfig,
    clock: Arc<dyn Clock>,
    reducer: ReducerHandle,
    replica: ReplicaState,
    lease: LeaseMachine,
    members: Vec<Member>,
    pending: Vec<(PeerId, PendingHello)>,
    /// Sockets connected but not yet admitted.
    unknown: Vec<(PeerId, EpochMs)>,
    dirty: bool,
    last_sweep: EpochMs,
    out: Vec<RoomOutput>,
}

impl std::fmt::Debug for Room {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Room")
            .field("scope", &self.cfg.scope)
            .field("revision", &self.replica.document.revision)
            .field("members", &self.members.len())
            .field("lease", &self.lease.lease())
            .finish()
    }
}

impl Room {
    /// A room around a persisted replica, or a fresh one.
    pub fn new(
        cfg: RoomConfig,
        clock: Arc<dyn Clock>,
        reducer: ReducerHandle,
        replica: Option<ReplicaState>,
    ) -> Self {
        let now = clock.now_ms();
        let replica = replica.unwrap_or_else(|| {
            let sid = cfg.session_id.clone().unwrap_or_else(crate::util::new_id);
            new_replica(
                crate::session::new_document(cfg.scope.clone(), sid, now),
                now,
            )
        });
        let lease = LeaseMachine::from_lease(replica.transport_lease.clone());
        Room {
            cfg,
            clock,
            reducer,
            replica,
            lease,
            members: vec![],
            pending: vec![],
            unknown: vec![],
            dirty: false,
            last_sweep: now,
            out: vec![],
        }
    }

    pub fn scope(&self) -> &str {
        &self.cfg.scope
    }

    pub fn replica(&self) -> &ReplicaState {
        &self.replica
    }

    pub fn lease(&self) -> &TransportLease {
        self.lease.lease()
    }

    pub fn revision(&self) -> u32 {
        self.replica.document.revision
    }

    pub fn member_count(&self) -> usize {
        self.members.len()
    }

    pub fn member_device_ids(&self) -> Vec<DeviceId> {
        self.members.iter().map(|m| m.device.id.clone()).collect()
    }

    pub fn has_member(&self, device_id: &str) -> bool {
        self.members.iter().any(|m| m.device.id == device_id)
    }

    /// Replace the replica document wholesale (the host adopting a local
    /// document when it becomes the LAN coordinator). Keeps the lease.
    pub fn adopt_document(&mut self, doc: crate::api::SessionDocument) {
        let now = self.now();
        let mut doc = doc;
        doc.transport.lease = self.lease.lease().clone();
        if doc.revision < self.replica.document.revision {
            doc.revision = self.replica.document.revision;
        }
        self.replica.set_document(doc, now);
        self.dirty = true;
    }

    /// Pre-load the dedupe log with pairs the host already knows were
    /// scrobbled (the log follows the session when the LAN role moves).
    pub fn seed_scrobbles(&mut self, pairs: &[(crate::api::TrackId, EpochMs, DeviceId)]) {
        let now = self.now();
        for (track_id, started_at, device_id) in pairs {
            self.replica
                .claim_scrobble(track_id, *started_at, device_id, now);
        }
    }

    /// Live transport owner, if any.
    pub fn live_owner(&self) -> Option<DeviceId> {
        self.lease.live_owner(self.now()).cloned()
    }

    fn now(&self) -> EpochMs {
        self.clock.now_ms()
    }

    /// Drive the room. Perform every output in order.
    pub fn handle(&mut self, input: RoomInput) -> Vec<RoomOutput> {
        match input {
            RoomInput::Connected(peer) => {
                let now = self.now();
                self.unknown.push((peer, now));
            }
            RoomInput::Message(peer, msg) => self.on_message(peer, msg),
            RoomInput::Disconnected(peer) => self.on_disconnected(&peer),
            RoomInput::Verified { peer, ok } => self.on_verified(&peer, ok),
            RoomInput::Tick => self.on_tick(),
        }
        if self.dirty {
            self.dirty = false;
            self.out.push(RoomOutput::ReplicaChanged);
        }
        std::mem::take(&mut self.out)
    }

    // -- helpers ------------------------------------------------------------

    fn send(&mut self, peer: &str, msg: Msg) {
        self.out
            .push(RoomOutput::Send(peer.to_string(), WireMessage::new(msg)));
    }

    fn broadcast(&mut self, msg: Msg, except: Option<&str>) {
        let peers: Vec<PeerId> = self
            .members
            .iter()
            .filter(|m| Some(m.peer.as_str()) != except)
            .map(|m| m.peer.clone())
            .collect();
        for p in peers {
            self.send(&p, msg.clone());
        }
    }

    /// Negotiated protocol of a member (for diagnostics).
    pub fn member_protocol(&self, device_id: &str) -> Option<u32> {
        self.members
            .iter()
            .find(|m| m.device.id == device_id)
            .map(|m| m.protocol)
    }

    fn member_by_peer(&self, peer: &str) -> Option<&Member> {
        self.members.iter().find(|m| m.peer == peer)
    }

    fn member_mut_by_peer(&mut self, peer: &str) -> Option<&mut Member> {
        self.members.iter_mut().find(|m| m.peer == peer)
    }

    fn peer_of_device(&self, device_id: &str) -> Option<PeerId> {
        self.members
            .iter()
            .find(|m| m.device.id == device_id)
            .map(|m| m.peer.clone())
    }

    fn presence_list(&self) -> Vec<DeviceInfo> {
        let now = self.now();
        let owner = self.lease.live_owner(now).cloned();
        self.members
            .iter()
            .map(|m| {
                let mut d = m.device.clone();
                d.playing = owner.as_deref() == Some(d.id.as_str());
                d.ready = m.ready_key.is_some();
                d.last_seen = m.last_seen;
                d.is_self = false;
                d
            })
            .collect()
    }

    fn broadcast_presence(&mut self) {
        let list = self.presence_list();
        self.broadcast(Msg::Presence { devices: list }, None);
    }

    fn broadcast_lease(&mut self) {
        let lease = self.lease.lease().clone();
        let now = self.now();
        self.replica.set_lease(lease.clone(), now);
        self.dirty = true;
        self.broadcast(
            Msg::LeaseGranted {
                lease,
                ack_of: None,
            },
            None,
        );
        self.broadcast_presence();
    }

    fn fenced(&mut self, peer: &str, current: TransportLease) {
        self.send(
            peer,
            Msg::LeaseFenced {
                current_epoch: current.epoch,
                lease: current,
            },
        );
    }

    fn refuse(&mut self, peer: &str, reason: RefuseReason, message: &str) {
        self.send(
            peer,
            Msg::Refuse {
                reason,
                message: message.to_string(),
            },
        );
        self.out.push(RoomOutput::Close(peer.to_string()));
        self.unknown.retain(|(p, _)| p != peer);
    }

    // -- handshake ----------------------------------------------------------

    fn on_message(&mut self, peer: PeerId, msg: WireMessage) {
        let now = self.clock.now_ms();
        if let Some(m) = self.member_mut_by_peer(&peer) {
            m.last_seen = now;
        }
        match msg.msg {
            Msg::Hello {
                device,
                protocol_min,
                protocol_max,
                scope,
                credential,
                ..
            } => self.on_hello(&peer, device, protocol_min, protocol_max, scope, credential),
            other => {
                if self.member_by_peer(&peer).is_none() {
                    tracing::debug!(peer, msg = other.name(), "message before hello ignored");
                    return;
                }
                self.on_member_message(&peer, other)
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn on_hello(
        &mut self,
        peer: &str,
        device: DeviceInfo,
        protocol_min: u32,
        protocol_max: u32,
        scope: String,
        credential: Option<Credential>,
    ) {
        let protocol = match negotiate(protocol_min, protocol_max) {
            Ok(p) => p,
            Err(e) => {
                let reason = if protocol_max < crate::connect::wire::PROTOCOL_MIN {
                    RefuseReason::ProtocolTooOld
                } else {
                    RefuseReason::ProtocolTooNew
                };
                self.refuse(peer, reason, &e.to_string());
                return;
            }
        };
        if scope != self.cfg.scope {
            self.refuse(
                peer,
                RefuseReason::ScopeMismatch,
                "scope does not match this room",
            );
            return;
        }
        if self.members.len() >= self.cfg.max_members {
            self.refuse(peer, RefuseReason::Full, "room is full");
            return;
        }
        if self.cfg.verify && peer != LOOPBACK {
            let Some(cred) = credential else {
                self.refuse(peer, RefuseReason::Unauthorised, "credential required");
                return;
            };
            if scope_key(&cred.server_url, &cred.username) != self.cfg.scope {
                self.refuse(
                    peer,
                    RefuseReason::ScopeMismatch,
                    "credential does not match scope",
                );
                return;
            }
            self.pending.retain(|(p, _)| p != peer);
            self.pending
                .push((peer.to_string(), PendingHello { device, protocol }));
            self.out.push(RoomOutput::Verify {
                peer: peer.to_string(),
                credential: cred,
            });
            return;
        }
        self.admit(peer, device, protocol);
    }

    fn on_verified(&mut self, peer: &str, ok: bool) {
        let Some(idx) = self.pending.iter().position(|(p, _)| p == peer) else {
            return;
        };
        let (_, pending) = self.pending.remove(idx);
        if !ok {
            self.refuse(
                peer,
                RefuseReason::Unauthorised,
                "server rejected credential",
            );
            return;
        }
        self.admit(peer, pending.device, pending.protocol);
    }

    fn admit(&mut self, peer: &str, mut device: DeviceInfo, protocol: u32) {
        let now = self.now();
        self.unknown.retain(|(p, _)| p != peer);
        // A device reconnecting on a new socket replaces its old one.
        let stale: Vec<PeerId> = self
            .members
            .iter()
            .filter(|m| m.device.id == device.id && m.peer != peer)
            .map(|m| m.peer.clone())
            .collect();
        for p in stale {
            self.send(
                &p,
                Msg::Bye {
                    reason: "replaced by a newer connection".into(),
                },
            );
            self.out.push(RoomOutput::Close(p.clone()));
            self.members.retain(|m| m.peer != p);
        }
        device.is_self = false;
        device.last_seen = now;
        self.replica.touch_device(&device, now);
        // A second Hello on the same socket re-admits rather than duplicates.
        self.members.retain(|m| m.peer != peer);
        let members_before = self.presence_list();
        self.members.push(Member {
            peer: peer.to_string(),
            device,
            protocol,
            last_seen: now,
            ready_key: None,
            picker_open: false,
        });
        // Always sent, even when empty: the joiner adopts the room's session
        // identity from it (or pushes its own document into a trivial one).
        let replica = Some(self.replica.clone());
        self.send(
            peer,
            Msg::Welcome {
                session_clock_ms: now,
                accepted_protocol: protocol,
                replica,
                members: members_before,
                extra: Default::default(),
            },
        );
        self.dirty = true;
        self.broadcast_presence();
        tracing::info!(scope = %self.cfg.scope, peer, protocol, members = self.members.len(), "member joined");
    }

    fn on_disconnected(&mut self, peer: &str) {
        self.unknown.retain(|(p, _)| p != peer);
        self.pending.retain(|(p, _)| p != peer);
        let Some(idx) = self.members.iter().position(|m| m.peer == peer) else {
            return;
        };
        let m = self.members.remove(idx);
        tracing::info!(scope = %self.cfg.scope, peer, device = %m.device.id, "member left");
        if m.picker_open {
            self.broadcast(
                Msg::HandoffPickerClose {
                    from: m.device.id.clone(),
                },
                None,
            );
            for other in &mut self.members {
                other.ready_key = None;
            }
        }
        self.broadcast_presence();
        // The lease is deliberately kept: reconnecting inside the window keeps ownership.
    }

    // -- members ------------------------------------------------------------

    fn on_member_message(&mut self, peer: &str, msg: Msg) {
        let now = self.now();
        let Some(me) = self.member_by_peer(peer).cloned() else {
            return;
        };
        let device_id = me.device.id.clone();
        match msg {
            Msg::Hello { .. } | Msg::Welcome { .. } | Msg::Refuse { .. } | Msg::Unknown => {}
            Msg::Bye { .. } => {
                self.out.push(RoomOutput::Close(peer.to_string()));
                self.on_disconnected(peer);
            }
            Msg::Op {
                base_revision,
                op,
                device_id: op_device,
                op_id,
                epoch,
                at,
                position_ms,
            } => {
                if let Some(e) = epoch {
                    if let Err(LeaseError::Fenced { current } | LeaseError::Held { current }) =
                        self.lease.check(&device_id, e, now)
                    {
                        self.send(
                            peer,
                            Msg::OpReject {
                                op_id,
                                current_revision: self.revision(),
                                reason: RejectReason::Fenced,
                                document: self.replica.document.clone(),
                            },
                        );
                        self.fenced(peer, current);
                        return;
                    }
                }
                if base_revision != self.revision() {
                    self.send(
                        peer,
                        Msg::OpReject {
                            op_id,
                            current_revision: self.revision(),
                            reason: RejectReason::Stale,
                            document: self.replica.document.clone(),
                        },
                    );
                    return;
                }
                // Apply with the originator's time and position, exactly as every replica will.
                let at = if at > 0.0 { at } else { now };
                let ctx = op_context(&op_id, at, position_ms);
                let target = base_revision.saturating_add(1);
                let mut doc = self.replica.document.clone();
                let adopt_identity =
                    matches!(op, SessionOp::Replace { .. }) && doc_is_trivial(&doc);
                match apply_op(self.reducer.as_ref(), &doc, &op, &ctx, target) {
                    Ok(next) => {
                        doc = next;
                        if adopt_identity {
                            if let SessionOp::Replace { document } = &op {
                                doc.session_id = document.session_id.clone();
                            }
                        }
                        self.replica.set_document(doc, now);
                        self.dirty = true;
                        self.send(
                            peer,
                            Msg::OpAck {
                                op_id: op_id.clone(),
                                revision: target,
                            },
                        );
                        let committed = Msg::OpCommitted {
                            op,
                            revision: target,
                            device_id: op_device,
                            op_id,
                            at,
                            position_ms,
                        };
                        self.broadcast(committed, Some(peer));
                    }
                    Err(e) => {
                        tracing::debug!(op_id, error = %e, "op rejected as unapplicable");
                        self.send(
                            peer,
                            Msg::OpReject {
                                op_id,
                                current_revision: self.revision(),
                                reason: RejectReason::Unapplicable,
                                document: self.replica.document.clone(),
                            },
                        );
                    }
                }
            }
            Msg::SyncRequest => {
                let document = self.replica.document.clone();
                self.send(peer, Msg::Document { document });
            }
            Msg::OpAck { .. }
            | Msg::OpReject { .. }
            | Msg::OpCommitted { .. }
            | Msg::Document { .. } => {}

            Msg::TransportStamp {
                key,
                position,
                played_ms,
                started_at,
                scrobbled,
                epoch,
                ..
            } => {
                if let Err(LeaseError::Fenced { current } | LeaseError::Held { current }) =
                    self.lease.check(&device_id, epoch, now)
                {
                    self.fenced(peer, current);
                    return;
                }
                let prev = self.replica.last_stamp.clone();
                let stamp = LastStamp {
                    device_id: device_id.clone(),
                    device_name: me.device.name.clone(),
                    key: key.clone(),
                    position: position.clone(),
                    played_ms,
                    started_at,
                    scrobbled,
                };
                // Write cadence: track change and play/pause persist; a seek only updates memory.
                let persist = match &prev {
                    Some(p) => {
                        p.key != key
                            || p.position.is_playing != position.is_playing
                            || p.device_id != device_id
                    }
                    None => true,
                };
                self.replica.set_stamp(stamp, now);
                if persist {
                    self.dirty = true;
                }
                let relay = Msg::TransportStamp {
                    device_id,
                    key,
                    position,
                    played_ms,
                    started_at,
                    scrobbled,
                    epoch,
                };
                self.broadcast(relay, Some(peer));
            }
            Msg::TransportRequest { command, from } => {
                if let Some(owner) = self.lease.live_owner(now).cloned() {
                    if let Some(p) = self.peer_of_device(&owner) {
                        self.send(&p, Msg::TransportRequest { command, from });
                    }
                }
            }
            Msg::LeaseHeartbeat { epoch, sent_at } => {
                match self.lease.heartbeat(&device_id, epoch, now) {
                    Ok(LeaseEvent::Renewed { lease }) | Ok(LeaseEvent::Changed { lease, .. }) => {
                        let now = self.now();
                        self.replica.set_lease(lease.clone(), now);
                        self.dirty = true;
                        self.send(
                            peer,
                            Msg::LeaseGranted {
                                lease,
                                ack_of: Some(sent_at),
                            },
                        );
                    }
                    Err(LeaseError::Fenced { current } | LeaseError::Held { current }) => {
                        self.fenced(peer, current)
                    }
                }
            }
            Msg::LeaseClaim {
                epoch_expected,
                takeover,
                sent_at,
            } => {
                match self.lease.claim(&device_id, epoch_expected, takeover, now) {
                    Ok(LeaseEvent::Changed { .. }) => {
                        // The claimant gets its echo first, then everyone the change.
                        let lease = self.lease.lease().clone();
                        self.send(
                            peer,
                            Msg::LeaseGranted {
                                lease,
                                ack_of: Some(sent_at),
                            },
                        );
                        self.broadcast_lease();
                    }
                    Ok(LeaseEvent::Renewed { lease }) => {
                        let now = self.now();
                        self.replica.set_lease(lease.clone(), now);
                        self.dirty = true;
                        self.send(
                            peer,
                            Msg::LeaseGranted {
                                lease,
                                ack_of: Some(sent_at),
                            },
                        );
                    }
                    Err(LeaseError::Fenced { current } | LeaseError::Held { current }) => {
                        self.fenced(peer, current)
                    }
                }
            }
            Msg::LeaseRelease { epoch } | Msg::HandoffRelease { epoch } => {
                match self.lease.release(&device_id, epoch, now) {
                    Ok(_) => self.broadcast_lease(),
                    Err(LeaseError::Fenced { current } | LeaseError::Held { current }) => {
                        // A release after a takeover already moved the lease is expected; only
                        // tell the sender when it still thinks it owns something.
                        if current.owner.as_deref() == Some(device_id.as_str()) {
                            self.fenced(peer, current);
                        }
                    }
                }
            }
            Msg::LeaseGranted { .. } | Msg::LeaseFenced { .. } | Msg::Presence { .. } => {}

            Msg::ClockPing { t0 } => {
                let t = self.now();
                self.send(peer, Msg::ClockPong { t0, t1: t, t2: t });
            }
            Msg::ClockPong { .. } => {}

            Msg::HandoffPickerOpen { .. } => {
                if let Some(m) = self.member_mut_by_peer(peer) {
                    m.picker_open = true;
                }
                self.broadcast(Msg::HandoffPickerOpen { from: device_id }, Some(peer));
            }
            Msg::HandoffPickerClose { .. } => {
                if let Some(m) = self.member_mut_by_peer(peer) {
                    m.picker_open = false;
                }
                for m in &mut self.members {
                    m.ready_key = None;
                }
                self.broadcast(Msg::HandoffPickerClose { from: device_id }, Some(peer));
                self.broadcast_presence();
            }
            Msg::HandoffPrepare {
                target,
                key,
                track_id,
                position_ms,
                ..
            } => {
                if let Some(p) = self.peer_of_device(&target) {
                    self.send(
                        &p,
                        Msg::HandoffPrepare {
                            from: device_id,
                            target,
                            key,
                            track_id,
                            position_ms,
                        },
                    );
                }
            }
            Msg::HandoffReady {
                from, key, ready, ..
            } => {
                if let Some(m) = self.member_mut_by_peer(peer) {
                    m.ready_key = if ready { Some(key.clone()) } else { None };
                }
                if let Some(p) = self.peer_of_device(&from) {
                    self.send(
                        &p,
                        Msg::HandoffReady {
                            from,
                            target: device_id,
                            key,
                            ready,
                        },
                    );
                }
                self.broadcast_presence();
            }
            Msg::HandoffTakeover {
                target,
                key,
                track_id,
                position_ms,
                played_ms,
                started_at,
                scrobbled,
                epoch,
                ..
            } => {
                if !self.has_member(&target) {
                    // Target vanished: nothing to transfer to; the source keeps playing.
                    return;
                }
                match self.lease.transfer(&device_id, epoch, &target, now) {
                    Ok(_) => {
                        let lease = self.lease.lease().clone();
                        if let Some(p) = self.peer_of_device(&target) {
                            self.send(
                                &p,
                                Msg::HandoffTakeover {
                                    from: device_id.clone(),
                                    target: target.clone(),
                                    key,
                                    track_id,
                                    position_ms,
                                    played_ms,
                                    started_at,
                                    scrobbled,
                                    epoch,
                                    lease: Some(lease),
                                },
                            );
                        }
                        for m in &mut self.members {
                            m.ready_key = None;
                            m.picker_open = false;
                        }
                        self.broadcast_lease();
                    }
                    Err(LeaseError::Fenced { current } | LeaseError::Held { current }) => {
                        self.fenced(peer, current)
                    }
                }
            }

            Msg::ScrobbleSubmitted {
                track_id,
                started_at,
                device_id: d,
            } => {
                self.replica.claim_scrobble(&track_id, started_at, &d, now);
                self.dirty = true;
            }
            Msg::ScrobbleDedupeQuery {
                query_id,
                track_id,
                started_at,
                device_id: d,
            } => {
                let claim = self.replica.claim_scrobble(&track_id, started_at, &d, now);
                self.dirty = true;
                self.send(
                    peer,
                    Msg::ScrobbleDedupeAnswer {
                        query_id,
                        duplicate: claim == ScrobbleClaim::Duplicate,
                    },
                );
            }
            Msg::ScrobbleDedupeAnswer { .. } => {}

            Msg::SavedQueuesSync { queues } => {
                let changed = self.replica.merge_saved_queues(&queues, now);
                let merged = self.replica.saved_queues.clone();
                if changed {
                    self.dirty = true;
                    self.broadcast(Msg::SavedQueuesSync { queues: merged }, None);
                } else if merged.len() != queues.len() {
                    self.send(peer, Msg::SavedQueuesSync { queues: merged });
                }
            }
            Msg::SettingsSync { settings } => {
                let changed = self.replica.merge_settings(&settings, now);
                let merged = self.replica.settings.clone();
                if changed {
                    self.dirty = true;
                    self.broadcast(Msg::SettingsSync { settings: merged }, None);
                } else if merged.len() != settings.len() {
                    self.send(peer, Msg::SettingsSync { settings: merged });
                }
            }
            Msg::UndoEntryShared {
                entry,
                before_revision,
                before,
            } => {
                self.broadcast(
                    Msg::UndoEntryShared {
                        entry,
                        before_revision,
                        before,
                    },
                    Some(peer),
                );
            }
        }
    }

    fn on_tick(&mut self) {
        let now = self.now();
        if let Some(LeaseEvent::Changed { previous_owner, .. }) = self.lease.tick(now) {
            tracing::info!(scope = %self.cfg.scope, ?previous_owner, "transport lease lapsed");
            self.broadcast_lease();
        }
        let idle: Vec<PeerId> = self
            .members
            .iter()
            .filter(|m| m.peer != LOOPBACK && now - m.last_seen >= MEMBER_TIMEOUT_MS)
            .map(|m| m.peer.clone())
            .collect();
        for p in idle {
            tracing::info!(scope = %self.cfg.scope, peer = %p, "member timed out");
            self.send(
                &p,
                Msg::Bye {
                    reason: "timed out".into(),
                },
            );
            self.out.push(RoomOutput::Close(p.clone()));
            self.on_disconnected(&p);
        }
        let stale_unknown: Vec<PeerId> = self
            .unknown
            .iter()
            .filter(|(_, at)| now - at >= MEMBER_TIMEOUT_MS)
            .map(|(p, _)| p.clone())
            .collect();
        for p in stale_unknown {
            self.out.push(RoomOutput::Close(p.clone()));
            self.unknown.retain(|(q, _)| q != &p);
        }
        if now - self.last_sweep >= SWEEP_INTERVAL_MS {
            self.last_sweep = now;
            self.replica.expire(now);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::{Platform, PositionStamp};
    use crate::connect::session_adapter::RealReducer;
    use crate::connect::wire::Msg;
    use std::sync::atomic::{AtomicU64, Ordering};

    pub struct TestClock(pub AtomicU64);
    impl Clock for TestClock {
        fn now_ms(&self) -> f64 {
            self.0.load(Ordering::SeqCst) as f64
        }
    }

    fn dev(id: &str) -> DeviceInfo {
        DeviceInfo {
            id: id.into(),
            name: id.into(),
            platform: Platform::Linux,
            app_version: "1".into(),
            playing: false,
            ready: false,
            last_seen: 0.0,
            is_self: true,
        }
    }

    fn hello(id: &str) -> WireMessage {
        WireMessage::new(Msg::Hello {
            device: dev(id),
            protocol_min: 1,
            protocol_max: 1,
            scope: "scope".into(),
            credential: None,
            session_id: None,
            session_revision: 0,
            held_epoch: None,
            extra: Default::default(),
        })
    }

    fn room() -> (Room, Arc<TestClock>) {
        let clock = Arc::new(TestClock(AtomicU64::new(1_000)));
        let mut cfg = RoomConfig::new("scope");
        cfg.session_id = Some("sid".into());
        let r = Room::new(cfg, clock.clone(), RealReducer::shared(), None);
        (r, clock)
    }

    fn join(r: &mut Room, peer: &str, id: &str) -> Vec<RoomOutput> {
        r.handle(RoomInput::Connected(peer.into()));
        r.handle(RoomInput::Message(peer.into(), hello(id)))
    }

    fn sent<'a>(outs: &'a [RoomOutput], peer: &str) -> Vec<&'a Msg> {
        outs.iter()
            .filter_map(|o| match o {
                RoomOutput::Send(p, m) if p == peer => Some(&m.msg),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn hello_is_welcomed_with_empty_replica_and_presence() {
        let (mut r, _) = room();
        let outs = join(&mut r, "p1", "a");
        let msgs = sent(&outs, "p1");
        assert!(
            matches!(msgs[0], Msg::Welcome { replica: Some(r), members, .. } if members.is_empty() && r.document.revision == 0)
        );
        assert!(msgs
            .iter()
            .any(|m| matches!(m, Msg::Presence { devices } if devices.len() == 1)));
        assert!(outs.contains(&RoomOutput::ReplicaChanged));
        let outs = join(&mut r, "p2", "b");
        let msgs = sent(&outs, "p2");
        assert!(matches!(msgs[0], Msg::Welcome { members, .. } if members.len() == 1));
        assert!(sent(&outs, "p1")
            .iter()
            .any(|m| matches!(m, Msg::Presence { devices } if devices.len() == 2)));
    }

    #[test]
    fn protocol_and_scope_refusals_close() {
        let (mut r, _) = room();
        r.handle(RoomInput::Connected("p".into()));
        let mut h = hello("a");
        if let Msg::Hello {
            protocol_max,
            protocol_min,
            ..
        } = &mut h.msg
        {
            *protocol_max = 0;
            *protocol_min = 0;
        }
        let outs = r.handle(RoomInput::Message("p".into(), h));
        assert!(matches!(
            sent(&outs, "p")[0],
            Msg::Refuse {
                reason: RefuseReason::ProtocolTooOld,
                ..
            }
        ));
        assert!(outs.contains(&RoomOutput::Close("p".into())));
        let mut h = hello("a");
        if let Msg::Hello { scope, .. } = &mut h.msg {
            *scope = "other".into();
        }
        let outs = r.handle(RoomInput::Message("p".into(), h));
        assert!(matches!(
            sent(&outs, "p")[0],
            Msg::Refuse {
                reason: RefuseReason::ScopeMismatch,
                ..
            }
        ));
        assert_eq!(r.member_count(), 0);
    }

    #[test]
    fn verification_is_delegated_and_credential_not_kept() {
        let clock = Arc::new(TestClock(AtomicU64::new(0)));
        let scope = scope_key("https://music.example", "bob");
        let r_cfg = RoomConfig::new(scope.clone()).verified();
        let mut r = Room::new(r_cfg, clock, RealReducer::shared(), None);
        r.handle(RoomInput::Connected("p".into()));
        let mut h = hello("a");
        if let Msg::Hello {
            scope: s,
            credential,
            ..
        } = &mut h.msg
        {
            *s = scope.clone();
            *credential = Some(Credential {
                server_url: "https://music.example/".into(),
                username: "bob".into(),
                token: Some("t".into()),
                salt: Some("s".into()),
                api_key: None,
                client: "hocket".into(),
                api_version: "1.16.1".into(),
            });
        }
        let outs = r.handle(RoomInput::Message("p".into(), h.clone()));
        assert!(
            matches!(&outs[0], RoomOutput::Verify { peer, credential } if peer == "p" && credential.username == "bob")
        );
        assert_eq!(r.member_count(), 0);
        let outs = r.handle(RoomInput::Verified {
            peer: "p".into(),
            ok: false,
        });
        assert!(matches!(
            sent(&outs, "p")[0],
            Msg::Refuse {
                reason: RefuseReason::Unauthorised,
                ..
            }
        ));
        // again, accepted this time
        r.handle(RoomInput::Connected("q".into()));
        r.handle(RoomInput::Message("q".into(), h));
        let outs = r.handle(RoomInput::Verified {
            peer: "q".into(),
            ok: true,
        });
        assert!(matches!(sent(&outs, "q")[0], Msg::Welcome { .. }));
        assert_eq!(r.member_count(), 1);
        // no credential anywhere in the replica
        let json = serde_json::to_string(r.replica()).unwrap();
        assert!(!json.contains("\"token\""));
        // missing credential refused
        r.handle(RoomInput::Connected("z".into()));
        let mut h2 = hello("c");
        if let Msg::Hello { scope: s, .. } = &mut h2.msg {
            *s = scope.clone();
        }
        let outs = r.handle(RoomInput::Message("z".into(), h2));
        assert!(matches!(
            sent(&outs, "z")[0],
            Msg::Refuse {
                reason: RefuseReason::Unauthorised,
                ..
            }
        ));
    }

    fn op(base: u32, op: SessionOp, dev: &str, id: &str, epoch: Option<u32>) -> WireMessage {
        WireMessage::new(Msg::Op {
            base_revision: base,
            op,
            device_id: dev.into(),
            op_id: id.into(),
            epoch,
            at: 5.0,
            position_ms: 0,
        })
    }

    fn play_op() -> SessionOp {
        SessionOp::PlayTracks {
            server_id: "srv".into(),
            track_ids: vec!["t1".into(), "t2".into(), "t3".into()],
            start_index: 0,
            label: "sel".into(),
            shuffle: false,
            save_outgoing: false,
        }
    }

    #[test]
    fn ops_are_ordered_acked_and_relayed() {
        let (mut r, _) = room();
        join(&mut r, "p1", "a");
        join(&mut r, "p2", "b");
        let outs = r.handle(RoomInput::Message(
            "p1".into(),
            op(0, play_op(), "a", "o1", None),
        ));
        assert!(matches!(
            sent(&outs, "p1")[0],
            Msg::OpAck { revision: 1, .. }
        ));
        assert!(matches!(
            sent(&outs, "p2")[0],
            Msg::OpCommitted { revision: 1, .. }
        ));
        assert!(outs.contains(&RoomOutput::ReplicaChanged));
        assert_eq!(r.revision(), 1);
        // two people press next against revision 1: one wins
        let outs_a = r.handle(RoomInput::Message(
            "p1".into(),
            op(1, SessionOp::Next, "a", "o2", None),
        ));
        let outs_b = r.handle(RoomInput::Message(
            "p2".into(),
            op(1, SessionOp::Next, "b", "o3", None),
        ));
        assert!(matches!(
            sent(&outs_a, "p1")[0],
            Msg::OpAck { revision: 2, .. }
        ));
        assert!(
            matches!(sent(&outs_b, "p2")[0], Msg::OpReject { reason: RejectReason::Stale, current_revision: 2, document, .. } if document.revision == 2)
        );
        assert_eq!(r.revision(), 2);
        assert_eq!(
            r.replica().document.current.as_ref().unwrap().track_id,
            "t2"
        );
    }

    #[test]
    fn unapplicable_op_is_rejected() {
        let (mut r, _) = room();
        join(&mut r, "p1", "a");
        let outs = r.handle(RoomInput::Message(
            "p1".into(),
            op(
                0,
                SessionOp::JumpToQueueItem { key: "nope".into() },
                "a",
                "o1",
                None,
            ),
        ));
        assert!(matches!(
            sent(&outs, "p1")[0],
            Msg::OpReject {
                reason: RejectReason::Unapplicable,
                ..
            }
        ));
        assert_eq!(r.revision(), 0);
    }

    #[test]
    fn replace_into_fresh_room_adopts_session_identity() {
        let (mut r, _) = room();
        join(&mut r, "p1", "a");
        let mut d = crate::session::new_document("scope", "mine".into(), 0.0);
        d.autoplay = true;
        d.revision = 17;
        let outs = r.handle(RoomInput::Message(
            "p1".into(),
            op(0, SessionOp::Replace { document: d }, "a", "o1", None),
        ));
        assert!(matches!(
            sent(&outs, "p1")[0],
            Msg::OpAck { revision: 1, .. }
        ));
        assert_eq!(r.replica().document.session_id, "mine");
        assert!(r.replica().document.autoplay);
        assert_eq!(r.revision(), 1);
        // a second joiner now gets the replica
        let outs = join(&mut r, "p2", "b");
        assert!(
            matches!(sent(&outs, "p2")[0], Msg::Welcome { replica: Some(rep), .. } if rep.document.session_id == "mine")
        );
    }

    #[test]
    fn lease_claim_heartbeat_lapse_and_fencing() {
        let (mut r, clock) = room();
        join(&mut r, "p1", "a");
        join(&mut r, "p2", "b");
        let outs = r.handle(RoomInput::Message(
            "p1".into(),
            WireMessage::new(Msg::LeaseClaim {
                epoch_expected: None,
                takeover: false,
                sent_at: 0.0,
            }),
        ));
        assert!(sent(&outs, "p1").iter().any(|m| matches!(m, Msg::LeaseGranted { lease, .. } if lease.owner.as_deref() == Some("a") && lease.epoch == 1)));
        assert!(sent(&outs, "p2")
            .iter()
            .any(|m| matches!(m, Msg::LeaseGranted { .. })));
        assert!(sent(&outs, "p2").iter().any(|m| matches!(m, Msg::Presence { devices } if devices.iter().any(|d| d.id == "a" && d.playing))));
        // b cannot claim without takeover
        let outs = r.handle(RoomInput::Message(
            "p2".into(),
            WireMessage::new(Msg::LeaseClaim {
                epoch_expected: None,
                takeover: false,
                sent_at: 0.0,
            }),
        ));
        assert!(matches!(
            sent(&outs, "p2")[0],
            Msg::LeaseFenced {
                current_epoch: 1,
                ..
            }
        ));
        // heartbeat renews
        clock.0.store(6_000, Ordering::SeqCst);
        let outs = r.handle(RoomInput::Message(
            "p1".into(),
            WireMessage::new(Msg::LeaseHeartbeat {
                epoch: 1,
                sent_at: 6000.0,
            }),
        ));
        assert!(
            matches!(sent(&outs, "p1")[0], Msg::LeaseGranted { lease, ack_of: Some(t) } if lease.expires_at == 26_000.0 && *t == 6000.0)
        );
        // silence: lapse at 26 s
        clock.0.store(26_000, Ordering::SeqCst);
        let outs = r.handle(RoomInput::Tick);
        assert!(sent(&outs, "p2").iter().any(|m| matches!(m, Msg::LeaseGranted { lease, .. } if lease.owner.is_none() && lease.epoch == 2)));
        // a's stamp with the old epoch is fenced and not relayed
        let outs = r.handle(RoomInput::Message(
            "p1".into(),
            WireMessage::new(Msg::TransportStamp {
                device_id: "a".into(),
                key: None,
                position: PositionStamp::default(),
                played_ms: 0,
                started_at: 0.0,
                scrobbled: false,
                epoch: 1,
            }),
        ));
        assert!(matches!(
            sent(&outs, "p1")[0],
            Msg::LeaseFenced {
                current_epoch: 2,
                ..
            }
        ));
        assert!(sent(&outs, "p2").is_empty());
        assert!(r.replica().last_stamp.is_none());
        // owner-issued op with a stale epoch is fenced too
        let outs = r.handle(RoomInput::Message(
            "p1".into(),
            op(0, SessionOp::TrackEnded, "a", "o9", Some(1)),
        ));
        assert!(matches!(
            sent(&outs, "p1")[0],
            Msg::OpReject {
                reason: RejectReason::Fenced,
                ..
            }
        ));
    }

    #[test]
    fn stamps_relay_and_persist_only_on_play_pause_or_track_change() {
        let (mut r, _) = room();
        join(&mut r, "p1", "a");
        join(&mut r, "p2", "b");
        r.handle(RoomInput::Message(
            "p1".into(),
            WireMessage::new(Msg::LeaseClaim {
                epoch_expected: None,
                takeover: false,
                sent_at: 0.0,
            }),
        ));
        let stamp = |pos: u32, playing: bool| {
            WireMessage::new(Msg::TransportStamp {
                device_id: "a".into(),
                key: Some("k".into()),
                position: PositionStamp {
                    position_ms: pos,
                    taken_at: 1.0,
                    rate: 1.0,
                    is_playing: playing,
                },
                played_ms: 0,
                started_at: 0.0,
                scrobbled: false,
                epoch: 1,
            })
        };
        let outs = r.handle(RoomInput::Message("p1".into(), stamp(0, true)));
        assert!(outs.contains(&RoomOutput::ReplicaChanged));
        assert!(matches!(sent(&outs, "p2")[0], Msg::TransportStamp { .. }));
        let outs = r.handle(RoomInput::Message("p1".into(), stamp(5000, true))); // seek
        assert!(!outs.contains(&RoomOutput::ReplicaChanged));
        assert!(matches!(sent(&outs, "p2")[0], Msg::TransportStamp { .. }));
        let outs = r.handle(RoomInput::Message("p1".into(), stamp(5000, false))); // pause
        assert!(outs.contains(&RoomOutput::ReplicaChanged));
        assert_eq!(
            r.replica()
                .last_stamp
                .as_ref()
                .unwrap()
                .position
                .position_ms,
            5000
        );
    }

    #[test]
    fn handoff_transfers_lease_atomically() {
        let (mut r, _) = room();
        join(&mut r, "p1", "a");
        join(&mut r, "p2", "b");
        join(&mut r, "p3", "c");
        r.handle(RoomInput::Message(
            "p1".into(),
            WireMessage::new(Msg::LeaseClaim {
                epoch_expected: None,
                takeover: false,
                sent_at: 0.0,
            }),
        ));
        let outs = r.handle(RoomInput::Message(
            "p1".into(),
            WireMessage::new(Msg::HandoffPickerOpen { from: "a".into() }),
        ));
        assert!(matches!(
            sent(&outs, "p2")[0],
            Msg::HandoffPickerOpen { .. }
        ));
        assert!(matches!(
            sent(&outs, "p3")[0],
            Msg::HandoffPickerOpen { .. }
        ));
        let outs = r.handle(RoomInput::Message(
            "p1".into(),
            WireMessage::new(Msg::HandoffPrepare {
                from: "a".into(),
                target: "b".into(),
                key: "k".into(),
                track_id: "t".into(),
                position_ms: 10,
            }),
        ));
        assert!(matches!(
            sent(&outs, "p2")[0],
            Msg::HandoffPrepare {
                position_ms: 10,
                ..
            }
        ));
        assert!(sent(&outs, "p3").is_empty());
        let outs = r.handle(RoomInput::Message(
            "p2".into(),
            WireMessage::new(Msg::HandoffReady {
                from: "a".into(),
                target: "b".into(),
                key: "k".into(),
                ready: true,
            }),
        ));
        assert!(matches!(
            sent(&outs, "p1")[0],
            Msg::HandoffReady { ready: true, .. }
        ));
        assert!(sent(&outs, "p3").iter().any(|m| matches!(m, Msg::Presence { devices } if devices.iter().any(|d| d.id == "b" && d.ready))));
        let outs = r.handle(RoomInput::Message(
            "p1".into(),
            WireMessage::new(Msg::HandoffTakeover {
                from: "a".into(),
                target: "b".into(),
                key: "k".into(),
                track_id: "t".into(),
                position_ms: 90_000,
                played_ms: 90_000,
                started_at: 5.0,
                scrobbled: false,
                epoch: 1,
                lease: None,
            }),
        ));
        assert!(
            matches!(sent(&outs, "p2")[0], Msg::HandoffTakeover { lease: Some(l), played_ms: 90_000, .. } if l.owner.as_deref() == Some("b") && l.epoch == 2)
        );
        assert!(sent(&outs, "p1").iter().any(
            |m| matches!(m, Msg::LeaseGranted { lease, .. } if lease.owner.as_deref() == Some("b"))
        ));
        assert_eq!(r.live_owner().as_deref(), Some("b"));
        // the source's late release is silently ignored
        let outs = r.handle(RoomInput::Message(
            "p1".into(),
            WireMessage::new(Msg::HandoffRelease { epoch: 1 }),
        ));
        assert!(sent(&outs, "p1").is_empty());
        // and a stale-epoch takeover from a is fenced
        let outs = r.handle(RoomInput::Message(
            "p1".into(),
            WireMessage::new(Msg::HandoffTakeover {
                from: "a".into(),
                target: "c".into(),
                key: "k".into(),
                track_id: "t".into(),
                position_ms: 0,
                played_ms: 0,
                started_at: 0.0,
                scrobbled: false,
                epoch: 1,
                lease: None,
            }),
        ));
        assert!(matches!(
            sent(&outs, "p1")[0],
            Msg::LeaseFenced {
                current_epoch: 2,
                ..
            }
        ));
    }

    #[test]
    fn scrobble_dedupe_across_devices() {
        let (mut r, _) = room();
        join(&mut r, "p1", "a");
        join(&mut r, "p2", "b");
        let q = |peer: &str, dev: &str, id: &str| {
            RoomInput::Message(
                peer.into(),
                WireMessage::new(Msg::ScrobbleDedupeQuery {
                    query_id: id.into(),
                    track_id: "t".into(),
                    started_at: 100.0,
                    device_id: dev.into(),
                }),
            )
        };
        let outs = r.handle(q("p1", "a", "q1"));
        assert!(matches!(
            sent(&outs, "p1")[0],
            Msg::ScrobbleDedupeAnswer {
                duplicate: false,
                ..
            }
        ));
        let outs = r.handle(q("p2", "b", "q2"));
        assert!(matches!(
            sent(&outs, "p2")[0],
            Msg::ScrobbleDedupeAnswer {
                duplicate: true,
                ..
            }
        ));
        let outs = r.handle(q("p1", "a", "q3"));
        assert!(matches!(
            sent(&outs, "p1")[0],
            Msg::ScrobbleDedupeAnswer {
                duplicate: false,
                ..
            }
        ));
    }

    #[test]
    fn member_timeout_and_duplicate_device() {
        let (mut r, clock) = room();
        join(&mut r, "p1", "a");
        clock.0.store(1_000 + 30_000, Ordering::SeqCst);
        let outs = r.handle(RoomInput::Tick);
        assert!(outs.contains(&RoomOutput::Close("p1".into())));
        assert_eq!(r.member_count(), 0);
        join(&mut r, "p2", "a");
        let outs = join(&mut r, "p3", "a");
        assert!(outs.contains(&RoomOutput::Close("p2".into())));
        assert_eq!(r.member_count(), 1);
        assert_eq!(r.member_device_ids(), vec!["a".to_string()]);
    }

    #[test]
    fn clock_pong_uses_room_clock() {
        let (mut r, clock) = room();
        join(&mut r, "p1", "a");
        clock.0.store(4242, Ordering::SeqCst);
        let outs = r.handle(RoomInput::Message(
            "p1".into(),
            WireMessage::new(Msg::ClockPing { t0: 7.0 }),
        ));
        assert!(
            matches!(sent(&outs, "p1")[0], Msg::ClockPong { t0, t1, t2 } if *t0 == 7.0 && *t1 == 4242.0 && *t2 == 4242.0)
        );
    }

    #[test]
    fn saved_queues_and_settings_sync_merge_and_fan_out() {
        use crate::api::{Setting, SettingScope};
        let (mut r, _) = room();
        join(&mut r, "p1", "a");
        join(&mut r, "p2", "b");
        let s = Setting {
            key: "k".into(),
            value: "v".into(),
            scope: SettingScope::AccountSynced,
            updated_at: 5.0,
        };
        let outs = r.handle(RoomInput::Message(
            "p1".into(),
            WireMessage::new(Msg::SettingsSync {
                settings: vec![s.clone()],
            }),
        ));
        assert!(
            matches!(sent(&outs, "p2")[0], Msg::SettingsSync { settings } if settings.len() == 1)
        );
        assert!(outs.contains(&RoomOutput::ReplicaChanged));
        // unchanged resend: nothing broadcast
        let outs = r.handle(RoomInput::Message(
            "p1".into(),
            WireMessage::new(Msg::SettingsSync { settings: vec![s] }),
        ));
        assert!(sent(&outs, "p2").is_empty());
        // a member with fewer entries gets the full set back
        let outs = r.handle(RoomInput::Message(
            "p2".into(),
            WireMessage::new(Msg::SettingsSync { settings: vec![] }),
        ));
        assert!(
            matches!(sent(&outs, "p2")[0], Msg::SettingsSync { settings } if settings.len() == 1)
        );
    }

    #[test]
    fn unknown_messages_and_pre_hello_traffic_are_ignored() {
        let (mut r, _) = room();
        r.handle(RoomInput::Connected("p".into()));
        let outs = r.handle(RoomInput::Message(
            "p".into(),
            WireMessage::new(Msg::SyncRequest),
        ));
        assert!(outs.is_empty());
        join(&mut r, "p", "a");
        let outs = r.handle(RoomInput::Message(
            "p".into(),
            WireMessage::new(Msg::Unknown),
        ));
        assert!(outs.is_empty());
    }
}
