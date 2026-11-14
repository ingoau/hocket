# Providers, then YouTube Music

*Status: Phase 1 implemented (15 Nov 2026); later phases planned. Revised after review.*

Hocket is built around one Navidrome server. This plan turns music sources into
**providers**: first several Navidrome servers, then YouTube Music, and later
Audiobookshelf and podcasts. The decisions come from a grilling session and a
follow-up review, and all of them are confirmed.

## What's true today

- `SubsonicApi` (`crates/hocket-core/src/subsonic/mod.rs:187`) is the only backend trait.
  - Its methods are Subsonic endpoints, so it works as a test seam, not a provider interface.
  - Seven files call `Core::api()` (`core/actor.rs:1285`).
  - Outside `subsonic/`, twelve files name `SubsonicApi`, plus `hocket-coordinator/src/worker.rs`.
  - `Downloads::resolve_with` takes `&dyn SubsonicApi` and a transcode profile (`downloads/mod.rs:1610`).
- `Core.server` is an `Option`, so only one server can be active.
  - A `servers` table already exists (`db/schema.sql:36`).
  - Mirror tables are keyed `(server_id, id)`, and outbox rows already carry `server_id` (`schema.sql:288`).
  - Migrations 0002–0010 sit on top of the schema.
  - `ServerId` is deterministic: `server_id_for(url, username)` (`core/handlers/servers.rs:21`).
- Track ids are bare strings throughout:
  - `QueueItem` (`api.rs:627`)
  - session context, history and stats
  - saved queues (`connect/replica.rs:8`)
  - AutoMix entries (`connect/wire.rs:229`)
  - stash, undo and album resume
- Sessions are scoped to the Navidrome `url|user` (`connect/wire.rs:199`).
  - LAN discovery filters on `md5(scope)` (`connect/discovery.rs:53`).
  - LAN rooms authenticate with a mutual HMAC keyed from the Subsonic password (`connect/auth.rs`). The joiner sends nothing until the leader has proved itself.
  - The hosted coordinator verifies clients through a Subsonic ping (`--verify-url`).
- Playback is resolved in `resolve_with`, in this order:
  1. a pinned download
  2. `hocket-stream://`, which checks the stream cache
  3. a stream-cache file
  4. the raw Subsonic URL
- `stream_reader` assumes Subsonic behaviour: error envelopes, estimated lengths, and a server that may ignore `Range` (`core/stream_reader.rs:23-44`).
- The setup link has the shape `(url, user, password, coordinator, settings)`, it only goes one way, and credential stores are keyed `(url, username)`.
- AutoMix fingerprints use `size_bytes` and `suffix` (`automix/store.rs:16`). The coordinator's analysis store is keyed `(server, track)` (`hocket-coordinator/src/lib.rs:60-62`).
- Cast (`SetRenderer`, `api.rs:2700`) always preloads the next track. When the receiver fails, Hocket falls back to transcoding.
- The desktop engine (Symphonia) has no Opus decoder (`audio/formats.rs:14`).

## Decisions

