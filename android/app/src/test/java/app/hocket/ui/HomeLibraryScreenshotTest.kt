package app.hocket.ui

import android.graphics.Bitmap
import androidx.activity.ComponentActivity
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.asAndroidBitmap
import androidx.compose.ui.test.captureToImage
import androidx.compose.ui.test.hasTestTag
import androidx.compose.ui.test.isRoot
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.test.longClick
import androidx.compose.ui.test.onAllNodesWithTag
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performSemanticsAction
import androidx.compose.ui.test.performScrollToNode
import androidx.compose.ui.test.performTouchInput
import androidx.test.ext.junit.runners.AndroidJUnit4
import app.hocket.core.Commands
import app.hocket.core.SettingKeys
import app.hocket.ui.nav.AppRoot
import app.hocket.ui.nav.LocalFakeCoreBanner
import org.junit.Assume.assumeTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode
import java.io.File

/**
 * Renders the customisable Home, the Library hub (pins, browse, recently added) and its pin editor
 * to PNGs on the fake core, for visual review. Like [AppScreenshotTest], it only runs with an output directory:
 *
 *     HOCKET_SCREENSHOT_DIR=/tmp/shots ./gradlew :app:testDebugUnitTest --tests '*HomeLibraryScreenshotTest*' --rerun
 */
@RunWith(AndroidJUnit4::class)
@Config(sdk = [35], qualifiers = "w411dp-h891dp-xxhdpi")
@GraphicsMode(GraphicsMode.Mode.NATIVE)
class HomeLibraryScreenshotTest {
    @get:Rule
    val compose = createAndroidComposeRule<ComponentActivity>()

    private val outDir: File? = (System.getProperty("hocket.screenshotDir") ?: System.getenv("HOCKET_SCREENSHOT_DIR"))
        ?.takeIf { it.isNotBlank() }?.let(::File)

    @Test
    @Config(qualifiers = "+night")
    fun dark() = capture("dark")

    @Test
    @Config(qualifiers = "+notnight")
    fun light() = capture("light")

    private fun capture(prefix: String) {
        val dir = outDir
        assumeTrue("set HOCKET_SCREENSHOT_DIR (or -Dhocket.screenshotDir) to write screenshots", dir != null)
        dir!!.mkdirs()
        val core = TestCore(startPlaying = true)
        compose.setThemedContent(core) { CompositionLocalProvider(LocalFakeCoreBanner provides false) { AppRoot(core.client) } }
        core.start()
        compose.waitUntil(5_000) { core.client.started.value }
        val lib = core.fake.library
        val album = lib.albums[1]
        val artist = lib.artists[0]
        val playlist = lib.playlists[0]
        val genre = lib.genres.first().name
        val track = lib.tracks[3]
        val pins = """[{"kind":"playlist","id":"${playlist.id}","tap":"open"},{"kind":"album","id":"${album.id}","tap":"shuffle"},""" +
            """{"kind":"artist","id":"${artist.id}","tap":"open"},{"kind":"genre","id":"${genre.replace("\"", "\\\"")}","tap":"open"},""" +
            """{"kind":"track","id":"${track.id}","tap":"play"},{"kind":"filter","id":"f-loved","tap":"open"}]"""
        core.client.dispatch(Commands.setSetting(SettingKeys.LIBRARY_PINS, pins))
        awaitTag("home.screen")
        awaitTag("miniPlayer")
        settle()
        shot(dir, "$prefix-01-home")

        step {
            awaitTag("home.section.rediscover")
            compose.onNodeWithTag("home.section.rediscover").performSemanticsAction(androidx.compose.ui.semantics.SemanticsActions.OnLongClick)
            settle()
            shot(dir, "$prefix-02-section-menu")
            dismissSheet()
        }
        step {
            compose.onNodeWithTag("home.customise").performClick()
            awaitTag("home.customiseSheet")
            settle()
            shot(dir, "$prefix-03-customise")
            dismissSheet()
        }
        step {
            compose.onNodeWithTag("navBar.library").performClick()
            awaitTag("library.pins")
            settle()
            shot(dir, "$prefix-04-library")
        }
        step {
            compose.onAllNodesWithTag("library.pin")[1].performTouchInput { longClick() }
            settle()
            shot(dir, "$prefix-05-pin-menu")
            dismissSheet()
        }
        step {
            compose.onNodeWithTag("library.grid").performScrollToNode(hasTestTag("library.album"))
            settle()
            shot(dir, "$prefix-06-library-recent")
        }
        // The pin editor (Edit in the Library's top bar): at rest, a tile held mid-drag, a pin unpinned.
        step {
            compose.onNodeWithTag("library.pins.edit").performClick()
            awaitTag("library.pins.editor")
            settle()
            shot(dir, "$prefix-07-pin-editor")
        }
        step {
            val handle = compose.onAllNodesWithTag("library.pins.editor.handle", useUnmergedTree = true)[1]
            handle.performTouchInput { down(center); moveBy(Offset(0f, 30f)); moveBy(Offset(0f, 30f)) }
            settle()
            shot(dir, "$prefix-08-pin-editor-drag")
            handle.performTouchInput { up() }
            settle()
        }
        step {
            compose.onAllNodesWithTag("library.pins.editor.unpin")[3].performClick()
            compose.mainClock.advanceTimeBy(600)
            compose.waitForIdle()
            shot(dir, "$prefix-09-pin-editor-unpinned")
            compose.onNodeWithTag("library.pins.editor.cancel").performClick()
            settle()
        }
    }

