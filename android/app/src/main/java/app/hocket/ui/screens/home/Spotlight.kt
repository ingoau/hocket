package app.hocket.ui.screens.home

import androidx.annotation.StringRes
import app.hocket.R
import app.hocket.core.Queries
import app.hocket.core.api.Album
import app.hocket.core.api.AlbumResumePoint
import app.hocket.core.api.Artist
import app.hocket.core.api.FilterField
import app.hocket.core.api.FilterNode
import app.hocket.core.api.FilterOp
import app.hocket.core.api.FilterRule
import app.hocket.core.api.FilterValue
import app.hocket.core.api.ListeningStats
import app.hocket.core.api.Page
import app.hocket.core.api.Playlist
import app.hocket.core.api.QueryResult
import app.hocket.core.api.SortOrder
import app.hocket.core.api.TrackSummary
import app.hocket.core.client.CoreClient
import kotlin.math.pow
import kotlin.random.Random

/**
 * Home's Spotlight: up to [SPOTLIGHT_MAX] big slides, each one reason to play something. A slide
 * with nothing behind it (no album to resume, no plays yet) isn't made; what's playing now isn't
 * shown, and no album or artist is shown twice. Which reasons come up, and which album each picks,
 * is a little random, the same for a day within one run of the app (so a song changing doesn't
 * reshuffle the carousel under the user's thumb). The desktop's twin is `desktop/src/renderer/lib/spotlight.ts`.
 */
enum class SlideKind(@StringRes val eyebrow: Int, val weight: Double) {
    Resume(R.string.spotlight_resume, 0.0),
    OnRepeat(R.string.spotlight_on_repeat, 3.0),
    NewlyAdded(R.string.spotlight_newly_added, 3.0),
    Rediscover(R.string.spotlight_rediscover, 3.0),
    TopGenre(R.string.spotlight_top_genre, 2.0),
    TopArtist(R.string.spotlight_top_artist, 2.5),
    Playlist(R.string.spotlight_playlist, 1.5),
    LuckyDip(R.string.spotlight_lucky_dip, 1.0),
}

data class SpotlightSlide(
    val kind: SlideKind,
    val album: Album? = null,
    val artist: Artist? = null,
    val playlist: Playlist? = null,
    val resume: AlbumResumePoint? = null,
    /** The genre a [SlideKind.TopGenre] slide is about. */
    val genre: String? = null,
    /** Artwork for an artist without an image of their own: one of their albums'. */
    val fallbackCover: String? = null,
) {
    val key: String get() = kind.name + ":" + (album?.id ?: artist?.id ?: playlist?.id)
    val title: String get() = album?.name ?: artist?.name ?: playlist?.name ?: ""
    val coverArt: String? get() = album?.coverArt ?: artist?.coverArt ?: playlist?.coverArt ?: fallbackCover
}

const val SPOTLIGHT_MAX = 5

/** The day's shuffle within this run of the app. */
private val sessionSeed = Random.nextInt()
fun spotlightSeed(now: Long = System.currentTimeMillis()): Int = sessionSeed xor (now / 86_400_000L).toInt()

private val LOVED_NOT_LATELY: FilterNode = FilterNode.All(listOf(
    FilterNode.Any(listOf(
        FilterNode.Rule(FilterRule(FilterField.Loved, FilterOp.IsTrue, FilterValue.Bool(true))),
        FilterNode.Rule(FilterRule(FilterField.Rating, FilterOp.Gt, FilterValue.Number(3.0))),
    )),
    FilterNode.Rule(FilterRule(FilterField.LastPlayed, FilterOp.NotInTheLast, FilterValue.Days(60u))),
))

/** An album last played this long ago counts as one to rediscover. */
private const val REDISCOVER_AFTER_MS = 60.0 * 86_400_000

/**
 * The slides, Resume (the album left most recently) first, then the other reasons in a weighted
 * shuffle. [stats] are the favourites' stats (the month's, or the year's in a quiet month);
 * [playing] is excluded along with its artist.
 */
