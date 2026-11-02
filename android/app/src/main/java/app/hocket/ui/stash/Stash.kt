package app.hocket.ui.stash

import androidx.compose.animation.AnimatedContent
import androidx.compose.animation.core.Spring
import androidx.compose.animation.core.animateDpAsState
import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.animation.core.spring
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.slideInVertically
import androidx.compose.animation.slideOutVertically
import androidx.compose.animation.togetherWith
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyRow
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Button
import androidx.compose.material3.FilledTonalIconButton
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.key
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.shadow
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.hapticfeedback.HapticFeedbackType
import androidx.compose.ui.platform.LocalHapticFeedback
import androidx.compose.ui.platform.LocalResources
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.CustomAccessibilityAction
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.customActions
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.selected
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import app.hocket.R
import app.hocket.core.ActionIds
import app.hocket.core.ArtworkSizes
import app.hocket.core.Commands
import app.hocket.core.api.Command
import app.hocket.core.api.CommandStashMoveInner
import app.hocket.core.api.CommandStashRemoveInner
import app.hocket.core.api.StashItem
import app.hocket.ui.LocalCoreClient
import app.hocket.ui.a11y.LocalReducedMotion
import app.hocket.ui.components.Artwork
import app.hocket.ui.components.LocalAppSnackbar
import app.hocket.ui.components.NewPlaylistDialog
import app.hocket.ui.components.PlaylistPicker
import app.hocket.ui.icons.HocketIcons
import sh.calvin.reorderable.ReorderableItem
import sh.calvin.reorderable.rememberReorderableLazyListState

/**
 * The song stash (#150) on the phone: a chip with a fan of the first two stashed covers and the
 * count. With the floating navigation bar it sits beside the pill, as tall as it; otherwise it
 * floats above the mini player at the end edge ([StashChipDefaults.StandaloneHeight]). Tapping it
 * opens [StashSheet]. A new song landing in the stash ticks (haptic) and pops the count.
 */
