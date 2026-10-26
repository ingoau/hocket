package app.hocket.ui.screens.stats

import androidx.compose.foundation.Canvas
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.IntrinsicSize
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.selection.selectableGroup
import androidx.compose.material3.ButtonGroupDefaults
import androidx.compose.material3.ListItemDefaults
import androidx.compose.material3.Surface
import androidx.compose.material3.ToggleButton
import androidx.compose.material3.ToggleButtonDefaults
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.role
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import app.hocket.ui.components.segmentColor
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.MediumFlexibleTopAppBar
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TopAppBarDefaults
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.input.nestedscroll.nestedScroll
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import androidx.navigation.NavHostController
import app.hocket.R
import app.hocket.ui.icons.HocketIcons
import app.hocket.ui.nav.AccountButton
import app.hocket.ui.nav.BottomContentInset
import app.hocket.core.Queries
import app.hocket.core.api.ListeningStats
import app.hocket.core.api.QueryResult
import app.hocket.ui.LocalCoreClient
import app.hocket.ui.rememberCachedState
import app.hocket.ui.screens.library.rememberServerId
import app.hocket.ui.components.EmptyState
import app.hocket.ui.components.SectionHeaderSkeleton
import app.hocket.ui.components.ShimmerHost
import app.hocket.ui.components.TrackRowSkeleton
import app.hocket.ui.components.skeleton
import androidx.compose.foundation.shape.RoundedCornerShape
import app.hocket.ui.components.SectionHeader
import app.hocket.ui.components.TrackRow
import app.hocket.ui.components.formatDurationWords
import app.hocket.ui.nav.Route
import app.hocket.ui.screens.home.AlbumStrip

