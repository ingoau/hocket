package app.hocket.ui

import androidx.activity.ComponentActivity
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.semantics.getOrNull
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.test.onAllNodesWithTag
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.performSemanticsAction
import androidx.compose.ui.test.performTouchInput
import androidx.compose.runtime.snapshots.Snapshot
import androidx.test.ext.junit.runners.AndroidJUnit4
import app.hocket.core.Commands
import app.hocket.core.api.Command
import app.hocket.core.api.QueueEntry
import app.hocket.core.api.QueueItem
import app.hocket.core.api.QueueMode
import app.hocket.core.api.QueueSource
import app.hocket.core.api.QueueSourceContextInner
import app.hocket.core.api.QueueView
import app.hocket.core.api.RepeatMode
import app.hocket.core.api.TrackSummary
import app.hocket.core.api.OfflineState
import app.hocket.ui.nav.AppRoot
import app.hocket.ui.player.Neighbours
import app.hocket.ui.player.previousRestarts
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.annotation.Config

/** Previous restarts a track some device plays, as in the core. */
class PreviousRestartTest : PlayerRulesTestBase() {

    @Test
    fun previousRestartsATrackPlayingAnywhere() {
        assertTrue(previousRestarts(leaseHeld = true, positionMs = 5_000, historyEmpty = false))
        assertFalse("nobody plays (nothing loaded yet)", previousRestarts(leaseHeld = false, positionMs = 5_000, historyEmpty = false))
        assertFalse(previousRestarts(leaseHeld = true, positionMs = 1_000, historyEmpty = false))
        assertTrue("nothing before it", previousRestarts(leaseHeld = false, positionMs = 0, historyEmpty = true))
    }

    @Test
    fun theFakeRestartsWhileAnotherDevicePlays() {
        var clock = 1_000_000.0
        val core = TestCore(now = { clock })
        compose.setThemedContent(core) { AppRoot(core.client) }
        core.start()
        compose.waitUntil(5_000) { core.client.started.value }
        // Something to go back to, and far enough into the track to restart it.
        core.client.dispatch(Command.Next)
        compose.waitUntil(5_000) { core.client.queue.value.history.isNotEmpty() }
        clock += 10_000.0
        core.client.requestSnapshot()
        compose.waitForIdle()
        assertTrue("far enough in to restart", core.client.positionNow() >= 3_000)
        val before = core.client.queue.value.current?.item?.key
        core.client.dispatch(Commands.handoffTo("laptop"))
        core.client.dispatch(Command.Previous)
        core.client.requestSnapshot()
        compose.waitForIdle()
        assertEquals("restarted rather than going back", before, core.client.queue.value.current?.item?.key)
        assertTrue("from the top", core.client.positionNow() < 3_000)
    }
}
