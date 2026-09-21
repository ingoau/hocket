package app.hocket.ui

import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performTouchInput
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.test.swipe
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

    private fun displayed(tag: String): Boolean = runCatching { compose.onNodeWithTag(tag).assertIsDisplayed() }.isSuccess

    @Test
    fun miniPlayerExpandsOnSwipeUpAndCollapsesOnSwipeDown() {
        val core = TestCore()
        compose.setThemedContent(core) { AppRoot(core.client) }
        core.start()
        compose.waitUntil(5_000) { core.client.started.value }
        compose.onNodeWithTag("miniPlayer").assertIsDisplayed()
        // Drag the mini bar up: a fast, long swipe starting inside its bounds.
        compose.onNodeWithTag("miniPlayer").performTouchInput { swipe(start = center, end = center - Offset(0f, 1200f), durationMillis = 150) }
        compose.waitUntil(5_000) { displayed("player.playPause") }
        compose.onNodeWithTag("player.title").assertIsDisplayed()
        // Drag the expanded content down (through the nested scroll connection) to collapse.
        compose.onNodeWithTag("player.title").performTouchInput { swipe(start = center, end = center + Offset(0f, 1200f), durationMillis = 150) }
        compose.waitUntil(5_000) { !displayed("player.playPause") }
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
        compose.waitUntil(5_000) { displayed("player.next") }
        val before = core.client.nowPlaying.value!!.track.id
        compose.onNodeWithTag("player.next").performClick()
        compose.waitUntil(5_000) { core.client.nowPlaying.value?.track?.id != before }
    }
}
