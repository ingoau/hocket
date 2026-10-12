package app.hocket.ui.player

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.Text
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.produceState
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import app.hocket.R
import app.hocket.core.ArtworkSizes
import app.hocket.core.Queries
import app.hocket.core.api.QueryResult
import app.hocket.core.api.TrackSummary
import app.hocket.ui.LocalCoreClient
import app.hocket.ui.components.Artwork
import app.hocket.ui.icons.HocketIcons

/** One of a song's artists: its library id (when the server gave one) and its name. */
internal data class TrackArtist(val id: String?, val name: String)

/**
 * The song's artists, in the order the server lists them (the core's `artists`), or without that
 * list the one artist the summary names (its `artistId` and its artist string).
 */
internal fun TrackSummary.trackArtists(): List<TrackArtist> =
    artists?.filter { it.name.isNotBlank() }?.takeIf { it.isNotEmpty() }?.map { TrackArtist(it.id, it.name) }
        ?: listOfNotNull(artist?.takeIf { it.isNotBlank() }?.let { TrackArtist(artistId, it) })

/**
 * The artist line's sheet: every artist on the song (avatar, name; a tap opens the artist's page),
 * then a divider and the song's album (cover, name, album artist; a tap opens the album).
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
internal fun ArtistsSheet(track: TrackSummary, onDismiss: () -> Unit, onOpenAlbum: (String) -> Unit, onOpenArtist: (String) -> Unit) {
    ModalBottomSheet(onDismissRequest = onDismiss, sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = true), modifier = Modifier.testTag("player.artists")) {
        Column(Modifier.fillMaxWidth().navigationBarsPadding().padding(bottom = 16.dp)) {
            Text(
                stringResource(R.string.player_artists_title),
                style = MaterialTheme.typography.titleLargeEmphasized,
                fontWeight = FontWeight.Bold,
                modifier = Modifier.padding(horizontal = 24.dp, vertical = 8.dp).semantics { heading() },
            )
            for ((i, artist) in track.trackArtists().withIndex()) ArtistItem(artist, onOpenArtist, Modifier.testTag("player.artists.artist.$i"))
            val albumName = track.album
            if (albumName != null) {
                HorizontalDivider(Modifier.padding(horizontal = 24.dp, vertical = 8.dp), color = MaterialTheme.colorScheme.outlineVariant)
                AlbumItem(track, albumName, onOpenAlbum)
            }
        }
    }
}

@Composable
private fun ArtistItem(artist: TrackArtist, onOpenArtist: (String) -> Unit, modifier: Modifier = Modifier) {
    val client = LocalCoreClient.current
    val id = artist.id
    // The artist's picture, where the library has one.
    val cover by produceState<String?>(null, id) {
        value = id?.let { (client.query(Queries.artist(it)) as? QueryResult.ArtistDetail)?.data?.coverArt }
    }
    SheetItem(
        onClick = id?.let { { onOpenArtist(it) } },
        clickLabel = stringResource(R.string.action_go_to_artist),
        modifier = modifier,
        leading = { Artwork(cover, ArtworkSizes.THUMB, null, Modifier.size(48.dp), CircleShape) },
    ) {
        Text(artist.name, style = MaterialTheme.typography.bodyLarge, maxLines = 2, overflow = TextOverflow.Ellipsis)
    }
}

@Composable
private fun AlbumItem(track: TrackSummary, name: String, onOpenAlbum: (String) -> Unit) {
    val client = LocalCoreClient.current
    val id = track.albumId
    // The album's own artist (a compilation's "Various Artists", say), once the library answers.
    val albumArtist by produceState<String?>(null, id) {
        value = id?.let { (client.query(Queries.album(it)) as? QueryResult.AlbumDetail)?.data?.artist }
    }
    SheetItem(
        onClick = id?.let { { onOpenAlbum(it) } },
        clickLabel = stringResource(R.string.action_go_to_album),
        modifier = Modifier.testTag("player.artists.album"),
        leading = { Artwork(track.coverArt, ArtworkSizes.THUMB, null, Modifier.size(48.dp), RoundedCornerShape(8.dp)) },
    ) {
        Text(stringResource(R.string.player_about_album), style = MaterialTheme.typography.labelMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
        Text(name, style = MaterialTheme.typography.bodyLarge, maxLines = 2, overflow = TextOverflow.Ellipsis)
        (albumArtist ?: track.artist)?.let { Text(it, style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant, maxLines = 1, overflow = TextOverflow.Ellipsis) }
    }
}

/** A row of the sheet: one item for a screen reader, with "Go to …" where it links. */
@Composable
private fun SheetItem(onClick: (() -> Unit)?, clickLabel: String, modifier: Modifier, leading: @Composable () -> Unit, content: @Composable () -> Unit) {
    val base = modifier.fillMaxWidth().heightIn(min = 64.dp)
    Row(
        (if (onClick != null) base.clickable(onClickLabel = clickLabel, onClick = onClick) else base)
            .semantics(mergeDescendants = true) { }
            .padding(horizontal = 24.dp, vertical = 8.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        leading()
        Spacer(Modifier.width(16.dp))
        Column(Modifier.weight(1f), verticalArrangement = Arrangement.Center) { content() }
        if (onClick != null) Icon(HocketIcons.Filled.ChevronRight, null, tint = MaterialTheme.colorScheme.onSurfaceVariant, modifier = Modifier.padding(start = 8.dp))
    }
}
