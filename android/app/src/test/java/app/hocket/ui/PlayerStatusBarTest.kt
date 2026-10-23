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

/** The open player's status bar icons. */
class PlayerStatusBarTest : PlayerRulesTestBase() {

    @Test
    @Config(qualifiers = "+notnight")
    fun theOpenPlayersStatusBarIconsAreLightOverItsDarkBackdrop() {
        val core = TestCore()
        val controller = androidx.core.view.WindowCompat.getInsetsController(compose.activity.window, compose.activity.window.decorView)
        controller.isAppearanceLightStatusBars = true
        startExpanded(core)
        assertFalse("light icons over the dark player", controller.isAppearanceLightStatusBars)
        tap("player.mode.queue")
        compose.waitForIdle()
        assertFalse("in the queue too", controller.isAppearanceLightStatusBars)
        compose.onNodeWithTag("nowPlaying.sheet").performSemanticsAction(SemanticsActions.Collapse)
        compose.waitUntil(5_000) { controller.isAppearanceLightStatusBars }
    }
}
