# Hocket desktop

Electron + React 19 + Vite + TypeScript (pnpm). The Electron main process owns one
Rust core (`crates/hocket-node`, a napi-rs addon over `hocket-core`); the renderer
is view state only and talks to main over a typed `contextBridge`.

## Build, run, test

Prerequisites: Node 22, pnpm 10, a Rust stable toolchain, and `typeshare-cli`
(`cargo install typeshare-cli`) for the generated types.

```
pnpm install
pnpm gen            # typeshare -> src/core/api.ts, then `napi build` -> native/ (release)
pnpm gen:types      # only the TypeScript types (enough for typecheck/tests/renderer work)
pnpm dev            # Vite dev server + esbuild --watch for main/preload + Electron
pnpm build          # main + preload (esbuild) and the renderer (Vite) into out/
pnpm start          # run the built app
pnpm typecheck      # renderer (tsconfig.json) and main/preload/e2e (tsconfig.node.json)
pnpm lint           # eslint
pnpm test           # vitest: reducer, position extrapolation, selection model,
                    # shortcut parser, palette ranking, lyrics mapping, accent extraction
pnpm test:e2e       # Playwright Electron e2e (needs a display: `xvfb-run -a pnpm test:e2e` on a
                    # headless Linux box; run `pnpm build` first). e2e/*.spec.ts except native.spec.ts
                    # run against the FakeCore; e2e/native.spec.ts runs the REAL core (native addon,
                    # skipped when native/ isn't built) against e2e/fake-navidrome.ts, a small
                    # Subsonic JSON server: setup → sync → play → queue → rate → undo → lyrics →
                    # settings persistence → credential replay across a restart.
pnpm dist           # electron-builder: AppImage + deb (Linux), dmg (macOS), nsis (Windows)
pnpm dist:dir       # unpacked build in release/ (what CI uses to prove packaging)
```

`src/core/api.ts` and `native/` are generated and gitignored. `pnpm build` runs
`gen:types` automatically when `api.ts` is missing. `scripts/postprocess-api.mjs`
rewrites typeshare's `export enum` output into string-literal unions so the JSON
the core actually sends (`"off"`, `"all"`, …) typechecks; every union also gets a
`<Name>Values` array.

Environment variables:

| Variable | Effect |
|---|---|
| `HOCKET_FAKE_CORE=1` | Use the TypeScript `FakeCore` instead of the native addon. Also used automatically when the addon is missing (a dev banner is shown). |
| `HOCKET_FAKE_TIMESCALE=8` | Speed up the fake core's timers (jobs, server search) — the e2e suite sets this. |
| `HOCKET_FAKE_CORE_FRESH=1` | Ignore the fake core's remembered server so setup shows again. |
| `HOCKET_USER_DATA=<dir>` | Override Electron's userData dir (fresh profile per e2e test). |
| `HOCKET_DEVICE_NAME` | Device name reported to the core (defaults to the hostname). |
| `HOCKET_LOG=debug` | `tracing` filter for the native core. |
| `HOCKET_DEV=1` | Set by `pnpm dev`: load the renderer from the Vite dev server. |

## Layout

```
src/main/            Electron main: windows, tray, IPC, core host, credentials, media session
  index.ts           entry: single-instance lock, protocols, CSP, power/network -> core
  core-host.ts       loads native/index.js (schema-checked) or falls back to FakeCore
  fake-core/         FakeCore: ~3,200-track deterministic library, queue reducer, jobs, undo…
  media-session.ts   feeds the playwire addon from Event.MediaSession, maps its callbacks
  credentials.ts     ServerCredentialStore: safeStorage-encrypted blobs, AddServer replay
  windows.ts         main window, always-on-top mini player, persistent hidden anchor window
  updater.ts         documented hook; auto-update deliberately not implemented
src/preload/         the contextBridge (`window.hocket`)
src/shared/          contract shared by main/preload/renderer: bridge types, IPC names,
                     strings.ts (every user-visible string, keyed), default keymap
src/renderer/        React app
  store/             zustand store, pure event reducer, position ticker, selection model,
                     shortcut parser, palette ranking, action executor, query hooks
  components/        player bar, right panel (queue + lyrics), track table, grid, menus…
  views/             setup, home, library views, filters, stats, settings, fullscreen, mini
  lib/               lyrics -> AMLL mapping, accent extraction, formatting
e2e/                 Playwright Electron tests
```

