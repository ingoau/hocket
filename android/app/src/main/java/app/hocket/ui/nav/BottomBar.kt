package app.hocket.ui.nav

import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.foundation.gestures.waitForUpOrCancellation
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.QueueMusic
import androidx.compose.material.icons.automirrored.outlined.QueueMusic
import androidx.compose.material.icons.filled.Add
import androidx.compose.material.icons.filled.Album
import androidx.compose.material.icons.filled.BarChart
import androidx.compose.material.icons.filled.Category
import androidx.compose.material.icons.filled.Download
import androidx.compose.material.icons.filled.DragHandle
import androidx.compose.material.icons.filled.FilterAlt
import androidx.compose.material.icons.filled.History
import androidx.compose.material.icons.filled.Home
import androidx.compose.material.icons.filled.LibraryMusic
import androidx.compose.material.icons.filled.MusicNote
import androidx.compose.material.icons.filled.Person
import androidx.compose.material.icons.filled.RemoveCircleOutline
import androidx.compose.material.icons.filled.Search
import androidx.compose.material.icons.outlined.Album
import androidx.compose.material.icons.outlined.BarChart
import androidx.compose.material.icons.outlined.Category
import androidx.compose.material.icons.outlined.Download
import androidx.compose.material.icons.outlined.FilterAlt
import androidx.compose.material.icons.outlined.History
import androidx.compose.material.icons.outlined.Home
import androidx.compose.material.icons.outlined.LibraryMusic
import androidx.compose.material.icons.outlined.MusicNote
import androidx.compose.material.icons.outlined.Person
import androidx.compose.material.icons.outlined.Search
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.key
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.hapticfeedback.HapticFeedbackType
import androidx.compose.ui.input.pointer.PointerEventPass
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalHapticFeedback
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.CustomAccessibilityAction
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.customActions
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.navigation.NavBackStackEntry
import androidx.navigation.NavDestination
import androidx.navigation.NavDestination.Companion.hasRoute
import androidx.navigation.NavGraph
import androidx.navigation.NavGraph.Companion.findStartDestination
import androidx.navigation.NavHostController
import app.hocket.HocketApp
import app.hocket.InMemoryNavBarPrefs
import app.hocket.NavBarPrefs
import app.hocket.R
import app.hocket.core.Commands
import app.hocket.ui.LocalCoreClient
import kotlinx.coroutines.launch
import sh.calvin.reorderable.ReorderableColumn

/*
 * The phone's bottom bar: which places it holds (a device-local preference), what is selected, the
 * long-press shortcut to its editor, and the editor itself (Settings > Customise > Bottom bar, or
 * the sheet the long-press opens).
 */

/** The bar's storage for this composition; AppRoot provides one so every reader shares it. */
val LocalNavBarPrefs = staticCompositionLocalOf<NavBarPrefs?> { null }

/** The app's DataStore-backed prefs when running in the app, else an in-memory store. */
@Composable
fun rememberNavBarPrefs(): NavBarPrefs {
    val provided = LocalNavBarPrefs.current
    val app = LocalContext.current.applicationContext as? HocketApp
    val fallback = remember { InMemoryNavBarPrefs() }
    return provided ?: app?.prefs ?: fallback
}

/** The user's bar, migrated and bounded ([NavItem.fromIds]). */
@Composable
fun navItems(prefs: NavBarPrefs = rememberNavBarPrefs()): List<NavItem> {
    val ids by prefs.navItems.collectAsStateWithLifecycle(initialValue = null)
    return remember(ids) { NavItem.fromIds(ids ?: emptyList()) }
}

/**
 * What the shell offers the screens inside it: switching to a place (as the bar does), the
 * account sheet and the bar editor. Null outside the main shell (setup, screens rendered alone).
 */
class ShellNavigator(
    val items: List<NavItem>,
    val goTo: (NavItem) -> Unit,
    val openAccount: () -> Unit,
    val openBarEditor: () -> Unit,
    /** The "Available offline" list (from the offline indicator). */
    val openAvailableOffline: () -> Unit = {},
)

