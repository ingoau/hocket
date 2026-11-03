package app.hocket.ui.queue

import android.os.SystemClock
import androidx.compose.animation.animateColorAsState
import androidx.compose.foundation.gestures.scrollBy
import androidx.compose.foundation.background
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.draw.drawWithContent
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
import androidx.compose.material3.FilledTonalToggleButton
import androidx.compose.material3.Icon
import androidx.compose.material3.LocalContentColor
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.ToggleButtonColors
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
import app.hocket.core.api.QueryResult
import app.hocket.core.api.QueueSource
import app.hocket.core.api.RepeatMode
import app.hocket.core.QueueWindows
import app.hocket.core.windowEnd
import app.hocket.core.client.SelectionKind
import app.hocket.ui.LocalCoreClient
import app.hocket.ui.components.Artwork
import app.hocket.ui.components.EmptyState
import app.hocket.ui.components.ListArtCorner
import app.hocket.ui.components.OfflineBadge
import app.hocket.ui.components.PlaylistPicker
import app.hocket.ui.components.LocalAppSnackbar
import app.hocket.ui.components.SnackbarRedirect
import app.hocket.ui.components.SwipeAction
import app.hocket.ui.components.SwipeActionBox
import app.hocket.ui.components.formatClock
import app.hocket.ui.components.queueSectionOn
import app.hocket.ui.components.SwipeSurface
import app.hocket.ui.components.swipeActionIds
import app.hocket.ui.components.trackSwipeAction
import app.hocket.ui.components.RatingDialog
import app.hocket.ui.components.SelectionToolbar
import app.hocket.ui.components.offlineStateText
import app.hocket.ui.components.trackLabel
import app.hocket.ui.components.trackRowActions
import app.hocket.core.ActionIds
import app.hocket.ui.icons.HocketIcons
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.launch
import kotlinx.coroutines.withTimeoutOrNull
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.SnackbarDuration
import androidx.compose.material3.SnackbarResult
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.rememberUpdatedState
import sh.calvin.reorderable.ReorderableItem
import sh.calvin.reorderable.rememberReorderableLazyListState

/** How long after the user last scrolled the queue it stops following the current track. */
private const val USER_SCROLL_GRACE_MS = 8_000L

/** How long a dropped drag keeps its order on screen waiting for the core's queue. */
private const val DROP_SETTLE_MS = 2_000L

private const val NOW_KEY = "hdr:c"
private const val FOOTER_KEY = "footer"

/** Kept for existing callers: the queue list with its toggles. */
@Composable
fun QueuePanel(modifier: Modifier = Modifier) = QueueList(modifier)

/**
 * The player's queue: shuffle / repeat / autoplay toggles with ONE list of history (scrolled away
 * above), now playing, the "Next in queue" group and "Next from <context>". Everything is
 * drawn on a transparent background so the player's artwork gradient shows through; colours come
 * from [LocalContentColor] and the (artwork-derived) [MaterialTheme]. [contentPadding] pads the list
 * only (e.g. room for controls drawn over its bottom edge).
 *
 * The toggles head the list, or with [controlsAtBottom] (the full player's queue, #40) float at its
 * bottom straight on the backdrop, the list fading out before them (nothing is drawn behind them).
 */
@Composable
fun QueueList(modifier: Modifier = Modifier, contentPadding: PaddingValues = PaddingValues(), listModifier: Modifier = Modifier, controlsAtBottom: Boolean = false) {
    val dir = LocalLayoutDirection.current
    if (controlsAtBottom) {
        val strip = QUEUE_CONTROLS_STRIP
        // The list ends a little above the toggles (where it used to be masked away): rows never
        // sit behind them, so none takes their taps or accessibility focus. The toggles take a
        // 48 dp touch target, taller than they draw, so the gap clears that.
        val under = strip - STRIP_TOP_PAD + 14.dp
        Box(modifier) {
            QueueTimeline(
                Modifier.fillMaxSize().padding(bottom = under).then(listModifier).fadeOut().testTag("queue.timeline"),
                PaddingValues(
                    start = contentPadding.calculateStartPadding(dir), end = contentPadding.calculateEndPadding(dir),
                    top = contentPadding.calculateTopPadding() + 8.dp, bottom = contentPadding.calculateBottomPadding() + strip + 44.dp - under,
                ),
            )
            QueueToggles(
                Modifier
                    .align(Alignment.BottomCenter)
                    .fillMaxWidth()
                    .padding(start = 16.dp + contentPadding.calculateStartPadding(dir), end = 16.dp + contentPadding.calculateEndPadding(dir), top = STRIP_TOP_PAD, bottom = 12.dp)
                    .testTag("queue.controls"),
                labels = true,
            )
        }
        return
    }
    Column(modifier) {
        QueueToggles(
            Modifier.fillMaxWidth()
                .padding(start = 16.dp + contentPadding.calculateStartPadding(dir), end = 16.dp + contentPadding.calculateEndPadding(dir), top = 4.dp, bottom = 8.dp),
        )
        QueueTimeline(Modifier.fillMaxWidth().weight(1f).then(listModifier), contentPadding)
    }
}

