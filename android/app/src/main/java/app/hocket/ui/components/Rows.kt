package app.hocket.ui.components

import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.animation.animateColorAsState
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.indication
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.material3.ripple
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.ui.platform.testTag
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.hapticfeedback.HapticFeedbackType
import androidx.compose.ui.platform.LocalHapticFeedback
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.CustomAccessibilityAction
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.customActions
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.selected
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import app.hocket.R
import app.hocket.core.ActionIds
import app.hocket.core.ArtworkSizes
import app.hocket.core.Commands
import app.hocket.core.SwipeOptions
import app.hocket.core.api.ActionTarget
import app.hocket.ui.LocalCoreClient
import app.hocket.ui.LocalDetailNavigator
import app.hocket.core.api.Album
import app.hocket.core.api.Artist
import app.hocket.core.api.Genre
import app.hocket.core.api.OfflineState
import app.hocket.core.api.Playlist
import app.hocket.core.api.TrackSummary
import app.hocket.ui.icons.HocketIcons

/**
 * Long-press enters selection mode; tap toggles while selection is active (design: lists).
 *
 * Accessibility: the row is one item. [label] is its whole description (children's text is merged
 * but the description wins, so nothing is read twice; icons inside rows carry no description of
 * their own, their meaning is in [label] or [state]). [state] is read after the label ("playing");
 * [actions] are the row menu as custom actions, so a screen-reader user never needs the long-press,
 * the swipe or the drag handle. The click has a label ("Play", "Select", "Deselect").
 */
@OptIn(ExperimentalFoundationApi::class)
@Composable
fun SelectableRow(
    selected: Boolean,
    selectionActive: Boolean,
    onClick: () -> Unit,
    onToggleSelect: () -> Unit,
    label: String,
    modifier: Modifier = Modifier,
    state: String? = null,
    actions: List<CustomAccessibilityAction> = emptyList(),
    clickLabel: String? = null,
    /** Shared with a child that draws the press ripple itself (grid cells ripple on the artwork only). */
    interactionSource: MutableInteractionSource? = null,
    /** False when a child draws the press indication from [interactionSource]. */
    rowIndication: Boolean = true,
    content: @Composable () -> Unit,
) {
    val haptics = LocalHapticFeedback.current
    val container by animateColorAsState(
        if (selected) MaterialTheme.colorScheme.secondaryContainer else MaterialTheme.colorScheme.surface, label = "rowSelected",
    )
    val selectLabel = stringResource(R.string.a11y_select)
    val deselectLabel = stringResource(R.string.a11y_deselect)
    val openLabel = clickLabel ?: stringResource(R.string.a11y_open)
    Surface(
        color = container,
        modifier = modifier
            .fillMaxWidth()
            .combinedClickable(
                interactionSource = interactionSource,
                indication = if (rowIndication) ripple() else null,
                onClick = { if (selectionActive) onToggleSelect() else onClick() },
                onClickLabel = if (!selectionActive) openLabel else if (selected) deselectLabel else selectLabel,
                onLongClick = { haptics.performHapticFeedback(HapticFeedbackType.LongPress); onToggleSelect() },
                onLongClickLabel = if (selected) deselectLabel else selectLabel,
            )
            .semantics(mergeDescendants = true) {
                contentDescription = label
                if (selectionActive) this.selected = selected
                if (state != null) stateDescription = state
                if (actions.isNotEmpty()) customActions = actions
            },
    ) { content() }
}

/** The spoken description of a track row: "Tally, twenty one pilots, 3:32, loved". */
@Composable
fun trackLabel(track: TrackSummary): String {
    val artist = track.artist ?: stringResource(R.string.unknown_artist)
    return listOfNotNull(
        stringResource(R.string.row_track_a11y, track.title, artist, formatClock(track.durationMs)),
        if (track.loved) stringResource(R.string.row_state_loved) else null,
    ).joinToString(", ")
}

/**
 * The spoken offline state of a row ("downloaded" / "cached, available offline"), read as part of
 * the row's state after its label; null when the item needs the network (a partial cache entry
 * counts as not cached).
 */
@Composable
fun offlineStateText(state: OfflineState): String? = when (state) {
    OfflineState.Downloaded -> stringResource(R.string.row_state_downloaded)
    OfflineState.Cached -> stringResource(R.string.row_state_cached)
    else -> null
}

/**
 * The row menu as accessibility actions: play next, add to queue, go to album / artist (when a
 * [LocalDetailNavigator] is present), download or remove the download, rate (a dialog), and the full
 * menu. [target] is what the row menu acts on (queue rows pass their queue item).
 */
