package app.hocket.ui.player

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.IntrinsicSize
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.wrapContentHeight
import androidx.compose.foundation.selection.selectableGroup
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.ButtonGroupDefaults
import androidx.compose.material3.FilledTonalButton
import androidx.compose.material3.FilledTonalIconButton
import androidx.compose.material3.IconButton
import androidx.compose.material3.IconButtonDefaults
import androidx.compose.material3.ListItemDefaults
import androidx.compose.material3.LocalContentColor
import androidx.compose.material3.TextButton
import androidx.compose.material3.ToggleButton
import androidx.compose.material3.ToggleButtonDefaults
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Shape
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.role
import androidx.compose.ui.semantics.selected
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.Dp
import app.hocket.core.ArtworkSizes
import app.hocket.core.api.RelatedTrack
import app.hocket.ui.components.Artwork
import app.hocket.ui.components.Segment
import app.hocket.ui.components.segmentColor
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.material3.AssistChip
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import app.hocket.ui.icons.HocketIcons
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.produceState
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.onClick
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import app.hocket.R
import app.hocket.core.Commands
import app.hocket.core.Queries
import app.hocket.core.api.QueryResult
import app.hocket.core.api.Album
import app.hocket.core.api.ExternalInfo
import app.hocket.core.api.Track
import app.hocket.core.api.TrackSummary
import app.hocket.ui.LocalCoreClient
import app.hocket.ui.components.RatingStars
import app.hocket.ui.components.formatBytes
import app.hocket.ui.components.formatClock
import java.text.DateFormat
import java.util.Date
import java.util.Locale

/**
 * The song's details, the info sheet's content (from a tap on the player's title, or a song menu's
 * "Song info"): a header (the cover, the title, the artist and album, which go to their pages),
 * then three tabs as a connected button group. Song: plays and last played, the rating (one
 * adjustable control, tag `rating`), length, track, year and genre as tiles, the album and artist
 * as links, the comment. About: what the server adds from Last.fm and MusicBrainz (the album's
 * notes, the artist's biography, similar songs and artists, the two sites). File: format, quality,
 * bitrate, size, ReplayGain, the path and MusicBrainz id (copyable). Fetched once per track; while
 * that loads, the summary's fields show on their own. No other actions.
 */
@Composable
internal fun PlayerAbout(
    modifier: Modifier = Modifier,
    contentPadding: PaddingValues = PaddingValues(),
    onOpenAlbum: ((String) -> Unit)? = null,
    onOpenArtist: ((String) -> Unit)? = null,
    track: TrackSummary? = null,
    title: String = stringResource(R.string.player_about_title),
    onClose: (() -> Unit)? = null,
) {
    val client = LocalCoreClient.current
    val entry by client.nowPlaying.collectAsStateWithLifecycle()
    // [track] when given (a song menu's "Song info"), otherwise the song playing.
    val summary = track ?: entry?.track
    val id = summary?.id
    val detail by produceState<Track?>(initialValue = null, id) {
        value = null
        value = id?.let { (client.query(Queries.track(it)) as? QueryResult.TrackDetail)?.data }
    }
    val d = detail?.takeIf { it.id == id }
    val albumId = summary?.albumId
    val artistId = summary?.artistId
    val album by produceState<Album?>(initialValue = null, albumId) {
        value = null
        value = albumId?.let { (client.query(Queries.album(it)) as? QueryResult.AlbumDetail)?.data }
    }
    val albumInfo by produceState<ExternalInfo?>(initialValue = null, albumId) {
        value = null
        value = albumId?.let { (client.query(Queries.albumInfo(it)) as? QueryResult.ExternalInfo)?.data }
    }
    val artistInfo by produceState<ExternalInfo?>(initialValue = null, artistId) {
        value = null
        value = artistId?.let { (client.query(Queries.artistInfo(it)) as? QueryResult.ExternalInfo)?.data }
    }
    val similar by produceState<List<RelatedTrack>>(initialValue = emptyList(), id) {
        value = emptyList()
        value = id?.let { (client.query(Queries.related(it, 4)) as? QueryResult.Related)?.data }.orEmpty()
    }
    var tab by rememberSaveable { mutableIntStateOf(0) }
    val dot = stringResource(R.string.dot_separator)
    LazyColumn(
        modifier.testTag("player.about"),
        contentPadding = PaddingValues(top = contentPadding.calculateTopPadding(), bottom = contentPadding.calculateBottomPadding() + 32.dp),
    ) {
        if (summary == null) return@LazyColumn
        item(key = "head") { AboutHeader(summary, title, onClose, onOpenAlbum, onOpenArtist) }
        item(key = "tabs") {
            val tabs = listOf(R.string.about_tab_song, R.string.about_tab_about, R.string.about_tab_file)
            Row(Modifier.fillMaxWidth().padding(start = 16.dp, end = 16.dp, top = 14.dp, bottom = 10.dp).selectableGroup(), horizontalArrangement = Arrangement.spacedBy(ButtonGroupDefaults.ConnectedSpaceBetween)) {
                tabs.forEachIndexed { i, label ->
                    ToggleButton(
                        checked = tab == i, onCheckedChange = { tab = i },
                        shapes = when (i) { 0 -> ButtonGroupDefaults.connectedLeadingButtonShapes(); tabs.lastIndex -> ButtonGroupDefaults.connectedTrailingButtonShapes(); else -> ButtonGroupDefaults.connectedMiddleButtonShapes() },
                        colors = ToggleButtonDefaults.colors(containerColor = MaterialTheme.colorScheme.secondaryContainer, contentColor = MaterialTheme.colorScheme.onSecondaryContainer),
                        modifier = Modifier.weight(1f).heightIn(min = 48.dp).semantics { role = Role.Tab; selected = tab == i }.testTag("player.about.tab.$i"),
                    ) { Text(stringResource(label)) }
                }
            }
        }
        when (tab) {
            0 -> item(key = "song") { SongTab(summary, d, album, dot, onOpenAlbum, onOpenArtist) }
            1 -> item(key = "about") { AboutTab(albumInfo, artistInfo, similar, summary, onOpenArtist) }
            else -> item(key = "file") { FileTab(d) }
        }
    }
}

