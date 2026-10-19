package app.hocket.playback

import app.hocket.core.api.Event
import app.hocket.core.api.EventJobsChangedInner
import app.hocket.core.api.EventLyricsChangedInner
import app.hocket.core.api.EventLogInner
import app.hocket.core.api.EventTransportChangedInner
import app.hocket.core.api.PositionStamp
import app.hocket.core.api.TransportLease
import app.hocket.core.api.TransportState
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/** What reaches the service's main thread: what it (or the watch) acts on, and nothing else. */
class ServiceEventGateTest {
    private fun transport(positionMs: Long, takenAt: Double, playing: Boolean = true, volume: Double = 0.5) = Event.TransportChanged(
        EventTransportChangedInner(TransportState(TransportLease("me", 1u, 1.0), PositionStamp(positionMs.toUInt(), takenAt, 1.0, playing), false, positionMs.toUInt(), volume)),
    )

    @Test
    fun aMinuteOfPlaybackPostsOnlyTheFirstStamp() {
        val gate = ServiceEventGate()
        var posted = 0
        for (i in 0..80) if (gate.admit(transport(i * 750L, i * 750.0))) posted++
        println("MEASURE service main posts for 81 transport stamps (one each 750 ms): $posted")
        assertEquals(1, posted)
        assertTrue("a pause is posted", gate.admit(transport(60_750, 60_800.0, playing = false)))
        assertTrue("a seek is posted", gate.admit(transport(5_000, 61_000.0, playing = false)))
        assertTrue("a volume change is posted", gate.admit(transport(5_000, 61_200.0, playing = false, volume = 0.2)))
    }

    @Test
    fun progressTheServiceIgnoresIsNotPosted() {
        val gate = ServiceEventGate()
        assertFalse(gate.admit(Event.JobsChanged(EventJobsChangedInner(emptyList()))))
        assertFalse(gate.admit(Event.Log(EventLogInner("warn", "hocket_core", "x"))))
        assertFalse(gate.admit(Event.LyricsChanged(EventLyricsChangedInner("t1", null))))
    }
}
