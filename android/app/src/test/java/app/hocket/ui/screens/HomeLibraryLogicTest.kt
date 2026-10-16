package app.hocket.ui.screens

import app.hocket.ui.screens.home.HomeSection
import app.hocket.ui.screens.home.HomeSections
import app.hocket.ui.screens.home.SectionPref
import app.hocket.ui.screens.library.LibraryPins
import app.hocket.ui.screens.library.PinEntry
import app.hocket.ui.screens.library.PinKind
import org.junit.Assert.assertEquals
import org.junit.Test

/** Home's `home.sections` and the Library's `library.pins`: parsing, moving and pinning. */
class HomeLibraryLogicTest {
    /**
     * The catalogue, in its default order with each section's default: the desktop's
     * `lib/home-sections.test.ts` checks the same list, so the two platforms stay in step.
     */
    @Test
    fun catalogueMatchesTheDesktops() {
        assertEquals(
            listOf(
                "rediscover:on:row", "filters:on:row", "favouriteArtists:on:row", "topGenre:on:grid", "newInLibrary:on:row",
                "moreFromArtist:off:list", "onRepeat:off:row", "recentlyPlayed:off:row", "recentQueues:off:list", "stats:off:tile",
            ),
            HomeSection.entries.map { "${it.id}:${if (it.defaultOn) "on" else "off"}:${it.defaultStyle.id}" },
        )
    }

    @Test
    fun sectionsParseKeepingUnknownIdsAndAppendingNewOnesOff() {
        assertEquals(HomeSections.DEFAULT, HomeSections.parse("[]"))
        assertEquals(HomeSections.DEFAULT, HomeSections.parse("not json"))
        val parsed = HomeSections.parse("""[{"id":"stats","on":true},{"id":"fromTheFuture","on":true,"style":"wall"},{"id":"stats","on":false}]""")
        assertEquals(SectionPref("stats", true, null), parsed[0])
        assertEquals(SectionPref("fromTheFuture", true, "wall"), parsed[1])
        assertEquals(HomeSection.entries.size + 1, parsed.size)
        assertEquals(listOf(false), parsed.drop(2).map { it.on }.distinct())
    }

    @Test
    fun movingASectionSkipsHiddenOnes() {
        val prefs = listOf(SectionPref("rediscover", true, null), SectionPref("stats", false, null), SectionPref("unknown", true, null), SectionPref("filters", true, null))
        assertEquals(listOf("stats", "unknown", "filters", "rediscover"), HomeSections.moveShown(prefs, "rediscover", 1).map { it.id })
        assertEquals(prefs, HomeSections.moveShown(prefs, "rediscover", -1))
    }

    @Test
    fun pinsKeepFieldsAndKindsThisBuildDoesntKnow() {
        val raw = """[{"kind":"station","id":"s1","tap":"playNext","serverId":"srv"},{"kind":"album","id":"a1"},{"kind":"album","id":"a1"}]"""
        val pins = LibraryPins.parse(raw)
        assertEquals(2, pins.size)
        assertEquals(null, pins[0].pinKind)
        assertEquals("""[{"serverId":"srv","kind":"station","id":"s1","tap":"playNext"},{"kind":"album","id":"a1","tap":"open"}]""", LibraryPins.encode(pins))
    }

    @Test
    fun pinningIsRefusedWhenTheLibraryIsFullButUnpinningIsNot() {
        val full = (0 until LibraryPins.MAX).map { PinEntry("album", "a$it") }
        assertEquals(full, LibraryPins.toggled(full, PinKind.Genre, "Folk"))
        assertEquals(LibraryPins.MAX - 1, LibraryPins.toggled(full, PinKind.Album, "a3").size)
        assertEquals(PinEntry("track", "t1", "play"), LibraryPins.toggled(emptyList(), PinKind.Track, "t1").single())
    }

    @Test
    fun movingAPinSkipsOnesThatArentShown() {
        val a = PinEntry("album", "a")
        val gone = PinEntry("album", "gone")
        val b = PinEntry("playlist", "b")
        val pins = listOf(a, gone, b)
        val shown = listOf(a, b)
        assertEquals(listOf(gone, b, a), LibraryPins.moved(pins, a, 1, shown))
        assertEquals(listOf(b, a, gone), LibraryPins.moved(pins, b, -1, shown))
        assertEquals(null, LibraryPins.shownNeighbour(shown, a, -1))
        assertEquals(pins, LibraryPins.moved(pins, a, -1, shown))
    }

    @Test
    fun undoPutsAPinBackWithoutUndoingLaterEdits() {
        val a = PinEntry("album", "a")
        val b = PinEntry("album", "b")
        val c = PinEntry("album", "c")
        // b was unpinned from index 1, then c was pinned.
        assertEquals(listOf(a, b, c), LibraryPins.restored(listOf(a, c), b, 1))
    }
}
