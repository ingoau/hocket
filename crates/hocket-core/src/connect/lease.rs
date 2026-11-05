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

/// Owners renew every 5 s (at the normal cadence, see [`cadence_slack_ms`]).
pub const HEARTBEAT_INTERVAL_MS: f64 = 5_000.0;
/// A lease lapses 20 s after its last renewal (at the normal cadence).
pub const LEASE_DURATION_MS: f64 = 20_000.0;

/// The slowest keepalive cadence a room accepts from a member
/// ([`Msg::Cadence`](crate::connect::wire::Msg::Cadence)). Carrier NATs drop
/// idle TCP after a few minutes, so a member never goes quieter than this.
pub const MAX_CADENCE_MS: f64 = 60_000.0;

/// A member's announced keepalive cadence (its clock ping and, while it owns
/// transport, its lease heartbeat interval) clamped to what a room accepts:
/// never faster than the normal [`HEARTBEAT_INTERVAL_MS`], never slower than
/// [`MAX_CADENCE_MS`].
pub fn clamp_cadence(cadence_ms: f64) -> f64 {
    if cadence_ms.is_nan() {
        return HEARTBEAT_INTERVAL_MS;
    }
    cadence_ms.clamp(HEARTBEAT_INTERVAL_MS, MAX_CADENCE_MS)
}

/// How much every silence limit tied to a member's cadence grows when the
/// member talks less often than the normal 5 s: two of its extra periods.
/// Lease window, room member timeout, the engine's upstream-idle limit and
/// the LAN leader's member-quiet limit all add this to their 5 s-cadence
/// value, so a slower member is still allowed the same number of missed
/// beats (plus slack for a radio waking up) before anything lapses.
pub fn cadence_slack_ms(cadence_ms: f64) -> f64 {
    2.0 * (clamp_cadence(cadence_ms) - HEARTBEAT_INTERVAL_MS)
}

