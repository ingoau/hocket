package app.hocket.ui.screens.home

import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.background
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyRow
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.ExperimentalMaterial3ExpressiveApi
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialShapes
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Scaffold
import androidx.compose.material3.SnackbarDuration
import androidx.compose.material3.SnackbarHost
import androidx.compose.material3.SnackbarHostState
import androidx.compose.material3.SnackbarResult
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.MediumFlexibleTopAppBar
import androidx.compose.material3.TopAppBarDefaults
import app.hocket.ui.brand.HeaderLockup
import androidx.compose.material3.toShape
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.runtime.getValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.hapticfeedback.HapticFeedbackType
import androidx.compose.ui.input.nestedscroll.nestedScroll
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.ui.platform.LocalHapticFeedback
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.CustomAccessibilityAction
import androidx.compose.ui.semantics.customActions
import androidx.compose.ui.semantics.heading
import androidx.compose.foundation.layout.wrapContentHeight
import androidx.compose.ui.semantics.onLongClick
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.navigation.NavHostController
import app.hocket.R
import app.hocket.core.ArtworkSizes
import app.hocket.core.Commands
import app.hocket.core.SettingKeys
import app.hocket.core.api.Album
import app.hocket.core.api.Artist
import app.hocket.core.api.Filter
import app.hocket.core.api.SavedQueue
import app.hocket.core.api.TrackSummary
import app.hocket.ui.LocalCoreClient
import app.hocket.ui.rememberCachedState
import app.hocket.ui.components.ActionSheet
import app.hocket.ui.components.Artwork
import app.hocket.ui.components.EmptyState
import app.hocket.ui.components.GridTitleStyle
import app.hocket.ui.components.IconArt
import app.hocket.ui.components.JobsIndicator
import app.hocket.ui.components.JobsSheet
import app.hocket.ui.components.ShimmerHost
import app.hocket.ui.components.TrackRow
import app.hocket.ui.components.TrackRowSkeleton
import app.hocket.ui.components.SectionHeaderSkeleton
import app.hocket.ui.components.AlbumStripSkeleton
import app.hocket.ui.components.rememberSongMenu
import app.hocket.ui.icons.HocketIcons
import app.hocket.ui.nav.AccountButton
import app.hocket.ui.nav.ArtworkShare
import app.hocket.ui.nav.BottomContentInset
import app.hocket.ui.nav.Route
import app.hocket.ui.nav.ScrollToTopOnReselect
import app.hocket.ui.nav.rememberArtworkShareSource
import app.hocket.ui.screens.library.FilterMenuSheet
import app.hocket.ui.screens.library.rememberSettledGeneration
import kotlinx.coroutines.launch

/** Width of a Row-style cell (Navic's carousel width). */
private val CellWidth = 150.dp

/**
 * Home: the sections the user chose (`home.sections`, synced), each a header naming its topic over
 * a row, grid or list of albums, artists, songs, filters or queues. "Customise Home" (the top bar's
 * tune button, the button under the sections, or a section's own menu) switches sections on and
 * off, orders them and picks how each is shown.
 */
