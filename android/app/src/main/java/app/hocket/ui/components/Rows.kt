package app.hocket.ui.components

import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.combinedClickable
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
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Check
import androidx.compose.material.icons.filled.DownloadDone
import androidx.compose.material.icons.filled.Favorite
import androidx.compose.material.icons.filled.GraphicEq
import androidx.compose.material.icons.filled.MoreVert
import androidx.compose.material.icons.filled.OfflinePin
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.hapticfeedback.HapticFeedbackType
import androidx.compose.ui.platform.LocalHapticFeedback
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.selected
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import app.hocket.R
import app.hocket.core.ArtworkSizes
import app.hocket.core.api.Album
import app.hocket.core.api.Artist
import app.hocket.core.api.Genre
import app.hocket.core.api.OfflineState
import app.hocket.core.api.Playlist
import app.hocket.core.api.TrackSummary

/** Long-press enters selection mode; tap toggles while selection is active (design: lists). */
@OptIn(ExperimentalFoundationApi::class)
@Composable
fun SelectableRow(
    selected: Boolean,
    selectionActive: Boolean,
    onClick: () -> Unit,
    onToggleSelect: () -> Unit,
    label: String,
    modifier: Modifier = Modifier,
    content: @Composable () -> Unit,
) {
    val haptics = LocalHapticFeedback.current
    val selectedDesc = stringResource(R.string.row_selected, label)
    val selectDesc = stringResource(R.string.row_select, label)
    Surface(
        color = if (selected) MaterialTheme.colorScheme.secondaryContainer else MaterialTheme.colorScheme.surface,
        modifier = modifier
            .fillMaxWidth()
            .combinedClickable(
                onClick = { if (selectionActive) onToggleSelect() else onClick() },
                onLongClick = { haptics.performHapticFeedback(HapticFeedbackType.LongPress); onToggleSelect() },
            )
            .semantics(mergeDescendants = true) {
                this.selected = selected
                contentDescription = if (selected) selectedDesc else if (selectionActive) selectDesc else label
            },
    ) { content() }
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
) {
    val artist = track.artist ?: stringResource(R.string.unknown_artist)
    val label = stringResource(R.string.row_track_a11y, track.title, artist, formatClock(track.durationMs))
    SelectableRow(selected, selectionActive, onClick, onToggleSelect, label, modifier) {
        Row(Modifier.heightIn(min = 64.dp).padding(horizontal = 16.dp, vertical = 8.dp), verticalAlignment = Alignment.CenterVertically) {
            leading?.invoke()
            if (showArtwork) {
                Box(Modifier.size(48.dp)) {
                    Artwork(track.coverArt, ArtworkSizes.THUMB, null, Modifier.size(48.dp), RoundedCornerShape(8.dp))
                    if (selected) Box(Modifier.size(48.dp).clip(RoundedCornerShape(8.dp)).semantics { }, contentAlignment = Alignment.Center) {
                        Surface(shape = CircleShape, color = MaterialTheme.colorScheme.primary) { Icon(Icons.Filled.Check, null, Modifier.padding(4.dp), tint = MaterialTheme.colorScheme.onPrimary) }
                    }
                }
                Spacer(Modifier.width(14.dp))
            }
            Column(Modifier.weight(1f)) {
                Row(verticalAlignment = Alignment.CenterVertically) {
                    if (nowPlaying) {
                        Icon(Icons.Filled.GraphicEq, stringResource(R.string.row_now_playing), tint = MaterialTheme.colorScheme.primary, modifier = Modifier.size(16.dp))
                        Spacer(Modifier.width(6.dp))
                    }
                    Text(track.title, style = MaterialTheme.typography.bodyLarge, maxLines = 1, overflow = TextOverflow.Ellipsis,
                        color = if (nowPlaying) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.onSurface)
                }
                Row(verticalAlignment = Alignment.CenterVertically) {
                    OfflineBadge(track.offline)
                    Text(artist, style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant, maxLines = 1, overflow = TextOverflow.Ellipsis)
                }
            }
            if (track.loved) {
                Icon(Icons.Filled.Favorite, stringResource(R.string.player_loved), tint = MaterialTheme.colorScheme.tertiary, modifier = Modifier.size(16.dp))
                Spacer(Modifier.width(8.dp))
            }
            Text(formatClock(track.durationMs), style = MaterialTheme.typography.labelMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
            trailing?.invoke()
            if (onMore != null) {
                IconButton(onClick = onMore) { Icon(Icons.Filled.MoreVert, stringResource(R.string.action_more)) }
            }
        }
    }
}

@Composable
fun OfflineBadge(state: OfflineState) {
    when (state) {
        OfflineState.Downloaded -> { Icon(Icons.Filled.OfflinePin, stringResource(R.string.badge_downloaded), tint = MaterialTheme.colorScheme.primary, modifier = Modifier.size(14.dp)); Spacer(Modifier.width(4.dp)) }
        OfflineState.Cached -> { Icon(Icons.Filled.DownloadDone, stringResource(R.string.badge_cached), tint = MaterialTheme.colorScheme.outline, modifier = Modifier.size(14.dp)); Spacer(Modifier.width(4.dp)) }
        else -> Unit
    }
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
    SelectableRow(selected, selectionActive, onClick, onToggleSelect, label, modifier) {
        Column(Modifier.padding(6.dp)) {
            Box {
                Artwork(album.coverArt, ArtworkSizes.GRID, null, Modifier.fillMaxWidth().aspectRatio(1f), RoundedCornerShape(16.dp))
                if (selected) Box(Modifier.padding(8.dp)) {
                    Surface(shape = CircleShape, color = MaterialTheme.colorScheme.primary) { Icon(Icons.Filled.Check, null, Modifier.padding(4.dp), tint = MaterialTheme.colorScheme.onPrimary) }
                }
            }
            Spacer(Modifier.height(8.dp))
            Text(album.name, style = MaterialTheme.typography.titleSmall, maxLines = 1, overflow = TextOverflow.Ellipsis)
            Text(artist, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant, maxLines = 1, overflow = TextOverflow.Ellipsis)
        }
    }
}

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
            if (selected) Icon(Icons.Filled.Check, null, tint = MaterialTheme.colorScheme.primary)
        }
    }
}

