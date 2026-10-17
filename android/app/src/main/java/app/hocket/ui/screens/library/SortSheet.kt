package app.hocket.ui.screens.library

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.selection.selectable
import androidx.compose.foundation.selection.selectableGroup
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.ButtonGroupDefaults
import androidx.compose.material3.ExperimentalMaterial3ExpressiveApi
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.ToggleButton
import androidx.compose.material3.ToggleButtonDefaults
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.role
import androidx.compose.ui.semantics.selected
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import app.hocket.R
import app.hocket.ui.components.connectedShape
import app.hocket.ui.components.menuRowColor
import app.hocket.ui.icons.HocketIcons

/** The words for a sort's current direction ("Newest", "Z–A"). */
@Composable
fun directionLabel(sort: ListSort): String = stringResource(if (sort.descending) sort.key.words.descending else sort.key.words.ascending)

/**
 * A library list's sort control: a tonal chip naming the current sort and its direction
 * ("Recently added ↓") that opens the [SortSheet]. Tag `library.sort`.
 */
@Composable
internal fun SortChip(sort: ListSort, options: List<SortKey>, onSort: (ListSort) -> Unit, modifier: Modifier = Modifier) {
    var open by remember { mutableStateOf(false) }
    val label = stringResource(sort.key.label)
    val spoken = stringResource(R.string.sort_chip_a11y, label, directionLabel(sort))
    Surface(
        onClick = { open = true },
        shape = CircleShape,
        color = MaterialTheme.colorScheme.secondaryContainer,
        contentColor = MaterialTheme.colorScheme.onSecondaryContainer,
        modifier = modifier.testTag("library.sort").semantics { contentDescription = spoken; role = Role.Button },
    ) {
        Row(Modifier.heightIn(min = 36.dp).padding(start = 14.dp, end = 12.dp), verticalAlignment = Alignment.CenterVertically) {
            Text(label, style = MaterialTheme.typography.labelLarge, maxLines = 1, overflow = TextOverflow.Ellipsis)
            if (sort.key.hasDirection) {
                Spacer(Modifier.width(4.dp))
                Icon(if (sort.descending) HocketIcons.Filled.ArrowDownward else HocketIcons.Filled.ArrowUpward, null, Modifier.size(18.dp))
            }
        }
    }
    if (open) SortSheet(sort, options, onSort, onDismiss = { open = false })
}

/**
 * The sort sheet: "Sort by", the list's orders as one connected group (single choice, the chosen one
 * tinted and checked), then the direction as a connected button group whose words follow the order
 * ("A–Z" / "Z–A", "Oldest" / "Newest", "Fewest" / "Most"). Everything applies at once and the sheet
 * stays open, so an order and its direction can be picked in one visit. Tags: `library.sortSheet`,
 * `library.sort.option.<id>`, `library.sort.ascending`, `library.sort.descending`.
 */
@OptIn(ExperimentalMaterial3ExpressiveApi::class)
@Composable
internal fun SortSheet(sort: ListSort, options: List<SortKey>, onSort: (ListSort) -> Unit, onDismiss: () -> Unit) {
    val sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = true)
    ModalBottomSheet(onDismissRequest = onDismiss, sheetState = sheetState, modifier = Modifier.testTag("library.sortSheet")) {
        Column(Modifier.verticalScroll(rememberScrollState()).navigationBarsPadding().padding(bottom = 24.dp)) {
            Text(
                stringResource(R.string.sort_by), style = MaterialTheme.typography.titleLarge, fontWeight = FontWeight.SemiBold,
                modifier = Modifier.padding(start = 24.dp, end = 24.dp, bottom = 16.dp).semantics { heading() },
            )
            Column(Modifier.fillMaxWidth().padding(horizontal = 16.dp).selectableGroup(), verticalArrangement = Arrangement.spacedBy(2.dp)) {
                options.forEachIndexed { i, key ->
                    SortOptionRow(key, selected = key == sort.key, shape = connectedShape(i, options.size)) { onSort(sort.choose(key)) }
                }
            }
            DirectionGroup(sort, onSort, Modifier.padding(start = 16.dp, end = 16.dp, top = 16.dp))
        }
    }
}

/** One order: its icon, its name and, when chosen, a check; announced as a radio button, "selected". */
@Composable
private fun SortOptionRow(key: SortKey, selected: Boolean, shape: androidx.compose.ui.graphics.Shape, onClick: () -> Unit) {
    val scheme = MaterialTheme.colorScheme
    Box(
        Modifier
            .fillMaxWidth()
            .clip(shape)
            .background(if (selected) scheme.secondaryContainer else menuRowColor())
            .selectable(selected = selected, role = Role.RadioButton, onClick = onClick)
            .testTag("library.sort.option.${key.id}"),
    ) {
        Row(Modifier.heightIn(min = 52.dp).padding(horizontal = 16.dp, vertical = 4.dp), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(16.dp)) {
            Icon(key.icon, null, tint = if (selected) scheme.onSecondaryContainer else scheme.onSurfaceVariant)
            Text(
                stringResource(key.label), style = MaterialTheme.typography.bodyLarge, fontWeight = if (selected) FontWeight.SemiBold else null,
                color = if (selected) scheme.onSecondaryContainer else scheme.onSurface, maxLines = 1, overflow = TextOverflow.Ellipsis, modifier = Modifier.weight(1f),
            )
            // Reserved either way, so the label never shifts as the choice moves.
            Box(Modifier.size(20.dp)) { if (selected) Icon(HocketIcons.Filled.Check, null, Modifier.size(20.dp), tint = scheme.onSecondaryContainer) }
        }
    }
}

/**
 * Ascending / descending as two connected toggle buttons with arrows, labelled for the order. A
 * shuffle has no direction: the group stays, greyed out, so the sheet doesn't jump.
 */
@OptIn(ExperimentalMaterial3ExpressiveApi::class)
@Composable
private fun DirectionGroup(sort: ListSort, onSort: (ListSort) -> Unit, modifier: Modifier = Modifier) {
    val groupLabel = stringResource(R.string.sort_direction)
    Row(
        modifier.fillMaxWidth().selectableGroup().semantics { contentDescription = groupLabel },
        horizontalArrangement = Arrangement.spacedBy(ButtonGroupDefaults.ConnectedSpaceBetween),
    ) {
        listOf(false, true).forEach { descending ->
            val words = sort.key.words
            val checked = sort.key.hasDirection && sort.descending == descending
            ToggleButton(
                checked = checked,
                onCheckedChange = { onSort(sort.copy(descending = descending)) },
                enabled = sort.key.hasDirection,
                shapes = if (descending) ButtonGroupDefaults.connectedTrailingButtonShapes() else ButtonGroupDefaults.connectedLeadingButtonShapes(),
                // The unchecked fill a step up from the sheet's, as the item menu's rows are.
                colors = ToggleButtonDefaults.colors(containerColor = menuRowColor()),
                modifier = Modifier.weight(1f).heightIn(min = 56.dp).semantics { role = Role.RadioButton; selected = checked }.testTag(if (descending) "library.sort.descending" else "library.sort.ascending"),
            ) {
                Icon(if (descending) HocketIcons.Filled.ArrowDownward else HocketIcons.Filled.ArrowUpward, null, Modifier.size(20.dp).clearAndSetSemantics { })
                Spacer(Modifier.width(8.dp))
                Text(stringResource(if (descending) words.descending else words.ascending), maxLines = 1, overflow = TextOverflow.Ellipsis)
            }
        }
    }
}
