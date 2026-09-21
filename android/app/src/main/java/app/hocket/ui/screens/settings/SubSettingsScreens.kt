package app.hocket.ui.screens.settings

import androidx.compose.foundation.gestures.detectDragGestures
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.DragHandle
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.ButtonGroup
import androidx.compose.material3.ButtonGroupDefaults
import androidx.compose.material3.Checkbox
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Slider
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.hapticfeedback.HapticFeedbackType
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.platform.LocalHapticFeedback
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.navigation.NavHostController
import app.hocket.R
import app.hocket.HocketApp
import app.hocket.core.ActionIds
import app.hocket.core.Commands
import app.hocket.core.SettingKeys
import androidx.compose.runtime.rememberCoroutineScope
import kotlinx.coroutines.flow.flowOf
import kotlinx.coroutines.launch
import kotlinx.serialization.builtins.MapSerializer
import app.hocket.core.HocketJson
import app.hocket.core.NativeCore
import app.hocket.core.api.Command
import app.hocket.core.api.EqBand
import app.hocket.core.api.ReplayGainMode
import app.hocket.core.api.TranscodingProfile
import app.hocket.core.fake.FakeCore
import app.hocket.ui.LocalCoreClient
import app.hocket.ui.nav.NavItem
import kotlinx.serialization.builtins.ListSerializer
import kotlinx.serialization.builtins.serializer
import sh.calvin.reorderable.ReorderableItem
import sh.calvin.reorderable.rememberReorderableLazyListState

/** ReplayGain, preamp, normalisation, gapless, EQ with draggable bands, output device. */
@Composable
fun AudioSettingsScreen(nav: NavHostController) {
    val client = LocalCoreClient.current
    val audio by client.audio.collectAsStateWithLifecycle()
    val outputs by client.outputDevices.collectAsStateWithLifecycle()
    LaunchedEffect(Unit) { client.dispatch(Command.RefreshOutputDevices) }
    SubScreen(nav, stringResource(R.string.settings_section_audio)) {
        val rgOptions = listOf(ReplayGainMode.Off to stringResource(R.string.settings_rg_off), ReplayGainMode.Track to stringResource(R.string.settings_rg_track), ReplayGainMode.Album to stringResource(R.string.settings_rg_album), ReplayGainMode.Auto to stringResource(R.string.settings_rg_auto))
        SettingRow(stringResource(R.string.settings_replay_gain)) {
            ButtonGroup(overflowIndicator = {}, horizontalArrangement = Arrangement.spacedBy(ButtonGroupDefaults.ConnectedSpaceBetween)) {
                rgOptions.forEach { (m, label) -> toggleableItem(checked = audio.replayGain == m, label = label, onCheckedChange = { client.dispatch(Commands.setAudioSettings(audio.copy(replayGain = m))) }) }
            }
        }
        SettingRow(stringResource(R.string.settings_preamp), stringResource(R.string.settings_db, audio.replayGainPreampDb))
        Slider(value = audio.replayGainPreampDb.toFloat(), onValueChange = { client.dispatch(Commands.setAudioSettings(audio.copy(replayGainPreampDb = it.toDouble()))) }, valueRange = -15f..15f, modifier = Modifier.padding(horizontal = 16.dp))
        SwitchRow(stringResource(R.string.settings_normalisation), audio.normalisation, { client.dispatch(Commands.setAudioSettings(audio.copy(normalisation = it))) })
        SwitchRow(stringResource(R.string.settings_gapless), audio.gapless, { client.dispatch(Commands.setAudioSettings(audio.copy(gapless = it))) })
        SettingsSection(stringResource(R.string.settings_eq))
        SwitchRow(stringResource(R.string.settings_eq_enabled), audio.eq.enabled, { client.dispatch(Commands.setAudioSettings(audio.copy(eq = audio.eq.copy(enabled = it)))) })
        SettingRow(stringResource(R.string.settings_eq_preamp), stringResource(R.string.settings_db, audio.eq.preampDb)) {
            TextButton(onClick = { client.dispatch(Commands.setAudioSettings(audio.copy(eq = audio.eq.copy(bands = audio.eq.bands.map { it.copy(gainDb = 0.0) }, preampDb = 0.0, preset = null)))) }) { Text(stringResource(R.string.settings_eq_reset)) }
        }
        Slider(value = audio.eq.preampDb.toFloat(), onValueChange = { client.dispatch(Commands.setAudioSettings(audio.copy(eq = audio.eq.copy(preampDb = it.toDouble())))) }, valueRange = -12f..12f, modifier = Modifier.padding(horizontal = 16.dp))
        val bands = audio.eq.bands.ifEmpty { FakeCore.defaultBands() }
        EqEditor(bands, enabled = audio.eq.enabled) { new -> client.dispatch(Commands.setAudioSettings(audio.copy(eq = audio.eq.copy(bands = new, preset = null)))) }
        if (outputs.isNotEmpty()) {
            SettingsSection(stringResource(R.string.settings_output_device))
            ButtonGroup(overflowIndicator = {}, modifier = Modifier.padding(horizontal = 16.dp), horizontalArrangement = Arrangement.spacedBy(ButtonGroupDefaults.ConnectedSpaceBetween)) {
                outputs.forEach { d -> toggleableItem(checked = (audio.outputDevice ?: outputs.firstOrNull { it.isDefault }?.id) == d.id, label = d.name, onCheckedChange = { client.dispatch(Commands.setOutputDevice(d.id)) }) }
            }
        }
    }
}

