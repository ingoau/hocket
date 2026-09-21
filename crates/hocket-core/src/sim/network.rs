//! An in-memory network that can partition, delay, drop and reorder on
//! command. Nodes are named; a URL maps to a node. Connections are pairs of
//! peer ids, one per end, exactly like the real transport hands the engine.
//!
//! Deterministic: every random choice comes from the seeded RNG the world
//! owns, and deliveries are ordered by `(time, sequence)`.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use rand::Rng;
use rand_chacha::ChaCha8Rng;

use crate::connect::wire::WireMessage;
use crate::connect::PeerId;
use crate::sim::clock::to_micros;

pub type NodeId = String;

/// What a node receives.
#[derive(Debug, Clone, PartialEq)]
pub enum NetEvent {
    /// A connection this node asked for is open.
    Connected { peer: PeerId, url: String },
    /// A connection this node asked for could not be opened.
    ConnectFailed { error: String },
    /// Someone connected to this node's listener.
    Accepted { peer: PeerId },
    Message { peer: PeerId, msg: WireMessage },
    Closed { peer: PeerId },
}

#[derive(Debug, Clone)]
struct Delivery {
    at: f64,
    to: NodeId,
    event: NetEvent,
}

#[derive(Debug, Clone)]
struct Link {
    a: NodeId,
    a_peer: PeerId,
    b: NodeId,
    b_peer: PeerId,
    /// Last scheduled delivery time in each direction: a stream never
    /// reorders (jitter delays, it does not overtake), like TCP.
    last_a_to_b: f64,
    last_b_to_a: f64,
}

/// Link-level conditions between two nodes.
///
/// `drop` is the probability that a frame hits packet loss. The protocol runs
/// over TCP, which never loses or reorders a frame on a live connection: loss
/// shows up as a retransmission delay ([`RETRANSMIT_MIN_MS`]..[`RETRANSMIT_MAX_MS`])
/// that also holds back everything queued behind it on that stream. Real
/// breakage is modelled explicitly by `cut`, `partition` and `node_down`.
#[derive(Debug, Clone, Copy)]
pub struct Conditions {
    pub delay_ms: f64,
    pub jitter_ms: f64,
    pub drop: f64,
}

/// Retransmission delay range for a frame that hit packet loss.
pub const RETRANSMIT_MIN_MS: f64 = 200.0;
pub const RETRANSMIT_MAX_MS: f64 = 1_500.0;

impl Default for Conditions {
    fn default() -> Self {
        Conditions { delay_ms: 20.0, jitter_ms: 5.0, drop: 0.0 }
    }
}

#[derive(Debug)]
pub struct Network {
    urls: HashMap<String, NodeId>,
    links: Vec<Link>,
    queue: BTreeMap<(i64, u64), Delivery>,
    seq: u64,
    peer_seq: u64,
    partitions: BTreeSet<(NodeId, NodeId)>,
    /// Nodes that are switched off: everything to or from them is dropped.
    down: BTreeSet<NodeId>,
    default: Conditions,
    per_pair: HashMap<(NodeId, NodeId), Conditions>,
    pub stats_dropped: u64,
    pub stats_delivered: u64,
    pub stats_retransmitted: u64,
}

fn pair(a: &str, b: &str) -> (NodeId, NodeId) {
    if a <= b {
        (a.to_string(), b.to_string())
    } else {
        (b.to_string(), a.to_string())
    }
}

impl Network {
    pub fn new(default: Conditions) -> Self {
        Network {
            urls: HashMap::new(),
            links: vec![],
            queue: BTreeMap::new(),
            seq: 0,
            peer_seq: 0,
            partitions: BTreeSet::new(),
            down: BTreeSet::new(),
            default,
            per_pair: HashMap::new(),
            stats_dropped: 0,
            stats_delivered: 0,
            stats_retransmitted: 0,
        }
    }

    /// Make `url` reach `node`.
    pub fn bind(&mut self, url: &str, node: &str) {
        self.urls.insert(url.to_string(), node.to_string());
    }

    pub fn unbind(&mut self, url: &str) {
        self.urls.remove(url);
    }

    pub fn set_conditions(&mut self, a: &str, b: &str, c: Conditions) {
        self.per_pair.insert(pair(a, b), c);
    }

    pub fn set_default_conditions(&mut self, c: Conditions) {
        self.default = c;
    }

