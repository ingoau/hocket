package app.hocket.ui

import app.hocket.core.groupEntries
import app.hocket.core.historyEntries
import app.hocket.core.restEntries
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performScrollToNode
import androidx.compose.ui.test.performTouchInput
import androidx.compose.ui.test.hasTestTag
import androidx.compose.ui.test.swipeUp
import androidx.compose.ui.test.swipeLeft
import androidx.compose.ui.test.swipeDown
import androidx.compose.ui.Modifier
import androidx.compose.ui.Alignment
import app.hocket.core.windowEnd
import androidx.compose.ui.geometry.Offset
import androidx.test.ext.junit.runners.AndroidJUnit4
import app.hocket.core.Commands
import app.hocket.core.api.Command
import app.hocket.core.api.RepeatMode
import app.hocket.ui.queue.QueuePanel
import app.hocket.ui.queue.RecentQueuesList
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.semantics.getOrNull
import androidx.compose.ui.test.SemanticsMatcher
import androidx.compose.ui.test.assertContentDescriptionEquals
import androidx.compose.ui.test.assertIsNotDisplayed
import androidx.compose.ui.test.assertIsOff
import androidx.compose.ui.test.assertIsOn
import androidx.compose.ui.test.assertIsToggleable
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.unit.dp
import androidx.navigation.compose.rememberNavController
import org.junit.Assert.assertTrue
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotEquals
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode

@RunWith(AndroidJUnit4::class)
@Config(sdk = [35], application = android.app.Application::class, qualifiers = "w411dp-h891dp")
@GraphicsMode(GraphicsMode.Mode.NATIVE)
class QueueScreenTest {
    @get:Rule
    val compose = createComposeRule()

    @Test
    fun oneListShowsSectionsAndJumpingChangesTheCurrentTrack() {
        val core = TestCore()
        compose.setThemedContent(core) { QueuePanel() }
        core.start()
        compose.waitUntil(5_000) { core.client.started.value }
        val queue = core.client.queue.value
        val current = queue.current!!.track.title
        // No tabs: saved / recent queues and the undo history live in Library's "Recent queues".
        assertTrue(compose.onAllNodes(hasTestTagPrefix("queue.tab.")).fetchSemanticsNodes().isEmpty())
        compose.onNodeWithText("Now playing").assertIsDisplayed()
        compose.onNodeWithText("Next in queue · 1").assertIsDisplayed()
        compose.onNodeWithTag("queue.group.stash").assertIsDisplayed()
        compose.onNodeWithTag("queue.group.clear").assertIsDisplayed()
        compose.onNodeWithTag("queue.list").performScrollToNode(hasText("Next from: " + queue.contextLabel))
        compose.onNodeWithText("Next from: " + queue.contextLabel).assertIsDisplayed()
        // Tap the first entry after the group: it becomes current, the pointer passing the group.
        val target = queue.restEntries.first()
        compose.onNodeWithTag("queue.list").performScrollToNode(hasText(target.track.title))
        compose.onAllNodesWithText(target.track.title).onFirst().performClick()
        compose.waitUntil(5_000) { core.client.nowPlaying.value?.track?.id == target.track.id }
        assertNotEquals(current, core.client.nowPlaying.value!!.track.title)
        assertEquals(target.track.id, core.client.queue.value.current!!.track.id)
    }

    @Test
    fun tappingThePlayingRowNeverSendsItBackToTheStart() {
        val core = TestCore()
        compose.setThemedContent(core) { QueuePanel() }
        core.start()
        compose.waitUntil(5_000) { core.client.started.value }
        compose.waitForIdle()
        val key = core.client.queue.value.current!!.item.key
        core.client.dispatch(Commands.seekTo(60_000))
        compose.waitForIdle()
        compose.onNodeWithTag("queue.row.$key").performClick()
        compose.waitForIdle()
        assertEquals(key, core.client.queue.value.current!!.item.key)
        assertTrue("still ${core.client.positionNow()} ms in, not restarted", core.client.positionNow() >= 60_000)
        // Paused, the tap plays on from there.
        core.client.dispatch(Command.Pause)
        compose.waitForIdle()
        compose.onNodeWithTag("queue.row.$key").performClick()
        compose.waitForIdle()
        assertTrue(core.client.isPlaying.value)
        assertTrue(core.client.positionNow() >= 60_000)
    }

