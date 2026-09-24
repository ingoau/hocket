package app.hocket.ui

import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.test.ext.junit.runners.AndroidJUnit4
import app.hocket.core.Commands
import app.hocket.core.api.Event
import app.hocket.core.api.Lyrics
import app.hocket.core.api.LyricsTier
import app.hocket.core.lyrics.LyricsCursor
import app.hocket.ui.lyrics.LyricsPage
import app.hocket.ui.lyrics.LyricsSemantics
import kotlinx.coroutines.CoroutineStart
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.launchIn
import kotlinx.coroutines.flow.onEach
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.SupervisorJob
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode
import java.util.concurrent.CopyOnWriteArrayList

/**
 * The syllable renderer against lyrics in Navidrome's enhanced shape (served by the fake core the
 * way the real core adapts them): the sweep follows each syllable's own cue start/end, is driven by
 * the extrapolated position (`PositionStamp` + clock) rather than by event arrival, lights a
 * background-vocal line as a sub-voice beside its main line, holds the last syllable through a gap
 * and shows the instrumental marker only for a long one.
 */
@RunWith(AndroidJUnit4::class)
@Config(sdk = [35], application = android.app.Application::class, qualifiers = "w411dp-h891dp")
@GraphicsMode(GraphicsMode.Mode.NATIVE)
class LyricsPageTest {
    @get:Rule
    val compose = createComposeRule()

    private var clock = 1_000_000.0
    private val core = TestCore(startPlaying = false, now = { clock })
    private val events = CopyOnWriteArrayList<Event>()

    private fun sweep(line: Int): List<Float> = compose.onNodeWithTag("lyrics.line.$line").fetchSemanticsNode().config[LyricsSemantics.Sweep]
    private fun lit(line: Int): Boolean = compose.onNodeWithTag("lyrics.line.$line").fetchSemanticsNode().config[LyricsSemantics.Lit]
    private fun focused(line: Int): Boolean = compose.onNodeWithTag("lyrics.line.$line").fetchSemanticsNode().config[LyricsSemantics.Focused]
    private fun voice(line: Int): String = compose.onNodeWithTag("lyrics.line.$line").fetchSemanticsNode().config[LyricsSemantics.Voice]

    /** Moves the clock (no core event) and lets the frame loop pick the new position up. */
    private fun at(positionMs: Long) {
        clock = 1_000_000.0 + positionMs
        compose.mainClock.advanceTimeBy(64)
        compose.waitForIdle()
    }

    /**
     * The page runs a frame loop for as long as it is shown, so the test clock is advanced by hand
     * (an automatic clock would wait forever for an idle frame).
     */
    private fun advanceUntil(what: String, condition: () -> Boolean) {
        repeat(600) { if (condition()) return; compose.mainClock.advanceTimeByFrame() }
        throw AssertionError("timed out waiting for $what")
    }

    private fun exists(tag: String) = runCatching { compose.onNodeWithTag(tag).assertExists() }.isSuccess

    /** Opens the page and starts playing [trackId] from 0 at [clock]. */
    private fun open(trackId: String) {
        core.fake.events.onEach { events += it }.launchIn(CoroutineScope(SupervisorJob() + Dispatchers.Unconfined))
        compose.mainClock.autoAdvance = false
        compose.setThemedContent(core) { LyricsPage(visible = true) }
        core.start()
        advanceUntil("the core to start") { core.client.started.value }
        core.client.dispatch(Commands.playTracks(core.fake.library.serverId, listOf(trackId), 0, "Lyrics test"))
        advanceUntil("$trackId to play") { core.client.nowPlaying.value?.track?.id == trackId }
        advanceUntil("the lyrics to show") { exists("lyrics.line.0") }
    }

    /** What the renderer must show for [line] at [t]: each syllable swept by its own cue start/end. */
    private fun expectedSweep(line: app.hocket.core.api.LyricLine, t: Long): List<Float> = line.syllables.map { s ->
        val start = s.startMs.toLong(); val end = s.endMs.toLong()
        when { t < start -> 0f; t >= end -> 1f; else -> (t - start).toFloat() / (end - start) }
    }

    /** Navidrome's real `enhanced=true` answer for "Tally", from the core's fixtures. */
    private fun tallyRaw(): String {
        var dir: java.io.File? = java.io.File(System.getProperty("user.dir")).absoluteFile
        while (dir != null) {
            val f = java.io.File(dir, "crates/hocket-core/src/subsonic/fixtures/lyrics_enhanced.json")
            if (f.exists()) return f.readText()
            dir = dir.parentFile
        }
        error("lyrics_enhanced.json not found")
    }

