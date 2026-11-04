package app.hocket.ui.components

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.RowScope
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.ListItemColors
import androidx.compose.material3.ListItemDefaults
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Shape
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import app.hocket.ui.theme.LocalDarkTheme

/**
 * M3 Expressive segmented groups: rows [ListItemDefaults.SegmentedGap] apart, the group's outer
 * corners large ([SegmentOuter]) and the corners between rows extra small ([SegmentInner]), the
 * shape [ListItemDefaults.segmentedShapes] gives a [androidx.compose.material3.SegmentedListItem].
 * This is the same shape for rows that are not list items (cards with a progress bar, chart tiles).
 */
fun segmentedShape(index: Int, count: Int, outer: Dp = SegmentOuter, inner: Dp = SegmentInner): Shape = when {
    count <= 1 -> RoundedCornerShape(outer)
    index == 0 -> RoundedCornerShape(topStart = outer, topEnd = outer, bottomStart = inner, bottomEnd = inner)
    index == count - 1 -> RoundedCornerShape(topStart = inner, topEnd = inner, bottomStart = outer, bottomEnd = outer)
    else -> RoundedCornerShape(inner)
}

val SegmentOuter = 20.dp
val SegmentInner = 4.dp

/**
 * A segment's container. M3's segmented token is `surface`, which vanishes on our `surface` screens;
 * the boards lift the rows a step instead (the lowest container in light, a higher one in dark).
 */
@Composable
fun segmentColor(): Color = if (LocalDarkTheme.current) MaterialTheme.colorScheme.surfaceContainerHigh else MaterialTheme.colorScheme.surfaceContainerLowest

/** [ListItemDefaults.segmentedColors] on [segmentColor]. */
@Composable
fun segmentedListColors(container: Color = segmentColor()): ListItemColors = ListItemDefaults.segmentedColors(containerColor = container)

/** The rows of one segmented group, 2dp apart. */
@Composable
fun SegmentedGroup(modifier: Modifier = Modifier, content: @Composable ColumnScope.() -> Unit) {
    Column(modifier, verticalArrangement = Arrangement.spacedBy(ListItemDefaults.SegmentedGap), content = content)
}

/** One segment that is not a list item: a lifted surface with the group's shape for [index] of [count]. */
@Composable
fun Segment(
    index: Int,
    count: Int,
    modifier: Modifier = Modifier,
    color: Color = segmentColor(),
    onClick: (() -> Unit)? = null,
    content: @Composable () -> Unit,
) {
    val shape = segmentedShape(index, count)
    if (onClick != null) Surface(onClick = onClick, shape = shape, color = color, modifier = modifier.fillMaxWidth(), content = content)
    else Surface(shape = shape, color = color, modifier = modifier.fillMaxWidth(), content = content)
}

/**
 * The small label over a group ("Playing now", "Pinned", "Look and sound"): title small, a little
 * inset from the group's edge, with room for an [action] on the right.
 */
@Composable
fun GroupLabel(
    text: String,
    modifier: Modifier = Modifier,
    color: Color = MaterialTheme.colorScheme.onSurfaceVariant,
    action: (@Composable RowScope.() -> Unit)? = null,
) {
    Row(modifier.fillMaxWidth().heightIn(min = 40.dp).padding(start = 8.dp, end = 0.dp), verticalAlignment = Alignment.CenterVertically) {
        Text(text, style = MaterialTheme.typography.titleSmall, fontWeight = FontWeight.SemiBold, color = color, modifier = Modifier.weight(1f).semantics { heading() })
        action?.invoke(this)
    }
}

/** A clickable full-width row inside a [Segment], with the list item's padding. */
@Composable
fun SegmentRow(onClick: (() -> Unit)?, modifier: Modifier = Modifier, minHeight: Dp = 64.dp, content: @Composable RowScope.() -> Unit) {
    val base = modifier.fillMaxWidth().heightIn(min = minHeight)
    Row(
        (if (onClick != null) base.clickable(onClick = onClick) else base).padding(start = 16.dp, end = 8.dp, top = 8.dp, bottom = 8.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(12.dp),
        content = content,
    )
}
