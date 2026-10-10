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
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import app.hocket.R
import app.hocket.core.ActionIds
import app.hocket.core.Commands
import app.hocket.core.api.ActionDescriptor
import app.hocket.ui.LocalCoreClient
import app.hocket.ui.icons.HocketIcons
import kotlinx.coroutines.flow.collectLatest

/**
 * Selection-mode actions in an expressive floating toolbar. The first four applicable actions from
 * `Query.Actions(contextMenu, selection)` are icons; the rest go into an overflow menu. Predictive
 * back clears the selection. The rateN entries collapse into one "Set rating…" (in rate5's place,
 * as in the action sheet) whose dialog picks the stars for every selected item.
 */
@Composable
fun SelectionToolbar(modifier: Modifier = Modifier, onSelectAll: (() -> Unit)? = null) {
    val client = LocalCoreClient.current
    val selection by client.selection.collectAsStateWithLifecycleCompat()
    val kind by client.selectionKind.collectAsStateWithLifecycleCompat()
    var actions by remember { mutableStateOf<List<ActionDescriptor>>(emptyList()) }
    var menu by remember { mutableStateOf(false) }
    var playlistPicker by remember { mutableStateOf(false) }
    var rating by remember { mutableStateOf(false) }
    LaunchedEffect(selection, kind) {
        if (selection.active) actions = client.actions("contextMenu", client.selectionTarget())
    }
    LaunchedEffect(Unit) { client.actionsChanged.collectLatest { if (selection.active) actions = client.actions("contextMenu", client.selectionTarget()) } }
    if (selection.active) {
        PredictiveBackHandler { progress -> try { progress.collect { } ; client.clearSelection() } catch (e: kotlinx.coroutines.CancellationException) { } }
    }
    // Bulk toolbar: navigation and per-item ui-handled entries make no sense for a selection.
    val toolbarActions = actions.filter { it.id !in ActionIds.UI_HANDLED || it.id == ActionIds.ADD_TO_PLAYLIST }.filter { !it.id.matches(Regex("rate[0-4]")) }
    val setRating = stringResource(R.string.action_set_rating)
    fun label(a: ActionDescriptor) = if (a.id == ActionIds.rate(5)) setRating else a.label
    fun icon(a: ActionDescriptor) = actionIcon(if (a.id == ActionIds.rate(5)) "star_rate" else a.icon)
    fun run(a: ActionDescriptor) {
        when (a.id) {
            ActionIds.ADD_TO_PLAYLIST -> playlistPicker = true
            ActionIds.rate(5) -> rating = true
            else -> { client.dispatch(Commands.runAction(a.id, client.selectionTarget())); client.clearSelection() }
        }
    }
    AnimatedVisibility(visible = selection.active, enter = slideInVertically { it } + fadeIn(), exit = slideOutVertically { it } + fadeOut(), modifier = modifier) {
        Box(Modifier.fillMaxWidth().padding(16.dp), contentAlignment = Alignment.BottomCenter) {
            val count = stringResource(R.string.selected_count, selection.count)
            HorizontalFloatingToolbar(
                expanded = true,
                colors = FloatingToolbarDefaults.vibrantFloatingToolbarColors(),
                modifier = Modifier.semantics { contentDescription = count },
                leadingContent = {
                    IconButton(onClick = { client.clearSelection() }) { Icon(HocketIcons.Filled.Close, stringResource(R.string.action_clear_selection)) }
                    Text(count, style = MaterialTheme.typography.labelLarge)
                },
                trailingContent = {
                    if (onSelectAll != null) IconButton(onClick = onSelectAll) { Icon(HocketIcons.Filled.SelectAll, stringResource(R.string.action_select_all)) }
                    Box {
                        IconButton(onClick = { menu = true }) { Icon(HocketIcons.Filled.MoreVert, stringResource(R.string.action_more)) }
                        DropdownMenu(expanded = menu, onDismissRequest = { menu = false }) {
                            toolbarActions.drop(4).forEach { a ->
                                DropdownMenuItem(text = { Text(label(a)) }, leadingIcon = { Icon(icon(a), null) }, enabled = a.enabled, onClick = { menu = false; run(a) },
                                    modifier = Modifier.testTag("selection.action.${a.id}"))
                            }
                        }
                    }
                },
            ) {
                toolbarActions.take(4).forEach { a ->
                    IconButton(onClick = { run(a) }, enabled = a.enabled, modifier = Modifier.testTag("selection.action.${a.id}")) { Icon(icon(a), label(a)) }
                }
            }
        }
    }
    if (playlistPicker) PlaylistPicker(target = client.selectionTarget(), onDismiss = { playlistPicker = false; client.clearSelection() })
    if (rating) {
        // Applies to the whole selection, so 0 (clear) is offered alongside the stars.
        RatingDialog(current = 0, onRate = { stars -> client.dispatch(Commands.runAction(ActionIds.rate(stars), client.selectionTarget())); rating = false; client.clearSelection() },
            onDismiss = { rating = false }, title = setRating, offerClear = true)
    }
}
