package app.hocket.ui.player

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.FilledTonalIconButton
import androidx.compose.material3.IconButtonDefaults
import androidx.compose.material3.SegmentedListItem
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.selected
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import app.hocket.ui.components.GroupLabel
import app.hocket.ui.components.Segment
import app.hocket.ui.components.SegmentedGroup
import app.hocket.ui.components.segmentColor
import app.hocket.ui.components.segmentedListColors
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.ListItemDefaults
import androidx.compose.material3.LoadingIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import android.content.Intent
import android.provider.Settings
import app.hocket.R
import app.hocket.playback.AudioOutputs
import app.hocket.core.Commands
import app.hocket.core.api.Command
import app.hocket.core.api.Platform
import app.hocket.ui.LocalCoreClient
import app.hocket.ui.components.LabelledSlider
import app.hocket.ui.components.formatAgo
import app.hocket.ui.icons.HocketIcons

/** Opens Connect & devices settings; provided by the app's root, where navigation lives. */
val LocalOpenConnectSettings = staticCompositionLocalOf<(() -> Unit)?> { null }

/**
 * The Connect picker ("Play on"). Opening it makes targets pre-buffer (the "ready" state); playback
 * keeps going here until a target is picked (design: handoff). On top, on a card, the volume of the
 * device playing (this phone's own volume, its music volume, while it plays or nobody does); then
 * this phone's connected outputs (speaker, earphones; [AudioOutputs]) as a segmented group, the one
 * playing filled, switchable either way, with a link to Bluetooth settings for connecting another;
 * then the other Hocket devices. The gear by the title opens Connect & devices settings
 * ([onOpenSettings]).
 */
@Composable
fun HandoffSheet(onDismiss: () -> Unit, onOpenSettings: (() -> Unit)? = LocalOpenConnectSettings.current) {
    val client = LocalCoreClient.current
    val picker by client.handoffPicker.collectAsStateWithLifecycle()
    val devices by client.devices.collectAsStateWithLifecycle()
    DisposableEffect(Unit) {
        client.dispatch(Command.OpenHandoffPicker)
        onDispose { client.dispatch(Command.CloseHandoffPicker) }
    }
    val targets = (if (picker.open) picker.targets else devices.filter { !it.isSelf })
    val context = LocalContext.current
    DisposableEffect(context) { AudioOutputs.start(context); onDispose { } }
    val outputs by AudioOutputs.outputs.collectAsStateWithLifecycle()
    val self = devices.firstOrNull { it.isSelf }
    // The output playing on this phone, named on the volume card ("Volume – Pixel Buds Pro").
    val currentOutput = outputs.current?.takeIf { self?.playing != false }?.let { outputName(it) }
    ModalBottomSheet(onDismissRequest = onDismiss, sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = true), modifier = Modifier.testTag("handoff.sheet")) {
        Column(Modifier.navigationBarsPadding().verticalScroll(rememberScrollState()).padding(start = 16.dp, end = 16.dp, bottom = 24.dp)) {
            Row(Modifier.fillMaxWidth().padding(start = 8.dp, bottom = 12.dp), verticalAlignment = Alignment.CenterVertically) {
                Text(stringResource(R.string.handoff_title), style = MaterialTheme.typography.headlineSmall, fontWeight = FontWeight.SemiBold, modifier = Modifier.weight(1f).semantics { heading() })
                if (onOpenSettings != null) FilledTonalIconButton(
                    onClick = { onDismiss(); onOpenSettings() }, shapes = IconButtonDefaults.shapes(),
                    colors = IconButtonDefaults.filledTonalIconButtonColors(containerColor = segmentColor(), contentColor = MaterialTheme.colorScheme.onSurface),
                    modifier = Modifier.size(IconButtonDefaults.mediumContainerSize()).testTag("handoff.settings"),
                ) { Icon(HocketIcons.Outlined.Settings, stringResource(R.string.handoff_settings)) }
            }
            VolumeCard(currentOutput)
            if (self != null) {
                // Another device plays: picking an output here takes playback back to this phone first.
                val elsewhere = !self.playing && devices.any { it.playing && !it.isSelf }
                val pick = { id: Int? -> if (elsewhere) client.dispatch(Commands.handoffTo(self.id)); AudioOutputs.select(id); onDismiss() }
                GroupLabel(stringResource(R.string.handoff_this_phone), Modifier.padding(top = 16.dp), color = MaterialTheme.colorScheme.primary)
                SegmentedGroup {
                    if (outputs.outputs.isEmpty()) {
                        DeviceRow(0, 1, stringResource(R.string.handoff_this_device), if (self.playing) stringResource(R.string.handoff_playing) else null, HocketIcons.Outlined.PhoneAndroid, self.playing, onClick = { pick(null) })
                    } else outputs.outputs.forEachIndexed { i, o ->
                        val on = self.playing && o.id == outputs.current?.id
                        val detail = outputDetail(o)
                        val sub = if (on) listOfNotNull(stringResource(R.string.handoff_playing), detail).joinToString(stringResource(R.string.dot_separator)) else detail
                        DeviceRow(i, outputs.outputs.size, outputName(o), sub, outputIcon(o.kind), on, onClick = { pick(o.id) }, modifier = Modifier.testTag("handoff.output.${o.id}"))
                    }
                }
                Row(Modifier.fillMaxWidth().padding(top = 4.dp), horizontalArrangement = Arrangement.End) {
                    TextButton(onClick = { runCatching { context.startActivity(Intent(Settings.ACTION_BLUETOOTH_SETTINGS).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)) } }, shapes = ButtonDefaults.shapes(), modifier = Modifier.testTag("handoff.bluetooth")) {
                        Icon(HocketIcons.Filled.Bluetooth, null, Modifier.size(ButtonDefaults.IconSize))
                        Spacer(Modifier.width(ButtonDefaults.IconSpacing))
                        Text(stringResource(R.string.handoff_bluetooth_settings))
                        Spacer(Modifier.width(ButtonDefaults.IconSpacing))
                        Icon(HocketIcons.Filled.OpenInNew, null, Modifier.size(ButtonDefaults.IconSize))
                    }
                }
            }
            GroupLabel(stringResource(R.string.handoff_other_devices), Modifier.padding(top = 8.dp), color = MaterialTheme.colorScheme.primary)
            if (targets.isEmpty()) {
                Segment(0, 1) {
                    Column(Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(4.dp)) {
                        Text(stringResource(R.string.empty_devices_title), style = MaterialTheme.typography.titleMedium)
                        Text(stringResource(R.string.empty_devices_body), style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
                    }
                }
            } else SegmentedGroup {
                targets.forEachIndexed { i, d ->
                    val state = when { d.playing -> stringResource(R.string.handoff_playing); d.ready -> stringResource(R.string.handoff_ready); else -> stringResource(R.string.handoff_preparing) }
                    DeviceRow(i, targets.size, d.name, state, deviceRowIcon(d.platform), d.playing,
                        onClick = { client.dispatch(Commands.handoffTo(d.id)); onDismiss() }, modifier = Modifier.testTag("handoff.device.${d.id}"),
                        trailing = when {
                            d.playing -> null
                            !d.ready -> ({ LoadingIndicator(Modifier.size(32.dp)) })
                            else -> ({ Text(stringResource(R.string.handoff_last_seen, formatAgo(d.lastSeen)), style = MaterialTheme.typography.labelMedium, color = MaterialTheme.colorScheme.onSurfaceVariant) })
                        })
                }
            }
        }
    }
}

