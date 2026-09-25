package app.hocket.ui.player

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.HorizontalDivider
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
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import app.hocket.R
import app.hocket.core.Commands
import app.hocket.core.Queries
import app.hocket.core.api.QueryResult
import app.hocket.core.api.Track
import app.hocket.ui.LocalCoreClient
import app.hocket.ui.components.RatingStars
import app.hocket.ui.components.formatBytes
import app.hocket.ui.components.formatClock
import java.text.DateFormat
import java.util.Date
import java.util.Locale

/**
 * The player's About mode: the song's details on tonal cards over the player's gradient. The rating
 * (one adjustable control, tag `rating`) comes first; then what the library knows of the track
 * (album, artists, year, genre, track and disc, duration, format, bitrate, sample rate, bit depth,
 * channels, plays, last played, file size and path, comment), fetched once per track. While that
 * loads, the summary's fields (album, artist, duration) show on their own.
 */
@Composable
internal fun PlayerAbout(modifier: Modifier = Modifier, contentPadding: PaddingValues = PaddingValues()) {
    val client = LocalCoreClient.current
    val entry by client.nowPlaying.collectAsStateWithLifecycle()
    val summary = entry?.track
    val id = summary?.id
    val detail by produceState<Track?>(initialValue = null, id) {
        value = null
        value = id?.let { (client.query(Queries.track(it)) as? QueryResult.TrackDetail)?.data }
    }
    val d = detail?.takeIf { it.id == id }
    val rows = buildList {
        (d?.album ?: summary?.album)?.let { add(stringResource(R.string.player_about_album) to it) }
        (d?.artist ?: summary?.artist)?.let { add(stringResource(R.string.player_about_artist) to it) }
        d?.albumArtist?.takeIf { it != d.artist }?.let { add(stringResource(R.string.player_about_album_artist) to it) }
        d?.year?.takeIf { it > 0u }?.let { add(stringResource(R.string.player_about_year) to it.toString()) }
        d?.genre?.takeIf { it.isNotBlank() }?.let { add(stringResource(R.string.player_about_genre) to it) }
        d?.trackNumber?.takeIf { it > 0u }?.let { add(stringResource(R.string.player_about_track) to it.toString()) }
        d?.discNumber?.takeIf { it > 0u }?.let { add(stringResource(R.string.player_about_disc) to it.toString()) }
        summary?.let { add(stringResource(R.string.player_about_duration) to formatClock(it.durationMs)) }
        d?.let { t -> listOfNotNull(t.suffix?.uppercase(Locale.ROOT), t.contentType).distinct().takeIf { it.isNotEmpty() }?.let { add(stringResource(R.string.player_about_format) to it.joinToString(" · ")) } }
        d?.bitRate?.takeIf { it > 0u }?.let { add(stringResource(R.string.player_about_bitrate) to stringResource(R.string.player_about_bitrate_value, it.toInt())) }
        d?.sampleRate?.takeIf { it > 0u }?.let { add(stringResource(R.string.player_about_sample_rate) to stringResource(R.string.player_about_sample_rate_value, kilo(it.toInt()))) }
        d?.bitDepth?.takeIf { it > 0u }?.let { add(stringResource(R.string.player_about_bit_depth) to stringResource(R.string.player_about_bit_depth_value, it.toInt())) }
        d?.channels?.takeIf { it > 0u }?.let { add(stringResource(R.string.player_about_channels) to it.toString()) }
        d?.let { add(stringResource(R.string.player_about_play_count) to it.playCount.toString()) }
        d?.lastPlayed?.let { add(stringResource(R.string.player_about_last_played) to DateFormat.getDateTimeInstance(DateFormat.MEDIUM, DateFormat.SHORT).format(Date(it.toLong()))) }
        d?.sizeBytes?.takeIf { it > 0.0 }?.let { add(stringResource(R.string.player_about_size) to formatBytes(it)) }
        d?.path?.takeIf { it.isNotBlank() }?.let { add(stringResource(R.string.player_about_path) to it) }
        d?.comment?.takeIf { it.isNotBlank() }?.let { add(stringResource(R.string.player_about_comment) to it) }
    }
    val card = MaterialTheme.colorScheme.surfaceContainerHigh.copy(alpha = 0.55f)
    LazyColumn(
        modifier.testTag("player.about"),
        contentPadding = PaddingValues(start = 24.dp, end = 24.dp, top = contentPadding.calculateTopPadding() + 8.dp, bottom = contentPadding.calculateBottomPadding() + 16.dp),
        verticalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        item(key = "title") {
            Text(stringResource(R.string.player_about_title), style = MaterialTheme.typography.titleLargeEmphasized, fontWeight = androidx.compose.ui.text.font.FontWeight.Bold, modifier = Modifier.semantics { heading() })
        }
        if (summary != null) item(key = "rating") {
            Surface(shape = RoundedCornerShape(24.dp), color = card, modifier = Modifier.fillMaxWidth()) {
                Row(Modifier.padding(start = 20.dp, end = 12.dp, top = 4.dp, bottom = 4.dp), verticalAlignment = Alignment.CenterVertically) {
                    Text(stringResource(R.string.player_about_rating), style = MaterialTheme.typography.labelLarge, color = MaterialTheme.colorScheme.onSurfaceVariant, modifier = Modifier.weight(1f))
                    RatingStars(summary.rating.toInt(), onRate = { client.dispatch(Commands.rateTrack(summary.id, it)) }, starSize = 26.dp, tint = MaterialTheme.colorScheme.primary)
                }
            }
        }
        item(key = "details") {
            Surface(shape = RoundedCornerShape(24.dp), color = card, modifier = Modifier.fillMaxWidth()) {
                Column(Modifier.padding(vertical = 8.dp)) {
                    rows.forEachIndexed { i, (label, value) ->
                        if (i > 0) HorizontalDivider(Modifier.padding(horizontal = 20.dp), color = MaterialTheme.colorScheme.outlineVariant.copy(alpha = 0.4f))
                        DetailRow(label, value)
                    }
                    if (d == null && summary != null) {
                        Text(stringResource(R.string.player_about_loading), style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant, modifier = Modifier.padding(horizontal = 20.dp, vertical = 8.dp))
                    }
                }
            }
        }
    }
}

@Composable
private fun DetailRow(label: String, value: String) {
    // One item for a screen reader: "Album, Trench".
    Row(Modifier.fillMaxWidth().heightIn(min = 48.dp).semantics(mergeDescendants = true) { }.padding(horizontal = 20.dp, vertical = 10.dp), verticalAlignment = Alignment.CenterVertically) {
        Text(label, style = MaterialTheme.typography.labelLarge, color = MaterialTheme.colorScheme.onSurfaceVariant, modifier = Modifier.weight(0.4f))
        Text(value, style = MaterialTheme.typography.bodyLarge, maxLines = 4, overflow = TextOverflow.Ellipsis, modifier = Modifier.weight(0.6f))
    }
}

/** 44100 -> "44.1", 48000 -> "48". */
private fun kilo(hz: Int): String = if (hz % 1000 == 0) (hz / 1000).toString() else String.format(Locale.getDefault(), "%.1f", hz / 1000f)