/// The lease window a room grants an owner renewing at `cadence_ms`.
pub fn lease_window_ms(cadence_ms: f64) -> f64 {
    LEASE_DURATION_MS + cadence_slack_ms(cadence_ms)
}

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
    Changed {
        previous_owner: Option<DeviceId>,
        lease: TransportLease,
    },
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

    fn bump(&mut self, owner: Option<DeviceId>, now: EpochMs, window_ms: f64) -> LeaseEvent {
        let previous_owner = self.lease.owner.take();
        self.lease.epoch = self.lease.epoch.wrapping_add(1);
        self.lease.owner = owner;
        self.lease.expires_at = if self.lease.owner.is_some() {
            now + window_ms
        } else {
            now
        };
        LeaseEvent::Changed {
            previous_owner,
            lease: self.lease.clone(),
        }
    }

    /// Ask for the lease.
    ///
    /// - Nobody holds it (or it lapsed): granted with a new epoch.
    /// - The caller already holds it (reconnect inside the window): renewed,
    ///   same epoch, provided `epoch_expected` is unset or matches.
    /// - Someone else holds it: refused unless `takeover`, which grants with a
    ///   new epoch immediately.
    ///
    /// A grant or renewal lasts `window_ms` (see [`lease_window_ms`]).
    pub fn claim(
        &mut self,
        device: &str,
        epoch_expected: Option<u32>,
        takeover: bool,
        now: EpochMs,
        window_ms: f64,
    ) -> Result<LeaseEvent, LeaseError> {
        match self.live_owner(now).cloned() {
            Some(owner) if owner == device => {
                if let Some(e) = epoch_expected {
                    if e != self.lease.epoch {
                        // It thinks it holds an older epoch: refresh it honestly.
                        if takeover {
                            return Ok(self.bump(Some(device.to_string()), now, window_ms));
                        }
                        return Err(LeaseError::Fenced {
                            current: self.lease.clone(),
                        });
                    }
                }
                self.lease.expires_at = now + window_ms;
                Ok(LeaseEvent::Renewed {
                    lease: self.lease.clone(),
                })
            }
            Some(_other) => {
                if takeover {
                    Ok(self.bump(Some(device.to_string()), now, window_ms))
                } else {
                    Err(LeaseError::Held {
                        current: self.lease.clone(),
                    })
                }
            }
            None => {
                // Free (never held, released, or lapsed). A device that believed
                // it held an older epoch is told so unless it takes over; this
                // is how a returning offline player learns it was fenced. (One
                // still playing then claims it afresh, epoch unset.)
                if let Some(e) = epoch_expected {
                    if e != self.lease.epoch && !takeover {
                        return Err(LeaseError::Fenced {
                            current: self.lease.clone(),
                        });
                    }
                    // Same epoch and still nominally ours but expired? live_owner
                    // said None, so it lapsed and the epoch moved; unreachable
                    // unless owner == None with the same epoch (fresh machine).
                }
                Ok(self.bump(Some(device.to_string()), now, window_ms))
            }
        }
    }

    /// Renew for `window_ms`. Fenced when the caller isn't the live owner at
    /// this epoch.
    pub fn heartbeat(
        &mut self,
        device: &str,
        epoch: u32,
        now: EpochMs,
        window_ms: f64,
    ) -> Result<LeaseEvent, LeaseError> {
        if self.is_live_owner(device, now) && epoch == self.lease.epoch {
            self.lease.expires_at = now + window_ms;
            Ok(LeaseEvent::Renewed {
                lease: self.lease.clone(),
            })
        } else {
            Err(LeaseError::Fenced {
                current: self.lease.clone(),
            })
        }
    }

    /// Check a stamp or an owner-issued op. Does not renew.
    pub fn check(&self, device: &str, epoch: u32, now: EpochMs) -> Result<(), LeaseError> {
        if self.is_live_owner(device, now) && epoch == self.lease.epoch {
            Ok(())
        } else {
            Err(LeaseError::Fenced {
                current: self.lease.clone(),
            })
        }
    }

    /// Give the lease up. Fenced when the caller doesn't hold it at this epoch.
    pub fn release(
        &mut self,
        device: &str,
        epoch: u32,
        now: EpochMs,
    ) -> Result<LeaseEvent, LeaseError> {
        self.check(device, epoch, now)?;
        Ok(self.bump(None, now, 0.0))
    }

    /// Transfer to `target` as part of a handoff. The source must hold the
    /// lease at `epoch`; the target gets a fresh epoch lasting `window_ms`
    /// (the target's window).
    pub fn transfer(
        &mut self,
        from: &str,
        epoch: u32,
        target: &str,
        now: EpochMs,
        window_ms: f64,
    ) -> Result<LeaseEvent, LeaseError> {
        self.check(from, epoch, now)?;
        Ok(self.bump(Some(target.to_string()), now, window_ms))
    }

    /// Advance time. Lapses the lease when its expiry has passed; the epoch
    /// bumps so the silent owner's later writes are fenced.
    pub fn tick(&mut self, now: EpochMs) -> Option<LeaseEvent> {
        if self.lease.owner.is_some() && self.lease.expires_at <= now {
            Some(self.bump(None, now, 0.0))
        } else {
            None
        }
    }

    /// Time until the lease lapses, if held.
    pub fn remaining_ms(&self, now: EpochMs) -> Option<f64> {
        self.lease
            .owner
            .as_ref()
            .map(|_| (self.lease.expires_at - now).max(0.0))
    }
}

/// Owner-side safety margin: a device stops trusting its lease this much
/// before the room would lapse it, covering tick granularity and clock skew,
/// so the two views can never both say "owner" at the same instant.
pub const OWNER_LAPSE_MARGIN_MS: f64 = 2_000.0;

