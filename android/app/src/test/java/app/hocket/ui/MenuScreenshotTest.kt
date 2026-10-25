package app.hocket.ui

import android.graphics.Bitmap
import androidx.activity.ComponentActivity
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.graphics.asAndroidBitmap
import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.test.captureToImage
import androidx.compose.ui.test.isRoot
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.test.longClick
import androidx.compose.ui.test.onAllNodesWithContentDescription
import androidx.compose.ui.test.onAllNodesWithTag
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.performSemanticsAction
import androidx.compose.ui.test.performTextInput
import androidx.compose.ui.test.performTextClearance
import androidx.compose.ui.test.performTouchInput
import androidx.test.ext.junit.runners.AndroidJUnit4
import app.hocket.core.ActionIds
import app.hocket.core.Commands
import app.hocket.core.SettingKeys
import app.hocket.core.client.SelectionKind
import app.hocket.ui.components.ActionSheet
import org.junit.Assume.assumeTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode
import java.io.File

/**
 * Renders the song menu (compact and expanded), the album menu, the selected songs' menu and the
 * add to playlist sheet (default, searching, the remove-copies dialog, the hold menu, pending
 * changes) over the "Late night" playlist on the fake core. Only runs with an output directory:
 *
 *     HOCKET_SCREENSHOT_DIR=/tmp/shots ./gradlew :app:testDebugUnitTest --tests '*MenuScreenshotTest*' --rerun
 */
@RunWith(AndroidJUnit4::class)
@Config(sdk = [35], qualifiers = "w411dp-h891dp-xxhdpi")
@GraphicsMode(GraphicsMode.Mode.NATIVE)
class MenuScreenshotTest {
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

    private var albumMenu by mutableStateOf<String?>(null)