@Composable
fun trackRowActions(track: TrackSummary, target: ActionTarget, onRate: () -> Unit, onMore: (() -> Unit)?, extra: List<CustomAccessibilityAction> = emptyList()): List<CustomAccessibilityAction> {
    val client = LocalCoreClient.current
    val nav = LocalDetailNavigator.current
    val playNext = stringResource(R.string.action_play_next)
    val addToQueue = stringResource(R.string.a11y_add_to_queue)
    val goAlbum = stringResource(R.string.action_go_to_album)
    val goArtist = stringResource(R.string.action_go_to_artist)
    val download = stringResource(R.string.action_download)
    val removeDownload = stringResource(R.string.action_remove_download)
    val rate = stringResource(R.string.action_rate)
    val more = stringResource(R.string.action_more)
    return remember(track, target, nav, onMore, extra, playNext) {
        buildList {
            add(CustomAccessibilityAction(playNext) { client.dispatch(Commands.runAction(ActionIds.PLAY_NEXT, target)); true })
            add(CustomAccessibilityAction(addToQueue) { client.dispatch(Commands.runAction(ActionIds.PLAY_LATER, target)); true })
            addAll(extra)
            if (nav != null) {
                track.albumId?.let { id -> add(CustomAccessibilityAction(goAlbum) { nav.openAlbum(id); true }) }
                track.artistId?.let { id -> add(CustomAccessibilityAction(goArtist) { nav.openArtist(id); true }) }
            }
            if (track.offline == OfflineState.Downloaded) add(CustomAccessibilityAction(removeDownload) { client.dispatch(Commands.runAction(ActionIds.UNPIN, target)); true })
            else add(CustomAccessibilityAction(download) { client.dispatch(Commands.runAction(ActionIds.DOWNLOAD, target)); true })
            add(CustomAccessibilityAction(rate) { onRate(); true })
            if (onMore != null) add(CustomAccessibilityAction(more) { onMore(); true })
        }
    }
}

@Composable
fun TrackRow(
    track: TrackSummary,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
    trailing: (@Composable () -> Unit)? = null,
    onMore: (() -> Unit)? = null,
    selected: Boolean = false,
    selectionActive: Boolean = false,
    onToggleSelect: () -> Unit = {},
    nowPlaying: Boolean = false,
    showArtwork: Boolean = true,
    leading: (@Composable () -> Unit)? = null,
    /** What the row's accessibility actions act on (default: this track). */
    actionTarget: ActionTarget? = null,
    /** Row-specific accessibility actions (the queue's move and remove), added to the row menu's. */
    extraActions: List<CustomAccessibilityAction> = emptyList(),
    /** Whose swipe settings the row follows; rows with a menu swipe like song lists, read-only rows not at all. */
    swipe: SwipeSurface? = if (onMore != null) SwipeSurface.List else null,
) {
    val artist = track.artist ?: stringResource(R.string.unknown_artist)
    val label = trackLabel(track)
    var rating by remember { mutableStateOf(false) }
    var playlistPicker by remember { mutableStateOf(false) }
    val target = actionTarget ?: Commands.tracks(listOf(track.id))
    // Rows with a menu offer it as actions; read-only rows (stats, filter previews) offer none.
    val actions = if (onMore != null) trackRowActions(track, target, onRate = { rating = true }, onMore = onMore, extra = extraActions) else extraActions
    val (startId, endId) = if (swipe != null) swipeActionIds(swipe) else SwipeOptions.NONE to SwipeOptions.NONE
    // The row (and the caller's modifier) stays the one merged accessibility item; only its content
    // slides under the swipe.
    SelectableRow(
        selected, selectionActive, onClick, onToggleSelect, label,
        modifier = modifier,
        // "playing, downloaded": the row's state, merged with its label into one item.
        state = listOfNotNull(if (nowPlaying) stringResource(R.string.row_state_playing) else null, offlineStateText(track.offline))
            .joinToString(", ").ifEmpty { null },
        actions = actions,
        clickLabel = stringResource(R.string.action_play),
    ) {
        SwipeActionBox(
            startToEnd = trackSwipeAction(startId, track, target, onAddToPlaylist = { playlistPicker = true }),
            endToStart = trackSwipeAction(endId, track, target, onAddToPlaylist = { playlistPicker = true }),
            // Not while selecting: rows toggle then, and a stray swipe must not act on one of them.
            enabled = swipe != null && !selectionActive,
            swipeSurface = MaterialTheme.colorScheme.surface,
        ) {
            Row(Modifier.heightIn(min = 64.dp).padding(horizontal = 16.dp, vertical = 8.dp), verticalAlignment = Alignment.CenterVertically) {
                leading?.invoke()
                if (showArtwork) {
                    Box(Modifier.size(48.dp)) {
                        Artwork(track.coverArt, ArtworkSizes.THUMB, null, Modifier.size(48.dp), RoundedCornerShape(ListArtCorner))
                        if (selected) Box(Modifier.size(48.dp).clip(RoundedCornerShape(ListArtCorner)), contentAlignment = Alignment.Center) {
                            Surface(shape = CircleShape, color = MaterialTheme.colorScheme.primary) { Icon(HocketIcons.Filled.Check, null, Modifier.padding(4.dp), tint = MaterialTheme.colorScheme.onPrimary) }
                        }
                    }
                    Spacer(Modifier.width(14.dp))
                }
                Column(Modifier.weight(1f)) {
                    Row(verticalAlignment = Alignment.CenterVertically) {
                        if (nowPlaying) {
                            Icon(HocketIcons.Filled.GraphicEq, null, tint = MaterialTheme.colorScheme.primary, modifier = Modifier.size(16.dp))
                            Spacer(Modifier.width(6.dp))
                        }
                        Text(track.title, style = MaterialTheme.typography.bodyLarge, maxLines = 1, overflow = TextOverflow.Ellipsis,
                            color = if (nowPlaying) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.onSurface)
                    }
                    Row(verticalAlignment = Alignment.CenterVertically) {
                        OfflineBadge(track.offline, describe = false)
                        Text(artist, style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant, maxLines = 1, overflow = TextOverflow.Ellipsis)
                    }
                }
                if (track.loved) {
                    Icon(HocketIcons.Filled.Favorite, null, tint = MaterialTheme.colorScheme.tertiary, modifier = Modifier.size(16.dp))
                    Spacer(Modifier.width(8.dp))
                }
                Text(formatClock(track.durationMs), style = MaterialTheme.typography.labelMedium, color = MaterialTheme.colorScheme.onSurfaceVariant, maxLines = 1)
                trailing?.invoke()
                if (onMore != null) {
                    IconButton(onClick = onMore) { Icon(HocketIcons.Filled.MoreVert, stringResource(R.string.action_more_for, track.title)) }
                }
            }
        }
    }
    if (playlistPicker) PlaylistPicker(target = target, onDismiss = { playlistPicker = false })
    if (rating) {
        val client = LocalCoreClient.current
        RatingDialog(current = track.rating.toInt(), onRate = { stars -> client.dispatch(Commands.runAction(ActionIds.rate(stars), target)); rating = false }, onDismiss = { rating = false })
    }
}

