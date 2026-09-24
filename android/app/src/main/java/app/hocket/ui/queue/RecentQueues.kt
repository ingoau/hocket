package app.hocket.ui.queue

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.filled.Delete
import androidx.compose.material.icons.filled.History
import androidx.compose.material.icons.filled.PlaylistAdd
import androidx.compose.material.icons.filled.PushPin
import androidx.compose.material.icons.outlined.PushPin
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.IconToggleButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.navigation.NavHostController
import app.hocket.R
import app.hocket.ui.nav.AccountButton
import app.hocket.core.ArtworkSizes
import app.hocket.core.Commands
import app.hocket.core.api.Command
import app.hocket.core.api.SavedQueue
import app.hocket.ui.LocalCoreClient
import app.hocket.ui.components.Artwork
import app.hocket.ui.components.ConfirmDialog
import app.hocket.ui.components.EmptyState
import app.hocket.ui.components.SectionHeader
import app.hocket.ui.components.formatAgo

/** Saved queues: pinned first, then recent; restore / pin / delete / save as playlist. */
@Composable
fun RecentQueuesList(modifier: Modifier = Modifier) {
    val client = LocalCoreClient.current
    val saved by client.savedQueues.collectAsStateWithLifecycle()
    val connection by client.connection.collectAsStateWithLifecycle()
    var deleting by remember { mutableStateOf<SavedQueue?>(null) }
    var naming by remember { mutableStateOf<SavedQueue?>(null) }
    if (saved.isEmpty()) {
        EmptyState(stringResource(R.string.empty_saved_queues_title), if (!connection.connected) stringResource(R.string.empty_recent_offline) else stringResource(R.string.empty_saved_queues_body), modifier)
        return
    }
    val pinned = saved.filter { it.pinned }
    val recent = saved.filter { !it.pinned }.sortedByDescending { it.lastInteractedAt }
    LazyColumn(modifier, contentPadding = PaddingValues(bottom = 120.dp)) {
        if (pinned.isNotEmpty()) { item { SectionHeader(stringResource(R.string.saved_pinned)) }; items(pinned, key = { it.id }) { SavedQueueRow(it, onDelete = { deleting = it }, onSaveAs = { naming = it }) } }
        if (recent.isNotEmpty()) { item { SectionHeader(stringResource(R.string.saved_recent)) }; items(recent, key = { it.id }) { SavedQueueRow(it, onDelete = { deleting = it }, onSaveAs = { naming = it }) } }
    }
    deleting?.let { sq -> ConfirmDialog(stringResource(R.string.playlist_delete_confirm, sq.label), stringResource(R.string.action_delete), onConfirm = { client.dispatch(Commands.deleteSavedQueue(sq.id)) }, onDismiss = { deleting = null }) }
    naming?.let { sq ->
        var name by remember { mutableStateOf(sq.label) }
        AlertDialog(onDismissRequest = { naming = null }, title = { Text(stringResource(R.string.queue_save_as_playlist)) },
            text = { OutlinedTextField(value = name, onValueChange = { name = it }, label = { Text(stringResource(R.string.playlist_name)) }, singleLine = true) },
            confirmButton = { TextButton(enabled = name.isNotBlank(), onClick = { client.dispatch(Commands.saveQueueAsPlaylist(sq.id, name.trim())); naming = null }) { Text(stringResource(R.string.action_save)) } },
            dismissButton = { TextButton(onClick = { naming = null }) { Text(stringResource(R.string.action_cancel)) } })
    }
}

