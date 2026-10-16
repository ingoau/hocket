package app.hocket.ui.screens.library

import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.background
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.grid.GridItemSpan
import androidx.compose.foundation.lazy.grid.LazyVerticalGrid
import androidx.compose.foundation.lazy.grid.rememberLazyGridState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.ExperimentalMaterial3ExpressiveApi
import androidx.compose.material3.FilledTonalIconButton
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialShapes
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.MediumFlexibleTopAppBar
import androidx.compose.material3.Scaffold
import androidx.compose.material3.SnackbarDuration
import androidx.compose.material3.SnackbarHost
import androidx.compose.material3.SnackbarHostState
import androidx.compose.material3.SnackbarResult
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBarDefaults
import androidx.compose.material3.toShape
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.runtime.getValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.hapticfeedback.HapticFeedbackType
import androidx.compose.ui.input.nestedscroll.nestedScroll
import androidx.compose.ui.platform.LocalHapticFeedback
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.CustomAccessibilityAction
import androidx.compose.ui.semantics.customActions
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.graphics.Shape
import androidx.compose.foundation.layout.offset
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.runtime.produceState
import app.hocket.core.Queries
import app.hocket.core.api.QueryResult
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.navigation.NavHostController
import app.hocket.R
import app.hocket.core.ArtworkSizes
import app.hocket.core.api.SortOrder
import app.hocket.core.client.AlbumListKey
import app.hocket.ui.LocalCoreClient
import app.hocket.ui.components.AlbumCard
import app.hocket.ui.components.AlbumCardSkeleton
import app.hocket.ui.components.Artwork
import app.hocket.ui.components.JobsIndicator
import app.hocket.ui.components.JobsSheet
import app.hocket.ui.components.ShimmerHost
import app.hocket.ui.icons.HocketIcons
import app.hocket.ui.nav.AccountButton
import app.hocket.ui.nav.BottomContentInset
import app.hocket.ui.nav.Route
import app.hocket.ui.nav.ScrollToTopOnReselect
import kotlinx.coroutines.launch

/** How many Recently added albums the Library lists before Albums takes over. */
private const val RECENT_ALBUMS = 60

