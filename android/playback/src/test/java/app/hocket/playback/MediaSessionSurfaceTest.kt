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
    private fun state(playing: Boolean = true, positionMs: UInt = 1000u) = MediaSessionState(
        MediaSessionMetadata("Song", "Artist", "Album", 200_000u, null, "t1", false, 0u),
        playing,
        PositionStamp(positionMs, 0.0, 1.0, playing),
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
}
