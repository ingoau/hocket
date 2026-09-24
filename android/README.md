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

Without the native library (no `jniLibs` for the running ABI) a debuggable build runs against
`FakeCore` and a banner says so; a release build never falls back to fake data: `CoreHost.fatalError`
is set and the UI shows a fatal screen with a "reset app data" action (`FatalErrorScreen`).
`CoreHost.forceFake` selects the fake explicitly (UI tests).

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

- `Event.Backend(BackendCommand)` -> `ExoBackend` -> ExoPlayer. `Load` builds a per-item media
  source through `DefaultMediaSourceFactory` over `CoreStreamDataSourceFactory` (each item carries its
  own headers) plus the gapless follow-up as a second playlist item; `SetNext` replaces everything after the current item; the transition is
  detected from `onMediaItemTransition(AUTO)`, reported as `Ended` (played item) then
  `TransitionedToNext`, and the played item removed. The player holds `C.WAKE_MODE_NETWORK` (wake +
  Wi-Fi lock) so streams keep going with the screen off. Network errors (connection failed/timeout)
  are retried with `prepare()` on a ~1 min backoff and reported non-fatal; only then fatal. `PreBuffer` prepares a
  second silent ExoPlayer at the requested position (`PreBufferReady` when READY); `DiscardPreBuffer`
  releases it. `gain_db` is applied as `10^(gain/20) * masterVolume` clamped to 1.0 — Media3 has no
  gain stage, so positive gain is an approximation (documented in `ExoBackend`).
- Core streams: `CoreHost.start` sends `SetBackendCapabilities{core_stream}` ahead of `Start` on
  every core start (the core does not persist it), so the native core hands ExoPlayer
  `hocket-stream://<token>` sources instead of server URLs. `HocketStreamDataSource`
  (`BaseDataSource`, network) reads them through the core's blocking UniFFI calls
  (`streamOpen(url, offset, length)` / `streamRead(handle, ≤256 KiB)` / `streamClose`, on ExoPlayer's
  loader threads): a seek is a new open at the position; the core fetches from the server, caches a
  whole read and serves later plays and seeks from disk. An unknown/expired token is
  `ERROR_CODE_IO_FILE_NOT_FOUND`, never retried (`CoreStreamLoadErrorPolicy`), so the backend reports
  a fatal error and the core resolves a fresh token. The factory routes the `hocket-stream` scheme to
  it and everything else (`file:`, a direct server URL for the fake core) to `DefaultDataSource`; there
  is no `CacheDataSource` (the core caches). `NativeCore` implements the `CoreStreams` seam.
- Reports back: `Ready`, `Playing`, `Paused`, `Buffering`, `Position` every 750 ms while playing and
  on every seek/transition, `Ended`, `TransitionedToNext`, `Error`, `PreBufferReady`,
  `AudioFocusLost` (transient when Media3 suppresses rather than pauses; the core then leaves the
  player alone so it resumes when focus returns).
- `Event.MediaSession(state)` -> `MediaSessionBridge` -> `CoreSessionPlayer`, a `SimpleBasePlayer`
  whose state *is* the core's session state (metadata, extrapolated position from the stamp,
  shuffle/repeat, available commands from the customised action list). Custom buttons (love,
  shuffle, repeat, rate) are media button preferences. Controls map to
  `Command.MediaSessionCommand`. Notification, lockscreen, Bluetooth and headset controls come from
  Media3; the service `addSession`s the session in `onCreate` (the UI is not a Media3 controller, so
  `onGetSession` alone would never register it and no notification or foreground promotion would
  happen).
- Remote output: while another Connect device plays, `CoreSessionPlayer` reports
  `DeviceInfo(PLAYBACK_TYPE_REMOTE, routingControllerId = "hocket-connect")` (fixed volume: Connect
  volume is per device) and `ConnectRouteProvider` (a `MediaRoute2ProviderService`, API 30+) keeps a
  routing session with that id, named after the playing device. SystemUI pairs the two and shows the
  device on the media controls' output chip; the other devices are routes in the system output
  switcher, and picking one (or this phone) is a `HandoffTo`. `PlaybackService` feeds
  `ConnectRoutes` from `DevicesChanged`/`TransportChanged`/`MediaSession` and registers the app's
  MediaRouter2 discovery preference for `app.hocket.feature.CONNECT`, which keeps the provider bound. Playback resumption from the system is refused (resuming is always explicit).
