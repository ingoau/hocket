package app.hocket.ui.screens.home

import app.hocket.ui.components.SyncRefreshBox
import app.hocket.ui.components.rememberSyncRefresh
import app.hocket.ui.components.syncRefresh
import app.hocket.ui.theme.pressScale
import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.background
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.ui.draw.drawWithCache
import androidx.compose.ui.geometry.Size
import app.hocket.ui.nav.easedScrim
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
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.TopAppBarDefaults
import app.hocket.ui.brand.HeaderMark
import app.hocket.ui.brand.MarkWipe
import androidx.compose.ui.layout.onGloballyPositioned
import androidx.compose.ui.layout.positionInWindow
import kotlin.math.floor
import androidx.compose.ui.platform.LocalLayoutDirection
import androidx.compose.ui.unit.LayoutDirection
import androidx.compose.material3.toShape
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.derivedStateOf
import androidx.compose.runtime.key
import androidx.compose.foundation.pager.rememberPagerState
import androidx.compose.material3.MaterialExpressiveTheme
import androidx.compose.ui.graphics.Color
import app.hocket.ui.a11y.LocalReducedMotion
import app.hocket.ui.player.artworkSeed
import app.hocket.ui.theme.ArtworkColors
import app.hocket.ui.theme.LocalDarkTheme
import app.hocket.ui.theme.animatedScheme
import app.hocket.ui.brand.Brand
import app.hocket.ui.nav.PageColors
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.runtime.getValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.hapticfeedback.HapticFeedbackType
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.ui.platform.LocalHapticFeedback
import androidx.compose.ui.platform.testTag
import app.hocket.ui.components.adaptiveColumns
import app.hocket.ui.components.contentMaxWidth
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
import app.hocket.core.api.Playlist
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
    val playing = client.nowPlaying.collectAsStateWithLifecycle().value?.track
    val nowPlayingId = playing?.id
    val resumePoints = client.albumResumes.collectAsStateWithLifecycle().value
    // Spotlight's Resume slide: re-read when an album gains or loses its resume point.
    val resumeAlbums = resumePoints.map { it.albumId }.toSet()
    val prefs = remember(settings[SettingKeys.HOME_SECTIONS]?.value) { HomeSections.parse(settings[SettingKeys.HOME_SECTIONS]?.value) }
    val shown = prefs.filter { it.on && it.section != null }
    val shownSet = shown.mapNotNull { it.section }.toSet()
    // Starts from the last feed, so coming back to Home (a pop from a page opened here) shows its
    // rows on the first frame — the artwork that flew out has somewhere to fly back to.
    var feed by rememberCachedState<Map<HomeSection, SectionContent>?>(listOf("home.feed", serverId)) { null }
    val feedSections = shownSet - HomeSection.Spotlight
    // The sections read so far on a cold load (no feed yet), for a reveal that can't wait any longer.
    var partial by remember(serverId) { mutableStateOf<Map<HomeSection, SectionContent>>(emptyMap()) }
    LaunchedEffect(serverId, libraryGen, feedSections, nowPlayingId) {
        if (libraryGen < 0) return@LaunchedEffect
        // Merged, not replaced: a reload mid-cold-load (a track change, a sync) keeps what's revealed.
        feed = loadHomeFeed(client, serverId, feedSections) { if (feed == null) partial = partial + it }
    }
    // Spotlight loads on its own: on arriving at Home, when another album starts, and when the
    // library or the albums to resume change; not per track. What's playing as it loads is left
    // out, except from the slide in view: Play on a slide keeps it under the user's thumb.
    val spotlightOn = HomeSection.Spotlight in shownSet
    var spotlight by rememberCachedState<List<SpotlightSlide>?>(listOf("home.spotlight", serverId)) { null }
    val shownSlide = remember { mutableStateOf<SpotlightSlide?>(null) }
    LaunchedEffect(serverId, libraryGen, spotlightOn, resumeAlbums, playing?.albumId) {
        if (libraryGen < 0 || !spotlightOn) return@LaunchedEffect
        val track = client.nowPlaying.value?.track
        val inView = shownSlide.value
        val fromInView = track != null && inView != null && (inView.album?.id == track.albumId || (inView.album == null && inView.artist?.id == track.artistId))
        spotlight = loadSpotlight(client, serverId, client.albumResumes.value, track.takeUnless { fromInView }, favouriteStats(client), spotlightSeed())
    }
    // Spotlight comes first whatever the stored order (it's pinned), drawn without a header. A
    // Resume slide shows the album's latest point (positions save without reloading).
    val latestResume = resumePoints.associateBy { it.albumId }
    val slides = spotlight.orEmpty().takeIf { spotlightOn }.orEmpty().map { s -> s.resume?.let { latestResume[it.albumId] }?.let { s.copy(resume = it) } ?: s }
    val pager = rememberPagerState { slides.size }
    shownSlide.value = slides.getOrNull(pager.currentPage)
    val dark = LocalDarkTheme.current
    // Each slide's artwork seed, and whether the top of its cover (under the bar) is light; until
    // it's been read, the slide shows the page's own surface: the theme's ink.
    val slideLooks = slides.map { key(it.key) { artworkSeed(it.coverArt).value to (artworkTopLight(it.coverArt).value ?: !dark) } }
    val slideSeeds = slideLooks.map { it.first }
    val slideInks = slideLooks.map { if (it.second) Brand.inkOnLight else Brand.inkOnDark }
    // The slides, and the rest of Home, take the colours of the slide in view (as the app does from
    // what's playing), unless colour from artwork is off.
    val dynamicColour = settings[SettingKeys.DISPLAY_DYNAMIC_COLOUR]?.value?.trim() != "false"
    val appScheme = MaterialTheme.colorScheme
    val slideSchemes = remember(slides, slideSeeds, dark, dynamicColour, appScheme) {
        slides.mapIndexed { i, s -> if (dynamicColour) ArtworkColors.scheme(slideSeeds[i] ?: ArtworkColors.seedFor(s.coverArt), dark) else appScheme }
    }
    val pageScheme = slideSchemes.getOrNull(pager.currentPage) ?: appScheme
    // A cold load reveals Spotlight with the rest, so the carousel and the sections come in together.
    val reveal = rememberHomeReveal(serverId, ready = { feed != null && (!spotlightOn || spotlight != null) })
    val heroFirst = slides.isNotEmpty() && reveal.phase == RevealPhase.Shown
    var customising by remember { mutableStateOf(false) }
    var sectionMenu by remember { mutableStateOf<SectionPref?>(null) }
    var jobs by remember { mutableStateOf(false) }
    val sheets = remember { HomeSheets() }
    val snackbar = remember { SnackbarHostState() }
    val scope = rememberCoroutineScope()
    val songMenu = rememberSongMenu()
    // Read from the list itself (not a scroll behaviour's nested-scroll offset), so a programmatic
    // scroll back to the top (reselecting Home) clears it too. The bar never moves.
    val listState = rememberLazyListState()
    val scrolled by remember(listState) { derivedStateOf { listState.firstVisibleItemIndex > 0 || listState.firstVisibleItemScrollOffset > 0 } }
    // Over the cover (Spotlight first, the page not scrolled under the bar) the bar's mark, icons
    // and the status bar take the ink that reads on the top of the slide in view.
    val overArt = heroFirst && !scrolled
    val topLight = slideInks.getOrNull(pager.currentPage)?.let { it == Brand.inkOnLight } ?: !dark
    val barInk = slideInks.getOrNull(pager.currentPage) ?: Brand.inkOnDark
    // The mark takes each slide's ink where that slide is behind it: split at the edge between the
    // two slides in view, read from the pager's live offset, so it wipes across with the finger.
    val spotlightLeft = remember { mutableFloatStateOf(0f) }
    val rtl = LocalLayoutDirection.current == LayoutDirection.Rtl
    val markWipe: () -> MarkWipe? = {
        val info = pager.layoutInfo
        val step = info.pageSize + info.pageSpacing
        if (step <= 0 || slideInks.isEmpty()) null else {
            val p = pager.currentPage + pager.currentPageOffsetFraction
            val i = floor(p).toInt().coerceIn(0, slideInks.lastIndex)
            val here = slideInks[i]
            val next = slideInks.getOrElse(i + 1) { here }
            // Page i+1 comes in from the right, or from the left in RTL. The pager is one page wide.
            if (rtl) MarkWipe(spotlightLeft.floatValue + (p - i) * step, next, here)
            else MarkWipe(spotlightLeft.floatValue + (i + 1 - p) * step, here, next)
        }
    }
    // Only with Spotlight showing: otherwise Home leaves the chrome and status bar to the app theme, as before.
    if (heroFirst) PageColors(pageScheme, statusLight = if (overArt) topLight else !dark)
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
    MaterialExpressiveTheme(colorScheme = animatedScheme(pageScheme, LocalReducedMotion.current), motionScheme = MaterialTheme.motionScheme, typography = MaterialTheme.typography) {
    CompositionLocalProvider(androidx.compose.material3.LocalContentColor provides MaterialTheme.colorScheme.onBackground) {
    val refresh = rememberSyncRefresh()
    Scaffold(
        modifier = Modifier.syncRefresh(refresh).testTag("home.screen"),
        // The app's mark as the title, level with the actions; the server is in the account sheet (#29).
        // Clear over Spotlight's artwork. Once the page scrolls under it, a scrim in the page's surface
        // colour fades in behind it, solid at the top and easing out below the bar, not a solid block.
        topBar = {
            val clear = Color.Transparent
            val barColors = if (overArt) {
                TopAppBarDefaults.topAppBarColors(containerColor = clear, scrolledContainerColor = clear, actionIconContentColor = barInk, navigationIconContentColor = barInk, titleContentColor = barInk)
            } else {
                TopAppBarDefaults.topAppBarColors(containerColor = clear, scrolledContainerColor = clear)
            }
            val scrimAlpha by animateFloatAsState(if (scrolled) 1f else 0f, MaterialTheme.motionScheme.defaultEffectsSpec(), label = "homeBarScrim")
            val scrimColor = MaterialTheme.colorScheme.surface
            Box(Modifier.drawWithCache {
                // Solid over the bar (status bar included) down to just under its icons, then a long, soft
                // fade past its bottom edge.
                val solidEnd = (size.height - HomeBarScrimFadeStart.toPx()).coerceAtLeast(0f)
                val reach = solidEnd + HomeBarScrimFade.toPx()
                val scrim = easedScrim(scrimColor, stops = 32, solid = solidEnd / reach, fromTop = true, endY = reach)
                onDrawBehind { if (scrimAlpha > 0f) drawRect(scrim, size = Size(size.width, reach), alpha = scrimAlpha) }
            }.testTag("home.barScrim")) {
            TopAppBar(title = { if (overArt) HeaderMark(wipe = markWipe) else HeaderMark() }, colors = barColors, actions = {
                IconButton(onClick = { customising = true }, modifier = Modifier.testTag("home.customise")) { Icon(HocketIcons.Filled.Tune, stringResource(R.string.home_customise)) }
                JobsIndicator(onClick = { jobs = true })
                AccountButton()
            })
            }
        },
        snackbarHost = { SnackbarHost(snackbar, Modifier.padding(bottom = app.hocket.ui.nav.BottomOverlayInset)) },
    ) { padding ->
        // Nothing until the reveal says so, even a feed that's in (the skeleton's minimum, the entrance).
        val loaded = if (reveal.phase == RevealPhase.Shown) feed ?: partial else null
        ScrollToTopOnReselect(listState)
        ShimmerHost {
            SyncRefreshBox(refresh, topInset = padding.calculateTopPadding()) {
                LazyColumn(Modifier.fillMaxSize().testTag("home.list"), state = listState, contentPadding = PaddingValues(top = if (heroFirst) 0.dp else padding.calculateTopPadding(), bottom = BottomContentInset)) {
                    // Nothing yet: blank at first (most loads land before a skeleton would be worth showing).
                    if (reveal.phase == RevealPhase.Waiting) return@LazyColumn
                    if (loaded == null) {
                        item(key = "sk.h1", contentType = "sectionSkeleton") { SectionHeaderSkeleton() }
                        item(key = "sk.s1", contentType = "stripSkeleton") { AlbumStripSkeleton() }
                        item(key = "sk.h2", contentType = "sectionSkeleton") { SectionHeaderSkeleton() }
                        items(4, key = { "sk.r$it" }, contentType = { "rowSkeleton" }) { TrackRowSkeleton() }
                        return@LazyColumn
                    }
                    var any = false
                    var order = 0
                    if (heroFirst) {
                        any = true
                        val step = order++
                        item(key = "spotlight", contentType = "spotlight") {
                            SpotlightCarousel(
                                slides, pager, slideSchemes,
                                onOpen = { s -> openSlide(nav, s) },
                                onPlay = { s, shuffle -> playSlide(client, serverId, s, shuffle) },
                                onMenu = { s -> s.album?.let { sheets.album = it } ?: s.artist?.let { sheets.artist = it } ?: s.playlist?.let { sheets.playlist = it } },
                                modifier = Modifier.homeEntrance(reveal, step).onGloballyPositioned { spotlightLeft.floatValue = it.positionInWindow().x },
                            )
                        }
                    }
                    shown.forEach { pref ->
                        val section = pref.section ?: return@forEach
                        if (section == HomeSection.Spotlight) return@forEach
                        val content = loaded[section]
                        if (content == null) {
                            // Revealed at the cap before this one was read: hold its place until it is.
                            if (feed == null) {
                                any = true
                                item(key = "sk.h.${pref.id}", contentType = "sectionSkeleton") { SectionHeaderSkeleton(Modifier.animateItem()) }
                                item(key = "sk.s.${pref.id}", contentType = "stripSkeleton") { AlbumStripSkeleton(Modifier.animateItem()) }
                            }
                            return@forEach
                        }
                        val empty = when (section) {
                            HomeSection.Filters -> filters.isEmpty()
                            HomeSection.RecentQueues -> saved.isEmpty()
                            else -> content.isEmpty
                        }
                        if (empty) return@forEach
                        any = true
                        val step = order++
                        item(key = "h.${pref.id}", contentType = "sectionHeader") {
                            val title = sectionTitle(section, content)
                            HomeSectionHeader(title, Modifier.animateItem().homeEntrance(reveal, step).testTag("home.section.${pref.id}"), onMenu = { sectionMenu = pref })
                        }
                        item(key = "s.${pref.id}", contentType = "section.${pref.resolvedStyle.id}") {
                            Box(Modifier.animateItem().homeEntrance(reveal, step)) {
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
    sheets.playlist?.let { p -> ActionSheet(Commands.playlists(listOf(p.id)), p.name, null, onDismiss = { sheets.playlist = null }) }
    if (jobs) JobsSheet { jobs = false }
    }
    }
}

private fun openSlide(nav: NavHostController, s: SpotlightSlide) {
    when {
        s.album != null -> nav.navigate(Route.Album(s.album.id))
        s.artist != null -> nav.navigate(Route.Artist(s.artist.id))
        s.playlist != null -> nav.navigate(Route.Playlist(s.playlist.id))
    }
}

/** Play (or shuffle) a slide's album, artist or playlist; Resume carries on from the saved point. */
private fun playSlide(client: app.hocket.core.client.CoreClient, serverId: String, s: SpotlightSlide, shuffle: Boolean) {
    if (s.resume != null && !shuffle) {
        client.dispatch(Commands.resumeAlbum(s.resume.albumId))
        return
    }
    val context = when {
        s.album != null -> Commands.albumContext(serverId, s.album.id, s.album.name)
        s.artist != null -> Commands.artistContext(serverId, s.artist.id, s.artist.name)
        s.playlist != null -> Commands.playlistContext(serverId, s.playlist.id, s.playlist.name)
        else -> return
    }
    client.dispatch(Commands.playContext(context, shuffle = shuffle))
}

/** The item menus Home can have open (one at a time). */
class HomeSheets {
    var album by mutableStateOf<Album?>(null)
    var artist by mutableStateOf<Artist?>(null)
    var queue by mutableStateOf<SavedQueue?>(null)
    var filter by mutableStateOf<Filter?>(null)
    var playlist by mutableStateOf<Playlist?>(null)
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
        // Spotlight's carousel is drawn by Home itself, ahead of every section.
        SectionStyle.Hero -> Unit
        SectionStyle.Tile -> (content as? SectionContent.Listening)?.let { ListeningTile(it) { nav.navigate(Route.Stats) } }
        SectionStyle.Row -> LazyRow(contentPadding = PaddingValues(horizontal = 16.dp), horizontalArrangement = Arrangement.spacedBy(12.dp)) {
            items(items, key = { it.key }) { HomeCell(it, actions, Modifier.width(if (it is HomeItem.ArtistItem) 124.dp else CellWidth)) }
        }
        SectionStyle.Grid -> androidx.compose.foundation.layout.BoxWithConstraints(Modifier.padding(horizontal = 16.dp)) {
            // Three rows of two on a phone; a wider area fits more to a row, still three rows.
            val columns = adaptiveColumns(maxWidth, GRID_CELL_MIN, 12.dp, max = GRID_MAX_COLUMNS)
            Column(Modifier.testTag("home.grid"), verticalArrangement = Arrangement.spacedBy(16.dp)) {
                items.take(columns * GRID_ROWS).chunked(columns).forEach { row ->
                    Row(Modifier.testTag("home.grid.row"), horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                        row.forEach { HomeCell(it, actions, Modifier.weight(1f)) }
                        repeat(columns - row.size) { Spacer(Modifier.weight(1f)) }
                    }
                }
            }
        }
        SectionStyle.List -> Column(Modifier.contentMaxWidth()) {
            items.take(8).forEach { item ->
                if (item is HomeItem.TrackItem) TrackRow(item.track, onClick = { actions.open(item) }, onMore = { actions.menu(item) })
                else HomeListRow(item, actions)
            }
        }
    }
}

/** A Home grid's rows, its narrowest cell and its most columns (a cell's art stays a cover, not a poster). */
private const val GRID_ROWS = 3
private val GRID_CELL_MIN = 160.dp
private const val GRID_MAX_COLUMNS = 6

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
    val interaction = remember { androidx.compose.foundation.interaction.MutableInteractionSource() }
    Column(
        modifier
            .combinedClickable(interactionSource = interaction, indication = androidx.compose.material3.ripple(), onClick = { share.prepare(); actions.open(item) }, onLongClick = { haptics.performHapticFeedback(HapticFeedbackType.LongPress); actions.menu(item) })
            .semantics(mergeDescendants = true) { customActions = a11y }
            .testTag("home.item"),
        horizontalAlignment = if (round) Alignment.CenterHorizontally else Alignment.Start,
    ) {
        val icon = iconOf(item)
        if (icon != null) IconArt(icon, shape, Modifier.fillMaxWidth().aspectRatio(1f).pressScale(interaction), iconSize = 40.dp)
        else Artwork(coverOf(item), ArtworkSizes.GRID, null, Modifier.fillMaxWidth().aspectRatio(1f).then(share.modifier).pressScale(interaction), shape)
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

/** How far above the bottom of Home's top bar its scrim starts to fade (the empty space under its icons). */
private val HomeBarScrimFadeStart = 8.dp

/** How long Home's top-bar scrim takes to fade out. */
private val HomeBarScrimFade = 96.dp