@Composable
private fun AboutHeader(summary: TrackSummary, title: String, onClose: (() -> Unit)?, onOpenAlbum: ((String) -> Unit)?, onOpenArtist: ((String) -> Unit)?) {
    Column {
        Row(Modifier.fillMaxWidth().padding(start = if (onClose != null) 4.dp else 16.dp, end = 16.dp, top = 4.dp, bottom = 16.dp), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(12.dp)) {
            if (onClose != null) IconButton(onClick = onClose, shapes = IconButtonDefaults.shapes()) { Icon(HocketIcons.Filled.Close, stringResource(R.string.about_close)) }
            Artwork(summary.coverArt, ArtworkSizes.LIST, null, Modifier.size(64.dp), RoundedCornerShape(16.dp))
            Column(Modifier.weight(1f)) {
                // The sheet's name for a screen reader; the song's title is what shows.
                Text(summary.title, style = MaterialTheme.typography.titleLarge, fontWeight = FontWeight.Bold, maxLines = 2, overflow = TextOverflow.Ellipsis,
                    modifier = Modifier.semantics { heading(); contentDescription = "$title: ${summary.title}" })
                Row(verticalAlignment = Alignment.CenterVertically) {
                    summary.artist?.let { a ->
                        Text(a, style = MaterialTheme.typography.bodyLarge, fontWeight = FontWeight.SemiBold, maxLines = 1, overflow = TextOverflow.Ellipsis,
                            modifier = Modifier.weight(1f, fill = false).heightIn(min = 36.dp).wrapContentHeight().then(if (onOpenArtist != null && summary.artistId != null) Modifier.clickable(onClickLabel = stringResource(R.string.action_go_to_artist)) { onOpenArtist(summary.artistId!!) } else Modifier))
                    }
                    summary.album?.let { al ->
                        Text(stringResource(R.string.dot_separator), style = MaterialTheme.typography.bodyLarge, color = MaterialTheme.colorScheme.onSurfaceVariant)
                        Text(al, style = MaterialTheme.typography.bodyLarge, color = MaterialTheme.colorScheme.onSurfaceVariant, maxLines = 1, overflow = TextOverflow.Ellipsis,
                            modifier = Modifier.weight(1f, fill = false).heightIn(min = 36.dp).wrapContentHeight().then(if (onOpenAlbum != null && summary.albumId != null) Modifier.clickable(onClickLabel = stringResource(R.string.action_go_to_album)) { onOpenAlbum(summary.albumId!!) } else Modifier))
                    }
                }
            }
        }
        HorizontalDivider(color = MaterialTheme.colorScheme.outlineVariant)
    }
}

