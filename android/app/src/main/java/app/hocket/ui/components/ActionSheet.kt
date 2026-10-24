package app.hocket.ui.components

import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.produceState
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.platform.LocalResources
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.stringResource
import app.hocket.R
import app.hocket.core.Commands
import app.hocket.core.Queries
import app.hocket.core.api.ActionDescriptor
import app.hocket.core.api.ActionTarget
import app.hocket.core.api.Album
import app.hocket.core.api.OfflineState
import app.hocket.core.api.Pin
import app.hocket.core.api.QueryResult
import app.hocket.core.client.CoreClient
import app.hocket.share.ShareRequest
import app.hocket.share.ShareRequestEffect
import app.hocket.ui.LocalCoreClient
import app.hocket.ui.LocalDetailNavigator
import app.hocket.ui.icons.HocketIcons

/** Material Symbols names the core's action registry emits (`actions/defs.rs`), mapped to icons. */
fun actionIcon(name: String): ImageVector = when (name) {
    "play_arrow" -> HocketIcons.Filled.PlayArrow
    "play_pause" -> HocketIcons.Filled.PlayArrow
    "pause" -> HocketIcons.Filled.Pause
    "stop" -> HocketIcons.Filled.Stop
    "skip_next" -> HocketIcons.Filled.SkipNext
    "skip_previous" -> HocketIcons.Filled.SkipPrevious
    "shuffle", "shuffle_on" -> HocketIcons.Filled.Shuffle
    "repeat" -> HocketIcons.Filled.Repeat
    "all_inclusive" -> HocketIcons.Filled.AllInclusive
    "playlist_play" -> HocketIcons.Filled.PlaylistPlay
    "playlist_add" -> HocketIcons.Filled.PlaylistAdd
    "playlist_add_check" -> HocketIcons.Filled.PlaylistAddCheck
    "playlist_remove", "remove_from_queue" -> HocketIcons.Filled.PlaylistRemove
    "queue_music" -> HocketIcons.Filled.QueueMusic
    "favorite" -> HocketIcons.Filled.Favorite
    "heart_minus", "heart_broken" -> HocketIcons.Filled.FavoriteBorder
    "star", "star_rate" -> HocketIcons.Filled.Star
    "star_outline" -> HocketIcons.Filled.StarOutline
    "download" -> HocketIcons.Filled.Download
    "download_done" -> HocketIcons.Filled.DownloadDone
    "delete", "delete_forever" -> HocketIcons.Filled.Delete
    "remove" -> HocketIcons.Filled.Remove
    "clear_all" -> HocketIcons.Filled.ClearAll
    "album" -> HocketIcons.Filled.Album
    "artist", "person" -> HocketIcons.Filled.Person
    "history" -> HocketIcons.Filled.History
    "push_pin" -> HocketIcons.Filled.PushPin
    "keep_off" -> HocketIcons.Outlined.PushPin
    "bedtime" -> HocketIcons.Filled.Bedtime
    "cast" -> HocketIcons.Filled.Cast
    "lyrics" -> HocketIcons.Filled.Lyrics
    "undo" -> HocketIcons.AutoMirrored.Filled.Undo
    "redo" -> HocketIcons.AutoMirrored.Filled.Redo
    else -> HocketIcons.Filled.Add
}