/** The height of the toggles' strip at the bottom of the player's queue panel (the list scrolls under it). */
val QUEUE_CONTROLS_STRIP = 72.dp

/** The list's last [FADE] fading out (above the bottom toggles, where it ends). */
private fun Modifier.fadeOut(): Modifier = this
    .graphicsLayer { compositingStrategy = androidx.compose.ui.graphics.CompositingStrategy.Offscreen }
    .drawWithContent {
        drawContent()
        val from = (size.height - FADE.toPx()).coerceAtLeast(0f)
        drawRect(
            // Eased like the lyrics' edges: mostly gone over the last half.
            Brush.verticalGradient(0f to Color.Black, 0.5f to Color.Black.copy(alpha = 0.3f), 1f to Color.Transparent, startY = from, endY = size.height),
            topLeft = androidx.compose.ui.geometry.Offset(0f, from),
            size = size.copy(height = size.height - from),
            blendMode = androidx.compose.ui.graphics.BlendMode.DstIn,
        )
    }

/** How far the list fades out above the toggles, and the room above the toggles in their strip. */
private val FADE = 56.dp
private val STRIP_TOP_PAD = 20.dp

/** The widest a queue toggle pill grows. */
private val QUEUE_PILL_MAX_WIDTH = 200.dp

/**
 * Shuffle, repeat (off / all / one) and autoplay as three equal tonal pills; with [labels], each
 * names itself beside its icon where all three names fit.
 */
@Composable
private fun QueueToggles(modifier: Modifier = Modifier, labels: Boolean = false) {
    val client = LocalCoreClient.current
    val queue by client.queue.collectAsStateWithLifecycle()
    val measurer = androidx.compose.ui.text.rememberTextMeasurer()
    val style = MaterialTheme.typography.labelLarge
    val names = listOf(stringResource(R.string.action_shuffle), stringResource(R.string.action_label_repeat), stringResource(R.string.player_autoplay))
    androidx.compose.foundation.layout.BoxWithConstraints(modifier) {
    // Three across the width, but no longer than a pill reads well: wider, they sit centred.
    val pill = ((maxWidth - 16.dp) / 3).coerceAtMost(QUEUE_PILL_MAX_WIDTH)
    val widest = with(LocalDensity.current) { names.maxOf { measurer.measure(it, style).size.width }.toDp() }
    val showLabels = labels && widest + 20.dp + 6.dp + 24.dp <= pill
    Row(Modifier.width(pill * 3 + 16.dp).align(Alignment.Center), horizontalArrangement = Arrangement.spacedBy(8.dp), verticalAlignment = Alignment.CenterVertically) {
        ModeToggle(
            checked = queue.shuffle, onCheckedChange = { client.dispatch(Commands.setShuffle(it)) },
            icon = HocketIcons.Filled.Shuffle, label = stringResource(R.string.action_shuffle), tag = "queue.shuffle", modifier = Modifier.weight(1f), showLabel = showLabels,
        )
        ModeToggle(
            checked = queue.repeat != RepeatMode.Off,
            onCheckedChange = { client.dispatch(Commands.setRepeat(when (queue.repeat) { RepeatMode.Off -> RepeatMode.All; RepeatMode.All -> RepeatMode.One; RepeatMode.One -> RepeatMode.Off })) },
            icon = if (queue.repeat == RepeatMode.One) HocketIcons.Filled.RepeatOne else HocketIcons.Filled.Repeat,
            label = stringResource(R.string.action_label_repeat),
            state = stringResource(when (queue.repeat) { RepeatMode.Off -> R.string.player_repeat_off; RepeatMode.All -> R.string.player_repeat_all; RepeatMode.One -> R.string.player_repeat_one }),
            tag = "queue.repeat", modifier = Modifier.weight(1f), showLabel = showLabels,
        )
        ModeToggle(
            checked = queue.autoplay, onCheckedChange = { client.dispatch(Commands.setAutoplay(it)) },
            icon = HocketIcons.Filled.AllInclusive, label = stringResource(R.string.player_autoplay), tag = "queue.infinite", modifier = Modifier.weight(1f), showLabel = showLabels,
        )
    }
    }
}

