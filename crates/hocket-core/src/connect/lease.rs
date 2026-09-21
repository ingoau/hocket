//! Transport lease with a fencing epoch.
//!
//! Exactly one device plays at a time. Ownership is a lease renewed by
//! heartbeat every [`HEARTBEAT_INTERVAL_MS`] and lapsing after
//! [`LEASE_DURATION_MS`] without one, carrying a monotonic `epoch` that
//! increments on *every* ownership change (grant to a different device,
//! release, lapse, takeover). Anything that arrives with a stale epoch is
//! fenced: a device that lost the network and kept playing cannot write its
//! stamps over the new owner's when it comes back.
//!
//! - A device reconnecting *inside* the window keeps ownership and its epoch.
//! - A deliberate takeover from the picker always works immediately; the
//!   lease only covers failure.
//!
//! This is a pure state machine: time is passed in by the caller (the room),
//! which reads it from its injected clock.

use crate::api::{DeviceId, EpochMs, TransportLease};

/// Owners renew every 5 s.
pub const HEARTBEAT_INTERVAL_MS: f64 = 5_000.0;
/// A lease lapses 20 s after its last renewal.
pub const LEASE_DURATION_MS: f64 = 20_000.0;

/// Why a lease operation was refused.
#[derive(Debug, Clone, PartialEq)]
pub enum LeaseError {
    /// The caller's epoch is not the current one. Carries the live lease.
    Fenced { current: TransportLease },
    /// Someone else holds a live lease and the caller did not ask to take over.
    Held { current: TransportLease },
}

/// What changed, for the room to broadcast and persist.
#[derive(Debug, Clone, PartialEq)]
pub enum LeaseEvent {
    /// Ownership changed (new owner, release or lapse). Epoch bumped.
    Changed { previous_owner: Option<DeviceId>, lease: TransportLease },
    /// Same owner, same epoch, later expiry.
    Renewed { lease: TransportLease },
}

/// The lease as the room sees it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct LeaseMachine {
    lease: TransportLease,
}

impl LeaseMachine {
    pub fn new() -> Self {
        Self::default()
    }

    /// Resume from a persisted replica. A persisted lease is treated as
    /// already lapsed if its expiry has passed, otherwise it stays valid so
    /// a coordinator restart inside the window doesn't steal ownership.
    pub fn from_lease(lease: TransportLease) -> Self {
        LeaseMachine { lease }
    }

    pub fn lease(&self) -> &TransportLease {
        &self.lease
    }

    pub fn epoch(&self) -> u32 {
        self.lease.epoch
    }

    /// Owner with a live (unexpired) lease at `now`.
    pub fn live_owner(&self, now: EpochMs) -> Option<&DeviceId> {
        if self.lease.expires_at > now {
            self.lease.owner.as_ref()
        } else {
            None
        }
    }

    pub fn is_live_owner(&self, device: &str, now: EpochMs) -> bool {
        self.live_owner(now).map(|o| o == device).unwrap_or(false)
    }

    fn bump(&mut self, owner: Option<DeviceId>, now: EpochMs) -> LeaseEvent {
        let previous_owner = self.lease.owner.take();
        self.lease.epoch = self.lease.epoch.wrapping_add(1);
        self.lease.owner = owner;
        self.lease.expires_at = if self.lease.owner.is_some() { now + LEASE_DURATION_MS } else { now };
        LeaseEvent::Changed { previous_owner, lease: self.lease.clone() }
    }

    /// Ask for the lease.
    ///
    /// - Nobody holds it (or it lapsed): granted with a new epoch.
    /// - The caller already holds it (reconnect inside the window): renewed,
    ///   same epoch, provided `epoch_expected` is unset or matches.
    /// - Someone else holds it: refused unless `takeover`, which grants with a
    ///   new epoch immediately.
    pub fn claim(
        &mut self,
        device: &str,
        epoch_expected: Option<u32>,
        takeover: bool,
        now: EpochMs,
    ) -> Result<LeaseEvent, LeaseError> {
        match self.live_owner(now).cloned() {
            Some(owner) if owner == device => {
                if let Some(e) = epoch_expected {
                    if e != self.lease.epoch {
                        // It thinks it holds an older epoch: refresh it honestly.
                        if takeover {
                            return Ok(self.bump(Some(device.to_string()), now));
                        }
                        return Err(LeaseError::Fenced { current: self.lease.clone() });
                    }
                }
                self.lease.expires_at = now + LEASE_DURATION_MS;
                Ok(LeaseEvent::Renewed { lease: self.lease.clone() })
            }
            Some(_other) => {
                if takeover {
                    Ok(self.bump(Some(device.to_string()), now))
                } else {
                    Err(LeaseError::Held { current: self.lease.clone() })
                }
            }
            None => {
                // Free (never held, released, or lapsed). A device that believed
                // it held an older epoch is told so unless it takes over; this
                // is how a returning offline player learns it was fenced.
                if let Some(e) = epoch_expected {
                    if e != self.lease.epoch && !takeover {
                        return Err(LeaseError::Fenced { current: self.lease.clone() });
                    }
                    // Same epoch and still nominally ours but expired? live_owner
                    // said None, so it lapsed and the epoch moved; unreachable
                    // unless owner == None with the same epoch (fresh machine).
                }
                Ok(self.bump(Some(device.to_string()), now))
            }
        }
    }

