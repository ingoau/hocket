package app.hocket.playback

import android.app.Application
import android.os.Bundle
import android.provider.MediaStore
import androidx.test.core.app.ApplicationProvider
import androidx.test.ext.junit.runners.AndroidJUnit4
import app.hocket.core.CoreHandle
import app.hocket.core.api.Command
import app.hocket.core.api.ContextKind
import app.hocket.core.api.MediaSessionAction
import app.hocket.core.fake.FakeCore
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.runBlocking
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.annotation.Config

/**
 * The browse tree, search and play/enqueue resolution over the fake core's library: what Android
 * Auto or any MediaBrowser sees, and the commands a tap there sends.
 */
@RunWith(AndroidJUnit4::class)
@Config(sdk = [34], application = Application::class)
class LibraryBrowserTest {
    private val app: Application get() = ApplicationProvider.getApplicationContext()
    private val fake = FakeCore(timers = false)
    private val sent = mutableListOf<Command>()
    private val core = object : CoreHandle by fake {
        override fun dispatch(command: Command) { sent += command }
    }
    private val server = fake.library.serverId
    private var serverId: String? = server
    private val browser = LibraryBrowser(app, { core }, { serverId }, CoroutineScope(SupervisorJob() + Dispatchers.Unconfined))

    @Test
    fun rootListsTheFourSections() = runBlocking {
        val sections = browser.children(MediaId.ROOT, 0, 100)!!
        assertEquals(listOf("albums", "artists", "playlists", "genres"), sections.map { it.mediaId })
        assertTrue(sections.all { it.mediaMetadata.isBrowsable == true && it.mediaMetadata.isPlayable == false })
        assertEquals("Albums", sections[0].mediaMetadata.title.toString())
    }

    @Test
    fun sectionsPageThroughTheLibrary() = runBlocking {
        val first = browser.children("albums", 0, 5)!!
        val second = browser.children("albums", 1, 5)!!
        assertEquals(5, first.size)
        assertTrue("pages do not overlap", first.map { it.mediaId }.intersect(second.map { it.mediaId }.toSet()).isEmpty())
        val album = fake.library.albums.first()
        val item = first.first { it.mediaId == MediaId.Album(server, album.id).format() }
        assertEquals(album.name, item.mediaMetadata.title.toString())
        assertTrue(item.mediaMetadata.isBrowsable == true && item.mediaMetadata.isPlayable == true)
        assertEquals("content", item.mediaMetadata.artworkUri?.scheme)
        assertEquals(fake.library.genres.size, browser.children("genres", 0, 1000)!!.size)
        assertEquals(fake.library.playlists.size, browser.children("playlists", 0, 1000)!!.size)
        assertTrue(browser.children("artists", 0, 10)!!.isNotEmpty())
    }

    @Test
    fun noServerMeansEmptySectionsAndUnknownIdsAreErrors() = runBlocking {
        serverId = null
        assertEquals(emptyList<Any>(), browser.children("albums", 0, 10))
        assertNull(browser.children("nonsense", 0, 10))
        assertNull(browser.item("album/x"))
    }

    @Test
    fun anAlbumsChildrenAreItsTracksInOrder() = runBlocking {
        val album = fake.library.albums.first()
        val tracks = browser.children(MediaId.Album(server, album.id).format(), 0, 100)!!
        assertEquals(fake.library.albumTracks(album.id).map { it.title }, tracks.map { it.mediaMetadata.title.toString() })
        assertEquals(MediaId.AlbumTrack(server, album.id, 1).format(), tracks[1].mediaId)
        assertTrue(tracks.all { it.mediaMetadata.isPlayable == true && it.mediaMetadata.isBrowsable == false })
        assertEquals(tracks[1].mediaMetadata.title, browser.item(tracks[1].mediaId)?.mediaMetadata?.title)
    }

    @Test
    fun aTrackInsideAnAlbumPlaysTheAlbumFromThere() = runBlocking {
        val album = fake.library.albums.first()
        assertTrue(browser.playNow(listOf(MediaId.AlbumTrack(server, album.id, 2).format()), 0))
        val play = sent.single() as Command.PlayContext
        assertEquals(2u, play.data.args.startIndex)
        assertEquals(album.name, play.data.args.context.label)
        assertEquals(album.id, (play.data.args.context.kind as ContextKind.Album).data.id)
    }

