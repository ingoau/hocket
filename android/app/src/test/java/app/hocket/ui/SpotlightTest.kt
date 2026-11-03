package app.hocket.ui

import android.graphics.Bitmap
import androidx.activity.ComponentActivity
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.ui.graphics.asAndroidBitmap
import androidx.compose.ui.test.captureToImage
import androidx.compose.ui.test.isRoot
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.test.onAllNodesWithTag
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performTouchInput
import androidx.compose.ui.test.swipeLeft
import androidx.test.ext.junit.runners.AndroidJUnit4
import app.hocket.core.ArtworkSizes
import app.hocket.core.Queries
import app.hocket.core.toSummary
import app.hocket.core.api.AlbumResumePoint
import app.hocket.core.api.QueryResult
import app.hocket.ui.components.ArtworkPathCache
import app.hocket.ui.nav.AppRoot
import app.hocket.ui.nav.LocalFakeCoreBanner
import app.hocket.ui.screens.home.SPOTLIGHT_MAX
import app.hocket.ui.screens.home.SlideKind
import app.hocket.ui.screens.home.loadSpotlight
import kotlinx.coroutines.runBlocking
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Assume.assumeTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode
import java.io.File

/**
 * Home's Spotlight: the slides' rules on the fake core (always run), and screenshots of the
 * carousel in both themes when an output directory is set:
 *
 *     HOCKET_SCREENSHOT_DIR=/tmp/shots ./gradlew :app:testDebugUnitTest --tests '*SpotlightTest*' --rerun
 */
@RunWith(AndroidJUnit4::class)
@Config(sdk = [35], qualifiers = "w411dp-h891dp-xxhdpi")
@GraphicsMode(GraphicsMode.Mode.NATIVE)
class SpotlightTest {
    @get:Rule
    val compose = createAndroidComposeRule<ComponentActivity>()

    private val outDir: File? = (System.getProperty("hocket.screenshotDir") ?: System.getenv("HOCKET_SCREENSHOT_DIR"))
        ?.takeIf { it.isNotBlank() }?.let(::File)

