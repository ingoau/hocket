package app.hocket.ui.screens.settings

import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.padding
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.ChevronRight
import androidx.compose.material3.ButtonGroup
import androidx.compose.material3.ButtonGroupDefaults
import androidx.compose.material3.Icon
import androidx.compose.material3.Slider
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.navigation.NavHostController
import app.hocket.R
import app.hocket.core.Commands
import app.hocket.core.Queries
import app.hocket.core.SettingKeys
import app.hocket.core.api.Command
import app.hocket.core.api.QueryResult
import app.hocket.core.api.QueueMode
import app.hocket.playback.CoreHost
import app.hocket.ui.LocalCoreClient
import app.hocket.ui.components.ConfirmDialog
import app.hocket.ui.components.formatAgo
import app.hocket.ui.components.formatBytes
import app.hocket.ui.nav.Route
import kotlinx.coroutines.launch
import kotlin.math.roundToInt

/*
 * The settings categories that are not an older sub-screen of their own (Audio, Streaming &
 * transcoding, Connect, Customise and About live in SubSettingsScreens.kt). Every row carries a
 * stable `tag` (test tag `setting.<tag>`): SettingsCategoriesTest walks every category and checks
 * that each setting is in exactly one of them.
 */

@Composable
private fun Chevron() = Icon(Icons.Filled.ChevronRight, null)

/** Account & server: the settings-sync master switch, the server, sync now / full sync, sign out. */
@Composable
fun AccountSettingsScreen(nav: NavHostController) {
    val client = LocalCoreClient.current
    val server by client.server.collectAsStateWithLifecycle()
    val sync = setting(SettingKeys.SYNC_ENABLED)
    var signOut by remember { mutableStateOf(false) }
    SubScreen(nav, stringResource(R.string.settings_category_account)) {
        server?.let { s ->
            SettingRow(stringResource(R.string.settings_server_body, s.url, s.username), s.lastSync?.let { stringResource(R.string.settings_last_sync, formatAgo(it)) } ?: stringResource(R.string.settings_never_synced), tag = "server.info")
            SettingRow(stringResource(R.string.settings_sync_now), onClick = { client.dispatch(Commands.syncLibrary(s.id, full = false)) }, tag = "server.syncNow")
            SettingRow(stringResource(R.string.settings_full_sync), onClick = { client.dispatch(Commands.syncLibrary(s.id, full = true)) }, tag = "server.fullSync")
            SettingRow(stringResource(R.string.settings_remove_server), onClick = { signOut = true }, tag = "server.remove")
        }
        SwitchRow(stringResource(R.string.settings_sync_master), sync.bool ?: true, { client.dispatch(Commands.setSettingsSync(it)) }, stringResource(R.string.settings_sync_master_body), tag = "sync.master")
    }
    // Sign-out goes through CoreHost.removeServer: the stored login is removed before RemoveServer,
    // so nothing can replay it and resurrect the server.
    if (signOut) server?.let { s -> ConfirmDialog(stringResource(R.string.settings_remove_server_confirm, s.name), stringResource(R.string.settings_remove_server), onConfirm = { CoreHost.removeServer(client::dispatch, s) }, onDismiss = { signOut = false }) }
}

/** Appearance: theme, accent colour, colour from the artwork, animated background. */
@Composable
fun AppearanceSettingsScreen(nav: NavHostController) {
    val theme = setting(SettingKeys.DISPLAY_THEME)
    val accent = setting(SettingKeys.DISPLAY_ACCENT)
    val dynamicColour = setting(SettingKeys.DISPLAY_DYNAMIC_COLOUR)
    val animated = setting(SettingKeys.DISPLAY_ANIMATED_BACKGROUND)
    SubScreen(nav, stringResource(R.string.settings_section_appearance)) {
        val themeOptions = listOf("system" to stringResource(R.string.settings_theme_system), "light" to stringResource(R.string.settings_theme_light), "dark" to stringResource(R.string.settings_theme_dark))
        SettingRow(stringResource(R.string.settings_theme), scope = theme.scope, tag = "display.theme") {
            ButtonGroup(overflowIndicator = {}, horizontalArrangement = Arrangement.spacedBy(ButtonGroupDefaults.ConnectedSpaceBetween)) {
                themeOptions.forEach { (v, label) -> toggleableItem(checked = (theme.string ?: "system") == v, label = label, onCheckedChange = { theme.setString(v) }) }
            }
        }
        // display.accent: null = follow the wallpaper (dynamic) / the artwork; a #RRGGBB overrides it.
        val accentValue = accent.string
        val dynamicLabel = stringResource(R.string.settings_accent_dynamic)
        SettingRow(stringResource(R.string.settings_accent), scope = accent.scope, tag = "display.accent") {
            ButtonGroup(overflowIndicator = {}, horizontalArrangement = Arrangement.spacedBy(ButtonGroupDefaults.ConnectedSpaceBetween)) {
                toggleableItem(checked = accentValue == null, label = dynamicLabel, onCheckedChange = { accent.setRaw("null") })
                listOf("#6750A4", "#1B6B5E", "#B3261E").forEach { hex -> toggleableItem(checked = accentValue.equals(hex, true), label = "●", onCheckedChange = { accent.setString(hex) }) }
            }
        }
        SwitchRow(stringResource(R.string.settings_accent_artwork), dynamicColour.bool ?: true, { dynamicColour.setBool(it) }, scope = dynamicColour.scope, tag = "display.artworkColour")
        SwitchRow(stringResource(R.string.settings_animated_background), animated.bool ?: true, { animated.setBool(it) }, stringResource(R.string.settings_animated_background_body), animated.scope, tag = "display.animatedBackground")
    }
}

