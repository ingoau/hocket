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
        assertEquals(LyricsCursor.EMPTY, LyricsCursor.at(syllableDoc, 500))
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
    fun offsetShiftsTiming() {
        val shifted = syllableDoc.copy(offsetMs = -500)
        assertEquals(LyricsCursor.EMPTY, LyricsCursor.at(shifted, 1200))
        assertEquals(0, LyricsCursor.at(shifted, 1600).lineIndex)
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
