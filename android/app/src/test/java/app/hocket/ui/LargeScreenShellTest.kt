package app.hocket.ui

import androidx.activity.ComponentActivity
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.test.onAllNodesWithTag
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.performSemanticsAction
import androidx.test.ext.junit.runners.AndroidJUnit4
import app.hocket.core.Commands
import app.hocket.core.SettingKeys
import app.hocket.ui.nav.AppRoot
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode
import kotlin.math.abs

/**
 * The shell on large and landscape windows: the floating pill at the bottom start with the mini
 * player beside it at the end (one row), the rail with the mini player at the end when the floating
 * bar is off, the two-column full player in landscape, and on landscape tablets the side panel:
 * the page stays beside it, its Lyrics and Queue take over the main screen, and its button goes
 * full screen and back.
 */
@RunWith(AndroidJUnit4::class)
@Config(sdk = [35], application = android.app.Application::class)
@GraphicsMode(GraphicsMode.Mode.NATIVE)
class LargeScreenShellTest {
    @get:Rule
    val compose = createAndroidComposeRule<ComponentActivity>()

    private val core = TestCore()

    private fun exists(tag: String) = compose.onAllNodesWithTag(tag).fetchSemanticsNodes().isNotEmpty()
    private fun displayed(tag: String) = runCatching { compose.onNodeWithTag(tag).assertExists(); compose.onNodeWithTag(tag).fetchSemanticsNode().let { n -> n.boundsInRoot.width > 0f } }.getOrDefault(false)
    private fun bounds(tag: String): Rect = compose.onNodeWithTag(tag).fetchSemanticsNode().boundsInRoot
    private val dp get() = compose.activity.resources.displayMetrics.density
    private val widthPx get() = compose.activity.resources.displayMetrics.widthPixels.toFloat()

    private fun tap(tag: String) {
        compose.onNodeWithTag(tag).performSemanticsAction(SemanticsActions.OnClick)
        compose.runOnUiThread { androidx.compose.runtime.snapshots.Snapshot.sendApplyNotifications() }
        compose.waitForIdle()
    }

    private fun start(floatingBar: Boolean = true) {
        compose.setThemedContent(core) { AppRoot(core.client) }
        core.start()
        compose.waitUntil(5_000) { core.client.started.value }
        if (!floatingBar) core.client.dispatch(Commands.setSetting(SettingKeys.DISPLAY_FLOATING_NAV_BAR, "false"))
        compose.waitUntil(5_000) { exists("miniPlayer") }
        compose.waitForIdle()
    }

    private fun expand() {
        compose.onNodeWithTag("nowPlaying.sheet").performSemanticsAction(SemanticsActions.Expand)
        compose.waitUntil(5_000) { exists("player.playPause") }
        compose.mainClock.advanceTimeBy(1_000)
        compose.waitForIdle()
    }

    /** The pill at the bottom start, the mini player at the end, centred on one row. */
    private fun assertBarRow(miniWidthDp: ClosedFloatingPointRange<Float>) {
        val bar = bounds("navBar.home")
        val mini = bounds("miniPlayer")
        assertTrue("pill at the start ($bar)", bar.left < 40 * dp)
        assertTrue("mini player at the end ($mini of $widthPx)", abs(mini.right - (widthPx - 16 * dp)) < 2 * dp)
        assertTrue("mini player ${mini.width / dp} dp wide, expected $miniWidthDp", mini.width / dp in miniWidthDp.start - 1f..miniWidthDp.endInclusive + 1f)
        assertTrue("same row: pill centre ${bar.center.y}, mini centre ${mini.center.y}", abs(bar.center.y - mini.center.y) < 2 * dp)
        assertTrue("the pill ends before the mini player", bounds("navBar").right <= mini.left + 1f)
    }

