package app.hocket.ui.queue

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.FilledTonalIconToggleButton
import androidx.compose.material3.IconButtonDefaults
import androidx.compose.material3.ListItemDefaults
import androidx.compose.material3.SegmentedListItem
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.font.FontWeight
import app.hocket.ui.components.GroupLabel
import app.hocket.ui.components.Segment
import app.hocket.ui.components.SegmentOuter
import app.hocket.ui.components.formatClock
import app.hocket.ui.components.segmentedListColors
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
 * Queues: the playing queue on a filled primary card (its name, size and place, rename and create
 * playlist) above the pinned and recent saved queues as segmented groups, each row with pin, rename
 * and delete. No artwork. Delete is immediate: the core makes it undoable like any queue change.
 * [onRestore] runs after a queue is restored (the player's queue switcher closes itself).
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
    val playingIndex = if (queue.current != null) queue.history.size + 1 else 0
    LazyColumn(modifier, contentPadding = PaddingValues(start = 16.dp, end = 16.dp, top = contentPadding.calculateTopPadding(), bottom = contentPadding.calculateBottomPadding())) {
        item(key = "playing.label") { GroupLabel(stringResource(R.string.queue_playing_now)) }
        item(key = "playing") {
            PlayingQueueCard(playingLabel, playingCount, playingIndex, onRename = { renaming = "" }, onCreatePlaylist = { naming = true })
        }
        if (pinned.isNotEmpty()) {
            item(key = "pinned.label") { GroupLabel(stringResource(R.string.saved_pinned), Modifier.padding(top = 12.dp)) }
            itemsIndexed(pinned, key = { _, q -> q.id }) { i, q -> SavedQueueRow(q, i, pinned.size, onRename = { renaming = q.id }, onRestore = onRestore) }
        }
        item(key = "recent.label") { GroupLabel(stringResource(R.string.saved_recent), Modifier.padding(top = 12.dp)) }
        if (recent.isEmpty()) item(key = "recent.empty") {
            Segment(0, 1) {
                Text(
                    if (!connection.connected && saved.isEmpty()) stringResource(R.string.empty_recent_offline) else stringResource(R.string.empty_saved_queues_body),
                    style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant, modifier = Modifier.padding(16.dp),
                )
            }
        }
        itemsIndexed(recent, key = { _, q -> q.id }) { i, q -> SavedQueueRow(q, i, recent.size, onRename = { renaming = q.id }, onRestore = onRestore) }
    }
    renaming?.let { id ->
        val initial = if (id.isEmpty()) playingLabel.orEmpty() else saved.firstOrNull { it.id == id }?.label.orEmpty()
        NameDialog(stringResource(R.string.queue_rename_title), stringResource(R.string.queue_name), initial, stringResource(R.string.action_save),
            onConfirm = { client.dispatch(Commands.renameQueue(id.ifEmpty { null }, it)) }, onDismiss = { renaming = null })
    }
    if (naming) {
        NameDialog(stringResource(R.string.queue_create_playlist), stringResource(R.string.playlist_name), playingLabel.orEmpty(), stringResource(R.string.action_create),
            onConfirm = { client.dispatch(Commands.saveQueueAsPlaylist(null, it)) }, onDismiss = { naming = false })
    }
}

/**
 * The playing queue at the top of Queues, on the primary colour: its name, how long it is, where
 * in it you are, and its two actions (Create playlist, Rename) as tonal buttons on the card.
 */
