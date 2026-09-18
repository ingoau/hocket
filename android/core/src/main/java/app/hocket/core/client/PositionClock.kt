package app.hocket.core.client

import app.hocket.core.api.PositionStamp
import app.hocket.core.api.TransportState

/**
 * Extrapolates playback position from the last [PositionStamp] the way the Connect protocol
 * intends: `position + (now - takenAt) * rate` while playing, frozen otherwise. `takenAt` is on the
 * session clock, so the connection's `clockOffsetMs` (session - local) is applied first.
 *
 * Pure so it can be unit-tested; the 60 Hz ticker in [CoreClient] just calls it every frame.
 */
object PositionClock {
    fun extrapolate(stamp: PositionStamp, nowLocalMs: Double, clockOffsetMs: Double = 0.0, durationMs: Long? = null): Long {
        val base = stamp.positionMs.toLong()
        if (!stamp.isPlaying) return clamp(base, durationMs)
        val sessionNow = nowLocalMs + clockOffsetMs
        val elapsed = (sessionNow - stamp.takenAt).coerceAtLeast(0.0) * stamp.rate
        return clamp(base + elapsed.toLong(), durationMs)
    }

    fun extrapolate(transport: TransportState, nowLocalMs: Double, clockOffsetMs: Double = 0.0, durationMs: Long? = null): Long =
        extrapolate(transport.position, nowLocalMs, clockOffsetMs, durationMs)

    private fun clamp(value: Long, durationMs: Long?): Long {
        val lower = value.coerceAtLeast(0)
        return if (durationMs != null && durationMs > 0) lower.coerceAtMost(durationMs) else lower
    }
}
