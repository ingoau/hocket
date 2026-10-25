# Hocket — design notes

A Navidrome client for Android and desktop. Multi-device playback that keeps working when the network doesn't.

Hocket — a medieval technique where one melody is split between two or more voices, each supplying alternate notes. The line is continuous; no single performer plays all of it.


## The shape of it  
*Status: decided*

One Navidrome server holds the music. Several devices can play it, exactly one at a time, while the others watch, control, and can take over. A coordinator ties them together across networks, and on a LAN they find each other without one.

> Figure: Solid: audio, always server to player, never proxied through another device. Dashed: session state, over the LAN or through the coordinator, or nowhere at all.


### Three principles everything follows from

- **The session document is the unit of state.** Queue, context, cursor, history, shuffle order, and saved snapshots are one small serialisable thing. Sync, undo, and saved queues are all operations on it.
- **The coordinator is transport and replica, never authority.** Every device holds a complete session locally. A device alone is a session with one participant.
- **Platform code lives behind two seams.** `PlaybackBackend` and `MediaSessionAdapter`. Everything above is shared, everything below is thin.


## Shared core  
*Status: decided*

A **Rust core** with generated bindings: UniFFI for Kotlin, napi-rs for Node. The argument that settles it is the coordinator, which becomes a fourth build target of the same crate rather than a second implementation that drifts.

> Figure: The DSP sits in the core deliberately: the same biquads run on both platforms, so an EQ preset can't sound different on the phone than on the laptop.

The core is an actor. Commands in, events out, nothing reaching into its state synchronously. Both platforms get the same shape — Android's playback service and Electron's main process each own an instance and forward events to the UI. It also makes the Connect state machine testable against a fake clock.


### How it gets tested

Deterministic simulation. The core exposes a harness with a virtual clock and an in-memory network that can partition, delay, drop and reorder on command. Randomised multi-device scenarios run with invariants asserted throughout: never two transport owners, revisions monotonic, no scrobble lost or duplicated, queue reducer total. Property tests on the reducer itself. This is the highest-value testing investment in the project.


### Wire format and migrations

- JSON over WebSocket, Rust types as schema authority, TypeScript generated from them. Being able to read the protocol in a log is worth more than the bytes saved.
- Session documents carry a schema version; the handshake negotiates a minimum common protocol and refuses below a hard floor. **Older devices must round-trip unknown fields untouched**, or a stale device silently strips new state every time it writes.
- Forward-only numbered migrations locally. The mirror is a cache, so a failed migration can drop and re-sync; downloads, outbox, saved queues and settings are not rebuildable and get real migrations plus a backup.
- No network telemetry. Local crash logs with a copy-diagnostics action. *(Since: with developer mode on, a bundle can also collect the Connect room's state and the other devices' bundles over Connect. Each device answers only while its own developer mode is on. The hosted coordinator adds its version, and its whole process log only for the accounts its operator names with `--diagnostics-user`. Nothing goes anywhere but the user's own devices.)*
- Strings externalised from the first commit.


## Server baseline  
*Status: decided*

**Navidrome 0.63.0 is the minimum.** Sidecar lyrics landed in 0.63, `sonicSimilarity` in 0.62, and 0.63 also made full-library sync via `search3` flat at every offset. Setting the floor there collapses most of the capability matrix into "assume it's there".

| Concern | Decision |
|---|---|
| Capabilities | Probe `getOpenSubsonicExtensions` plus a native-API ping on connect, store a capability set per server as a first-class record. Never feature-detect by attempting an action and catching failure. |
| Auth | Token+salt today, which means retaining the password in the platform keystore. `apiKeyAuthentication` is tracked by Navidrome but has no implementation yet; the probe lights it up when it lands, and then the password can be discarded. Never plaintext, never the `jwt` parameter. |
| Coordinator auth | Verifies a client by proxying a `ping` with the presented credential. It never holds credentials itself. |
| Transcoding | Client is authoritative about what it can play. Use the OpenSubsonic transcoding extension where advertised, otherwise explicit `format` and `maxBitRate`. Profiles vary per network. Document that server-side player profiles should be left permissive. |
| Smart playlists | Rules can only be changed by editing the .nsp file or through the native API, and a smart playlist's track list is read-only — so no drag-reorder or manual add in the UI for one. |
| Native API | Undocumented and unstable; Feishin reverse-engineered it and an official rewrite is planned. Keep native-API writes behind a capability flag and don't depend on them. |
| Multi-server | No UI yet. `serverId` on every entity table and in every queue, outbox, download and snapshot reference. A `ServerRegistry` that always returns one element for now. Download and cache paths include it. |
| Households | Sessions are strictly per-user, scoped by the Navidrome identity the device authenticated with. |


## Queue model  
*Status: decided*

The queue is derived, never a stored flat list. Four pieces: a context with its order, a cursor, a shuffle permutation, and an insertion list. Apple Music behaviour and YouTube Music behaviour are two reducers over identical state.

> Figure: Shuffle is a permutation over the context, so turning it off restores the original order and your place in it.


### Nothing is consumed

Advancing does not destroy a queued item. The session holds a materialised history of what actually played, with the cursor at the boundary. Next pops from the future and appends to history; previous pops from history and pushes back to the front of the future, preserving what kind of item it was. Next and previous stay inverses, so one stray tap loses nothing.

> Figure: In the consume-on-advance model used by some players, tracks 2 and 3 would both be gone by this point, unreachable in either direction.

- Played queued items don't replay. Persisting in the timeline means reachable by pressing back, not played again.
- History caps around 200 items; past that, previous falls back to context order. History travels inside saved-queue snapshots.
- Previous walks real history rather than running the shuffle permutation backwards, so reshuffling mid-session doesn't scramble where you've been.
- Repeat-all loops the context only. Repeat-one repeats whatever is playing regardless of where it came from.
- The queue view is one scrollable timeline: history above, current, upcoming below. The model should be visible rather than inferred.


## Saved queues  
*Status: decided*

Playing something new replaces the queue and the old one is kept. Snapshotting derived state costs a few hundred bytes rather than a track list copy.

| Rule | Behaviour |
|---|---|
| Auto-save | On context replacement. Skips trivial queues: single tracks, contexts never played from. |
| Cap | 10 unpinned, configurable 0–50. Zero disables auto-save, leaving only pinned queues. |
| Eviction | Least recently interacted with, where interaction means played from or restored. |
| Expiry | Unpinned entries drop after 30 days regardless of position. |
| Dedupe | Keyed on context identity, so replaying an album bumps one entry rather than adding three. |
| Pinned | Never evicted, never expire, don't count toward the cap. |
| Restore | Includes history and playback position, so you land mid-track with your back button intact. |
| Sync | A last-write-wins set keyed on context identity; pin state rides along as another LWW field. |
| Size | Most reference their context by ID. Ad-hoc contexts — a search result, an arbitrary selection — store the track list, so cap on total bytes with 10 as the count ceiling. |

Surfaced in the queue view's selector (the queue's name drops down the recent and pinned queues to restore) and in the side panel's Recent queues drill-in, with a save-as-playlist action writing through `updatePlaylist`.


