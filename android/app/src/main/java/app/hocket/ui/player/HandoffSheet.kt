package app.hocket.ui.player

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Computer
import androidx.compose.material.icons.filled.PhoneAndroid
import androidx.compose.material.icons.filled.Speaker
import androidx.compose.material.icons.filled.VolumeUp
import androidx.compose.material3.Icon
import androidx.compose.material3.ListItem
import androidx.compose.material3.ListItemDefaults
import androidx.compose.material3.LoadingIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.Text
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.getValue
import androidx.compose.ui.Modifier
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

/**
 * The Connect picker. Opening it makes targets pre-buffer (the "ready" state); playback keeps going
 * here until a target is picked (design: handoff).
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
            devices.firstOrNull { it.isSelf }?.let { self ->
                ListItem(headlineContent = { Text(stringResource(R.string.handoff_this_device)) }, supportingContent = { if (self.playing) Text(stringResource(R.string.handoff_playing)) },
                    leadingContent = { Icon(Icons.Filled.PhoneAndroid, null) }, trailingContent = { if (self.playing) Icon(Icons.Filled.VolumeUp, null, tint = MaterialTheme.colorScheme.primary) },
                    colors = ListItemDefaults.colors(containerColor = Color.Transparent), modifier = Modifier.clickable { if (!self.playing) client.dispatch(Commands.handoffTo(self.id)); onDismiss() })
            }
            if (targets.isEmpty()) {
                EmptyState(stringResource(R.string.empty_devices_title), stringResource(R.string.empty_devices_body))
            } else LazyColumn {
                items(targets, key = { it.id }) { d ->
                    ListItem(
                        headlineContent = { Text(d.name) },
                        supportingContent = { Text(when { d.playing -> stringResource(R.string.handoff_playing); d.ready -> stringResource(R.string.handoff_ready); else -> stringResource(R.string.handoff_preparing) }) },
                        leadingContent = { Icon(when (d.platform) { Platform.Android -> Icons.Filled.PhoneAndroid; Platform.Coordinator -> Icons.Filled.Speaker; else -> Icons.Filled.Computer }, null) },
                        trailingContent = { if (d.playing) Icon(Icons.Filled.VolumeUp, null, tint = MaterialTheme.colorScheme.primary) else if (!d.ready) LoadingIndicator(Modifier.size(28.dp)) else Text(stringResource(R.string.handoff_last_seen, formatAgo(d.lastSeen)), style = MaterialTheme.typography.labelSmall) },
                        colors = ListItemDefaults.colors(containerColor = Color.Transparent),
                        modifier = Modifier.clickable { client.dispatch(Commands.handoffTo(d.id)); onDismiss() }.testTag("handoff.device.${d.id}"),
                    )
                }
            }
            Text(stringResource(R.string.handoff_hint), style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant, modifier = Modifier.padding(24.dp))
        }
    }
}