| Topic | Decision |
|---|---|
| Framing | YouTube Music is one provider among several: multiple Navidrome servers, YouTube Music, and later Audiobookshelf. The provider layer comes first. |
| ToS | Use the unofficial InnerTube API. It's always on, with no build flag and no Play Store build. |
| Client | A slim InnerTube client of our own in `hocket-core` (decided after Phase 0: rustypipe is unmaintained and its stream extraction is broken). Streams come from the `VISIONOS` player client, which needs no JS or PO token. |
| YTM accounts | Anonymous, signed in, or signed in with Premium. Premium is detected, not chosen. |
| Queues | Fully mixed. Every track reference names its source, starting in Phase 1. |
| Identity | Every device is equal. Each device has its own keys, and the member list is replicated to all of them. Any device can pair or remove another, so the first device can leave. |
| Coordinator | You claim it by running a `pair` command on the server, which prints a short-lived code and QR. The owner can invite other identities and revoke those invites. |
| Credentials | One global "Sync credentials" toggle, off by default. It never carries Google cookies. |
| YTM sign-in | Each device signs in to Google itself. The coordinator analyses tracks anonymously, and you can optionally sign it in with a session of its own. |
| Library | One unified library with a source filter. A song on two providers appears once. |
| Dedup | Strict matching: normalised artist, title and album, duration within ±2s, and ISRC when present. Matches can be linked or unlinked by hand, and those links sync through the identity. |
| Priority | You order accounts by dragging them. Navidrome comes above YouTube Music by default. A pinned or cached copy always wins. |
| Playlists | With one provider, playlists belong to that provider. With several, the create menu lists each provider plus "Mixed", which Hocket stores. Adding a track from another provider uses the matching copy if there is one, or offers to convert the playlist to Mixed. |
| YTM library | Liked songs, saved albums and artists, playlists and uploads are mirrored. The catalogue is fetched live and cached. |
| Anonymous YTM | Stars and adds go into a Hocket library on the identity. |
| Offline | YouTube Music tracks use the stream cache and pinned downloads. |
| Write-back | Star means Like. Playlist edits, history, and library adds and removes are written back. There's no dislike, and ratings are hidden. |
| Format | AAC. itag 140 (128k) through visionOS is the baseline and the fallback. For Premium accounts, a JS path tries itag 141 (256k): the app's WebView (Android) or Chromium (desktop) solves the signature and `n` puzzles and produces the PO token for the core. If that fails, playback drops back to 140. |
| Content | Songs only. Your own uploads still count as songs. |
| Lyrics | YouTube Music lyrics, then LRCLIB. |
| Autoplay | Stays within one provider. |
| Cast | The direct googlevideo URL, fetched just in time. No proxy. |
| Stream fallback | None in v1: no coordinator resolving or relaying, and no remote client config. Breakages are fixed with app updates. |
| Trait shape | Music items plus capability flags, with `kind` and a resume position added in Phase 1. |
| Setup | "Where's your music?" cards, then "Add another source?". Settings → Accounts reuses the same cards. |
| Home | Shelves contributed by each provider, which you can reorder and hide. |
| Books & podcasts | A separate section with Up Next for episodes and a Reading shelf for books. Two player lanes; the switcher only shows when both kinds of account exist. |
| Branch | One long-lived feature branch, merged once YouTube Music works end to end. |

## Phase 0: Spikes (3–4 days)

These check the facts the plan relies on. Each spike ends with a notes file and a
pass or fail against its threshold. A failure goes back for a decision before Phase 4.

- **rustypipe on Android.**
  - Cross-compile for aarch64 and x86_64, including QuickJS.
  - Pass: it adds less than 8 MB to the `.so`, and the first resolve takes under 1.5 s on the emulator.
- **What rustypipe can do.** Check each of these against a signed-in account:
  - read the library, including uploads
  - like and unlike
  - add to, remove from and reorder a playlist
  - add to and remove from the library
  - history pings
  - synced lyrics
  - home feed
  - radio

  Anything missing gets an estimate as in-tree InnerTube work.
- **Sign-in.**
  - Capture cookies from an Android WebView and from an Electron `BrowserWindow`.
  - If Google blocks the Electron window as an insecure browser, try signing in through the system browser instead (loopback, or pasting cookies).
- **Streams.**
  - Check whether itags 140 and 141 are offered for anonymous, signed-in and Premium accounts.
  - Check whether PO tokens are needed and whether they can be produced on Android, and whether any client is SABR-only.
  - Check that the `n` parameter transform works and that range-size throttling is handled.
  - Check that `loudnessDb` is present.
- **Desktop decoding.**
  - Decode itag 140 (fragmented MP4) through Symphonia, including seeking.
  - Check gapless playback when the file has no encoder-delay metadata.
  - Check that the priming offset matches Media3's, so AutoMix beat grids line up across platforms.
- **URL expiry.** Re-resolve partway through a download and check whether the content length and bytes stay the same.
- **Cast IP binding.** Hand a URL to the Default Media Receiver from the home LAN over IPv4 and over IPv6, and from a phone on mobile data. This needs a physical Chromecast and is a manual test.

## Phase 1: Provider layer, accounts and track references

