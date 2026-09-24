package app.hocket.ui.screens.library

import app.hocket.ui.nav.ScrollToTopOnReselect
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.size
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
import androidx.compose.material.icons.outlined.OfflineBolt
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.MediumFlexibleTopAppBar
import androidx.compose.material3.PrimaryScrollableTabRow
import androidx.compose.material3.FilterChip
import androidx.compose.material3.FilterChipDefaults
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
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.selected
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.navigation.NavHostController
import app.hocket.R
import androidx.annotation.StringRes
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.rememberScrollState
import androidx.compose.material3.AssistChip
import androidx.compose.material3.AssistChipDefaults
import app.hocket.ui.nav.AccountButton
import app.hocket.ui.nav.NavItem
import app.hocket.ui.nav.icon
import app.hocket.ui.nav.label
import app.hocket.ui.nav.BottomContentInset
import app.hocket.core.Commands
import app.hocket.core.Queries
import app.hocket.core.api.Filter
import app.hocket.core.api.FilterField
import app.hocket.core.api.FilterNode
import app.hocket.core.api.FilterOp
import app.hocket.core.api.FilterRule
import app.hocket.core.api.FilterValue
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
import app.hocket.ui.components.AlbumCardSkeleton
import app.hocket.ui.components.ShimmerHost
import app.hocket.ui.components.TrackRowSkeleton
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.ui.unit.Dp
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
    var offlineOnly by rememberSaveable { mutableStateOf(false) }
    var sortMenu by remember { mutableStateOf(false) }
    var jobs by remember { mutableStateOf(false) }
    var detail by rememberSaveable(stateSaver = detailSaver) { mutableStateOf<DetailTarget?>(null) }
    val scroll = TopAppBarDefaults.enterAlwaysScrollBehavior()
    val onLabel = stringResource(R.string.settings_on)
    val offLabel = stringResource(R.string.settings_off)
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
                    title = { Text(stringResource(R.string.nav_library), modifier = Modifier.semantics { heading() }) },
                    actions = {
                        Box {
                            IconButton(onClick = { sortMenu = true }) { Icon(Icons.Filled.Sort, stringResource(R.string.action_sort)) }
                            DropdownMenu(expanded = sortMenu, onDismissRequest = { sortMenu = false }) {
                                val options = if (pager.currentPage == 3) listOf(SortOrder.Title, SortOrder.Artist, SortOrder.Album, SortOrder.Year, SortOrder.DateAdded, SortOrder.Rating, SortOrder.PlayCount, SortOrder.Duration, SortOrder.Bpm, SortOrder.Energy, SortOrder.Random)
                                else listOf(SortOrder.Default, SortOrder.Artist, SortOrder.Year, SortOrder.DateAdded, SortOrder.Rating, SortOrder.PlayCount, SortOrder.Random)
                                options.forEach { o ->
                                    // The check mark is drawn only; the chosen order is announced as "selected".
                                    DropdownMenuItem(text = { Text(sortLabel(o)) }, trailingIcon = { if (sort == o) Icon(Icons.Filled.Check, null) }, onClick = { sort = o; sortMenu = false },
                                        modifier = Modifier.semantics { selected = sort == o })
                                }
                                DropdownMenuItem(text = { Text(stringResource(R.string.sort_descending)) }, trailingIcon = { if (descending) Icon(Icons.Filled.Check, null) }, onClick = { descending = !descending; sortMenu = false },
                                    modifier = Modifier.semantics { stateDescription = if (descending) onLabel else offLabel })
                            }
                        }
                        JobsIndicator(onClick = { jobs = true })
                        AccountButton()
                    },
                    scrollBehavior = scroll,
                )
                // The places beyond the tabs, so each stays reachable when it is not in the bottom bar.
                LibraryLinks(nav)
                PrimaryScrollableTabRow(selectedTabIndex = pager.currentPage, edgePadding = 8.dp) {
                    tabs.forEachIndexed { i, res ->
                        Tab(selected = pager.currentPage == i, onClick = { scope.launch { pager.animateScrollToPage(i) } }, text = { Text(stringResource(res)) })
                    }
                }
            }
        },
    ) { padding ->
        // The lists scroll under the bar: its height goes in as content padding, not as a layout
        // offset, so collapsing it does not shift the page (design: lists).
        val top = padding.calculateTopPadding()
        Row(Modifier.fillMaxSize()) {
            Box(Modifier.weight(1f).fillMaxSize()) {
                ShimmerHost {
                    HorizontalPager(state = pager, modifier = Modifier.fillMaxSize(), beyondViewportPageCount = 0) { page ->
                        when (page) {
                            0 -> AlbumsTab(serverId, sort, descending, ::open, top)
                            1 -> ArtistsTab(serverId, ::open, top)
                            2 -> PlaylistsTab(serverId, ::open, top)
                            3 -> SongsTab(serverId, sort, descending, nav, offlineOnly, onOfflineOnlyChange = { offlineOnly = it }, topPadding = top)
                            else -> GenresTab(serverId, ::open, top)
                        }
                    }
                }
                SelectionToolbar(Modifier.align(Alignment.BottomCenter).padding(bottom = app.hocket.ui.nav.BottomOverlayInset), onSelectAll = {
                    scope.launch {
                        when (pager.currentPage) {
                            0 -> (client.query(Queries.albumCount(serverId)) as? QueryResult.Count)?.data?.let { client.selectAll(SelectionKind.Albums, it.toInt()) }
                            3 -> (client.query(Queries.trackCount(serverId, if (offlineOnly) availableOfflineFilter(client.filters.value).root else null)) as? QueryResult.Count)?.data?.let { client.selectAll(SelectionKind.Tracks, it.toInt()) }
                            else -> Unit
                        }
                    }
                })
            }
            if (wide) {
                Box(Modifier.weight(1.3f).fillMaxSize().padding(top = top)) {
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

/** How many skeleton rows a paged list shows before its total is known. */
private const val SKELETON_COUNT = 12

/**
 * Album grids: two columns on phones, 150 dp cells (as many as fit) on wider panes, like Navic.
 */
internal val AlbumGridCells: GridCells = object : GridCells {
    private val adaptive = GridCells.Adaptive(150.dp)
    override fun androidx.compose.ui.unit.Density.calculateCrossAxisCellSizes(availableSize: Int, spacing: Int): List<Int> {
        if (availableSize.toDp() >= 480.dp) return with(adaptive) { calculateCrossAxisCellSizes(availableSize, spacing) }
        val cell = (availableSize - spacing) / 2
        return listOf(cell, availableSize - spacing - cell)
    }
}

/** Grid padding: 10 dp at the edges plus each card's 6 dp inner padding is Navic's 16 dp margin; 12 dp between covers. */
internal fun albumGridPadding(top: Dp, bottom: Dp) = PaddingValues(start = 10.dp, end = 10.dp, top = top + 8.dp, bottom = bottom)

@Composable
internal fun AlbumsTab(serverId: String, sort: SortOrder, descending: Boolean, open: (DetailTarget) -> Unit, topPadding: Dp = 0.dp) {
    val client = LocalCoreClient.current
    val key = remember(serverId, sort, descending) { AlbumListKey(serverId, sort, descending) }
    val state by client.albumPages.state(key).collectAsStateWithLifecycle()
    val selection by client.selection.collectAsStateWithLifecycle()
    val kind by client.selectionKind.collectAsStateWithLifecycle()
    val selecting = selection.active && kind == SelectionKind.Albums
    var sheetFor by remember { mutableStateOf<app.hocket.core.api.Album?>(null) }
    LaunchedEffect(key, state.generation) { client.albumPages.ensure(key, 0) }
    LaunchedEffect(state.total) { if (state.total >= 0 && kind == SelectionKind.Albums) client.setSelectionTotal(state.total) }
    if (state.known && state.total == 0) { EmptyState(stringResource(R.string.empty_library_title), stringResource(R.string.empty_library_body), Modifier.padding(top = topPadding)); return }
    val grid = rememberLazyGridState()
    ScrollToTopOnReselect(grid)
    LazyVerticalGrid(columns = AlbumGridCells, state = grid, contentPadding = albumGridPadding(topPadding, BottomContentInset), modifier = Modifier.fillMaxSize()) {
        // Until the total is known, a screenful of skeletons; items are keyed by position so a
        // skeleton turns into its card in place (no remove and insert as pages arrive).
        val count = if (state.known) state.total else SKELETON_COUNT
        items(count, contentType = { i -> if (state.item(i, client.albumPages.pageSize) == null) "skeleton" else "album" }) { i ->
            val album = state.item(i, client.albumPages.pageSize)
            if (state.known) LaunchedEffect(i, state.generation) { client.albumPages.ensure(key, i) }
            if (album == null) {
                AlbumCardSkeleton()
            } else {
                AlbumCard(album, onClick = { open(DetailTarget.Album(album.id)) }, modifier = Modifier.testTag("library.album"), selected = selecting && selection.contains(album.id), selectionActive = selecting,
                    onToggleSelect = { client.toggleSelected(SelectionKind.Albums, album.id) })
            }
        }
    }
    sheetFor?.let { a -> ActionSheet(Commands.albums(listOf(a.id)), a.name, a.artist, onDismiss = { sheetFor = null }) }
}

@Composable
internal fun ArtistsTab(serverId: String, open: (DetailTarget) -> Unit, topPadding: Dp = 0.dp) {
    val client = LocalCoreClient.current
    val key = remember(serverId) { ArtistListKey(serverId) }
    val state by client.artistPages.state(key).collectAsStateWithLifecycle()
    val selection by client.selection.collectAsStateWithLifecycle()
    val kind by client.selectionKind.collectAsStateWithLifecycle()
    val selecting = selection.active && kind == SelectionKind.Artists
    LaunchedEffect(key, state.generation) { client.artistPages.ensure(key, 0) }
    if (state.known && state.total == 0) { EmptyState(stringResource(R.string.empty_library_title), stringResource(R.string.empty_library_body), Modifier.padding(top = topPadding)); return }
    val listState = rememberLazyListState()
    ScrollToTopOnReselect(listState)
    LazyColumn(state = listState, contentPadding = PaddingValues(top = topPadding, bottom = BottomContentInset), modifier = Modifier.fillMaxSize()) {
        val count = if (state.known) state.total else SKELETON_COUNT
        items(count, contentType = { i -> if (state.item(i, client.artistPages.pageSize) == null) "skeleton" else "artist" }) { i ->
            val artist = state.item(i, client.artistPages.pageSize)
            if (state.known) LaunchedEffect(i, state.generation) { client.artistPages.ensure(key, i) }
            if (artist != null) ArtistRow(artist, onClick = { open(DetailTarget.Artist(artist.id)) }, selected = selecting && selection.contains(artist.id), selectionActive = selecting, onToggleSelect = { client.toggleSelected(SelectionKind.Artists, artist.id) })
            else TrackRowSkeleton(artShape = CircleShape)
        }
    }
}

@Composable
internal fun PlaylistsTab(serverId: String, open: (DetailTarget) -> Unit, topPadding: Dp = 0.dp) {
    val client = LocalCoreClient.current
    val libraryGen by client.libraryChanged.collectAsStateWithLifecycle(initialValue = null)
    var playlists by remember { mutableStateOf<List<Playlist>?>(null) }
    LaunchedEffect(serverId, libraryGen) { playlists = (client.query(Queries.playlists(serverId)) as? QueryResult.Playlists)?.data ?: playlists ?: emptyList() }
    val selection by client.selection.collectAsStateWithLifecycle()
    val kind by client.selectionKind.collectAsStateWithLifecycle()
    val selecting = selection.active && kind == SelectionKind.Playlists
    val list = playlists
    if (list != null && list.isEmpty()) { EmptyState(stringResource(R.string.empty_playlists_title), stringResource(R.string.empty_playlists_body), Modifier.padding(top = topPadding)); return }
    val listState = rememberLazyListState()
    ScrollToTopOnReselect(listState)
    LazyColumn(state = listState, contentPadding = PaddingValues(top = topPadding, bottom = BottomContentInset), modifier = Modifier.fillMaxSize()) {
        if (list == null) items(SKELETON_COUNT, contentType = { "skeleton" }) { TrackRowSkeleton() }
        else items(list, key = { it.id }, contentType = { "playlist" }) { p ->
            PlaylistRow(p, onClick = { open(DetailTarget.Playlist(p.id)) }, modifier = Modifier.animateItem(), selected = selecting && selection.contains(p.id), selectionActive = selecting, onToggleSelect = { client.toggleSelected(SelectionKind.Playlists, p.id) })
        }
    }
}

/** The rule of the core's built-in "Available offline" filter (downloaded, or complete in the stream cache). */
fun availableOfflineFilter(filters: List<Filter>): Filter = filters.firstOrNull { it.id == BUILTIN_AVAILABLE_OFFLINE }
    ?: Filter(BUILTIN_AVAILABLE_OFFLINE, "Available offline", FilterNode.All(listOf(FilterNode.Rule(FilterRule(FilterField.AvailableOffline, FilterOp.IsTrue, FilterValue.Bool(true))))), SortOrder.Artist, false, null)

const val BUILTIN_AVAILABLE_OFFLINE = "builtin:available-offline"

/**
 * The songs list over the track page cache. [offlineOnly] narrows it to the built-in "Available
 * offline" filter; with [onOfflineOnlyChange] the list starts with the chip that toggles it.
 */
@Composable
internal fun SongsTab(serverId: String, sort: SortOrder, descending: Boolean, nav: NavHostController, offlineOnly: Boolean = false, onOfflineOnlyChange: ((Boolean) -> Unit)? = null, topPadding: Dp = 0.dp) {
    val client = LocalCoreClient.current
    val filters by client.filters.collectAsStateWithLifecycle()
    val effectiveSort = if (sort == SortOrder.Default) SortOrder.Title else sort
    val offlineFilter = availableOfflineFilter(filters)
    val filterRoot = if (offlineOnly) offlineFilter.root else null
    val key = remember(serverId, effectiveSort, descending, filterRoot) { TrackListKey(serverId, effectiveSort, descending, filterRoot) }
    val state by client.trackPages.state(key).collectAsStateWithLifecycle()
    val selection by client.selection.collectAsStateWithLifecycle()
    val kind by client.selectionKind.collectAsStateWithLifecycle()
    val nowPlaying by client.nowPlaying.collectAsStateWithLifecycle()
    val selecting = selection.active && kind == SelectionKind.Tracks
    var sheetFor by remember { mutableStateOf<app.hocket.core.api.Track?>(null) }
    val listLabel = stringResource(if (offlineOnly) R.string.available_offline else R.string.library_songs)
    LaunchedEffect(key, state.generation) { client.trackPages.ensure(key, 0) }
    LaunchedEffect(state.total) { if (state.total >= 0 && kind == SelectionKind.Tracks) client.setSelectionTotal(state.total) }
    val empty = state.known && state.total == 0
    if (empty && onOfflineOnlyChange == null) {
        if (offlineOnly) EmptyState(stringResource(R.string.available_offline_empty_title), stringResource(R.string.available_offline_empty_body), Modifier.padding(top = topPadding))
        else EmptyState(stringResource(R.string.empty_library_title), stringResource(R.string.empty_library_body), Modifier.padding(top = topPadding))
        return
    }
    val listState = rememberLazyListState()
    ScrollToTopOnReselect(listState)
    LazyColumn(state = listState, contentPadding = PaddingValues(top = topPadding, bottom = BottomContentInset), modifier = Modifier.fillMaxSize().testTag("library.songs")) {
        if (onOfflineOnlyChange != null) item(key = "chips") {
            Row(Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 4.dp)) {
                FilterChip(
                    selected = offlineOnly, onClick = { onOfflineOnlyChange(!offlineOnly) },
                    label = { Text(stringResource(R.string.available_offline)) },
                    leadingIcon = { Icon(if (offlineOnly) Icons.Filled.Check else Icons.Outlined.OfflineBolt, null, Modifier.size(FilterChipDefaults.IconSize)) },
                    modifier = Modifier.testTag("library.availableOffline"),
                )
            }
        }
        if (empty) item(key = "empty") {
            if (offlineOnly) EmptyState(stringResource(R.string.available_offline_empty_title), stringResource(R.string.available_offline_empty_body))
            else EmptyState(stringResource(R.string.empty_library_title), stringResource(R.string.empty_library_body))
        }
        val count = if (state.known) state.total else SKELETON_COUNT
        items(count, contentType = { i -> if (state.item(i, client.trackPages.pageSize) == null) "skeleton" else "track" }) { i ->
            val track = state.item(i, client.trackPages.pageSize)
            if (state.known) LaunchedEffect(i, state.generation) { client.trackPages.ensure(key, i) }
            if (track != null) {
                TrackRow(track.toSummary(), onClick = {
                    // Play the sorted list as an ad-hoc context starting here: the visible page's ids are known, the rest resolve in the core.
                    val ctx = Commands.adHocContext(serverId, listLabel, state.pages.toSortedMap().values.flatten().map { it.id }, effectiveSort)
                    client.dispatch(Commands.playContext(ctx, startIndex = state.pages.toSortedMap().values.flatten().indexOfFirst { it.id == track.id }.coerceAtLeast(0)))
                }, onMore = { sheetFor = track }, selected = selecting && selection.contains(track.id), selectionActive = selecting,
                    onToggleSelect = { client.toggleSelected(SelectionKind.Tracks, track.id) }, nowPlaying = nowPlaying?.track?.id == track.id)
            } else TrackRowSkeleton()
        }
    }
    sheetFor?.let { t ->
        ActionSheet(Commands.tracks(listOf(t.id)), t.title, t.artist, onDismiss = { sheetFor = null },
            onGoToAlbum = t.albumId?.let { id -> { nav.navigate(Route.Album(id)) } }, onGoToArtist = t.artistId?.let { id -> { nav.navigate(Route.Artist(id)) } })
    }
}

@Composable
internal fun GenresTab(serverId: String, open: (DetailTarget) -> Unit, topPadding: Dp = 0.dp) {
    val client = LocalCoreClient.current
    val libraryGen by client.libraryChanged.collectAsStateWithLifecycle(initialValue = null)
    var genres by remember { mutableStateOf<List<Genre>?>(null) }
    LaunchedEffect(serverId, libraryGen) { genres = (client.query(Queries.genres(serverId)) as? QueryResult.Genres)?.data ?: genres ?: emptyList() }
    val list = genres
    if (list != null && list.isEmpty()) { EmptyState(stringResource(R.string.empty_library_title), stringResource(R.string.empty_library_body), Modifier.padding(top = topPadding)); return }
    val listState = rememberLazyListState()
    ScrollToTopOnReselect(listState)
    LazyColumn(state = listState, contentPadding = PaddingValues(top = topPadding, bottom = BottomContentInset), modifier = Modifier.fillMaxSize()) {
        if (list == null) items(SKELETON_COUNT, contentType = { "skeleton" }) { TrackRowSkeleton(showArtwork = false) }
        else items(list, key = { it.name }, contentType = { "genre" }) { g -> GenreRow(g, onClick = { open(DetailTarget.Genre(g.name)) }, modifier = Modifier.animateItem()) }
    }
}

/** The library's lists as bottom-bar places of their own. */
enum class LibraryList(@StringRes val title: Int) {
    Albums(R.string.library_albums), Artists(R.string.library_artists), Playlists(R.string.library_playlists), Songs(R.string.library_songs), Genres(R.string.library_genres)
}

/** One library list on its own screen (a bottom-bar place): its top app bar with the account button, then the list. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun LibraryListScreen(nav: NavHostController, list: LibraryList) {
    val client = LocalCoreClient.current
    val server by client.server.collectAsStateWithLifecycle()
    val serverId = server?.id ?: return
    val scroll = TopAppBarDefaults.enterAlwaysScrollBehavior()
    fun open(target: DetailTarget) = when (target) {
        is DetailTarget.Album -> nav.navigate(Route.Album(target.id))
        is DetailTarget.Artist -> nav.navigate(Route.Artist(target.id))
        is DetailTarget.Playlist -> nav.navigate(Route.Playlist(target.id))
        is DetailTarget.Genre -> nav.navigate(Route.Genre(target.name))
    }
    Scaffold(
        modifier = Modifier.nestedScroll(scroll.nestedScrollConnection).testTag("libraryList.${list.name.lowercase()}"),
        topBar = {
            MediumFlexibleTopAppBar(
                title = { Text(stringResource(list.title), modifier = Modifier.semantics { heading() }) },
                actions = { AccountButton() },
                scrollBehavior = scroll,
            )
        },
    ) { padding ->
        val top = padding.calculateTopPadding()
        Box(Modifier.fillMaxSize()) {
            ShimmerHost {
                when (list) {
                    LibraryList.Albums -> AlbumsTab(serverId, SortOrder.Default, false, ::open, top)
                    LibraryList.Artists -> ArtistsTab(serverId, ::open, top)
                    LibraryList.Playlists -> PlaylistsTab(serverId, ::open, top)
                    LibraryList.Songs -> {
                        var offlineOnly by rememberSaveable { mutableStateOf(false) }
                        SongsTab(serverId, SortOrder.Default, false, nav, offlineOnly, onOfflineOnlyChange = { offlineOnly = it }, topPadding = top)
                    }
                    LibraryList.Genres -> GenresTab(serverId, ::open, top)
                }
            }
            SelectionToolbar(Modifier.align(Alignment.BottomCenter).padding(bottom = app.hocket.ui.nav.BottomOverlayInset))
        }
    }
}

/** Links from the library to the places its tabs do not cover: recent queues, downloads, filters. */
@Composable
private fun LibraryLinks(nav: NavHostController) {
    Row(
        Modifier.fillMaxWidth().horizontalScroll(rememberScrollState()).padding(horizontal = 12.dp).testTag("library.links"),
        horizontalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        listOf(NavItem.RecentQueues to Route.SavedQueues, NavItem.Downloads to Route.Downloads, NavItem.Filters to Route.Filters).forEach { (item, route) ->
            AssistChip(
                onClick = { nav.navigate(route) { launchSingleTop = true } },
                label = { Text(item.label()) },
                leadingIcon = { Icon(item.icon(false), null, Modifier.size(AssistChipDefaults.IconSize)) },
                modifier = Modifier.testTag("library.link.${item.id}"),
            )
        }
    }
}