@Composable
private fun PlayingQueueCard(label: String?, count: Int, index: Int, onRename: () -> Unit, onCreatePlaylist: () -> Unit) {
    val client = LocalCoreClient.current
    val position by client.position.collectAsStateWithLifecycle()
    val onCard = MaterialTheme.colorScheme.onPrimary
    val onCardSoft = onCard.copy(alpha = 0.82f)
    val tonal = ButtonDefaults.filledTonalButtonColors(containerColor = MaterialTheme.colorScheme.primaryContainer, contentColor = MaterialTheme.colorScheme.onPrimaryContainer)
    Surface(color = MaterialTheme.colorScheme.primary, contentColor = onCard, shape = RoundedCornerShape(SegmentOuter + 4.dp), modifier = Modifier.fillMaxWidth().testTag("queues.playing")) {
        Column(Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(14.dp)) {
            Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(14.dp)) {
                Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(2.dp)) {
                    Text(label ?: stringResource(R.string.player_mode_queue), style = MaterialTheme.typography.titleLarge, fontWeight = FontWeight.SemiBold, maxLines = 1, overflow = TextOverflow.Ellipsis)
                    Text(if (count == 0) stringResource(R.string.queue_empty_meta) else stringResource(R.string.saved_tracks, count), style = MaterialTheme.typography.bodyMedium, color = onCardSoft, maxLines = 1)
                    if (index > 0) Text(stringResource(R.string.queue_where, index, count, formatClock(position)), style = MaterialTheme.typography.bodySmall, color = onCardSoft, maxLines = 1)
                    else if (count == 0) Text(stringResource(R.string.queue_empty_hint), style = MaterialTheme.typography.bodySmall, color = onCardSoft, maxLines = 1)
                }
                if (index > 0) Icon(HocketIcons.Filled.Equalizer, null, tint = onCard)
            }
            FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
                FilledTonalButton(onClick = onCreatePlaylist, enabled = count > 0, colors = tonal, shapes = ButtonDefaults.shapes(), modifier = Modifier.testTag("queues.playing.createPlaylist")) {
                    Icon(HocketIcons.AutoMirrored.Filled.PlaylistAdd, null, Modifier.size(ButtonDefaults.IconSize))
                    Spacer(Modifier.width(ButtonDefaults.IconSpacing))
                    Text(stringResource(R.string.queue_create_playlist))
                }
                if (label != null) FilledTonalButton(onClick = onRename, colors = tonal, shapes = ButtonDefaults.shapes(), modifier = Modifier.testTag("queues.playing.rename")) {
                    Icon(HocketIcons.Outlined.Edit, null, Modifier.size(ButtonDefaults.IconSize))
                    Spacer(Modifier.width(ButtonDefaults.IconSpacing))
                    Text(stringResource(R.string.queue_rename))
                }
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
private fun SavedQueueRow(sq: SavedQueue, index: Int, count: Int, onRename: () -> Unit, onRestore: () -> Unit) {
    val client = LocalCoreClient.current
    Box(Modifier.padding(top = if (index > 0) ListItemDefaults.SegmentedGap else 0.dp)) {
        SegmentedListItem(
            onClick = { client.dispatch(Commands.restoreSavedQueue(sq.id)); onRestore() },
            shapes = ListItemDefaults.segmentedShapes(index, count),
            colors = segmentedListColors(),
            modifier = Modifier.testTag("savedQueue.${sq.id}"),
            supportingContent = {
                Text(stringResource(R.string.saved_tracks, sq.trackCount.toInt()) + stringResource(R.string.dot_separator) + stringResource(R.string.saved_played, formatAgo(sq.lastInteractedAt)), maxLines = 1, overflow = TextOverflow.Ellipsis)
            },
            trailingContent = {
                Row {
                    // Pinned reads as a filled tonal toggle; unpinned as a plain outlined pin.
                    FilledTonalIconToggleButton(checked = sq.pinned, onCheckedChange = { client.dispatch(Commands.pinSavedQueue(sq.id, it)) },
                        colors = IconButtonDefaults.filledTonalIconToggleButtonColors(containerColor = Color.Transparent, checkedContainerColor = MaterialTheme.colorScheme.secondaryContainer, checkedContentColor = MaterialTheme.colorScheme.primary),
                        shapes = IconButtonDefaults.toggleableShapes(), modifier = Modifier.testTag("savedQueue.${sq.id}.pin")) {
                        Icon(if (sq.pinned) HocketIcons.Filled.PushPin else HocketIcons.Outlined.PushPin, stringResource(if (sq.pinned) R.string.saved_unpin else R.string.saved_pin))
                    }
                    IconButton(onClick = onRename, shapes = IconButtonDefaults.shapes(), modifier = Modifier.testTag("savedQueue.${sq.id}.rename")) { Icon(HocketIcons.Outlined.Edit, stringResource(R.string.queue_rename_named, sq.label)) }
                    IconButton(onClick = { client.dispatch(Commands.deleteSavedQueue(sq.id)) }, shapes = IconButtonDefaults.shapes(), modifier = Modifier.testTag("savedQueue.${sq.id}.delete")) { Icon(HocketIcons.Outlined.Delete, stringResource(R.string.queue_delete_named, sq.label)) }
                }
            },
        ) { Text(sq.label, maxLines = 1, overflow = TextOverflow.Ellipsis) }
    }
}
