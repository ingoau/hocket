package app.hocket.ui.screens

import app.hocket.core.api.ArtistRef
import app.hocket.core.api.Genre
import app.hocket.core.api.OfflineState
import app.hocket.core.api.PlayHistoryEntry
import app.hocket.core.api.Playlist
import app.hocket.core.api.SortOrder
import app.hocket.core.api.Track
import app.hocket.core.api.TrackSummary
import app.hocket.ui.screens.library.LibraryList
import app.hocket.ui.screens.library.ListSort
import app.hocket.ui.screens.library.PinEntry
import app.hocket.ui.screens.library.PinKind
import app.hocket.ui.screens.library.SortKey
import app.hocket.ui.screens.library.decodeSort
import app.hocket.ui.screens.library.defaultSort
import app.hocket.ui.screens.library.pinnedIds
import app.hocket.ui.screens.library.recentAlbumIds
import app.hocket.ui.screens.library.recentArtistIds
import app.hocket.ui.screens.library.recentGenres
import app.hocket.ui.screens.library.recentPlaylists
import app.hocket.ui.screens.library.recentTracks
import app.hocket.ui.screens.library.sortGenres
import app.hocket.ui.screens.library.sortKeys
import app.hocket.ui.screens.library.sortPlaylists
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/** The library lists' sorts, their stored form, the in-app sorting of playlists and genres, and the Pinned / Recent picks. */
class LibrarySortTest {
    private fun playlist(id: String, name: String, songs: Int = 1, duration: Int = 1, created: Double? = null, changed: Double? = null) =
        Playlist(id, "s", name, null, null, false, songs.toUInt(), duration.toUInt(), null, created, changed, false, true, OfflineState.None)

    private fun summary(id: String, album: String? = null, artist: String? = null, artists: List<ArtistRef>? = null) =
        TrackSummary(id, "s", "T$id", null, null, album, artist, 1u, null, 0u, false, OfflineState.None, artists)

    private fun played(vararg tracks: TrackSummary) = tracks.mapIndexed { i, t -> PlayHistoryEntry(t, 1000.0 - i, 1u, true, "d") }

    private fun track(id: String, genre: String?) = Track(
        id = id, serverId = "s", title = id, genre = genre, durationMs = 1u, rating = 0u, loved = false, playCount = 0u, offline = OfflineState.None, explicit = false,
    )

    @Test
    fun eachListOffersItsOrdersAndDefaultsToNameAscending() {
        assertEquals(listOf(SortKey.Name, SortKey.AlbumCount, SortKey.SongCount, SortKey.MostPlayed, SortKey.RecentlyAdded, SortKey.Random), LibraryList.Artists.sortKeys)
        assertEquals(listOf(SortKey.Name, SortKey.Updated, SortKey.Created, SortKey.SongCount, SortKey.Duration), LibraryList.Playlists.sortKeys)
        assertEquals(listOf(SortKey.Name, SortKey.SongCount, SortKey.AlbumCount), LibraryList.Genres.sortKeys)
        assertEquals(ListSort(SortKey.Title, false), LibraryList.Songs.defaultSort)
        LibraryList.entries.forEach { assertFalse(it.defaultSort.descending) }
        // The artist orders reach the core as its new artist-only values.
        assertEquals(SortOrder.AlbumCount, SortKey.AlbumCount.core)
        assertEquals(SortOrder.SongCount, SortKey.SongCount.core)
    }

    @Test
    fun aStoredSortRoundTripsAndAnythingElseIsTheDefault() {
        val sort = ListSort(SortKey.RecentlyAdded, true)
        assertEquals("dateAdded:desc", sort.encode())
        assertEquals(sort, LibraryList.Albums.decodeSort(sort.encode()))
        assertEquals(ListSort(SortKey.SongCount, false), LibraryList.Genres.decodeSort("songCount:asc"))
        assertEquals(LibraryList.Albums.defaultSort, LibraryList.Albums.decodeSort(null))
        assertEquals(LibraryList.Albums.defaultSort, LibraryList.Albums.decodeSort("garbage"))
        // An order the list doesn't offer (Bpm is songs only).
        assertEquals(LibraryList.Albums.defaultSort, LibraryList.Albums.decodeSort("bpm:desc"))
    }

    @Test
    fun choosingAnOrderStartsItInItsUsualDirection() {
        val byName = ListSort(SortKey.Name, true)
        assertEquals(ListSort(SortKey.RecentlyAdded, true), byName.choose(SortKey.RecentlyAdded))
        assertEquals(ListSort(SortKey.Artist, false), byName.choose(SortKey.Artist))
        assertEquals(byName, byName.choose(SortKey.Name))
        // A shuffle has no direction: the core is always asked ascending.
        assertFalse(ListSort(SortKey.Random, true).coreDescending)
        assertTrue(ListSort(SortKey.MostPlayed, true).coreDescending)
    }

