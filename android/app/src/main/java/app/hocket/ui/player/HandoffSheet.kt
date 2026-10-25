package app.hocket.ui.player

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.ListItem
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
import androidx.compose.ui.graphics.Color
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
import app.hocket.core.api.ConnectionTier
import app.hocket.core.api.Platform
import app.hocket.ui.LocalCoreClient
import app.hocket.ui.components.EmptyState
import app.hocket.ui.components.SectionHeader
import app.hocket.ui.components.LabelledSlider
import app.hocket.ui.components.formatAgo
import app.hocket.ui.icons.HocketIcons

/** Opens Connect & devices settings; provided by the app's root, where navigation lives. */
val LocalOpenConnectSettings = staticCompositionLocalOf<(() -> Unit)?> { null }

/**
 * The Connect picker ("Play on"). Opening it makes targets pre-buffer (the "ready" state); playback
 * keeps going here until a target is picked (design: handoff). On top, the volume of the device
 * playing (this phone's own volume, its music volume, while it plays or nobody does); then this
 * phone's connected outputs (speaker, earphones; [AudioOutputs]), switchable either way, with a link
 * to Bluetooth settings for connecting another; then the other Hocket devices. The gear by the
 * title opens Connect & devices settings ([onOpenSettings]).
 */
@Composable
fun HandoffSheet(onDismiss: () -> Unit, onOpenSettings: (() -> Unit)? = LocalOpenConnectSettings.current) {
    val client = LocalCoreClient.current
    val picker by client.handoffPicker.collectAsStateWithLifecycle()
    val devices by client.devices.collectAsStateWithLifecycle()
    val connection by client.connection.collectAsStateWithLifecycle()
    DisposableEffect(Unit) {
        client.dispatch(Command.OpenHandoffPicker)
        onDispose { client.dispatch(Command.CloseHandoffPicker) }
    }
    val targets = (if (picker.open) picker.targets else devices.filter { !it.isSelf })
    val context = LocalContext.current
    DisposableEffect(context) { AudioOutputs.start(context); onDispose { } }
    val outputs by AudioOutputs.outputs.collectAsStateWithLifecycle()
    ModalBottomSheet(onDismissRequest = onDismiss, sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = true), modifier = Modifier.testTag("handoff.sheet")) {
        Column(Modifier.navigationBarsPadding()) {
            Row(Modifier.fillMaxWidth().padding(start = 24.dp, end = 12.dp), verticalAlignment = Alignment.CenterVertically) {
                Text(stringResource(R.string.handoff_title), style = MaterialTheme.typography.titleLarge, modifier = Modifier.weight(1f))
                if (onOpenSettings != null) IconButton(onClick = { onDismiss(); onOpenSettings() }, modifier = Modifier.testTag("handoff.settings")) { Icon(HocketIcons.Filled.Settings, stringResource(R.string.handoff_settings)) }
            }
            val conn = when (connection.tier) {
                ConnectionTier.Local -> stringResource(R.string.connection_local)
                ConnectionTier.Lan -> stringResource(R.string.connection_lan, connection.peerCount.toInt())
                ConnectionTier.Coordinator -> stringResource(R.string.connection_coordinator, connection.peerCount.toInt())
            }
            Text(conn, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant, modifier = Modifier.padding(horizontal = 24.dp))
            Spacer(Modifier.height(8.dp))
            VolumeRow()
            devices.firstOrNull { it.isSelf }?.let { self ->
                // Another device plays: picking an output here takes playback back to this phone first.
                val elsewhere = !self.playing && devices.any { it.playing && !it.isSelf }
                val pick = { id: Int? -> if (elsewhere) client.dispatch(Commands.handoffTo(self.id)); AudioOutputs.select(id); onDismiss() }
                SectionHeader(stringResource(R.string.handoff_this_phone))
                if (outputs.outputs.isEmpty()) {
                    OutputRow(stringResource(R.string.handoff_this_device), HocketIcons.Filled.PhoneAndroid, self.playing, Modifier.clickable { pick(null) })
                } else outputs.outputs.forEach { o ->
                    val on = !elsewhere && o.id == outputs.current?.id
                    OutputRow(outputName(o), outputIcon(o.kind), on, Modifier.clickable { pick(o.id) }.testTag("handoff.output.${o.id}"))
                }
                TextButton(onClick = { runCatching { context.startActivity(Intent(Settings.ACTION_BLUETOOTH_SETTINGS).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)) } }, modifier = Modifier.padding(horizontal = 12.dp).testTag("handoff.bluetooth")) {
                    Icon(HocketIcons.Filled.Bluetooth, null)
                    Spacer(Modifier.width(8.dp))
                    Text(stringResource(R.string.handoff_bluetooth_settings))
                }
            }
            SectionHeader(stringResource(R.string.handoff_other_devices))
            if (targets.isEmpty()) {
                EmptyState(stringResource(R.string.empty_devices_title), stringResource(R.string.empty_devices_body))
            } else LazyColumn {
                items(targets, key = { it.id }) { d ->
                    ListItem(
                        headlineContent = { Text(d.name) },
                        supportingContent = { Text(when { d.playing -> stringResource(R.string.handoff_playing); d.ready -> stringResource(R.string.handoff_ready); else -> stringResource(R.string.handoff_preparing) }) },
                        leadingContent = { Icon(when (d.platform) { Platform.Android -> HocketIcons.Filled.PhoneAndroid; Platform.Coordinator -> HocketIcons.Filled.Speaker; else -> HocketIcons.Filled.Computer }, null) },
                        trailingContent = { if (d.playing) Icon(HocketIcons.Filled.VolumeUp, null, tint = MaterialTheme.colorScheme.primary) else if (!d.ready) LoadingIndicator(Modifier.size(28.dp)) else Text(stringResource(R.string.handoff_last_seen, formatAgo(d.lastSeen)), style = MaterialTheme.typography.labelSmall) },
                        colors = ListItemDefaults.colors(containerColor = Color.Transparent),
                        modifier = Modifier.clickable { client.dispatch(Commands.handoffTo(d.id)); onDismiss() }.testTag("handoff.device.${d.id}"),
                    )
                }
            }
            Text(stringResource(R.string.handoff_hint), style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant, modifier = Modifier.padding(24.dp))
        }
    }
}

