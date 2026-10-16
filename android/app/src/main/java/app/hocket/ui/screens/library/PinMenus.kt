package app.hocket.ui.screens.library

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.FilterChip
import androidx.compose.material3.FilterChipDefaults
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.ListItem
import androidx.compose.material3.ListItemDefaults
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.Text
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.rotate
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.navigation.NavHostController
import app.hocket.R
import app.hocket.core.Commands
import app.hocket.core.SettingKeys
import app.hocket.core.api.Filter
import app.hocket.ui.LocalCoreClient
import app.hocket.ui.components.ActionSheet
import app.hocket.ui.icons.HocketIcons
import app.hocket.ui.nav.Route

@Composable
private fun SheetRow(label: String, icon: ImageVector, enabled: Boolean = true, flip: Boolean = false, tag: String? = null, onClick: () -> Unit) {
    ListItem(
        headlineContent = { Text(label, color = if (enabled) Color.Unspecified else MaterialTheme.colorScheme.onSurface.copy(alpha = 0.38f)) },
        leadingContent = { Icon(icon, null, Modifier.let { if (flip) it.rotate(180f) else it }, tint = MaterialTheme.colorScheme.onSurfaceVariant.copy(alpha = if (enabled) 1f else 0.38f)) },
        colors = ListItemDefaults.colors(containerColor = Color.Transparent),
        modifier = Modifier.clickable(enabled = enabled, onClick = onClick).let { if (tag != null) it.testTag(tag) else it },
    )
}

/** "When tapped: Open / Play / Shuffle" and moving the pin, at the top of a pin's menu. */
@Composable
private fun PinControls(pin: ResolvedPin, shown: List<PinEntry>, onDone: () -> Unit) {
    val client = LocalCoreClient.current
    val settings by client.settings.collectAsStateWithLifecycle()
    val pins = LibraryPins.parse(settings[SettingKeys.LIBRARY_PINS]?.value)
    val tap = pins.firstOrNull { it.same(pin.entry) }?.pinTap ?: pin.entry.pinTap
    if (pin.kind.choosesTap) {
        Text(stringResource(R.string.library_pin_when_tapped), style = MaterialTheme.typography.labelLarge, color = MaterialTheme.colorScheme.onSurfaceVariant, modifier = Modifier.padding(start = 24.dp, top = 4.dp))
        Row(Modifier.padding(horizontal = 24.dp, vertical = 4.dp).testTag("pin.tap"), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            PinTap.entries.forEach { t ->
                FilterChip(
                    selected = tap == t,
                    onClick = { LibraryPins.setTap(client, pin.entry, t) },
                    label = { Text(stringResource(t.label)) },
                    leadingIcon = if (tap == t) { { Icon(HocketIcons.Filled.Check, null, Modifier.padding(0.dp), tint = MaterialTheme.colorScheme.onSecondaryContainer) } } else null,
                    colors = FilterChipDefaults.filterChipColors(),
                    modifier = Modifier.testTag("pin.tap.${t.id}"),
                )
            }
        }
    }
    SheetRow(stringResource(R.string.library_pin_move_earlier), HocketIcons.Filled.KeyboardArrowDown, enabled = LibraryPins.shownNeighbour(shown, pin.entry, -1) != null, flip = true) { LibraryPins.move(client, pin.entry, -1, shown); onDone() }
    SheetRow(stringResource(R.string.library_pin_move_later), HocketIcons.Filled.KeyboardArrowDown, enabled = LibraryPins.shownNeighbour(shown, pin.entry, 1) != null) { LibraryPins.move(client, pin.entry, 1, shown); onDone() }
}