    @Test
    fun playlistsSortInTheAppWithMissingDatesLast() {
        val a = playlist("a", "beta", songs = 5, duration = 30, created = 1.0, changed = 9.0)
        val b = playlist("b", "Alpha", songs = 9, duration = 10, created = 3.0, changed = null)
        val c = playlist("c", "gamma", songs = 5, duration = 20, created = 2.0, changed = 4.0)
        val all = listOf(a, b, c)
        assertEquals(listOf(b, a, c), sortPlaylists(all, ListSort(SortKey.Name)))
        assertEquals(listOf(c, a, b), sortPlaylists(all, ListSort(SortKey.Name, true)))
        assertEquals(listOf(a, c, b), sortPlaylists(all, ListSort(SortKey.Updated, true)))
        assertEquals(listOf(c, a, b), sortPlaylists(all, ListSort(SortKey.Updated, false)))
        assertEquals(listOf(b, c, a), sortPlaylists(all, ListSort(SortKey.Created, true)))
        // Equal counts fall back to the name.
        assertEquals(listOf(a, c, b), sortPlaylists(all, ListSort(SortKey.SongCount, false)))
        assertEquals(listOf(a, c, b), sortPlaylists(all, ListSort(SortKey.Duration, true)))
    }

    @Test
    fun genresSortInTheApp() {
        val rock = Genre("Rock", 10u, 2u)
        val ambient = Genre("ambient", 3u, 5u)
        val jazz = Genre("Jazz", 7u, 1u)
        val all = listOf(rock, ambient, jazz)
        assertEquals(listOf(ambient, jazz, rock), sortGenres(all, ListSort(SortKey.Name)))
        assertEquals(listOf(rock, jazz, ambient), sortGenres(all, ListSort(SortKey.SongCount, true)))
        assertEquals(listOf(jazz, rock, ambient), sortGenres(all, ListSort(SortKey.AlbumCount, false)))
    }

    @Test
    fun pinnedIdsKeepPinOrderForOneKind() {
        val pins = listOf(PinEntry("album", "a2"), PinEntry("playlist", "p1"), PinEntry("album", "a1"), PinEntry("station", "x"))
        assertEquals(listOf("a2", "a1"), pinnedIds(pins, PinKind.Album))
        assertEquals(listOf("p1"), pinnedIds(pins, PinKind.Playlist))
        assertEquals(emptyList<String>(), pinnedIds(pins, PinKind.Genre))
    }

    @Test
    fun recentGroupsComeFromTheLatestPlaysEachOnce() {
        val history = played(
            summary("t1", album = "al1", artist = "ar1"),
            summary("t2", album = "al2", artist = null, artists = listOf(ArtistRef(null, "Anon"), ArtistRef("ar2", "Two"))),
            summary("t1", album = "al1", artist = "ar1"),
            summary("t3", album = "al3", artist = "ar1"),
            summary("t4", album = null, artist = "ar3"),
        )
        assertEquals(listOf("al1", "al2", "al3"), recentAlbumIds(history, 6))
        assertEquals(listOf("al2", "al3"), recentAlbumIds(history, 2, exclude = setOf("al1")))
        assertEquals(listOf("ar1", "ar2", "ar3"), recentArtistIds(history, 6))
        assertEquals(listOf("t1", "t2", "t3"), recentTracks(history, 3).map { it.id })
        assertEquals(listOf("t2", "t3", "t4"), recentTracks(history, 3, exclude = setOf("t1")).map { it.id })
    }

    @Test
    fun recentGenresAreThoseOfTheSongsPlayedLast() {
        val genres = listOf(Genre("Rock", 1u, 1u), Genre("Jazz", 1u, 1u), Genre("Dub", 1u, 1u))
        val played = listOf(track("a", "Jazz"), track("b", null), track("c", "Jazz"), track("d", "Gone"), track("e", "Rock"), track("f", "Dub"))
        assertEquals(listOf("Jazz", "Rock", "Dub"), recentGenres(played, genres, 6).map { it.name })
        assertEquals(listOf("Rock"), recentGenres(played, genres, 1, exclude = setOf("Jazz")).map { it.name })
    }

    @Test
    fun recentPlaylistsAreTheLastChangedLeavingOutPins() {
        val a = playlist("a", "A", changed = 5.0)
        val b = playlist("b", "B", changed = 9.0)
        val c = playlist("c", "C", changed = null)
        val d = playlist("d", "D", changed = 7.0)
        assertEquals(listOf(b, d, a), recentPlaylists(listOf(a, b, c, d), 3))
        assertEquals(listOf(d, a), recentPlaylists(listOf(a, b, c, d), 3, exclude = setOf("b")))
    }
}