    private fun capture(prefix: String) {
        val dir = outDir
        assumeTrue("set HOCKET_SCREENSHOT_DIR (or -Dhocket.screenshotDir) to write screenshots", dir != null)
        dir!!.mkdirs()
        val core = TestCore(startPlaying = false)
        val lib = core.fake.library
        core.start()
        compose.waitUntil(5_000) { core.client.started.value }
        // Late night's first song: loved, rated 4, in Late night once and Sunday morning twice.
        val trackId = lib.playlistTracks.getValue("pl0").first()
        core.client.dispatch(Commands.loveTrack(trackId, true))
        core.client.dispatch(Commands.rateTrack(trackId, 4))
        core.client.dispatch(Commands.setSetting(SettingKeys.LIBRARY_PINS, """[{"kind":"playlist","id":"pl0","tap":"open"},{"kind":"playlist","id":"pl6","tap":"open"}]"""))
        val title = lib.track(trackId)!!.title
        compose.setThemedContent(core) {
            CompositionLocalProvider(LocalDetailNavigator provides DetailNavigator({}, {}, {})) {
                app.hocket.ui.screens.detail.PlaylistDetailScreen(androidx.navigation.compose.rememberNavController(), "pl0")
                albumMenu?.let { id -> ActionSheet(Commands.albums(listOf(id)), "", null, onDismiss = { albumMenu = null }) }
            }
        }
        compose.waitUntil(5_000) { compose.onAllNodesWithContentDescription("More options for $title").fetchSemanticsNodes().isNotEmpty() }
        settle()

        fun openSongMenu() {
            compose.onAllNodesWithContentDescription("More options for $title")[0].performSemanticsAction(SemanticsActions.OnClick)
            awaitTag("songMenu.header")
            settle()
        }
        step {
            openSongMenu()
            shot(dir, "$prefix-01-song-menu")
            compose.onNodeWithTag("songMenu.share").performSemanticsAction(SemanticsActions.OnClick)
            awaitTag("songMenu.share.navidrome")
            settle()
            shot(dir, "$prefix-01c-share")
            compose.onNodeWithTag("songMenu.share.back").performSemanticsAction(SemanticsActions.OnClick)
            settle()
            // Held part way through a swipe up: the rest opening with the finger, More folding away.
            compose.mainClock.autoAdvance = false
            compose.onNodeWithTag("songMenu.header").performTouchInput { down(center); repeat(4) { moveBy(androidx.compose.ui.geometry.Offset(0f, -40f)) } }
            compose.mainClock.advanceTimeBy(100)
            shot(dir, "$prefix-01b-song-menu-swiping")
            compose.onNodeWithTag("songMenu.header").performTouchInput { up() }
            compose.mainClock.autoAdvance = true
            settle()
            compose.onNodeWithTag("songMenu.more").performSemanticsAction(SemanticsActions.OnClick)
            settle()
            shot(dir, "$prefix-02-song-menu-expanded")
            dismissSheet()
        }
        step {
            albumMenu = lib.track(trackId)!!.albumId
            awaitTag("itemMenu.header")
            settle()
            shot(dir, "$prefix-03-album-menu")
            compose.onNodeWithTag("itemMenu.share").performSemanticsAction(SemanticsActions.OnClick)
            awaitTag("itemMenu.share.songLink")
            settle()
            shot(dir, "$prefix-03b-album-share")
            compose.onNodeWithTag("itemMenu.share.back").performSemanticsAction(SemanticsActions.OnClick)
            settle()
            dismissSheet()
            albumMenu = null
            settle()
        }
        step {
            openSongMenu()
            compose.onNodeWithTag("songMenu.action.${ActionIds.ADD_TO_PLAYLIST}").performSemanticsAction(SemanticsActions.OnClick)
            awaitTag("picker.row.pl2")
            settle()
            shot(dir, "$prefix-04-add-to-playlist")
            compose.onNodeWithTag("picker.search").performTextInput("in")
            settle()
            shot(dir, "$prefix-05-add-to-playlist-search")
            compose.onNodeWithTag("picker.search").performTextClearance()
            compose.onNodeWithTag("picker.search").performTextInput("zzz")
            settle()
            shot(dir, "$prefix-05b-add-to-playlist-no-match")
            compose.onNodeWithTag("picker.search").performTextClearance()
            settle()
            compose.onNodeWithTag("picker.row.pl2").performSemanticsAction(SemanticsActions.OnClick)
            awaitTag("picker.removeOne")
            settle()
            shot(dir, "$prefix-06-remove-copies-dialog")
            compose.onNodeWithTag("picker.removeOne").performSemanticsAction(SemanticsActions.OnClick)
            settle()
            compose.onNodeWithTag("picker.row.pl3").performTouchInput { longClick() }
            awaitTag("picker.menu.addCopy")
            settle()
            shot(dir, "$prefix-07-hold-menu")
            compose.onNodeWithTag("picker.menu.addCopy").performSemanticsAction(SemanticsActions.OnClick)
            settle()
            shot(dir, "$prefix-08-add-to-playlist-pending")
            dismissSheet()
        }
        step {
            val ids = lib.playlistTracks.getValue("pl0").take(3)
            compose.runOnIdle { ids.forEach { core.client.toggleSelected(SelectionKind.Tracks, it) } }
            awaitTag("selection.more")
            compose.onNodeWithTag("selection.more").performSemanticsAction(SemanticsActions.OnClick)
            awaitTag("selectionMenu.header")
            settle()
            shot(dir, "$prefix-09-multi-select-menu")
            compose.onNodeWithTag("selectionMenu.more").performSemanticsAction(SemanticsActions.OnClick)
            settle()
            shot(dir, "$prefix-09b-multi-select-menu-expanded")
            dismissSheet()
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

    /** Closes the window on top with its own back handling. */
    private fun dismissSheet() {
        rootViews().filter { it !== compose.activity.window.decorView }.lastOrNull()?.let { v ->
            val down = android.view.KeyEvent(android.view.KeyEvent.ACTION_DOWN, android.view.KeyEvent.KEYCODE_BACK)
            val up = android.view.KeyEvent(android.view.KeyEvent.ACTION_UP, android.view.KeyEvent.KEYCODE_BACK)
            compose.runOnUiThread { v.dispatchKeyEvent(down); v.dispatchKeyEvent(up) }
        }
        settle()
    }

    /** The activity's window with every other window (sheets, dialogs, menus) drawn over it. */
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
