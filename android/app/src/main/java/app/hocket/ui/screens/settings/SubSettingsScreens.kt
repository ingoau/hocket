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
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
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
import androidx.compose.ui.res.pluralStringResource
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
import app.hocket.ui.icons.HocketIcons
import kotlinx.coroutines.flow.flowOf
import kotlinx.coroutines.launch
import kotlinx.serialization.builtins.MapSerializer
import app.hocket.core.HocketJson
import app.hocket.core.api.Command
import app.hocket.core.api.EqBand
import app.hocket.core.api.Query
import app.hocket.core.api.QueryResult
import app.hocket.core.api.ReplayGainMode
import app.hocket.core.api.TransitionMode
import app.hocket.core.api.TransitionSettings
import kotlin.math.roundToInt
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
import androidx.compose.ui.platform.LocalLayoutDirection
import androidx.compose.ui.unit.LayoutDirection
import app.hocket.core.SwipeOptions
import app.hocket.ui.components.SwipeSurface
import app.hocket.ui.components.swipeActionIds
import app.hocket.ui.nav.BottomBarEditor
import app.hocket.ui.nav.label
import androidx.compose.foundation.clickable
import androidx.compose.material3.IconButton
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.liveRegion

/** ReplayGain, preamp, normalisation, gapless, EQ with draggable bands, output device. */
@Composable
fun AudioSettingsScreen(nav: NavHostController) {
    val client = LocalCoreClient.current
    val audio by client.audio.collectAsStateWithLifecycle()
    val outputs by client.outputDevices.collectAsStateWithLifecycle()
    LaunchedEffect(Unit) { client.dispatch(Command.RefreshOutputDevices) }
    SubScreen(nav, stringResource(R.string.settings_section_audio)) {
        val rgOptions = listOf(
            iconOption(ReplayGainMode.Off, stringResource(R.string.settings_rg_off), HocketIcons.Filled.Block),
            iconOption(ReplayGainMode.Track, stringResource(R.string.settings_rg_track), HocketIcons.Filled.MusicNote),
            iconOption(ReplayGainMode.Album, stringResource(R.string.settings_rg_album), HocketIcons.Filled.Album),
            iconOption(ReplayGainMode.Auto, stringResource(R.string.settings_rg_auto), HocketIcons.Filled.AutoAwesome),
        )
        SettingRow(stringResource(R.string.settings_replay_gain), tag = "audio.replayGain")
        OptionCards(rgOptions, isSelected = { audio.replayGain == it }, onSelect = { m -> client.dispatch(Commands.setAudioSettings(audio.copy(replayGain = m))) }, tag = "audio.replayGain.choices", previewHeight = 48.dp)
        val preampLabel = stringResource(R.string.settings_preamp)
        val preampValue = stringResource(R.string.settings_db, audio.replayGainPreampDb)
        SettingRow(preampLabel, preampValue, tag = "audio.preamp")
        // Spoken as "Preamp, +3.0 dB" rather than a bare percentage.
        LabelledSlider(value = audio.replayGainPreampDb.toFloat(), onValueChange = { client.dispatch(Commands.setAudioSettings(audio.copy(replayGainPreampDb = it.toDouble()))) }, valueRange = -15f..15f,
            label = preampLabel, valueText = preampValue, modifier = Modifier.padding(horizontal = 16.dp))
        SwitchRow(stringResource(R.string.settings_normalisation), audio.normalisation, { client.dispatch(Commands.setAudioSettings(audio.copy(normalisation = it))) }, tag = "audio.normalisation")
        SwitchRow(stringResource(R.string.settings_gapless), audio.gapless, { client.dispatch(Commands.setAudioSettings(audio.copy(gapless = it))) }, tag = "audio.gapless")
        TransitionRows(client)
        SettingsSection(stringResource(R.string.settings_eq), tag = "audio.eq")
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
        // The device's own equalizer (or an installed effects app), attached to the player's session.
        val context = androidx.compose.ui.platform.LocalContext.current
        val systemEq = androidx.compose.runtime.remember { app.hocket.playback.AudioEffectSession.controlPanelIntent(context) }
        if (systemEq != null) {
            SettingRow(stringResource(R.string.settings_system_eq), onClick = {
                runCatching { context.startActivity(app.hocket.playback.AudioEffectSession.controlPanelIntent(context) ?: systemEq) }
            }, tag = "audio.systemEq") { Chevron() }
        }
        if (outputs.isNotEmpty()) {
            val selectedOutput = audio.outputDevice ?: outputs.firstOrNull { it.isDefault }?.id
            ChoiceDialogRow(stringResource(R.string.settings_output_device), outputs.map { DialogOption(it.id, it.name, HocketIcons.Filled.Speaker) },
                selected = selectedOutput, onSelect = { client.dispatch(Commands.setOutputDevice(it)) }, tag = "audio.output",
                // A device set before and since unplugged: named by its id rather than left blank.
                valueText = outputs.firstOrNull { it.id == selectedOutput }?.name ?: selectedOutput)
        }
    }
}