/**
 * Downloaded / cached marker: a filled "download for offline" disc for a pinned download, an
 * outlined bolt for a complete stream-cache entry (plays offline until evicted); nothing for a
 * partial entry or an uncached item. [describe] false inside rows, whose state already says it.
 */
@Composable
fun OfflineBadge(state: OfflineState, describe: Boolean = true) {
    val (icon, text, tint) = when (state) {
        OfflineState.Downloaded -> Triple(HocketIcons.Filled.DownloadForOffline, R.string.badge_downloaded, MaterialTheme.colorScheme.primary)
        OfflineState.Cached -> Triple(HocketIcons.Outlined.OfflineBolt, R.string.badge_cached, MaterialTheme.colorScheme.onSurfaceVariant)
        else -> return
    }
    Icon(icon, if (describe) stringResource(text) else null, tint = tint, modifier = Modifier.size(16.dp).testTag("offlineBadge.${state.string}"))
    Spacer(Modifier.width(4.dp))
}

@Composable
fun AlbumCard(
    album: Album,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
    selected: Boolean = false,
    selectionActive: Boolean = false,
    onToggleSelect: () -> Unit = {},
) {
    val artist = album.artist ?: stringResource(R.string.unknown_artist)
    val label = stringResource(R.string.row_album_a11y, album.name, artist)
    val interaction = remember { MutableInteractionSource() }
    SelectableRow(selected, selectionActive, onClick, onToggleSelect, label, modifier, interactionSource = interaction, rowIndication = false) {
        Column(Modifier.padding(6.dp)) {
            Box {
                // The press ripple is drawn on the cover only (Navic): the text below stays calm.
                Artwork(album.coverArt, ArtworkSizes.GRID, null, Modifier.fillMaxWidth().aspectRatio(1f).clip(RoundedCornerShape(GridArtCorner)).indication(interaction, ripple()), RoundedCornerShape(GridArtCorner))
                if (selected) Box(Modifier.padding(8.dp)) {
                    Surface(shape = CircleShape, color = MaterialTheme.colorScheme.primary) { Icon(HocketIcons.Filled.Check, null, Modifier.padding(4.dp), tint = MaterialTheme.colorScheme.onPrimary) }
                }
            }
            Spacer(Modifier.height(8.dp))
            GridCellText(album.name, artist)
        }
    }
}

/**
 * The two text lines under a grid or carousel cell: a title of up to two lines (always two lines
 * tall, so cells in a row and their loading skeletons line up) and a one-line subtitle.
 */