@OptIn(ExperimentalMaterial3Api::class, ExperimentalMaterial3ExpressiveApi::class)
@Composable
fun HomeScreen(nav: NavHostController) {
    val client = LocalCoreClient.current
    val server by client.server.collectAsStateWithLifecycle()
    val serverId = server?.id ?: return
    val settings by client.settings.collectAsStateWithLifecycle()
    val libraryGen = rememberSettledGeneration()
    val filters by client.filters.collectAsStateWithLifecycle()
    val saved by client.savedQueues.collectAsStateWithLifecycle()
    // A new song starting changes what "most played" and "lately" mean: re-read on track change.
    val nowPlayingId = client.nowPlaying.collectAsStateWithLifecycle().value?.track?.id
    val prefs = remember(settings[SettingKeys.HOME_SECTIONS]?.value) { HomeSections.parse(settings[SettingKeys.HOME_SECTIONS]?.value) }
    val shown = prefs.filter { it.on && it.section != null }
    val shownSet = shown.mapNotNull { it.section }.toSet()
    // Starts from the last feed, so coming back to Home (a pop from a page opened here) shows its
    // rows on the first frame — the artwork that flew out has somewhere to fly back to.
    var feed by rememberCachedState<Map<HomeSection, SectionContent>?>(listOf("home.feed", serverId)) { null }
    LaunchedEffect(serverId, libraryGen, shownSet, nowPlayingId) {
        if (libraryGen < 0) return@LaunchedEffect
        feed = loadHomeFeed(client, serverId, shownSet)
    }
    var customising by remember { mutableStateOf(false) }
    var sectionMenu by remember { mutableStateOf<SectionPref?>(null) }
    var jobs by remember { mutableStateOf(false) }
    val sheets = remember { HomeSheets() }
    val snackbar = remember { SnackbarHostState() }
    val scope = rememberCoroutineScope()
    val songMenu = rememberSongMenu()
    val scroll = TopAppBarDefaults.enterAlwaysScrollBehavior()
    val hiddenLabel = stringResource(R.string.home_section_hidden, "%s")
    val undoLabel = stringResource(R.string.action_undo)
    fun hide(pref: SectionPref, title: String) {
        val before = HomeSections.current(client)
        HomeSections.save(client, HomeSections.update(before, pref.id) { it.copy(on = false) })
        scope.launch {
            if (snackbar.showSnackbar(hiddenLabel.replace("%s", title), undoLabel, duration = SnackbarDuration.Short) == SnackbarResult.ActionPerformed) {
                HomeSections.save(client, HomeSections.update(HomeSections.current(client), pref.id) { it.copy(on = true) })
            }
        }
    }
    Scaffold(
        modifier = Modifier.nestedScroll(scroll.nestedScrollConnection).testTag("home.screen"),
        // The app's lockup as the title, at the Library header's size; the server is in the account sheet (#29).
        topBar = {
            MediumFlexibleTopAppBar(title = { HeaderLockup() }, actions = {
                IconButton(onClick = { customising = true }, modifier = Modifier.testTag("home.customise")) { Icon(HocketIcons.Filled.Tune, stringResource(R.string.home_customise)) }
                JobsIndicator(onClick = { jobs = true })
                AccountButton()
            }, scrollBehavior = scroll)
        },
        snackbarHost = { SnackbarHost(snackbar, Modifier.padding(bottom = app.hocket.ui.nav.BottomOverlayInset)) },
    ) { padding ->
        val loaded = feed
        val listState = rememberLazyListState()
        ScrollToTopOnReselect(listState)
        ShimmerHost {
            LazyColumn(Modifier.fillMaxSize(), state = listState, contentPadding = PaddingValues(top = padding.calculateTopPadding(), bottom = BottomContentInset)) {
                if (loaded == null) {
                    item(key = "sk.h1", contentType = "sectionSkeleton") { SectionHeaderSkeleton() }
                    item(key = "sk.s1", contentType = "stripSkeleton") { AlbumStripSkeleton() }
                    item(key = "sk.h2", contentType = "sectionSkeleton") { SectionHeaderSkeleton() }
                    items(4, key = { "sk.r$it" }, contentType = { "rowSkeleton" }) { TrackRowSkeleton() }
                    return@LazyColumn
                }
                var any = false
                shown.forEach { pref ->
                    val section = pref.section ?: return@forEach
                    val content = loaded[section] ?: return@forEach
                    val empty = when (section) {
                        HomeSection.Filters -> filters.isEmpty()
                        HomeSection.RecentQueues -> saved.isEmpty()
                        else -> content.isEmpty
                    }
                    if (empty) return@forEach
                    any = true
                    item(key = "h.${pref.id}", contentType = "sectionHeader") {
                        val title = sectionTitle(section, content)
                        HomeSectionHeader(title, Modifier.animateItem().testTag("home.section.${pref.id}"), onMenu = { sectionMenu = pref })
                    }
                    item(key = "s.${pref.id}", contentType = "section.${pref.resolvedStyle.id}") {
                        Box(Modifier.animateItem()) {
                            SectionBody(section, content, pref.resolvedStyle, filters, saved, nav, serverId, sheets, onSongMore = { songMenu.open(it) })
                        }
                    }
                }
                if (!any) {
                    item(key = "empty") {
                        EmptyState(stringResource(R.string.home_empty_sections_title), stringResource(R.string.home_empty_sections_body), actionLabel = stringResource(R.string.home_customise)) { customising = true }
                    }
                } else {
                    item(key = "customise") {
                        Box(Modifier.fillMaxWidth().padding(vertical = 16.dp), contentAlignment = Alignment.Center) {
                            OutlinedButton(onClick = { customising = true }, modifier = Modifier.testTag("home.customise.footer")) {
                                Icon(HocketIcons.Filled.Tune, null, Modifier.size(18.dp))
                                Spacer(Modifier.width(8.dp))
                                Text(stringResource(R.string.home_customise))
                            }
                        }
                    }
                }
            }
        }
    }
    if (customising) CustomiseHomeSheet(onDismiss = { customising = false })
    sectionMenu?.let { pref ->
        val section = pref.section
        if (section != null) {
            val title = feed?.get(section)?.let { sectionTitle(section, it) } ?: stringResource(section.title)
            SectionMenuSheet(pref, title, onDismiss = { sectionMenu = null }, onHide = { hide(pref, title) }, onCustomise = { customising = true })
        }
    }
    sheets.album?.let { a -> ActionSheet(Commands.albums(listOf(a.id)), a.name, a.artist, onDismiss = { sheets.album = null }, onGoToArtist = a.artistId?.let { id -> { nav.navigate(Route.Artist(id)) } }) }
    sheets.artist?.let { a -> ActionSheet(Commands.artists(listOf(a.id)), a.name, null, onDismiss = { sheets.artist = null }) }
    sheets.queue?.let { q -> ActionSheet(Commands.savedQueue(q.id), q.label, null, onDismiss = { sheets.queue = null }) }
    sheets.filter?.let { f -> FilterMenuSheet(f, nav, serverId, onDismiss = { sheets.filter = null }) }
    if (jobs) JobsSheet { jobs = false }
}

