package app.hocket.ui.screens.search

import app.hocket.ui.nav.ScrollToTopOnReselect
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.width
import androidx.compose.ui.layout.onSizeChanged
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Close
import androidx.compose.material.icons.filled.Search
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.LoadingIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.runtime.snapshotFlow
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.navigation.NavHostController
import app.hocket.R
import app.hocket.ui.nav.AccountButton
import app.hocket.ui.nav.BottomContentInset
import app.hocket.core.Commands
import app.hocket.core.Queries
import app.hocket.core.api.QueryResult
import app.hocket.core.api.SearchResults
import app.hocket.ui.LocalCoreClient
import app.hocket.ui.components.ActionSheet
import app.hocket.ui.screens.home.AlbumStrip
import app.hocket.ui.components.ArtistRow
import app.hocket.ui.components.EmptyState
import app.hocket.ui.components.PlaylistRow
import app.hocket.ui.components.SectionHeader
import app.hocket.ui.components.TrackRow
import app.hocket.ui.nav.Route
import kotlinx.coroutines.FlowPreview
import kotlinx.coroutines.flow.debounce

/**
 * Local-first, instant as you type. Server results arrive in `Event.SearchResults` and are appended
 * below a divider; the results container keeps its previous minimum height between the two batches
 * so nothing shifts under the finger (design: search).
 */
