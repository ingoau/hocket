package app.hocket.ui

import android.graphics.Bitmap
import android.graphics.Canvas
import android.graphics.Color
import android.graphics.Paint
import android.view.Gravity
import android.view.WindowManager
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.asAndroidBitmap
import androidx.compose.ui.platform.ViewRootForTest
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.test.SemanticsMatcher
import androidx.compose.ui.test.captureToImage
import androidx.compose.ui.test.isDialog
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onRoot
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performScrollTo
import androidx.compose.ui.test.performTouchInput
import androidx.test.ext.junit.runners.AndroidJUnit4
import app.hocket.ui.nav.AppRoot
import app.hocket.ui.screens.settings.SettingsCategory
import org.junit.Assume.assumeTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode
import java.io.File

/**
 * Renders the settings UI (category list, every category screen, the sign-out confirmation) to PNGs
 * with Robolectric native graphics, in the light and the dark theme, for visual review. Not an
 * assertion test: it only runs when an output directory is given, so ordinary test runs write
 * nothing, e.g.
 *
 *     HOCKET_SCREENSHOT_DIR=/tmp/shots ./gradlew :app:testDebugUnitTest --tests '*SettingsScreenshotTest*' --rerun
 *
 * (`-Dhocket.screenshotDir=` works too when the property reaches the test JVM.) Screens taller than
 * the phone are captured as several overlapping pages (`<screen>.png`, `<screen>-2.png`, ...),
 * scrolled with a real drag so the large top app bar collapses as it does on a device.
 */
@RunWith(AndroidJUnit4::class)
@Config(sdk = [35], application = android.app.Application::class, qualifiers = "w411dp-h891dp-xxhdpi")
@GraphicsMode(GraphicsMode.Mode.NATIVE)
class SettingsScreenshotTest {
    @get:Rule
    val compose = createComposeRule()

    private val outDir: File? = (System.getProperty("hocket.screenshotDir") ?: System.getenv("HOCKET_SCREENSHOT_DIR"))
        ?.takeIf { it.isNotBlank() }?.let(::File)

    @Test
    @Config(qualifiers = "+notnight")
    fun light() = captureAll("light")

    @Test
    @Config(qualifiers = "+night")
    fun dark() = captureAll("dark")

    private fun captureAll(theme: String) {
        val dir = outDir
        assumeTrue("set HOCKET_SCREENSHOT_DIR (or -Dhocket.screenshotDir) to write settings screenshots", dir != null)
        dir!!.mkdirs()
        val prefix = "android-settings-$theme"

        val core = TestCore(startPlaying = false)
        compose.setThemedContent(core) { AppRoot(core.client) }
        core.start()
        awaitTag("navBar.settings")
        compose.onNodeWithTag("navBar.settings").performClick()
        awaitTag("settings.categories")
        capturePages(File(dir, "$prefix-categories"))
        scrollToTop()

        for (category in SettingsCategory.entries) {
            compose.onNodeWithTag("settings.category.${category.id}").performScrollTo().performClick()
            awaitTag("settings.screen")
            capturePages(File(dir, "$prefix-${category.id}"))
            compose.onNodeWithTag("settings.back").performClick()
            awaitTag("settings.categories")
            scrollToTop()
        }

        // Sign-out confirmation, over the Account screen.
        compose.onNodeWithTag("settings.category.account").performScrollTo().performClick()
        awaitTag("setting.server.remove")
        compose.onNodeWithTag("setting.server.remove").performScrollTo().performClick()
        compose.waitForIdle()
        awaitTag("confirm.ok")
        captureDialog(File(dir, "$prefix-signout-dialog"))
    }

    private fun awaitTag(tag: String) {
        compose.waitUntil(5_000) { runCatching { compose.onNodeWithTag(tag).assertExists() }.isSuccess }
        compose.waitForIdle()
    }

    private val verticallyScrollable = SemanticsMatcher("vertically scrollable") { node ->
        node.config.getOrElseNullable(SemanticsProperties.VerticalScrollAxisRange) { null }?.let { it.maxValue() > 0f } == true
    }

    /** The outermost vertically scrollable node on screen and its (value, max). */
    private fun scrollState(): Pair<Float, Float>? {
        val nodes = compose.onAllNodes(verticallyScrollable, useUnmergedTree = true).fetchSemanticsNodes()
        val node = nodes.maxByOrNull { it.boundsInRoot.height } ?: return null
        val range = node.config[SemanticsProperties.VerticalScrollAxisRange]
        return range.value() to range.maxValue()
    }

