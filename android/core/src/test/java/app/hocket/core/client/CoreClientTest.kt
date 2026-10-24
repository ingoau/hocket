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
import org.junit.Assert.assertSame
import org.junit.Assert.assertTrue
import org.junit.Test

@OptIn(ExperimentalCoroutinesApi::class)
class CoreClientTest {
    private var clock = 5_000_000.0

    @Test
    fun roomDiagnosticsAreCollectedOnceAndReusedBriefly() = runTest {
        val core = FakeCore(seed = 3, timers = false, now = { clock }, dispatcher = Dispatchers.Unconfined)
        val client = CoreClient(core, backgroundScope, now = { clock })
        runCurrent()
        client.requestSnapshot()
        client.dispatch(Commands.setSetting(app.hocket.core.SettingKeys.DEVELOPER_MODE, "true"))
        runCurrent()
        val first = client.collectRoomDiagnostics()
        assertTrue(first!!.contains("Laptop"))
        val id = client.roomDiagnostics.value!!.requestId
        assertTrue(client.roomDiagnostics.value!!.done)
        // Within the room's limit: reused, nothing asked.
        clock += 5_000
        assertEquals(first, client.collectRoomDiagnostics())
        assertEquals(id, client.roomDiagnostics.value!!.requestId)
        // Forced (Refresh), or later: asked again.
        assertNotNull(client.collectRoomDiagnostics(force = true))
        val forced = client.roomDiagnostics.value!!.requestId
        assertTrue(forced != id)
        clock += REUSE_ROOM_COLLECTION_MS
        client.collectRoomDiagnostics()
        assertTrue(client.roomDiagnostics.value!!.requestId != forced)
    }

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
    fun networkAndCodedNoticesFollowTheirEvents() = runTest {
        val core = FakeCore(seed = 3, timers = false, now = { clock }, dispatcher = Dispatchers.Unconfined)
        val client = CoreClient(core, backgroundScope, now = { clock })
        runCurrent()
        client.requestSnapshot()
        runCurrent()
        // NetworkChanged keeps `network` current (the snapshot alone would go stale).
        client.onEvent(Event.NetworkChanged(EventNetworkChangedInner(NetworkState(NetworkKind.Cellular, true, null))))
        assertEquals(NetworkKind.Cellular, client.network.value?.kind)
        client.dispatch(Commands.setNetworkState(NetworkState(NetworkKind.Offline, false, null)))
        runCurrent()
        assertEquals(NetworkKind.Offline, client.network.value?.kind)
        // A notice carries its stable code; an all-null one clears it.
        client.onEvent(Event.PlayerNotice(EventPlayerNoticeInner("Couldn't play X, skipped", PlayerNoticeCode.CouldNotPlaySkipped, "X")))
        assertEquals(PlayerNoticeCode.CouldNotPlaySkipped, client.playerNotice.value?.code)
        assertEquals("X", client.playerNotice.value?.detail)
        client.onEvent(Event.PlayerNotice(EventPlayerNoticeInner()))
        assertEquals(null, client.playerNotice.value)
    }

    @Test
    fun batterySaverFollowsItsEventWithoutASnapshot() = runTest {
        val core = FakeCore(seed = 3, timers = false, now = { clock }, dispatcher = Dispatchers.Unconfined)
        val client = CoreClient(core, backgroundScope, now = { clock })
        runCurrent()
        client.requestSnapshot()
        runCurrent()
        assertFalse(client.batterySaver.value)
        client.onEvent(Event.BatterySaverChanged(EventBatterySaverChangedInner(true)))
        assertTrue(client.batterySaver.value)
        client.onEvent(Event.BatterySaverChanged(EventBatterySaverChangedInner(false)))
        assertFalse(client.batterySaver.value)
        // Through the core: each change is announced on its own.
        client.dispatch(Commands.setBatterySaver(true))
        runCurrent()
        assertTrue(client.batterySaver.value)
        client.dispatch(Commands.setBatterySaver(false))
        runCurrent()
        assertFalse(client.batterySaver.value)
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
    fun positionTicksDoNotPublishANewTransportState() = runTest {
        val core = FakeCore(seed = 3, timers = false, now = { clock }, dispatcher = Dispatchers.Unconfined)
        val client = CoreClient(core, backgroundScope, now = { clock })
        runCurrent()
        client.requestSnapshot()
        runCurrent()
        val published = client.transport.value
        val tick = published.copy(
            position = published.position.copy(
                positionMs = published.position.positionMs + 750u,
                takenAt = published.position.takenAt + 750.0,
            ),
        )
        client.onEvent(Event.TransportChanged(EventTransportChangedInner(tick)))
        assertSame(published, client.transport.value)
        clock = tick.position.takenAt
        assertEquals(tick.position.positionMs.toLong(), client.positionNow())
        val paused = tick.copy(position = tick.position.copy(isPlaying = false, rate = 0.0))
        client.onEvent(Event.TransportChanged(EventTransportChangedInner(paused)))
        assertEquals(false, client.transport.value.position.isPlaying)
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
    fun libraryChangedDoesNotWipePlaylistPagesOnATrackOnlyUpdate() = runTest {
        val core = FakeCore(seed = 3, timers = false, now = { clock }, dispatcher = Dispatchers.Unconfined)
        val client = CoreClient(core, backgroundScope, now = { clock })
        runCurrent()
        client.requestSnapshot()
        runCurrent()
        val tracks = TrackListKey(core.library.serverId, SortOrder.Title, false)
        val playlists = PlaylistTracksKey("p1")
        client.trackPages.ensure(tracks, 0)
        client.playlistTrackPages.ensure(playlists, 0)
        runCurrent()
        val playlistGen = client.playlistTrackPages.state(playlists).value.generation
        client.onEvent(Event.LibraryChanged(EventLibraryChangedInner(core.library.serverId, listOf("tracks"), emptyList())))
        assertEquals(0, client.trackPages.state(tracks).value.pages.size)
        assertEquals(playlistGen, client.playlistTrackPages.state(playlists).value.generation)
    }

    @Test
    fun lyricsCacheEvictsOldestDocuments() = runTest {
        val core = FakeCore(seed = 3, timers = false, now = { clock }, dispatcher = Dispatchers.Unconfined)
        val client = CoreClient(core, backgroundScope, now = { clock })
        runCurrent()
        val extra = 5
        repeat(CoreClient.LYRICS_CACHE_CAP + extra) { i ->
            client.onEvent(Event.LyricsChanged(EventLyricsChangedInner("t$i", null)))
        }
        assertEquals(CoreClient.LYRICS_CACHE_CAP, client.lyrics.value.size)
        assertFalse(client.lyrics.value.containsKey("t0"))
        assertTrue(client.lyrics.value.containsKey("t${CoreClient.LYRICS_CACHE_CAP + extra - 1}"))
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