    @Test
    fun historySitsAboveNowPlayingAndCanBeReplayedOrCleared() {
        val core = TestCore()
        compose.setThemedContent(core) { QueuePanel() }
        core.start()
        compose.waitUntil(5_000) { core.client.started.value }
        val h0 = core.client.queue.value.historyCount.toInt()
        repeat(3) { core.client.dispatch(Command.Next) }
        compose.waitUntil(5_000) { core.client.queue.value.historyEntries.size == h0 + 3 }
        compose.waitForIdle()
        // The list follows the current track: "Now playing" at the top, history scrolled away above.
        compose.onNodeWithText("Now playing").assertIsDisplayed()
        compose.onNodeWithText("History").assertIsNotDisplayed()
        val played = core.client.queue.value.historyEntries.first()
        compose.onNodeWithTag("queue.list").performScrollToNode(hasText("History"))
        compose.onNodeWithText("History").assertIsDisplayed()
        // Oldest first, directly under the header; tapping moves the pointer back to it.
        val total = core.client.queue.value.total
        compose.onAllNodesWithText(played.track.title).onFirst().assertIsDisplayed().performClick()
        compose.waitUntil(5_000) { core.client.queue.value.current?.item?.key == played.item.key }
        assertEquals("navigation never adds or removes", total, core.client.queue.value.total)
        assertEquals(0u, core.client.queue.value.historyCount)
        // Build history again and clear it with the header's "Clear".
        repeat(2) { core.client.dispatch(Command.Next) }
        compose.waitUntil(5_000) { core.client.queue.value.historyEntries.size >= 2 }
        compose.onNodeWithTag("queue.list").performScrollToNode(hasText("History"))
        compose.onNodeWithContentDescription("Clear history").performClick()
        compose.waitUntil(5_000) { core.client.queue.value.historyEntries.isEmpty() }
        compose.onNodeWithText("History").assertDoesNotExist()
    }

    @Test
    fun theGroupHasItsOwnClearWithUndo() {
        val core = TestCore()
        val snackbar = androidx.compose.material3.SnackbarHostState()
        compose.setThemedContent(core) {
            val scope = androidx.compose.runtime.rememberCoroutineScope()
            androidx.compose.runtime.CompositionLocalProvider(app.hocket.ui.components.LocalAppSnackbar provides app.hocket.ui.components.AppSnackbar(snackbar, scope)) {
                androidx.compose.foundation.layout.Box { QueuePanel(); androidx.compose.material3.SnackbarHost(snackbar, Modifier.align(Alignment.BottomCenter)) }
            }
        }
        core.start()
        compose.waitUntil(5_000) { core.client.started.value }
        val group = core.client.queue.value.groupEntries.map { it.item.key }
        assertTrue(group.isNotEmpty())
        compose.onNodeWithContentDescription("Clear next in queue").performClick()
        compose.waitUntil(5_000) { core.client.queue.value.groupCount == 0u }
        // The heading goes with the group.
        compose.waitUntil(5_000) { compose.onAllNodesWithText("Next in queue", substring = true).fetchSemanticsNodes().isEmpty() }
        compose.waitUntil(5_000) { compose.onAllNodesWithText("Undo").fetchSemanticsNodes().isNotEmpty() }
        compose.onNodeWithText("Cleared next in queue").assertIsDisplayed()
        compose.onNodeWithText("Undo").performClick()
        compose.waitUntil(5_000) { core.client.queue.value.groupEntries.map { it.item.key } == group }
    }

