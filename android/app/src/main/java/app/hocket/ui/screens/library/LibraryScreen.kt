package app.hocket.ui.screens.library

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.grid.GridCells
import androidx.compose.foundation.lazy.grid.LazyVerticalGrid
import androidx.compose.foundation.lazy.grid.rememberLazyGridState
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.pager.HorizontalPager
import androidx.compose.foundation.pager.rememberPagerState
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Check
import androidx.compose.material.icons.filled.Sort
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.MediumFlexibleTopAppBar
import androidx.compose.material3.PrimaryScrollableTabRow
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Tab
import androidx.compose.material3.Text
import androidx.compose.material3.TopAppBarDefaults
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.input.nestedscroll.nestedScroll
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.navigation.NavHostController
import app.hocket.R
import app.hocket.ui.nav.BottomContentInset
import app.hocket.core.Commands
import app.hocket.core.Queries
import app.hocket.core.api.Genre
import app.hocket.core.api.Playlist
import app.hocket.core.api.QueryResult
import app.hocket.core.api.SortOrder
import app.hocket.core.client.AlbumListKey
import app.hocket.core.client.ArtistListKey
import app.hocket.core.client.SelectionKind
import app.hocket.core.client.TrackListKey
import app.hocket.core.toSummary
import app.hocket.ui.LocalCoreClient
import app.hocket.ui.LocalWideLayout
import app.hocket.ui.components.ActionSheet
import app.hocket.ui.components.AlbumCard
import app.hocket.ui.components.ArtistRow
import app.hocket.ui.components.EmptyState
import app.hocket.ui.components.GenreRow
import app.hocket.ui.components.JobsIndicator
import app.hocket.ui.components.JobsSheet
import app.hocket.ui.components.PlaylistRow
import app.hocket.ui.components.SelectionToolbar
import app.hocket.ui.components.TrackRow
import app.hocket.ui.nav.Route
import app.hocket.ui.screens.detail.DetailPane
import kotlinx.coroutines.launch

@Composable
fun sortLabel(sort: SortOrder): String = when (sort) {
    SortOrder.Default -> stringResource(R.string.sort_default); SortOrder.Title -> stringResource(R.string.sort_title); SortOrder.Artist -> stringResource(R.string.sort_artist)
    SortOrder.Album -> stringResource(R.string.sort_album); SortOrder.Year -> stringResource(R.string.sort_year); SortOrder.DateAdded -> stringResource(R.string.sort_date_added)
    SortOrder.Rating -> stringResource(R.string.sort_rating); SortOrder.PlayCount -> stringResource(R.string.sort_play_count); SortOrder.Duration -> stringResource(R.string.sort_duration)
    SortOrder.Random -> stringResource(R.string.sort_random); SortOrder.Bpm -> stringResource(R.string.sort_bpm); SortOrder.Energy -> stringResource(R.string.sort_energy)
}

/** What a wide layout shows in its detail pane. */
sealed interface DetailTarget {
    data class Album(val id: String) : DetailTarget
    data class Artist(val id: String) : DetailTarget
    data class Playlist(val id: String) : DetailTarget
    data class Genre(val name: String) : DetailTarget
}