    private fun step(block: () -> Unit) {
        runCatching(block).onFailure { println("screenshot step failed: $it"); it.stackTrace.take(12).forEach { e -> println("   at $e") } }
    }

    private fun settle() {
        compose.mainClock.advanceTimeBy(1_500)
        compose.waitForIdle()
    }

    private fun awaitTag(tag: String) {
        compose.waitUntil(5_000) { compose.onAllNodesWithTag(tag).fetchSemanticsNodes().isNotEmpty() }
        compose.waitForIdle()
    }

    @Suppress("UNCHECKED_CAST")
    private fun rootViews(): List<android.view.View> {
        val global = Class.forName("android.view.WindowManagerGlobal").getMethod("getInstance").invoke(null)
        val field = global.javaClass.getDeclaredField("mViews").apply { isAccessible = true }
        return (field.get(global) as List<android.view.View>).toList()
    }

    /** Closes the sheet on top with its own back handling. */
    private fun dismissSheet() {
        rootViews().filter { it !== compose.activity.window.decorView }.lastOrNull()?.let { v ->
            val down = android.view.KeyEvent(android.view.KeyEvent.ACTION_DOWN, android.view.KeyEvent.KEYCODE_BACK)
            val up = android.view.KeyEvent(android.view.KeyEvent.ACTION_UP, android.view.KeyEvent.KEYCODE_BACK)
            compose.runOnUiThread { v.dispatchKeyEvent(down); v.dispatchKeyEvent(up) }
        }
        settle()
    }

    /** The activity's window with every other window (a sheet is one) drawn over it. */
    private fun shot(dir: File, name: String) {
        val bitmap = compose.onAllNodes(isRoot())[0].captureToImage().asAndroidBitmap().copy(Bitmap.Config.ARGB_8888, true)
        val canvas = android.graphics.Canvas(bitmap)
        val activityDecor = compose.activity.window.decorView
        for (view in rootViews()) {
            if (view === activityDecor || view.width == 0) continue
            val at = IntArray(2).also { view.getLocationOnScreen(it) }
            canvas.save()
            canvas.translate(at[0].toFloat(), at[1].toFloat())
            view.draw(canvas)
            canvas.restore()
        }
        val file = File(dir, "$name.png")
        file.outputStream().use { bitmap.compress(Bitmap.CompressFormat.PNG, 100, it) }
        println("screenshot: ${file.absolutePath}")
    }
}
