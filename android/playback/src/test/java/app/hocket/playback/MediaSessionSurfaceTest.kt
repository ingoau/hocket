package app.hocket.playback

import app.hocket.core.api.MediaSessionAction
import app.hocket.core.api.MediaSessionMetadata
import app.hocket.core.api.MediaSessionState
import app.hocket.core.api.PositionStamp
import app.hocket.core.api.RepeatMode
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class MediaSessionSurfaceTest {
    private fun state(playing: Boolean = true, positionMs: UInt = 1000u, takenAt: Double = 0.0) = MediaSessionState(
        MediaSessionMetadata("Song", "Artist", "Album", 200_000u, null, "t1", false, 0u),
        playing,
        PositionStamp(positionMs, takenAt, 1.0, playing),
        false,
        RepeatMode.Off,
        1.0,
        false,
        listOf(MediaSessionAction.Play, MediaSessionAction.Pause, MediaSessionAction.Next),
        true,
    )

    @Test
    fun positionOnlyTicksShareAUiSurface() {
        val a = state(positionMs = 1_000u)
        val b = state(positionMs = 40_000u)
        assertTrue(a.sameUiSurface(b))
        assertFalse(a == b)
    }

    @Test
    fun playPauseAndMetadataChangeTheSurface() {
        val a = state(playing = true)
        assertFalse(a.sameUiSurface(a.copy(isPlaying = false)))
        assertFalse(a.sameUiSurface(a.copy(metadata = a.metadata!!.copy(title = "Other"))))
        assertFalse(a.sameUiSurface(a.copy(volume = 0.2)))
    }

    @Test
    fun aSeekIsNotAPositionTick() {
        val a = state(positionMs = 1_000u)
        assertTrue(a.isPositionTick(state(positionMs = 31_000u, takenAt = 30_000.0)))
        // Same surface, but the position jumped: controllers and the watch must hear it.
        assertFalse(a.isPositionTick(state(positionMs = 90_000u, takenAt = 1_000.0)))
        assertFalse(a.isPositionTick(state(positionMs = 0u, takenAt = 5_000.0)))
        val paused = state(playing = false, positionMs = 40_000u)
        assertTrue(paused.isPositionTick(state(playing = false, positionMs = 40_000u, takenAt = 60_000.0)))
        assertFalse(paused.isPositionTick(state(playing = false, positionMs = 10_000u, takenAt = 60_000.0)))
    }
}
