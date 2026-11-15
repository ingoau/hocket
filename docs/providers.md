# Providers, then YouTube Music

*Status: Phases 1–4 implemented (16 Nov 2026); later phases planned. Revised after review.*

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
- **Client fallback chain.** `player` requests try client profiles in order: `VISIONOS`, then `ANDROID_VR`, then `IOS`, then the web music client with signature, `n` and PO token from the platform JS host below. The first one that returns a usable AAC stream wins. The chain is per track and remembered for a while, so a broken client is skipped quickly. All profiles (names, versions, user agents) live in one table so they're easy to bump.
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

## Phase 2 notes

*Written while implementing Phase 2. Calls made where the plan was silent or didn't fit the code.*

- **Where things live.** `crates/hocket-core/src/identity/`: `crypto` (ed25519/X25519 keys,
  ChaCha20-Poly1305 sealed boxes, HKDF envelopes, a canonical byte encoding), `log` (the member
  list), `mod.rs` (`Identity`: keys, group keys, grants, the credential blob, signed records) and
  `pairing`. `connect/auth.rs` is now the signature handshake. The coordinator's admission lives
  in `crates/hocket-coordinator/src/identities.rs`.
- **Keys are in the core's database.** The identity (device secret included) is one
  `saved_state` row (`identity`). Neither platform's credential store is a good fit (Android's is
  keyed per login, the desktop's per server), so there is no keystore integration yet.
