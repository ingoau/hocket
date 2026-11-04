package app.hocket.ui.components

import androidx.compose.foundation.layout.BoxScope
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.ExperimentalMaterial3ExpressiveApi
import androidx.compose.material3.pulltorefresh.PullToRefreshBox
import androidx.compose.material3.pulltorefresh.PullToRefreshDefaults
import androidx.compose.material3.pulltorefresh.rememberPullToRefreshState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.derivedStateOf
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.runtime.snapshotFlow
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.unit.Dp
import app.hocket.core.Commands
import app.hocket.core.api.JobKind
import app.hocket.ui.LocalCoreClient
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.withTimeoutOrNull

/**
 * Pull down to sync the library now (the same incremental sync as Settings > Account > Sync now).
 * The expressive loading indicator morphs while the sync's job runs, then tucks away; it gives up
 * after [SYNC_SHOWN_MS] (a full sync can take minutes, and the jobs indicator keeps its progress).
 * [topInset] puts the indicator under the screen's top bar. Without a server it does nothing.
 */
@OptIn(ExperimentalMaterial3ExpressiveApi::class)
@Composable
fun SyncRefreshBox(topInset: Dp, modifier: Modifier = Modifier, content: @Composable BoxScope.() -> Unit) {
    val client = LocalCoreClient.current
    val server by client.server.collectAsStateWithLifecycleCompat()
    val jobs by client.activeJobs.collectAsStateWithLifecycleCompat()
    val syncing by rememberUpdatedState(jobs.any { it.kind == JobKind.LibrarySync })
    var refreshing by rememberSaveable { mutableStateOf(false) }
    LaunchedEffect(refreshing) {
        if (!refreshing) return@LaunchedEffect
        withTimeoutOrNull(SYNC_SHOWN_MS) {
            // The job may take a moment to be queued; if it never is (offline), stop at once.
            withTimeoutOrNull(SYNC_START_MS) { snapshotFlow { syncing }.first { it } }
            snapshotFlow { syncing }.first { !it }
        }
        refreshing = false
    }
    val state = rememberPullToRefreshState()
    // Tucked away, the indicator says nothing (it would otherwise read as an empty progress bar).
    val hidden by remember { derivedStateOf { !refreshing && state.distanceFraction == 0f } }
    val current = server
    PullToRefreshBox(
        isRefreshing = refreshing,
        onRefresh = {
            if (current != null) {
                client.dispatch(Commands.syncLibrary(current.id, full = false))
                refreshing = true
            }
        },
        state = state,
        modifier = modifier.testTag("pullToRefresh"),
        indicator = {
            PullToRefreshDefaults.LoadingIndicator(
                state = state,
                isRefreshing = refreshing,
                modifier = Modifier.align(Alignment.TopCenter).padding(top = topInset).then(if (hidden) Modifier.clearAndSetSemantics { } else Modifier),
            )
        },
        content = content,
    )
}

/** How long the indicator waits for the sync's job to appear. */
private const val SYNC_START_MS = 3_000L
/** The longest the indicator stays out. */
private const val SYNC_SHOWN_MS = 15_000L
