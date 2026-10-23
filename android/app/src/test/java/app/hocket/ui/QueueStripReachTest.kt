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

/** Rows under the queue's toggles are out of reach. */
class QueueStripReachTest : PlayerRulesTestBase() {

    @Test
    fun rowsUnderTheQueueTogglesTakeNoTapsOrFocus() {
        val core = TestCore()
        startExpanded(core)
        tap("player.mode.queue")
        compose.waitUntil(5_000) { exists("queue.list") }
        compose.mainClock.advanceTimeBy(1_500)
        compose.waitForIdle()
        // Scroll so a row sits across the toggles' strip.
        compose.onNodeWithTag("queue.list").performSemanticsAction(SemanticsActions.ScrollBy) { it(0f, 44f * dp) }
        compose.waitForIdle()
        val controls = bounds("queue.controls")
        // Every row the accessibility tree offers sits above the toggles' strip.
        val rows = compose.onAllNodes(androidx.compose.ui.test.SemanticsMatcher("a queue row") { it.config.getOrNull(SemanticsProperties.TestTag)?.startsWith("queue.row.") == true })
            .fetchSemanticsNodes().filter { it.boundsInRoot.height > 0.5f }
        assertTrue("some rows", rows.isNotEmpty())
        println("QROWS controls=$controls rows=${rows.map { it.boundsInRoot.top.toInt() to it.boundsInRoot.bottom.toInt() }}")
        for (r in rows) assertTrue("a row reachable under the toggles: ${r.boundsInRoot} / $controls", r.boundsInRoot.bottom <= controls.top - 0.5f || r.boundsInRoot.top >= controls.bottom)
    }
}