**Goal:** Navidrome sits behind a source-neutral trait, and several Navidrome servers work at the same time, including in one queue.

- **The `Provider` trait.** A new `provider` module defines it, covering:
  - `capabilities()`
  - paged library sync, search and browse
  - `resolve_stream` returning a stream descriptor: URL, headers, expiry, container and codec, gain, and whether it's range-capable. Name it so it doesn't clash with the existing `MediaSource`, `StreamTarget` and `QueueSource`.
  - cover art
  - writes
  - `report_play`
  - lyrics
  - radio
  - `home_shelves()`

  `NavidromeProvider` wraps `SubsonicApi`, which stays as Navidrome's internal client and test seam.
- **Track references.**
  - `TrackRef {source, id}` replaces every bare track id: queue items, session context, history, saved queues, AutoMix entries, stats, stash, undo and album resume.
  - `source` names where the id is valid. For Navidrome that's the server id. For YouTube Music it's one global `ytm` source, because video ids are the same for every account. That way, switching from anonymous to signed in doesn't orphan pins, cache entries or queue items.
  - Bump `PROTOCOL` (`wire.rs:47`).
- **Accounts.**
  - `AccountId` stays deterministic on every device. For Navidrome it is `url|user` as today. For YouTube Music it is the channel id, or a fixed `ytm:anon`.
  - Rename the `servers` table to `accounts` and add kind, priority and capabilities.
  - Squash migrations 0002–0010 into `schema.sql`.
  - `AccountRegistry` replaces `Core.server`. New commands: `AddAccount`, `RemoveAccount`, `ReorderAccounts`.
- **Move the callers.**
  - Every `api()` and `SubsonicApi` caller goes through a provider, including `hocket-coordinator/src/worker.rs`.
  - `resolve_with` takes a provider, not `&dyn SubsonicApi`.
- **Interim scope.** Until Phase 2, the session scope is the first Navidrome account (highest priority). Rooms and the coordinator work as they do now.
- **Platforms.**
  - An account list in Settings, plus an add-account flow for each kind.
  - Credential stores are re-keyed by `AccountId`.
  - Setup gets the "Where's your music?" cards: Navidrome, and "I use Hocket on another device".
- **Spoken-content hooks.** Add `kind` and `resume_position` columns. Nothing uses them yet.

**Done when:**
- A core `sim` test with two fake servers passes. It covers a mixed queue, playback, download, scrobble routing, and an outbox flush while one server is offline.
- On Android and desktop, two real servers show as one library, and a queue mixing tracks from both plays and hands off between devices.
- CI is green.

## Phase 2: Identity and coordinator

**Goal:** sync is scoped to a Hocket identity, not to a Navidrome login.

### Identity and keys

- **Identity.** An identity is a random 128-bit id created on the first device. It replaces `url|user` everywhere: in the scope hash for discovery, the room key and the replica key.
- **Device keys.** Each device has an ed25519 key for signing and an X25519 key for key agreement.
- **Member list.** The member list is a hash-chained op log (`add`, `remove`). Every op is signed and records its parents. Every peer and the coordinator apply the same rules:
  - Removal wins.
  - An op made after its author was removed is invalid. "After" means it comes later in the log's causal order.
  - Concurrent conflicting ops are settled by a fixed tie-break (lowest op hash).
  - Old or replayed lists are rejected once a newer head is known.
  - **Accepted risk:** a stolen device can remove all the others. Recovery is to create a new identity.
- **LAN rooms.**
  - The mutual handshake stays, but uses device signatures over nonces.
  - The leader proves membership, including the head of its member list, before the joiner sends anything.
  - A peer with an older list pulls the missing ops before it admits or follows anyone.
- **Removing a device.**
  - The remaining devices rotate the group key and stop accepting the removed device.
  - Rotating doesn't revoke secrets the device has already seen. The removal screen therefore prompts you to change Navidrome passwords and sign that device out of Google.
  - The device list shows when each device last synced, so you can see whether it holds data no other device has.

### Pairing

Pairing is a two-way exchange, not a static link.