val LocalShellNavigator = staticCompositionLocalOf<ShellNavigator?> { null }

fun NavItem.route(): Route = when (this) {
    NavItem.Home -> Route.Home
    NavItem.Search -> Route.Search
    NavItem.Library -> Route.Library()
    NavItem.Playlists -> Route.Playlists
    NavItem.Artists -> Route.Artists
    NavItem.Albums -> Route.Albums
    NavItem.Songs -> Route.Songs
    NavItem.Genres -> Route.Genres
    NavItem.RecentQueues -> Route.SavedQueues
    NavItem.Stats -> Route.Stats
    NavItem.Downloads -> Route.Downloads
    NavItem.Filters -> Route.Filters
}

/** Whether [destination] is this place's own screen. */
fun NavItem.isScreen(destination: NavDestination): Boolean = when (this) {
    NavItem.Home -> destination.hasRoute(Route.Home::class)
    NavItem.Search -> destination.hasRoute(Route.Search::class)
    NavItem.Library -> destination.hasRoute(Route.Library::class)
    NavItem.Playlists -> destination.hasRoute(Route.Playlists::class)
    NavItem.Artists -> destination.hasRoute(Route.Artists::class)
    NavItem.Albums -> destination.hasRoute(Route.Albums::class)
    NavItem.Songs -> destination.hasRoute(Route.Songs::class)
    NavItem.Genres -> destination.hasRoute(Route.Genres::class)
    NavItem.RecentQueues -> destination.hasRoute(Route.SavedQueues::class)
    NavItem.Stats -> destination.hasRoute(Route.Stats::class)
    NavItem.Downloads -> destination.hasRoute(Route.Downloads::class)
    NavItem.Filters -> destination.hasRoute(Route.Filters::class)
}

/**
 * The selected place: the one whose back stack is showing. Switching places pops to the start
 * destination and pushes the place's screen, so the stack is `[start, place, ...deeper]`, and the
 * shell remembers that root entry ([rootEntryId], from [goToPlace]). While it is the entry above
 * the start, its place is selected (an album opened from Albums keeps Albums, from Library keeps
 * Library; Settings pushed from the account button keeps the place it was opened over), or none
 * when that place is not in the bar. Otherwise the stack is the start's own (a screen pushed from
 * Home, or nothing above it), and the start's place is selected.
 */
fun selectedPlace(backStack: List<NavBackStackEntry>, items: List<NavItem>, rootEntryId: String?): NavItem? {
    val entries = backStack.filter { it.destination !is NavGraph }
    val root = entries.getOrNull(1)
    if (root != null && root.id == rootEntryId) return NavItem.entries.firstOrNull { it.isScreen(root.destination) }?.takeIf { it in items }
    return entries.firstOrNull()?.let { start -> items.firstOrNull { it.isScreen(start.destination) } }
}

/**
 * Switches to [item] the way the bar does: its own back stack, saved and restored. Returns the id
 * of the stack's root entry (the one above the start destination), null for the start's own.
 */
fun NavHostController.goToPlace(item: NavItem): String? {
    navigate(item.route()) {
        popUpTo(graph.findStartDestination().id) { saveState = true }
        launchSingleTop = true
        restoreState = true
    }
    return currentBackStack.value.filter { it.destination !is NavGraph }.getOrNull(1)?.id
}

