package app.hocket.ui

import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.test.hasTestTag
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.performScrollToNode
import androidx.compose.ui.test.performTouchInput
import org.junit.Assert.assertNotEquals
import org.junit.Test

/** Reordering the queue in the open player: every drag moves its item, the first one too. */
class QueueDragInPlayerTest : PlayerRulesTestBase() {

    @Test
    fun everyDragInThePlayersQueueMovesItsItem() {
        val core = TestCore()
        startExpanded(core)
        tap("player.mode.queue")
        compose.waitUntil(5_000) { exists("queue.list") }
        compose.mainClock.advanceTimeBy(1_500)
        compose.waitForIdle()
        fun tracks() = core.client.queue.value.let { q -> (q.playingNext + q.upcoming).map { it.track.id } }
        fun drag(index: Int, rows: Float) {
            val q = core.client.queue.value
            val entry = (q.playingNext + q.upcoming)[index]
            compose.onNodeWithTag("queue.list").performScrollToNode(hasTestTag("queue.row.${entry.item.key}"))
            compose.waitForIdle()
            val rowHeight = compose.onNodeWithTag("queue.row.${entry.item.key}").fetchSemanticsNode().size.height
            compose.onNodeWithTag("queue.handle.${entry.item.key}", useUnmergedTree = true).performTouchInput {
                down(center)
                repeat(25) { moveBy(Offset(0f, rowHeight * rows / 25f), delayMillis = 16) }
                up()
            }
            compose.waitForIdle()
        }
        val undo0 = core.client.undo.value.history.size
        val t0 = tracks()
        drag(1, 2.5f)
        compose.mainClock.advanceTimeBy(500)
        compose.waitForIdle()
        val t1 = tracks()
        assertNotEquals("the first drag moved", t0, t1)
        drag(4, -2.5f)
        compose.mainClock.advanceTimeBy(500)
        compose.waitForIdle()
        assertNotEquals("the second drag moved", t1, tracks())
    }
}