## Global undo  
*Status: decided*

Every mutating action is a command object with an inverse, registered alongside its label in the action registry. Undo then works for features not yet written, which is the real reason to do it.

| Tier | Covers | Mechanism |
|---|---|---|
| Session state | Queue ops, context replacement, reorder, shuffle toggle, clear | Snapshot. Instant, always undoable, shared across devices in the session. |
| Remote mutations | Ratings, loves, playlist add / remove / reorder | Still in the outbox: cancel. Already sent: inverse from per-item prior state, run through the job queue. Device-local. |
| Confirm instead | Playlist deletion, download deletion, config restore | Dialog, no stack entry. No faithful inverse means no stack entry. |

- **Selection is metadata, not an entry.** Each command snapshots the selection it acted on and restores it on undo. A large selection destroyed by something non-undoable gets its own "Selection cleared · Restore" toast instead.
- **Inverses compare-and-swap.** Check the current value still matches what the command set, skip if not, and report honestly: undid 487 of 500, 13 changed elsewhere.
- Coalesce rapid repeats — dragging a rating, nudging a track, holding a key.
- Redo on Ctrl+Shift+Z and Ctrl+Y; a new action clears the redo stack.
- Focus in a text field means Ctrl+Z belongs to the field.
- Bounded by total bytes, not entry count. Entries stamped with the originating device so shared undo can't revert another device's deliberate action.
- Android has no Ctrl+Z, so the stack needs a history sheet — shares a panel with Recent queues.
- Undo on accidental queue replacement carries the outgoing track's position, or you land in the right place with the track restarted.


## Connect  
*Status: decided*


### Split authority

The session document owns queue, context, history, shuffle, repeat and snapshots. The device currently playing owns transport: position, playing or paused, buffering. Mutations are ops against a monotonic revision, applied optimistically and rolled back on rejection, so two people hitting next produces one skip.

Position is never broadcast on a timer. Send `(position, takenAt, rate, isPlaying)` and let receivers extrapolate, with clock offset estimated from the minimum round-trip sample. The lyric renderers consume this same extrapolated position, so lyrics stay correct when audio is playing elsewhere.


### Volume

The volume control always controls the speaker that's playing.

- **Each device owns its own volume** (level and mute). The playing device's rides on its transport stamps, so every other device's control shows it, labelled with its name on desktop ("Volume – Pixel 8") and in the phone's Connect sheet. Moving any control while another device plays sends that device a `SetVolume`; when nobody plays, a control sets its own device's volume, ready for next time.
- **Handoff keeps each device's level.** Moving from headphones on the phone to the desk speaker plays at the level last used on the speaker. Mute remembers the level from before it, per device.
- **On the phone the volume is the system music volume**, with no second volume inside the app: the keys, the system slider and the in-app slider are one thing, and changing it any other way (another app, a headset) shows on every other device. While another device plays, the phone's volume keys and the volume slider in its system media controls drive that device, and the system volume panel names it. The Pebble's volume buttons drive whichever device is playing.
- **On desktop the slider is the app's own volume**, separate from the computer's, on a dB scale (−50 dB to 0, tapering to silence) so equal steps sound equal.


### Discovery and tiers

> Figure: Any client can perform the coordinator role, so the hosted server is a headless build of code that already ships in the desktop app. mDNS covers the LAN; the remote case is a typed URL, with the last-known address tried first.


