package app.hocket.ui

import android.graphics.Bitmap
import androidx.activity.ComponentActivity
import androidx.compose.ui.graphics.asAndroidBitmap
import androidx.compose.ui.test.captureToImage
import androidx.compose.ui.test.isRoot
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.performClick
import androidx.navigation.compose.rememberNavController
import androidx.test.ext.junit.runners.AndroidJUnit4
import app.hocket.core.api.AlbumResumePoint
import app.hocket.ui.screens.detail.AlbumDetailScreen
import app.hocket.ui.screens.settings.PlaybackSettingsScreen
import org.junit.Assume.assumeTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode
import java.io.File

/**
 * The album page when an album can be resumed, and the setting that turns that off.
 *
 *     HOCKET_SCREENSHOT_DIR=/tmp/shots ./gradlew :app:testDebugUnitTest --tests '*AlbumResumeScreenshotTest*' --rerun
 */
@RunWith(AndroidJUnit4::class)
@Config(sdk = [35], qualifiers = "w411dp-h891dp-xxhdpi")
@GraphicsMode(GraphicsMode.Mode.NATIVE)
class AlbumResumeScreenshotTest {
    @get:Rule
    val compose = createAndroidComposeRule<ComponentActivity>()

    private val outDir: File? = (System.getProperty("hocket.screenshotDir") ?: System.getenv("HOCKET_SCREENSHOT_DIR"))
        ?.takeIf { it.isNotBlank() }?.let(::File)

    @Test
    @Config(qualifiers = "+notnight")
    fun albumResumeAndTheSetting() {
        val dir = outDir
        assumeTrue("set HOCKET_SCREENSHOT_DIR to write screenshots", dir != null)
        dir!!.mkdirs()
        val core = TestCore(startPlaying = false)
        val album = core.fake.library.albums.first()
        val track = core.fake.library.albumTracks(album.id)[1]
        core.fake.albumResumes[album.id] = AlbumResumePoint(source = "fake-server", albumId = album.id, track.id, 1u, track.durationMs / 3u, track.durationMs, 1.0)
        core.start()
        compose.setThemedContent(core) { AlbumDetailScreen(rememberNavController(), album.id) }
        compose.waitUntil(8_000) { compose.onAllNodesWithText("Resume", useUnmergedTree = true).fetchSemanticsNodes().isNotEmpty() }
        settle()
        shot(dir, "android-album-resume")
        compose.onNodeWithTag("detail.more").performClick()
        compose.waitUntil(5_000) { compose.onAllNodesWithText("Start over").fetchSemanticsNodes().isNotEmpty() }
        settle()
        shot(dir, "android-album-start-over", sheet = true)
    }

    @Test
    @Config(qualifiers = "+notnight")
    fun resumeSetting() {
        val dir = outDir
        assumeTrue("set HOCKET_SCREENSHOT_DIR to write screenshots", dir != null)
        dir!!.mkdirs()
        val core = TestCore(startPlaying = false)
        core.start()
        compose.setThemedContent(core) { PlaybackSettingsScreen(rememberNavController()) }
        compose.waitUntil(8_000) { compose.onAllNodesWithText("Resume albums", substring = true).fetchSemanticsNodes().isNotEmpty() }
        settle()
        shot(dir, "android-resume-setting")
    }

    private fun settle() {
        Thread.sleep(200)
        org.robolectric.Shadows.shadowOf(android.os.Looper.getMainLooper()).idleFor(java.time.Duration.ofMillis(300))
        compose.waitForIdle()
    }

    private fun shot(dir: File, name: String, sheet: Boolean = false) {
        val bitmap = compose.onAllNodes(isRoot())[0].captureToImage().asAndroidBitmap().copy(Bitmap.Config.ARGB_8888, true)
        if (sheet) {
            val canvas = android.graphics.Canvas(bitmap)
            val activityDecor = compose.activity.window.decorView
            val global = Class.forName("android.view.WindowManagerGlobal").getMethod("getInstance").invoke(null)
            val field = global.javaClass.getDeclaredField("mViews").apply { isAccessible = true }
            @Suppress("UNCHECKED_CAST")
            val views = field.get(global) as List<android.view.View>
            for (view in views) {
                if (view === activityDecor || view.width == 0) continue
                val at = IntArray(2).also { view.getLocationOnScreen(it) }
                canvas.save()
                canvas.translate(at[0].toFloat(), at[1].toFloat())
                view.draw(canvas)
                canvas.restore()
            }
        }
        File(dir, "$name.png").outputStream().use { bitmap.compress(Bitmap.CompressFormat.PNG, 100, it) }
    }
}
