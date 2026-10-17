package app.hocket.ui.screens.library

import app.hocket.ui.icons.HocketIcons
import app.hocket.ui.nav.ScrollToTopOnReselect
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.grid.GridCells
import androidx.compose.foundation.lazy.grid.LazyVerticalGrid
import androidx.compose.foundation.lazy.grid.rememberLazyGridState
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MediumFlexibleTopAppBar
import androidx.compose.material3.FilterChip
import androidx.compose.material3.FilterChipDefaults
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TopAppBarDefaults
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.runtime.getValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.input.nestedscroll.nestedScroll
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.navigation.NavHostController
import app.hocket.R
import androidx.annotation.StringRes
import androidx.compose.foundation.layout.Arrangement
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
import app.hocket.ui.components.AlbumCard
import app.hocket.ui.components.EmptyState
import app.hocket.ui.components.GenreRow
import app.hocket.ui.components.PlaylistRow
import app.hocket.ui.components.SelectionToolbar
import app.hocket.ui.components.TrackRow
import app.hocket.ui.components.AlbumCardSkeleton
import app.hocket.ui.components.ShimmerHost
import app.hocket.ui.components.TrackRowSkeleton
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.ui.unit.Dp
import app.hocket.ui.nav.Route
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.ExperimentalMaterial3ExpressiveApi
import androidx.compose.material3.MaterialShapes
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.toShape
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.foundation.lazy.LazyListScope
import androidx.compose.foundation.lazy.grid.GridItemSpan
import androidx.compose.foundation.lazy.grid.LazyGridItemSpanScope
import androidx.compose.foundation.lazy.grid.LazyGridScope
import androidx.compose.runtime.produceState
import app.hocket.core.api.Album
import app.hocket.core.api.Artist
import app.hocket.core.api.Track
import app.hocket.core.api.TrackSummary
import app.hocket.core.api.OfflineState
import app.hocket.ui.components.ArtistCard
import app.hocket.ui.components.ArtistCardSkeleton

@Composable
fun sortLabel(sort: SortOrder): String = when (sort) {
    SortOrder.Default -> stringResource(R.string.sort_default); SortOrder.Title -> stringResource(R.string.sort_title); SortOrder.Artist -> stringResource(R.string.sort_artist)
    SortOrder.Album -> stringResource(R.string.sort_album); SortOrder.Year -> stringResource(R.string.sort_year); SortOrder.DateAdded -> stringResource(R.string.sort_date_added)
    SortOrder.Rating -> stringResource(R.string.sort_rating); SortOrder.PlayCount -> stringResource(R.string.sort_play_count); SortOrder.Duration -> stringResource(R.string.sort_duration)
    SortOrder.Random -> stringResource(R.string.sort_random); SortOrder.Bpm -> stringResource(R.string.sort_bpm); SortOrder.Energy -> stringResource(R.string.sort_energy)
    SortOrder.AlbumCount -> stringResource(R.string.sort_album_count); SortOrder.SongCount -> stringResource(R.string.sort_song_count)
}

/** What a wide layout shows in its detail pane. */
sealed interface DetailTarget {
    data class Album(val id: String) : DetailTarget
    data class Artist(val id: String) : DetailTarget
    data class Playlist(val id: String) : DetailTarget
    data class Genre(val name: String) : DetailTarget
}

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

/** The grid's edge padding: with each card's 6 dp inner padding, Navic's 16 dp margin. */
private val GRID_EDGE = 10.dp

/** Grid padding: 10 dp at the edges plus each card's 6 dp inner padding is Navic's 16 dp margin; 12 dp between covers. */
internal fun albumGridPadding(top: Dp, bottom: Dp) = PaddingValues(start = GRID_EDGE, end = GRID_EDGE, top = top + 8.dp, bottom = bottom)

/** A card in a grid list's Pinned or Recent row (album covers a little smaller than the grid's, artists smaller still). */
private val ALBUM_ROW_CELL = 128.dp
private val ARTIST_ROW_CELL = 112.dp

private val fullSpan: LazyGridItemSpanScope.() -> GridItemSpan = { GridItemSpan(maxLineSpan) }

