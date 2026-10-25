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
import app.hocket.share.ShareRequest
import app.hocket.share.ShareRequestEffect
import app.hocket.ui.LocalCoreClient
import app.hocket.ui.icons.HocketIcons
import app.hocket.core.Queries
import app.hocket.core.api.OfflineState
import app.hocket.core.api.QueryResult
import app.hocket.core.api.TrackSummary
import app.hocket.core.client.CoreClient
import app.hocket.core.client.Selection
import app.hocket.core.client.SelectionKind
import app.hocket.core.toSummary
import kotlinx.coroutines.flow.collectLatest
import kotlinx.coroutines.launch

/**
 * Selection-mode actions in an expressive floating toolbar. The first four applicable actions from
 * `Query.Actions(contextMenu, selection)` are icons. For selected songs (or queue entries) More
 * opens the songs' menu ([MenuSheet] for several songs: "3 songs · 11 min", the play group acting
 * on all of them, the rating ("Mixed" while they differ), love (partly loved shows half a heart),
 * "Download 3 songs", "Remove 3 from Late night"); for albums, artists and playlists the rest of
 * the actions go into an overflow menu, where the rateN entries collapse into one "Set rating…"
 * (in rate5's place) whose dialog picks the stars for every selected item. Predictive back clears
 * the selection. [context] names the playlist the songs are listed in; [onRemoveFromContext] (an
 * editable playlist) removes the selected entries from it ("Remove 3 from Late night").
 */
@Composable
fun SelectionToolbar(modifier: Modifier = Modifier, onSelectAll: (() -> Unit)? = null, context: String? = null, onRemoveFromContext: (() -> Unit)? = null) {
    val client = LocalCoreClient.current
    val selection by client.selection.collectAsStateWithLifecycleCompat()
    val kind by client.selectionKind.collectAsStateWithLifecycleCompat()
    var actions by remember { mutableStateOf<List<ActionDescriptor>>(emptyList()) }
    var menu by remember { mutableStateOf(false) }
    var playlistPicker by remember { mutableStateOf(false) }
    var share by remember { mutableStateOf<ShareRequest?>(null) }
    var rating by remember { mutableStateOf(false) }
    var songsMenu by remember { mutableStateOf<MenuSubject?>(null) }
    val songs = kind == SelectionKind.Tracks || kind == SelectionKind.QueueItems
    val resources = androidx.compose.ui.platform.LocalResources.current
    val scope = androidx.compose.runtime.rememberCoroutineScope()
    val dot = stringResource(R.string.dot_separator)
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
                        IconButton(onClick = {
                            if (!songs) menu = true
                            else scope.launch { songsMenu = selectionSubject(client, selection, kind, context, resources, dot) }
                        }, modifier = Modifier.testTag("selection.more")) { Icon(HocketIcons.Filled.MoreVert, stringResource(R.string.action_more)) }
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
    songsMenu?.let { subject ->
        androidx.compose.runtime.key(subject) {
            MenuSheet(
                subject, client.selectionTarget(), actions,
                extras = listOfNotNull(onRemoveFromContext?.takeIf { context != null }?.let { remove ->
                    SongMenuExtra(resources.getString(R.string.menu_remove_n_from, subject.count, context), HocketIcons.Filled.PlaylistRemove,
                        onClick = { remove(); client.clearSelection() }, destructive = true, testTag = "selectionMenu.removeFromPlaylist", replaces = ActionIds.REMOVE_FROM_PLAYLIST)
                }),
                onDismiss = { songsMenu = null },
                onAddToPlaylist = { playlistPicker = true },
                onShare = { share = ShareRequest(it, subject, client.selectionTarget()) },
                onRan = { client.clearSelection() },
                tag = "selectionMenu",
            )
        }
    }
    ShareRequestEffect(share) { share = null }
    if (playlistPicker) PlaylistPicker(target = client.selectionTarget(), onDismiss = { playlistPicker = false; client.clearSelection() })
    if (rating) {
        // Applies to the whole selection, so 0 (clear) is offered alongside the stars.
        RatingDialog(current = 0, onRate = { stars -> client.dispatch(Commands.runAction(ActionIds.rate(stars), client.selectionTarget())); rating = false; client.clearSelection() },
            onDismiss = { rating = false }, title = setRating, offerClear = true)
    }
}

/**
 * The header and toggles of the selected songs' menu: "3 songs · 11 min", their titles, the first
 * one's artwork, how many are loved, their rating (null when they differ: "Mixed"), whether all are
 * downloaded. A select-all selection names only its count (its songs are not listed here).
 */
internal suspend fun selectionSubject(client: CoreClient, selection: Selection, kind: SelectionKind, context: String?, resources: android.content.res.Resources, dot: String): MenuSubject {
    val count = selection.count
    val tracks: List<TrackSummary> = if (selection.inverted) emptyList() else when (kind) {
        SelectionKind.QueueItems -> {
            val q = client.queue.value
            val all = q.history + listOfNotNull(q.current) + q.playingNext + q.upcoming
            selection.ids.mapNotNull { k -> all.firstOrNull { it.item.key == k }?.track }
        }
        else -> (client.query(Queries.tracksByIds(selection.ids.toList())) as? QueryResult.TrackList)?.data?.map { it.toSummary() }.orEmpty()
    }
    return songsSubject(count, tracks, context, resources, dot, removeFromQueue = kind == SelectionKind.QueueItems)
}

/** [selectionSubject] once the songs are known (pure but for the words). */
internal fun songsSubject(count: Int, tracks: List<TrackSummary>, context: String?, resources: android.content.res.Resources, dot: String, removeFromQueue: Boolean = false): MenuSubject {
    val minutes = (tracks.sumOf { it.durationMs.toLong() } / 60_000).toInt()
    val title = resources.getQuantityString(R.plurals.menu_songs, count, count) + if (tracks.isNotEmpty()) dot + if (minutes >= 60) resources.getString(R.string.duration_hours_minutes, minutes / 60, minutes % 60) else resources.getString(R.string.duration_minutes, minutes) else ""
    val loved = tracks.count { it.loved }
    val ratings = tracks.map { it.rating.toInt() }.distinct()
    return MenuSubject(
        kind = MenuKind.Songs,
        title = title,
        subtitle = tracks.joinToString(", ") { it.title }.ifEmpty { null },
        coverArt = tracks.firstOrNull()?.coverArt,
        loved = when { tracks.isEmpty() || loved == 0 -> Tick.Off; loved == tracks.size -> Tick.On; else -> Tick.Mixed },
        lovedCount = loved,
        rating = ratings.singleOrNull() ?: if (ratings.isEmpty()) 0 else null,
        downloaded = tracks.isNotEmpty() && tracks.all { it.offline == OfflineState.Downloaded },
        count = count,
        context = if (removeFromQueue) null else context,
    )
}
