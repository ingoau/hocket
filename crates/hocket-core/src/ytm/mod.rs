//! YouTube Music (`docs/providers.md`, Phase 4): a slim in-tree InnerTube
//! client and the provider built on it.
//!
//! - [`clients`]: every client profile (names, versions, user agents) in one table.
//! - [`client`]: one call per InnerTube endpoint (search, browse, next,
//!   player, the writes), with `visitorData` and the signed-in session.
//! - [`parse`]: pure parsers from InnerTube JSON to items, tested against
//!   recorded fixtures (`fixtures/`).
//! - [`player`]: the `player` client fallback chain and the platform's JS
//!   host for the web client's signature, `n` and PO token.
//! - [`provider`]: [`YtmProvider`].

pub mod auth;
pub mod client;
pub mod clients;
pub mod http;
pub mod parse;
pub mod player;
pub mod provider;

pub use provider::{YtmProvider, ANON, SOURCE};

#[cfg(test)]
mod tests;