/**
 * Library tabs (albums grid, artists, playlists, songs, genres) as virtualised lists over the page
 * caches. On >= 600 dp the list and a detail pane sit side by side; on phones details are routes.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun LibraryScreen(nav: NavHostController, initialTab: Int = 0) {
    val client = LocalCoreClient.current
    val wide = LocalWideLayout.current
    val server by client.server.collectAsStateWithLifecycle()
    val serverId = server?.id ?: return
    val tabs = listOf(R.string.library_albums, R.string.library_artists, R.string.library_playlists, R.string.library_songs, R.string.library_genres)
    val pager = rememberPagerState(initialPage = initialTab.coerceIn(0, tabs.lastIndex)) { tabs.size }
    val scope = rememberCoroutineScope()
    var sort by rememberSaveable { mutableStateOf(SortOrder.Default) }
    var descending by rememberSaveable { mutableStateOf(false) }
    var sortMenu by remember { mutableStateOf(false) }
    var jobs by remember { mutableStateOf(false) }
    var detail by rememberSaveable(stateSaver = detailSaver) { mutableStateOf<DetailTarget?>(null) }
    val scroll = TopAppBarDefaults.enterAlwaysScrollBehavior()
    fun open(target: DetailTarget) {
        if (wide) detail = target else when (target) {
            is DetailTarget.Album -> nav.navigate(Route.Album(target.id))
            is DetailTarget.Artist -> nav.navigate(Route.Artist(target.id))
            is DetailTarget.Playlist -> nav.navigate(Route.Playlist(target.id))
            is DetailTarget.Genre -> nav.navigate(Route.Genre(target.name))
        }
    }
    Scaffold(
        modifier = Modifier.nestedScroll(scroll.nestedScrollConnection),
        topBar = {
            Column {
                MediumFlexibleTopAppBar(
                    title = { Text(stringResource(R.string.nav_library)) },
                    actions = {
                        Box {
                            IconButton(onClick = { sortMenu = true }) { Icon(Icons.Filled.Sort, stringResource(R.string.action_sort)) }
                            DropdownMenu(expanded = sortMenu, onDismissRequest = { sortMenu = false }) {
                                val options = if (pager.currentPage == 3) listOf(SortOrder.Title, SortOrder.Artist, SortOrder.Album, SortOrder.Year, SortOrder.DateAdded, SortOrder.Rating, SortOrder.PlayCount, SortOrder.Duration, SortOrder.Bpm, SortOrder.Energy, SortOrder.Random)
                                else listOf(SortOrder.Default, SortOrder.Artist, SortOrder.Year, SortOrder.DateAdded, SortOrder.Rating, SortOrder.PlayCount, SortOrder.Random)
                                options.forEach { o ->
                                    DropdownMenuItem(text = { Text(sortLabel(o)) }, trailingIcon = { if (sort == o) Icon(Icons.Filled.Check, null) }, onClick = { sort = o; sortMenu = false })
                                }
                                DropdownMenuItem(text = { Text(stringResource(R.string.sort_descending)) }, trailingIcon = { if (descending) Icon(Icons.Filled.Check, null) }, onClick = { descending = !descending; sortMenu = false })
                            }
                        }
                        JobsIndicator(onClick = { jobs = true })
                    },
                    scrollBehavior = scroll,
                )
                PrimaryScrollableTabRow(selectedTabIndex = pager.currentPage, edgePadding = 8.dp) {
                    tabs.forEachIndexed { i, res ->
                        Tab(selected = pager.currentPage == i, onClick = { scope.launch { pager.animateScrollToPage(i) } }, text = { Text(stringResource(res)) })
                    }
                }
            }
        },
    ) { padding ->
        Row(Modifier.fillMaxSize().padding(top = padding.calculateTopPadding())) {
            Box(Modifier.weight(1f).fillMaxSize()) {
                HorizontalPager(state = pager, modifier = Modifier.fillMaxSize(), beyondViewportPageCount = 0) { page ->
                    when (page) {
                        0 -> AlbumsTab(serverId, sort, descending, ::open)
                        1 -> ArtistsTab(serverId, ::open)
                        2 -> PlaylistsTab(serverId, ::open)
                        3 -> SongsTab(serverId, sort, descending, nav)
                        else -> GenresTab(serverId, ::open)
                    }
                }
                SelectionToolbar(Modifier.align(Alignment.BottomCenter).padding(bottom = 88.dp), onSelectAll = {
                    scope.launch {
                        when (pager.currentPage) {
                            0 -> (client.query(Queries.albumCount(serverId)) as? QueryResult.Count)?.data?.let { client.selectAll(SelectionKind.Albums, it.toInt()) }
                            3 -> (client.query(Queries.trackCount(serverId)) as? QueryResult.Count)?.data?.let { client.selectAll(SelectionKind.Tracks, it.toInt()) }
                            else -> Unit
                        }
                    }
                })
            }
            if (wide) {
                Box(Modifier.weight(1.3f).fillMaxSize()) {
                    DetailPane(nav, detail)
                }
            }
        }
    }
    if (jobs) JobsSheet { jobs = false }
}

private val detailSaver = androidx.compose.runtime.saveable.Saver<DetailTarget?, List<String>>(
    save = { t -> when (t) { is DetailTarget.Album -> listOf("album", t.id); is DetailTarget.Artist -> listOf("artist", t.id); is DetailTarget.Playlist -> listOf("playlist", t.id); is DetailTarget.Genre -> listOf("genre", t.name); null -> emptyList() } },
    restore = { l -> when (l.getOrNull(0)) { "album" -> DetailTarget.Album(l[1]); "artist" -> DetailTarget.Artist(l[1]); "playlist" -> DetailTarget.Playlist(l[1]); "genre" -> DetailTarget.Genre(l[1]); else -> null } },
)

@Composable
private fun AlbumsTab(serverId: String, sort: SortOrder, descending: Boolean, open: (DetailTarget) -> Unit) {
    val client = LocalCoreClient.current
    val key = remember(serverId, sort, descending) { AlbumListKey(serverId, sort, descending) }
    val state by client.albumPages.state(key).collectAsStateWithLifecycle()
    val selection by client.selection.collectAsStateWithLifecycle()
    val kind by client.selectionKind.collectAsStateWithLifecycle()
    val selecting = selection.active && kind == SelectionKind.Albums
    var sheetFor by remember { mutableStateOf<app.hocket.core.api.Album?>(null) }
    LaunchedEffect(key, state.generation) { client.albumPages.ensure(key, 0) }
    LaunchedEffect(state.total) { if (state.total >= 0 && kind == SelectionKind.Albums) client.setSelectionTotal(state.total) }
    if (state.known && state.total == 0) { EmptyState(stringResource(R.string.empty_library_title), stringResource(R.string.empty_library_body)); return }
    val grid = rememberLazyGridState()
    LazyVerticalGrid(columns = GridCells.Adaptive(150.dp), state = grid, contentPadding = PaddingValues(start = 10.dp, end = 10.dp, top = 8.dp, bottom = BottomContentInset), modifier = Modifier.fillMaxSize()) {
        val count = if (state.known) state.total else 0
        items(count, key = { i -> state.item(i, client.albumPages.pageSize)?.id ?: "ph$i" }) { i ->
            val album = state.item(i, client.albumPages.pageSize)
            LaunchedEffect(i, state.generation) { client.albumPages.ensure(key, i) }
            if (album == null) {
                app.hocket.ui.components.ArtworkPlaceholder(null, Modifier.fillMaxWidth().padding(6.dp).aspectRatio(1f))
            } else {
                AlbumCard(album, onClick = { open(DetailTarget.Album(album.id)) }, selected = selecting && selection.contains(album.id), selectionActive = selecting,
                    onToggleSelect = { client.toggleSelected(SelectionKind.Albums, album.id) })
            }
        }
    }
    sheetFor?.let { a -> ActionSheet(Commands.albums(listOf(a.id)), a.name, a.artist, onDismiss = { sheetFor = null }) }
}

@Composable
private fun ArtistsTab(serverId: String, open: (DetailTarget) -> Unit) {
    val client = LocalCoreClient.current
    val key = remember(serverId) { ArtistListKey(serverId) }
    val state by client.artistPages.state(key).collectAsStateWithLifecycle()
    val selection by client.selection.collectAsStateWithLifecycle()
    val kind by client.selectionKind.collectAsStateWithLifecycle()
    val selecting = selection.active && kind == SelectionKind.Artists
    LaunchedEffect(key, state.generation) { client.artistPages.ensure(key, 0) }
    if (state.known && state.total == 0) { EmptyState(stringResource(R.string.empty_library_title), stringResource(R.string.empty_library_body)); return }
    LazyColumn(state = rememberLazyListState(), contentPadding = PaddingValues(bottom = BottomContentInset), modifier = Modifier.fillMaxSize()) {
        val count = if (state.known) state.total else 0
        items(count, key = { i -> state.item(i, client.artistPages.pageSize)?.id ?: "ph$i" }) { i ->
            val artist = state.item(i, client.artistPages.pageSize)
            LaunchedEffect(i, state.generation) { client.artistPages.ensure(key, i) }
            if (artist != null) ArtistRow(artist, onClick = { open(DetailTarget.Artist(artist.id)) }, selected = selecting && selection.contains(artist.id), selectionActive = selecting, onToggleSelect = { client.toggleSelected(SelectionKind.Artists, artist.id) })
            else Box(Modifier.fillMaxWidth().padding(vertical = 32.dp))
        }
    }
}

@Composable
private fun PlaylistsTab(serverId: String, open: (DetailTarget) -> Unit) {
    val client = LocalCoreClient.current
    val libraryGen by client.libraryChanged.collectAsStateWithLifecycle(initialValue = null)
    var playlists by remember { mutableStateOf<List<Playlist>?>(null) }
    LaunchedEffect(serverId, libraryGen) { playlists = (client.query(Queries.playlists(serverId)) as? QueryResult.Playlists)?.data }
    val selection by client.selection.collectAsStateWithLifecycle()
    val kind by client.selectionKind.collectAsStateWithLifecycle()
    val selecting = selection.active && kind == SelectionKind.Playlists
    val list = playlists ?: return
    if (list.isEmpty()) { EmptyState(stringResource(R.string.empty_playlists_title), stringResource(R.string.empty_playlists_body)); return }
    LazyColumn(contentPadding = PaddingValues(bottom = BottomContentInset), modifier = Modifier.fillMaxSize()) {
        items(list, key = { it.id }) { p ->
            PlaylistRow(p, onClick = { open(DetailTarget.Playlist(p.id)) }, selected = selecting && selection.contains(p.id), selectionActive = selecting, onToggleSelect = { client.toggleSelected(SelectionKind.Playlists, p.id) })
        }
    }
}

@Composable
private fun SongsTab(serverId: String, sort: SortOrder, descending: Boolean, nav: NavHostController) {
    val client = LocalCoreClient.current
    val effectiveSort = if (sort == SortOrder.Default) SortOrder.Title else sort
    val key = remember(serverId, effectiveSort, descending) { TrackListKey(serverId, effectiveSort, descending) }
    val state by client.trackPages.state(key).collectAsStateWithLifecycle()
    val selection by client.selection.collectAsStateWithLifecycle()
    val kind by client.selectionKind.collectAsStateWithLifecycle()
    val nowPlaying by client.nowPlaying.collectAsStateWithLifecycle()
    val selecting = selection.active && kind == SelectionKind.Tracks
    var sheetFor by remember { mutableStateOf<app.hocket.core.api.Track?>(null) }
    val allSongsLabel = stringResource(R.string.library_songs)
    LaunchedEffect(key, state.generation) { client.trackPages.ensure(key, 0) }
    LaunchedEffect(state.total) { if (state.total >= 0 && kind == SelectionKind.Tracks) client.setSelectionTotal(state.total) }
    if (state.known && state.total == 0) { EmptyState(stringResource(R.string.empty_library_title), stringResource(R.string.empty_library_body)); return }
    LazyColumn(state = rememberLazyListState(), contentPadding = PaddingValues(bottom = BottomContentInset), modifier = Modifier.fillMaxSize()) {
        val count = if (state.known) state.total else 0
        items(count, key = { i -> state.item(i, client.trackPages.pageSize)?.id ?: "ph$i" }) { i ->
            val track = state.item(i, client.trackPages.pageSize)
            LaunchedEffect(i, state.generation) { client.trackPages.ensure(key, i) }
            if (track != null) {
                TrackRow(track.toSummary(), onClick = {
                    // Play the sorted library as an ad-hoc context starting here: the visible page's ids are known, the rest resolve in the core.
                    val ctx = Commands.adHocContext(serverId, allSongsLabel, state.pages.toSortedMap().values.flatten().map { it.id }, effectiveSort)
                    client.dispatch(Commands.playContext(ctx, startIndex = state.pages.toSortedMap().values.flatten().indexOfFirst { it.id == track.id }.coerceAtLeast(0)))
                }, onMore = { sheetFor = track }, selected = selecting && selection.contains(track.id), selectionActive = selecting,
                    onToggleSelect = { client.toggleSelected(SelectionKind.Tracks, track.id) }, nowPlaying = nowPlaying?.track?.id == track.id)
            } else Box(Modifier.fillMaxWidth().padding(vertical = 32.dp))
        }
    }
    sheetFor?.let { t ->
        ActionSheet(Commands.tracks(listOf(t.id)), t.title, t.artist, onDismiss = { sheetFor = null },
            onGoToAlbum = t.albumId?.let { id -> { nav.navigate(Route.Album(id)) } }, onGoToArtist = t.artistId?.let { id -> { nav.navigate(Route.Artist(id)) } })
    }
}

@Composable
private fun GenresTab(serverId: String, open: (DetailTarget) -> Unit) {
    val client = LocalCoreClient.current
    val libraryGen by client.libraryChanged.collectAsStateWithLifecycle(initialValue = null)
    var genres by remember { mutableStateOf<List<Genre>?>(null) }
    LaunchedEffect(serverId, libraryGen) { genres = (client.query(Queries.genres(serverId)) as? QueryResult.Genres)?.data }
    val list = genres ?: return
    if (list.isEmpty()) { EmptyState(stringResource(R.string.empty_library_title), stringResource(R.string.empty_library_body)); return }
    LazyColumn(contentPadding = PaddingValues(bottom = BottomContentInset), modifier = Modifier.fillMaxSize()) {
        items(list, key = { it.name }) { g -> GenreRow(g, onClick = { open(DetailTarget.Genre(g.name)) }) }
    }
}
