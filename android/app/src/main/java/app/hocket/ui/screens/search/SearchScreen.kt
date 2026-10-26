package app.hocket.ui.screens.search

import app.hocket.ui.icons.HocketIcons
import app.hocket.ui.nav.ScrollToTopOnReselect
import androidx.compose.foundation.clickable
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.width
import androidx.compose.ui.layout.onSizeChanged
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.LoadingIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.TextField
import androidx.compose.material3.TextFieldDefaults
import androidx.compose.material3.FilterChip
import androidx.compose.material3.FilterChipDefaults
import androidx.compose.material3.ListItemDefaults
import androidx.compose.material3.SegmentedListItem
import androidx.compose.material3.TextButton
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.wrapContentHeight
import androidx.compose.foundation.lazy.LazyRow
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import app.hocket.ui.components.Segment
import app.hocket.ui.components.segmentedListColors
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.runtime.snapshotFlow
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.navigation.NavHostController
import app.hocket.R
import app.hocket.ui.nav.AccountButton
import app.hocket.ui.nav.BottomContentInset
import app.hocket.core.Commands
import app.hocket.core.Queries
import app.hocket.core.api.QueryResult
import app.hocket.core.api.SearchResults
import app.hocket.core.client.LibraryPatches
import app.hocket.ui.LocalCoreClient
import app.hocket.ui.rememberCachedState
import app.hocket.ui.screens.home.AlbumStrip
import app.hocket.ui.components.ArtistRow
import app.hocket.ui.components.EmptyState
import app.hocket.ui.components.PlaylistRow
import app.hocket.ui.components.SectionHeader
import app.hocket.ui.components.TrackRow
import app.hocket.ui.nav.Route
import kotlinx.coroutines.FlowPreview
import kotlinx.coroutines.flow.debounce

/**
 * Local-first, instant as you type. Server results arrive in `Event.SearchResults` and are appended
 * below a divider; the results container keeps its previous minimum height between the two batches
 * so nothing shifts under the finger (design: search).
 */
