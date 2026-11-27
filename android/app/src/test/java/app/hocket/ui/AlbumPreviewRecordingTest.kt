package app.hocket.ui

import android.graphics.Bitmap
import android.graphics.Canvas
import android.graphics.Paint
import androidx.activity.ComponentActivity
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.asAndroidBitmap
import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.test.captureToImage
import androidx.compose.ui.test.hasAnyAncestor
import androidx.compose.ui.test.hasTestTag
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.test.onAllNodesWithTag
import androidx.compose.ui.test.onFirst
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onRoot
import androidx.compose.ui.test.performScrollToNode
import androidx.compose.ui.test.performSemanticsAction
import androidx.compose.ui.test.performTouchInput
import androidx.test.ext.junit.runners.AndroidJUnit4
import app.hocket.core.ArtworkSizes
import app.hocket.core.CoreHandle
import app.hocket.core.api.Query
import app.hocket.core.api.QueryResult
import app.hocket.core.client.CoreClient
import app.hocket.core.fake.FakeCore
import app.hocket.ui.nav.AppRoot
import app.hocket.ui.nav.LocalFakeCoreBanner
import app.hocket.ui.theme.HocketTheme
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import org.junit.Assume.assumeTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode
import java.io.File
import java.io.FileOutputStream

/**
 * A frame-by-frame recording (one frame per 16 ms) of the album preview in the whole app, a song
 * playing in the mini player: a pull down from the top of an album page brings the cover to the
 * middle, two swipes move the preview on a song each, a drag on the seek bar, pause and play, then
 * a swipe down puts the cover back and the song that was playing carries on (and, in
 * [immersiveSwipeDown], the same over a cover the page shows edge to edge). The finger is drawn as a
 * dot. For review by eye (stitch with ffmpeg at 60 fps); only runs with an output directory, and
 * wears real covers with HOCKET_COVERS (a folder of images, one per album in turn). SDK 32, as
 * ImmersiveScreenshotTest: covers don't decode under Robolectric's SDK 35.
 *
 * The classifier doesn't run here: HOCKET_LAYOUTS (a JSON object from a cover's file name to its
 * `ArtworkLayout`, as the `artwork_layout` example prints them) hands in its answers for the covers.
 *
 *     HOCKET_SCREENSHOT_DIR=/tmp/rec HOCKET_COVERS=/tmp/covers HOCKET_LAYOUTS=/tmp/layouts.json \
 *         ./gradlew :app:testDebugUnitTest --tests '*AlbumPreviewRecordingTest*' --rerun
 */
@RunWith(AndroidJUnit4::class)
@Config(sdk = [32], application = android.app.Application::class, qualifiers = "w411dp-h891dp-xhdpi")
@GraphicsMode(GraphicsMode.Mode.NATIVE)
class AlbumPreviewRecordingTest {
    @get:Rule
    val compose = createAndroidComposeRule<ComponentActivity>()

    private fun env(name: String): String? = (System.getProperty(name) ?: System.getenv(name))?.takeIf { it.isNotBlank() }
    private val outDir: File? = env("HOCKET_SCREENSHOT_DIR")?.let(::File)
    private val covers = env("HOCKET_COVERS")?.let { File(it).listFiles()?.filter { f -> f.isFile }?.sortedBy { f -> f.name } }.orEmpty()
    private val layouts: Map<String, app.hocket.core.api.ArtworkLayout> = env("HOCKET_LAYOUTS")?.let { path ->
        val all = app.hocket.core.HocketJson.json.parseToJsonElement(File(path).readText()) as kotlinx.serialization.json.JsonObject
        all.mapValues { (_, v) -> app.hocket.core.HocketJson.json.decodeFromJsonElement(app.hocket.core.api.ArtworkLayout.serializer(), v) }
    }.orEmpty()

    /** The fake core, with album `i` wearing cover `i` (round the folder). */
    private class CoverHandle(private val fake: FakeCore, private val cover: (String) -> File?) : CoreHandle by fake {
        override suspend fun query(query: Query): QueryResult =
            if (query is Query.Artwork) QueryResult.Path(cover(query.data.id)?.absolutePath) else fake.query(query)
    }

    private fun exists(tag: String) = compose.onAllNodesWithTag(tag).fetchSemanticsNodes().isNotEmpty()

    private var n = 0
    private var layoutsForCovers: ((String) -> app.hocket.core.api.ArtworkLayout?)? = null

    @org.junit.After
    fun forgetLayouts() {
        app.hocket.ui.player.ImmersiveArtwork.layoutsForTest = null
    }
    private var finger: Offset? = null
    private lateinit var dir: File

