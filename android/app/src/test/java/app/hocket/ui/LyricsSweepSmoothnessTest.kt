package app.hocket.ui

import app.hocket.ui.lyrics.PlayheadSmoother
import app.hocket.ui.lyrics.sweepEdge
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

/** The word sweep never steps back: not where syllables hand over, not when the position is re-synced. */
class LyricsSweepSmoothnessTest {
    /** A word of three syllables, 40, 60 and 20 px wide, swept through in order (from the right for [rtl] text). */
    private fun edges(rtl: Boolean): List<Float> {
        val syllables = if (rtl) listOf(80f to 120f, 20f to 80f, 0f to 20f) else listOf(0f to 40f, 40f to 100f, 100f to 120f)
        return syllables.flatMap { (l, r) -> (0..10).map { sweepEdge(0f, 120f, l, r, it / 10f, 30f, rtl) } }
    }

    @Test
    fun theEdgeMovesOnlyForwardThroughAWordAcrossSyllables() {
        val ltr = edges(rtl = false)
        assertTrue("left to right: $ltr", ltr.zipWithNext().all { (a, b) -> b >= a - 0.001f })
        assertEquals("nothing sung: the unsung side starts at the word", -30f, ltr.first(), 0.001f)
        assertEquals("all sung: the sung side reaches the end", 120f, ltr.last(), 0.001f)
        val rtl = edges(rtl = true)
        assertTrue("right to left: $rtl", rtl.zipWithNext().all { (a, b) -> b <= a + 0.001f })
        assertEquals(150f, rtl.first(), 0.001f)
        assertEquals(0f, rtl.last(), 0.001f)
    }

    @Test
    fun aReSyncBehindHoldsThePlayheadAndCatchesUpAtHalfPace() {
        val s = PlayheadSmoother()
        assertEquals(1_000L, s.next(1_000, moving = true))
        assertEquals(1_016L, s.next(1_016, moving = true))
        // The player's stamp lands 80 ms behind: held, not stepped back.
        assertEquals(1_016L, s.next(936, moving = true))
        // Then it moves on at half the real pace until the two meet.
        assertEquals(1_024L, s.next(952, moving = true))
        var last = 1_024L
        var raw = 952L
        repeat(20) { raw += 16; val shown = s.next(raw, moving = true); assertTrue(shown >= last); last = shown }
        assertEquals("caught up", raw, last)
        // Standing still: the playhead stands still too (it never runs ahead on its own).
        assertEquals(raw, s.next(raw, moving = true))
        // A seek back, or a paused position, is taken as it is.
        assertEquals(200L, s.next(200, moving = true))
        assertEquals(150L, s.next(150, moving = false))
    }
}