- **Member-list rules, precisely.** Validity is a pure function of the op set (so convergence is by
  construction; the proptests check order-independence and mutual removal). An op is valid when
  its author is a member in its causal past. "Removal wins" became: a valid `Remove(D)` voids every
  op by D outside the removal's causal past (and whatever those enabled, such as a device D added
  meanwhile), removals take effect in ascending op-hash order (so of two devices removing each
  other the lowest hash wins, however deep either side's history), and a removed id or key is never
  added again. "Old or replayed lists are rejected" falls out of the union: an old list can't undo
  anything, and every check is against the newest list known.
- **The group key** is committed in the log (genesis, `Remove`, `Rotate` carry its hash), so a key
  handed over by anyone can be checked; the current key is the last committing op in the log's
  deterministic order. Grants (X25519 envelopes) are part of the synced state, so "pending
  envelopes for offline members" are simply the grants every member and the coordinator keep and
  pass on. Any key holder makes the grants a member lacks.
- **Records a removed device signed** (the coordinator pin, the credential-sync flag) would vanish
  with it; the remover re-signs them as part of the removal.
- **Credential sync** keeps the plaintext only in memory on each device; the blob (sealed with the
  group key, `SecretKind` has no YouTube Music kind) is re-sealed whenever the key rotates. A device
  receiving it adds the accounts itself and emits `CredentialsSynced` for the platform's credential
  store. The coordinator gets a grant only while the toggle is on and a coordinator is pinned;
  turning it off rotates the key and drops the blob, and pruning drops the coordinator's grant.
- **The coordinator proves itself** with its own key (`coordinator-key.json`), pinned in the
  identity when a device claims it or redeems an invite (`CoordAdmitted`); the pin syncs, so other
  devices trust the same coordinator, and the pinned URL becomes the coordinator URL setting.
- **`pair` writes `<data-dir>/claim.json`** for the running server to read (so `docker exec` works);
  it needs `--data-dir`. Claiming again hands ownership to the new identity (recovery); the old one
  stays admitted. Invites last a week, work once and can be revoked. A claim or redemption for an
  identity the coordinator knows only merges what was sent, so a forged list can't replace a real
  one.
- **Flags removed:** `--verify-url`, `--no-verify`, `--allow-private-servers`, `--diagnostics-user`.
  The owner identity's devices get the full log and report grants instead.
- **Analysis store.** Keyed by `wire::AnalysisKey { provider, fingerprint }`: `navidrome:<server>`
  and `<track id>|<size>|<suffix>`. The file is new (`analysis.sqlite`); old records aren't read.
  Every admitted identity on a server shares them. The worker keeps its own Navidrome account
  (`--analysis-server`/`--analysis-user`), as before; signing it in to YouTube Music is Phase 4.
- **Pairing rendezvous.** LAN addresses are the source address of the default route (IPv4 and
  IPv6) plus the listener's port; the coordinator relays a join to the member that sent
  `PairOffer` for the invite. The new device tries the LAN addresses (4 s each), then the
  coordinator, and says clearly when neither answers. The invite's `settings` field exists but is
  left empty: synced settings arrive with the session once paired.
- **Session scope.** The session belongs to the identity, so it is open from first start, with or
  without accounts; reordering or removing accounts no longer moves it. Pairing moves the device to
  the other identity's session (the old one is closed, not merged).
- **Engine details worth knowing.** A peer turned away while it was of another identity is
  unblocked once the member list newly lists it; a reopened session gets the LAN adverts already
  seen; a follower leaves a LAN leader the list removed, and a removed device closes its room.
- **Not verified by hand:** pairing two real devices over a real LAN and through a deployed
  coordinator, and the QR scan on a phone. The sim tests, the actor test over the in-memory network
  and the coordinator's socket tests cover the protocol.

## Phase 3 notes

*Written while implementing Phase 3. Calls made where the plan was silent or didn't fit the code.*

- **Where things live.** `crates/hocket-core/src/library/`: `links.rs` is the matcher (pure, with
  the dedup fixtures in `links_tests.rs`), `mod.rs` the identity's library records. The actor side
  is `core/handlers/identity_library.rs` (records, relinking, choosing a copy), `mixed_playlists.rs`
  and `home.rs`; the tables are in `db/links.rs`.
- **One `links` table, not `track_links`.** Rows are `(kind, source, id, group_id, rep, manual)` for
  songs, albums and artists, and only for members of groups of two or more, so a one-account
  library has none. It is rebuilt from the mirror and the manual links after every sync, record
  merge, reorder and account removal (100k songs take well under a second). `group_id` is the
  smallest member reference, so it doesn't move when priorities do; `rep` (the highest-priority
  account's copy) is the row the unified library lists.
- **The matcher.** Only across sources, one copy per source per group (closest durations first).
  Titles and albums keep their qualifiers: "(Live)", "- 2011 Remaster", "(Clean)", "(Radio Edit)"
  and a different album (live album, compilation, deluxe edition) stay apart; case, accents,
  quotes, bracket style, "&"/"and", "The" and featuring credits are folded away, and " - Live"
  reads as "(Live)". The explicit flag must match; ISRC (`tracks.isrc`, from OpenSubsonic `isrc`)
  decides when both copies have one. Albums group by normalised album artist and name (and year
  when both have one), artists by name; there are no manual album or artist links.
- **The identity's library is one record set.** Manual links (`link`, a pair and same/not the
  same), Mixed playlists (`playlist`, the whole playlist LWW) and the Hocket library (`saved`) are
  `LibraryRecord`s in one last-write-wins set, synced like the stash (`Msg::LibraryRecordsSync`,
  `ReplicaState::library_records`). "Not the same song" writes a not-the-same link to every other
  member, so the matcher can't put it back; "Link to…" a same link, which joins two groups whatever
  the matcher says.
- **Only manual links sync; queue items carry the copies.** Automatic groups are re-derived on each
  device from its own mirror, so a device without an account can't see them. Instead a device that
  knows a queue item's group records the other copies on the item (`QueueItem::alts`, written by a
  local `QueueOp::SetAlternates` document replace after queue changes), and a device without the
  item's account plays, and lists, its own copy. Playlists and the context list carry no such
  copies: a Mixed playlist's song from an account this device lacks isn't listed here.
- **Choosing a copy.** `Actor::best_copy`: a pinned or cached copy first, then the copy on the
  highest-priority account this device can reach, else the item's own reference. The playing
  track is that copy, so now playing, scrobbles and history go to the account that played it.
  Scrobble dedupe across a handoff is still keyed by reference: a play handed between devices that
  pick different copies can be scrobbled to both accounts (accepted, rare).
