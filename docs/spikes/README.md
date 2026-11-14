# Phase 0 spike results

*14 Nov 2026. These are the spikes from [providers.md](../providers.md#phase-0-spikes-34-days) that could run on the dev host. The ones that need a person, a Google account or a Chromecast are written up in [manual.md](manual.md).*

## Needs a decision

1. **"Client: `rustypipe` runs in the Rust core" no longer holds as written.**
   - rustypipe's last release was 0.11.4 in April 2025, and the last code change was in June 2025.
   - Its JS deobfuscator is broken on the current YouTube player (issue #75, reproduced here). Every client except iOS fails, and the iOS URLs return 403 after 1 MiB without a PO token.
   - A working anonymous path does exist: the `VISIONOS` InnerTube client. It needs no JS and no PO token, and serves the whole of itag 140. rustypipe doesn't have it.
   - Options:
     - (a) Keep rustypipe for catalogue and library reads, and write our own `player` call, writes, home and uploads. Pin it, and patch or fork it when a parser breaks.
     - (b) Fork it into the workspace and maintain it.
     - (c) Write a slim in-tree InnerTube client and drop rustypipe.

     (a) is the smallest change from the plan.
2. **Stream fallback ("decide after Phase 0").**
   - Extraction did break: rustypipe's whole stream path is dead today. The working client is a single one (visionOS), and yt-dlp's defaults change every few months.
   - A server-side resolver, or at least remotely configurable client versions and contexts, would let us fix breakage without an app release. Note that URLs are IP-bound, so a coordinator-resolved URL only works if the coordinator relays the bytes.
3. **Premium quality (itag 141) may be out of reach.**
   - 141 never appeared anonymously.
   - The clients yt-dlp uses for Premium all need player-JS solving and/or PO tokens, which nothing we have does today. The decision ("141 when available") still stands, but expect it to be unavailable until we ship a JS solver.
   - Confirm with the manual Premium test before deciding whether to build one.

## Results

| Spike | Result | Notes |
|---|---|---|
| What rustypipe can do ([rustypipe-capabilities.md](rustypipe-capabilities.md)) | **Partial / fail on maintenance** | Search, album, artist, playlist, radio, liked and saved reads, and plain lyrics work. No writes at all. No home, no uploads, no synced lyrics, no history pings. Uploads would be misclassified as videos. |
| Android build size ([android-build.md](android-build.md)) | **Pass, with a workaround** | +5.41 MB on aarch64 and +6.13 MB on x86_64 (stripped), under the 8 MB limit. `rquickjs-sys` has no Android bindings, so the spike used a `[patch]` with the linux-gnu bindings. |
| First-resolve latency | **Pass on the host**; emulator still to do | Cold visionOS resolve to the first bytes takes 0.70–0.93 s, against a 1.5 s threshold. |
| Streams: itags, PO tokens, `n`, throttling, loudness ([streams.md](streams.md)) | **Pass via visionOS, fail via rustypipe** | 140 is offered and 141 isn't. PO tokens are needed by every client except visionOS. iOS, Android and web are SABR-only. No `n` is needed on visionOS. `loudnessDb` is present on every format. Open-ended requests are throttled to about 2× realtime, while ranged chunks of 10 MiB or less are fast. |
| URL expiry ([streams.md](streams.md#url-expiry--re-resolve)) | **Pass** | Re-resolving partway through gives a new URL but the same `clen`, and the assembled bytes are identical. |
| Desktop decoding ([desktop-decoding.md](desktop-decoding.md)) | **Decode and seek pass; gapless fails** | Fragmented MP4 140 decodes and seeks in Symphonia 0.5.5. There's no iTunSMPB. The `elst` gives **1600 frames of priming** (not 1024/2112) and there are 968 padding frames, which Symphonia ignores. Today the repo trims nothing. |
| Sign-in, Premium, Cast, on-device latency | **Manual** | See [manual.md](manual.md). |

## What this changes in the plan

- **Phase 4, playback.**
  - `resolve_stream` uses our own `player` request with the visionOS client and a cached `visitorData`, not rustypipe's `player()`.
  - The client contexts (name, version, UA) go in data, so they can be bumped quickly.
- **Phase 4, `stream_reader`.**
  - Bounded `Range` requests only: 1–2 MiB to start playback, then up to about 8 MiB. Never open-ended.
  - Send the client's User-Agent.
  - Take the length from `clen`.
  - On a 403, re-resolve and resume at the same offset.
- **Format.** itag 140 is the effective baseline for everyone. 141 depends on manual test 3.
- **Normalisation.** Read `loudnessDb` from the adaptive format, not from `audioConfig`, which is missing on visionOS.
- **Missing endpoints, estimated in-tree InnerTube work for Phase 4 (about 6–7 days plus fixtures):**
  - like and unlike (`like/like`, `like/removelike`)
  - playlist edits (`browse/edit_playlist` with `setVideoId`)
  - create and delete playlists
  - library add and remove (`like/like` on a playlist, `subscription/subscribe`, `feedback` tokens)
  - history (`videostatsPlaybackUrl` and `videostatsWatchtimeUrl`, which the player response provides)
  - home (`FEmusic_home`)
  - uploads (`FEmusic_library_privately_owned_*`)
  - synced lyrics (`ANDROID_MUSIC` `timedLyricsData`)
- **Songs-only filter.** rustypipe merges OMV and UGC into `Video` and can't recognise `PRIVATELY_OWNED_TRACK`. Use our own video-type enum, or uploads get hidden.
- **Desktop gapless.** Add an `elst` parser to `audio/padding.rs` and apply it in `decoder.rs` before iTunSMPB. It's about 0.5 day and pure, so it can be unit-tested.
- **AutoMix.** Symphonia's timeline today is 1600 frames (36.28 ms) behind Media3's for itag 140, because Media3 applies `elst`. After the desktop fix the correction is 0. Store the correction per `(itag, elst media_time)` and measure 141 when it's available.
- **Android build.**
  - Either commit Android bindings for `rquickjs-sys` (vendor it, or generate once with bindgen), or drop QuickJS: the visionOS path doesn't need it.
  - Without QuickJS the size cost drops below the measured +5.4 MB.
- **Risks** (add to `providers.md` if accepted):
  - single-client dependence (visionOS)
  - IP-bound URLs for Cast (the host is IPv4-only, so IPv6 is untested)
  - rustypipe being unmaintained
