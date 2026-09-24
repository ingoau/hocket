# Media session — platform notes

How the `MediaSessionAdapter` seam is implemented on each platform. The core
side is `media_session/mod.rs`: `derive_media_session_state` produces an
`api::MediaSessionState`, the actor emits it as `Event::MediaSession`, and
OS commands come back as `Command::MediaSessionCommand { action, value }`,
mapped by `command_for`. Everything below is platform glue; **no business
logic lives in the adapter**.

## Contract recap (both platforms)

| Field | Meaning |
|---|---|
| `metadata` | `None` when nothing is loaded → clear the session / hide the notification. |
| `metadata.artwork_path` | A local `file://` path resolved through the image cache, or `None`. Never a remote URL. |
| `is_playing` | Playback state for the OS. `false` with metadata = paused. |
| `position` | A stamp `(position_ms, taken_at, rate, is_playing)`, already extrapolated to `taken_at`. The OS extrapolates from it; **never run a timer in the adapter**. |
| `shuffle`, `repeat`, `volume` | Published as properties where the platform has them. |
| `actions` | The customised button list for the `mediaSession` surface, in display order. `Play`/`Pause`/`Stop` are always present when metadata is. |
| `owns_transport` | `false` means this device is a remote control: keep the session alive and publish the state of the playing device; commands are forwarded by the core. |

Incoming values: `Seek` carries the absolute position in ms, `Rate` carries
0–5, `Shuffle`/`Repeat`/`Love` may carry `0`/`1` (`0`/`1`/`2` for repeat) or
nothing to toggle/cycle. Map platform-native "seek by delta" to an absolute
position using the last published stamp.

## Desktop: playwire addon (`crates/hocket-node` or a sibling crate)

Chromium's `navigator.mediaSession` only publishes once an audible player
exists in the page. Desktop audio comes out of Rust, so there is never a
player and the renderer must **not** be used for the media session. The
napi-rs addon wraps [playwire](https://crates.io/crates/playwire) and is fed
from Electron's main process by `Event::MediaSession`.

### Electron switches (mandatory)

```
app.commandLine.appendSwitch(
  'disable-features',
  'HardwareMediaKeyHandling,MediaSessionService'
);
```

Without both, any renderer audio at all (a notification sound, a `<video>`
preview) makes Chromium grab the media keys and the OS session, and the
native session silently loses them. Set before `app.whenReady()`.

### Position

Publish on **every tick** the core emits (it emits `Event::MediaSession` at
≥ 1 Hz while playing and on every discontinuity). playwire diffs against its
last snapshot, so republishing identical metadata is free. On Linux, MPRIS
`Position` is deliberately *not* change-signalling — clients read it on
demand and extrapolate from `Rate`; a seek must raise the `Seeked` signal,
which playwire does when the published position jumps by more than the
elapsed time. Publish `rate` as given (0 when paused), or the macOS
scrubber never advances (`MPNowPlayingInfoPropertyPlaybackRate`).

### Artwork

Pass `metadata.artwork_path` through unchanged. It is a `file://` URL to a
cached image at a fixed size. A remote URL may work on one desktop and
silently fail on another (KDE's MPRIS widget fetches, GNOME's does not; SMTC
needs a stream; macOS needs an `NSImage`), which is why the core resolves it.
When `None`, clear the artwork rather than keeping the previous track's.

### Windows: SMTC + close-to-tray

SMTC ties the session to an HWND. Destroying the main window on
close-to-tray destroys the session with it and the keys stop working until
a new window exists. Create a **persistent hidden window** at startup (a
1×1 `BrowserWindow { show: false }` that is never closed, or a native
message-only window from the addon) and initialise playwire against *its*
handle, not the main window's. Pass the HWND as `hwnd` in playwire's
platform config.

### Linux: MPRIS

- `DesktopEntry` must be set to the app's desktop file basename
  (`app.hocket`) so GNOME/KDE show the right icon and name and can raise
  the app. Set `identity` to the display name.
- Publish `Shuffle`, `LoopStatus` (`None`/`Playlist`/`Track` ↔
  `repeat` `Off`/`All`/`One`), `Volume` (0–1) and `Rate` as properties;
  Linux widgets surface them and send `Shuffle`/`LoopStatus` writes back,
  which map to `MediaSessionAction::Shuffle { value }` and
  `Repeat { value }`.
- `mpris:trackid` must be a valid D-Bus object path unique per queue item,
  e.g. `/app/hocket/track/<queueKey-hex>`; a bare `/` breaks `playerctl`.
- `CanGoNext`/`CanGoPrevious`/`CanSeek` follow `actions`; `CanControl` is
  true whenever metadata is present, including when `owns_transport` is
  false (remote control case).

### macOS

`MPNowPlayingInfoCenter` needs `MPNowPlayingInfoPropertyPlaybackRate` and
`MPNowPlayingInfoPropertyElapsedPlaybackTime` on every publish (both come
from the stamp). Register remote command handlers for exactly the actions
in `actions`; unregistered commands should be disabled (`isEnabled =
false`) so the Now Playing widget greys them out. Love maps to
`likeCommand`, Rate to `ratingCommand` (0–5).

### Maturity note

playwire is newer and less proven than souvlaki was. Keep the addon thin so
fixes can go upstream: no state in the addon beyond the last snapshot it
published, and every platform quirk listed here documented next to the code
that works around it.

## Android: Media3

The playback `MediaSessionService` owns one `HocketCore`. `Event::MediaSession`
maps to:

- `MediaSession.setPlayer` on a `ForwardingPlayer` (or `SimpleBasePlayer`)
  whose state is set from the event: `MediaMetadata` from `metadata`
  (artwork via `setArtworkUri(Uri.fromFile(...))`), playback state from
  `is_playing`, position from the stamp (`SimpleBasePlayer.PositionSupplier`
  extrapolating from `taken_at`/`rate`), shuffle/repeat from the fields.
- `actions` → available `Player.Command`s plus custom `SessionCommand`s
  (`love`, `rate`) exposed as `CommandButton`s in the notification, in the
  given order.
- `owns_transport == false` keeps the session and notification alive as a
  remote control. The session reports remote playback (Media3 `DeviceInfo`
  `PLAYBACK_TYPE_REMOTE` with a routing controller id) and a
  `MediaRoute2ProviderService` publishes the Connect devices as routes plus a
  routing session with that id named after the playing device, so the system
  media controls' output chip shows it (see `android/README.md`).

Incoming `Player` calls (`play`, `pause`, `seekTo`, `seekToNext`,
`setShuffleModeEnabled`, `setRepeatMode`, custom commands) become
`Command::MediaSessionCommand` posted to the core — the service never
touches ExoPlayer directly for them; the core answers through
`Event::Backend`.
