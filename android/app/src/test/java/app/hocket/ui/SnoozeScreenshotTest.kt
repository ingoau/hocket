package app.hocket.ui

import android.graphics.Bitmap
import androidx.activity.ComponentActivity
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.ui.graphics.asAndroidBitmap
import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.test.captureToImage
import androidx.compose.ui.test.isRoot
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.test.onAllNodesWithContentDescription
import androidx.compose.ui.test.onAllNodesWithTag
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.performSemanticsAction
import androidx.navigation.compose.rememberNavController
import androidx.test.ext.junit.runners.AndroidJUnit4
import app.hocket.core.ActionIds
import app.hocket.ui.screens.library.SnoozedScreen
import org.junit.Assume.assumeTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode
import java.io.File

/**
 * Snooze screens on the fake core. Only runs with an output directory:
 *
 *     HOCKET_SCREENSHOT_DIR=/tmp/shots ./gradlew :app:testDebugUnitTest --tests '*SnoozeScreenshotTest*' --rerun
 */
@RunWith(AndroidJUnit4::class)
@Config(sdk = [35], qualifiers = "w411dp-h891dp-xxhdpi")
@GraphicsMode(GraphicsMode.Mode.NATIVE)
class SnoozeScreenshotTest {
    @get:Rule
    val compose = createAndroidComposeRule<ComponentActivity>()

    private val outDir: File? = (System.getProperty("hocket.screenshotDir") ?: System.getenv("HOCKET_SCREENSHOT_DIR"))
        ?.takeIf { it.isNotBlank() }?.let(::File)

    @Test
    @Config(qualifiers = "+notnight")
    fun light() = capture()

    private fun capture() {
        val dir = outDir
        assumeTrue("set HOCKET_SCREENSHOT_DIR to write screenshots", dir != null)
        dir!!.mkdirs()
        val core = TestCore(startPlaying = false)
        val lib = core.fake.library
        core.start()
        compose.waitUntil(5_000) { core.client.started.value }
        val trackId = lib.playlistTracks.getValue("pl0").first()
        val title = lib.track(trackId)!!.title
        compose.setThemedContent(core) {
            CompositionLocalProvider(LocalDetailNavigator provides DetailNavigator({}, {}, {})) {
                app.hocket.ui.screens.detail.PlaylistDetailScreen(rememberNavController(), "pl0")
            }
        }
        compose.waitUntil(5_000) { compose.onAllNodesWithContentDescription("More options for $title").fetchSemanticsNodes().isNotEmpty() }
        settle()
        compose.onAllNodesWithContentDescription("More options for $title")[0].performSemanticsAction(SemanticsActions.OnClick)
        awaitTag("songMenu.more")
        settle()
        compose.onNodeWithTag("songMenu.more").performSemanticsAction(SemanticsActions.OnClick)
        settle()
        compose.onNodeWithTag("songMenu.action.${ActionIds.SNOOZE}").performSemanticsAction(SemanticsActions.OnClick)
        awaitTag("songMenu.snooze.week")
        settle()
        shot(dir, "android-snooze-durations")
        compose.onNodeWithTag("songMenu.snooze.week").performSemanticsAction(SemanticsActions.OnClick)
        settle()
        compose.waitUntil(5_000) { core.client.snoozes.value.any { it.trackId == trackId } }
        settle()
        shot(dir, "android-snoozed-row")
        compose.onAllNodesWithContentDescription("More options for $title")[0].performSemanticsAction(SemanticsActions.OnClick)
        awaitTag("songMenu.more")
        compose.onNodeWithTag("songMenu.more").performSemanticsAction(SemanticsActions.OnClick)
        awaitTag("songMenu.action.${ActionIds.UNSNOOZE}")
        settle()
        shot(dir, "android-unsnooze")

        compose.setThemedContent(core) {
            SnoozedScreen(rememberNavController())
        }
        awaitTag("library.snoozed")
        settle()
        shot(dir, "android-snoozed-list")
    }

    private fun settle() {
        compose.mainClock.advanceTimeBy(1_200)
        compose.waitForIdle()
    }

    private fun awaitTag(tag: String) {
        compose.waitUntil(5_000) { compose.onAllNodesWithTag(tag).fetchSemanticsNodes().isNotEmpty() }
        compose.waitForIdle()
    }

    private fun shot(dir: File, name: String) {
        val bitmap = compose.onAllNodes(isRoot())[0].captureToImage().asAndroidBitmap().copy(Bitmap.Config.ARGB_8888, true)
        val file = File(dir, "$name.png")
        file.outputStream().use { bitmap.compress(Bitmap.CompressFormat.PNG, 100, it) }
        println("screenshot: ${file.absolutePath}")
    }
}
