package app.hocket.playback

import android.content.Context
import android.net.Uri
import android.os.Bundle
import android.provider.MediaStore
import android.util.Log
import androidx.media3.common.MediaItem
import androidx.media3.common.MediaMetadata
import androidx.media3.session.MediaConstants
import app.hocket.core.Commands
import app.hocket.core.CoreHandle
import app.hocket.core.Queries
import app.hocket.core.api.Album
import app.hocket.core.api.Artist
import app.hocket.core.api.Command
import app.hocket.core.api.Genre
import app.hocket.core.api.MediaSessionAction
import app.hocket.core.api.Page
import app.hocket.core.api.Playlist
import app.hocket.core.api.QueryResult
import app.hocket.core.api.SearchResults
import app.hocket.core.api.Track
import app.hocket.core.api.TrackSummary
import app.hocket.core.toSummary
import java.util.UUID
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.launch

/**
 * The library as other apps see it: the browse tree of the [MediaSessionBridge]'s library session
 * (Android Auto, Wear, Assistant, any `MediaBrowser`), its search, and what the session's player does
 * with a media id a controller asks it to play or enqueue.
 *
 * Everything is a core query on the first server, the same one the UI shows; nothing is cached here
 * (the core's mirror is local, so browsing works offline). Ids are [MediaId]s. Artwork is a
 * [ArtworkProvider] content URI, since a controller cannot read the core's cache files.
 *
 * Play requests become the same commands the UI sends: an album, artist, playlist or genre plays as
 * that context (from the tapped track, for a track inside one), a lone track via `PlayTracks`, a
 * queue entry is a jump. Enqueued items become `PlayNext` / `PlayLater`.
 */
