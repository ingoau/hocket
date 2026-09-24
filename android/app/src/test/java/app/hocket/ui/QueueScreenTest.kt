package app.hocket.ui

import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performScrollToNode
import androidx.compose.ui.test.performTouchInput
import androidx.compose.ui.test.hasTestTag
import androidx.compose.ui.test.swipeUp
import androidx.compose.ui.geometry.Offset
import androidx.test.ext.junit.runners.AndroidJUnit4
import app.hocket.core.Commands
import app.hocket.core.api.Command
import app.hocket.core.api.RepeatMode
import app.hocket.ui.queue.QueuePanel
import app.hocket.ui.queue.SavedQueuesScreen
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.semantics.getOrNull
import androidx.compose.ui.test.SemanticsMatcher
import androidx.compose.ui.test.assertContentDescriptionEquals
import androidx.compose.ui.test.assertIsNotDisplayed
import androidx.compose.ui.test.assertIsOff
import androidx.compose.ui.test.assertIsOn
import androidx.compose.ui.test.assertIsToggleable
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.unit.dp
import androidx.navigation.compose.rememberNavController
import org.junit.Assert.assertTrue
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotEquals
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode

@RunWith(AndroidJUnit4::class)
@Config(sdk = [35], application = android.app.Application::class, qualifiers = "w411dp-h891dp")
@GraphicsMode(GraphicsMode.Mode.NATIVE)
class QueueScreenTest {
    @get:Rule
    val compose = createComposeRule()

    @Test
    fun oneListShowsSectionsAndJumpingChangesTheCurrentTrack() {
        val core = TestCore()
        compose.setThemedContent(core) { QueuePanel() }
        core.start()
        compose.waitUntil(5_000) { core.client.started.value }
        val queue = core.client.queue.value
        val current = queue.current!!.track.title
        // No tabs: saved / recent queues and the undo history live in Library's "Recent queues".
        assertTrue(compose.onAllNodes(hasTestTagPrefix("queue.tab.")).fetchSemanticsNodes().isEmpty())
        compose.onNodeWithText("Now playing").assertIsDisplayed()
        compose.onNodeWithText("Playing next").assertIsDisplayed()
        compose.onNodeWithTag("queue.list").performScrollToNode(hasText("Continue playing"))
        compose.onNodeWithText("Continue playing").assertIsDisplayed()
        compose.onNodeWithText("From " + queue.contextLabel).assertIsDisplayed()
        // Tap the first upcoming context item: it becomes current and the old one goes to history.
        val target = queue.upcoming.first()
        compose.onNodeWithTag("queue.list").performScrollToNode(hasText(target.track.title))
        compose.onAllNodesWithText(target.track.title).onFirst().performClick()
        compose.waitUntil(5_000) { core.client.nowPlaying.value?.track?.id == target.track.id }
        assertNotEquals(current, core.client.nowPlaying.value!!.track.title)
        assertEquals(target.track.id, core.client.queue.value.current!!.track.id)
    }

    @Test
    fun historySitsAboveNowPlayingAndCanBeReplayedOrCleared() {
        val core = TestCore()
        compose.setThemedContent(core) { QueuePanel() }
        core.start()
        compose.waitUntil(5_000) { core.client.started.value }
        repeat(3) { core.client.dispatch(Command.Next) }
        compose.waitUntil(5_000) { core.client.queue.value.history.size == 3 }
        compose.waitForIdle()
        // The list follows the current track: "Now playing" at the top, history scrolled away above.
        compose.onNodeWithText("Now playing").assertIsDisplayed()
        compose.onNodeWithText("History").assertIsNotDisplayed()
        val played = core.client.queue.value.history.first()
        compose.onNodeWithTag("queue.list").performScrollToNode(hasText("History"))
        compose.onNodeWithText("History").assertIsDisplayed()
        // Oldest first, directly under the header; tapping plays it again.
        compose.onAllNodesWithText(played.track.title).onFirst().assertIsDisplayed().performClick()
        compose.waitUntil(5_000) { core.client.queue.value.current?.item?.key == played.item.key }
        // Build history again and clear it with the header's "Clear".
        repeat(2) { core.client.dispatch(Command.Next) }
        compose.waitUntil(5_000) { core.client.queue.value.history.size >= 2 }
        compose.onNodeWithTag("queue.list").performScrollToNode(hasText("History"))
        compose.onNodeWithContentDescription("Clear history").performClick()
        compose.waitUntil(5_000) { core.client.queue.value.history.isEmpty() }
        compose.onNodeWithText("History").assertDoesNotExist()
    }

