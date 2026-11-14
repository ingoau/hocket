//! Command handling, split by concern. Every `impl Actor` block here is part
//! of the one actor; the split is only for readability.

pub(crate) mod accounts;
pub(crate) mod album_resume;
pub(crate) mod analysis;
pub(crate) mod analysis_share;
pub(crate) mod cache;
pub(crate) mod command;
pub(crate) mod connect;
pub(crate) mod diagnostics;
pub(crate) mod library;
pub(crate) mod playback;
pub(crate) mod prefetch;
pub(crate) mod preview;
pub(crate) mod prime;
pub(crate) mod reports;
pub(crate) mod session;
pub(crate) mod settings;
pub(crate) mod snooze;
pub(crate) mod stash;
pub(crate) mod transitions;
