//! Deterministic shuffling of a run of queue entries.
//!
//! A Fisher–Yates shuffle driven by ChaCha8 seeded from the op's entropy, so
//! every replica applying the same op produces the same order. The random
//! index for each step is taken straight from the generator's raw 32-bit
//! output (`next_u32() % (i + 1)`) rather than through `rand`'s range
//! sampling, so the order cannot change under a `rand` upgrade. The modulo
//! bias is on the order of `n / 2^32` and irrelevant for a queue.
//!
//! The unshuffled order is not kept here: every entry carries its own
//! `ordinal`, and turning shuffle off sorts by it (see the reducer).

use rand::{RngCore, SeedableRng};
use rand_chacha::ChaCha8Rng;

/// Shuffle `items` in place, deterministically from `seed`.
pub fn shuffle_in_place<T>(items: &mut [T], seed: u32) {
    let mut rng = ChaCha8Rng::seed_from_u64(seed as u64);
    for i in (1..items.len()).rev() {
        let j = (rng.next_u32() as usize) % (i + 1);
        items.swap(i, j);
    }
}

/// The order `shuffle_in_place` puts `0..n` in.
pub fn seeded_order(seed: u32, n: usize) -> Vec<u32> {
    let mut order: Vec<u32> = (0..n as u32).collect();
    shuffle_in_place(&mut order, seed);
    order
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deterministic_and_a_permutation() {
        let a = seeded_order(42, 50);
        let b = seeded_order(42, 50);
        assert_eq!(a, b);
        let mut sorted = a.clone();
        sorted.sort_unstable();
        assert_eq!(sorted, (0..50).collect::<Vec<u32>>());
        assert_ne!(a, seeded_order(43, 50));
    }

    #[test]
    fn small_inputs() {
        assert!(seeded_order(1, 0).is_empty());
        assert_eq!(seeded_order(1, 1), vec![0]);
    }
}
