# Hocket

A Navidrome client for Android and desktop, with multi-device playback that
keeps working when the network doesn't.

- `crates/`: Rust core, coordinator, bindings
- `android/`: Android app (Compose, Material 3 Expressive, Media3)
- `desktop/`: Electron desktop app
- `docs/`: design notes (`design.md`), architecture and build (`ARCHITECTURE.md`), brand (`brand.md`)

## Goals

- Works across devices seamlessly: playback, queue and state follow you
  from one device to the next.
- Responsive: cache aggressively so the UI never waits on the network.
- Good UX that feels native on each platform.

## How we work

Hocket has no users yet and we're moving fast. Breaking changes are fine:
change APIs, schemas, storage formats and protocols freely. Don't add
migrations, compatibility shims or deprecation paths unless asked.
