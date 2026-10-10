Note: This is a work in progress. Do not install it, and if you do, do not expect it to work or to get support

# Hocket

A Navidrome client for Android and desktop. Multi-device playback that keeps
working when the network doesn't.

*Hocket* — a medieval technique where one melody is split between two or more
voices, each supplying alternate notes. The line is continuous; no single
performer plays all of it.

- `docs/design.md` — the design notes (decision record)
- `docs/ARCHITECTURE.md` — repository layout, the core seam, build
- `docs/brand.md` — the logo, its colours and wordmark, and how they were chosen
- `crates/` — Rust core, coordinator, bindings
- `android/` — Android app (Compose, Material 3 Expressive, Media3)
- `desktop/` — Electron desktop app

Minimum server: Navidrome 0.63.0. Licence: AGPL-3.0.
