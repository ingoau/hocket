# Spike: streams, PO tokens, throttling and URL expiry (anonymous)

*14 Nov 2026, run from the dev host (residential-style IPv4, no IPv6).*

Test tracks:
- `4D7u5KF7SP8`: Daft Punk, "Get Lucky", 6:10, ATV
- `OF4rw3LOfqY`: Pink Floyd, "Echoes", 23:33
- `Rgrt_8mXrK8`: "Get Lucky", radio edit

Test code (in the scratchpad, not the repo):
- a Rust binary using rustypipe 0.11.4 (rustls)
- a stdlib Python probe that calls `youtubei/v1/player` directly with the client contexts from current yt-dlp (`yt_dlp/extractor/youtube/_base.py`, master as of today)

## Which clients give working URLs anonymously

| Client | Result | Audio itags | Usable? |
|---|---|---|---|
| rustypipe `Ios` (20.03.02) | OK, plain URLs | 139, 140, 249, 250, 251 | **No.** `Range: bytes=0-1048576` works. Anything past ~1 MiB, or a range bigger than that, gets **403**. That's the GVS PO-token wall. |
| rustypipe `DesktopMusic`/`Desktop`/`Tv`/`Android`/`Mobile` | `deobfuscation error` | | No |
| raw `IOS` 21.26.4 (yt-dlp version) | OK, but **no URLs**: SABR-only (`serverAbrStreamingUrl`) | | No |
| raw `ANDROID` 21.26.364 | OK, no URLs: SABR-only | (adds 599/600) | No |
| raw `ANDROID_VR` 1.65.10 | OK with visitorData, plain URLs | 139, 140, 249, 251 | **No**: 403 after the first 1 MiB (needs a PO token) |
| raw `TVHTML5` (tv_samsung), `WEB_REMIX` | "The page needs to be reloaded" | | Need the player JS `signatureTimestamp` and deobfuscation |
| raw **`VISIONOS` 1.02** (yt-dlp's current no-JS default) | **OK with visitorData, plain URLs** | 139, 140, 249, 250, 251 | **Yes.** No `n`, no `sig` to solve, no PO token. The whole file downloads. |

Without `visitorData` (from `https://www.youtube.com/sw.js_data`, or rustypipe's `get_visitor_data`), visionOS, android_vr and TV all return "Sign in to confirm you're not a bot".

So:
- **PO tokens:** needed by every client except visionOS. That includes rustypipe's working iOS path.
- **SABR-only:** current iOS, Android and web.
- **The `n` transform:** not needed on the visionOS path. rustypipe's version is broken anyway.

## Formats and loudness

- **itag 141 was never offered** anonymously, by any client, for any of the 3 tracks. Only 139/140 (AAC) and 249/250/251 (Opus) were.
- **`loudnessDb`** is present on every adaptive format (3.14 dB for Get Lucky). `playerConfig.audioConfig.loudnessDb` is missing on visionOS and iOS but present on android/android_vr. Read the per-format field.
- `perceptualLoudnessDb` and `trackAbsoluteLoudnessLkfs` (−10.86) are also there, with `loudnessTargetLkfs` −14.
- `contentLength` (`clen`) is present, and `expiresInSeconds` is 21540 (6 h).
- The 140 URL has `ip=` (IPv4 here), `expire`, `sig`/`lsig` already applied, and `c=VISIONOS`. URLs are **IP-bound**, which matters for Cast.
- `playbackTracking` has `videostatsPlaybackUrl` and `videostatsWatchtimeUrl`, which history reporting can use.

## Throttling (itag 140, visionOS URLs, with the client's User-Agent)

| Request pattern | Get Lucky (5.98 MB) | Echoes (22.9 MB) |
|---|---|---|
| Open-ended `Range: bytes=0-` | 22.5 Mbit/s on one run, 5.2 on another | **0.26 Mbit/s: 705 s for 22.9 MB, about 2× realtime.** Throttled. |
| 10 MiB ranged chunks | 75 Mbit/s (1 request) | 15.4 Mbit/s overall. First chunk 10.1 s cold, then 1.0 s and 0.3 s. |
| 1 MiB ranged chunks | 4.8 Mbit/s (≈1.6 s/request cold) | 6.9 Mbit/s, ≈0.17 s/request warm |

- **Never use open-ended or large single requests.** Use bounded ranges ≤10 MiB. rustypipe's downloader uses random 9–10 MB chunks, and the `&range=a-b` URL parameter acts like the header.
- A cold googlevideo edge serves about 5 Mbit/s (≈40× the 129 kbit/s bitrate), so 1–2 MiB first chunks keep startup snappy. Later chunks can grow to about 8 MiB.

**Pass** (with bounded ranges).

## URL expiry / re-resolve

Each run re-resolved halfway through and carried on with the new URL. Results for both tracks and all chunk sizes:
- the URL changed every time
- `contentLength` stayed the same (5983881 and 22861275)
- the SHA-256 of the assembled files was identical across all 3 methods

**Pass**: switching URLs mid-download is safe. Still key the cache by `(videoId, itag, clen)` as planned.

## Latency (host, not emulator)

| Step | Time |
|---|---|
| visionOS cold path (visitorData fetch, then `player`, then the first 64 KiB) | **0.70–0.93 s** to the first bytes (visitorData 0.26–0.29 s, player 0.33–0.34 s) |
| rustypipe `player()` | 0.22–1.2 s (iOS path) |
| rustypipe `RustyPipe::build()` | <1 ms |

Below the 1.5 s threshold on the host. The emulator measurement is still to do (see [manual.md](manual.md)).

## Implications

- Phase 4 `resolve_stream` should use our own visionOS `player` call (visitorData cached and refreshed), not rustypipe's `player()`.
- No QuickJS or botguard is needed for anonymous 140 today.
- Keep a client list and a fallback order. yt-dlp's defaults move every few months: this month `visionos` is the only JS-less default. A remote-configurable client context (version strings) would let us react without an app release.
- itag 140 is the real baseline. 141 needs a signed-in Premium test (see manual.md). yt-dlp's Premium clients (`web_creator`, `tv_samsung`, `web`) all need player-JS solving and/or PO tokens, so **141 may need a JS solver even for Premium users**.
- `stream_reader`:
  - bounded ranges (1–2 MiB to start, then ≤8 MiB)
  - real `Content-Length` from `clen`
  - the client's User-Agent
  - a 403 triggers a re-resolve and resumes at the same offset
- Cast: the URL is bound to the resolving device's public IP and family. This host is IPv4-only, so the IPv6 case is untested.
