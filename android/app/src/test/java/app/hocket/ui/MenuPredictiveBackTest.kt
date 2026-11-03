package app.hocket.ui

import androidx.activity.BackEventCompat
import androidx.activity.ComponentActivity
import androidx.activity.findViewTreeOnBackPressedDispatcherOwner
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.test.onAllNodesWithContentDescription
import androidx.compose.ui.test.onAllNodesWithTag
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.performSemanticsAction
import androidx.test.ext.junit.runners.AndroidJUnit4
import app.hocket.core.ActionIds
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode

/**
 * Back on the song menu's sub-pages: a predictive back gesture let go returns to the menu, one
 * cancelled stays on the sub-page, a back key returns too, and only back on the menu itself closes
 * the sheet.
 */
@RunWith(AndroidJUnit4::class)
@Config(sdk = [35], qualifiers = "w411dp-h891dp-xxhdpi")
@GraphicsMode(GraphicsMode.Mode.NATIVE)
class MenuPredictiveBackTest {
    @get:Rule
    val compose = createAndroidComposeRule<ComponentActivity>()

    private fun has(tag: String) = compose.onAllNodesWithTag(tag).fetchSemanticsNodes().isNotEmpty()

    @Test
    fun backLeavesSubPagesBeforeClosingTheSheet() {
        openMenu()
        compose.onNodeWithTag("songMenu.share").performSemanticsAction(SemanticsActions.OnClick)
        settle()
        assertTrue(has("songMenu.share.navidrome"))

        gesture(0.6f, commit = false)
        assertTrue("a cancelled gesture stays on the sub-page", has("songMenu.share.navidrome"))

        gesture(1f, commit = true)
        assertEquals("a released gesture returns to the menu", false, has("songMenu.share.navidrome"))
        assertTrue(has("songMenu.header"))

        compose.onNodeWithTag("songMenu.more").performSemanticsAction(SemanticsActions.OnClick)
        settle()
        compose.onNodeWithTag("songMenu.action.${ActionIds.SNOOZE}").performSemanticsAction(SemanticsActions.OnClick)
        settle()
        assertTrue(has("songMenu.snooze.week"))
        compose.runOnUiThread { dispatcher().onBackPressed() }
        settle()
        assertEquals("a back key returns to the menu", false, has("songMenu.snooze.week"))
        assertTrue(has("songMenu.header"))

        gesture(1f, commit = true)
        assertEquals("back on the menu closes the sheet", false, has("songMenu.header"))
    }

    @Test
    fun quickBacksDuringTheSlide() {
        val title = openMenu()
        compose.mainClock.autoAdvance = false
        // Back while Share is still sliding in: back to the menu, not the sheet closed.
        compose.onNodeWithTag("songMenu.share").performSemanticsAction(SemanticsActions.OnClick)
        compose.mainClock.advanceTimeBy(100)
        compose.runOnUiThread { dispatcher().onBackPressed() }
        settle()
        assertTrue("back mid-slide keeps the sheet", has("songMenu.header"))
        assertEquals(false, has("songMenu.share.navidrome"))
        // Back let go, then a second back swipe while the menu is still sliding back: it closes the sheet.
        compose.onNodeWithTag("songMenu.share").performSemanticsAction(SemanticsActions.OnClick)
        settle()
        gesture(1f, commit = true, settleAfter = false)
        compose.mainClock.advanceTimeBy(80)
        gesture(1f, commit = true)
        assertEquals("a second back closes the sheet", false, has("songMenu.header"))
        assertTrue(title.isNotEmpty())
    }

    /** The playlist with its first song's menu open; the song's title. */
    private fun openMenu(): String {
        val core = TestCore(startPlaying = false)
        val lib = core.fake.library
        core.start()
        compose.waitUntil(5_000) { core.client.started.value }
        val title = lib.track(lib.playlistTracks.getValue("pl0").first())!!.title
        compose.setThemedContent(core) {
            CompositionLocalProvider(LocalDetailNavigator provides DetailNavigator({}, {}, {})) {
                app.hocket.ui.screens.detail.PlaylistDetailScreen(androidx.navigation.compose.rememberNavController(), "pl0")
            }
        }
        compose.waitUntil(5_000) { compose.onAllNodesWithContentDescription("More options for $title").fetchSemanticsNodes().isNotEmpty() }
        compose.onAllNodesWithContentDescription("More options for $title")[0].performSemanticsAction(SemanticsActions.OnClick)
        settle()
        return title
    }

    private fun settle() {
        compose.mainClock.advanceTimeBy(1_500)
        compose.waitForIdle()
    }

    @Suppress("UNCHECKED_CAST")
    private fun dispatcher(): androidx.activity.OnBackPressedDispatcher {
        val global = Class.forName("android.view.WindowManagerGlobal").getMethod("getInstance").invoke(null)
        val field = global.javaClass.getDeclaredField("mViews").apply { isAccessible = true }
        val sheet = (field.get(global) as List<android.view.View>).last { it !== compose.activity.window.decorView }
        return sheet.findViewTreeOnBackPressedDispatcherOwner()!!.onBackPressedDispatcher
    }

    /** A back swipe from the left edge to [to], then let go ([commit]) or cancelled. */
    private fun gesture(to: Float, commit: Boolean, settleAfter: Boolean = true) {
        val d = dispatcher()
        compose.runOnUiThread { d.dispatchOnBackStarted(BackEventCompat(0f, 1200f, 0f, BackEventCompat.EDGE_LEFT)) }
        for (i in 1..10) {
            val p = to * i / 10
            compose.runOnUiThread { d.dispatchOnBackProgressed(BackEventCompat(p * 400f, 1200f, p, BackEventCompat.EDGE_LEFT)) }
            compose.mainClock.advanceTimeBy(16)
        }
        compose.runOnUiThread { if (commit) d.onBackPressed() else d.dispatchOnBackCancelled() }
        if (settleAfter) settle() else compose.waitForIdle()
    }
}
