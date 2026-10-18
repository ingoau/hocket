package app.hocket.playback.pebble

import java.io.ByteArrayOutputStream
import java.util.UUID

/**
 * The messages between Hocket and its watchapp (`pebble/src/c/hocket.h` is the other half; keep the
 * two in step and bump [PROTO] on any incompatible change). Pure encoding, no Android, so it is
 * unit-testable.
 */
object PebbleProtocol {
    val APP_UUID: UUID = UUID.fromString("2447e130-2efa-41b2-a729-e57ee2f531b2")
    const val PROTO = 1

    /** Rows per list page; the watch caches two pages. */
    const val PAGE = 12

    /** Album-art bytes per message (the watch's inbox is 4 KiB). */
    const val ART_CHUNK = 1800

    // Message keys (pebble/package.json "messageKeys")
    object Key {
        const val TYPE = 1u
        const val PROTO = 2u
        const val BUILD = 3u
        const val TITLE = 10u
        const val ARTIST = 11u
        const val ALBUM = 12u
        const val DURATION = 13u
        const val POSITION = 14u
        const val FLAGS = 15u
        const val REPEAT = 16u
        const val RATING = 17u
        const val RESUME_LABEL = 18u
        const val ART_GEN = 19u
        const val VOLUME = 20u
        const val REQ_ID = 30u
        const val OFFSET = 31u
        const val TOTAL = 32u
        const val ITEMS = 33u
        const val LIST_TITLE = 34u
        const val ART_OFFSET = 41u
        const val ART_DATA = 42u
        const val ART_SIZE = 43u
        const val ART_WIDTH = 44u
        const val ART_HEIGHT = 45u
        const val ART_PALETTE = 46u
        const val SETTINGS = 50u
        const val CMD = 60u
        const val ARG = 61u
        const val PATH = 62u
        const val ITEM = 63u
        const val NOTICE = 70u
    }

    object Type {
        const val NOW = 1
        const val LIST = 2
        const val ART = 3
        const val SETTINGS = 4
        const val HELLO_REPLY = 5
        const val NOTICE = 6
        const val HELLO = 10
        const val CMD = 11
        const val LIST_REQ = 12
        const val ART_REQ = 13
    }

    object Flag {
        const val PLAYING = 1
        const val SHUFFLE = 2
        const val LOVED = 4
        const val HAS_TRACK = 8
        const val CAN_RESUME = 16
        const val HAS_VOLUME = 32
        const val REPORTS_POSITION = 64
    }

    object Cmd {
        const val PLAY_PAUSE = 1
        const val PLAY = 2
        const val PAUSE = 3
        const val NEXT = 4
        const val PREVIOUS = 5
        const val VOLUME_UP = 6
        const val VOLUME_DOWN = 7
        const val TOGGLE_LOVE = 8
        const val SET_RATING = 9
        const val TOGGLE_SHUFFLE = 10
        const val CYCLE_REPEAT = 11
        const val RESUME = 12
        const val ACTIVATE = 13
        const val PLAY_NEXT = 14
        const val PLAY_LATER = 15
        const val SEEK = 16
    }

    /** Max volume steps one VOLUME_UP / VOLUME_DOWN applies (the watch caps its merged count too). */
    const val MAX_VOLUME_STEPS = 20

    /**
     * Steps in a VOLUME_UP / VOLUME_DOWN: the watch sends Arg = presses (merged while its outbox was
     * busy); an older watch sent 0, meaning one.
     */
    fun volumeSteps(arg: Int): Int = arg.coerceIn(1, MAX_VOLUME_STEPS)

    object RowFlag {
        const val BROWSABLE = 1
        const val CURRENT = 2
        const val SEARCH = 4
        const val TRACK = 8
        const val HEADER = 16
    }

    /** One list row as the watch shows it. [id] is a path when [RowFlag.BROWSABLE] is set. */
    data class Row(val id: String, val title: String, val subtitle: String = "", val flags: Int = 0)