/** Playback & queue: queue mode, how many recent queues to keep, autoplay, sleep-timer defaults. */
@Composable
fun PlaybackSettingsScreen(nav: NavHostController) {
    val client = LocalCoreClient.current
    val queue by client.queue.collectAsStateWithLifecycle()
    val savedCap = setting(SettingKeys.QUEUE_SAVED_CAP)
    val mode = setting(SettingKeys.QUEUE_MODE)
    val sleepMinutes = setting(SettingKeys.SLEEP_DEFAULT_MINUTES)
    val sleepEnd = setting(SettingKeys.SLEEP_STOP_AT_END_OF_TRACK)
    SubScreen(nav, stringResource(R.string.settings_category_playback)) {
        val current = queue.mode
        val appleLabel = stringResource(R.string.queue_mode_apple)
        val youTubeLabel = stringResource(R.string.queue_mode_youtube)
        SettingRow(stringResource(R.string.queue_mode), scope = mode.scope, tag = "queue.mode") {
            ButtonGroup(overflowIndicator = {}, horizontalArrangement = Arrangement.spacedBy(ButtonGroupDefaults.ConnectedSpaceBetween)) {
                toggleableItem(checked = current == QueueMode.Apple, label = appleLabel, onCheckedChange = { client.dispatch(Commands.setQueueMode(QueueMode.Apple)) })
                toggleableItem(checked = current == QueueMode.YouTube, label = youTubeLabel, onCheckedChange = { client.dispatch(Commands.setQueueMode(QueueMode.YouTube)) })
            }
        }
        val cap = savedCap.int ?: 10
        var dragging by remember(cap) { mutableFloatStateOf(cap.toFloat()) }
        val shown = dragging.roundToInt()
        SettingRow(stringResource(R.string.settings_saved_cap), if (shown == 0) stringResource(R.string.settings_saved_cap_zero) else stringResource(R.string.saved_cap, shown), savedCap.scope, tag = "queue.savedCap")
        Slider(
            value = dragging, onValueChange = { dragging = it }, onValueChangeFinished = { client.dispatch(Commands.setSavedQueueCap(dragging.roundToInt())) },
            valueRange = 0f..50f, steps = 49, modifier = Modifier.padding(horizontal = 16.dp).testTag("queue.savedCap.slider"),
        )
        SwitchRow(stringResource(R.string.settings_autoplay), queue.autoplay, { client.dispatch(Commands.setAutoplay(it)) }, tag = "queue.autoplay")
        val minutes = sleepMinutes.int ?: 30
        SettingRow(stringResource(R.string.settings_sleep_default), stringResource(R.string.sleep_minutes, minutes), sleepMinutes.scope, tag = "sleep.defaultMinutes")
        ButtonGroup(overflowIndicator = {}, modifier = Modifier.padding(horizontal = 16.dp), horizontalArrangement = Arrangement.spacedBy(ButtonGroupDefaults.ConnectedSpaceBetween)) {
            listOf(15, 30, 45, 60, 90).forEach { m -> toggleableItem(checked = minutes == m, label = m.toString(), onCheckedChange = { sleepMinutes.setInt(m) }) }
        }
        SwitchRow(stringResource(R.string.sleep_end_of_track), sleepEnd.bool ?: true, { sleepEnd.setBool(it) }, scope = sleepEnd.scope, tag = "sleep.stopAtEndOfTrack")
    }
}

