package app.hocket.playback.pebble

import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Test

/** The byte layouts the watchapp parses (pebble/src/c/main.c, list.c). */
class PebbleProtocolTest {
    @Test
    fun rowsArePackedFlagsThenThreeTerminatedStrings() {
        val bytes = PebbleProtocol.encodeRows(listOf(PebbleProtocol.Row("a", "B", "", PebbleProtocol.RowFlag.TRACK)))
        assertArrayEquals(byteArrayOf(8, 'a'.code.toByte(), 0, 'B'.code.toByte(), 0, 0), bytes)
    }

    @Test
    fun longTextIsCutOnACharacterBoundary() {
        val cut = PebbleProtocol.utf8Prefix("a" + "é".repeat(30), 4)
        assertEquals("aé", String(cut, Charsets.UTF_8))
    }

    @Test
    fun settingsBlobMatchesTheWatchLayout() {
        val blob = PebbleProtocol.encodeSettings(PebbleProtocol.WatchSettings(gestures = listOf("queue", "lyrics", "library", "back", "actions"), menu = listOf("queue", "lyrics"), actions = listOf("love")))
        assertArrayEquals(byteArrayOf(1, 28, 1, 5, 2, 4, 3, 5, 2, 1, 15, 1, 3), blob)
    }

    @Test
    fun artPutsTheFirstPixelInTheHighNibble() {
        // Three pixels: red, blue, red. Red is used most, so it is palette entry 0.
        val art = PebbleProtocol.encodeArt(intArrayOf(0xffff0000.toInt(), 0xff0000ff.toInt(), 0xffff0000.toInt()), 3, 1)
        assertEquals(0xC0 or 0x30, art.palette[0].toInt() and 0xff)
        assertEquals(0xC0 or 0x03, art.palette[1].toInt() and 0xff)
        assertArrayEquals(byteArrayOf(0x01, 0x00), art.data)
    }
}
