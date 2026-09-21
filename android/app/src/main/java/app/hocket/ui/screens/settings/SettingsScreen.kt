package app.hocket.ui.screens.settings

import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import android.content.Intent
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.ChevronRight
import androidx.compose.material3.ButtonGroup
import androidx.compose.material3.ButtonGroupDefaults
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.LargeFlexibleTopAppBar
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Slider
import androidx.compose.material3.Text
import androidx.compose.material3.TopAppBarDefaults
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.input.nestedscroll.nestedScroll
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.navigation.NavHostController
import app.hocket.R
import app.hocket.ui.nav.BottomContentInset
import app.hocket.core.Commands
import app.hocket.core.Queries
import app.hocket.core.SettingKeys
import app.hocket.core.api.Command
import app.hocket.core.api.Event
import app.hocket.core.api.QueryResult
import app.hocket.core.api.SettingScope
import app.hocket.ui.LocalCoreClient
import app.hocket.ui.components.ConfirmDialog
import app.hocket.ui.components.formatAgo
import app.hocket.ui.components.formatBytes
import app.hocket.ui.nav.Route
import kotlinx.coroutines.launch

/** Every scoped setting with its synced/local badge, grouped; subpages for the denser ones. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun SettingsScreen(nav: NavHostController) {
    val client = LocalCoreClient.current
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    val server by client.server.collectAsStateWithLifecycle()
    val storage by client.storage.collectAsStateWithLifecycle()
    val batterySaver by client.batterySaver.collectAsStateWithLifecycle()
    val queue by client.queue.collectAsStateWithLifecycle()
    val sync = setting(SettingKeys.SYNC_ENABLED)
    val theme = setting(SettingKeys.DISPLAY_THEME)
    val accent = setting(SettingKeys.DISPLAY_ACCENT)
    val dynamicColour = setting(SettingKeys.DISPLAY_DYNAMIC_COLOUR)
    val animated = setting(SettingKeys.DISPLAY_ANIMATED_BACKGROUND)
    val autoSaver = setting(SettingKeys.BATTERY_AUTO_ENGAGE)
    val externalLyrics = setting(SettingKeys.LYRICS_EXTERNAL_ENABLED)
    val loveBridge = setting(SettingKeys.RATINGS_LOVE_BRIDGE_ENABLED)
    val loveThreshold = setting(SettingKeys.RATINGS_LOVE_BRIDGE_THRESHOLD)
    val savedCap = setting(SettingKeys.QUEUE_SAVED_CAP)
    var signOut by remember { mutableStateOf(false) }
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
    LaunchedEffect(Unit) { client.exports.collect { e -> if (e is Event.ConfigExported) { /* handled through the document launcher */ } } }
    val scroll = TopAppBarDefaults.exitUntilCollapsedScrollBehavior()
    Scaffold(modifier = Modifier.nestedScroll(scroll.nestedScrollConnection), topBar = { LargeFlexibleTopAppBar(title = { Text(stringResource(R.string.settings_title)) }, scrollBehavior = scroll) }) { padding ->
        Column(Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(top = padding.calculateTopPadding(), bottom = BottomContentInset)) {
            SwitchRow(stringResource(R.string.settings_sync_master), sync.bool ?: true, { client.dispatch(Commands.setSettingsSync(it)) }, stringResource(R.string.settings_sync_master_body))

            SettingsSection(stringResource(R.string.settings_server))
            server?.let { s ->
                SettingRow(stringResource(R.string.settings_server_body, s.url, s.username), s.lastSync?.let { stringResource(R.string.settings_last_sync, formatAgo(it)) } ?: stringResource(R.string.settings_never_synced))
                SettingRow(stringResource(R.string.settings_sync_now), onClick = { client.dispatch(Commands.syncLibrary(s.id, full = false)) })
                SettingRow(stringResource(R.string.settings_full_sync), onClick = { client.dispatch(Commands.syncLibrary(s.id, full = true)) })
                SettingRow(stringResource(R.string.settings_remove_server), onClick = { signOut = true })
            }

            SettingsSection(stringResource(R.string.settings_section_appearance))
            val themeOptions = listOf("system" to stringResource(R.string.settings_theme_system), "light" to stringResource(R.string.settings_theme_light), "dark" to stringResource(R.string.settings_theme_dark))
            SettingRow(stringResource(R.string.settings_theme), scope = theme.scope) {
                ButtonGroup(overflowIndicator = {}, horizontalArrangement = Arrangement.spacedBy(ButtonGroupDefaults.ConnectedSpaceBetween)) {
                    themeOptions.forEach { (v, label) -> toggleableItem(checked = (theme.string ?: "system") == v, label = label, onCheckedChange = { theme.setString(v) }) }
                }
            }
            // display.accent: null = follow the wallpaper (dynamic) / the artwork; a #RRGGBB overrides it.
            val accentValue = accent.string
            val dynamicLabel = stringResource(R.string.settings_accent_dynamic)
            SettingRow(stringResource(R.string.settings_accent), scope = accent.scope) {
                ButtonGroup(overflowIndicator = {}, horizontalArrangement = Arrangement.spacedBy(ButtonGroupDefaults.ConnectedSpaceBetween)) {
                    toggleableItem(checked = accentValue == null, label = dynamicLabel, onCheckedChange = { accent.setRaw("null") })
                    listOf("#6750A4", "#1B6B5E", "#B3261E").forEach { hex -> toggleableItem(checked = accentValue.equals(hex, true), label = "●", onCheckedChange = { accent.setString(hex) }) }
                }
            }
            SwitchRow(stringResource(R.string.settings_accent_artwork), dynamicColour.bool ?: true, { dynamicColour.setBool(it) }, scope = dynamicColour.scope)
            SwitchRow(stringResource(R.string.settings_animated_background), animated.bool ?: true, { animated.setBool(it) }, stringResource(R.string.settings_animated_background_body), animated.scope)

            SettingsSection(stringResource(R.string.settings_section_audio))
            SettingRow(stringResource(R.string.settings_section_audio), stringResource(R.string.settings_replay_gain) + stringResource(R.string.dot_separator) + stringResource(R.string.settings_eq) + stringResource(R.string.dot_separator) + stringResource(R.string.settings_gapless), onClick = { nav.navigate(Route.AudioSettings) }) { Icon(Icons.Filled.ChevronRight, null) }
            SettingRow(stringResource(R.string.settings_section_transcoding), stringResource(R.string.settings_transcoding_body), onClick = { nav.navigate(Route.TranscodingSettings) }) { Icon(Icons.Filled.ChevronRight, null) }

            SettingsSection(stringResource(R.string.settings_section_connect))
            SettingRow(stringResource(R.string.settings_section_connect), stringResource(R.string.settings_coordinator_url) + stringResource(R.string.dot_separator) + stringResource(R.string.settings_lan_discovery), onClick = { nav.navigate(Route.ConnectSettings) }) { Icon(Icons.Filled.ChevronRight, null) }

            SettingsSection(stringResource(R.string.settings_section_queue))
            val cap = savedCap.int ?: 10
            SettingRow(stringResource(R.string.settings_saved_cap), if (cap == 0) stringResource(R.string.settings_saved_cap_zero) else stringResource(R.string.saved_cap, cap), savedCap.scope)
            Slider(value = cap.toFloat(), onValueChange = { }, onValueChangeFinished = null, valueRange = 0f..50f, steps = 49, modifier = Modifier.padding(horizontal = 16.dp), enabled = false)
            ButtonGroup(overflowIndicator = {}, modifier = Modifier.padding(horizontal = 16.dp), horizontalArrangement = Arrangement.spacedBy(ButtonGroupDefaults.ConnectedSpaceBetween)) {
                listOf(0, 5, 10, 25, 50).forEach { n -> toggleableItem(checked = cap == n, label = n.toString(), onCheckedChange = { client.dispatch(Commands.setSavedQueueCap(n)) }) }
            }
            SwitchRow(stringResource(R.string.settings_autoplay), queue.autoplay, { client.dispatch(Commands.setAutoplay(it)) })

            SettingsSection(stringResource(R.string.settings_section_storage))
            SettingRow(stringResource(R.string.nav_downloads), formatBytes(storage.downloadsBytes) + stringResource(R.string.dot_separator) + stringResource(R.string.settings_cache_images, formatBytes(storage.imagesBytes)), onClick = { nav.navigate(Route.Downloads) }) { Icon(Icons.Filled.ChevronRight, null) }
            SettingRow(stringResource(R.string.downloads_clear_cache), formatBytes(storage.cacheBytes), onClick = { client.dispatch(Command.ClearStreamCache) })
            val warn = storage.warnThresholdBytes
            SettingRow(stringResource(R.string.downloads_threshold), warn?.let { formatBytes(it) } ?: stringResource(R.string.downloads_threshold_none))
            val noLimit = stringResource(R.string.downloads_threshold_none)
            ButtonGroup(overflowIndicator = {}, modifier = Modifier.padding(horizontal = 16.dp), horizontalArrangement = Arrangement.spacedBy(ButtonGroupDefaults.ConnectedSpaceBetween)) {
                listOf(null, 2.0, 4.0, 8.0, 16.0).forEach { gb -> toggleableItem(checked = (gb?.let { it * 1e9 }) == warn, label = gb?.let { "${it.toInt()} GB" } ?: noLimit, onCheckedChange = { client.dispatch(Commands.setStorageWarnThreshold(gb?.let { it * 1e9 })) }) }
            }

            SettingsSection(stringResource(R.string.settings_section_battery))
            SwitchRow(stringResource(R.string.settings_battery_saver), batterySaver, { client.dispatch(Commands.setBatterySaver(it)) }, stringResource(R.string.settings_battery_body))
            SwitchRow(stringResource(R.string.settings_battery_auto), autoSaver.bool ?: true, { autoSaver.setBool(it) }, scope = autoSaver.scope)

            SettingsSection(stringResource(R.string.settings_section_lyrics))
            SwitchRow(stringResource(R.string.settings_external_lyrics), externalLyrics.bool ?: false, { client.dispatch(Commands.setExternalLyricsEnabled(it)) }, stringResource(R.string.settings_external_lyrics_privacy), externalLyrics.scope)

            SettingsSection(stringResource(R.string.settings_section_ratings))
            // ratings.loveBridge.enabled + threshold (1..5): "off" is the bridge disabled.
            val bridgeOn = loveBridge.bool ?: false
            val threshold = loveThreshold.int ?: 4
            SettingRow(stringResource(R.string.settings_love_threshold), if (!bridgeOn) stringResource(R.string.settings_love_threshold_off) else stringResource(R.string.rating_set, threshold), loveThreshold.scope)
            val offLabel = stringResource(R.string.settings_love_threshold_off)
            ButtonGroup(overflowIndicator = {}, modifier = Modifier.padding(horizontal = 16.dp), horizontalArrangement = Arrangement.spacedBy(ButtonGroupDefaults.ConnectedSpaceBetween)) {
                toggleableItem(checked = !bridgeOn, label = offLabel, onCheckedChange = { loveBridge.setBool(false) })
                (1..5).forEach { n -> toggleableItem(checked = bridgeOn && threshold == n, label = "$n★", onCheckedChange = { loveThreshold.setInt(n); loveBridge.setBool(true) }) }
            }

            SettingsSection(stringResource(R.string.settings_section_customise))
            SettingRow(stringResource(R.string.settings_section_customise), stringResource(R.string.settings_context_menu) + stringResource(R.string.dot_separator) + stringResource(R.string.settings_media_buttons) + stringResource(R.string.dot_separator) + stringResource(R.string.settings_sidebar), onClick = { nav.navigate(Route.CustomiseSettings) }) { Icon(Icons.Filled.ChevronRight, null) }
            SettingRow(stringResource(R.string.nav_filters), onClick = { nav.navigate(Route.Filters) }) { Icon(Icons.Filled.ChevronRight, null) }
            SettingRow(stringResource(R.string.nav_stats), onClick = { nav.navigate(Route.Stats) }) { Icon(Icons.Filled.ChevronRight, null) }

            SettingsSection(stringResource(R.string.settings_section_backup))
            SwitchRow(stringResource(R.string.settings_export_with_secrets), includeSecrets, { includeSecrets = it })
            SettingRow(stringResource(R.string.settings_export_config), onClick = { exportLauncher.launch("hocket-config.json") })
            SettingRow(stringResource(R.string.settings_import_config), onClick = { importLauncher.launch(arrayOf("application/json", "text/plain")) })
            SettingRow(stringResource(R.string.settings_copy_diagnostics), onClick = {
                scope.launch {
                    val text = (client.query(app.hocket.core.api.Query.Diagnostics) as? QueryResult.Text)?.data ?: ""
                    (context.getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager).setPrimaryClip(ClipData.newPlainText("Hocket diagnostics", text))
                }
            })

            SettingsSection(stringResource(R.string.settings_section_about))
            SettingRow(stringResource(R.string.settings_section_about), stringResource(R.string.settings_licence), onClick = { nav.navigate(Route.About) }) { Icon(Icons.Filled.ChevronRight, null) }
        }
    }
    if (signOut) server?.let { s -> ConfirmDialog(stringResource(R.string.settings_remove_server_confirm, s.name), stringResource(R.string.settings_remove_server), onConfirm = { client.dispatch(Commands.removeServer(s.id)) }, onDismiss = { signOut = false }) }
    importConfirm?.let { doc -> ConfirmDialog(stringResource(R.string.settings_import_confirm), stringResource(R.string.settings_import_config), onConfirm = { client.dispatch(Commands.importConfig(doc)) }, onDismiss = { importConfirm = null }, destructive = false) }
}

@Composable
internal fun str(id: Int): String = stringResource(id)
