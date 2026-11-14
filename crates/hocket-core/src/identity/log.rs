//! The member list: a hash-chained, signed op log.
//!
//! Every op names its parents (the heads its author knew), is signed by its
//! author's device key and is identified by the SHA-256 of its signed
//! encoding. The log is a grow-only set: merging is a union, so replaying an
//! old list never takes anything away, and the member list is a pure
//! function of the set of ops ([`MemberLog::state`]). Every device and the
//! coordinator run the same function, so the same ops give the same members
//! everywhere, in whatever order they arrived.
//!
//! The rules ([`compute`]):
//!
//! 1. **Structure.** One genesis (no parents, an `Add` of its own author
//!    carrying the first group key). Every other op's parents are in the
//!    log, and its author has an `Add` in its causal past whose key verifies
//!    its signature. Anything else is never stored.
//! 2. **Authority, causally.** An op is valid only if its author is a member
//!    in the op's causal past (added by a valid op there, not removed by
//!    one). So an op made after its author's removal is invalid.
//! 3. **Removal wins.** A valid `Remove` of device D voids every op by D that
//!    is not in the removal's causal past: what D did concurrently with its
//!    removal, or after it, never counts (including whatever those ops
//!    enabled, such as a device D added). A removed device id or key is never
//!    added again.
//! 4. **Tie-break.** Removals take effect in ascending op-hash order. When
//!    two removals conflict (each voids the other: two devices removing each
//!    other at the same moment), the lowest hash wins.
//! 5. **Group key.** Genesis, `Remove` and `Rotate` commit to a new group
//!    key (`keyId`, the hash of the key). The current key is the one
//!    committed by the last valid such op in the log's deterministic
//!    topological order (parents first, ties by lowest hash).
//!
//! Voiding is monotonic (a voided op stays void), so the fixpoint in
//! [`compute`] terminates, and it depends only on the set of ops.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::api::{DeviceId, EpochMs};
use crate::identity::crypto::{self, Canon, DeviceSecret, PublicKeys};

/// Most ops one log holds; merges beyond it are refused.
pub const MAX_OPS: usize = 4096;
/// Most parents one op may name.
pub const MAX_PARENTS: usize = 32;
/// Longest device name kept in an op.
pub const MAX_NAME: usize = 120;

/// A device as the member list knows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemberDevice {
    pub id: DeviceId,
    pub name: String,
    pub keys: PublicKeys,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "type")]
pub enum OpBody {
    /// Admit a device. `keyId` only on the genesis op (the first key).
    Add {
        device: MemberDevice,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        key_id: Option<String>,
    },
    /// Remove a device (it may be the author: leaving), with the new group
    /// key the remaining devices move to.
    Remove { device_id: DeviceId, key_id: String },
    /// A new group key without a membership change (credential sync turned
    /// off).
    Rotate { key_id: String },
}

/// One signed entry of the member list.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemberOp {
    pub identity: String,
    pub author: DeviceId,
    /// Op hashes, sorted.
    pub parents: Vec<String>,
    /// The author's clock when it made the op. Shown, never trusted.
    pub at: EpochMs,
    pub body: OpBody,
    /// ed25519 over [`MemberOp::signed_bytes`], hex.
    pub sig: String,
}

impl MemberOp {
    fn signed_bytes(&self) -> Vec<u8> {
        let mut c = Canon::new("hocket-member-op-v1");
        c.str(&self.identity)
            .str(&self.author)
            .list(&self.parents)
            .num(self.at);
        match &self.body {
            OpBody::Add { device, key_id } => {
                c.str("add")
                    .str(&device.id)
                    .str(&device.name)
                    .str(&device.keys.sign)
                    .str(&device.keys.kx)
                    .str(key_id.as_deref().unwrap_or(""));
            }
            OpBody::Remove { device_id, key_id } => {
                c.str("remove").str(device_id).str(key_id);
            }
            OpBody::Rotate { key_id } => {
                c.str("rotate").str(key_id);
            }
        }
        c.finish().to_vec()
    }

    /// The op's id: SHA-256 of its signed bytes and signature, hex.
    pub fn hash(&self) -> String {
        let mut b = self.signed_bytes();
        b.extend_from_slice(self.sig.as_bytes());
        crypto::sha256_hex(&b)
    }