    /// Renew. Fenced when the caller isn't the live owner at this epoch.
    pub fn heartbeat(&mut self, device: &str, epoch: u32, now: EpochMs) -> Result<LeaseEvent, LeaseError> {
        if self.is_live_owner(device, now) && epoch == self.lease.epoch {
            self.lease.expires_at = now + LEASE_DURATION_MS;
            Ok(LeaseEvent::Renewed { lease: self.lease.clone() })
        } else {
            Err(LeaseError::Fenced { current: self.lease.clone() })
        }
    }

    /// Check a stamp or an owner-issued op. Does not renew.
    pub fn check(&self, device: &str, epoch: u32, now: EpochMs) -> Result<(), LeaseError> {
        if self.is_live_owner(device, now) && epoch == self.lease.epoch {
            Ok(())
        } else {
            Err(LeaseError::Fenced { current: self.lease.clone() })
        }
    }

    /// Give the lease up. Fenced when the caller doesn't hold it at this epoch.
    pub fn release(&mut self, device: &str, epoch: u32, now: EpochMs) -> Result<LeaseEvent, LeaseError> {
        self.check(device, epoch, now)?;
        Ok(self.bump(None, now))
    }

    /// Transfer to `target` as part of a handoff. The source must hold the
    /// lease at `epoch`; the target gets a fresh epoch.
    pub fn transfer(
        &mut self,
        from: &str,
        epoch: u32,
        target: &str,
        now: EpochMs,
    ) -> Result<LeaseEvent, LeaseError> {
        self.check(from, epoch, now)?;
        Ok(self.bump(Some(target.to_string()), now))
    }

    /// Advance time. Lapses the lease when its expiry has passed; the epoch
    /// bumps so the silent owner's later writes are fenced.
    pub fn tick(&mut self, now: EpochMs) -> Option<LeaseEvent> {
        if self.lease.owner.is_some() && self.lease.expires_at <= now {
            Some(self.bump(None, now))
        } else {
            None
        }
    }

    /// Time until the lease lapses, if held.
    pub fn remaining_ms(&self, now: EpochMs) -> Option<f64> {
        self.lease.owner.as_ref().map(|_| (self.lease.expires_at - now).max(0.0))
    }
}

/// Owner-side safety margin: a device stops trusting its lease this much
/// before the room would lapse it, covering tick granularity and clock skew,
/// so the two views can never both say "owner" at the same instant.
pub const OWNER_LAPSE_MARGIN_MS: f64 = 2_000.0;

/// The owner's own view: when did the room last confirm our lease, and are we
/// still inside the window by our own clock. A device that hasn't heard an
/// ack for [`LEASE_DURATION_MS`] (minus the margin) must assume it has been
/// fenced, keep playing as a detached participant, and reclaim (or be told)
/// on reconnect.
///
/// `last_ack_at` is the *send* time of the heartbeat (or claim) the room
/// answered (the room echoes it as `ackOf`), not the arrival time of the
/// answer: the room's window starts when it received the heartbeat, which is
/// after we sent it, so measuring from the send keeps our window strictly
/// inside the room's.
#[derive(Debug, Clone, PartialEq)]
pub struct HeldLease {
    pub epoch: u32,
    pub last_ack_at: EpochMs,
    pub last_heartbeat_at: EpochMs,
}

impl HeldLease {
    pub fn new(epoch: u32, now: EpochMs) -> Self {
        HeldLease { epoch, last_ack_at: now, last_heartbeat_at: now }
    }

    /// Whether a heartbeat is due.
    pub fn heartbeat_due(&self, now: EpochMs) -> bool {
        now - self.last_heartbeat_at >= HEARTBEAT_INTERVAL_MS
    }

    /// Record a heartbeat or claim being sent.
    pub fn sent(&mut self, now: EpochMs) {
        self.last_heartbeat_at = now;
    }

    /// The room answered the heartbeat/claim we sent at `sent_at`.
    pub fn acked(&mut self, sent_at: EpochMs) {
        if sent_at > self.last_ack_at {
            self.last_ack_at = sent_at;
        }
    }

