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
import androidx.compose.ui.test.onRoot
import androidx.compose.ui.test.performSemanticsAction
import androidx.compose.ui.test.performTouchInput
import androidx.test.ext.junit.runners.AndroidJUnit4
import app.hocket.core.ArtworkSizes
import app.hocket.core.CoreHandle
import app.hocket.core.Commands
import app.hocket.core.NativeCore
import app.hocket.core.api.Query
import app.hocket.core.api.QueryResult
import app.hocket.core.client.CoreClient
import app.hocket.core.fake.FakeCore
import app.hocket.core.fake.FakeLibrary
import app.hocket.ui.components.ArtworkPathCache
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

/**
 * The immersive player in motion, with real covers and the real classifier, for review by eye (#40):
 * artwork mode, a half swipe with the next song's cover peeking in, and the dismiss drag as a frame
 * sequence (sheet progress 0.9, 0.7, 0.5, 0.3). Not an assertion test; it runs only with
 *
 *     HOCKET_SCREENSHOT_DIR=/tmp/shots        where the PNGs go
 *     HOCKET_COVERS=/path/to/covers           cover images; the first four (by name) are played as
 *                                             pairs: 2nd then 1st, 1st then 2nd, 3rd then 4th
 *     HOCKET_HOST_LIB=target/debug/libhocket_android.so   the core built for the host
 *
 * (and HOCKET_SCREENSHOT_THEME=light for light mode; dark otherwise). SDK 32 for the same reason
 * as [ImmersiveScreenshotTest].
 */
@RunWith(AndroidJUnit4::class)
@Config(sdk = [32], qualifiers = "w411dp-h891dp-night-xxhdpi")
@GraphicsMode(GraphicsMode.Mode.NATIVE)
class ImmersiveMotionScreenshotTest {
    @get:Rule
    val compose = createAndroidComposeRule<ComponentActivity>()

    private fun env(name: String): String? = (System.getProperty(name) ?: System.getenv(name))?.takeIf { it.isNotBlank() }

    /** The fake core, except that album `i`'s artwork is cover `i`. */
    private class CoverHandle(private val fake: FakeCore, private val cover: (String) -> File?) : CoreHandle by fake {
        override suspend fun query(query: Query): QueryResult =
            if (query is Query.Artwork) QueryResult.Path(cover(query.data.id)?.absolutePath) else fake.query(query)
    }

    @Test
    fun motion() = run(tall = false)

    /**
     * Swipes between a card and an immersive cover (each way, and immersive to immersive), held at
     * a quarter, half and three quarters: each page in its own style, the backdrop and colours
     * following the swipe, and the controls never moving.
     */
    @Test
    fun styleSwipes() = run(tall = false, styles = true)

    /** A tall phone: the immersive artwork stays at the top, the controls at the bottom, the continuation between. */
    @Test
    @Config(qualifiers = "w411dp-h960dp-night-xxhdpi")
    fun tall() = run(tall = true)