suspend fun loadSpotlight(
    client: CoreClient,
    serverId: String,
    resumes: List<AlbumResumePoint>,
    playing: TrackSummary?,
    stats: ListeningStats?,
    seed: Int,
    now: Double = System.currentTimeMillis().toDouble(),
): List<SpotlightSlide> {
    val picker = SpotlightPicker(playing?.albumId, playing?.artistId)
    val order = listOf(SlideKind.Resume) + weightedShuffle(SlideKind.entries.filter { it.weight > 0 }, Random(seed)) { it.weight }
    suspend fun album(id: String): Album? = (client.query(Queries.album(id)) as? QueryResult.AlbumDetail)?.data
    suspend fun albums(sort: SortOrder, limit: Int, genre: String? = null): List<Album> =
        (client.query(Queries.albums(serverId, Page(0u, limit.toUInt()), sort, descending = true, genre = genre)) as? QueryResult.Albums)?.data?.items.orEmpty()
    for (kind in order) {
        if (picker.slides.size >= SPOTLIGHT_MAX) break
        val rnd = Random(seed + kind.ordinal * 7919)
        when (kind) {
            SlideKind.Resume -> for (p in resumes.sortedByDescending { it.updatedAt }) {
                if (!picker.canUse(p.albumId, null)) continue
                val a = album(p.albumId) ?: continue
                if (picker.offer(SpotlightSlide(kind, album = a, resume = p))) break
            }
            SlideKind.OnRepeat -> picker.offerAlbums(kind, stats?.topAlbums.orEmpty().take(5).shuffled(rnd))
            SlideKind.NewlyAdded -> {
                val newest = albums(SortOrder.DateAdded, 8)
                // Often the very newest; otherwise one of the last few added.
                picker.offerAlbums(kind, if (rnd.nextBoolean()) newest else newest.shuffled(rnd))
            }
            SlideKind.Rediscover -> {
                val fromLoved = (client.query(Queries.tracks(serverId, Page(0u, 40u), SortOrder.Random, descending = false, filter = LOVED_NOT_LATELY)) as? QueryResult.Tracks)
                    ?.data?.items.orEmpty().mapNotNull { it.albumId }.distinct().shuffled(rnd).take(4)
                val year = (client.query(Queries.stats(365)) as? QueryResult.Stats)?.data?.topAlbums.orEmpty()
                    .filter { a -> a.lastPlayed?.let { now - it > REDISCOVER_AFTER_MS } == true }.shuffled(rnd).take(4)
                val candidates = (year.map { it.id to it } + fromLoved.map { it to null }).shuffled(rnd)
                for ((id, known) in candidates) {
                    if (!picker.canUse(id, known?.artistId)) continue
                    val a = known ?: album(id) ?: continue
                    if (picker.offer(SpotlightSlide(kind, album = a))) break
                }
            }
            SlideKind.TopGenre -> {
                val genre = stats?.topAlbums.orEmpty().mapNotNull { it.genre?.takeIf(String::isNotBlank) }.groupingBy { it }.eachCount().maxByOrNull { it.value }?.key
                if (genre != null) picker.offerAlbums(kind, albums(SortOrder.PlayCount, 12, genre).shuffled(rnd), genre)
            }
            SlideKind.TopArtist -> for (a in stats?.topArtists.orEmpty().take(4).shuffled(rnd)) {
                if (!picker.canUse(null, a.id)) continue
                val cover = if (a.coverArt != null) null else (stats?.topAlbums.orEmpty().firstOrNull { it.artistId == a.id }
                    ?: (client.query(Queries.albums(serverId, Page(0u, 1u), SortOrder.PlayCount, descending = true, artistId = a.id)) as? QueryResult.Albums)?.data?.items?.firstOrNull())?.coverArt
                if (picker.offer(SpotlightSlide(kind, artist = a, fallbackCover = cover))) break
            }
            SlideKind.Playlist -> {
                val lists = (client.query(Queries.playlists(serverId)) as? QueryResult.Playlists)?.data.orEmpty()
                    .filter { it.songCount > 0u }.sortedByDescending { it.changed ?: it.created ?: 0.0 }.take(4)
                lists.shuffled(rnd).firstOrNull()?.let { picker.offer(SpotlightSlide(kind, playlist = it)) }
            }
            SlideKind.LuckyDip -> picker.offerAlbums(kind, albums(SortOrder.Random, 6))
        }
    }
    return picker.slides
}

/** Collects slides, refusing an album or artist already shown (or playing). */
internal class SpotlightPicker(playingAlbum: String?, playingArtist: String?) {
    val slides = mutableListOf<SpotlightSlide>()
    private val albums = mutableSetOf<String>().apply { playingAlbum?.let(::add) }
    private val artists = mutableSetOf<String>().apply { playingArtist?.let(::add) }

    fun canUse(albumId: String?, artistId: String?): Boolean = (albumId == null || albumId !in albums) && (artistId == null || artistId !in artists)

    fun offer(slide: SpotlightSlide): Boolean {
        if (slides.size >= SPOTLIGHT_MAX) return false
        val albumId = slide.album?.id
        val artistId = slide.album?.artistId ?: slide.artist?.id
        if (!canUse(albumId, artistId)) return false
        if (slide.playlist != null && slides.any { it.playlist?.id == slide.playlist.id }) return false
        albumId?.let(albums::add)
        artistId?.let(artists::add)
        slides += slide
        return true
    }

    fun offerAlbums(kind: SlideKind, candidates: List<Album>, genre: String? = null) {
        for (a in candidates) if (offer(SpotlightSlide(kind, album = a, genre = genre))) return
    }
}

/** [items] in a random order where heavier ones tend to come first (Efraimidis–Spirakis). */
internal fun <T> weightedShuffle(items: List<T>, rnd: Random, weight: (T) -> Double): List<T> =
    items.map { it to rnd.nextDouble().pow(1.0 / weight(it)) }.sortedByDescending { it.second }.map { it.first }
