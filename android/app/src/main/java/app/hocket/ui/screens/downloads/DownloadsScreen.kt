package app.hocket.ui.screens.downloads

import app.hocket.core.api.PinTarget
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.draw.clip
import androidx.compose.material3.Surface
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.material3.IconButtonDefaults
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.ListItemDefaults
import androidx.compose.material3.SegmentedListItem
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import app.hocket.ui.components.GroupLabel
import app.hocket.ui.components.Segment
import app.hocket.ui.components.SegmentedGroup
import app.hocket.ui.components.segmentColor
import app.hocket.ui.components.segmentedListColors
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.LargeFlexibleTopAppBar
import androidx.compose.material3.LinearWavyProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBarDefaults
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.input.nestedscroll.nestedScroll
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.navigation.NavHostController
import app.hocket.R
import app.hocket.ui.icons.HocketIcons
import app.hocket.ui.nav.AccountButton
import androidx.compose.foundation.clickable
import androidx.compose.material3.ListItem
import androidx.compose.ui.platform.testTag
import app.hocket.ui.nav.Route
import app.hocket.ui.nav.BottomContentInset
import app.hocket.core.ArtworkSizes
import app.hocket.core.Commands
import app.hocket.core.api.Command
import app.hocket.core.api.Pin
import app.hocket.ui.LocalCoreClient
import app.hocket.ui.components.Artwork
import app.hocket.ui.components.Badge
import app.hocket.ui.components.ConfirmDialog
import app.hocket.ui.components.EmptyState
import app.hocket.ui.components.formatBytes