/**
 * A grid list's top: Pinned and Recent as rows of cards (each hidden when empty), then the full
 * list's header with its sort chip. Grid headers sit 6 dp in, to line up with the cards' art.
 */
private fun <T> LazyGridScope.gridGroups(groups: ListGroups<T>, allTitle: Int, control: SortControl, key: (T) -> String, cellWidth: Dp, card: @Composable (T, Modifier) -> Unit) {
    if (groups.pinned.isNotEmpty()) {
        item(key = "pinned.h", span = fullSpan, contentType = "header") { ListHeader(stringResource(R.string.library_group_pinned), Modifier.testTag("library.group.pinned"), start = 6.dp) }
        item(key = "pinned", span = fullSpan, contentType = "row") { CardRow(groups.pinned, key, cellWidth, GRID_EDGE, card = card) }
    }
    if (groups.recent.isNotEmpty()) {
        item(key = "recent.h", span = fullSpan, contentType = "header") { ListHeader(stringResource(R.string.library_group_recent), Modifier.testTag("library.group.recent"), start = 6.dp) }
        item(key = "recent", span = fullSpan, contentType = "row") { CardRow(groups.recent, key, cellWidth, GRID_EDGE, card = card) }
    }
    item(key = "all.h", span = fullSpan, contentType = "header") {
        ListHeader(stringResource(allTitle), Modifier.testTag("library.group.all"), start = 6.dp) { SortChip(control.sort, control.options, control.onSort) }
    }
}

/**
 * A row list's top: Pinned and Recent as rows (each hidden when empty, at most a few), then the full
 * list's header with its sort chip. Group rows are keyed apart from the full list's, which may hold
 * the same items.
 */
private fun <T> LazyListScope.listGroups(groups: ListGroups<T>, allTitle: Int, control: SortControl, key: (T) -> String, row: @Composable (T, List<T>) -> Unit) {
    if (groups.pinned.isNotEmpty()) {
        item(key = "pinned.h", contentType = "header") { ListHeader(stringResource(R.string.library_group_pinned), Modifier.testTag("library.group.pinned")) }
        items(groups.pinned, key = { "pinned." + key(it) }) { row(it, groups.pinned) }
    }
    if (groups.recent.isNotEmpty()) {
        item(key = "recent.h", contentType = "header") { ListHeader(stringResource(R.string.library_group_recent), Modifier.testTag("library.group.recent")) }
        items(groups.recent, key = { "recent." + key(it) }) { row(it, groups.recent) }
    }
    item(key = "all.h", contentType = "header") {
        ListHeader(stringResource(allTitle), Modifier.testTag("library.group.all")) { SortChip(control.sort, control.options, control.onSort) }
    }
}

