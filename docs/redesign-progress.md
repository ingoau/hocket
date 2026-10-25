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
| Settings | – | [x] | [ ] |
| Queues (+ rename) | [x] | [x] | [ ] |
| Connect | – | [x] | [ ] |
| Search (recent searches, local only) | – | [ ] | [ ] |
| Album / Artist / Playlist | [x] | [ ] | [ ] |
| Server setup | – | [ ] | [ ] |
| Downloads | – | [ ] | [ ] |
| Stats | – | [ ] | [ ] |
| Filters | – | [ ] | [ ] |
| Song info | [x] | [ ] | [ ] |

Out of scope: bottom nav and mini player; undo history (issue #127).

## Log
- Core: RenameQueue, album release types (migration 0007), ArtistInfo/AlbumInfo queries. Fake core updated.
- Desktop Queues: side panel queue name opens the Queues page (SavedQueues.tsx): Now playing card
  (rename, Create playlist), Pinned/Recent rows without art (rename, pin, delete). Fullscreen keeps
  its drop-down. e2e updated (now-playing, a11y, keyboard-a11y, exit-motion, screenshots).
- Desktop Play on: This computer row, Connect group with status/seen, Connect & devices link. e2e/redesign-screenshots.spec.ts shoots redesigned screens.
- Desktop Settings: nav grouped under section headers (Look and sound, Library and storage, Devices and controls, App), account card on top, sections renamed.