@Composable
private fun SavedQueueRow(sq: SavedQueue, onDelete: () -> Unit, onSaveAs: () -> Unit) {
    val client = LocalCoreClient.current
    Row(Modifier.fillMaxWidth().clickable { client.dispatch(Commands.restoreSavedQueue(sq.id)) }.padding(horizontal = 16.dp, vertical = 8.dp), verticalAlignment = Alignment.CenterVertically) {
        Artwork(sq.coverArt, ArtworkSizes.THUMB, null, Modifier.size(48.dp), RoundedCornerShape(8.dp))
        Spacer(Modifier.width(14.dp))
        Column(Modifier.weight(1f)) {
            Text(sq.label, style = MaterialTheme.typography.bodyLarge, maxLines = 1, overflow = TextOverflow.Ellipsis)
            Text(stringResource(R.string.saved_tracks, sq.trackCount.toInt()) + stringResource(R.string.dot_separator) + stringResource(R.string.saved_played, formatAgo(sq.lastInteractedAt)), style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
        }
        IconToggleButton(checked = sq.pinned, onCheckedChange = { client.dispatch(Commands.pinSavedQueue(sq.id, it)) }) {
            Icon(if (sq.pinned) Icons.Filled.PushPin else Icons.Outlined.PushPin, stringResource(if (sq.pinned) R.string.saved_unpin else R.string.saved_pin), tint = if (sq.pinned) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.onSurfaceVariant)
        }
        IconButton(onClick = onSaveAs) { Icon(Icons.Filled.PlaylistAdd, stringResource(R.string.queue_save_as_playlist)) }
        IconButton(onClick = onDelete) { Icon(Icons.Filled.Delete, stringResource(R.string.action_delete)) }
    }
}

/** The undo history sheet content: newest first, each undoable directly; undo/redo at the top. */
@Composable
fun UndoHistoryPanel(modifier: Modifier = Modifier) {
    val client = LocalCoreClient.current
    val undo by client.undo.collectAsStateWithLifecycle()
    val self = remember { client.session.value?.let { "" } ?: "" }
    Column(modifier) {
        Row(Modifier.fillMaxWidth().padding(horizontal = 16.dp), horizontalArrangement = androidx.compose.foundation.layout.Arrangement.spacedBy(8.dp)) {
            TextButton(enabled = undo.canUndo, onClick = { client.dispatch(Command.Undo) }) { Text(undo.undoLabel?.let { stringResource(R.string.history_undo_label, it) } ?: stringResource(R.string.action_undo)) }
            TextButton(enabled = undo.canRedo, onClick = { client.dispatch(Command.Redo) }) { Text(undo.redoLabel?.let { stringResource(R.string.history_redo_label, it) } ?: stringResource(R.string.action_redo)) }
        }
        if (undo.history.isEmpty()) { EmptyState(stringResource(R.string.empty_history_title), stringResource(R.string.empty_history_body)); return }
        LazyColumn(contentPadding = PaddingValues(bottom = 120.dp)) {
            items(undo.history, key = { it.id }) { e ->
                Row(Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 6.dp), verticalAlignment = Alignment.CenterVertically) {
                    Icon(Icons.Filled.History, null, tint = MaterialTheme.colorScheme.onSurfaceVariant)
                    Spacer(Modifier.width(12.dp))
                    Column(Modifier.weight(1f)) {
                        Text(e.label, style = MaterialTheme.typography.bodyLarge)
                        Text(formatAgo(e.at) + (e.note?.let { stringResource(R.string.dot_separator) + it } ?: ""), style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
                    }
                    TextButton(onClick = { client.dispatch(Commands.undoEntry(e.id)) }) { Text(stringResource(R.string.history_undo_this)) }
                }
            }
        }
    }
}

/** Full-screen route for saved queues (from Home "see all"). */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun SavedQueuesScreen(nav: NavHostController) {
    Scaffold(topBar = { TopAppBar(title = { Text(stringResource(R.string.home_saved_queues)) }, actions = { AccountButton() }, navigationIcon = { IconButton(onClick = { nav.popBackStack() }) { Icon(Icons.AutoMirrored.Filled.ArrowBack, stringResource(R.string.action_back)) } }) }) { padding ->
        RecentQueuesList(Modifier.fillMaxSize().padding(padding))
    }
}
