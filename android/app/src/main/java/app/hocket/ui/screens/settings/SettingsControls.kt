package app.hocket.ui.screens.settings

import androidx.compose.animation.animateColorAsState
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.IntrinsicSize
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.selection.selectable
import androidx.compose.foundation.selection.selectableGroup
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.RadioButton
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import app.hocket.R
import app.hocket.core.api.SettingScope

/**
 * One choice of an [OptionCards] row: its value, its name, and what it looks like ([preview], drawn
 * in the card's preview well; told whether it is the picked one, to colour itself).
 */
class VisualOption<T>(val value: T, val label: String, val preview: @Composable (selected: Boolean) -> Unit)

/** A [VisualOption] whose preview is just an icon. */
fun <T> iconOption(value: T, label: String, icon: ImageVector): VisualOption<T> =
    VisualOption(value, label) { selected -> PreviewIcon(icon, selected) }

@Composable
fun PreviewIcon(icon: ImageVector, selected: Boolean) {
    val tint by animateColorAsState(if (selected) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.onSurfaceVariant, label = "previewIcon")
    Icon(icon, null, tint = tint, modifier = Modifier.size(28.dp))
}

/**
 * Mutually exclusive choices as cards side by side, each a picture of what it does over its name:
 * the picked one is outlined in the primary colour and its name is bold. For up to four choices
 * (the cards share the width); longer lists are a [ChoiceDialogRow]. Each card is one radio button
 * ("Dark, selected"); the pictures are decoration. [tag] tags the row and each card (`<tag>.<value>`).
 */
@Composable
fun <T> OptionCards(options: List<VisualOption<T>>, isSelected: (T) -> Boolean, onSelect: (T) -> Unit, modifier: Modifier = Modifier, tag: String? = null, previewHeight: Dp = 64.dp) {
    Row(
        modifier.fillMaxWidth().padding(start = 16.dp, end = 16.dp, top = 4.dp, bottom = 12.dp).height(IntrinsicSize.Min).selectableGroup()
            .let { if (tag != null) it.testTag(tag) else it },
        horizontalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        options.forEach { option ->
            val selected = isSelected(option.value)
            val scheme = MaterialTheme.colorScheme
            val outline by animateColorAsState(if (selected) scheme.primary else Color.Transparent, label = "optionOutline")
            val shape = RoundedCornerShape(20.dp)
            Column(
                Modifier.weight(1f).fillMaxHeight().clip(shape).background(scheme.surfaceContainerHigh).border(2.dp, outline, shape)
                    .selectable(selected = selected, role = Role.RadioButton, onClick = { onSelect(option.value) })
                    .let { if (tag != null) it.testTag("$tag.${option.value}") else it }
                    .padding(6.dp),
                horizontalAlignment = Alignment.CenterHorizontally,
            ) {
                Box(
                    Modifier.fillMaxWidth().height(previewHeight).clip(RoundedCornerShape(14.dp)).background(scheme.surfaceContainerLowest).clearAndSetSemantics { },
                    contentAlignment = Alignment.Center,
                ) { option.preview(selected) }
                Spacer(Modifier.height(6.dp))
                Text(
                    option.label, style = MaterialTheme.typography.labelLarge, textAlign = TextAlign.Center, maxLines = 2,
                    fontWeight = if (selected) FontWeight.Bold else FontWeight.Normal,
                    color = if (selected) scheme.onSurface else scheme.onSurfaceVariant,
                    modifier = Modifier.padding(horizontal = 2.dp).padding(bottom = 4.dp),
                )
            }
        }
    }
}

/** One choice of a [ChoiceDialogRow]: its value, its name and, optionally, an icon. */
class DialogOption<T>(val value: T, val label: String, val icon: ImageVector? = null)

/**
 * A setting with a longer list of choices (sizes, intervals, actions): one row showing the current
 * choice, which opens a dialog of radio buttons; picking one applies it and closes the dialog. The
 * row's [tag] also tags the dialog's options (`<tag>.option.<value>`). [trailing] replaces the
 * default (the current choice's icon, when it has one).
 */
