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
//! - [`provider`][]: [`YtmProvider`].

pub mod auth;
pub mod client;
pub mod clients;
#[cfg(any(test, feature = "sim"))]
pub mod fake;
pub mod http;
pub mod parse;
pub mod player;
pub mod provider;

/// The platform JS host's script: yt-dlp's EJS solver (0.8.0, vendored from
/// github.com/yt-dlp/ejs releases; Unlicense, with meriyah (ISC) and astring
/// (MIT) bundled) and Hocket's `hocketYtm` on top (`js/host.js`).
pub fn js_host_script() -> String {
    [
        include_str!("js/ejs-lib.min.js"),
        "\nObject.assign(globalThis, lib);\n",
        include_str!("js/ejs-core.min.js"),
        "\n",
        include_str!("js/host.js"),
    ]
    .concat()
}

pub use provider::{YtmProvider, ANON, SOURCE};

#[cfg(test)]
mod tests;
