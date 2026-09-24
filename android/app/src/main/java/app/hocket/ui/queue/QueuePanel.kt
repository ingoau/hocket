package app.hocket.ui.queue

import android.os.SystemClock
import androidx.compose.animation.animateColorAsState
import androidx.compose.foundation.background
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.interaction.DragInteraction
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.calculateEndPadding
import androidx.compose.foundation.layout.calculateStartPadding
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.AllInclusive
import androidx.compose.material.icons.filled.Check
import androidx.compose.material.icons.filled.Delete
import androidx.compose.material.icons.filled.DragHandle
import androidx.compose.material.icons.filled.GraphicEq
import androidx.compose.material.icons.filled.Repeat
import androidx.compose.material.icons.filled.RepeatOne
import androidx.compose.material.icons.filled.Shuffle
import androidx.compose.material3.FilledTonalToggleButton
import androidx.compose.material3.Icon
import androidx.compose.material3.LocalContentColor
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.SwipeToDismissBox
import androidx.compose.material3.SwipeToDismissBoxValue
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.ToggleButtonColors
import androidx.compose.material3.rememberSwipeToDismissBoxState
import androidx.compose.material3.ripple
import androidx.compose.runtime.Composable
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.runtime.withFrameNanos
import androidx.compose.runtime.snapshotFlow
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableLongStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.drawBehind
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.hapticfeedback.HapticFeedbackType
import androidx.compose.ui.platform.LocalHapticFeedback
import androidx.compose.ui.platform.LocalLayoutDirection
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.CustomAccessibilityAction
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.customActions
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.selected
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import app.hocket.R
import app.hocket.core.ArtworkSizes
import app.hocket.core.Commands
import app.hocket.core.api.Command
import app.hocket.core.api.QueueEntry
import app.hocket.core.api.QueueKey
import app.hocket.core.api.QueueView
import app.hocket.core.api.QueueSource
import app.hocket.core.api.RepeatMode
import app.hocket.core.client.SelectionKind
import app.hocket.ui.LocalCoreClient
import app.hocket.ui.components.ActionSheet
import app.hocket.ui.components.Artwork
import app.hocket.ui.components.EmptyState
import app.hocket.ui.components.ListArtCorner
import app.hocket.ui.components.OfflineBadge
import app.hocket.ui.components.RatingDialog
import app.hocket.ui.components.SelectionToolbar
import app.hocket.ui.components.offlineStateText
import app.hocket.ui.components.trackLabel
import app.hocket.ui.components.trackRowActions
import app.hocket.core.ActionIds
import kotlinx.coroutines.delay
import sh.calvin.reorderable.ReorderableItem
import sh.calvin.reorderable.rememberReorderableLazyListState

/** How long after the user last scrolled the queue it stops following the current track. */
private const val USER_SCROLL_GRACE_MS = 8_000L

/** How long a dropped drag keeps its order on screen waiting for the core's queue. */
private const val DROP_SETTLE_MS = 2_000L

private const val NOW_KEY = "hdr:c"
private const val FOOTER_KEY = "footer"

/** Kept for existing callers: the queue list with its mode header. */
@Composable
fun QueuePanel(modifier: Modifier = Modifier) = QueueList(modifier)

/**
 * The player's queue, Apple Music style: a fixed header of shuffle / repeat / autoplay toggles over
 * ONE list of history (scrolled away above), now playing, "Playing next" and "Continue playing".
 * Everything is drawn on a transparent background so the player's artwork gradient shows through;
 * colours come from [LocalContentColor] and the (artwork-derived) [MaterialTheme].
 * [contentPadding] pads the list only (e.g. room for controls drawn over its bottom edge).
 */
@Composable
fun QueueList(modifier: Modifier = Modifier, contentPadding: PaddingValues = PaddingValues(), listModifier: Modifier = Modifier) {
    val dir = LocalLayoutDirection.current
    Column(modifier) {
        QueueModeHeader(
            Modifier.fillMaxWidth()
                .padding(start = 16.dp + contentPadding.calculateStartPadding(dir), end = 16.dp + contentPadding.calculateEndPadding(dir), top = 4.dp, bottom = 8.dp),
        )
        QueueTimeline(Modifier.fillMaxWidth().weight(1f).then(listModifier), contentPadding)
    }
}

