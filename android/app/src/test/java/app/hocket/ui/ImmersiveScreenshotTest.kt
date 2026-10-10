package app.hocket.ui

import android.graphics.Bitmap
import androidx.activity.ComponentActivity
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.ui.graphics.asAndroidBitmap
import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.test.captureToImage
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onRoot
import androidx.compose.ui.test.performSemanticsAction
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
import app.hocket.ui.theme.HocketTheme
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import org.junit.Assume.assumeTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.ParameterizedRobolectricTestRunner
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode
import java.io.File

/**
 * The full player with real covers and the real immersive-artwork classifier, for reviewing
 * `hocket_core::artwork` by eye. Not an assertion test: it runs only with all three of
 *
 *     HOCKET_SCREENSHOT_DIR=/tmp/shots        where the PNGs go
 *     HOCKET_COVERS=/path/to/covers           a folder of cover images (any names)
 *     HOCKET_HOST_LIB=target/debug/libhocket_android.so   the core built for the host
 *
 * and plays the fake library's albums one by one, each wearing the next cover, a batch of covers
 * per test case. SDK 32: on 33+ the UniFFI glue needs a cleaner Robolectric cannot shadow (so the
 * backdrop is the still blur, not the moving one).
 */
@RunWith(ParameterizedRobolectricTestRunner::class)
@Config(sdk = [32], qualifiers = "w411dp-h891dp-night-xxhdpi")
@GraphicsMode(GraphicsMode.Mode.NATIVE)
class ImmersiveScreenshotTest(private val batch: Int) {
    @get:Rule
    val compose = createAndroidComposeRule<ComponentActivity>()

    companion object {
        private fun env(name: String): String? = (System.getProperty(name) ?: System.getenv(name))?.takeIf { it.isNotBlank() }
        private val outDir = env("HOCKET_SCREENSHOT_DIR")?.let(::File)
        private val covers = env("HOCKET_COVERS")?.let { File(it).listFiles()?.filter { f -> f.isFile }?.sortedBy { f -> f.name } }.orEmpty()
        private val lib = env("HOCKET_HOST_LIB")?.let(::File)?.takeIf { it.exists() }
        private const val SEED = 7L
        private val albumCount = FakeLibrary(SEED).albums.size

        @JvmStatic
        @ParameterizedRobolectricTestRunner.Parameters(name = "batch {0}")
        fun batches(): List<Array<Any>> {
            val n = if (outDir == null || lib == null || covers.isEmpty()) 1 else (covers.size + albumCount - 1) / albumCount
            return (0 until n).map { arrayOf<Any>(it) }
        }
    }

    /** The fake core, except that album `i`'s artwork is cover `offset + i`. */
    private class CoverHandle(private val fake: FakeCore, private val cover: (String) -> File?) : CoreHandle by fake {
        override suspend fun query(query: Query): QueryResult =
            if (query is Query.Artwork) QueryResult.Path(cover(query.data.id)?.absolutePath) else fake.query(query)
    }

    @Test
    fun player() {
        val dir = outDir
        assumeTrue("set HOCKET_SCREENSHOT_DIR, HOCKET_COVERS and HOCKET_HOST_LIB", dir != null && lib != null && covers.isNotEmpty())
        System.setProperty("uniffi.component.hocket_android.libraryOverride", lib!!.absolutePath)
        assumeTrue("host library not loadable on this JVM", NativeCore.isAvailable())
        dir!!.mkdirs()
        val now = { System.currentTimeMillis().toDouble() }
        val fake = FakeCore(seed = SEED, startWithServer = true, startPlaying = true, timers = false, now = now, dispatcher = Dispatchers.Unconfined)
        val albums = fake.library.albums
        val offset = batch * albumCount
        val byAlbum = albums.withIndex().associate { (i, a) -> a.id to covers.getOrNull(offset + i) }
        // The path cache outlives a test case: forget the previous batch's covers.
        for (a in albums) for (size in listOf(ArtworkSizes.THUMB, ArtworkSizes.LIST, ArtworkSizes.GRID, ArtworkSizes.FULL)) ArtworkPathCache.remove(ArtworkPathCache.key(a.id, size))
        val client = CoreClient(CoverHandle(fake) { id -> byAlbum[id] }, CoroutineScope(SupervisorJob() + Dispatchers.Main.immediate), now)
        compose.setContent {
            HocketTheme(client = client) {
                CompositionLocalProvider(LocalCoreClient provides client) { AppRoot(client) }
            }
        }
        client.requestSnapshot()
        compose.waitUntil(5_000) { client.started.value }
        awaitTag("miniPlayer")
        compose.onNodeWithTag("miniPlayer.info").performSemanticsAction(SemanticsActions.OnClick)
        awaitTag("player.playPause")
        for ((i, album) in albums.withIndex()) {
            val cover = covers.getOrNull(offset + i) ?: break
            // Load the cover into the image loader's memory first: a cache hit draws at once,
            // where a fresh load crossfades on a clock the test only advances by hand.
            kotlinx.coroutines.runBlocking {
                val context = compose.activity
                coil3.SingletonImageLoader.get(context).execute(coil3.request.ImageRequest.Builder(context).data(cover).size(ArtworkSizes.FULL).build())
            }
            client.dispatch(Commands.playContext(Commands.albumContext(fake.library.serverId, album.id, album.name)))
            // Decoding and classifying run off the main thread: let them land, then let the UI settle.
            // Robolectric's SystemClock stands still unless advanced, and the image loader's
            // crossfade runs on it: advance it too, or new artwork stays transparent.
            repeat(4) {
                Thread.sleep(350)
                org.robolectric.Shadows.shadowOf(android.os.Looper.getMainLooper()).idleFor(java.time.Duration.ofMillis(400))
                compose.mainClock.advanceTimeBy(600)
                compose.waitForIdle()
            }
            val full = compose.onRoot().captureToImage().asAndroidBitmap()
            val small = Bitmap.createScaledBitmap(full, full.width / 3, full.height / 3, true)
            File(dir, "%03d-%s.png".format(offset + i, cover.nameWithoutExtension)).outputStream().use { small.compress(Bitmap.CompressFormat.PNG, 100, it) }
        }
    }

    private fun awaitTag(tag: String) {
        compose.waitUntil(5_000) { runCatching { compose.onNodeWithTag(tag).assertExists() }.isSuccess }
        compose.waitForIdle()
    }
}
