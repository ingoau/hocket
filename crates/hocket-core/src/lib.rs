//! Hocket core. See `docs/design.md` and `docs/ARCHITECTURE.md`.
//!
//! The crate is an actor: [`Core`] takes [`api::Command`]s, answers
//! [`api::Query`]s asynchronously and streams [`api::Event`]s to a listener.
//! Platform layers (Android via UniFFI, Electron via napi-rs, the headless
//! coordinator) are thin shells around it.

// The api enums carry whole documents in a few variants by design (they are
// the wire and FFI shape); boxing them would only move the allocation.
#![allow(clippy::large_enum_variant)]

pub mod api;
pub mod core;
pub mod util;

pub mod actions;
pub mod artwork;
pub mod audio;
pub mod autoplay;
pub mod cache;
pub mod connect;
pub mod db;
pub mod downloads;
pub mod filters;
pub mod jobs;
pub mod lyrics;
pub mod media_session;
pub mod outbox;
pub mod session;
pub mod settings;
pub mod stats;
pub mod subsonic;
pub mod undo;

#[cfg(any(feature = "sim", test))]
pub mod sim;

pub use crate::core::{Core, CoreError, EventSink};
pub use api::{Command, CoreConfig, Event, Query, QueryResult};