    /// Build and sign an op.
    pub fn new(
        identity: &str,
        author: &str,
        mut parents: Vec<String>,
        at: EpochMs,
        body: OpBody,
        secret: &DeviceSecret,
    ) -> MemberOp {
        parents.sort();
        parents.dedup();
        let mut op = MemberOp {
            identity: identity.to_string(),
            author: author.to_string(),
            parents,
            at,
            body,
            sig: String::new(),
        };
        op.sig = secret.sign(&op.signed_bytes());
        op
    }

    fn key_commit(&self) -> Option<&str> {
        match &self.body {
            OpBody::Add { key_id, .. } => key_id.as_deref(),
            OpBody::Remove { key_id, .. } | OpBody::Rotate { key_id } => Some(key_id),
        }
    }
}

/// Why ops were not taken.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum LogError {
    #[error("the op belongs to another identity")]
    WrongIdentity,
    #[error("the member list is full")]
    Full,
}

/// The member list as computed from the log.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MemberState {
    /// Current members by device id.
    pub members: BTreeMap<DeviceId, MemberDevice>,
    /// Devices that were members and were removed.
    pub removed: BTreeSet<DeviceId>,
    /// The current group key's id.
    pub key_id: Option<String>,
    /// Valid op hashes.
    pub valid: BTreeSet<String>,
}

impl MemberState {
    pub fn is_member(&self, device_id: &str) -> bool {
        self.members.contains_key(device_id)
    }

    /// The member's signing key, when `device_id` is a current member.
    pub fn sign_key(&self, device_id: &str) -> Option<&str> {
        self.members.get(device_id).map(|d| d.keys.sign.as_str())
    }
}

/// The op log of one identity.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemberLog {
    pub identity: String,
    #[serde(with = "ops_as_list")]
    ops: BTreeMap<String, MemberOp>,
    #[serde(skip)]
    state: Option<MemberState>,
}

impl MemberLog {
    /// A new identity's log: the genesis op admitting its first device.
    pub fn genesis(
        identity: &str,
        device: MemberDevice,
        key_id: String,
        at: EpochMs,
        secret: &DeviceSecret,
    ) -> MemberLog {
        let op = MemberOp::new(
            identity,
            &device.id.clone(),
            vec![],
            at,
            OpBody::Add {
                device,
                key_id: Some(key_id),
            },
            secret,
        );
        let mut log = MemberLog {
            identity: identity.to_string(),
            ..Default::default()
        };
        log.ops.insert(op.hash(), op);
        log
    }

    /// An empty log for an identity known only by id (a coordinator before
    /// its first device synced).
    pub fn empty(identity: &str) -> MemberLog {
        MemberLog {
            identity: identity.to_string(),
            ..Default::default()
        }
    }

    /// A log holding just `ops` (a partial sync; never validated as a whole).
    pub(crate) fn partial(identity: &str, ops: Vec<MemberOp>) -> MemberLog {
        MemberLog {
            identity: identity.to_string(),
            ops: ops.into_iter().map(|o| (o.hash(), o)).collect(),
            state: None,
        }
    }

    pub fn len(&self) -> usize {
        self.ops.len()
    }

    pub fn is_empty(&self) -> bool {
        self.ops.is_empty()
    }

    pub fn contains(&self, hash: &str) -> bool {
        self.ops.contains_key(hash)
    }

    pub fn get(&self, hash: &str) -> Option<&MemberOp> {
        self.ops.get(hash)
    }

    /// Every op, parents before children.
    pub fn ops(&self) -> Vec<MemberOp> {
        topo_order(&self.ops)
            .into_iter()
            .map(|h| self.ops[&h].clone())
            .collect()
    }

    /// The ops that are nobody's parent, sorted.
    pub fn heads(&self) -> Vec<String> {
        let parents: BTreeSet<&String> = self.ops.values().flat_map(|o| &o.parents).collect();
        self.ops
            .keys()
            .filter(|h| !parents.contains(h))
            .cloned()
            .collect()
    }

    /// A short digest of [`MemberLog::heads`] (what proofs sign).
    pub fn head_digest(&self) -> String {
        let mut c = Canon::new("hocket-heads");
        c.list(&self.heads());
        crypto::sha256_hex(c.finish())
    }

    /// Whether every op in `heads` is already here.
    pub fn knows(&self, heads: &[String]) -> bool {
        heads.iter().all(|h| self.ops.contains_key(h))
    }