/** The item menus Home can have open (one at a time). */
class HomeSheets {
    var album by mutableStateOf<Album?>(null)
    var artist by mutableStateOf<Artist?>(null)
    var queue by mutableStateOf<SavedQueue?>(null)
    var filter by mutableStateOf<Filter?>(null)
}

/** A section's title: its topic where it has one ("More from Theo Lund", "More Indie rock"). */
@Composable
private fun sectionTitle(section: HomeSection, content: SectionContent): String = when (section) {
    HomeSection.TopGenre -> content.title?.let { stringResource(R.string.home_section_top_genre_named, it) } ?: stringResource(section.title)
    HomeSection.MoreFromArtist -> content.title?.let { stringResource(R.string.home_section_more_from_named, it) } ?: stringResource(section.title)
    else -> stringResource(section.title)
}

/**
 * A section's header: its title. Long-press opens the section's menu (TalkBack offers it as the
 * long-press action); everything in it is also in Customise Home, at the end of the feed.
 */
@Composable
private fun HomeSectionHeader(title: String, modifier: Modifier = Modifier, onMenu: () -> Unit) {
    val haptics = LocalHapticFeedback.current
    val options = stringResource(R.string.home_section_options, title)
    Text(
        title,
        style = MaterialTheme.typography.titleLarge,
        fontWeight = FontWeight.Bold,
        maxLines = 1,
        overflow = TextOverflow.Ellipsis,
        modifier = modifier.fillMaxWidth()
            .pointerInput(onMenu) { detectTapGestures(onLongPress = { haptics.performHapticFeedback(HapticFeedbackType.LongPress); onMenu() }) }
            .semantics { heading(); onLongClick(label = options) { onMenu(); true } }
            .heightIn(min = 56.dp).padding(start = 16.dp, end = 16.dp, top = 20.dp, bottom = 8.dp)
            .wrapContentHeight(Alignment.Bottom),
    )
}

