package app.hocket.ui.nav

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.navigationBars
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.layout.WindowInsetsSides
import androidx.compose.foundation.layout.consumeWindowInsets
import androidx.compose.foundation.layout.only
import androidx.compose.foundation.layout.safeDrawing
import androidx.compose.foundation.layout.windowInsetsPadding
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.BarChart
import androidx.compose.material.icons.filled.Download
import androidx.compose.material.icons.filled.FilterAlt
import androidx.compose.material.icons.filled.Home
import androidx.compose.material.icons.filled.LibraryMusic
import androidx.compose.material.icons.filled.Search
import androidx.compose.material.icons.filled.Settings
import androidx.compose.material.icons.outlined.BarChart
import androidx.compose.material.icons.outlined.Download
import androidx.compose.material.icons.outlined.FilterAlt
import androidx.compose.material.icons.outlined.Home
import androidx.compose.material.icons.outlined.LibraryMusic
import androidx.compose.material.icons.outlined.Search
import androidx.compose.material.icons.outlined.Settings
import androidx.compose.material3.Button
import androidx.compose.material3.Icon
import androidx.compose.material3.LoadingIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalWideNavigationRail
import androidx.compose.material3.ShortNavigationBar
import androidx.compose.material3.ShortNavigationBarItem
import androidx.compose.material3.Snackbar
import androidx.compose.material3.SnackbarDuration
import androidx.compose.material3.SnackbarHost
import androidx.compose.material3.SnackbarHostState
import androidx.compose.material3.SnackbarResult
import androidx.compose.material3.Text
import androidx.compose.material3.WideNavigationRailItem
import androidx.compose.material3.WideNavigationRailValue
import androidx.compose.material3.rememberWideNavigationRailState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.derivedStateOf
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.foundation.layout.height
import androidx.compose.ui.layout.onSizeChanged
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.IntOffset
import androidx.compose.ui.zIndex
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.hapticfeedback.HapticFeedbackType
import androidx.compose.ui.platform.LocalHapticFeedback
import androidx.compose.ui.semantics.CustomAccessibilityAction
import androidx.compose.ui.semantics.customActions
import androidx.compose.ui.semantics.semantics
import kotlin.math.roundToInt
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.navigation.NavHostController
import androidx.navigation.compose.NavHost
import androidx.navigation.compose.composable
import androidx.navigation.compose.currentBackStackEntryAsState
import androidx.navigation.compose.rememberNavController
import androidx.navigation.toRoute
import app.hocket.BuildConfig
import app.hocket.HocketApp
import androidx.compose.ui.platform.LocalContext
import app.hocket.R
import app.hocket.core.CoreKind
import app.hocket.core.HocketJson
import app.hocket.playback.CoreHost
import kotlinx.coroutines.flow.StateFlow
import app.hocket.core.api.ErrorKind
import app.hocket.core.api.PlayerNoticeCode
import app.hocket.core.client.CoreClient
import app.hocket.ui.LocalCoreClient
import app.hocket.ui.LocalWideLayout
import app.hocket.ui.components.ConfirmDialog
import app.hocket.ui.player.NowPlayingSheet
import app.hocket.ui.player.NowPlayingSheetState
import app.hocket.ui.player.rememberNowPlayingSheetState
import app.hocket.ui.screens.detail.AlbumDetailScreen
import app.hocket.ui.screens.detail.ArtistDetailScreen
import app.hocket.ui.screens.detail.GenreDetailScreen
import app.hocket.ui.screens.detail.PlaylistDetailScreen
import app.hocket.ui.screens.downloads.DownloadsScreen
import app.hocket.ui.screens.filters.FilterBuilderScreen
import app.hocket.ui.screens.filters.FiltersScreen
import app.hocket.ui.screens.home.HomeScreen
import app.hocket.ui.screens.library.LibraryScreen
import app.hocket.ui.screens.search.SearchScreen
import app.hocket.ui.screens.settings.AboutScreen
import app.hocket.ui.screens.settings.AccountSettingsScreen
import app.hocket.ui.screens.settings.AppearanceSettingsScreen
import app.hocket.ui.screens.settings.BackupSettingsScreen
import app.hocket.ui.screens.settings.BatterySettingsScreen
import app.hocket.ui.screens.settings.DownloadsSettingsScreen
import app.hocket.ui.screens.settings.LibrarySettingsScreen
import app.hocket.ui.screens.settings.LyricsSettingsScreen
import app.hocket.ui.screens.settings.PlaybackSettingsScreen
import app.hocket.ui.screens.settings.AudioSettingsScreen
import app.hocket.ui.screens.settings.ConnectSettingsScreen
import app.hocket.ui.screens.settings.CustomiseSettingsScreen
import app.hocket.ui.screens.settings.SettingsScreen
import app.hocket.ui.screens.settings.TranscodingSettingsScreen
import app.hocket.ui.screens.setup.ServerSetupScreen
import app.hocket.ui.screens.stats.StatsScreen
import app.hocket.ui.queue.SavedQueuesScreen
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.runtime.compositionLocalOf
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.ui.platform.LocalLayoutDirection
import app.hocket.ui.a11y.LocalReducedMotion
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.launch

