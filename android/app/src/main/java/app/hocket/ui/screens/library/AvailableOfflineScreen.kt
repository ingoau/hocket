package app.hocket.ui.screens.library

import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.ExperimentalMaterial3ExpressiveApi
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MediumFlexibleTopAppBar
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TopAppBarDefaults
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.input.nestedscroll.nestedScroll
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.navigation.NavHostController
import app.hocket.R
import app.hocket.core.Commands
import app.hocket.core.api.SortOrder
import app.hocket.ui.LocalCoreClient
import app.hocket.ui.screens.detail.PlayShuffleRow

/**
 * Everything that plays with no network: the core's built-in "Available offline" filter
 * (downloads and complete stream-cache entries), with play and shuffle. Opened from Downloads; the
 * Library's songs tab has the same view as a chip.
 */
@OptIn(ExperimentalMaterial3Api::class, ExperimentalMaterial3ExpressiveApi::class)
@Composable
fun AvailableOfflineScreen(nav: NavHostController) {
    val client = LocalCoreClient.current
    val server by client.server.collectAsStateWithLifecycle()
    val filters by client.filters.collectAsStateWithLifecycle()
    val serverId = server?.id ?: return
    val filter = availableOfflineFilter(filters)
    val scroll = TopAppBarDefaults.exitUntilCollapsedScrollBehavior()
    Scaffold(
        modifier = Modifier.nestedScroll(scroll.nestedScrollConnection).testTag("availableOffline.screen"),
        topBar = {
            MediumFlexibleTopAppBar(
                title = { Text(stringResource(R.string.available_offline), modifier = Modifier.semantics { heading() }) },
                subtitle = { Text(stringResource(R.string.settings_available_offline_body)) },
                navigationIcon = { IconButton(onClick = { nav.popBackStack() }) { Icon(Icons.AutoMirrored.Filled.ArrowBack, stringResource(R.string.action_back)) } },
                scrollBehavior = scroll,
            )
        },
    ) { padding ->
        Column(Modifier.padding(top = padding.calculateTopPadding()).fillMaxSize()) {
            Column(Modifier.padding(horizontal = 16.dp, vertical = 8.dp)) {
                PlayShuffleRow(
                    onPlay = { client.dispatch(Commands.playContext(Commands.filterContext(serverId, filter))) },
                    onShuffle = { client.dispatch(Commands.playContext(Commands.filterContext(serverId, filter), shuffle = true)) },
                )
            }
            SongsTab(serverId, SortOrder.Artist, false, nav, offlineOnly = true)
        }
    }
}
