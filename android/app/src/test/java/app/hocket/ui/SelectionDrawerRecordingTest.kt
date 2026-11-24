package app.hocket.ui

import android.graphics.Bitmap
import android.graphics.Canvas
import android.graphics.Paint
import androidx.activity.ComponentActivity
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.asAndroidBitmap
import androidx.compose.ui.test.captureToImage
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.test.onAllNodesWithTag
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onRoot
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.hasAnyAncestor
import androidx.compose.ui.test.hasTestTag
import androidx.compose.ui.test.performSemanticsAction
import androidx.compose.ui.test.performTouchInput
import androidx.compose.ui.test.swipe
import androidx.compose.ui.test.click
import androidx.compose.ui.semantics.SemanticsActions
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
import java.io.FileOutputStream

/**
 * A frame-by-frame recording (one frame per 16 ms) of the selection drawer in the whole app: songs
 * picked on a playlist (a long press, then taps), the drawer rising as the bar and the mini player
 * step down, a swipe up opening the songs' menu, a swipe down back to the peek, and Clear. The finger
 * is drawn as a dot. For review by eye (stitch with ffmpeg at 60 fps); only runs with an output directory:
 *
 *     HOCKET_SCREENSHOT_DIR=/tmp/drawer ./gradlew :app:testDebugUnitTest --tests '*SelectionDrawerRecordingTest*' --rerun
 */
@RunWith(AndroidJUnit4::class)
@Config(sdk = [35], application = android.app.Application::class, qualifiers = "w411dp-h891dp-xhdpi")
@GraphicsMode(GraphicsMode.Mode.NATIVE)
class SelectionDrawerRecordingTest {
    @get:Rule
    val compose = createAndroidComposeRule<ComponentActivity>()

    private val outDir: File? = (System.getProperty("hocket.screenshotDir") ?: System.getenv("HOCKET_SCREENSHOT_DIR"))
        ?.takeIf { it.isNotBlank() }?.let(::File)

    private fun exists(tag: String) = compose.onAllNodesWithTag(tag).fetchSemanticsNodes().isNotEmpty()

    private var n = 0
    private var finger: Offset? = null
    private lateinit var dir: File

    @Test
    fun selectAndOpen() {
        val out = outDir
        assumeTrue("set HOCKET_SCREENSHOT_DIR to write recording frames", out != null)
        dir = File(out!!, "selection-drawer").apply { mkdirs() }
        val core = TestCore(startPlaying = true)
        val lib = core.fake.library
        compose.setThemedContent(core) { CompositionLocalProvider(LocalFakeCoreBanner provides false) { AppRoot(core.client) } }
        core.start()
        compose.waitUntil(5_000) { exists("home.screen") }
        core.client.dispatch(Commands.setSetting(SettingKeys.LIBRARY_PINS, """[{"kind":"playlist","id":"pl0","tap":"open"}]"""))
        compose.onNodeWithTag("navBar.library").performSemanticsAction(SemanticsActions.OnClick)
        compose.waitUntil(5_000) { exists("library.pin") }
        compose.onAllNodesWithTag("library.pin")[0].performTouchInput { click() }
        val titles = lib.playlistTracks.getValue("pl0").take(6).map { lib.track(it)!!.title }
        compose.waitUntil(5_000) { compose.onAllNodesWithText(titles[0]).fetchSemanticsNodes().isNotEmpty() }
        compose.mainClock.advanceTimeBy(1_500)
        compose.waitForIdle()
        // The songs up the screen, clear of where the drawer will rise.
        val first = row(titles[0])
        compose.onRoot().performTouchInput { swipe(Offset(width / 2f, height * 0.8f), Offset(width / 2f, height * 0.8f - (first.y - height * 0.3f)), 600) }
        compose.mainClock.advanceTimeBy(2_000)
        compose.waitForIdle()

        compose.mainClock.autoAdvance = false
        frames(24)
        longPress(row(titles[0]))
        tap(row(titles[1]))
        tap(row(titles[3]))
        openAndClear()
    }