    @Test
    fun pullSwipeSeekAndPutAway() {
        openAlbum("album-preview")
        // Pull: slowly at first, past the threshold, then let go.
        val art = bounds("detail.artwork")
        val from = Offset(art.center.x, art.top + art.height * 0.55f)
        drag(from, from + Offset(0f, 190f * density()), 48, hold = 6)
        frames(70)
        // Two songs on, a swipe each.
        val card = bounds("albumPreview.pager")
        repeat(2) {
            drag(Offset(card.width * 0.72f, card.center.y), Offset(card.width * 0.26f, card.center.y), 18)
            frames(60)
        }
        // Scrub the preview further in.
        val seek = compose.onNode(hasTestTag("player.seek") and hasAnyAncestor(hasTestTag("albumPreview")), useUnmergedTree = true).fetchSemanticsNode().boundsInRoot
        drag(Offset(seek.left + seek.width * 0.42f, seek.center.y), Offset(seek.left + seek.width * 0.78f, seek.center.y), 24, hold = 6)
        frames(50)
        // Pause, then play.
        val toggle = compose.onNode(hasTestTag("player.playPause") and hasAnyAncestor(hasTestTag("albumPreview")), useUnmergedTree = true).fetchSemanticsNode().boundsInRoot.center
        tap(toggle, after = 40)
        tap(toggle, after = 50)
        // Swipe the card down: it follows the finger, then flies home, and the song that was playing carries on.
        val c = bounds("albumPreview.pager").center
        drag(c, c + Offset(0f, 200f * density()), 30)
        frames(70)
    }

    /**
     * An album whose cover the page shows immersive (edge to edge from the top, carried on in its
     * colour): the cover pulled out of the header, a swipe on, then swiped down and home again.
     * The classifier's answer is handed in (a flat edge, as the core finds for these covers' plain
     * foot), as FullPlayerLayoutTest does: the native classifier doesn't run here.
     */
    /**
     * Pulled out, a song on, then dragged down to the bottom of the screen and let go there: the
     * album plays from that song, the card dropping into the mini player as it comes back.
     */
    @Test
    fun dragToPlay() {
        openAlbum("album-preview-drag-to-play")
        val art = bounds("detail.artwork")
        val from = Offset(art.center.x, art.top + art.height * 0.55f)
        drag(from, from + Offset(0f, 190f * density()), 40, hold = 4)
        frames(60)
        val card = bounds("albumPreview.pager")
        drag(Offset(card.width * 0.72f, card.center.y), Offset(card.width * 0.26f, card.center.y), 18)
        frames(50)
        // Down past the dismissal, on to the bottom, held there, and let go.
        val c = bounds("albumPreview.pager").center
        val bottom = compose.onRoot().fetchSemanticsNode().boundsInRoot.bottom
        drag(c, Offset(c.x, bottom - 70f * density()), 56, hold = 14)
        frames(80)
    }

    /** Pulled out, then Play from here: the card drops into the mini player. */
    @Test
    fun playFromHereButton() {
        openAlbum("album-preview-play-from-here")
        val art = bounds("detail.artwork")
        val from = Offset(art.center.x, art.top + art.height * 0.55f)
        drag(from, from + Offset(0f, 190f * density()), 40, hold = 4)
        frames(60)
        tap(bounds("albumPreview.playFrom").center, after = 80)
    }

    @Test
    fun immersiveSwipeDown() {
        val metrics = app.hocket.core.api.ArtworkMetrics(flat = 0.95, drift = 0.02, rough = 0.02, marks = 0u, busy = 0.02, faces = 0u)
        val edge = app.hocket.core.api.ArtworkEdge(style = app.hocket.core.api.ArtworkStyle.Extend, edgeColors = List(8) { 0x3A1C71u }, baseColor = 0x3A1C71u,
            light = false, scrim = 0.2, reason = "extend:flat", metrics = metrics)
        val handed = app.hocket.core.api.ArtworkLayout(bottom = edge, right = edge, left = edge, top = edge, topLight = false, topMarks = false)
        app.hocket.ui.player.ImmersiveArtwork.layoutsForTest = { id -> layoutsForCovers?.invoke(id) ?: handed }
        try {
            openAlbum("album-preview-immersive")
            // Pull from the cover at the top of the screen, past the threshold, and let go.
            val art = bounds("detail.artwork")
            val from = Offset(art.center.x, art.top + art.height * 0.45f)
            drag(from, from + Offset(0f, 200f * density()), 48, hold = 6)
            frames(70)
            // One song on.
            val card = bounds("albumPreview.pager")
            drag(Offset(card.width * 0.72f, card.center.y), Offset(card.width * 0.26f, card.center.y), 18)
            frames(60)
            // A little way down and let go: it springs back.
            val c = bounds("albumPreview.pager").center
            drag(c, c + Offset(0f, 60f * density()), 18, hold = 4)
            frames(40)
            // Down past the threshold, slowly, held, and let go: it flies home into the header.
            drag(c, c + Offset(0f, 220f * density()), 40, hold = 8)
            frames(70)
        } finally {
            app.hocket.ui.player.ImmersiveArtwork.layoutsForTest = null
        }
    }

