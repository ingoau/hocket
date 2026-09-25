package app.hocket.ui.components

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.wrapContentWidth
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.ListItem
import androidx.compose.material3.ListItemDefaults
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.Text
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.Immutable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.Stable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import app.hocket.R
import app.hocket.core.ActionIds
import app.hocket.core.ArtworkSizes
import app.hocket.core.Commands
import app.hocket.core.api.ActionDescriptor
import app.hocket.core.api.ActionTarget
import app.hocket.core.api.TrackSummary
import app.hocket.ui.DetailNavigator
import app.hocket.ui.LocalCoreClient
import app.hocket.ui.LocalDetailNavigator
import kotlinx.coroutines.launch

/**
 * A row a context adds to the song menu (the playlist's "Remove from playlist", the player's sleep
 * timer). Extras always sit in one place: after the registry's actions, below a divider.
 */
@Immutable
class SongMenuExtra(
    val label: String,
    val icon: ImageVector,
    val onClick: () -> Unit,
    val supporting: String? = null,
    val destructive: Boolean = false,
    val highlighted: Boolean = false,
    val testTag: String? = null,
)

/** One opening of the song menu: the song, what its actions act on (queue rows pass their item), the context's extras. */
@Immutable
data class SongMenuRequest(val track: TrackSummary, val target: ActionTarget, val extras: List<SongMenuExtra> = emptyList())

/**
 * Opens THE song menu. There is one per screen tree, hosted at the app level ([SongMenuHost]) and
 * driven by this state, so the menu looks and behaves the same from every list, the queue and the
 * full player, and its sheet never lives inside a surface that moves or leaves composition (the
 * player sheet): it is dismissed by its own gesture and nothing else.
 */
@Stable
class SongMenuController {
    /** Asked for, actions not loaded yet. */
    internal var pending by mutableStateOf<SongMenuRequest?>(null)
    /** On screen, with the registry's actions it was opened with. */
    internal var shown by mutableStateOf<Pair<SongMenuRequest, List<ActionDescriptor>>?>(null)

    fun open(track: TrackSummary, target: ActionTarget = Commands.tracks(listOf(track.id)), extras: List<SongMenuExtra> = emptyList()) {
        if (shown == null) pending = SongMenuRequest(track, target, extras)
    }

    val isOpen: Boolean get() = pending != null || shown != null

    internal fun close() { pending = null; shown = null }
}

val LocalSongMenu = staticCompositionLocalOf<SongMenuController?> { null }

/**
 * The song menu for this tree: the app-level one inside the main shell, or (for a screen shown on its
 * own) one hosted here.
 */
@Composable
fun rememberSongMenu(): SongMenuController {
    LocalSongMenu.current?.let { return it }
    val local = remember { SongMenuController() }
    SongMenuHost(local, LocalDetailNavigator.current)
    return local
}

/**
 * Hosts [controller]'s sheet. The registry's actions are fetched before the sheet is shown, so it
 * opens at its final height instead of growing (and re-anchoring) under the finger. [navigator]
 * performs "Go to album / artist" (the shell's collapses the player as it navigates).
 */
@Composable
fun SongMenuHost(controller: SongMenuController, navigator: DetailNavigator?) {
    val client = LocalCoreClient.current
    val pending = controller.pending
    LaunchedEffect(pending) {
        val request = pending ?: return@LaunchedEffect
        val actions = client.actions("contextMenu", request.target)
        if (controller.pending == request) {
            controller.pending = null
            controller.shown = request to actions
        }
    }
    var pickerFor by remember { mutableStateOf<ActionTarget?>(null) }
    controller.shown?.let { (request, actions) ->
        // Keyed on the request: a new opening starts from a fresh sheet state, never a stale one.
        androidx.compose.runtime.key(request) {
            SongMenuSheet(
                request, actions, navigator,
                onDismiss = { controller.close() },
                onAddToPlaylist = { pickerFor = request.target },
            )
        }
    }
    pickerFor?.let { t -> PlaylistPicker(target = t, onDismiss = { pickerFor = null }) }
}

/** The registry's rate entries: the menu's star row replaces them. */
private val RATE_IDS = (0..5).map { ActionIds.rate(it) }.toSet() + ActionIds.RATE

/** Registry actions that only apply in one context (the queue, a playlist): listed with the extras. */
private val CONTEXT_IDS = setOf(ActionIds.REMOVE_FROM_QUEUE, ActionIds.REMOVE_FROM_PLAYLIST)

/** The song menu's rows: the common actions, and the context's (listed last, with the extras). */
internal data class SongMenuRows(val common: List<ActionDescriptor>, val context: List<ActionDescriptor>)

/**
 * What the song menu lists, in the registry's (user-customisable) order: the rating is the star row
 * above, go-to entries only where the song has an album / artist and something can navigate, and
 * context-only actions split off so they always sit in the same place, after the common ones.
 */