- **The invite** (QR or link) carries:
  - the identity id
  - the inviter's public key
  - a one-time secret
  - an expiry
  - a rendezvous: the coordinator URL or the inviter's LAN address
  - non-secret account configs (server URLs, usernames, kinds), so a new device with credential sync off only needs passwords
- **The exchange.** The new device contacts the rendezvous and proves it holds the secret. It sends its public keys and receives a signed `add` op plus a group-key envelope.
- **No coordinator and no shared LAN:** pairing can't complete. The app says so and offers to wait until both devices are on the same network.

### Credential sync

- One global toggle, off by default.
- When it's on, Navidrome and Audiobookshelf secrets travel as a blob encrypted with the group key.
- The group key reaches each member as an X25519 envelope. Envelopes for offline members wait on the coordinator or on LAN peers.
- **Google cookies are never synced.** Each device signs in to Google itself.
- The coordinator becomes a key holder while the toggle is on, and the UI says so. Turning the toggle off rotates the key and deletes the coordinator's copy.

### Coordinator

- **Claiming.** Run `hocket-coordinator pair` (or `docker exec … pair`) on the server. It prints a short-lived code and QR. The first device to present that code within the window becomes the owner identity.
- **Invites.** The owner creates invites in the app. An invite admits another identity, new or existing, and can be revoked. This replaces `--verify-url` and the Subsonic-ping check.
- **Analysis store.** Keyed by provider-aware fingerprint, not `(server, track)`.
- **AutoMix worker.** Uses its own provider accounts. For YouTube Music it analyses anonymously by default. The "Sign the coordinator in to YouTube Music" button opens Google sign-in on a device and sends that new session only to the coordinator. That covers uploads and tracks blocked in the coordinator's region.

**Done when:**
- Two devices pair by QR with no passwords in the link, both on LAN and through the coordinator.
- With three devices paired:
  - Removing the first leaves the other two working.
  - The removed device is rejected by a LAN peer and by the coordinator.
- Removal works while the removed device is offline.
- Two devices removing each other at the same moment settle the same way everywhere. Covered by a property test.
- Key rotation is tested with credential sync on and with it off.
- Claiming with a `pair` code works, an expired code fails, and an invite can be revoked.

## Phase 3: Unified library, dedup and Mixed playlists

- **Dedup.**
  - The `track_links` table stores a canonical group, its members, and whether each was matched automatically or by hand.
  - The matcher runs on each device after sync and errs towards leaving tracks unmerged.
  - Manual links and unlinks sync through the identity and override the automatic matches.
- **Playback.**
  - Lists show one row per group.
  - Play picks a pinned or cached copy first, then the highest-priority account that has the track.
  - A queue item from an account the receiving device doesn't have resolves through its group, instead of showing as unavailable.
- **Writes for grouped tracks.** Plays, scrobbles and history go to the provider whose copy actually played. Stars go to every linked copy whose provider supports them. In the UI, starring is one action.
- **Track details.** List every source, with "Not the same song" and "Link to…".
- **Hocket library.** Stars and adds for accounts that can't store them live on the identity.
- **Mixed playlists.** Stored in the identity replica. The create menu only shows provider choices when there's more than one account. Adding a track from another provider uses the matched copy if there is one, otherwise offers "Convert to Mixed".
- **Source filter.** A chip on the library, search and playlists.
- **Home.** Built from shelves supplied by `home_shelves()`, plus Hocket's own Continue listening. The shelf layout syncs.

**Done when:**
- Dedup fixtures pass: live, remastered and explicit/clean versions stay separate, and true duplicates merge.
- A manual link made on one device shows up on another.
- A queue item handed to a device without the original account plays the linked copy.
- A Mixed playlist syncs.
- The Hocket library, the filter and the shelves work on both platforms.

## Phase 4: YouTube Music provider

### Accounts

- **Anonymous, or signed in on each device.**
  - Android signs in through a WebView.
  - Desktop uses a `BrowserWindow`, or the fallback chosen in Phase 0.
  - Premium is detected.
- **Expired cookies** prompt you to sign in again on that device.
- **The setup card** offers "Sign in with Google" first and "Continue without an account" second. Signing in later re-keys only the library ownership, because track references use the global `ytm` source.