    /** The same over the full player's queue: the drawer rises inside the queue's panel. */
    @Test
    fun inTheQueue() {
        val out = outDir
        assumeTrue("set HOCKET_SCREENSHOT_DIR to write recording frames", out != null)
        dir = File(out!!, "selection-drawer-queue").apply { mkdirs() }
        val core = TestCore(startPlaying = true)
        compose.setThemedContent(core) { CompositionLocalProvider(LocalFakeCoreBanner provides false) { AppRoot(core.client) } }
        core.start()
        compose.waitUntil(5_000) { core.client.started.value && core.client.queue.value.entries.size >= 5 }
        compose.waitUntil(5_000) { exists("miniPlayer") }
        compose.onNodeWithTag("nowPlaying.sheet").performSemanticsAction(SemanticsActions.Expand)
        compose.waitUntil(5_000) { exists("player.mode.queue") }
        compose.onNodeWithTag("player.mode.queue").performSemanticsAction(SemanticsActions.OnClick)
        compose.waitUntil(5_000) { exists("queue.list") }
        compose.mainClock.advanceTimeBy(2_000)
        compose.waitForIdle()
        val q = core.client.queue.value
        val upcoming = q.entries.drop(q.entries.indexOfFirst { it.item.key == q.current?.item?.key } + 1).map { it.track.title }

        compose.mainClock.autoAdvance = false
        frames(24)
        // The first pick folds the controls away: the queue takes their room.
        longPress(queueRow(upcoming[0]))
        frames(20)
        tap(queueRow(upcoming[2]))
        // Select all: every upcoming song.
        tap(compose.onNodeWithContentDescription("Select all").fetchSemanticsNode().boundsInRoot.center, after = 40)
        openAndClear()
    }

    /** Up from the peek (the songs' menu opens over a scrim), down by the handle back to the peek, then Clear. */
    private fun openAndClear() {
        frames(20)
        val peekTop = bounds("selection.peek").top
        val peek = center("selection.peek")
        drag(peek, Offset(peek.x, peek.y - height() * 0.45f), 22)
        frames(70)
        // Most of the way back down: the drawer settles on its peek.
        val handle = center("selection.handle")
        drag(handle, Offset(handle.x, handle.y + (peekTop - bounds("selection.peek").top) * 0.8f), 20)
        frames(50)
        // Clear: the drawer slides away (and on a page, the bar and the mini player come back).
        val bar = bounds("selection.toolbar")
        tap(Offset(bar.left + 32f * density(), bar.center.y), after = 60)
        compose.mainClock.autoAdvance = true
    }

    private fun queueRow(title: String) = compose.onAllNodes(hasText(title) and hasAnyAncestor(hasTestTag("queue.list")))[0].fetchSemanticsNode().boundsInRoot.center

    private fun density() = compose.activity.resources.displayMetrics.density
    private fun height() = compose.onRoot().fetchSemanticsNode().size.height.toFloat()
    private fun bounds(tag: String) = compose.onNodeWithTag(tag, useUnmergedTree = true).fetchSemanticsNode().boundsInRoot
    private fun center(tag: String) = bounds(tag).center
    private fun row(title: String) = compose.onAllNodesWithText(title)[0].fetchSemanticsNode().boundsInRoot.center

    private fun longPress(at: Offset) {
        finger = at
        compose.onRoot().performTouchInput { down(at) }
        frames(36)
        compose.onRoot().performTouchInput { up() }
        finger = null
        frames(30)
    }

    private fun tap(at: Offset, after: Int = 26) {
        finger = at
        compose.onRoot().performTouchInput { down(at) }
        frames(5)
        compose.onRoot().performTouchInput { up() }
        finger = null
        frames(after)
    }

    /** A finger drag over [count] frames, eased in and out (so it lets go gently). */
    private fun drag(from: Offset, to: Offset, count: Int) {
        finger = from
        compose.onRoot().performTouchInput { down(from) }
        frames(3)
        for (i in 1..count) {
            val t = i / count.toFloat()
            val e = t * t * (3f - 2f * t)
            val at = from + (to - from) * e
            finger = at
            compose.onRoot().performTouchInput { moveTo(at, 16) }
            frames(1)
        }
        frames(2)
        compose.onRoot().performTouchInput { up() }
        finger = null
    }

    private val dot = Paint(Paint.ANTI_ALIAS_FLAG).apply { color = android.graphics.Color.argb(110, 255, 255, 255) }
    private val ring = Paint(Paint.ANTI_ALIAS_FLAG).apply { color = android.graphics.Color.argb(150, 60, 60, 60); style = Paint.Style.STROKE }

    private fun frames(count: Int) = repeat(count) {
        compose.mainClock.advanceTimeBy(16)
        val bmp = compose.onRoot().captureToImage().asAndroidBitmap().copy(Bitmap.Config.ARGB_8888, true)
        finger?.let { f ->
            val r = 20f * density()
            ring.strokeWidth = 2f * density()
            Canvas(bmp).apply { drawCircle(f.x, f.y, r, dot); drawCircle(f.x, f.y, r, ring) }
        }
        FileOutputStream(File(dir, "f" + (n++).toString().padStart(4, '0') + ".png")).use { bmp.compress(Bitmap.CompressFormat.PNG, 100, it) }
    }
}
