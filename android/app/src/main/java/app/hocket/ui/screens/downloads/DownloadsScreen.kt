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
                ListItem(
                    headlineContent = { Text(stringResource(R.string.available_offline)) },
                    supportingContent = { Text(stringResource(R.string.settings_available_offline_body)) },
                    leadingContent = { Icon(HocketIcons.Outlined.OfflineBolt, null) },
                    trailingContent = { Icon(HocketIcons.Filled.ChevronRight, null) },
                    modifier = Modifier.clickable { nav.navigate(Route.AvailableOffline) }.testTag("downloads.availableOffline"),
                )
            }
            if (pins.isNotEmpty()) item(key = "h.pins") { app.hocket.ui.components.SectionHeader(pluralStringResource(R.plurals.downloads_count, pins.size, pins.size)) }
            if (pins.isEmpty()) item(key = "empty") { EmptyState(stringResource(R.string.empty_downloads_title), stringResource(R.string.empty_downloads_body)) }
            if (warn != null && storage.downloadsBytes > warn) {
                item { Text(stringResource(R.string.downloads_warn, formatBytes(warn)), color = MaterialTheme.colorScheme.error, modifier = Modifier.padding(16.dp)) }
            }
            items(pins, key = { it.label + it.createdAt }) { pin ->
                Column(Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 8.dp)) {
                    Row(verticalAlignment = Alignment.CenterVertically) {
                        Artwork(pin.coverArt, ArtworkSizes.THUMB, null, Modifier.size(48.dp), RoundedCornerShape(8.dp))
                        Spacer(Modifier.width(14.dp))
                        Column(Modifier.weight(1f)) {
                            Text(pin.label, style = MaterialTheme.typography.bodyLarge)
                            Row(horizontalArrangement = androidx.compose.foundation.layout.Arrangement.spacedBy(6.dp), verticalAlignment = Alignment.CenterVertically) {
                                Text(stringResource(pinKind(pin.target)) + stringResource(R.string.dot_separator) + stringResource(R.string.downloads_progress, pin.downloadedCount.toInt(), pin.trackCount.toInt()) + stringResource(R.string.dot_separator) + formatBytes(pin.bytes), style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
                                Badge(stringResource(if (pin.transcoded) R.string.downloads_transcoded else R.string.downloads_original))
                            }
                        }
                        IconButton(onClick = { removing = pin }) { Icon(HocketIcons.Filled.Delete, stringResource(R.string.action_remove_download)) }
                    }
                    if (pin.downloadedCount < pin.trackCount) {
                        LinearWavyProgressIndicator(progress = { pin.downloadedCount.toFloat() / pin.trackCount.toFloat().coerceAtLeast(1f) }, modifier = Modifier.fillMaxWidth().padding(top = 6.dp))
                    }
                }
            }
            item {
                Row(Modifier.fillMaxWidth().padding(16.dp), horizontalArrangement = androidx.compose.foundation.layout.Arrangement.SpaceBetween) {
                    TextButton(onClick = { client.dispatch(Command.ClearStreamCache) }) { Text(stringResource(R.string.downloads_clear_cache)) }
                    Text(stringResource(R.string.downloads_threshold) + ": " + (warn?.let { formatBytes(it) } ?: stringResource(R.string.downloads_threshold_none)), style = MaterialTheme.typography.bodySmall, modifier = Modifier.align(Alignment.CenterVertically))
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

/** What this phone's storage holds: downloads, the stream cache and what is free, as one bar with its legend. */
@Composable
private fun StorageCard(downloads: Double, cache: Double, free: Double?) {
    val whole = (downloads + cache + (free ?: 0.0)).coerceAtLeast(1.0)
    val dl = MaterialTheme.colorScheme.primary
    val ca = MaterialTheme.colorScheme.tertiary
    val track = MaterialTheme.colorScheme.surfaceContainerHighest
    Surface(shape = RoundedCornerShape(28.dp), color = MaterialTheme.colorScheme.surfaceContainer, modifier = Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 8.dp).testTag("downloads.storage")) {
        Column(Modifier.padding(20.dp), verticalArrangement = androidx.compose.foundation.layout.Arrangement.spacedBy(12.dp)) {
            Row(Modifier.fillMaxWidth().height(12.dp).clip(RoundedCornerShape(6.dp)).background(track)) {
                if (downloads > 0) Box(Modifier.fillMaxHeight().weight((downloads / whole).toFloat()).background(dl))
                if (cache > 0) Box(Modifier.fillMaxHeight().weight((cache / whole).toFloat()).background(ca))
                val rest = (1.0 - (downloads + cache) / whole).toFloat()
                if (rest > 0f) Spacer(Modifier.weight(rest))
            }
            Legend(dl, stringResource(R.string.downloads_legend_downloads), formatBytes(downloads))
            Legend(ca, stringResource(R.string.downloads_legend_cache), formatBytes(cache))
            if (free != null) Legend(track, stringResource(R.string.downloads_legend_free), formatBytes(free))
        }
    }
}

@Composable
private fun Legend(color: androidx.compose.ui.graphics.Color, label: String, value: String) {
    Row(verticalAlignment = Alignment.CenterVertically) {
        Box(Modifier.size(10.dp).clip(CircleShape).background(color))
        Spacer(Modifier.width(10.dp))
        Text(label, style = MaterialTheme.typography.bodyMedium, modifier = Modifier.weight(1f))
        Text(value, style = MaterialTheme.typography.bodyMedium)
    }
}
