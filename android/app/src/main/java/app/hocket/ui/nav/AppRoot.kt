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
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.IntOffset
import androidx.compose.ui.zIndex
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
    CompositionLocalProvider(LocalCoreClient provides client) {
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
        Box(Modifier.fillMaxSize().background(MaterialTheme.colorScheme.background)) {
            when {
                !started || !ready -> LoadingScreen()
                server == null || !hasLogin || !server.capabilities.meetsFloor -> ServerSetupScreen(existing = server, needsRelogin = needsRelogin)
                else -> MainShell()
            }
            SnackbarHost(snackbar, Modifier.align(Alignment.BottomCenter).padding(bottom = 96.dp)) { data -> Snackbar(data) }
            if (BuildConfig.DEBUG && client.kind == CoreKind.Fake) {
                Box(Modifier.align(Alignment.TopCenter).statusBarsPadding().fillMaxWidth().background(MaterialTheme.colorScheme.errorContainer)) {
                    Text(stringResource(R.string.debug_fake_core_banner), style = MaterialTheme.typography.labelSmall, color = MaterialTheme.colorScheme.onErrorContainer, modifier = Modifier.padding(horizontal = 12.dp, vertical = 4.dp))
                }
            }
        }
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

/** Event.Toast -> snackbar with its single action dispatching `action_command`; Event.Error -> snackbar. */
@Composable
private fun ToastCollector(host: SnackbarHostState) {
    val client = LocalCoreClient.current
    val authPrefix = stringResource(R.string.error_auth)
    val networkPrefix = stringResource(R.string.error_network)
    LaunchedEffect(client) {
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

private fun NavItem.icon(selected: Boolean): ImageVector = when (this) {
    NavItem.Home -> if (selected) Icons.Filled.Home else Icons.Outlined.Home
    NavItem.Library -> if (selected) Icons.Filled.LibraryMusic else Icons.Outlined.LibraryMusic
    NavItem.Search -> if (selected) Icons.Filled.Search else Icons.Outlined.Search
    NavItem.Downloads -> if (selected) Icons.Filled.Download else Icons.Outlined.Download
    NavItem.Filters -> if (selected) Icons.Filled.FilterAlt else Icons.Outlined.FilterAlt
    NavItem.Stats -> if (selected) Icons.Filled.BarChart else Icons.Outlined.BarChart
    NavItem.Settings -> if (selected) Icons.Filled.Settings else Icons.Outlined.Settings
}

@Composable
private fun NavItem.label(): String = when (this) {
    NavItem.Home -> stringResource(R.string.nav_home)
    NavItem.Library -> stringResource(R.string.nav_library)
    NavItem.Search -> stringResource(R.string.nav_search)
    NavItem.Downloads -> stringResource(R.string.nav_downloads)
    NavItem.Filters -> stringResource(R.string.nav_filters)
    NavItem.Stats -> stringResource(R.string.nav_stats)
    NavItem.Settings -> stringResource(R.string.nav_settings)
}

private fun NavItem.route(): Route = when (this) {
    NavItem.Home -> Route.Home
    NavItem.Library -> Route.Library()
    NavItem.Search -> Route.Search
    NavItem.Downloads -> Route.Downloads
    NavItem.Filters -> Route.Filters
    NavItem.Stats -> Route.Stats
    NavItem.Settings -> Route.Settings
}

private fun NavItem.matches(routeName: String?): Boolean = routeName != null && routeName.contains(
    when (this) {
        NavItem.Home -> "Route.Home"; NavItem.Library -> "Route.Library"; NavItem.Search -> "Route.Search"; NavItem.Downloads -> "Route.Downloads"
        NavItem.Filters -> "Route.Filters"; NavItem.Stats -> "Route.Stats"; NavItem.Settings -> "Route.Settings"
    },
)

/** The user's ordered navigation items: an app-local preference (not a core setting). */
@Composable
fun navItems(): List<NavItem> {
    val app = LocalContext.current.applicationContext as? HocketApp
    val ids by (app?.prefs?.navItems ?: kotlinx.coroutines.flow.flowOf(emptyList())).collectAsStateWithLifecycle(initialValue = emptyList())
    return remember(ids) { if (ids.isEmpty()) NavItem.DEFAULT else NavItem.fromIds(ids) }
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
            val backStack by nav.currentBackStackEntryAsState()
            val routeName = backStack?.destination?.route
            val scope = rememberCoroutineScope()
            val density = LocalDensity.current
            // The phone navigation bar is measured (its height includes the edge-to-edge navigation-bar
            // inset) so the content column ends above it and the sheet's collapsed anchor sits on it.
            var navBarHeightPx by remember { mutableIntStateOf(0) }
            val systemBottomPx = WindowInsets.navigationBars.getBottom(density)
            val bottomInsetPx = if (wide) systemBottomPx else navBarHeightPx
            fun go(item: NavItem) {
                nav.navigate(item.route()) { popUpTo(nav.graph.startDestinationId) { saveState = true }; launchSingleTop = true; restoreState = true }
            }
            Row(Modifier.fillMaxSize()) {
                if (wide) {
                    val railState = rememberWideNavigationRailState(WideNavigationRailValue.Collapsed)
                    ModalWideNavigationRail(state = railState, hideOnCollapse = false) {
                        items.forEach { item ->
                            val selected = item.matches(routeName)
                            WideNavigationRailItem(
                                selected = selected,
                                onClick = { go(item); scope.launch { railState.collapse() } },
                                icon = { Icon(item.icon(selected), null) },
                                label = { Text(item.label()) },
                                railExpanded = railState.targetValue == WideNavigationRailValue.Expanded,
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
                // sheet expands and the full player takes the screen.
                ShortNavigationBar(
                    modifier = Modifier
                        .align(Alignment.BottomCenter)
                        .zIndex(20f)
                        .onSizeChanged { navBarHeightPx = it.height }
                        .offset { IntOffset(0, (sheet.progress * navBarHeightPx).roundToInt()) }
                        .testTag("navBar"),
                ) {
                    items.forEach { item ->
                        val selected = item.matches(routeName)
                        ShortNavigationBarItem(selected = selected, onClick = { go(item) }, icon = { Icon(item.icon(selected), null) }, label = { Text(item.label()) }, modifier = Modifier.testTag("navBar." + item.id))
                    }
                }
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