@Composable
internal fun AlbumsTab(serverId: String, control: SortControl, open: (DetailTarget) -> Unit, topPadding: Dp = 0.dp) {
    val client = LocalCoreClient.current
    val sort = control.sort
    val key = remember(serverId, sort) { AlbumListKey(serverId, sort.order, sort.coreDescending) }
    val state by client.albumPages.state(key).collectAsStateWithLifecycle()
    val selection by client.selection.collectAsStateWithLifecycle()
    val kind by client.selectionKind.collectAsStateWithLifecycle()
    val selecting = selection.active && kind == SelectionKind.Albums
    val groups = rememberGroups(PinKind.Album, { history, pinned -> recentAlbumIds(history, RECENT_CARDS, pinned) }) { id -> (client.query(Queries.album(id)) as? QueryResult.AlbumDetail)?.data }
    LaunchedEffect(key, state.generation) { client.albumPages.ensure(key, 0) }
    LaunchedEffect(state.total) { if (state.total >= 0 && kind == SelectionKind.Albums) client.setSelectionTotal(state.total) }
    if (state.known && state.total == 0) { EmptyState(stringResource(R.string.empty_library_title), stringResource(R.string.empty_library_body), Modifier.padding(top = topPadding)); return }
    val grid = rememberLazyGridState()
    ScrollToTopOnReselect(grid)
    KeepTopForGroups(groups, { grid.firstVisibleItemIndex == 0 && grid.firstVisibleItemScrollOffset == 0 }) { grid.requestScrollToItem(0) }
    @Composable fun Card(album: Album, modifier: Modifier, titleLines: Int = 2) = AlbumCard(album, onClick = { open(DetailTarget.Album(album.id)) }, modifier = modifier.testTag("library.album"),
        selected = selecting && selection.contains(album.id), selectionActive = selecting, onToggleSelect = { client.toggleSelected(SelectionKind.Albums, album.id) }, titleLines = titleLines)
    LazyVerticalGrid(columns = AlbumGridCells, state = grid, contentPadding = albumGridPadding(topPadding, BottomContentInset), modifier = Modifier.fillMaxSize()) {
        gridGroups(groups, R.string.library_all_albums, control, { it.id }, ALBUM_ROW_CELL) { album, modifier -> Card(album, modifier, titleLines = 1) }
        // Until the total is known, a screenful of skeletons; items are keyed by position so a
        // skeleton turns into its card in place (no remove and insert as pages arrive).
        val count = if (state.known) state.total else SKELETON_COUNT
        items(count, contentType = { i -> if (state.item(i, client.albumPages.pageSize) == null) "skeleton" else "album" }) { i ->
            val album = state.item(i, client.albumPages.pageSize)
            if (state.known) LaunchedEffect(i, state.generation) { client.albumPages.ensure(key, i) }
            if (album == null) AlbumCardSkeleton() else Card(album, Modifier)
        }
    }
}

/** Artists as a grid of round images, in the core's order (name, album or song count, plays, date added, shuffle). */
@Composable
internal fun ArtistsTab(serverId: String, control: SortControl, open: (DetailTarget) -> Unit, topPadding: Dp = 0.dp) {
    val client = LocalCoreClient.current
    val sort = control.sort
    val key = remember(serverId, sort) { ArtistListKey(serverId, sort.order, sort.coreDescending) }
    val state by client.artistPages.state(key).collectAsStateWithLifecycle()
    val selection by client.selection.collectAsStateWithLifecycle()
    val kind by client.selectionKind.collectAsStateWithLifecycle()
    val selecting = selection.active && kind == SelectionKind.Artists
    val groups = rememberGroups(PinKind.Artist, { history, pinned -> recentArtistIds(history, RECENT_CARDS, pinned) }) { id -> (client.query(Queries.artist(id)) as? QueryResult.ArtistDetail)?.data }
    LaunchedEffect(key, state.generation) { client.artistPages.ensure(key, 0) }
    if (state.known && state.total == 0) { EmptyState(stringResource(R.string.empty_library_title), stringResource(R.string.empty_library_body), Modifier.padding(top = topPadding)); return }
    val grid = rememberLazyGridState()
    ScrollToTopOnReselect(grid)
    KeepTopForGroups(groups, { grid.firstVisibleItemIndex == 0 && grid.firstVisibleItemScrollOffset == 0 }) { grid.requestScrollToItem(0) }
    @Composable fun Card(artist: Artist, modifier: Modifier) = ArtistCard(artist, onClick = { open(DetailTarget.Artist(artist.id)) }, modifier = modifier.testTag("library.artist"),
        selected = selecting && selection.contains(artist.id), selectionActive = selecting, onToggleSelect = { client.toggleSelected(SelectionKind.Artists, artist.id) })
    LazyVerticalGrid(columns = AlbumGridCells, state = grid, contentPadding = albumGridPadding(topPadding, BottomContentInset), modifier = Modifier.fillMaxSize().testTag("library.artists")) {
        gridGroups(groups, R.string.library_all_artists, control, { it.id }, ARTIST_ROW_CELL) { artist, modifier -> Card(artist, modifier) }
        val count = if (state.known) state.total else SKELETON_COUNT
        items(count, contentType = { i -> if (state.item(i, client.artistPages.pageSize) == null) "skeleton" else "artist" }) { i ->
            val artist = state.item(i, client.artistPages.pageSize)
            if (state.known) LaunchedEffect(i, state.generation) { client.artistPages.ensure(key, i) }
            if (artist == null) ArtistCardSkeleton() else Card(artist, Modifier)
        }
    }
}

