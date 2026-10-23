package app.hocket.ui

import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.performTouchInput
import app.hocket.core.api.Command
import app.hocket.core.api.QueueSource
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

/** A committed skip swipe sends one skip, whatever happens while it settles. */
class CarouselCommitTest : PlayerRulesTestBase() {
    private fun keys(core: TestCore): List<String> {
        val q = core.client.queue.value
        return listOfNotNull(q.current?.item?.trackId) + q.playingNext.map { it.item.trackId } + q.upcoming.map { it.item.trackId }
    }

    /** A swipe towards next released [share] of the way (past half: a skip; the settle then runs on the test's clock). */
    private fun swipeToNext(share: Float = 0.8f) = swipe(-share)

    private fun swipeToPrevious(share: Float = 0.8f) = swipe(share)

    private fun swipe(share: Float) {
        val width = compose.onNodeWithTag("player.artwork").fetchSemanticsNode().size.width.toFloat()
        val dir = kotlin.math.sign(share)
        compose.onNodeWithTag("player.artwork").performTouchInput {
            down(center)
            moveBy(Offset(dir * (viewConfiguration.touchSlop + 1f), 0f))
            repeat(6) { moveBy(Offset(width * share / 6f, 0f)) }
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

    @Test
    fun aSwipeBackGoesBackEvenFarIntoTheSong() {
        var clock = 1_000_000.0
        val core = TestCore(now = { clock })
        startExpanded(core)
        val first = core.client.nowPlaying.value?.item?.key
        core.client.dispatch(Command.Next)
        compose.waitUntil(5_000) { core.client.queue.value.history.isNotEmpty() }
        // Well past Previous's restart point.
        clock += 30_000.0
        core.client.requestSnapshot()
        compose.waitForIdle()
        assertTrue("far into the song", core.client.positionNow() >= 3_000)
        compose.mainClock.autoAdvance = false
        swipeToPrevious()
        settle()
        assertEquals("back a song, not restarted", first, core.client.nowPlaying.value?.item?.key)
    }

    /** No history, but the album's earlier tracks (it was started from its third): back a track still. */
    @Test
    fun aSwipeBackFromALaterTrackWithNoHistoryGoesBack() {
        var clock = 1_000_000.0
        val core = TestCore(now = { clock })
        startExpanded(core)
        val q = core.client.queue.value
        assertTrue(q.history.isEmpty())
        val index = (q.current?.item?.source as QueueSource.Context).data.index
        assertTrue(index > 0u)
        clock += 30_000.0
        core.client.requestSnapshot()
        compose.waitForIdle()
        compose.mainClock.autoAdvance = false
        swipeToPrevious()
        settle()
        assertEquals(index - 1u, (core.client.nowPlaying.value?.item?.source as QueueSource.Context).data.index)
    }

    @Test
    fun aSwipeBackWithNothingBeforeDoesNothing() {
        var clock = 1_000_000.0
        val core = TestCore(now = { clock })
        startExpanded(core)
        // Back to the album's first track (under 3 s in, so Previous goes back).
        while ((core.client.queue.value.current?.item?.source as QueueSource.Context).data.index > 0u) {
            core.client.dispatch(Command.Previous)
            compose.waitForIdle()
        }
        assertTrue(core.client.queue.value.history.isEmpty())
        clock += 30_000.0
        core.client.requestSnapshot()
        compose.waitForIdle()
        val key = core.client.nowPlaying.value?.item?.key
        compose.mainClock.autoAdvance = false
        swipeToPrevious()
        settle()
        assertEquals(key, core.client.nowPlaying.value?.item?.key)
        assertTrue("not restarted", core.client.positionNow() >= 30_000)
    }
}
