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
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.FilledTonalButton
import androidx.compose.material3.Surface
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
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.navigation.NavHostController
import app.hocket.R
import app.hocket.ui.icons.HocketIcons
import app.hocket.ui.nav.AccountButton
import app.hocket.core.Commands
import app.hocket.core.api.SavedQueue
import app.hocket.ui.LocalCoreClient
import app.hocket.ui.components.EmptyState
import app.hocket.ui.components.SectionHeader
import app.hocket.ui.components.formatAgo

/**
 * Queues: the playing queue (rename, create playlist) above the pinned and recent saved queues,
 * each with rename, pin and delete. No artwork. Delete is immediate: the core makes it undoable
 * like any queue change. [onRestore] runs after a queue is restored (the player's queue switcher
 * closes itself).
 */
@Composable
fun RecentQueuesList(modifier: Modifier = Modifier, contentPadding: PaddingValues = PaddingValues(bottom = app.hocket.ui.nav.BottomContentInset), onRestore: () -> Unit = {}) {
    val client = LocalCoreClient.current
    val saved by client.savedQueues.collectAsStateWithLifecycle()
    val queue by client.queue.collectAsStateWithLifecycle()
    val connection by client.connection.collectAsStateWithLifecycle()
    // The queue being renamed: a saved queue's id, "" for the playing queue, null for none.
    var renaming by remember { mutableStateOf<String?>(null) }
    var naming by remember { mutableStateOf(false) }
    val pinned = saved.filter { it.pinned }.sortedByDescending { it.lastInteractedAt }
    val recent = saved.filter { !it.pinned }.sortedByDescending { it.lastInteractedAt }
    val playingLabel = queue.contextLabel?.takeIf { it.isNotBlank() }
    val playingCount = queue.history.size + (if (queue.current != null) 1 else 0) + queue.playingNext.size + queue.totalUpcoming.toInt()
    LazyColumn(modifier, contentPadding = contentPadding) {
        item {
            PlayingQueueCard(playingLabel, playingCount, onRename = { renaming = "" }, onCreatePlaylist = { naming = true })
        }
        if (pinned.isNotEmpty()) { item { SectionHeader(stringResource(R.string.saved_pinned)) }; items(pinned, key = { it.id }) { SavedQueueRow(it, onRename = { renaming = it.id }, onRestore = onRestore) } }
        if (recent.isNotEmpty()) { item { SectionHeader(stringResource(R.string.saved_recent)) }; items(recent, key = { it.id }) { SavedQueueRow(it, onRename = { renaming = it.id }, onRestore = onRestore) } }
        if (saved.isEmpty()) item { EmptyState(stringResource(R.string.empty_saved_queues_title), if (!connection.connected) stringResource(R.string.empty_recent_offline) else stringResource(R.string.empty_saved_queues_body)) }
    }
    renaming?.let { id ->
        val initial = if (id.isEmpty()) playingLabel.orEmpty() else saved.firstOrNull { it.id == id }?.label.orEmpty()
        NameDialog(stringResource(R.string.queue_rename), stringResource(R.string.queue_name), initial, stringResource(R.string.action_save),
            onConfirm = { client.dispatch(Commands.renameQueue(id.ifEmpty { null }, it)) }, onDismiss = { renaming = null })
    }
    if (naming) {
        NameDialog(stringResource(R.string.queue_create_playlist), stringResource(R.string.playlist_name), playingLabel.orEmpty(), stringResource(R.string.action_create),
            onConfirm = { client.dispatch(Commands.saveQueueAsPlaylist(null, it)) }, onDismiss = { naming = false })
    }
}