/** Playlists (one whole list, sorted here), the pinned ones first, then the ones changed last. */
@Composable
internal fun PlaylistsTab(serverId: String, control: SortControl, open: (DetailTarget) -> Unit, topPadding: Dp = 0.dp) {
    val client = LocalCoreClient.current
    val libraryGen by client.libraryGeneration.collectAsStateWithLifecycle()
    var playlists by remember { mutableStateOf<List<Playlist>?>(null) }
    LaunchedEffect(serverId, libraryGen) { playlists = (client.query(Queries.playlists(serverId)) as? QueryResult.Playlists)?.data ?: playlists ?: emptyList() }
    val selection by client.selection.collectAsStateWithLifecycle()
    val kind by client.selectionKind.collectAsStateWithLifecycle()
    val selecting = selection.active && kind == SelectionKind.Playlists
    val list = playlists
    val pinnedIds = rememberPinnedIds(PinKind.Playlist)
    val sorted = remember(list, control.sort) { list?.let { sortPlaylists(it, control.sort) } }
    val groups = remember(list, pinnedIds) {
        val byId = list.orEmpty().associateBy { it.id }
        ListGroups(pinnedIds.take(PINNED_LIMIT).mapNotNull { byId[it] }, recentPlaylists(list.orEmpty(), RECENT_ROWS, pinnedIds))
    }
    if (list != null && list.isEmpty()) { EmptyState(stringResource(R.string.empty_playlists_title), stringResource(R.string.empty_playlists_body), Modifier.padding(top = topPadding)); return }
    val listState = rememberLazyListState()
    ScrollToTopOnReselect(listState)
    KeepTopForGroups(groups, { listState.firstVisibleItemIndex == 0 && listState.firstVisibleItemScrollOffset == 0 }) { listState.requestScrollToItem(0) }
    @Composable fun Item(p: Playlist, modifier: Modifier) = PlaylistRow(p, onClick = { open(DetailTarget.Playlist(p.id)) }, modifier = modifier, selected = selecting && selection.contains(p.id),
        selectionActive = selecting, onToggleSelect = { client.toggleSelected(SelectionKind.Playlists, p.id) })
    LazyColumn(state = listState, contentPadding = PaddingValues(top = topPadding, bottom = BottomContentInset), modifier = Modifier.fillMaxSize().testTag("library.playlists")) {
        listGroups(groups, R.string.library_all_playlists, control, { it.id }) { p, _ -> Item(p, Modifier) }
        if (sorted == null) items(SKELETON_COUNT, contentType = { "skeleton" }) { TrackRowSkeleton() }
        else items(sorted, key = { it.id }, contentType = { "playlist" }) { p -> Item(p, Modifier.animateItem().testTag("library.playlist")) }
    }
}

/** The rule of the core's built-in "Available offline" filter (downloaded, or complete in the stream cache). */
fun availableOfflineFilter(filters: List<Filter>): Filter = filters.firstOrNull { it.id == BUILTIN_AVAILABLE_OFFLINE }
    ?: Filter(BUILTIN_AVAILABLE_OFFLINE, "Available offline", FilterNode.All(listOf(FilterNode.Rule(FilterRule(FilterField.AvailableOffline, FilterOp.IsTrue, FilterValue.Bool(true))))), SortOrder.Artist, false, null)

const val BUILTIN_AVAILABLE_OFFLINE = "builtin:available-offline"

/**
 * The songs list over the track page cache. [offlineOnly] narrows it to the built-in "Available
 * offline" filter; with [onOfflineOnlyChange] the list starts with the chip that toggles it. With a
 * [control] (the Songs list) it opens with the pinned and recently played songs and its sort chip;
 * without one (Available offline) it is the bare list in [sort] order.
 */
