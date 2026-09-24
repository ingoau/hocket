package app.hocket.ui.screens.settings

import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.selection.selectable
import androidx.compose.foundation.selection.selectableGroup
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material.icons.automirrored.filled.Logout
import androidx.compose.material.icons.filled.Check
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.ToggleButton
import androidx.compose.ui.Alignment
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.clearAndSetSemantics
import app.hocket.core.api.SettingScope
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.ChevronRight
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
import app.hocket.ui.components.ActionRow
import app.hocket.ui.components.LabelledSlider
import androidx.compose.foundation.layout.heightIn
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.stateDescription
import app.hocket.ui.components.ChoiceRow
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
        // The server login is kept on this device only (the keystore); the sync switch has its registry scope.
        server?.let { s ->
            SettingRow(stringResource(R.string.settings_server_body, s.url, s.username), s.lastSync?.let { stringResource(R.string.settings_last_sync, formatAgo(it)) } ?: stringResource(R.string.settings_never_synced), scope = SettingScope.DeviceLocal, tag = "server.info")
            SettingRow(stringResource(R.string.settings_sync_now), onClick = { client.dispatch(Commands.syncLibrary(s.id, full = false)) }, tag = "server.syncNow")
            SettingRow(stringResource(R.string.settings_full_sync), onClick = { client.dispatch(Commands.syncLibrary(s.id, full = true)) }, tag = "server.fullSync")
        }
        SwitchRow(stringResource(R.string.settings_sync_master), sync.bool ?: true, { client.dispatch(Commands.setSettingsSync(it)) }, stringResource(R.string.settings_sync_master_body), scope = sync.scope ?: SettingScope.DeviceLocal, tag = "sync.master")
        // Destructive, and last: nothing below it to hit by mistake.
        server?.let { SettingRow(stringResource(R.string.settings_remove_server), onClick = { signOut = true }, tag = "server.remove", destructive = true) { Icon(Icons.AutoMirrored.Filled.Logout, null, tint = MaterialTheme.colorScheme.error) } }
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
        SettingRow(stringResource(R.string.settings_theme), scope = theme.scope, tag = "display.theme")
        ChoiceRow(themeOptions, isSelected = { (theme.string ?: "system") == it }, onSelect = { theme.setString(it) }, modifier = Modifier.padding(horizontal = 16.dp))
        // display.accent: null = follow the wallpaper (dynamic) / the artwork; a #RRGGBB overrides it.
        val accentValue = accent.string
        val dynamicLabel = stringResource(R.string.settings_accent_dynamic)
        SettingRow(stringResource(R.string.settings_accent), scope = accent.scope, tag = "display.accent")
        AccentChoices(
            dynamicLabel = dynamicLabel,
            swatches = ACCENTS.map { (hex, name) -> hex to stringResource(name) },
            selected = accentValue,
            onSelect = { hex -> if (hex == null) accent.setRaw("null") else accent.setString(hex) },
            modifier = Modifier.padding(horizontal = 16.dp),
        )
        SwitchRow(stringResource(R.string.settings_accent_artwork), dynamicColour.bool ?: true, { dynamicColour.setBool(it) }, scope = dynamicColour.scope, tag = "display.artworkColour")
        SwitchRow(stringResource(R.string.settings_animated_background), animated.bool ?: true, { animated.setBool(it) }, stringResource(R.string.settings_animated_background_body), animated.scope, tag = "display.animatedBackground")
    }
}

/** The accent presets: `display.accent` hex and the colour's name. */
internal val ACCENTS = listOf("#6750A4" to R.string.settings_accent_purple, "#1B6B5E" to R.string.settings_accent_teal, "#B3261E" to R.string.settings_accent_red)

internal fun accentColour(hex: String): Color = Color(android.graphics.Color.parseColor(hex))

/**
 * "Dynamic" as a toggle button, then one round swatch per preset in its real colour. The swatches
 * are radio buttons spoken by their colour name ("Teal, selected"); the selected one has a ring and
 * a check.
 */
