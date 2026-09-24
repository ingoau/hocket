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
        Box(Modifier.fillMaxSize().background(MaterialTheme.colorScheme.background)) {
            // The debug banner is laid out in flow above the app (never over its top bars): it takes
            // the status-bar inset and consumes it, so the screens below do not pad for it again.
            Column(Modifier.fillMaxSize()) {
                if (fakeBanner) FakeCoreBanner()
                Box(Modifier.weight(1f).fillMaxWidth().then(if (fakeBanner) Modifier.consumeWindowInsets(BannerInsets) else Modifier)) {
                    when {
                        !started || !ready -> LoadingScreen()
                        server == null || !hasLogin || !server.capabilities.meetsFloor -> ServerSetupScreen(existing = server, needsRelogin = needsRelogin)
                        else -> MainShell()
                    }
                }
            }
            SnackbarHost(snackbar, Modifier.align(Alignment.BottomCenter).padding(bottom = 96.dp)) { data -> Snackbar(data) }
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

/** Bottom content inset for scrolling screens: the mini player (taller at large font sizes) floats over the last rows. */
val BottomContentInset: Dp
    @Composable get() = app.hocket.ui.player.miniPlayerHeight() + 32.dp

@Composable
private fun MainShell() {
    val nav = rememberNavController()
    val sheet = rememberNowPlayingSheetState()
    BoxWithConstraints(Modifier.fillMaxSize()) {
        val wide = maxWidth >= 600.dp
        CompositionLocalProvider(LocalWideLayout provides wide) {
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
            var devicesOpen by remember { mutableStateOf(false) }
            // The phone navigation bar is measured (its height includes the edge-to-edge navigation-bar
            // inset) so the content column ends above it and the sheet's collapsed anchor sits on it.
            var navBarHeightPx by remember { mutableIntStateOf(0) }
            val systemBottomPx = WindowInsets.navigationBars.getBottom(density)
            val bottomInsetPx = if (wide) systemBottomPx else navBarHeightPx
            // While the full player covers the screen, what is behind it (the page, the navigation
            // bar and rail) leaves the accessibility tree, as a modal would: TalkBack must not wander
            // into content nobody can see.
            val covered by remember(sheet) { derivedStateOf { sheet.progress >= 0.6f } }
            val hiddenWhenCovered = if (covered) Modifier.clearAndSetSemantics { } else Modifier
            fun go(item: NavItem) { rootEntryId = nav.goToPlace(item) }
            val shell = remember(items, nav) { ShellNavigator(items, goTo = { rootEntryId = nav.goToPlace(it) }, openAccount = { accountOpen = true }, openBarEditor = { editorOpen = true }, openAvailableOffline = { nav.navigate(Route.AvailableOffline) { launchSingleTop = true } }) }
            val editLabel = stringResource(R.string.bottom_bar_edit)
            val openEditor = remember { { haptics.performHapticFeedback(HapticFeedbackType.LongPress); editorOpen = true } }
            CompositionLocalProvider(LocalShellNavigator provides shell) {
            Row(Modifier.fillMaxSize().then(hiddenWhenCovered)) {
                if (wide) {
                    val railState = rememberWideNavigationRailState(WideNavigationRailValue.Collapsed)
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
                // Content ends above the navigation bar; only the mini player floats over it.
                Box(Modifier.weight(1f).fillMaxSize().padding(bottom = with(density) { bottomInsetPx.toDp() })) {
                    val contentNavigator = remember(nav) { app.hocket.ui.DetailNavigator({ nav.navigate(Route.Album(it)) }, { nav.navigate(Route.Artist(it)) }) }
                    CompositionLocalProvider(app.hocket.ui.LocalDetailNavigator provides contentNavigator) {
                        AppNavHost(nav, Modifier.fillMaxSize())
                    }
                }
            }
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
                onOpenAlbum = { nav.navigate(Route.Album(it)) },
                onOpenArtist = { nav.navigate(Route.Artist(it)) },
            )
            }
            if (!wide) {
                // Drawn above the sheet so the collapsed sheet body never covers it; slides out as the
                // sheet expands and the full player takes the screen. A long press opens its editor
                // (also a TalkBack action on every item).
                ShortNavigationBar(
                    modifier = Modifier
                        .align(Alignment.BottomCenter)
                        .zIndex(20f)
                        .onSizeChanged { navBarHeightPx = it.height }
                        .offset { IntOffset(0, (sheet.progress * navBarHeightPx).roundToInt()) }
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

@Composable
private fun AppNavHost(nav: NavHostController, modifier: Modifier) {
    NavHost(nav, startDestination = Route.Home, modifier = modifier) {
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