/**
 * Root: waits for the core and for the credential replay, shows setup only when there is no stored
 * server login (or the server is below the floor), then the shell. A release build whose native
 * core could not start shows [FatalErrorScreen] instead (never a fake library).
 */
@Composable
fun AppRoot(
    client: CoreClient?,
    credentialsReady: StateFlow<Boolean> = CoreHost.credentialsReplayed,
    logins: StateFlow<Set<String>> = CoreHost.logins,
    unreadableLogins: StateFlow<Set<String>> = CoreHost.unreadableLogins,
    fatalError: StateFlow<String?> = CoreHost.fatalError,
) {
    val fatal by fatalError.collectAsStateWithLifecycle()
    fatal?.let { FatalErrorScreen(it); return }
    if (client == null) { LoadingScreen(); return }
    CompositionLocalProvider(LocalCoreClient provides client, LocalNavBarPrefs provides rememberNavBarPrefs()) {
        val started by client.started.collectAsStateWithLifecycle()
        val servers by client.servers.collectAsStateWithLifecycle()
        // The fake core has nothing to replay; the native core waits for the keystore replay first.
        val replayed by credentialsReady.collectAsStateWithLifecycle()
        val ready = replayed || client.kind == CoreKind.Fake
        // Login state is a flow kept by CoreHost (no keystore reads during composition): it moves
        // when a setup login is confirmed, a stored one is refused, or the server list is pruned.
        val loginKeys by logins.collectAsStateWithLifecycle()
        val unreadable by unreadableLogins.collectAsStateWithLifecycle()
        val snackbar = remember { SnackbarHostState() }
        ToastCollector(snackbar)
        val server = servers.firstOrNull()
        val hasLogin = server != null && (client.kind == CoreKind.Fake || CoreHost.hasLogin(server, loginKeys))
        val needsRelogin = server != null && CoreHost.needsRelogin(server, unreadable)
        val fakeBanner = BuildConfig.DEBUG && client.kind == CoreKind.Fake
        val inShell = started && ready && server != null && hasLogin && server.capabilities.meetsFloor
        Box(Modifier.fillMaxSize().background(MaterialTheme.colorScheme.background)) {
            // The debug banner is laid out in flow above the app (never over its top bars): it takes
            // the status-bar inset and consumes it, so the screens below do not pad for it again.
            Column(Modifier.fillMaxSize()) {
                if (fakeBanner) FakeCoreBanner()
                Box(Modifier.weight(1f).fillMaxWidth().then(if (fakeBanner) Modifier.consumeWindowInsets(BannerInsets) else Modifier)) {
                    when {
                        !started || !ready -> LoadingScreen()
                        !inShell -> ServerSetupScreen(existing = server, needsRelogin = needsRelogin)
                        else -> MainShell(snackbar)
                    }
                }
            }
            // The main shell places its own host (above the mini player and the bar); setup and
            // loading keep one just above the system navigation bar.
            if (!inShell) SnackbarHost(snackbar, Modifier.align(Alignment.BottomCenter).navigationBarsPadding().padding(bottom = 8.dp)) { data -> Snackbar(data) }
        }
    }
}

/** The status bar and the top/side display cutout: what the banner pads for and consumes. */
private val BannerInsets: WindowInsets
    @Composable get() = WindowInsets.safeDrawing.only(WindowInsetsSides.Top + WindowInsetsSides.Horizontal)

