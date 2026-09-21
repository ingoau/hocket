package app.hocket.ui.screens.downloads

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
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.filled.Delete
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
import androidx.compose.ui.input.nestedscroll.nestedScroll
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.navigation.NavHostController
import app.hocket.R
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
                title = { Text(stringResource(R.string.downloads_title)) },
                subtitle = { Text(stringResource(R.string.downloads_storage, formatBytes(storage.downloadsBytes), formatBytes(storage.cacheBytes), storage.freeBytes?.let { formatBytes(it) } ?: "?")) },
                navigationIcon = { IconButton(onClick = { nav.popBackStack() }) { Icon(Icons.AutoMirrored.Filled.ArrowBack, stringResource(R.string.action_back)) } },
                scrollBehavior = scroll,
            )
        },
    ) { padding ->
        if (pins.isEmpty()) {
            EmptyState(stringResource(R.string.empty_downloads_title), stringResource(R.string.empty_downloads_body), Modifier.padding(padding))
            return@Scaffold
        }
        LazyColumn(Modifier.fillMaxSize(), contentPadding = PaddingValues(top = padding.calculateTopPadding(), bottom = 160.dp)) {
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
                                Text(stringResource(R.string.downloads_progress, pin.downloadedCount.toInt(), pin.trackCount.toInt()) + stringResource(R.string.dot_separator) + formatBytes(pin.bytes), style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
                                Badge(stringResource(if (pin.transcoded) R.string.downloads_transcoded else R.string.downloads_original))
                            }
                        }
                        IconButton(onClick = { removing = pin }) { Icon(Icons.Filled.Delete, stringResource(R.string.action_remove_download)) }
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
