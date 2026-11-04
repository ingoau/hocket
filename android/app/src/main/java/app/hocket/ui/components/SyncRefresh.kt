package app.hocket.ui.components

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxScope
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.ExperimentalMaterial3ExpressiveApi
import androidx.compose.material3.pulltorefresh.PullToRefreshDefaults
import androidx.compose.material3.pulltorefresh.PullToRefreshState
import androidx.compose.material3.pulltorefresh.pullToRefresh
import androidx.compose.material3.pulltorefresh.rememberPullToRefreshState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.MutableState
import androidx.compose.runtime.Stable
import androidx.compose.runtime.derivedStateOf
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.snapshotFlow
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
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
 * Without a server a pull does nothing.
 *
 * The gesture ([syncRefresh]) goes on the screen's Scaffold *outside* its top bar's scroll
 * connection, so dragging back up first takes the indicator in rather than collapsing the bar; the
 * indicator ([SyncRefreshBox]) is drawn inside the content, under the bar.
 */
@Stable
class SyncRefresh internal constructor(
    val state: PullToRefreshState,
    private val requested: MutableState<Boolean>,
    private val start: () -> Boolean,
) {
    val refreshing: Boolean get() = requested.value

    fun refresh() {
        if (start()) requested.value = true
    }
}

@Composable
fun rememberSyncRefresh(): SyncRefresh {
    val client = LocalCoreClient.current
    val server by client.server.collectAsStateWithLifecycleCompat()
    val jobs by client.activeJobs.collectAsStateWithLifecycleCompat()
    val syncing by rememberUpdatedState(jobs.any { it.kind == JobKind.LibrarySync })
    val currentServer by rememberUpdatedState(server)
    val requested = rememberSaveable { mutableStateOf(false) }
    LaunchedEffect(requested.value) {
        if (!requested.value) return@LaunchedEffect
        withTimeoutOrNull(SYNC_SHOWN_MS) {
            // The job may take a moment to be queued; if it never is (offline), stop at once.
            withTimeoutOrNull(SYNC_START_MS) { snapshotFlow { syncing }.first { it } }
            snapshotFlow { syncing }.first { !it }
        }
        requested.value = false
    }
    val state = rememberPullToRefreshState()
    return remember(state, requested, client) {
        SyncRefresh(state, requested) {
            val s = currentServer ?: return@SyncRefresh false
            client.dispatch(Commands.syncLibrary(s.id, full = false))
            true
        }
    }
}

/** The pull gesture; put it before the top bar's `nestedScroll` on the screen's Scaffold. */
fun Modifier.syncRefresh(refresh: SyncRefresh): Modifier =
    pullToRefresh(isRefreshing = refresh.refreshing, state = refresh.state, onRefresh = refresh::refresh)

/** [content] with [refresh]'s indicator over its top, [topInset] down (under the screen's top bar). */
@OptIn(ExperimentalMaterial3ExpressiveApi::class)
@Composable
fun SyncRefreshBox(refresh: SyncRefresh, topInset: Dp, modifier: Modifier = Modifier, content: @Composable BoxScope.() -> Unit) {
    Box(modifier) {
        content()
        // Tucked away, the indicator says nothing (it would otherwise read as an empty progress bar).
        val hidden by remember(refresh) { derivedStateOf { !refresh.refreshing && refresh.state.distanceFraction == 0f } }
        PullToRefreshDefaults.LoadingIndicator(
            state = refresh.state,
            isRefreshing = refresh.refreshing,
            modifier = Modifier.align(Alignment.TopCenter).padding(top = topInset).then(if (hidden) Modifier.clearAndSetSemantics { } else Modifier),
        )
    }
}

/** How long the indicator waits for the sync's job to appear. */
private const val SYNC_START_MS = 3_000L
/** The longest the indicator stays out. */
private const val SYNC_SHOWN_MS = 15_000L