@Composable
fun StashChip(
    items: List<StashItem>,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
    height: Dp = StashChipDefaults.StandaloneHeight,
    container: Color = MaterialTheme.colorScheme.secondaryContainer,
    content: Color = MaterialTheme.colorScheme.onSecondaryContainer,
    elevation: Dp = 6.dp,
) {
    val reduced = LocalReducedMotion.current
    val haptics = LocalHapticFeedback.current
    val count = items.size
    var last by remember { mutableStateOf(count) }
    LaunchedEffect(count) {
        if (count > last && last > 0) haptics.performHapticFeedback(HapticFeedbackType.Confirm)
        last = count
    }
    val description = pluralStringResource(R.plurals.stash_chip_description, count, count)
    val art = (height - 16.dp).coerceAtMost(32.dp)
    Row(
        modifier
            .height(height)
            .shadow(elevation, CircleShape, ambientColor = Color.Black, spotColor = Color.Black)
            .clip(CircleShape)
            .background(container)
            .clickable(role = Role.Button, onClick = onClick)
            .semantics { contentDescription = description }
            .testTag("stash.chip")
            .padding(start = (height - art) / 2, end = 14.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Fan(items, art, reduced)
        Spacer(Modifier.width(8.dp))
        AnimatedContent(
            targetState = count,
            transitionSpec = {
                if (reduced) fadeIn() togetherWith fadeOut()
                else {
                    val up = targetState > initialState
                    (slideInVertically(spring(dampingRatio = 0.6f, stiffness = 500f)) { if (up) it else -it } + fadeIn()) togetherWith
                        (slideOutVertically(spring(stiffness = Spring.StiffnessMedium)) { if (up) -it else it } + fadeOut())
                }
            },
            label = "stashCount",
        ) { n ->
            Text(n.toString(), color = content, style = MaterialTheme.typography.titleMedium, fontWeight = FontWeight.SemiBold, modifier = Modifier.testTag("stash.count"))
        }
    }
}

/** The first two covers fanned out, the front one tilted right over the back one tilted left. */
@Composable
private fun Fan(items: List<StashItem>, size: Dp, reduced: Boolean) {
    val two = items.size > 1
    val spec = if (reduced) spring<Float>(stiffness = Spring.StiffnessHigh) else spring(dampingRatio = 0.55f, stiffness = 400f)
    val spread by animateFloatAsState(if (two) 1f else 0f, spec, label = "stashFan")
    val width by animateDpAsState(if (two) size + 10.dp else size, if (reduced) spring(stiffness = Spring.StiffnessHigh) else spring(dampingRatio = 0.8f, stiffness = 500f), label = "stashFanWidth")
    Box(Modifier.width(width).height(size)) {
        items.getOrNull(1)?.let { back ->
            key(back.entryId) {
                Card(back, size, Modifier.graphicsLayer { rotationZ = -10f * spread; alpha = spread.coerceIn(0f, 1f) })
            }
        }
        items.firstOrNull()?.let { front ->
            key(front.entryId) {
                // A newly stashed front card drops in.
                var shown by remember { mutableStateOf(reduced) }
                LaunchedEffect(Unit) { shown = true }
                val drop by animateFloatAsState(if (shown) 1f else 0f, spec, label = "stashCardDrop")
                Card(
                    front, size,
                    Modifier.graphicsLayer {
                        translationX = 10.dp.toPx() * spread
                        rotationZ = 8f * spread - 20f * (1f - drop)
                        translationY = -size.toPx() * 0.4f * (1f - drop)
                        scaleX = 0.6f + 0.4f * drop
                        scaleY = 0.6f + 0.4f * drop
                        alpha = drop
                    },
                )
            }
        }
    }
}

@Composable
private fun Card(item: StashItem, size: Dp, modifier: Modifier) {
    Surface(modifier.size(size), shape = RoundedCornerShape(8.dp), shadowElevation = 2.dp, color = Color.Transparent) {
        Artwork(item.track.coverArt, ArtworkSizes.THUMB, null, Modifier.size(size), RoundedCornerShape(8.dp))
    }
}

object StashChipDefaults {
    /** The chip's height when it floats on its own (above the mini player). */
    val StandaloneHeight: Dp = 44.dp
    /** The room the standalone chip takes above the mini player: its height and the gap under it. */
    val StandaloneRoom: Dp = StandaloneHeight + 8.dp
    /** The width the chip takes beside the floating bar (for the bar's label budget). */
    val BesideBarWidth: Dp = 96.dp
}

/**
 * The stash as a sheet: its songs in order (drag the handle to reorder, × to remove, tap to pick
 * some), and what to do with them: play, shuffle, play next, add to the queue, add to a playlist,
 * a new playlist, download. With songs picked the actions apply to those, otherwise to the whole
 * stash; either way the stash is kept. Clear empties it (the core's toast has an Undo).
 */
@Composable
fun StashSheet(onDismiss: () -> Unit) {
    val client = LocalCoreClient.current
    val items by client.stash.collectAsStateWithLifecycle()
    val haptics = LocalHapticFeedback.current
    val resources = LocalResources.current
    val snackbar = LocalAppSnackbar.current
    var picked by remember { mutableStateOf(setOf<String>()) }
    var playlistPicker by remember { mutableStateOf(false) }
    var naming by remember { mutableStateOf(false) }
    // Emptied (here or on another device): nothing left to show.
    LaunchedEffect(items.isEmpty()) { if (items.isEmpty()) onDismiss() }
    val live = items.map { it.entryId }.toSet()
    val selection = items.filter { it.entryId in picked && it.entryId in live }
    val acting = selection.ifEmpty { items }
    val ids = acting.map { it.track.id }
    val serverId = items.firstOrNull()?.track?.serverId ?: ""
    val title = stringResource(R.string.stash_title)
    fun play(shuffle: Boolean, from: Int = 0, list: List<String> = ids) = client.dispatch(Commands.playTracks(serverId, list, from, title, shuffle))
    fun queue(next: Boolean) {
        client.dispatch(Commands.runAction(if (next) ActionIds.PLAY_NEXT else ActionIds.PLAY_LATER, Commands.tracks(ids)))
        val text = resources.getQuantityString(if (next) R.plurals.stash_queued_next else R.plurals.stash_queued, ids.size, ids.size)
        snackbar?.let { s -> s.launch { s.host.showSnackbar(text) } }
    }
    ModalBottomSheet(onDismissRequest = onDismiss, sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = true), modifier = Modifier.testTag("stash.sheet")) {
        Column(Modifier.navigationBarsPadding()) {
            Row(Modifier.fillMaxWidth().padding(start = 24.dp, end = 12.dp), verticalAlignment = Alignment.CenterVertically) {
                Column(Modifier.weight(1f)) {
                    Text(title, style = MaterialTheme.typography.headlineSmall, fontWeight = FontWeight.SemiBold, modifier = Modifier.semantics { heading() })
                    Text(
                        if (selection.isNotEmpty()) pluralStringResource(R.plurals.stash_selected, selection.size, selection.size)
                        else pluralStringResource(R.plurals.stash_count, items.size, items.size),
                        style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
                if (selection.isNotEmpty()) TextButton(onClick = { picked = emptySet() }) { Text(stringResource(R.string.stash_deselect)) }
                TextButton(onClick = { client.dispatch(Command.StashClear) }, modifier = Modifier.testTag("stash.clear")) {
                    Icon(HocketIcons.Filled.ClearAll, null, Modifier.size(18.dp))
                    Spacer(Modifier.width(6.dp))
                    Text(stringResource(R.string.stash_clear))
                }
            }
            LazyRow(
                contentPadding = PaddingValues(horizontal = 20.dp, vertical = 12.dp),
                horizontalArrangement = Arrangement.spacedBy(8.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                item {
                    Button(onClick = { play(false) }, modifier = Modifier.testTag("stash.play")) {
                        Icon(HocketIcons.Filled.PlayArrow, null, Modifier.size(20.dp))
                        Spacer(Modifier.width(6.dp))
                        Text(stringResource(R.string.stash_play))
                    }
                }
                item { Action(HocketIcons.Filled.Shuffle, R.string.stash_shuffle, "stash.shuffle") { play(true) } }
                item { Action(HocketIcons.AutoMirrored.Filled.PlaylistPlay, R.string.stash_play_next, "stash.playNext") { queue(true) } }
                item { Action(HocketIcons.AutoMirrored.Filled.QueueMusic, R.string.stash_add_to_queue, "stash.playLater") { queue(false) } }
                item { Action(HocketIcons.AutoMirrored.Filled.PlaylistAdd, R.string.stash_add_to_playlist, "stash.addToPlaylist") { playlistPicker = true } }
                item { Action(HocketIcons.Filled.Add, R.string.stash_new_playlist, "stash.newPlaylist") { naming = true } }
                item { Action(HocketIcons.Filled.Download, R.string.stash_download, "stash.download") { client.dispatch(Commands.runAction(ActionIds.DOWNLOAD, Commands.tracks(ids))) } }
            }
            StashList(
                items = items,
                picked = picked,
                onToggle = { id -> picked = if (id in picked) picked - id else picked + id; haptics.performHapticFeedback(HapticFeedbackType.SegmentTick) },
                onPlayFrom = { i -> play(false, i, items.map { it.track.id }) },
                onRemove = { id -> client.dispatch(Command.StashRemove(CommandStashRemoveInner(listOf(id)))) },
                onMove = { id, to -> client.dispatch(Command.StashMove(CommandStashMoveInner(id, to.toUInt()))) },
            )
        }
    }
    if (playlistPicker) PlaylistPicker(Commands.tracks(ids), onDismiss = { playlistPicker = false })
    if (naming) NewPlaylistDialog(onCreate = { name -> naming = false; client.dispatch(Commands.createPlaylist(serverId, name, ids)) }, onDismiss = { naming = false })
}

@Composable
private fun Action(icon: androidx.compose.ui.graphics.vector.ImageVector, label: Int, tag: String, onClick: () -> Unit) {
    val text = stringResource(label)
    FilledTonalIconButton(onClick = onClick, modifier = Modifier.semantics { contentDescription = text }.testTag(tag)) {
        Icon(icon, null)
    }
}

@Composable
private fun StashList(
    items: List<StashItem>,
    picked: Set<String>,
    onToggle: (String) -> Unit,
    onPlayFrom: (Int) -> Unit,
    onRemove: (String) -> Unit,
    onMove: (String, Int) -> Unit,
) {
    val haptics = LocalHapticFeedback.current
    // The list as shown: moves land here at once and reach the core when the drag ends.
    var shown by remember { mutableStateOf(items) }
    var dragging by remember { mutableStateOf<String?>(null) }
    LaunchedEffect(items) { if (dragging == null) shown = items }
    val listState = rememberLazyListState()
    val reorder = rememberReorderableLazyListState(listState) { from, to ->
        shown = shown.toMutableList().apply { add(to.index, removeAt(from.index)) }
        haptics.performHapticFeedback(HapticFeedbackType.SegmentFrequentTick)
    }
    val moveUp = stringResource(R.string.stash_move_up)
    val moveDown = stringResource(R.string.stash_move_down)
    LazyColumn(
        state = listState,
        modifier = Modifier.fillMaxWidth().heightIn(min = 120.dp).testTag("stash.list"),
        contentPadding = PaddingValues(start = 12.dp, end = 4.dp, bottom = 16.dp),
    ) {
        items(shown, key = { it.entryId }) { item ->
            val index = shown.indexOf(item)
            ReorderableItem(reorder, key = item.entryId) { isDragging ->
                val on = item.entryId in picked
                val lift by animateDpAsState(if (isDragging) 8.dp else 0.dp, label = "stashRowLift")
                Surface(
                    shadowElevation = lift,
                    shape = RoundedCornerShape(16.dp),
                    color = if (on) MaterialTheme.colorScheme.secondaryContainer else if (isDragging) MaterialTheme.colorScheme.surfaceContainerHigh else Color.Transparent,
                ) {
                    Row(
                        Modifier
                            .fillMaxWidth()
                            .clickable { onToggle(item.entryId) }
                            .semantics(mergeDescendants = true) {
                                selected = on
                                customActions = buildList {
                                    if (index > 0) add(CustomAccessibilityAction(moveUp) { onMove(item.entryId, index - 1); true })
                                    if (index < shown.lastIndex) add(CustomAccessibilityAction(moveDown) { onMove(item.entryId, index + 1); true })
                                }
                            }
                            .testTag("stash.row")
                            .padding(start = 8.dp, top = 6.dp, bottom = 6.dp),
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        Artwork(item.track.coverArt, ArtworkSizes.THUMB, null, Modifier.size(48.dp).clickable { onPlayFrom(index) }, RoundedCornerShape(10.dp))
                        Spacer(Modifier.width(14.dp))
                        Column(Modifier.weight(1f)) {
                            Text(item.track.title, style = MaterialTheme.typography.bodyLarge, maxLines = 1, overflow = TextOverflow.Ellipsis)
                            Text(item.track.artist ?: "", style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant, maxLines = 1, overflow = TextOverflow.Ellipsis)
                        }
                        IconButton(onClick = { onRemove(item.entryId) }, modifier = Modifier.testTag("stash.remove")) {
                            Icon(HocketIcons.Filled.Close, stringResource(R.string.stash_remove, item.track.title))
                        }
                        Box(
                            Modifier
                                .size(48.dp)
                                .draggableHandle(
                                    onDragStarted = { dragging = item.entryId; haptics.performHapticFeedback(HapticFeedbackType.GestureThresholdActivate) },
                                    onDragStopped = {
                                        val id = dragging
                                        dragging = null
                                        val to = shown.indexOfFirst { it.entryId == id }
                                        if (id != null && to >= 0 && items.indexOfFirst { it.entryId == id } != to) onMove(id, to)
                                        haptics.performHapticFeedback(HapticFeedbackType.GestureEnd)
                                    },
                                ),
                            contentAlignment = Alignment.Center,
                        ) {
                            Icon(HocketIcons.Filled.DragHandle, null, tint = MaterialTheme.colorScheme.onSurfaceVariant)
                        }
                    }
                }
            }
        }
    }
}