@Composable
private fun ModeToggle(checked: Boolean, onCheckedChange: (Boolean) -> Unit, icon: ImageVector, label: String, tag: String, modifier: Modifier = Modifier, state: String? = null, showLabel: Boolean = false) {
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
        contentPadding = PaddingValues(horizontal = 12.dp),
        modifier = modifier.heightIn(min = 40.dp).testTag(tag).semantics { contentDescription = label; if (state != null) stateDescription = state },
    ) {
        Icon(icon, null, Modifier.size(20.dp))
        if (showLabel) {
            Spacer(Modifier.width(6.dp))
            Text(label, style = MaterialTheme.typography.labelLarge, maxLines = 1, modifier = Modifier.clearAndSetSemantics { })
        }
    }
}

/**
 * A drag in progress (or just dropped): the drag order ([dragOrder]: the group, the "Next from"
 * heading, the rest) as the finger has put it, over the queue as it was when the drag started.
 * The list reorders this local copy while the finger is down and the core gets ONE move when it
 * lifts; [dropped] keeps the new order on screen until the core's queue arrives.
 */
private data class QueueDrag(val base: QueueView, val key: QueueKey, val start: List<String>, val order: List<String>, val dropped: Boolean = false)

/**
 * Runs [run] (a queue edit the core records for undo) and offers to undo it in a snackbar saying
 * [text]: the Undo button takes back that history entry, once the core has recorded it.
 */
@Composable
private fun rememberUndoable(): (String, () -> Unit) -> Unit {
    val client = LocalCoreClient.current
    val snackbar = LocalAppSnackbar.current
    val undoLabel = stringResource(R.string.action_undo)
    return remember(client, snackbar, undoLabel) {
        { text, run ->
            val before = client.undo.value.history.mapTo(HashSet()) { it.id }
            run()
            snackbar?.launch {
                val after = withTimeoutOrNull(2_000) { client.undo.first { u -> u.history.any { it.id !in before } } }
                val entry = after?.history?.firstOrNull { it.id !in before }?.id
                val host = SnackbarRedirect.host ?: snackbar.host
                val result = host.showSnackbar(text, actionLabel = entry?.let { undoLabel }, duration = SnackbarDuration.Short)
                if (result == SnackbarResult.ActionPerformed && entry != null) client.dispatch(Commands.undoEntry(entry))
            }
        }
    }
}

/** Every history entry's key, paged in from the core (the view only carries a window of it). */
private suspend fun allHistoryKeys(client: app.hocket.core.client.CoreClient, view: QueueView): List<QueueKey> {
    val count = view.historyCount.toInt()
    val keys = ArrayList<QueueKey>(count)
    var start = 0
    while (start < count) {
        val page = (client.query(Commands.queueRange(start, minOf(Commands.QUEUE_RANGE_MAX, count - start))) as? QueryResult.Queue)?.data ?: break
        if (page.entries.isEmpty()) break
        page.entries.take(count - start).forEach { keys += it.item.key }
        start += page.entries.size
    }
    return keys
}

/**
 * The one scrollable list: history above (oldest first), the current item, the "Next in queue"
 * group (Stash and Clear on its heading), then "Next from <context>" (or "Later"). It opens at
 * "Now playing" so history is only revealed by scrolling up, and pages in more rows at either end
 * as they come into view. Drag handles reorder (one move per drag; dropping above the "Next from"
 * heading puts the song in the group, below takes it out), swipes run the queue's swipe actions
 * (remove by default, with Undo), tap jumps (history too: the pointer moves back).
 */
