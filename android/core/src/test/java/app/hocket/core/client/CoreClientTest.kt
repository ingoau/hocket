package app.hocket.core.client

import app.hocket.core.Commands
import app.hocket.core.api.*
import app.hocket.core.fake.FakeCore
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.test.runCurrent
import kotlinx.coroutines.test.runTest
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertTrue
import org.junit.Test

@OptIn(ExperimentalCoroutinesApi::class)
class CoreClientTest {
    private var clock = 5_000_000.0

    @Test
    fun snapshotPopulatesStateFlows() = runTest {
        val core = FakeCore(seed = 3, timers = false, now = { clock }, dispatcher = Dispatchers.Unconfined)
        val client = CoreClient(core, backgroundScope, now = { clock })
        runCurrent() // background tasks only run on runCurrent(); advanceUntilIdle stops when no foreground task is pending
        client.requestSnapshot()
        runCurrent()
        assertTrue(client.started.value)
        assertNotNull(client.server.value)
        assertNotNull(client.nowPlaying.value)
        assertTrue(client.isPlaying.value)
        assertTrue(client.savedQueues.value.isNotEmpty())
        assertTrue(client.pins.value.isNotEmpty())
        assertTrue(client.finishedWithProblems.value)
        assertEquals(1, client.problems.value.size)
    }

    @Test
    fun eventsUpdateDerivedState() = runTest {
        val core = FakeCore(seed = 3, timers = false, now = { clock }, dispatcher = Dispatchers.Unconfined)
        val client = CoreClient(core, backgroundScope, now = { clock })
        runCurrent() // background tasks only run on runCurrent(); advanceUntilIdle stops when no foreground task is pending
        client.requestSnapshot()
        runCurrent()
        val before = client.nowPlaying.value!!.track.id
        client.dispatch(Command.Next)
        runCurrent()
        assertTrue(client.nowPlaying.value!!.track.id != before)
        assertTrue(client.undo.value.canUndo || client.queue.value.history.isNotEmpty())
        client.dispatch(Command.Pause)
        runCurrent()
        assertFalse(client.isPlaying.value)
    }

    @Test
    fun positionExtrapolatesFromStamp() = runTest {
        val core = FakeCore(seed = 3, timers = false, now = { clock }, dispatcher = Dispatchers.Unconfined)
        val client = CoreClient(core, backgroundScope, now = { clock })
        runCurrent() // background tasks only run on runCurrent(); advanceUntilIdle stops when no foreground task is pending
        client.requestSnapshot()
        runCurrent()
        client.dispatch(Commands.seekTo(10_000))
        runCurrent()
        assertEquals(10_000L, client.positionNow())
        clock += 2_500
        assertEquals(12_500L, client.positionNow())
    }

    @Test
    fun libraryChangedInvalidatesPageCaches() = runTest {
        val core = FakeCore(seed = 3, timers = false, now = { clock }, dispatcher = Dispatchers.Unconfined)
        val client = CoreClient(core, backgroundScope, now = { clock })
        runCurrent() // background tasks only run on runCurrent(); advanceUntilIdle stops when no foreground task is pending
        client.requestSnapshot()
        runCurrent()
        val key = TrackListKey(core.library.serverId, SortOrder.Title, false)
        client.trackPages.ensure(key, 0)
        runCurrent()
        assertTrue(client.trackPages.state(key).value.known)
        client.onEvent(Event.LibraryChanged(EventLibraryChangedInner(core.library.serverId, listOf("tracks"), emptyList())))
        assertEquals(0, client.trackPages.state(key).value.pages.size)
        assertEquals(1, client.trackPages.state(key).value.generation)
    }

    @Test
    fun selectionPublishesToCore() = runTest {
        val core = FakeCore(seed = 3, timers = false, now = { clock }, dispatcher = Dispatchers.Unconfined)
        val client = CoreClient(core, backgroundScope, now = { clock })
        runCurrent() // background tasks only run on runCurrent(); advanceUntilIdle stops when no foreground task is pending
        client.toggleSelected(SelectionKind.Tracks, "t1")
        client.toggleSelected(SelectionKind.Tracks, "t2")
        assertEquals(2, client.selection.value.count)
        client.selectAll(SelectionKind.Tracks, 1000)
        assertEquals(1000, client.selection.value.count)
        client.toggleSelected(SelectionKind.Albums, "al1") // switching kind clears the old selection
        assertEquals(SelectionKind.Albums, client.selectionKind.value)
        assertEquals(1, client.selection.value.count)
        client.clearSelection()
        assertFalse(client.selection.value.active)
    }
}
