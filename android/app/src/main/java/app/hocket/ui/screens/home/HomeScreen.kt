package app.hocket.ui.screens.home

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
import app.hocket.ui.nav.Route

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun HomeScreen(nav: NavHostController) {
    val client = LocalCoreClient.current
    val server by client.server.collectAsStateWithLifecycle()
    val serverId = server?.id ?: return
    val libraryGen by client.libraryChanged.collectAsStateWithLifecycle(initialValue = null)
    val recent by produceState<List<PlayHistoryEntry>>(emptyList(), libraryGen, client.nowPlaying.collectAsStateWithLifecycle().value) {
        value = (client.query(Queries.recentlyPlayed(20)) as? QueryResult.History)?.data ?: emptyList()
    }
    val added by produceState<List<Album>>(emptyList(), libraryGen) {
        value = (client.query(Queries.albums(serverId, Page(0u, 20u), SortOrder.DateAdded, descending = true)) as? QueryResult.Albums)?.data?.items ?: emptyList()
    }
    val played by produceState<List<Album>>(emptyList(), libraryGen) {
        value = (client.query(Queries.albums(serverId, Page(0u, 20u), SortOrder.PlayCount, descending = true)) as? QueryResult.Albums)?.data?.items?.filter { it.playCount > 0u } ?: emptyList()
    }
    val saved by client.savedQueues.collectAsStateWithLifecycle()
    val scroll = TopAppBarDefaults.exitUntilCollapsedScrollBehavior()
    var jobs by remember { mutableStateOf(false) }
    Scaffold(
        modifier = Modifier.nestedScroll(scroll.nestedScrollConnection),
        topBar = { LargeFlexibleTopAppBar(title = { Text(stringResource(R.string.home_greeting)) }, subtitle = { Text(server?.name ?: "") }, actions = { JobsIndicator(onClick = { jobs = true }); AccountButton() }, scrollBehavior = scroll) },
    ) { padding ->
        if (recent.isEmpty() && added.isEmpty()) {
            EmptyState(stringResource(R.string.empty_home_title), stringResource(R.string.empty_home_body), Modifier.padding(padding), stringResource(R.string.nav_library)) { nav.navigate(Route.Library()) }
            return@Scaffold
        }
        val continueLabel = stringResource(R.string.home_continue)
        LazyColumn(Modifier.fillMaxSize(), contentPadding = PaddingValues(top = padding.calculateTopPadding(), bottom = BottomContentInset)) {
            if (recent.isNotEmpty()) {
                item { SectionHeader(stringResource(R.string.home_continue)) }
                items(recent.take(5), key = { "r" + it.playedAt + it.track.id }) { entry ->
                    TrackRow(entry.track, onClick = { client.dispatch(Commands.playTracks(serverId, recent.map { it.track.id }.distinct(), recent.map { it.track.id }.distinct().indexOf(entry.track.id), continueLabel)) },
                        trailing = { Text(entryAgo(entry), style = MaterialTheme.typography.labelSmall, color = MaterialTheme.colorScheme.onSurfaceVariant, modifier = Modifier.padding(start = 8.dp)) })
                }
            }
            if (played.isNotEmpty()) {
                item { SectionHeader(stringResource(R.string.home_recently_played)) }
                item { AlbumStrip(played) { nav.navigate(Route.Album(it.id)) } }
            }
            if (added.isNotEmpty()) {
                item { SectionHeader(stringResource(R.string.home_recently_added)) }
                item { AlbumStrip(added) { nav.navigate(Route.Album(it.id)) } }
            }
            if (saved.isNotEmpty()) {
                item { SectionHeader(stringResource(R.string.home_saved_queues)) { TextButton(onClick = { nav.navigate(Route.SavedQueues) }) { Text(stringResource(R.string.home_see_all)) } } }
                items(saved.take(3), key = { it.id }) { sq ->
                    Row(Modifier.fillMaxWidth().clickable { client.dispatch(Commands.restoreSavedQueue(sq.id)) }.padding(horizontal = 16.dp, vertical = 8.dp), verticalAlignment = androidx.compose.ui.Alignment.CenterVertically) {
                        Artwork(sq.coverArt, ArtworkSizes.THUMB, null, Modifier.size(48.dp), RoundedCornerShape(8.dp))
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
    if (jobs) JobsSheet { jobs = false }
}

@Composable
private fun entryAgo(entry: PlayHistoryEntry): String = app.hocket.ui.components.formatAgo(entry.playedAt)

@Composable
fun AlbumStrip(albums: List<Album>, onClick: (Album) -> Unit) {
    LazyRow(contentPadding = PaddingValues(horizontal = 16.dp), horizontalArrangement = Arrangement.spacedBy(12.dp)) {
        items(albums, key = { it.id }) { album ->
            Column(Modifier.width(140.dp).clickable { onClick(album) }) {
                Artwork(album.coverArt, ArtworkSizes.GRID, stringResource(R.string.row_album_a11y, album.name, album.artist ?: ""), Modifier.size(140.dp), RoundedCornerShape(16.dp))
                Spacer(Modifier.height(6.dp))
                Text(album.name, style = MaterialTheme.typography.titleSmall, maxLines = 1, overflow = TextOverflow.Ellipsis)
                Text(album.artist ?: stringResource(R.string.unknown_artist), style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant, maxLines = 1, overflow = TextOverflow.Ellipsis)
            }
        }
    }
}
