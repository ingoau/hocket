//! Small shared helpers: clocks and ids.

use std::time::{SystemTime, UNIX_EPOCH};

/// Wall clock, epoch milliseconds. Prefer an injected [`Clock`] in subsystems
/// so the simulation harness can drive time.
pub fn now_ms() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as f64)
        .unwrap_or(0.0)
}

/// Injectable clock. The real one wraps the wall clock; the simulation harness
/// provides a virtual one.
pub trait Clock: Send + Sync + 'static {
    fn now_ms(&self) -> f64;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct WallClock;

impl Clock for WallClock {
    fn now_ms(&self) -> f64 {
        now_ms()
    }
}

pub fn new_id() -> String {
    uuid::Uuid::new_v4().simple().to_string()
}

/// A settable clock for tests.
#[derive(Debug, Default)]
pub struct FixedClock(parking_lot::Mutex<f64>);

impl FixedClock {
    pub fn new(now_ms: f64) -> FixedClock {
        FixedClock(parking_lot::Mutex::new(now_ms))
    }

    pub fn set(&self, now_ms: f64) {
        *self.0.lock() = now_ms;
    }

    pub fn advance(&self, ms: f64) {
        *self.0.lock() += ms;
    }
}

impl Clock for FixedClock {
    fn now_ms(&self) -> f64 {
        *self.0.lock()
    }
}
