package app.hocket.ui.components

import androidx.compose.animation.animateColorAsState
import androidx.compose.animation.core.animateDpAsState
import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.animation.core.tween
import androidx.compose.foundation.background
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.foundation.gestures.drag
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.systemGestureExclusion
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyListState
import androidx.compose.foundation.lazy.grid.LazyGridState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material3.MaterialTheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.derivedStateOf
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.hapticfeedback.HapticFeedbackType
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.input.pointer.positionChange
import androidx.compose.ui.layout.onSizeChanged
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.LocalHapticFeedback
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.IntOffset
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.delay
import kotlin.math.floor
import kotlin.math.roundToInt

/**
 * A long list's scroll thumb, which drags. It shows at the list's end edge while the list scrolls
 * and fades a moment after it stops; while it shows, pressing it and dragging takes the list with
 * the finger, end to end in one sweep (a playlist of thousands of songs, the whole library). Hidden,
 * it takes no touches, so the rows under it work as ever. Only a list of a few screens or more has
 * one (like the platform's own fast scroll).
 *
 * Sits in a Box over the list, filling its height; [top] and [bottom] keep it clear of the bars over
 * the list's ends.
 */
@Composable
fun FastScrollbar(state: LazyListState, modifier: Modifier = Modifier, top: Dp = 0.dp, bottom: Dp = 0.dp) {
    val geometry = remember(state) { { listGeometry(state) } }
    FastScrollbar(geometry, state.isScrollInProgress, { unit, offset -> state.requestScrollToItem(unit, offset) }, modifier, top, bottom)
}

/** [FastScrollbar] for a vertical grid: it moves a row at a time, the way the grid lays out. */
@Composable
fun FastScrollbar(state: LazyGridState, modifier: Modifier = Modifier, top: Dp = 0.dp, bottom: Dp = 0.dp) {
    val geometry = remember(state) { { gridGeometry(state) } }
    FastScrollbar(geometry, state.isScrollInProgress, { index, offset -> state.requestScrollToItem(index, offset) }, modifier, top, bottom)
}

/**
 * Where a list is, in units: items for a list, rows for a grid. [above] is how many units have
 * scrolled off the top (the first visible one in part), [below] how many are still to come under the
 * bottom; [unitPx] is a typical unit's size (the median of the ones on screen, so a page's tall header
 * doesn't skew it), [index] turns a unit back into the item to scroll to, [sizeOf] is a unit's own
 * size while it's on screen.
 */
internal class ScrollGeometry(
    val above: Float,
    val below: Float,
    val unitPx: Int,
    val totalUnits: Int,
    val viewportPx: Int,
    val index: (unit: Int) -> Int,
    val sizeOf: (unit: Int) -> Int?,
) {
    /** How far down the list is, 0 at the top and 1 at the bottom. */
    val fraction: Float get() = if (above + below <= 0f) 0f else (above / (above + below)).coerceIn(0f, 1f)

    /** Four screens or more, like the platform's fast scroll. */
    val long: Boolean get() = unitPx > 0 && totalUnits.toLong() * unitPx >= LONG_SCREENS * viewportPx.toLong()

    /** The item and pixel offset that put the list at [fraction] down. */
    fun target(fraction: Float): Pair<Int, Int> {
        if (fraction >= 1f) return index(totalUnits - 1) to 0
        val at = fraction.coerceAtLeast(0f) * (above + below)
        val unit = floor(at).toInt().coerceIn(0, (totalUnits - 1).coerceAtLeast(0))
        // Into a unit on screen by its own size (a page's tall header), others by the typical one.
        return index(unit) to ((at - unit) * (sizeOf(unit) ?: unitPx)).roundToInt()
    }
}

internal fun listGeometry(state: LazyListState): ScrollGeometry? {
    val info = state.layoutInfo
    val items = info.visibleItemsInfo
    val total = info.totalItemsCount
    if (items.isEmpty() || total == 0) return null
    val first = items.firstOrNull { it.index == state.firstVisibleItemIndex } ?: items.first()
    val last = items.last()
    val above = if (!state.canScrollBackward) 0f else first.index + (state.firstVisibleItemScrollOffset.toFloat() / first.size.coerceAtLeast(1)).coerceIn(0f, 1f)
    val contentEnd = info.viewportEndOffset - info.afterContentPadding
    val hiddenBelow = ((last.offset + last.size - contentEnd).toFloat() / last.size.coerceAtLeast(1)).coerceIn(0f, 1f)
    val below = if (!state.canScrollForward) 0f else (total - 1 - last.index) + hiddenBelow
    return ScrollGeometry(above, below, median(items.map { it.size }), total, info.viewportSize.height,
        index = { it }, sizeOf = { unit -> items.firstOrNull { it.index == unit }?.size })
}

