package app.hocket.ui.screens.library

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyRow
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.setValue
import androidx.compose.runtime.produceState
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.layout.layout
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.offset
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import kotlinx.coroutines.FlowPreview
import kotlinx.coroutines.async
import kotlinx.coroutines.awaitAll
import kotlinx.coroutines.coroutineScope
import kotlinx.coroutines.flow.debounce
import kotlinx.coroutines.flow.drop
import app.hocket.core.Queries
import app.hocket.core.SettingKeys
import app.hocket.core.api.Album
import app.hocket.core.api.PlayHistoryEntry
import app.hocket.core.api.QueryResult
import app.hocket.ui.LocalCoreClient
import app.hocket.ui.LocalScreenCache
import app.hocket.ui.rememberCachedState

/** What a library list shows above its full list: its pins of that kind (pin order), then its recent items. */
data class ListGroups<T>(val pinned: List<T>, val recent: List<T>)

/** A list's sort and the orders it offers, for its "All …" header. */
data class SortControl(val sort: ListSort, val options: List<SortKey>, val onSort: (ListSort) -> Unit)

/** The ids `library.pins` holds for [kind], in pin order. */
@Composable
internal fun rememberPinnedIds(kind: PinKind): List<String> {
    val client = LocalCoreClient.current
    val settings by client.settings.collectAsStateWithLifecycle()
    val raw = settings[SettingKeys.LIBRARY_PINS]?.value
    return remember(raw, kind) { pinnedIds(LibraryPins.parse(raw), kind) }
}

/** How long the library generation must stay put before the groups reload (a sync bumps it in bursts). */
private const val GENERATION_SETTLE_MS = 500L

/**
 * The library generation, settled: it bumps on every play and sync batch, and the groups re-query on
 * it, so a burst of bumps reloads them once, [GENERATION_SETTLE_MS] after the last. Starts from the
 * current generation, so a page's first query goes out with its first frame (callers still treat
 * -1 as not read yet).
 */
@OptIn(FlowPreview::class)
@Composable
internal fun rememberSettledGeneration(): Int {
    val client = LocalCoreClient.current
    val generation by produceState(client.libraryGeneration.value, client) {
        value = client.libraryGeneration.value
        client.libraryGeneration.drop(1).debounce(GENERATION_SETTLE_MS).collect { value = it }
    }
    return generation
}

/** The signed-in server's id, for keying [rememberCachedState] (null before sign-in). */
@Composable
internal fun rememberServerId(): String? = LocalCoreClient.current.server.collectAsStateWithLifecycle().value?.id

/**
 * This account's latest plays, newest first (as Home's Recently played reads them); null while
 * loading. Starts from the last answer (see [rememberCachedState]) when coming back to a page.
 */
@Composable
internal fun rememberRecentHistory(): List<PlayHistoryEntry>? {
    val client = LocalCoreClient.current
    val generation = rememberSettledGeneration()
    var history by rememberCachedState<List<PlayHistoryEntry>?>(listOf("library.history", rememberServerId())) { null }
    LaunchedEffect(generation) {
        if (generation < 0) return@LaunchedEffect
        history = (client.query(Queries.recentlyPlayed(RECENT_HISTORY)) as? QueryResult.History)?.data.orEmpty()
    }
    return history
}

/**
 * A paged list's groups: its pins of [kind] (at most [PINNED_LIMIT]) and the ids [recentIds] picks
 * from the play history (pinned ones excluded), each resolved by [resolve] (all at once; there is
 * no batch query for albums or artists); gone items are left out. Loaded once the history is, and
 * again only when it or the pins change.
 *
 * Starts from the last answer for this list; on a first visit with no pins, from [recentStart] (the
 * leading Recent items already known elsewhere, e.g. the Albums tile's fan), so those cards are in
 * place on the first frame and the rest join after them.
 */
@Composable
internal fun <T : Any> rememberGroups(
    kind: PinKind,
    recentIds: (List<PlayHistoryEntry>, Collection<String>) -> List<String>,
    recentStart: List<T>? = null,
    resolve: suspend (String) -> T?,
): ListGroups<T> {
    val pinned = rememberPinnedIds(kind)
    val history = rememberRecentHistory()
    var groups by rememberCachedState(listOf("library.groups", rememberServerId(), kind)) {
        ListGroups(emptyList(), if (pinned.isEmpty()) recentStart.orEmpty() else emptyList())
    }
    LaunchedEffect(pinned, history) {
        if (history == null) return@LaunchedEffect
        groups = coroutineScope {
            val pins = pinned.take(PINNED_LIMIT).map { async { resolve(it) } }
            val recent = recentIds(history, pinned).map { async { resolve(it) } }
            ListGroups(pins.awaitAll().filterNotNull(), recent.awaitAll().filterNotNull())
        }
    }
    return groups
}

