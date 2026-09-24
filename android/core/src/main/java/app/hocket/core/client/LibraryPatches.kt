package app.hocket.core.client

import app.hocket.core.api.Album
import app.hocket.core.api.Artist
import app.hocket.core.api.LibraryItemKind
import app.hocket.core.api.LibraryItemState
import app.hocket.core.api.QueueEntry
import app.hocket.core.api.QueueView
import app.hocket.core.api.SearchResults
import app.hocket.core.api.Track
import app.hocket.core.api.TrackSummary

/**
 * In-place updates for copies of library items a screen already holds, from `Event.LibraryItemsChanged`
 * (a rating or love set here, undone, or set on another signed-in device). The core has already
 * written these values to its mirror; this only saves a screen a refetch (search results, a menu's
 * track, the now-playing entry until the core's own `NowPlayingChanged` lands). No decisions here:
 * the values are the core's.
 */
object LibraryPatches {
    private fun find(items: List<LibraryItemState>, kind: LibraryItemKind, id: String) = items.firstOrNull { it.kind == kind && it.id == id }

    fun track(t: TrackSummary, items: List<LibraryItemState>): TrackSummary =
        find(items, LibraryItemKind.Track, t.id)?.let { s -> if (s.rating == t.rating && s.loved == t.loved) t else t.copy(rating = s.rating, loved = s.loved) } ?: t

    fun track(t: Track, items: List<LibraryItemState>): Track =
        find(items, LibraryItemKind.Track, t.id)?.let { s -> if (s.rating == t.rating && s.loved == t.loved) t else t.copy(rating = s.rating, loved = s.loved) } ?: t

    fun album(a: Album, items: List<LibraryItemState>): Album =
        find(items, LibraryItemKind.Album, a.id)?.let { s -> if (s.rating == a.rating && s.loved == a.loved) a else a.copy(rating = s.rating, loved = s.loved) } ?: a

    fun artist(a: Artist, items: List<LibraryItemState>): Artist =
        find(items, LibraryItemKind.Artist, a.id)?.let { s -> if (s.loved == a.loved) a else a.copy(loved = s.loved) } ?: a

    fun entry(e: QueueEntry, items: List<LibraryItemState>): QueueEntry = track(e.track, items).let { if (it === e.track) e else e.copy(track = it) }

    fun search(r: SearchResults, items: List<LibraryItemState>): SearchResults = r.copy(
        tracks = r.tracks.map { track(it, items) },
        albums = r.albums.map { album(it, items) },
        artists = r.artists.map { artist(it, items) },
    )

    fun queue(q: QueueView, items: List<LibraryItemState>): QueueView = q.copy(
        history = q.history.map { entry(it, items) },
        current = q.current?.let { entry(it, items) },
        playingNext = q.playingNext.map { entry(it, items) },
        upcoming = q.upcoming.map { entry(it, items) },
    )
}