@Composable
internal fun SongsTab(serverId: String, sort: SortOrder, descending: Boolean, nav: NavHostController, offlineOnly: Boolean = false, onOfflineOnlyChange: ((Boolean) -> Unit)? = null, topPadding: Dp = 0.dp, control: SortControl? = null) {
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
    val songMenu = app.hocket.ui.components.rememberSongMenu()
    val listLabel = stringResource(if (offlineOnly) R.string.available_offline else R.string.library_songs)
    val groups = if (control == null) ListGroups(emptyList(), emptyList()) else {
        val pinnedIds = rememberPinnedIds(PinKind.Track)
        val history = rememberRecentHistory()
        val generation = rememberSettledGeneration()
        val pinned by produceState(emptyList<TrackSummary>(), pinnedIds, generation) {
            if (generation < 0) return@produceState
            // One batch lookup, back in pin order.
            val ids = pinnedIds.take(PINNED_LIMIT)
            val byId = (client.query(Queries.tracksByIds(ids)) as? QueryResult.TrackList)?.data.orEmpty().associateBy { it.id }
            value = ids.mapNotNull { byId[it]?.toSummary() }
        }
        // With the Available offline chip on, the groups too keep only what plays offline.
        fun List<TrackSummary>.shown() = if (offlineOnly) filter { it.offline == OfflineState.Downloaded || it.offline == OfflineState.Cached } else this
        ListGroups(pinned.shown(), remember(history, pinnedIds) { recentTracks(history.orEmpty(), RECENT_ROWS, pinnedIds) }.shown())
    }
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
    KeepTopForGroups(groups, { listState.firstVisibleItemIndex == 0 && listState.firstVisibleItemScrollOffset == 0 }) { listState.requestScrollToItem(0) }
    val recentLabel = stringResource(R.string.library_group_recent)
    val pinnedLabel = stringResource(R.string.library_group_pinned)
    LazyColumn(state = listState, contentPadding = PaddingValues(top = topPadding, bottom = BottomContentInset), modifier = Modifier.fillMaxSize().testTag("library.songs")) {
        if (control != null) listGroups(groups, R.string.library_all_songs, control, { it.id }) { track, group ->
            // A pinned or recent song plays its group from there.
            TrackRow(track, onClick = {
                client.dispatch(Commands.playTracks(serverId, group.map { it.id }, group.indexOf(track).coerceAtLeast(0), if (group === groups.pinned) pinnedLabel else recentLabel))
            }, onMore = { songMenu.open(track) }, selected = selecting && selection.contains(track.id), selectionActive = selecting,
                onToggleSelect = { client.toggleSelected(SelectionKind.Tracks, track.id) }, nowPlaying = nowPlaying?.track?.id == track.id)
        }
        if (onOfflineOnlyChange != null) item(key = "chips") {
            Row(Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 4.dp)) {
                FilterChip(
                    selected = offlineOnly, onClick = { onOfflineOnlyChange(!offlineOnly) },
                    label = { Text(stringResource(R.string.available_offline)) },
                    leadingIcon = { Icon(if (offlineOnly) HocketIcons.Filled.Check else HocketIcons.Outlined.OfflineBolt, null, Modifier.size(FilterChipDefaults.IconSize)) },
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
                }, onMore = { songMenu.open(track.toSummary()) }, selected = selecting && selection.contains(track.id), selectionActive = selecting,
                    onToggleSelect = { client.toggleSelected(SelectionKind.Tracks, track.id) }, nowPlaying = nowPlaying?.track?.id == track.id)
            } else TrackRowSkeleton()
        }
    }
}

