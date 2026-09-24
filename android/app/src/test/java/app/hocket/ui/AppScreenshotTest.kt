package app.hocket.ui

import android.graphics.Bitmap
import androidx.activity.ComponentActivity
import androidx.compose.ui.graphics.asAndroidBitmap
import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.test.captureToImage
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.test.onAllNodesWithTag
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.onRoot
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performSemanticsAction
import androidx.test.ext.junit.runners.AndroidJUnit4
import app.hocket.ui.nav.AppRoot
import org.junit.Assume.assumeTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode
import java.io.File

/**
 * Renders the main app screens (home, library, album, the mini player, the full player and its queue
 * and lyrics pages) to PNGs on the fake core, for visual review. Not an assertion test: it only runs
 * when an output directory is given, e.g.
 *
 *     HOCKET_SCREENSHOT_DIR=/tmp/shots ./gradlew :app:testDebugUnitTest --tests '*AppScreenshotTest*' --rerun
 */
@RunWith(AndroidJUnit4::class)
@Config(sdk = [35], qualifiers = "w411dp-h891dp-xxhdpi")
@GraphicsMode(GraphicsMode.Mode.NATIVE)
class AppScreenshotTest {
    @get:Rule
    val compose = createAndroidComposeRule<ComponentActivity>()

    private val outDir: File? = (System.getProperty("hocket.screenshotDir") ?: System.getenv("HOCKET_SCREENSHOT_DIR"))
        ?.takeIf { it.isNotBlank() }?.let(::File)

    @Test
    @Config(qualifiers = "+night")
    fun dark() = captureAll("dark")

    @Test
    @Config(qualifiers = "+notnight")
    fun light() = captureAll("light")

    private fun captureAll(theme: String) {
        val dir = outDir
        assumeTrue("set HOCKET_SCREENSHOT_DIR (or -Dhocket.screenshotDir) to write screenshots", dir != null)
        dir!!.mkdirs()
        val prefix = "android-app-$theme"
        val core = TestCore(startPlaying = true)
        compose.setThemedContent(core) { AppRoot(core.client) }
        core.start()
        compose.waitUntil(5_000) { core.client.started.value }
        awaitTag("miniPlayer")
        settle()
        shot(dir, "$prefix-1-home")

        step {
            compose.onNodeWithTag("navBar.library").performClick()
            compose.waitUntil(5_000) { runCatching { compose.onNodeWithText("Albums").assertExists() }.isSuccess }
            settle()
            shot(dir, "$prefix-2-library")
            compose.onAllNodesWithTag("library.album")[0].performClick()
            awaitTag("detail.header")
            settle()
            shot(dir, "$prefix-3-album")
        }

        step {
            compose.onNodeWithTag("miniPlayer.info").performSemanticsAction(SemanticsActions.OnClick)
            awaitTag("player.playPause")
            settle()
            shot(dir, "$prefix-4-player")
            compose.onNodeWithTag("player.mode.queue").performSemanticsAction(SemanticsActions.OnClick)
            settle()
            shot(dir, "$prefix-5-queue")
            compose.onNodeWithTag("player.mode.about").performSemanticsAction(SemanticsActions.OnClick)
            settle()
            shot(dir, "$prefix-7-about")
            // The lyrics page animates forever: step the clock rather than waiting for idle.
            compose.mainClock.autoAdvance = false
            compose.onNodeWithTag("player.mode.lyrics").performSemanticsAction(SemanticsActions.OnClick)
            repeat(60) { compose.mainClock.advanceTimeByFrame() }
            shot(dir, "$prefix-6-lyrics")
        }
    }

    private fun step(block: () -> Unit) {
        runCatching(block).onFailure { println("screenshot step failed: $it") }
    }

    private fun settle() {
        compose.mainClock.advanceTimeBy(1_500)
        compose.waitForIdle()
    }

    private fun awaitTag(tag: String) {
        compose.waitUntil(5_000) { runCatching { compose.onNodeWithTag(tag).assertExists() }.isSuccess }
        compose.waitForIdle()
    }

    private fun shot(dir: File, name: String) {
        val bitmap = compose.onRoot().captureToImage().asAndroidBitmap()
        val file = File(dir, "$name.png")
        file.outputStream().use { bitmap.compress(Bitmap.CompressFormat.PNG, 100, it) }
        println("screenshot: ${file.absolutePath}")
    }
}