/** One section's items in its style. */
@Composable
private fun SectionBody(
    section: HomeSection,
    content: SectionContent,
    style: SectionStyle,
    filters: List<Filter>,
    saved: List<SavedQueue>,
    nav: NavHostController,
    serverId: String,
    sheets: HomeSheets,
    onSongMore: (TrackSummary) -> Unit,
) {
    val client = LocalCoreClient.current
    val items: List<HomeItem> = when {
        section == HomeSection.Filters -> filters.take(SECTION_ITEMS).map { HomeItem.FilterItem(it) }
        section == HomeSection.RecentQueues -> saved.take(SECTION_ITEMS).map { HomeItem.QueueItem(it) }
        content is SectionContent.Albums -> content.items.map { HomeItem.AlbumItem(it) }
        content is SectionContent.Artists -> content.items.map { HomeItem.ArtistItem(it) }
        content is SectionContent.Tracks -> content.items.mapIndexed { i, t -> HomeItem.TrackItem(t, content.items, i) }
        else -> emptyList()
    }
    val label = stringResource(section.title)
    val actions = remember(items, serverId) {
        HomeItemActions(
            open = { item ->
                when (item) {
                    is HomeItem.AlbumItem -> nav.navigate(Route.Album(item.album.id))
                    is HomeItem.ArtistItem -> nav.navigate(Route.Artist(item.artist.id))
                    is HomeItem.TrackItem -> client.dispatch(Commands.playTracks(serverId, item.all.map { it.id }, item.index, label))
                    is HomeItem.FilterItem -> client.dispatch(Commands.playContext(Commands.filterContext(serverId, item.filter)))
                    is HomeItem.QueueItem -> client.dispatch(Commands.restoreSavedQueue(item.queue.id))
                }
            },
            menu = { item ->
                when (item) {
                    is HomeItem.AlbumItem -> sheets.album = item.album
                    is HomeItem.ArtistItem -> sheets.artist = item.artist
                    is HomeItem.TrackItem -> onSongMore(item.track)
                    is HomeItem.FilterItem -> sheets.filter = item.filter
                    is HomeItem.QueueItem -> sheets.queue = item.queue
                }
            },
        )
    }
    when (style) {
        SectionStyle.Tile -> (content as? SectionContent.Listening)?.let { ListeningTile(it) { nav.navigate(Route.Stats) } }
        SectionStyle.Row -> LazyRow(contentPadding = PaddingValues(horizontal = 16.dp), horizontalArrangement = Arrangement.spacedBy(12.dp)) {
            items(items, key = { it.key }) { HomeCell(it, actions, Modifier.width(if (it is HomeItem.ArtistItem) 124.dp else CellWidth)) }
        }
        SectionStyle.Grid -> Column(Modifier.padding(horizontal = 16.dp), verticalArrangement = Arrangement.spacedBy(16.dp)) {
            items.take(6).chunked(2).forEach { row ->
                Row(horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                    row.forEach { HomeCell(it, actions, Modifier.weight(1f)) }
                    if (row.size == 1) Spacer(Modifier.weight(1f))
                }
            }
        }
        SectionStyle.List -> Column {
            items.take(8).forEach { item ->
                if (item is HomeItem.TrackItem) TrackRow(item.track, onClick = { actions.open(item) }, onMore = { actions.menu(item) })
                else HomeListRow(item, actions)
            }
        }
    }
}

/** Something a Home section shows. */
sealed interface HomeItem {
    val key: String
    data class AlbumItem(val album: Album) : HomeItem { override val key get() = "al:" + album.id }
    data class ArtistItem(val artist: Artist) : HomeItem { override val key get() = "ar:" + artist.id }
    data class TrackItem(val track: TrackSummary, val all: List<TrackSummary>, val index: Int) : HomeItem { override val key get() = "t:$index:" + track.id }
    data class FilterItem(val filter: Filter) : HomeItem { override val key get() = "f:" + filter.id }
    data class QueueItem(val queue: SavedQueue) : HomeItem { override val key get() = "q:" + queue.id }
}