/** Debug builds on the fake core say so at the top of the screen. */
@Composable
private fun FakeCoreBanner() {
    Box(Modifier.fillMaxWidth().background(MaterialTheme.colorScheme.errorContainer).windowInsetsPadding(BannerInsets).testTag("debug.fakeCoreBanner")) {
        Text(stringResource(R.string.debug_fake_core_banner), style = MaterialTheme.typography.labelSmall, color = MaterialTheme.colorScheme.onErrorContainer, modifier = Modifier.padding(horizontal = 12.dp, vertical = 4.dp))
    }
}

/** The native core failed to start (release): say so, offer a data reset, never fake a library. */
@Composable
fun FatalErrorScreen(detail: String, onReset: (android.content.Context) -> Unit = { CoreHost.resetData(it) }) {
    val context = LocalContext.current
    var confirm by remember { mutableStateOf(false) }
    Box(Modifier.fillMaxSize().background(MaterialTheme.colorScheme.background).statusBarsPadding().padding(24.dp).testTag("fatal"), contentAlignment = Alignment.Center) {
        Column(horizontalAlignment = Alignment.CenterHorizontally) {
            Text(stringResource(R.string.fatal_title), style = MaterialTheme.typography.headlineMedium)
            Text(stringResource(R.string.fatal_body), style = MaterialTheme.typography.bodyMedium, modifier = Modifier.padding(top = 12.dp))
            Text(stringResource(R.string.fatal_detail, detail), style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant, modifier = Modifier.padding(top = 12.dp))
            Button(onClick = { confirm = true }, modifier = Modifier.padding(top = 24.dp).testTag("fatal.reset")) { Text(stringResource(R.string.fatal_reset)) }
        }
    }
    if (confirm) ConfirmDialog(stringResource(R.string.fatal_reset_confirm), stringResource(R.string.fatal_reset), onConfirm = { onReset(context) }, onDismiss = { confirm = false })
}

@Composable
fun LoadingScreen() {
    Box(Modifier.fillMaxSize(), contentAlignment = Alignment.Center) {
        Column(horizontalAlignment = Alignment.CenterHorizontally) {
            LoadingIndicator()
            Text(stringResource(R.string.connecting_to_core), style = MaterialTheme.typography.bodyMedium, modifier = Modifier.padding(top = 12.dp))
        }
    }
}

/**
 * Event.Toast -> snackbar with its single action dispatching `action_command`; Event.Error ->
 * snackbar; the offline PlayerNotices -> snackbar.
 */
@Composable
private fun ToastCollector(host: SnackbarHostState) {
    val client = LocalCoreClient.current
    val authPrefix = stringResource(R.string.error_auth)
    val networkPrefix = stringResource(R.string.error_network)
    val offlineSkipping = stringResource(R.string.player_notice_offline_skipping)
    val offlineNothing = stringResource(R.string.player_notice_offline_nothing)
    LaunchedEffect(client) {
        // The core's offline notices (going offline mid-queue) also show as a snackbar: the mini
        // player's notice line is easy to miss when the queue silently skips.
        launch {
            client.playerNotice.collect { notice ->
                // Matched on the core's stable code, shown in this app's words.
                val text = when (notice?.code) {
                    PlayerNoticeCode.OfflineSkipping -> offlineSkipping
                    PlayerNoticeCode.NothingAvailableOffline -> offlineNothing
                    else -> null
                } ?: return@collect
                host.showSnackbar(text, duration = SnackbarDuration.Long)
            }
        }
        launch {
            client.toasts.collect { toast ->
                val result = host.showSnackbar(toast.message, actionLabel = toast.actionLabel, duration = if (toast.durationMs > 6000u) SnackbarDuration.Long else SnackbarDuration.Short)
                if (result == SnackbarResult.ActionPerformed) toast.actionCommand?.let { client.dispatch(HocketJson.decodeCommand(it)) }
            }
        }
        launch {
            client.errors.collect { e ->
                val prefix = when (e.kind) { ErrorKind.Auth -> authPrefix; ErrorKind.Network -> networkPrefix; else -> "" }
                host.showSnackbar(if (prefix.isNotEmpty() && prefix != e.message) "$prefix: ${e.message}" else e.message, duration = SnackbarDuration.Long)
            }
        }
    }
}