/** Pins with sizes and progress, the storage summary and its warn threshold. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun DownloadsScreen(nav: NavHostController) {
    val client = LocalCoreClient.current
    val pins by client.pins.collectAsStateWithLifecycle()
    val storage by client.storage.collectAsStateWithLifecycle()
    var removing by remember { mutableStateOf<Pin?>(null) }
    val scroll = TopAppBarDefaults.exitUntilCollapsedScrollBehavior()
    val warn = storage.warnThresholdBytes
    Scaffold(
        modifier = Modifier.nestedScroll(scroll.nestedScrollConnection),
        topBar = {
            LargeFlexibleTopAppBar(
                title = { Text(stringResource(R.string.downloads_title), style = app.hocket.ui.brand.brandTitleStyle(), maxLines = 1, modifier = Modifier.semantics { heading() }) },
                subtitle = { Text(stringResource(R.string.downloads_storage, formatBytes(storage.downloadsBytes), formatBytes(storage.cacheBytes), storage.freeBytes?.let { formatBytes(it) } ?: "?")) },
                navigationIcon = { IconButton(onClick = { nav.popBackStack() }) { Icon(HocketIcons.AutoMirrored.Filled.ArrowBack, stringResource(R.string.action_back)) } },
                actions = { AccountButton() },
                scrollBehavior = scroll,
            )
        },
    ) { padding ->
        LazyColumn(Modifier.fillMaxSize(), contentPadding = PaddingValues(top = padding.calculateTopPadding(), bottom = BottomContentInset)) {
            item(key = "storage") { StorageCard(storage.downloadsBytes, storage.cacheBytes, storage.freeBytes) }
            // Everything that plays offline: downloads plus complete stream-cache entries.
            item(key = "availableOffline") {
                Surface(onClick = { nav.navigate(Route.AvailableOffline) }, shape = RoundedCornerShape(32.dp), color = MaterialTheme.colorScheme.secondaryContainer,
                    modifier = Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 6.dp).testTag("downloads.availableOffline")) {
                    Row(Modifier.heightIn(min = 72.dp).padding(start = 20.dp, end = 16.dp, top = 12.dp, bottom = 12.dp), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(16.dp)) {
                        Icon(HocketIcons.Filled.Bolt, null, tint = MaterialTheme.colorScheme.primary)
                        Column(Modifier.weight(1f)) {
                            Text(stringResource(R.string.available_offline), style = MaterialTheme.typography.titleMedium, fontWeight = FontWeight.SemiBold)
                            Text(stringResource(R.string.settings_available_offline_body), style = MaterialTheme.typography.bodyMedium)
                        }
                        Icon(HocketIcons.Filled.ChevronRight, null)
                    }
                }
            }
            if (pins.isNotEmpty()) item(key = "h.pins") { GroupLabel(pluralStringResource(R.plurals.downloads_count, pins.size, pins.size), Modifier.padding(start = 16.dp, end = 16.dp, top = 12.dp)) }
            if (pins.isEmpty()) item(key = "empty") { EmptyState(stringResource(R.string.empty_downloads_title), stringResource(R.string.empty_downloads_body)) }
            if (warn != null && storage.downloadsBytes > warn) {
                item { Text(stringResource(R.string.downloads_warn, formatBytes(warn)), color = MaterialTheme.colorScheme.error, modifier = Modifier.padding(16.dp)) }
            }
            itemsIndexed(pins, key = { _, p -> p.label + p.createdAt }) { i, pin ->
                Segment(i, pins.size, Modifier.padding(start = 16.dp, end = 16.dp, top = if (i > 0) ListItemDefaults.SegmentedGap else 0.dp)) {
                    Row(Modifier.heightIn(min = 72.dp).padding(start = 12.dp, end = 4.dp, top = 10.dp, bottom = 10.dp), verticalAlignment = Alignment.CenterVertically) {
                        Artwork(pin.coverArt, ArtworkSizes.THUMB, null, Modifier.size(48.dp), RoundedCornerShape(14.dp))
                        Spacer(Modifier.width(14.dp))
                        Column(Modifier.weight(1f)) {
                            Text(pin.label, style = MaterialTheme.typography.bodyLarge, maxLines = 1, overflow = TextOverflow.Ellipsis)
                            Row(horizontalArrangement = Arrangement.spacedBy(6.dp), verticalAlignment = Alignment.CenterVertically) {
                                Text(stringResource(pinKind(pin.target)) + stringResource(R.string.dot_separator) + stringResource(R.string.downloads_progress, pin.downloadedCount.toInt(), pin.trackCount.toInt()) + stringResource(R.string.dot_separator) + formatBytes(pin.bytes),
                                    style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant, maxLines = 1, overflow = TextOverflow.Ellipsis, modifier = Modifier.weight(1f, fill = false))
                                Badge(stringResource(if (pin.transcoded) R.string.downloads_transcoded else R.string.downloads_original), container = MaterialTheme.colorScheme.surfaceContainerHigh)
                            }
                            if (pin.downloadedCount < pin.trackCount) {
                                LinearProgressIndicator(progress = { pin.downloadedCount.toFloat() / pin.trackCount.toFloat().coerceAtLeast(1f) }, modifier = Modifier.fillMaxWidth().padding(top = 8.dp, end = 8.dp))
                            }
                        }
                        IconButton(onClick = { removing = pin }, shapes = IconButtonDefaults.shapes()) { Icon(HocketIcons.Outlined.Delete, stringResource(R.string.action_remove_download)) }
                    }
                }
            }
            item(key = "tools") {
                SegmentedGroup(Modifier.padding(start = 16.dp, end = 16.dp, top = 24.dp)) {
                    SegmentedListItem(onClick = { client.dispatch(Command.ClearStreamCache) }, shapes = ListItemDefaults.segmentedShapes(0, 2), colors = segmentedListColors(),
                        leadingContent = { Icon(HocketIcons.Outlined.Delete, null) }, trailingContent = { Text(formatBytes(storage.cacheBytes)) }, modifier = Modifier.testTag("downloads.clearCache"),
                    ) { Text(stringResource(R.string.downloads_clear_cache)) }
                    SegmentedListItem(onClick = { nav.navigate(Route.SettingsDownloads) }, shapes = ListItemDefaults.segmentedShapes(1, 2), colors = segmentedListColors(),
                        leadingContent = { Icon(HocketIcons.Outlined.Warning, null) }, trailingContent = { Text(warn?.let { formatBytes(it) } ?: stringResource(R.string.downloads_threshold_none)) },
                    ) { Text(stringResource(R.string.downloads_threshold)) }
                }
            }
        }
    }
    removing?.let { pin -> ConfirmDialog(stringResource(R.string.downloads_remove_confirm, pin.label), stringResource(R.string.action_delete), onConfirm = { client.dispatch(Commands.unpin(pin.target)) }, onDismiss = { removing = null }) }
}

private fun pinKind(target: PinTarget): Int = when (target) {
    is PinTarget.Album -> R.string.downloads_kind_album
    is PinTarget.Playlist -> R.string.downloads_kind_playlist
    else -> R.string.downloads_kind_song
}

/**
 * What this phone's storage holds: downloads, the stream cache and what is free, as one segmented
 * bar (rounded pieces with gaps, M3 Expressive's progress style) over an inline legend.
 */