/** Shuffle, repeat (off / all / one) and autoplay as three equal tonal pills. */
@Composable
private fun QueueModeHeader(modifier: Modifier = Modifier) {
    val client = LocalCoreClient.current
    val queue by client.queue.collectAsStateWithLifecycle()
    Row(modifier, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
        ModeToggle(
            checked = queue.shuffle, onCheckedChange = { client.dispatch(Commands.setShuffle(it)) },
            icon = Icons.Filled.Shuffle, label = stringResource(R.string.action_shuffle), tag = "queue.shuffle", modifier = Modifier.weight(1f),
        )
        ModeToggle(
            checked = queue.repeat != RepeatMode.Off,
            onCheckedChange = { client.dispatch(Commands.setRepeat(when (queue.repeat) { RepeatMode.Off -> RepeatMode.All; RepeatMode.All -> RepeatMode.One; RepeatMode.One -> RepeatMode.Off })) },
            icon = if (queue.repeat == RepeatMode.One) Icons.Filled.RepeatOne else Icons.Filled.Repeat,
            label = stringResource(R.string.action_label_repeat),
            state = stringResource(when (queue.repeat) { RepeatMode.Off -> R.string.player_repeat_off; RepeatMode.All -> R.string.player_repeat_all; RepeatMode.One -> R.string.player_repeat_one }),
            tag = "queue.repeat", modifier = Modifier.weight(1f),
        )
        ModeToggle(
            checked = queue.autoplay, onCheckedChange = { client.dispatch(Commands.setAutoplay(it)) },
            icon = Icons.Filled.AllInclusive, label = stringResource(R.string.player_autoplay), tag = "queue.infinite", modifier = Modifier.weight(1f),
        )
    }
}

@Composable
private fun ModeToggle(checked: Boolean, onCheckedChange: (Boolean) -> Unit, icon: ImageVector, label: String, tag: String, modifier: Modifier = Modifier, state: String? = null) {
    val content = LocalContentColor.current
    // Off: a faint wash of the content colour over the artwork backdrop; on: the scheme's primary.
    val container by animateColorAsState(if (checked) MaterialTheme.colorScheme.primary else content.copy(alpha = 0.12f), label = "modeContainer")
    val foreground by animateColorAsState(if (checked) MaterialTheme.colorScheme.onPrimary else content, label = "modeContent")
    FilledTonalToggleButton(
        checked = checked,
        onCheckedChange = onCheckedChange,
        colors = ToggleButtonColors(
            containerColor = container, contentColor = foreground,
            disabledContainerColor = container.copy(alpha = 0.06f), disabledContentColor = foreground.copy(alpha = 0.38f),
            checkedContainerColor = container, checkedContentColor = foreground,
        ),
        modifier = modifier.heightIn(min = 40.dp).testTag(tag).semantics { contentDescription = label; if (state != null) stateDescription = state },
    ) { Icon(icon, null) }
}

private sealed interface Line {
    data class Header(val id: String, val title: String, val subtitle: String? = null, val clear: Clear? = null) : Line
    data class Item(val entry: QueueEntry, val section: Section) : Line
    data object Footer : Line
    enum class Section { History, Current, Next, Upcoming, Autoplay }
    enum class Clear { History, Next }
}

/**
 * A drag in progress (or just dropped): the movable keys (playing next + upcoming) in the order the
 * finger has put them, over the queue as it was when the drag started. The core re-keys an item
 * that moves (a context item's `ctx-<index>` follows the context list; one moved into playing next
 * gets a new key), so sending a move per slot lost the dragged item after the first one. The list
 * reorders this local copy while the finger is down and the core gets ONE move when it lifts;
 * [dropped] keeps the new order on screen until the core's queue arrives.
 */
private data class QueueDrag(val base: QueueView, val key: QueueKey, val order: List<QueueKey>, val dropped: Boolean = false)