    fn conditions(&self, a: &str, b: &str) -> Conditions {
        self.per_pair.get(&pair(a, b)).copied().unwrap_or(self.default)
    }

    /// Silently drop everything between `a` and `b` (sockets stay "open"
    /// until someone times out).
    pub fn partition(&mut self, a: &str, b: &str) {
        self.partitions.insert(pair(a, b));
    }

    pub fn heal(&mut self, a: &str, b: &str) {
        self.partitions.remove(&pair(a, b));
    }

    pub fn heal_all(&mut self) {
        self.partitions.clear();
        self.down.clear();
    }

    pub fn is_partitioned(&self, a: &str, b: &str) -> bool {
        self.partitions.contains(&pair(a, b)) || self.down.contains(a) || self.down.contains(b)
    }

    /// Switch a node off (crash, sleep): its links close for the other side
    /// after a delay, as a real TCP reset would, and nothing reaches it.
    pub fn node_down(&mut self, now: f64, node: &str) {
        self.down.insert(node.to_string());
        let links: Vec<Link> = self.links.iter().filter(|l| l.a == node || l.b == node).cloned().collect();
        for l in links {
            let (other, other_peer) = if l.a == node { (l.b.clone(), l.b_peer.clone()) } else { (l.a.clone(), l.a_peer.clone()) };
            self.enqueue(now + 50.0, &other, NetEvent::Closed { peer: other_peer });
        }
        self.links.retain(|l| l.a != node && l.b != node);
        self.queue.retain(|_, d| d.to != node);
    }

    pub fn node_up(&mut self, node: &str) {
        self.down.remove(node);
    }

    pub fn is_down(&self, node: &str) -> bool {
        self.down.contains(node)
    }

    /// Cut every link between `a` and `b` right now (both sides see `Closed`).
    pub fn cut(&mut self, now: f64, a: &str, b: &str) {
        let links: Vec<Link> =
            self.links.iter().filter(|l| pair(&l.a, &l.b) == pair(a, b)).cloned().collect();
        for l in links {
            self.enqueue(now + 1.0, &l.a, NetEvent::Closed { peer: l.a_peer.clone() });
            self.enqueue(now + 1.0, &l.b, NetEvent::Closed { peer: l.b_peer.clone() });
        }
        self.links.retain(|l| pair(&l.a, &l.b) != pair(a, b));
    }

    fn next_peer(&mut self, prefix: &str) -> PeerId {
        self.peer_seq += 1;
        format!("{prefix}-{}", self.peer_seq)
    }

    fn enqueue(&mut self, at: f64, to: &str, event: NetEvent) {
        self.seq += 1;
        let key = (to_micros(at), self.seq);
        self.queue.insert(key, Delivery { at, to: to.to_string(), event });
    }

    /// `from` opens a connection to the first candidate URL that resolves to
    /// a reachable node. Delivers `Connected` to `from` and `Accepted` to the
    /// target, or `ConnectFailed`.
    pub fn connect(&mut self, now: f64, rng: &mut ChaCha8Rng, from: &str, candidates: &[String]) {
        for url in candidates {
            let Some(target) = self.urls.get(url).cloned() else { continue };
            if target == from {
                continue;
            }
            if self.is_partitioned(from, &target) {
                continue;
            }
            let c = self.conditions(from, &target);
            let a_peer = self.next_peer("out");
            let b_peer = self.next_peer("in");
            self.links.push(Link {
                a: from.to_string(),
                a_peer: a_peer.clone(),
                b: target.clone(),
                b_peer: b_peer.clone(),
                last_a_to_b: 0.0,
                last_b_to_a: 0.0,
            });
            let rtt = c.delay_ms * 2.0 + rng.random_range(0.0..=c.jitter_ms);
            self.enqueue(now + rtt, from, NetEvent::Connected { peer: a_peer, url: url.clone() });
            self.enqueue(now + rtt / 2.0, &target, NetEvent::Accepted { peer: b_peer });
            return;
        }
        let delay = self.default.delay_ms * 2.0;
        self.enqueue(now + delay, from, NetEvent::ConnectFailed { error: "unreachable".into() });
    }

