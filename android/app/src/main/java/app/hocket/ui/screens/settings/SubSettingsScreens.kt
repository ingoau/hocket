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
import app.hocket.ui.components.ChoiceRow
import app.hocket.ui.components.LabelledSlider
import androidx.compose.foundation.layout.heightIn
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.semantics.ProgressBarRangeInfo
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.CustomAccessibilityAction
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.customActions
import androidx.compose.foundation.selection.toggleable
import androidx.compose.ui.semantics.disabled
import androidx.compose.ui.semantics.progressBarRangeInfo
import androidx.compose.ui.semantics.setProgress
import app.hocket.ui.nav.NavItem
import kotlinx.serialization.builtins.ListSerializer
import kotlinx.serialization.builtins.serializer
import sh.calvin.reorderable.ReorderableColumn
import androidx.compose.runtime.key
import androidx.compose.ui.platform.testTag
import app.hocket.ui.nav.label

/** ReplayGain, preamp, normalisation, gapless, EQ with draggable bands, output device. */
@Composable
fun AudioSettingsScreen(nav: NavHostController) {
    val client = LocalCoreClient.current
    val audio by client.audio.collectAsStateWithLifecycle()
    val outputs by client.outputDevices.collectAsStateWithLifecycle()
    LaunchedEffect(Unit) { client.dispatch(Command.RefreshOutputDevices) }
    SubScreen(nav, stringResource(R.string.settings_section_audio)) {
        val rgOptions = listOf(ReplayGainMode.Off to stringResource(R.string.settings_rg_off), ReplayGainMode.Track to stringResource(R.string.settings_rg_track), ReplayGainMode.Album to stringResource(R.string.settings_rg_album), ReplayGainMode.Auto to stringResource(R.string.settings_rg_auto))
        SettingRow(stringResource(R.string.settings_replay_gain))
        ChoiceRow(rgOptions, isSelected = { audio.replayGain == it }, onSelect = { m -> client.dispatch(Commands.setAudioSettings(audio.copy(replayGain = m))) }, modifier = Modifier.padding(horizontal = 16.dp))
        val preampLabel = stringResource(R.string.settings_preamp)
        val preampValue = stringResource(R.string.settings_db, audio.replayGainPreampDb)
        SettingRow(preampLabel, preampValue)
        // Spoken as "Preamp, +3.0 dB" rather than a bare percentage.
        LabelledSlider(value = audio.replayGainPreampDb.toFloat(), onValueChange = { client.dispatch(Commands.setAudioSettings(audio.copy(replayGainPreampDb = it.toDouble()))) }, valueRange = -15f..15f,
            label = preampLabel, valueText = preampValue, modifier = Modifier.padding(horizontal = 16.dp))
        SwitchRow(stringResource(R.string.settings_normalisation), audio.normalisation, { client.dispatch(Commands.setAudioSettings(audio.copy(normalisation = it))) })
        SwitchRow(stringResource(R.string.settings_gapless), audio.gapless, { client.dispatch(Commands.setAudioSettings(audio.copy(gapless = it))) })
        SettingsSection(stringResource(R.string.settings_eq))
        SwitchRow(stringResource(R.string.settings_eq_enabled), audio.eq.enabled, { client.dispatch(Commands.setAudioSettings(audio.copy(eq = audio.eq.copy(enabled = it)))) })
        val eqPreampLabel = stringResource(R.string.settings_eq_preamp)
        val eqPreampValue = stringResource(R.string.settings_db, audio.eq.preampDb)
        SettingRow(eqPreampLabel, eqPreampValue) {
            TextButton(onClick = { client.dispatch(Commands.setAudioSettings(audio.copy(eq = audio.eq.copy(bands = audio.eq.bands.map { it.copy(gainDb = 0.0) }, preampDb = 0.0, preset = null)))) }) { Text(stringResource(R.string.settings_eq_reset)) }
        }
        LabelledSlider(value = audio.eq.preampDb.toFloat(), onValueChange = { client.dispatch(Commands.setAudioSettings(audio.copy(eq = audio.eq.copy(preampDb = it.toDouble())))) }, valueRange = -12f..12f,
            label = eqPreampLabel, valueText = eqPreampValue, modifier = Modifier.padding(horizontal = 16.dp))
        val bands = audio.eq.bands.ifEmpty { FakeCore.defaultBands() }
        EqEditor(bands, enabled = audio.eq.enabled) { new -> client.dispatch(Commands.setAudioSettings(audio.copy(eq = audio.eq.copy(bands = new, preset = null)))) }
        if (outputs.isNotEmpty()) {
            SettingsSection(stringResource(R.string.settings_output_device))
            val selectedOutput = audio.outputDevice ?: outputs.firstOrNull { it.isDefault }?.id
            ChoiceRow(outputs.map { it.id to it.name }, isSelected = { selectedOutput == it }, onSelect = { client.dispatch(Commands.setOutputDevice(it)) }, modifier = Modifier.padding(horizontal = 16.dp))
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
            val bandLabel = stringResource(R.string.settings_eq_band_label, label)
            val value = stringResource(R.string.settings_db, band.gainDb)
            // The drag is pointer-only; for a screen reader each band is an adjustable control
            // (swipe up/down steps it by 1 dB).
            Column(Modifier.weight(1f).fillMaxHeight().clearAndSetSemantics {
                contentDescription = bandLabel
                stateDescription = value
                progressBarRangeInfo = ProgressBarRangeInfo(band.gainDb.toFloat(), -12f..12f, steps = 23)
                if (enabled) setProgress(bandLabel) { g ->
                    val next = working.toMutableList().also { it[i] = it[i].copy(gainDb = g.toDouble().coerceIn(-12.0, 12.0)) }
                    working = next
                    onChange(next)
                    true
                } else disabled()
            }, horizontalAlignment = Alignment.CenterHorizontally) {
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
    SubScreen(nav, stringResource(R.string.settings_category_streaming)) {
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
    SettingRow(stringResource(R.string.settings_format))
    ChoiceRow(listOf(null, "opus", "mp3", "aac").map { it to (it ?: originalLabel) }, isSelected = { profile.format == it }, onSelect = { push(profile.copy(format = it)) }, modifier = Modifier.padding(horizontal = 16.dp))
    val bitrates = listOf(null, 96, 128, 192, 320).map { b -> b to (b?.let { stringResource(R.string.settings_bitrate_kbps, it) } ?: stringResource(R.string.settings_bitrate_unlimited)) }
    SettingRow(stringResource(R.string.settings_max_bitrate))
    ChoiceRow(bitrates, isSelected = { profile.maxBitRate?.toInt() == it }, onSelect = { push(profile.copy(maxBitRate = it?.toUInt())) }, modifier = Modifier.padding(horizontal = 16.dp))
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
    SubScreen(nav, stringResource(R.string.settings_category_connect)) {
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
        ChooseAndOrder(allNav, navIds.ifEmpty { NavItem.DEFAULT.map { it.id } }, minEnabled = 2, label = { id -> NavItem.entries.first { it.id == id }.label() }, tag = "customise.nav") { ids ->
            scope.launch { app?.prefs?.setNavItems(ids) }
            client.dispatch(Commands.setActionOrder("sidebar", ids.map { NavItem.fromIds(listOf(it)).first().canonicalActionId }))
        }
        SettingsSection(stringResource(R.string.settings_context_menu))
        // Canonical registry ids for the contextMenu surface; an empty stored list means the registry default.
        val allMenu = ActionIds.CONTEXT_MENU
        val menuIds = menuSetting.raw?.let { runCatching { HocketJson.json.decodeFromString(ListSerializer(String.serializer()), it) }.getOrNull() }?.ifEmpty { null } ?: allMenu
        ChooseAndOrder(allMenu, menuIds, minEnabled = 1, label = { actionLabel(it) }, tag = "customise.contextMenu") { ids -> client.dispatch(Commands.setActionOrder("contextMenu", ids)) }
        SettingsSection(stringResource(R.string.settings_media_buttons))
        val allMedia = ActionIds.MEDIA_SESSION
        val mediaIds = mediaSetting.raw?.let { runCatching { HocketJson.json.decodeFromString(ListSerializer(String.serializer()), it) }.getOrNull() }?.ifEmpty { null } ?: allMedia
        ChooseAndOrder(allMedia, mediaIds, minEnabled = 0, label = { actionLabel(it) }, tag = "customise.mediaSession") { ids -> client.dispatch(Commands.setActionOrder("mediaSession", ids)) }
    }
}

/**
 * Enabled items first (draggable, in order), then disabled ones; toggling moves between the sets.
 * A plain column (not a fixed-height lazy list), so the section is exactly as tall as its rows.
 * [label] is each id's human label.
 */
@Composable
internal fun ChooseAndOrder(all: List<String>, enabled: List<String>, minEnabled: Int, label: @Composable (String) -> String, tag: String? = null, onChange: (List<String>) -> Unit) {
    val haptics = LocalHapticFeedback.current
    var order by remember(enabled) { mutableStateOf(enabled.filter { it in all } + all.filter { it !in enabled }) }
    val enabledSet = remember(enabled) { enabled.toSet() }
    val moveUp = stringResource(R.string.a11y_move_up)
    val moveDown = stringResource(R.string.a11y_move_down)
    ReorderableColumn(
        list = order,
        onSettle = { from, to ->
            order = order.toMutableList().apply { add(to, removeAt(from)) }
            onChange(order.filter { it in enabledSet })
        },
        onMove = { haptics.performHapticFeedback(HapticFeedbackType.SegmentFrequentTick) },
        modifier = Modifier.fillMaxWidth().let { if (tag != null) it.testTag(tag) else it },
    ) { index, id, _ ->
        key(id) {
            ReorderableItem {
                val checked = id in enabledSet
                fun toggle(on: Boolean) {
                    val next = if (on) order.filter { it in enabledSet || it == id } else order.filter { it in enabledSet && it != id }
                    if (next.size >= minEnabled) onChange(next)
                }
                fun move(to: Int) {
                    val moved = order.toMutableList().apply { add(to, removeAt(index)) }
                    order = moved
                    onChange(moved.filter { it in enabledSet })
                }
                // One item per action: the row toggles it (role checkbox), and moving it up or down
                // is an action, the accessible alternative to the drag handle.
                Row(
                    Modifier.fillMaxWidth()
                        .toggleable(value = checked, role = Role.Checkbox, onValueChange = ::toggle)
                        .semantics {
                            customActions = buildList {
                                if (checked && index > 0) add(CustomAccessibilityAction(moveUp) { move(index - 1); true })
                                if (checked && index + 1 < order.size && order[index + 1] in enabledSet) add(CustomAccessibilityAction(moveDown) { move(index + 1); true })
                            }
                        }
                        .let { if (tag != null) it.testTag("$tag.$id") else it }
                        .heightIn(min = 48.dp)
                        .padding(horizontal = 16.dp, vertical = 4.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Checkbox(checked = checked, onCheckedChange = null)
                    Spacer(Modifier.width(12.dp))
                    Text(label(id), modifier = Modifier.weight(1f))
                    Icon(Icons.Filled.DragHandle, null, modifier = Modifier.draggableHandle(onDragStopped = { onChange(order.filter { it in enabledSet }) }))
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