    @Test
    fun swipingARowAwayOffersUndo() {
        val core = TestCore()
        val snackbar = androidx.compose.material3.SnackbarHostState()
        compose.setThemedContent(core) {
            val scope = androidx.compose.runtime.rememberCoroutineScope()
            androidx.compose.runtime.CompositionLocalProvider(app.hocket.ui.components.LocalAppSnackbar provides app.hocket.ui.components.AppSnackbar(snackbar, scope)) {
                androidx.compose.foundation.layout.Box { QueuePanel(); androidx.compose.material3.SnackbarHost(snackbar, Modifier.align(Alignment.BottomCenter)) }
            }
        }
        core.start()
        compose.waitUntil(5_000) { core.client.started.value }
        compose.waitForIdle()
        val gone = core.client.queue.value.restEntries.first()
        compose.onNodeWithTag("queue.list").performScrollToNode(hasTestTag("queue.row.${gone.item.key}"))
        compose.onNodeWithTag("queue.row.${gone.item.key}").performTouchInput { swipeLeft() }
        compose.waitUntil(5_000) { core.client.queue.value.entries.none { it.item.key == gone.item.key } }
        compose.waitUntil(5_000) { compose.onAllNodesWithText("Undo").fetchSemanticsNodes().isNotEmpty() }
        compose.onNodeWithText("Removed from queue").assertIsDisplayed()
        compose.onNodeWithText("Undo").performClick()
        compose.waitUntil(5_000) { core.client.queue.value.entries.any { it.item.key == gone.item.key } }
    }

    @Test
    fun addedSongsOutsideTheGroupAndAutoplaySongsCarryMarkers() {
        val core = TestCore()
        compose.setThemedContent(core) { QueuePanel() }
        core.start()
        compose.waitUntil(5_000) { core.client.started.value }
        // One song, so autoplay tops the list up, then a song added later and one added to the queue.
        val lib = core.fake.library
        core.client.dispatch(Commands.playTracks(lib.serverId, listOf(lib.tracks[5].id), 0, "One"))
        compose.waitUntil(5_000) { core.client.queue.value.restEntries.any { it.autoplay == true } }
        core.client.dispatch(Commands.playLater(lib.serverId, listOf(lib.tracks[6].id)))
        core.client.dispatch(Commands.addToQueue(lib.serverId, listOf(lib.tracks[7].id)))
        compose.waitUntil(5_000) { core.client.queue.value.groupCount == 1u && core.client.queue.value.restEntries.any { it.item.added == true } }
        compose.waitForIdle()
        val q = core.client.queue.value
        // The group's own rows have no marker: the heading says it.
        val grouped = q.groupEntries.single()
        assertEquals(true, grouped.item.added)
        compose.onNodeWithTag("queue.added.${grouped.item.key}", useUnmergedTree = true).assertDoesNotExist()
        // An added song later on shows the queue icon, and says so.
        val later = q.restEntries.first { it.item.added == true }
        compose.onNodeWithTag("queue.list").performScrollToNode(hasTestTag("queue.row.${later.item.key}"))
        compose.onNodeWithTag("queue.added.${later.item.key}", useUnmergedTree = true).assertExists()
        assertTrue(compose.onNodeWithTag("queue.row.${later.item.key}").fetchSemanticsNode().config[SemanticsProperties.StateDescription].contains("Added by you"))
        // Context songs have none; autoplay's have its own.
        compose.onNodeWithTag("queue.added.${q.current!!.item.key}", useUnmergedTree = true).assertDoesNotExist()
        val auto = q.restEntries.first { it.autoplay == true }
        compose.onNodeWithTag("queue.list").performScrollToNode(hasTestTag("queue.row.${auto.item.key}"))
        compose.onNodeWithTag("queue.autoplay.${auto.item.key}", useUnmergedTree = true).assertExists()
        compose.onNodeWithTag("queue.added.${auto.item.key}", useUnmergedTree = true).assertDoesNotExist()
    }

