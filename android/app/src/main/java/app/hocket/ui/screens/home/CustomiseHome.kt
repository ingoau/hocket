package app.hocket.ui.screens.home

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.FilterChip
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.ListItem
import androidx.compose.material3.ListItemDefaults
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.rotate
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import app.hocket.R
import app.hocket.core.SettingKeys
import app.hocket.ui.LocalCoreClient
import app.hocket.ui.icons.HocketIcons

/**
 * Customise Home: every section, in Home's order, with a switch, up and down, and how it's shown.
 * Each change saves at once (`home.sections`, synced to the user's other devices).
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun CustomiseHomeSheet(onDismiss: () -> Unit) {
    val client = LocalCoreClient.current
    val settings by client.settings.collectAsStateWithLifecycle()
    val prefs = remember(settings[SettingKeys.HOME_SECTIONS]?.value) { HomeSections.parse(settings[SettingKeys.HOME_SECTIONS]?.value) }
    // Sections this build doesn't know stay in the list (a newer device's), unlisted here.
    val known = prefs.filter { it.section != null }
    fun save(next: List<SectionPref>) = HomeSections.save(client, next)
    fun moveKnown(id: String, by: Int) {
        val i = known.indexOfFirst { it.id == id }
        val other = known.getOrNull(i + by) ?: return
        save(HomeSections.move(prefs, prefs.indexOfFirst { it.id == id }, prefs.indexOfFirst { it.id == other.id }))
    }
    ModalBottomSheet(onDismissRequest = onDismiss, sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = true), modifier = Modifier.testTag("home.customiseSheet")) {
        Column(Modifier.navigationBarsPadding()) {
            Text(stringResource(R.string.home_customise), style = MaterialTheme.typography.titleLarge, modifier = Modifier.padding(horizontal = 24.dp).semantics { heading() })
            Text(stringResource(R.string.home_customise_sync), style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant, modifier = Modifier.padding(horizontal = 24.dp, vertical = 4.dp))
            Spacer(Modifier.height(8.dp))
            LazyColumn(Modifier.weight(1f, fill = false)) {
                items(known, key = { it.id }) { pref ->
                    val section = pref.section ?: return@items
                    val title = stringResource(section.title)
                    val i = known.indexOf(pref)
                    Column(Modifier.animateItem().fillMaxWidth().padding(start = 24.dp, end = 12.dp, top = 8.dp, bottom = 8.dp).testTag("home.customise.${pref.id}")) {
                        Row(verticalAlignment = Alignment.CenterVertically) {
                            Column(Modifier.weight(1f)) {
                                Text(title, style = MaterialTheme.typography.bodyLarge, color = if (pref.on) MaterialTheme.colorScheme.onSurface else MaterialTheme.colorScheme.onSurfaceVariant)
                                Text(stringResource(section.description), style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
                            }
                            IconButton(onClick = { moveKnown(pref.id, -1) }, enabled = i > 0) {
                                Icon(HocketIcons.Filled.KeyboardArrowDown, stringResource(R.string.home_section_move_up_named, title), Modifier.rotate(180f))
                            }
                            IconButton(onClick = { moveKnown(pref.id, 1) }, enabled = i < known.lastIndex) {
                                Icon(HocketIcons.Filled.KeyboardArrowDown, stringResource(R.string.home_section_move_down_named, title))
                            }
                            Spacer(Modifier.width(4.dp))
                            val showLabel = stringResource(R.string.home_section_show_named, title)
                            Switch(checked = pref.on, onCheckedChange = { on -> save(HomeSections.update(prefs, pref.id) { it.copy(on = on) }) },
                                modifier = Modifier.semantics { contentDescription = showLabel }.testTag("home.customise.${pref.id}.switch"))
                        }
                        if (pref.on && section.styles.size > 1) {
                            Row(Modifier.padding(top = 4.dp), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                                section.styles.forEach { st ->
                                    FilterChip(selected = pref.resolvedStyle == st, onClick = { save(HomeSections.update(prefs, pref.id) { it.copy(style = st.id) }) }, label = { Text(stringResource(st.label)) })
                                }
                            }
                        }
                    }
                }
            }
            HorizontalDivider()
            TextButton(onClick = { save(HomeSections.DEFAULT) }, modifier = Modifier.padding(horizontal = 12.dp, vertical = 8.dp)) { Text(stringResource(R.string.home_customise_reset)) }
        }
    }
}

/** A section's own menu (its header's long-press or menu button): move, show as, hide, customise. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun SectionMenuSheet(pref: SectionPref, title: String, onDismiss: () -> Unit, onHide: () -> Unit, onCustomise: () -> Unit) {
    val client = LocalCoreClient.current
    val section = pref.section ?: return
    val prefs = HomeSections.current(client)
    val shown = prefs.filter { it.on && it.section != null }
    val i = shown.indexOfFirst { it.id == pref.id }
    val current = prefs.firstOrNull { it.id == pref.id } ?: pref
    @Composable fun Item(label: String, enabled: Boolean = true, rotateIcon: Boolean = false, icon: androidx.compose.ui.graphics.vector.ImageVector, onClick: () -> Unit) {
        ListItem(
            headlineContent = { Text(label) },
            leadingContent = { Icon(icon, null, Modifier.size(24.dp).let { if (rotateIcon) it.rotate(180f) else it }, tint = MaterialTheme.colorScheme.onSurfaceVariant) },
            colors = ListItemDefaults.colors(containerColor = Color.Transparent),
            modifier = Modifier.heightIn(min = 56.dp).let { m -> if (enabled) m.then(Modifier.clickableItem(onClick)) else m },
        )
    }
    ModalBottomSheet(onDismissRequest = onDismiss, sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = true)) {
        Column(Modifier.navigationBarsPadding()) {
            Text(title, style = MaterialTheme.typography.titleMedium, modifier = Modifier.padding(horizontal = 24.dp).semantics { heading() })
            Spacer(Modifier.height(8.dp))
            HorizontalDivider()
            Item(stringResource(R.string.home_section_move_up), enabled = i > 0, rotateIcon = true, icon = HocketIcons.Filled.KeyboardArrowDown) { HomeSections.save(client, HomeSections.moveShown(prefs, pref.id, -1)); onDismiss() }
            Item(stringResource(R.string.home_section_move_down), enabled = i in 0 until shown.lastIndex, icon = HocketIcons.Filled.KeyboardArrowDown) { HomeSections.save(client, HomeSections.moveShown(prefs, pref.id, 1)); onDismiss() }
            if (section.styles.size > 1) {
                Text(stringResource(R.string.home_section_show_as), style = MaterialTheme.typography.labelLarge, color = MaterialTheme.colorScheme.onSurfaceVariant, modifier = Modifier.padding(start = 24.dp, top = 12.dp))
                Row(Modifier.padding(horizontal = 24.dp, vertical = 4.dp), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    section.styles.forEach { st ->
                        FilterChip(selected = current.resolvedStyle == st, onClick = { HomeSections.save(client, HomeSections.update(prefs, pref.id) { it.copy(style = st.id) }) }, label = { Text(stringResource(st.label)) })
                    }
                }
            }
            Item(stringResource(R.string.home_section_hide), icon = HocketIcons.Filled.VisibilityOff) { onDismiss(); onHide() }
            Item(stringResource(R.string.home_customise), icon = HocketIcons.Filled.Tune) { onDismiss(); onCustomise() }
            Spacer(Modifier.height(16.dp))
        }
    }
}

private fun Modifier.clickableItem(onClick: () -> Unit) = this.clickable(onClick = onClick)
