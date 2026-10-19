package app.hocket.ui

import android.graphics.Bitmap
import androidx.compose.ui.graphics.asAndroidBitmap
import androidx.compose.ui.test.captureToImage
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onRoot
import androidx.test.ext.junit.runners.AndroidJUnit4
import app.hocket.core.Commands
import app.hocket.core.api.Lyrics
import app.hocket.ui.lyrics.LyricsPage
import org.junit.Assume.assumeTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode
import java.io.File

/**
 * The lyrics page in motion, for review by eye, on Navidrome's real syllable answer for "Tally":
 * mid-line (the soft sweep edge and the rising words), a long held word swelling, a background
 * vocal sliding out under its line, the lines rippling into place frame by frame after a line
 * change, and the instrumental dots. Not an assertion test; it runs only with
 *
 *     HOCKET_SCREENSHOT_DIR=/tmp/shots ./gradlew :app:testDebugUnitTest --tests '*LyricsMotionScreenshotTest*' --rerun
 */
@RunWith(AndroidJUnit4::class)
@Config(sdk = [35], application = android.app.Application::class, qualifiers = "w411dp-h891dp-night-xxhdpi")
@GraphicsMode(GraphicsMode.Mode.NATIVE)
class LyricsMotionScreenshotTest {
    @get:Rule
    val compose = createComposeRule()

    private val outDir: File? = (System.getProperty("hocket.screenshotDir") ?: System.getenv("HOCKET_SCREENSHOT_DIR"))?.takeIf { it.isNotBlank() }?.let(::File)
    private var clock = 1_000_000.0
    private val core = TestCore(startPlaying = false, now = { clock })

    private fun exists(tag: String) = runCatching { compose.onNodeWithTag(tag).assertExists() }.isSuccess

    private fun advanceUntil(what: String, condition: () -> Boolean) {
        repeat(600) { if (condition()) return; compose.mainClock.advanceTimeByFrame() }
        throw AssertionError("timed out waiting for $what")
    }

    /** Moves the song to [positionMs] and the frame clock on by the same time, a frame at a time. */
    private fun at(positionMs: Long, settle: Boolean = true) {
        val from = clock - 1_000_000.0
        clock = 1_000_000.0 + positionMs
        if (!settle) return
        repeat(((positionMs - from).toLong().coerceIn(0, 1_500) / 16).toInt() + 1) { compose.mainClock.advanceTimeByFrame() }
    }

    /** Steps the song and the frame clock together by [ms]. */
    private fun play(ms: Long) {
        repeat((ms / 16).toInt()) { clock += 16.0; compose.mainClock.advanceTimeByFrame() }
    }

    private fun shot(dir: File, name: String) {
        val bitmap = compose.onRoot().captureToImage().asAndroidBitmap()
        val file = File(dir, "$name.png")
        file.outputStream().use { bitmap.compress(Bitmap.CompressFormat.PNG, 100, it) }
        println("screenshot: ${file.absolutePath}")
    }

    private fun tallyRaw(): String {
        var dir: File? = File(System.getProperty("user.dir")).absoluteFile
        while (dir != null) {
            val f = File(dir, "crates/hocket-core/src/subsonic/fixtures/lyrics_enhanced.json")
            if (f.exists()) return f.readText()
            dir = dir.parentFile
        }
        error("lyrics_enhanced.json not found")
    }

    private fun open(): Lyrics {
        val track = core.fake.library.lyricsByTrack.keys.maxBy { id -> core.fake.library.track(id)?.durationMs ?: 0u }
        val doc = core.fake.serveServerLyrics(track, tallyRaw())!!
        compose.mainClock.autoAdvance = false
        compose.setThemedContent(core) { LyricsPage(visible = true) }
        core.start()
        advanceUntil("the core to start") { core.client.started.value }
        core.client.dispatch(Commands.playTracks(core.fake.library.serverId, listOf(track), 0, "Lyrics motion"))
        advanceUntil("the track") { core.client.nowPlaying.value?.track?.id == track }
        advanceUntil("the lyrics") { exists("lyrics.line.0") }
        return doc
    }

    @Test
    fun motion() {
        val dir = outDir
        assumeTrue("HOCKET_SCREENSHOT_DIR not set", dir != null)
        dir!!.mkdirs()
        val doc = open()
        val main = doc.lines.indexOfFirst { it.text == "Lost it all at a discount" }
        val s = doc.lines[main].syllables
        // Settled on the line before, then the change to "Lost it all…" frame by frame.
        at(doc.lines[main].startMs!!.toLong() - 1_200)
        play(1_000)
        shot(dir, "lyrics-01-before-change")
        for (k in 1..6) { play(80); shot(dir, "lyrics-02-change-${k * 80}ms") }
        // Mid-line: the soft edge across the syllable being sung, the sung words risen.
        at((s[2].startMs.toLong() + s[2].endMs.toLong()) / 2)
        play(16)
        shot(dir, "lyrics-03-mid-line")
        // The background vocal, slid out under its line.
        val bg = doc.lines[main + 1].syllables
        at((bg[0].startMs.toLong() + bg[0].endMs.toLong()) / 2)
        play(400)
        shot(dir, "lyrics-04-background-vocal")
        // The longest held word, mid-swell.
        val (line, syl) = doc.lines.withIndex().filter { !it.value.background }.flatMap { (i, l) -> l.syllables.map { i to it } }.maxBy { (_, x) -> x.endMs.toLong() - x.startMs.toLong() }
        println("held: ${doc.lines[line].text} / ${syl.text} ${syl.endMs.toLong() - syl.startMs.toLong()} ms")
        at(syl.startMs.toLong() - 300)
        play(300)
        for (k in 1..4) { play(syl.endMs.toLong().minus(syl.startMs.toLong()) / 4); shot(dir, "lyrics-05-held-$k") }
        // A long gap: the breathing dots.
        val gapAfter = doc.lines.indices.firstOrNull { i ->
            val end = doc.lines[i].endMs?.toLong() ?: return@firstOrNull false
            val next = doc.lines.drop(i + 1).mapNotNull { it.startMs?.toLong() }.filter { it > end }.minOrNull() ?: return@firstOrNull false
            next - end > 6_000 && doc.lines.none { it.startMs != null && it.startMs!!.toLong() < next && (it.endMs?.toLong() ?: 0) > end }
        }
        if (gapAfter != null) {
            at(doc.lines[gapAfter].endMs!!.toLong() + 2_000)
            play(600)
            shot(dir, "lyrics-06-interlude")
        }
    }

    /** The breathing dots, in the fake library's 6 s instrumental gap. */
    @Test
    fun interlude() {
        val dir = outDir
        assumeTrue("HOCKET_SCREENSHOT_DIR not set", dir != null)
        dir!!.mkdirs()
        val (track, doc) = core.fake.library.lyricsByTrack.entries.first { it.value?.tier == app.hocket.core.api.LyricsTier.Syllable && it.value!!.lines.any { l -> l.background } }
        compose.mainClock.autoAdvance = false
        compose.setThemedContent(core) { LyricsPage(visible = true) }
        core.start()
        advanceUntil("the core to start") { core.client.started.value }
        core.client.dispatch(Commands.playTracks(core.fake.library.serverId, listOf(track), 0, "Lyrics motion"))
        advanceUntil("the track") { core.client.nowPlaying.value?.track?.id == track }
        advanceUntil("the lyrics") { exists("lyrics.line.0") }
        val end = doc!!.lines[8].endMs!!.toLong()
        at(end - 500)
        play(500 + 1_000 + 1_800)
        shot(dir, "lyrics-06-interlude")
    }
}