    @Test
    fun withoutAContextTheRestIsLater() {
        val core = TestCore()
        compose.setThemedContent(core) { QueuePanel() }
        core.start()
        compose.waitUntil(5_000) { core.client.started.value }
        core.client.dispatch(Command.ClearQueue)
        core.client.dispatch(Commands.playLater(core.fake.library.serverId, listOf(core.fake.library.tracks[40].id)))
        compose.waitUntil(5_000) { core.client.queue.value.restEntries.none { it.item.source is app.hocket.core.api.QueueSource.Context } }
        compose.waitForIdle()
        compose.onNodeWithTag("queue.list").performScrollToNode(hasText("Later"))
        compose.onNodeWithText("Later").assertIsDisplayed()
    }

    @Test
    fun scrollingIntoALongHistoryPagesItIn() {
        val core = TestCore()
        compose.setThemedContent(core) { QueuePanel() }
        core.start()
        compose.waitUntil(5_000) { core.client.started.value }
        val ids = core.fake.library.tracks.take(400).map { it.id }
        core.client.dispatch(Commands.playTracks(core.fake.library.serverId, ids, 250, "Long"))
        compose.waitUntil(5_000) { core.client.queue.value.historyCount == 250u }
        compose.waitForIdle()
        // The core sends 100 rows each side of the pointer; the rest comes in on scroll.
        assertEquals(150u, core.client.queue.value.windowStart)
        repeat(60) {
            if (core.client.queue.value.windowStart > 0u) {
                compose.onNodeWithTag("queue.list").performTouchInput { swipeDown() }
                compose.waitForIdle()
            }
        }
        compose.waitUntil(5_000) { core.client.queue.value.windowStart == 0u }
        assertEquals(ids.first(), core.client.queue.value.entries.first().track.id)
        compose.onNodeWithTag("queue.list").performScrollToNode(hasText("History"))
        compose.onNodeWithText("History").assertIsDisplayed()
        // And past the end: rows after the window come in at the bottom.
        repeat(80) {
            if (core.client.queue.value.windowEnd < 400) {
                compose.onNodeWithTag("queue.list").performTouchInput { swipeUp() }
                compose.waitForIdle()
            }
        }
        compose.waitUntil(5_000) { core.client.queue.value.windowEnd == 400 }
    }

    /** Dropping above the "Next from" heading puts a song in the group; below takes it out. */
    @Test
    fun dragMembershipFollowsTheNextFromHeading() {
        val core = TestCore()
        compose.setThemedContent(core) { QueuePanel() }
        core.start()
        compose.waitUntil(5_000) { core.client.started.value }
        compose.waitForIdle()
        val q0 = core.client.queue.value
        val entry = q0.restEntries[1]
        val rowHeight = compose.onNodeWithTag("queue.row.${q0.groupEntries.single().item.key}").fetchSemanticsNode().size.height
        // Up past the first rest row and the heading: into the group, at its end.
        compose.onNodeWithTag("queue.handle.${entry.item.key}", useUnmergedTree = true).performTouchInput {
            down(center)
            repeat(28) { moveBy(Offset(0f, -rowHeight / 10f), delayMillis = 16) }
            up()
        }
        compose.waitUntil(5_000) { core.client.queue.value.groupEntries.any { it.item.key == entry.item.key } }
        assertEquals(2u, core.client.queue.value.groupCount)
        val move = core.fake.lastQueueMove!!
        assertTrue(move.in_group)
        // And back down below the heading: out of the group again.
        compose.waitForIdle()
        compose.onNodeWithTag("queue.handle.${entry.item.key}", useUnmergedTree = true).performTouchInput {
            down(center)
            repeat(28) { moveBy(Offset(0f, rowHeight / 10f), delayMillis = 16) }
            up()
        }
        compose.waitUntil(5_000) { core.client.queue.value.restEntries.any { it.item.key == entry.item.key } }
        assertEquals(1u, core.client.queue.value.groupCount)
        assertEquals(false, core.fake.lastQueueMove!!.in_group)
    }