@Composable
private fun OutputRow(name: String, icon: androidx.compose.ui.graphics.vector.ImageVector, playing: Boolean, modifier: Modifier) {
    ListItem(headlineContent = { Text(name) }, supportingContent = { if (playing) Text(stringResource(R.string.handoff_playing)) },
        leadingContent = { Icon(icon, null, tint = if (playing) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.onSurfaceVariant) },
        trailingContent = { if (playing) Icon(HocketIcons.Filled.VolumeUp, null, tint = MaterialTheme.colorScheme.primary) },
        colors = ListItemDefaults.colors(containerColor = Color.Transparent), modifier = modifier)
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
    AudioOutputs.Kind.Phone -> HocketIcons.Filled.PhoneAndroid
    AudioOutputs.Kind.Bluetooth -> HocketIcons.Filled.Bluetooth
    AudioOutputs.Kind.Wired -> HocketIcons.Filled.Headphones
    AudioOutputs.Kind.Usb -> HocketIcons.Filled.Usb
    AudioOutputs.Kind.Other -> HocketIcons.Filled.Speaker
}

/**
 * The volume control: the device playing (named while that is another device), or this phone when
 * nothing plays elsewhere. The core routes the change; each device keeps its own level and mute.
 */
@Composable
private fun VolumeRow() {
    val client = LocalCoreClient.current
    val transport by client.transport.collectAsStateWithLifecycle()
    val devices by client.devices.collectAsStateWithLifecycle()
    val remote = devices.firstOrNull { it.id == transport.lease.owner && !it.isSelf }
    val unknown = remote != null && transport.volumeDevice != remote.id
    val muted = transport.muted == true
    val shown = if (muted || unknown) 0f else transport.volume.toFloat().coerceIn(0f, 1f)
    val label = if (remote != null) stringResource(R.string.handoff_volume_on, remote.name) else stringResource(R.string.player_volume)
    val percent = "${(shown * 100).toInt()}%"
    val state = if (muted) stringResource(R.string.volume_muted) else percent
    Column(Modifier.padding(horizontal = 24.dp).testTag("handoff.volume")) {
        Text(label, style = MaterialTheme.typography.labelLarge, modifier = Modifier.padding(top = 4.dp))
        Row(verticalAlignment = Alignment.CenterVertically) {
            IconButton(
                onClick = { client.dispatch(Commands.setMuted(!muted)) },
                enabled = !unknown,
                modifier = Modifier.testTag("handoff.volume.mute"),
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
        }
        if (unknown) Text(stringResource(R.string.volume_unknown, remote.name), style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
    }
}
