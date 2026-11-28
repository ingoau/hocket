package app.hocket.ui.screens.settings

import androidx.compose.foundation.layout.padding
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import androidx.navigation.NavHostController
import app.hocket.R
import app.hocket.core.HocketJson
import app.hocket.core.SettingKeys
import app.hocket.playback.pebble.PebbleInstaller
import app.hocket.playback.pebble.PebbleProtocol
import androidx.compose.ui.graphics.vector.ImageVector
import app.hocket.ui.icons.HocketIcons
import app.hocket.ui.nav.Route
import kotlinx.coroutines.launch
import kotlinx.serialization.builtins.ListSerializer
import kotlinx.serialization.builtins.serializer

/**
 * Pebble watch: how the Hocket watchapp behaves (docs/pebble.md, Settings) and installing it. All
 * `pebble.*` settings are device-local: the watch belongs to this phone. The phone pushes them to
 * the watch, which keeps them while disconnected.
 */
@Composable
fun PebbleSettingsScreen(nav: NavHostController) {
    val touchNav = setting(SettingKeys.PEBBLE_TOUCH_NAVIGATION)
    val buttonsOnly = setting(SettingKeys.PEBBLE_BUTTONS_ONLY)
    val timeout = setting(SettingKeys.PEBBLE_MENU_TIMEOUT_SECONDS)
    val volume = setting(SettingKeys.PEBBLE_SHOW_VOLUME_CONTROLS)
    val progress = setting(SettingKeys.PEBBLE_SHOW_PROGRESS_BAR)
    val art = setting(SettingKeys.PEBBLE_SHOW_ALBUM_ART)
    val autoOpen = setting(SettingKeys.PEBBLE_AUTO_OPEN)
    val closeWhenPaused = setting(SettingKeys.PEBBLE_CLOSE_WHEN_PAUSED_MINUTES)
    SubScreen(nav, stringResource(R.string.settings_category_pebble)) {
        WatchappRow()
        SwitchRow(stringResource(R.string.pebble_touch_navigation), touchNav.bool ?: false, { touchNav.setBool(it) }, stringResource(R.string.pebble_touch_navigation_body), touchNav.scope, tag = "pebble.touchNavigation")
        SwitchRow(stringResource(R.string.pebble_buttons_only), buttonsOnly.bool ?: false, { buttonsOnly.setBool(it) }, stringResource(R.string.pebble_buttons_only_body), buttonsOnly.scope, tag = "pebble.buttonsOnly")

        SettingsSection(stringResource(R.string.pebble_gestures))
        GestureChoice(SettingKeys.PEBBLE_GESTURE_UP, R.string.pebble_gesture_up, "queue")
        GestureChoice(SettingKeys.PEBBLE_GESTURE_DOWN, R.string.pebble_gesture_down, "none")
        GestureChoice(SettingKeys.PEBBLE_GESTURE_LEFT, R.string.pebble_gesture_left, "library")
        GestureChoice(SettingKeys.PEBBLE_GESTURE_RIGHT, R.string.pebble_gesture_right, "back")
        GestureChoice(SettingKeys.PEBBLE_GESTURE_HOLD, R.string.pebble_gesture_hold, "actions")

        SettingsSection(stringResource(R.string.pebble_menus))
        PEBBLE_LISTS.forEach { list ->
            val handle = setting(list.key)
            val ids = storedIds(handle) ?: list.default
            NavRow(stringResource(list.title), stringResource(R.string.settings_order_summary, ids.count { it in list.all }, list.all.size), tag = list.key) { nav.navigate(Route.SettingsPage(list.key)) }
        }
        val seconds = timeout.int ?: 5
        val off = stringResource(R.string.settings_off)
        ChoiceDialogRow(stringResource(R.string.pebble_menu_timeout), listOf(0, 3, 5, 10).map { DialogOption(it, if (it == 0) off else stringResource(R.string.pebble_seconds, it)) },
            selected = seconds, onSelect = { timeout.setInt(it) }, scope = timeout.scope, tag = "pebble.menuTimeoutSeconds")

        SettingsSection(stringResource(R.string.pebble_now_playing))
        SwitchRow(stringResource(R.string.pebble_volume_controls), volume.bool ?: true, { volume.setBool(it) }, scope = volume.scope, tag = "pebble.showVolumeControls")
        SwitchRow(stringResource(R.string.pebble_progress_bar), progress.bool ?: true, { progress.setBool(it) }, scope = progress.scope, tag = "pebble.showProgressBar")
        SwitchRow(stringResource(R.string.pebble_album_art), art.bool ?: true, { art.setBool(it) }, scope = art.scope, tag = "pebble.showAlbumArt")
        SwitchRow(stringResource(R.string.pebble_auto_open), autoOpen.bool ?: false, { autoOpen.setBool(it) }, scope = autoOpen.scope, tag = "pebble.autoOpen")
        val minutes = closeWhenPaused.int ?: 0
        ChoiceDialogRow(stringResource(R.string.pebble_close_when_paused), listOf(0, 1, 5, 10, 30).map { DialogOption(it, if (it == 0) off else stringResource(R.string.pebble_minutes, it)) },
            selected = minutes, onSelect = { closeWhenPaused.setInt(it) }, scope = closeWhenPaused.scope, tag = "pebble.closeWhenPausedMinutes")
    }
}