/** Vertical draggable bands, ±12 dB, with haptic ticks on whole-dB steps. */
@Composable
fun EqEditor(bands: List<EqBand>, enabled: Boolean, onChange: (List<EqBand>) -> Unit) {
    val haptics = LocalHapticFeedback.current
    var working by remember(bands) { mutableStateOf(bands) }
    Row(Modifier.fillMaxWidth().height(220.dp).padding(horizontal = 16.dp), horizontalArrangement = Arrangement.SpaceBetween) {
        working.forEachIndexed { i, band ->
            val label = if (band.frequencyHz >= 1000) "${(band.frequencyHz / 1000).toInt()}k" else band.frequencyHz.toInt().toString()
            val desc = stringResource(R.string.settings_eq_band_a11y, label, band.gainDb)
            Column(Modifier.weight(1f).fillMaxHeight().semantics { contentDescription = desc }, horizontalAlignment = Alignment.CenterHorizontally) {
                Text(stringResource(R.string.settings_db, band.gainDb).removePrefix("+0.0 dB").ifEmpty { "0" }, style = MaterialTheme.typography.labelSmall)
                Box(Modifier.weight(1f).width(28.dp).pointerInput(enabled, i) {
                    if (!enabled) return@pointerInput
                    var lastWhole = band.gainDb.toInt()
                    detectDragGestures(onDragStart = { haptics.performHapticFeedback(HapticFeedbackType.GestureThresholdActivate) }, onDragEnd = { onChange(working) }) { change, drag ->
                        change.consume()
                        val perPx = 24.0 / size.height
                        val g = (working[i].gainDb - drag.y * perPx).coerceIn(-12.0, 12.0)
                        working = working.toMutableList().also { it[i] = it[i].copy(gainDb = g) }
                        if (g.toInt() != lastWhole) { lastWhole = g.toInt(); haptics.performHapticFeedback(HapticFeedbackType.SegmentTick) }
                    }
                }, contentAlignment = Alignment.Center) {
                    Surface(shape = MaterialTheme.shapes.extraLarge, color = MaterialTheme.colorScheme.surfaceContainerHighest, modifier = Modifier.width(8.dp).fillMaxHeight()) {}
                    val fraction = ((band.gainDb + 12.0) / 24.0).toFloat()
                    Column(Modifier.fillMaxHeight(), verticalArrangement = Arrangement.Bottom) {
                        Surface(shape = MaterialTheme.shapes.extraLarge, color = if (enabled) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.outline, modifier = Modifier.width(8.dp).fillMaxHeight(fraction.coerceIn(0.02f, 1f))) {}
                    }
                }
                Text(label, style = MaterialTheme.typography.labelSmall)
            }
        }
    }
}

/** Per-network transcoding profiles: format, max bitrate, always-transcode suffixes. */
@Composable
fun TranscodingSettingsScreen(nav: NavHostController) {
    val client = LocalCoreClient.current
    val network by client.network.collectAsStateWithLifecycle()
    SubScreen(nav, stringResource(R.string.settings_section_transcoding)) {
        Text(stringResource(R.string.settings_transcoding_body), style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant, modifier = Modifier.padding(16.dp))
        ProfileEditor(stringResource(R.string.settings_transcoding_wifi), "default")
        ProfileEditor(stringResource(R.string.settings_transcoding_cellular), "cellular")
        network?.networkId?.takeIf { it != "default" && it != "cellular" }?.let { id -> ProfileEditor(stringResource(R.string.settings_transcoding_current), id) }
    }
}