    fn link_for(&self, node: &str, peer: &str) -> Option<(usize, NodeId, PeerId)> {
        self.links.iter().enumerate().find_map(|(i, l)| {
            if l.a == node && l.a_peer == peer {
                Some((i, l.b.clone(), l.b_peer.clone()))
            } else if l.b == node && l.b_peer == peer {
                Some((i, l.a.clone(), l.a_peer.clone()))
            } else {
                None
            }
        })
    }

    /// Send a frame from `node` on `peer`. Applies partition, drop, delay and
    /// jitter (which reorders).
    pub fn send(&mut self, now: f64, rng: &mut ChaCha8Rng, node: &str, peer: &str, msg: WireMessage) {
        let Some((idx, other, other_peer)) = self.link_for(node, peer) else { return };
        if self.is_partitioned(node, &other) {
            self.stats_dropped += 1;
            return;
        }
        let c = self.conditions(node, &other);
        let mut at = now + c.delay_ms + rng.random_range(0.0..=c.jitter_ms);
        if c.drop > 0.0 && rng.random::<f64>() < c.drop {
            // Packet loss on a TCP stream: retransmitted, later, still in order.
            self.stats_retransmitted += 1;
            at += rng.random_range(RETRANSMIT_MIN_MS..=RETRANSMIT_MAX_MS);
        }
        let link = &mut self.links[idx];
        let last = if link.a == node { &mut link.last_a_to_b } else { &mut link.last_b_to_a };
        if at < *last {
            at = *last;
        }
        *last = at;
        self.enqueue(at, &other, NetEvent::Message { peer: other_peer, msg });
    }

    /// Close a connection from one side; the other side sees `Closed`.
    pub fn close(&mut self, now: f64, node: &str, peer: &str) {
        let Some((i, other, other_peer)) = self.link_for(node, peer) else { return };
        self.links.remove(i);
        if !self.is_partitioned(node, &other) {
            let c = self.conditions(node, &other);
            self.enqueue(now + c.delay_ms, &other, NetEvent::Closed { peer: other_peer });
        }
    }

    /// Time of the next delivery, if any.
    pub fn next_at(&self) -> Option<f64> {
        self.queue.values().next().map(|d| d.at)
    }

    /// Pop every delivery due at or before `now`.
    pub fn due(&mut self, now: f64) -> Vec<(NodeId, NetEvent)> {
        let mut out = vec![];
        let limit = (to_micros(now), u64::MAX);
        let keys: Vec<(i64, u64)> = self.queue.range(..=limit).map(|(k, _)| *k).collect();
        for k in keys {
            if let Some(d) = self.queue.remove(&k) {
                if self.down.contains(&d.to) {
                    self.stats_dropped += 1;
                    continue;
                }
                self.stats_delivered += 1;
                out.push((d.to, d.event));
            }
        }
        out
    }

    pub fn in_flight(&self) -> usize {
        self.queue.len()
    }

    /// Message-type histogram of what is queued (for diagnosing storms).
    pub fn pending_summary(&self) -> String {
        let mut counts: BTreeMap<String, usize> = BTreeMap::new();
        for d in self.queue.values() {
            let name = match &d.event {
                NetEvent::Message { msg, .. } => format!("{}->{}", msg.msg.name(), d.to),
                other => format!("{other:?}").chars().take(12).collect(),
            };
            *counts.entry(name).or_default() += 1;
        }
        format!("{counts:?}")
    }

    pub fn link_count(&self) -> usize {
        self.links.len()
    }