@Composable
private fun StorageCard(downloads: Double, cache: Double, free: Double?) {
    val whole = (downloads + cache + (free ?: 0.0)).coerceAtLeast(1.0)
    val dl = MaterialTheme.colorScheme.primary
    val ca = MaterialTheme.colorScheme.primary.copy(alpha = 0.45f)
    val track = segmentColor()
    Surface(shape = RoundedCornerShape(28.dp), color = MaterialTheme.colorScheme.surfaceContainer, modifier = Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 8.dp).testTag("downloads.storage")) {
        Column(Modifier.padding(start = 16.dp, end = 16.dp, top = 16.dp, bottom = 12.dp), verticalArrangement = Arrangement.spacedBy(10.dp)) {
            Row(Modifier.fillMaxWidth().height(20.dp), horizontalArrangement = Arrangement.spacedBy(4.dp)) {
                // A sliver of each is kept visible, so a small cache still reads as a piece of the bar.
                val min = 0.02
                if (downloads > 0) Box(Modifier.fillMaxHeight().weight(maxOf(downloads / whole, min).toFloat()).background(dl, CircleShape))
                if (cache > 0) Box(Modifier.fillMaxHeight().weight(maxOf(cache / whole, min).toFloat()).background(ca, CircleShape))
                val rest = (1.0 - (downloads + cache) / whole).toFloat()
                if (rest > 0f) Box(Modifier.fillMaxHeight().weight(rest).background(track, CircleShape))
            }
            FlowRow(horizontalArrangement = Arrangement.spacedBy(20.dp), verticalArrangement = Arrangement.spacedBy(4.dp)) {
                Legend(dl, stringResource(R.string.downloads_legend_downloads), formatBytes(downloads))
                Legend(ca, stringResource(R.string.downloads_legend_cache_short), formatBytes(cache))
                if (free != null) Legend(null, stringResource(R.string.downloads_legend_free), formatBytes(free))
            }
        }
    }
}

/** A legend entry: its colour's dot (a ring for free space), the label and the size. */
@Composable
private fun Legend(color: androidx.compose.ui.graphics.Color?, label: String, value: String) {
    Row(Modifier.heightIn(min = 32.dp).semantics(mergeDescendants = true) { }, verticalAlignment = Alignment.CenterVertically) {
        if (color != null) Box(Modifier.size(10.dp).clip(CircleShape).background(color))
        else Box(Modifier.size(10.dp).border(1.5.dp, MaterialTheme.colorScheme.outline, CircleShape))
        Spacer(Modifier.width(8.dp))
        Text(label, style = MaterialTheme.typography.bodyMedium)
        Spacer(Modifier.width(6.dp))
        Text(value, style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
    }
}
