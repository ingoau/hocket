package app.hocket.core

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/** What the native seam logs about an event it could not decode: never the payload. */
class NativeCoreLogTest {
    private val token = "t=0123456789abcdef&s=saltsalt"
    // A Backend.Load whose shape drifted (a field of the wrong type), carrying a stream URL with auth.
    private val drifted = """{"type":"backend","data":{"command":{"type":"load","data":{"source":{"key":"k","url":"https://music.example.net/rest/stream?id=1&$token","headers":{}},"start_ms":"soon"}}}}"""

    @Test
    fun theLogLineNamesTheEventTypeButNeverQuotesTheJson() {
        val error = runCatching { HocketJson.decodeEvent(drifted) }.exceptionOrNull()
        assertTrue("the fixture must fail to decode", error != null)
        assertTrue("kotlinx quotes the input in its message", error!!.message.orEmpty().contains("JSON input") || error.message.orEmpty().contains(token))
        val line = NativeCore.undecodable(drifted, error)
        assertTrue(line, line.contains("backend"))
        assertFalse(line, line.contains(token))
        assertFalse(line, line.contains("music.example.net"))
    }

    @Test
    fun eventTypeReadsOnlyTheTag() {
        assertEquals("backend", NativeCore.eventType(drifted))
        assertEquals("?", NativeCore.eventType("not json"))
    }
}
