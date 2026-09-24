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

    // The same event cut short: kotlinx's message then quotes the input around the failure.
    private val truncated = drifted.substring(0, drifted.indexOf(token) + token.length + 3)

    @Test
    fun theLogLineNamesTheEventTypeButNeverQuotesTheJson() {
        for (json in listOf(drifted, truncated)) {
            val error = runCatching { HocketJson.decodeEvent(json) }.exceptionOrNull()
            assertTrue("the fixture must fail to decode", error != null)
            val line = NativeCore.undecodable(json, error!!)
            assertTrue(line, line.contains("backend"))
            assertFalse(line, line.contains(token))
            assertFalse(line, line.contains("music.example.net"))
        }
        val quoted = runCatching { HocketJson.decodeEvent(truncated) }.exceptionOrNull()!!.message.orEmpty()
        assertTrue("kotlinx quotes the input (why the log line strips it): $quoted", quoted.contains("JSON input"))
    }

    @Test
    fun eventTypeReadsOnlyTheTag() {
        assertEquals("backend", NativeCore.eventType(drifted))
        assertEquals("?", NativeCore.eventType("not json"))
    }
}
