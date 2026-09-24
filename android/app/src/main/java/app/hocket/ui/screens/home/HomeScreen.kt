package app.hocket.ui.screens.home

import app.hocket.ui.nav.ScrollToTopOnReselect
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyRow
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.LargeFlexibleTopAppBar
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBarDefaults
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.produceState
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.input.nestedscroll.nestedScroll
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.navigation.NavHostController
import app.hocket.R
import app.hocket.ui.nav.AccountButton
import app.hocket.ui.nav.BottomContentInset
import app.hocket.core.ArtworkSizes
import app.hocket.core.Commands
import app.hocket.core.Queries
import app.hocket.core.api.Album
import app.hocket.core.api.Page
import app.hocket.core.api.PlayHistoryEntry
import app.hocket.core.api.QueryResult
import app.hocket.core.api.SortOrder
import app.hocket.ui.LocalCoreClient
import app.hocket.ui.components.Artwork
import app.hocket.ui.components.EmptyState
import app.hocket.ui.components.JobsIndicator
import app.hocket.ui.components.JobsSheet
import app.hocket.ui.components.SectionHeader
import app.hocket.ui.components.TrackRow
import app.hocket.ui.components.AlbumStripSkeleton
import app.hocket.ui.components.CarouselItemWidth
import app.hocket.ui.components.GridArtCorner
import app.hocket.ui.components.GridCellText
import app.hocket.ui.components.ListArtCorner
import app.hocket.ui.components.SectionHeaderSkeleton
import app.hocket.ui.components.ShimmerHost
import app.hocket.ui.components.TrackRowSkeleton
import androidx.compose.foundation.indication
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.layout.heightIn
import androidx.compose.material3.ripple
import androidx.compose.ui.draw.clip
import app.hocket.ui.nav.Route

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun HomeScreen(nav: NavHostController) {
    val client = LocalCoreClient.current
    val server by client.server.collectAsStateWithLifecycle()
    val serverId = server?.id ?: return
    val libraryGen by client.libraryChanged.collectAsStateWithLifecycle(initialValue = null)
    // History changes when a new track starts, not on every now-playing update (rating, position,
    // state): key the re-query on the track id only. null until the first answer, so loading
    // shows skeletons instead of a false "nothing here yet".
    val nowPlayingId = client.nowPlaying.collectAsStateWithLifecycle().value?.track?.id
    val recent by produceState<List<PlayHistoryEntry>?>(null, libraryGen, nowPlayingId) {
        value = (client.query(Queries.recentlyPlayed(20)) as? QueryResult.History)?.data ?: value ?: emptyList()
    }
    val added by produceState<List<Album>?>(null, libraryGen) {
        value = (client.query(Queries.albums(serverId, Page(0u, 20u), SortOrder.DateAdded, descending = true)) as? QueryResult.Albums)?.data?.items ?: value ?: emptyList()
    }
    val played by produceState<List<Album>?>(null, libraryGen) {
        value = (client.query(Queries.albums(serverId, Page(0u, 20u), SortOrder.PlayCount, descending = true)) as? QueryResult.Albums)?.data?.items?.filter { it.playCount > 0u } ?: value ?: emptyList()
    }
    val saved by client.savedQueues.collectAsStateWithLifecycle()
    val scroll = TopAppBarDefaults.exitUntilCollapsedScrollBehavior()
    var jobs by remember { mutableStateOf(false) }
    Scaffold(
        modifier = Modifier.nestedScroll(scroll.nestedScrollConnection),
        topBar = { LargeFlexibleTopAppBar(title = { Text(stringResource(R.string.home_greeting)) }, subtitle = { Text(server?.name ?: "") }, actions = { JobsIndicator(onClick = { jobs = true }); AccountButton() }, scrollBehavior = scroll) },
    ) { padding ->
        val recentList = recent
        val addedList = added
        val playedList = played
        val loading = recentList == null || addedList == null
        if (!loading && recentList.isEmpty() && addedList.isEmpty()) {
            // Only the top: the shell already keeps content clear of the navigation bar.
            EmptyState(stringResource(R.string.empty_home_title), stringResource(R.string.empty_home_body), Modifier.padding(top = padding.calculateTopPadding()), stringResource(R.string.nav_library)) { nav.navigate(Route.Library()) }
            return@Scaffold
        }
        val continueLabel = stringResource(R.string.home_continue)
        val listState = rememberLazyListState()
        ScrollToTopOnReselect(listState)
        ShimmerHost {
            LazyColumn(Modifier.fillMaxSize(), state = listState, contentPadding = PaddingValues(top = padding.calculateTopPadding(), bottom = BottomContentInset)) {
                if (loading) {
                    item(key = "sk.h1", contentType = "sectionSkeleton") { SectionHeaderSkeleton() }
                    items(5, key = { "sk.r$it" }, contentType = { "rowSkeleton" }) { TrackRowSkeleton() }
                    item(key = "sk.h2", contentType = "sectionSkeleton") { SectionHeaderSkeleton() }
                    item(key = "sk.s", contentType = "stripSkeleton") { AlbumStripSkeleton() }
                    return@LazyColumn
                }
                if (recentList.isNotEmpty()) {
                    val ids = recentList.map { it.track.id }.distinct()
                    item(key = "h.continue", contentType = "header") { SectionHeader(stringResource(R.string.home_continue), Modifier.animateItem()) }
                    items(recentList.take(5), key = { "r" + it.playedAt + it.track.id }, contentType = { "track" }) { entry ->
                        TrackRow(entry.track, onClick = { client.dispatch(Commands.playTracks(serverId, ids, ids.indexOf(entry.track.id), continueLabel)) },
                            modifier = Modifier.animateItem(),
                            trailing = { Text(entryAgo(entry), style = MaterialTheme.typography.labelSmall, color = MaterialTheme.colorScheme.onSurfaceVariant, modifier = Modifier.padding(start = 8.dp)) })
                    }
                }
                if (!playedList.isNullOrEmpty()) {
                    item(key = "h.played", contentType = "header") { SectionHeader(stringResource(R.string.home_recently_played), Modifier.animateItem()) }
                    item(key = "s.played", contentType = "strip") { AlbumStrip(playedList, Modifier.animateItem()) { nav.navigate(Route.Album(it.id)) } }
                }
                if (addedList.isNotEmpty()) {
                    item(key = "h.added", contentType = "header") { SectionHeader(stringResource(R.string.home_recently_added), Modifier.animateItem()) }
                    item(key = "s.added", contentType = "strip") { AlbumStrip(addedList, Modifier.animateItem()) { nav.navigate(Route.Album(it.id)) } }
                }
                if (saved.isNotEmpty()) {
                    item(key = "h.saved", contentType = "header") { SectionHeader(stringResource(R.string.home_saved_queues), Modifier.animateItem()) { TextButton(onClick = { nav.navigate(Route.SavedQueues) }) { Text(stringResource(R.string.home_see_all)) } } }
                    items(saved.take(3), key = { "q" + it.id }, contentType = { "saved" }) { sq ->
                        Row(Modifier.animateItem().fillMaxWidth().clickable { client.dispatch(Commands.restoreSavedQueue(sq.id)) }.heightIn(min = 64.dp).padding(horizontal = 16.dp, vertical = 8.dp), verticalAlignment = androidx.compose.ui.Alignment.CenterVertically) {
                            Artwork(sq.coverArt, ArtworkSizes.THUMB, null, Modifier.size(48.dp), RoundedCornerShape(ListArtCorner))
                            Spacer(Modifier.width(14.dp))
                            Column(Modifier.weight(1f)) {
                                Text(sq.label, style = MaterialTheme.typography.bodyLarge, maxLines = 1, overflow = TextOverflow.Ellipsis)
                                Text(stringResource(R.string.saved_tracks, sq.trackCount.toInt()), style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
                            }
                        }
                    }
                }
            }
        }
    }
    if (jobs) JobsSheet { jobs = false }
}

@Composable
private fun entryAgo(entry: PlayHistoryEntry): String = app.hocket.ui.components.formatAgo(entry.playedAt)

/**
 * A horizontal carousel of albums (Navic): 150 dp covers 12 dp apart inside a 16 dp margin, a
 * two-line title and the artist under each. The press ripple is drawn on the cover only.
 */
@Composable
fun AlbumStrip(albums: List<Album>, modifier: Modifier = Modifier, onClick: (Album) -> Unit) {
    LazyRow(modifier, contentPadding = PaddingValues(horizontal = 16.dp), horizontalArrangement = Arrangement.spacedBy(12.dp)) {
        items(albums, key = { it.id }) { album ->
            val interaction = remember { MutableInteractionSource() }
            Column(Modifier.animateItem().width(CarouselItemWidth).clickable(interaction, indication = null) { onClick(album) }) {
                Artwork(album.coverArt, ArtworkSizes.GRID, stringResource(R.string.row_album_a11y, album.name, album.artist ?: ""),
                    Modifier.size(CarouselItemWidth).clip(RoundedCornerShape(GridArtCorner)).indication(interaction, ripple()), RoundedCornerShape(GridArtCorner))
                Spacer(Modifier.height(6.dp))
                GridCellText(album.name, album.artist ?: stringResource(R.string.unknown_artist))
            }
        }
    }
}