    @Test
    fun slidesResumeFirstNeverRepeatAndSkipWhatsPlaying() = runBlocking {
        val core = TestCore(startPlaying = true)
        val lib = core.fake.library
        val playing = lib.tracks[3]
        val resumed = lib.albums.first { it.id != playing.albumId && it.artistId != playing.artistId }
        val track = lib.albumTracks(resumed.id)[1]
        core.fake.albumResumes[resumed.id] = AlbumResumePoint(resumed.id, track.id, 1u, track.durationMs / 3u, track.durationMs, 1.0)
        // The playing album's own point is never offered: it's what's on.
        playing.albumId?.let { id -> core.fake.albumResumes[id] = AlbumResumePoint(id, playing.id, 0u, 1000u, playing.durationMs, 2.0) }
        core.start()
        val serverId = core.client.server.value!!.id
        val stats = (core.client.query(Queries.stats(365)) as? QueryResult.Stats)?.data
        val resumes = core.fake.albumResumes.values.toList()
        for (seed in 0 until 20) {
            val slides = loadSpotlight(core.client, serverId, resumes, playing.toSummary(), stats, seed)
            assertTrue("seed $seed: ${slides.size} slides", slides.size in 2..SPOTLIGHT_MAX)
            assertEquals(SlideKind.Resume, slides.first().kind)
            assertEquals(resumed.id, slides.first().album?.id)
            val albums = slides.mapNotNull { it.album?.id }
            val artists = slides.mapNotNull { it.album?.artistId ?: it.artist?.id }
            assertEquals("seed $seed albums $albums", albums.distinct(), albums)
            assertEquals("seed $seed artists $artists", artists.distinct(), artists)
            assertTrue(playing.albumId !in albums)
            assertTrue(playing.artistId !in artists)
        }
        // With nothing to resume there's no Resume slide.
        val none = loadSpotlight(core.client, serverId, emptyList(), null, stats, 1)
        assertTrue(none.none { it.kind == SlideKind.Resume })
    }

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
        val lib = core.fake.library
        val playing = lib.tracks[3]
        val resumed = lib.albums.first { it.id != playing.albumId && it.artistId != playing.artistId }
        val track = lib.albumTracks(resumed.id)[3]
        core.fake.albumResumes[resumed.id] = AlbumResumePoint(resumed.id, track.id, 3u, track.durationMs / 3u, track.durationMs, 1.0)
        // A different painted cover per album; the resumed one nearly white, to show a pale cover
        // keeps the slide dark in the dark theme.
        val covers = Cover.entries.associateWith { coverFile(it) }
        val ids = lib.albums.mapNotNull { it.coverArt } + lib.artists.mapNotNull { it.coverArt } + lib.playlists.mapNotNull { it.coverArt } + lib.tracks.mapNotNull { it.coverArt }
        for (id in ids) for (size in listOf(ArtworkSizes.THUMB, ArtworkSizes.LIST, ArtworkSizes.GRID, ArtworkSizes.FULL, ArtworkSizes.HERO)) ArtworkPathCache.remove(ArtworkPathCache.key(id, size))
        core.fake.artworkFile = { id, _ ->
            val cover = if (id == resumed.coverArt) Cover.Pale else Cover.entries.filter { it != Cover.Pale }[Math.floorMod(id.hashCode(), Cover.entries.size - 1)]
            covers.getValue(cover).absolutePath
        }
        // Into the image loader's memory first: a cache hit draws at once, where a fresh decode
        // crossfades on a clock the test only advances by hand.
        runBlocking {
            val context = compose.activity
            // Robolectric's ImageDecoder can't decode these ("Only supported on Android"): decode with BitmapFactory.
            coil3.SingletonImageLoader.setUnsafe(coil3.ImageLoader.Builder(context).components { add(coil3.decode.BitmapFactoryDecoder.Factory()) }.build())
            for (file in covers.values) for (size in listOf(ArtworkSizes.THUMB, ArtworkSizes.LIST, ArtworkSizes.GRID, ArtworkSizes.FULL, ArtworkSizes.HERO))
                coil3.SingletonImageLoader.get(context).execute(coil3.request.ImageRequest.Builder(context).data(file).size(size).build())
        }
        compose.setThemedContent(core) { CompositionLocalProvider(LocalFakeCoreBanner provides false) { AppRoot(core.client) } }
        core.start()
        compose.waitUntil(5_000) { core.client.started.value }
        awaitTag("home.spotlight")
        realSettle()
        shot(dir, "$prefix-spotlight-1")
        repeat(2) { i ->
            compose.onNodeWithTag("home.spotlight").performTouchInput { swipeLeft() }
            realSettle()
            shot(dir, "$prefix-spotlight-${i + 2}")
        }
        if (prefix == "dark") {
            compose.onNodeWithTag("home.customise").performClick()
            awaitTag("home.customiseSheet")
            realSettle()
            shot(dir, "$prefix-spotlight-customise")
        }
    }

    private enum class Cover { Sunset, Ocean, Forest, Pale, Plum }

    /** A square painted cover: a sky in [cover]'s colours, a sun and a horizon. */
    private fun coverFile(cover: Cover): File {
        val px = 640
        val file = File(compose.activity.cacheDir, "spotlight-${cover.name}.png")
        val (sky, sun, ground) = when (cover) {
            Cover.Sunset -> Triple(intArrayOf(0xFF0E2233.toInt(), 0xFF3B5566.toInt(), 0xFFE0955A.toInt(), 0xFF5A3A28.toInt()), 0xFFFFF1C9.toInt(), 0xFF2A1B14.toInt())
            Cover.Ocean -> Triple(intArrayOf(0xFF0B1F4A.toInt(), 0xFF1F5FA8.toInt(), 0xFF5BB6D9.toInt(), 0xFF0E3B66.toInt()), 0xFFE8F6FF.toInt(), 0xFF082440.toInt())
            Cover.Forest -> Triple(intArrayOf(0xFF1C3324.toInt(), 0xFF3F6B45.toInt(), 0xFF9CC48A.toInt(), 0xFF24402A.toInt()), 0xFFF4F7D0.toInt(), 0xFF13241A.toInt())
            Cover.Pale -> Triple(intArrayOf(0xFFFFFFFF.toInt(), 0xFFF7F3EE.toInt(), 0xFFF1E6DA.toInt(), 0xFFE9DCCF.toInt()), 0xFFE58C5A.toInt(), 0xFFD9C7B5.toInt())
            Cover.Plum -> Triple(intArrayOf(0xFF2B0F33.toInt(), 0xFF6A2C70.toInt(), 0xFFD27FB5.toInt(), 0xFF3C1840.toInt()), 0xFFFCE3B8.toInt(), 0xFF2A0F2E.toInt())
        }
        val bmp = Bitmap.createBitmap(px, px, Bitmap.Config.ARGB_8888)
        val c = android.graphics.Canvas(bmp)
        val p = android.graphics.Paint(android.graphics.Paint.ANTI_ALIAS_FLAG)
        p.shader = android.graphics.LinearGradient(0f, 0f, 0f, px * 0.7f, sky, floatArrayOf(0f, 0.4f, 0.85f, 1f), android.graphics.Shader.TileMode.CLAMP)
        c.drawRect(0f, 0f, px.toFloat(), px.toFloat(), p)
        p.shader = null
        p.color = sun; c.drawCircle(px * 0.66f, px * 0.5f, px * 0.1f, p)
        p.color = ground; c.drawRect(0f, px * 0.7f, px.toFloat(), px.toFloat(), p)
        file.outputStream().use { bmp.compress(Bitmap.CompressFormat.PNG, 100, it) }
        return file
    }

    /** Artwork decodes and palettes run on background threads: give them real time too. */
    private fun realSettle() {
        repeat(4) {
            Thread.sleep(350)
            org.robolectric.Shadows.shadowOf(android.os.Looper.getMainLooper()).idleFor(java.time.Duration.ofMillis(400))
            compose.mainClock.advanceTimeBy(1_000)
            compose.waitForIdle()
        }
    }

    private fun awaitTag(tag: String) {
        compose.waitUntil(10_000) { compose.onAllNodesWithTag(tag).fetchSemanticsNodes().isNotEmpty() }
        compose.waitForIdle()
    }

    @Suppress("UNCHECKED_CAST")
    private fun rootViews(): List<android.view.View> {
        val global = Class.forName("android.view.WindowManagerGlobal").getMethod("getInstance").invoke(null)
        val field = global.javaClass.getDeclaredField("mViews").apply { isAccessible = true }
        return (field.get(global) as List<android.view.View>).toList()
    }

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