/** Genres (one whole list, sorted here), the pinned ones first, then those of the songs played last. */
@Composable
internal fun GenresTab(serverId: String, control: SortControl, open: (DetailTarget) -> Unit, topPadding: Dp = 0.dp) {
    val client = LocalCoreClient.current
    val libraryGen by client.libraryGeneration.collectAsStateWithLifecycle()
    var genres by remember { mutableStateOf<List<Genre>?>(null) }
    LaunchedEffect(serverId, libraryGen) { genres = (client.query(Queries.genres(serverId)) as? QueryResult.Genres)?.data ?: genres ?: emptyList() }
    val list = genres
    val pinnedIds = rememberPinnedIds(PinKind.Genre)
    val history = rememberRecentHistory()
    // The history's songs carry no genre: the most recent ones are looked up whole.
    val played by produceState(emptyList<Track>(), history) {
        if (history == null) return@produceState
        val ids = recentTracks(history.orEmpty(), RECENT_GENRE_SONGS).map { it.id }
        val byId = (client.query(Queries.tracksByIds(ids)) as? QueryResult.TrackList)?.data.orEmpty().associateBy { it.id }
        value = ids.mapNotNull { byId[it] }
    }
    val sorted = remember(list, control.sort) { list?.let { sortGenres(it, control.sort) } }
    val groups = remember(list, pinnedIds, played) {
        val byName = list.orEmpty().associateBy { it.name }
        ListGroups(pinnedIds.take(PINNED_LIMIT).mapNotNull { byName[it] }, recentGenres(played, list.orEmpty(), RECENT_ROWS, pinnedIds))
    }
    if (list != null && list.isEmpty()) { EmptyState(stringResource(R.string.empty_library_title), stringResource(R.string.empty_library_body), Modifier.padding(top = topPadding)); return }
    val listState = rememberLazyListState()
    ScrollToTopOnReselect(listState)
    KeepTopForGroups(groups, { listState.firstVisibleItemIndex == 0 && listState.firstVisibleItemScrollOffset == 0 }) { listState.requestScrollToItem(0) }
    LazyColumn(state = listState, contentPadding = PaddingValues(top = topPadding, bottom = BottomContentInset), modifier = Modifier.fillMaxSize().testTag("library.genres")) {
        listGroups(groups, R.string.library_all_genres, control, { it.name }) { g, _ -> GenreRow(g, onClick = { open(DetailTarget.Genre(g.name)) }) }
        if (sorted == null) items(SKELETON_COUNT, contentType = { "skeleton" }) { TrackRowSkeleton(showArtwork = false) }
        else items(sorted, key = { it.name }, contentType = { "genre" }) { g -> GenreRow(g, onClick = { open(DetailTarget.Genre(g.name)) }, modifier = Modifier.animateItem().testTag("library.genre")) }
    }
}

/** How many recently played songs the Genres list looks up for its Recent group. */
private const val RECENT_GENRE_SONGS = 40

/** The library's lists as bottom-bar places of their own. */
enum class LibraryList(@StringRes val title: Int) {
    Albums(R.string.library_albums), Artists(R.string.library_artists), Playlists(R.string.library_playlists), Songs(R.string.library_songs), Genres(R.string.library_genres)
}