@Composable
fun GridCellText(title: String, subtitle: String) {
    Text(title, style = GridTitleStyle, minLines = 2, maxLines = 2, overflow = TextOverflow.Ellipsis)
    Text(subtitle, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant, maxLines = 1, overflow = TextOverflow.Ellipsis)
}

internal val GridTitleStyle: androidx.compose.ui.text.TextStyle
    @Composable get() = MaterialTheme.typography.titleSmallEmphasized

@Composable
fun ArtistRow(artist: Artist, onClick: () -> Unit, modifier: Modifier = Modifier, selected: Boolean = false, selectionActive: Boolean = false, onToggleSelect: () -> Unit = {}) {
    val label = stringResource(R.string.row_artist_a11y, artist.name, artist.albumCount.toInt())
    SelectableRow(selected, selectionActive, onClick, onToggleSelect, label, modifier) {
        Row(Modifier.heightIn(min = 64.dp).padding(horizontal = 16.dp, vertical = 8.dp), verticalAlignment = Alignment.CenterVertically) {
            Artwork(artist.coverArt, ArtworkSizes.THUMB, null, Modifier.size(48.dp), CircleShape)
            Spacer(Modifier.width(14.dp))
            Column(Modifier.weight(1f)) {
                Text(artist.name, style = MaterialTheme.typography.bodyLarge, maxLines = 1, overflow = TextOverflow.Ellipsis)
                Text(stringResource(R.string.library_count_albums, artist.albumCount.toInt()), style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
            }
            if (selected) Icon(HocketIcons.Filled.Check, null, tint = MaterialTheme.colorScheme.primary)
        }
    }
}

@Composable
fun PlaylistRow(playlist: Playlist, onClick: () -> Unit, modifier: Modifier = Modifier, selected: Boolean = false, selectionActive: Boolean = false, onToggleSelect: () -> Unit = {}) {
    val label = listOfNotNull(
        stringResource(R.string.row_playlist_a11y, playlist.name, playlist.songCount.toInt()),
        if (playlist.isSmart) stringResource(R.string.badge_smart) else null,
    ).joinToString(", ")
    SelectableRow(selected, selectionActive, onClick, onToggleSelect, label, modifier, state = offlineStateText(playlist.offline)) {
        Row(Modifier.heightIn(min = 64.dp).padding(horizontal = 16.dp, vertical = 8.dp), verticalAlignment = Alignment.CenterVertically) {
            Artwork(playlist.coverArt, ArtworkSizes.THUMB, null, Modifier.size(48.dp), RoundedCornerShape(ListArtCorner))
            Spacer(Modifier.width(14.dp))
            Column(Modifier.weight(1f)) {
                Text(playlist.name, style = MaterialTheme.typography.bodyLarge, maxLines = 1, overflow = TextOverflow.Ellipsis)
                Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                    OfflineBadge(playlist.offline, describe = false)
                    Text(stringResource(R.string.library_count_songs, playlist.songCount.toInt()), style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
                    if (playlist.isSmart) Badge(stringResource(R.string.badge_smart))
                }
            }
            if (selected) Icon(HocketIcons.Filled.Check, null, tint = MaterialTheme.colorScheme.primary)
        }
    }
}

@Composable
fun GenreRow(genre: Genre, onClick: () -> Unit, modifier: Modifier = Modifier) {
    val label = stringResource(R.string.row_genre_a11y, genre.name, genre.songCount.toInt())
    SelectableRow(false, false, onClick, {}, label, modifier) {
        Row(Modifier.heightIn(min = 56.dp).padding(horizontal = 16.dp, vertical = 8.dp), verticalAlignment = Alignment.CenterVertically) {
            Column(Modifier.weight(1f)) {
                Text(genre.name, style = MaterialTheme.typography.bodyLarge)
                Text(stringResource(R.string.library_count_songs, genre.songCount.toInt()), style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
            }
        }
    }
}

@Composable
fun Badge(text: String, modifier: Modifier = Modifier, container: androidx.compose.ui.graphics.Color = MaterialTheme.colorScheme.secondaryContainer) {
    Surface(shape = RoundedCornerShape(6.dp), color = container, modifier = modifier) {
        Text(text, style = MaterialTheme.typography.labelSmall, modifier = Modifier.padding(horizontal = 6.dp, vertical = 2.dp))
    }
}

@Composable
fun SectionHeader(title: String, modifier: Modifier = Modifier, action: (@Composable () -> Unit)? = null) {
    Row(modifier.fillMaxWidth().heightIn(min = 48.dp).padding(start = 16.dp, end = 8.dp, top = 12.dp, bottom = 4.dp), verticalAlignment = Alignment.CenterVertically) {
        Text(title, style = MaterialTheme.typography.titleLarge, fontWeight = FontWeight.Bold, modifier = Modifier.weight(1f).semantics { heading() })
        action?.invoke()
    }
}
