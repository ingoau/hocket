package app.hocket.ui

import android.graphics.Bitmap
import androidx.activity.ComponentActivity
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.ui.graphics.asAndroidBitmap
import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.test.captureToImage
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.test.onAllNodesWithTag
import androidx.compose.ui.test.onFirst
import androidx.compose.ui.test.hasTestTag
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onRoot
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performScrollTo
import androidx.compose.ui.test.performScrollToNode
import androidx.compose.ui.test.performSemanticsAction
import androidx.test.ext.junit.runners.AndroidJUnit4
import app.hocket.ui.nav.AppRoot
import app.hocket.ui.nav.LocalFakeCoreBanner
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode
import java.io.File
import java.io.FileOutputStream

/**
 * Miuix-style page pushes: the new screen enters from the trailing edge while the old one stays
 * visible underneath. Also captures mid-transition frames (and a playlist push) when
 * `HOCKET_SCREENSHOT_DIR` is set, so the PR can show what the motion looks like.
 */
@RunWith(AndroidJUnit4::class)
@Config(sdk = [35], application = android.app.Application::class, qualifiers = "w411dp-h891dp-xxhdpi")
@GraphicsMode(GraphicsMode.Mode.NATIVE)
class PageTransitionTest {
    @get:Rule
    val compose = createAndroidComposeRule<ComponentActivity>()

    private val outDir: File? = (System.getProperty("hocket.screenshotDir") ?: System.getenv("HOCKET_SCREENSHOT_DIR"))
        ?.takeIf { it.isNotBlank() }?.let(::File)

    private fun exists(tag: String, unmerged: Boolean = false) =
        compose.onAllNodesWithTag(tag, useUnmergedTree = unmerged).fetchSemanticsNodes().isNotEmpty()

    @Test
    fun aPushEntersFromTheTrailingEdgeOverThePreviousPage() {
        val core = TestCore(startPlaying = false)
        compose.setThemedContent(core) { CompositionLocalProvider(LocalFakeCoreBanner provides false) { AppRoot(core.client) } }
        core.start()
        compose.waitUntil(5_000) { exists("home.screen") }
        compose.openSettingsFromAccount()
        compose.waitUntil(5_000) { exists("settings.categories") }
        compose.mainClock.autoAdvance = false
        compose.onNodeWithTag("settings.category.appearance").performScrollTo().performSemanticsAction(SemanticsActions.OnClick)
        compose.runOnUiThread { androidx.compose.runtime.snapshots.Snapshot.sendApplyNotifications() }
        // Mid-push: both pages are composed, the new one offset toward the trailing edge.
        compose.mainClock.advanceTimeBy(160)
        assertTrue("previous page still composed", exists("settings.categories"))
        assertTrue("new page is entering", exists("settings.screen"))
        val full = compose.onRoot().fetchSemanticsNode().boundsInRoot
        val entering = compose.onNodeWithTag("settings.screen").fetchSemanticsNode().boundsInRoot
        assertTrue("enters from the trailing edge (right in LTR): $entering in $full", entering.left > full.left + 8f)
        shot("page-push-settings-mid")
        compose.mainClock.autoAdvance = true
        compose.waitUntil(5_000) { exists("settings.screen") && !exists("settings.categories") }
        val settled = compose.onNodeWithTag("settings.screen").fetchSemanticsNode().boundsInRoot
        assertTrue("settles flush with the screen: $settled", kotlin.math.abs(settled.left - full.left) < 2f)
    }

    @Test
    fun openingAnAlbumKeepsTheCoverOnTheDetailPage() {
        val core = TestCore(startPlaying = false)
        compose.setThemedContent(core) { CompositionLocalProvider(LocalFakeCoreBanner provides false) { AppRoot(core.client) } }
        core.start()
        compose.waitUntil(5_000) { exists("home.screen") }
        compose.onNodeWithTag("navBar.library").performClick()
        compose.waitUntil(5_000) { exists("library.album") }
        compose.mainClock.autoAdvance = false
        compose.onAllNodesWithTag("library.album").onFirst().performSemanticsAction(SemanticsActions.OnClick)
        compose.runOnUiThread { androidx.compose.runtime.snapshots.Snapshot.sendApplyNotifications() }
        compose.mainClock.advanceTimeBy(200)
        assertTrue("library still composed under the album page", exists("library.album") || exists("library.grid"))
        assertTrue("album page is entering", exists("detail.header") || exists("detail.artwork"))
        shot("page-push-album-mid")
        compose.mainClock.autoAdvance = true
        compose.waitUntil(5_000) { exists("detail.header") }
        compose.mainClock.advanceTimeBy(600)
        compose.waitForIdle()
        shot("page-push-album-settled")
        assertTrue(exists("detail.artwork") || exists("detail.header"))
    }