/**
 * Off / Crossfade / AutoMix, and the crossfade length while Crossfade is picked (as on desktop). The
 * core owns the value (`playback.transitions`, synced): read with `Query.Transitions`, read again
 * when that setting changes (here or from another device), written with `SetTransitionSettings`.
 * What was just picked shows straight away and gives way to the core's value once it arrives.
 */
@Composable
private fun TransitionRows(client: app.hocket.core.client.CoreClient) {
    val settings by client.settings.collectAsStateWithLifecycle()
    val stored = settings[app.hocket.core.SettingKeys.PLAYBACK_TRANSITIONS]?.value
    var core by remember { mutableStateOf<TransitionSettings?>(null) }
    var picked by remember { mutableStateOf<TransitionSettings?>(null) }
    LaunchedEffect(stored) {
        (client.query(Query.Transitions) as? QueryResult.Transitions)?.let {
            core = it.data
            picked = null
        }
    }
    val value = picked ?: core ?: TransitionSettings(TransitionMode.Off, 6.0)
    fun set(next: TransitionSettings) {
        picked = next
        client.dispatch(Commands.setTransitionSettings(next))
    }
    // AutoMix's limits (never within albums, ...), so it doesn't look broken there.
    SettingRow(stringResource(R.string.settings_transitions), if (value.mode == TransitionMode.AutoMix) stringResource(R.string.settings_transitions_automix_desc) else null, tag = "audio.transitions")
    val modes = listOf(
        Triple(TransitionMode.Off, "off", R.string.settings_transitions_off),
        Triple(TransitionMode.Crossfade, "crossfade", R.string.settings_transitions_crossfade),
        Triple(TransitionMode.AutoMix, "automix", R.string.settings_transitions_automix),
    ).map { (mode, look, label) -> VisualOption(mode, stringResource(label)) { selected -> TransitionPreview(look, selected) } }
    OptionCards(modes, isSelected = { value.mode == it }, onSelect = { set(value.copy(mode = it)) }, tag = "audio.transitions.choices")
    if (value.mode == TransitionMode.Crossfade) {
        val seconds = value.crossfadeSeconds.roundToInt().coerceIn(1, 12)
        var dragged by remember(seconds) { androidx.compose.runtime.mutableFloatStateOf(seconds.toFloat()) }
        val label = stringResource(R.string.settings_crossfade_seconds)
        val valueText = stringResource(R.string.settings_crossfade_seconds_value, dragged.roundToInt())
        SettingRow(label, valueText)
        LabelledSlider(
            value = dragged, onValueChange = { dragged = it }, valueRange = 1f..12f, steps = 10,
            onValueChangeFinished = { set(value.copy(crossfadeSeconds = dragged.roundToInt().toDouble())) },
            label = label, valueText = valueText, modifier = Modifier.padding(horizontal = 16.dp),
        )
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

/**
 * Per-network transcoding profiles: one row per network (Wi-Fi and wired, metered, and this
 * network when it has its own), each summing up its format and bitrate and opening its own page.
 */
@Composable
fun TranscodingSettingsScreen(nav: NavHostController) {
    val client = LocalCoreClient.current
    val network by client.network.collectAsStateWithLifecycle()
    SubScreen(nav, stringResource(R.string.settings_category_streaming)) {
        ProfileRow(nav, "default")
        ProfileRow(nav, "metered")
        // This network's own profile (a Wi-Fi network's SSID hash, or wired), which wins over the
        // metered one; never one of the keys the rows above own.
        network?.networkId?.takeIf { it != "default" && it != "metered" }?.let { id -> ProfileRow(nav, id) }
    }
}

/** A transcoding profile's page title: the network it is for. */
@Composable
internal fun profileTitle(networkId: String): String = stringResource(
    when (networkId) {
        "default" -> R.string.settings_transcoding_wifi
        "metered" -> R.string.settings_transcoding_metered
        else -> R.string.settings_transcoding_current
    },
)

/** The stored profile for [networkId] (an empty one when there is none) and a setter. */
@Composable
private fun rememberProfile(networkId: String): Pair<TranscodingProfile, (TranscodingProfile) -> Unit> {
    val client = LocalCoreClient.current
    val handle = setting(SettingKeys.TRANSCODING_PROFILES)
    val profile = remember(handle.raw, networkId) {
        handle.raw?.let { runCatching { HocketJson.json.decodeFromString(MapSerializer(String.serializer(), TranscodingProfile.serializer()), it) }.getOrNull() }?.get(networkId)
            ?: TranscodingProfile(null, null, emptyList())
    }
    return profile to { p -> client.dispatch(Commands.setTranscodingProfile(networkId.takeIf { it != "default" }, p)) }
}

@Composable
private fun formatLabel(format: String?): String = format?.uppercase() ?: stringResource(R.string.settings_format_original)

@Composable
private fun bitrateLabel(kbps: Int?): String = kbps?.let { stringResource(R.string.settings_bitrate_kbps, it) } ?: stringResource(R.string.settings_bitrate_unlimited)

@Composable
private fun ProfileRow(nav: NavHostController, networkId: String) {
    val (profile, _) = rememberProfile(networkId)
    val summary = formatLabel(profile.format) + stringResource(R.string.dot_separator) + bitrateLabel(profile.maxBitRate?.toInt())
    val tag = "transcoding.${if (networkId == "default" || networkId == "metered") networkId else "current"}"
    SettingRow(profileTitle(networkId), summary, onClick = { nav.navigate(app.hocket.ui.nav.Route.SettingsPage("transcoding.$networkId")) }, tag = tag) { Chevron() }
}

/**
 * One entry of the `transcoding.profiles` map. Keys: `default`; `metered` (any metered connection);
 * `wifi` / `wired` (by kind); any other is a `NetworkState.networkId`.
 */
@Composable
internal fun TranscodingProfileScreen(nav: NavHostController, networkId: String) {
    val (profile, push) = rememberProfile(networkId)
    SubScreen(nav, profileTitle(networkId)) {
        ChoiceDialogRow(stringResource(R.string.settings_format), listOf(null, "opus", "mp3", "aac").map { DialogOption(it, formatLabel(it)) },
            selected = profile.format, onSelect = { push(profile.copy(format = it)) }, tag = "transcoding.format")
        ChoiceDialogRow(stringResource(R.string.settings_max_bitrate), listOf(null, 96, 128, 192, 320).map { DialogOption(it, bitrateLabel(it)) },
            selected = profile.maxBitRate?.toInt(), onSelect = { push(profile.copy(maxBitRate = it?.toUInt())) }, tag = "transcoding.bitrate",
            valueText = bitrateLabel(profile.maxBitRate?.toInt()))
        SettingRow(stringResource(R.string.settings_cannot_decode), stringResource(R.string.settings_cannot_decode_body), tag = "transcoding.cannotDecode")
        CannotDecodeEditor(profile.cannotDecode) { push(profile.copy(cannotDecode = it)) }
    }
}

/** File types players commonly stumble on, offered as chips for the "always transcode" list. */
private val KNOWN_CODECS = listOf("flac", "alac", "wav", "aiff", "ape", "wv", "dsf", "dff", "tak", "shn", "opus", "ogg", "mp3", "m4a", "aac", "wma")

/**
 * The "always transcode" list as chips: the known file types and any others already on the list,
 * each toggled on a tap and saved at once; a field adds a type the chips don't offer.
 */
@OptIn(androidx.compose.foundation.layout.ExperimentalLayoutApi::class)
@Composable
private fun CannotDecodeEditor(selected: List<String>, onChange: (List<String>) -> Unit) {
    // Kept here and updated on each tap: the saved value comes back only once the core has it, so
    // two quick taps building on it would lose the first. Taken afresh whenever the saved value changes.
    var chosen by remember(selected) { mutableStateOf(selected.map { it.trim().lowercase() }.filter { it.isNotEmpty() }) }
    fun change(to: List<String>) { chosen = to; onChange(to) }
    androidx.compose.foundation.layout.FlowRow(
        Modifier.fillMaxWidth().padding(horizontal = 16.dp).testTag("transcoding.cannotDecode.chips"),
        horizontalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        (KNOWN_CODECS + chosen.filter { it !in KNOWN_CODECS }).forEach { c ->
            val on = c in chosen
            androidx.compose.material3.FilterChip(
                selected = on, onClick = { change(if (on) chosen - c else chosen + c) },
                label = { Text(c.uppercase()) },
                leadingIcon = if (on) ({ Icon(HocketIcons.Filled.Check, null, Modifier.size(androidx.compose.material3.FilterChipDefaults.IconSize)) }) else null,
                modifier = Modifier.testTag("transcoding.cannotDecode.$c"),
            )
        }
    }
    var other by remember { mutableStateOf("") }
    fun add() {
        val c = other.trim().trimStart('.').lowercase()
        if (c.isNotEmpty() && c !in chosen) change(chosen + c)
        other = ""
    }
    OutlinedTextField(
        value = other, onValueChange = { other = it }, label = { Text(stringResource(R.string.settings_cannot_decode_other)) }, singleLine = true,
        keyboardOptions = androidx.compose.foundation.text.KeyboardOptions(imeAction = androidx.compose.ui.text.input.ImeAction.Done),
        keyboardActions = androidx.compose.foundation.text.KeyboardActions(onDone = { add() }),
        trailingIcon = { TextButton(onClick = ::add, enabled = other.isNotBlank()) { Text(stringResource(R.string.settings_cannot_decode_add)) } },
        modifier = Modifier.fillMaxWidth().padding(horizontal = 16.dp).padding(top = 4.dp, bottom = 8.dp).testTag("transcoding.cannotDecode.other"),
    )
}

/**
 * LAN discovery, this device, and the identity: its devices and its coordinator (claim or join one,
 * then its address, connection and Disconnect / Reconnect).
 */
@Composable
fun ConnectSettingsScreen(nav: NavHostController) {
    val client = LocalCoreClient.current
    val connection by client.connection.collectAsStateWithLifecycle()
    val devices by client.devices.collectAsStateWithLifecycle()
    val lan = setting(SettingKeys.CONNECT_LAN_DISCOVERY)
    SubScreen(nav, stringResource(R.string.settings_category_connect)) {
        // The devices found on this network, while that is how this device is connected.
        val lanPeers = if (connection.tier == app.hocket.core.api.ConnectionTier.Lan) pluralStringResource(R.plurals.connection_lan, connection.peerCount.toInt(), connection.peerCount.toInt()) else null
        SwitchRow(stringResource(R.string.settings_lan_discovery), lan.bool ?: true, { client.dispatch(Commands.setLanDiscovery(it)) }, subtitle = lanPeers, scope = lan.scope, tag = "connect.lan")
        devices.firstOrNull { it.isSelf }?.let { SettingRow(stringResource(R.string.settings_device_name, it.name), it.id) }
        IdentitySettings()
    }
}

/**
 * Customise: one row per editor (bottom bar, item menus, notification buttons, swipe actions),
 * each summing up what is set and opening its own page (SettingsPages.kt); then what swiping up on
 * the player opens, as cards.
 */
@Composable
fun CustomiseSettingsScreen(nav: NavHostController) {
    SubScreen(nav, stringResource(R.string.settings_section_customise)) {
        fun open(page: String) = nav.navigate(app.hocket.ui.nav.Route.SettingsPage(page))
        val bar = app.hocket.ui.nav.navItems()
        NavRow(stringResource(R.string.settings_bottom_bar), bar.map { it.label() }.joinToString(", "), tag = "customise.bottomBar") { open(PAGE_BOTTOM_BAR) }
        val menu = setting(SettingKeys.MENU_OVERRIDES)
        val changed = remember(menu.raw) { menuOverrideCount(menu.raw) }
        NavRow(stringResource(R.string.settings_item_menus), if (changed == 0) stringResource(R.string.settings_item_menus_default) else pluralStringResource(R.plurals.settings_item_menus_changed, changed, changed),
            scope = menu.scope, tag = "customise.itemMenuSection") { open(PAGE_ITEM_MENU) }
        val media = mediaSessionIds().filter { it in ActionIds.MEDIA_SESSION_CHOICES }
        NavRow(stringResource(R.string.settings_media_buttons), media.map { actionLabel(it) }.joinToString(", ").ifEmpty { null }, scope = setting(SettingKeys.ACTIONS_ORDER_MEDIA_SESSION).scope, tag = "customise.mediaSessionSection") { open(PAGE_MEDIA_BUTTONS) }
        NavRow(stringResource(R.string.settings_swipe_actions), scope = setting(SettingKeys.SWIPE_LIST_START_TO_END).scope, tag = "customise.swipe") { open(PAGE_SWIPE) }
        // swipe.playerUp: what swiping up on the full player's artwork opens.
        val playerUp = setting(SettingKeys.SWIPE_PLAYER_UP)
        val playerUpOptions = listOf(
            iconOption(SwipeOptions.PLAYER_UP_QUEUE, stringResource(R.string.settings_player_swipe_up_queue), HocketIcons.AutoMirrored.Filled.QueueMusic),
            iconOption(SwipeOptions.PLAYER_UP_LYRICS, stringResource(R.string.settings_player_swipe_up_lyrics), HocketIcons.Filled.Lyrics),
            iconOption(SwipeOptions.PLAYER_UP_CONNECT, stringResource(R.string.settings_player_swipe_up_connect), HocketIcons.Outlined.Devices),
            iconOption(SwipeOptions.PLAYER_UP_MENU, stringResource(R.string.settings_player_swipe_up_menu), HocketIcons.Filled.MoreVert),
        )
        SettingRow(stringResource(R.string.settings_player_swipe_up), scope = playerUp.scope, tag = "customise.playerSwipeUp")
        OptionCards(
            playerUpOptions,
            isSelected = { (playerUp.string ?: SwipeOptions.PLAYER_UP_QUEUE) == it },
            onSelect = { playerUp.setString(it) },
            tag = "customise.${SettingKeys.SWIPE_PLAYER_UP}", previewHeight = 48.dp,
        )
    }
}

/** How many item menu rows `menu.overrides` (a JSON object) moves from their default place. */
internal fun menuOverrideCount(raw: String?): Int =
    runCatching { (app.hocket.core.HocketJson.json.parseToJsonElement(raw ?: "{}") as? kotlinx.serialization.json.JsonObject)?.size ?: 0 }.getOrDefault(0)

/** The notification's buttons in order; the default set while none are stored. */
@Composable
internal fun mediaSessionIds(): List<String> = storedIds(setting(SettingKeys.ACTIONS_ORDER_MEDIA_SESSION)) ?: ActionIds.MEDIA_SESSION

/** A stored id list, or null when there is none (or it is empty: the default). */
internal fun storedIds(handle: SettingHandle): List<String>? =
    handle.raw?.let { runCatching { HocketJson.json.decodeFromString(ListSerializer(String.serializer()), it) }.getOrNull() }?.ifEmpty { null }

/**
 * One surface's two swipe settings (core registry keys, synced): a picture of a song row with both
 * swipes showing, then a choice of action per direction. The directions are named as the finger
 * moves on screen, so right-to-left layouts swap them (and the picture mirrors, as the row does).
 */
@Composable
internal fun SwipeChoices(title: String, options: List<String>, surface: SwipeSurface) {
    val rtl = LocalLayoutDirection.current == LayoutDirection.Rtl
    val (startToEnd, endToStart) = swipeActionIds(surface)
    val none = stringResource(R.string.settings_swipe_none)
    val section = app.hocket.ui.components.queueSectionOn()
    SettingsSection(title)
    SwipeRowPreview(startToEnd, endToStart, Modifier.padding(horizontal = 16.dp, vertical = 8.dp))
    for ((key, current) in listOf(surface.startToEndKey to startToEnd, surface.endToStartKey to endToStart)) {
        val handle = setting(key)
        val right = (key == surface.startToEndKey) != rtl
        ChoiceDialogRow(
            stringResource(if (right) R.string.settings_swipe_right else R.string.settings_swipe_left),
            // Add to queue is offered with the queue section on (off, it runs as play later).
            options.filter { it != ActionIds.ADD_TO_QUEUE || section || it == current }.map { id ->
                if (id == SwipeOptions.NONE) DialogOption(id, none, HocketIcons.Filled.Block) else DialogOption(id, actionLabel(id), app.hocket.ui.components.swipeLook(id)?.icon)
            },
            selected = current, onSelect = { handle.setString(it) }, scope = handle.scope, tag = "customise.$key",
            // By the key: the icons mirror themselves in right-to-left layouts, as the names swap.
            trailing = { Icon(if (key == surface.startToEndKey) HocketIcons.AutoMirrored.Filled.SwipeRight else HocketIcons.AutoMirrored.Filled.SwipeLeft, null, tint = MaterialTheme.colorScheme.onSurfaceVariant) },
        )
    }
}

/**
 * Choose and order: what is [enabled] ("Shown"), in order, each with its icon, a hide button and a
 * drag handle (or the row's Move up / Move down actions for TalkBack); then the rest ("Hidden"),
 * each shown again with a tap; then Reset to default ([default]). Hiding past [minEnabled] says
 * why instead of doing it. [label] and [icon] name and picture each id; rows are tagged
 * `<tag>.<id>`.
 */
@Composable
internal fun ChooseAndOrder(
    all: List<String>,
    enabled: List<String>,
    minEnabled: Int,
    label: @Composable (String) -> String,
    icon: (String) -> androidx.compose.ui.graphics.vector.ImageVector?,
    default: List<String>,
    tag: String,
    onChange: (List<String>) -> Unit,
) {
    val haptics = LocalHapticFeedback.current
    // What this list last wrote, shown until the stored value comes back (and replaced when it changes).
    var shown by remember(enabled) { mutableStateOf(enabled.filter { it in all }) }
    val hidden = all.filter { it !in shown }
    var message by remember { mutableStateOf<String?>(null) }
    fun update(next: List<String>) { message = null; shown = next; onChange(next) }
    val moveUp = stringResource(R.string.a11y_move_up)
    val moveDown = stringResource(R.string.a11y_move_down)
    val minimum = pluralStringResource(R.plurals.settings_order_minimum, minEnabled, minEnabled)
    val scheme = MaterialTheme.colorScheme
    Column(Modifier.fillMaxWidth().testTag(tag)) {
        SettingsSection(stringResource(R.string.settings_order_shown))
        ReorderableColumn(
            list = shown,
            onSettle = { from, to -> update(shown.toMutableList().apply { add(to, removeAt(from)) }) },
            onMove = { haptics.performHapticFeedback(HapticFeedbackType.SegmentFrequentTick) },
            modifier = Modifier.fillMaxWidth(),
        ) { index, id, _ ->
            key(id) {
                ReorderableItem {
                    val name = label(id)
                    Row(
                        Modifier.fillMaxWidth()
                            .semantics(mergeDescendants = true) {
                                customActions = buildList {
                                    if (index > 0) add(CustomAccessibilityAction(moveUp) { update(shown.toMutableList().apply { add(index - 1, removeAt(index)) }); true })
                                    if (index < shown.lastIndex) add(CustomAccessibilityAction(moveDown) { update(shown.toMutableList().apply { add(index + 1, removeAt(index)) }); true })
                                }
                            }
                            .testTag("$tag.$id")
                            .heightIn(min = 56.dp)
                            .padding(start = 16.dp, end = 4.dp),
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        icon(id)?.let { Icon(it, null, tint = scheme.primary); Spacer(Modifier.width(16.dp)) }
                        Text(name, style = MaterialTheme.typography.bodyLarge, modifier = Modifier.weight(1f))
                        IconButton(
                            onClick = { if (shown.size <= minEnabled) message = minimum else update(shown - id) },
                            modifier = Modifier.testTag("$tag.hide.$id"),
                        ) { Icon(HocketIcons.Filled.RemoveCircleOutline, stringResource(R.string.settings_order_hide, name)) }
                        Box(Modifier.padding(12.dp).draggableHandle(onDragStarted = { haptics.performHapticFeedback(HapticFeedbackType.GestureThresholdActivate) })) {
                            Icon(HocketIcons.Filled.DragHandle, null)
                        }
                    }
                }
            }
        }
        message?.let {
            Text(it, color = scheme.error, style = MaterialTheme.typography.bodyMedium,
                modifier = Modifier.padding(horizontal = 16.dp, vertical = 4.dp).semantics { liveRegion = LiveRegionMode.Polite }.testTag("$tag.message"))
        }
        if (hidden.isNotEmpty()) {
            SettingsSection(stringResource(R.string.settings_order_hidden))
            hidden.forEach { id ->
                val name = label(id)
                Row(
                    Modifier.fillMaxWidth()
                        .clickable(onClickLabel = stringResource(R.string.settings_order_show, name)) { update(shown + id) }
                        .testTag("$tag.$id")
                        .heightIn(min = 56.dp)
                        .padding(start = 16.dp, end = 4.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    icon(id)?.let { Icon(it, null, tint = scheme.onSurfaceVariant); Spacer(Modifier.width(16.dp)) }
                    Text(name, style = MaterialTheme.typography.bodyLarge, color = scheme.onSurfaceVariant, modifier = Modifier.weight(1f))
                    Box(Modifier.padding(12.dp)) { Icon(HocketIcons.Filled.AddCircle, null, tint = scheme.primary) }
                }
            }
        }
        val reset = default.filter { it in all }
        TextButton(onClick = { update(reset) }, enabled = shown != reset, modifier = Modifier.padding(horizontal = 8.dp, vertical = 8.dp).testTag("$tag.reset")) {
            Text(stringResource(R.string.bottom_bar_reset))
        }
    }
}

@Composable
fun AboutScreen(nav: NavHostController) {
    val context = androidx.compose.ui.platform.LocalContext.current
    val version = remember { runCatching { context.packageManager.getPackageInfo(context.packageName, 0).versionName }.getOrNull() ?: "?" }
    SubScreen(nav, stringResource(R.string.settings_section_about)) {
        // The other settings screens' rows and section heading, so the page reads like them.
        SettingRow(stringResource(R.string.settings_version, version), subtitle = stringResource(R.string.settings_licence))
        SettingsSection(stringResource(R.string.settings_licences_title))
        Text(stringResource(R.string.settings_licences_body), style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant, modifier = Modifier.padding(horizontal = 16.dp, vertical = 8.dp))
    }
}
