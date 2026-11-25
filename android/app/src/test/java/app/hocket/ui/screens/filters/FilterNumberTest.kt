package app.hocket.ui.screens.filters

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

/** The filter builder's number fields: text in progress is kept, only numbers are committed (#277). */
class FilterNumberTest {
    @Test
    fun textThatIsNotANumberYetCommitsNothing() {
        assertNull(parseFilterNumber("", whole = false))
        assertNull(parseFilterNumber("-", whole = false))
        assertNull(parseFilterNumber("1e", whole = false))
        assertNull(parseFilterNumber("", whole = true))
        assertNull(parseFilterNumber("1.5", whole = true))
        assertNull(parseFilterNumber("-3", whole = true))
    }

    @Test
    fun numbersCommit() {
        assertEquals(1.0, parseFilterNumber("1.", whole = false)!!, 0.0)
        assertEquals(2.5, parseFilterNumber(" 2.5 ", whole = false)!!, 0.0)
        assertEquals(30.0, parseFilterNumber("30", whole = true)!!, 0.0)
    }

    @Test
    fun aDecimalCommaReadsAsAPoint() {
        assertEquals(2.5, parseFilterNumber("2,5", whole = false)!!, 0.0)
        assertEquals(1.0, parseFilterNumber("1,", whole = false)!!, 0.0)
        assertEquals(-0.25, parseFilterNumber("-0,25", whole = false)!!, 0.0)
        assertNull(parseFilterNumber("1,2,3", whole = false))
        assertNull(parseFilterNumber("1,000.5", whole = false))
        assertNull(parseFilterNumber("2,5", whole = true))
    }

    @Test
    fun valuesShowAsTyped() {
        assertEquals("0", formatFilterNumber(0.0))
        assertEquals("1990", formatFilterNumber(1990.0))
        assertEquals("2.5", formatFilterNumber(2.5))
        assertEquals("10000000", formatFilterNumber(1e7))
    }
}