    @Test
    fun theRealSyllableLyricsSweepPerSyllableFromTheExtrapolatedPosition() {
        // The fake core serves Navidrome's real answer for "Tally", adapted the way the core adapts it,
        // on the longest track so the position is never clamped by the duration.
        val track = core.fake.library.lyricsByTrack.keys.maxBy { id -> core.fake.library.track(id)?.durationMs ?: 0u }
        val doc = core.fake.serveServerLyrics(track, tallyRaw())!!
        assertEquals(LyricsTier.Syllable, doc.tier)
        open(track)
        // "Lost it all at a discount" with the background "(Yeah, yeah)" sung over its end.
        val main = doc.lines.indexOfFirst { it.text == "Lost it all at a discount" }
        val bg = main + 1
        assertTrue(doc.lines[bg].background)
        val s = doc.lines[main].syllables
        val eventsBefore = events.size
        val stampBefore = core.client.transport.value.position
        for (k in listOf(0, 2, s.lastIndex)) {
            val t = (s[k].startMs.toLong() + s[k].endMs.toLong()) / 2
            at(t)
            assertSweep(expectedSweep(doc.lines[main], t), sweep(main))
            assertEquals("the syllable being sung is half swept", 0.5f, sweep(main)[k], 0.05f)
            assertTrue(lit(main))
            assertEquals("main", voice(main))
        }
        // The background vocal: rendered as a sub-voice line, lit and swept by its own cues while the
        // main line still runs.
        val b = doc.lines[bg].syllables
        val tb = (b[0].startMs.toLong() + b[0].endMs.toLong()) / 2
        at(tb)
        assertEquals("bg", voice(bg))
        assertTrue(lit(bg))
        assertSweep(expectedSweep(doc.lines[bg], tb), sweep(bg))
        assertFalse("the main voice keeps the focus", focused(bg))
        // Once both have been sung (before the next line): the main line stays lit and held at 1, the
        // finished background line goes dim, whichever of the two the cursor calls primary.
        val done = maxOf(doc.lines[main].endMs!!.toLong(), doc.lines[bg].endMs!!.toLong()) + 100
        assertTrue("the fixture shape: a pause before the next line", done < doc.lines[bg + 1].startMs!!.toLong())
        at(done)
        assertTrue(lit(main))
        assertTrue(focused(main))
        assertFalse(lit(bg))
        assertSweep(s.map { 1f }, sweep(main))
        assertSweep(b.map { 1f }, sweep(bg))
        // All of it from the stamp and the clock: no event arrived while the sweep moved.
        assertEquals(eventsBefore, events.size)
        assertEquals(stampBefore, core.client.transport.value.position)
    }

    private fun assertSweep(expected: List<Float>, actual: List<Float>) {
        assertEquals(expected.size, actual.size)
        expected.zip(actual).forEach { (e, a) -> assertEquals("sweep $expected vs $actual", e, a, 0.03f) }
    }

    /** Starts the fake playing (from 0, at [clock]) a track with syllable lyrics and opens the page on it. */
    private fun play(): Lyrics {
        val (trackId, doc) = core.fake.library.lyricsByTrack.entries.first { it.value?.tier == LyricsTier.Syllable && it.value!!.lines.any { l -> l.background } }
        open(trackId)
        return doc!!
    }