    @Test
    fun poppingAnAlbumKeepsTheCoverOnBothPages() {
        val core = TestCore(startPlaying = false)
        compose.setThemedContent(core) { CompositionLocalProvider(LocalFakeCoreBanner provides false) { AppRoot(core.client) } }
        core.start()
        compose.waitUntil(5_000) { exists("home.screen") }
        compose.onNodeWithTag("navBar.library").performClick()
        compose.waitUntil(5_000) { exists("library.album") }
        compose.onAllNodesWithTag("library.album").onFirst().performSemanticsAction(SemanticsActions.OnClick)
        compose.waitUntil(5_000) { exists("detail.header") }
        compose.mainClock.advanceTimeBy(600)
        compose.waitForIdle()
        compose.mainClock.autoAdvance = false
        compose.activityRule.scenario.onActivity { it.onBackPressedDispatcher.onBackPressed() }
        compose.runOnUiThread { androidx.compose.runtime.snapshots.Snapshot.sendApplyNotifications() }
        compose.mainClock.advanceTimeBy(200)
        assertTrue("library is entering underneath", exists("library.album") || exists("library.grid"))
        assertTrue("album page is still leaving", exists("detail.header") || exists("detail.artwork"))
        shot("page-pop-album-mid")
        compose.mainClock.autoAdvance = true
        compose.waitUntil(5_000) { exists("library.album") }
    }

    @Test
    fun openingAlbumsFromTheTileKeepsTheFanOnTheWayIn() {
        val core = TestCore(startPlaying = false)
        compose.setThemedContent(core) { CompositionLocalProvider(LocalFakeCoreBanner provides false) { AppRoot(core.client) } }
        core.start()
        compose.waitUntil(5_000) { exists("home.screen") }
        compose.onNodeWithTag("navBar.library").performClick()
        compose.waitUntil(5_000) { exists("library.grid") }
        compose.onNodeWithTag("library.grid").performScrollToNode(hasTestTag("library.browse.albums"))
        compose.waitUntil(5_000) { exists("library.browse.albums.fan.0", unmerged = true) }
        compose.mainClock.autoAdvance = false
        compose.onNodeWithTag("library.browse.albums").performSemanticsAction(SemanticsActions.OnClick)
        compose.runOnUiThread { androidx.compose.runtime.snapshots.Snapshot.sendApplyNotifications() }
        compose.mainClock.advanceTimeBy(200)
        assertTrue("fan still composed on the library", exists("library.browse.albums.fan", unmerged = true) || exists("library.browse.albums.fan.0", unmerged = true))
        assertTrue("albums page is entering", exists("libraryList.albums") || exists("library.group.recent"))
        shot("page-push-albums-tile-mid")
        compose.mainClock.autoAdvance = true
        compose.waitUntil(5_000) { exists("libraryList.albums") || exists("library.group.recent") }
    }

    @Test
    fun switchingTabsLeavesTheIncomingPlaceOffsetWithTheTab() {
        val core = TestCore(startPlaying = false)
        compose.setThemedContent(core) { CompositionLocalProvider(LocalFakeCoreBanner provides false) { AppRoot(core.client) } }
        core.start()
        compose.waitUntil(5_000) { exists("home.screen") && exists("navBar.library") }
        compose.mainClock.autoAdvance = false
        compose.onNodeWithTag("navBar.library").performSemanticsAction(SemanticsActions.OnClick)
        compose.runOnUiThread { androidx.compose.runtime.snapshots.Snapshot.sendApplyNotifications() }
        compose.mainClock.advanceTimeBy(80)
        assertTrue("home still composed during the tab fade", exists("home.screen"))
        shot("page-tab-library-mid")
        compose.mainClock.autoAdvance = true
        compose.waitUntil(5_000) { exists("library.grid") || exists("library.album") }
    }

    private fun shot(name: String) {
        val dir = outDir ?: return
        dir.mkdirs()
        val bmp = compose.onRoot().captureToImage().asAndroidBitmap()
        FileOutputStream(File(dir, "$name.png")).use { bmp.compress(Bitmap.CompressFormat.PNG, 100, it) }
    }
}
