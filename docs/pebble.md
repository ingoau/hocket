# Hocket — Pebble watchapp

A Pebble watchapp that controls Hocket from the wrist. Now Playing behaves exactly like the official PebbleOS music app; touch gestures and menus add the queue, the library and track actions on top.

Read alongside `docs/design.md` (decision record) and `docs/ARCHITECTURE.md` (layout and seams).

**Status: implemented** (all four steps below). Code: `pebble/` (watchapp; `src/c/hocket.h` is the protocol), `android/playback/.../pebble/` (`PebbleBridge`, `PebbleListenerService`, `PebbleProtocol`, `PebbleInstaller`), Settings → Pebble watch. Screenshots: `docs/screenshots/pebble/`.


## The shape of it  
*Status: decided*

Three pieces:

- **The watchapp** (`pebble/`, C, public Pebble SDK). Now Playing is a port of the official music app (`fw/apps/system/music.c` in coredevices/PebbleOS) to the public SDK; that file uses private firmware APIs, so it is a port of layout and behaviour, not a copy. Licence: PebbleOS is Apache-2.0, compatible with Hocket's AGPL-3.0.
- **The phone side** (Android, inside `PlaybackService`, which already owns the core). Talks to the watchapp over **PebbleKit 2**, which Core's phone app (libpebble3) supports. It translates watch messages into existing `Command`s / `Query`s and pushes state back. No business logic, per the platform-layers-are-thin rule.
- **A small versioned message protocol** between them: now-playing state, commands, paged list requests/responses, album-art chunks, settings, and a version handshake.

We cannot replace the system music app; this is a separate "Hocket" watchapp in the launcher. No PebbleKit JS and no network hop: it works offline, and controls desktop playback through Connect like any other Hocket remote.

`api.rs` did not change: rating, love, shuffle, repeat, seek, handoff, search, lyrics and library queries already existed.

**Protocol** (`pebble/src/c/hocket.h`, mirrored by `PebbleProtocol.kt`; bump `HOCKET_PROTO` on any incompatible change). Phone → watch: Now (state), List (one page of ≤ 12 packed rows), Art (chunks), Settings (a small blob the watch persists), HelloReply, Notice. Watch → phone: Hello (protocol + build), Cmd (command, argument, list path and row id), ListReq (path + offset), ArtReq. Lists are addressed by path: `queue`, `library`, `playlists`, `albums`, `artists`, `recent`, `downloads`, `devices`, `lyrics`, `album:<id>`, `playlist:<id>`, `artist:<id>`, `search:<text>`, `goAlbum`, `goArtist`; a browsable row's id is itself a path. The phone pushes Now (debounced) and Settings only while the watchapp is open.


## Watches  
*Status: decided*

All current watches. Touch features apply to the Pebble Time 2; everything else uses buttons-only mode (below). Album art on Time 2 and Round 2 only, as in the official app. List data is paged from the phone so it fits the smaller app heap on the Core 2 Duo.


## Reference: the official music app  
*Status: reference*

What "exactly like the official app" means, read from `music.c`.

**Layout (rect, no art).** Status-bar clock; light grey background on colour (white on B/W). Cassette icon top left (pause icon while paused; volume icon for 2 s after a volume change). Artist (1 line), title (2 lines, more at large text size), elapsed / total, progress bar. Action bar on the right. Sizes follow the system text-size preference.

**Buttons, "Show volume controls" on (default).**
- Up / Down: previous / next. Every press sends a command, so repeated presses skip several. While playing, the cassette slides out left and back in from the right; on a title change the text slides up and bounces back in. If the player says skipping seeks within the track, the icons become rewind / fast-forward.
- Select while paused: ▶, press plays.
- Select while playing: **…** icon. Press switches the bar to volume mode: up = volume up, down = volume down, select = play/pause. Up/down repeat every 400 ms when held, haptic tick on repeats. Reverts to skip mode after 2 s without a press; any press restarts the timer.
- Hold up / hold down: volume up / down immediately, repeating every 400 ms with haptics; bar shows volume icons while held, reverts on release.
- Hold select: play/pause with a haptic buzz.
- Back: exit.

**"Show volume controls" off.** Select is always play/pause; no holds.

