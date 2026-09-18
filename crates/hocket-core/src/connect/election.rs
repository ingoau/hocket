//! LAN coordinator-role election.
//!
//! On a LAN without a hosted coordinator, one device performs the coordinator
//! role (holds the room and the replica) and the others connect to it. The
//! election is deterministic so every peer reaches the same answer from the
//! same view, without a round of messages:
//!
//! 1. a peer already *serving* a room with members wins, so an established
//!    session isn't disrupted by a newcomer (stickiness);
//! 2. otherwise the highest session revision wins (the freshest state);
//! 3. ties go to the lowest device id.
//!
//! It is re-run on every membership change (peer discovered, peer lost). Any
//! client can perform the role: the code is the same one the hosted
//! coordinator runs.

use crate::api::DeviceId;

/// What a peer advertises, as far as the election cares.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    pub device_id: DeviceId,
    pub session_revision: u32,
    /// Currently serving a LAN room with at least one other member.
    pub serving: bool,
}

/// Pick the coordinator among `candidates`. `None` for an empty slice.
pub fn elect(candidates: &[Candidate]) -> Option<&Candidate> {
    candidates.iter().min_by(|a, b| {
        b.serving
            .cmp(&a.serving)
            .then_with(|| b.session_revision.cmp(&a.session_revision))
            .then_with(|| a.device_id.cmp(&b.device_id))
    })
}

/// Convenience: the winning device id.
pub fn elect_id(candidates: &[Candidate]) -> Option<DeviceId> {
    elect(candidates).map(|c| c.device_id.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(id: &str, rev: u32, serving: bool) -> Candidate {
        Candidate { device_id: id.into(), session_revision: rev, serving }
    }

    #[test]
    fn empty_has_no_winner() {
        assert_eq!(elect(&[]), None);
    }

    #[test]
    fn highest_revision_wins() {
        let cs = [c("zz", 10, false), c("aa", 3, false)];
        assert_eq!(elect_id(&cs).as_deref(), Some("zz"));
    }

    #[test]
    fn ties_go_to_lowest_id() {
        let cs = [c("m", 5, false), c("b", 5, false), c("k", 5, false)];
        assert_eq!(elect_id(&cs).as_deref(), Some("b"));
    }

    #[test]
    fn serving_peer_is_sticky() {
        let cs = [c("a", 99, false), c("z", 1, true)];
        assert_eq!(elect_id(&cs).as_deref(), Some("z"));
        let cs = [c("y", 1, true), c("z", 1, true)];
        assert_eq!(elect_id(&cs).as_deref(), Some("y"));
    }

    #[test]
    fn order_independent() {
        let a = [c("a", 1, false), c("b", 2, false), c("c", 2, false)];
        let mut b = a.clone();
        b.reverse();
        assert_eq!(elect_id(&a), elect_id(&b));
        assert_eq!(elect_id(&a).as_deref(), Some("b"));
    }

    #[test]
    fn membership_change_reruns() {
        let mut cs = vec![c("b", 2, false), c("c", 2, false)];
        assert_eq!(elect_id(&cs).as_deref(), Some("b"));
        cs.remove(0);
        assert_eq!(elect_id(&cs).as_deref(), Some("c"));
        cs.push(c("a", 7, false));
        assert_eq!(elect_id(&cs).as_deref(), Some("a"));
    }
}
