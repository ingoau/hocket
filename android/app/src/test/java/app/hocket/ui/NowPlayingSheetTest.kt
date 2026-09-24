package app.hocket.ui

import androidx.activity.ComponentActivity
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performTouchInput
import androidx.compose.ui.test.swipe
import androidx.test.ext.junit.runners.AndroidJUnit4
import app.hocket.core.Commands
import app.hocket.core.api.Command
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
 * The now-playing sheet's open and dismiss paths: the drag up from the mini bar, the drag down
 * through the player's nested scroll, the back gesture (predictive back), the mini bar's skip swipe,
 * with playback running and paused, and repeatedly, so a dismissal never reads state after the
 * sheet has left composition or trips over a null current item.
 */
@RunWith(AndroidJUnit4::class)
@Config(sdk = [35], application = android.app.Application::class, qualifiers = "w411dp-h891dp")
@GraphicsMode(GraphicsMode.Mode.NATIVE)
class NowPlayingSheetTest {
    @get:Rule
    val compose = createAndroidComposeRule<ComponentActivity>()

    private fun displayed(tag: String): Boolean = runCatching { compose.onNodeWithTag(tag).assertIsDisplayed() }.isSuccess

    private fun start(core: TestCore) {
        compose.setThemedContent(core) { AppRoot(core.client) }
        core.start()
        compose.waitUntil(5_000) { core.client.started.value }
        compose.onNodeWithTag("miniPlayer").assertIsDisplayed()
    }

    private fun expand() {
        // Drag the mini bar up: a fast, long swipe starting inside its bounds.
        compose.onNodeWithTag("miniPlayer").performTouchInput { swipe(start = center, end = center - Offset(0f, 1200f), durationMillis = 150) }
        compose.waitUntil(5_000) { displayed("player.playPause") }
        compose.onNodeWithTag("player.title").assertIsDisplayed()
    }

    private fun collapseByDrag() {
        // Drag the expanded content down (through the nested scroll connection) to collapse.
        compose.onNodeWithTag("player.title").performTouchInput { swipe(start = center, end = center + Offset(0f, 1200f), durationMillis = 150) }
        compose.waitUntil(5_000) { !displayed("player.playPause") }
        compose.onNodeWithTag("miniPlayer").assertIsDisplayed()
    }

    @Test
    fun miniPlayerExpandsOnSwipeUpAndCollapsesOnSwipeDown() {
        val core = TestCore()
        start(core)
        assertTrue(core.client.isPlaying.value)
        expand()
        collapseByDrag()
    }

    @Test
    fun dismissingTheSheetWhilePausedWorksToo() {
        val core = TestCore()
        start(core)
        core.client.dispatch(Command.Pause)
        compose.waitUntil(5_000) { !core.client.isPlaying.value }
        expand()
        collapseByDrag()
        // And back up and down again on the same sheet state.
        expand()
        collapseByDrag()
        assertFalse(core.client.isPlaying.value)
    }

    @Test
    fun theBackGestureCollapsesTheExpandedSheet() {
        val core = TestCore()
        start(core)
        expand()
        compose.activityRule.scenario.onActivity { it.onBackPressedDispatcher.onBackPressed() }
        compose.waitUntil(5_000) { !displayed("player.playPause") }
        compose.onNodeWithTag("miniPlayer").assertIsDisplayed()
        // With the sheet collapsed, back is no longer intercepted: the sheet stays collapsed and the
        // shell is still there.
        compose.activityRule.scenario.onActivity { it.onBackPressedDispatcher.onBackPressed() }
        compose.waitForIdle()
        compose.onNodeWithTag("miniPlayer").assertIsDisplayed()
        // And the sheet still opens afterwards.
        expand()
        collapseByDrag()
    }

    @Test
    fun swipingTheMiniPlayerToSkipPastTheEndOfTheQueueDoesNotCrash() {
        val core = TestCore()
        start(core)
        val track = core.client.nowPlaying.value!!.track.id
        core.client.dispatch(Commands.playTracks(core.fake.library.serverId, listOf(track), 0, "One track"))
        compose.waitUntil(5_000) { core.client.queue.value.upcoming.isEmpty() }
        // Skip forward on the last track, then back on the first: both swipes cross the threshold.
        compose.onNodeWithTag("miniPlayer").performTouchInput { swipe(start = center, end = center - Offset(300f, 0f), durationMillis = 150) }
        compose.waitForIdle()
        compose.onNodeWithTag("miniPlayer").assertIsDisplayed()
        compose.onNodeWithTag("miniPlayer").performTouchInput { swipe(start = center, end = center + Offset(300f, 0f), durationMillis = 150) }
        compose.waitForIdle()
        compose.onNodeWithTag("miniPlayer").assertIsDisplayed()
        assertEquals(track, core.client.nowPlaying.value!!.track.id)
        expand()
        collapseByDrag()
    }

    @Test
    fun tappingTheMiniPlayerExpandsAndTransportWorks() {
        val core = TestCore()
        start(core)
        assertTrue(core.client.isPlaying.value)
        compose.onNodeWithTag("miniPlayer.playPause").performClick()
        compose.waitUntil(5_000) { !core.client.isPlaying.value }
        assertFalse(core.client.isPlaying.value)
        compose.onNodeWithTag("miniPlayer").performClick()
        compose.waitUntil(5_000) { displayed("player.next") }
        val before = core.client.nowPlaying.value!!.track.id
        compose.onNodeWithTag("player.next").performClick()
        compose.waitUntil(5_000) { core.client.nowPlaying.value?.track?.id != before }
    }
}