/**
 * The albums the Library's Albums tile fans out, matching the first [limit] of the albums page's
 * Recent group (latest plays, pinned albums left out).
 */
@Composable
internal fun rememberRecentAlbums(limit: Int): List<Album> {
    val client = LocalCoreClient.current
    val pinned = rememberPinnedIds(PinKind.Album)
    val history = rememberRecentHistory()
    var albums by rememberCachedState(recentAlbumsKey(rememberServerId(), limit)) { emptyList<Album>() }
    LaunchedEffect(pinned, history, limit) {
        if (history == null) return@LaunchedEffect
        albums = coroutineScope {
            recentAlbumIds(history, limit, pinned).map { id ->
                async { (client.query(Queries.album(id)) as? QueryResult.AlbumDetail)?.data }
            }.awaitAll().filterNotNull()
        }
    }
    return albums
}

private fun recentAlbumsKey(serverId: String?, limit: Int) = listOf("library.recentAlbums", serverId, limit)

/** What [rememberRecentAlbums] last showed for [limit], without loading anything (null if never loaded). */
@Composable
internal fun cachedRecentAlbums(limit: Int): List<Album>? =
    LocalScreenCache.current?.get<List<Album>>(recentAlbumsKey(rememberServerId(), limit))

/**
 * A group's header in a library list ("Pinned", "Recent", "All albums"), a heading, with an
 * optional [action] at its end (the full list's sort chip). [start] lines the title up with the
 * rows under it (grids have their own edge padding).
 */
@Composable
internal fun ListHeader(title: String, modifier: Modifier = Modifier, start: Dp = 16.dp, action: (@Composable () -> Unit)? = null) {
    Row(
        modifier.fillMaxWidth().heightIn(min = 48.dp).padding(start = start, end = if (action != null) 12.dp else 16.dp, top = 8.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        Text(title, style = MaterialTheme.typography.titleMedium, fontWeight = FontWeight.SemiBold, maxLines = 1, overflow = TextOverflow.Ellipsis,
            modifier = Modifier.weight(1f).semantics { heading() })
        action?.invoke()
    }
}

/**
 * A grid list's group as one horizontal row of cards ([cellWidth] each), reaching past the grid's
 * [edge] padding to the screen's edges so it scrolls edge to edge while its first card lines up
 * with the grid's.
 */
@Composable
internal fun <T> CardRow(items: List<T>, key: (T) -> String, cellWidth: Dp, edge: Dp, modifier: Modifier = Modifier, card: @Composable (T, Modifier) -> Unit) {
    LazyRow(
        modifier.bleed(edge).testTag("library.group.row"),
        contentPadding = PaddingValues(horizontal = edge),
    ) {
        items(items, key = key) { card(it, Modifier.width(cellWidth)) }
    }
}

/** Lays the content out [by] wider on each side than it was given, so it reaches past its parent's padding. */
private fun Modifier.bleed(by: Dp): Modifier = layout { measurable, constraints ->
    val extra = (by * 2).roundToPx()
    val placeable = measurable.measure(constraints.offset(horizontal = extra))
    layout((placeable.width - extra).coerceAtLeast(0), placeable.height) { placeable.place(-extra / 2, 0) }
}

/**
 * The groups arrive after the full list has started showing: while the list is still at its top,
 * it stays there, so the groups appear in view (a lazy list would otherwise keep its first visible
 * item, the full list's header, in place and push the new groups off the top). Call in composition,
 * before the groups' items are emitted; [atTop] says whether the list is at its very top.
 */
@Composable
internal fun KeepTopForGroups(groups: ListGroups<*>, atTop: () -> Boolean, requestTop: () -> Unit) {
    val shown = groups.pinned.size + groups.recent.size
    val last = remember { intArrayOf(0) }
    if (shown != last[0]) {
        // Read only when the groups change, so scrolling never recomposes the list for it.
        if (atTop()) requestTop()
        last[0] = shown
    }
}
