package app.hocket.ui

import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performTouchInput
import androidx.compose.ui.test.swipeDown
import androidx.compose.ui.test.swipeUp
import androidx.test.ext.junit.runners.AndroidJUnit4
import app.hocket.ui.nav.AppRoot
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode

@RunWith(AndroidJUnit4::class)
@Config(sdk = [35], application = android.app.Application::class, qualifiers = "w411dp-h891dp")
@GraphicsMode(GraphicsMode.Mode.NATIVE)
class NowPlayingSheetTest {
    @get:Rule
    val compose = createComposeRule()

    @Test
    fun miniPlayerExpandsOnSwipeUpAndCollapsesOnSwipeDown() {
        val core = TestCore()
        compose.setThemedContent(core) { AppRoot(core.client) }
        core.start()
        compose.waitUntil(5_000) { core.client.started.value }
        compose.onNodeWithTag("miniPlayer").assertIsDisplayed()
        compose.onNodeWithTag("nowPlaying.sheet").performTouchInput { swipeUp() }
        compose.waitUntil(5_000) { compose.onAllNodesWithTagCount("player.playPause") == 1 }
        compose.onNodeWithTag("player.playPause").assertIsDisplayed()
        compose.onNodeWithTag("player.title").assertIsDisplayed()
        compose.onNodeWithTag("nowPlaying.sheet").performTouchInput { swipeDown() }
        compose.waitUntil(5_000) { compose.onAllNodesWithTagCount("player.playPause") == 0 }
        compose.onNodeWithTag("miniPlayer").assertIsDisplayed()
    }

    @Test
    fun tappingTheMiniPlayerExpandsAndTransportWorks() {
        val core = TestCore()
        compose.setThemedContent(core) { AppRoot(core.client) }
        core.start()
        compose.waitUntil(5_000) { core.client.started.value }
        assertTrue(core.client.isPlaying.value)
        compose.onNodeWithTag("miniPlayer.playPause").performClick()
        compose.waitUntil(5_000) { !core.client.isPlaying.value }
        assertFalse(core.client.isPlaying.value)
        compose.onNodeWithTag("miniPlayer").performClick()
        compose.waitUntil(5_000) { compose.onAllNodesWithTagCount("player.next") == 1 }
        val before = core.client.nowPlaying.value!!.track.id
        compose.onNodeWithTag("player.next").performClick()
        compose.waitUntil(5_000) { core.client.nowPlaying.value?.track?.id != before }
    }
}
