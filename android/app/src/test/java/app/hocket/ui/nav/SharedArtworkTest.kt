package app.hocket.ui.nav

import app.hocket.ui.ScreenCache
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class SharedArtworkTest {
    @Test
    fun theNextPagePairsOneSourceWithItself() {
        val session = SharedArtworkSession()
        session.attach("home")
        session.prepare("album:al1", "grid-cell", "cover-1")
        // Prepared but not yet pushed: nothing flies.
        assertFalse(session.matchesSource("album:al1", "grid-cell"))
        assertTrue(session.attach("album-page"))
        assertTrue(session.matchesSource("album:al1", "grid-cell"))
        assertFalse(session.matchesSource("album:al1", "other-cell"))
        assertTrue(session.matchesDestination("album-page", "album:al1"))
        assertFalse(session.matchesDestination("album-page", "album:al2"))
        assertFalse(session.matchesDestination("home", "album:al1"))
        assertEquals("cover-1", session.coverArtFor("album-page", "album:al1"))
    }

    @Test
    fun aPageKeepsItsFlightWhilePagesAboveItComeAndGo() {
        val session = SharedArtworkSession()
        session.attach("home")
        session.prepare("album:al1", "home-card", "c1")
        session.attach("album-1")
        // Deeper: the album's artist, then the same album again from the artist's strip.
        session.attach("artist")
        session.prepare("album:al1", "artist-strip", "c1")
        session.attach("album-1-again")
        assertTrue(session.matchesSource("album:al1", "artist-strip"))

        // Pop back to the artist, then to the first album page: the first flight is still there.
        session.attach("artist")
        session.release("album-1-again")
        assertFalse(session.matchesSource("album:al1", "artist-strip"))
        session.attach("album-1")
        assertTrue(session.matchesDestination("album-1", "album:al1"))
        assertTrue(session.matchesSource("album:al1", "home-card"))

        // And the pop to Home pairs the Home card with it; once the page is gone, nothing does.
        session.attach("home")
        assertTrue(session.matchesSource("album:al1", "home-card"))
        session.release("album-1")
        assertFalse(session.matchesSource("album:al1", "home-card"))
    }

    @Test
    fun aPopDoesNotTakeAFlightThatWasNeverPushed() {
        val session = SharedArtworkSession()
        session.attach("home")
        session.attach("settings")
        session.prepare("album:al1", "card", "c1")
        assertFalse(session.attach("home"))
        assertFalse(session.matchesDestination("home", "album:al1"))
        // The stale preparation is spent, not handed to the next page pushed.
        session.attach("album-page")
        assertFalse(session.matchesDestination("album-page", "album:al1"))
    }

    @Test
    fun theFanFliesTogetherCarryingItsPose() {
        val session = SharedArtworkSession()
        val a = SharedArtworkEntry("albums-tile:al1", "fan-0", "c1", rotation = -8f, zIndex = 3f)
        val b = SharedArtworkEntry("albums-tile:al2", "fan-1", "c2", rotation = 4f, zIndex = 2f)
        val c = SharedArtworkEntry("albums-tile:al3", "fan-2", "c3", rotation = 12f, zIndex = 1f)
        session.attach("library")
        session.prepareAll(listOf(a, b, c))
        session.attach("albums")
        assertTrue(session.matchesSource(a.key, a.sourceId))
        assertTrue(session.matchesSource(b.key, b.sourceId))
        assertTrue(session.matchesDestination("albums", c.key))
        assertEquals(b, session.entryFor("albums", b.key))
        assertEquals("c2", session.coverArtFor("albums", "albums-tile:al2"))

        // A Recent card opened from there flies on its own; popping back to the albums page still
        // has the fan's entries for the flight home to the tile.
        session.prepare("album:al1", "recent-card", "c1")
        session.attach("album")
        assertTrue(session.matchesDestination("album", "album:al1"))
        assertFalse(session.matchesDestination("album", a.key))
        session.attach("albums")
        session.release("album")
        assertTrue(session.matchesSource(a.key, a.sourceId))
    }

    @Test
    fun destinationsPreparedWithoutASourceMatchNoSource() {
        val session = SharedArtworkSession()
        session.attach("home")
        session.prepareDestination("album:al1", "c1")
        session.attach("album")
        assertTrue(session.matchesDestination("album", "album:al1"))
        assertFalse(session.matchesSource("album:al1", "any-card"))
        assertNull(session.coverArtFor(null, "album:al1"))
    }

    @Test
    fun albumsTileKeyDiffersFromTheAlbumDetailKey() {
        assertEquals("albums-tile:al1", ArtworkShare.albumsTile("al1"))
        assertEquals("album:al1", ArtworkShare.album("al1"))
    }

    @Test
    fun screenCacheStartsFromTheLastValueAndWritesThrough() {
        val cache = ScreenCache(max = 2)
        val first = cache.state<List<String>?>("k") { null }
        assertNull(first.value)
        first.value = listOf("a")
        assertEquals(listOf("a"), cache.state<List<String>?>("k") { null }.value)
        val (_, set) = cache.state<List<String>?>("k") { null }
        set(listOf("b"))
        assertEquals(listOf("b"), cache.get<List<String>>("k"))
        cache.put("x", 1)
        cache.put("y", 2)
        assertFalse(cache.contains("k"))
    }
}