/**
 * One output or device: its icon in a tonal circle, its name and state. The one playing is filled
 * with the primary colour and ends in the playing bars, so where the sound goes reads at a glance.
 */
@Composable
private fun DeviceRow(
    index: Int, count: Int, name: String, supporting: String?, icon: ImageVector, playing: Boolean,
    onClick: () -> Unit, modifier: Modifier = Modifier, trailing: (@Composable () -> Unit)? = null,
) {
    val onP = MaterialTheme.colorScheme.onPrimary
    val colors = if (playing) ListItemDefaults.segmentedColors(
        containerColor = MaterialTheme.colorScheme.primary, contentColor = onP, supportingContentColor = onP.copy(alpha = 0.82f),
        leadingContentColor = onP, trailingContentColor = onP,
    ) else segmentedListColors()
    SegmentedListItem(
        onClick = onClick, shapes = ListItemDefaults.segmentedShapes(index, count), colors = colors,
        modifier = modifier.semantics { if (playing) selected = true },
        leadingContent = {
            val circle = if (playing) MaterialTheme.colorScheme.onPrimary.copy(alpha = 0.16f) else MaterialTheme.colorScheme.secondaryContainer
            Box(Modifier.size(44.dp).background(circle, CircleShape), contentAlignment = Alignment.Center) {
                Icon(icon, null, tint = if (playing) MaterialTheme.colorScheme.onPrimary else MaterialTheme.colorScheme.onSecondaryContainer)
            }
        },
        supportingContent = supporting?.let { { Text(it, maxLines = 1, overflow = TextOverflow.Ellipsis) } },
        trailingContent = if (playing) ({ Icon(HocketIcons.Filled.Equalizer, null) }) else trailing,
    ) { Text(name, maxLines = 1, overflow = TextOverflow.Ellipsis) }
}