@OptIn(FlowPreview::class)
@Composable
fun SearchScreen(nav: NavHostController) {
    val client = LocalCoreClient.current
    val server by client.server.collectAsStateWithLifecycle()
    val serverId = server?.id ?: return
    var query by rememberSaveable { mutableStateOf("") }
    var local by remember { mutableStateOf<SearchResults?>(null) }
    var remote by remember { mutableStateOf<SearchResults?>(null) }
    var pendingRemote by remember { mutableStateOf(false) }
    var requestCounter by remember { mutableIntStateOf(0) }
    var minHeightPx by remember { mutableIntStateOf(0) }
    val focus = remember { FocusRequester() }
    var sheetFor by remember { mutableStateOf<app.hocket.core.api.TrackSummary?>(null) }

    LaunchedEffect(Unit) {
        snapshotFlow { query }.debounce(120).collect { q ->
            val text = q.trim()
            if (text.isEmpty()) { local = null; remote = null; pendingRemote = false; minHeightPx = 0; return@collect }
            val id = "s${requestCounter++}"
            val res = (client.query(Queries.search(serverId, text, 25, includeServer = true, requestId = id)) as? QueryResult.Search)?.data
            local = res
            remote = null
            pendingRemote = true
        }
    }
    LaunchedEffect(Unit) {
        client.searchResults.collect { r -> if (r.fromServer && r.query == query.trim()) { remote = r; pendingRemote = false } }
    }
    // The keyboard comes up on the first visit only; coming back to the tab keeps the results in view.
    var focusedOnce by rememberSaveable { mutableStateOf(false) }
    LaunchedEffect(Unit) {
        if (!focusedOnce) { focusedOnce = true; runCatching { focus.requestFocus() } }
    }

    Column(Modifier.fillMaxSize().statusBarsPadding()) {
        Row(verticalAlignment = androidx.compose.ui.Alignment.CenterVertically) {
            OutlinedTextField(
                value = query, onValueChange = { query = it }, singleLine = true, placeholder = { Text(stringResource(R.string.search_hint)) },
                leadingIcon = { Icon(Icons.Filled.Search, null) },
                trailingIcon = { if (query.isNotEmpty()) IconButton(onClick = { query = "" }) { Icon(Icons.Filled.Close, stringResource(R.string.search_clear)) } },
                modifier = Modifier.weight(1f).padding(start = 16.dp, end = 4.dp, top = 8.dp, bottom = 8.dp).focusRequester(focus).testTag("search.input"),
            )
            AccountButton(Modifier.padding(end = 4.dp))
        }
        val results = local
        if (results == null) { EmptyState(stringResource(R.string.empty_search_title), stringResource(R.string.empty_search_body)); return }
        val empty = results.tracks.isEmpty() && results.albums.isEmpty() && results.artists.isEmpty() && results.playlists.isEmpty()
        val density = LocalDensity.current
        Box(Modifier.heightIn(min = with(density) { minHeightPx.toDp() }).onSizeChangedKeepMax { if (it > minHeightPx) minHeightPx = it }) {
            val listState = rememberLazyListState()
            ScrollToTopOnReselect(listState)
            LazyColumn(Modifier.fillMaxSize(), state = listState, contentPadding = PaddingValues(bottom = BottomContentInset)) {
                if (empty && remote == null && !pendingRemote) item { EmptyState(stringResource(R.string.empty_search_none, query), "") }
                if (results.tracks.isNotEmpty()) {
                    item { SectionHeader(stringResource(R.string.search_songs)) }
                    items(results.tracks, key = { "t" + it.id }) { t ->
                        TrackRow(t, onClick = { client.dispatch(Commands.playTracks(serverId, results.tracks.map { it.id }, results.tracks.indexOf(t), query)) }, onMore = { sheetFor = t })
                    }
                }
                if (results.albums.isNotEmpty()) {
                    item { SectionHeader(stringResource(R.string.search_albums)) }
                    item(key = "albums") { AlbumStrip(results.albums, Modifier.animateItem()) { a -> nav.navigate(Route.Album(a.id)) } }
                }
                if (results.artists.isNotEmpty()) {
                    item { SectionHeader(stringResource(R.string.search_artists)) }
                    items(results.artists, key = { "ar" + it.id }) { ar -> ArtistRow(ar, onClick = { nav.navigate(Route.Artist(ar.id)) }) }
                }
                if (results.playlists.isNotEmpty()) {
                    item { SectionHeader(stringResource(R.string.search_playlists)) }
                    items(results.playlists, key = { "p" + it.id }) { p -> PlaylistRow(p, onClick = { nav.navigate(Route.Playlist(p.id)) }) }
                }
                item {
                    HorizontalDivider(Modifier.padding(vertical = 8.dp))
                    Row(Modifier.padding(horizontal = 16.dp), verticalAlignment = androidx.compose.ui.Alignment.CenterVertically) {
                        Text(stringResource(R.string.search_from_server), style = MaterialTheme.typography.titleMedium)
                        Spacer(Modifier.width(12.dp))
                        if (pendingRemote) LoadingIndicator(Modifier.height(24.dp))
                    }
                    if (pendingRemote) Text(stringResource(R.string.search_server_pending), style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant, modifier = Modifier.padding(horizontal = 16.dp))
                }
                remote?.let { r ->
                    items(r.tracks, key = { "rt" + it.id }) { t -> TrackRow(t, onClick = { client.dispatch(Commands.playTracks(serverId, r.tracks.map { it.id }, r.tracks.indexOf(t), query)) }, onMore = { sheetFor = t }) }
                    if (r.albums.isNotEmpty()) item(key = "ralbums") { AlbumStrip(r.albums, Modifier.animateItem().padding(vertical = 8.dp)) { a -> nav.navigate(Route.Album(a.id)) } }
                    items(r.artists, key = { "rar" + it.id }) { ar -> ArtistRow(ar, onClick = { nav.navigate(Route.Artist(ar.id)) }) }
                }
            }
        }
    }
    sheetFor?.let { t -> ActionSheet(Commands.tracks(listOf(t.id)), t.title, t.artist, onDismiss = { sheetFor = null }, onGoToAlbum = t.albumId?.let { id -> { nav.navigate(Route.Album(id)) } }, onGoToArtist = t.artistId?.let { id -> { nav.navigate(Route.Artist(id)) } }) }
}


private fun Modifier.onSizeChangedKeepMax(onHeight: (Int) -> Unit): Modifier = this.onSizeChanged { onHeight(it.height) }