@Composable
private fun AccentChoices(dynamicLabel: String, swatches: List<Pair<String, String>>, selected: String?, onSelect: (String?) -> Unit, modifier: Modifier = Modifier) {
    FlowRow(modifier.selectableGroup(), horizontalArrangement = Arrangement.spacedBy(8.dp), verticalArrangement = Arrangement.spacedBy(4.dp), itemVerticalAlignment = Alignment.CenterVertically) {
        ToggleButton(checked = selected == null, onCheckedChange = { onSelect(null) }) { Text(dynamicLabel, maxLines = 1) }
        swatches.forEach { (hex, name) ->
            val isSelected = selected.equals(hex, ignoreCase = true)
            val colour = accentColour(hex)
            Box(
                Modifier.size(48.dp)
                    .selectable(selected = isSelected, role = Role.RadioButton, onClick = { onSelect(hex) })
                    .semantics { contentDescription = name }
                    .testTag("accent.$hex"),
                contentAlignment = Alignment.Center,
            ) {
                Box(
                    Modifier.size(40.dp).clip(CircleShape)
                        .then(if (isSelected) Modifier.border(3.dp, MaterialTheme.colorScheme.onSurface, CircleShape) else Modifier)
                        .padding(if (isSelected) 5.dp else 0.dp)
                        .clip(CircleShape).background(colour),
                    contentAlignment = Alignment.Center,
                ) {
                    if (isSelected) Icon(Icons.Filled.Check, null, tint = Color.White, modifier = Modifier.size(18.dp))
                }
            }
        }
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
        SettingRow(stringResource(R.string.queue_mode), scope = mode.scope, tag = "queue.mode")
        ChoiceRow(listOf(QueueMode.Apple to appleLabel, QueueMode.YouTube to youTubeLabel), isSelected = { current == it }, onSelect = { client.dispatch(Commands.setQueueMode(it)) }, modifier = Modifier.padding(horizontal = 16.dp))
        val cap = savedCap.int ?: 10
        var dragging by remember(cap) { mutableFloatStateOf(cap.toFloat()) }
        val shown = dragging.roundToInt()
        val capLabel = stringResource(R.string.settings_saved_cap)
        val capValue = if (shown == 0) stringResource(R.string.settings_saved_cap_zero) else stringResource(R.string.saved_cap, shown)
        SettingRow(capLabel, capValue, savedCap.scope, tag = "queue.savedCap")
        LabelledSlider(
            value = dragging, onValueChange = { dragging = it }, onValueChangeFinished = { dragging = dragging.roundToInt().toFloat(); client.dispatch(Commands.setSavedQueueCap(dragging.roundToInt())) },
            // Continuous to the eye (49 tick dots crowded the track); snapped to whole queues on release
            // and for TalkBack's adjust, and the value is the row's subtitle above.
            valueRange = 0f..50f, steps = 49, showTicks = false, label = capLabel, valueText = capValue,
            modifier = Modifier.padding(horizontal = 16.dp).testTag("queue.savedCap.slider"),
        )
        SwitchRow(stringResource(R.string.settings_autoplay), queue.autoplay, { client.dispatch(Commands.setAutoplay(it)) }, tag = "queue.autoplay")
        val minutes = sleepMinutes.int ?: 30
        SettingRow(stringResource(R.string.settings_sleep_default), stringResource(R.string.sleep_minutes, minutes), sleepMinutes.scope, tag = "sleep.defaultMinutes")
        val minuteResources = androidx.compose.ui.platform.LocalResources.current
        ChoiceRow(listOf(15, 30, 45, 60, 90).map { it to it.toString() }, isSelected = { minutes == it }, onSelect = { sleepMinutes.setInt(it) }, modifier = Modifier.padding(horizontal = 16.dp),
            describe = { m -> minuteResources.getQuantityString(R.plurals.a11y_minutes, m, m) })
        SwitchRow(stringResource(R.string.sleep_end_of_track), sleepEnd.bool ?: true, { sleepEnd.setBool(it) }, scope = sleepEnd.scope, tag = "sleep.stopAtEndOfTrack")
    }
}

/** Custom stream-cache budgets offered next to "Automatic" (decimal, as sizes are shown). */
internal val CACHE_BUDGET_CHOICES = listOf(0.5e9, 1e9, 2e9, 4e9, 8e9, 16e9)

/** "500 MB", "4 GB": round choice labels, like the warning threshold's. */
private fun budgetChoiceLabel(bytes: Double): String = if (bytes < 1e9) "${(bytes / 1e6).toInt()} MB" else "${(bytes / 1e9).toInt()} GB"

/**
 * Downloads & storage: the downloads screen, the stream cache (usage, budget, prefetch on mobile
 * data, data saved), Wi-Fi only and the storage warning.
 */