private fun deviceRowIcon(platform: Platform): ImageVector = when (platform) {
    Platform.Android -> HocketIcons.Outlined.PhoneAndroid
    Platform.Coordinator -> HocketIcons.Outlined.Speaker
    Platform.MacOs, Platform.Windows, Platform.Linux -> HocketIcons.Outlined.Laptop
    else -> HocketIcons.Outlined.DesktopWindows
}

/** What an output is, under its name: the phone's model for its speaker, the link for the rest. */
@Composable
private fun outputDetail(o: AudioOutputs.Output): String? = when (o.kind) {
    AudioOutputs.Kind.Phone -> android.os.Build.MODEL?.takeIf { it.isNotBlank() }
    AudioOutputs.Kind.Bluetooth -> stringResource(R.string.handoff_bluetooth_short)
    AudioOutputs.Kind.Wired -> null
    AudioOutputs.Kind.Usb -> stringResource(R.string.handoff_usb_short)
    AudioOutputs.Kind.Other -> null
}

@Composable
private fun outputName(o: AudioOutputs.Output): String = when {
    o.kind == AudioOutputs.Kind.Phone -> stringResource(R.string.handoff_phone_speaker)
    o.name.isNotBlank() && o.kind == AudioOutputs.Kind.Bluetooth -> o.name
    o.kind == AudioOutputs.Kind.Wired -> stringResource(R.string.handoff_wired)
    o.kind == AudioOutputs.Kind.Usb -> o.name.ifBlank { stringResource(R.string.handoff_usb) }
    else -> o.name.ifBlank { stringResource(R.string.handoff_bluetooth) }
}

private fun outputIcon(kind: AudioOutputs.Kind) = when (kind) {
    AudioOutputs.Kind.Phone -> HocketIcons.Outlined.PhoneAndroid
    AudioOutputs.Kind.Bluetooth -> HocketIcons.Outlined.Headphones
    AudioOutputs.Kind.Wired -> HocketIcons.Outlined.Headphones
    AudioOutputs.Kind.Usb -> HocketIcons.Filled.Usb
    AudioOutputs.Kind.Other -> HocketIcons.Outlined.Speaker
}

/**
 * The volume card: the device playing (named while that is another device, or the output playing
 * here), or this phone when nothing plays elsewhere. The core routes the change; each device keeps
 * its own level and mute.
 */
@Composable
private fun VolumeCard(output: String?) {
    val client = LocalCoreClient.current
    val transport by client.transport.collectAsStateWithLifecycle()
    val devices by client.devices.collectAsStateWithLifecycle()
    val remote = devices.firstOrNull { it.id == transport.lease.owner && !it.isSelf }
    val unknown = remote != null && transport.volumeDevice != remote.id
    val muted = transport.muted == true
    val shown = if (muted || unknown) 0f else transport.volume.toFloat().coerceIn(0f, 1f)
    val label = (remote?.name ?: output)?.let { stringResource(R.string.handoff_volume_on, it) } ?: stringResource(R.string.player_volume)
    val percent = "${(shown * 100).toInt()}%"
    val state = if (muted) stringResource(R.string.volume_muted) else percent
    Segment(0, 1, Modifier.testTag("handoff.volume")) {
        Column(Modifier.padding(start = 16.dp, end = 16.dp, top = 14.dp, bottom = 8.dp)) {
            Text(label, style = MaterialTheme.typography.titleSmall, fontWeight = FontWeight.SemiBold, maxLines = 1, overflow = TextOverflow.Ellipsis)
            Row(verticalAlignment = Alignment.CenterVertically) {
                IconButton(
                    onClick = { client.dispatch(Commands.setMuted(!muted)) },
                    enabled = !unknown, shapes = IconButtonDefaults.shapes(),
                    modifier = Modifier.offset(x = (-12).dp).testTag("handoff.volume.mute"),
                ) {
                    Icon(if (muted || shown == 0f) HocketIcons.Filled.VolumeOff else HocketIcons.Filled.VolumeUp,
                        stringResource(if (muted) R.string.volume_unmute else R.string.volume_mute))
                }
                LabelledSlider(
                    value = shown,
                    onValueChange = { client.dispatch(Commands.setVolume(it.toDouble())) },
                    label = label,
                    valueText = state,
                    enabled = !unknown,
                    modifier = Modifier.weight(1f).testTag("handoff.volume.slider"),
                )
                Text(state, style = MaterialTheme.typography.titleSmall, modifier = Modifier.padding(start = 12.dp).widthIn(min = 40.dp), textAlign = TextAlign.End)
            }
            if (unknown) Text(stringResource(R.string.volume_unknown, remote.name), style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant, modifier = Modifier.padding(bottom = 8.dp))
        }
    }
}
