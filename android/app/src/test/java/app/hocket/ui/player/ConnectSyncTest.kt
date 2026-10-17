package app.hocket.ui.player

import app.hocket.core.api.ConnectionState
import app.hocket.core.api.ConnectionTier
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class ConnectSyncTest {
    private fun conn(connected: Boolean = false, url: String? = "wss://c/ws", error: String? = null) =
        ConnectionState(tier = ConnectionTier.Local, connected = connected, coordinatorUrl = url, clockOffsetMs = 0.0, roundTripMs = null, peerCount = 0u, error = error)

    @Test
    fun waitsWhileACoordinatorIsConfiguredAndNotYetReached() {
        assertTrue(connectSyncing(conn(), 0))
    }

    @Test
    fun showsLocalStateOnceConnectedOnAnErrorOrWithoutACoordinator() {
        assertFalse(connectSyncing(conn(connected = true), 0))
        assertFalse(connectSyncing(conn(error = "unreachable"), 0))
        assertFalse(connectSyncing(conn(url = null), 0))
    }

    @Test
    fun stopsWaitingAfterTheTimeout() {
        assertTrue(connectSyncing(conn(), CONNECT_SYNC_TIMEOUT_MS - 1))
        assertFalse(connectSyncing(conn(), CONNECT_SYNC_TIMEOUT_MS))
    }
}