/** A bento tile: a small label over a value, in a group's segment shape. */
@Composable
private fun Tile(label: String, shape: Shape, modifier: Modifier = Modifier, container: Color = segmentColor(), content: Color = MaterialTheme.colorScheme.onSurface, minHeight: Dp = 88.dp, body: @Composable ColumnScope.() -> Unit) {
    Surface(shape = shape, color = container, contentColor = content, modifier = modifier) {
        Column(Modifier.heightIn(min = minHeight).padding(horizontal = 14.dp, vertical = 14.dp).semantics(mergeDescendants = true) { }, verticalArrangement = Arrangement.SpaceBetween) {
            Text(label, style = MaterialTheme.typography.labelLarge, color = content.copy(alpha = 0.78f))
            body()
        }
    }
}

private val Big = 22.dp
private val Small = 8.dp
private fun corners(ts: Dp = Small, te: Dp = Small, bs: Dp = Small, be: Dp = Small) = RoundedCornerShape(topStart = ts, topEnd = te, bottomStart = bs, bottomEnd = be)

@Composable
private fun TileValue(value: String, suffix: String? = null) {
    Row(verticalAlignment = Alignment.Bottom) {
        Text(value, style = MaterialTheme.typography.headlineMedium, fontWeight = FontWeight.Bold, maxLines = 1, overflow = TextOverflow.Ellipsis, modifier = Modifier.weight(1f, fill = false))
        if (suffix != null) Text(suffix, style = MaterialTheme.typography.bodyMedium, color = LocalContentColor.current.copy(alpha = 0.78f), maxLines = 1, modifier = Modifier.padding(start = 2.dp, bottom = 3.dp))
    }
}

