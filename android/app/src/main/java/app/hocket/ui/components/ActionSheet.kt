package app.hocket.ui.components

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Add
import androidx.compose.material.icons.filled.Album
import androidx.compose.material.icons.filled.Delete
import androidx.compose.material.icons.filled.Download
import androidx.compose.material.icons.filled.Favorite
import androidx.compose.material.icons.filled.FavoriteBorder
import androidx.compose.material.icons.filled.Person
import androidx.compose.material.icons.filled.PlayArrow
import androidx.compose.material.icons.filled.PlaylistAdd
import androidx.compose.material.icons.filled.PlaylistAddCheck
import androidx.compose.material.icons.filled.PlaylistPlay
import androidx.compose.material.icons.filled.Remove
import androidx.compose.material.icons.filled.Shuffle
import androidx.compose.material.icons.filled.Star
import androidx.compose.material.icons.filled.StarOutline
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.ListItem
import androidx.compose.material3.ListItemDefaults
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.produceState
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import app.hocket.R
import app.hocket.core.Commands
import app.hocket.core.Queries
import app.hocket.core.api.ActionDescriptor
import app.hocket.core.api.ActionTarget
import app.hocket.core.api.Playlist
import app.hocket.core.api.QueryResult
import app.hocket.ui.LocalCoreClient

/** Icon names the core's action registry emits, mapped to Material symbols. */
fun actionIcon(name: String): ImageVector = when (name) {
    "play_arrow" -> Icons.Filled.PlayArrow
    "shuffle" -> Icons.Filled.Shuffle
    "playlist_play" -> Icons.Filled.PlaylistPlay
    "playlist_add" -> Icons.Filled.PlaylistAdd
    "playlist_add_check" -> Icons.Filled.PlaylistAddCheck
    "favorite" -> Icons.Filled.Favorite
    "heart_broken" -> Icons.Filled.FavoriteBorder
    "star" -> Icons.Filled.Star
    "star_outline" -> Icons.Filled.StarOutline
    "download" -> Icons.Filled.Download
    "delete" -> Icons.Filled.Delete
    "remove" -> Icons.Filled.Remove
    "album" -> Icons.Filled.Album
    "person" -> Icons.Filled.Person
    else -> Icons.Filled.Add
}

/**
 * The context menu: actions come from `Query.Actions(surface = "contextMenu", target)`, ordered by the
 * user's customisation, and run through `Command.RunAction`. Only "add to playlist" (which needs a
 * picker) and the go-to navigation items are handled here.
 */
@Composable
fun ActionSheet(
    target: ActionTarget,
    title: String,
    subtitle: String? = null,
    onDismiss: () -> Unit,
    onGoToAlbum: (() -> Unit)? = null,
    onGoToArtist: (() -> Unit)? = null,
    extraTop: (@Composable () -> Unit)? = null,
) {
    val client = LocalCoreClient.current
    val sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = true)
    val actions by produceState<List<ActionDescriptor>?>(initialValue = null, target) { value = client.actions("contextMenu", target) }
    var playlistPicker by remember { mutableStateOf(false) }
    ModalBottomSheet(onDismissRequest = onDismiss, sheetState = sheetState) {
        Column(Modifier.navigationBarsPadding()) {
            Text(title, style = MaterialTheme.typography.titleMedium, modifier = Modifier.padding(horizontal = 24.dp))
            subtitle?.let { Text(it, style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant, modifier = Modifier.padding(horizontal = 24.dp)) }
            Spacer(Modifier.height(8.dp))
            extraTop?.invoke()
            HorizontalDivider()
            LazyColumn {
                items(actions ?: emptyList(), key = { it.id }) { a ->
                    ListItem(
                        headlineContent = { Text(a.label, color = if (a.destructive) MaterialTheme.colorScheme.error else Color.Unspecified) },
                        leadingContent = { Icon(actionIcon(a.icon), null, tint = if (a.destructive) MaterialTheme.colorScheme.error else MaterialTheme.colorScheme.onSurfaceVariant) },
                        colors = ListItemDefaults.colors(containerColor = Color.Transparent),
                        modifier = Modifier.clickable(enabled = a.enabled) {
                            if (a.id == "addToPlaylist") playlistPicker = true
                            else { client.dispatch(Commands.runAction(a.id, target)); onDismiss() }
                        },
                    )
                }
                if (onGoToAlbum != null) item { ListItem(headlineContent = { Text(stringResource(R.string.action_go_to_album)) }, leadingContent = { Icon(Icons.Filled.Album, null) }, colors = ListItemDefaults.colors(containerColor = Color.Transparent), modifier = Modifier.clickable { onDismiss(); onGoToAlbum() }) }
                if (onGoToArtist != null) item { ListItem(headlineContent = { Text(stringResource(R.string.action_go_to_artist)) }, leadingContent = { Icon(Icons.Filled.Person, null) }, colors = ListItemDefaults.colors(containerColor = Color.Transparent), modifier = Modifier.clickable { onDismiss(); onGoToArtist() }) }
                item { Spacer(Modifier.height(16.dp)) }
            }
        }
    }
    if (playlistPicker) {
        PlaylistPicker(target = target, onDismiss = { playlistPicker = false; onDismiss() })
    }
}

