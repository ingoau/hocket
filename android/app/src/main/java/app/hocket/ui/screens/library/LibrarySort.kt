package app.hocket.ui.screens.library

import androidx.annotation.StringRes
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.platform.LocalContext
import app.hocket.ListSortPrefs
import app.hocket.R
import app.hocket.core.api.Genre
import app.hocket.core.api.PlayHistoryEntry
import app.hocket.core.api.Playlist
import app.hocket.core.api.SortOrder
import app.hocket.core.api.Track
import app.hocket.core.api.TrackSummary
import app.hocket.ui.icons.HocketIcons

/** How a sort's two directions read: "A–Z" / "Z–A" for names, "Oldest" / "Newest" for dates, … */
enum class SortWords(@StringRes val ascending: Int, @StringRes val descending: Int) {
    Alphabetical(R.string.sort_dir_a_z, R.string.sort_dir_z_a),
    Date(R.string.sort_dir_oldest, R.string.sort_dir_newest),
    Count(R.string.sort_dir_fewest, R.string.sort_dir_most),
    Length(R.string.sort_dir_shortest, R.string.sort_dir_longest),
    Level(R.string.sort_dir_lowest, R.string.sort_dir_highest),
    Tempo(R.string.sort_dir_slowest, R.string.sort_dir_fastest),
    Plain(R.string.sort_ascending, R.string.sort_descending),
}

/**
 * One way a library list can be sorted. [id] is what the list's preference stores; [core] is the
 * core's order for the paged lists (albums, artists, songs), while playlists and genres sort in the
 * app ([sortPlaylists], [sortGenres]). [descendingFirst]: the direction the sort starts in when
 * chosen (names A–Z; dates newest, counts most first).
 */
enum class SortKey(val id: String, @StringRes val label: Int, val words: SortWords, val descendingFirst: Boolean, val core: SortOrder) {
    Name("name", R.string.sort_name, SortWords.Alphabetical, false, SortOrder.Default),
    Title("title", R.string.sort_title, SortWords.Alphabetical, false, SortOrder.Title),
    Artist("artist", R.string.sort_artist, SortWords.Alphabetical, false, SortOrder.Artist),
    Album("album", R.string.sort_album, SortWords.Alphabetical, false, SortOrder.Album),
    AlbumCount("albumCount", R.string.sort_album_count, SortWords.Count, true, SortOrder.AlbumCount),
    SongCount("songCount", R.string.sort_song_count, SortWords.Count, true, SortOrder.SongCount),
    Year("year", R.string.sort_year, SortWords.Date, true, SortOrder.Year),
    RecentlyAdded("dateAdded", R.string.sort_recently_added, SortWords.Date, true, SortOrder.DateAdded),
    Updated("changed", R.string.sort_recently_updated, SortWords.Date, true, SortOrder.Default),
    Created("created", R.string.sort_date_added, SortWords.Date, true, SortOrder.DateAdded),
    Rating("rating", R.string.sort_rating, SortWords.Level, true, SortOrder.Rating),
    MostPlayed("playCount", R.string.sort_most_played, SortWords.Count, true, SortOrder.PlayCount),
    Duration("duration", R.string.sort_duration, SortWords.Length, true, SortOrder.Duration),
    Bpm("bpm", R.string.sort_bpm, SortWords.Tempo, false, SortOrder.Bpm),
    Energy("energy", R.string.sort_energy, SortWords.Level, false, SortOrder.Energy),
    Random("random", R.string.sort_random, SortWords.Plain, false, SortOrder.Random);

    /** A shuffle has no direction: the sort sheet greys its direction out. */
    val hasDirection: Boolean get() = this != Random

    val icon: ImageVector get() = when (this) {
        Name, Title -> HocketIcons.Filled.SortByAlpha
        Artist -> HocketIcons.Filled.Person
        Album, AlbumCount -> HocketIcons.Filled.Album
        SongCount -> HocketIcons.Filled.MusicNote
        Year -> HocketIcons.Filled.CalendarToday
        RecentlyAdded, Created -> HocketIcons.Filled.Schedule
        Updated -> HocketIcons.Filled.Update
        Rating -> HocketIcons.Filled.Star
        MostPlayed -> HocketIcons.Filled.TrendingUp
        Duration -> HocketIcons.Filled.Timer
        Bpm -> HocketIcons.Filled.Speed
        Energy -> HocketIcons.Filled.Bolt
        Random -> HocketIcons.Filled.Shuffle
    }
}

