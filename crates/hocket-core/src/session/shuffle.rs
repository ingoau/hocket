//! Shuffle as a permutation over the context.
//!
//! The permutation is derived deterministically from `(seed, n)` with a
//! Fisher–Yates shuffle driven by ChaCha8, so the same seed produces the same
//! order on every platform and only 4 bytes travel over the wire. The random
//! index for each step is taken straight from the generator's raw 32-bit
//! output (`next_u32() % (i + 1)`) rather than through `rand`'s range
//! sampling, so the order cannot change under a `rand` upgrade. The modulo
//! bias is on the order of `n / 2^32` and irrelevant for a queue.
//!
//! Two refinements live in [`api::ShuffleState`]:
//! - `anchor`: the unshuffled index that must sit at permuted position 0 (the
//!   item that was current when shuffle was enabled). It is swapped to the
//!   front after the Fisher–Yates pass.
//! - `order`: an explicit permutation, materialised only after a structural
//!   edit while shuffled. When present and valid it wins over the seed.

use rand::{RngCore, SeedableRng};
use rand_chacha::ChaCha8Rng;

use crate::api::ShuffleState;

/// A bijection between permuted positions and unshuffled context indices.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Permutation {
    /// `order[position] = context index`.
    order: Vec<u32>,
    /// `inverse[context index] = position`.
    inverse: Vec<u32>,
}

impl Permutation {
    /// Identity permutation (shuffle off).
    pub fn identity(n: usize) -> Permutation {
        let order: Vec<u32> = (0..n as u32).collect();
        Permutation { inverse: order.clone(), order }
    }

    /// Deterministic seeded permutation with an optional anchor moved to the front.
    pub fn seeded(seed: u32, n: usize, anchor: Option<u32>) -> Permutation {
        let mut order = seeded_order(seed, n);
        if let Some(a) = anchor {
            if (a as usize) < n {
                let pos = order.iter().position(|&x| x == a).unwrap_or(0);
                order.swap(0, pos);
            }
        }
        Permutation::from_order(order).unwrap_or_else(|| Permutation::identity(n))
    }

    /// Build from an explicit order. `None` unless it is a bijection over `0..n`.
    pub fn from_order(order: Vec<u32>) -> Option<Permutation> {
        if !is_bijection(&order) {
            return None;
        }
        let mut inverse = vec![0u32; order.len()];
        for (pos, &idx) in order.iter().enumerate() {
            inverse[idx as usize] = pos as u32;
        }
        Some(Permutation { order, inverse })
    }

    /// The permutation a document state describes for a context of `n` tracks.
    /// `None` state means shuffle is off (identity). An invalid explicit
    /// `order` (wrong length, not a bijection) falls back to the seed.
    pub fn from_state(state: Option<&ShuffleState>, n: usize) -> Permutation {
        match state {
            None => Permutation::identity(n),
            Some(s) => {
                if let Some(order) = &s.order {
                    if order.len() == n {
                        if let Some(p) = Permutation::from_order(order.clone()) {
                            return p;
                        }
                    }
                }
                Permutation::seeded(s.seed, n, s.anchor)
            }
        }
    }

    pub fn len(&self) -> usize {
        self.order.len()
    }

    pub fn is_empty(&self) -> bool {
        self.order.is_empty()
    }

    /// Context index at a permuted position.
    pub fn to_context(&self, position: u32) -> Option<u32> {
        self.order.get(position as usize).copied()
    }

    /// Permuted position of a context index.
    pub fn to_position(&self, index: u32) -> Option<u32> {
        self.inverse.get(index as usize).copied()
    }

    /// The full order, `order[position] = index`.
    pub fn order(&self) -> &[u32] {
        &self.order
    }

    pub fn into_order(self) -> Vec<u32> {
        self.order
    }

    /// Whether this is the identity.
    pub fn is_identity(&self) -> bool {
        self.order.iter().enumerate().all(|(p, &i)| p as u32 == i)
    }
}

fn seeded_order(seed: u32, n: usize) -> Vec<u32> {
    let mut order: Vec<u32> = (0..n as u32).collect();
    if n < 2 {
        return order;
    }
    let mut rng = ChaCha8Rng::seed_from_u64(seed as u64);
    // Fisher–Yates, walking down from the end.
    for i in (1..n).rev() {
        let j = (rng.next_u32() as usize) % (i + 1);
        order.swap(i, j);
    }
    order
}

