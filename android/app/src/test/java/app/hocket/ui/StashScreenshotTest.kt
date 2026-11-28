package app.hocket.ui

import android.graphics.Bitmap
import androidx.activity.ComponentActivity
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.ui.graphics.asAndroidBitmap
import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.test.captureToImage
import androidx.compose.ui.test.hasTestTag
import androidx.compose.ui.test.isRoot
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.test.onAllNodesWithTag
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.performScrollToNode
import androidx.compose.ui.test.performScrollToIndex
import androidx.compose.ui.test.performSemanticsAction
import androidx.test.ext.junit.runners.AndroidJUnit4
import app.hocket.core.api.Command
import app.hocket.core.api.CommandStashAddInner
import app.hocket.core.groupEntries
import app.hocket.core.restEntries
import app.hocket.ui.nav.AppRoot
import app.hocket.ui.nav.LocalFakeCoreBanner
import org.junit.Assume.assumeTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode
import org.robolectric.shadows.ShadowLooper
import androidx.test.core.view.captureToBitmapAsync
import java.io.File

/**
 * The stash sheet on the fake core: its songs, picking some, its menu, one song's own menu;
 * plus frame-by-frame recordings (one frame per 16 ms) of opening it and picking songs. Only runs
 * with an output directory:
 *
 *     HOCKET_SCREENSHOT_DIR=/tmp/stash ./gradlew :app:testDebugUnitTest --tests '*StashScreenshotTest*' --rerun
 */
@RunWith(AndroidJUnit4::class)
@Config(sdk = [35], application = android.app.Application::class, qualifiers = "w411dp-h915dp-night-xxhdpi")
@GraphicsMode(GraphicsMode.Mode.NATIVE)
class StashScreenshotTest {
    @get:Rule
    val compose = createAndroidComposeRule<ComponentActivity>()

    private val outDir: File? = (System.getProperty("hocket.screenshotDir") ?: System.getenv("HOCKET_SCREENSHOT_DIR"))
        ?.takeIf { it.isNotBlank() }?.let(::File)

    private fun exists(tag: String) = compose.onAllNodesWithTag(tag).fetchSemanticsNodes().isNotEmpty()

    private fun open(songs: Int): File {
        val dir = outDir
        assumeTrue("set HOCKET_SCREENSHOT_DIR to write screenshots", dir != null)
        dir!!.mkdirs()
        val core = TestCore()
        compose.setThemedContent(core) { CompositionLocalProvider(LocalFakeCoreBanner provides false) { AppRoot(core.client) } }
        core.start()
        compose.waitUntil(5_000) { core.client.started.value }
        val q = core.client.queue.value
        val ids = (listOfNotNull(q.current) + q.groupEntries + q.restEntries).map { it.item.track }.distinct().take(songs)
        core.client.dispatch(Command.StashAdd(CommandStashAddInner(ids)))
        compose.waitUntil(5_000) { core.client.stash.value.size == ids.size }
        settle()
        return dir
    }

    @Test
    fun shortStash() {
        val dir = open(2)
        compose.onNodeWithTag("stash.chip").performSemanticsAction(SemanticsActions.OnClick)
        compose.waitUntil(5_000) { exists("stash.row") }
        settle()
        shot(dir, "stash-0-short")
        compose.onNodeWithTag("stash.menu.action.playNext", useUnmergedTree = true).performSemanticsAction(SemanticsActions.OnClick)
        compose.mainClock.advanceTimeBy(700)
        compose.waitForIdle()
        shot(dir, "stash-0a-queued")
        settle(); settle(); settle(); settle()
        record(dir, "rec-0-expand", frames = 30) { compose.onNodeWithTag("stash.sheetHandle").performSemanticsAction(SemanticsActions.OnClick) }
        settle()
        shot(dir, "stash-0b-short-open")
    }