- `NetworkMonitor` -> `SetNetworkState` (kind, metered, hashed SSID or transport id);
  `BatterySaverMonitor` -> `SetBatterySaver` while `battery.autoSaver` is on.
- The app binds with `ACTION_BIND_CORE` (plus a per-process token, since the service is exported
  for Media3) only while some UI is started (`ForegroundBinder` on `ProcessLifecycleOwner`), with a
  plain `startService` (never `startForegroundService`: Media3 promotes the service itself once
  something plays). The service stops itself after five idle minutes with no UI bound, and goes
  when the task is swiped away while idle. `HocketApp` builds the single `CoreClient` for the
  delivered handle while `CoreHost` still runs it.
- `CoreHost` owns the core's lifetime: `Started` (once per core) triggers the one keystore
  credential replay (`AddServer` per stored login); `RequestSnapshot` re-emits state as `Snapshot`
  and never replays. A setup login is stored only once the core reports the server reachable with
  it, dropped on an auth error, and removed before `RemoveServer` on sign-out. Shutdown flushes the
  core on a worker thread (`NativeCore.close` blocks for the flush, bounded).

`CoreClient.query` fails soft: a query the core cannot answer (shut down, freed, the dead core)
returns `null` instead of throwing into a composition coroutine; every caller already treats a
missing result as "nothing". `CoreClient` folds events into `StateFlow`s per snapshot piece, extrapolates position (a 60 Hz
`WhileSubscribed` ticker that runs only while a screen collecting it is resumed), keeps keyed page
caches for tracks/albums/artists/playlist tracks (invalidated on `LibraryChanged`) and holds the
id-keyed selection with select-all as a predicate.

## Settings keys, action ids and app-only preferences

The app reads and writes only the core registry's keys (`app.hocket.core.SettingKeys`, mirroring
`crates/hocket-core/src/settings/registry.rs`): `display.theme`, `display.accent` (null or
`#RRGGBB`), `display.dynamicColour`, `display.animatedBackground`, `battery.autoEngage`,
`lyrics.external.enabled`, `ratings.loveBridge.{enabled,threshold}`, `queue.savedCap`,
`transcoding.profiles` (one map keyed `default` / `cellular` / `<networkId>`), `sync.enabled`,
`connect.{coordinatorUrl,lanDiscovery}`, `actions.order.{contextMenu,sidebar,mediaSession}`, ….
The core refuses unknown keys, so app-only choices (the ordered navigation items) live in DataStore
(`app.hocket.AppPrefs`). Action descriptors from `Query.Actions` carry the registry's canonical ids
(`app.hocket.core.ActionIds`: `play`, `playShuffled`, `playNext`, `rate0`…`rate5`, `love`,
`unlove`, `download`, `unpin`, `goToAlbum`, …); ui-handled ids (`addToPlaylist`, `rate`,
`goToAlbum`, `goToArtist`, `sleepTimer`, `navigate*`) produce no core command and the sheet or
toolbar performs them. `FakeCore` serves the same keys and ids.

## Credentials

The core persists server metadata only. `playback/ServerCredentialStore` keeps the password
AES/GCM-encrypted with an Android Keystore key (`KeystoreCredentialStore`); `CoreHost` replays
`AddServer` for every stored login once `Started` arrives on each core start, prunes logins on
`ServersChanged`, and `AppRoot` shows the shell only when a server is known *and* a login is stored
(otherwise the setup screen). Passwords never reach logs or `toString`.

## The real core on the JVM

`app/src/test/java/app/hocket/realcore/RealCoreEndToEndTest` loads the host build of the core
(`CARGO_TARGET_DIR=target-connect cargo build -p hocket-android` →
`target-connect/debug/libhocket_android.so`, or `HOCKET_HOST_LIB`) through JNA using UniFFI's
`uniffi.component.hocket_android.libraryOverride`, and drives `NativeCore` against
`FakeNavidrome` — an in-process HTTP server on `ServerSocket` serving the core's own Subsonic
fixtures (ping 0.63.1, extensions, getArtists, getAlbumList2/search3 paged, playlists, genres,
scan status, PNG cover art, WAV streams, lyrics, setRating/star/scrobble, 404 `/auth/login`). It
covers AddServer → probe → sync → albums/tracks in the mirror → artwork cached → PlayContext →
`Backend.Load` → Ready/Playing/Position/Ended reports advancing the queue → SetRating reaching the
server → Undo (compare-and-swap) reverting it; credential replay on restart; the wrong-password
Auth error; and a track played through `HocketStreamDataSource` (open, read, a seek as a new open at
an offset, close) with the second play and the seek making no stream request to the server.
`RealServerTest` (only with `HOCKET_TEST_URL/USER/PASS` in the environment) does login, sync, Tally's
syllable lyrics and a streamed read with a ranged re-open against a real Navidrome. The test runs at Robolectric SDK 32 because the UniFFI glue's `SystemCleaner` path on
33+ needs `jdk.internal.ref`, which the JDK does not export. It is skipped when the host `.so` is
absent.

