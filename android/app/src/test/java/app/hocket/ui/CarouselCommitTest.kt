package app.hocket.ui

import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.performTouchInput
import app.hocket.core.api.Command
import org.junit.Assert.assertEquals
import org.junit.Test

/** A committed skip swipe sends one skip, whatever happens while it settles. */
class CarouselCommitTest : PlayerRulesTestBase() {
    private fun keys(core: TestCore): List<String> {
        val q = core.client.queue.value
        return listOfNotNull(q.current?.item?.trackId) + q.playingNext.map { it.item.trackId } + q.upcoming.map { it.item.trackId }
    }

    /** A swipe towards next released [share] of the way (past half: a skip; the settle then runs on the test's clock). */
    private fun swipeToNext(share: Float = 0.8f) {
        val width = compose.onNodeWithTag("player.artwork").fetchSemanticsNode().size.width.toFloat()
        compose.onNodeWithTag("player.artwork").performTouchInput {
            down(center)
            moveBy(Offset(-viewConfiguration.touchSlop - 1f, 0f))
            repeat(6) { moveBy(Offset(-width * share / 6f, 0f)) }
            up()
        }
    }

    private fun settle() {
        compose.mainClock.autoAdvance = true
        compose.mainClock.advanceTimeBy(3_000)
        compose.waitForIdle()
    }

    /** [n] swipes, each started a few frames into the previous one's settle: n skips, never more. */
    private fun rapidSwipes(n: Int) {
        val core = TestCore()
        startExpanded(core)
        val order = keys(core)
        compose.mainClock.autoAdvance = false
        repeat(n) {
            swipeToNext()
            repeat(3) { compose.mainClock.advanceTimeByFrame() }
        }
        settle()
        assertEquals("$n skips", order[n], core.client.nowPlaying.value?.track?.id)
        // And it takes swipes again afterwards: one more, one more skip.
        compose.mainClock.autoAdvance = false
        swipeToNext()
        settle()
        assertEquals(order[n + 1], core.client.nowPlaying.value?.track?.id)
    }

    @Test
    fun twoSwipesWithinTheSettleSkipTwice() = rapidSwipes(2)

    @Test
    fun threeRapidSwipesSkipThrice() = rapidSwipes(3)

    @Test
    fun aSongChangeWhileTheSwipeSettlesIsTheSkip() {
        val core = TestCore()
        startExpanded(core)
        val order = keys(core)
        compose.mainClock.autoAdvance = false
        swipeToNext()
        compose.mainClock.advanceTimeByFrame()
        // Another device (or the song's end) moves on before the swipe has settled.
        core.client.dispatch(Command.Next)
        settle()
        assertEquals("one skip, not two", order[1], core.client.nowPlaying.value?.track?.id)
    }
}