/** Playing next + upcoming keys: the combined index `MoveQueueItem` takes. */
private fun QueueView.movableKeys(): List<QueueKey> = playingNext.map { it.item.key } + upcoming.map { it.item.key }

private data class TimelineLabels(val history: String, val now: String, val next: String, val continuing: String, val from: String?, val autoplay: String)

/**
 * The timeline's rows for [queue]; with a [drag], its items in the drag's order. The dragged item
 * takes the section the core will give it: playing next when it is among the insertions (the core
 * then makes it one), else among the context / autoplay items.
 */
private fun timelineRows(queue: QueueView, drag: QueueDrag?, labels: TimelineLabels): List<Line> = buildList {
    if (queue.history.isNotEmpty()) {
        add(Line.Header("h", labels.history, clear = Line.Clear.History))
        queue.history.forEach { add(Line.Item(it, Line.Section.History)) }
    }
    queue.current?.let { add(Line.Header("c", labels.now)); add(Line.Item(it, Line.Section.Current)) }
    var next = queue.playingNext
    var rest = queue.upcoming
    if (drag != null) {
        val entries = (queue.playingNext + queue.upcoming).associateBy { it.item.key }
        val ordered = drag.order.mapNotNull { entries[it] }
        val otherInsertions = queue.playingNext.count { it.item.key != drag.key }
        val at = drag.order.indexOf(drag.key)
        val nextCount = otherInsertions + if (at in 0 until otherInsertions) 1 else 0
        next = ordered.take(nextCount)
        rest = ordered.drop(nextCount)
    }
    if (next.isNotEmpty()) {
        add(Line.Header("n", labels.next, clear = Line.Clear.Next))
        next.forEach { add(Line.Item(it, Line.Section.Next)) }
    }
    val (auto, ctx) = rest.partition { it.item.source is QueueSource.Autoplay }
    if (ctx.isNotEmpty()) { add(Line.Header("u", labels.continuing, labels.from)); ctx.forEach { add(Line.Item(it, Line.Section.Upcoming)) } }
    if (auto.isNotEmpty()) { add(Line.Header("a", labels.autoplay)); auto.forEach { add(Line.Item(it, Line.Section.Autoplay)) } }
    if (isNotEmpty()) add(Line.Footer)
}

/**
 * The one scrollable list: history above (oldest first), the current item, then "Playing next"
 * (insertions) and "Continue playing · From <context>" (the permuted context), then autoplay with
 * its "why". It opens at "Now playing" so history is only revealed by scrolling up. Drag handles
 * reorder (haptics; one move per drag), swipe removes (undo toast from the core), tap jumps
 * (history: play again).
 */