class LibraryBrowser(
    private val context: Context,
    private val core: () -> CoreHandle,
    private val serverId: () -> String?,
    private val scope: CoroutineScope,
) : CoreSessionPlayer.MediaRequests {
    companion object {
        private const val TAG = "LibraryBrowser"
        /** Id of the item a voice "play something" (empty query) resolves to: resume the session. */
        const val RESUME = "resume"
        const val SEARCH_LIMIT = 25
        /** Enough to enqueue a whole playlist in one query. */
        private const val PLAYLIST_ALL = 10_000

        private const val STYLE_LIST = MediaConstants.EXTRAS_VALUE_CONTENT_STYLE_LIST_ITEM
        private const val STYLE_GRID = MediaConstants.EXTRAS_VALUE_CONTENT_STYLE_GRID_ITEM

        /** Root extras: search works, and children may carry content style hints. */
        fun rootExtras(): Bundle = Bundle().apply {
            putBoolean("android.media.browse.CONTENT_STYLE_SUPPORTED", true)
            putBoolean("android.media.browse.SEARCH_SUPPORTED", true)
            putInt(MediaConstants.EXTRAS_KEY_CONTENT_STYLE_BROWSABLE, STYLE_GRID)
            putInt(MediaConstants.EXTRAS_KEY_CONTENT_STYLE_PLAYABLE, STYLE_LIST)
        }

        private fun style(browsable: Int, playable: Int = STYLE_LIST) = Bundle().apply {
            putInt(MediaConstants.EXTRAS_KEY_CONTENT_STYLE_BROWSABLE, browsable)
            putInt(MediaConstants.EXTRAS_KEY_CONTENT_STYLE_PLAYABLE, playable)
        }
    }

    // -- browse tree -------------------------------------------------------------------------------

    fun root(): MediaItem = folder(MediaId.Root, context.getString(R.string.browse_root), MediaMetadata.MEDIA_TYPE_FOLDER_MIXED, rootExtras())

    /** The children of [parentId], one page; null when [parentId] is not a browsable id of ours. */
    suspend fun children(parentId: String, page: Int, pageSize: Int): List<MediaItem>? {
        val id = MediaId.parse(parentId) ?: return null
        val window = Page((page.coerceAtLeast(0).toLong() * pageSize).coerceAtMost(UInt.MAX_VALUE.toLong()).toUInt(), pageSize.coerceAtLeast(1).toUInt())
        return when (id) {
            MediaId.Root -> LibrarySection.entries.map(::section).window(window)
            is MediaId.Section -> {
                val server = serverId() ?: return emptyList()
                when (id.section) {
                    LibrarySection.Albums -> (query(Queries.albums(server, window)) as? QueryResult.Albums)?.data?.items.orEmpty().map(::album)
                    LibrarySection.Artists -> (query(Queries.artists(server, window)) as? QueryResult.Artists)?.data?.items.orEmpty().map(::artist)
                    LibrarySection.Playlists -> (query(Queries.playlists(server)) as? QueryResult.Playlists)?.data.orEmpty().map(::playlist).window(window)
                    LibrarySection.Genres -> (query(Queries.genres(server)) as? QueryResult.Genres)?.data.orEmpty().map { genre(server, it) }.window(window)
                }
            }
            is MediaId.Album -> albumTracks(id.id).mapIndexed { i, t -> track(t.toSummary(), MediaId.AlbumTrack(id.serverId, id.id, i)) }.window(window)
            is MediaId.Artist -> (query(Queries.albums(id.serverId, window, artistId = id.id)) as? QueryResult.Albums)?.data?.items.orEmpty().map(::album)
            is MediaId.Genre -> (query(Queries.albums(id.serverId, window, genre = id.name)) as? QueryResult.Albums)?.data?.items.orEmpty().map(::album)
            is MediaId.Playlist -> {
                val tracks = (query(Queries.playlistTracks(id.id, window)) as? QueryResult.Tracks)?.data ?: return emptyList()
                tracks.items.mapIndexed { i, t -> track(t.toSummary(), MediaId.PlaylistTrack(id.serverId, id.id, tracks.offset.toInt() + i)) }
            }
            else -> null
        }
    }

    /** One item by id (a controller restoring its place, or subscribing); null when unknown. */
    suspend fun item(mediaId: String): MediaItem? = when (val id = MediaId.parse(mediaId)) {
        null -> null
        MediaId.Root -> root()
        is MediaId.Section -> section(id.section)
        is MediaId.Album -> albumDetail(id.id)?.let(::album)
        is MediaId.AlbumTrack -> albumTracks(id.albumId).getOrNull(id.index)?.let { track(it.toSummary(), id) }
        is MediaId.Artist -> (query(Queries.artist(id.id)) as? QueryResult.ArtistDetail)?.data?.let(::artist)
        is MediaId.Playlist -> playlistDetail(id.id)?.let(::playlist)
        is MediaId.PlaylistTrack -> playlistTrack(id.playlistId, id.index)?.let { track(it, id) }
        is MediaId.Genre -> folder(id, id.name, MediaMetadata.MEDIA_TYPE_GENRE, style(STYLE_GRID), playable = true)
        is MediaId.Track -> trackDetail(id.id)?.let { track(it.toSummary(), id) }
        is MediaId.QueueItem -> null
    }

    /** Local results only: a controller's search must not wait on (or reach) the server. */
    suspend fun search(text: String, limit: Int = SEARCH_LIMIT): List<MediaItem> {
        val results = searchResults(text, limit) ?: return emptyList()
        return results.tracks.map { track(it, MediaId.Track(it.serverId, it.id)) } +
            results.albums.map(::album) + results.artists.map(::artist) + results.playlists.map(::playlist)
    }

    private suspend fun searchResults(text: String, limit: Int): SearchResults? {
        val server = serverId() ?: return null
        if (text.isBlank()) return null
        return (query(Queries.search(server, text.trim(), limit, includeServer = false, requestId = "browse-" + UUID.randomUUID())) as? QueryResult.Search)?.data
    }

    /**
     * A voice or text "play …" request (Assistant, Auto's search button) as one playable item:
     * blank asks to resume, the focus extra (`MediaStore.EXTRA_MEDIA_FOCUS`) prefers an artist,
     * album, playlist or genre match, otherwise the best track, then album, artist, playlist.
     */
    suspend fun resolveSearch(text: String, extras: Bundle?): MediaItem? {
        if (text.isBlank()) return MediaItem.Builder().setMediaId(RESUME).build()
        val server = serverId() ?: return null
        val focus = extras?.getString(MediaStore.EXTRA_MEDIA_FOCUS)
        if (focus == MediaStore.Audio.Genres.ENTRY_CONTENT_TYPE) {
            val genre = extras.getString(MediaStore.EXTRA_MEDIA_GENRE)?.takeIf { it.isNotBlank() } ?: text
            val known = (query(Queries.genres(server)) as? QueryResult.Genres)?.data.orEmpty()
            known.firstOrNull { it.name.equals(genre.trim(), ignoreCase = true) }?.let { return genre(server, it) }
        }
        val r = searchResults(text, SEARCH_LIMIT) ?: return null
        val preferred = when (focus) {
            MediaStore.Audio.Artists.ENTRY_CONTENT_TYPE -> r.artists.firstOrNull()?.let(::artist)
            MediaStore.Audio.Albums.ENTRY_CONTENT_TYPE -> r.albums.firstOrNull()?.let(::album)
            MediaStore.Audio.Playlists.ENTRY_CONTENT_TYPE -> r.playlists.firstOrNull()?.let(::playlist)
            else -> null
        }
        return preferred
            ?: r.tracks.firstOrNull()?.let { track(it, MediaId.Track(it.serverId, it.id)) }
            ?: r.albums.firstOrNull()?.let(::album)
            ?: r.artists.firstOrNull()?.let(::artist)
            ?: r.playlists.firstOrNull()?.let(::playlist)
    }

    // -- requests from the session's player --------------------------------------------------------

    override fun play(mediaIds: List<String>, startIndex: Int) {
        scope.launch { runCatching { playNow(mediaIds, startIndex) }.onFailure { Log.w(TAG, "play failed: ${it.message}") } }
    }

    override fun enqueue(mediaIds: List<String>, next: Boolean) {
        scope.launch { runCatching { enqueueNow(mediaIds, next) }.onFailure { Log.w(TAG, "enqueue failed: ${it.message}") } }
    }

    /** Dispatches the command for [mediaIds] (see the class docs); false when nothing is playable. */
    suspend fun playNow(mediaIds: List<String>, startIndex: Int): Boolean {
        if (mediaIds.isEmpty()) return false
        val start = startIndex.coerceIn(0, mediaIds.lastIndex)
        if (mediaIds[start] == RESUME) { dispatch(Commands.mediaSessionCommand(MediaSessionAction.Play)); return true }
        val ids = mediaIds.map { MediaId.parse(it) }
        // A list of lone tracks (a controller setting search results) plays as one ad-hoc list.
        val tracks = ids.filterIsInstance<MediaId.Track>()
        if (tracks.size == ids.size && tracks.size > 1 && tracks.all { it.serverId == tracks[0].serverId }) {
            dispatch(Commands.playTracks(tracks[0].serverId, tracks.map { it.id }, start, label(tracks[start].id)))
            return true
        }
        val command = when (val id = ids[start]) {
            is MediaId.Album -> albumDetail(id.id)?.let { Commands.playContext(Commands.albumContext(id.serverId, id.id, it.name)) }
            is MediaId.AlbumTrack -> albumDetail(id.albumId)?.let { Commands.playContext(Commands.albumContext(id.serverId, id.albumId, it.name), startIndex = id.index) }
            is MediaId.Artist -> (query(Queries.artist(id.id)) as? QueryResult.ArtistDetail)?.data?.let { Commands.playContext(Commands.artistContext(id.serverId, id.id, it.name)) }
            is MediaId.Playlist -> playlistDetail(id.id)?.let { Commands.playContext(Commands.playlistContext(id.serverId, id.id, it.name)) }
            is MediaId.PlaylistTrack -> playlistDetail(id.playlistId)?.let { Commands.playContext(Commands.playlistContext(id.serverId, id.playlistId, it.name), startIndex = id.index) }
            is MediaId.Genre -> Commands.playContext(Commands.genreContext(id.serverId, id.name))
            is MediaId.Track -> Commands.playTracks(id.serverId, listOf(id.id), 0, label(id.id))
            is MediaId.QueueItem -> Commands.jumpToQueueItem(id.key)
            MediaId.Root, is MediaId.Section, null -> null
        } ?: return false
        dispatch(command)
        return true
    }

    /** `PlayNext` / `PlayLater` with the tracks behind [mediaIds]; false when none resolve. */
    suspend fun enqueueNow(mediaIds: List<String>, next: Boolean): Boolean {
        val byServer = LinkedHashMap<String, MutableList<String>>()
        for (raw in mediaIds) {
            val (server, ids) = when (val id = MediaId.parse(raw)) {
                is MediaId.Track -> id.serverId to listOf(id.id)
                is MediaId.AlbumTrack -> id.serverId to listOfNotNull(albumTracks(id.albumId).getOrNull(id.index)?.id)
                is MediaId.Album -> id.serverId to albumTracks(id.id).map { it.id }
                is MediaId.PlaylistTrack -> id.serverId to listOfNotNull(playlistTrack(id.playlistId, id.index)?.id)
                is MediaId.Playlist -> id.serverId to ((query(Queries.playlistTracks(id.id, Page(0u, PLAYLIST_ALL.toUInt()))) as? QueryResult.Tracks)?.data?.items.orEmpty().map { it.id })
                else -> continue // artists, genres and sections are too broad to enqueue
            }
            byServer.getOrPut(server) { mutableListOf() } += ids
        }
        var any = false
        for ((server, ids) in byServer) {
            if (ids.isEmpty()) continue
            dispatch(if (next) Commands.playNext(server, ids) else Commands.playLater(server, ids))
            any = true
        }
        return any
    }

    private suspend fun label(trackId: String): String =
        trackDetail(trackId)?.title ?: context.getString(R.string.browse_root)

    // -- items -------------------------------------------------------------------------------------

    private fun section(section: LibrarySection): MediaItem = when (section) {
        LibrarySection.Albums -> folder(MediaId.Section(section), context.getString(R.string.browse_albums), MediaMetadata.MEDIA_TYPE_FOLDER_ALBUMS, style(STYLE_GRID))
        LibrarySection.Artists -> folder(MediaId.Section(section), context.getString(R.string.browse_artists), MediaMetadata.MEDIA_TYPE_FOLDER_ARTISTS, style(STYLE_LIST))
        LibrarySection.Playlists -> folder(MediaId.Section(section), context.getString(R.string.browse_playlists), MediaMetadata.MEDIA_TYPE_FOLDER_PLAYLISTS, style(STYLE_LIST))
        LibrarySection.Genres -> folder(MediaId.Section(section), context.getString(R.string.browse_genres), MediaMetadata.MEDIA_TYPE_FOLDER_GENRES, style(STYLE_LIST))
    }

    private fun folder(id: MediaId, title: String, type: Int, extras: Bundle, playable: Boolean = false, artwork: Uri? = null, subtitle: String? = null): MediaItem =
        MediaItem.Builder().setMediaId(id.format()).setMediaMetadata(
            MediaMetadata.Builder()
                .setTitle(title)
                .setSubtitle(subtitle)
                .setIsBrowsable(true)
                .setIsPlayable(playable)
                .setMediaType(type)
                .setArtworkUri(artwork)
                .setExtras(extras)
                .build(),
        ).build()

    private fun album(a: Album): MediaItem =
        folder(MediaId.Album(a.serverId, a.id), a.name, MediaMetadata.MEDIA_TYPE_ALBUM, style(STYLE_LIST), playable = true, artwork = art(a.coverArt), subtitle = a.artist)
            .let { item -> item.buildUpon().setMediaMetadata(item.mediaMetadata.buildUpon().setArtist(a.artist).setAlbumTitle(a.name).setAlbumArtist(a.artist).setReleaseYear(a.year?.toInt()).build()).build() }

    private fun artist(a: Artist): MediaItem =
        folder(MediaId.Artist(a.serverId, a.id), a.name, MediaMetadata.MEDIA_TYPE_ARTIST, style(STYLE_GRID), playable = true, artwork = art(a.coverArt))
            .let { item -> item.buildUpon().setMediaMetadata(item.mediaMetadata.buildUpon().setArtist(a.name).build()).build() }

    private fun playlist(p: Playlist): MediaItem =
        folder(MediaId.Playlist(p.serverId, p.id), p.name, MediaMetadata.MEDIA_TYPE_PLAYLIST, style(STYLE_LIST), playable = true, artwork = art(p.coverArt), subtitle = p.comment?.takeIf { it.isNotBlank() })

    private fun genre(serverId: String, g: Genre): MediaItem =
        folder(MediaId.Genre(serverId, g.name), g.name, MediaMetadata.MEDIA_TYPE_GENRE, style(STYLE_GRID), playable = true)

    private fun track(t: TrackSummary, id: MediaId): MediaItem = trackItem(context, t, id.format())

    private fun art(coverArt: String?): Uri? = coverArt?.let { ArtworkProvider.uri(context, it) }

    // -- queries -----------------------------------------------------------------------------------

    private suspend fun query(q: app.hocket.core.api.Query): QueryResult? =
        runCatching { core().query(q) }.onFailure { Log.w(TAG, "query failed: ${it.message}") }.getOrNull()

    private fun dispatch(command: Command) = core().dispatch(command)

    private suspend fun albumDetail(id: String): Album? = (query(Queries.album(id)) as? QueryResult.AlbumDetail)?.data
    private suspend fun albumTracks(id: String): List<Track> = (query(Queries.albumTracks(id)) as? QueryResult.TrackList)?.data.orEmpty()
    private suspend fun playlistDetail(id: String): Playlist? = (query(Queries.playlist(id)) as? QueryResult.PlaylistDetail)?.data
    private suspend fun trackDetail(id: String): Track? = (query(Queries.track(id)) as? QueryResult.TrackDetail)?.data
    private suspend fun playlistTrack(id: String, index: Int): TrackSummary? =
        (query(Queries.playlistTracks(id, Page(index.toUInt(), 1u))) as? QueryResult.Tracks)?.data?.items?.firstOrNull()?.toSummary()

    private fun <T> List<T>.window(page: Page): List<T> {
        val from = page.offset.toLong().coerceAtMost(size.toLong()).toInt()
        val to = (from.toLong() + page.limit.toLong()).coerceAtMost(size.toLong()).toInt()
        return subList(from, to)
    }
}

/** A playable track item, shared by the browse tree and the session's queue timeline. */
internal fun trackItem(context: Context, t: TrackSummary, mediaId: String): MediaItem =
    MediaItem.Builder().setMediaId(mediaId).setMediaMetadata(trackMetadata(t, t.coverArt?.let { ArtworkProvider.uri(context, it) })).build()

internal fun trackMetadata(t: TrackSummary, artwork: Uri?): MediaMetadata =
    MediaMetadata.Builder()
        .setTitle(t.title)
        .setArtist(t.artist)
        .setAlbumTitle(t.album)
        .setDurationMs(t.durationMs.toLong())
        .setArtworkUri(artwork)
        .setIsBrowsable(false)
        .setIsPlayable(true)
        .setMediaType(MediaMetadata.MEDIA_TYPE_MUSIC)
        .setUserRating(androidx.media3.common.HeartRating(t.loved))
        .build()