/**
 * A pin's menu (press and hold a pin): what a tap does, moving it, then the item's own menu (for
 * albums, artists, playlists and songs the registry's, which ends with Unpin from Library).
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun PinMenuSheet(pin: ResolvedPin, shown: List<PinEntry>, nav: NavHostController, serverId: String, onUnpinned: (PinEntry, Int) -> Unit, onDismiss: () -> Unit) {
    val client = LocalCoreClient.current
    val kindLabel = stringResource(pin.kind.label)
    val target = pin.target
    if (target != null) {
        ActionSheet(target, pin.title, kindLabel, onDismiss = onDismiss, extraTop = { PinControls(pin, shown, onDismiss) })
        return
    }
    ModalBottomSheet(onDismissRequest = onDismiss, sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = true), modifier = Modifier.testTag("pin.menu")) {
        Column(Modifier.navigationBarsPadding()) {
            Text(pin.title, style = MaterialTheme.typography.titleMedium, modifier = Modifier.padding(horizontal = 24.dp).semantics { heading() })
            Text(kindLabel, style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant, modifier = Modifier.padding(horizontal = 24.dp))
            Spacer(Modifier.height(8.dp))
            PinControls(pin, shown, onDismiss)
            HorizontalDivider()
            SheetRow(stringResource(R.string.library_pin_tap_open), HocketIcons.Filled.ChevronRight) { onDismiss(); performPin(client, nav, serverId, pin, PinTap.Open) }
            SheetRow(stringResource(R.string.action_play), HocketIcons.Filled.PlayArrow) {
                onDismiss()
                when (pin.kind) {
                    PinKind.Genre -> client.dispatch(Commands.playContext(Commands.genreContext(serverId, pin.entry.id)))
                    PinKind.Filter -> client.filters.value.firstOrNull { it.id == pin.entry.id }?.let { client.dispatch(Commands.playContext(Commands.filterContext(serverId, it))) }
                    else -> performPin(client, nav, serverId, pin, PinTap.Play)
                }
            }
            SheetRow(stringResource(R.string.library_pin_remove), HocketIcons.Outlined.PushPin, tag = "pin.unpin") {
                onDismiss()
                val at = LibraryPins.remove(client, pin.entry)
                if (at >= 0) onUnpinned(pin.entry, at)
            }
            Spacer(Modifier.height(16.dp))
        }
    }
}

/** A saved filter's menu (Home's filter cells, the Filters screen): play, shuffle, edit, pin. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun FilterMenuSheet(filter: Filter, nav: NavHostController, serverId: String, onDismiss: () -> Unit, onDelete: (() -> Unit)? = null) {
    val client = LocalCoreClient.current
    val settings by client.settings.collectAsStateWithLifecycle()
    val allPins = LibraryPins.parse(settings[SettingKeys.LIBRARY_PINS]?.value)
    val pinned = allPins.any { it.kind == PinKind.Filter.id && it.id == filter.id }
    ModalBottomSheet(onDismissRequest = onDismiss, sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = true), modifier = Modifier.testTag("filter.menu")) {
        Column(Modifier.navigationBarsPadding()) {
            Text(filter.name, style = MaterialTheme.typography.titleMedium, modifier = Modifier.padding(horizontal = 24.dp).semantics { heading() })
            Text(stringResource(R.string.kind_filter), style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant, modifier = Modifier.padding(horizontal = 24.dp))
            Spacer(Modifier.height(8.dp))
            SheetRow(stringResource(R.string.action_play), HocketIcons.Filled.PlayArrow) { onDismiss(); client.dispatch(Commands.playContext(Commands.filterContext(serverId, filter))) }
            SheetRow(stringResource(R.string.action_shuffle), HocketIcons.Filled.Shuffle) { onDismiss(); client.dispatch(Commands.playContext(Commands.filterContext(serverId, filter), shuffle = true)) }
            SheetRow(stringResource(R.string.filter_edit), HocketIcons.Filled.Tune) { onDismiss(); nav.navigate(Route.FilterBuilder(filter.id)) }
            SheetRow(stringResource(if (pinned) R.string.library_pin_remove else R.string.library_pin_add), if (pinned) HocketIcons.Outlined.PushPin else HocketIcons.Filled.PushPin, enabled = pinned || LibraryPins.canPin(allPins), tag = "filter.menu.pin") {
                onDismiss(); LibraryPins.toggle(client, PinKind.Filter, filter.id)
            }
            if (onDelete != null) SheetRow(stringResource(R.string.action_delete), HocketIcons.Filled.Delete, tag = "filter.menu.delete") { onDismiss(); onDelete() }
            Spacer(Modifier.height(16.dp))
        }
    }
}
