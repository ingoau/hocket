package app.hocket.core.client

import app.hocket.core.api.PositionStamp
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
fun TransportState.isPositionTick(next: TransportState): Boolean =
    sameUiSurface(next) && position.continuesTo(next.position, maxGapMs = 2_500.0)

/**
 * How far a stamp may stray from where the last one extrapolates to and still be the same playback.
 * The core only reports a stamp that strays 250 ms or more, so past this it is a seek or a stall.
 */
const val TICK_TOLERANCE_MS = 300L

/**
 * True when extrapolating this stamp lands on [next] (within [toleranceMs]): the same playback
 * carrying on, not a seek or a restart. A stamp more than [maxGapMs] older is never a continuation.
 */
fun PositionStamp.continuesTo(next: PositionStamp, toleranceMs: Long = TICK_TOLERANCE_MS, maxGapMs: Double = Double.MAX_VALUE): Boolean {
    val dt = next.takenAt - takenAt
    if (dt < 0.0 || dt > maxGapMs) return false
    val rate = if (isPlaying) rate else 0.0
    val expected = positionMs.toLong() + (dt * rate).toLong()
    return abs(next.positionMs.toLong() - expected) < toleranceMs
}