- **Writes.** Starring a song stars every copy on an account here that keeps stars, as separate
  outbox writes (each touches only its account's mirror row now); a copy on an account without
  stars goes to the Hocket library. Ratings go to the named copy only. The undo of a rating, love
  or playlist add still names songs by bare id; the core resolves those against the playlist's or
  the mirror's account.
- **Bare ids are gone from the song commands.** `PlayTracks`, `PlayNext`, `AddToQueue`,
  `PlayLater`, `InsertTracks`, `StashAdd`, `Snooze*`, `CreatePlaylist`, `PlaylistAdd`,
  `PreviewStart`, `PrimeTrack`, `ActionTarget::Tracks`, `RatingTarget::Track`, `Query::Track`,
  `TracksByIds` and `PlaylistMembership` take `TrackRef`s, and the core's "resolve a bare id"
  step for them is gone (`fix_refs` remains only for a context's own track list). Still bare,
  because they act on the playing song or a pin and resolve to it first: lyrics, related, analysis,
  media source, export, song link, `PinTarget::Track`, and album, artist and playlist ids (the
  matcher groups those). Library pins of songs now store `source` beside the id.
- **List queries.** An empty account id is the unified library (one row per group); a named
  account is that account's library, which is what the source chip sends. Platforms send "" for
  everything that isn't the chip (Home, details, Android Auto, Pebble).
- **Hocket library.** `SavedTrack {track, starred, added, snapshot}` records; a starred song is
  marked loved on the mirror after every sync, and a song only known from its snapshot is written
  into the mirror when this device has its account. `SetInLibrary` and the `library_edit`
  capability are the "add" half; Navidrome has nothing to add to, so there is no add UI until
  YouTube Music. The capability test uses a fake server with `feature_overrides` (no stars).
- **Mixed playlists** are `playlists` rows with `server_id = "hocket"` and per-song
  `playlist_tracks.track_source`, rebuilt from the records. Their edits change the record (no undo
  entry yet). Adding another account's song to a provider playlist uses that account's copy, else
  toasts "Convert to Mixed" (dismissing it cancels); converting creates the Mixed playlist with the
  songs and deletes the provider's. Creating a provider playlist from songs without copies there
  offers "Create as Mixed" instead. The create menu (desktop dialog, Android sheet) offers each
  account and Mixed only with more than one account.
- **Home.** `Query::HomeShelves` answers Hocket's Continue listening (albums left part way, then
  the latest plays' albums) and, for each account the core mirrors, Recently added, Most played and
  Recently played, built from the mirror so Home never waits on the network. Navidrome's
  `home_shelves` capability stays false: the live `Provider::home_shelves` is for providers
  without a mirror (YouTube Music, Phase 4). Shelves are Home sections named `shelf:<shelf id>`
  in the synced `home.sections` and order, hide and restyle like the rest; the built-in "New in
  your library" and "Recently played" sections became these shelves. Continue listening and each
  account's Recently added are on by default.
- **Schema** is version 12 (`links`, `tracks.isrc`, `playlist_tracks.track_source`); an older
  database is recreated as before.
- **Tests.** Dedup fixtures (`library/links_tests.rs`); record sync through the coordinator and a
  LAN room, a Mixed playlist's edits and deletion, catch-up from the replica (`sim/library_records_tests.rs`);
  `tests/actor_dedup.rs` (one row per group, play from the first account and scrobble there, a
  queue item handed to a device without the account plays its copy, stars to every copy, link and
  unlink, link candidates, the Hocket library, Mixed playlists and Convert to Mixed, Home shelves);
  desktop `e2e/providers.spec.ts` (shelves, chip, New playlist, Sources) and unit tests on both
  platforms for the shelves layout, the chip and the New playlist default.
- **Not verified by hand:** two real servers with overlapping libraries on a phone and the
  desktop; the Android chip, Sources and New playlist sheet are covered by logic tests only (the
  fake cores keep one library).

## Phase 4 notes

*Written while implementing Phase 4. Calls made where the plan was silent or didn't fit the code.*

- **Where things live.** `crates/hocket-core/src/ytm/`: `clients.rs` (every client profile in one
  table: VISIONOS, ANDROID_VR, IOS, WEB_REMIX), `client.rs` (one call per InnerTube endpoint, with
  `visitorData` from `sw.js_data` and, signed in, the cookies and `SAPISIDHASH`), `parse.rs` (pure
  parsers that walk for the item renderers rather than fixed paths), `player.rs` (the client chain
  and the `JsHost` callback), `provider.rs` (`YtmProvider`), `fixtures/` and `fake.rs` (recorded
  answers and a fake transport). The actor side is `core/handlers/youtube_music.rs` (accounts, the
  JS host bridge) and `catalogue.rs`; the mirror walk is `LibrarySync::for_provider`.
- **Accounts.** `AccountSetup::YoutubeMusic { cookies, js_host }`. Anonymous installs at once as
  `ytm:anon`; cookies are checked through `account/account_menu`, whose channel (`UC…`) becomes the
  account id, then the anonymous account is removed. Every YouTube Music account has source `ytm`,
  so mirror rows, pins, the stream cache, the outbox and queue items all name `ytm`; a list query
  naming the account id reads the `ytm` mirror (`library_scope`), and the last account of a shared
  source takes the source's rows with it. A refused session toasts "sign in again" and emits
  `YtmSignInExpired`. Premium is a heuristic (no "Get Music Premium" upsell in the account menu,
  shown as `capabilities.serverVersion = "premium"`); it needs checking against real accounts.
- **Deviation: cookies are never credential-synced.** The brief said YouTube Music credentials ride
  credential sync when it's on; the Decisions table says it never carries Google cookies and each
  device signs in itself, and the Risks section warns against using one session on several
  machines. The table won: `SecretKind` still has no YouTube Music kind. Each platform keeps its
  sign-in OS-encrypted under one fixed key (`ytm`) and replays it on start.
- **Library.** Liked songs (`VLLM`, starred), library songs (`FEmusic_liked_videos`) and uploads are
  the tracks; saved albums and artists and the account's playlists (without Liked Music) are
  mirrored. Each sync is a full walk (no incremental endpoint), paced by the existing sync interval.
  The mirror lists songs only; edits by position are mapped onto the live, unfiltered order at
  flush time (`setVideoId`s; inserts are an add then a move; a move to the end stays clear of hidden
  entries), which is the "unfiltered index mapping" without storing it.
- **Songs-only filter, found live:** YouTube Music labels many *album* tracks
  `MUSIC_VIDEO_TYPE_OMV` (every track of the recorded album). Album pages therefore count all
  their tracks as songs; elsewhere ATV, uploads and untyped items are songs and OMV/UGC/episodes
  are hidden.
- **Catalogue.** Search results, album and artist pages, home shelves and radio picks are kept in a
  `catalogue` table (schema 13). `track_ref`/`tracks_by_refs` fall back to it, and album, artist,
  artist-albums and top-songs queries answer from it or fetch live (album and artist ids are
  recognised by shape: `MPRE…`, `UC…`). List queries never read it, so it isn't the library.
  Provider answers are also TTL-cached in memory (30 min).
- **Playback.** Streams go through the stream reader on both platforms: ranges are bounded (1 MiB
  first, then 8 MiB; `ResolvedStream::max_range`), headers carry the resolving client's
  User-Agent, and a 403/410 calls `Provider::stream_failed` (a 403 marks that client broken for
  30 min) and re-resolves the same range. No transcoding profile applies. The cache stays keyed by
  (source, track, profile): a different variant changes the content length or type, which already
  discards cached spans rather than mixing them; `ResolvedStream::variant` (the itag) is carried but
  not part of the key. `loudnessDb` becomes the track gain (negated) when nothing was scanned;
  it's known once the reader resolved the stream, so the very first play of a track can miss it.
  Pins of YouTube Music tracks keep the stream (`download_to_file` in bounded ranges, re-resolving
  on a refusal).
- **Client chain.** Order VISIONOS → ANDROID_VR → IOS → web (only with a JS host); the client that
  answered last goes first. Premium with a JS host puts the web client first for itag 141 and falls
  back to the chain for 140. Plain clients never send the session.
- **JS host: core side only.** `JsHost`, `Event::YtmJsRequest` and `Command::YtmJsAnswer` are in
  place and tested with a fake host, but neither platform implements the solver yet (both pass
  `js_host: false`), so 141 is unreachable and Premium plays 140. Building the hidden
  WebView/BrowserWindow solver needs a Premium account to develop against.
- **Edit list.** `audio::padding::parse_mp4_edit_list` reads the single edit; the desktop decoder
  skips `media_time` (1600 frames) and plays `segment_duration` frames when there's no iTunSMPB.
  Media3 applies the edit list itself on Android; nothing is corrected there. AutoMix records are
  keyed `AnalysisKey::ytm(videoId, 140)`; with both platforms trimming, the priming correction is 0.
  The coordinator worker still analyses Navidrome only (its YouTube Music sign-in isn't built).
- **Writes.** Like/unlike for songs, albums save via their `OLAK5uy_…` playlist, artists subscribe,
  playlists create/rename/delete/add/remove/move; ratings and the play queue are no-ops. History is
  a ping of the web player's `videostatsPlaybackUrl` on a finished scrobble. Signed in, "Add to
  library" on a song is a like (the web app's own add needs per-item menu tokens a reference lacks).
- **Hocket library UI.** New core actions "Add to library" / "Remove from library", shown only for
  songs of an account without a library of its own (anonymous YouTube Music).
- **Lyrics** come from the YouTube Music lyrics tab (plain text) before LRCLIB; **radio**
  (`RDAMVM<id>`) feeds autoplay's similar-songs step and Related for YouTube Music seeds.
- **Home.** Live shelves are fetched in the background, cached for 30 min and announced with
  `LibraryChanged {tables: ["home"]}`; they're `<accountId>/<slug>` shelves, on by default.
- **Platforms.** Desktop: a sign-in `BrowserWindow` (own partition, cleared each time, plain
  Chrome UA) whose cookies main dispatches itself; the setup card and summary; `HOCKET_FAKE_YTM_SIGNIN`
  for e2e. Android: `YtmSignInActivity` (WebView without `; wv`, cookies cleared first), the card,
  the summary and the keystore replay (`YtmLogin`).
- **Tests.** Recorded fixtures (search, album, artist, playlist with hidden videos, home, next,
  lyrics, three player clients) in `ytm/tests.rs`; library and write answers are hand-made in the
  web client's shapes (nothing signed in was recorded). Also: chain ordering and skipping, 141 via
  a fake JS host, ranged downloads with a mid-stream refusal, the reader's re-resolve and bounded
  ranges (`stream_reader::googlevideo_tests`), the catalogue table, the edit-list parser,
  `tests/actor_ytm.rs` (anonymous catalogue, Hocket library, home; signed-in sync and outbox
  writes; expired sign-in), desktop unit tests and `e2e/youtube-music.spec.ts`, Android
  `YtmLoginTest` and a replay case in `CoreHostReplayTest`.
- **Live smoke test** (`cargo test -p hocket-core --features decode --test ytm_live -- --ignored`),
  16 Nov 2026 from the dev host: search 1.5 s (3 requests); resolve via VISIONOS, itag 140,
  5,983,881 bytes, gain −3.14 dB, 0.28 s; first 1 MiB range 0.8 s; whole stream in bounded ranges
  8.2 s; Symphonia decoded 369.6 s with the 1600-frame priming trimmed (the edit list's length).
- **Not done / needs a person:**
  - Google sign-in on a real Android device and in the desktop window (does Google accept them?),
    and how long the cookies last; cookie rotation isn't handled.
  - Premium detection, and itag 141 (needs the JS host on both platforms).
  - Media3's priming handling on a device (`docs/spikes/manual.md`, 6).
  - That a Like and history pings show up on music.youtube.com; playlist edits on a real playlist.
  - The coordinator analysing YouTube Music, and its optional sign-in.
  - Cast is Phase 5. Not verified: a mixed queue playing gaplessly and handing off between real
    devices, and the Android UI flows beyond compile and unit tests.