/** One entry of the `transcoding.profiles` map (keys: `default`, `cellular`, `<networkId>`). */
@Composable
private fun ProfileEditor(title: String, networkId: String) {
    val client = LocalCoreClient.current
    val handle = setting(SettingKeys.TRANSCODING_PROFILES)
    val profile = remember(handle.raw, networkId) {
        handle.raw?.let { runCatching { HocketJson.json.decodeFromString(MapSerializer(String.serializer(), TranscodingProfile.serializer()), it) }.getOrNull() }?.get(networkId)
            ?: TranscodingProfile(null, null, emptyList())
    }
    fun push(p: TranscodingProfile) = client.dispatch(Commands.setTranscodingProfile(networkId.takeIf { it != "default" }, p))
    SettingsSection(title)
    val originalLabel = stringResource(R.string.settings_format_original)
    SettingRow(stringResource(R.string.settings_format)) {
        ButtonGroup(overflowIndicator = {}, horizontalArrangement = Arrangement.spacedBy(ButtonGroupDefaults.ConnectedSpaceBetween)) {
            listOf(null, "opus", "mp3", "aac").forEach { f -> toggleableItem(checked = profile.format == f, label = f ?: originalLabel, onCheckedChange = { push(profile.copy(format = f)) }) }
        }
    }
    val bitrates = listOf(null, 96, 128, 192, 320).map { b -> b to (b?.let { stringResource(R.string.settings_bitrate_kbps, it) } ?: stringResource(R.string.settings_bitrate_unlimited)) }
    SettingRow(stringResource(R.string.settings_max_bitrate)) {
        ButtonGroup(overflowIndicator = {}, horizontalArrangement = Arrangement.spacedBy(ButtonGroupDefaults.ConnectedSpaceBetween)) {
            bitrates.forEach { (b, label) -> toggleableItem(checked = profile.maxBitRate?.toInt() == b, label = label, onCheckedChange = { push(profile.copy(maxBitRate = b?.toUInt())) }) }
        }
    }
    var cannot by remember(profile) { mutableStateOf(profile.cannotDecode.joinToString(", ")) }
    OutlinedTextField(value = cannot, onValueChange = { cannot = it }, label = { Text(stringResource(R.string.settings_cannot_decode)) }, singleLine = true, modifier = Modifier.fillMaxWidth().padding(horizontal = 16.dp),
        trailingIcon = { TextButton(onClick = { push(profile.copy(cannotDecode = cannot.split(",").map { it.trim().lowercase() }.filter { it.isNotEmpty() })) }) { Text(stringResource(R.string.action_save)) } })
}

/** Coordinator URL, connect/disconnect, LAN discovery, device identity. */
@Composable
fun ConnectSettingsScreen(nav: NavHostController) {
    val client = LocalCoreClient.current
    val connection by client.connection.collectAsStateWithLifecycle()
    val devices by client.devices.collectAsStateWithLifecycle()
    val lan = setting(SettingKeys.CONNECT_LAN_DISCOVERY)
    var url by remember(connection.coordinatorUrl) { mutableStateOf(connection.coordinatorUrl ?: "") }
    SubScreen(nav, stringResource(R.string.settings_section_connect)) {
        OutlinedTextField(value = url, onValueChange = { url = it }, label = { Text(stringResource(R.string.settings_coordinator_url)) }, placeholder = { Text(stringResource(R.string.settings_coordinator_hint)) }, singleLine = true, modifier = Modifier.fillMaxWidth().padding(16.dp))
        Row(Modifier.padding(horizontal = 16.dp), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            Button(shapes = ButtonDefaults.shapes(), onClick = { client.dispatch(Commands.setCoordinatorUrl(url.trim().ifEmpty { null })); client.dispatch(Command.ConnectCoordinator) }) { Text(stringResource(R.string.settings_coordinator_connect)) }
            TextButton(onClick = { client.dispatch(Command.DisconnectCoordinator) }, enabled = connection.connected) { Text(stringResource(R.string.settings_coordinator_disconnect)) }
        }
        val status = when (connection.tier) {
            app.hocket.core.api.ConnectionTier.Local -> stringResource(R.string.connection_local)
            app.hocket.core.api.ConnectionTier.Lan -> stringResource(R.string.connection_lan, connection.peerCount.toInt())
            app.hocket.core.api.ConnectionTier.Coordinator -> stringResource(R.string.connection_coordinator, connection.peerCount.toInt())
        }
        SettingRow(status, connection.error ?: connection.roundTripMs?.let { "${it.toInt()} ms" })
        SwitchRow(stringResource(R.string.settings_lan_discovery), lan.bool ?: true, { client.dispatch(Commands.setLanDiscovery(it)) }, scope = lan.scope)
        devices.firstOrNull { it.isSelf }?.let { SettingRow(stringResource(R.string.settings_device_name, it.name), it.id) }
    }
}