@Composable
private fun QueueTimeline(modifier: Modifier, contentPadding: PaddingValues) {
    val client = LocalCoreClient.current
    val queue by client.queue.collectAsStateWithLifecycle()
    val selection by client.selection.collectAsStateWithLifecycle()
    val kind by client.selectionKind.collectAsStateWithLifecycle()
    val selecting = selection.active && kind == SelectionKind.QueueItems
    val haptics = LocalHapticFeedback.current
    val density = LocalDensity.current
    var sheetFor by remember { mutableStateOf<QueueEntry?>(null) }
    var ratingFor by remember { mutableStateOf<QueueEntry?>(null) }
    val labels = TimelineLabels(
        history = stringResource(R.string.queue_section_history),
        now = stringResource(R.string.queue_now),
        next = stringResource(R.string.queue_playing_next),
        continuing = stringResource(R.string.queue_section_continue),
        from = queue.contextLabel?.let { stringResource(R.string.queue_section_from, it) },
        autoplay = stringResource(R.string.queue_autoplay_section),
    )
    val removeLabel = stringResource(R.string.action_remove_from_queue)
    val moveUpLabel = stringResource(R.string.a11y_move_up)
    val moveDownLabel = stringResource(R.string.a11y_move_down)
    var drag by remember { mutableStateOf<QueueDrag?>(null) }
    // A dropped drag gives way to the core's queue as soon as that changes, or after a while if
    // nothing changes (the core refused the move).
    LaunchedEffect(queue, drag?.dropped) {
        val d = drag?.takeIf { it.dropped } ?: return@LaunchedEffect
        if (queue != d.base) { drag = null; return@LaunchedEffect }
        delay(DROP_SETTLE_MS)
        if (drag === d) drag = null
    }
    // While a drag is under way the list shows the queue as it was when the drag started.
    val shown = drag?.base ?: queue
    val rows: List<Line> = remember(shown, drag, labels) { timelineRows(shown, drag, labels) }
    if (queue.current == null && rows.isEmpty()) {
        EmptyState(stringResource(R.string.empty_queue_title), stringResource(R.string.empty_queue_body), modifier)
        return
    }
    val listState = rememberLazyListState()
    // Combined index into playing-next + upcoming, which is what MoveQueueItem takes.
    val movable = remember(queue) { queue.movableKeys() }
    val reorderable = rememberReorderableLazyListState(listState) { from, to ->
        val d = drag?.takeIf { !it.dropped } ?: return@rememberReorderableLazyListState
        val fromIndex = d.order.indexOf(from.key)
        val toIndex = d.order.indexOf(to.key)
        if (fromIndex < 0 || toIndex < 0) return@rememberReorderableLazyListState
        drag = d.copy(order = d.order.toMutableList().apply { add(toIndex, removeAt(fromIndex)) })
        haptics.performHapticFeedback(HapticFeedbackType.SegmentFrequentTick)
    }
    fun startDrag(key: QueueKey) {
        drag = QueueDrag(queue, key, queue.movableKeys())
        haptics.performHapticFeedback(HapticFeedbackType.GestureThresholdActivate)
    }
    fun drop() {
        val d = drag?.takeIf { !it.dropped } ?: return
        haptics.performHapticFeedback(HapticFeedbackType.GestureEnd)
        val to = d.order.indexOf(d.key)
        if (to < 0 || to == d.base.movableKeys().indexOf(d.key)) { drag = null; return }
        drag = d.copy(dropped = true)
        client.dispatch(Commands.moveQueueItem(d.key, to))
    }
    // Follow the current track, but never fight the user: no auto-scroll while a finger is on the
    // list or for a while after they last scrolled it (Metrolist/Navic leave a browsed queue alone).
    var lastUserScroll by remember { mutableLongStateOf(-USER_SCROLL_GRACE_MS) }
    var userDragging by remember { mutableStateOf(false) }
    LaunchedEffect(listState) {
        listState.interactionSource.interactions.collect { i ->
            when (i) {
                is DragInteraction.Start -> { userDragging = true; lastUserScroll = SystemClock.uptimeMillis() }
                is DragInteraction.Stop, is DragInteraction.Cancel -> { userDragging = false; lastUserScroll = SystemClock.uptimeMillis() }
            }
        }
    }
    // Room below the last row so "Now playing" can reach the top even when little follows it
    // (otherwise the list clamps and history shows): the viewport minus what follows the header.
    val hasHistory = queue.history.isNotEmpty()
    var tailPx by remember { mutableIntStateOf(0) }
    LaunchedEffect(listState, hasHistory) {
        if (!hasHistory) { tailPx = 0; return@LaunchedEffect }
        snapshotFlow { listState.layoutInfo }.collect { info ->
            val visible = info.visibleItemsInfo
            val now = visible.firstOrNull { it.key == NOW_KEY } ?: return@collect
            val footer = visible.firstOrNull { it.key == FOOTER_KEY }
            val viewport = info.viewportSize.height - info.beforeContentPadding - info.afterContentPadding
            tailPx = if (footer == null) 0 else (viewport - (footer.offset + footer.size - now.offset)).coerceAtLeast(0)
        }
    }
    var positioned by remember { mutableStateOf(false) }
    LaunchedEffect(queue.current?.item?.key) {
        // "Now playing" at the top; history stays above, reached by scrolling up.
        val target = rows.indexOfFirst { it is Line.Header && it.id == "c" }
        if (target < 0) return@LaunchedEffect
        fun leaveAlone() = userDragging || listState.isScrollInProgress || SystemClock.uptimeMillis() - lastUserScroll < USER_SCROLL_GRACE_MS
        when {
            // Opening the queue: start at the current track, without an animation.
            !positioned -> listState.scrollToItem(target)
            leaveAlone() -> return@LaunchedEffect
            else -> listState.animateScrollToItem(target)
        }
        positioned = true
        // The tail spacer catches up a frame after the rows change; settle on the header then.
        repeat(2) { withFrameNanos { } }
        if (listState.firstVisibleItemIndex != target && !leaveAlone()) listState.scrollToItem(target)
    }
    Box(modifier) {
        LazyColumn(state = listState, contentPadding = contentPadding, modifier = Modifier.fillMaxSize().testTag("queue.list")) {
            items(rows, key = { r -> when (r) { is Line.Header -> "hdr:" + r.id; is Line.Item -> r.entry.item.key; Line.Footer -> FOOTER_KEY } }) { row ->
                when (row) {
                    is Line.Header -> SectionTitle(row.title, row.subtitle, clear = row.clear?.let { c ->
                        when (c) {
                            Line.Clear.History -> stringResource(R.string.queue_clear_history) to { client.dispatch(Commands.removeQueueItems(queue.history.map { it.item.key })) }
                            Line.Clear.Next -> stringResource(R.string.queue_clear_insertions) to { client.dispatch(Command.ClearInsertions) }
                        }
                    })
                    Line.Footer -> Column(Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 12.dp)) {
                        if (queue.totalUpcoming.toInt() > queue.upcoming.size) Text(stringResource(R.string.queue_more_upcoming, queue.totalUpcoming.toInt() - queue.upcoming.size), style = MaterialTheme.typography.labelMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
                        TextButton(onClick = { client.dispatch(Command.ClearQueue) }, modifier = Modifier.align(Alignment.CenterHorizontally)) { Text(stringResource(R.string.queue_clear)) }
                        Spacer(Modifier.height(24.dp))
                    }
                    is Line.Item -> {
                        val entry = row.entry
                        val draggable = row.section == Line.Section.Next || row.section == Line.Section.Upcoming || row.section == Line.Section.Autoplay
                        val removable = row.section != Line.Section.Current
                        val dismiss = rememberSwipeToDismissBoxState(positionalThreshold = { it * 0.45f }, confirmValueChange = { v ->
                            if (v != SwipeToDismissBoxValue.Settled) { client.dispatch(Commands.removeQueueItems(listOf(entry.item.key))); haptics.performHapticFeedback(HapticFeedbackType.Confirm); true } else false
                        })
                        ReorderableItem(reorderable, key = entry.item.key, enabled = draggable) { _ ->
                            SwipeToDismissBox(
                                state = dismiss,
                                enableDismissFromStartToEnd = removable && drag == null,
                                enableDismissFromEndToStart = removable && drag == null,
                                backgroundContent = {
                                    // Only while a swipe is under way: the rows are transparent at rest,
                                    // so a resting background would show straight through them.
                                    if (dismiss.dismissDirection != SwipeToDismissBoxValue.Settled) Box(Modifier.fillMaxSize().background(MaterialTheme.colorScheme.errorContainer).padding(horizontal = 24.dp), contentAlignment = if (dismiss.dismissDirection == SwipeToDismissBoxValue.StartToEnd) Alignment.CenterStart else Alignment.CenterEnd) {
                                        // Decorative: the row's "Remove from queue" action is the accessible path.
                                        Icon(Icons.Filled.Delete, null, tint = MaterialTheme.colorScheme.onErrorContainer)
                                    }
                                },
                            ) {
                                // Accessible alternatives to the swipe (remove) and the drag handle (move).
                                val index = movable.indexOf(entry.item.key)
                                val extra = buildList {
                                    if (removable) add(CustomAccessibilityAction(removeLabel) { client.dispatch(Commands.removeQueueItems(listOf(entry.item.key))); true })
                                    if (draggable && index > 0) add(CustomAccessibilityAction(moveUpLabel) { client.dispatch(Commands.moveQueueItem(entry.item.key, index - 1)); true })
                                    if (draggable && index >= 0 && index < movable.lastIndex) add(CustomAccessibilityAction(moveDownLabel) { client.dispatch(Commands.moveQueueItem(entry.item.key, index + 1)); true })
                                }
                                // Transparent at rest (the player's backdrop shows through); an opaque
                                // surface only while swiped, to cover the remove background.
                                val swipeSurface = MaterialTheme.colorScheme.surfaceContainerHighest
                                QueueRow(
                                    entry = entry,
                                    section = row.section,
                                    selected = selecting && selection.contains(entry.item.key),
                                    selecting = selecting,
                                    extraActions = extra,
                                    onClick = { client.dispatch(Commands.jumpToQueueItem(entry.item.key)) },
                                    onToggleSelect = { client.toggleSelected(SelectionKind.QueueItems, entry.item.key) },
                                    onMore = { sheetFor = entry },
                                    onRate = { ratingFor = entry },
                                    modifier = Modifier.testTag("queue.row." + entry.item.key).drawBehind { if (dismiss.dismissDirection != SwipeToDismissBoxValue.Settled) drawRect(swipeSurface) },
                                    handle = if (draggable) ({
                                        Box(
                                            Modifier.size(48.dp).testTag("queue.handle." + entry.item.key).draggableHandle(
                                                onDragStarted = { startDrag(entry.item.key) },
                                                onDragStopped = { drop() },
                                            ),
                                            contentAlignment = Alignment.Center,
                                        ) { Icon(Icons.Filled.DragHandle, null, tint = MaterialTheme.colorScheme.onSurfaceVariant) }
                                    }) else null,
                                )
                            }
                        }
                    }
                }
            }
            item(key = "tail") { Spacer(Modifier.height(with(density) { tailPx.toDp() })) }
        }
        SelectionToolbar(Modifier.align(Alignment.BottomCenter))
    }
    sheetFor?.let { e -> ActionSheet(Commands.queueItems(listOf(e.item.key)), e.track.title, e.track.artist, onDismiss = { sheetFor = null }) }
    ratingFor?.let { e ->
        RatingDialog(current = e.track.rating.toInt(), onRate = { stars -> client.dispatch(Commands.runAction(ActionIds.rate(stars), Commands.queueItems(listOf(e.item.key)))); ratingFor = null }, onDismiss = { ratingFor = null })
    }
}

