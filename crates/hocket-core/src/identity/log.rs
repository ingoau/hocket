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
//! 4. **Contested removals.** Two removals of each other's authors, neither
//!    in the other's causal past (two devices removing each other at the
//!    same moment, or a removed device signing a removal of its remover
//!    from before its own removal), both stand: both devices are out, and
//!    neither removal's new key is used ([`MemberState::rekey`] asks a
//!    remaining member to rotate). A removed device can never undo its
//!    removal. A removal only acts while no other removal voids it, so a
//!    void removal voids nothing; a cycle of three or more falls back to the
//!    lowest op hash.
//! 5. **Group key.** Genesis, `Remove` and `Rotate` commit to a new group
//!    key (`keyId`, the hash of the key). The current key is the one
//!    committed by the last valid such op in the log's deterministic
//!    topological order (parents first, ties by lowest hash).
//! 6. **Storage.** Void ops by a removed device that nothing kept builds on
//!    are dropped on merge, so a removed device can't fill the log.
//!
//! Voiding is monotonic (a voided op stays void), so the fixpoint in
//! [`compute`] terminates, and it depends only on the set of ops.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use serde::{Deserialize, Serialize};

use crate::api::{DeviceId, EpochMs};
use crate::identity::crypto::{self, Canon, DeviceSecret, PublicKeys};
use crate::identity::COORDINATOR_ID;

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
    /// The current key predates a contested removal (both removed devices
    /// may hold it): a remaining member should rotate.
    pub rekey: bool,
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
    /// Ops [`MemberLog::prune`] dropped, with their parents: still known
    /// (a peer's head may be one, and an op built on one before its author
    /// was removed still arrives), and passed through for ancestry.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pruned: BTreeMap<String, Vec<String>>,
    #[serde(skip)]
    state: Option<MemberState>,
}

/// Most pruned ops remembered.
pub const MAX_PRUNED: usize = 4 * MAX_OPS;

