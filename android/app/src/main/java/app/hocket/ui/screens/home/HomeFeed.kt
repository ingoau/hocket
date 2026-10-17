package app.hocket.ui.screens.home

import app.hocket.core.Queries
import app.hocket.core.toSummary
import app.hocket.core.api.Album
import app.hocket.core.api.Artist
import app.hocket.core.api.FilterField
import app.hocket.core.api.FilterNode
import app.hocket.core.api.FilterOp
import app.hocket.core.api.FilterRule
import app.hocket.core.api.FilterValue
import app.hocket.core.api.ListeningStats
import app.hocket.core.api.Page
import app.hocket.core.api.QueryResult
import app.hocket.core.api.SortOrder
import app.hocket.core.api.TrackSummary
import app.hocket.core.client.CoreClient

/** What one Home section shows, loaded from the core's existing queries. */
sealed interface SectionContent {
    /** Section-specific title and the line above it ("Indie rock" / "A genre you've been playing"). */
    val title: String? get() = null
    val isEmpty: Boolean

    data class Albums(val items: List<Album>, override val title: String? = null) : SectionContent { override val isEmpty get() = items.isEmpty() }
    data class Artists(val items: List<Artist>) : SectionContent { override val isEmpty get() = items.isEmpty() }
    data class Tracks(val items: List<TrackSummary>, override val title: String? = null) : SectionContent { override val isEmpty get() = items.isEmpty() }
    data class Listening(val totalMs: Double, val plays: Int, val byWeekday: List<Int>) : SectionContent { override val isEmpty get() = plays == 0 }
    /** Filled from the client's flows (filters, saved queues), not a query. */
    data object FromFlow : SectionContent { override val isEmpty get() = false }
}

/** How many items a section loads. */
const val SECTION_ITEMS = 12

/** "Loved or rated 4+, and not played in the last 90 days". */
private val REDISCOVER: FilterNode = FilterNode.All(listOf(
    FilterNode.Any(listOf(
        FilterNode.Rule(FilterRule(FilterField.Loved, FilterOp.IsTrue, FilterValue.Bool(true))),
        FilterNode.Rule(FilterRule(FilterField.Rating, FilterOp.Gt, FilterValue.Number(3.0))),
    )),
    FilterNode.Rule(FilterRule(FilterField.LastPlayed, FilterOp.NotInTheLast, FilterValue.Days(90u))),
))

/** Listening stats for favourites: the last month's, or the year's when the month has no plays. */
private suspend fun favouriteStats(client: CoreClient): ListeningStats? {
    val month = (client.query(Queries.stats(30)) as? QueryResult.Stats)?.data
    if (month != null && month.totalPlays != 0u) return month
    return (client.query(Queries.stats(365)) as? QueryResult.Stats)?.data ?: month
}

/** The artist "More from …" is about (the most played lately), or null with no plays. */
suspend fun moreFromArtist(client: CoreClient): Artist? = favouriteStats(client)?.topArtists?.firstOrNull()

/**
 * Loads the sections in [shown] (only those: a hidden section costs nothing). Listening stats are
 * read once and shared; a month with no plays falls back to the year, so a quiet month still has
 * favourites.
 */
suspend fun loadHomeFeed(client: CoreClient, serverId: String, shown: Set<HomeSection>): Map<HomeSection, SectionContent> {
    val needsStats = shown.any { it == HomeSection.FavouriteArtists || it == HomeSection.TopGenre || it == HomeSection.MoreFromArtist || it == HomeSection.OnRepeat }
    val stats = if (needsStats) favouriteStats(client) else null
    val out = LinkedHashMap<HomeSection, SectionContent>()
    for (section in shown) {
        val content: SectionContent? = when (section) {
            HomeSection.Rediscover -> (client.query(Queries.tracks(serverId, Page(0u, SECTION_ITEMS.toUInt()), SortOrder.Rating, descending = true, filter = REDISCOVER)) as? QueryResult.Tracks)
                ?.data?.items?.map { it.toSummary() }?.let { SectionContent.Tracks(it) }
            HomeSection.Filters, HomeSection.RecentQueues -> SectionContent.FromFlow
            HomeSection.FavouriteArtists -> SectionContent.Artists(stats?.topArtists.orEmpty().take(SECTION_ITEMS))
            HomeSection.OnRepeat -> SectionContent.Albums(stats?.topAlbums.orEmpty().take(SECTION_ITEMS))
            HomeSection.TopGenre -> {
                val genre = stats?.topAlbums.orEmpty().mapNotNull { it.genre?.takeIf(String::isNotBlank) }.groupingBy { it }.eachCount().maxByOrNull { it.value }?.key
                if (genre == null) SectionContent.Albums(emptyList())
                else (client.query(Queries.albums(serverId, Page(0u, SECTION_ITEMS.toUInt()), SortOrder.PlayCount, descending = true, genre = genre)) as? QueryResult.Albums)
                    ?.data?.items?.let { SectionContent.Albums(it, title = genre) }
            }
            HomeSection.MoreFromArtist -> {
                val artist = stats?.topArtists?.firstOrNull()
                if (artist == null) SectionContent.Tracks(emptyList())
                else (client.query(Queries.artistTopSongs(artist.id, SECTION_ITEMS)) as? QueryResult.TrackList)?.data?.map { it.toSummary() }?.let { SectionContent.Tracks(it, title = artist.name) }
            }
            HomeSection.NewInLibrary -> (client.query(Queries.albums(serverId, Page(0u, SECTION_ITEMS.toUInt()), SortOrder.DateAdded, descending = true)) as? QueryResult.Albums)
                ?.data?.items?.let { SectionContent.Albums(it) }
            HomeSection.RecentlyPlayed -> {
                val ids = (client.query(Queries.recentlyPlayed(100)) as? QueryResult.History)?.data.orEmpty().mapNotNull { it.track.albumId }.distinct().take(SECTION_ITEMS)
                SectionContent.Albums(ids.mapNotNull { (client.query(Queries.album(it)) as? QueryResult.AlbumDetail)?.data })
            }
            HomeSection.Stats -> (client.query(Queries.stats(7)) as? QueryResult.Stats)?.data?.let { s ->
                SectionContent.Listening(s.totalMs, s.totalPlays.toInt(), s.playsByWeekday.map { it.toInt() })
            }
        }
        if (content != null) out[section] = content
    }
    return out
}