@Composable
fun <T> ChoiceDialogRow(
    title: String,
    options: List<DialogOption<T>>,
    selected: T,
    onSelect: (T) -> Unit,
    scope: SettingScope? = null,
    tag: String? = null,
    valueText: String? = null,
    trailing: (@Composable () -> Unit)? = null,
) {
    var open by remember { mutableStateOf(false) }
    val current = options.firstOrNull { it.value == selected }
    val shownTrailing: (@Composable () -> Unit)? = trailing ?: current?.icon?.let { icon -> { Icon(icon, null, tint = MaterialTheme.colorScheme.primary) } }
    SettingRow(title, valueText ?: current?.label, scope, onClick = { open = true }, tag = tag, trailing = shownTrailing)
    if (open) {
        AlertDialog(
            onDismissRequest = { open = false },
            title = { Text(title) },
            text = {
                Column(Modifier.selectableGroup().verticalScroll(rememberScrollState())) {
                    options.forEach { option ->
                        val on = option.value == selected
                        Row(
                            Modifier.fillMaxWidth().heightIn(min = 52.dp).clip(RoundedCornerShape(12.dp))
                                .selectable(selected = on, role = Role.RadioButton, onClick = { open = false; onSelect(option.value) })
                                .let { if (tag != null) it.testTag("$tag.option.${option.value}") else it }
                                .padding(horizontal = 4.dp),
                            verticalAlignment = Alignment.CenterVertically,
                        ) {
                            RadioButton(selected = on, onClick = null)
                            Spacer(Modifier.width(12.dp))
                            option.icon?.let { Icon(it, null, tint = MaterialTheme.colorScheme.onSurfaceVariant); Spacer(Modifier.width(12.dp)) }
                            Text(option.label, style = MaterialTheme.typography.bodyLarge)
                        }
                    }
                }
            },
            confirmButton = { TextButton(onClick = { open = false }, modifier = Modifier.testTag("choice.cancel")) { Text(stringResource(R.string.action_cancel)) } },
        )
    }
}

/** A row that opens a page of its own: title, an optional summary of what is set there, a chevron. */
@Composable
fun NavRow(title: String, summary: String? = null, scope: SettingScope? = null, tag: String? = null, onClick: () -> Unit) =
    SettingRow(title, summary, scope, onClick = onClick, tag = tag) { Chevron() }

/**
 * A number stepped by buttons on the row itself: title on the left, then minus, the value, plus.
 * Each button speaks its own action ([lessLabel], [moreLabel]); the value is part of the row's text,
 * and with [onReset] tapping it resets ([resetLabel]).
 */
@Composable
fun StepperRow(title: String, value: String, onLess: () -> Unit, onMore: () -> Unit, lessLabel: String, moreLabel: String, scope: SettingScope? = null, tag: String? = null, onReset: (() -> Unit)? = null, resetLabel: String? = null) {
    SettingRow(title, scope = scope, tag = tag) {
        Row(Modifier.clip(CircleShape).background(MaterialTheme.colorScheme.surfaceContainerHigh), verticalAlignment = Alignment.CenterVertically) {
            IconButton(onClick = onLess, modifier = Modifier.testTag("${tag ?: "stepper"}.less").semantics { contentDescription = lessLabel }) { Icon(app.hocket.ui.icons.HocketIcons.Filled.Remove, null) }
            // The value itself resets (when [onReset] is given): back to the default in one tap.
            Text(value, style = MaterialTheme.typography.titleSmall, textAlign = TextAlign.Center,
                modifier = Modifier.width(72.dp).let { if (onReset != null) it.clip(CircleShape).clickable(onClickLabel = resetLabel, onClick = onReset).padding(vertical = 8.dp) else it }
                    // Each step is read out as it lands.
                    .semantics { liveRegion = androidx.compose.ui.semantics.LiveRegionMode.Polite })
            IconButton(onClick = onMore, modifier = Modifier.testTag("${tag ?: "stepper"}.more").semantics { contentDescription = moreLabel }) { Icon(app.hocket.ui.icons.HocketIcons.Filled.Add, null) }
        }
    }
}