## Gesture and motion conventions

- Anything the finger drives settles with a spring (`ui/theme/Motion.kt`): low-bouncy for the
  sheet and swipes, medium-bouncy for snaps, no-bounce for values that must not overshoot.
- Now-playing sheet: `AnchoredDraggable` with a velocity-aware fling, scrim, corners morphing from
  pill to square, the artwork scaling from the 48 dp thumbnail to the hero; predictive back drags
  it down with the gesture.
- Page navigation (`ui/nav/Transitions.kt`): pushes use a shared X axis, bar switches a short
  fade-through. Predictive back (after Navic) scrubs its own transition on every screen: the page
  shrinks into a rounded card that follows the finger away from the swipe edge while the previous
  page slides in from a short offset; releasing finishes it, cancelling runs it back. Reduced
  motion makes it a crossfade. Holding the bottom bar does nothing special: it is edited from
  Settings > Customise or the account sheet.
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

## Accessibility

- TalkBack: list rows are ONE item ("Tally, twenty one pilots, 3:32, loved", state "playing") with
  the row menu as custom actions (play next, add to queue, go to album/artist via
  `LocalDetailNavigator`, download, rate, more); queue rows add remove / move up / move down, the
  alternatives to the swipe and the drag handle. Icons inside rows carry no description of their
  own. Every gesture has an action: the mini player's skip swipe (next/previous-track actions), the
  sheet drag (expand / collapse / dismiss on `nowPlaying.sheet`, plus the collapse button and back),
  the artwork swipe and long-press, reordering.
- The mini player (collapsed) and the page title (expanded) are polite live regions whose text
  changes once per track: a track change is announced once, position never is. Exactly one of the
  two is in the tree; while the full player covers the screen the page and navigation behind it are
  removed from the tree (`clearAndSetSemantics`).
- Adjustable controls speak values, not percentages: the seek bar ("1 minute 32 seconds of 3 minutes
  32 seconds", whole-second range, `setProgress` seeks), the rating ("3 of 5 stars", one node,
  steps of one star), sliders (`LabelledSlider`: a 48 dp slot owns the semantics) and EQ bands.
- Lyrics: one labelled container; lines are items with "current line" / "background vocal" states;
  nothing is a live region.
- Reduced motion: `LocalReducedMotion` (provided by `HocketTheme`) follows the animator duration
  scale, which "Remove animations" sets to 0. Frame-driven motion honours it: the syllable sweep
  becomes a static highlight of the lit line, no depth-of-field blur or line scaling, jump scrolling,
  a still AGSL background, flat wavy progress lines. (Compose's own animations follow the scale
  anyway.)
- Contrast: `ArtworkColors.accessible` nudges every text/on-colour pair of an artwork scheme to WCAG
  AA (4.5:1; 3:1 for the outline), moving a container when even white/black text cannot reach it;
  the lyrics scrim is computed from the artwork texture's brightest pixel (`LyricsContrast`).
- Large fonts and display size: the mini player grows with the font (`miniPlayerHeight`), the
  player tabs scroll above 1.3x, transport and album header reflow on narrow screens, and option sets
  use `ChoiceRow` (wraps; `ButtonGroup` with an empty overflow hid options).
- Tests: `ui/a11y/A11yChecks` runs ATF's checks (labels, 48 dp touch area and 24 dp visible target,
  redundant "button", duplicate bounds) over the Compose semantics tree of every window, plus layout
  checks (clipped/squeezed controls, cut-off text, overlaps). ATF itself (`enableAccessibilityChecks`)
  is not used: under Robolectric it only sees the View tree, never Compose's virtual nodes.
  `LargeFontLayoutTest` runs at font scale 2.0 on a 320 dp wide screen.
