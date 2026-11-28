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
    fun lyricLinesAreNotCutAtTheListTitleLimit() {
        val line = "A lyric line that runs well past the forty-seven bytes a list title keeps"
        val bytes = PebbleProtocol.encodeLyricRows(listOf(PebbleProtocol.Row("1200", line)))
        assertEquals(line, String(bytes, 6, line.length, Charsets.UTF_8))
        assertEquals(1 + 5 + line.length + 1 + 1, bytes.size)
    }

    @Test
    fun aLyricsPageOfLongLinesCarriesFewerRows() {
        // A row is [flags]["1"\0][title\0][\0].
        val long = PebbleProtocol.encodeLyricRows(List(PebbleProtocol.PAGE) { PebbleProtocol.Row("1", "x".repeat(300)) })
        val longRow = 1 + 2 + PebbleProtocol.LYRIC_MAX + 1 + 1
        assertEquals(PebbleProtocol.LYRICS_PAGE_BYTES / longRow * longRow, long.size)
        val short = PebbleProtocol.encodeLyricRows(List(PebbleProtocol.PAGE + 3) { PebbleProtocol.Row("1", "la") })
        assertEquals(PebbleProtocol.PAGE * (1 + 2 + 3 + 1), short.size)
    }

    @Test
    fun settingsBlobMatchesTheWatchLayout() {
        val blob = PebbleProtocol.encodeSettings(PebbleProtocol.WatchSettings(gestures = listOf("queue", "lyrics", "library", "back", "actions"), menu = listOf("queue", "lyrics"), actions = listOf("love")))
        assertArrayEquals(byteArrayOf(1, 28, 1, 5, 2, 4, 3, 5, 2, 1, 15, 1, 3, 0), blob)
        val closing = PebbleProtocol.encodeSettings(PebbleProtocol.WatchSettings(closeWhenPausedMinutes = 10))
        assertEquals(10, closing.last().toInt())
    }

    @Test
    fun artPutsTheFirstPixelInTheHighNibble() {
        // Three pixels: red, blue, red. Red is used most, so it is palette entry 0.
        val art = PebbleProtocol.encodeArt(intArrayOf(0xffff0000.toInt(), 0xff0000ff.toInt(), 0xffff0000.toInt()), 3, 1)
        assertEquals(0xC0 or 0x30, art.palette[0].toInt() and 0xff)
        assertEquals(0xC0 or 0x03, art.palette[1].toInt() and 0xff)
        assertArrayEquals(byteArrayOf(0x01, 0x00), art.data)
    }

    @Test
    fun stashIsInTheDefaultMenusAtTheWatchsId() {
        assertEquals(true, PebbleProtocol.DEFAULT_ACTIONS.contains("stash"))
        assertEquals(true, PebbleProtocol.DEFAULT_MENU.contains("stash"))
        assertEquals(16, PebbleProtocol.MENU_ITEMS.indexOf("stash"))
        val blob = PebbleProtocol.encodeSettings(PebbleProtocol.WatchSettings())
        // [version][flags][5 gestures][timeout][n menu][menu…][n actions][actions…]
        val menuCount = blob[8].toInt()
        val actionsAt = 9 + menuCount
        val actionCount = blob[actionsAt].toInt()
        val actions = blob.copyOfRange(actionsAt + 1, actionsAt + 1 + actionCount).map { it.toInt() }
        assertEquals(true, actions.contains(16))
    }

    @Test
    fun volumeArgIsAStepCountWithZeroMeaningOne() {
        assertEquals(1, PebbleProtocol.volumeSteps(0))
        assertEquals(1, PebbleProtocol.volumeSteps(1))
        assertEquals(3, PebbleProtocol.volumeSteps(3))
        assertEquals(PebbleProtocol.MAX_VOLUME_STEPS, PebbleProtocol.volumeSteps(1000))
        assertEquals(1, PebbleProtocol.volumeSteps(-5))
    }

    @Test
    fun queueKeysTooLongForTheWatchGoAsAShortStableId() {
        assertEquals("k1", PebbleProtocol.queueRowId("k1"))
        val long = "0123456789abcdef0123456789abcdef-19a2b3c4d5e-123-4.17"
        val id = PebbleProtocol.queueRowId(long)
        assertEquals(id, PebbleProtocol.queueRowId(long))
        assertEquals(true, id.startsWith("q:") && id.length <= 47)
        assertEquals(false, id == PebbleProtocol.queueRowId(long.dropLast(1) + "8"))
    }

    @Test
    fun newItemsHaveTheWatchsIds() {
        // MenuItemId in hocket.h.
        assertEquals(17, PebbleProtocol.MENU_ITEMS.indexOf("queues"))
        assertEquals(18, PebbleProtocol.MENU_ITEMS.indexOf("snooze"))
        assertEquals(19, PebbleProtocol.MENU_ITEMS.indexOf("offline"))
        assertEquals(PebbleProtocol.MAX_MENU_ITEMS, PebbleProtocol.MENU_ITEMS.size)
        assertEquals(true, "snooze" in PebbleProtocol.DEFAULT_ACTIONS)
        assertEquals(true, "offline" in PebbleProtocol.LIBRARY_SECTIONS)
        // Every item fits one settings blob.
        val blob = PebbleProtocol.encodeSettings(PebbleProtocol.WatchSettings(menu = PebbleProtocol.MENU_ITEMS))
        assertEquals(PebbleProtocol.MENU_ITEMS.size, blob[8].toInt())
    }

    @Test
    fun aTrackRowNamesItsAccount() {
        val id = PebbleProtocol.trackRowId("acct-2", "0123456789abcdef0123456789abcdef")
        assertEquals(true, id.toByteArray().size <= 47)
        val (tag, track) = PebbleProtocol.parseTrackRow(id)!!
        assertEquals("0123456789abcdef0123456789abcdef", track)
        assertEquals("acct-2", PebbleProtocol.trackRowSource(tag, listOf("acct-1", "acct-2")))
        // Too long for the tag form: a hashed id instead, which only the bridge's memory maps back.
        val long = PebbleProtocol.trackRowId("acct-2", "x".repeat(60))
        assertEquals(true, long.startsWith("track:#"))
        assertEquals(null, PebbleProtocol.parseTrackRow(long))
    }

    @Test
    fun theQueueIsLaidOutInTheAppsSections() {
        // 20 played, the current one at 20, a group of 2, 5 more from the album.
        val slots = PebbleProtocol.queueSlots(historyCount = 20, currentIndex = 20, groupCount = 2, totalUpcoming = 7, contextLabel = "Night Drives")
        val h = PebbleProtocol.QueueSlot::Header
        val e = PebbleProtocol.QueueSlot::Entry
        assertEquals(
            listOf(h("History")) + (12 until 20).map(e) + h("Now playing") + e(20) +
                h("Next in queue") + e(21) + e(22) + h("Next from: Night Drives") + (23 until 28).map(e),
            slots,
        )
        // The current row is in the first page, where the watch selects it.
        assertEquals(true, slots.indexOf(e(20)) < PebbleProtocol.PAGE)
    }

    @Test
    fun aQueueWithoutAGroupOrContextSaysLater() {
        val slots = PebbleProtocol.queueSlots(historyCount = 0, currentIndex = 0, groupCount = 0, totalUpcoming = 1, contextLabel = null)
        assertEquals(
            listOf(PebbleProtocol.QueueSlot.Header("Now playing"), PebbleProtocol.QueueSlot.Entry(0), PebbleProtocol.QueueSlot.Header("Later"), PebbleProtocol.QueueSlot.Entry(1)),
            slots,
        )
        assertEquals(emptyList<PebbleProtocol.QueueSlot>(), PebbleProtocol.queueSlots(0, null, 0, 0, null))
    }
}
