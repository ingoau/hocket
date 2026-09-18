package app.hocket.core.client

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class SelectionTest {
    @Test
    fun toggleAndCount() {
        val s = Selection().toggle("a").toggle("b").toggle("a")
        assertEquals(setOf("b"), s.ids)
        assertEquals(1, s.count)
        assertTrue(s.active)
        assertFalse(s.clear().active)
    }

    @Test
    fun selectAllIsAPredicateWithExclusions() {
        val s = Selection().selectAll(50_000).toggle("x")
        assertTrue(s.contains("anything"))
        assertFalse(s.contains("x"))
        assertEquals(49_999, s.count)
        assertFalse(s.isAll)
        assertTrue(Selection().selectAll(3).isAll)
    }

    @Test
    fun resolveAgainstKnownIds() {
        val all = listOf("a", "b", "c")
        assertEquals(listOf("b"), Selection().toggle("b").resolve(all))
        assertEquals(listOf("a", "c"), Selection().selectAll(3).toggle("b").resolve(all))
    }
}
