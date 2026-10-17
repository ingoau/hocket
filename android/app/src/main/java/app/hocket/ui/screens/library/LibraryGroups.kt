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
import androidx.compose.runtime.getValue
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
import app.hocket.core.Queries
import app.hocket.core.SettingKeys
import app.hocket.core.api.PlayHistoryEntry
import app.hocket.core.api.QueryResult
import app.hocket.ui.LocalCoreClient

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

/** This account's latest plays, newest first (as Home's Recently played reads them); null while loading. */
@Composable
internal fun rememberRecentHistory(): List<PlayHistoryEntry>? {
    val client = LocalCoreClient.current
    val libraryGen by client.libraryGeneration.collectAsStateWithLifecycle()
    val history by produceState<List<PlayHistoryEntry>?>(null, libraryGen) {
        value = (client.query(Queries.recentlyPlayed(RECENT_HISTORY)) as? QueryResult.History)?.data.orEmpty()
    }
    return history
}

/**
 * A paged list's groups: its pins of [kind] (at most [PINNED_LIMIT]) and the ids [recentIds] picks
 * from the play history (pinned ones excluded), each resolved by [resolve]; gone items are left out.
 */
@Composable
internal fun <T : Any> rememberGroups(
    kind: PinKind,
    recentIds: (List<PlayHistoryEntry>, Collection<String>) -> List<String>,
    resolve: suspend (String) -> T?,
): ListGroups<T> {
    val client = LocalCoreClient.current
    val libraryGen by client.libraryGeneration.collectAsStateWithLifecycle()
    val pinned = rememberPinnedIds(kind)
    val history = rememberRecentHistory()
    val groups by produceState(ListGroups<T>(emptyList(), emptyList()), pinned, history, libraryGen) {
        value = ListGroups(pinned.take(PINNED_LIMIT).mapNotNull { resolve(it) }, recentIds(history.orEmpty(), pinned).mapNotNull { resolve(it) })
    }
    return groups
}

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
