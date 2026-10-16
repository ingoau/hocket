package app.hocket.ui

import android.graphics.Bitmap
import android.graphics.Canvas
import androidx.activity.ComponentActivity
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.ui.graphics.asAndroidBitmap
import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.test.captureToImage
import androidx.compose.ui.test.isRoot
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performSemanticsAction
import app.hocket.core.Commands
import app.hocket.core.CoreHandle
import app.hocket.core.api.LyricsTier
import app.hocket.core.api.Query
import app.hocket.core.api.QueryResult
import app.hocket.core.client.CoreClient
import app.hocket.core.fake.FakeCore
import app.hocket.ui.nav.AppRoot
import app.hocket.ui.theme.HocketTheme
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import org.junit.Assume.assumeTrue
import org.junit.Rule
import org.junit.Test
import org.junit.rules.TestRule
import org.junit.runners.model.Statement
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode
import java.io.File

/**
 * The lyrics share screen in the full player, for review by eye: the transition frame by frame
 * (`share-frame-NNN.png`, enough for a video) and the card with each option. Not an assertion test:
 * it runs only with `HOCKET_SCREENSHOT_DIR`; `HOCKET_COVERS` (its first image) gives every album a
 * real cover instead of the placeholder. Gradle's configuration cache keeps the environment of the
 * run that stored it, so run it with `--no-configuration-cache`.
 *
 * SDK 32 (so the backdrop is the still blur): at 35 Robolectric's native graphics aborts the JVM in
 * this test every time ("JniConstants: Class not found: java/nio/FloatBuffer"), and at 32 now and
 * then; a rerun gets through. The share card's recording layer is the likely trigger.
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [32], qualifiers = "w411dp-h891dp-night-xxhdpi")
@GraphicsMode(GraphicsMode.Mode.NATIVE)
class LyricsShareScreenshotTest {
    private fun env(name: String): String? = (System.getProperty(name) ?: System.getenv(name))?.takeIf { it.isNotBlank() }

    /** Skips before the activity starts: skipping after left its decoders running as the JVM exited. */
    @get:Rule(order = 0)
    val needsDir = TestRule { base, _ ->
        object : Statement() {
            override fun evaluate() {
                assumeTrue("set HOCKET_SCREENSHOT_DIR", env("HOCKET_SCREENSHOT_DIR") != null)
                base.evaluate()
            }
        }
    }

    @get:Rule(order = 1)
    val compose = createAndroidComposeRule<ComponentActivity>()

    /** The fake core, every artwork answered with [cover] when there is one. */
    private class CoverHandle(private val fake: FakeCore, private val cover: File?) : CoreHandle by fake {
        override suspend fun query(query: Query): QueryResult =
            if (query is Query.Artwork && cover != null) QueryResult.Path(cover.absolutePath) else fake.query(query)
    }

    @Test
    fun shareScreen() {
        val dir = File(env("HOCKET_SCREENSHOT_DIR")!!)
        dir.mkdirs()
        val cover = env("HOCKET_COVERS")?.let { File(it).listFiles()?.filter { f -> f.isFile }?.minByOrNull { f -> f.name } }
        var clock = 1_000_000.0
        val now = { clock }
        val fake = FakeCore(seed = 7L, startWithServer = true, startPlaying = false, timers = false, now = now, dispatcher = Dispatchers.Unconfined)
        val client = CoreClient(CoverHandle(fake, cover), CoroutineScope(SupervisorJob() + Dispatchers.Main.immediate), now)
        compose.setContent {
            HocketTheme(client = client) { CompositionLocalProvider(LocalCoreClient provides client) { AppRoot(client) } }
        }
        client.requestSnapshot()
        compose.waitUntil(5_000) { client.started.value }
        val (trackId, doc) = fake.library.lyricsByTrack.entries.first { it.value?.tier == LyricsTier.Syllable }
        client.dispatch(Commands.playTracks(fake.library.serverId, listOf(trackId), 0, "Share"))
        awaitTag("miniPlayer")
        compose.onNodeWithTag("miniPlayer.info").performSemanticsAction(SemanticsActions.OnClick)
        awaitTag("player.mode.lyrics")
        // The lyrics run a frame loop: from here the clock is driven by hand.
        compose.mainClock.autoAdvance = false
        compose.onNodeWithTag("player.mode.lyrics").performSemanticsAction(SemanticsActions.OnClick)
        // Into the second verse line, so the list has lines above and below the held one.
        val held = 2
        clock += doc!!.lines[held].startMs!!.toDouble() + 600
        settle(1_500)
        compose.onNodeWithTag("lyrics.line.$held").performSemanticsAction(SemanticsActions.OnLongClick)
        // The transition, frame by frame (a frame is 16 ms of the test clock).
        repeat(40) { i ->
            compose.mainClock.advanceTimeByFrame()
            idleLooper()
            save(dir, "share-frame-%03d".format(i))
        }
        settle(800)
        save(dir, "share-1-default")
        tap("lyrics.share.line.${held + 1}")
        save(dir, "share-2-two-lines")
        compose.onNodeWithText("Colour").performSemanticsAction(SemanticsActions.OnClick)
        settle(400)
        save(dir, "share-3-colour")
        compose.onNodeWithText("Centre").performSemanticsAction(SemanticsActions.OnClick)
        compose.onNodeWithText("Tall").performSemanticsAction(SemanticsActions.OnClick)
        settle(800)
        save(dir, "share-4-tall-centre")
        compose.onNodeWithTag("lyrics.share.card").captureToImage().asAndroidBitmap().let { card ->
            File(dir, "share-card-only.png").outputStream().use { card.compress(Bitmap.CompressFormat.PNG, 100, it) }
        }
        // Closing runs the motion back into the line.
        compose.onNodeWithTag("lyrics.share.close").performSemanticsAction(SemanticsActions.OnClick)
        repeat(30) { i ->
            compose.mainClock.advanceTimeByFrame()
            idleLooper()
            save(dir, "share-close-frame-%03d".format(i))
        }
    }

    private fun tap(tag: String) {
        compose.onNodeWithTag(tag).performSemanticsAction(SemanticsActions.OnClick)
        settle(400)
    }

    private fun idleLooper() {
        org.robolectric.Shadows.shadowOf(android.os.Looper.getMainLooper()).idle()
    }

    /** Lets decoding (off the main thread) land and the UI settle for [ms] of the test clock. */
    private fun settle(ms: Long) {
        repeat(3) {
            Thread.sleep(150)
            org.robolectric.Shadows.shadowOf(android.os.Looper.getMainLooper()).idleFor(java.time.Duration.ofMillis(ms / 3))
            compose.mainClock.advanceTimeBy(ms / 3)
        }
    }

    /** The app's window with the share dialog (its full-screen node) drawn over it. */
    private fun save(dir: File, name: String) {
        val base = compose.onAllNodes(isRoot())[0].captureToImage().asAndroidBitmap()
        val out = base.copy(Bitmap.Config.ARGB_8888, true)
        runCatching { compose.onNodeWithTag("lyrics.share").captureToImage().asAndroidBitmap() }.getOrNull()?.let { Canvas(out).drawBitmap(it, 0f, 0f, null) }
        val small = Bitmap.createScaledBitmap(out, out.width / 2, out.height / 2, true)
        File(dir, "$name.png").outputStream().use { small.compress(Bitmap.CompressFormat.PNG, 100, it) }
    }

    private fun awaitTag(tag: String) {
        compose.waitUntil(5_000) { runCatching { compose.onNodeWithTag(tag).assertExists() }.isSuccess }
        compose.waitForIdle()
    }
}