### Handoff

Not instant, deliberately. When the picker opens, targets pre-buffer while the current device keeps playing, so there's no dead air while you decide.

> Figure: Cap the pre-buffer fan-out; targets discard buffers on a timeout if nobody picks them.

- **Any device can pull playback to itself.** Picking "this device" asks the owner, through the room, to hand off as if it had picked it. If no handoff lands within 3 s (an older coordinator that drops the request, an owner that is gone), the device takes the lease over from the owner's last stamp.
- **Sleeping devices are hidden.** Presence means currently connected, which removes the push-notification problem entirely.
- **Scrobbling follows transport.** Same token, unambiguous, works offline through the outbox the playing device already has.
- **Accumulated `playedMs` travels with the track**, so taking over 90 seconds in doesn't silently eat the scrobble.
- **Dedupe is the coordinator's one optional job** — a short log of submitted `(trackId, startedAt)` pairs.
- **Unplayable items skip.** A device that can't play an item marks it unavailable and moves on, with a line in the player. The handoff doesn't fail, playback doesn't stop, and nothing scrobbles.


## Resume and recovery  
*Status: decided*

The coordinator keeps a replica of the session and the saved queues so a device that wasn't present can pick up. It stays a cache with an expiry: every device persists its own complete session, and if the replica and a live device disagree, the live device wins.

| Stored | Detail |
|---|---|
| Session | Context, cursor, history, shuffle permutation, insertion list, and `(position, takenAt, isPlaying)` as last received. |
| Saved queues | The LWW set, scoped per Navidrome user. |
| Leases | Per-device last-seen and the current transport lease. |
| Not stored | Play history, library metadata, anything re-derivable. A resume point, not a log. |
| Write cadence | Track change, queue mutation, play/pause, plus heartbeat. Never on position ticks. |


### Split-brain, not storage, is the hard part

A device going quiet might have died or might have lost network and kept playing. Transport ownership is a lease with an expiry, renewed by heartbeat every 5 seconds and lapsing after 20, carrying a monotonic epoch that increments on every ownership change. A device reconnecting inside the window keeps ownership. Deliberate takeover from the picker always works immediately — the lease only covers failure.

> Figure: Without a fencing token both devices end up playing at once, and it only shows up on flaky wifi.


### Resuming is always explicit

- A returning diverged device doesn't merge and doesn't overwrite — its state becomes a saved queue entry.
- Never auto-resume on connect. Opening a laptop should not start playing music.
- The OS's playback resumption (Android's resume card, a headset's play key while the app is stopped) counts as explicit: it only happens when someone presses play.
- Surface it dormant in the player bar: *Pixel was playing Track · Resume here*.
- Don't extrapolate position across the offline gap. Past about an hour, snap to the start of the track.
- Without a coordinator this degrades rather than breaks: on LAN the elected coordinator holds the replica. A device that was never present and can't reach a peer gets nothing, and the UI says so.


## Offline and sync  
*Status: decided*


### The mirror

Local SQLite is the source of truth for the UI. Sync by paging `search3` with an empty query, which returns the whole library and is now flat at every offset rather than degrading with depth — it was optimised for exactly this. Incremental refresh on launch and on a timer, with a periodic full reconcile because Subsonic's delta signals miss deletions.

The app is usable immediately while the mirror builds, browsing server-backed, with views switching over as tables complete and progress shown in the job queue. Blocking first run behind a sync is a first impression you don't get twice.


### One outbox

Ratings, playlist edits, play counts and scrobbles are all durable pending mutations with retry and conflict handling. Built once, generically — it's also what makes undo free for anything still unsent. Scrobbling follows Last.fm's rules: 50% or 4 minutes, whichever first, 30-second minimum track length, "now playing" at start, seek-skipped time not counted, and every repeat-one pass scrobbling like any other play.


### Downloads

| Concern | Decision |
|---|---|
| Separation | Downloads are pinned and never auto-evicted; the stream cache is evictable. Different directories, different policies, so a night of streaming can't evict an album you took offline. |
| Pins | Songs, albums and playlists, managed from a downloads list in settings. |
| Playlist pins | Track dynamically — the playlist changing enqueues downloads and removals, visible in the job queue. Album and song pins are fixed sets. |
| Payload | Original file by default, optional per-profile transcode for space. Gain values computed and stored at download time, since transcodes routinely drop the tags. |
| Limits | No hard cap. Show total size and warn past a configurable threshold on mobile. Out of space fails loudly into the problems list rather than silently dropping a pin. |


## Audio path  
*Status: decided*

Desktop playback does not go through Web Audio. `decodeAudioData` holds the whole file as float32 PCM — roughly 21 MB per minute at CD quality, doubled for gapless preload — and the streaming alternative gives up sample-accurate transitions. So **Symphonia for decoding, cpal for output**, both in the core, with the DSP chain in between.

