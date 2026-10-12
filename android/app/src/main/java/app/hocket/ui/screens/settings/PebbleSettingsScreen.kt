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
import app.hocket.ui.components.ChoiceRow
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
        ItemsEditor(SettingKeys.PEBBLE_MENU, R.string.pebble_menu, R.string.pebble_menu_body, PebbleProtocol.MENU_ITEMS, PebbleProtocol.DEFAULT_MENU)
        ItemsEditor(SettingKeys.PEBBLE_ACTIONS, R.string.pebble_actions, R.string.pebble_actions_body, PebbleProtocol.ACTION_ITEMS, PebbleProtocol.DEFAULT_ACTIONS)
        ItemsEditor(SettingKeys.PEBBLE_LIBRARY, R.string.pebble_library, R.string.pebble_library_body, PebbleProtocol.LIBRARY_SECTIONS, PebbleProtocol.LIBRARY_SECTIONS)
        val seconds = timeout.int ?: 5
        val off = stringResource(R.string.settings_off)
        SettingRow(stringResource(R.string.pebble_menu_timeout), if (seconds == 0) off else stringResource(R.string.pebble_seconds, seconds), timeout.scope, tag = "pebble.menuTimeoutSeconds")
        ChoiceRow(listOf(0, 3, 5, 10).map { it to if (it == 0) off else stringResource(R.string.pebble_seconds, it) }, isSelected = { seconds == it }, onSelect = { timeout.setInt(it) }, modifier = Modifier.padding(horizontal = 16.dp))

        SettingsSection(stringResource(R.string.pebble_now_playing))
        SwitchRow(stringResource(R.string.pebble_volume_controls), volume.bool ?: true, { volume.setBool(it) }, stringResource(R.string.pebble_volume_controls_body), volume.scope, tag = "pebble.showVolumeControls")
        SwitchRow(stringResource(R.string.pebble_progress_bar), progress.bool ?: true, { progress.setBool(it) }, stringResource(R.string.pebble_progress_bar_body), progress.scope, tag = "pebble.showProgressBar")
        SwitchRow(stringResource(R.string.pebble_album_art), art.bool ?: true, { art.setBool(it) }, stringResource(R.string.pebble_album_art_body), art.scope, tag = "pebble.showAlbumArt")
        SwitchRow(stringResource(R.string.pebble_auto_open), autoOpen.bool ?: false, { autoOpen.setBool(it) }, stringResource(R.string.pebble_auto_open_body), autoOpen.scope, tag = "pebble.autoOpen")
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
    val options = PebbleProtocol.GESTURES.map { it to gestureLabel(it) }
    SettingRow(stringResource(title), options.firstOrNull { it.first == current }?.second, handle.scope, tag = key)
    ChoiceRow(options, isSelected = { current == it }, onSelect = { handle.setString(it) }, modifier = Modifier.padding(horizontal = 16.dp))
}

/** A choose-and-order list stored as ids; an empty list is the default. */
@Composable
private fun ItemsEditor(key: String, title: Int, body: Int, all: List<String>, default: List<String>) {
    val handle = setting(key)
    val ids = handle.raw?.let { runCatching { HocketJson.json.decodeFromString(ListSerializer(String.serializer()), it) }.getOrNull() }?.ifEmpty { null } ?: default
    SettingRow(stringResource(title), stringResource(body), handle.scope, tag = key)
    ChooseAndOrder(all, ids, minEnabled = 1, label = { itemLabel(it) }, tag = "$key.editor") { next ->
        handle.setRaw(HocketJson.json.encodeToString(ListSerializer(String.serializer()), if (next == default) emptyList() else next))
    }
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