### Library and catalogue

- **Mirroring.**
  - Liked songs, saved albums and artists, playlists and uploads are mirrored.
  - Sync is incremental and polls gently.
- **Catalogue.** Search and the artist and album pages are fetched live, with a TTL cache.
- **Songs-only filter.**
  - Anything other than `MUSIC_VIDEO_TYPE_ATV` and your own uploads is hidden.
  - The mirror still stores the unfiltered playlist order. Index-based playlist mutations (`outbox/mod.rs:92-100`) map onto that order, so edits land in the right place.
- **Home shelves** come from the home feed, with the songs-only filter applied.

### Playback

- **Choosing a stream.**
  - `resolve_stream` uses itag 140, or 141 when it's offered.
  - URLs are cached until they expire and re-resolved on a 403.
- **Adapting `stream_reader` for googlevideo.**
  - It needs real lengths and honoured ranges.
  - Chunk sizes follow the throttling findings from Phase 0.
  - It re-resolves a URL partway through a stream without losing its place.
- **Stream cache and pins** are keyed `(videoId, itag, content length)`. A changed length after re-resolving restarts the download, not corrupts it.
- **Normalisation.** `loudnessDb` maps to track gain when there's no scanned gain.
- **AutoMix.**
  - The fingerprint for YouTube Music is `(videoId, itag)`, with a priming-offset correction so records for 140 and 141 line up.
  - The coordinator worker analyses anonymously, or with its own sign-in.
  - Desktop analyses on a miss as it does today.

### Writes, lyrics, radio

- **Outbox mutations** cover like and unlike, playlist edits on the unfiltered list, library add and remove, and history at the same point a scrobble would fire.
- **Hidden controls.** Capability flags hide ratings and dislike.
- **Lyrics.** YouTube Music lyrics come before LRCLIB.
- **Radio.** A YouTube Music radio `AutoplaySource`, used only for YouTube Music seeds.
- **Client.** Everything goes through our own InnerTube client: search, browse, library, writes, home, uploads, lyrics, radio and `player`.
- **Premium JS path.** The core asks the platform to solve the signature and `n` puzzles and to produce a PO token. It does this through a callback interface implemented with a hidden WebView on Android and a hidden `BrowserWindow` on desktop. This path is only used for Premium accounts, to reach itag 141, and failures fall back to visionOS 140.

**Done when:**
- Provider tests pass against recorded InnerTube fixtures: library sync, search, writes, lyrics and radio.
- On Android and desktop, a mixed queue plays gaplessly, pins and hands off between devices.
- Playback resumes after a URL expires partway through (forced in a test).
- A playlist edit lands at the right position in a playlist that contains hidden videos.
- **Manual checks:**
  - Premium quality is used when available.
  - A Like shows up on music.youtube.com.
  - History shows up on YouTube Music.

## Phase 5: Cast for YouTube Music

- **Handing over URLs.** For YouTube Music items, `SetRenderer` gives the receiver the direct googlevideo URL (`audio/mp4`).
  - The receiver only ever holds the current track and the next one.
  - The next track's URL is resolved at preload time, which is minutes before it plays, well inside the URL's lifetime.
  - After a long pause, the current URL is resolved again before playback resumes.
- **When the receiver fails.**
  - Re-resolve once, then skip with a toast.
  - The transcode fallback (`RendererFailed`) doesn't apply to YouTube Music.
- **Known failure cases.** The UI explains the cases found in Phase 0: mobile data, VPN, and IPv4/IPv6 mismatch.

For background: Metrolist and SimpMusic are the only FOSS YouTube Music clients that cast, and both use the direct URL.

**Done when** (manual, needs a physical Chromecast): a mixed queue casts from Android on the home LAN, and the next track still loads after the phone has been asleep for 10 minutes.

## Phase 6: Books & podcasts (later)

Spoken content becomes a second kind of media, on the same accounts, identity and sync.

- **Section.** Books & Podcasts appears only when an account offers spoken content. It holds Up Next, Reading, Inbox, Shows, Books and Finished.
- **Up Next.**
  - Episodes only, kept as a persistent backlog that syncs through the identity.
  - When an episode finishes, it's marked played, removed, and the next one starts. A setting can stop playback after each episode instead.