/** A list's sort: its order and direction. */
data class ListSort(val key: SortKey, val descending: Boolean = false) {
    /** The core's order and direction (a shuffle has none: always asked for ascending, so it stays put). */
    val order: SortOrder get() = key.core
    val coreDescending: Boolean get() = descending && key.hasDirection

    fun encode(): String = "${key.id}:${if (descending) "desc" else "asc"}"

    /** [next] chosen: in its usual direction, or this sort unchanged when it already is [next]. */
    fun choose(next: SortKey): ListSort = if (next == key) this else ListSort(next, next.descendingFirst)

    /** The same sort, minus a direction its order doesn't have. */
    fun normalized(): ListSort = if (key.hasDirection || !descending) this else copy(descending = false)
}

/**
 * The orders each list offers, the first its default (A–Z): "Title" for albums and songs, "Name"
 * for artists, playlists and genres. Songs and albums keep the core's sorts.
 */
val LibraryList.sortKeys: List<SortKey>
    get() = when (this) {
        LibraryList.Albums -> listOf(SortKey.Title, SortKey.Artist, SortKey.Year, SortKey.RecentlyAdded, SortKey.Rating, SortKey.MostPlayed, SortKey.Random)
        LibraryList.Artists -> listOf(SortKey.Name, SortKey.AlbumCount, SortKey.SongCount, SortKey.MostPlayed, SortKey.RecentlyAdded, SortKey.Random)
        LibraryList.Playlists -> listOf(SortKey.Name, SortKey.Updated, SortKey.Created, SortKey.SongCount, SortKey.Duration)
        LibraryList.Songs -> listOf(SortKey.Title, SortKey.Artist, SortKey.Album, SortKey.Year, SortKey.RecentlyAdded, SortKey.Rating, SortKey.MostPlayed, SortKey.Duration, SortKey.Bpm, SortKey.Energy, SortKey.Random)
        LibraryList.Genres -> listOf(SortKey.Name, SortKey.SongCount, SortKey.AlbumCount)
    }

/** Title or name, A–Z. */
val LibraryList.defaultSort: ListSort get() = ListSort(sortKeys.first(), false)

/** The preference key of this list's sort. */
val LibraryList.sortPrefKey: String get() = "sort.${name.lowercase()}"

/** A stored sort ([ListSort.encode]) read back; anything this list doesn't offer is its default. */
fun LibraryList.decodeSort(raw: String?): ListSort {
    val parts = raw?.split(':') ?: return defaultSort
    val key = sortKeys.firstOrNull { it.id == parts.getOrNull(0) } ?: return defaultSort
    return ListSort(key, parts.getOrNull(1) == "desc").normalized()
}

/** The list's sort, restored from this device's preferences and saved on each change. */
@Composable
fun rememberListSort(list: LibraryList): Pair<ListSort, (ListSort) -> Unit> {
    val context = LocalContext.current
    val prefs = remember(context) { ListSortPrefs(context) }
    var sort by remember(list) { mutableStateOf(list.decodeSort(prefs.get(list.sortPrefKey))) }
    return sort to { next -> next.normalized().let { sort = it; prefs.set(list.sortPrefKey, it.encode()) } }
}

/**
 * [list] ordered by [key] (nullable: dates may be missing, and those always go last), in
 * [descending] order or not, ties and a null key falling back to the name A–Z.
 */