@OptIn(FlowPreview::class)
@Composable
fun SearchScreen(nav: NavHostController) {
    val client = LocalCoreClient.current
    val server by client.server.collectAsStateWithLifecycle()
    val serverId = server?.id ?: return
    var query by rememberSaveable { mutableStateOf("") }
    // The last results stay with the (saved) query, so coming back to Search from a result shows
    // them on the first frame and the result's artwork can fly back into its row.
    var local by rememberCachedState<SearchResults?>(listOf("search.local", serverId)) { null }
    var remote by rememberCachedState<SearchResults?>(listOf("search.remote", serverId)) { null }
    var pendingRemote by remember { mutableStateOf(false) }
    var requestCounter by remember { mutableIntStateOf(0) }
    var minHeightPx by remember { mutableIntStateOf(0) }
    val focus = remember { FocusRequester() }
    val context = androidx.compose.ui.platform.LocalContext.current
    val recentPrefs = remember(context) { app.hocket.RecentSearchPrefs(context) }
    var recent by remember { mutableStateOf(recentPrefs.list()) }
    // A search counts as made once its query is acted on: a result opened or played, or the keyboard's search key.
    val rememberQuery = { recent = recentPrefs.remember(query) }
    val songMenu = app.hocket.ui.components.rememberSongMenu()
    val nowPlaying by client.nowPlaying.collectAsStateWithLifecycle()

    LaunchedEffect(Unit) {
        var restored = local != null
        snapshotFlow { query }.debounce(120).collect { q ->
            val text = q.trim()
            if (text.isEmpty()) { local = null; remote = null; pendingRemote = false; minHeightPx = 0; return@collect }
            // Back on the tab with the results still showing for this query: keep them, no re-search.
            if (restored) { restored = false; if (local?.query == text) return@collect }
            val id = "s${requestCounter++}"
            val res = (client.query(Queries.search(serverId, text, 25, includeServer = true, requestId = id)) as? QueryResult.Search)?.data
            local = res
            remote = null
            pendingRemote = true
        }
    }
    LaunchedEffect(Unit) {
        client.searchResults.collect { r -> if (r.fromServer && r.query == query.trim()) { remote = r; pendingRemote = false } }
    }
    // Results are a one-off answer, not a refetching list: a rating or love set anywhere (a menu here,
    // another signed-in device) is patched into them so the rows show it at once.
    LaunchedEffect(Unit) {
        client.libraryItemsChanged.collect { c ->
            local = local?.let { LibraryPatches.search(it, c.items) }
            remote = remote?.let { LibraryPatches.search(it, c.items) }
        }
    }
    // A search opened from outside the app (Assistant, a launcher shortcut) arrives with its query.
    LaunchedEffect(Unit) {
        app.hocket.AppLinks.search.collect { link ->
            if (link == null) return@collect
            link.query?.let { query = it }
            app.hocket.AppLinks.consume(link)
        }
    }
    // The keyboard comes up on the first visit only; coming back to the tab keeps the results in view.
    var focusedOnce by rememberSaveable { mutableStateOf(false) }
    LaunchedEffect(Unit) {
        if (!focusedOnce) { focusedOnce = true; runCatching { focus.requestFocus() } }
    }

    var kind by rememberSaveable { mutableStateOf(SearchKind.All) }
    // A new query starts back on All: the kind picked for the last one may have nothing here.
    var kindQuery by rememberSaveable { mutableStateOf(query) }
    LaunchedEffect(query) { if (query != kindQuery) { kindQuery = query; kind = SearchKind.All } }
    Column(Modifier.fillMaxSize().statusBarsPadding()) {
        Row(Modifier.padding(start = 16.dp, end = 4.dp, top = 10.dp, bottom = 8.dp), verticalAlignment = androidx.compose.ui.Alignment.CenterVertically) {
            // The M3 search bar: a full-round 56dp field on the highest container, no outline.
            TextField(
                value = query, onValueChange = { query = it }, singleLine = true, placeholder = { Text(stringResource(R.string.search_hint)) },
                leadingIcon = { Icon(HocketIcons.Filled.Search, null) },
                trailingIcon = { if (query.isNotEmpty()) IconButton(onClick = { query = "" }) { Icon(HocketIcons.Filled.Close, stringResource(R.string.search_clear)) } },
                keyboardOptions = androidx.compose.foundation.text.KeyboardOptions(imeAction = androidx.compose.ui.text.input.ImeAction.Search),
                keyboardActions = androidx.compose.foundation.text.KeyboardActions(onSearch = { rememberQuery() }),
                shape = androidx.compose.foundation.shape.CircleShape,
                colors = TextFieldDefaults.colors(
                    focusedContainerColor = MaterialTheme.colorScheme.surfaceContainerHigh, unfocusedContainerColor = MaterialTheme.colorScheme.surfaceContainerHigh,
                    focusedIndicatorColor = androidx.compose.ui.graphics.Color.Transparent, unfocusedIndicatorColor = androidx.compose.ui.graphics.Color.Transparent,
                ),
                modifier = Modifier.weight(1f).heightIn(min = 56.dp).focusRequester(focus).testTag("search.input"),
            )
            AccountButton(Modifier.padding(start = 4.dp))
        }
        val results = local
        if (results == null) {
            LazyColumn(Modifier.fillMaxSize().testTag("search.recent"), contentPadding = PaddingValues(start = 16.dp, end = 16.dp, bottom = BottomContentInset)) {
                if (query.isBlank() && recent.isNotEmpty()) recentSearches(recent, onPick = { query = it; recent = recentPrefs.remember(it) }, onForget = { recent = recentPrefs.forget(it) }, onClear = { recent = recentPrefs.clear() })
                item(key = "hint") { SearchHint() }
            }
            return
        }
        val server = remote
        val counts = mapOf(
            SearchKind.Songs to results.tracks.size, SearchKind.Albums to results.albums.size,
            SearchKind.Artists to results.artists.size, SearchKind.Playlists to results.playlists.size,
        )
        val empty = counts.values.all { it == 0 }
        val show = { k: SearchKind -> (kind == SearchKind.All || kind == k) && (counts[k] ?: 0) > 0 }
        // The kinds to narrow to, each with its count; one with nothing found can't be picked.
        LazyRow(contentPadding = PaddingValues(horizontal = 16.dp), horizontalArrangement = Arrangement.spacedBy(8.dp), modifier = Modifier.padding(bottom = 6.dp).testTag("search.kinds")) {
            items(SearchKind.entries.toList(), key = { it.name }) { k ->
                val n = counts[k]
                FilterChip(
                    selected = kind == k, onClick = { kind = k }, enabled = k == SearchKind.All || (n ?: 0) > 0,
                    label = { Text(if (n == null) stringResource(k.label) else stringResource(R.string.search_kind_count, stringResource(k.label), n)) },
                    leadingIcon = if (kind == k) ({ Icon(HocketIcons.Filled.Check, null, Modifier.size(FilterChipDefaults.IconSize)) }) else null,
                    modifier = Modifier.testTag("search.kind.${k.name.lowercase()}"),
                )
            }
        }
        val density = LocalDensity.current
        Box(Modifier.heightIn(min = with(density) { minHeightPx.toDp() }).onSizeChangedKeepMax { if (it > minHeightPx) minHeightPx = it }) {
            val listState = rememberLazyListState()
            ScrollToTopOnReselect(listState)
            LazyColumn(Modifier.fillMaxSize(), state = listState, contentPadding = PaddingValues(bottom = BottomContentInset)) {
                if (empty && server == null && !pendingRemote) item { EmptyState(stringResource(R.string.empty_search_none, query), stringResource(R.string.search_none_body)) }
                if (show(SearchKind.Songs)) {
                    val shown = if (kind == SearchKind.All) results.tracks.take(SONG_PREVIEW) else results.tracks
                    val more = kind == SearchKind.All && results.tracks.size > SONG_PREVIEW
                    val n = shown.size + if (more) 1 else 0
                    item(key = "h.songs") { SearchSectionLabel(stringResource(R.string.search_songs)) }
                    itemsIndexed(shown, key = { _, t -> "t" + t.id }) { i, t ->
                        TrackRow(t, onClick = { rememberQuery(); client.dispatch(Commands.playTracks(serverId, results.tracks.map { it.id }, results.tracks.indexOf(t), query)) }, onMore = { songMenu.open(t) },
                            nowPlaying = nowPlaying?.track?.id == t.id, segment = i to n, subtitle = listOfNotNull(t.artist, t.album).joinToString(stringResource(R.string.dot_separator)),
                            modifier = Modifier.segmentItem(i))
                    }
                    if (more) item(key = "songs.more") {
                        Segment(n - 1, n, Modifier.segmentItem(1), onClick = { kind = SearchKind.Songs }) {
                            Row(Modifier.heightIn(min = 52.dp).padding(horizontal = 16.dp).testTag("search.songs.all"), verticalAlignment = androidx.compose.ui.Alignment.CenterVertically) {
                                Text(stringResource(R.string.search_see_all_songs), style = MaterialTheme.typography.titleSmall, color = MaterialTheme.colorScheme.primary, modifier = Modifier.weight(1f))
                                Text(pluralStringResource(R.plurals.search_songs_count, results.tracks.size, results.tracks.size), style = MaterialTheme.typography.labelLarge, color = MaterialTheme.colorScheme.onSurfaceVariant)
                                Icon(HocketIcons.Filled.ChevronRight, null, tint = MaterialTheme.colorScheme.onSurfaceVariant)
                            }
                        }
                    }
                }
                if (show(SearchKind.Albums)) {
                    item(key = "h.albums") { SearchSectionLabel(stringResource(R.string.search_albums), top = 16.dp) }
                    item(key = "albums") { AlbumStrip(results.albums, Modifier.animateItem()) { a -> rememberQuery(); nav.navigate(Route.Album(a.id)) } }
                }
                if (show(SearchKind.Artists)) {
                    item(key = "h.artists") { SearchSectionLabel(stringResource(R.string.search_artists), top = 16.dp) }
                    itemsIndexed(results.artists, key = { _, ar -> "ar" + ar.id }) { i, ar -> ArtistRow(ar, onClick = { rememberQuery(); nav.navigate(Route.Artist(ar.id)) }, segment = i to results.artists.size, modifier = Modifier.segmentItem(i)) }
                }
                if (show(SearchKind.Playlists)) {
                    item(key = "h.playlists") { SearchSectionLabel(stringResource(R.string.search_playlists), top = 16.dp) }
                    itemsIndexed(results.playlists, key = { _, p -> "p" + p.id }) { i, p -> PlaylistRow(p, onClick = { rememberQuery(); nav.navigate(Route.Playlist(p.id)) }, segment = i to results.playlists.size, modifier = Modifier.segmentItem(i)) }
                }
                item(key = "server") {
                    HorizontalDivider(Modifier.padding(start = 16.dp, end = 16.dp, top = 20.dp, bottom = 12.dp), color = MaterialTheme.colorScheme.outlineVariant)
                    Row(Modifier.padding(horizontal = 16.dp).heightIn(min = 44.dp), verticalAlignment = androidx.compose.ui.Alignment.CenterVertically) {
                        Text(stringResource(R.string.search_from_server), style = MaterialTheme.typography.titleMedium, fontWeight = FontWeight.SemiBold, modifier = Modifier.weight(1f).semantics { heading() })
                        if (pendingRemote) LoadingIndicator(Modifier.size(28.dp))
                    }
                    if (pendingRemote) Text(stringResource(R.string.search_server_pending), style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant, modifier = Modifier.padding(horizontal = 16.dp))
                    else if (server != null && server.tracks.isEmpty() && server.albums.isEmpty() && server.artists.isEmpty()) Text(stringResource(R.string.search_server_none), style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant, modifier = Modifier.padding(horizontal = 16.dp))
                }
                server?.let { r ->
                    val tracks = if (kind == SearchKind.All || kind == SearchKind.Songs) r.tracks else emptyList()
                    val artists = if (kind == SearchKind.All || kind == SearchKind.Artists) r.artists else emptyList()
                    val n = tracks.size + artists.size
                    itemsIndexed(tracks, key = { _, t -> "rt" + t.id }) { i, t -> TrackRow(t, onClick = { client.dispatch(Commands.playTracks(serverId, r.tracks.map { it.id }, r.tracks.indexOf(t), query)) }, onMore = { songMenu.open(t) }, segment = i to n, modifier = Modifier.segmentItem(i)) }
                    if (r.albums.isNotEmpty() && (kind == SearchKind.All || kind == SearchKind.Albums)) item(key = "ralbums") { AlbumStrip(r.albums, Modifier.animateItem().padding(vertical = 8.dp)) { a -> nav.navigate(Route.Album(a.id)) } }
                    itemsIndexed(artists, key = { _, ar -> "rar" + ar.id }) { i, ar -> ArtistRow(ar, onClick = { nav.navigate(Route.Artist(ar.id)) }, segment = (tracks.size + i) to n, modifier = Modifier.segmentItem(tracks.size + i)) }
                }
            }
        }
    }
}

