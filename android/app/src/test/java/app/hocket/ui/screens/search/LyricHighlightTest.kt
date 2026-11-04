package app.hocket.ui.screens.search

import org.junit.Assert.assertEquals
import org.junit.Test

class LyricHighlightTest {
    @Test
    fun anEmojiBeforeTheMatchDoesNotShiftTheHighlight() {
        // One code point, two UTF-16 units. The core's offsets count code points.
        val line = "\uD83C\uDFB6 night was"
        val (start, end) = lyricUtf16Range(line, 2, 7)
        assertEquals("night", line.substring(start, end))
    }

    @Test
    fun anOffsetPastTheEndClampsInsteadOfThrowing() {
        val (start, end) = lyricUtf16Range("night", 40, 2)
        assertEquals(5, start)
        assertEquals(5, end)
    }
}
