//! Virtual time. One [`SimTime`] per world; each device reads it through a
//! [`DeviceClock`] with its own skew, so clock sync has something to correct.

use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Arc;

use crate::util::Clock;

/// The world's clock, in milliseconds. Only the scheduler advances it.
#[derive(Debug, Default)]
pub struct SimTime {
    now: AtomicI64,
}

impl SimTime {
    pub fn new(start_ms: f64) -> Arc<Self> {
        Arc::new(SimTime { now: AtomicI64::new(start_ms as i64) })
    }

    pub fn now_ms(&self) -> f64 {
        self.now.load(Ordering::SeqCst) as f64
    }

    /// Move time forward. Never backwards.
    pub fn set(&self, now_ms: f64) {
        let target = now_ms as i64;
        let _ = self.now.fetch_max(target, Ordering::SeqCst);
    }

    pub fn advance(&self, delta_ms: f64) -> f64 {
        let n = self.now.fetch_add(delta_ms as i64, Ordering::SeqCst) + delta_ms as i64;
        n as f64
    }
}

impl Clock for SimTime {
    fn now_ms(&self) -> f64 {
        SimTime::now_ms(self)
    }
}

/// A device's view of the world clock: world time plus a fixed skew.
#[derive(Debug, Clone)]
pub struct DeviceClock {
    world: Arc<SimTime>,
    skew_ms: f64,
}

impl DeviceClock {
    pub fn new(world: Arc<SimTime>, skew_ms: f64) -> Self {
        DeviceClock { world, skew_ms }
    }

    pub fn skew_ms(&self) -> f64 {
        self.skew_ms
    }
}

impl Clock for DeviceClock {
    fn now_ms(&self) -> f64 {
        self.world.now_ms() + self.skew_ms
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn world_time_only_moves_forward_and_skew_applies() {
        let t = SimTime::new(1000.0);
        let d = DeviceClock::new(t.clone(), -250.0);
        assert_eq!(d.now_ms(), 750.0);
        t.advance(500.0);
        assert_eq!(t.now_ms(), 1500.0);
        t.set(1200.0);
        assert_eq!(t.now_ms(), 1500.0);
        assert_eq!(d.now_ms(), 1250.0);
    }
}