**Progress.** "Show progress bar" on by default; updated every second while playing, interpolated on the watch. Off: a wrist flick (accel tap) shows it for 5 s. Hidden when the player doesn't report position.

**Nothing playing.** Separate screen: image and "START PLAYBACK ON YOUR PHONE". Back exits.

**Album art.** "Show album art", off by default. Time 2: cover fills the screen, outlined centred artist, single-line scrolling title beside the cassette (a couple of cycles after a track change, then rests), large bold outlined clock, no slide animation on track change. Art requested 500 ms after a track change (coalescing metadata bursts), retried twice at 3 s; the old cover stays until the new transfer starts. Round 2: always a centred layout with progress as an arc around the bezel.

**Touch (Time 2).** Taps only count on the action-bar icons (`window_set_touch_tap_requires_action_bar`), so touching the cover or text never toggles playback. With system touch navigation on, swipes become button presses (`touch_nav.c`): up → down button (next), down → up button (previous), left → select, right → back.


## Now Playing  
*Status: decided*

Identical to the reference above, in both modes, with these differences:

- **Touch gestures replace the system's swipe-to-button mapping** on Now Playing (the app opts out of touch navigation on this window and handles touch itself). Defaults: swipe **up** = Queue, swipe **left** = Library, swipe **right** = Back, swipe **down** = nothing, **press and hold** = Actions menu. All configurable (Settings).
- **Nothing playing** shows a **Resume** option (the last queue) instead of a dead end.
- **Star rating replaces the cassette.** Five stars (filled up to the rating; black-and-white watches draw only the filled ones, so unrated shows none) sit under the progress bar, left-aligned at the text margin (inside the circle on round). The volume icon still flashes in that spot for 2 s after a volume change. There is no paused-cassette icon (the play/pause icon shows the state) and no cassette slide on skip. With album art the title scrolls on its own line over the foot of the cover.
- **Volume: open.** Pending the Connect volume migration. The reference controls are kept; for now they drive the phone's music volume (as the official Pebble app does), through one placeholder in `PebbleBridge` (`changeVolume`).
- The official app's three preferences (volume controls, progress bar, album art) are watch prefs a third-party app can't read, so Hocket has its own copies (Settings) with the same defaults, except **album art on** by default (Hocket already has the covers cached).


## Touch mode and buttons-only mode  
*Status: decided*

**Touch mode** (default on Time 2). Opens straight into Now Playing. Back exits. Gestures as above.

**Touch navigation setting.** The SDK has no way to read the system touch-navigation setting, so Hocket has its own On / Off setting (default Off) that the watch obeys:
- **Off:** touch is used only for the swipe and press-and-hold gestures. Nothing is selected by tapping, in lists or on Now Playing; selection is on the buttons.
- **On:** taps also work: action-bar taps on Now Playing (only on the icons, like the official app), tapping list rows to open or play them, swipe up/down scrolling lists.

No upstream PebbleOS change is planned for this.

**Buttons-only mode** (always on watches without touch; a setting on Time 2). Opens straight into Now Playing with the official button behaviour. **Back** opens the menu:

```
Now Playing
Queue
Library
───────────
♥ Love
Rating ★★★☆☆
Shuffle: Off
Repeat: All
Go to album
Go to artist
Play on…
```

- Picking an action applies it and returns to Now Playing. Exceptions that open their own screen first: Rating (star picker, then Now Playing), Go to album / artist, Play on….
- **Timeout:** after 5 s without a button press the menu returns to Now Playing; any press restarts it. Opening Queue or Library cancels it.
- Back from the menu exits the app (quick double back). Holding back exits as everywhere.
- Contents and order are configurable (Settings).


## Actions menu  
*Status: decided*

Press and hold on Now Playing (touch mode), or the lower half of the menu (buttons-only):

- Rate (1–5 stars) and Love
- Shuffle, Repeat (off / all / one)
- Go to album, Go to artist
- Play on… (Connect handoff)

Contents and order configurable. Lyrics can be added (off by default).


## Lyrics  
*Status: implemented*

A page of line-synced lyrics for the playing track (line timing only, no word timing; unsynced lyrics show as a static list). Off by default: no gesture opens it unless one is set to Lyrics in Settings; it can also be added to the buttons-only menu or the actions menu.

