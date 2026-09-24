package app.hocket.ui.queue

import android.os.SystemClock
import androidx.compose.foundation.background
import androidx.compose.foundation.interaction.DragInteraction
import androidx.compose.ui.draw.drawBehind
import androidx.compose.runtime.mutableLongStateOf
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
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
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Delete
import androidx.compose.material.icons.filled.DragHandle
import androidx.compose.material.icons.filled.MoreVert
import androidx.compose.material3.SecondaryScrollableTabRow
import androidx.compose.material3.Tab
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.SwipeToDismissBox
import androidx.compose.material3.SwipeToDismissBoxValue
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.rememberSwipeToDismissBoxState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.hapticfeedback.HapticFeedbackType
import androidx.compose.ui.platform.LocalHapticFeedback
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.CustomAccessibilityAction
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import app.hocket.R
import app.hocket.core.Commands
import app.hocket.core.api.Command
import app.hocket.core.api.QueueEntry
import app.hocket.core.api.QueueSource
import app.hocket.core.client.SelectionKind
import app.hocket.ui.LocalCoreClient
import app.hocket.ui.components.ActionSheet
import app.hocket.ui.components.EmptyState
import app.hocket.ui.components.SelectionToolbar
import app.hocket.ui.components.TrackRow
import sh.calvin.reorderable.ReorderableItem
import sh.calvin.reorderable.rememberReorderableLazyListState

/** How long after the user last scrolled the queue it stops following the current track. */
private const val USER_SCROLL_GRACE_MS = 8_000L

/** Queue / Recent / History share one panel (design: saved queues, global undo on Android). */
@Composable
fun QueuePanel(modifier: Modifier = Modifier) {
    var tab by rememberSaveable { mutableIntStateOf(0) }
    Column(modifier) {
        // Scrollable tabs: at large font sizes the labels keep their size and the row scrolls,
        // instead of a button group pushing items into an (empty) overflow.
        val labels = listOf(stringResource(R.string.player_tab_queue), stringResource(R.string.player_tab_recent), stringResource(R.string.player_tab_history))
        SecondaryScrollableTabRow(selectedTabIndex = tab, edgePadding = 16.dp, containerColor = androidx.compose.ui.graphics.Color.Transparent) {
            labels.forEachIndexed { i, label ->
                Tab(selected = tab == i, onClick = { tab = i }, text = { Text(label, maxLines = 1) }, modifier = Modifier.testTag("queue.tab.$i"))
            }
        }
        when (tab) {
            0 -> QueueTimeline(Modifier.fillMaxSize())
            1 -> RecentQueuesList(Modifier.fillMaxSize())
            else -> UndoHistoryPanel(Modifier.fillMaxSize())
        }
    }
}

private sealed interface Row {
    data class Header(val id: String, val text: String) : Row
    data class Item(val entry: QueueEntry, val section: Section) : Row
    enum class Section { History, Current, Next, Upcoming, Autoplay }
}

/**
 * The one scrollable timeline: history above (dimmed), the current item, then "Playing next"
 * (insertions) and "Continuing from <context>" (the permuted context), then autoplay with its "why".
 * Drag handles reorder (haptics), swipe removes (undo toast from the core), tap jumps.
 */