@Composable
private fun SongTab(summary: TrackSummary, d: Track?, album: Album?, dot: String, onOpenAlbum: ((String) -> Unit)?, onOpenArtist: ((String) -> Unit)?) {
    val client = LocalCoreClient.current
    val gap = ListItemDefaults.SegmentedGap
    Column(Modifier.padding(horizontal = 16.dp), verticalArrangement = Arrangement.spacedBy(gap)) {
        Row(Modifier.height(IntrinsicSize.Min), horizontalArrangement = Arrangement.spacedBy(gap)) {
            Tile(stringResource(R.string.player_about_play_count), corners(ts = Big), Modifier.weight(1f).fillMaxHeight(), container = MaterialTheme.colorScheme.primary, content = MaterialTheme.colorScheme.onPrimary, minHeight = 112.dp) {
                Text((d?.playCount ?: 0u).toString(), style = MaterialTheme.typography.displayMedium, fontWeight = FontWeight.Bold)
            }
            Tile(stringResource(R.string.player_about_last_played), corners(te = Big), Modifier.weight(1f).fillMaxHeight(), minHeight = 112.dp) {
                val last = d?.lastPlayed
                if (last == null) TileValue(stringResource(R.string.about_never))
                else Column {
                    TileValue(relativeDay(last.toLong()))
                    Text(DateFormat.getDateTimeInstance(DateFormat.MEDIUM, DateFormat.SHORT).format(Date(last.toLong())), style = MaterialTheme.typography.bodySmall, color = LocalContentColor.current.copy(alpha = 0.78f), maxLines = 1)
                }
            }
        }
        Surface(shape = corners(), color = segmentColor()) {
            Row(Modifier.fillMaxWidth().heightIn(min = 64.dp).padding(start = 14.dp, end = 6.dp), verticalAlignment = Alignment.CenterVertically) {
                Text(stringResource(R.string.player_about_rating), style = MaterialTheme.typography.labelLarge, color = MaterialTheme.colorScheme.onSurfaceVariant, modifier = Modifier.weight(1f))
                RatingStars(summary.rating.toInt(), onRate = { client.dispatch(Commands.rateTrack(summary.id, it)) }, starSize = 26.dp, spacing = 16.dp, tint = MaterialTheme.colorScheme.primary)
            }
        }
        Row(Modifier.height(IntrinsicSize.Min), horizontalArrangement = Arrangement.spacedBy(gap)) {
            Tile(stringResource(R.string.player_about_duration), corners(), Modifier.weight(1f).fillMaxHeight()) { TileValue(formatClock(summary.durationMs)) }
            Tile(stringResource(R.string.player_about_track), corners(), Modifier.weight(1f).fillMaxHeight()) {
                val n = d?.trackNumber?.takeIf { it > 0u }
                if (n == null) TileValue("–")
                else TileValue(n.toString(), listOfNotNull(album?.songCount?.takeIf { it > 0u }?.let { stringResource(R.string.about_of, it.toInt()) }, d.discNumber?.takeIf { it > 0u }?.let { stringResource(R.string.about_disc, it.toInt()) }).joinToString(dot).ifEmpty { null }?.let { " $it" })
            }
        }
        Row(Modifier.height(IntrinsicSize.Min), horizontalArrangement = Arrangement.spacedBy(gap)) {
            Tile(stringResource(R.string.player_about_year), corners(bs = Big), Modifier.weight(1f).fillMaxHeight()) { TileValue(d?.year?.takeIf { it > 0u }?.toString() ?: "–") }
            Tile(stringResource(R.string.player_about_genre), corners(be = Big), Modifier.weight(1f).fillMaxHeight()) { TileValue(d?.genre?.takeIf { it.isNotBlank() } ?: "–") }
        }
        Spacer(Modifier.height(14.dp))
        val links = listOfNotNull(
            summary.albumId?.takeIf { onOpenAlbum != null }?.let { aid -> Triple(stringResource(R.string.player_about_album), summary.album.orEmpty()) { onOpenAlbum!!(aid) } },
            summary.artistId?.takeIf { onOpenArtist != null }?.let { aid -> Triple(stringResource(R.string.player_about_artist), summary.artist.orEmpty()) { onOpenArtist!!(aid) } },
        )
        links.forEachIndexed { i, (label, name, open) ->
            Segment(i, links.size, onClick = open) {
                Row(Modifier.heightIn(min = 72.dp).padding(horizontal = 12.dp), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(14.dp)) {
                    val isArtist = label == stringResource(R.string.player_about_artist)
                    Artwork(if (isArtist) null else summary.coverArt, ArtworkSizes.THUMB, null, Modifier.size(52.dp), if (isArtist) CircleShape else RoundedCornerShape(14.dp))
                    Column(Modifier.weight(1f)) {
                        Text(label, style = MaterialTheme.typography.labelLarge, color = MaterialTheme.colorScheme.onSurfaceVariant)
                        Text(name, style = MaterialTheme.typography.titleMedium, fontWeight = FontWeight.SemiBold, maxLines = 1, overflow = TextOverflow.Ellipsis)
                    }
                    Icon(HocketIcons.Filled.ChevronRight, null, tint = MaterialTheme.colorScheme.onSurfaceVariant)
                }
            }
        }
        d?.comment?.takeIf { it.isNotBlank() }?.let { c ->
            Spacer(Modifier.height(14.dp))
            Surface(shape = RoundedCornerShape(Big), color = MaterialTheme.colorScheme.surfaceContainer) {
                Column(Modifier.fillMaxWidth().padding(horizontal = 18.dp, vertical = 16.dp).semantics(mergeDescendants = true) { }) {
                    Text(stringResource(R.string.player_about_comment), style = MaterialTheme.typography.labelLarge, color = MaterialTheme.colorScheme.onSurfaceVariant)
                    Text("“$c”", style = MaterialTheme.typography.titleMedium, modifier = Modifier.padding(top = 6.dp))
                }
            }
        }
        if (d == null) Text(stringResource(R.string.player_about_loading), style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant, modifier = Modifier.padding(4.dp))
    }
}