/**
 * One library list on its own screen (a bottom-bar place): its top app bar (the title in the word
 * mark, the account button), then the list: Pinned, Recent and the full list under its sort chip.
 * The sort is this device's last for the list ([rememberListSort]).
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun LibraryListScreen(nav: NavHostController, list: LibraryList) {
    val client = LocalCoreClient.current
    val server by client.server.collectAsStateWithLifecycle()
    val serverId = server?.id ?: return
    val scroll = TopAppBarDefaults.enterAlwaysScrollBehavior()
    val (sort, setSort) = rememberListSort(list)
    val control = SortControl(sort, list.sortKeys, setSort)
    fun open(target: DetailTarget) = when (target) {
        is DetailTarget.Album -> nav.navigate(Route.Album(target.id))
        is DetailTarget.Artist -> nav.navigate(Route.Artist(target.id))
        is DetailTarget.Playlist -> nav.navigate(Route.Playlist(target.id))
        is DetailTarget.Genre -> nav.navigate(Route.Genre(target.name))
    }
    // A bottom-bar place of its own is top level; opened from the Library, it has a way back.
    val barItems = app.hocket.ui.nav.LocalShellNavigator.current?.items.orEmpty()
    val fromLibrary = barItems.none { it.name == list.name }
    Scaffold(
        modifier = Modifier.nestedScroll(scroll.nestedScrollConnection).testTag("libraryList.${list.name.lowercase()}"),
        topBar = {
            MediumFlexibleTopAppBar(
                title = { Text(stringResource(list.title), style = app.hocket.ui.brand.brandTitleStyle(), maxLines = 1, modifier = Modifier.semantics { heading() }) },
                navigationIcon = { if (fromLibrary) IconButton(onClick = { nav.popBackStack() }) { Icon(HocketIcons.AutoMirrored.Filled.ArrowBack, stringResource(R.string.action_back)) } },
                actions = { AccountButton() },
                scrollBehavior = scroll,
            )
        },
    ) { padding ->
        val top = padding.calculateTopPadding()
        Box(Modifier.fillMaxSize()) {
            ShimmerHost {
                when (list) {
                    LibraryList.Albums -> AlbumsTab(serverId, control, ::open, top)
                    LibraryList.Artists -> ArtistsTab(serverId, control, ::open, top)
                    LibraryList.Playlists -> PlaylistsTab(serverId, control, ::open, top)
                    LibraryList.Songs -> {
                        var offlineOnly by rememberSaveable { mutableStateOf(false) }
                        SongsTab(serverId, sort.order, sort.coreDescending, nav, offlineOnly, onOfflineOnlyChange = { offlineOnly = it }, topPadding = top, control = control)
                    }
                    LibraryList.Genres -> GenresTab(serverId, control, ::open, top)
                }
            }
            SelectionToolbar(Modifier.align(Alignment.BottomCenter).padding(bottom = app.hocket.ui.nav.BottomOverlayInset))
        }
    }
}

/**
 * Links from the Library to the places its lists don't cover: recent queues, downloads, filters,
 * statistics. An M3 Expressive connected group: four tonal tiles in a 2×2 block, large corners
 * outside and tight ones where the tiles meet, each with its icon in a shape. [joinedAbove]: the
 * group carries on from tiles above it (the Library's Browse), so only its bottom corners are large.
 */
@OptIn(ExperimentalMaterial3ExpressiveApi::class)
@Composable
internal fun LibraryLinks(nav: NavHostController, joinedAbove: Boolean = false) {
    val links = listOf(
        Triple(NavItem.RecentQueues, Route.SavedQueues, MaterialShapes.Cookie6Sided),
        Triple(NavItem.Downloads, Route.Downloads, MaterialShapes.Pill),
        Triple(NavItem.Filters, Route.Filters, MaterialShapes.Clover4Leaf),
        Triple(NavItem.Stats, Route.Stats, MaterialShapes.Sunny),
    )
    Column(Modifier.fillMaxWidth().padding(horizontal = 6.dp).testTag("library.links"), verticalArrangement = Arrangement.spacedBy(GROUP_GAP)) {
        links.chunked(2).forEachIndexed { r, row ->
            Row(horizontalArrangement = Arrangement.spacedBy(GROUP_GAP)) {
                row.forEachIndexed { c, (item, route, iconShape) ->
                    val shape = groupShape(top = r == 0 && !joinedAbove, bottom = r == 1, start = c == 0, end = c == 1)
                    Surface(
                        onClick = { nav.navigate(route) { launchSingleTop = true } },
                        shape = shape,
                        color = MaterialTheme.colorScheme.surfaceContainer,
                        modifier = Modifier.weight(1f).heightIn(min = 64.dp).testTag("library.link.${item.id}"),
                    ) {
                        Row(Modifier.padding(horizontal = 12.dp, vertical = 12.dp), verticalAlignment = Alignment.CenterVertically) {
                            Box(Modifier.size(40.dp).background(MaterialTheme.colorScheme.tertiaryContainer, iconShape.toShape()), contentAlignment = Alignment.Center) {
                                Icon(item.icon(false), null, Modifier.size(20.dp), tint = MaterialTheme.colorScheme.onTertiaryContainer)
                            }
                            Spacer(Modifier.width(12.dp))
                            Text(item.label(), style = MaterialTheme.typography.titleSmall, fontWeight = FontWeight.SemiBold, maxLines = 1, overflow = TextOverflow.Ellipsis)
                        }
                    }
                }
            }
        }
    }
}