@Composable
private fun QueueTimeline(modifier: Modifier, contentPadding: PaddingValues) {
    val client = LocalCoreClient.current
    val queue by client.queue.collectAsStateWithLifecycle()
    val selection by client.selection.collectAsStateWithLifecycle()
    val kind by client.selectionKind.collectAsStateWithLifecycle()
    val selecting = selection.active && kind == SelectionKind.QueueItems
    val sectionOn = queueSectionOn()
    val haptics = LocalHapticFeedback.current
    val density = LocalDensity.current
    val scope = rememberCoroutineScope()
    val songMenu = app.hocket.ui.components.rememberSongMenu()
    val undoable = rememberUndoable()
    var ratingFor by remember { mutableStateOf<QueueEntry?>(null) }
    var pickerFor by remember { mutableStateOf<QueueEntry?>(null) }
    val removeLabel = stringResource(R.string.action_remove_from_queue)
    val removedText = stringResource(R.string.queue_removed)
    val clearedText = stringResource(R.string.queue_group_cleared)
    val stashedText = stringResource(R.string.queue_group_stashed)
    val moveUpLabel = stringResource(R.string.a11y_move_up)
    val moveDownLabel = stringResource(R.string.a11y_move_down)
    val (swipeStart, swipeEnd) = swipeActionIds(SwipeSurface.Queue)
    // The paged-in rows are the client's; let them go when the queue closes.
    DisposableEffect(client) { onDispose { client.resetQueueWindow() } }
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
    val rows: List<QueueLine> = remember(shown, drag, sectionOn) { queueLines(shown, drag?.order, sectionOn) }
    if (queue.current == null && rows.isEmpty()) {
        EmptyState(stringResource(R.string.empty_queue_title), stringResource(R.string.empty_queue_body), modifier)
        return
    }
    val listState = rememberLazyListState()
    val currentKey = queue.current?.item?.key
    val order = remember(queue, sectionOn) { dragOrder(queue, sectionOn) }
    fun remove(key: QueueKey) = undoable(removedText) { client.dispatch(Commands.removeQueueItems(listOf(key))) }
    fun move(key: QueueKey, up: Boolean): Boolean {
        val moved = stepOrder(order, key, up) ?: return false
        dropMove(moved, key, currentKey, order)?.let(client::dispatch)
        return true
    }
    val reorderable = rememberReorderableLazyListState(listState) { from, to ->
        val d = drag?.takeIf { !it.dropped } ?: return@rememberReorderableLazyListState
        val fromIndex = d.order.indexOf(from.key)
        val toIndex = d.order.indexOf(to.key)
        if (fromIndex < 0 || toIndex < 0) return@rememberReorderableLazyListState
        drag = d.copy(order = d.order.toMutableList().apply { add(toIndex, removeAt(fromIndex)) })
        haptics.performHapticFeedback(HapticFeedbackType.SegmentFrequentTick)
    }
    fun startDrag(key: QueueKey) {
        val start = dragOrder(queue, sectionOn)
        drag = QueueDrag(queue, key, start, start)
        haptics.performHapticFeedback(HapticFeedbackType.GestureThresholdActivate)
    }
    fun drop() {
        val d = drag?.takeIf { !it.dropped } ?: return
        haptics.performHapticFeedback(HapticFeedbackType.GestureEnd)
        val command = dropMove(d.order, d.key, d.base.current?.item?.key, d.start)
        if (command == null) { drag = null; return }
        drag = d.copy(dropped = true)
        client.dispatch(command)
    }
    // Page in more rows as either loaded end comes into view (once the list has opened at "Now
    // playing", so opening it never pages in history nobody scrolled to).
    var positioned by remember { mutableStateOf(false) }
    val latest by rememberUpdatedState(queue)
    // The first real row under the "more above" row, and its offset, when that row asked for a page:
    // rows prepended above it must not push it down (the list would keep the "more" row as its
    // anchor at the top, show it again and chain-load all of history).
    var topAnchor by remember { mutableStateOf<Pair<Any, Int>?>(null) }
    LaunchedEffect(listState, client) {
        snapshotFlow { if (positioned) listState.layoutInfo.visibleItemsInfo.map { it.key } else emptyList() }.collect { keys ->
            val q = latest
            val pointer = q.currentIndex?.toInt() ?: q.historyCount.toInt()
            val before = pointer - q.windowStart.toInt()
            val after = q.windowEnd - pointer - 1
            val top = MORE_TOP_KEY in keys
            val bottom = MORE_BOTTOM_KEY in keys
            if (top) topAnchor = listState.layoutInfo.visibleItemsInfo.firstOrNull { it.key != MORE_TOP_KEY }?.let { it.key to it.offset }
            if (top || bottom) client.extendQueue(before + if (top) QueueWindows.PAGE else 0, after + if (bottom) QueueWindows.PAGE else 0)
        }
    }
    LaunchedEffect(shown.windowStart) {
        val (key, offset) = topAnchor ?: return@LaunchedEffect
        topAnchor = null
        if (drag != null) return@LaunchedEffect
        val at = rows.indexOfFirst { it.key == key }
        if (at <= 0) return@LaunchedEffect
        listState.scrollToItem(at)
        listState.scrollBy(-offset.toFloat())
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
    val hasHistory = queue.historyCount.toInt() > 0
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
    LaunchedEffect(queue.current?.item?.key) {
        // "Now playing" at the top; history stays above, reached by scrolling up.
        val target = rows.indexOfFirst { it.key == NOW_KEY }
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
            items(rows, key = { it.key }) { row ->
                when (row) {
                    // Headers and the footer move with the rows (ReorderableItem animates those), so a
                    // section appearing or the current track moving on slides rather than snaps (#27).
                    // The "Next from" heading is a drop target: dragging past it moves a song in or
                    // out of the group.
                    is QueueLine.Header -> ReorderableItem(reorderable, key = row.key, enabled = row.id == QueueLine.Header.Id.Later && groupDrops(shown, sectionOn) && currentKey != null) { _ ->
                        when (row.id) {
                            QueueLine.Header.Id.History -> SectionTitle(stringResource(R.string.queue_section_history), tag = "queue.header.history",
                                buttons = listOf(SectionButton(stringResource(R.string.queue_section_clear), stringResource(R.string.queue_clear_history), "queue.history.clear") {
                                    val q = queue
                                    scope.launch { allHistoryKeys(client, q).takeIf { it.isNotEmpty() }?.let { client.dispatch(Commands.removeQueueItems(it)) } }
                                }))
                            QueueLine.Header.Id.Now -> SectionTitle(stringResource(R.string.queue_now), tag = "queue.header.now")
                            QueueLine.Header.Id.Group -> SectionTitle(stringResource(R.string.queue_section_group, row.count), tag = "queue.header.group",
                                buttons = listOf(
                                    SectionButton(stringResource(R.string.queue_group_stash), stringResource(R.string.queue_group_stash_a11y), "queue.group.stash") {
                                        undoable(stashedText) { client.dispatch(Command.StashQueueGroup) }
                                    },
                                    SectionButton(stringResource(R.string.queue_section_clear), stringResource(R.string.queue_group_clear_a11y), "queue.group.clear") {
                                        undoable(clearedText) { client.dispatch(Command.ClearQueueGroup) }
                                    },
                                ))
                            QueueLine.Header.Id.Later -> SectionTitle(
                                row.label?.let { stringResource(R.string.queue_section_next_from, it) } ?: stringResource(R.string.queue_section_later),
                                tag = "queue.header.later",
                            )
                        }
                    }
                    is QueueLine.More -> Box(Modifier.fillMaxWidth().height(56.dp), contentAlignment = Alignment.Center) {
                        CircularProgressIndicator(Modifier.size(20.dp), strokeWidth = 2.dp)
                    }
                    QueueLine.Footer -> Column(Modifier.animateItem().fillMaxWidth().padding(horizontal = 16.dp, vertical = 12.dp)) {
                        TextButton(onClick = { client.dispatch(Command.ClearQueue) }, modifier = Modifier.align(Alignment.CenterHorizontally)) { Text(stringResource(R.string.queue_clear)) }
                        Spacer(Modifier.height(24.dp))
                    }
                    is QueueLine.Item -> {
                        val entry = row.entry
                        val key = entry.item.key
                        val draggable = row.section == Section.Group || row.section == Section.Later
                        val removable = row.section != Section.Current
                        val target = Commands.queueItems(listOf(key))
                        // The current item is never swiped away (it is removed by skipping it), and a
                        // removal offers Undo.
                        @Composable
                        fun swipe(id: String) = when {
                            id == ActionIds.REMOVE_FROM_QUEUE && !removable -> null
                            id == ActionIds.REMOVE_FROM_QUEUE -> trackSwipeAction(id, entry.track, target, onAddToPlaylist = {})?.let { a ->
                                SwipeAction(a.id, a.label, a.icon, a.container, a.content, a.dismisses) { remove(key) }
                            }
                            else -> trackSwipeAction(id, entry.track, target, onAddToPlaylist = { pickerFor = entry })
                        }
                        ReorderableItem(reorderable, key = key, enabled = draggable) { _ ->
                            SwipeActionBox(
                                startToEnd = swipe(swipeStart),
                                endToStart = swipe(swipeEnd),
                                enabled = !selecting && drag == null,
                                // Transparent at rest (the player's backdrop shows through); an opaque
                                // surface only while swiped, to cover the action's background.
                                swipeSurface = MaterialTheme.colorScheme.surfaceContainerHighest,
                            ) {
                                // Accessible alternatives to the swipe (remove) and the drag handle (move).
                                val extra = buildList {
                                    if (removable) add(CustomAccessibilityAction(removeLabel) { remove(key); true })
                                    if (draggable && stepOrder(order, key, up = true) != null) add(CustomAccessibilityAction(moveUpLabel) { move(key, up = true) })
                                    if (draggable && stepOrder(order, key, up = false) != null) add(CustomAccessibilityAction(moveDownLabel) { move(key, up = false) })
                                }
                                QueueRow(
                                    entry = entry,
                                    section = row.section,
                                    selected = selecting && selection.contains(key),
                                    selecting = selecting,
                                    extraActions = extra,
                                    // The playing row plays on (resumes when paused): a jump to the
                                    // current item restarts it, and this row sits right above the
                                    // ones being dragged and tapped while editing the queue.
                                    onClick = { client.dispatch(if (row.section == Section.Current) Command.Play else Commands.jumpToQueueItem(key)) },
                                    onToggleSelect = { client.toggleSelected(SelectionKind.QueueItems, key) },
                                    onMore = { songMenu.open(entry.track, Commands.queueItems(listOf(key))) },
                                    onRate = { ratingFor = entry },
                                    modifier = Modifier.testTag("queue.row.$key"),
                                    handle = if (draggable) ({
                                        Box(
                                            Modifier.size(48.dp).testTag("queue.handle.$key").draggableHandle(
                                                onDragStarted = { startDrag(key) },
                                                onDragStopped = { drop() },
                                            ),
                                            contentAlignment = Alignment.Center,
                                        ) { Icon(HocketIcons.Filled.DragHandle, null, tint = MaterialTheme.colorScheme.onSurfaceVariant) }
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
    pickerFor?.let { e -> PlaylistPicker(target = Commands.queueItems(listOf(e.item.key)), onDismiss = { pickerFor = null }) }
    ratingFor?.let { e ->
        RatingDialog(current = e.track.rating.toInt(), onRate = { stars -> client.dispatch(Commands.runAction(ActionIds.rate(stars), Commands.queueItems(listOf(e.item.key)))); ratingFor = null }, onDismiss = { ratingFor = null })
    }
}

/** A text button on a section heading: [label] on screen, [spoken] for TalkBack. */
private class SectionButton(val label: String, val spoken: String, val tag: String, val onClick: () -> Unit)

/** A section title in the reference's style: semibold title, its buttons ("Stash", "Clear") on the right. */
@Composable
private fun SectionTitle(title: String, tag: String, buttons: List<SectionButton> = emptyList()) {
    Row(Modifier.fillMaxWidth().heightIn(min = 48.dp).padding(start = 16.dp, end = 4.dp, top = 12.dp, bottom = 4.dp).testTag(tag), verticalAlignment = Alignment.CenterVertically) {
        Column(Modifier.weight(1f).semantics(mergeDescendants = true) { heading() }) {
            Text(title, style = MaterialTheme.typography.titleMedium, fontWeight = FontWeight.SemiBold, color = MaterialTheme.colorScheme.onSurface)
        }
        buttons.forEach { b ->
            TextButton(onClick = b.onClick, modifier = Modifier.testTag(b.tag).semantics { contentDescription = b.spoken }) {
                Text(b.label, color = MaterialTheme.colorScheme.primary)
            }
        }
    }
}

/**
 * One queue row: 48 dp artwork, title and artist, the row's marker (the queue icon for a song you
 * added, outside the "Next in queue" group; autoplay's icon for one autoplay picked) and a drag
 * handle for reorderable items; no opaque container. It speaks like a track row (label, "playing"
 * / offline state, row menu as actions) with the queue's remove / move actions added. Long press
 * starts multi-select.
 */
@Composable
private fun QueueRow(
    entry: QueueEntry,
    section: Section,
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
    val current = section == Section.Current
    val marker = rowMarker(entry, section)
    val actions = trackRowActions(track, Commands.queueItems(listOf(entry.item.key)), onRate = onRate, onMore = onMore, extra = extraActions)
    val markerText = when (marker) {
        RowMarker.Added -> stringResource(R.string.queue_row_added)
        RowMarker.Autoplay -> stringResource(R.string.queue_row_autoplay)
        RowMarker.None -> null
    }
    val state = listOfNotNull(if (current) stringResource(R.string.row_state_playing) else null, offlineStateText(track.offline), markerText).joinToString(", ").ifEmpty { null }
    val label = trackLabel(track)
    val selectLabel = stringResource(R.string.a11y_select)
    val deselectLabel = stringResource(R.string.a11y_deselect)
    val playLabel = stringResource(R.string.action_play)
    val content = LocalContentColor.current
    val wash by animateColorAsState(if (selected) content.copy(alpha = 0.14f) else Color.Transparent, label = "queueRowSelected")
    val snoozed = LocalCoreClient.current.snoozes.collectAsStateWithLifecycle().value.any { it.trackId == track.id }
    val dim = (if (section == Section.History) 0.7f else 1f) * (if (snoozed) 0.45f else 1f)
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
                if (selected) Surface(shape = CircleShape, color = MaterialTheme.colorScheme.primary) { Icon(HocketIcons.Filled.Check, null, Modifier.padding(4.dp), tint = MaterialTheme.colorScheme.onPrimary) }
            }
            Spacer(Modifier.width(14.dp))
            Column(Modifier.weight(1f)) {
                Row(verticalAlignment = Alignment.CenterVertically) {
                    if (current) {
                        Icon(HocketIcons.Filled.GraphicEq, null, tint = MaterialTheme.colorScheme.primary, modifier = Modifier.size(16.dp))
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
            // The marker sits before the duration, muted: it says where the song came from.
            val muted = MaterialTheme.colorScheme.onSurfaceVariant.let { it.copy(alpha = it.alpha * 0.7f * dim) }
            when (marker) {
                RowMarker.Added -> Icon(HocketIcons.AutoMirrored.Filled.QueueMusic, null, Modifier.padding(start = 8.dp).size(16.dp).testTag("queue.added.${entry.item.key}"), tint = muted)
                RowMarker.Autoplay -> Icon(HocketIcons.Filled.AllInclusive, null, Modifier.padding(start = 8.dp).size(16.dp).testTag("queue.autoplay.${entry.item.key}"), tint = muted)
                RowMarker.None -> Unit
            }
            Text(formatClock(track.durationMs), style = MaterialTheme.typography.labelMedium, maxLines = 1, color = muted,
                modifier = Modifier.padding(start = 6.dp).clearAndSetSemantics { })
            handle?.invoke()
        }
        if (entry.item.unavailable == true) Text(stringResource(R.string.queue_unavailable), style = MaterialTheme.typography.labelSmall, color = MaterialTheme.colorScheme.error, modifier = Modifier.padding(start = 78.dp, bottom = 6.dp))
        (entry.item.source as? QueueSource.Autoplay)?.data?.reason?.let {
            Text(stringResource(R.string.player_autoplay_reason, it), style = MaterialTheme.typography.labelSmall, color = MaterialTheme.colorScheme.tertiary, modifier = Modifier.padding(start = 78.dp, bottom = 6.dp))
        }
    }
}