@Composable
private fun AboutTab(albumInfo: ExternalInfo?, artistInfo: ExternalInfo?, similar: List<RelatedTrack>, summary: TrackSummary, onOpenArtist: ((String) -> Unit)?) {
    val client = LocalCoreClient.current
    val uri = androidx.compose.ui.platform.LocalUriHandler.current
    val parts = buildList<@Composable (Shape) -> Unit> {
        albumInfo?.text?.takeIf { it.isNotBlank() }?.let { text -> add { shape -> TextCard(stringResource(R.string.player_about_album_notes), text, shape) } }
        artistInfo?.text?.takeIf { it.isNotBlank() }?.let { text -> add { shape -> TextCard(stringResource(R.string.player_about_artist_bio), text, shape) } }
        if (similar.isNotEmpty()) add { shape ->
            Surface(shape = shape, color = segmentColor()) {
                Column(Modifier.fillMaxWidth().padding(start = 12.dp, end = 12.dp, top = 14.dp, bottom = 12.dp)) {
                    Text(stringResource(R.string.about_similar_songs), style = MaterialTheme.typography.titleMedium, fontWeight = FontWeight.SemiBold, modifier = Modifier.padding(start = 4.dp, bottom = 10.dp).semantics { heading() })
                    similar.chunked(2).forEach { row ->
                        Row(Modifier.padding(bottom = 8.dp), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                            row.forEach { r ->
                                val playNext = { client.dispatch(Commands.playNext(r.track.serverId, listOf(r.track.id))) }
                                val playNextLabel = stringResource(R.string.action_play_next)
                                Surface(onClick = playNext, shape = RoundedCornerShape(16.dp), color = MaterialTheme.colorScheme.surfaceContainer, modifier = Modifier.weight(1f)
                                    .semantics { contentDescription = "${r.track.title}, ${r.track.artist.orEmpty()}"; onClick(label = playNextLabel) { playNext(); true } }) {
                                    Row(Modifier.heightIn(min = 56.dp).padding(6.dp), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(10.dp)) {
                                        Artwork(r.track.coverArt, ArtworkSizes.THUMB, null, Modifier.size(44.dp), RoundedCornerShape(12.dp))
                                        Column(Modifier.weight(1f)) {
                                            Text(r.track.title, style = MaterialTheme.typography.labelLarge, maxLines = 1, overflow = TextOverflow.Ellipsis)
                                            Text(r.track.artist.orEmpty(), style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant, maxLines = 1, overflow = TextOverflow.Ellipsis)
                                        }
                                    }
                                }
                            }
                            if (row.size == 1) Spacer(Modifier.weight(1f))
                        }
                    }
                }
            }
        }
        artistInfo?.similarArtists?.takeIf { it.isNotEmpty() }?.let { list -> add { shape ->
            Surface(shape = shape, color = segmentColor()) {
                Column(Modifier.fillMaxWidth().padding(top = 14.dp, bottom = 10.dp)) {
                    Text(stringResource(R.string.player_about_similar_artists), style = MaterialTheme.typography.titleMedium, fontWeight = FontWeight.SemiBold, modifier = Modifier.padding(start = 16.dp, bottom = 10.dp).semantics { heading() })
                    androidx.compose.foundation.lazy.LazyRow(contentPadding = PaddingValues(horizontal = 12.dp), horizontalArrangement = Arrangement.spacedBy(4.dp)) {
                        items(list.size) { i ->
                            val a = list[i]
                            Column(Modifier.width(84.dp).clickable(enabled = onOpenArtist != null) { onOpenArtist?.invoke(a.id) }.padding(vertical = 4.dp), horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.spacedBy(6.dp)) {
                                Artwork(a.coverArt, ArtworkSizes.THUMB, null, Modifier.size(68.dp), CircleShape)
                                Text(a.name, style = MaterialTheme.typography.bodySmall, maxLines = 2, overflow = TextOverflow.Ellipsis, textAlign = TextAlign.Center)
                            }
                        }
                    }
                }
            }
        } }
        val lastFm = artistInfo?.lastFmUrl ?: albumInfo?.lastFmUrl
        // The album's id is a MusicBrainz release, the artist's an artist.
        val mbid = albumInfo?.musicBrainzId?.let { "release/$it" } ?: artistInfo?.musicBrainzId?.let { "artist/$it" }
        if (lastFm != null || mbid != null) add { shape ->
            Row(Modifier.fillMaxWidth().height(IntrinsicSize.Min), horizontalArrangement = Arrangement.spacedBy(ListItemDefaults.SegmentedGap)) {
                val tonal = ButtonDefaults.filledTonalButtonColors()
                lastFm?.let { url -> FilledTonalButton(onClick = { runCatching { uri.openUri(url) } }, colors = tonal, shape = if (mbid != null) corners(bs = Big) else corners(bs = Big, be = Big), modifier = Modifier.weight(1f).heightIn(min = 56.dp)) { Text("Last.fm"); Spacer(Modifier.width(8.dp)); Icon(HocketIcons.Filled.OpenInNew, null, Modifier.size(18.dp)) } }
                mbid?.let { m -> FilledTonalButton(onClick = { runCatching { uri.openUri("https://musicbrainz.org/$m") } }, colors = tonal, shape = if (lastFm != null) corners(be = Big) else corners(bs = Big, be = Big), modifier = Modifier.weight(1f).heightIn(min = 56.dp)) { Text("MusicBrainz"); Spacer(Modifier.width(8.dp)); Icon(HocketIcons.Filled.OpenInNew, null, Modifier.size(18.dp)) } }
            }
        }
    }
    Column(Modifier.padding(horizontal = 16.dp), verticalArrangement = Arrangement.spacedBy(ListItemDefaults.SegmentedGap)) {
        if (parts.isEmpty()) Text(stringResource(R.string.about_nothing), style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant, modifier = Modifier.padding(8.dp))
        parts.forEachIndexed { i, part -> part(when { parts.size == 1 -> RoundedCornerShape(Big); i == 0 -> corners(ts = Big, te = Big); i == parts.lastIndex -> corners(bs = Big, be = Big); else -> corners() }) }
        if (albumInfo != null || artistInfo != null) Text(stringResource(R.string.player_about_source), style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant, modifier = Modifier.padding(start = 4.dp, top = 8.dp))
    }
}

