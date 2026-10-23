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
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.annotation.Config

/** "Playing on" and the accent device button only while another device holds the lease. */
class PlayingOnLeaseTest : PlayerRulesTestBase() {

    @Test
    fun nobodyHoldingTheLeaseIsNotAnotherDevicePlaying() {
        val core = TestCore(leaseHeld = false)
        startExpanded(core)
        assertFalse(core.client.ownsTransport.value)
        assertFalse("no \"Playing on\" on a cold start", exists("player.playingOn"))
        assertTrue(compose.onAllNodesWithText("Playing on", substring = true).fetchSemanticsNodes().isEmpty())
        assertNull("no accent device button", compose.onNodeWithTag("player.connect").fetchSemanticsNode().config.getOrNull(SemanticsProperties.StateDescription))
    }

    @Test
    fun afterStopNobodyHoldsTheLease() {
        val core = TestCore()
        startExpanded(core)
        core.client.dispatch(Command.Stop)
        core.client.requestSnapshot()
        compose.waitUntil(5_000) { !core.client.ownsTransport.value }
        compose.mainClock.advanceTimeBy(1_000)
        compose.waitForIdle()
        assertFalse("no \"Playing on\" after Stop", exists("player.playingOn"))
        assertNull(compose.onNodeWithTag("player.connect").fetchSemanticsNode().config.getOrNull(SemanticsProperties.StateDescription))
    }

    @Test
    fun aLeaseLeftByAGoneDeviceIsNotAnotherDevicePlaying() {
        val core = TestCore(staleLeaseOwner = "gone")
        startExpanded(core)
        assertFalse(core.client.ownsTransport.value)
        assertFalse("no \"Playing on another device\" for a device that is not listed", exists("player.playingOn"))
        assertTrue(compose.onAllNodesWithText("Playing on", substring = true).fetchSemanticsNodes().isEmpty())
        assertNull("no accent device button", compose.onNodeWithTag("player.connect").fetchSemanticsNode().config.getOrNull(SemanticsProperties.StateDescription))
    }

    @Test
    fun aListedDeviceHoldingTheLeaseIsNamed() {
        val core = TestCore(staleLeaseOwner = "laptop")
        startExpanded(core)
        assertTrue(exists("player.playingOn"))
        assertEquals("Playing on Laptop", compose.onNodeWithTag("player.connect").fetchSemanticsNode().config.getOrNull(SemanticsProperties.StateDescription))
    }
}