| Stage | Notes |
|---|---|
| ReplayGain | Track and album gain from OpenSubsonic fields, plus a user preamp. |
| Equalizer | Biquad bands in the core. Own preamp and clipping protection, since gain plus boosted bands will clip. |
| Normalisation | Same chain, so queue-wide levelling doesn't fight ReplayGain. |
| Output | Device selection per platform. Bit-perfect and sample-rate switching are reachable here in a way they never were through Chromium. |
| Gapless | On by default with a toggle. It's the natural behaviour of one continuous output stream; encoder padding is trimmed using LAME and iTunes headers, and FLAC has nothing to trim. |
| Formats | Transcoding profiles carry a "can't decode natively" list rather than only a bitrate preference, and vary per network *and per platform*. Symphonia covers ALAC and WavPack behind feature flags, so on desktop the list shrinks to roughly APE and DSD; Android still has ExoPlayer's constraints. |
| Errors | Inline in the player bar, auto-skip after a couple of consecutive failures, detail logged to the problems list. A failed track never scrobbles and doesn't count as played. |


## OS media session  
*Status: decided*

The OS APIs don't need audio — the audio requirement belongs to Chromium's `navigator.mediaSession`, which only publishes once an audible player exists in the page. Since desktop audio comes out of Rust, Chromium never has a player, so the silent-loop approach would mean holding an output stream open permanently to describe a player that lives elsewhere.

Native instead: a napi-rs addon over **playwire**. Souvlaki was the obvious candidate but has been unmaintained for over a year, with no shuffle or repeat on any platform, capability flags hardcoded true, `mpris:trackid` as a literal `/`, no `DesktopEntry`, no `MPNowPlayingInfoPropertyPlaybackRate` — without which the macOS scrubber never advances — and a macOS backend reading seek position out of a private ivar. Several of those hit features we want directly.

| Detail | Why it matters |
|---|---|
| Maturity | playwire is newer and less proven than souvlaki was. Budget for contributing fixes upstream. |
| Position | Publish every tick. playwire diffs against the last snapshot, and MPRIS `Position` is deliberately not change-signalling — clients extrapolate from rate and the Seeked signal. |
| disable-features | Switch off `HardwareMediaKeyHandling` and `MediaSessionService` in Electron, or any renderer audio at all will make Chromium grab the media keys. |
| Artwork | Resolve through the image cache to a `file://` path. A remote URL may work in one place and silently fail in another. |
| SMTC + tray | Windows ties the session to an HWND. Destroying the window on close-to-tray kills it, so anchor to a persistent hidden window. |
| MPRIS extras | Shuffle, LoopStatus, Volume and Rate as properties; Linux widgets surface them. `DesktopEntry` fixes app identity. |

One `MediaSessionAdapter`: set metadata, set playback state, receive commands. Media3 implements it on Android, the playwire addon on desktop, and both are fed by whichever device currently owns transport.


## Library and bulk edits  
*Status: decided*


### The job queue

Subsonic has no batch endpoint for ratings, so rating 3,000 tracks is 3,000 calls. The queue is general: bulk rating, downloads, library sync, playlist import and outbox flushing are the same shape — durable, bounded concurrency, resumable after a crash, cancellable. One progress indicator at the top right of the content area, near search, with a popover listing individual jobs. Playlist adds are batchable through `updatePlaylist` and stay cheap.


### Lists

- Virtualised, never paginated.
- Selection keyed by ID, not row index.
- Select-all is a predicate over the current filter, not 50,000 materialised IDs.
- Shift-range, ctrl-click, full keyboard navigation on desktop.
- Screen reader labels from the start — retrofitting them onto a virtualised multi-select list is miserable.


### Search

Local-first, instant as you type. `search3` fires in parallel only when local results are thin, appending below a divider with the container height never shrinking between the two, so nothing shifts under the cursor.


### How failures surface

Nothing interrupts. The job indicator gains a "finished with problems" state and its popover lists what failed with a retry action; outbox failures accumulate there too. The only thing that gets a toast is an action the user just took failing immediately.


## Filters and autoplay  
*Status: decided*


### One builder, four outputs

The filter model is a superset of the NSP rule vocabulary, with each field tagged server-expressible or not, so the builder says live whether a filter can be pushed server-side rather than failing at save time.

| Output | When |
|---|---|
| Local filter | Always. Full vocabulary, live, including downloaded state, cache state, local play history and sonic attributes. |
| Smart playlist | When rules are expressible and the native API is reachable. Behind a capability flag until the official API lands. |
| Static playlist | Always. Runs the query and writes track IDs. |
| Export .nsp | Always. Drop it in the playlist folder; Navidrome imports it at scan time. |

Smart playlists built on user interactions update against the owner's interactions, so per-user personalisation needs separate files with separate ownership — ownership has to be explicit in the UI.


### Autoplay

An ordered chain, because providers fail and return empty. With the AudioMuse plugin installed, Navidrome answers the standard `getSimilarSongs2` and `getArtistInfo2` endpoints with sonic analysis instead of external metadata, so the default path needs no special handling and simply gets better when the plugin is present. The `sonicSimilarity` extension adds `getSonicSimilarTracks` with a per-match similarity score, which is worth using directly: it lets the UI explain why something was queued and lets autoplay hold a quality threshold.

Sources in order: sonic similarity, Navidrome's similar-songs and top-songs and random endpoints, any saved filter. Seed from a rolling window of recent tracks rather than only the last one so it drifts with the session, keep a recently-autoplayed exclusion set, and allow per-context overrides.

Sonic attributes — BPM, key, energy, mood — also appear as filter fields, sort options and track details. Nothing more ambitious than that.


