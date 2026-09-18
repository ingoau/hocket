package app.hocket.core.client

import app.hocket.core.api.PositionStamp
import org.junit.Assert.assertEquals
import org.junit.Test

class PositionClockTest {
    @Test
    fun pausedStampDoesNotMove() {
        val stamp = PositionStamp(5_000u, 1_000.0, 1.0, false)
        assertEquals(5_000L, PositionClock.extrapolate(stamp, nowLocalMs = 60_000.0))
    }

    @Test
    fun playingStampAdvancesByElapsedTimesRate() {
        val stamp = PositionStamp(5_000u, 1_000.0, 1.0, true)
        assertEquals(7_500L, PositionClock.extrapolate(stamp, nowLocalMs = 3_500.0))
        val fast = stamp.copy(rate = 2.0)
        assertEquals(10_000L, PositionClock.extrapolate(fast, nowLocalMs = 3_500.0))
    }

    @Test
    fun clockOffsetIsApplied() {
        // Session clock is 1 s ahead of local: takenAt was stamped on the session clock.
        val stamp = PositionStamp(0u, 11_000.0, 1.0, true)
        assertEquals(0L, PositionClock.extrapolate(stamp, nowLocalMs = 10_000.0, clockOffsetMs = 0.0))
        assertEquals(1_000L, PositionClock.extrapolate(stamp, nowLocalMs = 11_000.0, clockOffsetMs = 1_000.0))
    }

    @Test
    fun clampsToDuration() {
        val stamp = PositionStamp(9_000u, 0.0, 1.0, true)
        assertEquals(10_000L, PositionClock.extrapolate(stamp, nowLocalMs = 5_000.0, durationMs = 10_000))
    }

    @Test
    fun neverNegativeOnClockSkew() {
        val stamp = PositionStamp(1_000u, 50_000.0, 1.0, true)
        assertEquals(1_000L, PositionClock.extrapolate(stamp, nowLocalMs = 10_000.0))
    }
}
