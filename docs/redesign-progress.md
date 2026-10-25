# Redesign implementation progress

Working notes for implementing the final redesigns on the canvas
https://claude.ai/artifact/JQK3ArVDrYX6HKqPwseyRE ("Final designs (light)" section,
the Ship*.dc.html boards). Delete this file when the work lands.

## Order of work

1. Core (`crates/hocket-core`), testable here with `cargo test`:
   - [ ] Rename a saved queue (command + undo + event).
   - [ ] Read OpenSubsonic `releaseTypes` on albums; expose a release kind (album, EP, single, compilation).
   - [ ] `getAlbumInfo2` (album notes, MusicBrainz id, Last.fm URL), cached like artist info.
2. Desktop (`desktop/`), testable with the desktop test suite and screenshots.
3. Android (`android/`). No Android SDK in this container, so Kotlin changes can't be
   compiled or screenshot-tested here; keep them small and close to existing code.

## Screens

| Screen | Core | Desktop | Android |
| --- | --- | --- | --- |
| Settings | – | [ ] | [ ] |
| Queues (+ rename) | [ ] | [ ] | [ ] |
| Connect | – | [ ] | [ ] |
| Search (recent searches, local only) | – | [ ] | [ ] |
| Album / Artist / Playlist | [ ] | [ ] | [ ] |
| Server setup | – | [ ] | [ ] |
| Downloads | – | [ ] | [ ] |
| Stats | – | [ ] | [ ] |
| Filters | – | [ ] | [ ] |
| Song info | [ ] | [ ] | [ ] |

Out of scope: bottom nav and mini player; undo history (issue #127).

## Log