internal fun gridGeometry(state: LazyGridState): ScrollGeometry? {
    val info = state.layoutInfo
    val items = info.visibleItemsInfo
    val total = info.totalItemsCount
    if (items.isEmpty() || total == 0) return null
    val columns = info.maxSpan.coerceAtLeast(1)
    val rows = items.groupBy { it.row }
    val first = items.firstOrNull { it.index == state.firstVisibleItemIndex } ?: items.first()
    val last = items.last()
    val above = if (!state.canScrollBackward) 0f else first.index.toFloat() / columns + (state.firstVisibleItemScrollOffset.toFloat() / first.size.height.coerceAtLeast(1)).coerceIn(0f, 1f)
    val contentEnd = info.viewportEndOffset - info.afterContentPadding
    val hiddenBelow = ((last.offset.y + last.size.height - contentEnd).toFloat() / last.size.height.coerceAtLeast(1)).coerceIn(0f, 1f)
    val below = if (!state.canScrollForward) 0f else (total - 1 - last.index).toFloat() / columns + hiddenBelow
    val totalRows = (total + columns - 1) / columns
    return ScrollGeometry(above, below, median(rows.values.map { row -> row.maxOf { it.size.height } }), totalRows, info.viewportSize.height,
        index = { (it * columns).coerceAtMost(total - 1) }, sizeOf = { unit -> items.firstOrNull { it.index == unit * columns }?.size?.height })
}

private fun median(sizes: List<Int>): Int = sizes.sorted().let { if (it.isEmpty()) 0 else it[it.size / 2] }

@Composable
private fun FastScrollbar(
    geometry: () -> ScrollGeometry?,
    scrolling: Boolean,
    scrollTo: (index: Int, offsetPx: Int) -> Unit,
    modifier: Modifier,
    top: Dp,
    bottom: Dp,
) {
    // Worked out once per layout; the thumb's place is read only while laying out (no recomposition).
    val now by remember(geometry) { derivedStateOf(geometry) }
    val long by remember { derivedStateOf { now?.long == true } }
    var dragging by remember { mutableStateOf(false) }
    // While dragged the thumb is wherever the finger put it, not where the list's estimate says.
    var dragFraction by remember { mutableFloatStateOf(0f) }
    val fraction by remember { derivedStateOf { now?.fraction ?: 0f } }
    var shown by remember { mutableStateOf(false) }
    LaunchedEffect(scrolling, dragging) {
        if (scrolling || dragging) shown = true
        else if (shown) { delay(HIDE_AFTER_MS); shown = false }
    }
    if (!long && !dragging) return
    // A grab as the hide fires keeps it: the thumb shows (and takes touches) for as long as it's held.
    val visible = shown || dragging
    val haptics = LocalHapticFeedback.current
    val alpha by animateFloatAsState(if (visible) 1f else 0f, tween(if (visible) 150 else 400), label = "fastScroll.alpha")
    val width by animateDpAsState(if (dragging) 10.dp else 6.dp, label = "fastScroll.width")
    val color by animateColorAsState(if (dragging) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.onSurfaceVariant.copy(alpha = 0.7f), label = "fastScroll.color")
    var trackPx by remember { mutableIntStateOf(0) }
    val thumbPx = with(LocalDensity.current) { ThumbHeight.roundToPx() }
    val range = (trackPx - thumbPx).coerceAtLeast(1)
    Box(modifier.fillMaxHeight().padding(top = top, bottom = bottom).width(TouchWidth).onSizeChanged { trackPx = it.height }) {
        Box(
            Modifier.align(Alignment.TopEnd)
                .offset { IntOffset(0, ((if (dragging) dragFraction else fraction) * range).roundToInt()) }
                .size(TouchWidth, ThumbHeight)
                .graphicsLayer { this.alpha = alpha }
                // Only a thumb that shows can be grabbed: hidden, a touch here goes to the row under it.
                // Showing, it's kept out of the back gesture at that edge.
                .then(if (visible) Modifier.systemGestureExclusion().pointerInput(range) {
                    awaitEachGesture {
                        val down = awaitFirstDown()
                        down.consume()
                        var y = fraction * range
                        dragFraction = fraction
                        dragging = true
                        haptics.performHapticFeedback(HapticFeedbackType.GestureThresholdActivate)
                        try {
                            drag(down.id) { change ->
                                y += change.positionChange().y
                                change.consume()
                                dragFraction = (y / range).coerceIn(0f, 1f)
                                geometry()?.target(dragFraction)?.let { (index, offset) -> scrollTo(index, offset) }
                            }
                        } finally {
                            dragging = false
                            haptics.performHapticFeedback(HapticFeedbackType.GestureEnd)
                        }
                    }
                } else Modifier)
                .testTag("fastScroll.thumb"),
        ) {
            Box(Modifier.align(Alignment.CenterEnd).padding(end = 4.dp).size(width, ThumbHeight).background(color, CircleShape))
        }
    }
}

/** How long the thumb stays once the list stops. */
private const val HIDE_AFTER_MS = 1500L

/** How many screens a list needs before it has a thumb. */
private const val LONG_SCREENS = 4

private val ThumbHeight = 48.dp

/**
 * The thumb's touch width from the edge: wide enough to grab, narrow enough to stay (mostly) off a
 * row's trailing More button, whose target starts 16 dp in on a library row and 20 dp in on a
 * playlist's.
 */
private val TouchWidth = 20.dp