    @Test
    fun historyAndNowPlayingTakeNoDrop() {
        val core = TestCore()
        compose.setThemedContent(core) { QueuePanel() }
        core.start()
        compose.waitUntil(5_000) { core.client.started.value }
        compose.waitForIdle()
        val q0 = core.client.queue.value
        val head = q0.groupEntries.single()
        val rowHeight = compose.onNodeWithTag("queue.row.${head.item.key}").fetchSemanticsNode().size.height
        // Far up, past Now playing and into history: it stays at the top of what follows the current song.
        compose.onNodeWithTag("queue.handle.${head.item.key}", useUnmergedTree = true).performTouchInput {
            down(center)
            repeat(50) { moveBy(Offset(0f, -rowHeight / 10f), delayMillis = 16) }
            up()
        }
        compose.waitForIdle()
        val q = core.client.queue.value
        assertEquals(q0.current!!.item.key, q.current!!.item.key)
        assertEquals(q0.historyEntries.map { it.item.key }, q.historyEntries.map { it.item.key })
        assertEquals(head.item.key, q.groupEntries.first().item.key)
    }

    @Test
    fun headerTogglesShuffleRepeatAndAutoplay() {
        val core = TestCore()
        compose.setThemedContent(core) { QueuePanel() }
        core.start()
        compose.waitUntil(5_000) { core.client.started.value }
        val q0 = core.client.queue.value
        compose.onNodeWithTag("queue.shuffle").assertIsDisplayed().assertContentDescriptionEquals("Shuffle").performClick()
        compose.waitUntil(5_000) { core.client.queue.value.shuffle != q0.shuffle }
        compose.onNodeWithTag("queue.shuffle").assertIsToggleable().apply { if (core.client.queue.value.shuffle) assertIsOn() else assertIsOff() }
        // Repeat cycles off -> all -> one -> off, and says which.
        assertEquals(RepeatMode.Off, q0.repeat)
        compose.onNodeWithTag("queue.repeat").assertIsOff().performClick()
        compose.waitUntil(5_000) { core.client.queue.value.repeat == RepeatMode.All }
        compose.onNodeWithTag("queue.repeat").assertIsOn()
        assertEquals("Repeat all", compose.onNodeWithTag("queue.repeat").fetchSemanticsNode().config[SemanticsProperties.StateDescription])
        compose.onNodeWithTag("queue.repeat").performClick()
        compose.waitUntil(5_000) { core.client.queue.value.repeat == RepeatMode.One }
        assertEquals("Repeat one", compose.onNodeWithTag("queue.repeat").fetchSemanticsNode().config[SemanticsProperties.StateDescription])
        compose.onNodeWithTag("queue.repeat").performClick()
        compose.waitUntil(5_000) { core.client.queue.value.repeat == RepeatMode.Off }
        compose.onNodeWithTag("queue.infinite").assertContentDescriptionEquals("Autoplay").performClick()
        compose.waitUntil(5_000) { core.client.queue.value.autoplay != q0.autoplay }
        // Three pills of equal width, each a comfortable target.
        val widths = listOf("queue.shuffle", "queue.repeat", "queue.infinite").map { compose.onNodeWithTag(it).fetchSemanticsNode().size }
        assertEquals(1, widths.map { it.width }.distinct().size)
        widths.forEach { assertTrue(it.toString(), it.height >= with(compose.density) { 40.dp.roundToPx() }) }
    }

    @Test
    fun queuesScreenShowsThePlayingQueueThenPinnedAndRecent() {
        val core = TestCore()
        compose.setThemedContent(core) { RecentQueuesList() }
        core.start()
        compose.waitUntil(5_000) { core.client.started.value }
        compose.onNodeWithTag("queues.playing").assertIsDisplayed()
        compose.onNodeWithText("Pinned").assertIsDisplayed()
        compose.onNodeWithText("Recent").assertIsDisplayed()
        // The undo history is out for now (issue #127).
        compose.onNodeWithContentDescription("Undo history").assertDoesNotExist()
    }