/// The owner's own view: when did the room last confirm our lease, and are we
/// still inside the window by our own clock. A device that hasn't heard an
/// ack for the window the room granted (minus the margin) must assume it has
/// been fenced, keep playing as a detached participant, and reclaim (or be
/// told) on reconnect.
///
/// `last_ack_at` is the *send* time of the heartbeat (or claim) the room
/// answered (the room echoes it as `ackOf`), not the arrival time of the
/// answer: the room's window starts when it received the heartbeat, which is
/// after we sent it, so measuring from the send keeps our window strictly
/// inside the room's.
///
/// The window comes from the cadence the room knew for us when it took that
/// heartbeat in. The sender records, with each heartbeat, the smallest window
/// the room could have applied ([`HeldLease::sent`]); an answer adopts the
/// window recorded for the heartbeat it answers, and anything it can't match
/// (a claim, a fresh grant) counts as the shortest window,
/// [`LEASE_DURATION_MS`].
#[derive(Debug, Clone, PartialEq)]
pub struct HeldLease {
    pub epoch: u32,
    pub last_ack_at: EpochMs,
    pub last_heartbeat_at: EpochMs,
    /// The window of the last answered heartbeat.
    pub window_ms: f64,
    /// Heartbeats in flight: `(sent_at, window)`, oldest first.
    in_flight: Vec<(EpochMs, f64)>,
}

/// Heartbeats in flight remembered for their window (a room that answers
/// none of them lapses us long before this many are sent).
const IN_FLIGHT_CAP: usize = 16;

impl HeldLease {
    pub fn new(epoch: u32, now: EpochMs) -> Self {
        HeldLease {
            epoch,
            last_ack_at: now,
            last_heartbeat_at: now,
            window_ms: LEASE_DURATION_MS,
            in_flight: Vec::new(),
        }
    }

    /// Whether a heartbeat is due at a cadence of `interval_ms`. Never later
    /// than halfway through the window we can count on: one granted before
    /// the room knew our slower cadence (a claim, a handoff) is the short
    /// one, so the next heartbeat comes early and earns the long window.
    pub fn heartbeat_due(&self, now: EpochMs, interval_ms: f64) -> bool {
        let latest = (self.window_ms - OWNER_LAPSE_MARGIN_MS) / 2.0;
        now - self.last_heartbeat_at >= interval_ms.min(latest)
    }

    /// Record a heartbeat or claim being sent; the room will renew it for
    /// at least `window_ms`.
    pub fn sent(&mut self, now: EpochMs, window_ms: f64) {
        self.last_heartbeat_at = now;
        if self.in_flight.len() >= IN_FLIGHT_CAP {
            self.in_flight.remove(0);
        }
        self.in_flight.push((now, window_ms));
    }

    /// The room answered the heartbeat/claim we sent at `sent_at`.
    pub fn acked(&mut self, sent_at: EpochMs) {
        if sent_at > self.last_ack_at {
            self.last_ack_at = sent_at;
            self.window_ms = self
                .in_flight
                .iter()
                .find(|(t, _)| *t == sent_at)
                .map(|(_, w)| *w)
                .unwrap_or(LEASE_DURATION_MS);
        }
        self.in_flight.retain(|(t, _)| *t > sent_at);
    }

