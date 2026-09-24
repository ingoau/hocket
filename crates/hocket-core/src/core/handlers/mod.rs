//! Command handling, split by concern. Every `impl Actor` block here is part
//! of the one actor; the split is only for readability.

pub(crate) mod command;
pub(crate) mod connect;
pub(crate) mod library;
pub(crate) mod playback;
pub(crate) mod prefetch;
pub(crate) mod servers;
pub(crate) mod session;
pub(crate) mod settings;
