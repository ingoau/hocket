# Redesign implementation progress

Working notes for implementing the final redesigns on the canvas
https://claude.ai/artifact/JQK3ArVDrYX6HKqPwseyRE ("Final designs (light)" section,
the Ship*.dc.html boards). Delete this file when the work lands.

## Order of work

1. Core (`crates/hocket-core`), testable here with `cargo test`:
   - [x] Rename a saved queue (command + undo + event).
   - [x] Read OpenSubsonic `releaseTypes` on albums; expose a release kind (album, EP, single, compilation).
   - [x] `getAlbumInfo2` (album notes, MusicBrainz id, Last.fm URL), cached like artist info.
2. Desktop (`desktop/`), testable with the desktop test suite and screenshots.
3. Android (`android/`). No Android SDK in this container, so Kotlin changes can't be
   compiled or screenshot-tested here; keep them small and close to existing code.

## Screens

| Screen | Core | Desktop | Android |
| --- | --- | --- | --- |
| Settings | – | [x] | [x] (uncompiled) |
| Queues (+ rename) | [x] | [x] | [x] (uncompiled) |
| Connect | – | [x] | [x] (uncompiled) |
| Search (recent searches, local only) | – | [x] | [x] (uncompiled) |
| Album / Artist / Playlist | [x] | [x] | [~] shelves only (uncompiled) |
| Server setup | – | [x] | [ ] |
| Downloads | – | [x] | [x] (uncompiled) |
| Stats | – | [x] | [x] (uncompiled) |
| Filters | – | [x] | [ ] |
| Song info | [x] | [x] | [x] (uncompiled) |

Out of scope: bottom nav and mini player; undo history (issue #127).

## Log
- Core: RenameQueue, album release types (migration 0007), ArtistInfo/AlbumInfo queries. Fake core updated.
- Desktop Queues: side panel queue name opens the Queues page (SavedQueues.tsx): Now playing card
  (rename, Create playlist), Pinned/Recent rows without art (rename, pin, delete). Fullscreen keeps
  its drop-down. e2e updated (now-playing, a11y, keyboard-a11y, exit-motion, screenshots).
- Desktop Play on: This computer row, Connect group with status/seen, Connect & devices link. e2e/redesign-screenshots.spec.ts shoots redesigned screens.
- Desktop Settings: nav grouped under section headers (Look and sound, Library and storage, Devices and controls, App), account card on top, sections renamed.
- Desktop Search: kind chips, 5-song preview, recent searches in localStorage (lib/recent-searches.ts).
- Desktop detail pages: DetailHead (Large/Small/None, never upscaled); album/playlist use a compact
  hero because their track list scrolls under it. Artist: top songs preview, shelves per release kind
  with View more grid (newest/oldest), About from artistInfo.
- Desktop Song info: wide dialog (SongInfo.tsx), library details left, Last.fm right, no action buttons.
- Desktop Downloads (A): table (title, downloaded, size, files, remove) and a Storage card (bar, Available offline link, clear cache, warn threshold); card stacks above via a container query.
- Desktop Stats: period chips (7 days / 30 days / Year), chart readouts (hovered bar or the peak), peak bar on the accent.
- Desktop Filters: master/detail (list beside the editor), Unsaved changes badge, readable field names. Rules stay inline editors (not chips + popover) — deviation.
- Desktop Setup: board copy, URL hint, http:// cleartext warning, Enter hint; Name field dropped. The post-connect summary step is not implemented (needs version/capability info at setup) — deviation.
- Android Queues (not compiled here): Playing now card (rename, Create playlist), rows without art with rename/pin/immediate delete, undo-history button removed; Commands.renameQueue; FakeCore handles RenameQueue, ArtistInfo, AlbumInfo.
- Android Connect (not compiled here): AudioOutputs (playback module) lists connected outputs via
  AudioDeviceCallback and applies the pick with ExoPlayer setPreferredAudioDevice; the sheet shows
  This phone outputs (either direction, handing playback back first if another device plays), a
  Bluetooth settings link, then other devices; gear by the title opens Connect & devices settings
  (LocalOpenConnectSettings from AppRoot). New icons via scripts/gen-android-icons.mjs.
- Android search: RecentSearchPrefs (SharedPreferences, device only); Android artist shelves via
  core ReleaseKinds.kt (+ unit test); song info Last.fm cards.
- Android Settings: account card on top (stats/devices chips), group headings over the category cards.
