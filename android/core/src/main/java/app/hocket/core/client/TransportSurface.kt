package app.hocket.core.client

import app.hocket.core.api.TransportState
import kotlin.math.abs

/**
 * Transport fields Compose actually draws. Position is omitted: screens extrapolate
 * from [TransportState.position] via [PositionClock].
 */
fun TransportState.sameUiSurface(other: TransportState): Boolean =
    lease.owner == other.lease.owner &&
        lease.epoch == other.lease.epoch &&
        buffering == other.buffering &&
        volume == other.volume &&
        muted == other.muted &&
        volumeDevice == other.volumeDevice &&
        position.isPlaying == other.position.isPlaying &&
        position.rate == other.position.rate

/**
 * True when [next] is the ~1 Hz position report that follows [this], not a seek,
 * pause, or volume/lease change. UIs should keep extrapolating instead of recomposing.
 */
fun TransportState.isPositionTick(next: TransportState): Boolean {
    if (!sameUiSurface(next)) return false
    val dt = next.position.takenAt - position.takenAt
    if (dt <= 0.0 || dt > 2_500.0) return false
    val expected = position.positionMs.toLong() + (dt * position.rate).toLong()
    return abs(next.position.positionMs.toLong() - expected) < 1_500L
}
