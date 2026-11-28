package app.hocket.ui

import androidx.activity.ComponentActivity
import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.performSemanticsAction
import androidx.test.ext.junit.runners.AndroidJUnit4
import app.hocket.core.Commands
import app.hocket.core.SettingKeys
import app.hocket.core.api.LyricsTier
import app.hocket.core.api.TrackRef
import app.hocket.ui.nav.AppRoot
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode

/**
 * The line being sung in the full player (`lyrics.playerLine`): off by default (no line); on, it
 * shows above the title with the artwork moved up to make room, and nothing else on the page moves.
 */
@RunWith(AndroidJUnit4::class)
@Config(sdk = [35], application = android.app.Application::class, qualifiers = "w411dp-h891dp")
@GraphicsMode(GraphicsMode.Mode.NATIVE)
class PlayerLyricLineTest {
    @get:Rule
    val compose = createAndroidComposeRule<ComponentActivity>()

    private fun exists(tag: String) = runCatching { compose.onNodeWithTag(tag).assertExists() }.isSuccess

    private fun bounds(tag: String) = compose.onNodeWithTag(tag, useUnmergedTree = true).fetchSemanticsNode().boundsInRoot

    @Test
    fun offByDefaultAndOnWithoutMovingTheControls() {
        val core = TestCore()
        val lyrics = core.fake.library.lyricsByTrack
        val track = lyrics.entries.first { it.value?.tier == LyricsTier.Syllable }.key
        val plain = core.fake.library.tracks.first { lyrics[it.id] == null }.id
        compose.setThemedContent(core) { AppRoot(core.client) }
        core.start()
        compose.waitUntil(5_000) { core.client.started.value }
        core.client.dispatch(Commands.playTracks(listOf(TrackRef(core.fake.library.serverId, track)), 0, "Lyrics"))
        compose.waitUntil(5_000) { core.client.nowPlaying.value?.track?.id == track }
        assertEquals("false", core.client.settings.value[SettingKeys.LYRICS_PLAYER_LINE]?.value?.trim())
        compose.onNodeWithTag("miniPlayer.info").performSemanticsAction(SemanticsActions.OnClick)
        compose.waitUntil(5_000) { exists("player.playPause") }
        compose.waitForIdle()
        assertFalse("no line by default", exists("player.lyricLine"))
        val controls = listOf("player.title", "player.artistLine", "player.elapsed", "player.playPause", "player.mode.queue")
        val before = controls.associateWith { bounds(it) }
        val art = bounds("player.artwork")
        // The line's playhead ticks every frame while a song plays: the clock goes by hand.
        compose.mainClock.autoAdvance = false
        fun frames(n: Int = 30) = repeat(n) { compose.mainClock.advanceTimeByFrame() }

        core.client.dispatch(Commands.setSetting(SettingKeys.LYRICS_PLAYER_LINE, "true"))
        frames()
        assertTrue("the line", exists("player.lyricLine"))
        for (t in controls) assertEquals("$t moved", before[t], bounds(t))
        // The artwork moves up to make room for it (and only up): the same size or smaller.
        val up = bounds("player.artwork")
        assertTrue("the artwork up: $art -> $up", up.bottom < art.bottom && up.top <= art.top)

        // A song without lyrics keeps the room: the artwork stays where it is from song to song.
        core.client.dispatch(Commands.playTracks(listOf(TrackRef(core.fake.library.serverId, plain)), 0, "Plain"))
        frames(60)
        assertEquals(plain, core.client.nowPlaying.value?.track?.id)
        assertEquals("the artwork without lyrics", up, bounds("player.artwork"))
        for (t in listOf("player.elapsed", "player.playPause", "player.mode.queue")) assertEquals("$t moved", before[t], bounds(t))

        core.client.dispatch(Commands.setSetting(SettingKeys.LYRICS_PLAYER_LINE, "false"))
        frames()
        assertFalse("off again", exists("player.lyricLine"))
        assertEquals("the artwork back", art, bounds("player.artwork"))
    }
}