/**
 * Whether the mini player is showing (something is loaded in the player). Screens use it to reserve
 * room for the mini player at the end of their lists only when there is one ([BottomContentInset]).
 */
val LocalPlayerVisible = compositionLocalOf { false }

/**
 * What floats over the bottom of the screens: the (transparent) navigation bar with the system
 * navigation bar under it, or just the system inset beside a rail. Content runs under it (and
 * under the mini player), over a gradient scrim, so screens pad their lists by
 * [BottomContentInset] and place bottom overlays above [BottomOverlayInset].
 */
val LocalBottomBarInset = compositionLocalOf { 0.dp }

/** The gap the mini player keeps above the navigation bar. */
private val MiniPlayerGap = app.hocket.ui.player.NowPlayingSheetState.MINI_GAP

/**
 * Bottom content inset for scrolling screens: the navigation bar and, while something is playing,
 * the mini player (taller at large font sizes) float over the last rows, so they scroll clear of
 * both; otherwise only the bar and a small margin, so nothing leaves an empty band at the bottom.
 */
val BottomContentInset: Dp
    @Composable get() = LocalBottomBarInset.current + if (LocalPlayerVisible.current) app.hocket.ui.player.miniPlayerHeight() + MiniPlayerGap + 24.dp else 24.dp

/** Where a screen's bottom overlay (a selection toolbar, a floating button) sits clear of the bar and the mini player. */
val BottomOverlayInset: Dp
    @Composable get() = LocalBottomBarInset.current + (if (LocalPlayerVisible.current) app.hocket.ui.player.miniPlayerHeight() + MiniPlayerGap else 0.dp) + 12.dp

/**
 * The phone navigation bar's height before it has been measured: Material's short navigation bar
 * is 64 dp plus the edge-to-edge navigation-bar inset under it. Using it from the first frame means
 * the content padding and the sheet's collapsed anchor do not jump when the bar is first measured
 * (the measurement only corrects it where the bar grows, e.g. at very large font sizes).
 */
private val ShortNavBarHeight = 64.dp

/** The collapsed wide navigation rail's width, before it is measured. */
private val CollapsedRailWidth = 96.dp