private fun <T> sortedByKey(list: List<T>, name: (T) -> String, descending: Boolean, key: ((T) -> Comparable<*>?)?): List<T> {
    val byName = compareBy(String.CASE_INSENSITIVE_ORDER, name)
    if (key == null) return list.sortedWith(if (descending) byName.reversed() else byName)
    val byKey = Comparator<T> { a, b ->
        @Suppress("UNCHECKED_CAST")
        val x = key(a) as Comparable<Any>?
        @Suppress("UNCHECKED_CAST")
        val y = key(b) as Comparable<Any>?
        when {
            x == null && y == null -> 0
            x == null -> 1
            y == null -> -1
            descending -> y.compareTo(x)
            else -> x.compareTo(y)
        }
    }
    return list.sortedWith(byKey.then(byName))
}

/** Playlists (a whole list, sorted here): by name, last change, creation, song count or length. */
fun sortPlaylists(list: List<Playlist>, sort: ListSort): List<Playlist> = sortedByKey(list, { it.name }, sort.descending, when (sort.key) {
    SortKey.Updated -> { p -> p.changed }
    SortKey.Created -> { p -> p.created }
    SortKey.SongCount -> { p -> p.songCount }
    SortKey.Duration -> { p -> p.durationMs }
    else -> null
})

/** Genres (a whole list, sorted here): by name, song count or album count. */
fun sortGenres(list: List<Genre>, sort: ListSort): List<Genre> = sortedByKey(list, { it.name }, sort.descending, when (sort.key) {
    SortKey.SongCount -> { g -> g.songCount }
    SortKey.AlbumCount -> { g -> g.albumCount }
    else -> null
})

/** How many items a list's Pinned group shows, at most. */
const val PINNED_LIMIT = 6

/** How many items a row list's Recent group shows (grids show a row of [RECENT_CARDS]). */
const val RECENT_ROWS = 3
const val RECENT_CARDS = 6

/** How many Recent covers the Library's Albums browse tile fans out (the first of that page's Recent row). */
const val ALBUMS_TILE_COVERS = 3

/** How much play history the Recent groups look through. */
const val RECENT_HISTORY = 200

/** The pinned ids of [kind], in pin order. */
fun pinnedIds(pins: List<PinEntry>, kind: PinKind): List<String> = pins.filter { it.kind == kind.id }.map { it.id }

/** The albums of the songs played last (newest first), each once, [exclude]d ones left out. */
fun recentAlbumIds(history: List<PlayHistoryEntry>, limit: Int, exclude: Collection<String> = emptySet()): List<String> =
    history.filterNot { it.isGone() }.mapNotNull { it.track.albumId }.distinct().filter { it !in exclude }.take(limit)

/** The artists of the songs played last (a song's own artist, else its first credited one), each once. */
fun recentArtistIds(history: List<PlayHistoryEntry>, limit: Int, exclude: Collection<String> = emptySet()): List<String> =
    history.filterNot { it.isGone() }.mapNotNull { e -> e.track.artistId ?: e.track.artists?.firstNotNullOfOrNull { it.id } }.distinct().filter { it !in exclude }.take(limit)

/**
 * A history entry whose song is gone from the library: the core still lists the play, with a
 * placeholder summary (the id for a title, no album). Left out of the Recent groups.
 */
fun PlayHistoryEntry.isGone(): Boolean = track.title == track.id && track.album == null

/** The songs played last, each once, leaving out songs no longer in the library. */
fun recentTracks(history: List<PlayHistoryEntry>, limit: Int, exclude: Collection<String> = emptySet()): List<TrackSummary> =
    history.filterNot { it.isGone() }.map { it.track }.distinctBy { it.id }.filter { it.id !in exclude }.take(limit)

/** The genres of [played] (songs, newest play first), each once, as the library lists them. */
fun recentGenres(played: List<Track>, genres: List<Genre>, limit: Int, exclude: Collection<String> = emptySet()): List<Genre> {
    val byName = genres.associateBy { it.name }
    return played.mapNotNull { it.genre }.distinct().filter { it !in exclude }.mapNotNull { byName[it] }.take(limit)
}

/** The playlists changed last (as the add-to-playlist picker's Recent), pinned ones left out. */
fun recentPlaylists(playlists: List<Playlist>, limit: Int, exclude: Collection<String> = emptySet()): List<Playlist> =
    playlists.filter { it.changed != null && it.id !in exclude }.sortedByDescending { it.changed }.take(limit)
