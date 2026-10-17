package app.hocket.ui.components

import androidx.compose.foundation.gestures.detectHorizontalDragGestures
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.size
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.setValue
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.hapticfeedback.HapticFeedbackType
import androidx.compose.ui.platform.LocalHapticFeedback
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.CustomAccessibilityAction
import androidx.compose.ui.semantics.ProgressBarRangeInfo
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.customActions
import androidx.compose.ui.semantics.progressBarRangeInfo
import androidx.compose.ui.semantics.setProgress
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import app.hocket.R
import app.hocket.ui.a11y.Spoken
import app.hocket.ui.icons.HocketIcons
import kotlin.math.roundToInt

/**
 * Five integer stars (Subsonic has no half stars). Tapping the current rating clears it; dragging
 * along the row sets the rating under the finger (a tick per star as it changes, committed on
 * release; dragging left of the first star clears it).
 *
 * For a screen reader the stars are ONE adjustable control, "Rating, 3 of 5 stars": swipe up/down
 * (or the volume keys) steps it through 0..5 (`setProgress` over a 0..5 range with whole steps), and
 * a "Clear rating" action resets it. The single node is at least 48 dp tall; the individual stars
 * are pointer targets only. [spacing] is the gap between stars (their touch cells share it);
 * [verticalAlignment] places them in that 48 dp row (the player tops them, so the row's touch area
 * hangs below the stars rather than spacing them off the text above). [stateOverride] replaces
 * the spoken state (several songs rated differently read "Mixed", not "Not rated").
 */
@Composable
fun RatingStars(
    rating: Int,
    onRate: (Int) -> Unit,
    modifier: Modifier = Modifier,
    starSize: Dp = 28.dp,
    tint: Color = MaterialTheme.colorScheme.primary,
    spacing: Dp = 12.dp,
    emptyTint: Color = MaterialTheme.colorScheme.outline,
    verticalAlignment: Alignment.Vertical = Alignment.CenterVertically,
    stateOverride: String? = null,
) {
    val haptics = LocalHapticFeedback.current
    val resources = androidx.compose.ui.platform.LocalResources.current
    val label = stringResource(R.string.rating_stars)
    // While a finger drags along the row: the rating under it (shown, not yet sent).
    var dragged by remember { mutableStateOf<Int?>(null) }
    val shown = dragged ?: rating
    val state = stateOverride ?: Spoken.rating(resources, rating)
    val clear = stringResource(R.string.rating_clear)
    val current by rememberUpdatedState(rating)
    val rate by rememberUpdatedState(onRate)
    val density = LocalDensity.current
    val cellPx = with(density) { (starSize + spacing).toPx() }
    val halfGapPx = with(density) { (spacing / 2).toPx() }
    /** The rating a finger at [x] (px from the row's start) means: the star under it, 0 left of the first. */
    fun valueAt(x: Float): Int = (kotlin.math.floor((x + halfGapPx) / cellPx).toInt() + 1).coerceIn(0, 5).let { if (x < -halfGapPx) 0 else it }
    Row(
        modifier
            .heightIn(min = 48.dp)
            .testTag("rating")
            .clearAndSetSemantics {
                contentDescription = label
                stateDescription = state
                progressBarRangeInfo = ProgressBarRangeInfo(rating.toFloat(), 0f..5f, steps = 4)
                setProgress(label) { value ->
                    val stars = value.roundToInt().coerceIn(0, 5)
                    if (stars != rating) onRate(stars)
                    true
                }
                if (rating > 0) customActions = listOf(CustomAccessibilityAction(clear) { onRate(0); true })
            }
            .pointerInput(cellPx) {
                detectTapGestures { o ->
                    val i = valueAt(o.x).coerceAtLeast(1)
                    haptics.performHapticFeedback(HapticFeedbackType.Confirm)
                    rate(if (i == current) 0 else i)
                }
            }
            .pointerInput(cellPx) {
                // What the drag rates: the item (and its rating) it began on, even if another takes
                // its place under the finger (the player moving on to the next song mid-drag).
                var target = rate
                var from = current
                detectHorizontalDragGestures(
                    onDragStart = { o ->
                        target = rate
                        from = current
                        dragged = valueAt(o.x)
                        haptics.performHapticFeedback(HapticFeedbackType.GestureThresholdActivate)
                    },
                    onDragEnd = {
                        dragged?.let { if (it != from) target(it) }
                        dragged = null
                        haptics.performHapticFeedback(HapticFeedbackType.GestureEnd)
                    },
                    onDragCancel = { dragged = null },
                ) { change, _ ->
                    change.consume()
                    val v = valueAt(change.position.x)
                    if (v != dragged) { dragged = v; haptics.performHapticFeedback(HapticFeedbackType.SegmentTick) }
                }
            },
        horizontalArrangement = Arrangement.spacedBy(spacing),
        verticalAlignment = verticalAlignment,
    ) {
        for (i in 1..5) {
            val filled = i <= shown
            Icon(if (filled) HocketIcons.Filled.Star else HocketIcons.Outlined.StarOutline, contentDescription = null, tint = if (filled) tint else emptyTint, modifier = Modifier.size(starSize))
        }
    }
}

/**
 * "Rate" from a menu or an accessibility action: the stars in a dialog; picking one closes it.
 * [offerClear] adds a "Clear rating" button (rates 0), for when tapping the current stars cannot.
 */
@Composable
fun RatingDialog(current: Int, onRate: (Int) -> Unit, onDismiss: () -> Unit, title: String = stringResource(R.string.action_rate), offerClear: Boolean = false) {
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text(title) },
        text = { RatingStars(rating = current, onRate = onRate) },
        confirmButton = { TextButton(onClick = onDismiss) { Text(stringResource(R.string.action_cancel)) } },
        dismissButton = if (offerClear) { { TextButton(onClick = { onRate(0) }, modifier = Modifier.testTag("rating.clear")) { Text(stringResource(R.string.rating_clear)) } } } else null,
    )
}
