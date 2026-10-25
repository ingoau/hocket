package app.hocket.ui

import android.graphics.Bitmap
import androidx.activity.ComponentActivity
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.asAndroidBitmap
import androidx.compose.ui.test.captureToImage
import androidx.compose.ui.test.hasScrollAction
import androidx.compose.ui.test.isRoot
import androidx.compose.ui.test.performTouchInput
import androidx.compose.ui.test.swipeUp
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.test.onAllNodesWithTag
import androidx.compose.ui.test.onAllNodesWithText
import androidx.navigation.compose.rememberNavController
import androidx.test.ext.junit.runners.AndroidJUnit4
import app.hocket.RecentSearchPrefs
import app.hocket.ui.player.HandoffSheet
import app.hocket.ui.player.PlayerAbout
import app.hocket.ui.queue.SavedQueuesScreen
import app.hocket.ui.screens.detail.ArtistDetailScreen
import app.hocket.ui.screens.downloads.DownloadsScreen
import app.hocket.ui.screens.search.SearchScreen
import app.hocket.ui.screens.settings.SettingsScreen
import app.hocket.ui.screens.stats.StatsScreen
import org.junit.Assume.assumeTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode
import java.io.File

/**
 * Renders the redesigned screens (Queues, Play on, Settings, Search's recent searches, an artist's
 * discography shelves, Song info, Downloads, Stats) to PNGs on the fake core, for review against
 * the final designs. Not an assertion test: it only runs with an output directory, e.g.
 *
 *     HOCKET_SCREENSHOT_DIR=/tmp/shots ./gradlew :app:testDebugUnitTest --tests '*RedesignScreenshotTest*' --rerun
 */
@RunWith(AndroidJUnit4::class)
@Config(sdk = [35], qualifiers = "w411dp-h891dp-xxhdpi")
@GraphicsMode(GraphicsMode.Mode.NATIVE)
class RedesignScreenshotTest {
    @get:Rule
    val compose = createAndroidComposeRule<ComponentActivity>()

    private val outDir: File? = (System.getProperty("hocket.screenshotDir") ?: System.getenv("HOCKET_SCREENSHOT_DIR"))
        ?.takeIf { it.isNotBlank() }?.let(::File)

    @Test @Config(qualifiers = "+notnight") fun queuesLight() = capture("light-queues") { SavedQueuesScreen(rememberNavController()) }
    @Test @Config(qualifiers = "+night") fun queuesDark() = capture("dark-queues") { SavedQueuesScreen(rememberNavController()) }

    @Test @Config(qualifiers = "+notnight")
    fun playOn() = capture("light-play-on", sheet = true) {
        // The sheet is a window of its own: the activity behind it needs a size to capture.
        androidx.compose.foundation.layout.Box(Modifier.fillMaxSize().background(MaterialTheme.colorScheme.background)) { HandoffSheet(onDismiss = {}, onOpenSettings = {}) }
    }

    @Test @Config(qualifiers = "+notnight") fun settingsLight() = capture("light-settings") { SettingsScreen(rememberNavController()) }
    @Test @Config(qualifiers = "+night") fun settingsDark() = capture("dark-settings") { SettingsScreen(rememberNavController()) }

    @Test @Config(qualifiers = "+notnight")
    fun searchRecent() {
        RecentSearchPrefs(compose.activity).apply { clear(); listOf("saltwater hymns", "ada velour", "night", "lantern").forEach { remember(it) } }
        capture("light-search-recent") { SearchScreen(rememberNavController()) }
    }

    @Test @Config(qualifiers = "+notnight")
    fun artistShelves() {
        val core = TestCore(startPlaying = false)
        val lib = core.fake.library
        // An artist with every kind of release, so each shelf shows.
        val artist = lib.artists.maxBy { a -> lib.artistAlbums(a.id).size }
        lib.artistAlbums(artist.id).forEachIndexed { i, al ->
            val types = when (i % 4) { 1 -> listOf("ep"); 2 -> listOf("single"); 3 -> listOf("compilation"); else -> listOf("album") }
            lib.albums[lib.albums.indexOf(al)] = al.copy(releaseTypes = types)
        }
        capture("light-artist", core = core) { ArtistDetailScreen(rememberNavController(), artist.id) }
        // Down to the shelves.
        repeat(2) { compose.onAllNodes(hasScrollAction())[0].performTouchInput { swipeUp(startY = bottom * 0.9f, endY = top + 10f) } }
        repeat(2) { compose.mainClock.advanceTimeBy(1_500); compose.waitForIdle() }
        shot(outDir!!, "android-light-artist-shelves", false)
    }

    @Test @Config(qualifiers = "+notnight")
    fun songInfo() {
        val core = TestCore(startPlaying = true)
        capture("light-song-info", core = core, scrollTag = null) {
            Surface(Modifier.fillMaxSize(), color = MaterialTheme.colorScheme.surfaceContainerLow) { PlayerAbout(Modifier.fillMaxSize()) }
        }
    }

    @Test @Config(qualifiers = "+notnight") fun downloads() = capture("light-downloads") { DownloadsScreen(rememberNavController()) }

    @Test @Config(qualifiers = "+notnight")
    fun stats() = capture("light-stats", waitForText = "By hour of day") { StatsScreen(rememberNavController()) }

    private fun capture(name: String, core: TestCore = TestCore(startPlaying = true), sheet: Boolean = false, scrollTag: String? = null, waitForText: String? = null, content: @androidx.compose.runtime.Composable () -> Unit) {
        val dir = outDir
        assumeTrue("set HOCKET_SCREENSHOT_DIR (or -Dhocket.screenshotDir) to write screenshots", dir != null)
        dir!!.mkdirs()
        compose.setThemedContent(core) { content() }
        core.start()
        compose.waitUntil(5_000) { core.client.started.value }
        repeat(3) { compose.mainClock.advanceTimeBy(1_500); compose.waitForIdle() }
        if (waitForText != null) compose.waitUntil(10_000) { compose.onAllNodesWithText(waitForText).fetchSemanticsNodes().isNotEmpty() }
        if (scrollTag != null) compose.waitUntil(5_000) { compose.onAllNodesWithTag(scrollTag).fetchSemanticsNodes().isNotEmpty() }
        shot(dir, "android-$name", sheet)
    }

    private fun rootViews(): List<android.view.View> {
        val global = Class.forName("android.view.WindowManagerGlobal").getMethod("getInstance").invoke(null)
        val field = global.javaClass.getDeclaredField("mViews").apply { isAccessible = true }
        @Suppress("UNCHECKED_CAST")
        return (field.get(global) as List<android.view.View>).toList()
    }

    /** The activity's window with any sheet window drawn over it (Robolectric captures sheets blank). */
    private fun shot(dir: File, name: String, sheet: Boolean) {
        val bitmap = compose.onAllNodes(isRoot())[0].captureToImage().asAndroidBitmap().copy(Bitmap.Config.ARGB_8888, true)
        if (sheet) {
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
        }
        val file = File(dir, "$name.png")
        file.outputStream().use { bitmap.compress(Bitmap.CompressFormat.PNG, 100, it) }
        println("screenshot: ${file.absolutePath}")
    }
}
