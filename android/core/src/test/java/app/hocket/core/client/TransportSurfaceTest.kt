package app.hocket.core.client

import app.hocket.core.api.PositionStamp
import app.hocket.core.api.TransportLease
import app.hocket.core.api.TransportState
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class TransportSurfaceTest {
    private fun t(
        positionMs: UInt = 5_000u,
        takenAt: Double = 0.0,
        playing: Boolean = true,
        volume: Double = 0.5,
    ) = TransportState(
        TransportLease("me", 1u, 1.0),
        PositionStamp(positionMs, takenAt, 1.0, playing),
        false,
        positionMs,
        volume,
    )

    @Test
    fun positionTicksShareAUiSurface() {
        val a = t()
        val b = t(positionMs = 5_750u, takenAt = 750.0)
        assertTrue(a.sameUiSurface(b))
        assertTrue(a.isPositionTick(b))
    }

    @Test
    fun seekIsNotATick() {
        val a = t()
        val b = t(positionMs = 90_000u, takenAt = 800.0)
        assertTrue(a.sameUiSurface(b))
        assertFalse(a.isPositionTick(b))
    }

    @Test
    fun pauseAndVolumeAreNotTheSameSurface() {
        val a = t()
        assertFalse(a.sameUiSurface(t(playing = false)))
        assertFalse(a.sameUiSurface(t(volume = 0.2)))
        assertFalse(a.isPositionTick(t(playing = false)))
    }
}
