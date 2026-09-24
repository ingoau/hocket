package app.hocket.ui.components

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.selection.selectableGroup
import androidx.compose.material3.Text
import androidx.compose.material3.ToggleButton
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.ui.semantics.ProgressBarRangeInfo
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.progressBarRangeInfo
import androidx.compose.ui.semantics.setProgress
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.unit.dp

/**
 * A set of mutually exclusive options as toggle buttons that WRAP onto further lines when they do
 * not fit. It replaces `ButtonGroup(overflowIndicator = {})`, which moves whatever does not fit into
 * an overflow menu, and with an empty indicator simply hid options at large font sizes or narrow
 * widths. [describe] gives a spoken label for options whose visible label is a symbol ("●", "3★").
 */
@Composable
fun <T> ChoiceRow(
    options: List<Pair<T, String>>,
    isSelected: (T) -> Boolean,
    onSelect: (T) -> Unit,
    modifier: Modifier = Modifier,
    describe: ((T) -> String?)? = null,
) {
    FlowRow(modifier.selectableGroup(), horizontalArrangement = Arrangement.spacedBy(6.dp), verticalArrangement = Arrangement.spacedBy(4.dp)) {
        options.forEach { (value, label) ->
            val spoken = describe?.invoke(value)
            ToggleButton(
                checked = isSelected(value),
                onCheckedChange = { onSelect(value) },
                modifier = if (spoken != null) Modifier.semantics { contentDescription = spoken } else Modifier,
            ) { Text(label, maxLines = 1) }
        }
    }
}

/** Plain actions laid out like [ChoiceRow] (wrapping, never hidden). */
@Composable
fun ActionRow(actions: List<Pair<String, () -> Unit>>, modifier: Modifier = Modifier, describe: ((Int) -> String?)? = null) {
    FlowRow(modifier, horizontalArrangement = Arrangement.spacedBy(6.dp), verticalArrangement = Arrangement.spacedBy(4.dp)) {
        actions.forEachIndexed { i, (label, onClick) ->
            val spoken = describe?.invoke(i)
            androidx.compose.material3.OutlinedButton(onClick = onClick, modifier = if (spoken != null) Modifier.semantics { contentDescription = spoken } else Modifier) { Text(label, maxLines = 1) }
        }
    }
}

/**
 * A slider with a spoken label and value ("Preamp, +3.0 dB" rather than a bare percentage) in a
 * 48 dp tall slot. The expressive slider itself is 44 dp, below the minimum target, so the slot is
 * the accessibility node: it carries the range and the adjust action (snapped to [steps]) and the
 * slider inside is drawn and dragged only. [showTicks] false draws a continuous track for a long
 * range (dozens of tick dots are noise) while the adjust action still snaps to [steps].
 */
@Composable
fun LabelledSlider(
    value: Float,
    onValueChange: (Float) -> Unit,
    label: String,
    valueText: String,
    modifier: Modifier = Modifier,
    valueRange: ClosedFloatingPointRange<Float> = 0f..1f,
    steps: Int = 0,
    onValueChangeFinished: (() -> Unit)? = null,
    showTicks: Boolean = true,
) {
    androidx.compose.foundation.layout.Box(
        modifier.heightIn(min = 48.dp).clearAndSetSemantics {
            contentDescription = label
            stateDescription = valueText
            progressBarRangeInfo = ProgressBarRangeInfo(value.coerceIn(valueRange), valueRange, steps)
            setProgress(label) { target ->
                var v = target.coerceIn(valueRange)
                if (steps > 0) {
                    val step = (valueRange.endInclusive - valueRange.start) / (steps + 1)
                    v = valueRange.start + kotlin.math.round((v - valueRange.start) / step) * step
                }
                onValueChange(v)
                onValueChangeFinished?.invoke()
                true
            }
        },
        contentAlignment = androidx.compose.ui.Alignment.Center,
    ) {
        androidx.compose.material3.Slider(value = value, onValueChange = onValueChange, valueRange = valueRange, steps = if (showTicks) steps else 0, onValueChangeFinished = onValueChangeFinished, modifier = Modifier.fillMaxWidth())
    }
}