## Lyrics  
*Status: decided*

Navidrome 0.63 parses synced sidecar lyrics in TTML, ELRC, SRT, YAML and LRC with word-by-word karaoke timing and multi-voice agent layers, exposed through the OpenSubsonic v2 lyrics extensions. So syllable timing arrives over the API as structured data.

| Decision | Detail |
|---|---|
| Parsing | Adapt the v2 structured shape to a renderable model in the core. No client-side format parsers — the server has already done that work. |
| Tiers | Syllable, line, unsynced, degrading honestly. Never interpolate syllables across a line's duration to fake the top tier. |
| Multi-voice | Agent layers are rendered — per-voice alignment for duets and call-and-response, as Apple Music does. Both platforms. |
| Timing | Driven by the same extrapolated position the Connect protocol computes, so lyrics stay correct when audio plays on another device. Per-track user offset stored locally. |
| Sources | Server first. External fetch is opt-in, off by default, since it reveals what you're listening to. Everything fetched is cached. |


### Desktop rendering

**AMLL** (`@applemusic-like-lyrics`) for the lyric display — it targets the iPad Apple Music look, and its word gradient transitions are built on the Web Animation API. Its lyric player is DOM-based, so it brings no WebGL renderer with it.

**Kawarp** for the fluid background, not AMLL's own background component. AMLL's background needs PIXI as a peer dependency; Kawarp is zero-dependency WebGL and runs its Kawase blur on small textures only when the image changes, leaving per-frame work as just the warp shader. Feed it from the artwork cache via `loadBlob`, and tune tint and saturation to match AMLL's lyric styling.


### Android rendering

Native Compose, syllable-synced from the start, matching Apple Music as closely as possible: active line focus, depth-of-field blur on inactive lines, continuous syllable gradient sweeps, sub-voice styling. No WebView. `mldl-android` and Spicy Player are both native Compose implementations worth reading as reference, licences permitting. AGSL on Android 13+ is the Kawarp equivalent for the background.


### Performance budget

AMLL's own guidance: mainstream CPUs from the last five years manage 30fps, 60fps wants 3.0GHz or higher, 144fps wants 4.2GHz. That's the lyric component alone, before a fullscreen WebGL background. The fullscreen player is the one screen someone might leave open for an hour.

- **Hidden, minimised or fully occluded** → stop both renderers. `requestAnimationFrame` throttles here anyway, but stop explicitly so nothing wakes the GPU.
- **Visible but unfocused** → keep animating at a reduced rate, around 24fps for the background. Pausing on blur alone freezes the second monitor, which is wrong.
- **Focused** → full rate.
- The animated background has its own always-off setting, independent of battery saver.


### Battery saver

A mode, not a toggle, with "engage automatically on battery" on by default: animated background off with a static blurred still in its place, lyrics capped at 30fps, artwork resolved at the smaller cache size, background prefetch and speculative pre-buffering paused.


## Player features  
*Status: decided*

| Feature | Decision |
|---|---|
| Ratings | Tracks and albums, not artists. Stars and loves fully independent; an optional one-way bridge sets loved above a configurable star threshold, off by default. Subsonic ratings are integers 0–5, so no half stars. |
| Caching | Images, lyrics and metadata, each with its own policy, separate from downloads. Artwork requested at a small set of fixed sizes and cached per size — never downscale a large one for a grid. |
| Sleep timer | Including stop-at-end-of-track. |
| Listening stats | Built from local play history, so it works without server support. |
| Settings | Every key carries a scope flag — device-local or account-synced. Synced keys ride through the coordinator, merged LWW. A master sync toggle, on by default. |
| Config backup | A versioned document with migrations. Secrets handled separately. |
| Customisation | Choose-and-order over a curated action set: context menu items, sidebar items, media session buttons, keyboard rebinding, and the swipe actions of song rows (left and right, the queue apart from other lists). Accent colour and dynamic colour from artwork. Not arbitrary layout, not a theming engine. |


### The action registry

Every user-visible action is a first-class object: id, label, icon, applicability predicate, handler, inverse. Menus, context menus, keyboard shortcuts, the command palette, media session buttons, remote actions and undo labels are all generated from it. Without this, each customisation setting is its own plumbing and undo has to be built per feature.


## Interface  
*Status: decided*


### Desktop layout

Material 3 Expressive throughout: the shape scale, toggle buttons that fill and square off when on, a press that squeezes a button toward a rounded square on a spring, emphasized type, and 40px icon buttons (44px in the fullscreen player).

