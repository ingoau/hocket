package app.hocket.core.fake

import app.hocket.core.groupEntries
import app.hocket.core.historyEntries
import app.hocket.core.restEntries
import app.hocket.core.ActionIds
import app.hocket.core.Commands
import app.hocket.core.QueueWindows
import app.hocket.core.SettingKeys
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
        assertTrue(snap.queue.restEntries.isNotEmpty())
        assertEquals(1, snap.queue.groupEntries.size)
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
        assertEquals(before.groupEntries[0].track.id, after.current!!.track.id) // the group plays first
        assertEquals(before.current!!.item.key, after.historyEntries.last().item.key)
        core.dispatchAndWait(Command.Previous)
        val back = core.queue()
        assertEquals(before.current!!.item.key, back.current!!.item.key)
        // Back at the head of the group: membership rides with the entry.
        assertEquals(before.groupEntries.map { it.item.key }, back.groupEntries.map { it.item.key })
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
        val victim = q.restEntries[0]
        core.dispatchAndWait(Commands.removeQueueItems(listOf(victim.item.key)))
        val after = core.queue()
        assertFalse(after.entries.any { it.item.key == victim.item.key })
        // Into the group: right after the current entry, in it.
        val mover = after.restEntries[2]
        core.dispatchAndWait(Commands.moveQueueItem(mover.item.key, after.current!!.item.key, inGroup = true))
        val moved = core.queue()
        assertEquals(mover.item.key, moved.groupEntries[0].item.key)
        assertEquals(2u, moved.groupCount)
        // And out again, after the first entry past the group.
        val anchor = moved.restEntries[0]
        core.dispatchAndWait(Commands.moveQueueItem(mover.item.key, anchor.item.key, inGroup = false))
        val out = core.queue()
        assertEquals(1u, out.groupCount)
        assertEquals(mover.item.key, out.restEntries[1].item.key)
        assertEquals(false, out.restEntries[1].inGroup)
    }

    @Test
    fun playNextAddToQueueAndPlayLaterPlaceSongs() = runTest {
        val core = core()
        val server = core.library.serverId
        val ids = core.library.tracks.map { it.id }
        val group0 = core.queue().groupEntries.map { it.item.key }
        core.dispatchAndWait(Commands.addToQueue(server, listOf(ids[10])))
        var q = core.queue()
        // Add to queue: the end of the group.
        assertEquals(group0 + q.groupEntries.last().item.key, q.groupEntries.map { it.item.key })
        assertEquals(ids[10], q.groupEntries.last().track.id)
        core.dispatchAndWait(Commands.playNext(server, listOf(ids[11])))
        q = core.queue()
        // Play next: straight after the current song, ahead of the group, in it.
        assertEquals(ids[11], q.groupEntries.first().track.id)
        assertEquals(group0.size + 2, q.groupCount.toInt())
        core.dispatchAndWait(Commands.playLater(server, listOf(ids[12])))
        q = core.queue()
        // Play later: the end of the whole list, not queued but added.
        val last = q.entries.last()
        assertEquals(ids[12], last.track.id)
        assertEquals(true, last.item.added)
        assertEquals(false, last.inGroup)
        assertEquals(group0.size + 2, q.groupCount.toInt())
    }

    @Test
    fun clearAndStashTheGroup() = runTest {
        val core = core()
        core.dispatchAndWait(Commands.addToQueue(core.library.serverId, listOf(core.library.tracks[20].id)))
        val total = core.queue().total
        core.dispatchAndWait(Command.ClearQueueGroup)
        assertEquals(0u, core.queue().groupCount)
        assertEquals(total - 2u, core.queue().total)
        core.dispatchAndWait(Command.Undo)
        assertEquals(2u, core.queue().groupCount)
        val ids = core.queue().groupEntries.map { it.track.id }
        core.dispatchAndWait(Command.StashQueueGroup)
        assertEquals(0u, core.queue().groupCount)
        val stash = (core.query(Query.Stash) as QueryResult.StashItems).data.map { it.track.id }
        assertTrue(stash.containsAll(ids))
    }

    @Test
    fun theQueueSectionSettingHidesAddToQueue() = runTest {
        val core = core()
        val target = Commands.tracks(listOf(core.library.tracks[0].id))
        suspend fun ids() = (core.query(Queries.actions("contextMenu", target)) as QueryResult.Actions).data.map { it.id }
        assertTrue(ActionIds.ADD_TO_QUEUE in ids())
        core.dispatchAndWait(Commands.setSetting(SettingKeys.QUEUE_SECTION, "false"))
        assertFalse(ActionIds.ADD_TO_QUEUE in ids())
        assertTrue(ActionIds.PLAY_NEXT in ids() && ActionIds.PLAY_LATER in ids())
        // Off, nothing is queued: the group is gone and Play next makes none.
        assertEquals(0u, core.queue().groupCount)
        core.dispatchAndWait(Commands.playNext(core.library.serverId, listOf(core.library.tracks[1].id)))
        assertEquals(0u, core.queue().groupCount)
        assertEquals(core.library.tracks[1].id, core.queue().restEntries.first().track.id)
    }

    @Test
    fun theViewIsAWindowAndQueueRangePagesTheRest() = runTest {
        val core = core()
        val many = core.library.tracks.take(400).map { it.id }
        core.dispatchAndWait(Command.PlayTracks(CommandPlayTracksInner(core.library.serverId, many, 250u, "Many", false)))
        val q = core.queue()
        assertEquals(250u, q.currentIndex)
        assertEquals(250u, q.historyCount)
        assertEquals(150u, q.windowStart)
        assertEquals(201, q.entries.size)
        val page = (core.query(Commands.queueRange(0, 150)) as QueryResult.Queue).data
        assertEquals(0u, page.windowStart)
        assertEquals(many.take(150), page.entries.map { it.track.id })
        val merged = QueueWindows.merge(q, page)
        assertEquals(0u, merged.windowStart)
        assertEquals(many.take(351), merged.entries.map { it.track.id })
        assertEquals(null, QueueWindows.missing(merged, 250, 100))
        assertEquals(351 to 49, QueueWindows.missing(merged, 250, 149))
    }

    @Test
    fun repeatAllWithShuffleTellsWhatItWrapsTo() = runTest {
        val core = core()
        core.dispatchAndWait(Commands.setShuffle(true))
        core.dispatchAndWait(Commands.setRepeat(RepeatMode.All))
        assertNull(core.queue().wrapNext)
        var guard = 0
        while (core.queue().let { it.groupEntries.isNotEmpty() || it.restEntries.isNotEmpty() } && guard++ < 500) core.dispatchAndWait(Command.Next)
        val peek = core.queue().wrapNext
        assertNotNull(peek)
        core.dispatchAndWait(Command.Next)
        assertEquals(peek!!.track.id, core.queue().current!!.track.id)
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
        assertEquals(before.restEntries.map { it.track.id }, core.queue().restEntries.map { it.track.id })
    }

    @Test
    fun playTracksStartsAtTheRequestedPosition() = runTest {
        val core = core()
        val id = core.queue().restEntries.first().track.id
        core.dispatchAndWait(Command.PlayTracks(CommandPlayTracksInner(core.library.serverId, listOf(id), 0u, "Lyric", false, 42_000u)))
        val t = core.snapshot().transport
        assertEquals(id, core.queue().current!!.track.id)
        assertEquals(42_000u, t.position.positionMs)
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

    // -- stream cache ------------------------------------------------------------------------------

    @Test
    fun primeCommandsAreRecordedInOrder() = runTest {
        val core = core()
        core.dispatchAndWait(Commands.primeAlbum("al1"))
        core.dispatchAndWait(Commands.primeTrack("t9"))
        assertEquals(listOf("album:al1", "track:t9"), core.primeRequests)
    }

    @Test
    fun theCacheBudgetIsAutomaticUntilSetAndAgainAfterAReset() = runTest {
        val core = core()
        suspend fun storage() = (core.query(Query.Storage) as QueryResult.Storage).data
        assertEquals(true, storage().cacheBudgetAuto)
        assertEquals(FakeCore.AUTO_CACHE_BUDGET, storage().cacheBudgetBytes!!, 0.0)
        assertTrue(storage().partialCacheBytes!! < storage().cacheBytes)
        core.dispatchAndWait(Commands.setSetting("storage.cacheMaxBytes", "4.0E9"))
        assertEquals(false, storage().cacheBudgetAuto)
        assertEquals(4e9, storage().cacheBudgetBytes!!, 0.0)
        core.dispatchAndWait(Commands.resetSetting("storage.cacheMaxBytes"))
        assertEquals(true, storage().cacheBudgetAuto)
        // Like the core: 2 GiB is a size of its own; null is automatic.
        core.dispatchAndWait(Commands.setSetting("storage.cacheMaxBytes", "2147483648.0"))
        assertEquals(false, storage().cacheBudgetAuto)
        core.dispatchAndWait(Commands.setSetting("storage.cacheMaxBytes", "null"))
        assertEquals(true, storage().cacheBudgetAuto)
        core.dispatchAndWait(Command.ClearStreamCache)
        assertEquals(0.0, storage().partialCacheBytes!!, 0.0)
    }

    @Test
    fun theAvailableOfflineBuiltInMatchesDownloadedAndCachedTracks() = runTest {
        val core = core()
        val filters = (core.query(Query.Filters) as QueryResult.Filters).data
        val builtin = filters.single { it.id == FakeCore.BUILTIN_AVAILABLE_OFFLINE }
        val preview = (core.query(Query.FilterPreview(QueryFilterPreviewInner(builtin))) as QueryResult.Preview).data
        val expected = core.library.tracks.count { it.offline == OfflineState.Downloaded || it.offline == OfflineState.Cached }
        assertEquals(expected, preview.count.toInt())
        assertFalse("local-only", preview.capability.serverExpressible)
    }

    @Test
    fun goingOfflineWithUnplayableTracksQueuedRaisesTheOfflineNotice() = runTest {
        val core = core()
        core.dispatchAndWait(Command.Start)
        val notice = async(start = kotlinx.coroutines.CoroutineStart.UNDISPATCHED) { core.events.filterIsInstance<Event.PlayerNotice>().first { it.data.message != null } }
        core.dispatchAndWait(Commands.setNetworkState(NetworkState(NetworkKind.Offline, false, null)))
        val data = notice.await().data
        assertTrue(data.code == PlayerNoticeCode.OfflineSkipping || data.code == PlayerNoticeCode.NothingAvailableOffline)
        assertTrue(data.message == FakeCore.OFFLINE_SKIPPING || data.message == FakeCore.OFFLINE_NOTHING)
        val cleared = async(start = kotlinx.coroutines.CoroutineStart.UNDISPATCHED) { core.events.filterIsInstance<Event.PlayerNotice>().first() }
        core.dispatchAndWait(Commands.setNetworkState(NetworkState(NetworkKind.Wifi, false, null)))
        assertNull(cleared.await().data.message)
        assertNull(cleared.await().data.code)
    }

    @Test
    fun networkChangesAreAnnouncedOnceAndReplayed() = runTest {
        val core = core()
        core.dispatchAndWait(Command.Start)
        val offline = NetworkState(NetworkKind.Offline, false, null)
        val first = async(start = kotlinx.coroutines.CoroutineStart.UNDISPATCHED) { core.events.filterIsInstance<Event.NetworkChanged>().first() }
        core.dispatchAndWait(Commands.setNetworkState(offline))
        assertEquals(offline, first.await().data.network)
        // A snapshot request replays the current network.
        val replay = async(start = kotlinx.coroutines.CoroutineStart.UNDISPATCHED) { core.events.filterIsInstance<Event.NetworkChanged>().first() }
        core.dispatchAndWait(Command.RequestSnapshot)
        assertEquals(offline, replay.await().data.network)
    }

    /** Artists honour the core's sort and plain direction; the default (no sort) is by name. */
    @Test
    fun artistsSortByTheQuerysOrderAndDirection() = runTest {
        val core = core()
        core.dispatchAndWait(Command.Start)
        val server = core.snapshot().servers.first().id
        suspend fun artists(sort: SortOrder? = null, descending: Boolean? = null): List<Artist> =
            (core.query(Query.Artists(QueryArtistsInner(server, Page(0u, 500u), sort, descending))) as QueryResult.Artists).data.items
        val byName = artists()
        assertEquals(byName.map { it.name.lowercase() }.sorted(), byName.map { it.name.lowercase() })
        assertEquals(byName.reversed().map { it.name }, artists(SortOrder.Default, true).map { it.name })
        val most = artists(SortOrder.AlbumCount, true).map { it.albumCount }
        assertEquals(most.sortedDescending(), most)
        val fewest = artists(SortOrder.SongCount, false).map { it.songCount }
        assertEquals(fewest.sorted(), fewest)
        // Plays as the core sums them: over the albums an artist is the album artist of.
        val plays = artists(SortOrder.PlayCount, true).map { a -> core.library.artistAlbums(a.id).sumOf { it.playCount.toLong() } }
        assertEquals(plays.sortedDescending(), plays)
        // Through the client's query helper too.
        val helper = (core.query(Queries.artists(server, Page(0u, 500u), SortOrder.AlbumCount, true)) as QueryResult.Artists).data.items
        assertEquals(artists(SortOrder.AlbumCount, true), helper)
    }
}