    /// The ops a peer whose heads are `theirs` is missing: everything not in
    /// the causal past of the heads of theirs we know, parents first. A head
    /// we don't know is ignored (they are ahead there, and pull nothing).
    pub fn missing_for(&self, theirs: &[String]) -> Vec<MemberOp> {
        let mut known: BTreeSet<String> = BTreeSet::new();
        let mut stack: Vec<String> = theirs
            .iter()
            .filter(|h| self.ops.contains_key(*h))
            .cloned()
            .collect();
        while let Some(h) = stack.pop() {
            if !known.insert(h.clone()) {
                continue;
            }
            if let Some(op) = self.ops.get(&h) {
                stack.extend(op.parents.iter().cloned());
            }
        }
        self.ops()
            .into_iter()
            .filter(|o| !known.contains(&o.hash()))
            .collect()
    }

    /// Union in ops from a peer. Ops that fail the structural checks (bad
    /// signature, unknown parents, a second genesis) are dropped; the rest
    /// are kept whether or not they turn out valid. Returns how many were new.
    pub fn merge(&mut self, ops: Vec<MemberOp>) -> Result<usize, LogError> {
        let mut pending: Vec<MemberOp> = vec![];
        for op in ops {
            if op.identity != self.identity {
                return Err(LogError::WrongIdentity);
            }
            let h = op.hash();
            if !self.ops.contains_key(&h) && !pending.iter().any(|p| p.hash() == h) {
                pending.push(op);
            }
        }
        let mut added = 0;
        // Repeat while progress is made, so ops arriving children-first still land.
        loop {
            let mut progress = false;
            let mut rest = vec![];
            for op in pending {
                if self.ops.len() >= MAX_OPS {
                    if added > 0 {
                        self.state = None;
                    }
                    return Err(LogError::Full);
                }
                match self.structurally_ok(&op) {
                    Some(true) => {
                        self.ops.insert(op.hash(), op);
                        added += 1;
                        progress = true;
                    }
                    Some(false) => {}
                    None => rest.push(op),
                }
            }
            pending = rest;
            if !progress || pending.is_empty() {
                break;
            }
        }
        if added > 0 {
            self.state = None;
        }
        Ok(added)
    }

    /// `Some(ok)` when decidable now; `None` while a parent is missing.
    fn structurally_ok(&self, op: &MemberOp) -> Option<bool> {
        if op.parents.len() > MAX_PARENTS
            || op.parents.windows(2).any(|w| w[0] >= w[1])
            || op.author.is_empty()
        {
            return Some(false);
        }
        if let OpBody::Add { device, .. } = &op.body {
            if device.name.len() > MAX_NAME || device.id.is_empty() {
                return Some(false);
            }
        }
        if op.parents.is_empty() {
            // Genesis: the first device adds itself with the first key.
            let ok = self.ops.values().all(|o| !o.parents.is_empty())
                && matches!(&op.body, OpBody::Add { device, key_id: Some(_) }
                    if device.id == op.author
                        && crypto::verify(&device.keys.sign, &op.signed_bytes(), &op.sig));
            return Some(ok);
        }
        if op.parents.iter().any(|p| !self.ops.contains_key(p)) {
            return None;
        }
        // The author's key comes from an Add in the op's causal past.
        let keys = self.author_keys_in_past(&op.parents, &op.author);
        let signed = op.signed_bytes();
        Some(keys.iter().any(|k| crypto::verify(k, &signed, &op.sig)))
    }

    fn author_keys_in_past(&self, parents: &[String], author: &str) -> Vec<String> {
        let mut seen = BTreeSet::new();
        let mut stack: Vec<&String> = parents.iter().collect();
        let mut keys = vec![];
        while let Some(h) = stack.pop() {
            if !seen.insert(h.clone()) {
                continue;
            }
            let Some(op) = self.ops.get(h) else { continue };
            if let OpBody::Add { device, .. } = &op.body {
                if device.id == author {
                    keys.push(device.keys.sign.clone());
                }
            }
            stack.extend(op.parents.iter());
        }
        keys
    }

    /// The member list (computed once per change).
    pub fn state(&mut self) -> &MemberState {
        if self.state.is_none() {
            self.state = Some(compute(&self.ops));
        }
        self.state.as_ref().expect("just computed")
    }

    /// The member list without caching (for a shared reference).
    pub fn compute_state(&self) -> MemberState {
        match &self.state {
            Some(s) => s.clone(),
            None => compute(&self.ops),
        }
    }

