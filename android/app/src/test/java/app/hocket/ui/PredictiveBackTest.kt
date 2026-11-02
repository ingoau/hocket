package app.hocket.ui

import androidx.activity.BackEventCompat
import androidx.activity.ComponentActivity
import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.test.onAllNodesWithTag
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onRoot
import androidx.compose.ui.test.performScrollTo
import androidx.compose.ui.test.performSemanticsAction
import androidx.test.ext.junit.runners.AndroidJUnit4
import app.hocket.ui.nav.AppRoot
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode

/**
 * Predictive back between screens: the gesture reveals the previous screen while the current one
 * slides toward the trailing edge (right in LTR), the same direction from either swipe edge;
 * cancelling restores it, releasing finishes the pop.
 */
@RunWith(AndroidJUnit4::class)
@Config(sdk = [35], application = android.app.Application::class, qualifiers = "w411dp-h891dp")
@GraphicsMode(GraphicsMode.Mode.NATIVE)
class PredictiveBackTest {
    @get:Rule
    val compose = createAndroidComposeRule<ComponentActivity>()

    private fun exists(tag: String) = compose.onAllNodesWithTag(tag).fetchSemanticsNodes().isNotEmpty()

    private fun back(block: (androidx.activity.OnBackPressedDispatcher) -> Unit) {
        compose.activityRule.scenario.onActivity { block(it.onBackPressedDispatcher) }
        compose.waitForIdle()
    }

    private fun event(progress: Float, edge: Int = BackEventCompat.EDGE_LEFT) =
        BackEventCompat(touchX = 10f + progress * 400f, touchY = 800f, progress = progress, swipeEdge = edge)

    private fun openSubPage() {
        val core = TestCore(startPlaying = false)
        compose.setThemedContent(core) { AppRoot(core.client) }
        core.start()
        compose.openSettingsFromAccount()
        compose.onNodeWithTag("settings.category.appearance").performScrollTo().performSemanticsAction(SemanticsActions.OnClick)
        compose.waitUntil(5_000) { exists("settings.screen") && !exists("settings.categories") }
        compose.waitForIdle()
    }

    @Test
    fun theGestureSlidesThePageTowardTheTrailingEdgeAndCancelRestoresIt() {
        openSubPage()
        val full = compose.onRoot().fetchSemanticsNode().boundsInRoot
        back { it.dispatchOnBackStarted(event(0f)) }
        back { it.dispatchOnBackProgressed(event(0.3f)) }
        // Both screens are composed: the settings list is revealed behind the sub-page.
        assertTrue(exists("settings.categories"))
        val page = compose.onNodeWithTag("settings.screen").fetchSemanticsNode().boundsInRoot
        assertTrue("the page slides toward the trailing edge (right in LTR): $page in $full", page.left > full.left + 1f)
        back { it.dispatchOnBackCancelled() }
        compose.waitUntil(5_000) { !exists("settings.categories") }
        val restored = compose.onNodeWithTag("settings.screen").fetchSemanticsNode().boundsInRoot
        assertEquals(full.width, restored.width, 1f)
        assertEquals(full.left, restored.left, 1f)
    }

    @Test
    fun fromTheRightEdgeThePageStillMovesRightAndReleasingPops() {
        openSubPage()
        val full = compose.onRoot().fetchSemanticsNode().boundsInRoot
        back { it.dispatchOnBackStarted(event(0f, BackEventCompat.EDGE_RIGHT)) }
        back { it.dispatchOnBackProgressed(event(0.5f, BackEventCompat.EDGE_RIGHT)) }
        val page = compose.onNodeWithTag("settings.screen").fetchSemanticsNode().boundsInRoot
        assertTrue("same direction as a left-edge swipe: toward the trailing edge: $page", page.left > full.left + 1f)
        back { it.onBackPressed() }
        compose.waitUntil(5_000) { exists("settings.categories") && !exists("settings.screen") }
    }

    private fun startPushToSubPage() {
        val core = TestCore(startPlaying = false)
        compose.setThemedContent(core) { AppRoot(core.client) }
        core.start()
        compose.openSettingsFromAccount()
        compose.mainClock.autoAdvance = false
        compose.onNodeWithTag("settings.category.appearance").performScrollTo().performSemanticsAction(SemanticsActions.OnClick)
        compose.mainClock.advanceTimeBy(100)
    }

    private fun pageLeft() = compose.onNodeWithTag("settings.screen").fetchSemanticsNode().boundsInRoot.left

    /** Swipes back while the push is still playing; returns the page's left edge after each step. */
    private fun swipeDuringPush(): List<Float> {
        val lefts = mutableListOf(pageLeft())
        compose.activityRule.scenario.onActivity { it.onBackPressedDispatcher.dispatchOnBackStarted(event(0f)) }
        for (p in listOf(0.2f, 0.4f, 0.6f)) {
            compose.activityRule.scenario.onActivity { it.onBackPressedDispatcher.dispatchOnBackProgressed(event(p)) }
            compose.mainClock.advanceTimeBy(32)
            lefts += pageLeft()
        }
        return lefts
    }

    // R-20261102-yyrs: a gesture that started mid-push seeked the NavHost toward the previous page
    // while the push kept running underneath, so both pages blended under the scrim and snapped to
    // the end of the push instead of following the finger.
    @Test
    fun aGestureDuringAPushDoesNotScrubItAndCancellingLeavesThePushToFinish() {
        startPushToSubPage()
        val full = compose.onRoot().fetchSemanticsNode().boundsInRoot
        val lefts = swipeDuringPush()
        assertTrue("the push keeps sliding in smoothly, not seeked by the finger: $lefts", lefts.zipWithNext().all { (a, b) -> b < a } && lefts.last() > full.left)
        compose.activityRule.scenario.onActivity { it.onBackPressedDispatcher.dispatchOnBackCancelled() }
        compose.mainClock.autoAdvance = true
        compose.waitUntil(5_000) { !exists("settings.categories") }
        assertEquals(full.left, pageLeft(), 1f)
        // Settled: the next gesture is predictive again.
        back { it.dispatchOnBackStarted(event(0f)) }
        back { it.dispatchOnBackProgressed(event(0.3f)) }
        assertTrue(exists("settings.categories"))
        assertTrue(pageLeft() > full.left + 1f)
    }

    @Test
    fun releasingBackDuringAPushReversesItToThePreviousPage() {
        startPushToSubPage()
        val lefts = swipeDuringPush()
        compose.activityRule.scenario.onActivity { it.onBackPressedDispatcher.onBackPressed() }
        compose.mainClock.advanceTimeBy(100)
        assertTrue("the page slides back out toward the trailing edge: ${pageLeft()} after $lefts", pageLeft() > lefts.last())
        compose.mainClock.autoAdvance = true
        compose.waitUntil(5_000) { exists("settings.categories") && !exists("settings.screen") }
    }
}
