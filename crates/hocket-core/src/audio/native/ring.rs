//! Single-producer / single-consumer ring buffer of `f32` samples for the
//! hand-off between the engine thread (producer) and the output callback
//! (consumer). Lock-free: the callback never blocks.
//!
//! Capacity is rounded up to a power of two. One slot is kept empty to tell
//! full from empty, so `capacity()` is one less than allocated.

use std::cell::UnsafeCell;
use std::sync::atomic::{AtomicUsize, Ordering};

pub struct SpscRing {
    buf: Box<[UnsafeCell<f32>]>,
    mask: usize,
    head: AtomicUsize,
    tail: AtomicUsize,
}

// SAFETY: producer only writes slots in [tail, head) that the consumer cannot
// read until `tail` is published; consumer only reads [head, tail). Each
// index is written by exactly one side.
unsafe impl Send for SpscRing {}
unsafe impl Sync for SpscRing {}

impl SpscRing {
    pub fn new(min_capacity: usize) -> Self {
        let cap = (min_capacity + 1).next_power_of_two().max(2);
        let buf: Vec<UnsafeCell<f32>> = (0..cap).map(|_| UnsafeCell::new(0.0)).collect();
        Self { buf: buf.into_boxed_slice(), mask: cap - 1, head: AtomicUsize::new(0), tail: AtomicUsize::new(0) }
    }

    /// Usable capacity in samples.
    pub fn capacity(&self) -> usize {
        self.buf.len() - 1
    }

    pub fn len(&self) -> usize {
        let head = self.head.load(Ordering::Acquire);
        let tail = self.tail.load(Ordering::Acquire);
        tail.wrapping_sub(head) & self.mask
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn free(&self) -> usize {
        self.capacity() - self.len()
    }

    /// Producer: append as many samples as fit; returns how many were taken.
    pub fn push(&self, samples: &[f32]) -> usize {
        let head = self.head.load(Ordering::Acquire);
        let tail = self.tail.load(Ordering::Relaxed);
        let free = self.capacity() - (tail.wrapping_sub(head) & self.mask);
        let n = samples.len().min(free);
        for (i, &s) in samples[..n].iter().enumerate() {
            let idx = (tail + i) & self.mask;
            // SAFETY: slot is in the producer-owned region (see impl comment).
            unsafe { *self.buf[idx].get() = s };
        }
        self.tail.store(tail.wrapping_add(n) & self.mask, Ordering::Release);
        n
    }

    /// Consumer: fill `out` with up to `out.len()` samples; returns how many.
    pub fn pop(&self, out: &mut [f32]) -> usize {
        let tail = self.tail.load(Ordering::Acquire);
        let head = self.head.load(Ordering::Relaxed);
        let avail = tail.wrapping_sub(head) & self.mask;
        let n = out.len().min(avail);
        for (i, o) in out[..n].iter_mut().enumerate() {
            let idx = (head + i) & self.mask;
            // SAFETY: slot is in the consumer-owned region (see impl comment).
            *o = unsafe { *self.buf[idx].get() };
        }
        self.head.store(head.wrapping_add(n) & self.mask, Ordering::Release);
        n
    }

    /// Consumer: drop everything buffered. Only the consumer may call this
    /// (it moves `head`).
    pub fn discard_all(&self) -> usize {
        let tail = self.tail.load(Ordering::Acquire);
        let head = self.head.load(Ordering::Relaxed);
        let n = tail.wrapping_sub(head) & self.mask;
        self.head.store(tail, Ordering::Release);
        n
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn push_pop_round_trip_and_capacity() {
        let r = SpscRing::new(5);
        assert_eq!(r.capacity(), 7);
        assert_eq!(r.push(&[1.0, 2.0, 3.0]), 3);
        assert_eq!(r.len(), 3);
        let mut out = [0.0; 2];
        assert_eq!(r.pop(&mut out), 2);
        assert_eq!(out, [1.0, 2.0]);
        assert_eq!(r.push(&[4.0; 10]), 6, "fills to capacity");
        assert_eq!(r.free(), 0);
        let mut big = [0.0; 16];
        assert_eq!(r.pop(&mut big), 7);
        assert_eq!(big[0], 3.0);
        assert!(r.is_empty());
        r.push(&[9.0; 3]);
        assert_eq!(r.discard_all(), 3);
        assert!(r.is_empty());
    }

    #[test]
    fn wraps_correctly_across_many_cycles() {
        let r = SpscRing::new(64);
        let mut expected = 0.0f32;
        let mut produced = 0.0f32;
        let mut out = vec![0.0; 13];
        for _ in 0..2000 {
            let chunk: Vec<f32> = (0..7).map(|i| produced + i as f32).collect();
            let n = r.push(&chunk);
            produced += n as f32;
            let got = r.pop(&mut out);
            for v in &out[..got] {
                assert_eq!(*v, expected);
                expected += 1.0;
            }
        }
    }

    #[test]
    fn concurrent_producer_consumer_preserve_order() {
        let r = Arc::new(SpscRing::new(1024));
        let total = 200_000usize;
        let p = r.clone();
        let producer = std::thread::spawn(move || {
            let mut i = 0usize;
            while i < total {
                let end = (i + 100).min(total);
                let chunk: Vec<f32> = (i..end).map(|v| v as f32).collect();
                let n = p.push(&chunk);
                i += n;
                if n == 0 {
                    std::thread::yield_now();
                }
            }
        });
        let mut next = 0usize;
        let mut buf = vec![0.0; 77];
        while next < total {
            let n = r.pop(&mut buf);
            for v in &buf[..n] {
                assert_eq!(*v, next as f32);
                next += 1;
            }
            if n == 0 {
                std::thread::yield_now();
            }
        }
        producer.join().unwrap();
    }
}
