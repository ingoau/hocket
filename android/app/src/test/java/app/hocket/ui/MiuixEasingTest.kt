package app.hocket.ui

import app.hocket.ui.nav.MiuixEasing
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

/** Miuix's programmatic settle curve: starts at 0, never overshoots, ~half done at a fifth of the duration. */
class MiuixEasingTest {
    @Test
    fun startsAtZeroEndsNearOneAndIsMonotone() {
        val e = MiuixEasing
        assertEquals(0f, e.transform(0f), 1e-5f)
        val end = e.transform(1f)
        assertTrue("ends near 1: $end", end in 0.99f..1.001f)
        var prev = -1f
        for (i in 0..40) {
            val v = e.transform(i / 40f)
            assertTrue("monotone at ${i / 40f}: $v after $prev", v + 1e-4f >= prev)
            prev = v
        }
        val fifth = e.transform(0.2f)
        assertTrue("brisk middle (~48% at t=0.2): $fifth", fifth in 0.40f..0.58f)
    }
}