/**
 * The Library: what's pinned (`library.pins`, synced) as two-column shortcuts, the places to
 * browse (each list, plus queues, downloads, filters and statistics), then Recently added as a grid
 * that keeps going as you scroll. A pin's tap opens it unless its menu (press and hold) says to play
 * or shuffle it instead; Edit takes pins out.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun LibraryScreen(nav: NavHostController) {
    val client = LocalCoreClient.current
    val server by client.server.collectAsStateWithLifecycle()
    val serverId = server?.id ?: return
    val pins = rememberResolvedPins()
    var editing by rememberSaveable { mutableStateOf(false) }
    var pinMenu by remember { mutableStateOf<ResolvedPin?>(null) }
    var jobs by remember { mutableStateOf(false) }
    val snackbar = remember { SnackbarHostState() }
    val scope = rememberCoroutineScope()
    val scroll = TopAppBarDefaults.enterAlwaysScrollBehavior()
    val unpinnedLabel = stringResource(R.string.library_unpinned, "%s")
    val undoLabel = stringResource(R.string.action_undo)
    fun unpinned(entry: PinEntry, at: Int, title: String) {
        scope.launch {
            if (snackbar.showSnackbar(unpinnedLabel.replace("%s", title), undoLabel, duration = SnackbarDuration.Short) == SnackbarResult.ActionPerformed) LibraryPins.restore(client, entry, at)
        }
    }
    LaunchedEffect(pins?.isEmpty()) { if (pins?.isEmpty() == true) editing = false }
    val key = remember(serverId) { AlbumListKey(serverId, SortOrder.DateAdded, true) }
    val recent by client.albumPages.state(key).collectAsStateWithLifecycle()
    LaunchedEffect(key, recent.generation) { client.albumPages.ensure(key, 0) }
    Scaffold(
        modifier = Modifier.nestedScroll(scroll.nestedScrollConnection).testTag("library.screen"),
        topBar = {
            MediumFlexibleTopAppBar(
                title = { Text(stringResource(R.string.nav_library), style = app.hocket.ui.brand.brandTitleStyle(), maxLines = 1, modifier = Modifier.semantics { heading() }) },
                actions = {
                    // No "Pinned" heading (the pins speak for themselves): editing them lives up here.
                    if (!pins.isNullOrEmpty()) TextButton(onClick = { editing = !editing }, modifier = Modifier.testTag("library.pins.edit")) {
                        Text(stringResource(if (editing) R.string.library_pins_done else R.string.library_pins_edit))
                    }
                    JobsIndicator(onClick = { jobs = true }); AccountButton()
                },
                scrollBehavior = scroll,
            )
        },
        snackbarHost = { SnackbarHost(snackbar, Modifier.padding(bottom = app.hocket.ui.nav.BottomOverlayInset)) },
    ) { padding ->
        val grid = rememberLazyGridState()
        ScrollToTopOnReselect(grid)
        ShimmerHost {
            LazyVerticalGrid(columns = AlbumGridCells, state = grid, contentPadding = albumGridPadding(padding.calculateTopPadding(), BottomContentInset), modifier = Modifier.fillMaxSize().testTag("library.grid")) {
                val full: androidx.compose.foundation.lazy.grid.LazyGridItemSpanScope.() -> GridItemSpan = { GridItemSpan(maxLineSpan) }
                item(key = "pinned", span = full, contentType = "pins") {
                    Box(Modifier.padding(horizontal = 6.dp)) {
                        when {
                            pins == null -> Spacer(Modifier.height(64.dp))
                            pins.isEmpty() -> Surface(shape = RoundedCornerShape(20.dp), color = MaterialTheme.colorScheme.surfaceContainer, modifier = Modifier.fillMaxWidth().testTag("library.pins.empty")) {
                                Text(stringResource(R.string.library_pin_empty), style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant, modifier = Modifier.padding(16.dp))
                            }
                            else -> Column(verticalArrangement = Arrangement.spacedBy(GROUP_GAP), modifier = Modifier.testTag("library.pins")) {
                                val rows = pins.chunked(2)
                                rows.forEachIndexed { r, row ->
                                    Row(horizontalArrangement = Arrangement.spacedBy(GROUP_GAP)) {
                                        row.forEachIndexed { c, pin ->
                                            // A pin with nothing under it (the last row, or above a lone last pin) takes the group's outer corner.
                                            val bottom = r == rows.lastIndex || (r == rows.lastIndex - 1 && c >= rows.last().size)
                                            PinCard(pin, editing, Modifier.weight(1f), groupShape(top = r == 0, bottom = bottom, start = c == 0, end = c == 1 || row.size == 1, outer = 20.dp),
                                                onTap = { performPin(client, nav, serverId, pin) },
                                                onMenu = { pinMenu = pin },
                                                onRemove = { val at = LibraryPins.remove(client, pin.entry); if (at >= 0) unpinned(pin.entry, at, pin.title) })
                                        }
                                        if (row.size == 1) Spacer(Modifier.weight(1f))
                                    }
                                }
                            }
                        }
                    }
                }
                item(key = "gap.browse", span = full, contentType = "gap") { Spacer(Modifier.height(16.dp)) }
                // Browse and the links are one connected group: the links carry on under the tiles.
                item(key = "browse", span = full, contentType = "browse") {
                    Column(verticalArrangement = Arrangement.spacedBy(GROUP_GAP)) { BrowseTiles(nav); LibraryLinks(nav, joinedAbove = true) }
                }
                item(key = "recent.h", span = full, contentType = "header") {
                    Row(Modifier.fillMaxWidth().padding(start = 6.dp, end = 6.dp, top = 24.dp, bottom = 4.dp), verticalAlignment = Alignment.Bottom) {
                        Text(stringResource(R.string.library_recently_added), style = MaterialTheme.typography.titleLarge, fontWeight = FontWeight.Bold, modifier = Modifier.weight(1f).semantics { heading() })
                        Text(stringResource(R.string.library_newest_first), style = MaterialTheme.typography.labelMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
                    }
                }
                val count = if (recent.known) minOf(recent.total, RECENT_ALBUMS) else 6
                items(count, key = { "recent.$it" }, contentType = { "album" }) { i ->
                    val album = recent.item(i, client.albumPages.pageSize)
                    if (recent.known) LaunchedEffect(i, recent.generation) { client.albumPages.ensure(key, i) }
                    if (album == null) AlbumCardSkeleton()
                    else AlbumCard(album, onClick = { nav.navigate(Route.Album(album.id)) }, modifier = Modifier.testTag("library.album"))
                }
            }
        }
    }
    pinMenu?.let { pin ->
        PinMenuSheet(pin, pins.orEmpty().map { it.entry }, nav, serverId, onUnpinned = { entry, at -> unpinned(entry, at, pin.title) }, onDismiss = { pinMenu = null })
    }
    if (jobs) JobsSheet { jobs = false }
}

/** One pin: shaped artwork, title and kind (and "Tap plays" when it does); Edit shows its remove button. */
@OptIn(ExperimentalFoundationApi::class)
@Composable
private fun PinCard(pin: ResolvedPin, editing: Boolean, modifier: Modifier, shape: Shape, onTap: () -> Unit, onMenu: () -> Unit, onRemove: () -> Unit) {
    val haptics = LocalHapticFeedback.current
    val more = stringResource(R.string.action_more)
    val removeLabel = stringResource(R.string.library_pin_remove)
    val a11y = remember(pin, more) { listOf(CustomAccessibilityAction(more) { onMenu(); true }) }
    val sub = buildString {
        append(stringResource(pin.kind.label))
        if (pin.kind.choosesTap) when (pin.entry.pinTap) {
            PinTap.Play -> append(" · ").append(stringResource(R.string.library_pin_plays))
            PinTap.Shuffle -> append(" · ").append(stringResource(R.string.library_pin_shuffles))
            PinTap.Open -> Unit
        }
    }
    Surface(
        shape = shape,
        color = if (editing) MaterialTheme.colorScheme.surfaceContainerLow else MaterialTheme.colorScheme.surfaceContainerHigh,
        modifier = modifier.heightIn(min = 64.dp).testTag("library.pin"),
    ) {
        Row(
            Modifier
                .combinedClickable(enabled = !editing, onClick = onTap, onLongClick = { haptics.performHapticFeedback(HapticFeedbackType.LongPress); onMenu() })
                .semantics(mergeDescendants = true) { if (!editing) customActions = a11y }
                .padding(8.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            PinArt(pin, Modifier.size(48.dp))
            Spacer(Modifier.width(10.dp))
            Column(Modifier.weight(1f)) {
                Text(pin.title, style = MaterialTheme.typography.labelLarge, maxLines = 1, overflow = TextOverflow.Ellipsis)
                Text(sub, style = MaterialTheme.typography.labelSmall, color = MaterialTheme.colorScheme.onSurfaceVariant, maxLines = 1, overflow = TextOverflow.Ellipsis)
            }
            if (editing) FilledTonalIconButton(onClick = onRemove, modifier = Modifier.size(40.dp).testTag("library.pin.remove")) {
                Icon(HocketIcons.Filled.Close, "$removeLabel: ${pin.title}", Modifier.size(18.dp))
            }
        }
    }
}

/** A pin's artwork in its kind's shape; genres and filters (no cover) draw their icon. */
@Composable
internal fun PinArt(pin: ResolvedPin, modifier: Modifier) {
    val shape = pinShape(pin.kind)
    when (pin.kind) {
        PinKind.Genre, PinKind.Filter -> Box(modifier.background(if (pin.kind == PinKind.Genre) MaterialTheme.colorScheme.tertiaryContainer else MaterialTheme.colorScheme.secondaryContainer, shape), contentAlignment = Alignment.Center) {
            Icon(if (pin.kind == PinKind.Genre) HocketIcons.Filled.Category else HocketIcons.Filled.FilterAlt, null, Modifier.size(22.dp),
                tint = if (pin.kind == PinKind.Genre) MaterialTheme.colorScheme.onTertiaryContainer else MaterialTheme.colorScheme.onSecondaryContainer)
        }
        else -> Artwork(pin.coverArt, ArtworkSizes.THUMB, null, modifier, shape)
    }
}

/** The places to browse: Albums (wide), Artists, Playlists, Songs and Genres, each in its own shape. */
@OptIn(ExperimentalMaterial3ExpressiveApi::class)
@Composable
private fun BrowseTiles(nav: NavHostController) {
    val client = LocalCoreClient.current
    // The Albums tile shows the covers of the three most played albums (this month, else this year).
    val topCovers by produceState(emptyList<String>()) {
        var stats = (client.query(Queries.stats(30)) as? QueryResult.Stats)?.data
        if (stats == null || stats.topAlbums.isEmpty()) stats = (client.query(Queries.stats(365)) as? QueryResult.Stats)?.data ?: stats
        value = stats?.topAlbums.orEmpty().mapNotNull { it.coverArt }.take(3)
    }
    @Composable fun Tile(label: String, icon: ImageVector, route: Route, tag: String, modifier: Modifier, shape: androidx.compose.ui.graphics.Shape, corners: Shape, wide: Boolean = false, covers: List<String> = emptyList(), height: Dp = 96.dp) {
        Surface(
            onClick = { nav.navigate(route) { launchSingleTop = true } },
            shape = corners,
            color = if (wide) MaterialTheme.colorScheme.primaryContainer else MaterialTheme.colorScheme.surfaceContainerHigh,
            contentColor = if (wide) MaterialTheme.colorScheme.onPrimaryContainer else MaterialTheme.colorScheme.onSurface,
            // The first row's tiles share one height, so the row's 3dp gap is even under both.
            modifier = modifier.height(height).testTag("library.browse.$tag"),
        ) {
          Box {
            if (covers.isNotEmpty()) AlbumFan(covers, Modifier.align(Alignment.CenterEnd).padding(end = 16.dp))
            Column(Modifier.padding(14.dp), verticalArrangement = Arrangement.SpaceBetween) {
                Box(Modifier.size(36.dp).background(if (wide) MaterialTheme.colorScheme.onPrimaryContainer.copy(alpha = 0.14f) else MaterialTheme.colorScheme.secondaryContainer, shape), contentAlignment = Alignment.Center) {
                    Icon(icon, null, Modifier.size(20.dp), tint = if (wide) MaterialTheme.colorScheme.onPrimaryContainer else MaterialTheme.colorScheme.onSecondaryContainer)
                }
                Spacer(Modifier.weight(1f))
                Text(label, style = if (wide) MaterialTheme.typography.titleMedium else MaterialTheme.typography.labelLarge, fontWeight = FontWeight.SemiBold)
            }
          }
        }
    }
    // The top of the Browse-and-links group: outer corners on the first row's ends only.
    val inner = groupShape(top = false, bottom = false, start = false, end = false)
    Column(Modifier.padding(horizontal = 6.dp), verticalArrangement = Arrangement.spacedBy(GROUP_GAP)) {
        Row(horizontalArrangement = Arrangement.spacedBy(GROUP_GAP)) {
            Tile(stringResource(R.string.library_albums), HocketIcons.Filled.Album, Route.Albums, "albums", Modifier.weight(2f), MaterialShapes.Cookie9Sided.toShape(), groupShape(top = true, bottom = false, start = true, end = false), wide = true, covers = topCovers, height = 112.dp)
            Tile(stringResource(R.string.library_artists), HocketIcons.Filled.Person, Route.Artists, "artists", Modifier.weight(1f), MaterialShapes.Circle.toShape(), groupShape(top = true, bottom = false, start = false, end = true), height = 112.dp)
        }
        Row(horizontalArrangement = Arrangement.spacedBy(GROUP_GAP)) {
            Tile(stringResource(R.string.library_playlists), HocketIcons.Filled.QueueMusic, Route.Playlists, "playlists", Modifier.weight(1f), MaterialShapes.Cookie9Sided.toShape(), inner)
            Tile(stringResource(R.string.library_songs), HocketIcons.Filled.MusicNote, Route.Songs, "songs", Modifier.weight(1f), MaterialShapes.Clover4Leaf.toShape(), inner)
            Tile(stringResource(R.string.library_genres), HocketIcons.Filled.Category, Route.Genres, "genres", Modifier.weight(1f), MaterialShapes.Flower.toShape(), inner)
        }
    }
}

/** The gap between tiles in a connected group. */
internal val GROUP_GAP = 3.dp

/**
 * A tile's shape in an M3 Expressive connected group: [outer] corners where the tile is on the
 * group's edge ([top], [bottom], [start], [end]), tight [inner] ones where it meets another tile.
 */
internal fun groupShape(top: Boolean, bottom: Boolean, start: Boolean, end: Boolean, outer: Dp = 24.dp, inner: Dp = 6.dp) = RoundedCornerShape(
    topStart = if (top && start) outer else inner,
    topEnd = if (top && end) outer else inner,
    bottomStart = if (bottom && start) outer else inner,
    bottomEnd = if (bottom && end) outer else inner,
)

/** Up to three album covers fanned out and overlapping, the first on top (the Albums tile's art). */
@Composable
private fun AlbumFan(covers: List<String>, modifier: Modifier = Modifier) {
    val size = 60.dp
    val step = 30.dp
    Box(modifier.width(size + step * (covers.size - 1)).height(size + 8.dp), contentAlignment = Alignment.CenterStart) {
        // Drawn back to front: the last cover first, so the first sits on top at the left.
        covers.withIndex().reversed().forEach { (i, cover) ->
            val angle = listOf(-8f, 4f, 12f).getOrElse(i) { 0f }
            app.hocket.ui.components.Artwork(
                cover, 128, null,
                shape = RoundedCornerShape(10.dp),
                modifier = Modifier.offset(x = step * i).size(size).graphicsLayer { rotationZ = angle; shadowElevation = 6f; shape = RoundedCornerShape(10.dp); clip = true },
            )
        }
    }
}
