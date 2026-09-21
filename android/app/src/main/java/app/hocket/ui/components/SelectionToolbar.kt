package app.hocket.ui.components

import androidx.activity.compose.PredictiveBackHandler
import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.slideInVertically
import androidx.compose.animation.slideOutVertically
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Close
import androidx.compose.material.icons.filled.MoreVert
import androidx.compose.material.icons.filled.SelectAll
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.HorizontalFloatingToolbar
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.FloatingToolbarDefaults
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import app.hocket.R
import app.hocket.core.ActionIds
import app.hocket.core.Commands
import app.hocket.core.api.ActionDescriptor
import app.hocket.ui.LocalCoreClient
import kotlinx.coroutines.flow.collectLatest

/**
 * Selection-mode actions in an expressive floating toolbar. The first four applicable actions from
 * `Query.Actions(contextMenu, selection)` are icons; the rest go into an overflow menu. Predictive
 * back clears the selection.
 */
@Composable
fun SelectionToolbar(modifier: Modifier = Modifier, onSelectAll: (() -> Unit)? = null) {
    val client = LocalCoreClient.current
    val selection by client.selection.collectAsStateWithLifecycleCompat()
    val kind by client.selectionKind.collectAsStateWithLifecycleCompat()
    var actions by remember { mutableStateOf<List<ActionDescriptor>>(emptyList()) }
    var menu by remember { mutableStateOf(false) }
    var playlistPicker by remember { mutableStateOf(false) }
    LaunchedEffect(selection, kind) {
        if (selection.active) actions = client.actions("contextMenu", client.selectionTarget())
    }
    LaunchedEffect(Unit) { client.actionsChanged.collectLatest { if (selection.active) actions = client.actions("contextMenu", client.selectionTarget()) } }
    if (selection.active) {
        PredictiveBackHandler { progress -> try { progress.collect { } ; client.clearSelection() } catch (e: kotlinx.coroutines.CancellationException) { } }
    }
    // Bulk toolbar: navigation and per-item ui-handled entries make no sense for a selection.
    val toolbarActions = actions.filter { it.id !in ActionIds.UI_HANDLED || it.id == ActionIds.ADD_TO_PLAYLIST }.filter { !it.id.matches(Regex("rate[0-4]")) }
    AnimatedVisibility(visible = selection.active, enter = slideInVertically { it } + fadeIn(), exit = slideOutVertically { it } + fadeOut(), modifier = modifier) {
        Box(Modifier.fillMaxWidth().padding(16.dp), contentAlignment = Alignment.BottomCenter) {
            val count = stringResource(R.string.selected_count, selection.count)
            HorizontalFloatingToolbar(
                expanded = true,
                colors = FloatingToolbarDefaults.vibrantFloatingToolbarColors(),
                modifier = Modifier.semantics { contentDescription = count },
                leadingContent = {
                    IconButton(onClick = { client.clearSelection() }) { Icon(Icons.Filled.Close, stringResource(R.string.action_clear_selection)) }
                    Text(count, style = MaterialTheme.typography.labelLarge)
                },
                trailingContent = {
                    if (onSelectAll != null) IconButton(onClick = onSelectAll) { Icon(Icons.Filled.SelectAll, stringResource(R.string.action_select_all)) }
                    Box {
                        IconButton(onClick = { menu = true }) { Icon(Icons.Filled.MoreVert, stringResource(R.string.action_more)) }
                        DropdownMenu(expanded = menu, onDismissRequest = { menu = false }) {
                            toolbarActions.drop(4).forEach { a ->
                                DropdownMenuItem(text = { Text(a.label) }, leadingIcon = { Icon(actionIcon(a.icon), null) }, enabled = a.enabled, onClick = {
                                    menu = false
                                    if (a.id == ActionIds.ADD_TO_PLAYLIST) playlistPicker = true else { client.dispatch(Commands.runAction(a.id, client.selectionTarget())); client.clearSelection() }
                                })
                            }
                        }
                    }
                },
            ) {
                toolbarActions.take(4).forEach { a ->
                    IconButton(onClick = {
                        if (a.id == ActionIds.ADD_TO_PLAYLIST) playlistPicker = true else { client.dispatch(Commands.runAction(a.id, client.selectionTarget())); client.clearSelection() }
                    }, enabled = a.enabled) { Icon(actionIcon(a.icon), a.label) }
                }
            }
        }
    }
    if (playlistPicker) PlaylistPicker(target = client.selectionTarget(), onDismiss = { playlistPicker = false; client.clearSelection() })
}