    /** The watch's field sizes (`Row` in list.c), less the terminator. */
    private const val ID_MAX = 47
    private const val TITLE_MAX = 47
    private const val SUB_MAX = 39

    /** `[flags][id\0][title\0][sub\0]…`, each field cut to what the watch keeps, on a UTF-8 boundary. */
    fun encodeRows(rows: List<Row>): ByteArray {
        val out = ByteArrayOutputStream()
        for (r in rows) {
            out.write(r.flags and 0xff)
            for ((text, max) in listOf(r.id to ID_MAX, r.title to TITLE_MAX, r.subtitle to SUB_MAX)) {
                out.write(utf8Prefix(text, max))
                out.write(0)
            }
        }
        return out.toByteArray()
    }

    /** The longest prefix of [text] that fits [maxBytes] of UTF-8 without splitting a character. */
    fun utf8Prefix(text: String, maxBytes: Int): ByteArray {
        val bytes = text.replace('\u0000', ' ').toByteArray(Charsets.UTF_8)
        if (bytes.size <= maxBytes) return bytes
        var end = maxBytes
        while (end > 0 && (bytes[end].toInt() and 0xC0) == 0x80) end--
        return bytes.copyOf(end)
    }

    /**
     * A queue item's row id. The watch keeps [ID_MAX] bytes of an id and sends back what it kept, but
     * queue keys run longer (an op id with the device id in it, plus a slot number for an upcoming
     * song, #97), so a cut key named nothing and tapping or holding the row did nothing. A key that
     * fits goes as is; a longer one as `q:` and a 64-bit FNV-1a hash of it, which the bridge maps back
     * by hashing the queue's keys.
     */
    fun queueRowId(key: String): String {
        val bytes = key.toByteArray(Charsets.UTF_8)
        if (bytes.size <= ID_MAX && !key.startsWith("q:")) return key
        var h = 0xcbf29ce484222325uL.toLong()
        for (b in bytes) {
            h = h xor (b.toLong() and 0xff)
            h *= 0x100000001b3L
        }
        return "q:" + java.lang.Long.toHexString(h)
    }

    // -- Settings ----------------------------------------------------------------------------------

    val GESTURES = listOf("none", "queue", "library", "actions", "back", "lyrics")

    /** Menu and action ids, in the watch's numbering (`MenuItemId` in hocket.h). */
    val MENU_ITEMS = listOf(
        "nowPlaying", "queue", "library", "love", "rating", "shuffle", "repeat", "goAlbum",
        "goArtist", "playOn", "playlists", "albums", "artists", "downloads", "search", "lyrics",
    )
    val DEFAULT_MENU = listOf("nowPlaying", "queue", "library", "love", "rating", "shuffle", "repeat", "goAlbum", "goArtist", "playOn")
    val DEFAULT_ACTIONS = listOf("rating", "love", "shuffle", "repeat", "goAlbum", "goArtist", "playOn")
    val ACTION_ITEMS = DEFAULT_ACTIONS + "lyrics"
    val LIBRARY_SECTIONS = listOf("playlists", "albums", "artists", "recent", "downloads", "search")

    data class WatchSettings(
        val touchNavigation: Boolean = false,
        val buttonsOnly: Boolean = false,
        val showVolumeControls: Boolean = true,
        val showProgressBar: Boolean = true,
        val showAlbumArt: Boolean = true,
        /** up, down, left, right, hold */
        val gestures: List<String> = listOf("queue", "none", "library", "back", "actions"),
        val menuTimeoutSeconds: Int = 5,
        val menu: List<String> = DEFAULT_MENU,
        val actions: List<String> = DEFAULT_ACTIONS,
        val library: List<String> = LIBRARY_SECTIONS,
        /** Minutes paused before the watchapp closes itself; 0 = never. */
        val closeWhenPausedMinutes: Int = 0,
    )

