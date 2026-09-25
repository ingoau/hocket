package app.hocket.playback

import java.net.URLDecoder
import java.net.URLEncoder

/**
 * The media ids [LibraryBrowser] hands to other apps (Android Auto, Wear, Assistant, any
 * `MediaBrowser`) and gets back in `getChildren`, `getItem` and `setMediaItems`.
 *
 * Every id is self-contained: it carries the server id and whatever else is needed to play it
 * without remembering what was browsed, since a controller may play an id from an earlier connection
 * (Auto keeps its own history). Segments are URL-encoded and joined with `/`, so a genre named
 * "Drum/Bass" or an id with a colon round-trips.
 *
 * - `root`, `albums`, `artists`, `playlists`, `genres`
 * - `album/<server>/<id>` (browsable, playable: the album), `album/<server>/<id>/<index>` (a track,
 *   playing the album from there; the index is into `Query.AlbumTracks`, as the UI's)
 * - `artist/<server>/<id>` (its albums; playable: the artist)
 * - `playlist/<server>/<id>`, `playlist/<server>/<id>/<index>` (the index is into `PlaylistTracks`)
 * - `genre/<server>/<name>` (its albums; playable: the genre)
 * - `track/<server>/<id>` (a lone track: search results)
 * - `queue/<key>` (a queue entry in the session's timeline)
 */
sealed class MediaId {
    data object Root : MediaId()
    data class Section(val section: LibrarySection) : MediaId()
    data class Album(val serverId: String, val id: String) : MediaId()
    data class AlbumTrack(val serverId: String, val albumId: String, val index: Int) : MediaId()
    data class Artist(val serverId: String, val id: String) : MediaId()
    data class Playlist(val serverId: String, val id: String) : MediaId()
    data class PlaylistTrack(val serverId: String, val playlistId: String, val index: Int) : MediaId()
    data class Genre(val serverId: String, val name: String) : MediaId()
    data class Track(val serverId: String, val id: String) : MediaId()
    data class QueueItem(val key: String) : MediaId()

    fun format(): String = when (this) {
        Root -> ROOT
        is Section -> section.id
        is Album -> join("album", serverId, id)
        is AlbumTrack -> join("album", serverId, albumId, index.toString())
        is Artist -> join("artist", serverId, id)
        is Playlist -> join("playlist", serverId, id)
        is PlaylistTrack -> join("playlist", serverId, playlistId, index.toString())
        is Genre -> join("genre", serverId, name)
        is Track -> join("track", serverId, id)
        is QueueItem -> join("queue", key)
    }

    override fun toString(): String = format()

    companion object {
        const val ROOT = "root"

        /** Null for anything this app did not hand out. */
        fun parse(id: String): MediaId? {
            if (id == ROOT) return Root
            LibrarySection.entries.firstOrNull { it.id == id }?.let { return Section(it) }
            val parts = id.split('/').map { runCatching { URLDecoder.decode(it, "UTF-8") }.getOrNull() ?: return null }
            if (parts.any { it.isEmpty() }) return null
            return when (parts.first()) {
                "album" -> when (parts.size) {
                    3 -> Album(parts[1], parts[2])
                    4 -> parts[3].toIntOrNull()?.takeIf { it >= 0 }?.let { AlbumTrack(parts[1], parts[2], it) }
                    else -> null
                }
                "playlist" -> when (parts.size) {
                    3 -> Playlist(parts[1], parts[2])
                    4 -> parts[3].toIntOrNull()?.takeIf { it >= 0 }?.let { PlaylistTrack(parts[1], parts[2], it) }
                    else -> null
                }
                "artist" -> if (parts.size == 3) Artist(parts[1], parts[2]) else null
                "genre" -> if (parts.size == 3) Genre(parts[1], parts[2]) else null
                "track" -> if (parts.size == 3) Track(parts[1], parts[2]) else null
                "queue" -> if (parts.size == 2) QueueItem(parts[1]) else null
                else -> null
            }
        }

        private fun join(vararg parts: String) = parts.joinToString("/") { URLEncoder.encode(it, "UTF-8") }
    }
}

/** The top-level categories under the root (Auto shows up to four of them as tabs). */
enum class LibrarySection(val id: String) {
    Albums("albums"),
    Artists("artists"),
    Playlists("playlists"),
    Genres("genres"),
}