    /** Captures the screen, then drags up a page at a time (with overlap) until the end, capturing each page. */
    private fun capturePages(base: File) {
        save(compose.onRoot().captureToImage().asAndroidBitmap(), File(base.path + ".png"))
        var page = 2
        while (page <= 8) {
            val (before, max) = scrollState() ?: return
            if (before >= max - 0.5f) return
            drag(up = true)
            val after = scrollState()?.first ?: return
            if (after <= before + 0.5f) return
            save(compose.onRoot().captureToImage().asAndroidBitmap(), File("${base.path}-$page.png"))
            page++
        }
    }

    /** Drags the scrollable content back to the top (so rows are clickable below the expanded app bar again). */
    private fun scrollToTop() {
        repeat(10) {
            val (value, _) = scrollState() ?: return
            if (value <= 0.5f) return
            drag(up = false)
        }
    }

    /** A slow drag over the main scroller that stops before lifting: scrolls by the drag distance, no fling. */
    private fun drag(up: Boolean) {
        val scroller = compose.onAllNodes(verticallyScrollable, useUnmergedTree = true)
        val index = scroller.fetchSemanticsNodes().withIndex().maxBy { it.value.boundsInRoot.height }.index
        scroller[index].performTouchInput {
            val distance = height * 0.6f
            down(Offset(centerX, if (up) height * 0.85f else height * 0.15f))
            val steps = 30
            repeat(steps) { moveBy(Offset(0f, (if (up) -distance else distance) / steps)) }
            advanceEventTime(500)
            up()
        }
        compose.waitForIdle()
    }

    /**
     * The dialog lives in its own window: capture it alone, and also as the screen shows it, the
     * main window dimmed by the dialog window's dim amount with the dialog drawn centred on it.
     */
    private fun captureDialog(base: File) {
        val dialogNode = compose.onNode(isDialog())
        val dialog = dialogNode.captureToImage().asAndroidBitmap()
        save(dialog, File("${base.path}-only.png"))

        val dialogRoot = dialogNode.fetchSemanticsNode().root
        val isMainRoot = SemanticsMatcher("root of the main window") { it.parent == null && it.root !== dialogRoot }
        val mainNode = compose.onNode(isMainRoot)
        val screen = mainNode.captureToImage().asAndroidBitmap()
        val mainView = (mainNode.fetchSemanticsNode().root as ViewRootForTest).view
        val dialogView = (dialogNode.fetchSemanticsNode().root as ViewRootForTest).view
        val lp = dialogView.rootView.layoutParams as? WindowManager.LayoutParams
        val dim = if (lp != null && lp.flags and WindowManager.LayoutParams.FLAG_DIM_BEHIND != 0) lp.dimAmount else 0f
        val loc = IntArray(2).also { dialogView.getLocationOnScreen(it) }
        val mainLoc = IntArray(2).also { mainView.getLocationOnScreen(it) }
        val out = screen.copy(Bitmap.Config.ARGB_8888, true)
        Canvas(out).apply {
            drawColor(Color.argb((dim * 255).toInt(), 0, 0, 0))
            // Robolectric's window manager does not apply the dialog window's gravity (it reports the
            // window at 0,0), so centre it the way a device's window manager does for the default
            // Gravity.CENTER; otherwise use the reported window position.
            val inWindow = dialogNode.fetchSemanticsNode().positionInWindow
            val centred = lp == null || lp.gravity == Gravity.NO_GRAVITY || lp.gravity == Gravity.CENTER
            val x = if (centred) (out.width - dialog.width) / 2f else loc[0] - mainLoc[0] + inWindow.x
            val y = if (centred) (out.height - dialog.height) / 2f else loc[1] - mainLoc[1] + inWindow.y
            drawBitmap(dialog, x, y, Paint(Paint.FILTER_BITMAP_FLAG))
        }
        save(out, File(base.path + ".png"))
    }

    private fun save(bitmap: Bitmap, file: File) {
        file.outputStream().use { bitmap.compress(Bitmap.CompressFormat.PNG, 100, it) }
        println("screenshot: ${file.absolutePath} (${bitmap.width}x${bitmap.height})")
    }
}