    /// Sign a new op on top of the current heads.
    pub fn append(
        &mut self,
        author: &str,
        at: EpochMs,
        body: OpBody,
        secret: &DeviceSecret,
    ) -> MemberOp {
        let op = MemberOp::new(&self.identity, author, self.heads(), at, body, secret);
        self.ops.insert(op.hash(), op.clone());
        self.state = None;
        op
    }
}

/// Ops travel and persist as a list; the map is rebuilt with the hashes
/// recomputed, so a peer cannot mislabel one.
mod ops_as_list {
    use super::MemberOp;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    use std::collections::BTreeMap;

    pub fn serialize<S: Serializer>(
        ops: &BTreeMap<String, MemberOp>,
        s: S,
    ) -> Result<S::Ok, S::Error> {
        let list: Vec<&MemberOp> = ops.values().collect();
        list.serialize(s)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(
        d: D,
    ) -> Result<BTreeMap<String, MemberOp>, D::Error> {
        let list = Vec::<MemberOp>::deserialize(d)?;
        Ok(list.into_iter().map(|o| (o.hash(), o)).collect())
    }
}

/// Parents before children; among ops ready at once, lowest hash first.
fn topo_order(ops: &BTreeMap<String, MemberOp>) -> Vec<String> {
    let mut indegree: BTreeMap<&String, usize> = BTreeMap::new();
    let mut children: BTreeMap<&String, Vec<&String>> = BTreeMap::new();
    for (h, op) in ops {
        let n = op.parents.iter().filter(|p| ops.contains_key(*p)).count();
        indegree.insert(h, n);
        for p in &op.parents {
            if let Some((pk, _)) = ops.get_key_value(p) {
                children.entry(pk).or_default().push(h);
            }
        }
    }
    let mut ready: BTreeSet<&String> = indegree
        .iter()
        .filter(|(_, n)| **n == 0)
        .map(|(h, _)| *h)
        .collect();
    let mut order = Vec::with_capacity(ops.len());
    while let Some(h) = ready.pop_first() {
        order.push(h.clone());
        for c in children.get(h).into_iter().flatten() {
            let n = indegree.get_mut(c).expect("every op has an indegree");
            *n -= 1;
            if *n == 0 {
                ready.insert(c);
            }
        }
    }
    order
}

/// A set of op indices.
#[derive(Clone, PartialEq, Eq)]
struct Bits(Vec<u64>);

impl Bits {
    fn new(n: usize) -> Bits {
        Bits(vec![0; n.div_ceil(64)])
    }
    fn set(&mut self, i: usize) {
        self.0[i / 64] |= 1 << (i % 64);
    }
    fn has(&self, i: usize) -> bool {
        self.0[i / 64] & (1 << (i % 64)) != 0
    }
    fn union(&mut self, o: &Bits) {
        for (a, b) in self.0.iter_mut().zip(&o.0) {
            *a |= b;
        }
    }
}

/// The member list of a set of structurally valid ops. See the module docs.
pub fn compute(ops: &BTreeMap<String, MemberOp>) -> MemberState {
    let order = topo_order(ops);
    let n = order.len();
    let index: BTreeMap<&String, usize> = order.iter().enumerate().map(|(i, h)| (h, i)).collect();
    let op_at: Vec<&MemberOp> = order.iter().map(|h| &ops[h]).collect();
    // past[i]: strict ancestors of op i.
    let mut past: Vec<Bits> = Vec::with_capacity(n);
    for op in &op_at {
        let mut b = Bits::new(n);
        for p in &op.parents {
            if let Some(&j) = index.get(p) {
                b.set(j);
                let pj = past[j].clone();
                b.union(&pj);
            }
        }
        past.push(b);
    }
    let mut void = vec![false; n];
    let mut valid = vec![false; n];
    loop {
        valid.iter_mut().for_each(|v| *v = false);
        // Valid adds and removes per device id, in order.
        let mut adds: BTreeMap<&str, Vec<usize>> = BTreeMap::new();
        let mut removes: BTreeMap<&str, Vec<usize>> = BTreeMap::new();
        let mut removed_keys: Vec<(usize, &str)> = vec![];
        for i in 0..n {
            if void[i] {
                continue;
            }
            let op = op_at[i];
            let member_in_past = |d: &str| -> bool {
                adds.get(d)
                    .is_some_and(|v| v.iter().any(|&a| past[i].has(a)))
                    && !removes
                        .get(d)
                        .is_some_and(|v| v.iter().any(|&r| past[i].has(r)))
            };
            let ok = if op.parents.is_empty() {
                // Genesis (structure already checked: exactly one).
                true
            } else {
                member_in_past(&op.author)
                    && match &op.body {
                        OpBody::Add { device, key_id } => {
                            key_id.is_none()
                                && !removes
                                    .get(device.id.as_str())
                                    .is_some_and(|v| v.iter().any(|&r| past[i].has(r)))
                                && !removed_keys
                                    .iter()
                                    .any(|(r, k)| past[i].has(*r) && *k == device.keys.sign)
                                && !(member_in_past(&device.id)
                                    && current_key(&adds, &past[i], &op_at, &device.id)
                                        .is_some_and(|k| k != device.keys.sign))
                        }
                        OpBody::Remove { device_id, .. } => member_in_past(device_id),
                        OpBody::Rotate { .. } => true,
                    }
            };
            if ok {
                valid[i] = true;
                match &op.body {
                    OpBody::Add { device, .. } => adds.entry(&device.id).or_default().push(i),
                    OpBody::Remove { device_id, .. } => {
                        removes.entry(device_id).or_default().push(i);
                        for &a in adds.get(device_id.as_str()).into_iter().flatten() {
                            if let OpBody::Add { device, .. } = &op_at[a].body {
                                removed_keys.push((i, &device.keys.sign));
                            }
                        }
                    }
                    OpBody::Rotate { .. } => {}
                }
            }
        }
        // Removal wins: in ascending hash order, a valid removal voids its
        // target's ops outside its causal past.
        let mut next = void.clone();
        let mut removals: Vec<usize> = (0..n)
            .filter(|&i| valid[i] && matches!(op_at[i].body, OpBody::Remove { .. }))
            .collect();
        removals.sort_by(|a, b| order[*a].cmp(&order[*b]));
        for r in removals {
            if next[r] {
                continue;
            }
            let OpBody::Remove { device_id, .. } = &op_at[r].body else {
                continue;
            };
            for j in 0..n {
                if j != r && op_at[j].author == *device_id && !past[r].has(j) {
                    next[j] = true;
                }
            }
        }
        if next == void {
            break;
        }
        void = next;
    }
    let mut state = MemberState::default();
    for i in 0..n {
        if !valid[i] {
            continue;
        }
        let op = op_at[i];
        state.valid.insert(order[i].clone());
        match &op.body {
            OpBody::Add { device, .. } => {
                if !state.removed.contains(&device.id) {
                    state.members.insert(device.id.clone(), device.clone());
                }
            }
            OpBody::Remove { device_id, .. } => {
                state.members.remove(device_id);
                state.removed.insert(device_id.clone());
            }
            OpBody::Rotate { .. } => {}
        }
        if let Some(k) = op.key_commit() {
            state.key_id = Some(k.to_string());
        }
    }
    state
}

/// The signing key a device was added with in `past` (the first add).
fn current_key<'a>(
    adds: &BTreeMap<&str, Vec<usize>>,
    past: &Bits,
    ops: &[&'a MemberOp],
    device: &str,
) -> Option<&'a str> {
    adds.get(device)?
        .iter()
        .find(|&&a| past.has(a))
        .and_then(|&a| match &ops[a].body {
            OpBody::Add { device, .. } => Some(device.keys.sign.as_str()),
            _ => None,
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    pub(crate) struct Dev {
        pub id: String,
        pub secret: DeviceSecret,
    }

    impl Dev {
        pub fn new(id: &str) -> Dev {
            Dev {
                id: id.into(),
                secret: DeviceSecret::from_seed(id.as_bytes()),
            }
        }
        pub fn member(&self) -> MemberDevice {
            MemberDevice {
                id: self.id.clone(),
                name: self.id.clone(),
                keys: self.secret.public(),
            }
        }
    }

    fn genesis(a: &Dev) -> MemberLog {
        MemberLog::genesis("ident", a.member(), "k0".into(), 0.0, &a.secret)
    }

    fn add(log: &mut MemberLog, by: &Dev, d: &Dev) -> MemberOp {
        log.append(
            &by.id,
            1.0,
            OpBody::Add {
                device: d.member(),
                key_id: None,
            },
            &by.secret,
        )
    }

    fn remove(log: &mut MemberLog, by: &Dev, d: &str, key: &str) -> MemberOp {
        log.append(
            &by.id,
            2.0,
            OpBody::Remove {
                device_id: d.into(),
                key_id: key.into(),
            },
            &by.secret,
        )
    }

    fn members(log: &mut MemberLog) -> Vec<String> {
        log.state().members.keys().cloned().collect()
    }

    #[test]
    fn genesis_add_remove() {
        let (a, b, c) = (Dev::new("a"), Dev::new("b"), Dev::new("c"));
        let mut log = genesis(&a);
        assert_eq!(members(&mut log), ["a"]);
        assert_eq!(log.state().key_id.as_deref(), Some("k0"));
        add(&mut log, &a, &b);
        add(&mut log, &b, &c);
        assert_eq!(members(&mut log), ["a", "b", "c"]);
        // The first device can leave; the others carry on.
        remove(&mut log, &b, "a", "k1");
        assert_eq!(members(&mut log), ["b", "c"]);
        assert_eq!(log.state().key_id.as_deref(), Some("k1"));
        // a can no longer do anything.
        let before = log.state().clone();
        add(&mut log, &a, &Dev::new("d"));
        assert_eq!(log.state().members, before.members);
    }

    #[test]
    fn forged_and_foreign_ops_are_dropped() {
        let (a, b) = (Dev::new("a"), Dev::new("b"));
        let mut log = genesis(&a);
        let mut other = log.clone();
        // An op signed by a key that is not the author's.
        let mut forged = MemberOp::new(
            "ident",
            "a",
            log.heads(),
            1.0,
            OpBody::Add {
                device: b.member(),
                key_id: None,
            },
            &b.secret,
        );
        assert_eq!(log.merge(vec![forged.clone()]).unwrap(), 0);
        forged.identity = "other".into();
        assert_eq!(log.merge(vec![forged]), Err(LogError::WrongIdentity));
        // A second genesis.
        let g2 = MemberLog::genesis("ident", b.member(), "kx".into(), 0.0, &b.secret);
        assert_eq!(log.merge(g2.ops()).unwrap(), 0);
        // Children-first still merges.
        add(&mut other, &a, &b);
        let c = Dev::new("c");
        add(&mut other, &b, &c);
        let mut ops = other.ops();
        ops.reverse();
        assert_eq!(log.merge(ops).unwrap(), 2);
        assert_eq!(members(&mut log), ["a", "b", "c"]);
    }

    #[test]
    fn concurrent_mutual_removal_lowest_hash_wins() {
        let (a, b, c) = (Dev::new("a"), Dev::new("b"), Dev::new("c"));
        let mut base = genesis(&a);
        add(&mut base, &a, &b);
        add(&mut base, &a, &c);
        let mut la = base.clone();
        let mut lb = base.clone();
        let ra = remove(&mut la, &a, "b", "ka");
        let rb = remove(&mut lb, &b, "a", "kb");
        la.merge(lb.ops()).unwrap();
        lb.merge(la.ops()).unwrap();
        let sa = la.state().clone();
        assert_eq!(sa, *lb.state());
        let (winner, loser) = if ra.hash() < rb.hash() {
            ("a", "b")
        } else {
            ("b", "a")
        };
        assert!(sa.is_member(winner));
        assert!(!sa.is_member(loser));
        assert!(sa.is_member("c"));
    }

    #[test]
    fn removed_device_offline_ops_are_void() {
        let (a, b, c) = (Dev::new("a"), Dev::new("b"), Dev::new("c"));
        let mut base = genesis(&a);
        add(&mut base, &a, &b);
        add(&mut base, &a, &c);
        // b removes a while a is offline; b also adds more on top first.
        let mut lb = base.clone();
        let e = Dev::new("e");
        add(&mut lb, &b, &e);
        remove(&mut lb, &b, "a", "k1");
        // a, offline and unaware, adds an accomplice who adds another.
        let mut la = base.clone();
        let d = Dev::new("d");
        add(&mut la, &a, &d);
        add(&mut la, &d, &Dev::new("f"));
        lb.merge(la.ops()).unwrap();
        assert_eq!(members(&mut lb), ["b", "c", "e"]);
    }

    #[test]
    fn removed_keys_are_never_readded() {
        let (a, b) = (Dev::new("a"), Dev::new("b"));
        let mut log = genesis(&a);
        add(&mut log, &a, &b);
        remove(&mut log, &a, "b", "k1");
        add(&mut log, &a, &b);
        assert_eq!(members(&mut log), ["a"]);
    }

    #[test]
    fn missing_for_sends_only_what_the_peer_lacks() {
        let (a, b, c) = (Dev::new("a"), Dev::new("b"), Dev::new("c"));
        let mut log = genesis(&a);
        add(&mut log, &a, &b);
        let old = log.heads();
        add(&mut log, &a, &c);
        assert_eq!(log.missing_for(&old).len(), 1);
        assert_eq!(log.missing_for(&[]).len(), 3);
        assert!(log.missing_for(&log.heads()).is_empty());
    }

    /// One step a device takes in the random history.
    #[derive(Debug, Clone)]
    enum Step {
        Add { by: usize },
        Remove { by: usize, target: usize },
        Sync { from: usize, to: usize },
    }

    fn step() -> impl Strategy<Value = Step> {
        prop_oneof![
            (0..4usize).prop_map(|by| Step::Add { by }),
            (0..4usize, 0..8usize).prop_map(|(by, target)| Step::Remove { by, target }),
            (0..4usize, 0..4usize).prop_map(|(from, to)| Step::Sync { from, to }),
        ]
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(64))]

        /// Four devices act on their own copies and sync in random order:
        /// once everything is exchanged, every copy (merged in any order)
        /// holds the same members and key.
        #[test]
        fn logs_converge(steps in proptest::collection::vec(step(), 1..40), seed in 0u64..1000) {
            let devs: Vec<Dev> = (0..8).map(|i| Dev::new(&format!("d{i}"))).collect();
            let mut base = genesis(&devs[0]);
            for d in &devs[1..4] {
                add(&mut base, &devs[0], d);
            }
            let mut logs: Vec<MemberLog> = vec![base; 4];
            let mut next_new = 4;
            for (n, s) in steps.iter().enumerate() {
                match *s {
                    Step::Add { by } if next_new < devs.len() => {
                        add(&mut logs[by], &devs[by], &devs[next_new]);
                        next_new += 1;
                    }
                    Step::Add { .. } => {}
                    Step::Remove { by, target } => {
                        let t = devs[target].id.clone();
                        remove(&mut logs[by], &devs[by], &t, &format!("k{n}"));
                    }
                    Step::Sync { from, to } => {
                        let ops = logs[from].ops();
                        logs[to].merge(ops).unwrap();
                    }
                }
            }
            // Everyone gets everything, in different orders.
            let all: Vec<MemberOp> = logs.iter().flat_map(|l| l.ops()).collect();
            let mut results = vec![];
            for (i, l) in logs.iter_mut().enumerate() {
                let mut ops = all.clone();
                let k = (seed as usize + i * 7) % ops.len().max(1);
                ops.rotate_left(k);
                if i % 2 == 1 { ops.reverse(); }
                l.merge(ops).unwrap();
                results.push(l.state().clone());
            }
            for r in &results[1..] {
                prop_assert_eq!(&results[0], r);
            }
        }

        /// Two devices removing each other at the same moment, with any
        /// amount of unrelated history on either side, settle the same way
        /// everywhere: exactly one of them stays.
        #[test]
        fn mutual_removal_settles_one_way(extra_a in 0usize..3, extra_b in 0usize..3) {
            let devs: Vec<Dev> = (0..9).map(|i| Dev::new(&format!("m{i}"))).collect();
            let mut base = genesis(&devs[0]);
            add(&mut base, &devs[0], &devs[1]);
            add(&mut base, &devs[0], &devs[2]);
            let mut la = base.clone();
            let mut lb = base.clone();
            for i in 0..extra_a { add(&mut la, &devs[0], &devs[3 + i]); }
            for i in 0..extra_b { add(&mut lb, &devs[1], &devs[6 + i]); }
            remove(&mut la, &devs[0], "m1", "ka");
            remove(&mut lb, &devs[1], "m0", "kb");
            let mut lc = base.clone();
            lc.merge(lb.ops()).unwrap();
            lc.merge(la.ops()).unwrap();
            la.merge(lb.ops()).unwrap();
            lb.merge(la.ops()).unwrap();
            let s = la.state().clone();
            prop_assert_eq!(&s, lb.state());
            prop_assert_eq!(&s, lc.state());
            prop_assert!(s.is_member("m0") != s.is_member("m1"));
            prop_assert!(s.is_member("m2"));
        }
    }
}