    @Test
    @Config(qualifiers = "w891dp-h411dp")
    fun aLandscapePhoneHasOneRowAndATwoColumnPlayer() {
        start()
        assertBarRow(400f..400f)
        expand()
        val art = bounds("player.artwork")
        val title = bounds("player.title")
        val play = bounds("player.playPause")
        assertTrue("artwork beside the controls: art $art, title $title", art.right <= title.left + 1f)
        val heightPx = compose.activity.resources.displayMetrics.heightPixels
        assertTrue("the transport is on screen without scrolling ($play)", play.bottom <= heightPx)
        assertTrue("so is the action row (${bounds("player.mode.queue")})", bounds("player.mode.queue").bottom <= heightPx)
        assertTrue("and the whole artwork ($art)", art.bottom <= heightPx)
        assertFalse("no panel toggle on a phone", exists("player.fullScreen"))
        // The skip swipe's next cover waits off the page, not over the controls beside the artwork.
        // (Clipped away entirely, its bounds are empty.)
        if (exists("player.artwork.next")) bounds("player.artwork.next").let { assertTrue("next cover off screen ($it)", it.isEmpty || it.left >= widthPx - 1f) }
    }

    @Test
    @Config(qualifiers = "w800dp-h1280dp")
    fun aPortraitTabletHasOneRowAndTheFullScreenPlayer() {
        start()
        // 800 dp leaves the mini player what the widest pill and three margins leave, up to 400 dp.
        assertBarRow(320f..400f)
        expand()
        val art = bounds("player.artwork")
        val title = bounds("player.title")
        assertTrue("one column: the title under the artwork", title.top >= art.bottom - 1f)
        assertFalse("no panel in portrait", exists("player.fullScreen"))
    }

    @Test
    @Config(qualifiers = "w1280dp-h800dp")
    fun aLandscapeTabletOpensThePanelBesideThePage() {
        start()
        assertBarRow(420f..420f)
        expand()
        // The panel: at the end edge, as wide as the mini player was; the bar and the page stay.
        val sheet = bounds("nowPlaying.sheet")
        assertTrue("panel at the end ($sheet)", abs(sheet.right - (widthPx - 16 * dp)) < 2 * dp && abs(sheet.width / dp - 420f) < 2f)
        assertTrue("the bar stays beside the panel", displayed("navBar.home"))
        assertTrue("one column in the panel", bounds("player.title").top >= bounds("player.artwork").bottom - 1f)
        // Lyrics take over the main screen; the close button hands it back to the page. (The
        // lyrics animate for as long as they show: the clock is stepped by hand from here.)
        compose.mainClock.autoAdvance = false
        tap("player.mode.lyrics")
        compose.mainClock.advanceTimeBy(1_000)
        assertTrue("the lyrics take over the main screen", exists("panelPage.lyrics"))
        assertTrue("the lyrics are beside the panel", bounds("panelPage").right <= sheet.left + 1f)
        tap("panelPage.close")
        compose.mainClock.advanceTimeBy(1_000)
        assertFalse("back to the page", exists("panelPage"))
        // Full screen: the two-column player over everything; then back to the panel.
        tap("player.fullScreen")
        compose.mainClock.advanceTimeBy(1_000)
        compose.waitForIdle()
        val full = bounds("nowPlaying.sheet")
        assertTrue("full screen ($full)", full.left < 1f && abs(full.width - widthPx) < 2f)
        assertTrue("two columns", bounds("player.artwork").right <= bounds("player.title").left + 1f)
        tap("player.fullScreen")
        compose.mainClock.advanceTimeBy(1_000)
        compose.waitForIdle()
        assertTrue("back to the panel", abs(bounds("nowPlaying.sheet").width / dp - 420f) < 2f)
    }

    @Test
    @Config(qualifiers = "w1280dp-h800dp")
    fun withTheFloatingBarOffTheRailHasTheMiniPlayerAtTheEnd() {
        start(floatingBar = false)
        compose.waitUntil(5_000) { exists("navRail.home") }
        assertFalse("no bottom bar beside the rail", exists("navBar"))
        val mini = bounds("miniPlayer")
        assertTrue("mini player at the end ($mini)", abs(mini.right - (widthPx - 16 * dp)) < 2 * dp && abs(mini.width / dp - 420f) < 2f)
        // The rail's menu shows the labels.
        val before = bounds("navRail.home").width
        tap("navRail.menu")
        compose.mainClock.advanceTimeBy(1_000)
        compose.waitForIdle()
        assertTrue("the rail opens wider (${bounds("navRail.home").width} vs $before)", bounds("navRail.home").width > before)
    }
}