class HomeItemActions(val open: (HomeItem) -> Unit, val menu: (HomeItem) -> Unit)

private data class CellText(val title: String, val subtitle: String)

@Composable
private fun cellText(item: HomeItem): CellText = when (item) {
    is HomeItem.AlbumItem -> CellText(item.album.name, item.album.artist ?: stringResource(R.string.unknown_artist))
    is HomeItem.ArtistItem -> CellText(item.artist.name, stringResource(R.string.library_count_albums, item.artist.albumCount.toInt()))
    is HomeItem.TrackItem -> CellText(item.track.title, item.track.artist ?: stringResource(R.string.unknown_artist))
    is HomeItem.FilterItem -> CellText(item.filter.name, stringResource(R.string.kind_filter))
    is HomeItem.QueueItem -> CellText(item.queue.label, stringResource(R.string.saved_tracks, item.queue.trackCount.toInt()))
}

@OptIn(ExperimentalMaterial3ExpressiveApi::class)
@Composable
private fun itemShape(item: HomeItem) = when (item) {
    is HomeItem.ArtistItem -> CircleShape
    is HomeItem.FilterItem -> MaterialShapes.Clover4Leaf.toShape()
    is HomeItem.QueueItem -> MaterialShapes.Cookie9Sided.toShape()
    else -> RoundedCornerShape(16.dp)
}

private fun coverOf(item: HomeItem): String? = when (item) {
    is HomeItem.AlbumItem -> item.album.coverArt
    is HomeItem.ArtistItem -> item.artist.coverArt
    is HomeItem.TrackItem -> item.track.coverArt
    is HomeItem.QueueItem -> item.queue.coverArt
    is HomeItem.FilterItem -> null
}

private fun shareKeyOf(item: HomeItem): String? = when (item) {
    is HomeItem.AlbumItem -> ArtworkShare.album(item.album.id)
    is HomeItem.ArtistItem -> ArtworkShare.artist(item.artist.id)
    else -> null
}

/** The icon drawn in place of artwork for what has none: filters, and queues without a cover. */
private fun iconOf(item: HomeItem): androidx.compose.ui.graphics.vector.ImageVector? = when {
    item is HomeItem.FilterItem -> HocketIcons.Filled.FilterAlt
    item is HomeItem.QueueItem && item.queue.coverArt == null -> HocketIcons.AutoMirrored.Filled.QueueMusic
    else -> null
}

/** The menu as a screen-reader action, so nobody needs the long-press. */
@Composable
private fun menuAction(item: HomeItem, actions: HomeItemActions): List<CustomAccessibilityAction> {
    val more = stringResource(R.string.action_more)
    return remember(item, actions) { listOf(CustomAccessibilityAction(more) { actions.menu(item); true }) }
}

/** A Row or Grid cell: artwork (its shape says what it is), title and subtitle; long-press for its menu. */
@OptIn(ExperimentalFoundationApi::class)
@Composable
private fun HomeCell(item: HomeItem, actions: HomeItemActions, modifier: Modifier = Modifier) {
    val haptics = LocalHapticFeedback.current
    val text = cellText(item)
    val shape = itemShape(item)
    val round = item is HomeItem.ArtistItem
    val a11y = menuAction(item, actions)
    val share = rememberArtworkShareSource(shareKeyOf(item), coverOf(item))
    Column(
        modifier
            .combinedClickable(onClick = { share.prepare(); actions.open(item) }, onLongClick = { haptics.performHapticFeedback(HapticFeedbackType.LongPress); actions.menu(item) })
            .semantics(mergeDescendants = true) { customActions = a11y }
            .testTag("home.item"),
        horizontalAlignment = if (round) Alignment.CenterHorizontally else Alignment.Start,
    ) {
        val icon = iconOf(item)
        if (icon != null) IconArt(icon, shape, Modifier.fillMaxWidth().aspectRatio(1f), iconSize = 40.dp)
        else Artwork(coverOf(item), ArtworkSizes.GRID, null, Modifier.fillMaxWidth().aspectRatio(1f).then(share.modifier), shape)
        Spacer(Modifier.height(8.dp))
        // One line each, so the subtitle sits right under the title and every cell's caption is as tall.
        val align = if (round) TextAlign.Center else TextAlign.Start
        Text(text.title, style = GridTitleStyle, maxLines = 1, overflow = TextOverflow.Ellipsis, textAlign = align)
        Text(text.subtitle, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant, maxLines = 1, overflow = TextOverflow.Ellipsis, textAlign = align)
    }
}

