package app.hocket.ui.a11y

import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.semantics.getOrNull
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onRoot
import androidx.test.ext.junit.runners.AndroidJUnit4
import app.hocket.core.Commands
import app.hocket.core.api.Lyrics
import app.hocket.core.api.LyricsTier
import app.hocket.ui.TestCore
import app.hocket.ui.lyrics.LyricsBackgroundAnimated
import app.hocket.ui.lyrics.LyricsPage
import app.hocket.ui.lyrics.LyricsSemantics
import app.hocket.ui.setThemedContent
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode

/**
 * The lyrics page for a screen reader (one labelled container, lines as items with a "current line"
 * state and background vocals marked, no live regions) and with animations removed (a static
 * highlight instead of the sweep, no blur, a still background).
 */
@RunWith(AndroidJUnit4::class)
@Config(sdk = [35], application = android.app.Application::class, qualifiers = "w411dp-h891dp")
@GraphicsMode(GraphicsMode.Mode.NATIVE)
class LyricsAccessibilityTest {
    @get:Rule
    val compose = createComposeRule()

    private var clock = 1_000_000.0
    private val core = TestCore(startPlaying = false, now = { clock })

    private fun exists(tag: String) = runCatching { compose.onNodeWithTag(tag).assertExists() }.isSuccess
    private fun line(i: Int) = compose.onNodeWithTag("lyrics.line.$i").fetchSemanticsNode()
    private fun sweep(i: Int) = line(i).config[LyricsSemantics.Sweep]

    private fun advanceUntil(what: String, condition: () -> Boolean) {
        repeat(600) { if (condition()) return; compose.mainClock.advanceTimeByFrame() }
        throw AssertionError("timed out waiting for $what")
    }

    private fun at(positionMs: Long) {
        clock = 1_000_000.0 + positionMs
        compose.mainClock.advanceTimeBy(600) // past the 4 Hz reduced-motion sampling
        compose.waitForIdle()
    }

    private fun open(reducedMotion: Boolean): Lyrics {
        val (trackId, doc) = core.fake.library.lyricsByTrack.entries.first { it.value?.tier == LyricsTier.Syllable && it.value!!.lines.any { l -> l.background } }
        compose.mainClock.autoAdvance = false
        compose.setThemedContent(core) {
            CompositionLocalProvider(LocalReducedMotion provides reducedMotion) { LyricsPage(visible = true) }
        }
        core.start()
        advanceUntil("the core to start") { core.client.started.value }
        core.client.dispatch(Commands.playTracks(core.fake.library.serverId, listOf(trackId), 0, "Lyrics a11y"))
        advanceUntil("the track") { core.client.nowPlaying.value?.track?.id == trackId }
        advanceUntil("the lyrics") { exists("lyrics.line.0") && exists("lyrics.background") }
        return doc!!
    }

    @Test
    fun linesAreItemsWithTheirStateAndNothingIsALiveRegion() {
        val doc = open(reducedMotion = false)
        val bg = doc.lines.indexOfFirst { it.background }
        val main = bg - 1
        val s = doc.lines[main].syllables
        at((s[1].startMs.toLong() + s[1].endMs.toLong()) / 2)
        val page = compose.onNodeWithTag("lyrics.page").fetchSemanticsNode()
        assertEquals(listOf("Lyrics"), page.config[SemanticsProperties.ContentDescription])
        assertTrue(page.config[SemanticsProperties.IsTraversalGroup])
        val m = line(main)
        assertEquals(listOf(doc.lines[main].text), m.config[SemanticsProperties.ContentDescription])
        assertEquals("current line", m.config[SemanticsProperties.StateDescription])
        assertEquals("Play from this line", m.config[SemanticsActions.OnClick].label)
        assertTrue("syllables are not separate items", m.children.isEmpty())
        val b = line(bg)
        assertEquals("background vocal", b.config[SemanticsProperties.StateDescription])
        assertTrue("other lines carry no state", line(main + 2).config.getOrNull(SemanticsProperties.StateDescription) == null)
        // Lines and syllables are never announced as they pass: no live region anywhere on the page.
        assertTrue(A11yChecks.all(compose.onRoot().fetchSemanticsNode()).none { it.config.contains(SemanticsProperties.LiveRegion) })
        // As the song moves on, the "current line" state moves with it (no announcement).
        at(doc.lines[main + 2].startMs!!.toLong() + 200)
        assertEquals("current line", line(main + 2).config[SemanticsProperties.StateDescription])
        assertTrue(line(main).config.getOrNull(SemanticsProperties.StateDescription) == null)
        // Without reduced motion: the sweep is partial mid-syllable, far lines are blurred, the
        // background animates (Android 13+).
        at((s[1].startMs.toLong() + s[1].endMs.toLong()) / 2)
        assertTrue(sweep(main).any { it > 0.1f && it < 0.9f })
        assertTrue((0 until doc.lines.size).filter { exists("lyrics.line.$it") }.any { line(it).config[LyricsSemantics.Blur] > 0f })
        assertTrue(compose.onNodeWithTag("lyrics.background").fetchSemanticsNode().config[LyricsBackgroundAnimated])
    }

    @Test
    fun reducedMotionMakesTheSweepAStaticHighlightWithoutBlurOrMovingBackground() {
        val doc = open(reducedMotion = true)
        val bg = doc.lines.indexOfFirst { it.background }
        val main = bg - 1
        val s = doc.lines[main].syllables
        for (k in listOf(0, 1, s.lastIndex)) {
            at((s[k].startMs.toLong() + s[k].endMs.toLong()) / 2)
            // The whole lit line is highlighted at once, whichever syllable is being sung.
            assertEquals(s.map { 1f }, sweep(main))
            assertTrue(line(main).config[LyricsSemantics.Lit])
        }
        // Lines not being sung: not highlighted at all; nothing in between anywhere.
        val later = main + 2
        assertEquals(doc.lines[later].syllables.map { 0f }, sweep(later))
        val shown = (0 until doc.lines.size).filter { exists("lyrics.line.$it") }
        assertTrue(shown.all { i -> sweep(i).all { it == 0f || it == 1f } })
        assertTrue("no depth-of-field blur", shown.all { line(it).config[LyricsSemantics.Blur] == 0f })
        assertFalse("the fluid background stands still", compose.onNodeWithTag("lyrics.background").fetchSemanticsNode().config[LyricsBackgroundAnimated])
        // The highlight still moves between lines as the song does.
        at(doc.lines[later].startMs!!.toLong() + 200)
        assertEquals(doc.lines[later].syllables.map { 1f }, sweep(later))
        assertEquals("current line", line(later).config[SemanticsProperties.StateDescription])
    }
}
