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
import app.hocket.core.ArtworkSizes
import app.hocket.ui.components.ArtworkPathCache
import app.hocket.ui.player.HandoffSheet
import app.hocket.ui.player.PlayerAbout
import app.hocket.ui.player.QueuesSheet
import app.hocket.ui.screens.detail.AlbumDetailScreen
import app.hocket.ui.screens.detail.ArtistDetailScreen
import app.hocket.ui.screens.detail.PlaylistDetailScreen
import app.hocket.ui.screens.filters.FilterBuilderScreen
import app.hocket.ui.screens.filters.FiltersScreen
import app.hocket.ui.screens.setup.ServerSetupScreen
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performTextInput
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
 * Renders the redesigned screens (Queues, Play on, Settings, Search, album / artist / playlist in
 * each header mode, Song info's tabs, Downloads, Stats, Filters and the rule sheet, Setup's steps)
 * to PNGs on the fake core, for review against
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

    @Test @Config(qualifiers = "+notnight") fun queuesLight() = capture("light-queues", sheet = true) { androidx.compose.foundation.layout.Box(Modifier.fillMaxSize().background(MaterialTheme.colorScheme.background)) { QueuesSheet(onDismiss = {}) } }
    @Test @Config(qualifiers = "+night") fun queuesDark() = capture("dark-queues", sheet = true) { androidx.compose.foundation.layout.Box(Modifier.fillMaxSize().background(MaterialTheme.colorScheme.background)) { QueuesSheet(onDismiss = {}) } }

    @Test @Config(qualifiers = "+notnight")
    fun playOn() = capture("light-play-on", sheet = true) {
        // The sheet is a window of its own: the activity behind it needs a size to capture.
        androidx.compose.foundation.layout.Box(Modifier.fillMaxSize().background(MaterialTheme.colorScheme.background)) { HandoffSheet(onDismiss = {}, onOpenSettings = {}) }
    }

    @Test @Config(qualifiers = "+notnight") fun settingsLight() = capture("light-settings") { SettingsScreen(rememberNavController()) }
    @Test @Config(qualifiers = "+night") fun settingsDark() = capture("dark-settings") { SettingsScreen(rememberNavController()) }

    @Test @Config(qualifiers = "+notnight")
    fun searchRecent() {
        RecentSearchPrefs(compose.activity).apply { clear(); listOf("night", "saltwater hymns", "ada velour", "lantern").forEach { remember(it) } }
        capture("light-search-recent") { SearchScreen(rememberNavController()) }
    }

    @Test @Config(qualifiers = "+notnight")
    fun searchResults() {
        RecentSearchPrefs(compose.activity).clear()
        capture("light-search-results", after = {
            compose.onNodeWithTag("search.input").performTextInput("a")
            repeat(3) { compose.mainClock.advanceTimeBy(1_000); compose.waitForIdle() }
        }) { SearchScreen(rememberNavController()) }
    }

    /** An artist with every kind of release, so each shelf shows. */
    private fun richArtist(core: TestCore): app.hocket.core.api.Artist {
        val lib = core.fake.library
        val artist = lib.artists.maxBy { a -> lib.artistAlbums(a.id).size }
        lib.artistAlbums(artist.id).forEachIndexed { i, al ->
            val types = when (i % 4) { 1 -> listOf("ep"); 2 -> listOf("single"); 3 -> listOf("compilation"); else -> listOf("album") }
            lib.albums[lib.albums.indexOf(al)] = al.copy(releaseTypes = types)
        }
        return artist
    }

    @Test @Config(qualifiers = "+notnight")
    fun artistShelves() {
        val core = TestCore(startPlaying = true)
        core.fake.artworkFile = art(640, core)
        val artist = richArtist(core)
        capture("light-artist", core = core) { ArtistDetailScreen(rememberNavController(), artist.id) }
        // Down to the shelves.
        repeat(2) { compose.onAllNodes(hasScrollAction())[0].performTouchInput { swipeUp(startY = bottom * 0.9f, endY = top + 10f) } }
        repeat(2) { compose.mainClock.advanceTimeBy(1_500); compose.waitForIdle() }
        shot(outDir!!, "android-light-artist-shelves", false)
    }

    @Test @Config(qualifiers = "+notnight")
    fun albumSmall() {
        val core = TestCore(startPlaying = true)
        core.fake.artworkFile = art(640, core)
        val album = core.fake.library.albums.maxBy { it.songCount }
        capture("light-album-small", core = core) { AlbumDetailScreen(rememberNavController(), album.id) }
    }

    @Test @Config(qualifiers = "+notnight")
    fun albumLarge() {
        val core = TestCore(startPlaying = true)
        core.fake.artworkFile = art(1300, core)
        val album = core.fake.library.albums.maxBy { it.songCount }
        capture("light-album-large", core = core) { AlbumDetailScreen(rememberNavController(), album.id) }
    }

    @Test @Config(qualifiers = "+notnight")
    fun albumNone() {
        val core = TestCore(startPlaying = true)
        val lib = core.fake.library
        val album = lib.albums.maxBy { it.songCount }
        lib.albums[lib.albums.indexOf(album)] = album.copy(coverArt = null)
        capture("light-album-none", core = core) { AlbumDetailScreen(rememberNavController(), album.id) }
    }

    @Test @Config(qualifiers = "+notnight")
    fun playlist() {
        val core = TestCore(startPlaying = true)
        core.fake.artworkFile = art(640, core)
        val pl = core.fake.library.playlists.first { !it.isSmart }
        capture("light-playlist", core = core) { PlaylistDetailScreen(rememberNavController(), pl.id) }
    }

    @Test @Config(qualifiers = "+notnight")
    fun songInfo() {
        val core = TestCore(startPlaying = true)
        core.fake.artworkFile = art(640, core)
        capture("light-song-info", core = core) {
            Surface(Modifier.fillMaxSize(), color = MaterialTheme.colorScheme.surfaceContainerLow) { PlayerAbout(Modifier.fillMaxSize(), onOpenAlbum = {}, onOpenArtist = {}, onClose = {}) }
        }
        listOf(1 to "about", 2 to "file").forEach { (tab, name) ->
            compose.onNodeWithTag("player.about.tab.$tab").performClick()
            repeat(2) { settle() }
            shot(outDir!!, "android-light-song-info-$name", false)
        }
    }

    @Test @Config(qualifiers = "+notnight") fun downloads() = capture("light-downloads") { DownloadsScreen(rememberNavController()) }

    @Test @Config(qualifiers = "+notnight")
    fun stats() = capture("light-stats", waitForText = "By hour of day") { StatsScreen(rememberNavController()) }

    @Test @Config(qualifiers = "+notnight") fun filters() = capture("light-filters") { FiltersScreen(rememberNavController()) }

    @Test @Config(qualifiers = "+notnight")
    fun filterBuilder() {
        val core = TestCore(startPlaying = true)
        capture("light-filter-builder", core = core) { FilterBuilderScreen(rememberNavController(), "f-loved") }
        compose.onAllNodesWithTag("filter.rule")[0].performClick()
        repeat(2) { compose.mainClock.advanceTimeBy(1_000); compose.waitForIdle() }
        shot(outDir!!, "android-light-filter-rule", true)
    }

    @Test @Config(qualifiers = "+notnight")
    fun setup() {
        val core = TestCore(startWithServer = false, startPlaying = false)
        capture("light-setup", core = core) { ServerSetupScreen(existing = null) }
        compose.onNodeWithTag("setup.start").performClick()
        repeat(2) { compose.mainClock.advanceTimeBy(1_000); compose.waitForIdle() }
        shot(outDir!!, "android-light-setup-form", false)
    }

    @Test @Config(qualifiers = "+notnight")
    fun setupSummary() {
        val core = TestCore(startWithServer = true, startPlaying = false)
        capture("light-setup-summary", core = core) {
            val server by core.client.server.collectAsState()
            ServerSetupScreen(existing = server, summary = server != null)
        }
    }

    /**
     * A square cover [px] wide (a warm sky, a sun and two hills, like the boards' Halfway Home),
     * written once, answered for every artwork request.
     */
    private var coreForArt: TestCore? = null

    private fun art(px: Int, core: TestCore? = null): (String, Int) -> String? {
        coreForArt = core
        val file = File(compose.activity.cacheDir, "cover-$px.png")
        if (!file.exists()) {
            val bmp = Bitmap.createBitmap(px, px, Bitmap.Config.ARGB_8888)
            val c = android.graphics.Canvas(bmp)
            val p = android.graphics.Paint(android.graphics.Paint.ANTI_ALIAS_FLAG)
            p.shader = android.graphics.LinearGradient(0f, 0f, 0f, px.toFloat(), intArrayOf(0xFFE7A27C.toInt(), 0xFFD8936E.toInt(), 0xFFD27FB5.toInt(), 0xFF8C4F86.toInt()), floatArrayOf(0f, 0.36f, 0.66f, 1f), android.graphics.Shader.TileMode.CLAMP)
            c.drawRect(0f, 0f, px.toFloat(), px.toFloat(), p)
            p.shader = null
            p.color = 0xFFFCE3B8.toInt(); c.drawCircle(px * 0.62f, px * 0.35f, px * 0.12f, p)
            p.color = 0xFF8A4A7E.toInt(); c.drawOval(-0.15f * px, 0.73f * px, 0.62f * px, 1.25f * px, p)
            p.color = 0xFF6B3B6E.toInt(); c.drawOval(0.38f * px, 0.77f * px, 1.23f * px, 1.3f * px, p)
            file.outputStream().use { bmp.compress(Bitmap.CompressFormat.PNG, 100, it) }
        }
        // The path cache outlives a test case: forget the other cases' files.
        val lib = coreForArt?.fake?.library
        if (lib != null) for (id in lib.albums.mapNotNull { it.coverArt } + lib.artists.mapNotNull { it.coverArt } + lib.playlists.mapNotNull { it.coverArt } + lib.tracks.mapNotNull { it.coverArt })
            for (size in listOf(ArtworkSizes.THUMB, ArtworkSizes.LIST, ArtworkSizes.GRID, ArtworkSizes.FULL, ArtworkSizes.HERO)) ArtworkPathCache.remove(ArtworkPathCache.key(id, size))
        return { _, _ -> file.absolutePath }
    }

    private fun capture(name: String, core: TestCore = TestCore(startPlaying = true), sheet: Boolean = false, scrollTag: String? = null, waitForText: String? = null, after: () -> Unit = {}, content: @androidx.compose.runtime.Composable () -> Unit) {
        val dir = outDir
        assumeTrue("set HOCKET_SCREENSHOT_DIR (or -Dhocket.screenshotDir) to write screenshots", dir != null)
        dir!!.mkdirs()
        compose.setThemedContent(core) { content() }
        core.start()
        compose.waitUntil(5_000) { core.client.started.value }
        // Robolectric's wallpaper palette is not a real one: seed the scheme from the boards' indigo
        // instead, so the shots compare with the designs colour for colour.
        core.client.dispatch(app.hocket.core.Commands.setSetting(app.hocket.core.SettingKeys.DISPLAY_ACCENT, "\"#4A5F99\""))
        // Real time too: artwork decodes (coil, the header's size check) run on background threads.
        repeat(4) { settle() }
        if (waitForText != null) compose.waitUntil(10_000) { compose.onAllNodesWithText(waitForText).fetchSemanticsNodes().isNotEmpty() }
        if (scrollTag != null) compose.waitUntil(5_000) { compose.onAllNodesWithTag(scrollTag).fetchSemanticsNodes().isNotEmpty() }
        after()
        shot(dir, "android-$name", sheet)
    }

    /**
     * Lets background work land: artwork decodes and the header's size check run off the main
     * thread, and the image loader's crossfade runs on Robolectric's SystemClock, which stands still
     * unless the looper is idled forward.
     */
    private fun settle() {
        Thread.sleep(350)
        org.robolectric.Shadows.shadowOf(android.os.Looper.getMainLooper()).idleFor(java.time.Duration.ofMillis(400))
        compose.mainClock.advanceTimeBy(1_500)
        compose.waitForIdle()
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
