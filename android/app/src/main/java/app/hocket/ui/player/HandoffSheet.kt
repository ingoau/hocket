package app.hocket.ui.player

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.Spacer
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
import androidx.compose.material3.Slider
import androidx.compose.material3.Text
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.getValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import app.hocket.R
import app.hocket.core.Commands
import app.hocket.core.api.Command
import app.hocket.core.api.ConnectionTier
import app.hocket.core.api.Platform
import app.hocket.ui.LocalCoreClient
import app.hocket.ui.components.EmptyState
import app.hocket.ui.components.formatAgo
import app.hocket.ui.icons.HocketIcons

/**
 * The Connect picker. Opening it makes targets pre-buffer (the "ready" state); playback keeps going
 * here until a target is picked (design: handoff). On top, the volume of the device playing (this
 * phone's own volume, its music volume, while it plays or nobody does).
 */
@Composable
fun HandoffSheet(onDismiss: () -> Unit) {
    val client = LocalCoreClient.current
    val picker by client.handoffPicker.collectAsStateWithLifecycle()
    val devices by client.devices.collectAsStateWithLifecycle()
    val connection by client.connection.collectAsStateWithLifecycle()
    DisposableEffect(Unit) {
        client.dispatch(Command.OpenHandoffPicker)
        onDispose { client.dispatch(Command.CloseHandoffPicker) }
    }
    val targets = (if (picker.open) picker.targets else devices.filter { !it.isSelf })
    ModalBottomSheet(onDismissRequest = onDismiss, sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = true), modifier = Modifier.testTag("handoff.sheet")) {
        Column(Modifier.navigationBarsPadding()) {
            Text(stringResource(R.string.handoff_title), style = MaterialTheme.typography.titleLarge, modifier = Modifier.padding(horizontal = 24.dp))
            val conn = when (connection.tier) {
                ConnectionTier.Local -> stringResource(R.string.connection_local)
                ConnectionTier.Lan -> stringResource(R.string.connection_lan, connection.peerCount.toInt())
                ConnectionTier.Coordinator -> stringResource(R.string.connection_coordinator, connection.peerCount.toInt())
            }
            Text(conn, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant, modifier = Modifier.padding(horizontal = 24.dp))
            Spacer(Modifier.height(8.dp))
            VolumeRow()
            devices.firstOrNull { it.isSelf }?.let { self ->
                ListItem(headlineContent = { Text(stringResource(R.string.handoff_this_device)) }, supportingContent = { if (self.playing) Text(stringResource(R.string.handoff_playing)) },
                    leadingContent = { Icon(HocketIcons.Filled.PhoneAndroid, null) }, trailingContent = { if (self.playing) Icon(HocketIcons.Filled.VolumeUp, null, tint = MaterialTheme.colorScheme.primary) },
                    colors = ListItemDefaults.colors(containerColor = Color.Transparent), modifier = Modifier.clickable { if (!self.playing) client.dispatch(Commands.handoffTo(self.id)); onDismiss() })
            }
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
            Slider(
                value = shown,
                onValueChange = { client.dispatch(Commands.setVolume(it.toDouble())) },
                enabled = !unknown,
                modifier = Modifier.fillMaxWidth().semantics { contentDescription = label; stateDescription = state }.testTag("handoff.volume.slider"),
            )
        }
        if (unknown) Text(stringResource(R.string.volume_unknown, remote.name), style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
    }
}
