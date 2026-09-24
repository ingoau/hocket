package app.hocket.ui.components

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.selection.selectableGroup
import androidx.compose.material3.Text
import androidx.compose.material3.ToggleButton
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
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