- It follows the current line, which is shown large in the middle.
- Up / down move the focus to other lines and stop following. Select seeks playback to the focused line's start and resumes following.
- Back while browsing returns to the current line (following again); back while following closes the page.

The phone sends the lines as the list `lyrics` (row id = start in ms with the track's timing offset applied, empty when unsynced).


## Queue  
*Status: decided*

Current track at the top, upcoming below; scrolling up reveals recent history (the last few tracks). Selecting a track jumps to it; holding offers Play next / Add to queue.


## Library  
*Status: decided*

Sections (contents and order configurable): Playlists, Albums, Artists (artist → Play, Shuffle, then albums → tracks), Recently played (tracks), Downloads (the pinned albums, playlists and tracks, so it works offline), and Voice search.

- **Track:** select plays it now; hold for Play next / Add to queue.
- **Album and playlist screens are the same:** **Play** and **Shuffle** at the top, then the tracks.
- **Voice search:** the watch's dictation, then a results list: "Play top result" first, then matching tracks, albums, artists and playlists.


## Settings  
*Status: decided*

A Pebble page in the Android app's Settings. Keys are `pebble.*` in the core settings registry with `SettingScope::DeviceLocal` (the watch belongs to this phone), using the same reorder-and-toggle editor as `nav.mobileBar`. The phone pushes them to the watch, which persists them so they apply while disconnected.

- Touch navigation: On / Off (default Off)
- Buttons-only mode (Time 2)
- Gestures: swipe up / down / left / right and press-and-hold, each one of Queue, Library, Actions, Back, Lyrics, Nothing. Defaults as in Now Playing. The back button always goes back, so no assignment can trap you.
- Buttons-only menu: items and order. Items can be screens (Now Playing, Queue, Library, Lyrics), actions (Love, Rating, Shuffle, Repeat, Go to album / artist, Play on…), or shortcuts (Playlists, Albums, Artists, Downloads, Search).
- Actions menu: items and order
- Library sections: items and order
- Menu timeout: Off / 3 / 5 / 10 s (default 5)
- Show volume controls (default on), Show progress bar (default on), Show album art (default on; Time 2 and Round 2)
- Open on watch when playback starts (default off)
- Install / update the watchapp, with installed-version status (the version the watch last reported on connect)


## Distribution  
*Status: decided*

**Sideloaded from Hocket, not the Pebble app store.** No dependency on an external service, and the watchapp always matches the phone app's version.

- CI builds the `.pbw` and bundles it in the APK. Settings → Pebble → **Install on watch** hands it to Core's phone app (its manifest accepts `.pbw` via `ACTION_VIEW`, including `content://` URIs).
- **Version handshake:** the watchapp reports its protocol and build version on connect. On mismatch the phone shows a notification with Install, and the watch shows "Update from Hocket on your phone" instead of half-working.
- A sideloaded app doesn't follow you to a new phone or survive a watch reset. PebbleKit 2 cannot list installed apps, so the Install button is always there.
- Build: `scripts/build-pebble.sh` stages `pebble/build/android-assets/hocket.pbw`, which the app module adds as an assets source when present. The bundled version (from `pebble/package.json`) is `BuildConfig.PEBBLE_WATCHAPP_VERSION` in the playback module.
- Builds without the Pebble SDK hide the Install button.
- The app UUID is fixed, so a store listing can be added later without breaking anything.

PebbleKit 2 works only with Core's phone app. A PebbleKit Classic fallback (for Gadgetbridge) is possible later.


## Implementation order  
*Status: done*

1. Protocol, Android adapter, Now Playing parity in both modes (including the buttons-only menu and timeout).
2. Actions menu.
3. Queue and library, including album / playlist screens and the Downloads filter.
4. Voice search, album art, auto-open, settings page, install flow and version check, CI job for the `.pbw`.

Testing: `HOCKET_DEMO=1 pebble build` gives the emulator sample state for screenshots (`docs/screenshots/pebble/`), and `PebbleProtocolTest` covers the byte layouts. Automated emulator screenshot tests per platform are still to do.


## Open  

- **Volume:** what the volume controls drive (phone volume, or the playing Hocket device's volume). Waiting on the Connect volume migration. Today: the phone's music volume, via the placeholder in `PebbleBridge.changeVolume` / `volumePercent`.