    private fun run(tall: Boolean, styles: Boolean = false) {
        val dir = env("HOCKET_SCREENSHOT_DIR")?.let(::File)
        val covers = env("HOCKET_COVERS")?.let { File(it).listFiles()?.filter { f -> f.isFile }?.sortedBy { f -> f.name } }.orEmpty()
        val lib = env("HOCKET_HOST_LIB")?.let(::File)?.takeIf { it.exists() }
        assumeTrue("set HOCKET_SCREENSHOT_DIR, HOCKET_COVERS (4 covers) and HOCKET_HOST_LIB", dir != null && lib != null && covers.size >= 4)
        System.setProperty("uniffi.component.hocket_android.libraryOverride", lib!!.absolutePath)
        assumeTrue("host library not loadable on this JVM", NativeCore.isAvailable())
        dir!!.mkdirs()
        val theme = if (env("HOCKET_SCREENSHOT_THEME") == "light") "light" else "dark"
        if (theme == "light") org.robolectric.RuntimeEnvironment.setQualifiers("+notnight")
        val now = { System.currentTimeMillis().toDouble() }
        val fake = FakeCore(seed = 7, library = FakeLibrary(7).apply {
            for (i in tracks.indices) tracks[i] = tracks[i].copy(title = "Song Name", artist = "Artist", album = "Album")
            for (i in albums.indices) albums[i] = albums[i].copy(name = "Album", artist = "Artist")
        }, startWithServer = true, startPlaying = true, timers = false, now = now, dispatcher = Dispatchers.Unconfined)
        val albums = fake.library.albums
        val byAlbum = albums.withIndex().associate { (i, a) -> a.id to covers.getOrNull(i) }
        for (a in albums) for (size in listOf(ArtworkSizes.THUMB, ArtworkSizes.LIST, ArtworkSizes.GRID, ArtworkSizes.FULL)) ArtworkPathCache.remove(ArtworkPathCache.key(a.id, size))
        val client = CoreClient(CoverHandle(fake) { id -> byAlbum[id] }, CoroutineScope(SupervisorJob() + Dispatchers.Main.immediate), now)
        compose.setContent {
            HocketTheme(client = client) {
                CompositionLocalProvider(LocalCoreClient provides client, LocalFakeCoreBanner provides false) { AppRoot(client) }
            }
        }
        client.requestSnapshot()
        compose.waitUntil(5_000) { client.started.value }
        compose.waitUntil(5_000) { compose.onAllNodesWithTag("miniPlayer").fetchSemanticsNodes().isNotEmpty() }
        kotlinx.coroutines.runBlocking {
            val context = compose.activity
            for (cover in covers.take(4)) coil3.SingletonImageLoader.get(context).execute(coil3.request.ImageRequest.Builder(context).data(cover).size(ArtworkSizes.FULL).build())
        }
        // What the classifier made of each cover (named for the style it was drawn to get).
        kotlinx.coroutines.runBlocking {
            for (cover in covers.take(4)) {
                val layout = app.hocket.ui.player.ImmersiveArtwork.analyse(cover.absolutePath, app.hocket.core.api.ImmersiveArtwork.Automatic, dark = theme == "dark")
                println("classifier: ${cover.name} -> ${layout?.bottom?.style} (${layout?.bottom?.reason})")
            }
        }
        fun track(album: Int) = fake.library.albumTracks(albums[album].id).first().id
        fun name(i: Int) = covers[i].nameWithoutExtension
        if (styles) {
            // d (a card) -> b (immersive), b -> d, and b -> a (immersive to immersive).
            val controls = listOf("player.title", "player.addToPlaylist", "player.seekBar", "player.playPause", "player.mode.queue", "player.more")
            for ((current, next) in listOf(3 to 1, 1 to 3, 1 to 0)) {
                client.dispatch(Commands.setAutoplay(false))
                client.dispatch(Commands.playTracks(fake.library.serverId, listOf(track(current), track(next)), 0, "Pair"))
                settle()
                compose.onNodeWithTag("nowPlaying.sheet").performSemanticsAction(SemanticsActions.Expand)
                settle()
                awaitStill()
                val prefix = "android-$theme-style-${name(current)}-to-${name(next)}"
                shot(File(dir, "$prefix-0.00.png"))
                val rest = controls.associateWith { compose.onNodeWithTag(it).fetchSemanticsNode().boundsInRoot }
                val pageWidth = compose.onRoot().fetchSemanticsNode().size.width.toFloat()
                var moved = 0f
                compose.onNodeWithTag("player.artwork").performTouchInput {
                    down(Offset(width * 0.8f, height * 0.4f))
                    moveBy(Offset(-viewConfiguration.touchSlop - 1f, 0f))
                }
                for (f in listOf(0.25f, 0.5f, 0.75f)) {
                    val to = f * pageWidth
                    compose.onNodeWithTag("player.artwork").performTouchInput { repeat(4) { moveBy(Offset(-(to - moved) / 4f, 0f)) } }
                    moved = to
                    idle()
                    shot(File(dir, "$prefix-%.2f.png".format(f)))
                    controls.forEach { tag ->
                        val now = compose.onNodeWithTag(tag).fetchSemanticsNode().boundsInRoot
                        check(now == rest.getValue(tag)) { "$tag moved at $f of $prefix: ${rest.getValue(tag)} -> $now" }
                    }
                }
                if (current == 3) {
                    // Released at three quarters: it settles on the immersive song and hands over
                    // without a jump (frames every 48 ms, then at rest).
                    compose.mainClock.autoAdvance = false
                    compose.onNodeWithTag("player.artwork").performTouchInput { up() }
                    for (i in 1..6) {
                        compose.mainClock.advanceTimeBy(48)
                        org.robolectric.Shadows.shadowOf(android.os.Looper.getMainLooper()).idleFor(java.time.Duration.ofMillis(48))
                        shot(File(dir, "$prefix-release-$i.png"))
                    }
                    compose.mainClock.autoAdvance = true
                    settle()
                    shot(File(dir, "$prefix-committed.png"))
                    controls.filter { it != "player.title" }.forEach { tag ->
                        val now = compose.onNodeWithTag(tag).fetchSemanticsNode().boundsInRoot
                        check(now == rest.getValue(tag)) { "$tag moved after the commit of $prefix: ${rest.getValue(tag)} -> $now" }
                    }
                } else {
                    compose.onNodeWithTag("player.artwork").performTouchInput { cancel() }
                    settle()
                }
                compose.onNodeWithTag("nowPlaying.sheet").performSemanticsAction(SemanticsActions.Collapse)
                settle()
            }
            return
        }
        for ((current, next) in listOf(1 to 0, 0 to 1, 2 to 3)) {
            client.dispatch(Commands.setAutoplay(false))
            client.dispatch(Commands.playTracks(fake.library.serverId, listOf(track(current), track(next)), 0, "Pair"))
            settle()
            val collapsedTop = sheetTop()
            compose.onNodeWithTag("nowPlaying.sheet").performSemanticsAction(SemanticsActions.Expand)
            settle()
            val prefix = "android-$theme-immersive-${name(current)}"
            // The cover is decoded and crossfaded in on a clock the test steps by hand: wait for the
            // frame to stop changing, or the capture can catch the artwork's empty background.
            awaitStill()
            if (tall) {
                shot(File(dir, "$prefix-artwork-w411dp-h960dp.png"))
                return
            }
            shot(File(dir, "$prefix-artwork.png"))
            if (current == 1) {
                // The queue on its panel, the continuation fading away under it.
                compose.onNodeWithTag("player.mode.queue").performSemanticsAction(SemanticsActions.OnClick)
                settle()
                shot(File(dir, "$prefix-queue.png"))
                compose.onNodeWithTag("player.mode.queue").performSemanticsAction(SemanticsActions.OnClick)
                settle()
            }
            // Half a swipe: the next song's cover (and its continuation) sliding in.
            val width = compose.onRoot().fetchSemanticsNode().size.width
            compose.onNodeWithTag("player.artwork").performTouchInput {
                down(Offset(width * 0.7f, height * 0.4f))
                moveBy(Offset(-viewConfiguration.touchSlop - 1f, 0f))
                repeat(6) { moveBy(Offset(-width * 0.4f / 6f, 0f)) }
            }
            idle()
            shot(File(dir, "$prefix-swipe-peek-${name(next)}.png"))
            compose.onNodeWithTag("player.artwork").performTouchInput { cancel() }
            settle()
            // The dismiss drag, held at each progress.
            compose.onNodeWithTag("player.page").performTouchInput {
                down(Offset(centerX, height * 0.55f))
                moveBy(Offset(0f, viewConfiguration.touchSlop + 1f))
            }
            for (p in listOf(0.9f, 0.7f, 0.5f, 0.3f)) {
                repeat(6) {
                    val target = (1f - p) * collapsedTop
                    val delta = target - sheetTop()
                    if (kotlin.math.abs(delta) > 1f) compose.onNodeWithTag("nowPlaying.sheet").performTouchInput { moveBy(Offset(0f, delta)) }
                    idle()
                }
                shot(File(dir, "$prefix-dismiss-%.1f.png".format(p)))
            }
            compose.onNodeWithTag("nowPlaying.sheet").performTouchInput { up() }
            settle()
            if (sheetTop() < collapsedTop / 2f) {
                compose.onNodeWithTag("nowPlaying.sheet").performSemanticsAction(SemanticsActions.Collapse)
                settle()
            }
        }
    }

