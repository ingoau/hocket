package app.hocket.playback

import app.hocket.core.api.MediaSessionState

/**
 * Session fields the OS notification, widget and Pebble now-playing screen actually draw.
 * Position is omitted: every consumer extrapolates from [MediaSessionState.position].
 */
fun MediaSessionState.sameUiSurface(other: MediaSessionState): Boolean =
    metadata == other.metadata &&
        isPlaying == other.isPlaying &&
        shuffle == other.shuffle &&
        repeat == other.repeat &&
        volume == other.volume &&
        muted == other.muted &&
        actions == other.actions &&
        ownsTransport == other.ownsTransport