/** Downloads & storage: the downloads screen, stream cache, storage warning, Wi-Fi only. */
@Composable
fun DownloadsSettingsScreen(nav: NavHostController) {
    val client = LocalCoreClient.current
    val storage by client.storage.collectAsStateWithLifecycle()
    val wifiOnly = setting(SettingKeys.DOWNLOADS_WIFI_ONLY)
    SubScreen(nav, stringResource(R.string.settings_category_downloads)) {
        SettingRow(stringResource(R.string.nav_downloads), formatBytes(storage.downloadsBytes) + stringResource(R.string.dot_separator) + stringResource(R.string.settings_cache_images, formatBytes(storage.imagesBytes)), onClick = { nav.navigate(Route.Downloads) }, tag = "open.downloads") { Chevron() }
        SwitchRow(stringResource(R.string.settings_downloads_wifi_only), wifiOnly.bool ?: true, { wifiOnly.setBool(it) }, stringResource(R.string.settings_downloads_wifi_only_body), wifiOnly.scope, tag = "downloads.wifiOnly")
        SettingRow(stringResource(R.string.downloads_clear_cache), formatBytes(storage.cacheBytes), onClick = { client.dispatch(Command.ClearStreamCache) }, tag = "storage.clearCache")
        val warn = storage.warnThresholdBytes
        SettingRow(stringResource(R.string.downloads_threshold), warn?.let { formatBytes(it) } ?: stringResource(R.string.downloads_threshold_none), tag = "storage.warnThreshold")
        val noLimit = stringResource(R.string.downloads_threshold_none)
        ButtonGroup(overflowIndicator = {}, modifier = Modifier.padding(horizontal = 16.dp), horizontalArrangement = Arrangement.spacedBy(ButtonGroupDefaults.ConnectedSpaceBetween)) {
            listOf(null, 2.0, 4.0, 8.0, 16.0).forEach { gb -> toggleableItem(checked = (gb?.let { it * 1e9 }) == warn, label = gb?.let { "${it.toInt()} GB" } ?: noLimit, onCheckedChange = { client.dispatch(Commands.setStorageWarnThreshold(gb?.let { it * 1e9 })) }) }
        }
    }
}

/** Lyrics: external lookups, the default timing offset, translations. */
@Composable
fun LyricsSettingsScreen(nav: NavHostController) {
    val client = LocalCoreClient.current
    val externalLyrics = setting(SettingKeys.LYRICS_EXTERNAL_ENABLED)
    val offset = setting(SettingKeys.LYRICS_DEFAULT_OFFSET_MS)
    val translations = setting(SettingKeys.LYRICS_SHOW_TRANSLATIONS)
    SubScreen(nav, stringResource(R.string.settings_section_lyrics)) {
        SwitchRow(stringResource(R.string.settings_external_lyrics), externalLyrics.bool ?: false, { client.dispatch(Commands.setExternalLyricsEnabled(it)) }, stringResource(R.string.settings_external_lyrics_privacy), externalLyrics.scope, tag = "lyrics.external")
        val ms = offset.int ?: 0
        val earlier = stringResource(R.string.lyrics_offset_earlier)
        val reset = stringResource(R.string.lyrics_offset_reset)
        val later = stringResource(R.string.lyrics_offset_later)
        SettingRow(stringResource(R.string.settings_lyrics_default_offset), stringResource(R.string.lyrics_offset_value, ms), offset.scope, tag = "lyrics.defaultOffsetMs")
        ButtonGroup(overflowIndicator = {}, modifier = Modifier.padding(horizontal = 16.dp), horizontalArrangement = Arrangement.spacedBy(ButtonGroupDefaults.ConnectedSpaceBetween)) {
            clickableItem(onClick = { offset.setInt((ms - 100).coerceAtLeast(-10_000)) }, label = earlier)
            clickableItem(onClick = { offset.setInt(0) }, label = reset)
            clickableItem(onClick = { offset.setInt((ms + 100).coerceAtMost(10_000)) }, label = later)
        }
        SwitchRow(stringResource(R.string.settings_lyrics_translations), translations.bool ?: true, { translations.setBool(it) }, scope = translations.scope, tag = "lyrics.showTranslations")
    }
}

