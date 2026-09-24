package app.hocket.ui.screens.detail

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
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
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.filled.DragHandle
import androidx.compose.material.icons.filled.Favorite
import androidx.compose.material.icons.filled.FavoriteBorder
import androidx.compose.material.icons.filled.MoreVert
import androidx.compose.material.icons.filled.PlayArrow
import androidx.compose.material.icons.filled.Shuffle
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.FilledTonalButton
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.IconToggleButton
import androidx.compose.material3.LargeFlexibleTopAppBar
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TopAppBarDefaults
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.hapticfeedback.HapticFeedbackType
import androidx.compose.ui.input.nestedscroll.nestedScroll
import androidx.compose.ui.platform.LocalHapticFeedback
import androidx.compose.ui.res.stringResource
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.navigation.NavHostController
import app.hocket.R
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.compose.LocalLifecycleOwner
import androidx.lifecycle.repeatOnLifecycle
import kotlinx.coroutines.delay
import app.hocket.ui.nav.BottomContentInset
import app.hocket.core.ArtworkSizes
import app.hocket.core.Commands
import app.hocket.core.Queries
import app.hocket.core.api.Album
import app.hocket.core.api.Artist
import app.hocket.core.api.Page
import app.hocket.core.api.Playlist
import app.hocket.core.api.QueryResult
import app.hocket.core.api.Track
import app.hocket.core.client.SelectionKind
import app.hocket.core.toSummary
import app.hocket.ui.LocalCoreClient
import app.hocket.ui.LocalWideLayout
import app.hocket.ui.components.ActionSheet
import app.hocket.ui.components.Artwork
import app.hocket.ui.components.Badge
import app.hocket.ui.components.EmptyState
import app.hocket.ui.components.RatingStars
import app.hocket.ui.components.SectionHeader
import app.hocket.ui.components.SelectionToolbar
import app.hocket.ui.components.TrackRow
import app.hocket.ui.components.formatDurationWords
import app.hocket.ui.nav.Route
import app.hocket.ui.screens.home.AlbumStrip
import app.hocket.ui.screens.library.DetailTarget
import sh.calvin.reorderable.ReorderableItem
import sh.calvin.reorderable.rememberReorderableLazyListState