    @Test
    fun playlistsGenresAndQueueEntriesPlay() = runBlocking {
        val playlist = fake.library.playlists.first()
        assertTrue(browser.playNow(listOf(MediaId.Playlist(server, playlist.id).format()), 0))
        assertTrue((sent.last() as Command.PlayContext).data.args.context.kind is ContextKind.Playlist)
        assertTrue(browser.playNow(listOf(MediaId.Genre(server, "Rock").format()), 0))
        assertEquals("Rock", ((sent.last() as Command.PlayContext).data.args.context.kind as ContextKind.Genre).data.name)
        assertTrue(browser.playNow(listOf(MediaId.QueueItem("k1").format()), 0))
        assertEquals("k1", (sent.last() as Command.JumpToQueueItem).data.key)
        assertFalse("a section is not playable", browser.playNow(listOf("albums"), 0))
    }

    @Test
    fun aListOfLoneTracksPlaysAsOneList() = runBlocking {
        val ids = fake.library.tracks.take(3).map { MediaId.Track(server, it.id).format() }
        assertTrue(browser.playNow(ids, 1))
        val play = sent.single() as Command.PlayTracks
        assertEquals(fake.library.tracks.take(3).map { it.id }, play.data.track_ids)
        assertEquals(1u, play.data.start_index)
    }

    @Test
    fun enqueueingAnAlbumAddsItsTracks() = runBlocking {
        val album = fake.library.albums.first()
        assertTrue(browser.enqueueNow(listOf(MediaId.Album(server, album.id).format()), next = true))
        assertEquals(fake.library.albumTracks(album.id).map { it.id }, (sent.single() as Command.PlayNext).data.track_ids)
        val t = fake.library.tracks.first()
        assertTrue(browser.enqueueNow(listOf(MediaId.Track(server, t.id).format()), next = false))
        assertEquals(listOf(t.id), (sent.last() as Command.PlayLater).data.track_ids)
        assertFalse("artists are too broad to enqueue", browser.enqueueNow(listOf(MediaId.Artist(server, "ar0").format()), next = false))
    }

    @Test
    fun searchReturnsPlayableLocalResults() = runBlocking {
        val track = fake.library.tracks.first()
        val results = browser.search(track.title)
        assertTrue(results.any { it.mediaId == MediaId.Track(server, track.id).format() })
        assertTrue(browser.search("  ").isEmpty())
    }

    @Test
    fun voiceRequestsResolveToOneItem() = runBlocking {
        assertEquals(LibraryBrowser.RESUME, browser.resolveSearch("", null)?.mediaId)
        assertTrue(browser.playNow(listOf(LibraryBrowser.RESUME), 0))
        assertEquals(MediaSessionAction.Play, (sent.last() as Command.MediaSessionCommand).data.action)

        val album = fake.library.albums.first()
        val focusAlbum = Bundle().apply { putString(MediaStore.EXTRA_MEDIA_FOCUS, MediaStore.Audio.Albums.ENTRY_CONTENT_TYPE) }
        assertEquals(MediaId.Album(server, album.id).format(), browser.resolveSearch(album.name, focusAlbum)?.mediaId)

        val genre = fake.library.genres.first()
        val focusGenre = Bundle().apply { putString(MediaStore.EXTRA_MEDIA_FOCUS, MediaStore.Audio.Genres.ENTRY_CONTENT_TYPE); putString(MediaStore.EXTRA_MEDIA_GENRE, genre.name.uppercase()) }
        assertEquals(MediaId.Genre(server, genre.name).format(), browser.resolveSearch(genre.name, focusGenre)?.mediaId)

        assertNotNull(browser.resolveSearch(fake.library.tracks.first().title, null))
        assertNull(browser.resolveSearch("zzzz-no-such-thing", null))
    }

    @Test
    fun artworkIsOnlyForThisAppAndConnectedControllers() {
        ArtworkProvider.resetForTests()
        assertTrue(ArtworkProvider.isAllowed(app.packageName, app.packageName))
        assertFalse(ArtworkProvider.isAllowed("com.example.snoop", app.packageName))
        assertFalse(ArtworkProvider.isAllowed(null, app.packageName))
        ArtworkProvider.allow("com.google.android.projection.gearhead")
        assertTrue(ArtworkProvider.isAllowed("com.google.android.projection.gearhead", app.packageName))
        assertEquals("content://${app.packageName}.artwork/320/al%2F1", ArtworkProvider.uri(app, "al/1").toString())
    }
}
