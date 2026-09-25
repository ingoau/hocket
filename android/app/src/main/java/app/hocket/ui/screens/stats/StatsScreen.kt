package app.hocket.ui.screens.stats

import androidx.compose.foundation.Canvas
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
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
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
import androidx.compose.runtime.produceState
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
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import androidx.navigation.NavHostController
import app.hocket.R
import app.hocket.ui.nav.AccountButton
import app.hocket.ui.nav.BottomContentInset
import app.hocket.core.Queries
import app.hocket.core.api.ListeningStats
import app.hocket.core.api.QueryResult
import app.hocket.ui.LocalCoreClient
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
    val stats by produceState<ListeningStats?>(null, period) { value = (client.query(Queries.stats(period)) as? QueryResult.Stats)?.data }
    val scroll = TopAppBarDefaults.exitUntilCollapsedScrollBehavior()
    Scaffold(
        modifier = Modifier.nestedScroll(scroll.nestedScrollConnection),
        topBar = { MediumFlexibleTopAppBar(title = { Text(stringResource(R.string.stats_title)) }, actions = { AccountButton() }, navigationIcon = { IconButton(onClick = { nav.popBackStack() }) { Icon(Icons.AutoMirrored.Filled.ArrowBack, stringResource(R.string.action_back)) } }, scrollBehavior = scroll) },
    ) { padding ->
        val s = stats
        LazyColumn(Modifier.fillMaxSize(), contentPadding = PaddingValues(top = padding.calculateTopPadding(), bottom = BottomContentInset)) {
            item {
                Box(Modifier.fillMaxWidth().padding(16.dp), contentAlignment = Alignment.Center) {
                    val periods = listOf(7 to stringResource(R.string.stats_period_7), 30 to stringResource(R.string.stats_period_30), 365 to stringResource(R.string.stats_period_365))
                    app.hocket.ui.components.ChoiceRow(periods, isSelected = { period == it }, onSelect = { period = it })
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
            item {
                Row(Modifier.fillMaxWidth().padding(horizontal = 16.dp)) {
                    Text(stringResource(R.string.stats_plays, s.totalPlays.toInt()), style = MaterialTheme.typography.headlineSmall, modifier = Modifier.weight(1f))
                    Text(stringResource(R.string.stats_time, formatDurationWords(s.totalMs.toLong())), style = MaterialTheme.typography.titleMedium)
                }
            }
            item { SectionHeader(stringResource(R.string.stats_by_hour)); BarChart(s.playsByHour.map { it.toInt() }, labels = (0 until 24).map { if (it % 6 == 0) it.toString() else "" }) }
            item {
                val days = listOf(R.string.weekday_mon, R.string.weekday_tue, R.string.weekday_wed, R.string.weekday_thu, R.string.weekday_fri, R.string.weekday_sat, R.string.weekday_sun).map { stringResource(it) }
                SectionHeader(stringResource(R.string.stats_by_weekday)); BarChart(s.playsByWeekday.map { it.toInt() }, labels = days)
            }
            if (s.topTracks.isNotEmpty()) { item { SectionHeader(stringResource(R.string.stats_top_tracks)) }; items(s.topTracks, key = { it.id }) { t -> TrackRow(t, onClick = { t.albumId?.let { nav.navigate(Route.Album(it)) } }) } }
            if (s.topAlbums.isNotEmpty()) { item { SectionHeader(stringResource(R.string.stats_top_albums)); AlbumStrip(s.topAlbums) { nav.navigate(Route.Album(it.id)) } } }
            if (s.topArtists.isNotEmpty()) { item { SectionHeader(stringResource(R.string.stats_top_artists)) }; items(s.topArtists, key = { it.id }) { a -> app.hocket.ui.components.ArtistRow(a, onClick = { nav.navigate(Route.Artist(a.id)) }) } }
        }
    }
}

@Composable
fun BarChart(values: List<Int>, labels: List<String>, modifier: Modifier = Modifier) {
    val max = (values.maxOrNull() ?: 0).coerceAtLeast(1)
    val bar = MaterialTheme.colorScheme.primary
    val track = MaterialTheme.colorScheme.surfaceContainerHighest
    val desc = values.mapIndexed { i, v -> "${labels.getOrNull(i)?.ifEmpty { i.toString() } ?: i}: $v" }.joinToString(", ")
    Column(modifier.fillMaxWidth().padding(horizontal = 16.dp).semantics { contentDescription = desc }) {
        Canvas(Modifier.fillMaxWidth().height(120.dp)) {
            val n = values.size.coerceAtLeast(1)
            val gap = 4.dp.toPx()
            val w = (size.width - gap * (n - 1)) / n
            values.forEachIndexed { i, v ->
                val h = size.height * v / max
                val x = i * (w + gap)
                drawRoundRect(track, Offset(x, 0f), Size(w, size.height), CornerRadius(w / 3))
                if (v > 0) drawRoundRect(bar, Offset(x, size.height - h), Size(w, h), CornerRadius(w / 3))
            }
        }
        Row(Modifier.fillMaxWidth().padding(top = 4.dp)) {
            labels.forEach { Text(it, style = MaterialTheme.typography.labelSmall, color = MaterialTheme.colorScheme.onSurfaceVariant, modifier = Modifier.weight(1f)) }
        }
    }
}