- Left sidebar for navigation, main content area, the now playing side panel on the right, and the player bar along the bottom on the window's own surface (no divider). The content and the side panel are floating panes with an 8px gutter.
- **Player bar.** Left: the artwork, title and artist, the rating (Settings → Appearance can hide it in the player), add to playlist and More (the song's menu, which carries love). Centre: shuffle, previous, a wide filled play/pause that morphs from a pill (paused) to a rounded square (playing), next, repeat, and the wavy seek bar with the elapsed time and the duration either side. Right: the sleep and battery badges, volume, then toggles for the side panel, lyrics and queue, the Connect pill (the device icon; the other device's name on the accent only while another device plays), the mini player and the fullscreen player. Below 900px it takes two rows.
- **Side panel.** One scrolling pane, resizable by its left edge (a drawer below 900px). The overview: the artwork (crossfading on track change; a cover whose bottom edge the classifier carries on runs edge to edge at the top of the pane, see Immersive artwork), title and artist with add to playlist and More, then tonal cards: Lyrics (the previous, current and next lines, following playback; hidden when the song has none), Up next (the next three songs) and About (album, genre, plays, format). Each card's link drills in, with a back arrow: Lyrics (the synced lyrics; a tune button in its header opens Lyrics options), Queue, About this song (the details, the file with Show in folder, similar tracks; the same view as the fullscreen player's) and, from the queue, Recent queues. Drill-ins slide along the x axis. The bar's lyrics and queue buttons (and L, Q) open the panel straight on that view, and close it when it is already showing it; the side panel button opens the overview. Open, view and width are remembered on the device. The old queue/lyrics split is gone: one view at a time has the whole height.
- **The artist** under the title (the bar, the side panel's overview, the fullscreen card) is the track's artist string as given ("Marlowe Reed feat. Juno"), one line, no album. Clicking it opens a small menu anchored to it: each artist on the track (to that artist's page, leaving the fullscreen player), a divider, then the album (cover, name, album artist; to the album's page). The title still opens the album.
- **Queue view** (side panel and fullscreen): a header with the queue selector (the queue's name and a chevron, dropping down the recent and pinned saved queues to restore, each with its menu for pin, save as playlist and delete, and a link to them all) and the Autoplay toggle; then the timeline (history above, now playing, playing next, the rest of the context, drag to reorder) and, with autoplay on, a note that it carries on with similar songs.
- **Fullscreen player** (F): the blurred, moving Kawarp background; the collapse button top right with the window fullscreen toggle beside it (no "Playing from" header: the queue's name is the queue view's selector); a controls card (title, artist, love, add to playlist and More, the rating, the wavy seek bar, the transport) as glass; and a pill bottom right with the Connect device (named only when another device plays), the Lyrics, Queue and About toggles (the one that is on goes back to the artwork alone), the sleep timer and volume (a popover). It opens on the view it was left on, the queue the first time; Q, L and Escape work as elsewhere. Three layouts:
  - *Columns* (card covers, windows wider than tall): the rounded artwork with the card under it, centred; a view moves the column to the left and takes the right. In every wide layout a view is one width, clamp(400px, 36vw, 640px), so its lines are as long whatever the cover; only where it sits differs.
  - *Immersive* (3:2 or wider, with a cover the classifier carries on): the artwork runs full height, flush to the top and bottom; the card floats over its bottom like Cider's. When both its left and right edges carry on, the artwork is centred with a continuation either side; when only the right does, it sits on the left, carried on to the right (a cover whose right edge is a card is a card). Opening a view slides a centred artwork to the left (a spring; with reduced motion it fades in there), its card with it, the left continuation going off the edge with it while the right one dissolves as below; closing the view slides it back. With the artwork alone, during playback, the card, the pill and the header fade out once the mouse has rested three seconds, and come back on any movement or key; never while paused, while the keyboard is in them, or with a menu open, and a local setting turns it off. A view shortens the carried-on part, which dissolves into the moving blur over a wide, soft edge (for a crisp flat extension the artwork's own last 30% fades too, so there is no hard seam against the view), with a scrim coming in beside the artwork, so the view's white text holds 4.5:1 even over a light continuation; the view sits on that; the card does not move and keeps its glass.
  - *Stacked* (narrow or tall windows): the phone's layout, the artwork (or the view, with a thumbnail by the title) over the card; a tall window's immersive cover runs full width on top, carried on below.
- **Lyrics options** (the sync type and source, the per-track offset, the lyrics size) are a small popover from a tune button, never a row over the lyrics: in the side panel's lyrics view header (it drops below), and from a right-click (or the context-menu key, Shift+F10) on the lyrics toggles in the player bar and the fullscreen pill (it rises above them; a click still toggles the lyrics). Nothing is added to the pill for it, so its buttons never move between views.
- Another device's dormant resume offer (the core's ResumeOffer) is not shown in the player; the core still tracks it.
- Job status and the problems indicator top right, near search — library activity, not playback.
- Always-on-top mini player: artwork thumbnail, title and artist, transport, progress. No queue, no lyrics.


### Immersive artwork

Apple Music's full-bleed artwork, without its animated covers (which we don't have): the full player's artwork runs edge to edge and is carried on past its edge, or stays a card when that would look wrong. One setting, `display.immersiveArtwork`: Automatic, Always (never a card for a square cover), Never.

| Style | What it draws | When |
|---|---|---|
| Mirror | The artwork flipped past its edge, blurring more with distance, fading into the moving background. | Nothing near the edge looks wrong flipped: no close-up face, no text, logo or sticker in the zone a reflection shows, not so busy the controls drown. |
| Extend | The colours along the edge carried on, softening sideways with distance. An edge already in one colour carries it on with a crisp edge, no fade: a white cover stays white all the way down, with dark controls. | The edge is plain: its very last row is one colour and the rows just above it are too (a band, a backdrop; a thin border under busy art is not), or most of the strip is (text on a plain background); or each column holds its colour (a horizon, a gradient). Crisp only when the row or 95% of the strip is one colour and the cover is smooth there (a fine grain, measured on the ~256 px cover the apps send rather than the 128 px resample, rules it out); a plain but textured edge (dark ground, grain, a ceiling tile) fades in like any other. |
| Card | The artwork as a rounded card on its own blurred, moving colours. | Everything else: close-up portraits, busy covers, text across the bottom, non-square art. |