/** The right-hand pane on wide layouts. */
@Composable
fun DetailPane(nav: NavHostController, target: DetailTarget?) {
    when (target) {
        is DetailTarget.Album -> AlbumDetailScreen(nav, target.id, embedded = true)
        is DetailTarget.Artist -> ArtistDetailScreen(nav, target.id, embedded = true)
        is DetailTarget.Playlist -> PlaylistDetailScreen(nav, target.id, embedded = true)
        is DetailTarget.Genre -> GenreDetailScreen(nav, target.name, embedded = true)
        null -> Box(Modifier.fillMaxSize(), contentAlignment = Alignment.Center) { Text(stringResource(R.string.nav_library), color = MaterialTheme.colorScheme.onSurfaceVariant) }
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun DetailScaffold(
    nav: NavHostController, embedded: Boolean, title: String, subtitle: String?, coverArt: String?, roundArtwork: Boolean = false,
    actions: @Composable androidx.compose.foundation.layout.RowScope.() -> Unit = {},
    header: @Composable () -> Unit,
    content: androidx.compose.foundation.lazy.LazyListScope.() -> Unit,
) {
    val scroll = TopAppBarDefaults.exitUntilCollapsedScrollBehavior()
    Scaffold(
        modifier = Modifier.nestedScroll(scroll.nestedScrollConnection),
        topBar = {
            LargeFlexibleTopAppBar(
                title = { Text(title, maxLines = 2, overflow = androidx.compose.ui.text.style.TextOverflow.Ellipsis, modifier = Modifier.semantics { heading() }) }, subtitle = subtitle?.let { { Text(it, maxLines = 1, overflow = androidx.compose.ui.text.style.TextOverflow.Ellipsis) } },
                navigationIcon = { if (!embedded) IconButton(onClick = { nav.popBackStack() }) { Icon(Icons.AutoMirrored.Filled.ArrowBack, stringResource(R.string.action_back)) } },
                actions = actions, scrollBehavior = scroll,
            )
        },
    ) { padding ->
        Box(Modifier.fillMaxSize()) {
            LazyColumn(state = rememberLazyListState(), contentPadding = PaddingValues(top = padding.calculateTopPadding(), bottom = BottomContentInset), modifier = Modifier.fillMaxSize()) {
                item {
                    // Narrow screens (display size "largest") and large fonts stack the artwork above
                    // the header, so the rating, love and play buttons get the full width.
                    BoxWithConstraints(Modifier.fillMaxWidth().padding(16.dp).testTag("detail.header")) {
                        val stacked = maxWidth < 380.dp || LocalDensity.current.fontScale >= 1.5f
                        val art: @Composable () -> Unit = { Artwork(coverArt, ArtworkSizes.GRID, title, Modifier.size(140.dp), if (roundArtwork) CircleShape else RoundedCornerShape(20.dp)) }
                        if (stacked) {
                            Column { art(); Spacer(Modifier.height(12.dp)); header() }
                        } else {
                            Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
                                art()
                                Spacer(Modifier.width(16.dp))
                                Column(Modifier.weight(1f)) { header() }
                            }
                        }
                    }
                }
                content()
            }
            SelectionToolbar(Modifier.align(Alignment.BottomCenter).padding(bottom = 88.dp))
        }
    }
}

/** How long an album page stays resumed and on screen before its first track is primed. */
internal const val PRIME_DWELL_MS = 2_000L

/**
 * The album primer: once per visit, when the page has been resumed (on screen, app in front) for
 * [PRIME_DWELL_MS] without a break, ask the core to prime the album's first seconds so pressing
 * play starts from disk. Leaving or backgrounding before then restarts the wait; a new visit (the
 * page composed again) may prime again, which the core deduplicates. The core decides whether
 * priming is allowed (the playback owner's network and battery state).
 */
@Composable
fun PrimeAlbumOnDwell(albumId: String) {
    val client = LocalCoreClient.current
    val lifecycle = LocalLifecycleOwner.current.lifecycle
    val primed = remember(albumId) { java.util.concurrent.atomic.AtomicBoolean(false) }
    LaunchedEffect(albumId, lifecycle) {
        lifecycle.repeatOnLifecycle(Lifecycle.State.RESUMED) {
            if (primed.get()) return@repeatOnLifecycle
            delay(PRIME_DWELL_MS)
            if (primed.compareAndSet(false, true)) client.dispatch(Commands.primeAlbum(albumId))
        }
    }
}

@Composable
fun AlbumDetailScreen(nav: NavHostController, id: String, embedded: Boolean = false) {
    val client = LocalCoreClient.current
    PrimeAlbumOnDwell(id)
    val libraryGen by client.libraryChanged.collectAsStateWithLifecycle(initialValue = null)
    var album by remember { mutableStateOf<Album?>(null) }
    var tracks by remember { mutableStateOf<List<Track>>(emptyList()) }
    LaunchedEffect(id, libraryGen) {
        album = (client.query(Queries.album(id)) as? QueryResult.AlbumDetail)?.data
        tracks = (client.query(Queries.albumTracks(id)) as? QueryResult.TrackList)?.data ?: emptyList()
    }
    val a = album ?: return
    val nowPlaying by client.nowPlaying.collectAsStateWithLifecycle()
    val selection by client.selection.collectAsStateWithLifecycle()
    val kind by client.selectionKind.collectAsStateWithLifecycle()
    val selecting = selection.active && kind == SelectionKind.Tracks
    var sheetFor by remember { mutableStateOf<Track?>(null) }
    var albumSheet by remember { mutableStateOf(false) }
    val context = Commands.albumContext(a.serverId, a.id, a.name)
    DetailScaffold(nav, embedded, a.name, a.artist, a.coverArt, actions = { IconButton(onClick = { albumSheet = true }) { Icon(Icons.Filled.MoreVert, stringResource(R.string.action_more)) } }, header = {
        Text(listOfNotNull(a.year?.toString(), a.genre).joinToString(stringResource(R.string.dot_separator)), style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
        Text(stringResource(R.string.album_tracks_count, a.songCount.toInt(), formatDurationWords(a.durationMs.toLong())), style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
        Spacer(Modifier.height(8.dp))
        FlowRow(itemVerticalAlignment = Alignment.CenterVertically) {
            RatingStars(a.rating.toInt(), onRate = { client.dispatch(Commands.setRating(listOf(app.hocket.core.api.RatingTarget.Album(app.hocket.core.api.RatingTargetAlbumInner(a.id))), it)) }, starSize = 20.dp)
            IconToggleButton(checked = a.loved, onCheckedChange = { client.dispatch(Commands.loveAlbum(a.id, it)) }) {
                Icon(if (a.loved) Icons.Filled.Favorite else Icons.Filled.FavoriteBorder, stringResource(if (a.loved) R.string.action_unlove else R.string.action_love), tint = if (a.loved) MaterialTheme.colorScheme.tertiary else MaterialTheme.colorScheme.onSurfaceVariant)
            }
        }
        Spacer(Modifier.height(8.dp))
        PlayShuffleRow(onPlay = { client.dispatch(Commands.playContext(context)) }, onShuffle = { client.dispatch(Commands.playContext(context, shuffle = true)) })
    }) {
        itemsIndexed(tracks, key = { _, t -> t.id }) { i, t ->
            TrackRow(t.toSummary(), onClick = { client.dispatch(Commands.playContext(context, startIndex = i)) }, onMore = { sheetFor = t }, showArtwork = false,
                leading = { Text((t.trackNumber?.toInt() ?: (i + 1)).toString(), style = MaterialTheme.typography.labelLarge, color = MaterialTheme.colorScheme.onSurfaceVariant, modifier = Modifier.width(32.dp)) },
                selected = selecting && selection.contains(t.id), selectionActive = selecting, onToggleSelect = { client.toggleSelected(SelectionKind.Tracks, t.id) }, nowPlaying = nowPlaying?.track?.id == t.id)
        }
    }
    sheetFor?.let { t -> ActionSheet(Commands.tracks(listOf(t.id)), t.title, t.artist, onDismiss = { sheetFor = null }, onGoToArtist = t.artistId?.let { aid -> { nav.navigate(Route.Artist(aid)) } }) }
    if (albumSheet) ActionSheet(Commands.albums(listOf(a.id)), a.name, a.artist, onDismiss = { albumSheet = false }, onGoToArtist = a.artistId?.let { aid -> { nav.navigate(Route.Artist(aid)) } })
}

@Composable
fun PlayShuffleRow(onPlay: () -> Unit, onShuffle: () -> Unit) {
    FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
        Button(onClick = onPlay, shapes = ButtonDefaults.shapes()) { Icon(Icons.Filled.PlayArrow, null); Spacer(Modifier.width(6.dp)); Text(stringResource(R.string.action_play)) }
        FilledTonalButton(onClick = onShuffle, shapes = ButtonDefaults.shapes()) { Icon(Icons.Filled.Shuffle, null); Spacer(Modifier.width(6.dp)); Text(stringResource(R.string.action_shuffle)) }
    }
}

@Composable
fun ArtistDetailScreen(nav: NavHostController, id: String, embedded: Boolean = false) {
    val client = LocalCoreClient.current
    val libraryGen by client.libraryChanged.collectAsStateWithLifecycle(initialValue = null)
    val server by client.server.collectAsStateWithLifecycle()
    var artist by remember { mutableStateOf<Artist?>(null) }
    var albums by remember { mutableStateOf<List<Album>>(emptyList()) }
    var top by remember { mutableStateOf<List<Track>>(emptyList()) }
    LaunchedEffect(id, libraryGen) {
        artist = (client.query(Queries.artist(id)) as? QueryResult.ArtistDetail)?.data
        server?.id?.let { sid -> albums = (client.query(Queries.albums(sid, Page(0u, 200u), app.hocket.core.api.SortOrder.Year, artistId = id)) as? QueryResult.Albums)?.data?.items ?: emptyList() }
        top = (client.query(Queries.artistTopSongs(id, 10)) as? QueryResult.TrackList)?.data ?: emptyList()
    }
    val ar = artist ?: return
    val wide = LocalWideLayout.current
    val context = Commands.artistContext(ar.serverId, ar.id, ar.name)
    var sheetFor by remember { mutableStateOf<Track?>(null) }
    DetailScaffold(nav, embedded, ar.name, stringResource(R.string.library_count_albums, ar.albumCount.toInt()), ar.coverArt, roundArtwork = true, header = {
        IconToggleButton(checked = ar.loved, onCheckedChange = { client.dispatch(Commands.setArtistLoved(ar.id, it)) }) {
            Icon(if (ar.loved) Icons.Filled.Favorite else Icons.Filled.FavoriteBorder, stringResource(if (ar.loved) R.string.action_unlove else R.string.action_love), tint = if (ar.loved) MaterialTheme.colorScheme.tertiary else MaterialTheme.colorScheme.onSurfaceVariant)
        }
        PlayShuffleRow(onPlay = { client.dispatch(Commands.playContext(context)) }, onShuffle = { client.dispatch(Commands.playContext(context, shuffle = true)) })
    }) {
        ar.biography?.let { bio -> item { SectionHeader(stringResource(R.string.artist_biography)); Text(bio, style = MaterialTheme.typography.bodyMedium, modifier = Modifier.padding(horizontal = 16.dp)) } }
        if (top.isNotEmpty()) {
            item { SectionHeader(stringResource(R.string.artist_top_songs)) }
            itemsIndexed(top, key = { _, t -> "top" + t.id }) { i, t ->
                TrackRow(t.toSummary(), onClick = { client.dispatch(Commands.playTracks(t.serverId, top.map { it.id }, i, ar.name)) }, onMore = { sheetFor = t })
            }
        }
        item { SectionHeader(stringResource(R.string.artist_albums)) }
        item { AlbumStrip(albums) { if (wide && embedded) nav.navigate(Route.Album(it.id)) else nav.navigate(Route.Album(it.id)) } }
    }
    sheetFor?.let { t -> ActionSheet(Commands.tracks(listOf(t.id)), t.title, t.artist, onDismiss = { sheetFor = null }, onGoToAlbum = t.albumId?.let { aid -> { nav.navigate(Route.Album(aid)) } }) }
}

/** Playlists: drag-reorder with haptics unless smart (read-only: no reorder, no manual add). */
@Composable
fun PlaylistDetailScreen(nav: NavHostController, id: String, embedded: Boolean = false) {
    val client = LocalCoreClient.current
    val libraryGen by client.libraryChanged.collectAsStateWithLifecycle(initialValue = null)
    var playlist by remember { mutableStateOf<Playlist?>(null) }
    var tracks by remember { mutableStateOf<List<Track>>(emptyList()) }
    LaunchedEffect(id, libraryGen) {
        playlist = (client.query(Queries.playlist(id)) as? QueryResult.PlaylistDetail)?.data
        tracks = (client.query(Queries.playlistTracks(id, Page(0u, 5000u))) as? QueryResult.Tracks)?.data?.items ?: emptyList()
    }
    val p = playlist ?: return
    val haptics = LocalHapticFeedback.current
    val nowPlaying by client.nowPlaying.collectAsStateWithLifecycle()
    val selection by client.selection.collectAsStateWithLifecycle()
    val kind by client.selectionKind.collectAsStateWithLifecycle()
    val selecting = selection.active && kind == SelectionKind.Tracks
    var sheetFor by remember { mutableStateOf<Pair<Int, Track>?>(null) }
    var playlistSheet by remember { mutableStateOf(false) }
    val context = Commands.playlistContext(p.serverId, p.id, p.name)
    val listState = rememberLazyListState()
    val reorderable = rememberReorderableLazyListState(listState) { from, to ->
        val fromIdx = from.index - 1; val toIdx = to.index - 1 // header item offset
        if (fromIdx >= 0 && toIdx >= 0) {
            tracks = tracks.toMutableList().apply { add(toIdx, removeAt(fromIdx)) }
            client.dispatch(Commands.playlistMove(p.id, fromIdx, toIdx))
            haptics.performHapticFeedback(HapticFeedbackType.SegmentFrequentTick)
        }
    }
    val scroll = TopAppBarDefaults.exitUntilCollapsedScrollBehavior()
    Scaffold(
        modifier = Modifier.nestedScroll(scroll.nestedScrollConnection),
        topBar = {
            LargeFlexibleTopAppBar(title = { Text(p.name, maxLines = 2) }, subtitle = { Text(p.owner?.let { stringResource(R.string.playlist_by, it) } ?: "") },
                navigationIcon = { if (!embedded) IconButton(onClick = { nav.popBackStack() }) { Icon(Icons.AutoMirrored.Filled.ArrowBack, stringResource(R.string.action_back)) } },
                actions = { IconButton(onClick = { playlistSheet = true }) { Icon(Icons.Filled.MoreVert, stringResource(R.string.action_more)) } }, scrollBehavior = scroll)
        },
    ) { padding ->
        Box(Modifier.fillMaxSize()) {
            LazyColumn(state = listState, contentPadding = PaddingValues(top = padding.calculateTopPadding(), bottom = BottomContentInset), modifier = Modifier.fillMaxSize()) {
                item(key = "header") {
                    Row(Modifier.fillMaxWidth().padding(16.dp), verticalAlignment = Alignment.CenterVertically) {
                        Artwork(p.coverArt, ArtworkSizes.GRID, p.name, Modifier.size(140.dp), RoundedCornerShape(20.dp))
                        Spacer(Modifier.width(16.dp))
                        Column(Modifier.weight(1f)) {
                            Row(horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                                if (p.isSmart) Badge(stringResource(R.string.badge_smart))
                                if (p.isSmart || !p.isMine) Badge(stringResource(R.string.badge_read_only))
                                if (p.public) Badge(stringResource(R.string.playlist_public))
                            }
                            Text(stringResource(R.string.album_tracks_count, p.songCount.toInt(), formatDurationWords(p.durationMs.toLong())), style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
                            p.comment?.let { Text(it, style = MaterialTheme.typography.bodyMedium) }
                            if (p.isSmart) Text(stringResource(R.string.smart_playlist_note), style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
                            Spacer(Modifier.height(8.dp))
                            PlayShuffleRow(onPlay = { client.dispatch(Commands.playContext(context)) }, onShuffle = { client.dispatch(Commands.playContext(context, shuffle = true)) })
                        }
                    }
                }
                itemsIndexed(tracks, key = { i, t -> "$i:${t.id}" }) { i, t ->
                    val editable = !p.isSmart && p.isMine
                    ReorderableItem(reorderable, key = "$i:${t.id}", enabled = editable) { dragging ->
                        TrackRow(t.toSummary(), onClick = { client.dispatch(Commands.playContext(context, startIndex = i)) }, onMore = { sheetFor = i to t },
                            selected = selecting && selection.contains(t.id), selectionActive = selecting, onToggleSelect = { client.toggleSelected(SelectionKind.Tracks, t.id) },
                            nowPlaying = nowPlaying?.track?.id == t.id,
                            trailing = if (editable) ({
                                Icon(Icons.Filled.DragHandle, stringResource(R.string.playlist_reorder_handle), tint = MaterialTheme.colorScheme.onSurfaceVariant,
                                    modifier = Modifier.padding(start = 8.dp).draggableHandle(onDragStarted = { haptics.performHapticFeedback(HapticFeedbackType.GestureThresholdActivate) }, onDragStopped = { haptics.performHapticFeedback(HapticFeedbackType.GestureEnd) }))
                            }) else null)
                    }
                }
            }
            SelectionToolbar(Modifier.align(Alignment.BottomCenter).padding(bottom = 88.dp))
        }
    }
    sheetFor?.let { (i, t) ->
        ActionSheet(Commands.tracks(listOf(t.id)), t.title, t.artist, onDismiss = { sheetFor = null }, onGoToAlbum = t.albumId?.let { aid -> { nav.navigate(Route.Album(aid)) } },
            extraTop = if (!p.isSmart && p.isMine) ({
                androidx.compose.material3.TextButton(onClick = { client.dispatch(Commands.playlistRemove(p.id, listOf(i))); sheetFor = null }, modifier = Modifier.padding(horizontal = 12.dp)) { Text(stringResource(R.string.action_remove_from_playlist)) }
            }) else null)
    }
    if (playlistSheet) ActionSheet(Commands.playlists(listOf(p.id)), p.name, p.owner, onDismiss = { playlistSheet = false })
}

@Composable
fun GenreDetailScreen(nav: NavHostController, name: String, embedded: Boolean = false) {
    val client = LocalCoreClient.current
    val server by client.server.collectAsStateWithLifecycle()
    val serverId = server?.id ?: return
    val libraryGen by client.libraryChanged.collectAsStateWithLifecycle(initialValue = null)
    var albums by remember { mutableStateOf<List<Album>>(emptyList()) }
    var tracks by remember { mutableStateOf<List<Track>>(emptyList()) }
    LaunchedEffect(name, libraryGen) {
        albums = (client.query(Queries.albums(serverId, Page(0u, 200u), app.hocket.core.api.SortOrder.Year, genre = name)) as? QueryResult.Albums)?.data?.items ?: emptyList()
        tracks = (client.query(Queries.tracks(serverId, Page(0u, 500u), app.hocket.core.api.SortOrder.Title, filter = app.hocket.core.api.FilterNode.Rule(app.hocket.core.api.FilterRule(app.hocket.core.api.FilterField.Genre, app.hocket.core.api.FilterOp.Is, app.hocket.core.api.FilterValue.Text(name))))) as? QueryResult.Tracks)?.data?.items ?: emptyList()
    }
    val context = Commands.genreContext(serverId, name)
    var sheetFor by remember { mutableStateOf<Track?>(null) }
    if (albums.isEmpty() && tracks.isEmpty()) { EmptyState(stringResource(R.string.empty_library_title), stringResource(R.string.empty_library_body)); return }
    DetailScaffold(nav, embedded, name, stringResource(R.string.library_count_songs, tracks.size), albums.firstOrNull()?.coverArt, header = {
        PlayShuffleRow(onPlay = { client.dispatch(Commands.playContext(context)) }, onShuffle = { client.dispatch(Commands.playContext(context, shuffle = true)) })
    }) {
        item { SectionHeader(stringResource(R.string.genre_albums)) }
        item { AlbumStrip(albums) { nav.navigate(Route.Album(it.id)) } }
        item { SectionHeader(stringResource(R.string.genre_songs)) }
        itemsIndexed(tracks, key = { _, t -> t.id }) { i, t ->
            TrackRow(t.toSummary(), onClick = { client.dispatch(Commands.playContext(context, startIndex = i)) }, onMore = { sheetFor = t })
        }
    }
    sheetFor?.let { t -> ActionSheet(Commands.tracks(listOf(t.id)), t.title, t.artist, onDismiss = { sheetFor = null }, onGoToAlbum = t.albumId?.let { aid -> { nav.navigate(Route.Album(aid)) } }) }
}
