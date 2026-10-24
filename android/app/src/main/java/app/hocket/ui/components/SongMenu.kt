package app.hocket.ui.components

import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.Immutable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.Stable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.stringResource
import app.hocket.R
import app.hocket.core.Commands
import app.hocket.share.ShareRequest
import app.hocket.share.ShareRequestEffect
import app.hocket.core.api.ActionDescriptor
import app.hocket.core.api.ActionTarget
import app.hocket.core.api.OfflineState
import app.hocket.core.api.TrackSummary
import app.hocket.ui.DetailNavigator
import app.hocket.ui.LocalCoreClient
import app.hocket.ui.LocalDetailNavigator

/**
 * A row a context adds to the song menu (the playlist's "Remove from Late night", the player's sleep
 * timer). Extras always sit in one place: the short list's last rows, or (destructive ones) the
 * menu's last. [replaces] names a registry action the extra stands in for (the playlist's remove,
 * which knows the entry's position), so the menu does not list both.
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
    val replaces: String? = null,
)

/**
 * One opening of the song menu: the song, what its actions act on (queue rows pass their item), the
 * context's extras, and the playlist it was opened from (names the registry's "Remove from …").
 */
@Immutable
data class SongMenuRequest(val track: TrackSummary, val target: ActionTarget, val extras: List<SongMenuExtra> = emptyList(), val context: String? = null)

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

    fun open(track: TrackSummary, target: ActionTarget = Commands.tracks(listOf(track.id)), extras: List<SongMenuExtra> = emptyList(), context: String? = null) {
        if (shown == null) pending = SongMenuRequest(track, target, extras, context)
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
 * Hosts [controller]'s sheet ([MenuSheet] for one song). The registry's actions are fetched before
 * the sheet is shown, so it opens at its final height instead of growing (and re-anchoring) under
 * the finger. [navigator] performs "Go to album / artist" (the shell's collapses the player as it
 * navigates). Add to playlist and Song info open their own sheets here, after the menu has gone.
 */
@Composable
fun SongMenuHost(controller: SongMenuController, navigator: DetailNavigator?) {
    val client = LocalCoreClient.current
    val pending = controller.pending
    val pins by client.pins.collectAsStateWithLifecycleCompat()
    LaunchedEffect(pending) {
        val request = pending ?: return@LaunchedEffect
        val actions = client.actions("contextMenu", request.target)
        if (controller.pending == request) {
            controller.pending = null
            controller.shown = request to actions
        }
    }
    var pickerFor by remember { mutableStateOf<ActionTarget?>(null) }
    var infoFor by remember { mutableStateOf<TrackSummary?>(null) }
    var share by remember { mutableStateOf<ShareRequest?>(null) }
    controller.shown?.let { (request, actions) ->
        val track = request.track
        val dot = stringResource(R.string.dot_separator)
        val unknown = stringResource(R.string.unknown_artist)
        val subject = remember(request, pins) {
            MenuSubject(
                kind = MenuKind.Song,
                title = track.title,
                subtitle = listOfNotNull(track.artist ?: unknown, track.album).joinToString(dot),
                coverArt = track.coverArt,
                loved = if (track.loved) Tick.On else Tick.Off,
                rating = track.rating.toInt(),
                downloaded = track.offline == OfflineState.Downloaded || pins.any { it.target == Commands.trackPin(track.id) },
                trackId = track.id,
                albumId = track.albumId, album = track.album,
                artistId = track.artistId, artist = track.artist,
                context = request.context,
            )
        }
        // Keyed on the request: a new opening starts from a fresh sheet state, never a stale one.
        androidx.compose.runtime.key(request) {
            MenuSheet(
                subject, request.target, actions,
                onDismiss = { controller.close() },
                onAddToPlaylist = { pickerFor = request.target },
                extras = request.extras,
                onGoToAlbum = track.albumId?.let { id -> navigator?.let { { it.openAlbum(id) } } },
                onGoToArtist = track.artistId?.let { id -> navigator?.let { { it.openArtist(id) } } },
                onInfo = { infoFor = track },
                onShare = { share = ShareRequest(it, subject, request.target) },
            )
        }
    }
    ShareRequestEffect(share) { share = null }
    pickerFor?.let { t -> PlaylistPicker(target = t, onDismiss = { pickerFor = null }) }
    infoFor?.let { t ->
        ModalBottomSheet(onDismissRequest = { infoFor = null }, sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = true), modifier = Modifier.testTag("songMenu.infoSheet")) {
            app.hocket.ui.player.PlayerAbout(
                Modifier.fillMaxWidth().navigationBarsPadding(), track = t, title = stringResource(R.string.menu_song_info),
                onOpenAlbum = navigator?.let { n -> { id -> infoFor = null; n.openAlbum(id) } },
                onOpenArtist = navigator?.let { n -> { id -> infoFor = null; n.openArtist(id) } },
            )
        }
    }
}
