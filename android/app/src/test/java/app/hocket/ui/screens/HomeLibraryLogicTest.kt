package app.hocket.ui.screens

import app.hocket.ui.screens.home.HomeSection
import app.hocket.ui.screens.home.HomeSections
import app.hocket.ui.screens.home.SectionPref
import app.hocket.ui.screens.library.LibraryPins
import app.hocket.ui.screens.library.PinEntry
import app.hocket.ui.screens.library.GroupEdges
import app.hocket.ui.screens.library.PinKind
import app.hocket.ui.screens.library.PinTap
import app.hocket.ui.screens.library.pinGroupEdges
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/** Home's `home.sections` and the Library's `library.pins`: parsing, moving, pinning and the pin editor's draft. */
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

    @Test
    fun draggingAPinPutsItBeforeOrAfterTheOneItLandsOn() {
        val a = PinEntry("album", "a")
        val gone = PinEntry("album", "gone")
        val b = PinEntry("playlist", "b")
        val c = PinEntry("genre", "c")
        val pins = listOf(a, gone, b, c)
        assertEquals(listOf(gone, b, c, a), LibraryPins.movedPast(pins, a, c, later = true))
        assertEquals(listOf(c, a, gone, b), LibraryPins.movedPast(pins, c, a, later = false))
        assertEquals(listOf(a, gone, c, b), LibraryPins.movedPast(pins, c, b, later = false))
        assertEquals(pins, LibraryPins.movedPast(pins, a, a, later = true))
        assertEquals(pins, LibraryPins.movedPast(pins, PinEntry("album", "nope"), a, later = true))
    }

    @Test
    fun unpinningInTheDraftCanBeUndoneInPlace() {
        val a = PinEntry("album", "a")
        val b = PinEntry("artist", "b", "shuffle")
        val c = PinEntry("filter", "c")
        val (without, at) = LibraryPins.unpinned(listOf(a, b, c), b)
        assertEquals(listOf(a, c), without)
        assertEquals(1, at)
        assertEquals(listOf(a, b, c), LibraryPins.restored(without, b, at))
        assertEquals(listOf(a, c) to -1, LibraryPins.unpinned(without, b))
        // The last pin out leaves an empty draft (Done then saves "[]").
        assertEquals(emptyList<PinEntry>() to 0, LibraryPins.unpinned(listOf(a), a))
        assertEquals("[]", LibraryPins.encode(emptyList()))
    }

    @Test
    fun theChipStepsOpenPlayShuffleAndOnlyTheOnePinChanges() {
        assertEquals(listOf(PinTap.Play, PinTap.Shuffle, PinTap.Open), PinTap.entries.map { it.next })
        val a = PinEntry("genre", "Folk")
        val b = PinEntry("album", "b", "play", mapOf("serverId" to kotlinx.serialization.json.JsonPrimitive("s")))
        val pins = listOf(a, b)
        val cycled = LibraryPins.withTap(pins, b, b.pinTap.next)
        assertEquals(listOf(a, b.copy(tap = "shuffle")), cycled)
        assertEquals(b.extra, cycled[1].extra)
    }

    @Test
    fun everythingButSongsChoosesItsTap() {
        assertEquals(listOf(PinKind.Track), PinKind.entries.filterNot { it.choosesTap })
    }

    @Test
    fun theDraftIsChangedOnlyWhenItDiffers() {
        val a = PinEntry("album", "a")
        val b = PinEntry("album", "b")
        val saved = listOf(a, b)
        assertFalse(LibraryPins.changed(saved, LibraryPins.parse(LibraryPins.encode(saved))))
        assertTrue(LibraryPins.changed(saved, listOf(b, a)))
        assertTrue(LibraryPins.changed(saved, listOf(a)))
        assertTrue(LibraryPins.changed(saved, LibraryPins.withTap(saved, a, PinTap.Play)))
        // Moved away and back again is no change.
        assertFalse(LibraryPins.changed(saved, LibraryPins.moved(LibraryPins.moved(saved, a, 1), a, -1)))
    }

    @Test
    fun theGroupsCornersFollowTheDraft() {
        fun edges(n: Int) = (0 until n).map { pinGroupEdges(it, n) }
        val lone = GroupEdges(top = true, bottom = true, start = true, end = true)
        assertEquals(listOf(lone), edges(1))
        // Three: the second pin has nothing under it, so it takes the bottom-end corner; the lone last one both bottom corners.
        assertEquals(
            listOf(
                GroupEdges(top = true, bottom = false, start = true, end = false),
                GroupEdges(top = true, bottom = true, start = false, end = true),
                GroupEdges(top = false, bottom = true, start = true, end = true),
            ),
            edges(3),
        )
        // Four: an even last row, outer corners at the four ends only.
        assertEquals(listOf(true, true, false, false), edges(4).map { it.top })
        assertEquals(listOf(false, false, true, true), edges(4).map { it.bottom })
        assertEquals(listOf(false, true, false, true), edges(4).map { it.end })
    }
}