    @Test
    fun syllablesSweepFromTheirOwnCueTimingDrivenByTheClockNotByEvents() {
        val doc = play()
        val bg = doc.lines.indexOfFirst { it.background }
        val main = bg - 1 // the adapter puts a sub-voice line right after the line it sings over
        assertEquals("the fixture shape: the background line starts with its main line", doc.lines[main].startMs, doc.lines[bg].startMs)
        val s = doc.lines[main].syllables
        assertTrue(s.size >= 4)
        val stampBefore = core.client.transport.value.position
        val eventsBefore = events.size

        // Mid-way through the second syllable: the first is done, the second half swept, the rest untouched.
        at((s[1].startMs.toLong() + s[1].endMs.toLong()) / 2)
        val expected = s.indices.map { i -> if (i == 0) 1f else if (i == 1) 0.5f else 0f }
        assertSweep(expected, sweep(main))
        assertTrue(lit(main))
        assertTrue("the main voice keeps the focus although the background line is the later-starting primary", focused(main))
        assertEquals("main", voice(main))
        // The background line is lit beside it as a sub-voice, sweeping its own cues.
        assertTrue(lit(bg))
        assertFalse(focused(bg))
        assertEquals("bg", voice(bg))
        assertSweep(LyricsCursor.sweep(doc.lines[bg], s[1].startMs.toLong() + (s[1].endMs.toLong() - s[1].startMs.toLong()) / 2), sweep(bg))
        assertTrue("some of the background line has been sung", sweep(bg).any { it > 0f })
        // Nothing arrived from the core: the position came from the stamp and the clock alone.
        assertEquals(stampBefore, core.client.transport.value.position)
        assertEquals(eventsBefore, events.size)

        // Later in the same line: the sweep moved on with the clock only.
        at((s[3].startMs.toLong() + s[3].endMs.toLong()) / 2)
        assertSweep(s.indices.map { i -> if (i < 3) 1f else if (i == 3) 0.5f else 0f }, sweep(main))
        assertEquals(eventsBefore, events.size)

        // Past the line's end but before the next line starts: every syllable held at 1, still lit (no
        // flash); the background line, which ended half-way through, is dim although the cursor
        // calls it the primary line.
        at(doc.lines[main].endMs!!.toLong() + 200)
        assertSweep(s.map { 1f }, sweep(main))
        assertTrue(lit(main))
        assertTrue(focused(main))
        assertSweep(doc.lines[bg].syllables.map { 1f }, sweep(bg))
        assertFalse(lit(bg))
    }

    @Test
    fun aLongGapHoldsTheLastSyllableAndShowsTheInstrumentalMarker() {
        val doc = play()
        // The fake leaves a 6 s instrumental gap before its ninth verse; the line before it is index 8
        // (one background line was inserted above it).
        val line = 8
        val next = doc.lines.drop(line + 1).first { !it.background }
        val end = doc.lines[line].endMs!!.toLong()
        assertTrue("the fixture shape: a gap longer than the cursor's instrumental threshold", next.startMs!!.toLong() - end > LyricsCursor.GAP_MS)
        at(end - 100)
        assertTrue(lit(line))
        assertTrue(sweep(line).last() < 1f)
        // Within the grace period after the end: held, still lit, no marker.
        at(end + 500)
        assertSweep(doc.lines[line].syllables.map { 1f }, sweep(line))
        assertTrue(lit(line))
        assertFalse(runCatching { compose.onNodeWithText("♪").assertIsDisplayed() }.isSuccess)
        // Deep in the gap: the marker shows, the line dims, the sweep stays held at 1 (never swept further).
        at(end + 2_500)
        compose.onNodeWithText("♪").assertIsDisplayed()
        assertFalse(lit(line))
        assertSweep(doc.lines[line].syllables.map { 1f }, sweep(line))
    }

    @Test
    fun aLineWithoutCuesIsHighlightedWholeWithoutFakedSyllables() {
        val doc = play()
        val line = doc.lines.indexOfFirst { it.syllables.isEmpty() && it.startMs != null && !it.background }
        assertTrue(line >= 0)
        val start = doc.lines[line].startMs!!.toLong()
        at(start + 300)
        assertTrue(lit(line))
        assertTrue("no syllable sweep for a line the server timed only as a whole", sweep(line).isEmpty())
        compose.onNodeWithTag("lyrics.line.$line").assertIsDisplayed()
    }

    @Test
    fun theOffsetMovesTheLyricsTheCoreWay() {
        val doc = play()
        val bg = doc.lines.indexOfFirst { it.background }
        val main = bg - 1
        val s = doc.lines[main].syllables
        // A positive offset shows the lyrics later: at the syllable's own start nothing has been sung yet.
        core.client.dispatch(Commands.setLyricsOffset(doc.trackId, 400))
        advanceUntil("the offset") { core.client.lyrics.value[doc.trackId]?.offsetMs == 400 }
        at(s[1].startMs.toLong())
        val effective = s[1].startMs.toLong() - 400
        assertSweep(s.indices.map { i -> if (i == 0) ((effective - s[0].startMs.toLong()).toFloat() / (s[0].endMs.toLong() - s[0].startMs.toLong())).coerceIn(0f, 1f) else 0f }, sweep(main))
        at(s[1].startMs.toLong() + 400)
        assertEquals(1f, sweep(main)[0], 0.001f)
        assertEquals(0f, sweep(main)[1], 0.03f)
    }
}
