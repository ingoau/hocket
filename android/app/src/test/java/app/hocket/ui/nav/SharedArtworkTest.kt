package app.hocket.ui.nav

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class SharedArtworkTest {
    @Test
    fun preparePairsOneSourceWithItsDestination() {
        val session = SharedArtworkSession()
        val source = "grid-cell"
        session.prepare("album:al1", source, "cover-1")
        assertTrue(session.matches("album:al1", source = true, sourceId = source))
        assertFalse(session.matches("album:al1", source = true, sourceId = "other-cell"))
        assertTrue(session.matches("album:al1", source = false, sourceId = null))
        assertFalse(session.matches("album:al2", source = false, sourceId = null))
        assertEquals("cover-1", session.coverArtFor("album:al1"))
    }

    @Test
    fun prepareAllLetsTheFanFlyTogetherThenALaterOpenReplacesIt() {
        val session = SharedArtworkSession()
        val a = SharedArtworkEntry("albums-tile:al1", "fan-0", "c1")
        val b = SharedArtworkEntry("albums-tile:al2", "fan-1", "c2")
        val c = SharedArtworkEntry("albums-tile:al3", "fan-2", "c3")
        session.prepareAll(listOf(a, b, c))
        assertTrue(session.matches(a.key, source = true, sourceId = a.sourceId))
        assertTrue(session.matches(b.key, source = true, sourceId = b.sourceId))
        assertTrue(session.matches(c.key, source = false, sourceId = null))
        assertEquals("c2", session.coverArtFor("albums-tile:al2"))

        session.prepare("album:al1", "recent-card", "c1")
        assertFalse(session.matches(a.key, source = false, sourceId = null))
        assertTrue(session.matches("album:al1", source = true, sourceId = "recent-card"))
        assertEquals("c1", session.coverArtFor("album:al1"))
    }

    @Test
    fun albumsTileKeyDiffersFromTheAlbumDetailKey() {
        assertEquals("albums-tile:al1", ArtworkShare.albumsTile("al1"))
        assertEquals("album:al1", ArtworkShare.album("al1"))
    }
}
