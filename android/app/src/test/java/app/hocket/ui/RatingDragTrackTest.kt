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

/** A star drag rates the song it started on. */
class RatingDragTrackTest : PlayerRulesTestBase() {

    @Test
    fun aStarDragAcrossATrackChangeRatesTheSongItStartedOn() {
        val core = TestCore()
        startExpanded(core)
        val first = core.client.nowPlaying.value!!.track
        val star = { n: Int -> (n - 1) * 28f * dp + 10f * dp }
        compose.onNodeWithTag("rating").performTouchInput {
            down(Offset(star(1), height / 2f))
            moveTo(Offset(star(2), height / 2f))
            moveTo(Offset(star(4), height / 2f))
        }
        compose.waitForIdle()
        core.client.dispatch(Command.Next)
        compose.waitUntil(5_000) { core.client.nowPlaying.value?.track?.id != first.id }
        compose.waitForIdle()
        val second = core.client.nowPlaying.value!!.track
        compose.onNodeWithTag("rating").performTouchInput { up() }
        compose.waitForIdle()
        core.client.requestSnapshot()
        compose.waitForIdle()
        assertEquals("the new song is not rated", second.rating, core.fake.library.track(second.id)!!.rating)
        assertEquals("the song the drag began on is", 4u, core.fake.library.track(first.id)!!.rating)
    }
}