/** A section title in the reference's style: semibold title, optional subtitle, "Clear" on the right. */
@Composable
private fun SectionTitle(title: String, subtitle: String?, clear: Pair<String, () -> Unit>?) {
    Row(Modifier.fillMaxWidth().heightIn(min = 48.dp).padding(start = 16.dp, end = 4.dp, top = 12.dp, bottom = 4.dp), verticalAlignment = Alignment.CenterVertically) {
        Column(Modifier.weight(1f).semantics(mergeDescendants = true) { heading() }) {
            Text(title, style = MaterialTheme.typography.titleMedium, fontWeight = FontWeight.SemiBold, color = MaterialTheme.colorScheme.onSurface)
            subtitle?.let { Text(it, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant) }
        }
        clear?.let { (spoken, onClear) ->
            TextButton(onClick = onClear, modifier = Modifier.semantics { contentDescription = spoken }) {
                Text(stringResource(R.string.queue_section_clear), color = MaterialTheme.colorScheme.primary)
            }
        }
    }
}

/**
 * One queue row: 48 dp artwork, title and artist, and a drag handle for reorderable items; no
 * opaque container. It speaks like a track row (label, "playing" / offline state, row menu as
 * actions) with the queue's remove / move actions added. Long press starts multi-select.
 */
@Composable
private fun QueueRow(
    entry: QueueEntry,
    section: Line.Section,
    selected: Boolean,
    selecting: Boolean,
    extraActions: List<CustomAccessibilityAction>,
    onClick: () -> Unit,
    onToggleSelect: () -> Unit,
    onMore: () -> Unit,
    onRate: () -> Unit,
    modifier: Modifier = Modifier,
    handle: (@Composable () -> Unit)? = null,
) {
    val track = entry.track
    val haptics = LocalHapticFeedback.current
    val current = section == Line.Section.Current
    val actions = trackRowActions(track, Commands.queueItems(listOf(entry.item.key)), onRate = onRate, onMore = onMore, extra = extraActions)
    val state = listOfNotNull(if (current) stringResource(R.string.row_state_playing) else null, offlineStateText(track.offline)).joinToString(", ").ifEmpty { null }
    val label = trackLabel(track)
    val selectLabel = stringResource(R.string.a11y_select)
    val deselectLabel = stringResource(R.string.a11y_deselect)
    val playLabel = stringResource(R.string.action_play)
    val content = LocalContentColor.current
    val wash by animateColorAsState(if (selected) content.copy(alpha = 0.14f) else Color.Transparent, label = "queueRowSelected")
    val dim = if (section == Line.Section.History) 0.7f else 1f
    Column(
        modifier
            .fillMaxWidth()
            .background(wash)
            .combinedClickable(
                indication = ripple(),
                interactionSource = null,
                onClick = { if (selecting) onToggleSelect() else onClick() },
                onClickLabel = if (!selecting) playLabel else if (selected) deselectLabel else selectLabel,
                onLongClick = { haptics.performHapticFeedback(HapticFeedbackType.LongPress); onToggleSelect() },
                onLongClickLabel = if (selected) deselectLabel else selectLabel,
            )
            .semantics(mergeDescendants = true) {
                contentDescription = label
                if (selecting) this.selected = selected
                if (state != null) stateDescription = state
                if (actions.isNotEmpty()) customActions = actions
            },
    ) {
        Row(Modifier.fillMaxWidth().heightIn(min = 64.dp).padding(start = 16.dp, end = if (handle != null) 4.dp else 16.dp, top = 8.dp, bottom = 8.dp), verticalAlignment = Alignment.CenterVertically) {
            Box(Modifier.size(48.dp), contentAlignment = Alignment.Center) {
                Artwork(track.coverArt, ArtworkSizes.THUMB, null, Modifier.size(48.dp), RoundedCornerShape(ListArtCorner))
                if (selected) Surface(shape = CircleShape, color = MaterialTheme.colorScheme.primary) { Icon(Icons.Filled.Check, null, Modifier.padding(4.dp), tint = MaterialTheme.colorScheme.onPrimary) }
            }
            Spacer(Modifier.width(14.dp))
            Column(Modifier.weight(1f)) {
                Row(verticalAlignment = Alignment.CenterVertically) {
                    if (current) {
                        Icon(Icons.Filled.GraphicEq, null, tint = MaterialTheme.colorScheme.primary, modifier = Modifier.size(16.dp))
                        Spacer(Modifier.width(6.dp))
                    }
                    Text(track.title, style = MaterialTheme.typography.bodyLarge, maxLines = 1, overflow = TextOverflow.Ellipsis, color = content.copy(alpha = content.alpha * dim))
                }
                Row(verticalAlignment = Alignment.CenterVertically) {
                    OfflineBadge(track.offline, describe = false)
                    Text(track.artist ?: stringResource(R.string.unknown_artist), style = MaterialTheme.typography.bodyMedium, maxLines = 1, overflow = TextOverflow.Ellipsis,
                        color = MaterialTheme.colorScheme.onSurfaceVariant.let { it.copy(alpha = it.alpha * dim) })
                }
            }
            handle?.invoke()
        }
        if (entry.item.unavailable == true) Text(stringResource(R.string.queue_unavailable), style = MaterialTheme.typography.labelSmall, color = MaterialTheme.colorScheme.error, modifier = Modifier.padding(start = 78.dp, bottom = 6.dp))
        (entry.item.source as? QueueSource.Autoplay)?.data?.reason?.let {
            Text(stringResource(R.string.player_autoplay_reason, it), style = MaterialTheme.typography.labelSmall, color = MaterialTheme.colorScheme.tertiary, modifier = Modifier.padding(start = 78.dp, bottom = 6.dp))
        }
    }
}
