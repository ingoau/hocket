package app.hocket.ui.screens.detail

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.wrapContentHeight
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.layout.statusBars
import androidx.compose.foundation.layout.asPaddingValues
import androidx.compose.foundation.layout.heightIn
import androidx.compose.material3.ButtonGroupDefaults
import androidx.compose.material3.Surface
import androidx.compose.material3.ToggleButton
import androidx.compose.ui.semantics.role
import androidx.compose.ui.text.font.FontWeight
import app.hocket.ui.components.formatClock
import app.hocket.ui.components.skeleton
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
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.hapticfeedback.HapticFeedbackType
import androidx.compose.ui.input.nestedscroll.nestedScroll
import androidx.compose.ui.platform.LocalHapticFeedback
import androidx.compose.ui.res.pluralStringResource
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
import app.hocket.ui.icons.HocketIcons
import kotlinx.coroutines.delay
import app.hocket.ui.nav.BottomContentInset
import app.hocket.core.ArtworkSizes
import app.hocket.core.ActionIds
import app.hocket.core.Commands
import app.hocket.core.Queries
import app.hocket.core.api.Album
import app.hocket.core.api.Artist
import app.hocket.core.api.ReleaseKind
import app.hocket.core.byReleaseKind
import app.hocket.core.api.Page
import app.hocket.core.api.Playlist
import app.hocket.core.api.QueryResult
import app.hocket.core.api.Track
import app.hocket.core.client.SelectionKind
import app.hocket.core.toSummary
import app.hocket.ui.LocalCoreClient
import app.hocket.ui.rememberCachedState
import app.hocket.ui.LocalWideLayout
import app.hocket.ui.components.ActionSheet
import app.hocket.ui.components.Artwork
import app.hocket.ui.components.Badge
import app.hocket.ui.components.EmptyState
import app.hocket.ui.components.RatingStars
import app.hocket.ui.components.SectionHeader
import app.hocket.ui.components.SelectionToolbar
import app.hocket.ui.components.TrackRow
import app.hocket.ui.components.ShimmerHost
import app.hocket.ui.components.SkeletonLine
import app.hocket.ui.components.TrackRowSkeleton
import app.hocket.ui.components.skeleton
import androidx.compose.foundation.lazy.items
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.ui.semantics.clearAndSetSemantics
import app.hocket.ui.components.formatDurationWords
import app.hocket.ui.nav.ArtworkShare
import app.hocket.ui.nav.flyingCoverArt
import app.hocket.ui.nav.Route
import app.hocket.ui.nav.sharedArtworkDestination
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

/**
 * A detail page: the header ([DetailHero], in the mode the artwork allows) as the list's first item
 * under a floating bar (back, the page's [barActions]) that fills in with the name once the header
 * has scrolled away, then [content]. While [loading] (the page's first answer has not arrived) the
 * header is a banner with the title as a skeleton and the rows are skeletons of the same size, so
 * opening a page never shows a blank screen.
 */
@Composable
private fun DetailScaffold(
    nav: NavHostController, embedded: Boolean, title: String?, coverArt: String?, roundArtwork: Boolean = false,
    /** False where the artwork stands in for the page (a genre's first album): never edge to edge. */
    immersive: Boolean = true,
    loading: Boolean = false,
    shareKey: String? = null,
    listState: androidx.compose.foundation.lazy.LazyListState = rememberLazyListState(),
    barActions: @Composable androidx.compose.foundation.layout.RowScope.() -> Unit = {},
    eyebrow: (@Composable () -> Unit)? = null,
    below: @Composable androidx.compose.foundation.layout.ColumnScope.() -> Unit = {},
    heroActions: @Composable androidx.compose.foundation.layout.RowScope.() -> Unit = {},
    overlay: @Composable androidx.compose.foundation.layout.BoxScope.() -> Unit = { SelectionToolbar(Modifier.align(Alignment.BottomCenter).padding(bottom = app.hocket.ui.nav.BottomOverlayInset)) },
    content: androidx.compose.foundation.lazy.LazyListScope.() -> Unit,
) {
    BoxWithConstraints(Modifier.fillMaxSize()) {
        val look = rememberDetailLook(coverArt, roundArtwork || !immersive, maxWidth)
        // The bar fills in as the title passes under it (or once the header has gone).
        var titleBottom by remember { androidx.compose.runtime.mutableFloatStateOf(Float.MAX_VALUE) }
        val barBottom = with(LocalDensity.current) { (androidx.compose.foundation.layout.WindowInsets.statusBars.asPaddingValues().calculateTopPadding() + 64.dp).toPx() }
        val collapsed by remember(barBottom) { androidx.compose.runtime.derivedStateOf { listState.firstVisibleItemIndex > 0 || titleBottom < barBottom } }
        // The status bar's icons over the cover's top strip, then over the filled-in bar (on a
        // phone: a wide layout's pane shares the status bar with the list beside it).
        if (!embedded) app.hocket.ui.player.StatusBarOver(if (look.immersive && !collapsed) (look.topLight ?: !look.dark) else !look.dark)
        // The whole page in the artwork's colours, and the bar floating over it too.
        if (!embedded) app.hocket.ui.nav.PageColors(look.scheme)
        MaterialTheme(colorScheme = look.scheme) {
            androidx.compose.runtime.CompositionLocalProvider(app.hocket.ui.theme.LocalDarkTheme provides look.dark, androidx.compose.material3.LocalContentColor provides look.scheme.onBackground) {
                Box(Modifier.fillMaxSize().background(look.scheme.background)) {
                    ShimmerHost {
                        LazyColumn(state = listState, contentPadding = PaddingValues(bottom = BottomContentInset), modifier = Modifier.fillMaxSize()) {
                            item(key = "header", contentType = "header") {
                                DetailHero(
                                    look = look,
                                    title = if (loading) title else title.orEmpty(), coverArt = coverArt, shareKey = shareKey, round = roundArtwork,
                                    eyebrow = eyebrow.takeIf { !loading }, below = { if (loading) SkeletonLine(Modifier.padding(top = 10.dp).fillMaxWidth(0.4f), 14.dp) else below() },
                                    onTitleBottom = { titleBottom = it },
                                    actions = { if (loading) { Box(Modifier.weight(1f).height(56.dp).skeleton(CircleShape)); Box(Modifier.weight(1f).height(56.dp).skeleton(CircleShape)) } else heroActions() },
                                )
                            }
                            if (loading) {
                                items(8, key = { "skeleton.row$it" }, contentType = { "skeletonRow" }) { TrackRowSkeleton() }
                                return@LazyColumn
                            }
                            content()
                        }
                    }
                    DetailTopBar(title, collapsed = collapsed, onBack = if (embedded) null else ({ nav.popBackStack() }), overArt = look.topLight?.takeIf { look.immersive }, actions = barActions)
                    overlay()
                }
            }
        }
    }
}

