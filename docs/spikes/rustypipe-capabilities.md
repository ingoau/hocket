# Spike: what rustypipe can do

*14 Nov 2026. Source audit plus a live anonymous run. No signed-in account was available, so nothing here was checked against a real library (see [manual.md](manual.md)).*

## Version and maintenance

- Latest on crates.io: **0.11.4, released 23 Apr 2025**. Repo: `codeberg.org/ThetaDev/rustypipe`.
- `main` has 5 small commits since then, the last code change on 18 Jun 2025. The only 2026 commit is a licence-identifier docs fix (3 Aug 2026).
- Open issues show it's out of date:
  - #75 (Mar 2026), "Please update the library": `deobfuscation error: could not extract sig fn name`.
  - #73 (Jan 2026): `browse` continuations return "invalid argument".
- **Confirmed live today.** `player_from_client` fails for `DesktopMusic`, `Desktop`, `Tv`, `Android` and `Mobile` with the #75 deobfuscation error. Only `Ios` returns streams, and those URLs stop working after 1 MiB (see [streams.md](streams.md)). `main` doesn't touch `deobfuscate.rs`, so it's broken there too.

**Result: fail on maintenance.** In practice rustypipe is unmaintained, and its playback path doesn't work today.

## Capability table

Paths are relative to the rustypipe repo at `c93bafd`.

| Need | Status | Where / notes | Raw InnerTube if missing |
|---|---|---|---|
| Search (songs, albums, artists, playlists, suggestions) | **Supported**, works live (1.6–1.8 s cold) | `src/client/music_search.rs:39-135` | |
| Album, artist, playlist pages | **Supported** | `music_playlist.rs:28,49`, `music_artist.rs:31,83` | |
| Track details, related | **Supported** | `music_details.rs:42,88` | |
| Radio / watch playlist | **Supported**, works live (50 items) | `music_details.rs:111-146` (`music_radio`, `music_radio_track`, `music_radio_playlist`) | |
| Charts, new releases, moods/genres | Supported | `music_charts.rs`, `music_new.rs`, `music_genres.rs` | |
| Home feed | **Missing** | No `FEmusic_home` | `browse` `FEmusic_home` plus continuations. ~1 day with shelf mapping. |
| Library: liked songs | **Supported** (cookies) | `music_userdata.rs:137` (`music_liked_tracks` = playlist `LM`) | |
| Library: saved tracks/albums/artists/playlists | **Supported** (cookies) | `music_userdata.rs:73-120` (`userdata` feature) | |
| Library: **uploads** | **Missing** | Nothing browses `FEmusic_library_privately_owned_*`. `MusicVideoType` (`response/url_endpoint.rs:178`) has no `PRIVATELY_OWNED_TRACK` variant and maps unknown types to `Video` via `DefaultOnError`, so uploads would be **misclassified as videos** and hidden by our songs-only filter. | `browse` `FEmusic_library_privately_owned_tracks` / `_releases` / `_artists`. ~1 day. |
| Music history (read) | Supported (cookies) | `music_userdata.rs:23` (`FEmusic_history`) | |
| Like / unlike | **Missing**: rustypipe has no write calls at all | | `like/like`, `like/removelike` with `{target:{videoId}}`. ~0.5 day. |
| Playlist add / remove / reorder / create / delete | **Missing** | | `browse/edit_playlist` with `ACTION_ADD_VIDEO`, `ACTION_REMOVE_VIDEO` (needs `setVideoId`), `ACTION_MOVE_VIDEO_BEFORE`. `playlist/create`, `playlist/delete`. ~1.5 days including setVideoId mapping. |
| Library add / remove (albums, playlists, artists) | **Missing** | | `like/like` on `playlistId` (albums are `OLAK5uy_…` playlists), `subscription/subscribe` / `unsubscribe` for artists, and `feedback` with the `feedbackToken`s from the menus for "add to library". ~1 day. |
| History reporting | **Missing** | | GET `playbackTracking.videostatsPlaybackUrl` (then `…WatchtimeUrl`) from the player response, with `ver=2&cpn=…&docid=…`, signed in. The visionOS player response carries these URLs. ~1 day, plus manual check that it shows on music.youtube.com. |
| Lyrics | **Partial**: plain text only | `music_details.rs:68`, `model/mod.rs:1334` (`Lyrics {body, footer}`), works live (LyricFind) | Synced lyrics come from the YTM Android client (`ANDROID_MUSIC` `browse` with the lyrics id, `timedLyricsData`). ~1 day. LRCLIB covers the gap until then. |
| Music video type (ATV vs OMV/UGC) | **Partial** | `TrackType {Track, Video, Episode}` (`model/mod.rs:1160`). ATV maps to `Track`. OMV and UGC are merged into `Video`. Uploads are wrong (see above). | Add our own enum or patch. Trivial. |
| `loudnessDb` | **Supported**, per stream | `AudioStream::loudness_db` (`model/mod.rs:278`). Docs say it's missing on the Android client. Live: 3.14 dB on every format. | |
| Stream selection, itags 140/141 | Supported in the model, **broken in practice** | `player.rs:91-253`. Default order without botguard is `[Ios, Tv]`, with it `[Desktop, Ios, Tv]`. | Our own player call (visionOS client, see [streams.md](streams.md)). |
| PO tokens / botguard | **Partial**, needs an external process | `client/mod.rs:1013-1060`. PO tokens come from a separate `rustypipe-botguard` **executable** (`detect_botguard_bin`). We can't ship that on Android without bundling a binary. | The visionOS client needs none today. |
| `n` / signature deobfuscation | **Broken** | `src/deobfuscate.rs` (QuickJS via `rquickjs` 0.9) can't find the sig function in the current player JS (#75, reproduced). | Needs a maintained solver: keep patching regexes, or port yt-dlp's `jsc` approach. |
| Auth: cookies | Supported | `user_auth_set_cookie`, `user_auth_set_cookie_txt` (`client/mod.rs:1619,1648`), SAPISIDHASH header (`:2163`). There's no cookie rotation (`RotateCookies`), and the docs warn that the browser session the cookies came from must not be used again. | |
| Auth: OAuth | Supported (TV device-code flow) | `user_auth_get_code` / `user_auth_login` (`client/mod.rs:1353-1461`). Google has been blocking TV OAuth for third parties since late 2024, and yt-dlp dropped it. Don't rely on it. | |

## Implications

- **Catalogue and library reads**: rustypipe is usable today, with fixes for uploads and video type.
- **Playback**: rustypipe can't be used as shipped. We need our own `player` call (see streams.md).
- **All writes, history, home, uploads and synced lyrics** are in-tree InnerTube work. Rough estimate: **6–7 days**, plus recorded fixtures.
- With nobody maintaining it, every YouTube change becomes our job either way. Forking rustypipe into the workspace, or replacing it with a slim in-tree client, costs about the same over time. This goes against the "rustypipe in the core" decision: see README, *Needs a decision*.
