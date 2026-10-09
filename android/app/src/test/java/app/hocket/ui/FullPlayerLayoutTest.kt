package app.hocket.ui

import androidx.activity.ComponentActivity
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.test.onAllNodesWithTag
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.performSemanticsAction
import androidx.test.ext.junit.runners.AndroidJUnit4
import app.hocket.core.Commands
import app.hocket.ui.nav.AppRoot
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode

/**
 * The full player's page: the artwork at the top with the title right under it and the rest spread
 * over the remaining height; the album beside the thumbnail in the other modes; "Playing from"
 * opening the queue switcher; another device playing shown only by the highlighted Connect button,
 * which opens the device picker.
 */
@RunWith(AndroidJUnit4::class)
@Config(sdk = [35], application = android.app.Application::class, qualifiers = "w411dp-h891dp")
@GraphicsMode(GraphicsMode.Mode.NATIVE)
class FullPlayerLayoutTest {
    @get:Rule
    val compose = createAndroidComposeRule<ComponentActivity>()

    private val core = TestCore()

    private fun exists(tag: String) = compose.onAllNodesWithTag(tag).fetchSemanticsNodes().isNotEmpty()
    private fun bounds(tag: String): Rect = compose.onNodeWithTag(tag).fetchSemanticsNode().boundsInRoot
    private fun click(tag: String) = compose.onNodeWithTag(tag).performSemanticsAction(SemanticsActions.OnClick)
    private val dp get() = compose.activity.resources.displayMetrics.density

    private fun startExpanded() {
        compose.setThemedContent(core) { AppRoot(core.client) }
        core.start()
        compose.waitUntil(5_000) { core.client.started.value }
        compose.waitUntil(5_000) { exists("miniPlayer") }
        compose.onNodeWithTag("nowPlaying.sheet").performSemanticsAction(SemanticsActions.Expand)
        compose.waitUntil(5_000) { exists("player.playPause") }
        compose.waitForIdle()
    }

    @Test
    fun theArtworkIsAtTheTopWithTheTitleUnderItAndTheRestSpreadOut() {
        startExpanded()
        val source = bounds("player.source")
        val art = bounds("player.artwork")
        val title = bounds("player.title")
        val play = bounds("player.playPause")
        val pills = bounds("player.mode.lyrics")
        assertTrue("artwork right under the header: $source / $art", art.top - source.bottom in 0f..24f * dp)
        assertTrue("title right under the artwork: $art / $title", title.top - art.bottom in 0f..40f * dp)
        // The spare height goes between the controls (and under the last), not into one band:
        // the transport and the pills are well apart but nothing hangs off the bottom.
        val root = compose.onNodeWithTag("nowPlaying.sheet").fetchSemanticsNode().boundsInRoot
        val gap = pills.top - play.bottom
        assertTrue("the rest spread out (gap $gap)", gap > 16f * dp)
        assertTrue("the pills clear of the screen's bottom (${pills.bottom} in $root)", pills.bottom < root.bottom)
        assertTrue("and not all pushed to the bottom", root.bottom - pills.bottom > 12f * dp)
    }

    @Test
    fun theAlbumStaysBesideTheThumbnailInTheOtherModes() {
        startExpanded()
        val album = core.client.nowPlaying.value!!.track.album
        // The lyrics animate every frame while shown: step the clock by hand instead of waiting for idle.
        compose.mainClock.autoAdvance = false
        try {
            for (mode in listOf("lyrics", "queue", "about")) {
                click("player.mode.$mode")
                compose.mainClock.advanceTimeBy(1_000)
                assertTrue("album shown in $mode", exists("player.album"))
                if (album != null) assertEquals(album, compose.onNodeWithTag("player.album").fetchSemanticsNode().config[SemanticsProperties.Text].joinToString { it.text })
                click("player.mode.$mode")
                compose.mainClock.advanceTimeBy(1_000)
            }
        } finally {
            compose.mainClock.autoAdvance = true
        }
    }

    @Test
    fun playingFromOpensTheQueueSwitcherAndPickingAQueueRestoresItAndClosesIt() {
        startExpanded()
        click("player.source")
        compose.waitUntil(5_000) { exists("queueSwitcher.sheet") }
        val saved = core.client.savedQueues.value.first { it.pinned }
        compose.waitUntil(5_000) { exists("savedQueue.${saved.id}") }
        click("savedQueue.${saved.id}")
        compose.waitUntil(5_000) { !exists("queueSwitcher.sheet") }
        compose.waitUntil(5_000) { core.client.queue.value.contextLabel == saved.label }
        compose.waitForIdle()
        assertEquals("Playing from ${saved.label}", compose.onNodeWithTag("player.source").fetchSemanticsNode().config[SemanticsProperties.ContentDescription].single())
    }

    @Test
    fun playingOnAnotherDeviceHasNoTextAndConnectOpensTheDevicePicker() {
        startExpanded()
        core.client.dispatch(Commands.handoffTo("laptop"))
        // The client reads who owns the transport from snapshots.
        core.client.requestSnapshot()
        compose.waitUntil(5_000) { !core.client.ownsTransport.value }
        compose.waitForIdle()
        assertTrue("no \"Playing on\" line", compose.onAllNodesWithText("Playing on", substring = true).fetchSemanticsNodes().isEmpty())
        click("player.connect")
        compose.waitUntil(5_000) { exists("handoff.sheet") }
    }
}
