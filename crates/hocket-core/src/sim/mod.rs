//! Deterministic simulation harness: virtual clock, in-memory network that
//! can partition, delay, drop and reorder on command, N simulated devices
//! each running a real [`crate::connect::Engine`] with the real session
//! reducer, and randomised multi-device scenarios with invariants asserted
//! throughout. Owner: core-connect.
//!
//! Enabled with the `sim` feature (and always in tests). The coordinator's
//! integration tests use it too.
//!
//! Entry points:
//! - [`World::new`] / [`World::run_random`] build and drive a scenario;
//! - [`World::perform`] runs one [`Action`] (user or fault);
//! - [`World::run_for`] advances virtual time; [`World::finish`] heals
//!   everything, waits for quiescence and runs the end-state checks;
//! - [`World::assert_ok`] panics with the full action log on any violation.

pub mod clock;
pub mod device;
pub mod network;
pub mod world;

pub use clock::{DeviceClock, SimTime};
pub use device::{Library, SimDevice};
pub use network::{Conditions, Network};
pub use world::{Action, Topology, World, WorldConfig};

#[cfg(test)]
mod library_tests;
#[cfg(test)]
mod tests;