/** What the results can narrow to. */
private enum class SearchKind(val label: Int) {
    All(R.string.search_kind_all), Songs(R.string.search_songs), Albums(R.string.search_albums), Artists(R.string.search_artists), Playlists(R.string.search_playlists)
}

/** Songs shown under All before "See all songs". */
private const val SONG_PREVIEW = 4

/** A row of a segmented group in a list: inset from the screen's edges, 2dp below the one before. */
internal fun Modifier.segmentItem(index: Int): Modifier = padding(start = 16.dp, end = 16.dp, top = if (index > 0) ListItemDefaults.SegmentedGap else 0.dp)

@Composable
private fun SearchSectionLabel(text: String, top: androidx.compose.ui.unit.Dp = 8.dp) {
    Text(text, style = MaterialTheme.typography.titleMedium, fontWeight = FontWeight.SemiBold, modifier = Modifier.padding(start = 16.dp, end = 16.dp, top = top).heightIn(min = 44.dp).wrapContentHeight().semantics { heading() })
}

/** The empty tab's hint, under any recent searches: what Search finds. */
@Composable
private fun SearchHint() {
    Column(Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 40.dp), horizontalAlignment = androidx.compose.ui.Alignment.CenterHorizontally, verticalArrangement = Arrangement.spacedBy(8.dp)) {
        Box(Modifier.size(72.dp).background(MaterialTheme.colorScheme.secondaryContainer, RoundedCornerShape(24.dp)), contentAlignment = androidx.compose.ui.Alignment.Center) {
            Icon(HocketIcons.Filled.Search, null, Modifier.size(32.dp), tint = MaterialTheme.colorScheme.onSecondaryContainer)
        }
        Spacer(Modifier.height(4.dp))
        Text(stringResource(R.string.empty_search_title), style = MaterialTheme.typography.titleLarge, fontWeight = FontWeight.SemiBold, textAlign = TextAlign.Center)
        Text(stringResource(R.string.empty_search_body), style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant, textAlign = TextAlign.Center)
    }
}