    /**
     * `[1][flags][5 gestures][timeout][n][menu…][n][actions…][close when paused, minutes]`
     * (`prv_parse_settings` in main.c). The trailing byte is optional to the watch, so older
     * watchapps still read the rest.
     */
    fun encodeSettings(s: WatchSettings): ByteArray {
        val out = ByteArrayOutputStream()
        out.write(1)
        var flags = 0
        if (s.touchNavigation) flags = flags or 1
        if (s.buttonsOnly) flags = flags or 2
        if (s.showVolumeControls) flags = flags or 4
        if (s.showProgressBar) flags = flags or 8
        if (s.showAlbumArt) flags = flags or 16
        out.write(flags)
        for (i in 0 until 5) out.write(GESTURES.indexOf(s.gestures.getOrNull(i)).coerceAtLeast(0))
        out.write(s.menuTimeoutSeconds.coerceIn(0, 255))
        for (list in listOf(s.menu, s.actions)) {
            val codes = list.map { MENU_ITEMS.indexOf(it) }.filter { it >= 0 }.take(16)
            out.write(codes.size)
            codes.forEach(out::write)
        }
        out.write(s.closeWhenPausedMinutes.coerceIn(0, 255))
        return out.toByteArray()
    }

    /** `m:ss` (or `h:mm:ss`), as track rows show a duration. */
    fun duration(ms: Long): String {
        val s = (ms / 1000).coerceAtLeast(0)
        return if (s >= 3600) "%d:%02d:%02d".format(s / 3600, s / 60 % 60, s % 60) else "%d:%02d".format(s / 60, s % 60)
    }

    // -- Album art ---------------------------------------------------------------------------------

    /** A 4-bit palettised cover: 16 `GColor8` bytes and rows packed at `(width + 1) / 2` bytes. */
    class Art(val width: Int, val height: Int, val palette: ByteArray, val data: ByteArray)

    /**
     * [argb] (row-major, [width] × [height]) reduced to the Pebble's 64 colours, then to the 16 of
     * those used most; each pixel takes the nearest of the 16. Two pixels a byte, the first in the
     * high nibble (`raw_image_get_value_for_bitdepth` in PebbleOS).
     */
    fun encodeArt(argb: IntArray, width: Int, height: Int): Art {
        require(argb.size >= width * height)
        val codes = IntArray(width * height) { i ->
            val c = argb[i]
            (level(c shr 16) shl 4) or (level(c shr 8) shl 2) or level(c)
        }
        val counts = IntArray(64)
        for (c in codes) counts[c]++
        val chosen = (0 until 64).filter { counts[it] > 0 }.sortedByDescending { counts[it] }.take(16)
        // The nearest chosen colour for each of the 64, by distance in the 2-bit cube.
        val nearest = IntArray(64) { c ->
            chosen.indices.minBy { i ->
                val d = chosen[i]
                sq((c shr 4) - (d shr 4)) + sq((c shr 2 and 3) - (d shr 2 and 3)) + sq((c and 3) - (d and 3))
            }
        }
        val palette = ByteArray(16) { i -> (0xC0 or (chosen.getOrNull(i) ?: 0)).toByte() }
        val stride = (width + 1) / 2
        val data = ByteArray(stride * height)
        for (y in 0 until height) for (x in 0 until width) {
            val index = nearest[codes[y * width + x]]
            val at = y * stride + x / 2
            data[at] = (data[at].toInt() or if (x % 2 == 0) index shl 4 else index).toByte()
        }
        return Art(width, height, palette, data)
    }

    private fun level(channel: Int): Int = ((channel and 0xff) + 42) / 85
    private fun sq(v: Int) = v * v

    /** A cover's change counter for the watch: nonzero for a cover, 0 for none. */
    fun artGeneration(artworkPath: String?): Int =
        if (artworkPath.isNullOrEmpty()) 0 else (artworkPath.hashCode() and 0xffff).let { if (it == 0) 1 else it }
}