/// `parents` with pruned ones replaced by theirs (transitively), so
/// ancestry passes through what was pruned.
fn resolve_parents(
    parents: &[String],
    ops: &BTreeMap<String, MemberOp>,
    pruned: &BTreeMap<String, Vec<String>>,
) -> Vec<String> {
    if parents.iter().all(|p| ops.contains_key(p)) {
        return parents.to_vec();
    }
    if pruned.is_empty() {
        return parents
            .iter()
            .filter(|p| ops.contains_key(*p))
            .cloned()
            .collect();
    }
    let mut out = vec![];
    let mut seen = HashSet::new();
    let mut stack: Vec<&String> = parents.iter().collect();
    while let Some(p) = stack.pop() {
        if !seen.insert(p) {
            continue;
        }
        if ops.contains_key(p) {
            out.push(p.clone());
        } else if let Some(pp) = pruned.get(p) {
            stack.extend(pp.iter());
        }
    }
    out.sort();
    out.dedup();
    out
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
            pruned: BTreeMap::new(),
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
        topo_order(&self.ops, &self.pruned)
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

    /// Whether every op in `heads` is already here (or was, and was
    /// pruned).
    pub fn knows(&self, heads: &[String]) -> bool {
        heads
            .iter()
            .all(|h| self.ops.contains_key(h) || self.pruned.contains_key(h))
    }

    /// The ops a peer whose heads are `theirs` is missing: everything not in
    /// the causal past of the heads of theirs we know, parents first. A head
    /// we don't know is ignored (they are ahead there, and pull nothing).
    pub fn missing_for(&self, theirs: &[String]) -> Vec<MemberOp> {
        let mut known: BTreeSet<String> = BTreeSet::new();
        let mut stack: Vec<String> = theirs
            .iter()
            .filter(|h| self.ops.contains_key(*h) || self.pruned.contains_key(*h))
            .cloned()
            .collect();
        while let Some(h) = stack.pop() {
            if !known.insert(h.clone()) {
                continue;
            }
            if let Some(op) = self.ops.get(&h) {
                stack.extend(op.parents.iter().cloned());
            } else if let Some(pp) = self.pruned.get(&h) {
                stack.extend(pp.iter().cloned());
            }
        }
        self.ops()
            .into_iter()
            .filter(|o| !known.contains(&o.hash()))
            .collect()
    }

    /// Union in ops from a peer. Ops that fail the structural checks (bad
    /// signature, unknown parents, a second genesis) are dropped; the rest
    /// are kept whether or not they turn out valid, except void ops by a
    /// removed device that nothing kept builds on (see [`MemberLog::prune`]).
    /// A batch of more than [`MAX_OPS`] ops is refused before any work, and
    /// a merge that would leave more than [`MAX_OPS`] ops is undone.
    /// Returns how many ops the log changed by (new ones kept plus old ones
    /// pruned).
    pub fn merge(&mut self, ops: Vec<MemberOp>) -> Result<usize, LogError> {
        if ops.len() > MAX_OPS {
            return Err(LogError::Full);
        }
        let mut pending: BTreeMap<String, MemberOp> = BTreeMap::new();
        for op in ops {
            if op.identity != self.identity {
                return Err(LogError::WrongIdentity);
            }
            let h = op.hash();
            if !self.ops.contains_key(&h) {
                pending.entry(h).or_insert(op);
            }
        }
        if pending.is_empty() {
            return Ok(0);
        }
        // Parents first, so one pass decides every op (children of a dropped
        // op have a missing parent and are dropped too).
        let mut inserted: Vec<String> = vec![];
        let mut walk = AuthorWalk::new(&self.ops);
        for h in topo_order(&pending, &BTreeMap::new()) {
            let op = pending.remove(&h).expect("ordered from the map");
            if self.structurally_ok(&op, &h, &mut walk) == Some(true) {
                walk.inserted(&h, &op);
                self.ops.insert(h.clone(), op);
                inserted.push(h);
            }
        }
        if inserted.is_empty() {
            return Ok(0);
        }
        self.state = None;
        let pruned = self.prune();
        let new: HashSet<&String> = inserted.iter().collect();
        if self.ops.len() > MAX_OPS {
            for h in &inserted {
                self.ops.remove(h);
            }
            for (h, op) in pruned {
                self.pruned.remove(&h);
                if !new.contains(&h) {
                    self.ops.insert(h, op);
                }
            }
            self.state = None;
            return Err(LogError::Full);
        }
        for h in &inserted {
            if self.ops.contains_key(h) {
                self.pruned.remove(h);
            }
        }
        while self.pruned.len() > MAX_PRUNED {
            self.pruned.pop_first();
        }
        let kept = inserted
            .iter()
            .filter(|h| self.ops.contains_key(*h))
            .count();
        let pruned_old = pruned.iter().filter(|(h, _)| !new.contains(h)).count();
        Ok(kept + pruned_old)
    }

    /// Drop void ops by removed devices that no kept op builds on: what a
    /// removed device signed after (or concurrently with) its removal never
    /// counts, so it isn't stored either and can't fill the log. Repeats
    /// until nothing more goes (dropping one can change what is void).
    /// Returns what was dropped.
    fn prune(&mut self) -> Vec<(String, MemberOp)> {
        let mut dropped = vec![];
        loop {
            let state = compute(&self.ops, &self.pruned);
            let order = topo_order(&self.ops, &self.pruned);
            let mut needed: BTreeSet<&String> = BTreeSet::new();
            let mut drop: Vec<String> = vec![];
            for h in order.iter().rev() {
                let op = &self.ops[h];
                let prunable = !state.valid.contains(h) && state.removed.contains(&op.author);
                if prunable && !needed.contains(h) {
                    drop.push(h.clone());
                } else {
                    needed.extend(op.parents.iter());
                }
            }
            if drop.is_empty() {
                self.state = Some(state);
                return dropped;
            }
            for h in drop {
                if let Some(op) = self.ops.remove(&h) {
                    self.pruned.insert(h.clone(), op.parents.clone());
                    dropped.push((h, op));
                }
            }
        }
    }

    /// `Some(ok)` when decidable now; `None` while a parent is missing.
    fn structurally_ok(&self, op: &MemberOp, hash: &str, walk: &mut AuthorWalk) -> Option<bool> {
        if op.parents.len() > MAX_PARENTS
            || op.parents.windows(2).any(|w| w[0] >= w[1])
            || op.author.is_empty()
            || !crypto::is_lower_hex(&op.sig)
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
        if op
            .parents
            .iter()
            .any(|p| !self.ops.contains_key(p) && !self.pruned.contains_key(p))
        {
            return None;
        }
        // The author's key comes from an Add in the op's causal past.
        Some(walk.signed_in_past(self, op, hash))
    }

    /// The member list (computed once per change).
    pub fn state(&mut self) -> &MemberState {
        if self.state.is_none() {
            self.state = Some(compute(&self.ops, &self.pruned));
        }
        self.state.as_ref().expect("just computed")
    }

    /// The member list without caching (for a shared reference).
    pub fn compute_state(&self) -> MemberState {
        match &self.state {
            Some(s) => s.clone(),
            None => compute(&self.ops, &self.pruned),
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

/// What a merge knows about authors' keys: every `Add` per device id, and
/// per `Add` the ops seen to have it in their past, so a chain of ops by
/// one author is checked in constant time each, not by walking the log.
struct AuthorWalk {
    adds: HashMap<String, Vec<(String, String)>>,
    below: HashMap<String, HashSet<String>>,
}

impl AuthorWalk {
    fn new(ops: &BTreeMap<String, MemberOp>) -> AuthorWalk {
        let mut w = AuthorWalk {
            adds: HashMap::new(),
            below: HashMap::new(),
        };
        for (h, o) in ops {
            w.inserted(h, o);
        }
        w
    }

    fn inserted(&mut self, hash: &str, op: &MemberOp) {
        if let OpBody::Add { device, .. } = &op.body {
            self.adds
                .entry(device.id.clone())
                .or_default()
                .push((hash.to_string(), device.keys.sign.clone()));
        }
    }

    /// Whether an `Add` of `op`'s author in its causal past carries a key
    /// that verifies its signature.
    fn signed_in_past(&mut self, log: &MemberLog, op: &MemberOp, hash: &str) -> bool {
        let Some(adds) = self.adds.get(&op.author).cloned() else {
            return false;
        };
        let signed = op.signed_bytes();
        let mut verdicts: Vec<(&str, bool)> = vec![];
        for (add, key) in &adds {
            let good = match verdicts.iter().find(|(k, _)| *k == key) {
                Some((_, g)) => *g,
                None => {
                    let g = crypto::verify(key, &signed, &op.sig);
                    verdicts.push((key, g));
                    g
                }
            };
            if good && self.in_past(log, op, add) {
                self.below
                    .entry(add.clone())
                    .or_default()
                    .insert(hash.to_string());
                return true;
            }
        }
        false
    }

    /// Whether `add` is an ancestor of `op`: a parent is it or known to be
    /// below it, else a walk that stops at those.
    fn in_past(&self, log: &MemberLog, op: &MemberOp, add: &str) -> bool {
        let below = self.below.get(add);
        let hit = |h: &str| h == add || below.is_some_and(|b| b.contains(h));
        let mut seen: HashSet<&str> = HashSet::new();
        let mut stack: Vec<&String> = op.parents.iter().collect();
        while let Some(h) = stack.pop() {
            if hit(h) {
                return true;
            }
            if !seen.insert(h.as_str()) {
                continue;
            }
            if let Some(o) = log.ops.get(h) {
                stack.extend(o.parents.iter());
            } else if let Some(pp) = log.pruned.get(h) {
                stack.extend(pp.iter());
            }
        }
        false
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
fn topo_order(
    ops: &BTreeMap<String, MemberOp>,
    pruned: &BTreeMap<String, Vec<String>>,
) -> Vec<String> {
    let mut indegree: BTreeMap<&String, usize> = BTreeMap::new();
    let mut children: BTreeMap<&String, Vec<&String>> = BTreeMap::new();
    for (h, op) in ops {
        let parents = resolve_parents(&op.parents, ops, pruned);
        let n = parents.len();
        indegree.insert(h, n);
        for p in &parents {
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
pub fn compute(
    ops: &BTreeMap<String, MemberOp>,
    pruned: &BTreeMap<String, Vec<String>>,
) -> MemberState {
    let order = topo_order(ops, pruned);
    let n = order.len();
    let index: BTreeMap<&String, usize> = order.iter().enumerate().map(|(i, h)| (h, i)).collect();
    let op_at: Vec<&MemberOp> = order.iter().map(|h| &ops[h]).collect();
    // past[i]: strict ancestors of op i.
    let mut past: Vec<Bits> = Vec::with_capacity(n);
    for op in &op_at {
        let mut b = Bits::new(n);
        for p in &resolve_parents(&op.parents, ops, pruned) {
            if let Some(&j) = index.get(p) {
                b.set(j);
                let pj = past[j].clone();
                b.union(&pj);
            }
        }
        past.push(b);
    }
    // Where each device's first Add sits: lower is more senior (the
    // founding device is the most senior).
    let seniority = |d: &str| -> usize {
        (0..n)
            .find(|&i| matches!(&op_at[i].body, OpBody::Add { device, .. } if device.id == d))
            .unwrap_or(n)
    };
    // `losers`: removals that may not stand as contested (see below).
    let run = |losers: &[bool]| -> (Vec<bool>, Vec<bool>, BTreeMap<usize, usize>) {
        let mut void = vec![false; n];
        let mut valid = vec![false; n];
        let mut contested = vec![false; n];
        let mut partner: BTreeMap<usize, usize>;
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
                                    && device.id != COORDINATOR_ID
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
            // Removal wins: a valid removal voids its target's ops outside its
            // causal past. Two removals of each other's authors, neither in the
            // other's past, are *contested*: both stand (both devices are out)
            // and neither voids the other, so a removed device that signs a
            // removal of its remover from before its own removal only takes the
            // remover down with it; it never stays. Each removal pairs with at
            // most one contester (the lowest hash), so signing many changes
            // nothing.
            let removals: Vec<usize> = (0..n)
                .filter(|&i| valid[i] && matches!(op_at[i].body, OpBody::Remove { .. }))
                .collect();
            let target = |i: usize| match &op_at[i].body {
                OpBody::Remove { device_id, .. } => device_id.as_str(),
                _ => "",
            };
            let mutual = |r: usize, o: usize| {
                o != r
                    && target(o) == op_at[r].author
                    && target(r) == op_at[o].author
                    && !past[r].has(o)
                    && !past[o].has(r)
            };
            let partner_of = |r: usize| -> Option<usize> {
                if losers[r] {
                    return None;
                }
                removals
                    .iter()
                    .copied()
                    .filter(|&o| !losers[o] && mutual(r, o))
                    .min_by(|a, b| order[*a].cmp(&order[*b]))
            };
            partner = removals
                .iter()
                .filter_map(|&r| Some((r, partner_of(r)?)))
                .collect();
            contested = vec![false; n];
            for (&r, &o) in &partner {
                if partner.get(&o) == Some(&r) {
                    contested[r] = true;
                }
            }
            // A contested removal is never voided by a removal of its author
            // signed by its target (its partner, or further counter-removals,
            // which lose). A removal another removal voids is challenged and
            // waits: only unchallenged removals act this round, so a removal
            // that is itself void never voids anything. A cycle of three or
            // more (all challenged) falls back to the lowest hash.
            //
            // A contested removal is signed by a device its partner removes,
            // and it may be backdated: it voids only what its target did from
            // its partner on (never what the target did before removing its
            // author: earlier removals, adds, rotations stand). A loser voids
            // nothing.
            let partner_ref = &partner;
            let voids = |r: usize, j: usize| {
                !losers[r]
                    && j != r
                    && op_at[j].author == target(r)
                    && !past[r].has(j)
                    && !(contested[j] && mutual(r, j))
                    && !(contested[r] && partner_ref.get(&r).is_some_and(|&o| past[o].has(j)))
            };
            let challenged: Vec<bool> = (0..n)
                .map(|j| removals.iter().any(|&r| valid[j] && voids(r, j)))
                .collect();
            let apply = |rs: &[usize], next: &mut Vec<bool>| {
                for &r in rs {
                    for (j, v) in next.iter_mut().enumerate() {
                        if voids(r, j) {
                            *v = true;
                        }
                    }
                }
            };
            let mut next = void.clone();
            let free: Vec<usize> = removals
                .iter()
                .copied()
                .filter(|&r| !challenged[r])
                .collect();
            apply(&free, &mut next);
            if next == void {
                let mut rest: Vec<usize> = removals
                    .iter()
                    .copied()
                    .filter(|&r| challenged[r])
                    .collect();
                rest.sort_by(|a, b| order[*a].cmp(&order[*b]));
                apply(&rest[..rest.len().min(1)], &mut next);
            }
            if next == void {
                break;
            }
            void = next;
        }
        (valid, contested, partner)
    };
    // A contested pair removes both, unless that leaves nobody: then the
    // senior device of the pair stays (its removal loses), so a stolen
    // device can't brick a two-device identity, at the price that in one
    // the senior device always survives.
    let mut losers = vec![false; n];
    loop {
        let (valid, contested, partner) = run(&losers);
        let state = finish(&valid, &contested, &order, &op_at, &past);
        if !state.members.is_empty() {
            return state;
        }
        let target = |i: usize| match &op_at[i].body {
            OpBody::Remove { device_id, .. } => device_id.as_str(),
            _ => "",
        };
        let pick = (0..n)
            .filter(|&r| valid[r] && contested[r] && partner.contains_key(&r) && !losers[r])
            .min_by_key(|&r| (seniority(target(r)), order[r].clone()));
        match pick {
            Some(r) => losers[r] = true,
            None => return state,
        }
    }
}

/// The member list from the fixpoint's valid and contested ops.
fn finish(
    valid: &[bool],
    contested: &[bool],
    order: &[String],
    op_at: &[&MemberOp],
    past: &[Bits],
) -> MemberState {
    let n = order.len();
    let mut state = MemberState::default();
    let mut key_op: Option<usize> = None;
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
        // A contested removal's new key was made by a device that is now
        // out: it never becomes the group key.
        if let (Some(k), false) = (op.key_commit(), contested[i]) {
            state.key_id = Some(k.to_string());
            key_op = Some(i);
        }
    }
    // Every key committed before a contested removal is known to a device
    // it put out: someone still in has to rotate.
    state.rekey =
        (0..n).any(|c| contested[c] && valid[c] && key_op.is_none_or(|k| !past[k].has(c)));
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
    fn concurrent_mutual_removal_removes_both() {
        let (a, b, c) = (Dev::new("a"), Dev::new("b"), Dev::new("c"));
        let mut base = genesis(&a);
        add(&mut base, &a, &b);
        add(&mut base, &a, &c);
        let mut la = base.clone();
        let mut lb = base.clone();
        remove(&mut la, &a, "b", "ka");
        remove(&mut lb, &b, "a", "kb");
        la.merge(lb.ops()).unwrap();
        lb.merge(la.ops()).unwrap();
        let sa = la.state().clone();
        assert_eq!(sa, *lb.state());
        assert!(!sa.is_member("a"));
        assert!(!sa.is_member("b"));
        assert!(sa.is_member("c"));
        // Neither removed device's new key is used, and the one before is
        // known to both: c has to rotate.
        assert_eq!(sa.key_id.as_deref(), Some("k0"));
        assert!(sa.rekey);
        la.append(
            "c",
            3.0,
            OpBody::Rotate {
                key_id: "kc".into(),
            },
            &c.secret,
        );
        assert_eq!(la.state().key_id.as_deref(), Some("kc"));
        assert!(!la.state().rekey);
    }

    /// A removed device signs removals of its remover from before its own
    /// removal, with any `at` it likes: it never gets back in.
    #[test]
    fn removed_device_cannot_undo_its_removal() {
        let (a, b, c) = (Dev::new("a"), Dev::new("b"), Dev::new("c"));
        let mut base = genesis(&a);
        add(&mut base, &a, &b);
        add(&mut base, &a, &c);
        let mut la = base.clone();
        remove(&mut la, &a, "b", "ka");
        for at in 0..40 {
            let counter = MemberOp::new(
                "ident",
                "b",
                base.heads(),
                at as f64,
                OpBody::Remove {
                    device_id: "a".into(),
                    key_id: format!("kb{at}"),
                },
                &b.secret,
            );
            let mut l = la.clone();
            l.merge(vec![counter.clone()]).unwrap();
            let s = l.state().clone();
            assert!(!s.is_member("b"), "b got back in with at={at}");
            assert!(s.removed.contains("b"));
            assert!(s.is_member("c"));
            assert_ne!(s.key_id.as_deref(), Some(format!("kb{at}").as_str()));
            // A peer that saw the counter-removal first ends the same way.
            let mut other = base.clone();
            other.merge(vec![counter]).unwrap();
            other.merge(la.ops()).unwrap();
            assert_eq!(other.state(), &s);
        }
        // Many counter-removals at once: still out, and only one is kept.
        let counters: Vec<MemberOp> = (0..40)
            .map(|at| {
                MemberOp::new(
                    "ident",
                    "b",
                    base.heads(),
                    at as f64,
                    OpBody::Remove {
                        device_id: "a".into(),
                        key_id: format!("kb{at}"),
                    },
                    &b.secret,
                )
            })
            .collect();
        let before = la.len();
        la.merge(counters).unwrap();
        assert!(!la.state().is_member("b"));
        assert_eq!(la.len(), before + 1);
    }

    fn counter(base: &MemberLog, by: &Dev, target: &str, at: f64) -> MemberOp {
        MemberOp::new(
            "ident",
            &by.id,
            base.heads(),
            at,
            OpBody::Remove {
                device_id: target.into(),
                key_id: format!("k-{}-{at}", by.id),
            },
            &by.secret,
        )
    }

    /// A removes stolen B, then stolen D. D signs a removal of A from
    /// before both: it takes A out with it, but B's removal stands.
    #[test]
    fn a_contested_removal_never_undoes_its_targets_earlier_removals() {
        let (a, b, c, d) = (Dev::new("a"), Dev::new("b"), Dev::new("c"), Dev::new("d"));
        let mut base = genesis(&a);
        add(&mut base, &a, &b);
        add(&mut base, &a, &c);
        add(&mut base, &a, &d);
        let mut la = base.clone();
        remove(&mut la, &a, "b", "kab");
        remove(&mut la, &a, "d", "kad");
        let mut other = base.clone();
        let rda = counter(&base, &d, "a", 9.0);
        la.merge(vec![rda.clone()]).unwrap();
        other.merge(vec![rda]).unwrap();
        other.merge(la.ops()).unwrap();
        for log in [&mut la, &mut other] {
            let s = log.state().clone();
            assert!(!s.is_member("b"), "b stays out");
            assert!(!s.is_member("d") && !s.is_member("a"));
            assert!(s.is_member("c"));
            assert!(s.rekey, "c moves to a key neither a nor d holds");
        }
        assert_eq!(la.state(), other.state());
    }

    /// One removed device can't undo what its remover did before removing
    /// it: an add and a rotation stand.
    #[test]
    fn a_contested_removal_never_undoes_its_targets_earlier_ops() {
        let (a, c, d, e) = (Dev::new("a"), Dev::new("c"), Dev::new("d"), Dev::new("e"));
        let mut base = genesis(&a);
        add(&mut base, &a, &c);
        add(&mut base, &a, &d);
        let mut la = base.clone();
        add(&mut la, &a, &e);
        remove(&mut la, &a, "d", "kad");
        la.merge(vec![counter(&base, &d, "a", 3.0)]).unwrap();
        assert_eq!(members(&mut la), ["c", "e"]);
    }

    /// Two devices removing each other would leave nobody: the senior one
    /// (the founder here) stays, whichever of them is the thief.
    #[test]
    fn a_contested_pair_never_leaves_an_identity_empty() {
        let (a, d) = (Dev::new("a"), Dev::new("d"));
        let mut base = genesis(&a);
        add(&mut base, &a, &d);
        // a removes d; d backdates a removal of a.
        let mut la = base.clone();
        remove(&mut la, &a, "d", "kad");
        la.merge(vec![counter(&base, &d, "a", 3.0)]).unwrap();
        assert_eq!(members(&mut la), ["a"]);
        assert_eq!(la.state().key_id.as_deref(), Some("kad"));
        assert!(!la.state().rekey);
        // d removes a; a backdates a removal of d: a (senior) still stays.
        let mut ld = base.clone();
        remove(&mut ld, &d, "a", "kda");
        ld.merge(vec![counter(&base, &a, "d", 3.0)]).unwrap();
        assert_eq!(members(&mut ld), ["a"]);
    }

    /// A void op pruned here stays known (a peer's head may be it), and an
    /// op an honest device built on it before the removal still lands, and
    /// counts the same as where the pruned op was kept.
    #[test]
    fn pruned_ops_stay_known_and_their_children_land() {
        let (a, c, d) = (Dev::new("a"), Dev::new("c"), Dev::new("d"));
        let mut base = genesis(&a);
        add(&mut base, &a, &c);
        add(&mut base, &a, &d);
        // c saw d's op before d's removal and built on it.
        let mut lc = base.clone();
        let p = lc.append(
            "d",
            5.0,
            OpBody::Rotate {
                key_id: "kd".into(),
            },
            &d.secret,
        );
        let e = Dev::new("e");
        let child = add(&mut lc, &c, &e);
        // a removed d meanwhile; d's op reaches it alone and is pruned.
        let mut la = base.clone();
        remove(&mut la, &a, "d", "k1");
        let len = la.len();
        la.merge(vec![p.clone()]).unwrap();
        assert_eq!(la.len(), len);
        assert!(la.knows(&[p.hash()]));
        // Then c's op on top of it.
        assert_eq!(la.merge(vec![child]).unwrap(), 1);
        assert!(la.state().is_member("e"));
        let mut full = la.clone();
        full.pruned.clear();
        full.merge(lc.ops()).unwrap();
        lc.merge(la.ops()).unwrap();
        assert_eq!(la.state().members, lc.state().members);
        assert_eq!(la.state().key_id, lc.state().key_id);
    }

    #[test]
    fn signatures_have_one_spelling() {
        let (a, b) = (Dev::new("a"), Dev::new("b"));
        let mut log = genesis(&a);
        let mut other = log.clone();
        let op = add(&mut other, &a, &b);
        let mut upper = op.clone();
        upper.sig = upper.sig.to_uppercase();
        assert_ne!(upper.hash(), op.hash());
        assert_eq!(log.merge(vec![upper]).unwrap(), 0);
        assert_eq!(log.merge(vec![op]).unwrap(), 1);
        assert_eq!(log.len(), 2);
    }

    /// A removed device can't fill the log with ops it signs from before
    /// its removal: they are void, nothing builds on them, and they go.
    #[test]
    fn removed_device_cannot_fill_the_log() {
        let (a, b, c) = (Dev::new("a"), Dev::new("b"), Dev::new("c"));
        let mut base = genesis(&a);
        add(&mut base, &a, &b);
        let mut la = base.clone();
        remove(&mut la, &a, "b", "k1");
        let len = la.len();
        // More than MAX_OPS in all, in batches.
        let per = MAX_OPS / 3 + 1;
        for batch in 0..4 {
            let spam: Vec<MemberOp> = (0..per)
                .map(|i| {
                    MemberOp::new(
                        "ident",
                        "b",
                        base.heads(),
                        (batch * per + i) as f64,
                        OpBody::Rotate {
                            key_id: format!("x{i}"),
                        },
                        &b.secret,
                    )
                })
                .collect();
            la.merge(spam).unwrap();
            assert_eq!(la.len(), len);
        }
        add(&mut la, &a, &c);
        assert_eq!(members(&mut la), ["a", "c"]);
        // Too big a batch is refused before any work.
        let too_many = vec![la.ops()[0].clone(); MAX_OPS + 1];
        assert_eq!(la.merge(too_many), Err(LogError::Full));
    }

    #[test]
    fn the_coordinator_id_is_never_a_member() {
        let a = Dev::new("a");
        let mut log = genesis(&a);
        let fake = Dev::new(COORDINATOR_ID);
        add(&mut log, &a, &fake);
        assert_eq!(members(&mut log), ["a"]);
    }

    /// A long history arriving children-first merges in one pass.
    #[test]
    fn long_reversed_history_merges() {
        let a = Dev::new("a");
        let mut log = genesis(&a);
        let mut fresh = log.clone();
        for i in 0..600 {
            log.append(
                "a",
                i as f64,
                OpBody::Rotate {
                    key_id: format!("k{i}"),
                },
                &a.secret,
            );
        }
        let mut ops = log.ops();
        ops.reverse();
        assert_eq!(fresh.merge(ops).unwrap(), 600);
        assert_eq!(fresh.state().key_id.as_deref(), Some("k599"));
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
        /// everywhere: both are out.
        #[test]
        fn mutual_removal_removes_both(extra_a in 0usize..3, extra_b in 0usize..3) {
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
            prop_assert!(!s.is_member("m0") && !s.is_member("m1"));
            prop_assert!(s.rekey);
            prop_assert!(s.is_member("m2"));
        }
    }
}