/** Choose-and-order lists for context menu actions, media buttons and navigation items. */
@Composable
fun CustomiseSettingsScreen(nav: NavHostController) {
    val client = LocalCoreClient.current
    val app = androidx.compose.ui.platform.LocalContext.current.applicationContext as? HocketApp
    val scope = rememberCoroutineScope()
    val menuSetting = setting(SettingKeys.ACTIONS_ORDER_CONTEXT_MENU)
    val mediaSetting = setting(SettingKeys.ACTIONS_ORDER_MEDIA_SESSION)
    SubScreen(nav, stringResource(R.string.settings_section_customise)) {
        Text(stringResource(R.string.settings_reorder_hint), style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant, modifier = Modifier.padding(16.dp))
        SettingsSection(stringResource(R.string.settings_sidebar))
        // Navigation items are an app-local preference; the core's `sidebar` surface is kept in step.
        val allNav = NavItem.entries.map { it.id }
        val navIds by (app?.prefs?.navItems ?: flowOf(emptyList())).collectAsStateWithLifecycle(initialValue = emptyList())
        ChooseAndOrder(allNav, navIds.ifEmpty { NavItem.DEFAULT.map { it.id } }, minEnabled = 2) { ids ->
            scope.launch { app?.prefs?.setNavItems(ids) }
            client.dispatch(Commands.setActionOrder("sidebar", ids.map { NavItem.fromIds(listOf(it)).first().canonicalActionId }))
        }
        SettingsSection(stringResource(R.string.settings_context_menu))
        // Canonical registry ids for the contextMenu surface; an empty stored list means the registry default.
        val allMenu = ActionIds.CONTEXT_MENU
        val menuIds = menuSetting.raw?.let { runCatching { HocketJson.json.decodeFromString(ListSerializer(String.serializer()), it) }.getOrNull() }?.ifEmpty { null } ?: allMenu
        ChooseAndOrder(allMenu, menuIds, minEnabled = 1) { ids -> client.dispatch(Commands.setActionOrder("contextMenu", ids)) }
        SettingsSection(stringResource(R.string.settings_media_buttons))
        val allMedia = ActionIds.MEDIA_SESSION
        val mediaIds = mediaSetting.raw?.let { runCatching { HocketJson.json.decodeFromString(ListSerializer(String.serializer()), it) }.getOrNull() }?.ifEmpty { null } ?: allMedia
        ChooseAndOrder(allMedia, mediaIds, minEnabled = 0) { ids -> client.dispatch(Commands.setActionOrder("mediaSession", ids)) }
    }
}

/** Enabled items first (draggable, in order), then disabled ones; toggling moves between the sets. */
@Composable
private fun ChooseAndOrder(all: List<String>, enabled: List<String>, minEnabled: Int, onChange: (List<String>) -> Unit) {
    val haptics = LocalHapticFeedback.current
    var order by remember(enabled) { mutableStateOf(enabled.filter { it in all } + all.filter { it !in enabled }) }
    val enabledSet = remember(enabled) { enabled.toSet() }
    val listState = rememberLazyListState()
    val reorderable = rememberReorderableLazyListState(listState) { from, to ->
        order = order.toMutableList().apply { add(to.index, removeAt(from.index)) }
        haptics.performHapticFeedback(HapticFeedbackType.SegmentFrequentTick)
    }
    LazyColumn(state = listState, modifier = Modifier.fillMaxWidth().height((all.size * 52).dp), userScrollEnabled = false) {
        itemsIndexed(order, key = { _, id -> id }) { _, id ->
            ReorderableItem(reorderable, key = id) { _ ->
                Row(Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 4.dp), verticalAlignment = Alignment.CenterVertically) {
                    Checkbox(checked = id in enabledSet, onCheckedChange = { on ->
                        val next = if (on) order.filter { it in enabledSet || it == id } else order.filter { it in enabledSet && it != id }
                        if (next.size >= minEnabled) onChange(next)
                    })
                    Text(id, modifier = Modifier.weight(1f))
                    Icon(Icons.Filled.DragHandle, stringResource(R.string.playlist_reorder_handle), modifier = Modifier.draggableHandle(onDragStopped = { onChange(order.filter { it in enabledSet }) }))
                }
            }
        }
    }
}

@Composable
fun AboutScreen(nav: NavHostController) {
    val context = androidx.compose.ui.platform.LocalContext.current
    val version = remember { runCatching { context.packageManager.getPackageInfo(context.packageName, 0).versionName }.getOrNull() ?: "?" }
    val coreVersion = remember { NativeCore.version() ?: "fake" }
    SubScreen(nav, stringResource(R.string.settings_section_about)) {
        Column(Modifier.padding(16.dp)) {
            Text(stringResource(R.string.settings_version, version, coreVersion), style = MaterialTheme.typography.titleMedium)
            Spacer(Modifier.height(8.dp))
            Text(stringResource(R.string.settings_licence), style = MaterialTheme.typography.bodyLarge)
            Spacer(Modifier.height(16.dp))
            Text(stringResource(R.string.settings_licences_title), style = MaterialTheme.typography.titleSmall)
            Text(stringResource(R.string.settings_licences_body), style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
        }
    }
}