/** A group's rows in a list: inset from the screen's edges, 2dp below the one before. */
internal fun Modifier.segmentItem(index: Int): Modifier = padding(start = 16.dp, end = 16.dp, top = if (index > 0) androidx.compose.material3.ListItemDefaults.SegmentedGap else 0.dp)

/** "Your rating" and the stars, between the header and the tracks. */
@Composable
private fun RatingRow(rating: Int, onRate: (Int) -> Unit) {
    Row(Modifier.fillMaxWidth().padding(start = 20.dp, end = 12.dp, top = 12.dp), verticalAlignment = Alignment.CenterVertically) {
        Text(stringResource(R.string.player_about_rating), style = MaterialTheme.typography.titleSmall, color = MaterialTheme.colorScheme.onSurfaceVariant, modifier = Modifier.weight(1f))
        RatingStars(rating, onRate = onRate, starSize = 24.dp, spacing = 20.dp)
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
    val libraryGen by client.libraryGeneration.collectAsStateWithLifecycle()
    val shareKey = ArtworkShare.album(id)
    val flyingCover = flyingCoverArt(shareKey)
    // Start from the last answer for this album: a pop back here (or a second visit) has the header
    // and its artwork on the first frame, ready for a flight back into it.
    var album by rememberCachedState<Album?>(listOf("detail.album", id)) { null }
    var tracks by rememberCachedState<List<Track>?>(listOf("detail.albumTracks", id)) { null }
    var more by rememberCachedState(listOf("detail.albumMore", id)) { emptyList<Album>() }
    LaunchedEffect(id, libraryGen) {
        val a = (client.query(Queries.album(id)) as? QueryResult.AlbumDetail)?.data
        val t = (client.query(Queries.albumTracks(id)) as? QueryResult.TrackList)?.data
        // Both at once, so the header and its rows land together.
        album = a ?: album
        tracks = t ?: tracks ?: emptyList()
        // The artist's other releases, for "More by" under the tracks.
        val artistId = (a ?: album)?.artistId
        if (artistId != null) more = ((client.query(Queries.albums((a ?: album)!!.serverId, Page(0u, 50u), app.hocket.core.api.SortOrder.Year, artistId = artistId)) as? QueryResult.Albums)?.data?.items ?: more).filter { it.id != id }
    }
    val a = album
    if (a == null) {
        DetailScaffold(nav, embedded, null, flyingCover, shareKey = shareKey, loading = true) {}
        return
    }
    val nowPlaying by client.nowPlaying.collectAsStateWithLifecycle()
    val selection by client.selection.collectAsStateWithLifecycle()
    val kind by client.selectionKind.collectAsStateWithLifecycle()
    val pins by client.pins.collectAsStateWithLifecycle()
    val selecting = selection.active && kind == SelectionKind.Tracks
    val songMenu = app.hocket.ui.components.rememberSongMenu()
    var albumSheet by remember { mutableStateOf(false) }
    val context = Commands.albumContext(a.serverId, a.id, a.name)
    val target = Commands.albums(listOf(a.id))
    val downloaded = pins.any { it.target == Commands.albumPin(a.id) }
    val dot = stringResource(R.string.dot_separator)
    DetailScaffold(nav, embedded, a.name, a.coverArt, shareKey = shareKey,
        barActions = {
            DetailLoveButton(a.loved, onToggle = { client.dispatch(Commands.loveAlbum(a.id, it)) })
            DetailDownloadButton(downloaded, onToggle = { client.dispatch(Commands.runAction(if (it) ActionIds.DOWNLOAD else ActionIds.UNPIN, target)) })
            DetailBarButton(HocketIcons.Filled.MoreVert, stringResource(R.string.action_more), onClick = { albumSheet = true })
        },
        below = {
            a.artist?.let { name ->
                HeroLine(name, style = MaterialTheme.typography.titleMedium, fontWeight = FontWeight.SemiBold, color = MaterialTheme.colorScheme.primary, maxLines = 1,
                    modifier = Modifier.padding(top = 6.dp).heightIn(min = 40.dp).wrapContentHeight().then(a.artistId?.let { aid -> Modifier.clickable(onClickLabel = stringResource(R.string.action_go_to_artist)) { nav.navigate(Route.Artist(aid)) } } ?: Modifier).testTag("album.artist"))
            }
            HeroLine(listOfNotNull(a.genre?.takeIf { it.isNotBlank() }, a.year?.takeIf { it > 0u }?.toString(), stringResource(R.string.album_tracks_count, a.songCount.toInt(), formatDurationWords(a.durationMs.toLong()))).joinToString(dot))
        },
        heroActions = {
            HeroPlayButton(onClick = { client.dispatch(Commands.playContext(context)) })
            HeroShuffleButton(onClick = { client.dispatch(Commands.playContext(context, shuffle = true)) })
        },
    ) {
        item(key = "rating") { RatingRow(a.rating.toInt()) { client.dispatch(Commands.setRating(listOf(app.hocket.core.api.RatingTarget.Album(app.hocket.core.api.RatingTargetAlbumInner(a.id))), it)) } }
        val list = tracks
        item(key = "h.tracks") {
            DetailSectionHead(stringResource(R.string.detail_tracks), Modifier.padding(top = 16.dp)) {
                if (list != null) Text(stringResource(R.string.album_tracks_count, list.size, formatClock(list.sumOf { it.durationMs.toLong() })), style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
            }
        }
        if (list == null) items(8, key = { "skeleton.row$it" }, contentType = { "skeletonRow" }) { TrackRowSkeleton(showArtwork = false) }
        else itemsIndexed(list, key = { _, t -> t.id }, contentType = { _, _ -> "track" }) { i, t ->
            val playing = nowPlaying?.track?.id == t.id
            TrackRow(t.toSummary(), onClick = { client.dispatch(Commands.playContext(context, startIndex = i)) }, onMore = { songMenu.open(t.toSummary()) }, showArtwork = false,
                leading = {
                    Box(Modifier.width(40.dp), contentAlignment = Alignment.Center) {
                        if (playing) Icon(HocketIcons.Filled.GraphicEq, null, Modifier.size(18.dp), tint = MaterialTheme.colorScheme.primary)
                        else Text((t.trackNumber?.toInt() ?: (i + 1)).toString(), style = MaterialTheme.typography.labelLarge, color = MaterialTheme.colorScheme.onSurfaceVariant)
                    }
                    Spacer(Modifier.width(4.dp))
                },
                selected = selecting && selection.contains(t.id), selectionActive = selecting, onToggleSelect = { client.toggleSelected(SelectionKind.Tracks, t.id) }, nowPlaying = playing,
                segment = i to list.size, showSubtitle = false, nowPlayingMark = false, modifier = Modifier.segmentItem(i))
        }
        item(key = "footer") {
            val date = java.text.DateFormat.getDateInstance(java.text.DateFormat.LONG)
            val facts = listOfNotNull(
                a.year?.takeIf { it > 0u }?.let { stringResource(R.string.album_released, it.toInt()) },
                a.created?.let { stringResource(R.string.album_added, date.format(java.util.Date(it.toLong()))) },
                pluralStringResource(R.plurals.album_plays, a.playCount.toInt(), a.playCount.toInt()),
            )
            Text(facts.joinToString(dot), style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant, modifier = Modifier.padding(start = 20.dp, end = 20.dp, top = 14.dp))
        }
        if (more.isNotEmpty() && a.artistId != null) {
            item(key = "h.more") {
                DetailSectionHead(stringResource(R.string.album_more_by, a.artist ?: ""), Modifier.padding(top = 24.dp)) {
                    ViewMoreButton(onClick = { nav.navigate(Route.Artist(a.artistId!!)) })
                }
            }
            item(key = "more") {
                ReleaseShelf(more, onOpen = { nav.navigate(Route.Album(it.id)) }, onPlay = { m -> client.dispatch(Commands.playContext(Commands.albumContext(m.serverId, m.id, m.name))) }, modifier = Modifier.padding(top = 4.dp)) { m ->
                    listOfNotNull(m.year?.takeIf { it > 0u }?.toString(), stringResource(R.string.saved_tracks, m.songCount.toInt())).joinToString(dot)
                }
            }
        }
    }
    if (albumSheet) ActionSheet(target, a.name, a.artist, onDismiss = { albumSheet = false }, onGoToArtist = a.artistId?.let { aid -> { nav.navigate(Route.Artist(aid)) } })
}

@Composable
fun PlayShuffleRow(onPlay: () -> Unit, onShuffle: () -> Unit) {
    FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
        Button(onClick = onPlay, shapes = ButtonDefaults.shapes()) { Icon(HocketIcons.Filled.PlayArrow, null); Spacer(Modifier.width(6.dp)); Text(stringResource(R.string.action_play)) }
        FilledTonalButton(onClick = onShuffle, shapes = ButtonDefaults.shapes()) { Icon(HocketIcons.Filled.Shuffle, null); Spacer(Modifier.width(6.dp)); Text(stringResource(R.string.action_shuffle)) }
    }
}

/** How many top songs an artist page lists. */
private const val ARTIST_TOP_SONGS = 10

/** Top songs to a column of the sideways pager. */
private const val TOP_PER_COLUMN = 3

/** Releases on a shelf before "View more" opens the kind's full grid. */
private const val SHELF = 8

/** The artist page's own sub-views: all the top songs, or one release kind's full grid. */
private sealed interface ArtistView {
    data object TopSongs : ArtistView
    data class Releases(val kind: ReleaseKind) : ArtistView
}

private val ArtistViewSaver = androidx.compose.runtime.saveable.Saver<ArtistView?, String>(
    save = { v -> when (v) { null -> null; ArtistView.TopSongs -> "top"; is ArtistView.Releases -> v.kind.string } },
    restore = { v -> if (v == "top") ArtistView.TopSongs else ReleaseKind.entries.firstOrNull { it.string == v }?.let { ArtistView.Releases(it) } },
)

@Composable
fun ArtistDetailScreen(nav: NavHostController, id: String, embedded: Boolean = false) {
    val client = LocalCoreClient.current
    val libraryGen by client.libraryGeneration.collectAsStateWithLifecycle()
    val server by client.server.collectAsStateWithLifecycle()
    val shareKey = ArtworkShare.artist(id)
    val flyingCover = flyingCoverArt(shareKey)
    var artist by rememberCachedState<Artist?>(listOf("detail.artist", id)) { null }
    var albums by rememberCachedState(listOf("detail.artistAlbums", id)) { emptyList<Album>() }
    // null until the first answer: the section keeps its rows' space (skeletons) meanwhile, so the
    // albums below never jump when the songs arrive.
    var top by rememberCachedState<List<Track>?>(listOf("detail.artistTop", id)) { null }
    var about by rememberCachedState<String?>(listOf("detail.artistAbout", id)) { null }
    // The artist and its albums come from the local mirror: show them as soon as they are read.
    // They used to wait for the top songs too, which on a first visit means a server round trip.
    LaunchedEffect(id, libraryGen, server?.id) {
        val ar = (client.query(Queries.artist(id)) as? QueryResult.ArtistDetail)?.data
        val al = server?.id?.let { sid -> (client.query(Queries.albums(sid, Page(0u, 200u), app.hocket.core.api.SortOrder.Year, artistId = id)) as? QueryResult.Albums)?.data?.items }
        // Together, so the header and the albums do not pop in one after another.
        albums = al ?: albums
        artist = ar ?: artist
    }
    // Cached by the core (and refreshed behind the page), so usually as quick as the rest.
    LaunchedEffect(id, libraryGen) {
        val t = (client.query(Queries.artistTopSongs(id, ARTIST_TOP_SONGS)) as? QueryResult.TrackList)?.data
        top = t ?: top ?: emptyList()
    }
    // About: the server's biography (Last.fm), when the artist record has none of its own.
    LaunchedEffect(id) { about = (client.query(Queries.artistInfo(id)) as? QueryResult.ExternalInfo)?.data?.text ?: about }
    val ar = artist
    if (ar == null) {
        DetailScaffold(nav, embedded, null, flyingCover, roundArtwork = true, shareKey = shareKey, loading = true) {}
        return
    }
    val context = Commands.artistContext(ar.serverId, ar.id, ar.name)
    val songMenu = app.hocket.ui.components.rememberSongMenu()
    val nowPlaying by client.nowPlaying.collectAsStateWithLifecycle()
    val shelves = albums.byReleaseKind()
    val dot = stringResource(R.string.dot_separator)
    val counts = shelves.map { (kind, list) -> pluralStringResource(releaseCountRes(kind), list.size, list.size) }.joinToString(dot)
    // Saved as text, so the sub-view survives rotation: "top", or a release kind's id.
    var view by rememberSaveable(id, stateSaver = ArtistViewSaver) { mutableStateOf<ArtistView?>(null) }
    var oldest by remember(id) { mutableStateOf(false) }
    androidx.activity.compose.BackHandler(enabled = view != null) { view = null }
    val openAlbum = { al: Album -> nav.navigate(Route.Album(al.id)) }
    val playAlbum = { al: Album -> client.dispatch(Commands.playContext(Commands.albumContext(al.serverId, al.id, al.name))) }
    val releaseMeta = @Composable { al: Album -> listOfNotNull(al.year?.takeIf { it > 0u }?.toString(), stringResource(R.string.saved_tracks, al.songCount.toInt())).joinToString(dot) }
    val playTop = { i: Int -> top?.let { list -> client.dispatch(Commands.playTracks(list[i].serverId, list.map { it.id }, i, ar.name)) } }
    val topRow = @Composable { i: Int, t: Track, segment: Pair<Int, Int>, modifier: Modifier ->
        TrackRow(t.toSummary(), onClick = { playTop(i) }, onMore = { songMenu.open(t.toSummary()) }, nowPlaying = nowPlaying?.track?.id == t.id, segment = segment, rank = i + 1,
            subtitle = listOfNotNull(t.album, formatClock(t.durationMs.toLong())).joinToString(dot), showDuration = false, nowPlayingMark = false, modifier = modifier)
    }
    when (val v = view) {
        ArtistView.TopSongs -> ArtistSubPage(ar.name, stringResource(R.string.artist_top_songs), null, onBack = { view = null }) {
            val list = top.orEmpty()
            itemsIndexed(list, key = { _, t -> "all" + t.id }) { i, t -> topRow(i, t, i to list.size, Modifier.segmentItem(i)) }
        }
        is ArtistView.Releases -> {
            val list = shelves.firstOrNull { it.first == v.kind }?.second.orEmpty()
            ArtistSubPage(ar.name, stringResource(releaseKindRes(v.kind)), pluralStringResource(releaseCountRes(v.kind), list.size, list.size), onBack = { view = null }, trailing = {
                ConnectedPair(stringResource(R.string.release_newest), stringResource(R.string.release_oldest), first = !oldest, onPick = { oldest = !it })
            }) {
                val ordered = if (oldest) list.reversed() else list
                items(ordered.chunked(2), key = { row -> "grid." + row.first().id }) { row ->
                    Row(Modifier.padding(start = 16.dp, end = 16.dp, bottom = 20.dp), horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                        row.forEach { al ->
                            BoxWithConstraints(Modifier.weight(1f)) { ReleaseTile(al, releaseMeta(al), onOpen = { openAlbum(al) }, onPlay = { playAlbum(al) }, size = maxWidth) }
                        }
                        if (row.size == 1) Spacer(Modifier.weight(1f))
                    }
                }
            }
        }
        null -> DetailScaffold(nav, embedded, ar.name, ar.coverArt, roundArtwork = true, shareKey = shareKey,
            barActions = { DetailLoveButton(ar.loved, onToggle = { client.dispatch(Commands.setArtistLoved(ar.id, it)) }) },
            below = { HeroLine(counts.ifEmpty { stringResource(R.string.library_count_albums, ar.albumCount.toInt()) }, Modifier.padding(top = 8.dp)) },
            heroActions = {
                HeroPlayButton(onClick = { client.dispatch(Commands.playContext(context)) })
                HeroShuffleButton(onClick = { client.dispatch(Commands.playContext(context, shuffle = true)) })
            },
        ) {
            val topSongs = top
            if (topSongs == null) {
                item(key = "h.top") { DetailSectionHead(stringResource(R.string.artist_top_songs), Modifier.padding(top = 20.dp)) }
                items(TOP_PER_COLUMN, key = { "top.skeleton$it" }, contentType = { "skeletonRow" }) { TrackRowSkeleton() }
            } else if (topSongs.isNotEmpty()) {
                item(key = "h.top") {
                    DetailSectionHead(stringResource(R.string.artist_top_songs), Modifier.padding(top = 20.dp)) {
                        if (topSongs.size > TOP_PER_COLUMN) ViewMoreButton(onClick = { view = ArtistView.TopSongs }, modifier = Modifier.testTag("artist.top.more"))
                    }
                }
                // Columns of three, paged sideways: the next column peeks in at the edge.
                item(key = "top") {
                    BoxWithConstraints(Modifier.fillMaxWidth()) {
                        val columnWidth = maxWidth - 72.dp
                        val rowState = androidx.compose.foundation.lazy.rememberLazyListState()
                        androidx.compose.foundation.lazy.LazyRow(
                            state = rowState,
                            contentPadding = PaddingValues(horizontal = 16.dp), horizontalArrangement = Arrangement.spacedBy(12.dp),
                            flingBehavior = androidx.compose.foundation.gestures.snapping.rememberSnapFlingBehavior(rowState),
                            modifier = Modifier.padding(top = 4.dp).testTag("artist.top"),
                        ) {
                            val columns = topSongs.withIndex().chunked(TOP_PER_COLUMN)
                            items(columns.size, key = { "topcol$it" }) { c ->
                                Column(Modifier.width(columnWidth), verticalArrangement = Arrangement.spacedBy(androidx.compose.material3.ListItemDefaults.SegmentedGap)) {
                                    columns[c].forEachIndexed { j, (i, t) -> topRow(i, t, j to columns[c].size, Modifier) }
                                }
                            }
                        }
                    }
                }
            }
            // The discography, one shelf per release kind (albums, EPs, singles, compilations).
            shelves.forEach { (kind, list) ->
                item(key = "h.${kind.string}") {
                    DetailSectionHead(stringResource(releaseKindRes(kind)), Modifier.padding(top = 24.dp)) {
                        if (list.size > SHELF) ViewMoreButton(onClick = { view = ArtistView.Releases(kind); oldest = false }, modifier = Modifier.testTag("artist.shelf.more.${kind.string}"))
                    }
                }
                item(key = "shelf.${kind.string}") { ReleaseShelf(list.take(SHELF), onOpen = openAlbum, onPlay = playAlbum, modifier = Modifier.padding(top = 4.dp)) { releaseMeta(it) } }
            }
            // About comes last: the server's biography (Last.fm).
            (ar.biography?.takeIf { it.isNotBlank() } ?: about?.takeIf { it.isNotBlank() })?.let { bio ->
                item(key = "bio") {
                    Column(Modifier.padding(start = 16.dp, end = 16.dp, top = 28.dp)) {
                        Text(stringResource(R.string.artist_biography), style = MaterialTheme.typography.titleLarge, fontWeight = FontWeight.Bold, modifier = Modifier.padding(bottom = 8.dp).semantics { heading() })
                        Text(bio, style = MaterialTheme.typography.bodyLarge)
                    }
                }
            }
        }
    }
}

/**
 * One of the artist page's own views (all top songs, a release kind's grid): the artist's name
 * over the view's title, a count, an optional control on the right, then [content].
 */
@Composable
private fun ArtistSubPage(artist: String, title: String, count: String?, onBack: () -> Unit, trailing: (@Composable () -> Unit)? = null, content: androidx.compose.foundation.lazy.LazyListScope.() -> Unit) {
    val listState = rememberLazyListState()
    val collapsed by remember { androidx.compose.runtime.derivedStateOf { listState.firstVisibleItemIndex > 0 } }
    Box(Modifier.fillMaxSize().background(MaterialTheme.colorScheme.background)) {
        androidx.compose.runtime.CompositionLocalProvider(androidx.compose.material3.LocalContentColor provides MaterialTheme.colorScheme.onBackground) {
            LazyColumn(state = listState, contentPadding = PaddingValues(bottom = BottomContentInset), modifier = Modifier.fillMaxSize().testTag("artist.subpage")) {
                item(key = "head") {
                    Column(Modifier.statusBarsPadding().padding(top = 72.dp, start = 16.dp, end = 16.dp, bottom = 16.dp)) {
                        Text(artist, style = MaterialTheme.typography.titleSmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
                        Row(verticalAlignment = Alignment.Bottom) {
                            Text(title, style = MaterialTheme.typography.displaySmall, fontWeight = FontWeight.Bold, modifier = Modifier.weight(1f).semantics { heading() })
                            trailing?.invoke()
                        }
                        if (count != null) Text(count, style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant, modifier = Modifier.padding(top = 4.dp))
                    }
                }
                content()
            }
            DetailTopBar(title, collapsed = collapsed, onBack = onBack, actions = {})
        }
    }
}

/** Two connected toggle buttons, one of which is on (Newest | Oldest). */
@Composable
private fun ConnectedPair(firstLabel: String, secondLabel: String, first: Boolean, onPick: (Boolean) -> Unit) {
    Row(horizontalArrangement = Arrangement.spacedBy(ButtonGroupDefaults.ConnectedSpaceBetween)) {
        ToggleButton(checked = first, onCheckedChange = { onPick(true) }, shapes = ButtonGroupDefaults.connectedLeadingButtonShapes(), modifier = Modifier.semantics { role = androidx.compose.ui.semantics.Role.RadioButton }) { Text(firstLabel) }
        ToggleButton(checked = !first, onCheckedChange = { onPick(false) }, shapes = ButtonGroupDefaults.connectedTrailingButtonShapes(), modifier = Modifier.semantics { role = androidx.compose.ui.semantics.Role.RadioButton }) { Text(secondLabel) }
    }
}

private fun releaseKindRes(kind: ReleaseKind): Int = when (kind) {
    ReleaseKind.Album -> R.string.artist_albums
    ReleaseKind.Ep -> R.string.artist_eps
    ReleaseKind.Single -> R.string.artist_singles
    ReleaseKind.Compilation -> R.string.artist_compilations
}

private fun releaseCountRes(kind: ReleaseKind): Int = when (kind) {
    ReleaseKind.Album -> R.plurals.release_count_albums
    ReleaseKind.Ep -> R.plurals.release_count_eps
    ReleaseKind.Single -> R.plurals.release_count_singles
    ReleaseKind.Compilation -> R.plurals.release_count_compilations
}

/** One playlist row: its track and a key that stays with it while the list is reordered. */
@androidx.compose.runtime.Immutable
internal data class PlaylistEntry(val key: String, val track: Track)

/** Keys by id plus occurrence ("id#0", "id#1" for a track listed twice), never by position. */
internal fun playlistEntries(tracks: List<Track>): List<PlaylistEntry> {
    val seen = HashMap<String, Int>()
    return tracks.map { t -> val n = seen.merge(t.id, 1, Int::plus)!! - 1; PlaylistEntry("${t.id}#$n", t) }
}

/**
 * Playlists: drag-reorder with haptics unless smart (read-only: no reorder, no manual add).
 *
 * A drag moves rows locally only; the one move from where the row was picked up to where it was
 * dropped goes to the core on release. Re-queries that land during a drag, or that started before
 * the last committed move, are dropped, so the list never jumps under the finger.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun PlaylistDetailScreen(nav: NavHostController, id: String, embedded: Boolean = false) {
    val client = LocalCoreClient.current
    val libraryGen by client.libraryGeneration.collectAsStateWithLifecycle()
    val shareKey = ArtworkShare.playlist(id)
    val flyingCover = flyingCoverArt(shareKey)
    var playlist by rememberCachedState<Playlist?>(listOf("detail.playlist", id)) { null }
    var entries by rememberCachedState<List<PlaylistEntry>?>(listOf("detail.playlistEntries", id)) { null }
    var dragKey by remember { mutableStateOf<String?>(null) }
    var dragFrom by remember { mutableIntStateOf(-1) }
    var commits by remember { mutableIntStateOf(0) }
    LaunchedEffect(id, libraryGen) {
        val startedAt = commits
        val pl = (client.query(Queries.playlist(id)) as? QueryResult.PlaylistDetail)?.data
        val t = (client.query(Queries.playlistTracks(id, Page(0u, 5000u))) as? QueryResult.Tracks)?.data?.items
        playlist = pl ?: playlist
        if (dragKey == null && commits == startedAt) entries = t?.let(::playlistEntries) ?: entries ?: emptyList()
    }
    val p = playlist
    val haptics = LocalHapticFeedback.current
    val nowPlaying by client.nowPlaying.collectAsStateWithLifecycle()
    val selection by client.selection.collectAsStateWithLifecycle()
    val kind by client.selectionKind.collectAsStateWithLifecycle()
    val selecting = selection.active && kind == SelectionKind.Tracks
    val songMenu = app.hocket.ui.components.rememberSongMenu()
    val resources = androidx.compose.ui.platform.LocalResources.current
    var playlistSheet by remember { mutableStateOf(false) }
    val listState = rememberLazyListState()
    val reorderable = rememberReorderableLazyListState(listState) { from, to ->
        val list = entries ?: return@rememberReorderableLazyListState
        val fromIdx = list.indexOfFirst { it.key == from.key }
        val toIdx = list.indexOfFirst { it.key == to.key }
        if (fromIdx >= 0 && toIdx >= 0) {
            entries = list.toMutableList().apply { add(toIdx, removeAt(fromIdx)) }
            haptics.performHapticFeedback(HapticFeedbackType.SegmentFrequentTick)
        }
    }
    fun commitDrag() {
        val key = dragKey ?: return
        val to = entries?.indexOfFirst { it.key == key } ?: -1
        if (p != null && dragFrom >= 0 && to >= 0 && to != dragFrom) {
            commits++
            client.dispatch(Commands.playlistMove(p.id, dragFrom, to))
        }
        dragKey = null
        dragFrom = -1
    }
    val pins by client.pins.collectAsStateWithLifecycle()
    // "Select" turns tapping into choosing before anything is chosen; the toolbar takes over from the first pick.
    var selectMode by remember(id) { mutableStateOf(false) }
    if (selecting) selectMode = false
    val choosing = selecting || selectMode
    androidx.activity.compose.BackHandler(enabled = selectMode) { selectMode = false }
    if (p == null) {
        DetailScaffold(nav, embedded, null, flyingCover, shareKey = shareKey, loading = true, listState = listState) {}
        return
    }
    val context = Commands.playlistContext(p.serverId, p.id, p.name)
    val target = Commands.playlists(listOf(p.id))
    val downloaded = pins.any { it.target == Commands.playlistPin(p.id) }
    val editable = !p.isSmart && p.isMine
    val dot = stringResource(R.string.dot_separator)
    DetailScaffold(nav, embedded, p.name, p.coverArt, shareKey = shareKey, listState = listState,
        barActions = {
            DetailDownloadButton(downloaded, onToggle = { client.dispatch(Commands.runAction(if (it) ActionIds.DOWNLOAD else ActionIds.UNPIN, target)) })
            DetailBarButton(HocketIcons.Filled.MoreVert, stringResource(R.string.action_more), onClick = { playlistSheet = true })
        },
        eyebrow = {
            val badges = listOfNotNull(
                stringResource(R.string.badge_smart).takeIf { p.isSmart },
                stringResource(R.string.badge_read_only).takeIf { p.isSmart || !p.isMine },
                stringResource(R.string.playlist_public).takeIf { p.public },
            )
            if (badges.isNotEmpty()) Row(Modifier.padding(bottom = 10.dp), horizontalArrangement = Arrangement.spacedBy(6.dp)) { badges.forEach { HeroBadge(it) } }
        },
        below = {
            p.owner?.let { HeroLine(it, style = MaterialTheme.typography.titleMedium, fontWeight = FontWeight.SemiBold, color = MaterialTheme.colorScheme.primary, maxLines = 1, modifier = Modifier.padding(top = 6.dp)) }
            HeroLine(stringResource(R.string.album_tracks_count, p.songCount.toInt(), formatDurationWords(p.durationMs.toLong())), Modifier.padding(top = 4.dp))
            p.comment?.takeIf { it.isNotBlank() }?.let { HeroLine(it, Modifier.padding(top = 6.dp), style = MaterialTheme.typography.bodyLarge, color = MaterialTheme.colorScheme.onSurface, maxLines = 4) }
            if (p.isSmart) HeroLine(stringResource(R.string.smart_playlist_note), Modifier.padding(top = 4.dp), style = MaterialTheme.typography.bodySmall)
        },
        heroActions = {
            HeroPlayButton(onClick = { client.dispatch(Commands.playContext(context)) })
            HeroShuffleButton(onClick = { client.dispatch(Commands.playContext(context, shuffle = true)) })
        },
        overlay = {
            SelectionToolbar(Modifier.align(Alignment.BottomCenter).padding(bottom = app.hocket.ui.nav.BottomOverlayInset), context = p.name,
                onRemoveFromContext = if (editable) ({
                    // The selected songs' entries, by position (highest first, so each index stays valid).
                    val sel = client.selection.value
                    val indices = entries.orEmpty().mapIndexedNotNull { i, e -> i.takeIf { sel.contains(e.track.id) } }.sortedDescending()
                    if (indices.isNotEmpty()) client.dispatch(Commands.playlistRemove(p.id, indices))
                }) else null)
        },
    ) {
        val list = entries
        item(key = "h.tracks") {
            DetailSectionHead(stringResource(R.string.detail_tracks), Modifier.padding(top = 16.dp)) {
                if (!list.isNullOrEmpty()) FilledTonalButton(onClick = { if (choosing) { client.clearSelection(); selectMode = false } else selectMode = true }, shapes = ButtonDefaults.shapes(), modifier = Modifier.testTag("playlist.select")) {
                    Icon(if (choosing) HocketIcons.Filled.Close else HocketIcons.Filled.SelectAll, null, Modifier.size(ButtonDefaults.IconSize))
                    Spacer(Modifier.width(ButtonDefaults.IconSpacing))
                    Text(stringResource(if (choosing) R.string.action_cancel else R.string.detail_select))
                }
            }
        }
        if (list == null) items(8, key = { "skeleton.row$it" }, contentType = { "skeletonRow" }) { TrackRowSkeleton() }
        else itemsIndexed(list, key = { _, e -> e.key }, contentType = { _, _ -> "track" }) { i, e ->
            val t = e.track
            ReorderableItem(reorderable, key = e.key, enabled = editable) { dragging ->
                TrackRow(t.toSummary(), onClick = { client.dispatch(Commands.playContext(context, startIndex = i)) }, onMore = {
                    // The playlist's own extra: remove this entry (by its position).
                    val extras = if (editable) listOf(app.hocket.ui.components.SongMenuExtra(resources.getString(R.string.menu_remove_from, p.name), HocketIcons.Filled.PlaylistRemove,
                        onClick = { client.dispatch(Commands.playlistRemove(p.id, listOf(i))) }, destructive = true, testTag = "songMenu.removeFromPlaylist",
                        replaces = ActionIds.REMOVE_FROM_PLAYLIST)) else emptyList()
                    songMenu.open(t.toSummary(), extras = extras, context = p.name)
                },
                    selected = selecting && selection.contains(t.id), selectionActive = choosing, onToggleSelect = { client.toggleSelected(SelectionKind.Tracks, t.id) },
                    nowPlaying = nowPlaying?.track?.id == t.id,
                    segment = i to list.size, subtitle = listOfNotNull(t.artist, formatClock(t.durationMs.toLong())).joinToString(dot), showDuration = false,
                    modifier = Modifier.segmentItem(i),
                    trailing = if (editable) ({
                        Icon(HocketIcons.Filled.DragHandle, stringResource(R.string.playlist_reorder_handle), tint = MaterialTheme.colorScheme.onSurfaceVariant,
                            modifier = Modifier.padding(horizontal = 12.dp).draggableHandle(
                                onDragStarted = {
                                    dragKey = e.key
                                    dragFrom = entries?.indexOfFirst { it.key == e.key } ?: -1
                                    haptics.performHapticFeedback(HapticFeedbackType.GestureThresholdActivate)
                                },
                                onDragStopped = { commitDrag(); haptics.performHapticFeedback(HapticFeedbackType.GestureEnd) },
                            ))
                    }) else null)
            }
        }
    }
    if (playlistSheet) ActionSheet(target, p.name, p.owner, onDismiss = { playlistSheet = false })
}

/** A small pill over a playlist's name (Public, Smart, Read-only). */
@Composable
private fun HeroBadge(text: String) {
    Surface(shape = CircleShape, color = app.hocket.ui.components.segmentColor()) {
        Text(text, style = MaterialTheme.typography.labelLarge, modifier = Modifier.padding(horizontal = 12.dp, vertical = 6.dp))
    }
}

@Composable
fun GenreDetailScreen(nav: NavHostController, name: String, embedded: Boolean = false) {
    val client = LocalCoreClient.current
    val server by client.server.collectAsStateWithLifecycle()
    val serverId = server?.id ?: return
    val libraryGen by client.libraryGeneration.collectAsStateWithLifecycle()
    var albums by rememberCachedState<List<Album>?>(listOf("detail.genreAlbums", serverId, name)) { null }
    var tracks by rememberCachedState<List<Track>?>(listOf("detail.genreTracks", serverId, name)) { null }
    LaunchedEffect(name, libraryGen) {
        val al = (client.query(Queries.albums(serverId, Page(0u, 200u), app.hocket.core.api.SortOrder.Year, genre = name)) as? QueryResult.Albums)?.data?.items
        val t = (client.query(Queries.tracks(serverId, Page(0u, 500u), app.hocket.core.api.SortOrder.Title, filter = app.hocket.core.api.FilterNode.Rule(app.hocket.core.api.FilterRule(app.hocket.core.api.FilterField.Genre, app.hocket.core.api.FilterOp.Is, app.hocket.core.api.FilterValue.Text(name))))) as? QueryResult.Tracks)?.data?.items
        albums = al ?: albums ?: emptyList()
        tracks = t ?: tracks ?: emptyList()
    }
    val context = Commands.genreContext(serverId, name)
    val songMenu = app.hocket.ui.components.rememberSongMenu()
    val al = albums
    val tr = tracks
    // The genre's name is known up front: the bar shows it at once, the rest loads under it.
    val settings by client.settings.collectAsStateWithLifecycle()
    val allPins = app.hocket.ui.screens.library.LibraryPins.parse(settings[app.hocket.core.SettingKeys.LIBRARY_PINS]?.value)
    val pinned = allPins.any { it.kind == "genre" && it.id == name }
    DetailScaffold(nav, embedded, name, al?.firstOrNull()?.coverArt, immersive = false, loading = al == null || tr == null, barActions = {
        // Genres have no registry menu: pinning is the genre page's own toggle.
        // A full Library can still unpin, not pin.
        DetailBarButton(if (pinned) HocketIcons.Filled.PushPin else HocketIcons.Outlined.PushPin, stringResource(if (pinned) R.string.library_pin_remove else R.string.library_pin_add),
            onClick = { app.hocket.ui.screens.library.LibraryPins.toggle(client, app.hocket.ui.screens.library.PinKind.Genre, name) }, enabled = pinned || app.hocket.ui.screens.library.LibraryPins.canPin(allPins), modifier = Modifier.testTag("genre.pin"))
    }, below = {
        tr?.let { HeroLine(stringResource(R.string.library_count_songs, it.size), Modifier.padding(top = 8.dp)) }
    }, heroActions = {
        HeroPlayButton(onClick = { client.dispatch(Commands.playContext(context)) })
        HeroShuffleButton(onClick = { client.dispatch(Commands.playContext(context, shuffle = true)) })
    }) {
        if (al == null || tr == null) return@DetailScaffold
        if (al.isEmpty() && tr.isEmpty()) {
            item(key = "empty") { EmptyState(stringResource(R.string.empty_library_title), stringResource(R.string.empty_library_body)) }
            return@DetailScaffold
        }
        if (al.isNotEmpty()) {
            item(key = "h.albums") { DetailSectionHead(stringResource(R.string.genre_albums), Modifier.padding(top = 20.dp)) }
            item(key = "albums") { ReleaseShelf(al, onOpen = { nav.navigate(Route.Album(it.id)) }, onPlay = { a -> client.dispatch(Commands.playContext(Commands.albumContext(a.serverId, a.id, a.name))) }, modifier = Modifier.padding(top = 4.dp)) { it.artist.orEmpty() } }
        }
        item(key = "h.songs") { DetailSectionHead(stringResource(R.string.genre_songs), Modifier.padding(top = 20.dp)) }
        itemsIndexed(tr, key = { _, t -> t.id }, contentType = { _, _ -> "track" }) { i, t ->
            TrackRow(t.toSummary(), onClick = { client.dispatch(Commands.playContext(context, startIndex = i)) }, onMore = { songMenu.open(t.toSummary()) }, segment = i to tr.size, modifier = Modifier.segmentItem(i))
        }
    }
}
