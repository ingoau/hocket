package app.hocket.ui

import android.graphics.Bitmap
import androidx.activity.ComponentActivity
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.ui.graphics.asAndroidBitmap
import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.test.captureToImage
import androidx.compose.ui.test.hasTestTag
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.test.onAllNodesWithTag
import androidx.compose.ui.test.onFirst
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onRoot
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
 * Frame-by-frame recordings (one frame per 16 ms) of the artwork flights, out and back: a Home
 * card, the Library's Albums tile fan into the Recent row, an album card and a playlist row, each
 * to its page and back. For review by eye (stitch with ffmpeg); only runs with an output directory:
 *
 *     HOCKET_SCREENSHOT_DIR=/tmp/flights ./gradlew :app:testDebugUnitTest --tests '*ArtworkFlightRecordingTest*' --rerun
 */
@RunWith(AndroidJUnit4::class)
@Config(sdk = [35], application = android.app.Application::class, qualifiers = "w411dp-h891dp-xxhdpi")
@GraphicsMode(GraphicsMode.Mode.NATIVE)
class ArtworkFlightRecordingTest {
    @get:Rule
    val compose = createAndroidComposeRule<ComponentActivity>()

    private val outDir: File? = (System.getProperty("hocket.screenshotDir") ?: System.getenv("HOCKET_SCREENSHOT_DIR"))
        ?.takeIf { it.isNotBlank() }?.let(::File)

    private fun exists(tag: String, unmerged: Boolean = false) =
        compose.onAllNodesWithTag(tag, useUnmergedTree = unmerged).fetchSemanticsNodes().isNotEmpty()

    private fun back() = compose.activityRule.scenario.onActivity { it.onBackPressedDispatcher.onBackPressed() }

    @Test
    fun flights() {
        val dir = outDir
        assumeTrue("set HOCKET_SCREENSHOT_DIR to write recording frames", dir != null)
        dir!!.mkdirs()
        val core = TestCore(startPlaying = false)
        compose.setThemedContent(core) { CompositionLocalProvider(LocalFakeCoreBanner provides false) { AppRoot(core.client) } }
        core.start()
        compose.waitUntil(5_000) { exists("home.screen") }
        compose.waitUntil(5_000) { exists("home.item") }
        settle()

        record(dir, "1-home-card-open") { compose.onAllNodesWithTag("home.item").onFirst().performSemanticsAction(SemanticsActions.OnClick) }
        compose.waitUntil(5_000) { exists("detail.header") }
        record(dir, "2-home-card-back") { back() }
        compose.waitUntil(5_000) { exists("home.item") }

        compose.onNodeWithTag("navBar.library").performSemanticsAction(SemanticsActions.OnClick)
        compose.waitUntil(5_000) { exists("library.grid") }
        compose.onNodeWithTag("library.grid").performScrollToNode(hasTestTag("library.browse.albums"))
        compose.waitUntil(5_000) { exists("library.browse.albums.fan.0", unmerged = true) }
        settle()

        record(dir, "3-albums-tile-open") { compose.onNodeWithTag("library.browse.albums").performSemanticsAction(SemanticsActions.OnClick) }
        compose.waitUntil(5_000) { exists("library.group.recent") }
        record(dir, "4-albums-tile-back") { back() }
        compose.waitUntil(5_000) { exists("library.browse.albums") }

        compose.onNodeWithTag("library.grid").performScrollToNode(hasTestTag("library.album"))
        compose.waitUntil(5_000) { exists("library.album") }
        settle()
        record(dir, "5-album-open") { compose.onAllNodesWithTag("library.album").onFirst().performSemanticsAction(SemanticsActions.OnClick) }
        compose.waitUntil(5_000) { exists("detail.header") }
        record(dir, "6-album-back") { back() }
        compose.waitUntil(5_000) { exists("library.album") }

        compose.onNodeWithTag("library.grid").performScrollToNode(hasTestTag("library.browse.playlists"))
        compose.onNodeWithTag("library.browse.playlists").performSemanticsAction(SemanticsActions.OnClick)
        compose.waitUntil(5_000) { exists("library.playlist") }
        settle()
        record(dir, "7-playlist-open") { compose.onAllNodesWithTag("library.playlist").onFirst().performSemanticsAction(SemanticsActions.OnClick) }
        compose.waitUntil(5_000) { exists("detail.header") }
        record(dir, "8-playlist-back") { back() }
    }

    private fun settle() {
        compose.mainClock.advanceTimeBy(1_000)
        compose.waitForIdle()
    }

    /** A few still frames before the tap, then the motion one frame (16 ms) at a time, then the rest. */
    private fun record(dir: File, name: String, start: () -> Unit) {
        val sub = File(dir, name).apply { mkdirs() }
        var n = 0
        fun frame() = shot(sub, "f" + (n++).toString().padStart(3, '0'))
        repeat(6) { frame() }
        compose.mainClock.autoAdvance = false
        start()
        compose.runOnUiThread { androidx.compose.runtime.snapshots.Snapshot.sendApplyNotifications() }
        repeat(45) { i ->
            if (i > 0) compose.mainClock.advanceTimeBy(16)
            frame()
        }
        compose.mainClock.autoAdvance = true
        compose.mainClock.advanceTimeBy(600)
        compose.waitForIdle()
        repeat(10) { frame() }
    }

    private fun shot(dir: File, name: String) {
        val bmp = compose.onRoot().captureToImage().asAndroidBitmap()
        FileOutputStream(File(dir, "$name.png")).use { bmp.compress(Bitmap.CompressFormat.PNG, 100, it) }
    }
}