- **Inbox and auto-add.**
  - New episodes from subscribed shows land in the Inbox.
  - Each show has a setting, off by default, to add new episodes to the top or bottom of Up Next and to download them.
- **Reading shelf.**
  - Lists books in progress, such as "Ch 7 of 32 · 4h 12m left", and also appears on Home.
  - A book plays through its chapters and never moves on to Up Next.
  - When a book ends, playback stops and offers the next book in the series.
- **Two player lanes.**
  - Music, and Spoken (whichever book or Up Next item played last).
  - Each lane keeps its own position.
  - The session document carries both lanes, and handoff moves the active one.
  - The switcher only appears when there are both music and spoken accounts.
- **Spoken player.**
  - Has: resume position, chapters, speed, skip ±30s, a sleep timer that can run to the end of the chapter, and mark finished.
  - Doesn't have: shuffle, gapless, crossfade, AutoMix or normalisation.
- **Progress.**
  - Kept in the identity replica.
  - Pushed to providers that track progress (Audiobookshelf).
  - The furthest position wins, unless it was deliberately reset.
- **Providers.**
  - Audiobookshelf: books and podcasts.
  - YouTube Music podcasts.
  - Built-in RSS: subscriptions and progress live on the identity.
- **Downloads** use pins. Auto-download and auto-delete are settings.
- **Setup** gets an Audiobookshelf card, and the title becomes "What do you listen to?".
- **Later still:** named spoken lists and smart views.

## Later

- **Coordinator stream fallback.** Decide after Phase 0 and real-world experience with how often extraction on the device breaks.
  - In favour: you can redeploy the coordinator in minutes, faster than an app update, and your home coordinator has a residential IP.
  - Against: it runs the same client code, and away from home it has to relay the audio over an authenticated range endpoint, using your home upload bandwidth.
  - The coordinator gets YouTube Music support for AutoMix anyway, so adding this later is small.
- **Desktop Opus decoder,** if AAC at 128k sounds poor.
- **Proxied Cast,** if direct URLs fail too often.

## Risks

- **InnerTube changes** will break playback from time to time. Keep the client profiles (names, versions) in one place so they're easy to bump. Nothing else in v1 protects playback when YouTube changes.
- **Account flags.** Heavy library sync, or using a session from a datacenter IP, can flag a Google account. Poll slowly, never share one session between machines, and keep the coordinator's sign-in optional.
- **Signing in from desktop.** Google may block sign-in from Electron. The fallback comes out of Phase 0.
- **Size of Phases 1–3.** They're most of the work and change the wire protocol, the schema and every subsystem. They also deliver multi-server support on their own.
- **Long-lived branch.** Expect merge conflicts with `main`. Rebase often.
- **Wrong dedup merges** hide songs. Mitigated by a strict matcher, manual unlinking and fixtures.
- **Stolen device.** A stolen device can remove the others from the identity. This is accepted, and recovery is a new identity.

## Phase 1 notes

*Written while implementing Phase 1. Calls made where the plan was silent or didn't fit the code.*

- **Provider seam.** `crates/hocket-core/src/provider/` defines `Provider` (capabilities, a paged
  `library_page`, `search`, `album`/`artist`/`track` browse, `resolve_stream`, `download_url`,
  `cover_art_url`, `download_to_file`, `write`, `report_play`, `lyrics`, `radio`, `home_shelves`)
  and `ProviderSet` (the installed providers, routed by account or source). The stream descriptor is
  `ResolvedStream`. Methods a provider can't serve answer `ProviderError::Unsupported`; the error type
  is `SubsonicError` re-exported, since its variants (auth, network, not found, …) are already the
  generic set the outbox, jobs and problems classify.
- **What stays Subsonic.** `NavidromeProvider` wraps `SubsonicApi`, and `Provider::navidrome()` hands
  the client to work that is Subsonic through and through: the mirror sync's endpoint walk
  (`LibrarySync`), smart playlists over the native API, the outbox's playlist edits by index, the
  play-queue save and the capability probe. Every caller first picks the provider by the track's (or
  target's) account, so nothing reaches a server through the session's account by default any more.
  Scrobbles go through `report_play`, playback and downloads through `resolve_stream`/`download_url`,
  lyrics through `lyrics`, artwork through `cover_art_url`, the coordinator worker through
  `library_page`/`track`/`download_url`.