    @Test
    fun playingNextHasItsOwnClear() {
        val core = TestCore()
        compose.setThemedContent(core) { QueuePanel() }
        core.start()
        compose.waitUntil(5_000) { core.client.started.value }
        assertTrue(core.client.queue.value.playingNext.isNotEmpty())
        compose.onNodeWithContentDescription("Clear playing next").performClick()
        compose.waitUntil(5_000) { core.client.queue.value.playingNext.isEmpty() }
        compose.onNodeWithText("Playing next").assertDoesNotExist()
    }

    @Test
    fun headerTogglesShuffleRepeatAndAutoplay() {
        val core = TestCore()
        compose.setThemedContent(core) { QueuePanel() }
        core.start()
        compose.waitUntil(5_000) { core.client.started.value }
        val q0 = core.client.queue.value
        compose.onNodeWithTag("queue.shuffle").assertIsDisplayed().assertContentDescriptionEquals("Shuffle").performClick()
        compose.waitUntil(5_000) { core.client.queue.value.shuffle != q0.shuffle }
        compose.onNodeWithTag("queue.shuffle").assertIsToggleable().apply { if (core.client.queue.value.shuffle) assertIsOn() else assertIsOff() }
        // Repeat cycles off -> all -> one -> off, and says which.
        assertEquals(RepeatMode.Off, q0.repeat)
        compose.onNodeWithTag("queue.repeat").assertIsOff().performClick()
        compose.waitUntil(5_000) { core.client.queue.value.repeat == RepeatMode.All }
        compose.onNodeWithTag("queue.repeat").assertIsOn()
        assertEquals("Repeat all", compose.onNodeWithTag("queue.repeat").fetchSemanticsNode().config[SemanticsProperties.StateDescription])
        compose.onNodeWithTag("queue.repeat").performClick()
        compose.waitUntil(5_000) { core.client.queue.value.repeat == RepeatMode.One }
        assertEquals("Repeat one", compose.onNodeWithTag("queue.repeat").fetchSemanticsNode().config[SemanticsProperties.StateDescription])
        compose.onNodeWithTag("queue.repeat").performClick()
        compose.waitUntil(5_000) { core.client.queue.value.repeat == RepeatMode.Off }
        compose.onNodeWithTag("queue.infinite").assertContentDescriptionEquals("Autoplay").performClick()
        compose.waitUntil(5_000) { core.client.queue.value.autoplay != q0.autoplay }
        // Three pills of equal width, each a comfortable target.
        val widths = listOf("queue.shuffle", "queue.repeat", "queue.infinite").map { compose.onNodeWithTag(it).fetchSemanticsNode().size }
        assertEquals(1, widths.map { it.width }.distinct().size)
        widths.forEach { assertTrue(it.toString(), it.height >= with(compose.density) { 40.dp.roundToPx() }) }
    }

    @Test
    fun savedQueuesAndUndoHistoryMovedToTheirOwnScreen() {
        val core = TestCore()
        compose.setThemedContent(core) { SavedQueuesScreen(rememberNavController()) }
        core.start()
        compose.waitUntil(5_000) { core.client.started.value }
        compose.onNodeWithText("Pinned").assertIsDisplayed()
        compose.onNodeWithText("Recent").assertIsDisplayed()
        compose.onNodeWithContentDescription("Undo history").performClick()
        compose.onNodeWithText("Nothing to undo").assertIsDisplayed()
    }