@Composable
fun PlaylistRow(playlist: Playlist, onClick: () -> Unit, modifier: Modifier = Modifier, selected: Boolean = false, selectionActive: Boolean = false, onToggleSelect: () -> Unit = {}) {
    val label = stringResource(R.string.row_playlist_a11y, playlist.name, playlist.songCount.toInt())
    SelectableRow(selected, selectionActive, onClick, onToggleSelect, label, modifier) {
        Row(Modifier.heightIn(min = 64.dp).padding(horizontal = 16.dp, vertical = 8.dp), verticalAlignment = Alignment.CenterVertically) {
            Artwork(playlist.coverArt, ArtworkSizes.THUMB, null, Modifier.size(48.dp), RoundedCornerShape(8.dp))
            Spacer(Modifier.width(14.dp))
            Column(Modifier.weight(1f)) {
                Text(playlist.name, style = MaterialTheme.typography.bodyLarge, maxLines = 1, overflow = TextOverflow.Ellipsis)
                Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                    OfflineBadge(playlist.offline)
                    Text(stringResource(R.string.library_count_songs, playlist.songCount.toInt()), style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
                    if (playlist.isSmart) Badge(stringResource(R.string.badge_smart))
                }
            }
            if (selected) Icon(Icons.Filled.Check, null, tint = MaterialTheme.colorScheme.primary)
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
    Row(modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 10.dp), verticalAlignment = Alignment.CenterVertically) {
        Text(title, style = MaterialTheme.typography.titleMedium, modifier = Modifier.weight(1f))
        action?.invoke()
    }
}