/**
 * The menu of an album, an artist, a playlist or a saved queue: the same compact sheet as the song
 * menu ([MenuSheet]) with the item's header (an album's "artist · year" and "N songs · M min"),
 * built from `Query.Actions(surface = "contextMenu", target)` with the registry's ids and run
 * through `Command.RunAction`. The item and its actions are loaded before the sheet shows, so it
 * opens at its final height. [title] and [subtitle] head it until (or where) nothing more is known.
 * The registry's ui-handled ids are performed here: Add to playlist opens the picker, go-to
 * navigates, Album info opens the album's details. [extraTop] sits under the header.
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
    val resources = LocalResources.current
    val navigator = LocalDetailNavigator.current
    val dot = stringResource(R.string.dot_separator)
    val loaded by produceState<Triple<MenuSubject, Album?, List<ActionDescriptor>>?>(initialValue = null, target) {
        val (subject, album) = menuSubject(client, target, title, subtitle, resources, dot, client.pins.value)
        value = Triple(subject, album, client.actions("contextMenu", target))
    }
    // The sheet, then what it opened (the picker, the album's details); the caller hears once all are gone.
    var sheetOpen by remember { mutableStateOf(true) }
    var picker by remember { mutableStateOf(false) }
    var info by remember { mutableStateOf<Album?>(null) }
    var share by remember { mutableStateOf<ShareRequest?>(null) }
    val current = loaded
    if (sheetOpen && current != null) {
        val (subject, album, actions) = current
        MenuSheet(
            subject, target, actions,
            onDismiss = { sheetOpen = false; if (share == null && !picker && info == null) onDismiss() },
            onAddToPlaylist = { picker = true },
            onShare = { share = ShareRequest(it, subject, target) },
            onGoToAlbum = onGoToAlbum,
            onGoToArtist = onGoToArtist ?: subject.artistId?.let { id -> navigator?.let { { it.openArtist(id) } } },
            onInfo = album?.let { a -> { info = a } },
            extraTop = extraTop,
            tag = "itemMenu",
        )
    }
    ShareRequestEffect(share) { share = null; onDismiss() }
    if (picker) PlaylistPicker(target = target, onDismiss = { picker = false; onDismiss() })
    info?.let { a ->
        ModalBottomSheet(onDismissRequest = { info = null; onDismiss() }, sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = true), modifier = Modifier.testTag("itemMenu.infoSheet")) {
            app.hocket.ui.player.AlbumAbout(a, stringResource(R.string.menu_album_info), Modifier.fillMaxWidth().navigationBarsPadding(),
                onOpenArtist = navigator?.let { n -> { id -> info = null; onDismiss(); n.openArtist(id) } })
        }
    }
}

/** "1 h 12 min" / "42 min", outside composition. */
private fun durationWords(resources: android.content.res.Resources, ms: Long): String {
    val minutes = (ms / 60_000).toInt()
    return if (minutes >= 60) resources.getString(R.string.duration_hours_minutes, minutes / 60, minutes % 60) else resources.getString(R.string.duration_minutes, minutes)
}

/** The header a target's menu shows (and, for an album, the album, for its info sheet). */
private suspend fun menuSubject(
    client: CoreClient,
    target: ActionTarget,
    title: String,
    subtitle: String?,
    resources: android.content.res.Resources,
    dot: String,
    pins: List<Pin>,
): Pair<MenuSubject, Album?> {
    fun songs(n: Int) = resources.getQuantityString(R.plurals.menu_songs, n, n)
    val fallback = MenuSubject(MenuKind.Other, title, subtitle)
    return when (target) {
        is ActionTarget.Albums -> {
            val a = target.data.ids.singleOrNull()?.let { (client.query(Queries.album(it)) as? QueryResult.AlbumDetail)?.data } ?: return fallback.copy(kind = MenuKind.Album) to null
            MenuSubject(
                kind = MenuKind.Album,
                title = a.name,
                subtitle = listOfNotNull(a.artist ?: subtitle, a.year?.takeIf { it > 0u }?.toString()).joinToString(dot).ifEmpty { null },
                meta = songs(a.songCount.toInt()) + dot + durationWords(resources, a.durationMs.toLong()),
                coverArt = a.coverArt,
                loved = if (a.loved) Tick.On else Tick.Off,
                downloaded = a.offline == OfflineState.Downloaded || pins.any { it.target == Commands.albumPin(a.id) },
                downloadNote = songs(a.songCount.toInt()),
                albumId = a.id, album = a.name,
                artistId = a.artistId, artist = a.artist,
                count = a.songCount.toInt(),
            ) to a
        }
        is ActionTarget.Artists -> {
            val ar = target.data.ids.singleOrNull()?.let { (client.query(Queries.artist(it)) as? QueryResult.ArtistDetail)?.data } ?: return fallback.copy(kind = MenuKind.Artist) to null
            MenuSubject(MenuKind.Artist, ar.name, songs(ar.songCount.toInt()), coverArt = ar.coverArt, loved = if (ar.loved) Tick.On else Tick.Off, artistId = ar.id, artist = ar.name) to null
        }
        is ActionTarget.Playlists -> {
            val p = target.data.ids.singleOrNull()?.let { (client.query(Queries.playlist(it)) as? QueryResult.PlaylistDetail)?.data } ?: return fallback.copy(kind = MenuKind.Playlist) to null
            MenuSubject(
                kind = MenuKind.Playlist,
                title = p.name,
                subtitle = p.owner?.let { resources.getString(R.string.playlist_by, it) } ?: subtitle,
                meta = songs(p.songCount.toInt()) + dot + durationWords(resources, p.durationMs.toLong()),
                coverArt = p.coverArt,
                downloaded = p.offline == OfflineState.Downloaded || pins.any { it.target == Commands.playlistPin(p.id) },
                downloadNote = songs(p.songCount.toInt()),
            ) to null
        }
        else -> fallback to null
    }
}