/** The bundled watchapp's version, the one the watch last reported, and Install (only when bundled). */
@Composable
private fun WatchappRow() {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    val bundled = remember { PebbleInstaller.isBundled(context) }
    val onWatch = remember { PebbleInstaller.watchVersion(context) }
    val status = when {
        !bundled -> stringResource(R.string.pebble_not_bundled)
        onWatch == null -> stringResource(R.string.pebble_status_unknown, PebbleInstaller.bundledVersion)
        onWatch == PebbleInstaller.bundledVersion -> stringResource(R.string.pebble_status_current, onWatch)
        else -> stringResource(R.string.pebble_status_outdated, onWatch, PebbleInstaller.bundledVersion)
    }
    SettingRow(stringResource(R.string.pebble_watchapp), status, tag = "pebble.watchapp") {
        if (bundled) Button(shapes = ButtonDefaults.shapes(), onClick = { scope.launch { PebbleInstaller.install(context) } }) { Text(stringResource(R.string.pebble_install)) }
    }
}

@Composable
private fun GestureChoice(key: String, title: Int, default: String) {
    val handle = setting(key)
    val current = handle.string ?: default
    ChoiceDialogRow(stringResource(title), PebbleProtocol.GESTURES.map { DialogOption(it, gestureLabel(it), gestureIcon(it)) },
        selected = current, onSelect = { handle.setString(it) }, scope = handle.scope, tag = key)
}

private fun gestureIcon(id: String): ImageVector = when (id) {
    "queue" -> HocketIcons.AutoMirrored.Filled.QueueMusic
    "library" -> HocketIcons.Filled.LibraryMusic
    "actions" -> HocketIcons.Filled.MoreVert
    "back" -> HocketIcons.AutoMirrored.Filled.ArrowBack
    "lyrics" -> HocketIcons.Filled.Lyrics
    else -> HocketIcons.Filled.Block
}

/** A choose-and-order list of the watchapp's, stored as ids (an empty list is the default). */
private class PebbleList(val key: String, val title: Int, val body: Int, val all: List<String>, val default: List<String>)

