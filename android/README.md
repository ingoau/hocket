# Hocket for Android

Kotlin + Jetpack Compose (Material 3 Expressive) + Media3, over the shared Rust core.

```
android/
  app/        Compose UI (package app.hocket)
  core/       the seam: UniFFI glue + typeshare types (generated), CoreHandle, CoreClient, FakeCore
  playback/   MediaSessionService, ExoPlayer backend bridge, Media3 session adapter, monitors
```

## Building

Requirements: JDK 17+, Android SDK with platform 37 (AGP installs it when licences are accepted),
NDK 27, Rust with the Android targets (`aarch64-linux-android`, `armv7-linux-androideabi`,
`x86_64-linux-android`), `cargo-ndk`, `typeshare-cli`.

```
scripts/gen-bindings.sh          # typeshare -> core/src/main/java/app/hocket/core/api/Generated.kt
                                 # uniffi-bindgen -> core/src/main/java/app/hocket/core/ffi/hocket_android.kt
scripts/build-android-core.sh    # cargo-ndk -> core/src/main/jniLibs/<abi>/libhocket_android.so
cd android
./gradlew assembleDebug          # ANDROID_HOME=/path/to/sdk (or sdk.dir in local.properties)
./gradlew testDebugUnitTest      # JVM tests: JSON round-trips, client, lyrics cursor, fake core, Compose UI
./gradlew lint
```

The generated Kotlin and the `.so` are gitignored; the build scripts produce them. Gradle 9.7 /
AGP 9.4 with the Kotlin Gradle plugin kept on (`android.builtInKotlin=false`) because the
serialization and Compose compiler plugins ride on it.

Without the native library (no `jniLibs` for the running ABI) the app runs against `FakeCore` and a
debug-only banner says so. `CoreHost.forceFake` selects the fake explicitly (UI tests).

## How bindings work

`crates/hocket-core/src/api.rs` is the schema. `typeshare` turns it into `Generated.kt`
(kotlinx.serialization types; adjacent tagging `{"type":"playNext","data":{...}}`, unit variants
`{"type":"play"}`; anonymous struct variants become `<Enum><Variant>Inner` classes with snake_case
fields, named structs are camelCase; `u32` is `UInt`, `#[serde(default)]` fields are nullable).
`HocketJson` is the one `Json` instance: `classDiscriminator = "type"`, `ignoreUnknownKeys`,
`encodeDefaults`, `explicitNulls = false`.

UniFFI generates `app.hocket.core.ffi.HocketCore` (JSON in/out, `EventListener` callback,
`initLogging`, `coreVersion`) from the compiled host cdylib. `NativeCore` wraps it as a `CoreHandle`
and probes library availability without throwing. `crates/hocket-android/src/fixtures.rs` writes
`core/src/test/resources/json-fixtures.json`; `JsonRoundTripTest` decodes and re-encodes every
fixture so serde/kotlinx drift fails a JVM test.

`Commands`/`Queries` are terse constructors over the generated `*Inner` payloads.

## Service and backend bridge

`PlaybackService` (a Media3 `MediaSessionService`, `foregroundServiceType="mediaPlayback"`) owns
the one core for the process through `CoreHost`:

- `Event.Backend(BackendCommand)` -> `ExoBackend` -> ExoPlayer. `Load` builds a per-item
  `ProgressiveMediaSource` (each item carries its own headers) plus the gapless follow-up as a
  second playlist item; `SetNext` replaces everything after the current item; the transition is
  detected from `onMediaItemTransition(AUTO)` and the played item removed. `PreBuffer` prepares a
  second silent ExoPlayer at the requested position (`PreBufferReady` when READY); `DiscardPreBuffer`
  releases it. `gain_db` is applied as `10^(gain/20) * masterVolume` clamped to 1.0 — Media3 has no
  gain stage, so positive gain is an approximation (documented in `ExoBackend`).
- Reports back: `Ready`, `Playing`, `Paused`, `Buffering`, `Position` every 750 ms while playing and
  on every seek/transition, `Ended`, `TransitionedToNext`, `Error`, `PreBufferReady`,
  `AudioFocusLost` (transient when Media3 suppresses rather than pauses).
- `Event.MediaSession(state)` -> `MediaSessionBridge` -> `CoreSessionPlayer`, a `SimpleBasePlayer`
  whose state *is* the core's session state (metadata, extrapolated position from the stamp,
  shuffle/repeat, available commands from the customised action list). Custom buttons (love,
  shuffle, repeat, rate) are media button preferences. Controls map to
  `Command.MediaSessionCommand`. Notification, lockscreen, Bluetooth and headset controls come from
  Media3. Playback resumption from the system is refused (resuming is always explicit).
- `NetworkMonitor` -> `SetNetworkState` (kind, metered, hashed SSID or transport id);
  `BatterySaverMonitor` -> `SetBatterySaver` while `battery.autoSaver` is on.
- The service stops itself after five idle minutes with no bound client. The app binds with
  `ACTION_BIND_CORE` to obtain the `CoreHandle`; `HocketApp` builds the single `CoreClient`.

`CoreClient` folds events into `StateFlow`s per snapshot piece, extrapolates position (a 60 Hz
`WhileSubscribed` ticker that runs only while a screen collecting it is resumed), keeps keyed page
caches for tracks/albums/artists/playlist tracks (invalidated on `LibraryChanged`) and holds the
id-keyed selection with select-all as a predicate.

## Gesture and motion conventions

- Anything the finger drives settles with a spring (`ui/theme/Motion.kt`): low-bouncy for the
  sheet and swipes, medium-bouncy for snaps, no-bounce for values that must not overshoot.
- Now-playing sheet: `AnchoredDraggable` with a velocity-aware fling, scrim, corners morphing from
  pill to square, the artwork scaling from the 48 dp thumbnail to the hero; predictive back drags
  it down with the gesture.
- Mini player: tap to expand, swipe left/right to skip with resistance past the threshold, a thin
  wavy progress line.
- Hero artwork: horizontal swipe to skip (springs back); long-press toggles a whole-app dynamic
  colour preview from the artwork. The sheet content is themed from the artwork while open.
- Play/pause morphs between `MaterialShapes.Cookie9Sided` and `Square`; the seek bar is a
  `LinearWavyProgressIndicator` whose wave flattens while dragging, with a time bubble.
- Haptics: `GestureThresholdActivate` on drag start, `SegmentFrequentTick` on reorder,
  `Confirm` on a completed skip, `LongPress` on entering selection mode.
- Lists: long-press enters selection mode, the actions come from `Query.Actions(contextMenu, …)`
  in an expressive `HorizontalFloatingToolbar`; drag handles reorder (playlists, queue); swipe
  removes from the queue (undo from the core's toast).
- Lyrics: syllable gradient sweep per syllable driven by `withFrameNanos` and `LyricsCursor`,
  30 fps in battery saver, stopped when not visible; AGSL warp background on Android 13+, static
  blurred still otherwise or when battery saver / "animated background off" applies.
- Touch targets are at least 48 dp; every interactive element has a content description; strings
  live in `res/values/strings.xml`.