    /// Whether the window has closed without an ack.
    pub fn lapsed(&self, now: EpochMs) -> bool {
        now - self.last_ack_at >= LEASE_DURATION_MS - OWNER_LAPSE_MARGIN_MS
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn owner(m: &LeaseMachine, now: f64) -> Option<String> {
        m.live_owner(now).cloned()
    }

    #[test]
    fn fresh_claim_grants_with_new_epoch() {
        let mut m = LeaseMachine::new();
        let ev = m.claim("a", None, false, 0.0).unwrap();
        match ev {
            LeaseEvent::Changed { previous_owner, lease } => {
                assert_eq!(previous_owner, None);
                assert_eq!(lease.owner.as_deref(), Some("a"));
                assert_eq!(lease.epoch, 1);
                assert_eq!(lease.expires_at, LEASE_DURATION_MS);
            }
            other => panic!("{other:?}"),
        }
        assert_eq!(owner(&m, 100.0).as_deref(), Some("a"));
    }

    #[test]
    fn heartbeat_renews_only_for_owner_at_epoch() {
        let mut m = LeaseMachine::new();
        m.claim("a", None, false, 0.0).unwrap();
        assert!(matches!(m.heartbeat("a", 1, 5000.0), Ok(LeaseEvent::Renewed { .. })));
        assert_eq!(m.lease().expires_at, 25_000.0);
        assert!(matches!(m.heartbeat("b", 1, 6000.0), Err(LeaseError::Fenced { .. })));
        assert!(matches!(m.heartbeat("a", 0, 6000.0), Err(LeaseError::Fenced { .. })));
        assert!(matches!(m.heartbeat("a", 2, 6000.0), Err(LeaseError::Fenced { .. })));
    }

    #[test]
    fn lapses_after_twenty_seconds_and_bumps_epoch() {
        let mut m = LeaseMachine::new();
        m.claim("a", None, false, 0.0).unwrap();
        assert!(m.tick(19_999.0).is_none());
        let ev = m.tick(20_000.0).unwrap();
        assert!(matches!(ev, LeaseEvent::Changed { previous_owner: Some(ref p), .. } if p == "a"));
        assert_eq!(m.epoch(), 2);
        assert_eq!(owner(&m, 20_000.0), None);
        assert!(m.tick(30_000.0).is_none());
        // the lapsed owner's stamps are fenced
        assert!(matches!(m.check("a", 1, 21_000.0), Err(LeaseError::Fenced { current }) if current.epoch == 2));
    }

    #[test]
    fn heartbeats_keep_it_alive_indefinitely() {
        let mut m = LeaseMachine::new();
        m.claim("a", None, false, 0.0).unwrap();
        let mut now = 0.0;
        for _ in 0..100 {
            now += HEARTBEAT_INTERVAL_MS;
            assert!(m.tick(now).is_none());
            m.heartbeat("a", 1, now).unwrap();
        }
        assert_eq!(m.epoch(), 1);
        assert_eq!(owner(&m, now).as_deref(), Some("a"));
    }

    #[test]
    fn reconnect_inside_window_keeps_ownership_and_epoch() {
        let mut m = LeaseMachine::new();
        m.claim("a", None, false, 0.0).unwrap();
        m.tick(15_000.0);
        let ev = m.claim("a", Some(1), false, 15_000.0).unwrap();
        assert!(matches!(ev, LeaseEvent::Renewed { ref lease } if lease.epoch == 1));
        assert_eq!(m.lease().expires_at, 35_000.0);
    }

    #[test]
    fn reconnect_after_lapse_is_fenced_unless_takeover() {
        let mut m = LeaseMachine::new();
        m.claim("a", None, false, 0.0).unwrap();
        m.tick(25_000.0);
        assert_eq!(m.epoch(), 2);
        match m.claim("a", Some(1), false, 25_000.0) {
            Err(LeaseError::Fenced { current }) => assert_eq!(current.epoch, 2),
            other => panic!("{other:?}"),
        }
        // deliberate claim from a fresh state (no expectation) works
        let ev = m.claim("a", None, false, 25_000.0).unwrap();
        assert!(matches!(ev, LeaseEvent::Changed { ref lease, .. } if lease.epoch == 3));
    }

    #[test]
    fn someone_else_holding_it_refuses_without_takeover() {
        let mut m = LeaseMachine::new();
        m.claim("a", None, false, 0.0).unwrap();
        assert!(matches!(m.claim("b", None, false, 1000.0), Err(LeaseError::Held { .. })));
        let ev = m.claim("b", None, true, 1000.0).unwrap();
        assert!(matches!(ev, LeaseEvent::Changed { previous_owner: Some(ref p), ref lease } if p == "a" && lease.epoch == 2));
        assert_eq!(owner(&m, 1000.0).as_deref(), Some("b"));
        // the old owner is fenced from now on
        assert!(matches!(m.heartbeat("a", 1, 2000.0), Err(LeaseError::Fenced { .. })));
        assert!(matches!(m.check("a", 1, 2000.0), Err(LeaseError::Fenced { .. })));
    }

    #[test]
    fn takeover_after_lapse_works_immediately() {
        let mut m = LeaseMachine::new();
        m.claim("a", None, false, 0.0).unwrap();
        m.tick(50_000.0);
        let ev = m.claim("b", None, true, 50_000.0).unwrap();
        assert!(matches!(ev, LeaseEvent::Changed { previous_owner: None, .. }));
        assert_eq!(m.epoch(), 3);
    }

    #[test]
    fn release_bumps_epoch_and_frees() {
        let mut m = LeaseMachine::new();
        m.claim("a", None, false, 0.0).unwrap();
        assert!(matches!(m.release("b", 1, 100.0), Err(LeaseError::Fenced { .. })));
        assert!(matches!(m.release("a", 0, 100.0), Err(LeaseError::Fenced { .. })));
        let ev = m.release("a", 1, 100.0).unwrap();
        assert!(matches!(ev, LeaseEvent::Changed { ref lease, .. } if lease.owner.is_none() && lease.epoch == 2));
        assert_eq!(m.remaining_ms(100.0), None);
        assert!(m.tick(1_000_000.0).is_none());
    }

    #[test]
    fn transfer_moves_ownership_with_fresh_epoch() {
        let mut m = LeaseMachine::new();
        m.claim("a", None, false, 0.0).unwrap();
        assert!(matches!(m.transfer("b", 1, "c", 10.0), Err(LeaseError::Fenced { .. })));
        let ev = m.transfer("a", 1, "b", 10.0).unwrap();
        assert!(matches!(ev, LeaseEvent::Changed { previous_owner: Some(ref p), ref lease } if p == "a" && lease.owner.as_deref() == Some("b") && lease.epoch == 2));
        assert!(m.check("a", 1, 11.0).is_err());
        assert!(m.check("b", 2, 11.0).is_ok());
    }

    #[test]
    fn epoch_is_monotonic_across_every_transition() {
        let mut m = LeaseMachine::new();
        let mut last = m.epoch();
        let mut now = 0.0;
        let steps: Vec<Box<dyn Fn(&mut LeaseMachine, f64)>> = vec![
            Box::new(|m, t| {
                let _ = m.claim("a", None, false, t);
            }),
            Box::new(|m, t| {
                let _ = m.claim("b", None, true, t);
            }),
            Box::new(|m, t| {
                let e = m.epoch();
                let _ = m.release("b", e, t);
            }),
            Box::new(|m, t| {
                let _ = m.claim("c", None, false, t);
            }),
            Box::new(|m, t| {
                let _ = m.tick(t + 100_000.0);
            }),
            Box::new(|m, t| {
                let _ = m.claim("a", Some(1), false, t);
            }),
            Box::new(|m, t| {
                let _ = m.claim("a", None, false, t);
            }),
            Box::new(|m, t| {
                let e = m.epoch();
                let _ = m.transfer("a", e, "b", t);
            }),
        ];
        for step in steps {
            now += 1000.0;
            step(&mut m, now);
            assert!(m.epoch() >= last);
            last = m.epoch();
        }
        assert!(last >= 6);
    }

    #[test]
    fn persisted_lease_is_respected_inside_window() {
        let l = TransportLease { owner: Some("a".into()), epoch: 9, expires_at: 20_000.0 };
        let mut m = LeaseMachine::from_lease(l);
        assert_eq!(owner(&m, 10_000.0).as_deref(), Some("a"));
        assert!(matches!(m.claim("b", None, false, 10_000.0), Err(LeaseError::Held { .. })));
        assert_eq!(owner(&m, 20_000.0), None);
        assert!(m.tick(20_000.0).is_some());
        assert_eq!(m.epoch(), 10);
    }

    #[test]
    fn held_lease_timing_measures_from_send() {
        let mut h = HeldLease::new(3, 0.0);
        assert!(!h.heartbeat_due(4999.0));
        assert!(h.heartbeat_due(5000.0));
        assert!(!h.lapsed(17_999.0));
        assert!(h.lapsed(18_000.0));
        h.sent(5000.0);
        h.sent(10_000.0);
        h.acked(5000.0); // late answer to the first
        assert_eq!(h.last_ack_at, 5000.0);
        h.acked(10_000.0);
        assert_eq!(h.last_ack_at, 10_000.0);
        h.acked(7000.0); // out of order: never goes backwards
        assert_eq!(h.last_ack_at, 10_000.0);
        assert!(!h.lapsed(27_999.0));
        assert!(h.lapsed(28_000.0));
    }
}