fun NavItem.icon(selected: Boolean): ImageVector = when (this) {
    NavItem.Home -> if (selected) Icons.Filled.Home else Icons.Outlined.Home
    NavItem.Search -> if (selected) Icons.Filled.Search else Icons.Outlined.Search
    NavItem.Library -> if (selected) Icons.Filled.LibraryMusic else Icons.Outlined.LibraryMusic
    NavItem.Playlists -> if (selected) Icons.AutoMirrored.Filled.QueueMusic else Icons.AutoMirrored.Outlined.QueueMusic
    NavItem.Artists -> if (selected) Icons.Filled.Person else Icons.Outlined.Person
    NavItem.Albums -> if (selected) Icons.Filled.Album else Icons.Outlined.Album
    NavItem.Songs -> if (selected) Icons.Filled.MusicNote else Icons.Outlined.MusicNote
    NavItem.Genres -> if (selected) Icons.Filled.Category else Icons.Outlined.Category
    NavItem.RecentQueues -> if (selected) Icons.Filled.History else Icons.Outlined.History
    NavItem.Stats -> if (selected) Icons.Filled.BarChart else Icons.Outlined.BarChart
    NavItem.Downloads -> if (selected) Icons.Filled.Download else Icons.Outlined.Download
    NavItem.Filters -> if (selected) Icons.Filled.FilterAlt else Icons.Outlined.FilterAlt
}

@Composable
fun NavItem.label(): String = stringResource(
    when (this) {
        NavItem.Home -> R.string.nav_home
        NavItem.Search -> R.string.nav_search
        NavItem.Library -> R.string.nav_library
        NavItem.Playlists -> R.string.nav_playlists
        NavItem.Artists -> R.string.nav_artists
        NavItem.Albums -> R.string.nav_albums
        NavItem.Songs -> R.string.nav_songs
        NavItem.Genres -> R.string.nav_genres
        NavItem.RecentQueues -> R.string.nav_recent_queues
        NavItem.Stats -> R.string.nav_stats
        NavItem.Downloads -> R.string.nav_downloads
        NavItem.Filters -> R.string.nav_filters
    },
)

/** The label under the bar icon: [label], shortened where the full one does not fit five across. */
@Composable
fun NavItem.barLabel(): String = if (this == NavItem.RecentQueues) stringResource(R.string.nav_recent_queues_short) else label()

/**
 * A long press anywhere on the bar calls [onLongPress] (the editor shortcut). It watches the
 * gesture before the items do, and once it fires it swallows the rest of the gesture so the item
 * under the finger does not also navigate. A tap passes through untouched.
 */
fun Modifier.longPressToEdit(onLongPress: () -> Unit): Modifier = pointerInput(onLongPress) {
    awaitEachGesture {
        awaitFirstDown(requireUnconsumed = false, pass = PointerEventPass.Initial)
        val up = withTimeoutOrNull(viewConfiguration.longPressTimeoutMillis) { waitForUpOrCancellation(PointerEventPass.Initial) }
        if (up == null) {
            onLongPress()
            do {
                val event = awaitPointerEvent(PointerEventPass.Initial)
                event.changes.forEach { it.consume() }
            } while (event.changes.any { it.pressed })
        }
    }
}

/** Saves the bar and keeps the core's `sidebar` surface in step (as the navigation items always have). */
@Composable
fun rememberBarSaver(prefs: NavBarPrefs = rememberNavBarPrefs()): (List<NavItem>) -> Unit {
    val client = LocalCoreClient.current
    val scope = rememberCoroutineScope()
    return remember(prefs, client, scope) {
        { next ->
            val ids = if (next == NavItem.DEFAULT) emptyList() else next.map { it.id }
            scope.launch { prefs.setNavItems(ids) }
            client.dispatch(Commands.setActionOrder("sidebar", next.map { it.canonicalActionId }.distinct()))
        }
    }
}

/**
 * The bottom-bar editor: the bar's places in order (drag handle, or the row's Move up / Move down
 * actions for TalkBack; a remove button), "Add item" with the places not in the bar, and "Reset to
 * default". Two to five places: trying to go past either end says why instead of doing it.
 */
