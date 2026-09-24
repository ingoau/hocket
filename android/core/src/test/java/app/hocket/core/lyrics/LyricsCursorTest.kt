package app.hocket.core.lyrics

import app.hocket.core.api.*
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class LyricsCursorTest {
    private fun line(start: Long?, end: Long?, text: String, syllables: List<LyricSyllable> = emptyList()) =
        LyricLine(start?.toUInt(), end?.toUInt(), text, syllables, null, false, null)

    private fun syl(text: String, s: Long, e: Long) = LyricSyllable(text, s.toUInt(), e.toUInt(), false)

    private fun lyrics(tier: LyricsTier, lines: List<LyricLine>, offset: Int = 0) =
        Lyrics("t", tier, null, null, null, emptyList(), lines, LyricsSource.Server, offset)

    private val syllableDoc = lyrics(LyricsTier.Syllable, listOf(
        line(1000, 3000, "Hel lo", listOf(syl("Hel", 1000, 1500), syl("lo", 1500, 3000))),
        line(10_000, 12_000, "World", listOf(syl("World", 10_000, 12_000))),
    ))

    @Test
    fun beforeFirstLineIsEmpty() {
        val c = LyricsCursor.at(syllableDoc, 500)
        assertEquals(LyricsCursor.NONE, c.lineIndex)
        assertTrue(c.activeLines.isEmpty())
        assertEquals(500L, c.effectiveMs)
    }

    @Test
    fun syllableProgressIsWithinSyllableOnly() {
        val c = LyricsCursor.at(syllableDoc, 1250)
        assertEquals(0, c.lineIndex)
        assertEquals(0, c.syllableIndex)
        assertEquals(0.5f, c.syllableProgress, 0.001f)
        val c2 = LyricsCursor.at(syllableDoc, 2250)
        assertEquals(1, c2.syllableIndex)
        assertEquals(0.5f, c2.syllableProgress, 0.001f)
        assertEquals(0.625f, c2.lineProgress, 0.001f)
    }

    @Test
    fun gapBetweenLinesIsReported() {
        val c = LyricsCursor.at(syllableDoc, 6000)
        assertEquals(0, c.lineIndex)
        assertTrue(c.inGap)
        assertEquals(1f, c.syllableProgress, 0f)
        assertFalse(LyricsCursor.at(syllableDoc, 3500).inGap) // within GAP_MS of the end: no flash
    }

    @Test
    fun offsetShiftsTimingTheCoreWay() {
        // The core's rule (lyrics/cursor.rs): a positive offset makes the lyrics appear later, so the
        // audio position is compared as if it were earlier. +500 at 1200 is still before the first line.
        val later = syllableDoc.copy(offsetMs = 500)
        assertEquals(LyricsCursor.NONE, LyricsCursor.at(later, 1200).lineIndex)
        assertEquals(0, LyricsCursor.at(later, 1600).lineIndex)
        assertEquals(1100L, LyricsCursor.at(later, 1600).effectiveMs)
        // A negative offset shows them earlier.
        val earlier = syllableDoc.copy(offsetMs = -500)
        assertEquals(0, LyricsCursor.at(earlier, 600).lineIndex)
    }

    private fun bgLine(start: Long, end: Long, text: String, syllables: List<LyricSyllable>) =
        LyricLine(start.toUInt(), end.toUInt(), text, syllables, "__nd_bg__|v1", true, null)

    /** The "Tally" shape: a background line starting with its main line, and one starting mid-line. */
    private val duetDoc = lyrics(LyricsTier.Syllable, listOf(
        line(1000, 3000, "Sold it all", listOf(syl("Sold", 1000, 1500), syl("it", 1500, 2000), syl("all", 2000, 3000))),
        bgLine(1000, 2500, "(Yeah, yeah)", listOf(syl("(Yeah,", 1000, 1750), syl("yeah)", 1750, 2500))),
        line(4000, 6000, "Lost it all", listOf(syl("Lost", 4000, 5000), syl("it all", 5000, 6000))),
        bgLine(5000, 7000, "(Yeah, yeah)", listOf(syl("(Yeah,", 5000, 6000), syl("yeah)", 6000, 7000))),
        line(9000, 10_000, "Somehow", listOf(syl("Somehow", 9000, 10_000))),
    ))

    @Test
    fun overlappingLinesAreAllActiveAndEachSweepsItsOwnSyllables() {
        // Both lines start at 1000: the later one is primary (the core's tie rule), the main line stays active.
        val c = LyricsCursor.at(duetDoc, 1750)
        assertEquals(1, c.lineIndex)
        assertEquals(listOf(0, 1), c.activeLines)
        assertEquals(listOf(1f, 0.5f, 0f), LyricsCursor.sweep(duetDoc.lines[0], c.effectiveMs))
        assertEquals(listOf(1f, 0f), LyricsCursor.sweep(duetDoc.lines[1], c.effectiveMs))
        // The bg line ended, the main line is still running.
        val c2 = LyricsCursor.at(duetDoc, 2750)
        assertEquals(listOf(0), c2.activeLines)
        assertEquals(listOf(1f, 1f, 0.75f), LyricsCursor.sweep(duetDoc.lines[0], c2.effectiveMs))
        assertEquals("a finished line holds all its syllables", listOf(1f, 1f), LyricsCursor.sweep(duetDoc.lines[1], c2.effectiveMs))
        // A bg line starting mid-line becomes primary while the main line keeps running.
        val c3 = LyricsCursor.at(duetDoc, 5500)
        assertEquals(3, c3.lineIndex)
        assertEquals(listOf(2, 3), c3.activeLines)
        assertEquals(listOf(1f, 0.5f), LyricsCursor.sweep(duetDoc.lines[2], c3.effectiveMs))
        assertEquals(listOf(0.5f, 0f), LyricsCursor.sweep(duetDoc.lines[3], c3.effectiveMs))
        // The main line ended at 6000 while the bg line runs on: only the bg line is active.
        val c4 = LyricsCursor.at(duetDoc, 6500)
        assertEquals(listOf(3), c4.activeLines)
        assertEquals("a future line is all zeros", listOf(0f), LyricsCursor.sweep(duetDoc.lines[4], c4.effectiveMs))
    }

    @Test
    fun aGapBeforeTheNextLineHoldsTheLastSyllableAndNeverSweeps() {
        // Line "Lost it all" ends at 6000; bg line ends at 7000; the next line starts at 9000.
        val c = LyricsCursor.at(duetDoc, 7500)
        assertEquals(3, c.lineIndex)
        assertTrue(c.activeLines.isEmpty())
        assertFalse("a 2 s gap is not an instrumental", c.inGap)
        assertEquals(1f, c.syllableProgress, 0f)
        assertEquals(listOf(1f, 1f), LyricsCursor.sweep(duetDoc.lines[2], c.effectiveMs))
        assertEquals(listOf(1f, 1f), LyricsCursor.sweep(duetDoc.lines[3], c.effectiveMs))
        assertEquals(listOf(0f), LyricsCursor.sweep(duetDoc.lines[4], c.effectiveMs))
    }

    @Test
    fun aShortBackgroundLineNeverOpensTheGapWhileItsMainLineIsSung() {
        // The bg line (primary: it starts with its main line and is later in the list) ends at 2 s;
        // the main line runs to 9 s and the next line starts at 20 s.
        val doc = Lyrics("t", LyricsTier.Syllable, null, null, null, emptyList(), listOf(
            line(1000, 9000, "A long main line", listOf(syl("A", 1000, 3000), syl("long", 3000, 9000))),
            bgLine(1000, 2000, "(ooh)", listOf(syl("(ooh)", 1000, 2000))),
            line(20_000, 21_000, "Next", listOf(syl("Next", 20_000, 21_000))),
        ), LyricsSource.Server, 0)
        val during = LyricsCursor.at(doc, 5000)
        assertEquals(1, during.lineIndex)
        assertEquals(listOf(0), during.activeLines)
        assertFalse("the main voice is still singing", during.inGap)
        // After everything ended (plus the grace), the 11 s wait before "Next" is an instrumental.
        assertFalse(LyricsCursor.at(doc, 9500).inGap)
        assertTrue(LyricsCursor.at(doc, 10_500).inGap)
    }

    @Test
    fun linesNeedNotBeSortedByStart() {
        // A sub-voice line placed after its main line may start later than the next main line.
        val doc = lyrics(LyricsTier.Syllable, listOf(
            line(1000, 5000, "Main one", listOf(syl("Main", 1000, 2000), syl("one", 2000, 5000))),
            bgLine(3500, 4500, "(bg)", listOf(syl("(bg)", 3500, 4500))),
            line(3000, 6000, "Main two", listOf(syl("Main", 3000, 4000), syl("two", 4000, 6000))),
        ))
        val c = LyricsCursor.at(doc, 3600)
        assertEquals("latest start wins regardless of position in the list", 1, c.lineIndex)
        assertEquals(listOf(0, 1, 2), c.activeLines)
        // The bg line has ended but still has the latest start: it stays primary (the core's rule; the
        // renderer focuses the main voice through activeLines), and only the two main lines are active.
        assertEquals(1, LyricsCursor.at(doc, 4800).lineIndex)
        assertEquals(listOf(0, 2), LyricsCursor.at(doc, 4800).activeLines)
    }

    @Test
    fun lineTierNeverFabricatesSyllables() {
        val doc = lyrics(LyricsTier.Line, listOf(line(1000, null, "One"), line(4000, null, "Two"), line(7000, null, "Three")))
        val c = LyricsCursor.at(doc, 2500)
        assertEquals(0, c.lineIndex)
        assertEquals(LyricsCursor.NONE, c.syllableIndex)
        assertEquals(0.5f, c.lineProgress, 0.001f) // end inferred from next line start
        assertEquals(2, LyricsCursor.at(doc, 9000).lineIndex)
    }

    @Test
    fun untimedLinesAreSkipped() {
        val doc = lyrics(LyricsTier.Line, listOf(line(null, null, "Intro"), line(1000, null, "One"), line(null, null, "Note"), line(5000, null, "Two")))
        assertEquals(1, LyricsCursor.at(doc, 3000).lineIndex)
        assertEquals(3, LyricsCursor.at(doc, 6000).lineIndex)
    }

    @Test
    fun unsyncedIsAlwaysEmpty() {
        val doc = lyrics(LyricsTier.Unsynced, listOf(line(null, null, "text")))
        assertEquals(LyricsCursor.EMPTY, LyricsCursor.at(doc, 99_999))
    }
}
