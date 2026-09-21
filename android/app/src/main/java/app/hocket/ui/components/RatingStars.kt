package app.hocket.ui.components

import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.size
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Star
import androidx.compose.material.icons.outlined.StarOutline
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.hapticfeedback.HapticFeedbackType
import androidx.compose.ui.platform.LocalHapticFeedback
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import app.hocket.R

/** Five integer stars (Subsonic has no half stars). Tapping the current rating clears it. */
@Composable
fun RatingStars(rating: Int, onRate: (Int) -> Unit, modifier: Modifier = Modifier, starSize: Dp = 28.dp, tint: Color = MaterialTheme.colorScheme.primary) {
    val haptics = LocalHapticFeedback.current
    val desc = stringResource(R.string.rating_stars, rating)
    Row(modifier.semantics { contentDescription = desc }) {
        for (i in 1..5) {
            val filled = i <= rating
            val label = if (filled && i == rating) stringResource(R.string.rating_clear) else stringResource(R.string.rating_set, i)
            IconButton(onClick = { haptics.performHapticFeedback(HapticFeedbackType.Confirm); onRate(if (i == rating) 0 else i) }, modifier = Modifier.size(starSize + 12.dp)) {
                Icon(if (filled) Icons.Filled.Star else Icons.Outlined.StarOutline, contentDescription = label, tint = if (filled) tint else MaterialTheme.colorScheme.outline, modifier = Modifier.size(starSize))
            }
        }
    }
}