- **The decision is the core's** (`hocket_core::artwork`), a pure function of the cover's pixels and the faces the platform found: both platforms decide the same, and it is tested once. Everything is measured on the cover resampled to 128 px in OKLab. Text and logos are small shapes standing out from the median of their surroundings (a median keeps edges, so the boundary of a large shape never counts) on a plain background (foliage and crowds are texture, not text), at two scales so bold titles count too.
- **Faces**: platform-side, only close-ups (at least a tenth of the width; a figure seen whole reflects like one standing by water). One rules a mirror out when it reaches into the zone a reflection shows sharply (the outer 12%, as for text): a portrait whose face ends clear of the edge mirrors its neck and shoulders, the face itself blurred past reading. Any face, wherever it is, rules out extending column by column (it would smear skin and clothes down the screen). Android's box, worked out from the eyes, ends between the mouth and the chin: a chin tip alone at the seam doesn't read as a face upside down, the mouth and nose do. YuNet's box takes in the chin, so it rules out a little more. Android uses the framework's `FaceDetector`, no dependency; the desktop has none yet. On a 504-cover library a frontal-only detector found faces on 72 covers where YuNet found 170: a better detector is the main thing left.
- **Four edges at once**: the bottom for portrait screens and narrow windows (and the desktop side panel's overview), the top for the room above the artwork on a phone (under the status bar and the player's handle; the art at the bottom of that strip, carried on upwards), the right and the left for wide windows (the artwork at the window's full height; the desktop floats its controls over the artwork's bottom). The left edge is decided like the right on the cover mirrored left to right, so a cover plain on one side only carries on that side. Wide, the desktop centres the artwork when both sides carry on, keeps it on the left when only the right does, and shows a card otherwise; a view always puts it on the left. Between the shapes, a card. Android reads `bottom` and `top`; the desktop reads `bottom`, `right` and `left`, plus `topLight` (the strip under its header). (`left` and `top` are newer: a layout from before them decodes to a card there, and a reader that doesn't know them ignores the keys.)
- **The side panel's overview** (desktop): with a bottom edge that carries on, the artwork runs edge to edge at the top of the pane (the pane's own corners clip it, no inset or shadow) and an extension fills the whole overview in its colour (its columns carrying on where it has them), down to the end of the scroll and never fading back, and every card is glass in the edge's tone over it (light with dark text over a light extension, dark with light text over a dark one), title, links and accents included. A reflection runs on behind the title and fades, behind the first card under it (Lyrics, or Up next without lyrics; from the title's bottom to that card's bottom, along a long smoothstep; that card near-opaque in the end colour so its text reads the same throughout), into its end colour: the cover's bottom strip averaged, toned (keeping its hue) so the text over it reaches 4.5:1; that colour then fills the rest of the overview like an extension's. Near the artwork, where its own detail still shows through a reflection or an extension's bridge, the canvas adds whatever scrim the text there needs, measured on what was drawn. The panel's scrollbars are hidden (wheel, trackpad and keys still scroll). It scrolls with the overview, switches with the scene on track change and redraws when the panel's width changes. The drill-ins are unchanged; a card cover keeps the rounded card.
- **The seam never cuts over**: the artwork's last stretch (18% on Android, 30% on the desktop, with a vignette there so the fade curves in towards the corners) fades onto a blurred copy of itself, and the continuation starts from that same blur, as one image. Except a crisp extension, where the artwork already ends in the colour carried on: only its last 4% fades, onto that colour, so a cover a level or a grain off it shows no line; and the colour carries the cover's grain (the core's measure, at least a faint dither), top and bottom, so a texture the classifier missed in the thumbnail doesn't stop dead at the seam.
- **Contrast**: the controls follow the app's theme, unless what sits behind them (the continuation, not the whole cover) is almost all dark or light (80%), when they follow it. Centred, one tone serves both sides: theirs where they agree, the theme's where they do not, with the scrim that tone needs over the side that disagreed. The core sizes the scrim over the continuation for 4.5:1, and from the controls down the player fades partway (60%) to black under light controls, white under dark ones.
- **Rendering is a still**: drawn once per cover and window size (the desktop's canvas, Compose layers on Android); only the background beneath moves. Mirror's progressive blur needs Android 12; before it the reflection only fades.
- **Changing track, collapsing**: the desktop switches the artwork, its layout and the continuation together once the new cover is decoded and classified (the next track's ahead of time), crossfading the whole scene, so a new cover never sits beside the last one's continuation. On Android the artwork flying back to the mini player carries its fade out as it goes, its corners rounding to the thumbnail's.
- **Tuning**: `scripts/artwork-eval.py` runs the classifier over a folder of covers and draws what each would look like, grouped by style.


### Keyboard

| Key | Action |
|---|---|
| Ctrl/Cmd+K | Command palette — library results and actions in one list, results first, indexed from the action registry |
| Ctrl/Cmd+F | Search within the current list or playlist |
| Space | Play / pause |
| ← → | Seek |
| Shift+← → | Previous / next track |
| Q · L · F · M | Queue · lyrics (the side panel's views, or the fullscreen player's) · fullscreen player · mini player |
| 0–5 | Rate the selection |
| Ctrl+Z · Ctrl+Shift+Z | Undo · redo |
| Ctrl+A · Delete | Select all · remove from playlist or queue |

All rebindable. Media keys arrive through the native media session, not here.


### Empty states

Server setup is the entire first screen, nothing else. Every other empty state names the one action that fills it — an empty downloads list says how to pin something, an empty queue says nothing is playing. No illustrations, no tips carousel.


### Android v1

Media notification only. Lockscreen comes free with MediaSession. Now-playing screen with queue and lyrics reachable by swipe or tab rather than a side panel — the split panel and fullscreen player are desktop shapes. Widget, quick settings tile, Auto and Wear come later; they're shallow but they multiply the surface area of every playback state bug not yet found. *(Since: Auto, Wear and media browsers through the library session; system playback resumption; a now-playing widget naming the Connect device that plays; launcher shortcuts; Assistant's play-from-search; the audio-effect session broadcast for equalizer apps.)*


## Licence  
*Status: decided*

**AGPL-3.0 across the whole project.** The forcing factor is AMLL, which is AGPL-3.0 rather than permissive. GPLv3 §13 permits combining with AGPL code, but the combination carries AGPL's terms, so the desktop app can't be GPLv3 while using it.

For most desktop software AGPL's network clause would sit dormant. Not here: the coordinator is a server other people's devices talk to over a network, which is exactly the situation §13 addresses. The obligation is satisfied by keeping the source publicly available, which was always the plan, so the practical cost is nil — but it's a decision rather than an accident.

It's also arguably the more fitting licence for something whose whole shape is self-hosted infrastructure. The cost is that some contributors and downstream users avoid AGPL on principle.

| Dependency | Licence | Effect |
|---|---|---|
| AMLL | AGPL-3.0 | Sets the licence for everything it combines with. Renderer only — no AMLL code reaches the core or coordinator. |
| Kawarp | GPLv3 (org-stated) | Compatible. Confirm the repo's own LICENSE. |
| playwire | MIT OR Apache-2.0 | Permissive, no constraints. |
| Symphonia | MPL-2.0 | GPL-compatible through MPL's secondary-licence clause. |
| cpal, UniFFI, napi-rs | Unconfirmed, expected permissive | Check before shipping. |

> The alternative, recorded for the avoidance of doubt Staying GPLv3 would mean dropping AMLL and writing the desktop lyric renderer directly — less absurd than it sounds given a native Compose renderer is being built for Android anyway, but it throws away the single biggest shortcut in the plan. A third reading holds that the Rust core and coordinator, containing no AMLL code, stay separable under GPLv3; that's a judgement about what counts as one work and shouldn't be relied on without advice.


## Not doing yet  
*Status: deferred*

| Item | Reason |
|---|---|
| Chromecast | Dropped. It would have been a backend rather than a Connect peer, but the detach-and-keep-playing property depends on stream URLs being reachable and CORS-clean from the receiver, and the usual workaround — proxying through the sender — destroys exactly that property. The backend seam leaves the slot open for it, or DLNA or AirPlay, later. |
| Crossfade | Worst value per unit of effort, and unlike gapless it invents audio that isn't in the file. |
| findSonicPath | Sonic paths from one track to another are a genuinely novel queue-building idea, but not now. |
| Playback speed | Pitch-preserving resampling is a real chunk of DSP work for a feature not wanted. |
| Tag editing | Out of scope — Subsonic can't write tags and Navidrome is scanner-driven. Playlists are the management surface. |
| Multi-server UI | Framework now, interface later. |
| Direct Last.fm / ListenBrainz | Navidrome already fronts these. |
| Automatic downloads | Listening-based auto-download is surprising and eats storage. Filter-driven offline sync covers the same want explicitly. |


## Still open  
*Status: open*

| Question | What turns on it |
|---|---|
| Name availability | Confirm before committing: `github.com/hocket`, `npm view hocket`, `hocket.app` and `hocket.dev`, and IP Australia classes 9 and 42. The only known collision is `ryolang/hocket`, a one-commit protocol spec under another org — not a conflict in any sense that binds. Fallback: Ripieno. |
| Distribution | Which channels — Play Store, F-Droid, GitHub releases, Flathub. With Cast dropped there's no proprietary dependency left, so the whole app can be FOSS and no build flavour split is needed. |
| Auto-update | Blocked on distribution. Different per channel, and some channels handle it for you. |
| Code signing | Blocked on distribution. macOS notarisation applies to the native addons as much as the app. |
| Remaining licences | Kawarp's own LICENSE file (the org states GPLv3, but an org README isn't authoritative per repo), cpal, UniFFI and napi-rs. All expected to be fine; none load-bearing enough to block. |
| Android references | `mldl-android` and Spicy Player each need a licence check before any code is copied. Reading them for approach is unrestricted; copying is not. |

Everything else that was open has been settled or turned out to already exist: sidecar lyrics with syllable timing, sonic similarity through the standard endpoints, fast full-library sync, and a maintained media-controls crate.