    /** The whole app with a song playing, on the library's first album, the clock then by hand. */
    private fun openAlbum(name: String) {
        val out = outDir
        assumeTrue("set HOCKET_SCREENSHOT_DIR to write recording frames", out != null)
        dir = File(out!!, name).apply { mkdirs() }
        // One clock for the fake's samples and the UI's extrapolation: the test's frames, so a
        // preview advances 16 ms a frame however long a frame takes to draw.
        val t0 = System.currentTimeMillis().toDouble()
        val now = { t0 + compose.mainClock.currentTime }
        val fake = FakeCore(seed = 7, startWithServer = true, startPlaying = true, timers = false, now = now, dispatcher = Dispatchers.Unconfined)
        val byAlbum = fake.library.albums.withIndex().associate { (i, a) -> a.id to covers.takeIf { it.isNotEmpty() }?.let { it[i % it.size] } }
        // The classifier's answers for these covers, when handed in (a card for a cover it has none for).
        if (layouts.isNotEmpty()) {
            layoutsForCovers = { id -> byAlbum[id]?.name?.let(layouts::get) }
            if (app.hocket.ui.player.ImmersiveArtwork.layoutsForTest == null) app.hocket.ui.player.ImmersiveArtwork.layoutsForTest = layoutsForCovers
        }
        val client = CoreClient(CoverHandle(fake) { id -> byAlbum[id] }, CoroutineScope(SupervisorJob() + Dispatchers.Main.immediate), now)
        // Covers in the image loader's memory first: a hit draws at once, where a load crossfades on a clock the test advances by hand.
        kotlinx.coroutines.runBlocking {
            val context = compose.activity
            for (cover in covers) for (size in listOf(ArtworkSizes.GRID, ArtworkSizes.FULL, ArtworkSizes.HERO)) {
                coil3.SingletonImageLoader.get(context).execute(coil3.request.ImageRequest.Builder(context).data(cover).size(size).build())
            }
        }
        compose.setContent {
            HocketTheme(client = client) {
                CompositionLocalProvider(LocalCoreClient provides client, LocalFakeCoreBanner provides false) { AppRoot(client) }
            }
        }
        client.requestSnapshot()
        compose.waitUntil(5_000) { client.started.value && exists("home.screen") }
        compose.onNodeWithTag("navBar.library").performSemanticsAction(SemanticsActions.OnClick)
        compose.waitUntil(5_000) { exists("library.grid") }
        compose.onNodeWithTag("library.grid").performScrollToNode(hasTestTag("library.album"))
        compose.waitUntil(5_000) { exists("library.album") }
        settle()
        compose.onAllNodesWithTag("library.album").onFirst().performSemanticsAction(SemanticsActions.OnClick)
        compose.waitUntil(5_000) { exists("detail.header") }
        settle()

        // From here the clock goes by hand, a frame at a time.
        compose.mainClock.autoAdvance = false
        frames(30)
    }

    /** Real time for decoding and classifying off the main thread, and the test's clock for the UI to land them. */
    private fun settle() {
        repeat(3) {
            Thread.sleep(200)
            org.robolectric.Shadows.shadowOf(android.os.Looper.getMainLooper()).idleFor(java.time.Duration.ofMillis(400))
            compose.mainClock.advanceTimeBy(600)
            compose.waitForIdle()
        }
    }

    private fun density() = compose.activity.resources.displayMetrics.density
    private fun bounds(tag: String) = compose.onNodeWithTag(tag, useUnmergedTree = true).fetchSemanticsNode().boundsInRoot

    private fun tap(at: Offset, after: Int) {
        finger = at
        compose.onRoot().performTouchInput { down(at) }
        frames(5)
        compose.onRoot().performTouchInput { up() }
        finger = null
        frames(after)
    }

    /** A finger drag over [count] frames, eased in and out, held still for [hold] frames before letting go. */
    private fun drag(from: Offset, to: Offset, count: Int, hold: Int = 2) {
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
        frames(hold)
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
