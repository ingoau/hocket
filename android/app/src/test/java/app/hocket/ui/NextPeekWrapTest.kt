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

/** The skip swipe's next peek at the queue's end follows the core's wrap. */
class NextPeekWrapTest : PlayerRulesTestBase() {

    private fun entry(key: String, index: Int?) = QueueEntry(
        QueueItem(key, "t-$key", if (index != null) QueueSource.Context(QueueSourceContextInner(index.toUInt())) else QueueSource.Inserted),
        TrackSummary(id = "t-$key", serverId = "s", title = key, durationMs = 1u, rating = 0u, loved = false, offline = OfflineState.None),
    )

    /** As the core derives it: [QueueView.wrapNext] only with Repeat All, [wrapsTo] in play order. */
    private fun atTheEnd(repeat: RepeatMode, shuffle: Boolean = false, wrapsTo: QueueEntry = entry("ctx-0", 0)) = QueueView(
        contextLabel = "Album",
        // Played from the context's second item, a queued song between: the context's first is not
        // the history's first.
        history = listOf(entry("q", null), entry("ctx-1", 1), entry("ctx-0", 0), entry("ctx-2", 2)),
        current = entry("ctx-3", 3),
        playingNext = emptyList(), upcoming = emptyList(),
        shuffle = shuffle, repeat = repeat, autoplay = false, mode = QueueMode.Apple, totalUpcoming = 0u,
        wrapNext = wrapsTo.takeIf { repeat == RepeatMode.All },
    )

    @Test
    fun repeatOneAtTheEndPeeksAtNothing() {
        assertNull(Neighbours.of(atTheEnd(RepeatMode.One)).next)
        assertNull(Neighbours.of(atTheEnd(RepeatMode.Off)).next)
    }

    @Test
    fun repeatAllAtTheEndPeeksAtTheContextsFirstItem() {
        assertEquals("ctx-0", Neighbours.of(atTheEnd(RepeatMode.All)).next?.item?.key)
    }

    @Test
    fun repeatAllWithShufflePeeksAtTheShuffledFirst() {
        // The core wraps to the shuffled order's first item and says which in `wrapNext`.
        assertEquals("ctx-2", Neighbours.of(atTheEnd(RepeatMode.All, shuffle = true, wrapsTo = entry("ctx-2", 2))).next?.item?.key)
    }
}