    /// Whether the window has closed without an ack.
    pub fn lapsed(&self, now: EpochMs) -> bool {
        now - self.last_ack_at >= self.window_ms - OWNER_LAPSE_MARGIN_MS
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const W: f64 = LEASE_DURATION_MS;

    fn owner(m: &LeaseMachine, now: f64) -> Option<String> {
        m.live_owner(now).cloned()
    }

    #[test]
    fn fresh_claim_grants_with_new_epoch() {
        let mut m = LeaseMachine::new();
        let ev = m.claim("a", None, false, 0.0, W).unwrap();
        match ev {
            LeaseEvent::Changed {
                previous_owner,
                lease,
            } => {
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
        m.claim("a", None, false, 0.0, W).unwrap();
        assert!(matches!(
            m.heartbeat("a", 1, 5000.0, W),
            Ok(LeaseEvent::Renewed { .. })
        ));
        assert_eq!(m.lease().expires_at, 25_000.0);
        assert!(matches!(
            m.heartbeat("b", 1, 6000.0, W),
            Err(LeaseError::Fenced { .. })
        ));
        assert!(matches!(
            m.heartbeat("a", 0, 6000.0, W),
            Err(LeaseError::Fenced { .. })
        ));
        assert!(matches!(
            m.heartbeat("a", 2, 6000.0, W),
            Err(LeaseError::Fenced { .. })
        ));
    }

    #[test]
    fn lapses_after_twenty_seconds_and_bumps_epoch() {
        let mut m = LeaseMachine::new();
        m.claim("a", None, false, 0.0, W).unwrap();
        assert!(m.tick(19_999.0).is_none());
        let ev = m.tick(20_000.0).unwrap();
        assert!(matches!(ev, LeaseEvent::Changed { previous_owner: Some(ref p), .. } if p == "a"));
        assert_eq!(m.epoch(), 2);
        assert_eq!(owner(&m, 20_000.0), None);
        assert!(m.tick(30_000.0).is_none());
        // the lapsed owner's stamps are fenced
        assert!(
            matches!(m.check("a", 1, 21_000.0), Err(LeaseError::Fenced { current }) if current.epoch == 2)
        );
    }

    #[test]
    fn heartbeats_keep_it_alive_indefinitely() {
        let mut m = LeaseMachine::new();
        m.claim("a", None, false, 0.0, W).unwrap();
        let mut now = 0.0;
        for _ in 0..100 {
            now += HEARTBEAT_INTERVAL_MS;
            assert!(m.tick(now).is_none());
            m.heartbeat("a", 1, now, W).unwrap();
        }
        assert_eq!(m.epoch(), 1);
        assert_eq!(owner(&m, now).as_deref(), Some("a"));
    }

    #[test]
    fn reconnect_inside_window_keeps_ownership_and_epoch() {
        let mut m = LeaseMachine::new();
        m.claim("a", None, false, 0.0, W).unwrap();
        m.tick(15_000.0);
        let ev = m.claim("a", Some(1), false, 15_000.0, W).unwrap();
        assert!(matches!(ev, LeaseEvent::Renewed { ref lease } if lease.epoch == 1));
        assert_eq!(m.lease().expires_at, 35_000.0);
    }

    #[test]
    fn reconnect_after_lapse_is_fenced_unless_takeover() {
        let mut m = LeaseMachine::new();
        m.claim("a", None, false, 0.0, W).unwrap();
        m.tick(25_000.0);
        assert_eq!(m.epoch(), 2);
        match m.claim("a", Some(1), false, 25_000.0, W) {
            Err(LeaseError::Fenced { current }) => assert_eq!(current.epoch, 2),
            other => panic!("{other:?}"),
        }
        // deliberate claim from a fresh state (no expectation) works
        let ev = m.claim("a", None, false, 25_000.0, W).unwrap();
        assert!(matches!(ev, LeaseEvent::Changed { ref lease, .. } if lease.epoch == 3));
    }

    #[test]
    fn someone_else_holding_it_refuses_without_takeover() {
        let mut m = LeaseMachine::new();
        m.claim("a", None, false, 0.0, W).unwrap();
        assert!(matches!(
            m.claim("b", None, false, 1000.0, W),
            Err(LeaseError::Held { .. })
        ));
        let ev = m.claim("b", None, true, 1000.0, W).unwrap();
        assert!(
            matches!(ev, LeaseEvent::Changed { previous_owner: Some(ref p), ref lease } if p == "a" && lease.epoch == 2)
        );
        assert_eq!(owner(&m, 1000.0).as_deref(), Some("b"));
        // the old owner is fenced from now on
        assert!(matches!(
            m.heartbeat("a", 1, 2000.0, W),
            Err(LeaseError::Fenced { .. })
        ));
        assert!(matches!(
            m.check("a", 1, 2000.0),
            Err(LeaseError::Fenced { .. })
        ));
    }

    #[test]
    fn takeover_after_lapse_works_immediately() {
        let mut m = LeaseMachine::new();
        m.claim("a", None, false, 0.0, W).unwrap();
        m.tick(50_000.0);
        let ev = m.claim("b", None, true, 50_000.0, W).unwrap();
        assert!(matches!(
            ev,
            LeaseEvent::Changed {
                previous_owner: None,
                ..
            }
        ));
        assert_eq!(m.epoch(), 3);
    }

    #[test]
    fn release_bumps_epoch_and_frees() {
        let mut m = LeaseMachine::new();
        m.claim("a", None, false, 0.0, W).unwrap();
        assert!(matches!(
            m.release("b", 1, 100.0),
            Err(LeaseError::Fenced { .. })
        ));
        assert!(matches!(
            m.release("a", 0, 100.0),
            Err(LeaseError::Fenced { .. })
        ));
        let ev = m.release("a", 1, 100.0).unwrap();
        assert!(
            matches!(ev, LeaseEvent::Changed { ref lease, .. } if lease.owner.is_none() && lease.epoch == 2)
        );
        assert_eq!(m.remaining_ms(100.0), None);
        assert!(m.tick(1_000_000.0).is_none());
    }

    #[test]
    fn transfer_moves_ownership_with_fresh_epoch() {
        let mut m = LeaseMachine::new();
        m.claim("a", None, false, 0.0, W).unwrap();
        assert!(matches!(
            m.transfer("b", 1, "c", 10.0, W),
            Err(LeaseError::Fenced { .. })
        ));
        let ev = m.transfer("a", 1, "b", 10.0, W).unwrap();
        assert!(
            matches!(ev, LeaseEvent::Changed { previous_owner: Some(ref p), ref lease } if p == "a" && lease.owner.as_deref() == Some("b") && lease.epoch == 2)
        );
        assert!(m.check("a", 1, 11.0).is_err());
        assert!(m.check("b", 2, 11.0).is_ok());
    }

    #[test]
    fn epoch_is_monotonic_across_every_transition() {
        let mut m = LeaseMachine::new();
        let mut last = m.epoch();
        let mut now = 0.0;
        type Step = Box<dyn Fn(&mut LeaseMachine, f64)>;
        let steps: Vec<Step> = vec![
            Box::new(|m, t| {
                let _ = m.claim("a", None, false, t, W);
            }),
            Box::new(|m, t| {
                let _ = m.claim("b", None, true, t, W);
            }),
            Box::new(|m, t| {
                let e = m.epoch();
                let _ = m.release("b", e, t);
            }),
            Box::new(|m, t| {
                let _ = m.claim("c", None, false, t, W);
            }),
            Box::new(|m, t| {
                let _ = m.tick(t + 100_000.0);
            }),
            Box::new(|m, t| {
                let _ = m.claim("a", Some(1), false, t, W);
            }),
            Box::new(|m, t| {
                let _ = m.claim("a", None, false, t, W);
            }),
            Box::new(|m, t| {
                let e = m.epoch();
                let _ = m.transfer("a", e, "b", t, W);
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
        let l = TransportLease {
            owner: Some("a".into()),
            epoch: 9,
            expires_at: 20_000.0,
        };
        let mut m = LeaseMachine::from_lease(l);
        assert_eq!(owner(&m, 10_000.0).as_deref(), Some("a"));
        assert!(matches!(
            m.claim("b", None, false, 10_000.0, W),
            Err(LeaseError::Held { .. })
        ));
        assert_eq!(owner(&m, 20_000.0), None);
        assert!(m.tick(20_000.0).is_some());
        assert_eq!(m.epoch(), 10);
    }

    #[test]
    fn held_lease_timing_measures_from_send() {
        let mut h = HeldLease::new(3, 0.0);
        assert!(!h.heartbeat_due(4999.0, HEARTBEAT_INTERVAL_MS));
        assert!(h.heartbeat_due(5000.0, HEARTBEAT_INTERVAL_MS));
        assert!(!h.lapsed(17_999.0));
        assert!(h.lapsed(18_000.0));
        h.sent(5000.0, W);
        h.sent(10_000.0, W);
        h.acked(5000.0); // late answer to the first
        assert_eq!(h.last_ack_at, 5000.0);
        h.acked(10_000.0);
        assert_eq!(h.last_ack_at, 10_000.0);
        h.acked(7000.0); // out of order: never goes backwards
        assert_eq!(h.last_ack_at, 10_000.0);
        assert!(!h.lapsed(27_999.0));
        assert!(h.lapsed(28_000.0));
    }
    #[test]
    fn held_lease_adopts_the_window_of_the_answered_heartbeat() {
        let mut h = HeldLease::new(1, 0.0);
        assert_eq!(h.window_ms, LEASE_DURATION_MS);
        // Slow cadence, short window: due halfway through the window.
        assert!(!h.heartbeat_due(8_999.0, 45_000.0));
        assert!(h.heartbeat_due(9_000.0, 45_000.0));
        let slow = lease_window_ms(45_000.0);
        assert_eq!(slow, 100_000.0);
        h.sent(45_000.0, slow);
        h.acked(45_000.0);
        assert!(!h.lapsed(45_000.0 + 97_999.0));
        assert!(h.lapsed(45_000.0 + 98_000.0));
        assert!(!h.heartbeat_due(45_000.0 + 44_999.0, 45_000.0));
        assert!(h.heartbeat_due(45_000.0 + 45_000.0, 45_000.0));
        // Sped up: the next heartbeat's shorter window wins once answered.
        h.sent(50_000.0, W);
        h.acked(50_000.0);
        assert!(!h.lapsed(50_000.0 + 17_999.0));
        assert!(h.lapsed(50_000.0 + 18_000.0));
        // An answer we can't match (a fresh grant) is the shortest window.
        h.sent(60_000.0, slow);
        h.acked(61_000.0);
        assert_eq!(h.window_ms, LEASE_DURATION_MS);
    }

    #[test]
    fn cadence_helpers_scale_and_clamp() {
        assert_eq!(clamp_cadence(1.0), HEARTBEAT_INTERVAL_MS);
        assert_eq!(clamp_cadence(f64::NAN), HEARTBEAT_INTERVAL_MS);
        assert_eq!(clamp_cadence(1e9), MAX_CADENCE_MS);
        assert_eq!(cadence_slack_ms(HEARTBEAT_INTERVAL_MS), 0.0);
        assert_eq!(lease_window_ms(HEARTBEAT_INTERVAL_MS), LEASE_DURATION_MS);
        assert_eq!(lease_window_ms(15_000.0), 40_000.0);
        assert_eq!(lease_window_ms(MAX_CADENCE_MS), 130_000.0);
        // Every window still covers two missed beats plus the owner's margin.
        for c in [5_000.0, 15_000.0, 45_000.0, MAX_CADENCE_MS] {
            assert!(lease_window_ms(c) - OWNER_LAPSE_MARGIN_MS > 2.0 * c, "{c}");
        }
    }

    #[test]
    fn a_slow_owner_renewing_on_time_never_lapses() {
        let mut m = LeaseMachine::new();
        let w = lease_window_ms(45_000.0);
        m.claim("a", None, false, 0.0, w).unwrap();
        let mut now = 0.0;
        for _ in 0..20 {
            now += 45_000.0;
            assert!(m.tick(now).is_none());
            m.heartbeat("a", 1, now, w).unwrap();
        }
        assert!(m.tick(now + w).is_some());
    }
}
