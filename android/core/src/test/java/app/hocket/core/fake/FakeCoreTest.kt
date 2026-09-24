package app.hocket.core.fake

import app.hocket.core.Commands
import app.hocket.core.Queries
import app.hocket.core.api.*
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.async
import kotlinx.coroutines.launch
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.flow.filterIsInstance
import kotlinx.coroutines.test.advanceUntilIdle
import kotlinx.coroutines.test.runTest
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

@OptIn(ExperimentalCoroutinesApi::class)
class FakeCoreTest {
    private var clock = 1_000_000.0

    private fun core(startWithServer: Boolean = true, startPlaying: Boolean = true) =
        FakeCore(seed = 1, startWithServer = startWithServer, startPlaying = startPlaying, timers = false, now = { clock }, dispatcher = Dispatchers.Unconfined)

    private suspend fun FakeCore.queue(): QueueView = (query(Query.Queue) as QueryResult.Queue).data
    private suspend fun FakeCore.snapshot(): Snapshot = (query(Query.Snapshot) as QueryResult.SnapshotResult).data

    @Test
    fun startEmitsSnapshotWithLibraryAndQueue() = runTest {
        val core = core()
        val started = async { core.events.filterIsInstance<Event.Started>().first() }
        core.dispatchAndWait(Command.Start)
        val snap = started.await().data.snapshot
        assertEquals(1, snap.servers.size)
        assertTrue(snap.servers[0].capabilities.meetsFloor)
        assertNotNull(snap.queue.current)
        assertTrue(snap.queue.upcoming.isNotEmpty())
        assertEquals(1, snap.queue.playingNext.size)
        assertNotNull(snap.resumeOffer)
        assertTrue(snap.transport.position.isPlaying)
    }

    @Test
    fun startedIsEmittedOncePerCoreAndRequestSnapshotEmitsSnapshot() = runTest {
        val core = core()
        val seen = ArrayList<Event>()
        val job = launch(start = kotlinx.coroutines.CoroutineStart.UNDISPATCHED) { core.events.collect { seen += it } }
        core.dispatchAndWait(Command.Start)
        core.dispatchAndWait(Command.RequestSnapshot)
        core.dispatchAndWait(Command.Start)
        core.dispatchAndWait(Command.RequestSnapshot)
        advanceUntilIdle()
        assertEquals("Started exactly once", 1, seen.count { it is Event.Started })
        assertEquals("every later re-emit is a Snapshot", 3, seen.count { it is Event.Snapshot })
        assertEquals(seen.indexOfFirst { it is Event.Started }, seen.indexOfFirst { it is Event.Started || it is Event.Snapshot })
        job.cancel()
    }

    @Test
    fun setupFlowAddsServerAndSyncs() = runTest {
        val core = core(startWithServer = false)
        assertEquals(0, core.snapshot().servers.size)
        core.dispatchAndWait(Commands.addServer("https://music.example", "ada", "pw"))
        val snap = core.snapshot()
        assertEquals(1, snap.servers.size)
        assertTrue(snap.syncProgress!!.finished)
        assertTrue(snap.jobs.any { it.kind == JobKind.LibrarySync && it.state == JobState.Done })
    }

    @Test
    fun wrongPasswordIsAnAuthError() = runTest {
        val core = core(startWithServer = false)
        val err = async { core.events.filterIsInstance<Event.Error>().first() }
        core.dispatchAndWait(Commands.addServer("https://music.example", "ada", "wrong"))
        assertEquals(ErrorKind.Auth, err.await().data.kind)
        assertEquals(0, core.snapshot().servers.size)
    }

    @Test
    fun oldServerFailsTheFloor() = runTest {
        val core = core(startWithServer = false)
        core.dispatchAndWait(Commands.addServer("https://old.example", "ada", "pw"))
        assertFalse(core.snapshot().servers[0].capabilities.meetsFloor)
    }

    @Test
    fun nextAndPreviousAreInverses() = runTest {
        val core = core()
        val before = core.queue()
        core.dispatchAndWait(Command.Next)
        val after = core.queue()
        assertEquals(before.playingNext[0].track.id, after.current!!.track.id) // Apple mode: insertion first
        assertEquals(before.current!!.item.key, after.history.last().item.key)
        core.dispatchAndWait(Command.Previous)
        val back = core.queue()
        assertEquals(before.current!!.item.key, back.current!!.item.key)
        assertEquals(before.playingNext.map { it.track.id }, back.playingNext.map { it.track.id })
    }

    @Test
    fun playContextSavesOutgoingQueueAndIsUndoable() = runTest {
        val core = core()
        clock += 30_000 // played into the current track so the outgoing queue isn't trivial
        val album = core.library.albums[9]
        core.dispatchAndWait(Commands.playContext(Commands.albumContext(core.library.serverId, album.id, album.name), startIndex = 1))
        val q = core.queue()
        assertEquals(album.name, q.contextLabel)
        assertEquals(core.library.albumTracks(album.id)[1].id, q.current!!.track.id)
        val saved = (core.query(Query.SavedQueues) as QueryResult.SavedQueues).data
        assertTrue(saved.any { it.label == core.library.albums[5].name })
        val undo = (core.query(Query.UndoState) as QueryResult.Undo).data
        assertTrue(undo.canUndo)
        assertEquals("Play ${album.name}", undo.undoLabel)
        core.dispatchAndWait(Command.Undo)
        assertEquals(core.library.albums[5].name, core.queue().contextLabel)
    }