## The IPC bridge

`window.hocket` (see `src/shared/bridge-types.ts`) is the only surface the renderer
has. `contextIsolation` and `sandbox` are on, `nodeIntegration` is off, and a strict
CSP is applied to every document.

```ts
window.hocket.dispatch(command)             // Command -> core, fire and forget
await window.hocket.query(query)            // Query -> QueryResult
window.hocket.onEvent(cb)                   // every core Event; returns an unsubscribe
window.hocket.meta()                        // version, platform, coreKind, mediaSession status…
window.hocket.window.{minimize,maximize,close,setAlwaysOnTop,openFullscreen,openMiniPlayer,…}
window.hocket.shell.{showItemInFolder,openExternal}
window.hocket.dialog.{save,open,writeTextFile,readTextFile}
window.hocket.clipboard.writeText
window.hocket.onDeepLink(cb)                // hocket://album/<id>, hocket://artist/<id>, …
```

Types come from `src/core/api.ts`, generated by typeshare from
`crates/hocket-core/src/api.rs`. Enums with data are adjacently tagged
(`{ type: "playNext", data: {…} }`, unit variants `{ type: "play" }`).

Main forwards to the core: `SetVisibility` (window shown/minimised/focused and page
visibility), `SetBatterySaver` (power monitor, when "engage automatically on battery"
is on), `SetNetworkState` (online/offline). The renderer keeps a mirror of the snapshot
pieces from events (`store/reducer.ts`) and extrapolates the playback position from the
last `PositionStamp` on a `requestAnimationFrame` ticker that only runs while a
component showing position is mounted.

Artwork is resolved through `Query.Artwork` (fixed cache sizes 64/300/1000) and served
to the renderer over the `hocket-art://` protocol, restricted to the cache/data dirs.

## Media session addon

`crates/hocket-node` exports two napi classes:

- `HocketCore` — `new HocketCore(configJson)`, `setListener(cb)`, `dispatch(commandJson)`,
  `query(queryJson): Promise<string>`.
- `MediaSession` — the desktop `MediaSessionAdapter` over the `playwire` crate (MPRIS via
  zbus on Linux, SMTC on Windows, MPNowPlayingInfoCenter on macOS).
  `new MediaSession({ name, desktopEntry, hwnd, trackIdPrefix }, cb)`, `setState(stateJson)`
  with a serialised `MediaSessionState`, `setPosition(ms)` per tick, `detach()`.
  The callback receives `{ kind: "command", command: <Command> }`, `{ kind: "raise" }`,
  `{ kind: "quit" }` or `{ kind: "openUri", uri }`; main dispatches the command straight
  into the core (`Command.MediaSessionCommand`).

Notes from `docs/design.md` "OS media session" that the host honours:

- Electron starts with `--disable-features=HardwareMediaKeyHandling,MediaSessionService`
  so Chromium never grabs the media keys.
- Position is published every second while playing; playwire diffs and MPRIS `Position`
  isn't change-signalling anyway.
- Artwork must be a `file://` path; the core resolves it through the image cache and the
  host falls back to `Query.Artwork` if the state arrives without one.
- On Windows SMTC is tied to an HWND, so the session is anchored to a persistent hidden
  window (`Windows.ensureAnchor()`), never the closable main window (close-to-tray).
- `desktopEntry` is `hocket`, matching the `.desktop` file electron-builder generates.
- The crate feature `media-session` (default on) gates playwire. Without it, or when the
  OS service is unavailable, `MediaSession` reports unsupported and the app runs without
  OS integration; it never blocks the build.

## Action ids and settings keys

The UI keys everything on the registry's **canonical** action ids
(`crates/hocket-core/src/actions/defs.rs`): `play`, `playShuffled`, `playNext`, `rate0`…`rate5`,
`love`, `togglePlay`, `seekBackward`, `toggleQueuePanel`, `openCommandPalette`, `navigateAlbums`,
… The core accepts aliases on input; `src/shared/keymap.ts` carries the same alias table
(`ACTION_ALIASES`, `canonicalActionId`) so older ids keep working, and `NAV_VIEWS` maps
`navigate*` ids to views. Ids with a `ui.` prefix (`ui.escape`, `ui.back`, `ui.info`,
`ui.newPlaylist`, `ui.renamePlaylist`) have no registry equivalent and stay renderer-only.
Descriptor icons are Material Symbols names; `components/Icon.tsx` maps them to local glyphs.