/** The playing queue at the top of Queues: its name (renamable once it has one) and Create playlist. */
@Composable
private fun PlayingQueueCard(label: String?, count: Int, onRename: () -> Unit, onCreatePlaylist: () -> Unit) {
    Surface(color = MaterialTheme.colorScheme.surfaceContainerHigh, shape = RoundedCornerShape(24.dp), modifier = Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 8.dp).testTag("queues.playing")) {
        Column(Modifier.padding(horizontal = 20.dp, vertical = 16.dp)) {
            Text(stringResource(R.string.queue_playing_now), style = MaterialTheme.typography.labelLarge, color = MaterialTheme.colorScheme.primary)
            Row(verticalAlignment = Alignment.CenterVertically) {
                Text(label ?: stringResource(R.string.player_mode_queue), style = MaterialTheme.typography.titleLarge, maxLines = 1, overflow = TextOverflow.Ellipsis, modifier = Modifier.weight(1f, fill = false))
                if (label != null) IconButton(onClick = onRename, modifier = Modifier.testTag("queues.playing.rename")) { Icon(HocketIcons.Filled.Edit, stringResource(R.string.queue_rename)) }
            }
            Text(stringResource(R.string.saved_tracks, count), style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
            Spacer(Modifier.height(12.dp))
            FilledTonalButton(onClick = onCreatePlaylist, enabled = count > 0, modifier = Modifier.testTag("queues.playing.createPlaylist")) {
                Icon(HocketIcons.Filled.PlaylistAdd, null)
                Spacer(Modifier.width(8.dp))
                Text(stringResource(R.string.queue_create_playlist))
            }
        }
    }
}

/** A one-field dialog for naming something; a blank name can't be confirmed. */
@Composable
private fun NameDialog(title: String, label: String, initial: String, confirm: String, onConfirm: (String) -> Unit, onDismiss: () -> Unit) {
    var name by remember { mutableStateOf(initial) }
    AlertDialog(onDismissRequest = onDismiss, title = { Text(title) },
        text = { OutlinedTextField(value = name, onValueChange = { name = it }, label = { Text(label) }, singleLine = true, modifier = Modifier.testTag("queues.nameField")) },
        confirmButton = { TextButton(enabled = name.isNotBlank(), onClick = { onConfirm(name.trim()); onDismiss() }) { Text(confirm) } },
        dismissButton = { TextButton(onClick = onDismiss) { Text(stringResource(R.string.action_cancel)) } })
}

@Composable
private fun SavedQueueRow(sq: SavedQueue, onRename: () -> Unit, onRestore: () -> Unit) {
    val client = LocalCoreClient.current
    Row(Modifier.fillMaxWidth().clickable { client.dispatch(Commands.restoreSavedQueue(sq.id)); onRestore() }.testTag("savedQueue.${sq.id}").padding(start = 20.dp, end = 8.dp, top = 6.dp, bottom = 6.dp), verticalAlignment = Alignment.CenterVertically) {
        Column(Modifier.weight(1f)) {
            Text(sq.label, style = MaterialTheme.typography.bodyLarge, maxLines = 1, overflow = TextOverflow.Ellipsis)
            Text(stringResource(R.string.saved_tracks, sq.trackCount.toInt()) + stringResource(R.string.dot_separator) + stringResource(R.string.saved_played, formatAgo(sq.lastInteractedAt)), style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
        }
        IconButton(onClick = onRename, modifier = Modifier.testTag("savedQueue.${sq.id}.rename")) { Icon(HocketIcons.Filled.Edit, stringResource(R.string.queue_rename)) }
        IconToggleButton(checked = sq.pinned, onCheckedChange = { client.dispatch(Commands.pinSavedQueue(sq.id, it)) }) {
            Icon(if (sq.pinned) HocketIcons.Filled.PushPin else HocketIcons.Outlined.PushPin, stringResource(if (sq.pinned) R.string.saved_unpin else R.string.saved_pin), tint = if (sq.pinned) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.onSurfaceVariant)
        }
        IconButton(onClick = { client.dispatch(Commands.deleteSavedQueue(sq.id)) }, modifier = Modifier.testTag("savedQueue.${sq.id}.delete")) { Icon(HocketIcons.Filled.Delete, stringResource(R.string.action_delete)) }
    }
}

/**
 * Full-screen route for Queues (Home "see all", Library "Recent queues"): the playing queue, then the
 * pinned and recent ones. The undo history is out for now (issue #127).
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun SavedQueuesScreen(nav: NavHostController) {
    Scaffold(topBar = {
        TopAppBar(
            title = { Text(stringResource(R.string.queues_title), style = app.hocket.ui.brand.brandTitleStyle(), maxLines = 1, modifier = Modifier.semantics { heading() }) },
            actions = {
                AccountButton()
            },
            navigationIcon = { IconButton(onClick = { nav.popBackStack() }) { Icon(HocketIcons.AutoMirrored.Filled.ArrowBack, stringResource(R.string.action_back)) } },
        )
    }) { padding ->
        RecentQueuesList(Modifier.fillMaxSize().padding(padding))
    }
}
