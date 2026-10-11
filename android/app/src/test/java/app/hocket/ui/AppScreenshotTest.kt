package app.hocket.ui

import android.graphics.Bitmap
import androidx.activity.ComponentActivity
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.asAndroidBitmap
import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.test.captureToImage
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.test.onAllNodesWithTag
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.isRoot
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performSemanticsAction
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
 * Renders the main app screens (home, library, album, the mini player, the full player in each
 * mode, its sheets and gestures) to PNGs on the fake core, for visual review. Not an assertion
 * test: it only runs when an output directory is given, e.g.
 *
 *     HOCKET_SCREENSHOT_DIR=/tmp/shots ./gradlew :app:testDebugUnitTest --tests '*AppScreenshotTest*' --rerun
 *
 * The debug build's "Core not built" banner (Robolectric can only run the fake core) is switched off
 * here ([LocalFakeCoreBanner]), so the captures show the screens as a user sees them.
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

    /** The player on a narrow phone (display size "largest") at font scale 1.5. */
    @Test
    @Config(qualifiers = "w360dp-h760dp-night-xxhdpi")
    fun largeFont() {
        val dir = outDir
        assumeTrue("set HOCKET_SCREENSHOT_DIR (or -Dhocket.screenshotDir) to write screenshots", dir != null)
        dir!!.mkdirs()
        val core = TestCore(startPlaying = true)
        compose.setContent {
            val base = androidx.compose.ui.platform.LocalDensity.current
            CompositionLocalProvider(androidx.compose.ui.platform.LocalDensity provides androidx.compose.ui.unit.Density(base.density, fontScale = 1.5f)) {
                app.hocket.ui.theme.HocketTheme(client = core.client) {
                    CompositionLocalProvider(LocalCoreClient provides core.client, LocalFakeCoreBanner provides false) { AppRoot(core.client) }
                }
            }
        }
        core.start()
        compose.waitUntil(5_000) { core.client.started.value }
        awaitTag("miniPlayer")
        compose.onNodeWithTag("miniPlayer.info").performSemanticsAction(SemanticsActions.OnClick)
        awaitTag("player.playPause")
        settle()
        shot(dir, "android-dark-14-player-narrow-font-1.5")
        click("player.mode.queue")
        settle()
        shot(dir, "android-dark-15-queue-narrow-font-1.5")
    }

    private fun captureAll(theme: String) {
        val dir = outDir
        assumeTrue("set HOCKET_SCREENSHOT_DIR (or -Dhocket.screenshotDir) to write screenshots", dir != null)
        dir!!.mkdirs()
        val prefix = "android-$theme"
        val core = TestCore(startPlaying = true)
        compose.setThemedContent(core) { CompositionLocalProvider(LocalFakeCoreBanner provides false) { AppRoot(core.client) } }
        core.start()
        compose.waitUntil(5_000) { core.client.started.value }
        awaitTag("miniPlayer")
        settle()
        shot(dir, "$prefix-01-home")

        step {
            compose.onNodeWithTag("navBar.library").performClick()
            compose.waitUntil(5_000) { runCatching { compose.onNodeWithText("Albums").assertExists() }.isSuccess }
            settle()
            compose.onAllNodesWithTag("library.album")[0].performClick()
            awaitTag("detail.header")
            settle()
            shot(dir, "$prefix-02-album")
        }

        step {
            // The mini player half-way through a skip swipe: the next song sliding in.
            halfSwipe("miniPlayer", fraction = 0.3f) { shot(dir, "$prefix-03-mini-swipe-next-peek") }
        }

        step {
            compose.onNodeWithTag("miniPlayer.info").performSemanticsAction(SemanticsActions.OnClick)
            awaitTag("player.playPause")
            settle()
            shot(dir, "$prefix-04-player-artwork-local")
        }

        step {
            halfSwipe("player.artwork", fraction = 0.45f) { shot(dir, "$prefix-05-player-swipe-next-peek") }
        }

        step {
            click("player.title")
            awaitTag("player.about")
            settle()
            shot(dir, "$prefix-06-info-sheet")
            dismissSheet()
        }

        step {
            click("player.more")
            awaitTag("songMenu.header")
            settle()
            shot(dir, "$prefix-07-song-menu")
            dismissSheet()
        }

        step {
            click("player.switchQueue")
            awaitTag("queueSwitcher.sheet")
            settle()
            shot(dir, "$prefix-08-queue-selector")
            dismissSheet()
        }

        step {
            // Dragging along the stars: the rating follows the finger before it is committed.
            val stars = compose.onNodeWithTag("rating").fetchSemanticsNode().boundsInRoot
            compose.onNodeWithTag("rating").performTouchInput {
                down(Offset(4f, height / 2f))
                moveBy(Offset(stars.width * 0.15f, 0f))
                moveBy(Offset(stars.width * 0.45f, 0f))
            }
            compose.mainClock.advanceTimeBy(300)
            compose.waitForIdle()
            shot(dir, "$prefix-09-stars-dragging")
            compose.onNodeWithTag("rating").performTouchInput { cancel() }
            compose.waitForIdle()
        }

        step {
            core.client.dispatch(Commands.setSetting(SettingKeys.DISPLAY_PLAYER_RATING, "false"))
            settle()
            shot(dir, "$prefix-10-player-stars-hidden")
            click("player.mode.queue")
            settle()
            shot(dir, "$prefix-10b-queue-stars-hidden")
            click("player.mode.queue")
            settle()
            core.client.dispatch(Commands.setSetting(SettingKeys.DISPLAY_PLAYER_RATING, "true"))
            settle()
        }

        step {
            click("player.mode.queue")
            settle()
            shot(dir, "$prefix-11-queue")
        }

        step {
            // The lyrics page animates forever: step the clock rather than waiting for idle.
            compose.mainClock.autoAdvance = false
            click("player.mode.lyrics")
            repeat(60) { compose.mainClock.advanceTimeByFrame() }
            shot(dir, "$prefix-12-lyrics")
            // The lyrics' timing, from the song menu while they show.
            click("player.more")
            repeat(60) { compose.mainClock.advanceTimeByFrame() }
            click("player.lyricsTiming")
            // The sheet's window animates in on the looper's clock as well as the test's.
            repeat(30) {
                compose.mainClock.advanceTimeByFrame()
                org.robolectric.Shadows.shadowOf(android.os.Looper.getMainLooper()).idleFor(java.time.Duration.ofMillis(50))
            }
            shot(dir, "$prefix-12b-lyrics-timing-sheet")
            dismissSheet(frames = true)
            click("player.mode.lyrics")
            repeat(60) { compose.mainClock.advanceTimeByFrame() }
            compose.mainClock.autoAdvance = true
            settle()
        }

        step {
            // Another device plays: "Playing on Laptop" and the accent device button.
            core.client.dispatch(Commands.handoffTo("laptop"))
            core.client.requestSnapshot()
            compose.waitUntil(5_000) { !core.client.ownsTransport.value }
            // Let the "Now playing on Laptop" toast go first.
            compose.mainClock.advanceTimeBy(8_000)
            settle()
            shot(dir, "$prefix-13-player-artwork-remote")
        }
    }

    /** Puts a finger down on [tag], drags it left by [fraction] of its width, runs [whileHeld], lets go. */
    private fun halfSwipe(tag: String, fraction: Float, whileHeld: () -> Unit) {
        val width = compose.onNodeWithTag(tag).fetchSemanticsNode().size.width
        compose.onNodeWithTag(tag).performTouchInput {
            down(center)
            moveBy(Offset(-viewConfiguration.touchSlop - 1f, 0f))
            repeat(6) { moveBy(Offset(-width * fraction / 6f, 0f)) }
        }
        compose.mainClock.advanceTimeBy(100)
        compose.waitForIdle()
        whileHeld()
        compose.onNodeWithTag(tag).performTouchInput { cancel() }
        settle()
    }

    @Suppress("UNCHECKED_CAST")
    private fun rootViews(): List<android.view.View> {
        val global = Class.forName("android.view.WindowManagerGlobal").getMethod("getInstance").invoke(null)
        val field = global.javaClass.getDeclaredField("mViews").apply { isAccessible = true }
        return (field.get(global) as List<android.view.View>).toList()
    }

    /** Closes the sheet on top with its own back handling (the activity's dispatcher would not reach it). */
    private fun dismissSheet(frames: Boolean = false) {
        val views = rootViews().filter { it !== compose.activity.window.decorView }
        views.lastOrNull()?.let { v ->
            val down = android.view.KeyEvent(android.view.KeyEvent.ACTION_DOWN, android.view.KeyEvent.KEYCODE_BACK)
            val up = android.view.KeyEvent(android.view.KeyEvent.ACTION_UP, android.view.KeyEvent.KEYCODE_BACK)
            compose.runOnUiThread { v.dispatchKeyEvent(down); v.dispatchKeyEvent(up) }
        }
        if (frames) repeat(60) { compose.mainClock.advanceTimeByFrame() } else settle()
    }

    /** A semantics click, its state handed to composition even while the clock is stepped by hand. */
    private fun click(tag: String) {
        compose.onNodeWithTag(tag).performSemanticsAction(SemanticsActions.OnClick)
        compose.runOnUiThread { androidx.compose.runtime.snapshots.Snapshot.sendApplyNotifications() }
    }

    private fun back() {
        compose.activityRule.scenario.onActivity { it.onBackPressedDispatcher.onBackPressed() }
        settle()
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

    /**
     * The activity's window, with every other window (a sheet is a window of its own) drawn over it.
     * Robolectric's capture draws a sheet's window blank, so those are drawn here with the view's
     * own draw, which renders Compose's layers under Robolectric's native graphics.
     */
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