/** Listening stats from local play history with bar charts drawn in Canvas. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun StatsScreen(nav: NavHostController) {
    val client = LocalCoreClient.current
    var period by rememberSaveable { mutableIntStateOf(30) }
    // Starts from the last answer for this period, so a pop back from a top album or artist has its
    // row on the first frame to fly back into.
    val serverId = rememberServerId()
    var stats by rememberCachedState<ListeningStats?>(listOf("stats", serverId, period)) { null }
    // Keyed like the cached state: the server arriving after the first frame resets it, so ask again.
    LaunchedEffect(period, serverId) { stats = (client.query(Queries.stats(period)) as? QueryResult.Stats)?.data }
    val scroll = TopAppBarDefaults.exitUntilCollapsedScrollBehavior()
    Scaffold(
        modifier = Modifier.nestedScroll(scroll.nestedScrollConnection),
        topBar = { MediumFlexibleTopAppBar(title = { Text(stringResource(R.string.stats_title), style = app.hocket.ui.brand.brandTitleStyle(), maxLines = 1, modifier = Modifier.semantics { heading() }) }, actions = { AccountButton() }, navigationIcon = { IconButton(onClick = { nav.popBackStack() }) { Icon(HocketIcons.AutoMirrored.Filled.ArrowBack, stringResource(R.string.action_back)) } }, scrollBehavior = scroll) },
    ) { padding ->
        val s = stats
        LazyColumn(Modifier.fillMaxSize(), contentPadding = PaddingValues(top = padding.calculateTopPadding(), bottom = BottomContentInset)) {
            item(key = "periods") {
                val periods = listOf(7 to stringResource(R.string.stats_period_7), 30 to stringResource(R.string.stats_period_30), 365 to stringResource(R.string.stats_period_365))
                // M3 Expressive's connected button group: the three periods share the row, the chosen one filled with a check.
                Row(Modifier.fillMaxWidth().padding(start = 16.dp, end = 16.dp, top = 8.dp, bottom = 16.dp).selectableGroup(), horizontalArrangement = Arrangement.spacedBy(ButtonGroupDefaults.ConnectedSpaceBetween)) {
                    periods.forEachIndexed { i, (days, label) ->
                        ToggleButton(
                            checked = period == days, onCheckedChange = { period = days },
                            shapes = when (i) { 0 -> ButtonGroupDefaults.connectedLeadingButtonShapes(); periods.lastIndex -> ButtonGroupDefaults.connectedTrailingButtonShapes(); else -> ButtonGroupDefaults.connectedMiddleButtonShapes() },
                            modifier = Modifier.weight(1f).heightIn(min = 48.dp).semantics { role = Role.RadioButton }.testTag("stats.period.$days"),
                        ) {
                            if (period == days) { Icon(HocketIcons.Filled.Check, null, Modifier.size(ToggleButtonDefaults.IconSize)); Spacer(Modifier.width(ToggleButtonDefaults.IconSpacing)) }
                            Text(label)
                        }
                    }
                }
            }
            if (s == null) {
                item(key = "skeleton") {
                    ShimmerHost {
                        Column {
                            SectionHeaderSkeleton()
                            Box(Modifier.fillMaxWidth().padding(horizontal = 16.dp).height(120.dp).skeleton(RoundedCornerShape(12.dp)))
                            SectionHeaderSkeleton()
                            repeat(5) { TrackRowSkeleton() }
                        }
                    }
                }
                return@LazyColumn
            }
            if (s.totalPlays == 0u) { item { EmptyState(stringResource(R.string.empty_stats_title), stringResource(R.string.empty_stats_body)) }; return@LazyColumn }
            item(key = "summary") {
                val minutes = s.totalMs.toLong() / 60_000
                val (big, small) = if (minutes >= 60) stringResource(R.string.stats_hours, (minutes / 60).toInt()) to stringResource(R.string.stats_minutes_listened, (minutes % 60).toInt())
                    else stringResource(R.string.stats_minutes, minutes.toInt()) to stringResource(R.string.stats_listened)
                Row(Modifier.fillMaxWidth().height(IntrinsicSize.Min).padding(horizontal = 16.dp), horizontalArrangement = Arrangement.spacedBy(ListItemDefaults.SegmentedGap)) {
                    SummaryTile(s.totalPlays.toString(), pluralStringResource(R.plurals.stats_plays_unit, s.totalPlays.toInt()), RoundedCornerShape(topStart = 24.dp, bottomStart = 24.dp, topEnd = 8.dp, bottomEnd = 8.dp),
                        MaterialTheme.colorScheme.primaryContainer, MaterialTheme.colorScheme.onPrimaryContainer, Modifier.weight(1f).fillMaxHeight())
                    SummaryTile(big, small, RoundedCornerShape(topEnd = 24.dp, bottomEnd = 24.dp, topStart = 8.dp, bottomStart = 8.dp),
                        MaterialTheme.colorScheme.surfaceContainer, MaterialTheme.colorScheme.onSurface, Modifier.weight(1f).fillMaxHeight())
                }
            }
            item(key = "hours") {
                StatsHeading(stringResource(R.string.stats_by_hour))
                ChartCard {
                    BarChart(s.playsByHour.map { it.toInt() }, labels = listOf("0", "6", "12", "18", "23"), spreadLabels = true, height = 120.dp, gap = 3.dp, corner = 5.dp,
                        readout = { peak -> stringResource(R.string.stats_peak_hour, "%02d:00–%02d:00".format(peak, (peak + 1) % 24)) })
                }
            }
            item(key = "days") {
                val days = listOf(R.string.weekday_mon, R.string.weekday_tue, R.string.weekday_wed, R.string.weekday_thu, R.string.weekday_fri, R.string.weekday_sat, R.string.weekday_sun).map { stringResource(it) }
                StatsHeading(stringResource(R.string.stats_by_weekday))
                ChartCard { BarChart(s.playsByWeekday.map { it.toInt() }, labels = days, height = 96.dp, gap = 8.dp, corner = 10.dp, centredLabels = true) }
            }
            if (s.topTracks.isNotEmpty()) {
                item(key = "h.tracks") { StatsHeading(stringResource(R.string.stats_top_tracks)) }
                itemsIndexed(s.topTracks, key = { _, t -> t.id }) { i, t ->
                    TrackRow(t, onClick = { t.albumId?.let { nav.navigate(Route.Album(it)) } }, segment = i to s.topTracks.size, showDuration = false,
                        subtitle = listOfNotNull(t.artist, t.album).joinToString(stringResource(R.string.dot_separator)),
                        leading = { Text((i + 1).toString(), style = MaterialTheme.typography.titleSmall, color = MaterialTheme.colorScheme.onSurfaceVariant, textAlign = TextAlign.Center, modifier = Modifier.width(28.dp)); Spacer(Modifier.width(4.dp)) },
                        modifier = Modifier.padding(start = 16.dp, end = 16.dp, top = if (i > 0) ListItemDefaults.SegmentedGap else 0.dp))
                }
            }
            if (s.topAlbums.isNotEmpty()) item(key = "albums") {
                StatsHeading(stringResource(R.string.stats_top_albums))
                androidx.compose.foundation.lazy.LazyRow(contentPadding = PaddingValues(horizontal = 16.dp), horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                    items(s.topAlbums, key = { it.id }) { a ->
                        Column(Modifier.width(128.dp).clickable { nav.navigate(Route.Album(a.id)) }) {
                            app.hocket.ui.components.Artwork(a.coverArt, app.hocket.core.ArtworkSizes.GRID, stringResource(R.string.row_album_a11y, a.name, a.artist ?: ""), Modifier.size(128.dp), RoundedCornerShape(20.dp))
                            Text(a.name, style = MaterialTheme.typography.titleSmall, fontWeight = FontWeight.SemiBold, maxLines = 1, overflow = TextOverflow.Ellipsis, modifier = Modifier.padding(top = 6.dp))
                            Text(a.artist.orEmpty(), style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant, maxLines = 1, overflow = TextOverflow.Ellipsis)
                        }
                    }
                }
            }
            if (s.topArtists.isNotEmpty()) {
                item(key = "h.artists") { StatsHeading(stringResource(R.string.stats_top_artists)) }
                itemsIndexed(s.topArtists, key = { _, a -> a.id }) { i, a -> app.hocket.ui.components.ArtistRow(a, onClick = { nav.navigate(Route.Artist(a.id)) }, segment = i to s.topArtists.size,
                    modifier = Modifier.padding(start = 16.dp, end = 16.dp, top = if (i > 0) ListItemDefaults.SegmentedGap else 0.dp)) }
            }
        }
    }
}

@Composable
private fun StatsHeading(text: String) {
    Text(text, style = MaterialTheme.typography.titleMedium, fontWeight = FontWeight.SemiBold, modifier = Modifier.padding(start = 20.dp, end = 20.dp, top = 24.dp, bottom = 10.dp).semantics { heading() })
}

@Composable
private fun ChartCard(content: @Composable () -> Unit) {
    Surface(shape = RoundedCornerShape(20.dp), color = segmentColor(), modifier = Modifier.fillMaxWidth().padding(horizontal = 16.dp)) {
        Box(Modifier.padding(16.dp)) { content() }
    }
}

@Composable
private fun SummaryTile(big: String, small: String, shape: androidx.compose.ui.graphics.Shape, container: androidx.compose.ui.graphics.Color, content: androidx.compose.ui.graphics.Color, modifier: Modifier) {
    Surface(shape = shape, color = container, contentColor = content, modifier = modifier) {
        Column(Modifier.padding(horizontal = 18.dp, vertical = 16.dp).semantics(mergeDescendants = true) { }) {
            Text(big, style = MaterialTheme.typography.displaySmall, fontWeight = FontWeight.Bold, maxLines = 1)
            Text(small, style = MaterialTheme.typography.titleSmall, color = content.copy(alpha = 0.8f), maxLines = 1)
        }
    }
}

/**
 * Bars on tracks, the busiest one in the accent; [readout] names it under the chart. [labels] sit
 * one under each bar ([centredLabels]) or spread along the axis ([spreadLabels]).
 */