/** A List-style row for anything but a song (songs use the app's track row). */
@OptIn(ExperimentalFoundationApi::class)
@Composable
private fun HomeListRow(item: HomeItem, actions: HomeItemActions) {
    val haptics = LocalHapticFeedback.current
    val text = cellText(item)
    val shape = itemShape(item)
    val a11y = menuAction(item, actions)
    val share = rememberArtworkShareSource(shareKeyOf(item), coverOf(item))
    Row(
        Modifier.fillMaxWidth()
            .combinedClickable(onClick = { share.prepare(); actions.open(item) }, onLongClick = { haptics.performHapticFeedback(HapticFeedbackType.LongPress); actions.menu(item) })
            .semantics(mergeDescendants = true) { customActions = a11y }
            .heightIn(min = 64.dp).padding(start = 16.dp, end = 4.dp, top = 8.dp, bottom = 8.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        val icon = iconOf(item)
        if (icon != null) IconArt(icon, shape, Modifier.size(48.dp))
        else Artwork(coverOf(item), ArtworkSizes.THUMB, null, Modifier.size(48.dp).then(share.modifier), shape)
        Spacer(Modifier.width(14.dp))
        Column(Modifier.weight(1f)) {
            Text(text.title, style = MaterialTheme.typography.bodyLarge, maxLines = 1, overflow = TextOverflow.Ellipsis)
            Text(text.subtitle, style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant, maxLines = 1, overflow = TextOverflow.Ellipsis)
        }
        IconButton(onClick = { actions.menu(item) }) { Icon(HocketIcons.Filled.MoreVert, stringResource(R.string.action_more), tint = MaterialTheme.colorScheme.onSurfaceVariant) }
    }
}

/** The listening tile: time this week, a bar per day, and the way to the full statistics. */
@Composable
private fun ListeningTile(content: SectionContent.Listening, onOpen: () -> Unit) {
    val minutes = (content.totalMs / 60_000).toLong()
    val time = if (minutes >= 60) stringResource(R.string.home_stats_hours, (minutes / 60).toInt(), (minutes % 60).toInt()) else stringResource(R.string.home_stats_minutes, minutes.toInt())
    Surface(onClick = onOpen, shape = RoundedCornerShape(28.dp), color = MaterialTheme.colorScheme.surfaceContainerHigh, modifier = Modifier.fillMaxWidth().padding(horizontal = 16.dp)) {
        Row(Modifier.padding(20.dp), verticalAlignment = Alignment.CenterVertically) {
            Column(Modifier.weight(1f)) {
                Text(time, style = MaterialTheme.typography.headlineMedium, fontWeight = FontWeight.Bold)
                Text(stringResource(R.string.home_stats_listened), style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
                Spacer(Modifier.height(8.dp))
                Text(stringResource(R.string.home_stats_open), style = MaterialTheme.typography.labelLarge, color = MaterialTheme.colorScheme.primary)
            }
            val max = (content.byWeekday.maxOrNull() ?: 0).coerceAtLeast(1)
            Row(Modifier.height(56.dp), horizontalArrangement = Arrangement.spacedBy(6.dp), verticalAlignment = Alignment.Bottom) {
                content.byWeekday.forEach { n ->
                    Box(Modifier.width(10.dp).fillMaxHeight((n.toFloat() / max).coerceAtLeast(0.06f)).background(MaterialTheme.colorScheme.primary.copy(alpha = 0.75f), RoundedCornerShape(5.dp)))
                }
            }
        }
    }
}
