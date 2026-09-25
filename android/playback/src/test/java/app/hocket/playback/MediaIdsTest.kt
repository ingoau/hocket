package app.hocket.playback

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

/** The ids other apps hold on to: every kind round-trips, whatever its segments contain. */
class MediaIdsTest {
    @Test
    fun everyKindRoundTrips() {
        val ids = listOf(
            MediaId.Root,
            MediaId.Section(LibrarySection.Albums), MediaId.Section(LibrarySection.Genres),
            MediaId.Album("s1", "al1"), MediaId.AlbumTrack("s1", "al1", 3),
            MediaId.Artist("s1", "ar/1"), MediaId.Playlist("s1", "pl:9"), MediaId.PlaylistTrack("s1", "pl:9", 0),
            MediaId.Genre("s1", "Drum/Bass & Jungle"), MediaId.Genre("s1", "Électro 100%"),
            MediaId.Track("server with spaces", "t1"), MediaId.QueueItem("k-42"),
        )
        for (id in ids) assertEquals(id, MediaId.parse(id.format()))
    }

    @Test
    fun segmentsAreEncodedSoSlashesCannotShiftFields() {
        assertEquals("genre/s1/Drum%2FBass", MediaId.Genre("s1", "Drum/Bass").format())
        assertEquals(MediaId.Artist("a/b", "c"), MediaId.parse(MediaId.Artist("a/b", "c").format()))
    }

    @Test
    fun foreignOrMalformedIdsAreRejected() {
        for (bad in listOf("", "album", "album/s1", "album/s1/al1/x", "album/s1/al1/-1", "album//al1", "track/s1", "queue", "unknown/a/b", "album/s1/%zz"))
            assertNull(bad, MediaId.parse(bad))
    }
}
