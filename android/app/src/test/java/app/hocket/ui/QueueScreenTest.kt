package app.hocket.ui

import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performScrollToNode
import androidx.test.ext.junit.runners.AndroidJUnit4
import app.hocket.ui.queue.QueuePanel
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotEquals
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode

@RunWith(AndroidJUnit4::class)
@Config(sdk = [35], application = android.app.Application::class)
@GraphicsMode(GraphicsMode.Mode.NATIVE)
class QueueScreenTest {
    @get:Rule
    val compose = createComposeRule()

    @Test
    fun timelineShowsSectionsAndJumpingChangesTheCurrentTrack() {
        val core = TestCore()
        compose.setThemedContent(core) { QueuePanel() }
        core.start()
        compose.waitUntil(5_000) { core.client.started.value }
        val queue = core.client.queue.value
        val current = queue.current!!.track.title
        compose.onNodeWithText("Now playing").assertIsDisplayed()
        compose.onNodeWithText("Playing next").assertIsDisplayed()
        compose.onNodeWithTag("queue.list").performScrollToNode(hasText("Continuing from " + queue.contextLabel))
        compose.onNodeWithText("Continuing from " + queue.contextLabel).assertIsDisplayed()
        // Tap the first upcoming context item: it becomes current and the old one goes to history.
        val target = queue.upcoming.first()
        compose.onNodeWithTag("queue.list").performScrollToNode(hasText(target.track.title))
        compose.onAllNodesWithText(target.track.title).onFirst().performClick()
        compose.waitUntil(5_000) { core.client.nowPlaying.value?.track?.id == target.track.id }
        assertNotEquals(current, core.client.nowPlaying.value!!.track.title)
        assertEquals(target.track.id, core.client.queue.value.current!!.track.id)
    }

    @Test
    fun recentAndHistoryShareThePanel() {
        val core = TestCore()
        compose.setThemedContent(core) { QueuePanel() }
        core.start()
        compose.waitUntil(5_000) { core.client.started.value }
        compose.onNodeWithText("Recent").performClick()
        compose.onNodeWithText("Pinned").assertIsDisplayed()
        compose.onNodeWithText("History").performClick()
        compose.onNodeWithText("Nothing to undo").assertIsDisplayed()
    }

    private fun androidx.compose.ui.test.SemanticsNodeInteractionCollection.onFirst() = this[0]
}
