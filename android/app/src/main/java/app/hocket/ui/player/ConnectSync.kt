package app.hocket.ui.player

import android.os.SystemClock
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableLongStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import app.hocket.core.api.ConnectionState
import app.hocket.ui.LocalCoreClient
import kotlinx.coroutines.delay

/** How long the Connect controls wait for the room before showing local state undimmed. */
internal const val CONNECT_SYNC_TIMEOUT_MS = 5_000L

/** The alpha of a Connect control still waiting for the room's state. */
internal const val CONNECT_SYNC_ALPHA = 0.45f

/**
 * Whether the Connect controls (the device playing, the position) are still waiting for the room's
 * state: a coordinator is configured, not yet reached, with no error, and not for long. Offline or
 * unreachable, local state is all there is, so after [CONNECT_SYNC_TIMEOUT_MS] it shows as is.
 */
internal fun connectSyncing(c: ConnectionState, elapsedMs: Long): Boolean =
    c.coordinatorUrl != null && !c.connected && c.error == null && elapsedMs < CONNECT_SYNC_TIMEOUT_MS

/** When the UI first asked, and whether the room has answered (or we gave up) since. */
private object ConnectSync {
    var startedAt = -1L
    var settled = false
}

/**
 * [connectSyncing] for this UI's first connect only: once the room answers, a later drop doesn't
 * dim again (the offline banner covers that). A UI opened on a running service that is already
 * connected never dims.
 */
@Composable
internal fun rememberConnectSyncing(): Boolean {
    val client = LocalCoreClient.current
    val started by client.started.collectAsStateWithLifecycle()
    val connection by client.connection.collectAsStateWithLifecycle()
    if (ConnectSync.startedAt < 0) ConnectSync.startedAt = SystemClock.elapsedRealtime()
    var now by remember { mutableLongStateOf(SystemClock.elapsedRealtime()) }
    val elapsed = now - ConnectSync.startedAt
    // Before the first snapshot there's no telling whether a coordinator is configured: wait.
    val syncing = !ConnectSync.settled && (if (started) connectSyncing(connection, elapsed) else elapsed < CONNECT_SYNC_TIMEOUT_MS)
    if (!syncing) ConnectSync.settled = true
    LaunchedEffect(syncing) {
        if (syncing) {
            delay((ConnectSync.startedAt + CONNECT_SYNC_TIMEOUT_MS - SystemClock.elapsedRealtime()).coerceAtLeast(0))
            now = SystemClock.elapsedRealtime()
        }
    }
    return syncing
}