internal fun songMenuRows(actions: List<ActionDescriptor>, track: TrackSummary, canNavigate: Boolean): SongMenuRows {
    val shown = actions.filter { a ->
        when (a.id) {
            in RATE_IDS -> false
            ActionIds.GO_TO_ALBUM -> canNavigate && track.albumId != null
            ActionIds.GO_TO_ARTIST -> canNavigate && track.artistId != null
            else -> true
        }
    }
    val (context, common) = shown.partition { it.id in CONTEXT_IDS }
    return SongMenuRows(common, context)
}

/**
 * The song menu: artwork, title and "artist · album"; the rating as stars; the registry's actions
 * for the target; then, below a divider, the context's own (remove from queue / playlist, extras).
 * Every row closes the sheet with its hide animation (actions and navigation start at once, behind it).
 */
@Composable
private fun SongMenuSheet(
    request: SongMenuRequest,
    actions: List<ActionDescriptor>,
    navigator: DetailNavigator?,
    onDismiss: () -> Unit,
    onAddToPlaylist: () -> Unit,
) {
    val client = LocalCoreClient.current
    val track = request.track
    val sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = true)
    val scope = rememberCoroutineScope()
    fun hideThen(after: () -> Unit = {}) {
        scope.launch { sheetState.hide() }.invokeOnCompletion { onDismiss(); after() }
    }
    var rating by remember { mutableIntStateOf(track.rating.toInt()) }
    val rows = remember(actions, track, navigator) { songMenuRows(actions, track, navigator != null) }
    @Composable fun ActionRow(a: ActionDescriptor) {
        MenuRow(a.label, actionIcon(a.icon), destructive = a.destructive, enabled = a.enabled, tag = "songMenu.action.${a.id}") {
            when (a.id) {
                ActionIds.ADD_TO_PLAYLIST -> hideThen(onAddToPlaylist)
                ActionIds.GO_TO_ALBUM -> { track.albumId?.let { navigator?.openAlbum?.invoke(it) }; hideThen() }
                ActionIds.GO_TO_ARTIST -> { track.artistId?.let { navigator?.openArtist?.invoke(it) }; hideThen() }
                in ActionIds.UI_HANDLED -> hideThen()
                else -> { client.dispatch(Commands.runAction(a.id, request.target)); hideThen() }
            }
        }
    }
    ModalBottomSheet(onDismissRequest = onDismiss, sheetState = sheetState, modifier = Modifier.testTag("songMenu")) {
        Column(Modifier.navigationBarsPadding().verticalScroll(rememberScrollState())) {
            Row(Modifier.fillMaxWidth().padding(horizontal = 24.dp).testTag("songMenu.header"), verticalAlignment = Alignment.CenterVertically) {
                Artwork(track.coverArt, ArtworkSizes.THUMB, null, Modifier.size(48.dp), RoundedCornerShape(ListArtCorner))
                Spacer(Modifier.width(16.dp))
                Column(Modifier.weight(1f)) {
                    Text(track.title, style = MaterialTheme.typography.titleMedium, maxLines = 2, overflow = TextOverflow.Ellipsis, modifier = Modifier.semantics { heading() })
                    val sub = listOfNotNull(track.artist ?: stringResource(R.string.unknown_artist), track.album).joinToString(stringResource(R.string.dot_separator))
                    Text(sub, style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant, maxLines = 1, overflow = TextOverflow.Ellipsis)
                }
            }
            RatingStars(
                rating,
                onRate = { stars -> rating = stars; client.dispatch(Commands.runAction(ActionIds.rate(stars), request.target)) },
                starSize = 32.dp,
                modifier = Modifier.fillMaxWidth().wrapContentWidth(Alignment.CenterHorizontally).padding(vertical = 4.dp),
            )
            HorizontalDivider()
            rows.common.forEach { ActionRow(it) }
            if (rows.context.isNotEmpty() || request.extras.isNotEmpty()) {
                HorizontalDivider()
                rows.context.forEach { ActionRow(it) }
                request.extras.forEach { e ->
                    MenuRow(e.label, e.icon, supporting = e.supporting, destructive = e.destructive, highlighted = e.highlighted, tag = e.testTag) { hideThen(e.onClick) }
                }
            }
            Spacer(Modifier.height(16.dp))
        }
    }
}

@Composable
private fun MenuRow(
    label: String,
    icon: ImageVector,
    supporting: String? = null,
    destructive: Boolean = false,
    highlighted: Boolean = false,
    enabled: Boolean = true,
    tag: String? = null,
    onClick: () -> Unit,
) {
    val tint = when {
        destructive -> MaterialTheme.colorScheme.error
        highlighted -> MaterialTheme.colorScheme.primary
        else -> MaterialTheme.colorScheme.onSurfaceVariant
    }
    ListItem(
        headlineContent = { Text(label, color = if (destructive) MaterialTheme.colorScheme.error else Color.Unspecified) },
        supportingContent = supporting?.let { { Text(it) } },
        leadingContent = { Icon(icon, null, tint = tint) },
        colors = ListItemDefaults.colors(containerColor = Color.Transparent),
        modifier = Modifier.clickable(enabled = enabled, onClick = onClick).let { if (tag != null) it.testTag(tag) else it },
    )
}