    /// Whether `a` and `b` share an open link.
    pub fn linked(&self, a: &str, b: &str) -> bool {
        self.links.iter().any(|l| pair(&l.a, &l.b) == pair(a, b))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::connect::wire::Msg;
    use rand::SeedableRng;

    #[test]
    fn connect_send_reorder_partition_close() {
        let mut rng = ChaCha8Rng::seed_from_u64(1);
        let mut n = Network::new(Conditions { delay_ms: 10.0, jitter_ms: 0.0, drop: 0.0 });
        n.bind("wss://c/", "c");
        n.connect(0.0, &mut rng, "a", &["wss://nope/".into(), "wss://c/".into()]);
        let mut evs = n.due(100.0);
        evs.sort_by(|x, y| x.0.cmp(&y.0));
        assert!(matches!(&evs[0], (n, NetEvent::Connected { url, .. }) if n == "a" && url == "wss://c/"));
        assert!(matches!(&evs[1], (n, NetEvent::Accepted { .. }) if n == "c"));
        let a_peer = match &evs[0].1 {
            NetEvent::Connected { peer, .. } => peer.clone(),
            _ => unreachable!(),
        };
        let c_peer = match &evs[1].1 {
            NetEvent::Accepted { peer } => peer.clone(),
            _ => unreachable!(),
        };
        n.send(100.0, &mut rng, "a", &a_peer, WireMessage::new(Msg::ClockPing { t0: 1.0 }));
        assert_eq!(n.next_at(), Some(110.0));
        let evs = n.due(110.0);
        assert!(matches!(&evs[0], (to, NetEvent::Message { peer, msg }) if to == "c" && peer == &c_peer && msg.msg == Msg::ClockPing { t0: 1.0 }));
        // partition drops silently
        n.partition("a", "c");
        n.send(110.0, &mut rng, "c", &c_peer, WireMessage::new(Msg::SyncRequest));
        assert_eq!(n.in_flight(), 0);
        assert_eq!(n.stats_dropped, 1);
        n.heal("a", "c");
        // jitter delays but a stream stays in order (TCP); across streams it reorders
        n.set_default_conditions(Conditions { delay_ms: 10.0, jitter_ms: 50.0, drop: 0.0 });
        n.connect(150.0, &mut rng, "b", &["wss://c/".into()]);
        let b_peer = n
            .due(200.0)
            .into_iter()
            .find_map(|(to, e)| match e {
                NetEvent::Connected { peer, .. } if to == "b" => Some(peer),
                _ => None,
            })
            .unwrap();
        for i in 0..20 {
            n.send(200.0, &mut rng, "a", &a_peer, WireMessage::new(Msg::ClockPing { t0: i as f64 }));
            n.send(200.0, &mut rng, "b", &b_peer, WireMessage::new(Msg::ClockPing { t0: 100.0 + i as f64 }));
        }
        let evs = n.due(300.0);
        let order: Vec<f64> = evs
            .iter()
            .map(|(_, e)| match e {
                NetEvent::Message { msg, .. } => match msg.msg {
                    Msg::ClockPing { t0 } => t0,
                    _ => unreachable!(),
                },
                _ => unreachable!(),
            })
            .collect();
        let from_a: Vec<f64> = order.iter().copied().filter(|t| *t < 100.0).collect();
        let mut sorted_a = from_a.clone();
        sorted_a.sort_by(|x, y| x.partial_cmp(y).unwrap());
        assert_eq!(from_a, sorted_a, "one stream never reorders");
        let mut sorted = order.clone();
        sorted.sort_by(|x, y| x.partial_cmp(y).unwrap());
        assert_ne!(order, sorted, "streams interleave under jitter");
        // close notifies the other end
        n.close(300.0, "a", &a_peer);
        let evs = n.due(400.0);
        assert!(matches!(&evs[0], (to, NetEvent::Closed { peer }) if to == "c" && peer == &c_peer));
        assert_eq!(n.link_count(), 1); // b's stream is still up
        // unreachable url fails
        n.connect(400.0, &mut rng, "a", &["wss://nope/".into()]);
        let evs = n.due(500.0);
        assert!(matches!(&evs[0], (_, NetEvent::ConnectFailed { .. })));
    }

    #[test]
    fn node_down_closes_links_for_others_and_drops_inbound() {
        let mut rng = ChaCha8Rng::seed_from_u64(2);
        let mut n = Network::new(Conditions { delay_ms: 1.0, jitter_ms: 0.0, drop: 0.0 });
        n.bind("wss://c/", "c");
        n.connect(0.0, &mut rng, "a", &["wss://c/".into()]);
        n.due(10.0);
        assert_eq!(n.link_count(), 1);
        n.node_down(10.0, "a");
        let evs = n.due(100.0);
        assert!(matches!(&evs[0], (to, NetEvent::Closed { .. }) if to == "c"));
        assert!(n.is_partitioned("a", "c"));
        n.connect(100.0, &mut rng, "a", &["wss://c/".into()]);
        assert!(n.due(200.0).is_empty()); // a is down: nothing reaches it
        n.node_up("a");
        n.connect(200.0, &mut rng, "a", &["wss://c/".into()]);
        assert_eq!(n.due(300.0).len(), 2);
    }
}