- **`resolve_stream` is async; `Downloads::resolve_with` isn't.** Navidrome's answer is ready at once,
  so `resolve_with` takes it with `now_or_never` and a provider that must go to the network plays
  through the stream reader (which awaits it). Phase 4 revisits this with the googlevideo reader.
- **Track references.** `TrackRef {source, id}` is in queue items, the session context's `tracks`,
  saved queues, ops on the wire (`PlayTracks`… carry refs, not `server_id` + ids), autoplay items,
  stash entries, snoozes, undo, scrobble dedupe records, prime/handoff messages, AutoMix entries and
  pending scrobbles. Album resume entries carry a `source` beside their album and track ids. The
  outbox, play history, pins and the stream cache were already keyed `(server_id, track_id)`, which
  is the reference. `QueueContext` keeps a `source` for its own id (album, artist, playlist).
- **Platform-facing ids stay bare in Phase 1.** Commands and queries the UI sends name tracks by id
  (`PlayTracks {server_id, track_ids}`, `Query::Track {id}`, `ActionTarget::Tracks {ids}`, stash and
  snooze commands…). The core turns them into references: a list command's `server_id` is a hint, and
  a track that account's mirror doesn't have is the one the account that has it means; a bare id
  resolves to the playing track, else the highest-priority account whose mirror has it. Two accounts
  with the *same* id (the same files on two servers) can therefore resolve to the higher-priority
  copy from a UI action. Phase 3's groups replace this; changing every platform call site for an edge
  case that dedup removes wasn't worth it now.
- **Unified library.** List queries (`Tracks`, `Albums`, `Artists`, `Genres`, `Playlists`, `Search`,
  `PlaylistMembership`) cover every account when they name the session's account or none, and one
  account when they name another. Platforms keep passing the first account's id and get the union;
  the source filter chip (Phase 3) can name one. Album and artist pages show the owner's tracks
  (highest priority when ids collide). Filters and smart playlists stay on the session's account.
- **Accounts.** `AccountInfo` replaces `ServerInfo` (adds `kind`, `priority`, `source`, `features`);
  `AddAccount {setup: AccountSetup::Navidrome {…}}`, `RemoveAccount`, `ReorderAccounts`,
  `ProbeAccount`, `SyncLibrary {account_id}`; `AccountsChanged` and `Query::Accounts`. Library syncs
  run one account at a time through one runner that holds every account's sync. The outbox flushes
  each account's rows to its provider; an offline account defers its own rows only.
- **Session scope.** The session belongs to the highest-priority Navidrome account. Reordering or
  removing it closes that session and opens the new first account's; the stash, snoozes and album
  resume points follow the session as before. The coordinator's analysis store only keeps and serves
  records whose source is the room's account.
- **Schema.** Migrations 0002–0010 are squashed into `schema.sql` as version 11 (with `accounts`,
  `tracks.kind` and `tracks.resume_position`). A database older than that is backed up and recreated
  rather than migrated. `PROTOCOL` is 4 and so is the floor.
- **Platforms.** Setup opens on "Where's your music?". On desktop the Navidrome card is the sign-in
  form itself (one click fewer, and the setup link pasted into Server URL still works); "I use Hocket
  on another device" points at it. On Android both are cards (the device card scans or opens the
  form for a pasted link). After the summary comes "Add another source?". Settings → Accounts lists
  the accounts with move up/down (drag reordering can come later), sync, remove and Add account,
  which opens the same cards over the app. Credential stores are keyed by `AccountId`, derived on
  each platform the same way as the core (`accountIdFor`, `AccountIds`, checked against a shared
  test vector). The platforms' fake cores stay single-account.
- **Not verified by hand.** The "two real servers on Android and desktop" check needs two
  Navidrome servers and devices; the sim test `actor_multi_account` covers the core side.