    /**
     * The core re-keys a context item that moves (`ctx-<index>` follows the context list), so a
     * drag that sent a move per slot lost its item after the first one. The drag now reorders a
     * local copy and sends ONE move when the finger lifts.
     */
    @Test
    fun draggingTheHandleReordersTheQueueWithOneMove() {
        val core = TestCore()
        compose.setThemedContent(core) { QueuePanel() }
        core.start()
        compose.waitUntil(5_000) { core.client.started.value }
        compose.waitForIdle()
        val q0 = core.client.queue.value
        fun tracks() = core.client.queue.value.let { q -> (q.playingNext + q.upcoming).map { it.track.id } }
        val before = tracks()
        val entry = q0.upcoming.first()
        val from = before.indexOf(entry.track.id)
        assertTrue(q0.upcoming.size >= 4)
        val undoBefore = core.client.undo.value.history.size
        compose.onNodeWithTag("queue.list").performScrollToNode(hasTestTag("queue.row.${entry.item.key}"))
        compose.waitForIdle()
        val rowHeight = compose.onNodeWithTag("queue.row.${entry.item.key}").fetchSemanticsNode().size.height
        compose.onNodeWithTag("queue.handle.${entry.item.key}", useUnmergedTree = true).performTouchInput {
            down(center)
            // Past the touch slop, then about two and a half rows down in small steps, as a finger would.
            repeat(25) { moveBy(Offset(0f, rowHeight / 10f), delayMillis = 16) }
            up()
        }
        compose.waitForIdle()
        compose.waitUntil(5_000) { tracks().indexOf(entry.track.id) >= from + 2 }
        assertEquals("only the dragged item moved", before - entry.track.id, tracks() - entry.track.id)
        assertEquals("one drag is one move (one undo entry)", undoBefore + 1, core.client.undo.value.history.size)
        // The moved row is on screen under its new key, and the rows still follow the queue.
        compose.waitUntil(5_000) { core.client.queue.value.upcoming.any { it.track.id == entry.track.id } }
        val key = core.client.queue.value.upcoming.first { it.track.id == entry.track.id }.item.key
        compose.onNodeWithTag("queue.row.$key").assertIsDisplayed()
    }

    @Test
    fun scrollingTheListStillWorksWithHandles() {
        val core = TestCore()
        compose.setThemedContent(core) { QueuePanel() }
        core.start()
        compose.waitUntil(5_000) { core.client.started.value }
        // A queue longer than the screen.
        val ids = core.fake.library.tracks.take(30).map { it.id }
        core.client.dispatch(Commands.playLater(core.fake.library.serverId, ids))
        compose.waitUntil(5_000) { core.client.queue.value.playingNext.size >= 30 }
        compose.waitForIdle()
        val before = core.client.queue.value.let { q -> (q.playingNext + q.upcoming).map { it.item.key } }
        compose.onNodeWithText("Now playing").assertIsDisplayed()
        // A vertical swipe over the rows (not on a handle) scrolls and never reorders or swipes a row.
        compose.onNodeWithTag("queue.list").performTouchInput { swipeUp(startY = bottom - 10f, endY = top + 10f) }
        compose.waitForIdle()
        compose.onNodeWithText("Now playing").assertIsNotDisplayed()
        assertEquals(before, core.client.queue.value.let { q -> (q.playingNext + q.upcoming).map { it.item.key } })
    }

    private fun hasTestTagPrefix(prefix: String) = SemanticsMatcher("tag starts with $prefix") { it.config.getOrNull(SemanticsProperties.TestTag)?.startsWith(prefix) == true }

    private fun androidx.compose.ui.test.SemanticsNodeInteractionCollection.onFirst() = this[0]
}