/** Library & ratings: the love bridge, smart filters, listening stats. */
@Composable
fun LibrarySettingsScreen(nav: NavHostController) {
    val loveBridge = setting(SettingKeys.RATINGS_LOVE_BRIDGE_ENABLED)
    val loveThreshold = setting(SettingKeys.RATINGS_LOVE_BRIDGE_THRESHOLD)
    SubScreen(nav, stringResource(R.string.settings_category_library)) {
        // ratings.loveBridge.enabled + threshold (1..5): "off" is the bridge disabled.
        val bridgeOn = loveBridge.bool ?: false
        val threshold = loveThreshold.int ?: 4
        SettingRow(stringResource(R.string.settings_love_threshold), if (!bridgeOn) stringResource(R.string.settings_love_threshold_off) else stringResource(R.string.rating_set, threshold), loveThreshold.scope, tag = "ratings.loveThreshold")
        val offLabel = stringResource(R.string.settings_love_threshold_off)
        ButtonGroup(overflowIndicator = {}, modifier = Modifier.padding(horizontal = 16.dp), horizontalArrangement = Arrangement.spacedBy(ButtonGroupDefaults.ConnectedSpaceBetween)) {
            toggleableItem(checked = !bridgeOn, label = offLabel, onCheckedChange = { loveBridge.setBool(false) })
            (1..5).forEach { n -> toggleableItem(checked = bridgeOn && threshold == n, label = "$n★", onCheckedChange = { loveThreshold.setInt(n); loveBridge.setBool(true) }) }
        }
        SettingRow(stringResource(R.string.nav_filters), onClick = { nav.navigate(Route.Filters) }, tag = "open.filters") { Chevron() }
        SettingRow(stringResource(R.string.nav_stats), onClick = { nav.navigate(Route.Stats) }, tag = "open.stats") { Chevron() }
    }
}

/** Battery: the saver mode and whether it follows the system's battery saver. */
@Composable
fun BatterySettingsScreen(nav: NavHostController) {
    val client = LocalCoreClient.current
    val batterySaver by client.batterySaver.collectAsStateWithLifecycle()
    val autoSaver = setting(SettingKeys.BATTERY_AUTO_ENGAGE)
    SubScreen(nav, stringResource(R.string.settings_section_battery)) {
        SwitchRow(stringResource(R.string.settings_battery_saver), batterySaver, { client.dispatch(Commands.setBatterySaver(it)) }, stringResource(R.string.settings_battery_body), tag = "battery.saver")
        SwitchRow(stringResource(R.string.settings_battery_auto), autoSaver.bool ?: true, { autoSaver.setBool(it) }, scope = autoSaver.scope, tag = "battery.autoEngage")
    }
}

/** Backup & diagnostics: export (optionally with secrets), import, copy diagnostics. */
@Composable
fun BackupSettingsScreen(nav: NavHostController) {
    val client = LocalCoreClient.current
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    var importConfirm by remember { mutableStateOf<String?>(null) }
    var includeSecrets by remember { mutableStateOf(false) }
    val exportLauncher = rememberLauncherForActivityResult(ActivityResultContracts.CreateDocument("application/json")) { uri ->
        uri ?: return@rememberLauncherForActivityResult
        scope.launch {
            val doc = (client.query(Queries.configDocument(includeSecrets)) as? QueryResult.Text)?.data ?: return@launch
            context.contentResolver.openOutputStream(uri)?.use { it.write(doc.toByteArray()) }
        }
    }
    val importLauncher = rememberLauncherForActivityResult(ActivityResultContracts.OpenDocument()) { uri ->
        uri ?: return@rememberLauncherForActivityResult
        val text = context.contentResolver.openInputStream(uri)?.use { it.readBytes().decodeToString() } ?: return@rememberLauncherForActivityResult
        importConfirm = text
    }
    SubScreen(nav, stringResource(R.string.settings_category_backup)) {
        SwitchRow(stringResource(R.string.settings_export_with_secrets), includeSecrets, { includeSecrets = it }, tag = "backup.includeSecrets")
        SettingRow(stringResource(R.string.settings_export_config), onClick = { exportLauncher.launch("hocket-config.json") }, tag = "backup.export")
        SettingRow(stringResource(R.string.settings_import_config), onClick = { importLauncher.launch(arrayOf("application/json", "text/plain")) }, tag = "backup.import")
        SettingRow(stringResource(R.string.settings_copy_diagnostics), onClick = {
            scope.launch {
                val text = (client.query(app.hocket.core.api.Query.Diagnostics) as? QueryResult.Text)?.data ?: ""
                (context.getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager).setPrimaryClip(ClipData.newPlainText("Hocket diagnostics", text))
            }
        }, tag = "backup.diagnostics")
    }
    importConfirm?.let { doc -> ConfirmDialog(stringResource(R.string.settings_import_confirm), stringResource(R.string.settings_import_config), onConfirm = { client.dispatch(Commands.importConfig(doc)) }, onDismiss = { importConfirm = null }, destructive = false) }
}