    @Test
    fun removeQueueItemsAndMove() = runTest {
        val core = core()
        val q = core.queue()
        val victim = q.upcoming[0]
        core.dispatchAndWait(Commands.removeQueueItems(listOf(victim.item.key)))
        val after = core.queue()
        assertFalse(after.upcoming.any { it.track.id == victim.track.id && it.item.source is QueueSource.Context })
        val mover = after.upcoming[2]
        core.dispatchAndWait(Commands.moveQueueItem(mover.item.key, 0))
        val moved = core.queue()
        assertEquals(mover.track.id, moved.playingNext[0].track.id)
    }

    @Test
    fun shuffleKeepsCurrentAndUndoRestoresOrder() = runTest {
        val core = core()
        val before = core.queue()
        core.dispatchAndWait(Commands.setShuffle(true))
        val shuffled = core.queue()
        assertTrue(shuffled.shuffle)
        assertEquals(before.current!!.track.id, shuffled.current!!.track.id)
        core.dispatchAndWait(Commands.setShuffle(false))
        assertEquals(before.upcoming.map { it.track.id }, core.queue().upcoming.map { it.track.id })
    }

    @Test
    fun seekAndPauseProduceStamps() = runTest {
        val core = core()
        core.dispatchAndWait(Commands.seekTo(60_000))
        var t = core.snapshot().transport
        assertEquals(60_000u, t.position.positionMs)
        assertTrue(t.position.isPlaying)
        clock += 5_000
        core.dispatchAndWait(Command.Pause)
        t = core.snapshot().transport
        assertEquals(65_000u, t.position.positionMs)
        assertFalse(t.position.isPlaying)
    }

    @Test
    fun ratingIsUndoableAndChangesLibrary() = runTest {
        val core = core()
        val track = core.library.tracks[10]
        core.dispatchAndWait(Commands.rateTrack(track.id, 4))
        assertEquals(4u, core.library.track(track.id)!!.rating)
        core.dispatchAndWait(Command.Undo)
        assertEquals(track.rating, core.library.track(track.id)!!.rating)
    }

    @Test
    fun handoffPickerAndResume() = runTest {
        val core = core()
        core.dispatchAndWait(Command.OpenHandoffPicker)
        core.dispatchAndWait(Commands.handoffTo("laptop"))
        val snap = core.snapshot()
        assertFalse(snap.mediaSession.ownsTransport)
        assertTrue(snap.devices.first { it.id == "laptop" }.playing)
        core.dispatchAndWait(Command.ResumeHere)
        val after = core.snapshot()
        assertTrue(after.mediaSession.ownsTransport)
        assertNull(after.resumeOffer)
    }

    @Test
    fun filterPreviewReportsLocalOnlyFields() = runTest {
        val core = core()
        val filter = Filter("x", "x", FilterNode.All(listOf(
            FilterNode.Rule(FilterRule(FilterField.Downloaded, FilterOp.IsTrue, FilterValue.Bool(true))),
            FilterNode.Rule(FilterRule(FilterField.Genre, FilterOp.Is, FilterValue.Text("Jazz"))),
        )), SortOrder.Title, false, null)
        val p = (core.query(Queries.filterPreview(filter)) as QueryResult.Preview).data
        assertFalse(p.capability.serverExpressible)
        assertEquals(listOf(FilterField.Downloaded), p.capability.localOnlyFields)
        assertTrue(p.count > 0u)
    }

    @Test
    fun searchIsLocalFirst() = runTest {
        val core = core()
        val needle = core.library.tracks[0].title.take(4)
        val r = (core.query(Queries.search(core.library.serverId, needle, 10, includeServer = false, requestId = "r")) as QueryResult.Search).data
        assertFalse(r.fromServer)
        assertTrue(r.tracks.isNotEmpty())
    }

    @Test
    fun pagingQueriesRespectOffsets() = runTest {
        val core = core()
        val p1 = (core.query(Queries.tracks(core.library.serverId, Page(0u, 50u), SortOrder.Title)) as QueryResult.Tracks).data
        val p2 = (core.query(Queries.tracks(core.library.serverId, Page(50u, 50u), SortOrder.Title)) as QueryResult.Tracks).data
        assertEquals(50, p1.items.size)
        assertEquals(50u, p2.offset)
        assertEquals(p1.total, p2.total)
        assertTrue(p1.items.map { it.id }.intersect(p2.items.map { it.id }.toSet()).isEmpty())
    }

    @Test
    fun actionsAreOrderedPerCustomisation() = runTest {
        val core = core()
        core.dispatchAndWait(Commands.setActionOrder("contextMenu", listOf("love", "play")))
        val actions = (core.query(Queries.actions("contextMenu", Commands.tracks(listOf("t1")))) as QueryResult.Actions).data
        assertEquals("love", actions[0].id)
        assertEquals("play", actions[1].id)
    }

    private fun <T> kotlinx.coroutines.test.TestScope.async(block: suspend () -> T): kotlinx.coroutines.Deferred<T> =
        this.async(start = kotlinx.coroutines.CoroutineStart.UNDISPATCHED) { block() }
}