/// True when `order` contains every index in `0..order.len()` exactly once.
pub fn is_bijection(order: &[u32]) -> bool {
    let n = order.len();
    let mut seen = vec![false; n];
    for &i in order {
        let i = i as usize;
        if i >= n || seen[i] {
            return false;
        }
        seen[i] = true;
    }
    true
}

/// Remove the context index `index` from an explicit order, renumbering the
/// indices above it. Returns the position it occupied, if any.
pub fn order_remove_index(order: &mut Vec<u32>, index: u32) -> Option<usize> {
    let pos = order.iter().position(|&i| i == index)?;
    order.remove(pos);
    for i in order.iter_mut() {
        if *i > index {
            *i -= 1;
        }
    }
    Some(pos)
}

/// Insert a new context index at unshuffled index `index` (renumbering the
/// ones at or above it) and place it at permuted position `position`.
pub fn order_insert_index(order: &mut Vec<u32>, index: u32, position: usize) {
    for i in order.iter_mut() {
        if *i >= index {
            *i += 1;
        }
    }
    let position = position.min(order.len());
    order.insert(position, index);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_seed_same_order() {
        let a = Permutation::seeded(42, 50, None);
        let b = Permutation::seeded(42, 50, None);
        assert_eq!(a, b);
        assert!(is_bijection(a.order()));
        assert_ne!(a, Permutation::seeded(43, 50, None));
    }

    #[test]
    fn golden_order_is_stable_across_platforms() {
        // Pinned output for (seed 7, n 10). If this changes, every synced
        // document that carries seed 7 would play in a different order on the
        // upgraded device, so treat a failure here as a wire-format break.
        let p = Permutation::seeded(7, 10, None);
        assert_eq!(p.order(), &[3, 6, 5, 7, 2, 8, 1, 0, 4, 9]);
    }

    #[test]
    fn anchor_is_first_and_rest_unchanged() {
        let plain = Permutation::seeded(7, 10, None);
        let anchored = Permutation::seeded(7, 10, Some(9));
        assert_eq!(anchored.to_context(0), Some(9));
        assert_eq!(anchored.to_position(9), Some(0));
        // Only two positions differ: the swap.
        let diffs = plain.order().iter().zip(anchored.order()).filter(|(a, b)| a != b).count();
        assert_eq!(diffs, 2);
        // Out-of-range anchor is ignored.
        assert_eq!(Permutation::seeded(7, 10, Some(99)), plain);
    }

    #[test]
    fn inverse_round_trips() {
        let p = Permutation::seeded(99, 37, Some(4));
        for pos in 0..37u32 {
            let idx = p.to_context(pos).unwrap();
            assert_eq!(p.to_position(idx), Some(pos));
        }
        assert_eq!(p.to_context(37), None);
        assert_eq!(p.to_position(37), None);
    }

    #[test]
    fn small_sizes() {
        assert_eq!(Permutation::seeded(1, 0, None).len(), 0);
        assert_eq!(Permutation::seeded(1, 1, None).order(), &[0]);
        assert!(Permutation::identity(3).is_identity());
        assert!(Permutation::from_order(vec![0, 0, 1]).is_none());
        assert!(Permutation::from_order(vec![0, 3, 1]).is_none());
    }

    #[test]
    fn explicit_order_wins_when_valid() {
        let s = ShuffleState { seed: 1, anchor: None, order: Some(vec![2, 0, 1]) };
        assert_eq!(Permutation::from_state(Some(&s), 3).order(), &[2, 0, 1]);
        // Wrong length → seed.
        assert_eq!(Permutation::from_state(Some(&s), 4), Permutation::seeded(1, 4, None));
        // Not a bijection → seed.
        let bad = ShuffleState { seed: 1, anchor: None, order: Some(vec![1, 1, 0]) };
        assert_eq!(Permutation::from_state(Some(&bad), 3), Permutation::seeded(1, 3, None));
    }

    #[test]
    fn order_edits() {
        let mut o = vec![2, 0, 3, 1];
        assert_eq!(order_remove_index(&mut o, 2), Some(0));
        assert_eq!(o, vec![0, 2, 1]);
        assert!(is_bijection(&o));
        order_insert_index(&mut o, 1, 1);
        // Old indices 1,2 became 2,3; new index 1 at position 1.
        assert_eq!(o, vec![0, 1, 3, 2]);
        assert!(is_bijection(&o));
        order_insert_index(&mut o, 4, 100);
        assert_eq!(o, vec![0, 1, 3, 2, 4]);
        assert_eq!(order_remove_index(&mut o, 9), None);
    }
}
