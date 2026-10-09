package app.hocket.ui.components

import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.size
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Star
import androidx.compose.material.icons.outlined.StarOutline
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
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
import kotlin.math.roundToInt

/**
 * Five integer stars (Subsonic has no half stars). Tapping the current rating clears it.
 *
 * For a screen reader the stars are ONE adjustable control, "Rating, 3 of 5 stars": swipe up/down
 * (or the volume keys) steps it through 0..5 (`setProgress` over a 0..5 range with whole steps), and
 * a "Clear rating" action resets it. The single node is at least 48 dp tall; the individual stars
 * are pointer targets only.
 */
@Composable
fun RatingStars(rating: Int, onRate: (Int) -> Unit, modifier: Modifier = Modifier, starSize: Dp = 28.dp, tint: Color = MaterialTheme.colorScheme.primary) {
    val haptics = LocalHapticFeedback.current
    val resources = androidx.compose.ui.platform.LocalResources.current
    val label = stringResource(R.string.rating_stars)
    val state = Spoken.rating(resources, rating)
    val clear = stringResource(R.string.rating_clear)
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
            },
        verticalAlignment = Alignment.CenterVertically,
    ) {
        for (i in 1..5) {
            val filled = i <= rating
            IconButton(onClick = { haptics.performHapticFeedback(HapticFeedbackType.Confirm); onRate(if (i == rating) 0 else i) }, modifier = Modifier.size(starSize + 12.dp)) {
                Icon(if (filled) Icons.Filled.Star else Icons.Outlined.StarOutline, contentDescription = null, tint = if (filled) tint else MaterialTheme.colorScheme.outline, modifier = Modifier.size(starSize))
            }
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