@Composable
fun BottomBarEditor(modifier: Modifier = Modifier) {
    val prefs = rememberNavBarPrefs()
    val items = navItems(prefs)
    val save = rememberBarSaver(prefs)
    val haptics = LocalHapticFeedback.current
    var message by remember { mutableStateOf<String?>(null) }
    var adding by remember { mutableStateOf(false) }
    val moveUp = stringResource(R.string.a11y_move_up)
    val moveDown = stringResource(R.string.a11y_move_down)
    val fullMessage = stringResource(R.string.bottom_bar_full)
    val minimumMessage = stringResource(R.string.bottom_bar_minimum)
    fun update(next: List<NavItem>) { message = null; save(next) }
    Column(modifier.testTag("bottomBar.editor")) {
        Text(stringResource(R.string.bottom_bar_hint), style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant, modifier = Modifier.padding(horizontal = 16.dp, vertical = 8.dp))
        ReorderableColumn(
            list = items,
            onSettle = { from, to -> update(items.toMutableList().apply { add(to, removeAt(from)) }) },
            onMove = { haptics.performHapticFeedback(HapticFeedbackType.SegmentFrequentTick) },
            modifier = Modifier.fillMaxWidth(),
        ) { index, item, _ ->
            key(item.id) {
                ReorderableItem {
                    val label = item.label()
                    val position = stringResource(R.string.bottom_bar_position, index + 1, items.size)
                    Row(
                        Modifier.fillMaxWidth()
                            .semantics(mergeDescendants = true) {
                                stateDescription = position
                                customActions = buildList {
                                    if (index > 0) add(CustomAccessibilityAction(moveUp) { update(items.toMutableList().apply { add(index - 1, removeAt(index)) }); true })
                                    if (index < items.lastIndex) add(CustomAccessibilityAction(moveDown) { update(items.toMutableList().apply { add(index + 1, removeAt(index)) }); true })
                                }
                            }
                            .testTag("bottomBar.item.${item.id}")
                            .heightIn(min = 56.dp)
                            .padding(start = 16.dp, end = 4.dp),
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        Icon(item.icon(false), null, tint = MaterialTheme.colorScheme.onSurfaceVariant)
                        Spacer(Modifier.width(16.dp))
                        Text(label, style = MaterialTheme.typography.bodyLarge, modifier = Modifier.weight(1f))
                        IconButton(
                            onClick = { if (items.size <= NavItem.MIN) message = minimumMessage else update(items - item) },
                            modifier = Modifier.testTag("bottomBar.remove.${item.id}"),
                        ) { Icon(Icons.Filled.RemoveCircleOutline, stringResource(R.string.bottom_bar_remove, label)) }
                        Box(Modifier.padding(12.dp).draggableHandle(onDragStarted = { haptics.performHapticFeedback(HapticFeedbackType.GestureThresholdActivate) })) {
                            Icon(Icons.Filled.DragHandle, null)
                        }
                    }
                }
            }
        }
        message?.let {
            Text(it, color = MaterialTheme.colorScheme.error, style = MaterialTheme.typography.bodyMedium,
                modifier = Modifier.padding(horizontal = 16.dp, vertical = 4.dp).semantics { liveRegion = LiveRegionMode.Polite }.testTag("bottomBar.message"))
        }
        FlowRow(Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 8.dp), horizontalArrangement = Arrangement.spacedBy(8.dp), itemVerticalAlignment = Alignment.CenterVertically) {
            Box {
                OutlinedButton(onClick = { if (items.size >= NavItem.MAX) message = fullMessage else { message = null; adding = true } }, modifier = Modifier.testTag("bottomBar.add")) {
                    Icon(Icons.Filled.Add, null)
                    Spacer(Modifier.width(8.dp))
                    Text(stringResource(R.string.bottom_bar_add))
                }
                DropdownMenu(expanded = adding, onDismissRequest = { adding = false }) {
                    NavItem.entries.filter { it !in items }.forEach { item ->
                        DropdownMenuItem(
                            text = { Text(item.label()) },
                            leadingIcon = { Icon(item.icon(false), null) },
                            onClick = { adding = false; update(items + item) },
                            modifier = Modifier.testTag("bottomBar.addItem.${item.id}"),
                        )
                    }
                }
            }
            TextButton(onClick = { update(NavItem.DEFAULT) }, enabled = items != NavItem.DEFAULT, modifier = Modifier.testTag("bottomBar.reset")) {
                Text(stringResource(R.string.bottom_bar_reset))
            }
        }
    }
}