/** A titled card of server-provided text, clamped with More / Less. */
@Composable
private fun TextCard(title: String, text: String, shape: Shape) {
    var open by rememberSaveable(text) { mutableStateOf(false) }
    Surface(shape = shape, color = segmentColor()) {
        Column(Modifier.fillMaxWidth().padding(start = 16.dp, end = 16.dp, top = 16.dp, bottom = 4.dp)) {
            Text(title, style = MaterialTheme.typography.titleMedium, fontWeight = FontWeight.SemiBold, modifier = Modifier.semantics { heading() })
            Text(text, style = MaterialTheme.typography.bodyMedium, maxLines = if (open) Int.MAX_VALUE else 5, overflow = TextOverflow.Ellipsis, modifier = Modifier.padding(top = 8.dp))
            TextButton(onClick = { open = !open }, modifier = Modifier.offset(x = (-12).dp)) { Text(stringResource(if (open) R.string.about_less else R.string.about_more)) }
        }
    }
}

@Composable
private fun FileTab(d: Track?) {
    val gap = ListItemDefaults.SegmentedGap
    val clipboard = androidx.compose.ui.platform.LocalClipboardManager.current
    Column(Modifier.padding(horizontal = 16.dp), verticalArrangement = Arrangement.spacedBy(gap)) {
        if (d == null) { Text(stringResource(R.string.player_about_loading), style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant, modifier = Modifier.padding(4.dp)); return@Column }
        Row(Modifier.height(IntrinsicSize.Min), horizontalArrangement = Arrangement.spacedBy(gap)) {
            Tile(stringResource(R.string.player_about_format), corners(ts = Big), Modifier.weight(1f).fillMaxHeight(), container = MaterialTheme.colorScheme.secondaryContainer, content = MaterialTheme.colorScheme.onSecondaryContainer, minHeight = 112.dp) {
                Column {
                    Text(d.suffix?.uppercase(Locale.ROOT) ?: "–", style = MaterialTheme.typography.displaySmall, fontWeight = FontWeight.Bold, maxLines = 1)
                    d.contentType?.let { Text(it, style = MaterialTheme.typography.bodySmall, maxLines = 1) }
                }
            }
            Tile(stringResource(R.string.about_quality), corners(te = Big), Modifier.weight(1f).fillMaxHeight(), minHeight = 112.dp) {
                Column {
                    TileValue(d.bitDepth?.takeIf { it > 0u }?.let { stringResource(R.string.player_about_bit_depth_value, it.toInt()) } ?: d.sampleRate?.let { stringResource(R.string.player_about_sample_rate_value, kilo(it.toInt())) } ?: "–")
                    Text(listOfNotNull(d.sampleRate?.takeIf { it > 0u && d.bitDepth != null }?.let { stringResource(R.string.player_about_sample_rate_value, kilo(it.toInt())) }, d.channels?.takeIf { it > 0u }?.let { pluralStringResource(R.plurals.about_channels, it.toInt(), it.toInt()) }).joinToString(stringResource(R.string.dot_separator)),
                        style = MaterialTheme.typography.bodySmall, color = LocalContentColor.current.copy(alpha = 0.78f), maxLines = 1)
                }
            }
        }
        Row(Modifier.height(IntrinsicSize.Min), horizontalArrangement = Arrangement.spacedBy(gap)) {
            Tile(stringResource(R.string.player_about_bitrate), corners(), Modifier.weight(1f).fillMaxHeight()) { TileValue(d.bitRate?.takeIf { it > 0u }?.toString() ?: "–", d.bitRate?.takeIf { it > 0u }?.let { " " + stringResource(R.string.about_kbps) }) }
            Tile(stringResource(R.string.player_about_size), corners(), Modifier.weight(1f).fillMaxHeight()) {
                val size = d.sizeBytes?.takeIf { it > 0.0 }?.let { formatBytes(it) }
                val (n, unit) = size?.split(' ', limit = 2)?.let { it[0] to it.getOrNull(1) } ?: ("–" to null)
                TileValue(n, unit?.let { " $it" })
            }
        }
        val rg = d.replayGain
        val last = listOfNotNull(d.path?.takeIf { it.isNotBlank() }, d.musicBrainzId?.takeIf { it.isNotBlank() }).isEmpty()
        if (rg?.trackGainDb != null || rg?.albumGainDb != null) Tile(stringResource(R.string.about_replaygain), if (last) corners(bs = Big, be = Big) else corners(), Modifier.fillMaxWidth(), minHeight = 64.dp) {
            Text(listOfNotNull(
                rg.trackGainDb?.let { g -> stringResource(R.string.about_rg_track, db(g)) + (rg.trackPeak?.let { p -> stringResource(R.string.about_rg_peak, String.format(Locale.getDefault(), "%.2f", p)) } ?: "") },
                rg.albumGainDb?.let { g -> stringResource(R.string.about_rg_album, db(g)) },
            ).joinToString(stringResource(R.string.dot_separator)), style = MaterialTheme.typography.bodyLarge)
        }
        d.path?.takeIf { it.isNotBlank() }?.let { path ->
            CopyTile(stringResource(R.string.about_path), path, if (d.musicBrainzId.isNullOrBlank()) corners(bs = Big, be = Big) else corners(), stringResource(R.string.about_copy_path)) { clipboard.setText(androidx.compose.ui.text.AnnotatedString(path)) }
        }
        d.musicBrainzId?.takeIf { it.isNotBlank() }?.let { id ->
            CopyTile(stringResource(R.string.about_mbid), id, corners(bs = Big, be = Big), stringResource(R.string.about_copy_mbid)) { clipboard.setText(androidx.compose.ui.text.AnnotatedString(id)) }
        }
    }
}

