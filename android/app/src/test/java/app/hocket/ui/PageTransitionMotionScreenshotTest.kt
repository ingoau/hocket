package app.hocket.ui

import android.graphics.Bitmap
import androidx.activity.BackEventCompat
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
import org.junit.Assume.assumeTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode
import java.io.File
import java.io.FileOutputStream

/**
 * Frame sequences of the Miuix page motion (push, pop, predictive back from both edges, tab
 * switch, album-art shared element) for review by eye. Only runs with an output directory:
 *
 *     HOCKET_SCREENSHOT_DIR=/tmp/shots ./gradlew :app:testDebugUnitTest --tests '*PageTransitionMotionScreenshotTest*' --rerun
 */
@RunWith(AndroidJUnit4::class)
@Config(sdk = [35], application = android.app.Application::class, qualifiers = "w411dp-h891dp-xxhdpi")
@GraphicsMode(GraphicsMode.Mode.NATIVE)
class PageTransitionMotionScreenshotTest {
    @get:Rule
    val compose = createAndroidComposeRule<ComponentActivity>()

    private val outDir: File? = (System.getProperty("hocket.screenshotDir") ?: System.getenv("HOCKET_SCREENSHOT_DIR"))
        ?.takeIf { it.isNotBlank() }?.let(::File)

    private fun exists(tag: String, unmerged: Boolean = false) =
        compose.onAllNodesWithTag(tag, useUnmergedTree = unmerged).fetchSemanticsNodes().isNotEmpty()

    @Test
    fun sequences() {
        val dir = outDir
        assumeTrue("set HOCKET_SCREENSHOT_DIR to write motion frames", dir != null)
        dir!!.mkdirs()
        val core = TestCore(startPlaying = false)
        compose.setThemedContent(core) { CompositionLocalProvider(LocalFakeCoreBanner provides false) { AppRoot(core.client) } }
        core.start()
        compose.waitUntil(5_000) { exists("home.screen") }

        capturePush(dir, "tab-home-to-library") {
            compose.onNodeWithTag("navBar.library").performSemanticsAction(SemanticsActions.OnClick)
        }
        compose.waitUntil(5_000) { exists("library.album") }
        compose.onNodeWithTag("library.grid").performScrollToNode(hasTestTag("library.browse.albums"))
        compose.waitUntil(5_000) { exists("library.browse.albums.fan.0", unmerged = true) }

        capturePush(dir, "albums-from-tile") {
            compose.onNodeWithTag("library.browse.albums").performSemanticsAction(SemanticsActions.OnClick)
        }
        compose.waitUntil(5_000) { exists("libraryList.albums") || exists("library.group.recent") }

        capturePush(dir, "albums-tile-pop") {
            compose.activityRule.scenario.onActivity { it.onBackPressedDispatcher.onBackPressed() }
        }
        compose.waitUntil(5_000) { exists("library.browse.albums") }
        compose.onNodeWithTag("library.grid").performScrollToNode(hasTestTag("library.album"))
        compose.waitUntil(5_000) { exists("library.album") }

        capturePush(dir, "album-from-library") {
            compose.onAllNodesWithTag("library.album").onFirst().performSemanticsAction(SemanticsActions.OnClick)
        }
        compose.waitUntil(5_000) { exists("detail.header") }

        capturePush(dir, "album-pop") {
            compose.activityRule.scenario.onActivity { it.onBackPressedDispatcher.onBackPressed() }
        }
        compose.waitUntil(5_000) { exists("library.album") }

        compose.onNodeWithTag("navBar.home").performClick()
        compose.waitUntil(5_000) { exists("home.screen") }
        compose.openSettingsFromAccount()
        compose.waitUntil(5_000) { exists("settings.categories") }
        compose.onNodeWithTag("settings.category.appearance").performScrollTo().performSemanticsAction(SemanticsActions.OnClick)
        compose.waitUntil(5_000) { exists("settings.screen") }

        capturePredictive(dir, "predictive-back-from-left", BackEventCompat.EDGE_LEFT)
        compose.waitUntil(5_000) { exists("settings.categories") }

        compose.onNodeWithTag("settings.category.appearance").performScrollTo().performSemanticsAction(SemanticsActions.OnClick)
        compose.waitUntil(5_000) { exists("settings.screen") }
        capturePredictive(dir, "predictive-back-from-right", BackEventCompat.EDGE_RIGHT)
    }

    private fun capturePush(dir: File, name: String, start: () -> Unit) {
        compose.mainClock.autoAdvance = false
        start()
        compose.runOnUiThread { androidx.compose.runtime.snapshots.Snapshot.sendApplyNotifications() }
        val frames = 8
        val step = 70L
        repeat(frames) { i ->
            if (i > 0) compose.mainClock.advanceTimeBy(step)
            shot(dir, "$name-${i.toString().padStart(2, '0')}")
        }
        compose.mainClock.autoAdvance = true
        compose.mainClock.advanceTimeBy(800)
        compose.waitForIdle()
        shot(dir, "$name-settled")
    }

    private fun capturePredictive(dir: File, name: String, edge: Int) {
        fun event(progress: Float) = BackEventCompat(touchX = 10f + progress * 400f, touchY = 800f, progress = progress, swipeEdge = edge)
        compose.activityRule.scenario.onActivity { it.onBackPressedDispatcher.dispatchOnBackStarted(event(0f)) }
        compose.waitForIdle()
        val steps = listOf(0.08f, 0.16f, 0.28f, 0.42f, 0.58f, 0.74f, 0.9f)
        steps.forEachIndexed { i, p ->
            compose.activityRule.scenario.onActivity { it.onBackPressedDispatcher.dispatchOnBackProgressed(event(p)) }
            compose.waitForIdle()
            shot(dir, "$name-${i.toString().padStart(2, '0')}")
        }
        compose.activityRule.scenario.onActivity { it.onBackPressedDispatcher.onBackPressed() }
        compose.waitForIdle()
        shot(dir, "$name-settled")
    }

    private fun shot(dir: File, name: String) {
        val bmp = compose.onRoot().captureToImage().asAndroidBitmap()
        FileOutputStream(File(dir, "$name.png")).use { bmp.compress(Bitmap.CompressFormat.PNG, 100, it) }
    }
}
