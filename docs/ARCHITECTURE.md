# Hocket architecture

Read `docs/design.md` first. It is the decision record; this file is how the
repository is laid out to implement it and the rules that keep parallel work
from colliding.

## Layout

```
Cargo.toml                    workspace
crates/hocket-core/           the shared Rust core (library)
  src/api.rs                  THE CONTRACT: commands, queries, events, documents (typeshare)
  src/core.rs                 the actor that owns every subsystem
  src/session/                session document, queue reducer, history, saved queues
  src/undo/                   undo stack, command objects with inverses
  src/actions/                action registry
  src/connect/                Connect protocol: wire, clock sync, lease/epoch, election, discovery, handoff, replica
  src/sim/                    deterministic simulation harness (feature "sim")
  src/subsonic/               Subsonic/OpenSubsonic/Navidrome client, auth, capability probe
  src/db/                     SQLite mirror, migrations, library sync, queries
  src/outbox/                 durable mutations, retry, conflict, scrobbler
  src/jobs/                   job queue + problems
  src/downloads/              pins + evictable stream cache
  src/cache/                  image / lyrics / metadata caches
  src/lyrics/                 v2 structured lyrics -> renderable model
  src/filters/                NSP-superset filter model, SQL evaluation, .nsp export
  src/autoplay/               provider chain
  src/settings/               scoped settings, config document
  src/stats/                  listening stats
  src/audio/                  PlaybackBackend seam, native backend (Symphonia+cpal), DSP, external bridge
  src/media_session/          MediaSessionAdapter seam, state derivation
crates/hocket-coordinator/    headless binary: relay + replica (axum websocket)
crates/hocket-android/        UniFFI cdylib: JSON in/out + event callback
crates/hocket-node/           napi-rs addon: JSON in/out + event callback
android/                      Gradle project (Kotlin, Compose, Material 3 Expressive, Media3)
desktop/                      Electron + React + Vite + TypeScript (pnpm)
docs/                         design notes, this file, protocol notes
scripts/                      codegen and build helpers
```

## The seam

`crates/hocket-core/src/api.rs` is the schema authority. Everything crosses the
FFI as JSON strings of those types:

- `Command` — fire and forget, `Core::dispatch`.
- `Query` → `QueryResult` — async request/response, `Core::query`.
- `Event` — streamed to every registered `EventSink`.

Both platforms get generated types from the same file:

```
scripts/gen-bindings.sh
  typeshare → desktop/src/core/api.ts                     (TypeScript)
  typeshare → android/core/src/main/java/app/hocket/core/api/Generated.kt  (Kotlin, kotlinx.serialization)
  uniffi-bindgen → android/core/src/main/java/app/hocket/core/ffi/         (Kotlin FFI glue)
  napi → desktop/native/index.js + index.d.ts
```

Enums with payloads are adjacently tagged: `{"type": "playNext", "data": {...}}`.
Unit variants are `{"type": "play"}`. Kotlin uses `Json { classDiscriminator = "type"; ignoreUnknownKeys = true; encodeDefaults = true }`.

Changing `api.rs`: additive only once shipped (new `Option` fields or
`#[serde(default)]`). No `u64`/`i64`, no tuples, no `serde_json::Value`.
Every enum with data carries `#[serde(tag = "type", content = "data")]`.
Variant names must not equal their payload's type name (Kotlin nesting clash).
Rerun `scripts/gen-bindings.sh` after any change and commit the Rust; generated
files are gitignored and produced by the build.

## The actor

`Core` (in `core.rs`) owns: a tokio runtime (or borrows one), the session
subsystem, the undo stack, the action registry, the Connect engine, the
Subsonic client per server, the SQLite mirror, outbox, job queue, downloads,
caches, filters, autoplay, settings, stats, the playback backend and the
media-session state. The actor loop receives `Command`s on an unbounded
channel and handles them sequentially; long work is spawned onto the runtime
and reports back through internal messages so the loop never blocks.

Subsystems must not reach into each other's state. They communicate by:

- being called by the actor with explicit arguments and returning results, or
- posting `Internal` messages onto the actor's channel (`core::Internal`).

Anything that needs time uses an injected `util::Clock` so the simulation
harness can drive it. Anything that needs the network goes through a trait
that the harness can swap for an in-memory implementation.

## Platform layers are thin

- Android: a `MediaSessionService` owns one `HocketCore`. ExoPlayer is the
  external `PlaybackBackend`: the service turns `Event::Backend(BackendCommand)`
  into ExoPlayer calls and posts `Command::BackendReport` back. Media3's
  `MediaSession` is the `MediaSessionAdapter`, fed by `Event::MediaSession`.
  The Compose UI talks to the core through the service via a bound
  `CoreClient` (dispatch / query / event flow). No business logic in Kotlin.
- Desktop: Electron main process owns one `HocketCore` (napi). Audio is native
  in the core. The playwire addon (`crates/hocket-node` feature or a sibling
  crate) is the `MediaSessionAdapter`, fed by `Event::MediaSession`. The React
  renderer talks to main over a typed IPC bridge (`window.hocket.dispatch`,
  `query`, `onEvent`). No business logic in the renderer beyond view state.
- Coordinator: `hocket-coordinator` runs `Core` with `AudioMode::None` and
  `coordinator_listen` set; it never holds credentials and verifies clients by
  proxying a Subsonic `ping`.

## Conventions

