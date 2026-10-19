package app.hocket.playback

import app.hocket.core.api.MediaSessionState
import app.hocket.core.client.continuesTo

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

/**
 * Nothing a controller shows changed and the position carries on from [this] stamp: a seek, a
 * restart or a jump after a stall is not a tick, so the OS session and the watch hear about it.
 */
fun MediaSessionState.isPositionTick(next: MediaSessionState): Boolean =
    sameUiSurface(next) && position.continuesTo(next.position)