@Composable
fun BarChart(
    values: List<Int>, labels: List<String>, modifier: Modifier = Modifier, readout: (@Composable (Int) -> String)? = null,
    height: androidx.compose.ui.unit.Dp = 120.dp, gap: androidx.compose.ui.unit.Dp = 4.dp, corner: androidx.compose.ui.unit.Dp = 5.dp,
    spreadLabels: Boolean = false, centredLabels: Boolean = false,
) {
    val max = (values.maxOrNull() ?: 0).coerceAtLeast(1)
    val peak = values.indexOf(values.maxOrNull() ?: 0)
    val peakBar = MaterialTheme.colorScheme.primary
    val bar = MaterialTheme.colorScheme.primary.copy(alpha = 0.6f)
    val track = MaterialTheme.colorScheme.surfaceContainer
    val desc = values.mapIndexed { i, v -> "${if (centredLabels) labels.getOrNull(i) ?: i else i}: $v" }.joinToString(", ")
    Column(modifier.fillMaxWidth().semantics { contentDescription = desc }) {
        Canvas(Modifier.fillMaxWidth().height(height)) {
            val n = values.size.coerceAtLeast(1)
            val g = gap.toPx()
            val w = (size.width - g * (n - 1)) / n
            val r = CornerRadius(corner.toPx())
            values.forEachIndexed { i, v ->
                val h = size.height * v / max
                val x = i * (w + g)
                drawRoundRect(track, Offset(x, 0f), Size(w, size.height), r)
                if (v > 0) drawRoundRect(if (i == peak) peakBar else bar, Offset(x, size.height - h), Size(w, h), r)
            }
        }
        Row(Modifier.fillMaxWidth().padding(top = 6.dp), horizontalArrangement = if (spreadLabels) Arrangement.SpaceBetween else Arrangement.spacedBy(gap)) {
            labels.forEach { Text(it, style = MaterialTheme.typography.labelMedium, color = MaterialTheme.colorScheme.onSurfaceVariant, textAlign = if (centredLabels) TextAlign.Center else null, modifier = if (spreadLabels) Modifier else Modifier.weight(1f)) }
        }
        if (readout != null && values.any { it > 0 }) Text(readout(peak), style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant, modifier = Modifier.padding(top = 10.dp))
    }
}