- Rust 2021, `cargo fmt`, `cargo clippy -D warnings` clean, tests via `cargo test`.
- Errors: `thiserror` per subsystem, `anyhow` at the actor boundary, never `unwrap` on I/O.
- Logging: `tracing`. No network telemetry, ever.
- All persisted state has a schema version and forward-only numbered migrations.
  The mirror is a cache and may be dropped and rebuilt; downloads, outbox,
  saved queues and settings are not and get a backup before migrating.
- Strings shown to users come from the platform layer's resources, keyed by
  ids the core emits (e.g. `Toast.message` is a plain string for now; keep
  user-facing text in a small `strings` table so it can be externalised).
- Every queue/session mutation goes through the reducer and is a command
  object with an inverse. There is no other path.
- Symfonium is the reference for any behaviour the design notes don't specify
  (https://support.symfonium.app/). Match its conventions where sensible.

## Build

```
# Rust core, coordinator, binding crates
cargo build --workspace
cargo test --workspace --all-features            # `sim` feature enables the full-core integration tests
cargo clippy --workspace --all-features --all-targets -- -D warnings

# Generated types and FFI glue (typeshare -> TS/Kotlin, uniffi -> Kotlin glue)
scripts/gen-bindings.sh

# Desktop (Electron + React). `pnpm gen` also builds the napi addon into desktop/native/.
cd desktop && pnpm install && pnpm gen && pnpm build
pnpm typecheck && pnpm lint && pnpm test
xvfb-run -a pnpm test:e2e                        # Playwright Electron e2e (fake core and native core + fake Navidrome)
pnpm dist:dir                                    # electron-builder unpacked build

# Android (Gradle 9, AGP 9, Kotlin 2.4, compileSdk 37.1, Material 3 Expressive)
scripts/build-android-core.sh release            # cargo-ndk -> android/core/src/main/jniLibs/{arm64-v8a,armeabi-v7a,x86_64}
cd android && ./gradlew testDebugUnitTest assembleDebug :app:lintDebug   # ANDROID_HOME=/opt/android-sdk

# Coordinator
cargo run -p hocket-coordinator -- --listen 0.0.0.0:8790 --data-dir /var/lib/hocket
```

Linux builds of the core need `libasound2-dev` (cpal). The desktop app's OS media
session needs a D-Bus session bus on Linux; without one it logs and continues.

## Testing strategy

- Every subsystem has in-module unit tests; the queue reducer, filters, outbox
  scrobbler and connect wire types also have property tests.
- `crates/hocket-core/src/sim/` is the deterministic simulation harness
  (virtual clock, in-memory network with partitions/delay/loss/reorder) running
  real engines and reducers through scripted and randomised multi-device
  scenarios with invariants checked after every event.
- `crates/hocket-core/tests/actor_*.rs` drive full `Core` instances
  (`Core::new_for_test`) with a scripted backend, a fake Navidrome and virtual
  time: playback, undo, saved queues, scrobbling, sync, search, ratings via the
  outbox, filters, lyrics, settings, downloads, and two cores over an in-memory
  LAN doing a handoff.
- `crates/hocket-coordinator/tests/handoff.rs` runs the real binary on an
  ephemeral port with two engines over real WebSockets.
- Desktop: vitest for renderer logic; Playwright Electron e2e against the
  FakeCore and against the native core with a fake Navidrome HTTP server.
- Android: JVM tests for the seam, page cache, selection and lyrics cursor;
  Robolectric Compose tests for setup, queue, now-playing sheet and layout.
- Parsers that read untrusted input have `*_never_panics` proptest modules
  (stable, part of `cargo test`) next to them: Connect frames and
  `api::Command`/`Query` JSON (`connect/wire.rs`), Subsonic envelopes and the
  `convert.rs` mapping (`subsonic/types.rs`), LRC, structured lyrics and the
  lyrics cursor (`lyrics/lyrics_never_panics.rs`), config import
  (`settings/config.rs`) and NSP import (`filters/nsp.rs`). Inputs the
  fuzzers found live there as `fuzz_regression_*` tests.
- `fuzz/` is a cargo-fuzz crate (its own workspace, never built by the root
  one) with one libFuzzer target per entry point: `wire_decode`,
  `subsonic_envelope`, `lyrics_lrc`, `lyrics_structured` (also checks the
  actor's subsonic-types → `lyrics::raw` path agrees with the direct parse),
  `lyrics_cursor`, `config_import`, `nsp_import` and `api_json`. Besides "no
  panic", targets assert round trips (decode → encode → decode) and that the
  incremental lyrics cursor agrees with a fresh lookup. Run one with nightly:

  ```sh
  cargo install cargo-fuzz
  cd fuzz
  cargo +nightly fuzz list
  # new inputs go to work/ (ignored); corpus/ holds small committed seeds
  cargo +nightly fuzz run lyrics_structured work/lyrics_structured corpus/lyrics_structured -- -max_total_time=600 -max_len=65536
  # a crash lands in fuzz/artifacts/<target>/: minimise, then replay it
  cargo +nightly fuzz tmin lyrics_structured artifacts/lyrics_structured/crash-…
  cargo +nightly fuzz run lyrics_structured artifacts/lyrics_structured/minimized-…
  ```

  Fix the parser, add the minimised input as a `fuzz_regression_*` test, and
  re-run the target. The build (ASan, `fuzz/target/`) needs about 2 GB of
  disk; `--sanitizer none` roughly halves it. CI runs every target for five
  minutes weekly and on manual dispatch (the `fuzz` job in
  `.github/workflows/ci.yml`), uploading any crash as an artifact.