@Composable
fun DownloadsSettingsScreen(nav: NavHostController) {
    val client = LocalCoreClient.current
    val storage by client.storage.collectAsStateWithLifecycle()
    val wifiOnly = setting(SettingKeys.DOWNLOADS_WIFI_ONLY)
    val cacheMax = setting(SettingKeys.STORAGE_CACHE_MAX_BYTES)
    val prefetchMobile = setting(SettingKeys.STORAGE_PREFETCH_ON_MOBILE_DATA)
    val dot = stringResource(R.string.dot_separator)
    SubScreen(nav, stringResource(R.string.settings_category_downloads)) {
        SettingRow(stringResource(R.string.nav_downloads), formatBytes(storage.downloadsBytes) + dot + stringResource(R.string.settings_cache_images, formatBytes(storage.imagesBytes)), onClick = { nav.navigate(Route.Downloads) }, tag = "open.downloads") { Chevron() }
        SwitchRow(stringResource(R.string.settings_downloads_wifi_only), wifiOnly.bool ?: true, { wifiOnly.setBool(it) }, stringResource(R.string.settings_downloads_wifi_only_body), wifiOnly.scope, tag = "downloads.wifiOnly")
        val warn = storage.warnThresholdBytes
        SettingRow(stringResource(R.string.downloads_threshold), warn?.let { formatBytes(it) } ?: stringResource(R.string.downloads_threshold_none), tag = "storage.warnThreshold")
        val noLimit = stringResource(R.string.downloads_threshold_none)
        ChoiceRow(listOf(null, 2.0, 4.0, 8.0, 16.0).map { gb -> gb to (gb?.let { "${it.toInt()} GB" } ?: noLimit) }, isSelected = { gb -> (gb?.let { it * 1e9 }) == warn },
            onSelect = { gb -> client.dispatch(Commands.setStorageWarnThreshold(gb?.let { it * 1e9 })) }, modifier = Modifier.padding(horizontal = 16.dp))

        SettingsSection(stringResource(R.string.settings_cache_usage))
        // Usage: complete entries play offline; partial ones (seeks, skips, primed starts) do not.
        val partial = storage.partialCacheBytes ?: 0.0
        val complete = (storage.cacheBytes - partial).coerceAtLeast(0.0)
        val budget = storage.cacheBudgetBytes ?: 0.0
        SettingRow(stringResource(R.string.settings_cache_in_use), stringResource(R.string.settings_cache_usage_body, formatBytes(complete), formatBytes(partial), formatBytes(budget)), tag = "storage.cacheUsage")
        if (budget > 0) {
            LinearProgressIndicator(progress = { (storage.cacheBytes / budget).toFloat().coerceIn(0f, 1f) }, modifier = Modifier.fillMaxWidth().padding(horizontal = 16.dp).padding(bottom = 8.dp).clearAndSetSemantics { })
        }
        // Budget: "Automatic" while storage.cacheMaxBytes is null (its default); any size, 2 GB
        // included, is the user's own.
        val auto = storage.cacheBudgetAuto ?: (cacheMax.double == null)
        val autoLabel = stringResource(R.string.settings_cache_budget_auto, formatBytes(budget))
        SettingRow(stringResource(R.string.settings_cache_budget), if (auto) autoLabel else stringResource(R.string.settings_cache_budget_custom, formatBytes(budget)), cacheMax.scope, tag = "storage.cacheMaxBytes")
        ChoiceRow(
            listOf<Pair<Double?, String>>(null to stringResource(R.string.settings_cache_budget_auto_short)) + CACHE_BUDGET_CHOICES.map { it to budgetChoiceLabel(it) },
            isSelected = { b -> if (b == null) auto else !auto && kotlin.math.abs(budget - b) < 1e6 },
            onSelect = { b -> if (b == null) cacheMax.setNull() else cacheMax.setDouble(b) },
            modifier = Modifier.padding(horizontal = 16.dp).testTag("storage.cacheMaxBytes.choices"),
        )
        Text(stringResource(R.string.settings_cache_budget_body), style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant, modifier = Modifier.padding(horizontal = 16.dp, vertical = 4.dp))
        SwitchRow(stringResource(R.string.settings_prefetch_mobile), prefetchMobile.bool ?: false, { prefetchMobile.setBool(it) }, stringResource(R.string.settings_prefetch_mobile_body), prefetchMobile.scope, tag = "storage.prefetchOnMobileData")
        SettingRow(stringResource(R.string.settings_data_saved), stringResource(R.string.settings_data_saved_body, formatBytes(storage.servedFromDiskBytes ?: 0.0), formatBytes(storage.fetchedBytes ?: 0.0)), tag = "storage.dataSaved") {
            Text(formatBytes(storage.dataSavedBytes ?: 0.0), style = MaterialTheme.typography.titleMedium, color = MaterialTheme.colorScheme.primary)
        }
        SettingRow(stringResource(R.string.downloads_clear_cache), formatBytes(storage.cacheBytes), onClick = { client.dispatch(Command.ClearStreamCache) }, tag = "storage.clearCache")
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
        ActionRow(listOf(earlier to { offset.setInt((ms - 100).coerceAtLeast(-10_000)) }, reset to { offset.setInt(0) }, later to { offset.setInt((ms + 100).coerceAtMost(10_000)) }), modifier = Modifier.padding(horizontal = 16.dp))
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
        val resources = androidx.compose.ui.platform.LocalResources.current
        ChoiceRow(listOf<Pair<Int?, String>>(null to offLabel) + (1..5).map { it to "$it★" }, isSelected = { n -> if (n == null) !bridgeOn else bridgeOn && threshold == n },
            onSelect = { n -> if (n == null) loveBridge.setBool(false) else { loveThreshold.setInt(n); loveBridge.setBool(true) } },
            describe = { n -> n?.let { resources.getQuantityString(R.plurals.a11y_stars, it, it) } }, modifier = Modifier.padding(horizontal = 16.dp))
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
