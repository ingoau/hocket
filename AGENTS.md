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

## Building and testing (for agents)

CI ([.github/workflows/ci.yml](.github/workflows/ci.yml)) is the reference for what must pass.

`scripts/dev-env.sh` sets PATH (cargo, node, pnpm shim, Android SDK tools, JDK), `ANDROID_HOME`/`ANDROID_SDK_ROOT`
and `PKG_CONFIG_PATH` for the user-level ALSA headers. Any path it can't find is skipped, so it's safe anywhere.
Source it once per shell (`. scripts/dev-env.sh`) or prefix one command (`scripts/dev-env.sh <cmd>`).
Don't use `. ~/.profile;` instead.

When you need it on the dev host:
- **Not needed** for plain `cargo`, `node` or `adb`. They're already on the Bash tool's PATH.
- **Needed** for Gradle. `ANDROID_HOME` is unset and there's no `local.properties`, so it fails with "SDK location not found".
- **Needed** for anything that builds `hocket-core` with `native-audio`, because cpal needs `alsa.pc`. That covers
  `cargo … --all-features`, `pnpm gen` and `cargo build -p hocket-node`.

In a deskhand `android-dev` desktop, PATH is only `/usr/local/bin:/usr/bin:/bin`, so you need it for adb, emulator,
`start-emulator` and java. It finds `~/Android/Sdk` and `~/opt/jdk` there. Copy the script in with `file_put`, or use `. ~/.profile`.

Commands below run from the repo root unless they `cd`.

```sh
# Rust core
cargo test -p hocket-core                                   # ~700 tests, no env needed
scripts/dev-env.sh cargo test --workspace --all-features    # what CI runs (needs ALSA)

# Android. A fresh worktree first needs the gitignored generated bindings (~1 min):
scripts/gen-bindings.sh
cd android && ../scripts/dev-env.sh ./gradlew -q :playback:compileDebugKotlin :playback:testDebugUnitTest
# Results: android/playback/build/test-results/testDebugUnitTest/*.xml
# Cast (Google Cast SDK) lives in :cast; -Phocket.foss=true builds without it.

# Desktop. desktop/package.json pins pnpm 10 through corepack (pnpm 12 fails with ERR_PNPM_IGNORED_BUILDS).
cd desktop && . ../scripts/dev-env.sh
pnpm install --frozen-lockfile
pnpm gen                    # TS types + napi addon (cargo build, needs ALSA; ~1.5 min warm)
pnpm typecheck && pnpm lint && pnpm test && pnpm build   # vitest unit tests need no display
# Playwright e2e launches Electron, so it needs a display. On the host use xvfb (CI's screen size matters).
xvfb-run -a -s "-screen 0 2000x1600x24" pnpm test:e2e   # ~100 tests, ~20 min; pass a spec path to run one
```

If `pnpm install` says it must purge `node_modules` (left by another pnpm version), delete `desktop/node_modules` and rerun.

### Emulator / adb

The host has no emulator. Build the APK on the host and run it in a deskhand desktop from the `android-dev` template.
That template has a Pixel 7 AVD with a quickboot snapshot.

```sh
# host: an x86_64-only core is enough for the emulator (~7 min cold, all three ABIs take much longer)
scripts/dev-env.sh scripts/build-android-core.sh release x86_64
cd android && ../scripts/dev-env.sh ./gradlew -q assembleDebug   # -> app/build/outputs/apk/debug/app-debug.apk
```

Then in the desktop: `file_put` the APK (and `scripts/dev-env.sh`), then run:

```sh
. ~/dev-env.sh           # or . ~/.profile
start-emulator           # resumes the snapshot in seconds; --cold for a full boot
adb install -r ~/app-debug.apk
adb shell am start -W -n app.hocket.debug/app.hocket.MainActivity
```

The emulator never saves on exit: run `save-emulator` after installs you want to keep. See `~/ANDROID-DEV.md` in the desktop.