/** Picks an existing (non-smart, owned) playlist or creates a new one, then dispatches PlaylistAdd. */
@Composable
fun PlaylistPicker(target: ActionTarget, onDismiss: () -> Unit) {
    val client = LocalCoreClient.current
    val server by client.server.collectAsStateWithLifecycleCompat()
    val serverId = server?.id ?: return
    val playlists by produceState<List<Playlist>>(initialValue = emptyList(), serverId) {
        value = (client.query(Queries.playlists(serverId)) as? QueryResult.Playlists)?.data?.filter { !it.isSmart && it.isMine } ?: emptyList()
    }
    var newName by remember { mutableStateOf("") }
    val trackIds = remember(target) { mutableStateOf<List<String>?>(null) }
    LaunchedEffect(target) { trackIds.value = resolveTrackIds(client, target) }
    ModalBottomSheet(onDismissRequest = onDismiss, sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = true)) {
        Column(Modifier.navigationBarsPadding()) {
            Text(stringResource(R.string.action_add_to_playlist), style = MaterialTheme.typography.titleMedium, modifier = Modifier.padding(horizontal = 24.dp, vertical = 8.dp))
            Row(Modifier.fillMaxWidth().padding(horizontal = 24.dp), verticalAlignment = androidx.compose.ui.Alignment.CenterVertically) {
                OutlinedTextField(value = newName, onValueChange = { newName = it }, label = { Text(stringResource(R.string.playlist_new)) }, singleLine = true, modifier = Modifier.weight(1f))
                Spacer(Modifier.width(8.dp))
                TextButton(enabled = newName.isNotBlank() && trackIds.value != null, onClick = {
                    client.dispatch(Commands.createPlaylist(serverId, newName.trim(), trackIds.value ?: emptyList())); onDismiss()
                }) { Text(stringResource(R.string.action_save)) }
            }
            LazyColumn {
                items(playlists, key = { it.id }) { p ->
                    ListItem(
                        headlineContent = { Text(p.name) },
                        supportingContent = { Text(stringResource(R.string.library_count_songs, p.songCount.toInt())) },
                        leadingContent = { Artwork(p.coverArt, 64, null, Modifier.width(40.dp).height(40.dp)) },
                        colors = ListItemDefaults.colors(containerColor = Color.Transparent),
                        modifier = Modifier.clickable(enabled = trackIds.value != null) {
                            client.dispatch(Commands.playlistAdd(p.id, trackIds.value ?: emptyList())); onDismiss()
                        },
                    )
                }
                item { Spacer(Modifier.height(16.dp)) }
            }
        }
    }
}

/** Expands a target to track ids for playlist adds (albums/artists/playlists via their track queries). */
suspend fun resolveTrackIds(client: app.hocket.core.client.CoreClient, target: ActionTarget): List<String> = when (target) {
    is ActionTarget.Tracks -> target.data.ids
    is ActionTarget.Albums -> target.data.ids.flatMap { id -> (client.query(Queries.albumTracks(id)) as? QueryResult.TrackList)?.data?.map { it.id } ?: emptyList() }
    is ActionTarget.Artists -> target.data.ids.flatMap { id -> (client.query(Queries.artistTopSongs(id, 500)) as? QueryResult.TrackList)?.data?.map { it.id } ?: emptyList() }
    is ActionTarget.Playlists -> target.data.ids.flatMap { id -> (client.query(Queries.playlistTracks(id, app.hocket.core.api.Page(0u, 5000u))) as? QueryResult.Tracks)?.data?.items?.map { it.id } ?: emptyList() }
    is ActionTarget.QueueItems -> {
        val q = client.queue.value
        val all = q.history + listOfNotNull(q.current) + q.playingNext + q.upcoming
        target.data.keys.mapNotNull { k -> all.firstOrNull { it.item.key == k }?.track?.id }
    }
    else -> emptyList()
}
