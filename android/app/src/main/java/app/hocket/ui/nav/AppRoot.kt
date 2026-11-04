package app.hocket.ui.nav

import androidx.compose.animation.SharedTransitionLayout
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
import androidx.compose.material3.Button
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.WideNavigationRail
import androidx.compose.material3.ShortNavigationBar
import androidx.compose.material3.ShortNavigationBarItem
import androidx.compose.ui.hapticfeedback.HapticFeedbackType
import androidx.compose.ui.platform.LocalHapticFeedback
import androidx.compose.material3.Snackbar
import androidx.compose.material3.SnackbarDuration
import androidx.compose.material3.SnackbarHost
import androidx.compose.material3.SnackbarHostState
import androidx.compose.material3.SnackbarResult
import androidx.compose.material3.Text
import androidx.compose.material3.WideNavigationRailItem
import androidx.compose.material3.WideNavigationRailValue
import androidx.compose.material3.rememberWideNavigationRailState
import androidx.activity.compose.BackHandler
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.SideEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.derivedStateOf
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clipToBounds
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.graphics.Brush
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.ui.layout.onSizeChanged
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.colorResource
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.IntOffset
import androidx.compose.ui.zIndex
import androidx.compose.runtime.saveable.rememberSaveable
import kotlin.math.roundToInt
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.LifecycleEventObserver
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.navigation.NavBackStackEntry
import androidx.navigation.NavDestination.Companion.hasRoute
import androidx.navigation.NavGraphBuilder
import androidx.navigation.NavController
import androidx.navigation.NavHostController
import androidx.navigation.compose.NavHost
import androidx.navigation.compose.composable
import androidx.navigation.compose.currentBackStackEntryAsState
import androidx.navigation.compose.rememberNavController
import androidx.navigation.toRoute
import app.hocket.core.SettingKeys
import app.hocket.diagnostics.DeveloperTriggerEffect
import app.hocket.ui.screens.settings.setting
import app.hocket.BuildConfig
import app.hocket.HocketApp
import androidx.compose.ui.platform.LocalContext
import app.hocket.R
import kotlinx.coroutines.flow.drop
import kotlinx.coroutines.flow.first
import app.hocket.ui.icons.HocketIcons
import app.hocket.core.CoreKind
import app.hocket.core.HocketJson
import app.hocket.playback.CoreHost
import kotlinx.coroutines.flow.StateFlow
import app.hocket.core.api.ErrorKind
import app.hocket.core.api.PlayerNoticeCode
import app.hocket.core.client.CoreClient
import app.hocket.ui.LocalCoreClient
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
import app.hocket.ui.screens.settings.AdvancedSettingsScreen
import app.hocket.ui.screens.settings.AppearanceSettingsScreen
import app.hocket.ui.screens.settings.BackupSettingsScreen
import app.hocket.ui.screens.settings.BatterySettingsScreen
import app.hocket.ui.screens.settings.DeveloperDiagnosticsScreen
import app.hocket.ui.screens.settings.DownloadsSettingsScreen
import app.hocket.ui.screens.settings.LibrarySettingsScreen
import app.hocket.ui.screens.settings.LyricsSettingsScreen
import app.hocket.ui.screens.settings.PebbleSettingsScreen
import app.hocket.ui.screens.settings.PlaybackSettingsScreen
import app.hocket.ui.screens.settings.AudioSettingsScreen
import app.hocket.ui.screens.settings.ConnectSettingsScreen
import app.hocket.ui.screens.settings.CustomiseSettingsScreen
import app.hocket.ui.screens.settings.SettingsScreen
import app.hocket.ui.screens.settings.TranscodingSettingsScreen
import app.hocket.ui.screens.setup.ServerSetupScreen
import app.hocket.ui.screens.stats.StatsScreen
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
    onFirstScreen: () -> Unit = {},
) {
    val fatal by fatalError.collectAsStateWithLifecycle()
    fatal?.let { SideEffect(onFirstScreen); FatalErrorScreen(it); return }
    if (client == null) { LoadingScreen(); return }
    CompositionLocalProvider(LocalCoreClient provides client, LocalNavBarPrefs provides rememberShellNavBarPrefs(client), LocalPageChrome provides remember { PageChrome() }) {
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
        val snackbarScope = rememberCoroutineScope()
        val appSnackbar = remember(snackbar, snackbarScope) { app.hocket.ui.components.AppSnackbar(snackbar, snackbarScope) }
        ToastCollector(snackbar)
        val server = servers.firstOrNull()
        val hasLogin = server != null && (client.kind == CoreKind.Fake || CoreHost.hasLogin(server, loginKeys))
        val needsRelogin = server != null && CoreHost.needsRelogin(server, unreadable)
        val fakeBanner = BuildConfig.DEBUG && client.kind == CoreKind.Fake && LocalFakeCoreBanner.current
        // Set when the setup form signs in: setup stays up for the server's summary until Continue.
        var holdSetup by rememberSaveable { mutableStateOf(false) }
        val inShell = started && ready && server != null && hasLogin && server.capabilities.meetsFloor && !holdSetup
        CompositionLocalProvider(app.hocket.ui.components.LocalAppSnackbar provides appSnackbar) {
        Box(Modifier.fillMaxSize().background(MaterialTheme.colorScheme.background)) {
            // The debug banner is laid out in flow above the app (never over its top bars): it takes
            // the status-bar inset and consumes it, so the screens below do not pad for it again.
            // The report upload bar (docs/reports.md) sits the same way, under the banner when both show.
            val upload by client.reportUpload.collectAsStateWithLifecycle()
            val topBar = fakeBanner || upload != null
            Column(Modifier.fillMaxSize()) {
                if (fakeBanner) FakeCoreBanner()
                app.hocket.ui.components.ReportUploadBarHost(client, insets = if (fakeBanner) null else BannerInsets)
                Box(Modifier.weight(1f).fillMaxWidth().then(if (topBar) Modifier.consumeWindowInsets(BannerInsets) else Modifier)) {
                    if (started && ready) SideEffect(onFirstScreen)
                    when {
                        !started || !ready -> LoadingScreen()
                        !inShell -> ServerSetupScreen(existing = server, summary = server != null && !needsRelogin && (hasLogin || !server.capabilities.meetsFloor), needsRelogin = needsRelogin, onConnecting = { holdSetup = true }, onContinue = { holdSetup = false })
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
}

/**
 * Whether debug builds on the fake core show [FakeCoreBanner]. Always on in the app; screenshot
 * tests (which can only run the fake core under Robolectric) switch it off so the captures show
 * the screens as a user sees them.
 */
val LocalFakeCoreBanner = androidx.compose.runtime.staticCompositionLocalOf { true }

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

/**
 * While the core starts: the splash screen's tile and mark, still (MainActivity keeps the system
 * splash up over this, so it only shows if startup outlasts the splash or the core restarts). No
 * spinner; a word appears only if it takes a while.
 */
@Composable
fun LoadingScreen() {
    var slow by remember { mutableStateOf(false) }
    LaunchedEffect(Unit) { kotlinx.coroutines.delay(LOADING_WORDS_AFTER_MS); slow = true }
    Box(Modifier.fillMaxSize().background(colorResource(R.color.splash_background)).testTag("loading"), contentAlignment = Alignment.Center) {
        // The foreground's 108-unit tile at the splash icon's size (its 72-unit mark in a 192 dp circle).
        androidx.compose.foundation.Image(painterResource(R.drawable.ic_launcher_foreground), contentDescription = null, modifier = Modifier.size(288.dp))
        androidx.compose.animation.AnimatedVisibility(slow, enter = androidx.compose.animation.fadeIn(), modifier = Modifier.align(Alignment.Center).padding(top = 160.dp)) {
            Text(stringResource(R.string.connecting_to_core), style = MaterialTheme.typography.bodyMedium, color = colorResource(R.color.shortcut_foreground))
        }
    }
}

/** How long the loading screen stays wordless. */
private const val LOADING_WORDS_AFTER_MS = 3_000L

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
                val target = app.hocket.ui.components.SnackbarRedirect.host ?: host
                val result = target.showSnackbar(toast.message, actionLabel = toast.actionLabel, duration = if (toast.durationMs > 6000u) SnackbarDuration.Long else SnackbarDuration.Short)
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

/**
 * The room the stash chip takes above the mini player when it floats on its own (the standard bar,
 * or beside a rail; with the floating bar it sits beside the pill and takes none). Part of
 * [BottomContentInset] and [BottomOverlayInset].
 */
val LocalStashRoom = compositionLocalOf { 0.dp }

/**
 * Whether the mini player sits beside the floating bar (wide screens: the pill at the bottom start,
 * the mini player at the end) rather than above it: [LocalBottomBarInset] then already covers both,
 * so [BottomContentInset] and [BottomOverlayInset] add nothing for the mini player.
 */
val LocalMiniBesideBar = compositionLocalOf { false }

/**
 * The shell's shape for a window: the navigation (the floating pill or the standard bar at the
 * bottom, or a [rail] at the start edge) and where the mini player sits ([miniWidth] wide at the end
 * edge, or centred when null). [barRow]: the pill at the bottom start with the mini player beside it.
 * [panel]: the player opens as a side panel beside the page rather than over it.
 */
internal data class ShellLayout(val rail: Boolean, val barRow: Boolean, val miniWidth: Dp?, val panel: Boolean)

/** The mini player's width at the end edge, and the side panel's (the panel grows out of a mini player as wide). */
internal val EndMiniWidth = 400.dp
internal val PanelWidth = 420.dp

/** The narrowest the mini player gets beside the pill before the two stack again. */
private val MinMiniBesideBar = 320.dp

/**
 * The layout for a [width] × [height] window: phones (under 600 dp) keep the stacked, centred bar
 * and mini player; wider, the floating pill and the mini player share one row (as long as a mini
 * player of [MinMiniBesideBar] fits beside the widest pill of [places]), or with the floating bar
 * turned off a rail takes the start edge and the mini player the end. Landscape windows of 960 dp
 * and wider open the player as a side panel.
 */
internal fun shellLayout(width: Dp, height: Dp, floatingBar: Boolean, places: Int): ShellLayout {
    if (width < 600.dp) return ShellLayout(rail = false, barRow = false, miniWidth = null, panel = false)
    val panel = width >= 960.dp && width > height
    val endWidth = if (panel) PanelWidth else EndMiniWidth
    val margin = app.hocket.ui.player.HeroGeometry.MINI_MARGIN
    if (!floatingBar) {
        val room = width - CollapsedRailWidth - margin * 2
        return ShellLayout(rail = true, barRow = false, miniWidth = minOf(endWidth, room), panel = panel && room >= PanelWidth)
    }
    val room = width - floatingPillMaxWidth(places) - margin * 3
    if (room < MinMiniBesideBar) return ShellLayout(rail = false, barRow = false, miniWidth = null, panel = false)
    return ShellLayout(rail = false, barRow = true, miniWidth = minOf(endWidth, room), panel = panel && room >= PanelWidth)
}

/** The gap the mini player keeps above the navigation bar. */
private val MiniPlayerGap = app.hocket.ui.player.NowPlayingSheetState.MINI_GAP

/**
 * Bottom content inset for scrolling screens: the navigation bar and, while something is playing,
 * the mini player (taller at large font sizes) float over the last rows, so they scroll clear of
 * both; otherwise only the bar and a small margin, so nothing leaves an empty band at the bottom.
 */
val BottomContentInset: Dp
    @Composable get() = LocalBottomBarInset.current + LocalStashRoom.current + if (LocalPlayerVisible.current && !LocalMiniBesideBar.current) app.hocket.ui.player.miniPlayerHeight() + MiniPlayerGap + 24.dp else 24.dp

/** Where a screen's bottom overlay (a selection toolbar, a floating button) sits clear of the bar and the mini player. */
val BottomOverlayInset: Dp
    @Composable get() = LocalBottomBarInset.current + LocalStashRoom.current + (if (LocalPlayerVisible.current && !LocalMiniBesideBar.current) app.hocket.ui.player.miniPlayerHeight() + MiniPlayerGap else 0.dp) + 12.dp

/**
 * The standard phone navigation bar's height before it has been measured (Material's short
 * navigation bar; the floating one is [FloatingNavBarDefaults.Height]), plus the edge-to-edge
 * navigation-bar inset under it. Using it from the first frame means the content padding and the sheet's collapsed anchor do
 * not jump when the bar is first measured (the measurement only corrects it if they disagree).
 */
private val ShortNavBarHeight = 64.dp

/** How much the page behind the full player shrinks as it opens (a share of its size). */
private const val RecedeScale = 0.05f

/** The receding page's corners with the player fully open. */
private val RecedeCorner = 20.dp

/** The collapsed wide navigation rail's width, before it is measured. */
internal val CollapsedRailWidth = 96.dp

/** Where the shell remembers whether the side panel was left open (across restarts). */
private const val SHELL_PREFS = "hocket.shell"
private const val PANEL_OPEN = "player.panelOpen"

@Composable
private fun MainShell(snackbar: SnackbarHostState) {
    val nav = rememberNavController()
    val sheet = rememberNowPlayingSheetState()
    // The one song menu, hosted here (not inside the player sheet or a list) and opened from anywhere.
    val songMenu = remember { app.hocket.ui.components.SongMenuController() }
    val client = LocalCoreClient.current
    val nowPlaying = client.nowPlaying.collectAsStateWithLifecycle()
    val playerVisible by remember { derivedStateOf { nowPlaying.value != null } }
    BoxWithConstraints(Modifier.fillMaxSize()) {
        // The phone's bar: the compact floating pill, or the standard full-width bar.
        val floatingBar = setting(SettingKeys.DISPLAY_FLOATING_NAV_BAR).bool ?: true
        val shellLayout = shellLayout(maxWidth, maxHeight, floatingBar, navItems().size)
        // A rail at the start edge in place of a bottom bar.
        val wide = shellLayout.rail
        val barRow = shellLayout.barRow
        val sharedArtwork = remember { SharedArtworkSession() }
        val screenCache = remember { app.hocket.ui.ScreenCache() }
        // Each page takes the flight prepared for it as it becomes current (synchronously, inside
        // navigate, so its first frame already matches), and drops it once it has left for good.
        DisposableEffect(nav, sharedArtwork) {
            val listener = NavController.OnDestinationChangedListener { controller, _, _ ->
                val entry = controller.currentBackStackEntry ?: return@OnDestinationChangedListener
                val id = entry.id
                if (sharedArtwork.attach(id)) {
                    entry.lifecycle.addObserver(LifecycleEventObserver { _, event -> if (event == Lifecycle.Event.ON_DESTROY) sharedArtwork.release(id) })
                }
            }
            nav.addOnDestinationChangedListener(listener)
            onDispose { nav.removeOnDestinationChangedListener(listener) }
        }
        // Developer mode: a shake (or Ctrl+Shift+D) opens developer diagnostics.
        val developerMode = setting(SettingKeys.DEVELOPER_MODE).bool ?: false
        DeveloperTriggerEffect(developerMode) {
            if (nav.currentDestination?.hasRoute(Route.DeveloperDiagnostics::class) != true) nav.navigate(Route.DeveloperDiagnostics)
        }
        val openConnectSettings = remember(nav) { { nav.navigate(Route.ConnectSettings) { launchSingleTop = true } } }
        // Queues is a drawer over whatever is showing, from the bar, the Library and Home alike.
        var queuesOpen by rememberSaveable { mutableStateOf(false) }
        val openQueues = remember { { queuesOpen = true } }
        if (queuesOpen) app.hocket.ui.player.QueuesSheet(onDismiss = { queuesOpen = false })
        // The song stash (#150): a chip beside the floating bar, or on its own above the mini player.
        val stash by client.stash.collectAsStateWithLifecycle()
        var stashOpen by rememberSaveable { mutableStateOf(false) }
        val fontScale = LocalDensity.current.fontScale
        // The bar's room: beside the mini player in the bar row.
        val barWidth = if (barRow) maxWidth - (shellLayout.miniWidth ?: 0.dp) - app.hocket.ui.player.HeroGeometry.MINI_MARGIN - 8.dp else maxWidth
        val stashAlone = wide || !floatingBar || !stashFitsBesideBar(barWidth, navItems().size, fontScale)
        // Eased, so the content's bottom padding follows the chip in rather than jumping.
        val stashRoom by androidx.compose.animation.core.animateDpAsState(
            if (stashAlone && stash.isNotEmpty()) app.hocket.ui.stash.StashChipDefaults.StandaloneRoom else 0.dp, label = "stashRoom",
        )
        CompositionLocalProvider(app.hocket.ui.player.LocalOpenQueues provides openQueues, LocalPlayerVisible provides playerVisible, LocalMiniBesideBar provides barRow, LocalStashRoom provides stashRoom, app.hocket.ui.components.LocalSongMenu provides songMenu, LocalSharedArtwork provides sharedArtwork, app.hocket.ui.LocalScreenCache provides screenCache, app.hocket.ui.player.LocalOpenConnectSettings provides openConnectSettings) {
            val items = navItems()
            val backStack by nav.currentBackStack.collectAsStateWithLifecycle()
            // The root entry of the stack last switched to (see selectedPlace).
            var rootEntryId by rememberSaveable { mutableStateOf<String?>(null) }
            val selected = selectedPlace(backStack, items, rootEntryId)
            val scope = rememberCoroutineScope()
            val density = LocalDensity.current
            var accountOpen by rememberSaveable { mutableStateOf(false) }
            var editorOpen by rememberSaveable { mutableStateOf(false) }
            var devicesOpen by rememberSaveable { mutableStateOf(false) }
            // The phone navigation bar's height (including the edge-to-edge navigation-bar inset), so
            // the content column ends above it and the sheet's collapsed anchor sits on it. Known from
            // the first frame (fixed height + inset); the measurement only corrects it when it differs.
            val systemBottomPx = WindowInsets.navigationBars.getBottom(density)
            var navBarMeasuredPx by remember(floatingBar) { mutableIntStateOf(-1) }
            val navBarHeightPx = if (navBarMeasuredPx >= 0) navBarMeasuredPx else with(density) { (if (floatingBar) FloatingNavBarDefaults.Height else ShortNavBarHeight).roundToPx() } + systemBottomPx
            // The bar row: the pill and the mini player centred on one row (as tall as the taller),
            // its bottom 8 dp above the system bar.
            val miniHeightDp = app.hocket.ui.player.miniPlayerHeight()
            val rowHeight = maxOf(FloatingNavBarDefaults.PillHeight, miniHeightDp)
            val rowBottomPx = systemBottomPx + with(density) { FloatingNavBarDefaults.BottomMargin.roundToPx() }
            val bottomInsetPx = when {
                wide -> systemBottomPx
                barRow -> rowBottomPx + with(density) { rowHeight.roundToPx() }
                else -> navBarHeightPx
            }
            // The sheet's own inset: under the mini player's resting place (it adds its gap above that).
            val sheetInsetPx = if (barRow) rowBottomPx + with(density) { ((rowHeight - miniHeightDp) / 2 - MiniPlayerGap).roundToPx() } else bottomInsetPx
            var railWidthPx by remember { mutableIntStateOf(-1) }
            // While the full player covers the screen, what is behind it (the page, the navigation
            // bar and rail) leaves the accessibility tree, as a modal would: TalkBack must not wander
            // into content nobody can see.
            val covered by remember(sheet) { derivedStateOf { sheet.coverProgress >= 0.6f } }
            val hiddenWhenCovered = if (covered) Modifier.clearAndSetSemantics { } else Modifier
            // Transitions: what the last bar/rail switch was, so the NavHost can tell it from a push.
            val transitions = remember { ShellTransitionInfo() }
            transitions.reducedMotion = LocalReducedMotion.current
            transitions.layoutDirection = LocalLayoutDirection.current
            transitions.density = density.density
            val reselected = remember { MutableSharedFlow<Unit>(extraBufferCapacity = 1) }
            val currentSelected by rememberUpdatedState(selected)
            val currentItems by rememberUpdatedState(items)
            val go: (NavItem) -> Unit = remember(nav) {
                go@{ item ->
                    // A place in the bar hands the main screen back from the panel's lyrics or queue.
                    sheet.panelPage = null
                    if (item.route() == null) { openQueues(); return@go }
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
            // A tap on a bar or rail place: a tick as the page changes (a lighter one on a reselect,
            // which scrolls up or pops back), then [go]. Places opened from elsewhere (links, the
            // shell navigator) stay silent.
            val haptics = LocalHapticFeedback.current
            val tap: (NavItem) -> Unit = remember(go, haptics) {
                { item ->
                    haptics.performHapticFeedback(if (item == currentSelected) HapticFeedbackType.SegmentFrequentTick else HapticFeedbackType.SegmentTick)
                    go(item)
                }
            }
            val shell = remember(items, nav) { ShellNavigator(items, goTo = go, openAccount = { accountOpen = true }, openBarEditor = { editorOpen = true }, openAvailableOffline = { nav.navigate(Route.AvailableOffline) { launchSingleTop = true } }) }
            // Search opened from outside (a launcher shortcut, Assistant): the search screen takes the
            // query and consumes the link.
            LaunchedEffect(nav) {
                app.hocket.AppLinks.search.collect { link ->
                    if (link == null) return@collect
                    launch { sheet.collapse() }
                    if (NavItem.Search in currentItems) { if (currentSelected != NavItem.Search) go(NavItem.Search) }
                    else nav.navigate(Route.Search) { launchSingleTop = true }
                }
            }
            val bottomInset = with(density) { bottomInsetPx.toDp() }
            // The side panel is remembered: left open, it opens again with the app (and on turning
            // back to landscape); a panel never turns into the full-screen player on a rotation.
            val context = androidx.compose.ui.platform.LocalContext.current
            val shellPrefs = remember(context) { context.getSharedPreferences(SHELL_PREFS, android.content.Context.MODE_PRIVATE) }
            LaunchedEffect(sheet, shellLayout.panel, playerVisible) {
                if (!shellLayout.panel || !playerVisible || sheet.isExpanded || !shellPrefs.getBoolean(PANEL_OPEN, false)) return@LaunchedEffect
                androidx.compose.runtime.snapshotFlow { sheet.collapsedOffset > 0f }.first { it }
                sheet.expand()
            }
            LaunchedEffect(sheet, shellLayout.panel) {
                if (!shellLayout.panel) return@LaunchedEffect
                androidx.compose.runtime.snapshotFlow { sheet.draggable.settledValue to sheet.fullScreen }.drop(1).collect { (value, full) ->
                    if (!full) shellPrefs.edit().putBoolean(PANEL_OPEN, value == app.hocket.ui.player.SheetValue.Expanded).apply()
                }
            }
            var wasPanel by remember { mutableStateOf(shellLayout.panel) }
            LaunchedEffect(shellLayout.panel) {
                if (wasPanel && !shellLayout.panel && sheet.isExpanded && !sheet.fullScreen) sheet.collapse()
                wasPanel = shellLayout.panel
            }
            CompositionLocalProvider(LocalShellNavigator provides shell, LocalTabReselected provides reselected, LocalBottomBarInset provides bottomInset) {
            // As the full player rises, the page behind it recedes: it shrinks a little into rounded
            // corners over black, as a card pushed back under the sheet (read in the draw phase, so
            // dragging the sheet never recomposes the page).
            val recede = !LocalReducedMotion.current
            // Coming out of the splash ([ShellEntrance]): the page rises and settles into place, the
            // bar and the mini player slide up after it (with reduced motion, all of it just fades).
            val entrance = rememberShellEntrance()
            val entranceMoves = !LocalReducedMotion.current
            val entranceRise = with(density) { 32.dp.toPx() }
            val miniRise = with(density) { 64.dp.toPx() }
            // (The sheet's own scrim dims the page itself as it recedes.)
            if (recede) Box(Modifier.fillMaxSize().graphicsLayer { alpha = sheet.coverProgress.coerceIn(0f, 1f) }.background(Color.Black))
            Row(
                Modifier.fillMaxSize().then(hiddenWhenCovered).graphicsLayer {
                    val p = sheet.coverProgress.coerceIn(0f, 1f)
                    // Fully under the open player: not drawn at all (and left at its own size,
                    // so whatever reads its bounds meanwhile sees the page as laid out).
                    val e = entrancePart(entrance(), ShellEntrance.CONTENT_START_MS, ShellEntrance.CONTENT_END_MS)
                    alpha = if (p >= 1f) 0f else e
                    if (recede && p > 0f && p < 1f) {
                        val s = 1f - RecedeScale * p
                        scaleX = s
                        scaleY = s
                        shape = RoundedCornerShape(RecedeCorner * p)
                        clip = true
                    } else if (entranceMoves && e < 1f) {
                        val s = 0.94f + 0.06f * e
                        scaleX = s
                        scaleY = s
                        translationY = (1f - e) * entranceRise
                    }
                },
            ) {
                if (wide) {
                    // A standing rail (not a modal one): its menu button shows the places' labels
                    // beside their icons, and it stays that way (remembered across rotation).
                    var railOpen by rememberSaveable { mutableStateOf(false) }
                    val railState = rememberWideNavigationRailState(if (railOpen) WideNavigationRailValue.Expanded else WideNavigationRailValue.Collapsed)
                    LaunchedEffect(railOpen) { if (railOpen) railState.expand() else railState.collapse() }
                    val railLabel = stringResource(if (railOpen) R.string.nav_rail_collapse else R.string.nav_rail_expand)
                    Box(Modifier.onSizeChanged { railWidthPx = it.width }) {
                        WideNavigationRail(
                            state = railState,
                            header = {
                                androidx.compose.material3.IconButton(onClick = { railOpen = !railOpen }, modifier = Modifier.padding(start = 24.dp).testTag("navRail.menu")) {
                                    Icon(if (railOpen) HocketIcons.Filled.MenuOpen else HocketIcons.Filled.Menu, railLabel)
                                }
                            },
                        ) {
                            items.forEach { item ->
                                val isSelected = item == selected
                                WideNavigationRailItem(
                                    selected = isSelected,
                                    onClick = { tap(item) },
                                    icon = { Icon(item.icon(isSelected), null) },
                                    label = { Text(item.label()) },
                                    railExpanded = railState.targetValue == WideNavigationRailValue.Expanded,
                                    modifier = Modifier.testTag("navRail." + item.id),
                                )
                            }
                        }
                    }
                }
                // Content runs to the bottom of the screen: the floating navigation bar and the
                // mini player float over it, over the scrim below ([BottomContentInset]).
                // Beside the open side panel the page narrows to what the panel leaves.
                // Open, or opening (not closing): read from where it rests and where it is animating,
                // never from a drag's nearest anchor, so the page does not reflow under a drag.
                val panelOpen by remember(sheet, shellLayout.panel) {
                    derivedStateOf {
                        val d = sheet.draggable
                        val expanded = app.hocket.ui.player.SheetValue.Expanded
                        shellLayout.panel && !sheet.fullScreen && if (d.isAnimationRunning) d.targetValue == expanded else d.settledValue == expanded
                    }
                }
                val panelRoom by androidx.compose.animation.core.animateDpAsState(
                    if (panelOpen) PanelWidth + app.hocket.ui.player.HeroGeometry.MINI_MARGIN * 2 else 0.dp,
                    if (LocalReducedMotion.current) androidx.compose.animation.core.snap() else androidx.compose.animation.core.spring(dampingRatio = 1f, stiffness = 400f),
                    label = "panelRoom",
                )
                Box(Modifier.weight(1f).fillMaxSize()) {
                    val contentNavigator = remember(nav, sharedArtwork) {
                        app.hocket.ui.DetailNavigator(
                            { id -> sharedArtwork.prepareDestination(ArtworkShare.album(id)); nav.navigate(Route.Album(id)) },
                            { id -> sharedArtwork.prepareDestination(ArtworkShare.artist(id)); nav.navigate(Route.Artist(id)) },
                            { id -> sharedArtwork.prepareDestination(ArtworkShare.playlist(id)); nav.navigate(Route.Playlist(id)) },
                        )
                    }
                    CompositionLocalProvider(app.hocket.ui.LocalDetailNavigator provides contentNavigator) {
                        // Under the panel's lyrics or queue, the page leaves the accessibility tree.
                        AppNavHost(nav, transitions, Modifier.fillMaxSize().padding(end = panelRoom).then(if (panelOpen && sheet.panelPage != null) Modifier.clearAndSetSemantics { } else Modifier))
                    }
                    // The panel's Lyrics and Queue take over the main screen beside it (the lyrics'
                    // backdrop runs on behind the panel too).
                    PanelPageHost(page = if (panelOpen) sheet.panelPage else null, onClose = { sheet.panelPage = null }, endInset = panelRoom)
                }
            }
            // Links inside the full player (album/artist, the "More" sheet's go-to actions, rows'
            // accessibility actions) collapse it as they navigate: the page opens in view, not
            // behind the player.
            // (The side panel is part of the page: it stays open, the page it opens showing beside it.)
            val asPanel by rememberUpdatedState(shellLayout.panel)
            val navigator = remember(nav, sheet, sharedArtwork) {
                val leavePlayer = {
                    sheet.panelPage = null
                    if (!asPanel || sheet.fullScreen) scope.launch { sheet.collapse() }
                }
                app.hocket.ui.DetailNavigator(
                    openAlbum = { id -> sharedArtwork.prepareDestination(ArtworkShare.album(id)); leavePlayer(); nav.navigate(Route.Album(id)) },
                    openArtist = { id -> sharedArtwork.prepareDestination(ArtworkShare.artist(id)); leavePlayer(); nav.navigate(Route.Artist(id)) },
                    openPlaylist = { id -> sharedArtwork.prepareDestination(ArtworkShare.playlist(id)); leavePlayer(); nav.navigate(Route.Playlist(id)) },
                )
            }
            // The bottom chrome's whole strip (the bar, the gap above it and the margins beside the
            // mini player, all on the solid scrim) takes touches: under the sheet and the bar, so
            // those still get theirs, but over the page, so a row hidden behind them is never tapped.
            if (!wide || playerVisible) {
                val chromeHeight = bottomInset + (if (playerVisible && !barRow) MiniPlayerGap + app.hocket.ui.player.miniPlayerHeight() else 0.dp)
                // Beside the rail on wide layouts (its lowest places stay tappable).
                val railStart = if (wide) with(density) { (if (railWidthPx >= 0) railWidthPx else CollapsedRailWidth.roundToPx()).toDp() } else 0.dp
                Box(Modifier.align(Alignment.BottomCenter).fillMaxWidth().padding(start = railStart).height(chromeHeight).then(if (covered) Modifier else Modifier.blockTouchesBehind()).testTag("bottomChromeBlocker"))
            }
            CompositionLocalProvider(app.hocket.ui.LocalDetailNavigator provides navigator) {
            // The mini player's part of the entrance: up from below the bar, last of all. (At the
            // sheet's own zIndex, which only orders it among this Box's siblings from here: above
            // the bottom scrim, below the bar.)
            Box(Modifier.fillMaxSize().zIndex(10f).graphicsLayer {
                val e = entrancePart(entrance(), ShellEntrance.MINI_START_MS, ShellEntrance.MINI_END_MS)
                if (e < 1f) {
                    alpha = e
                    if (entranceMoves) translationY = (1f - e) * miniRise
                }
            }) {
            NowPlayingSheet(
                state = sheet,
                bottomInset = with(density) { sheetInsetPx.toDp() },
                placement = app.hocket.ui.player.PlayerPlacement(miniWidth = shellLayout.miniWidth, panel = shellLayout.panel),
                onOpenAlbum = navigator.openAlbum,
                onOpenArtist = navigator.openArtist,
            )
            }
            }
            app.hocket.ui.components.SongMenuHost(songMenu, navigator)
            // The scrim the bar and the mini player float on: the surface colour eased in from
            // transparent, so content scrolling under and around them fades out instead of clashing.
            // The status bar's icons for the page on top (a detail page over its cover), unless the
            // full player covers it and sets its own.
            PageStatusBar(backStack.lastOrNull()?.id, appDark = app.hocket.ui.theme.LocalDarkTheme.current, suspended = covered)
            // Phones only: on a large screen the bar and the mini player float in a row (or beside
            // the rail) with their own shadows, and a band across the whole width reads as a shadow.
            if (!wide && !barRow) {
                // Fully faded a little past half-way up the floating mini player (it has its own card
                // and shadow); without one, a gentle fade above the bar. (Beside the bar, the bar's row.)
                val scrimHeight = bottomInset + (if (playerVisible) MiniPlayerGap + app.hocket.ui.player.miniPlayerHeight() * 0.6f else 32.dp)
                // Over a page in its own colours (a detail page in its artwork's), into those instead.
                // (The page's own colours already ease in, so no animation of its own here: the bar
                // and the scrim under it always change together.)
                val surface = LocalPageChrome.current.chromeScheme(backStack.lastOrNull()?.id, MaterialTheme.colorScheme).surface
                // Solid around the bar's lower part (so rows scrolling past its edges barely show
                // through), eased in from well above it.
                val solid = ((bottomInset - 36.dp) / scrimHeight).coerceIn(0f, 0.8f)
                val scrim = remember(surface, solid) { easedScrim(surface, solid = solid) }
                Box(Modifier.align(Alignment.BottomCenter).fillMaxWidth().height(scrimHeight).zIndex(5f).graphicsLayer { alpha = barFade(sheet.coverProgress, SCRIM_FADE_END) * entrancePart(entrance(), ShellEntrance.BAR_START_MS, ShellEntrance.BAR_END_MS) }.background(scrim).testTag("bottomScrim"))
            }
            if (!wide) {
                // Drawn above the sheet so the collapsed sheet body never covers it; slides out as the
                // sheet expands and the full player takes the screen (read in the placement phase, so
                // dragging the sheet does not recompose the bar). Holding it does nothing special: the
                // editor lives in Settings > Customise and the account sheet.
                MaterialTheme(colorScheme = LocalPageChrome.current.chromeScheme(backStack.lastOrNull()?.id, MaterialTheme.colorScheme)) {
                // In the bar row: at the bottom start, beside the mini player, centred on the row.
                val barModifier = Modifier
                    .align(if (barRow) Alignment.BottomStart else Alignment.BottomCenter)
                    .zIndex(20f)
                    .then(if (barRow) Modifier.width(barWidth).padding(bottom = (rowHeight - FloatingNavBarDefaults.PillHeight) / 2) else Modifier)
                    .onSizeChanged { navBarMeasuredPx = it.height }
                    // Its part of the entrance: up from below the screen's edge, after the page.
                    .offset {
                        val e = entrancePart(entrance(), ShellEntrance.BAR_START_MS, ShellEntrance.BAR_END_MS)
                        IntOffset(0, ((sheet.coverProgress + if (entranceMoves) 1f - e else 0f) * navBarHeightPx).roundToInt())
                    }
                    // It fades out gently as the player grows over it.
                    .graphicsLayer { alpha = barFade(sheet.coverProgress) * entrancePart(entrance(), ShellEntrance.BAR_START_MS, ShellEntrance.BAR_END_MS) }
                    .testTag("navBar")
                    // The bar's whole strip (the pill, the margins around it and the system bar
                    // under it, all on the solid scrim) takes the touches its places leave, so
                    // nothing behind it is tapped or scrolled.
                    .blockTouchesBehind()
                    .then(hiddenWhenCovered)
                if (floatingBar) {
                    FloatingNavBar(items = items, selected = selected, onSelect = tap, modifier = barModifier, stash = if (stashAlone) emptyList() else stash, onOpenStash = { stashOpen = true }, alignment = if (barRow) Alignment.CenterStart else Alignment.Center)
                } else {
                    // Transparent over the scrim, along the bottom edge.
                    ShortNavigationBar(containerColor = Color.Transparent, modifier = barModifier) {
                        items.forEach { item ->
                            val isSelected = item == selected
                            ShortNavigationBarItem(
                                selected = isSelected, onClick = { tap(item) }, icon = { Icon(item.icon(isSelected), null) }, label = { Text(item.barLabel(), maxLines = 1) },
                                modifier = Modifier.testTag("navBar." + item.id),
                            )
                        }
                    }
                }
                }
            }
            val sheetRising by remember(sheet) { derivedStateOf { sheet.progress > 1f / 3f } }
            if (stashAlone) {
                // Above the mini player (or the bar when nothing plays) at the end edge, leaving with
                // the bar as the full player rises.
                val miniRoom = if (playerVisible && !barRow) app.hocket.ui.player.miniPlayerHeight() + MiniPlayerGap else 0.dp
                val held = remember { arrayOf(stash) }
                if (stash.isNotEmpty()) held[0] = stash
                val reducedMotion = LocalReducedMotion.current
                // It grows from the corner by the screen edge (the left one right to left).
                val edgeX = if (LocalLayoutDirection.current == androidx.compose.ui.unit.LayoutDirection.Rtl) 0f else 1f
                androidx.compose.animation.AnimatedVisibility(
                    // Not under the full player either: faded out there, it would still take taps.
                    // (Nor over the side panel: it does not cover the screen, but it does cover the chip's corner.)
                    visible = stash.isNotEmpty() && !covered && !sheetRising,
                    enter = if (reducedMotion) androidx.compose.animation.fadeIn() else androidx.compose.animation.scaleIn(androidx.compose.animation.core.spring(dampingRatio = 0.55f, stiffness = 380f), initialScale = 0.3f, transformOrigin = androidx.compose.ui.graphics.TransformOrigin(edgeX, 1f)) +
                        androidx.compose.animation.slideInVertically(androidx.compose.animation.core.spring(dampingRatio = 0.8f, stiffness = 380f)) { it / 2 } + androidx.compose.animation.fadeIn(),
                    exit = if (reducedMotion) androidx.compose.animation.fadeOut() else androidx.compose.animation.scaleOut(androidx.compose.animation.core.spring(stiffness = 600f), targetScale = 0.3f, transformOrigin = androidx.compose.ui.graphics.TransformOrigin(edgeX, 1f)) + androidx.compose.animation.fadeOut(),
                    modifier = Modifier
                        .align(Alignment.BottomEnd)
                        .zIndex(20f)
                        .padding(end = app.hocket.ui.player.HeroGeometry.MINI_MARGIN, bottom = bottomInset + miniRoom + 8.dp)
                        .offset { IntOffset(0, (sheet.coverProgress * navBarHeightPx).roundToInt()) }
                        .graphicsLayer { alpha = (1f - sheet.progress * 3f).coerceIn(0f, 1f) * entrancePart(entrance(), ShellEntrance.BAR_START_MS, ShellEntrance.BAR_END_MS) }
                        .then(hiddenWhenCovered),
                ) {
                    app.hocket.ui.stash.StashChip(held[0], onClick = { stashOpen = true })
                }
            }
            if (stashOpen) app.hocket.ui.stash.StashSheet(onDismiss = { stashOpen = false })
            // Snackbars sit just above the mini player (or the bar when nothing plays), centred on
            // the content beside the rail on wide layouts; as the full player opens they ease down
            // to the bottom of the screen instead of floating over its controls.
            val miniPx = if (playerVisible && !barRow) with(density) { (app.hocket.ui.player.miniPlayerHeight() + MiniPlayerGap).roundToPx() } else 0
            val marginPx = with(density) { 8.dp.roundToPx() }
            val collapsedBottomPx = bottomInsetPx + miniPx + marginPx + with(density) { stashRoom.roundToPx() }
            val expandedBottomPx = systemBottomPx + marginPx
            val railPx = if (wide) (if (railWidthPx >= 0) railWidthPx else with(density) { CollapsedRailWidth.roundToPx() }) else 0
            SnackbarHost(
                snackbar,
                Modifier
                    .align(Alignment.BottomCenter)
                    .zIndex(30f)
                    .padding(start = with(density) { railPx.toDp() }, bottom = with(density) { collapsedBottomPx.toDp() })
                    .offset { IntOffset(0, (sheet.coverProgress * (collapsedBottomPx - expandedBottomPx)).roundToInt()) },
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

/** How far up the sheet the bar has faded out completely. */
private const val BAR_FADE_END = 0.6f

/** How far up the sheet the scrim under the bar has faded out: sooner, so it never washes over the rising player. */
private const val SCRIM_FADE_END = 0.35f

/**
 * The bar's (or, with [end] = [SCRIM_FADE_END], its scrim's) opacity at sheet progress [p]: a
 * smoothstep from opaque at rest to gone at [end], so it eases away with the player instead of
 * snapping out at the start.
 */
internal fun barFade(p: Float, end: Float = BAR_FADE_END): Float {
    val t = (p / end).coerceIn(0f, 1f)
    return 1f - t * t * (3f - 2f * t)
}

/**
 * A vertical gradient from transparent (top) to [color] (bottom) on a smoothstep curve, so the scrim
 * eases in slowly from its top edge and levels off towards the bar instead of showing a band. It
 * peaks at [SCRIM_PEAK] (a hint of the page shows through); the lowest [solid] share holds there.
 * [fromTop] flips it for a top bar: fully opaque at the top (nothing reads through behind the
 * bar's title and icons), easing out downwards on a smootherstep curve. Pass [endY] when drawing it
 * past the drawing area's bottom: otherwise it spans that area, not the rect it fills.
 */
internal fun easedScrim(color: Color, stops: Int = 24, solid: Float = 0f, fromTop: Boolean = false, endY: Float = Float.POSITIVE_INFINITY): Brush {
    val peak = if (fromTop) color else color.copy(alpha = color.alpha * SCRIM_PEAK)
    // Smootherstep for the top's longer fade: flat at both ends, so neither the solid edge nor the tail shows a line.
    if (fromTop) return Brush.verticalGradient(
        0f to peak,
        *Array(stops) { i ->
            val t = 1f - i / (stops - 1f)
            solid + (1f - t) * (1f - solid) to color.copy(alpha = peak.alpha * t * t * t * (t * (t * 6f - 15f) + 10f))
        },
        endY = endY,
    )
    return Brush.verticalGradient(
        *Array(stops) { i ->
            val t = i / (stops - 1f)
            t * (1f - solid) to color.copy(alpha = peak.alpha * t * t * (3f - 2f * t))
        },
        1f to peak,
    )
}

/** The bottom scrim's strongest opacity. */
private const val SCRIM_PEAK = 0.94f

/**
 * A NavHost destination drawn inside [PageTransitionLayer], so every screen (places, details,
 * settings pages) takes part in the Miuix push/pop and predictive back animation the same way.
 * [LocalNavAnimatedVisibilityScope] and [LocalNavPageId] let artwork on the page join a shared-element flight.
 */
private inline fun <reified T : Any> NavGraphBuilder.screen(transitions: ShellTransitionInfo, noinline content: @Composable (NavBackStackEntry) -> Unit) {
    composable<T> { entry ->
        CompositionLocalProvider(LocalNavAnimatedVisibilityScope provides this, LocalNavPageId provides entry.id) {
            PageTransitionLayer(transitions, entry.id) { content(entry) }
        }
    }
}

@Composable
private fun AppNavHost(nav: NavHostController, transitions: ShellTransitionInfo, modifier: Modifier) {
    SharedTransitionLayout(modifier) {
        CompositionLocalProvider(LocalSharedTransitionScope provides this) {
            NavHost(
                nav,
                startDestination = Route.Home,
                modifier = Modifier.fillMaxSize().clipToBounds(),
                enterTransition = { transitions.enter(this) },
                exitTransition = { transitions.exit(this) },
                popEnterTransition = { transitions.popEnter(this) },
                popExitTransition = { transitions.popExit(this) },
                predictivePopEnterTransition = { edge -> transitions.predictivePopEnter(this, edge) },
                predictivePopExitTransition = { edge -> transitions.predictivePopExit(this, edge) },
                sizeTransform = null,
            ) {
        screen<Route.Home>(transitions) { HomeScreen(nav) }
        screen<Route.Library>(transitions) { LibraryScreen(nav) }
        screen<Route.Search>(transitions) { SearchScreen(nav) }
        screen<Route.Settings>(transitions) { SettingsScreen(nav) }
        screen<Route.Downloads>(transitions) { DownloadsScreen(nav) }
        screen<Route.Albums>(transitions) { app.hocket.ui.screens.library.LibraryListScreen(nav, app.hocket.ui.screens.library.LibraryList.Albums) }
        screen<Route.Artists>(transitions) { app.hocket.ui.screens.library.LibraryListScreen(nav, app.hocket.ui.screens.library.LibraryList.Artists) }
        screen<Route.Playlists>(transitions) { app.hocket.ui.screens.library.LibraryListScreen(nav, app.hocket.ui.screens.library.LibraryList.Playlists) }
        screen<Route.Songs>(transitions) { app.hocket.ui.screens.library.LibraryListScreen(nav, app.hocket.ui.screens.library.LibraryList.Songs) }
        screen<Route.Snoozed>(transitions) { app.hocket.ui.screens.library.SnoozedScreen(nav) }
        screen<Route.Genres>(transitions) { app.hocket.ui.screens.library.LibraryListScreen(nav, app.hocket.ui.screens.library.LibraryList.Genres) }
        screen<Route.AvailableOffline>(transitions) { app.hocket.ui.screens.library.AvailableOfflineScreen(nav) }
        screen<Route.Filters>(transitions) { FiltersScreen(nav) }
        screen<Route.FilterBuilder>(transitions) { entry -> FilterBuilderScreen(nav, entry.toRoute<Route.FilterBuilder>().id) }
        screen<Route.Stats>(transitions) { StatsScreen(nav) }
        screen<Route.Album>(transitions) { entry -> AlbumDetailScreen(nav, entry.toRoute<Route.Album>().id) }
        screen<Route.Artist>(transitions) { entry -> ArtistDetailScreen(nav, entry.toRoute<Route.Artist>().id) }
        screen<Route.Playlist>(transitions) { entry -> PlaylistDetailScreen(nav, entry.toRoute<Route.Playlist>().id) }
        screen<Route.Genre>(transitions) { entry -> GenreDetailScreen(nav, entry.toRoute<Route.Genre>().name) }
        screen<Route.AudioSettings>(transitions) { AudioSettingsScreen(nav) }
        screen<Route.TranscodingSettings>(transitions) { TranscodingSettingsScreen(nav) }
        screen<Route.ConnectSettings>(transitions) { ConnectSettingsScreen(nav) }
        screen<Route.CustomiseSettings>(transitions) { CustomiseSettingsScreen(nav) }
        screen<Route.About>(transitions) { AboutScreen(nav) }
        screen<Route.SettingsAccount>(transitions) { AccountSettingsScreen(nav) }
        screen<Route.SettingsAppearance>(transitions) { AppearanceSettingsScreen(nav) }
        screen<Route.SettingsPlayback>(transitions) { PlaybackSettingsScreen(nav) }
        screen<Route.SettingsDownloads>(transitions) { DownloadsSettingsScreen(nav) }
        screen<Route.SettingsLyrics>(transitions) { LyricsSettingsScreen(nav) }
        screen<Route.SettingsPebble>(transitions) { PebbleSettingsScreen(nav) }
        screen<Route.SettingsLibrary>(transitions) { LibrarySettingsScreen(nav) }
        screen<Route.SettingsBattery>(transitions) { BatterySettingsScreen(nav) }
        screen<Route.SettingsBackup>(transitions) { BackupSettingsScreen(nav) }
        screen<Route.SettingsAdvanced>(transitions) { AdvancedSettingsScreen(nav) }
        screen<Route.DeveloperDiagnostics>(transitions) { DeveloperDiagnosticsScreen(nav) }
            }
            // Composed after the NavHost so it registers later and outranks the NavHost's handler.
            NavigationSettleBackGuard(nav, transitions)
        }
    }
}

/**
 * While a push/pop animation is still playing, back is a plain pop rather than a predictive gesture
 * (see [ShellTransitionInfo.navigating]). Popping mid-push makes the NavHost reverse the push from
 * where it is; popping mid-pop retargets it. At rest this is disabled and the NavHost gets the
 * gesture as usual.
 */
@Composable
private fun NavigationSettleBackGuard(nav: NavHostController, transitions: ShellTransitionInfo) {
    DisposableEffect(nav) {
        val listener = NavController.OnDestinationChangedListener { _, _, _ -> transitions.navigating = true }
        nav.addOnDestinationChangedListener(listener)
        onDispose { nav.removeOnDestinationChangedListener(listener) }
    }
    val visible by nav.visibleEntries.collectAsState()
    LaunchedEffect(transitions.navigating, visible) {
        // Read the flow's live value: the collected one can lag a navigate() by a frame.
        if (transitions.navigating && nav.visibleEntries.value.size <= 1) transitions.navigating = false
    }
    BackHandler(enabled = transitions.navigating && nav.previousBackStackEntry != null) {
        // `enabled` lags a pop by a frame; never pop the start destination on a second quick back.
        if (nav.previousBackStackEntry != null) nav.popBackStack()
    }
}