@Composable
private fun MainShell(snackbar: SnackbarHostState) {
    val nav = rememberNavController()
    val sheet = rememberNowPlayingSheetState()
    val client = LocalCoreClient.current
    val nowPlaying = client.nowPlaying.collectAsStateWithLifecycle()
    val playerVisible by remember { derivedStateOf { nowPlaying.value != null } }
    BoxWithConstraints(Modifier.fillMaxSize()) {
        val wide = maxWidth >= 600.dp
        CompositionLocalProvider(LocalWideLayout provides wide, LocalPlayerVisible provides playerVisible) {
            val items = navItems()
            val backStack by nav.currentBackStack.collectAsStateWithLifecycle()
            // The root entry of the stack last switched to (see selectedPlace).
            var rootEntryId by rememberSaveable { mutableStateOf<String?>(null) }
            val selected = selectedPlace(backStack, items, rootEntryId)
            val scope = rememberCoroutineScope()
            val density = LocalDensity.current
            val haptics = LocalHapticFeedback.current
            var accountOpen by rememberSaveable { mutableStateOf(false) }
            var editorOpen by rememberSaveable { mutableStateOf(false) }
            var devicesOpen by rememberSaveable { mutableStateOf(false) }
            // The phone navigation bar's height (including the edge-to-edge navigation-bar inset), so
            // the content column ends above it and the sheet's collapsed anchor sits on it. Known from
            // the first frame (fixed height + inset); the measurement only corrects it when it differs.
            val systemBottomPx = WindowInsets.navigationBars.getBottom(density)
            var navBarMeasuredPx by remember { mutableIntStateOf(-1) }
            val navBarHeightPx = if (navBarMeasuredPx >= 0) navBarMeasuredPx else with(density) { ShortNavBarHeight.roundToPx() } + systemBottomPx
            val bottomInsetPx = if (wide) systemBottomPx else navBarHeightPx
            var railWidthPx by remember { mutableIntStateOf(-1) }
            // While the full player covers the screen, what is behind it (the page, the navigation
            // bar and rail) leaves the accessibility tree, as a modal would: TalkBack must not wander
            // into content nobody can see.
            val covered by remember(sheet) { derivedStateOf { sheet.progress >= 0.6f } }
            val hiddenWhenCovered = if (covered) Modifier.clearAndSetSemantics { } else Modifier
            // Transitions: what the last bar/rail switch was, so the NavHost can tell it from a push.
            val transitions = remember { ShellTransitionInfo() }
            transitions.reducedMotion = LocalReducedMotion.current
            transitions.layoutDirection = LocalLayoutDirection.current
            transitions.density = density
            val reselected = remember { MutableSharedFlow<Unit>(extraBufferCapacity = 1) }
            val currentSelected by rememberUpdatedState(selected)
            val currentItems by rememberUpdatedState(items)
            val go: (NavItem) -> Unit = remember(nav) {
                { item ->
                    val from = currentSelected
                    val handled = from == item && when (nav.reselectPlace(item)) {
                        Reselect.AtRoot -> { reselected.tryEmit(Unit); true }
                        Reselect.Popped -> true
                        Reselect.NotFound -> false
                    }
                    if (!handled) {
                        val list = currentItems
                        transitions.tabFromId = nav.currentBackStackEntry?.id
                        transitions.tabForward = list.indexOf(item) >= list.indexOf(from)
                        rootEntryId = nav.goToPlace(item)
                        transitions.tabTargetId = nav.currentBackStackEntry?.id
                    }
                }
            }
            val shell = remember(items, nav) { ShellNavigator(items, goTo = go, openAccount = { accountOpen = true }, openBarEditor = { editorOpen = true }, openAvailableOffline = { nav.navigate(Route.AvailableOffline) { launchSingleTop = true } }) }
            val editLabel = stringResource(R.string.bottom_bar_edit)
            val openEditor = remember { { haptics.performHapticFeedback(HapticFeedbackType.LongPress); editorOpen = true } }
            val bottomInset = with(density) { bottomInsetPx.toDp() }
            CompositionLocalProvider(LocalShellNavigator provides shell, LocalTabReselected provides reselected, LocalBottomBarInset provides bottomInset) {
            Row(Modifier.fillMaxSize().then(hiddenWhenCovered)) {
                if (wide) {
                    val railState = rememberWideNavigationRailState(WideNavigationRailValue.Collapsed)
                    Box(Modifier.onSizeChanged { railWidthPx = it.width }) {
                        ModalWideNavigationRail(state = railState, hideOnCollapse = false) {
                            items.forEach { item ->
                                val isSelected = item == selected
                                WideNavigationRailItem(
                                    selected = isSelected,
                                    onClick = { go(item); scope.launch { railState.collapse() } },
                                    icon = { Icon(item.icon(isSelected), null) },
                                    label = { Text(item.label()) },
                                    railExpanded = railState.targetValue == WideNavigationRailValue.Expanded,
                                    modifier = Modifier.testTag("navRail." + item.id),
                                )
                            }
                        }
                    }
                }
                // Content runs to the bottom of the screen: the transparent navigation bar and the
                // mini player float over it, over the scrim below ([BottomContentInset]).
                Box(Modifier.weight(1f).fillMaxSize()) {
                    val contentNavigator = remember(nav) { app.hocket.ui.DetailNavigator({ nav.navigate(Route.Album(it)) }, { nav.navigate(Route.Artist(it)) }) }
                    CompositionLocalProvider(app.hocket.ui.LocalDetailNavigator provides contentNavigator) {
                        AppNavHost(nav, transitions, Modifier.fillMaxSize())
                    }
                }
            }
            // Links inside the full player (album/artist, the "More" sheet's go-to actions, rows'
            // accessibility actions) collapse it as they navigate: the page opens in view, not
            // behind the player.
            val navigator = remember(nav, sheet) {
                app.hocket.ui.DetailNavigator(
                    openAlbum = { id -> scope.launch { sheet.collapse() }; nav.navigate(Route.Album(id)) },
                    openArtist = { id -> scope.launch { sheet.collapse() }; nav.navigate(Route.Artist(id)) },
                )
            }
            CompositionLocalProvider(app.hocket.ui.LocalDetailNavigator provides navigator) {
            NowPlayingSheet(
                state = sheet,
                bottomInset = with(density) { bottomInsetPx.toDp() },
                onOpenAlbum = navigator.openAlbum,
                onOpenArtist = navigator.openArtist,
            )
            }
            // The scrim the bar and the mini player float on (Navic): the surface colour eased in
            // from transparent, so content scrolling under them fades out instead of clashing.
            if (!wide || playerVisible) {
                val scrimHeight = bottomInset + (if (playerVisible) app.hocket.ui.player.miniPlayerHeight() + MiniPlayerGap else 0.dp) + 40.dp
                val surface = MaterialTheme.colorScheme.surface
                val scrim = remember(surface) { easedScrim(surface) }
                Box(Modifier.align(Alignment.BottomCenter).fillMaxWidth().height(scrimHeight).zIndex(5f).background(scrim).testTag("bottomScrim"))
            }
            if (!wide) {
                // Drawn above the sheet so the collapsed sheet body never covers it; slides out as the
                // sheet expands and the full player takes the screen (read in the placement phase, so
                // dragging the sheet does not recompose the bar). A long press opens its editor (also
                // a TalkBack action on every item).
                ShortNavigationBar(
                    // Transparent over the scrim (Navic's detached style).
                    containerColor = Color.Transparent,
                    modifier = Modifier
                        .align(Alignment.BottomCenter)
                        .zIndex(20f)
                        .onSizeChanged { navBarMeasuredPx = it.height }
                        .offset { IntOffset(0, (sheet.progress * navBarHeightPx).roundToInt()) }
                        // Its icons fade quickly as the player grows over them.
                        .graphicsLayer { alpha = (1f - sheet.progress * 3f).coerceIn(0f, 1f) }
                        .longPressToEdit(openEditor)
                        .testTag("navBar")
                        .then(hiddenWhenCovered),
                ) {
                    items.forEach { item ->
                        val isSelected = item == selected
                        ShortNavigationBarItem(
                            selected = isSelected, onClick = { go(item) }, icon = { Icon(item.icon(isSelected), null) }, label = { Text(item.barLabel(), maxLines = 1) },
                            modifier = Modifier.testTag("navBar." + item.id).semantics { customActions = listOf(CustomAccessibilityAction(editLabel) { editorOpen = true; true }) },
                        )
                    }
                }
            }
            // Snackbars sit just above the mini player (or the bar when nothing plays), centred on
            // the content beside the rail on wide layouts; as the full player opens they ease down
            // to the bottom of the screen instead of floating over its controls.
            val miniPx = if (playerVisible) with(density) { (app.hocket.ui.player.miniPlayerHeight() + MiniPlayerGap).roundToPx() } else 0
            val marginPx = with(density) { 8.dp.roundToPx() }
            val collapsedBottomPx = bottomInsetPx + miniPx + marginPx
            val expandedBottomPx = systemBottomPx + marginPx
            val railPx = if (wide) (if (railWidthPx >= 0) railWidthPx else with(density) { CollapsedRailWidth.roundToPx() }) else 0
            SnackbarHost(
                snackbar,
                Modifier
                    .align(Alignment.BottomCenter)
                    .zIndex(30f)
                    .padding(start = with(density) { railPx.toDp() }, bottom = with(density) { collapsedBottomPx.toDp() })
                    .offset { IntOffset(0, (sheet.progress * (collapsedBottomPx - expandedBottomPx)).roundToInt()) },
            ) { data -> Snackbar(data) }
            if (accountOpen) {
                AccountSheet(
                    barItems = items,
                    onSettings = { accountOpen = false; nav.navigate(Route.Settings) { launchSingleTop = true } },
                    onStats = { accountOpen = false; go(NavItem.Stats) },
                    onDevices = { accountOpen = false; devicesOpen = true },
                    onPlace = { accountOpen = false; go(it) },
                    onEditBar = { accountOpen = false; editorOpen = true },
                    onDismiss = { accountOpen = false },
                )
            }
            if (editorOpen) BottomBarEditorSheet(onDismiss = { editorOpen = false })
            if (devicesOpen) app.hocket.ui.player.HandoffSheet(onDismiss = { devicesOpen = false })
            }
        }
    }
}

