package app.hocket.core.fake

import app.hocket.core.api.LyricsTier
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * [EnhancedLyrics] against the shape of Navidrome's real `enhanced=true` answer for "Tally" (the
 * fixture reviewed with the core's adapter): inclusive byte offsets, syllable splits, a background
 * agent, lines without cues, multi-byte text and gaps between lines.
 */
class EnhancedLyricsTest {
    private val agents = listOf(RawAgent("v1", "main"), RawAgent("__nd_bg__|v1", "bg"))

    /** "I lost my rank and title" exactly as the server times it: `ti` 19..20 + `tle` 21..23 are one word. */
    private fun titleLine() = RawCueLine(
        0, 13750, 15350, "I lost my rank and title", "v1",
        listOf(
            RawCue(13750, 14041, "I", 0, 0), RawCue(14041, 14281, "lost", 2, 5), RawCue(14281, 14476, "my", 7, 8),
            RawCue(14476, 14701, "rank", 10, 13), RawCue(14701, 14872, "and", 15, 17), RawCue(14872, 15061, "ti", 19, 20), RawCue(15061, 15350, "tle", 21, 23),
        ),
    )

    @Test
    fun inclusiveByteOffsetsDecideJoins() {
        val syl = EnhancedLyrics.syllablesFrom(titleLine())
        assertEquals(listOf("I", "lost", "my", "rank", "and", "ti", "tle"), syl.map { it.text })
        assertEquals("only the syllable split is joined", listOf(false, false, false, false, false, true, false), syl.map { it.joined })
        assertEquals(14872u, syl[5].startMs)
        assertEquals(15061u, syl[5].endMs)
        assertEquals(15061u, syl[6].startMs)
    }

    @Test
    fun multiByteTextKeepsByteOffsetsHonest() {
        // "naïve" is 6 bytes; the offsets are bytes, the gap between "na" and "ïve" is empty (joined),
        // the gap before "summer" is the space (not joined).
        val text = "naïve summer"
        val cl = RawCueLine(0, 0, 900, text, "v1", listOf(RawCue(0, 300, "na", 0, 1), RawCue(300, 600, "ïve", 2, 5), RawCue(600, 900, "summer", 7, 12)))
        val syl = EnhancedLyrics.syllablesFrom(cl)
        assertEquals(listOf("na", "ïve", "summer"), syl.map { it.text })
        assertEquals(listOf(true, false, false), syl.map { it.joined })
        // An offset inside a multi-byte char is not a char boundary: no gap, fall back to the cue's own whitespace.
        assertNull(EnhancedLyrics.byteGap(text, RawCue(0, 300, "na", 0, 2), RawCue(300, 600, "ve", 4, 5)))
    }

    @Test
    fun punctuationBetweenJoinedCuesStaysVisibleAndWhitespaceSplits() {
        val text = "well-known song"
        val cl = RawCueLine(0, 0, 900, text, "v1", listOf(RawCue(0, 300, "well", 0, 3), RawCue(300, 600, "known", 5, 9), RawCue(600, 900, "song", 11, 14)))
        val syl = EnhancedLyrics.syllablesFrom(cl)
        assertEquals(listOf("well-", "known", "song"), syl.map { it.text })
        assertEquals(listOf(true, false, false), syl.map { it.joined })
        // Without offsets the cues' own whitespace decides.
        val plain = RawCueLine(0, 0, 900, text, "v1", listOf(RawCue(0, 300, "well-"), RawCue(300, 600, "known "), RawCue(600, 900, "song")))
        assertEquals(listOf(true, false, false), EnhancedLyrics.syllablesFrom(plain).map { it.joined })
    }

    @Test
    fun backgroundAgentLinesBecomeSubVoiceLinesAfterTheirMainLine() {
        val main = RawCueLine(1, 16966, 20517, "Sold it all at a discount", "v1", listOf(RawCue(16966, 17500, "Sold", 0, 3), RawCue(17500, 20517, "it all at a discount", 5, 24)))
        val bg = RawCueLine(1, 16966, 20517, "(Yeah, yeah)", "__nd_bg__|v1", listOf(RawCue(18000, 18500, "(Yeah,", 0, 5), RawCue(18500, 19000, "yeah)", 7, 11)))
        val entry = RawStructuredLyrics(
            agents = agents, line = listOf(RawLine(13750, "I lost my rank and title"), RawLine(16966, "Sold it all at a discount")),
            cueLine = listOf(titleLine(), bg, main), // the bg cue line listed first: the main voice is still the line itself
        )
        val doc = EnhancedLyrics.adapt("t", entry)
        assertEquals(LyricsTier.Syllable, doc.tier)
        assertEquals(3, doc.lines.size)
        assertFalse(doc.lines[1].background)
        assertEquals("Sold it all at a discount", doc.lines[1].text)
        val sub = doc.lines[2]
        assertTrue(sub.background)
        assertEquals("__nd_bg__|v1", sub.agent)
        assertEquals("(Yeah, yeah)", sub.text)
        assertEquals("a sub-voice line starts at its first timed word", 18000u, sub.startMs)
        assertEquals(20517u, sub.endMs)
        assertEquals(2, sub.syllables.size)
        // Agents: main first with side 0; the bg agent shares the side of the voice before it.
        assertEquals(listOf("v1" to 0u, "__nd_bg__|v1" to 0u), doc.agents.map { it.id to it.side })
    }

    @Test
    fun aCueLineWithoutCuesStaysAtLineTierWithoutFakingSyllables() {
        val entry = RawStructuredLyrics(
            agents = agents, line = listOf(RawLine(13750, "I lost my rank and title"), RawLine(16966, "Spoken word"), RawLine(21000, "Later")),
            cueLine = listOf(titleLine(), RawCueLine(1, 16966, null, "Spoken word", "v1", emptyList())),
        )
        val doc = EnhancedLyrics.adapt("t", entry)
        assertEquals("one syllable line makes the document syllable tier", LyricsTier.Syllable, doc.tier)
        assertTrue("no syllables invented", doc.lines[1].syllables.isEmpty())
        assertEquals("end defaults to the next line's start", 21000u, doc.lines[1].endMs)
        assertEquals("the line's own end from its cue line", 15350u, doc.lines[0].endMs)
        assertTrue(doc.lines[2].syllables.isEmpty())
    }

    @Test
    fun aCueMissingAStartDropsSyllablesForThatLineOnly() {
        val broken = RawCueLine(0, 13750, 15350, "I lost", "v1", listOf(RawCue(13750, 14041, "I", 0, 0), RawCue(null, 14281, "lost", 2, 5)))
        assertTrue(EnhancedLyrics.syllablesFrom(broken).isEmpty())
    }

    @Test
    fun unsyncedDocumentsHaveNoTiming() {
        val doc = EnhancedLyrics.adapt("t", RawStructuredLyrics(synced = false, line = listOf(RawLine(null, "a"), RawLine(null, "b"))))
        assertEquals(LyricsTier.Unsynced, doc.tier)
        assertTrue(doc.lines.all { it.startMs == null && it.syllables.isEmpty() })
    }
}