    /**
     * The core re-keys a context item that moves (`ctx-<index>` follows the context list), so a
     * drag that sent a move per slot lost its item after the first one. The drag now reorders a
     * local copy and sends ONE move when the finger lifts.
     */
    @Test
    fun draggingTheHandleReordersTheQueueWithOneMove() {
        val core = TestCore()
        compose.setThemedContent(core) { QueuePanel() }
        core.start()
        compose.waitUntil(5_000) { core.client.started.value }
        compose.waitForIdle()
        val q0 = core.client.queue.value
        fun tracks() = core.client.queue.value.let { q -> (q.groupEntries + q.restEntries).map { it.track.id } }
        val before = tracks()
        val entry = q0.restEntries.first()
        val from = before.indexOf(entry.track.id)
        assertTrue(q0.restEntries.size >= 4)
        val undoBefore = core.client.undo.value.history.size
        compose.onNodeWithTag("queue.list").performScrollToNode(hasTestTag("queue.row.${entry.item.key}"))
        compose.waitForIdle()
        val rowHeight = compose.onNodeWithTag("queue.row.${entry.item.key}").fetchSemanticsNode().size.height
        compose.onNodeWithTag("queue.handle.${entry.item.key}", useUnmergedTree = true).performTouchInput {
            down(center)
            // Past the touch slop, then about two and a half rows down in small steps, as a finger would.
            repeat(25) { moveBy(Offset(0f, rowHeight / 10f), delayMillis = 16) }
            up()
        }
        compose.waitForIdle()
        compose.waitUntil(5_000) { tracks().indexOf(entry.track.id) >= from + 2 }
        assertEquals("only the dragged item moved", before - entry.track.id, tracks() - entry.track.id)
        assertEquals("one drag is one move (one undo entry)", undoBefore + 1, core.client.undo.value.history.size)
        // The moved row is on screen under its new key, and the rows still follow the queue.
        compose.waitUntil(5_000) { core.client.queue.value.restEntries.any { it.track.id == entry.track.id } }
        val key = core.client.queue.value.restEntries.first { it.track.id == entry.track.id }.item.key
        compose.onNodeWithTag("queue.row.$key").assertIsDisplayed()
    }

    @Test
    fun scrollingTheListStillWorksWithHandles() {
        val core = TestCore()
        compose.setThemedContent(core) { QueuePanel() }
        core.start()
        compose.waitUntil(5_000) { core.client.started.value }
        // A queue longer than the screen.
        val ids = core.fake.library.tracks.take(30).map { it.id }
        core.client.dispatch(Commands.addToQueue(core.fake.library.serverId, ids))
        compose.waitUntil(5_000) { core.client.queue.value.groupEntries.size >= 30 }
        compose.waitForIdle()
        val before = core.client.queue.value.let { q -> (q.groupEntries + q.restEntries).map { it.item.key } }
        compose.onNodeWithText("Now playing").assertIsDisplayed()
        // A vertical swipe over the rows (not on a handle) scrolls and never reorders or swipes a row.
        compose.onNodeWithTag("queue.list").performTouchInput { swipeUp(startY = bottom - 10f, endY = top + 10f) }
        compose.waitForIdle()
        compose.onNodeWithText("Now playing").assertIsNotDisplayed()
        assertEquals(before, core.client.queue.value.let { q -> (q.groupEntries + q.restEntries).map { it.item.key } })
    }

    private fun hasTestTagPrefix(prefix: String) = SemanticsMatcher("tag starts with $prefix") { it.config.getOrNull(SemanticsProperties.TestTag)?.startsWith(prefix) == true }

    private fun androidx.compose.ui.test.SemanticsNodeInteractionCollection.onFirst() = this[0]
}