/** The empty search tab: this phone's recent searches as a segmented group, each to search again or forget, and Clear. */
private fun androidx.compose.foundation.lazy.LazyListScope.recentSearches(queries: List<String>, onPick: (String) -> Unit, onForget: (String) -> Unit, onClear: () -> Unit) {
    item(key = "recent.head") {
        Row(Modifier.fillMaxWidth().padding(top = 8.dp).heightIn(min = 48.dp), verticalAlignment = androidx.compose.ui.Alignment.CenterVertically) {
            Text(stringResource(R.string.search_recent), style = MaterialTheme.typography.titleMedium, fontWeight = FontWeight.SemiBold, modifier = Modifier.weight(1f).semantics { heading() })
            TextButton(onClick = onClear, modifier = Modifier.testTag("search.recent.clear")) { Text(stringResource(R.string.search_recent_clear)) }
        }
    }
    itemsIndexed(queries, key = { _, q -> "recent.$q" }) { i, q ->
        Box(Modifier.padding(top = if (i > 0) ListItemDefaults.SegmentedGap else 0.dp)) {
            SegmentedListItem(
                onClick = { onPick(q) }, shapes = ListItemDefaults.segmentedShapes(i, queries.size), colors = segmentedListColors(),
                leadingContent = { Icon(HocketIcons.Filled.History, null, tint = MaterialTheme.colorScheme.onSurfaceVariant) },
                trailingContent = { IconButton(onClick = { onForget(q) }) { Icon(HocketIcons.Filled.Close, stringResource(R.string.search_recent_forget, q)) } },
                modifier = Modifier.testTag("search.recent.item"),
            ) { Text(q, maxLines = 1, overflow = androidx.compose.ui.text.style.TextOverflow.Ellipsis) }
        }
    }
}

private fun Modifier.onSizeChangedKeepMax(onHeight: (Int) -> Unit): Modifier = this.onSizeChanged { onHeight(it.height) }
