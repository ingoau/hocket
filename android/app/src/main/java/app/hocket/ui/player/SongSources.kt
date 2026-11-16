package app.hocket.ui.player

import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.produceState
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
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
import app.hocket.core.api.TrackRef
import app.hocket.core.api.TrackSource
import app.hocket.core.ref
import app.hocket.ui.LocalCoreClient
import app.hocket.ui.screens.library.rememberSettledGeneration

/**
 * A song's sources (`docs/providers.md`, Phase 3): every copy of it across accounts, which one
 * plays here, and the two hand corrections: "Not the same song" takes a copy out of the group,
 * "Link to…" joins a song the matcher kept apart. Both sync through the identity. Shown with more
 * than one account, or when the song has several copies.
 */
@Composable
internal fun SongSources(track: TrackRef, modifier: Modifier = Modifier) {
    val client = LocalCoreClient.current
    val accounts by client.accounts.collectAsStateWithLifecycle()
    val generation = rememberSettledGeneration()
    val sources by produceState(emptyList<TrackSource>(), track, generation) {
        value = (client.query(Queries.trackSources(track)) as? QueryResult.TrackSources)?.data.orEmpty()
    }
    var linking by remember(track) { mutableStateOf(false) }
    if (accounts.size < 2 && sources.size < 2) return
    Column(modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 8.dp).testTag("about.sources")) {
        Text(stringResource(R.string.sources_title), style = MaterialTheme.typography.titleSmall, modifier = Modifier.semantics { heading() })
        sources.forEach { s ->
            Row(verticalAlignment = Alignment.CenterVertically, modifier = Modifier.fillMaxWidth().testTag("about.source")) {
                Column(Modifier.weight(1f).padding(vertical = 6.dp)) {
                    Text(s.accountName.ifBlank { s.accountId }, style = MaterialTheme.typography.bodyLarge, maxLines = 1, overflow = TextOverflow.Ellipsis)
                    val notes = listOfNotNull(
                        stringResource(R.string.sources_preferred).takeIf { s.preferred },
                        stringResource(R.string.sources_linked_by_hand).takeIf { s.manual },
                        stringResource(R.string.sources_not_here).takeIf { !s.available },
                    )
                    if (notes.isNotEmpty()) Text(notes.joinToString(stringResource(R.string.dot_separator)), style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
                }
                if (sources.size > 1) TextButton(onClick = { client.dispatch(Commands.unlinkTrack(s.track.ref)) }, modifier = Modifier.testTag("about.source.unlink")) {
                    Text(stringResource(R.string.sources_not_same))
                }
            }
        }
        if (accounts.size > 1) {
            if (linking) LinkPicker(track, onDone = { linking = false })
            else TextButton(onClick = { linking = true }, modifier = Modifier.testTag("about.source.link")) { Text(stringResource(R.string.sources_link_to)) }
        }
    }
}

@Composable
private fun LinkPicker(track: TrackRef, onDone: () -> Unit) {
    val client = LocalCoreClient.current
    var query by remember { mutableStateOf("") }
    val candidates by produceState(emptyList<Track>(), track, query) {
        value = (client.query(Queries.linkCandidates(track, query)) as? QueryResult.TrackList)?.data.orEmpty()
    }
    Column(Modifier.fillMaxWidth().testTag("about.link")) {
        OutlinedTextField(value = query, onValueChange = { query = it }, singleLine = true, label = { Text(stringResource(R.string.sources_link_search)) }, modifier = Modifier.fillMaxWidth())
        if (candidates.isEmpty()) Text(stringResource(R.string.sources_no_candidates), style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant, modifier = Modifier.padding(vertical = 8.dp))
        candidates.forEach { c ->
            TextButton(onClick = { client.dispatch(Commands.linkTracks(track, c.ref)); onDone() }, modifier = Modifier.fillMaxWidth().testTag("about.link.candidate")) {
                Text(listOfNotNull(c.title, c.artist, c.album).joinToString(stringResource(R.string.dot_separator)), maxLines = 1, overflow = TextOverflow.Ellipsis, modifier = Modifier.fillMaxWidth())
            }
        }
    }
}
