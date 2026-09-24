package app.hocket.ui

import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.assertIsSelected
import androidx.compose.ui.test.onAllNodesWithTag
import androidx.compose.ui.test.performSemanticsAction
import androidx.compose.ui.test.getBoundsInRoot
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.test.ext.junit.runners.AndroidJUnit4
import app.hocket.ui.nav.AppRoot
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode

/** The mini player floats just above the navigation bar on phones; neither hides the other. */
@RunWith(AndroidJUnit4::class)
@Config(sdk = [35], application = android.app.Application::class, qualifiers = "w411dp-h891dp")
@GraphicsMode(GraphicsMode.Mode.NATIVE)
class NavBarLayoutTest {
    @get:Rule
    val compose = createComposeRule()

    @Test
    fun miniPlayerSitsAboveTheNavBarWithoutOverlap() {
        val core = TestCore()
        compose.setThemedContent(core) { AppRoot(core.client) }
        core.start()
        compose.waitUntil(5_000) { core.client.started.value }
        compose.waitForIdle()
        compose.onNodeWithTag("navBar").assertIsDisplayed()
        compose.onNodeWithTag("miniPlayer").assertIsDisplayed()
        val mini = compose.onNodeWithTag("miniPlayer").getBoundsInRoot()
        val bar = compose.onNodeWithTag("navBar").getBoundsInRoot()
        // The mini player floats a small gap (8 dp) above the nav bar, never below or behind it.
        assertTrue("mini bottom ${mini.bottom} vs nav top ${bar.top}", mini.bottom <= bar.top + 1.dpTolerance())
        assertTrue("mini bar should float just above the nav bar", bar.top - mini.bottom < 10.dpTolerance())
        // The nav bar stays tappable while something is playing.
        compose.onNodeWithTag("navBar.library").performClick()
        compose.waitForIdle()
        compose.onNodeWithText("Albums").assertIsDisplayed() // the Library screen's first tab
        compose.onNodeWithTag("miniPlayer").assertIsDisplayed()
    }

    @Test
    fun navBarSlidesAwayWhenTheSheetIsExpanded() {
        val core = TestCore()
        compose.setThemedContent(core) { AppRoot(core.client) }
        core.start()
        compose.waitUntil(5_000) { core.client.started.value }
        compose.onNodeWithTag("miniPlayer").performClick()
        compose.waitUntil(5_000) { runCatching { compose.onNodeWithTag("player.playPause").assertIsDisplayed() }.isSuccess }
        val bar = compose.onNodeWithTag("navBar").getBoundsInRoot()
        val root = compose.onNodeWithTag("nowPlaying.sheet").getBoundsInRoot()
        assertTrue("nav bar should be off-screen when expanded: top ${bar.top} >= sheet bottom ${root.bottom}", bar.top >= root.bottom - 1.dpTolerance())
    }

    @Test
    fun navBarHeightIsKnownBeforeItIsMeasured() {
        // The shell lays out content and the sheet's anchor from the bar's fixed height (64 dp plus
        // the system navigation-bar inset, none here) on the first frame; measuring must agree, or
        // the layout jumps once at startup.
        val core = TestCore()
        compose.setThemedContent(core) { AppRoot(core.client) }
        core.start()
        compose.waitUntil(5_000) { core.client.started.value }
        compose.waitForIdle()
        val bar = compose.onNodeWithTag("navBar").getBoundsInRoot()
        val height = (bar.bottom - bar.top).value
        assertTrue("nav bar height $height should be 64dp", kotlin.math.abs(height - 64f) < 1f)
    }

    @Test
    fun retappingTheSelectedPlacePopsBackToItsRoot() {
        val core = TestCore()
        compose.setThemedContent(core) { AppRoot(core.client) }
        core.start()
        compose.waitUntil(5_000) { core.client.started.value }
        compose.onNodeWithTag("navBar.library").performClick()
        compose.waitUntil(5_000) { compose.onAllNodesWithTag("library.album").fetchSemanticsNodes().isNotEmpty() }
        compose.onAllNodesWithTag("library.album")[0].performSemanticsAction(SemanticsActions.OnClick)
        compose.waitUntil(5_000) { compose.onAllNodesWithTag("detail.header").fetchSemanticsNodes().isNotEmpty() }
        compose.onNodeWithTag("navBar.library").assertIsSelected()
        compose.onNodeWithTag("navBar.library").performClick()
        compose.waitUntil(5_000) { compose.onAllNodesWithTag("detail.header").fetchSemanticsNodes().isEmpty() }
        compose.onAllNodesWithTag("library.album")[0].assertIsDisplayed()
        compose.onNodeWithTag("navBar.library").assertIsSelected()
    }

    private fun Int.dpTolerance(): androidx.compose.ui.unit.Dp = androidx.compose.ui.unit.Dp(this.toFloat())
}