@Composable
private fun CopyTile(label: String, value: String, shape: Shape, copyLabel: String, onCopy: () -> Unit) {
    Surface(shape = shape, color = segmentColor()) {
        Row(Modifier.fillMaxWidth().heightIn(min = 72.dp).padding(start = 14.dp, end = 4.dp, top = 10.dp, bottom = 10.dp), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            Column(Modifier.weight(1f)) {
                Text(label, style = MaterialTheme.typography.labelLarge, color = MaterialTheme.colorScheme.onSurfaceVariant)
                Text(value, style = MaterialTheme.typography.bodyMedium, modifier = Modifier.padding(top = 2.dp))
            }
            FilledTonalIconButton(onClick = onCopy, shapes = IconButtonDefaults.shapes()) { Icon(HocketIcons.Filled.ContentCopy, copyLabel) }
        }
    }
}

/** "Thursday" within the week, "Today", or the date further back. */
@Composable
private fun relativeDay(epochMs: Long): String {
    val days = ((System.currentTimeMillis() - epochMs) / 86_400_000L)
    return when {
        days < 1 && android.text.format.DateUtils.isToday(epochMs) -> stringResource(R.string.about_today)
        days < 7 -> java.text.SimpleDateFormat("EEEE", Locale.getDefault()).format(Date(epochMs))
        else -> DateFormat.getDateInstance(DateFormat.MEDIUM).format(Date(epochMs))
    }
}

private fun db(g: Double): String = String.format(Locale.getDefault(), "%+.1f dB", g).replace('-', '−')