private val PEBBLE_LISTS = listOf(
    PebbleList(SettingKeys.PEBBLE_MENU, R.string.pebble_menu, R.string.pebble_menu_body, PebbleProtocol.MENU_ITEMS, PebbleProtocol.DEFAULT_MENU),
    PebbleList(SettingKeys.PEBBLE_ACTIONS, R.string.pebble_actions, R.string.pebble_actions_body, PebbleProtocol.ACTION_ITEMS, PebbleProtocol.DEFAULT_ACTIONS),
    PebbleList(SettingKeys.PEBBLE_LIBRARY, R.string.pebble_library, R.string.pebble_library_body, PebbleProtocol.LIBRARY_SECTIONS, PebbleProtocol.LIBRARY_SECTIONS),
)

/** A Pebble list's own page ([key] is its setting): what it is for, then choose and order. */
@Composable
internal fun PebbleItemsPage(nav: NavHostController, key: String) {
    val list = PEBBLE_LISTS.firstOrNull { it.key == key } ?: return
    val handle = setting(key)
    SubScreen(nav, stringResource(list.title)) {
        Text(stringResource(list.body), style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant, modifier = Modifier.padding(horizontal = 16.dp, vertical = 4.dp))
        ChooseAndOrder(list.all, storedIds(handle) ?: list.default, minEnabled = 1, label = { itemLabel(it) }, icon = ::itemIcon, default = list.default, tag = "$key.editor") { next ->
            handle.setRaw(HocketJson.json.encodeToString(ListSerializer(String.serializer()), if (next == list.default) emptyList() else next))
        }
    }
}

private fun itemIcon(id: String): ImageVector = when (id) {
    "nowPlaying" -> HocketIcons.Filled.PlayCircle
    "queue" -> HocketIcons.AutoMirrored.Filled.QueueMusic
    "library" -> HocketIcons.Filled.LibraryMusic
    "love" -> HocketIcons.Filled.Favorite
    "stash" -> HocketIcons.Filled.Inventory2
    "rating" -> HocketIcons.Filled.Star
    "shuffle" -> HocketIcons.Filled.Shuffle
    "repeat" -> HocketIcons.Filled.Repeat
    "goAlbum", "albums" -> HocketIcons.Filled.Album
    "goArtist", "artists" -> HocketIcons.Filled.Person
    "playOn" -> HocketIcons.Outlined.Devices
    "playlists" -> HocketIcons.AutoMirrored.Filled.PlaylistPlay
    "recent" -> HocketIcons.Filled.History
    "downloads" -> HocketIcons.Filled.Download
    "search" -> HocketIcons.Filled.Search
    "lyrics" -> HocketIcons.Filled.Lyrics
    else -> HocketIcons.Filled.MusicNote
}

@Composable
private fun gestureLabel(id: String): String = stringResource(
    when (id) {
        "queue" -> R.string.pebble_item_queue
        "library" -> R.string.pebble_item_library
        "actions" -> R.string.pebble_gesture_actions
        "back" -> R.string.pebble_gesture_back
        "lyrics" -> R.string.pebble_item_lyrics
        else -> R.string.pebble_gesture_none
    },
)

@Composable
private fun itemLabel(id: String): String = stringResource(
    when (id) {
        "nowPlaying" -> R.string.pebble_item_now_playing
        "queue" -> R.string.pebble_item_queue
        "library" -> R.string.pebble_item_library
        "love" -> R.string.pebble_item_love
        "stash" -> R.string.pebble_item_stash
        "rating" -> R.string.pebble_item_rating
        "shuffle" -> R.string.pebble_item_shuffle
        "repeat" -> R.string.pebble_item_repeat
        "goAlbum" -> R.string.pebble_item_go_album
        "goArtist" -> R.string.pebble_item_go_artist
        "playOn" -> R.string.pebble_item_play_on
        "playlists" -> R.string.pebble_item_playlists
        "albums" -> R.string.pebble_item_albums
        "artists" -> R.string.pebble_item_artists
        "recent" -> R.string.pebble_item_recent
        "downloads" -> R.string.pebble_item_downloads
        "search" -> R.string.pebble_item_search
        "lyrics" -> R.string.pebble_item_lyrics
        else -> R.string.pebble_item_now_playing
    },
)
