package app.hocket.ui

import android.graphics.Bitmap
import androidx.activity.ComponentActivity
import androidx.compose.ui.graphics.asAndroidBitmap
import androidx.compose.ui.test.captureToImage
import androidx.compose.ui.test.isRoot
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.test.onAllNodesWithTag
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performTextInput
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
 * Settings search on the fake core, to PNGs for review: a query's results, and the screen a result
 * opens with its setting highlighted. Not an assertion test: it only runs with an output directory,
 *
 *     HOCKET_SCREENSHOT_DIR=/tmp/shots ./gradlew :app:testDebugUnitTest --tests '*SettingsSearchScreenshotTest*' --rerun
 */
@RunWith(AndroidJUnit4::class)
@Config(sdk = [35], qualifiers = "w411dp-h891dp-notnight-xxhdpi")
@GraphicsMode(GraphicsMode.Mode.NATIVE)
class SettingsSearchScreenshotTest {
    @get:Rule
    val compose = createAndroidComposeRule<ComponentActivity>()

    private val outDir: File? = (System.getProperty("hocket.screenshotDir") ?: System.getenv("HOCKET_SCREENSHOT_DIR"))
        ?.takeIf { it.isNotBlank() }?.let(::File)

    @Test
    fun resultsAndJump() {
        val dir = outDir
        assumeTrue("set HOCKET_SCREENSHOT_DIR (or -Dhocket.screenshotDir) to write screenshots", dir != null)
        dir!!.mkdirs()
        val core = TestCore(startPlaying = false)
        compose.setThemedContent(core) { AppRoot(core.client) }
        core.start()
        compose.openSettingsFromAccount()
        compose.onNodeWithTag("settings.search").performTextInput("cache")
        repeat(2) { compose.mainClock.advanceTimeBy(600); compose.waitForIdle() }
        shot(dir, "android-settings-search")
        compose.onNodeWithTag("settings.hit.storage.clearCache").performClick()
        compose.waitUntil(5_000) { compose.onAllNodesWithTag("settings.screen").fetchSemanticsNodes().isNotEmpty() }
        // Mid-highlight: scrolled to the row and tinted, before it fades.
        compose.mainClock.autoAdvance = false
        repeat(8) { compose.mainClock.advanceTimeBy(100); compose.waitForIdle() }
        shot(dir, "android-settings-search-jump")
        compose.mainClock.autoAdvance = true
    }

    private fun shot(dir: File, name: String) {
        val bitmap = compose.onAllNodes(isRoot())[0].captureToImage().asAndroidBitmap().copy(Bitmap.Config.ARGB_8888, true)
        File(dir, "$name.png").outputStream().use { bitmap.compress(Bitmap.CompressFormat.PNG, 100, it) }
    }
}