@Composable
private fun DetailRow(label: String, value: String, link: Pair<String, () -> Unit>? = null) {
    // One item for a screen reader: "Album, Trench" (and "Go to album" where it links).
    val base = Modifier.fillMaxWidth().heightIn(min = 48.dp)
    Row(
        (if (link != null) base.clickable(onClickLabel = link.first, onClick = link.second) else base)
            .semantics(mergeDescendants = true) { }
            .padding(horizontal = 20.dp, vertical = 10.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Text(label, style = MaterialTheme.typography.labelLarge, color = MaterialTheme.colorScheme.onSurfaceVariant, modifier = Modifier.weight(0.4f))
        Text(value, style = MaterialTheme.typography.bodyLarge, maxLines = 4, overflow = TextOverflow.Ellipsis, modifier = Modifier.weight(0.6f))
        if (link != null) Icon(HocketIcons.Filled.ChevronRight, null, tint = MaterialTheme.colorScheme.onSurfaceVariant, modifier = Modifier.padding(start = 4.dp))
    }
}

/** A titled tonal card of server-provided text (clamped), with room for more below it. */
@Composable
private fun InfoCard(title: String, text: String?, color: androidx.compose.ui.graphics.Color, more: @Composable () -> Unit = {}) {
    Surface(shape = RoundedCornerShape(24.dp), color = color, modifier = Modifier.fillMaxWidth()) {
        Column(Modifier.padding(horizontal = 20.dp, vertical = 16.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
            Text(title, style = MaterialTheme.typography.titleMedium, modifier = Modifier.semantics { heading() })
            if (!text.isNullOrBlank()) Text(text, style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant, maxLines = 8, overflow = TextOverflow.Ellipsis)
            more()
        }
    }
}

/**
 * An album's details ("Album info" in its menu) on the same tonal card as [PlayerAbout]: artist,
 * year, genre, songs and length, plays, added and last played. The artist row goes to its page.
 */
@Composable
internal fun AlbumAbout(album: Album, title: String, modifier: Modifier = Modifier, onOpenArtist: ((String) -> Unit)? = null) {
    val date = DateFormat.getDateInstance(DateFormat.MEDIUM)
    val artistLabel = stringResource(R.string.player_about_artist)
    val artistLink = album.artistId?.takeIf { onOpenArtist != null }?.let { id -> stringResource(R.string.action_go_to_artist) to { onOpenArtist!!(id) } }
    val rows = buildList {
        album.artist?.let { add(artistLabel to it) }
        album.year?.takeIf { it > 0u }?.let { add(stringResource(R.string.player_about_year) to it.toString()) }
        album.genre?.takeIf { it.isNotBlank() }?.let { add(stringResource(R.string.player_about_genre) to it) }
        add(stringResource(R.string.library_songs) to album.songCount.toString())
        add(stringResource(R.string.player_about_duration) to formatClock(album.durationMs.toLong()))
        add(stringResource(R.string.player_about_play_count) to album.playCount.toString())
        album.created?.let { add(stringResource(R.string.album_info_added) to date.format(Date(it.toLong()))) }
        album.lastPlayed?.let { add(stringResource(R.string.player_about_last_played) to date.format(Date(it.toLong()))) }
    }
    val card = MaterialTheme.colorScheme.surfaceContainerHigh.copy(alpha = 0.55f)
    LazyColumn(
        modifier.testTag("album.about"),
        contentPadding = PaddingValues(start = 24.dp, end = 24.dp, top = 8.dp, bottom = 16.dp),
        verticalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        item(key = "title") {
            Text(title, style = MaterialTheme.typography.titleLargeEmphasized, fontWeight = androidx.compose.ui.text.font.FontWeight.Bold, modifier = Modifier.semantics { heading() })
        }
        item(key = "details") {
            Surface(shape = RoundedCornerShape(24.dp), color = card, modifier = Modifier.fillMaxWidth()) {
                Column(Modifier.padding(vertical = 8.dp)) {
                    rows.forEachIndexed { i, (label, value) ->
                        if (i > 0) HorizontalDivider(Modifier.padding(horizontal = 20.dp), color = MaterialTheme.colorScheme.outlineVariant.copy(alpha = 0.4f))
                        DetailRow(label, value, artistLink.takeIf { label == artistLabel })
                    }
                }
            }
        }
    }
}

/** 44100 -> "44.1", 48000 -> "48". */
private fun kilo(hz: Int): String = if (hz % 1000 == 0) (hz / 1000).toString() else String.format(Locale.getDefault(), "%.1f", hz / 1000f)