@Composable
fun QueueTimeline(modifier: Modifier = Modifier) {
    val client = LocalCoreClient.current
    val queue by client.queue.collectAsStateWithLifecycle()
    val selection by client.selection.collectAsStateWithLifecycle()
    val kind by client.selectionKind.collectAsStateWithLifecycle()
    val selecting = selection.active && kind == SelectionKind.QueueItems
    val haptics = LocalHapticFeedback.current
    var sheetFor by remember { mutableStateOf<QueueEntry?>(null) }
    val historyLabel = stringResource(R.string.queue_history)
    val nowLabel = stringResource(R.string.queue_now)
    val nextLabel = stringResource(R.string.queue_playing_next)
    val continuingLabel = queue.contextLabel?.let { stringResource(R.string.queue_continuing, it) } ?: stringResource(R.string.queue_up_next)
    val autoplayLabel = stringResource(R.string.queue_autoplay_section)
    val removeLabel = stringResource(R.string.action_remove_from_queue)
    val moveUpLabel = stringResource(R.string.a11y_move_up)
    val moveDownLabel = stringResource(R.string.a11y_move_down)
    val rows: List<Row> = remember(queue, historyLabel, nowLabel, nextLabel, continuingLabel, autoplayLabel) {
        buildList {
            if (queue.history.isNotEmpty()) { add(Row.Header("h", historyLabel)); queue.history.forEach { add(Row.Item(it, Row.Section.History)) } }
            queue.current?.let { add(Row.Header("c", nowLabel)); add(Row.Item(it, Row.Section.Current)) }
            if (queue.playingNext.isNotEmpty()) { add(Row.Header("n", nextLabel)); queue.playingNext.forEach { add(Row.Item(it, Row.Section.Next)) } }
            val (auto, ctx) = queue.upcoming.partition { it.item.source is QueueSource.Autoplay }
            if (ctx.isNotEmpty()) { add(Row.Header("u", continuingLabel)); ctx.forEach { add(Row.Item(it, Row.Section.Upcoming)) } }
            if (auto.isNotEmpty()) { add(Row.Header("a", autoplayLabel)); auto.forEach { add(Row.Item(it, Row.Section.Autoplay)) } }
        }
    }
    if (queue.current == null && rows.isEmpty()) {
        EmptyState(stringResource(R.string.empty_queue_title), stringResource(R.string.empty_queue_body), modifier)
        return
    }
    val listState = rememberLazyListState()
    // Combined index into playing-next + upcoming, which is what MoveQueueItem takes.
    val movable = remember(queue) { queue.playingNext.map { it.item.key } + queue.upcoming.map { it.item.key } }
    val reorderable = rememberReorderableLazyListState(listState) { from, to ->
        val fromKey = from.key as? String ?: return@rememberReorderableLazyListState
        val toKey = to.key as? String ?: return@rememberReorderableLazyListState
        val toIndex = movable.indexOf(toKey)
        if (fromKey in movable && toIndex >= 0) {
            client.dispatch(Commands.moveQueueItem(fromKey, toIndex))
            haptics.performHapticFeedback(HapticFeedbackType.SegmentFrequentTick)
        }
    }
    // Follow the current track, but never fight the user: no auto-scroll while a finger is on the
    // list or for a while after they last scrolled it (Metrolist/Navic leave a browsed queue alone).
    var lastUserScroll by remember { mutableLongStateOf(0L) }
    var userDragging by remember { mutableStateOf(false) }
    LaunchedEffect(listState) {
        listState.interactionSource.interactions.collect { i ->
            when (i) {
                is DragInteraction.Start -> { userDragging = true; lastUserScroll = SystemClock.uptimeMillis() }
                is DragInteraction.Stop, is DragInteraction.Cancel -> { userDragging = false; lastUserScroll = SystemClock.uptimeMillis() }
            }
        }
    }
    var firstScroll by remember { mutableStateOf(true) }
    LaunchedEffect(queue.current?.item?.key) {
        val idx = rows.indexOfFirst { it is Row.Item && it.section == Row.Section.Current }
        if (idx <= 0) return@LaunchedEffect
        val target = (idx - 1).coerceAtLeast(0)
        when {
            // Opening the queue: start at the current track, without an animation.
            firstScroll -> listState.scrollToItem(target)
            userDragging || listState.isScrollInProgress -> {}
            SystemClock.uptimeMillis() - lastUserScroll < USER_SCROLL_GRACE_MS -> {}
            else -> listState.animateScrollToItem(target)
        }
        firstScroll = false
    }
    Box(modifier) {
        LazyColumn(state = listState, contentPadding = PaddingValues(bottom = 120.dp), modifier = Modifier.fillMaxSize().testTag("queue.list")) {
            items(rows, key = { r -> when (r) { is Row.Header -> "hdr:" + r.id; is Row.Item -> r.entry.item.key } }) { row ->
                when (row) {
                    is Row.Header -> androidx.compose.material3.Text(row.text, style = MaterialTheme.typography.labelLarge, color = MaterialTheme.colorScheme.primary, modifier = Modifier.padding(horizontal = 16.dp, vertical = 8.dp).semantics { heading() })
                    is Row.Item -> {
                        val entry = row.entry
                        val draggable = row.section == Row.Section.Next || row.section == Row.Section.Upcoming
                        val dismiss = rememberSwipeToDismissBoxState(positionalThreshold = { it * 0.45f }, confirmValueChange = { v ->
                            if (v != SwipeToDismissBoxValue.Settled) { client.dispatch(Commands.removeQueueItems(listOf(entry.item.key))); haptics.performHapticFeedback(HapticFeedbackType.Confirm); true } else false
                        })
                        ReorderableItem(reorderable, key = entry.item.key, enabled = draggable) { _ ->
                            SwipeToDismissBox(
                                state = dismiss,
                                enableDismissFromStartToEnd = row.section != Row.Section.Current,
                                enableDismissFromEndToStart = row.section != Row.Section.Current,
                                backgroundContent = {
                                    Box(Modifier.fillMaxSize().background(MaterialTheme.colorScheme.errorContainer).padding(horizontal = 24.dp), contentAlignment = if (dismiss.dismissDirection == SwipeToDismissBoxValue.StartToEnd) Alignment.CenterStart else Alignment.CenterEnd) {
                                        // Decorative: the row's "Remove from queue" action is the accessible path.
                                        Icon(Icons.Filled.Delete, null, tint = MaterialTheme.colorScheme.onErrorContainer)
                                    }
                                },
                            ) {
                                val why = (entry.item.source as? QueueSource.Autoplay)?.data?.reason
                                // Accessible alternatives to the swipe (remove) and the drag handle (move).
                                val index = movable.indexOf(entry.item.key)
                                val extra = buildList {
                                    if (row.section != Row.Section.Current) add(CustomAccessibilityAction(removeLabel) { client.dispatch(Commands.removeQueueItems(listOf(entry.item.key))); true })
                                    if (draggable && index > 0) add(CustomAccessibilityAction(moveUpLabel) { client.dispatch(Commands.moveQueueItem(entry.item.key, index - 1)); true })
                                    if (draggable && index >= 0 && index < movable.lastIndex) add(CustomAccessibilityAction(moveDownLabel) { client.dispatch(Commands.moveQueueItem(entry.item.key, index + 1)); true })
                                }
                                // Transparent at rest (the player's backdrop shows through); an opaque
                                // surface only while swiped, to cover the remove background.
                                val swipeSurface = MaterialTheme.colorScheme.surfaceContainerHighest
                                Column(Modifier.drawBehind { if (dismiss.dismissDirection != SwipeToDismissBoxValue.Settled) drawRect(swipeSurface) }.alpha(if (row.section == Row.Section.History) 0.55f else 1f)) {
                                    TrackRow(entry.track, onClick = { client.dispatch(Commands.jumpToQueueItem(entry.item.key)) }, onMore = { sheetFor = entry },
                                        actionTarget = Commands.queueItems(listOf(entry.item.key)), extraActions = extra,
                                        selected = selecting && selection.contains(entry.item.key), selectionActive = selecting, onToggleSelect = { client.toggleSelected(SelectionKind.QueueItems, entry.item.key) },
                                        nowPlaying = row.section == Row.Section.Current,
                                        trailing = if (draggable) ({
                                            Icon(Icons.Filled.DragHandle, null, tint = MaterialTheme.colorScheme.onSurfaceVariant,
                                                modifier = Modifier.padding(start = 8.dp).draggableHandle(onDragStarted = { haptics.performHapticFeedback(HapticFeedbackType.GestureThresholdActivate) }, onDragStopped = { haptics.performHapticFeedback(HapticFeedbackType.GestureEnd) }))
                                        }) else null)
                                    if (entry.item.unavailable == true) Text(stringResource(R.string.queue_unavailable), style = MaterialTheme.typography.labelSmall, color = MaterialTheme.colorScheme.error, modifier = Modifier.padding(start = 78.dp, bottom = 6.dp))
                                    why?.let { Text(stringResource(R.string.player_autoplay_reason, it), style = MaterialTheme.typography.labelSmall, color = MaterialTheme.colorScheme.tertiary, modifier = Modifier.padding(start = 78.dp, bottom = 6.dp)) }
                                }
                            }
                        }
                    }
                }
            }
            item {
                Row(Modifier.fillMaxWidth().padding(16.dp), horizontalArrangement = androidx.compose.foundation.layout.Arrangement.SpaceEvenly) {
                    TextButton(onClick = { client.dispatch(Command.ClearInsertions) }, enabled = queue.playingNext.isNotEmpty()) { Text(stringResource(R.string.queue_clear_insertions)) }
                    TextButton(onClick = { client.dispatch(Command.ClearQueue) }) { Text(stringResource(R.string.queue_clear)) }
                }
                if (queue.totalUpcoming.toInt() > queue.upcoming.size) Text(stringResource(R.string.queue_more_upcoming, queue.totalUpcoming.toInt() - queue.upcoming.size), style = MaterialTheme.typography.labelSmall, modifier = Modifier.padding(horizontal = 16.dp))
                Spacer(Modifier.height(24.dp))
            }
        }
        SelectionToolbar(Modifier.align(Alignment.BottomCenter))
    }
    sheetFor?.let { e -> ActionSheet(Commands.queueItems(listOf(e.item.key)), e.track.title, e.track.artist, onDismiss = { sheetFor = null }) }
}
