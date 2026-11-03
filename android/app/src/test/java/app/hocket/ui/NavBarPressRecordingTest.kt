package app.hocket.ui

import android.graphics.Bitmap
import androidx.activity.ComponentActivity
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.ui.graphics.asAndroidBitmap
import androidx.compose.ui.test.captureToImage
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.test.onAllNodesWithTag
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onRoot
import androidx.compose.ui.test.performTouchInput
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
 * A frame-by-frame recording (one frame per 16 ms) of the floating bar being tapped through its
 * places: each press dips the place, the release springs it back, and the new capsule bounces open.
 * For review by eye (stitch with ffmpeg); only runs with an output directory:
 *
 *     HOCKET_SCREENSHOT_DIR=/tmp/navbar ./gradlew :app:testDebugUnitTest --tests '*NavBarPressRecordingTest*' --rerun
 */
@RunWith(AndroidJUnit4::class)
@Config(sdk = [35], application = android.app.Application::class, qualifiers = "w411dp-h891dp-xxhdpi")
@GraphicsMode(GraphicsMode.Mode.NATIVE)
class NavBarPressRecordingTest {
    @get:Rule
    val compose = createAndroidComposeRule<ComponentActivity>()

    private val outDir: File? = (System.getProperty("hocket.screenshotDir") ?: System.getenv("HOCKET_SCREENSHOT_DIR"))
        ?.takeIf { it.isNotBlank() }?.let(::File)

    private fun exists(tag: String) = compose.onAllNodesWithTag(tag).fetchSemanticsNodes().isNotEmpty()

    private var n = 0

    @Test
    fun taps() {
        val dir = outDir
        assumeTrue("set HOCKET_SCREENSHOT_DIR to write recording frames", dir != null)
        val sub = File(dir!!, "navbar-taps").apply { mkdirs() }
        val core = TestCore(startPlaying = false)
        compose.setThemedContent(core) { CompositionLocalProvider(LocalFakeCoreBanner provides false) { AppRoot(core.client) } }
        core.start()
        compose.waitUntil(5_000) { exists("home.screen") }
        compose.waitUntil(5_000) { exists("home.item") }
        compose.mainClock.advanceTimeBy(1_500)
        compose.waitForIdle()

        compose.mainClock.autoAdvance = false
        frames(sub, 12)
        // A slow press, a quick tap, a reselect-free return home.
        tap(sub, "navBar.library", holdFrames = 14)
        tap(sub, "navBar.search", holdFrames = 5)
        tap(sub, "navBar.home", holdFrames = 9)
        compose.mainClock.autoAdvance = true
    }

    private fun tap(dir: File, tag: String, holdFrames: Int) {
        compose.onNodeWithTag(tag).performTouchInput { down(center) }
        frames(dir, holdFrames)
        compose.onNodeWithTag(tag).performTouchInput { up() }
        frames(dir, 50)
    }

    private fun frames(dir: File, count: Int) = repeat(count) {
        compose.mainClock.advanceTimeBy(16)
        val bmp = compose.onRoot().captureToImage().asAndroidBitmap()
        FileOutputStream(File(dir, "f" + (n++).toString().padStart(4, '0') + ".png")).use { bmp.compress(Bitmap.CompressFormat.PNG, 100, it) }
    }
}
