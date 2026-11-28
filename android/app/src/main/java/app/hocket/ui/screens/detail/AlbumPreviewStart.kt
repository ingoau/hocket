package app.hocket.ui.screens.detail

import app.hocket.core.api.Track

/** Titles that open an album rather than being one of its songs: "Intro", "Prelude", "Skit (feat. …)", "Something (Intro)". */
private val OPENER_TITLE = Regex("""\b(intro|introduction|prelude|prologue|overture|interlude|skit)\b""", RegexOption.IGNORE_CASE)

/** An opener by its title is skipped only when it's no longer than this, so a real song called "Intro" still opens. */
private const val OPENER_TITLE_MAX_MS = 150_000L

/** A song this short (and well below the album's typical length) is an opener whatever it's called. */
private const val OPENER_SHORT_MAX_MS = 90_000L
private const val OPENER_SHORT_OF_MEDIAN = 0.4

/** At most this many openers are skipped. */
private const val OPENER_MAX_SKIP = 2

/**
 * The card the album preview opens on: the song that's playing when it's this album's, or else the
 * first song past a short intro, skit or interlude at the start, so the preview doesn't open on 40 s
 * of ambience. The skipped cards stay one swipe back.
 */
internal fun albumPreviewStart(tracks: List<Track>, playingId: String?): Int {
    val playing = tracks.indexOfFirst { it.id == playingId }
    if (playing >= 0) return playing
    return firstSongIndex(tracks.map { it.title to it.durationMs.toLong() })
}

/** The index of the first song after the album's openers ([songs]: title and length in ms, 0 when unknown). */
internal fun firstSongIndex(songs: List<Pair<String, Long>>): Int {
    val known = songs.map { it.second }.filter { it > 0 }.sorted()
    val median = if (known.isEmpty()) 0L else known[known.size / 2]
    fun opener(title: String, ms: Long): Boolean {
        // An unknown length (0) counts as short.
        val byTitle = OPENER_TITLE.containsMatchIn(title) && ms <= OPENER_TITLE_MAX_MS
        // Only against an album of at least four songs, so a short EP or an album of short songs keeps its first.
        val byLength = songs.size >= 4 && ms in 1 until OPENER_SHORT_MAX_MS && ms < median * OPENER_SHORT_OF_MEDIAN
        return byTitle || byLength
    }
    var i = 0
    while (i < OPENER_MAX_SKIP && i < songs.lastIndex && opener(songs[i].first, songs[i].second)) i++
    return i
}