    /**
     * Settles until the artwork's part of the screen (the top third: the seek bar's clock below it
     * ticks on) is the same picture in two captures in a row, for a few seconds at most.
     */
    private fun awaitStill() {
        fun top(): Bitmap = compose.onRoot().captureToImage().asAndroidBitmap().let { Bitmap.createBitmap(it, 0, 0, it.width, it.height / 3) }
        settle()
        settle()
        var last = top()
        repeat(8) {
            settle()
            val now = top()
            if (now.sameAs(last)) return
            last = now
        }
    }

    private fun sheetTop(): Float = compose.onNodeWithTag("nowPlaying.sheet").fetchSemanticsNode().boundsInRoot.top

    private fun idle() {
        org.robolectric.Shadows.shadowOf(android.os.Looper.getMainLooper()).idle()
        compose.waitForIdle()
    }

    /** Decoding and classifying run off the main thread: let them land, then let the UI settle. */
    private fun settle() {
        repeat(4) {
            Thread.sleep(300)
            org.robolectric.Shadows.shadowOf(android.os.Looper.getMainLooper()).idleFor(java.time.Duration.ofMillis(400))
            compose.mainClock.advanceTimeBy(600)
            compose.waitForIdle()
        }
    }

    private fun shot(file: File) {
        val bitmap = compose.onRoot().captureToImage().asAndroidBitmap()
        file.outputStream().use { bitmap.compress(Bitmap.CompressFormat.PNG, 100, it) }
        println("screenshot: ${file.absolutePath}")
    }
}
