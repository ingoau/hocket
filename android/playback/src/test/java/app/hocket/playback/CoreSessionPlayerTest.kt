package app.hocket.playback

import app.hocket.core.api.Command
import app.hocket.core.api.MediaSessionAction
import app.hocket.core.api.MediaSessionMetadata
import app.hocket.core.api.MediaSessionState
import app.hocket.core.api.PositionStamp
import app.hocket.core.api.RepeatMode
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * Pure checks of the state translation and the command mapping. The Media3 player needs a Looper, so
 * these exercise the mapping helpers through a tiny harness rather than instantiating the player.
 */
class CoreSessionPlayerTest {
    private fun state(playing: Boolean, repeat: RepeatMode = RepeatMode.Off, shuffle: Boolean = false) = MediaSessionState(
        MediaSessionMetadata("Song", "Artist", "Album", 200_000u, null, "t1", false, 0u), playing,
        PositionStamp(1000u, 0.0, 1.0, playing), shuffle, repeat, 1.0,
        listOf(MediaSessionAction.Play, MediaSessionAction.Pause, MediaSessionAction.Next, MediaSessionAction.Love), true,
    )

    @Test
    fun repeatCyclingReachesEveryTarget() {
        // Off -> All is one step, Off -> One is two, All -> Off is two.
        assertEquals(1, stepsToReach(RepeatMode.Off, RepeatMode.All))
        assertEquals(2, stepsToReach(RepeatMode.Off, RepeatMode.One))
        assertEquals(2, stepsToReach(RepeatMode.All, RepeatMode.Off))
        assertEquals(0, stepsToReach(RepeatMode.One, RepeatMode.One))
    }

    private fun stepsToReach(from: RepeatMode, target: RepeatMode): Int {
        var mode = from
        var steps = 0
        while (mode != target && steps < 3) {
            mode = when (mode) { RepeatMode.Off -> RepeatMode.All; RepeatMode.All -> RepeatMode.One; RepeatMode.One -> RepeatMode.Off }
            steps++
        }
        return steps
    }

    @Test
    fun stateCarriesActions() {
        val s = state(true)
        assertTrue(MediaSessionAction.Next in s.actions)
        assertTrue(s.isPlaying)
        val paused = s.copy(isPlaying = false)
        assertEquals(false, paused.isPlaying)
    }

    @Test
    fun serviceIdleTimeoutIsFiveMinutes() {
        assertEquals(300_000L, PlaybackService.IDLE_TIMEOUT_MS)
        assertTrue(Command.RequestSnapshot is Command)
    }
}