Settings keys are the registry's (`crates/hocket-core/src/settings/registry.rs`), listed in
`src/shared/settings-keys.ts`: `display.theme`, `display.accent`, `display.dynamicColour`,
`display.animatedBackground`, `display.lyricsFps`, `display.queuePanelSplit`, `battery.autoEngage`,
`battery.lyricsFps`, `lyrics.external.enabled`, `ratings.loveBridge.{enabled,threshold}`,
`storage.warnThresholdBytes`, `queue.{mode,savedCap}`, `sync.enabled`, `connect.*`,
`actions.order.*`, `shortcuts`. Close-to-tray is not a registry key; it is a device-local
preference in main's state file, exposed as `window.hocket.prefs`.

Real-core behaviours the renderer relies on: `AddServer` probes first, so `ServersChanged` only
arrives for a working server (a failed probe is `Error{auth|network|server}` + a toast, and the
setup screen stays); the core emits `LyricsChanged` on every `NowPlayingChanged` (the store asks
`Query.Lyrics` once on attach for whatever is already playing); `Query.Artwork` returns a
filesystem path (`artworkFilePath` in `src/shared/constants.ts` also tolerates `file://`).
Optional fields arrive as `null`, never `undefined`. A fresh undoable mutation produces only
`UndoChanged`; the reducer synthesises the "<label> · Undo" toast from it, once per entry id.

## Panel and keyboard conventions

- Left sidebar (choose-and-order via Settings → Customisation, backed by
  `Query.Actions(surface = "sidebar")` / `Command.SetActionOrder`), main content, right
  panel with the queue above the lyrics. The divider ratio, right-panel width and sidebar
  width are device-local (`localStorage`, `store/app.ts`). Either pane collapses to give
  the other the full height (`Q` / `L` toggle them).
- Lists are virtualised (`@tanstack/react-virtual`), never paginated. Selection is keyed by
  id; Ctrl+A selects the predicate "everything matching" with the count from the query's
  `total`, never materialised ids. Shift-range, Ctrl-click, arrows/Home/End/PageUp/Down and
  type-ahead all work; roving `role="grid"` / `aria-selected` from the start.
- Context menus and the command palette (Ctrl/Cmd+K) are generated from the action
  registry (`Query.Actions`); `ui.*` action ids are executed in the renderer
  (`store/actions.ts`), everything else goes through `Command.RunAction`.
- Keyboard map: `src/shared/keymap.ts` holds the defaults from the design table; the
  core's `Query.Shortcuts` overrides by action id and Settings → Keyboard shortcuts
  rebinds (with conflict detection and reset) through `Command.SetShortcut`. A focused
  text field keeps Ctrl+Z/typing; media keys never arrive here.
- Fullscreen player (`F`): Kawarp fluid background fed from the cached artwork via
  `loadBlob`, AMLL lyrics. Performance budget: hidden/minimised → both renderers stopped;
  visible-but-unfocused → ~24 fps; focused → full rate (capped by the lyrics fps setting);
  battery saver → static blurred still, lyrics ≤ 30 fps. The animated background has its
  own always-off setting.
- Lyrics tiers are honoured honestly: syllables → AMLL words, line tier → one word per
  line, unsynced → a static list. Agents map to duet alignment, background lines to `isBG`.

## Packaging

`electron-builder.yml`: Linux AppImage + deb, macOS dmg (hardened runtime, notarisation
placeholders, no signing configured), Windows nsis. The napi addon ships as an extra
resource (`resources/native/`), loaded by `core-host.ts` from `process.resourcesPath`
first and the dev tree second. Auto-update is intentionally absent (`src/main/updater.ts`
explains the hook; `publish: null`).

Licence: AGPL-3.0-only (`LICENSE`). AMLL (renderer only) is AGPL-3.0, Kawarp MIT,
playwire MIT OR Apache-2.0.