/**
 * A vertical gradient from transparent (top) to [color] (bottom) with an eased curve (circular
 * ease-in, measured from the bottom), so the scrim is nearly solid behind the bar and fades out
 * softly above the mini player instead of showing a hard band.
 */
private fun easedScrim(color: Color, stops: Int = 16): Brush = Brush.verticalGradient(
    *Array(stops) { i ->
        val t = i / (stops - 1f)
        val fromBottom = 1f - t
        t to color.copy(alpha = color.alpha * kotlin.math.sqrt((1f - fromBottom * fromBottom).coerceAtLeast(0f)))
    },
)

@Composable
private fun AppNavHost(nav: NavHostController, transitions: ShellTransitionInfo, modifier: Modifier) {
    NavHost(
        nav,
        startDestination = Route.Home,
        modifier = modifier,
        enterTransition = { transitions.enter(this) },
        exitTransition = { transitions.exit(this) },
        popEnterTransition = { transitions.popEnter(this) },
        popExitTransition = { transitions.popExit(this) },
        sizeTransform = null,
    ) {
        composable<Route.Home> { HomeScreen(nav) }
        composable<Route.Library> { entry -> LibraryScreen(nav, initialTab = entry.toRoute<Route.Library>().tab) }
        composable<Route.Search> { SearchScreen(nav) }
        composable<Route.Settings> { SettingsScreen(nav) }
        composable<Route.Downloads> { DownloadsScreen(nav) }
        composable<Route.Albums> { app.hocket.ui.screens.library.LibraryListScreen(nav, app.hocket.ui.screens.library.LibraryList.Albums) }
        composable<Route.Artists> { app.hocket.ui.screens.library.LibraryListScreen(nav, app.hocket.ui.screens.library.LibraryList.Artists) }
        composable<Route.Playlists> { app.hocket.ui.screens.library.LibraryListScreen(nav, app.hocket.ui.screens.library.LibraryList.Playlists) }
        composable<Route.Songs> { app.hocket.ui.screens.library.LibraryListScreen(nav, app.hocket.ui.screens.library.LibraryList.Songs) }
        composable<Route.Genres> { app.hocket.ui.screens.library.LibraryListScreen(nav, app.hocket.ui.screens.library.LibraryList.Genres) }
        composable<Route.AvailableOffline> { app.hocket.ui.screens.library.AvailableOfflineScreen(nav) }
        composable<Route.Filters> { FiltersScreen(nav) }
        composable<Route.FilterBuilder> { entry -> FilterBuilderScreen(nav, entry.toRoute<Route.FilterBuilder>().id) }
        composable<Route.Stats> { StatsScreen(nav) }
        composable<Route.Album> { entry -> AlbumDetailScreen(nav, entry.toRoute<Route.Album>().id) }
        composable<Route.Artist> { entry -> ArtistDetailScreen(nav, entry.toRoute<Route.Artist>().id) }
        composable<Route.Playlist> { entry -> PlaylistDetailScreen(nav, entry.toRoute<Route.Playlist>().id) }
        composable<Route.Genre> { entry -> GenreDetailScreen(nav, entry.toRoute<Route.Genre>().name) }
        composable<Route.SavedQueues> { SavedQueuesScreen(nav) }
        composable<Route.AudioSettings> { AudioSettingsScreen(nav) }
        composable<Route.TranscodingSettings> { TranscodingSettingsScreen(nav) }
        composable<Route.ConnectSettings> { ConnectSettingsScreen(nav) }
        composable<Route.CustomiseSettings> { CustomiseSettingsScreen(nav) }
        composable<Route.About> { AboutScreen(nav) }
        composable<Route.SettingsAccount> { AccountSettingsScreen(nav) }
        composable<Route.SettingsAppearance> { AppearanceSettingsScreen(nav) }
        composable<Route.SettingsPlayback> { PlaybackSettingsScreen(nav) }
        composable<Route.SettingsDownloads> { DownloadsSettingsScreen(nav) }
        composable<Route.SettingsLyrics> { LyricsSettingsScreen(nav) }
        composable<Route.SettingsLibrary> { LibrarySettingsScreen(nav) }
        composable<Route.SettingsBattery> { BatterySettingsScreen(nav) }
        composable<Route.SettingsBackup> { BackupSettingsScreen(nav) }
    }
}