    @Test
    fun stash() {
        val dir = open(9)

        record(dir, "rec-1-open", frames = 60) { compose.onNodeWithTag("stash.chip").performSemanticsAction(SemanticsActions.OnClick) }
        compose.waitUntil(5_000) { exists("stash.row") }
        settle()
        shot(dir, "stash-1-open")
        compose.onNodeWithTag("stash.list").performScrollToNode(hasTestTag("stash.menu"))
        settle()
        shot(dir, "stash-1b-scrolled")
        compose.onNodeWithTag("stash.list").performScrollToIndex(0)
        settle()

        record(dir, "rec-2-pick", frames = 30) { compose.onAllNodesWithTag("stash.row")[1].performSemanticsAction(SemanticsActions.OnClick) }
        record(dir, "rec-3-pick", frames = 30) { compose.onAllNodesWithTag("stash.row")[3].performSemanticsAction(SemanticsActions.OnClick) }
        settle()
        shot(dir, "stash-2-picked")

        compose.onNodeWithTag("stash.list").performScrollToNode(hasTestTag("stash.menu"))
        settle()
        shot(dir, "stash-3-menu-picked")

        compose.onNodeWithTag("stash.list").performScrollToIndex(0)
        settle()
        compose.onNodeWithTag("stash.deselect").performSemanticsAction(SemanticsActions.OnClick)
        settle()
        compose.onNodeWithTag("stash.list").performScrollToNode(hasTestTag("stash.menu"))
        settle()
        shot(dir, "stash-4-menu-all")

        compose.onNodeWithTag("stash.list").performScrollToIndex(0)
        settle()
        compose.onAllNodesWithTag("stash.row")[0].performSemanticsAction(SemanticsActions.OnClick)
        settle()
        compose.onNodeWithTag("stash.list").performScrollToNode(hasTestTag("stash.menu"))
        settle()
        shot(dir, "stash-5-one-song")
    }

    private fun settle() {
        compose.mainClock.advanceTimeBy(1_200)
        compose.waitForIdle()
    }

    /** A few still frames, then the motion one frame (16 ms) at a time, then the rest. */
    private fun record(dir: File, name: String, frames: Int = 45, start: () -> Unit) {
        val sub = File(dir, name).apply { mkdirs() }
        var n = 0
        fun frame() = shot(sub, "f" + (n++).toString().padStart(3, '0'))
        repeat(4) { frame() }
        compose.mainClock.autoAdvance = false
        start()
        compose.runOnUiThread { androidx.compose.runtime.snapshots.Snapshot.sendApplyNotifications() }
        repeat(frames) { i ->
            if (i > 0) compose.mainClock.advanceTimeBy(16)
            frame()
        }
        compose.mainClock.autoAdvance = true
        compose.mainClock.advanceTimeBy(600)
        compose.waitForIdle()
        repeat(8) { frame() }
    }

    @Suppress("UNCHECKED_CAST")
    private fun rootViews(): List<android.view.View> {
        val global = Class.forName("android.view.WindowManagerGlobal").getMethod("getInstance").invoke(null)
        val field = global.javaClass.getDeclaredField("mViews").apply { isAccessible = true }
        return (field.get(global) as List<android.view.View>).toList()
    }

    /** The activity's window with every other window (the sheets) drawn over it. */
    private fun shot(dir: File, name: String) {
        val bitmap = compose.onAllNodes(isRoot())[0].captureToImage().asAndroidBitmap().copy(Bitmap.Config.ARGB_8888, true)
        val canvas = android.graphics.Canvas(bitmap)
        val activityDecor = compose.activity.window.decorView
        for (view in rootViews()) {
            if (view === activityDecor || view.width == 0) continue
            val at = IntArray(2).also { view.getLocationOnScreen(it) }
            var drawn: Bitmap? = null
            compose.runOnUiThread {
                val future = view.captureToBitmapAsync(null)
                while (!future.isDone) ShadowLooper.idleMainLooper()
                drawn = future.get()
            }
            canvas.drawBitmap(drawn!!, at[0].toFloat(), at[1].toFloat(), null)
        }
        val file = File(dir, "$name.png")
        file.outputStream().use { bitmap.compress(Bitmap.CompressFormat.PNG, 100, it) }
    }
}
