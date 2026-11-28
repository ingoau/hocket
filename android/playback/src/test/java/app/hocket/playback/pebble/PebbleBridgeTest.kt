package app.hocket.playback.pebble

import app.hocket.core.api.TrackRef
import app.hocket.core.SettingKeys
import app.hocket.core.api.Album
import app.hocket.core.api.CommandAddToQueueInner
import app.hocket.core.api.CommandPlayLaterInner
import app.hocket.core.api.CommandPlayNextInner
import app.hocket.core.api.EventNetworkChangedInner
import app.hocket.core.api.EventQueueChangedInner
import app.hocket.core.api.NetworkKind
import app.hocket.core.api.NetworkState
import app.hocket.core.api.OfflineState
import app.hocket.core.api.QueueEntry
import app.hocket.core.api.QueueItem
import app.hocket.core.api.QueueSource
import app.hocket.core.api.QueueView
import app.hocket.core.api.Track
import app.hocket.core.api.TrackPage
import app.hocket.core.api.TrackSummary

import android.os.SystemClock
import android.app.Application
import androidx.test.core.app.ApplicationProvider
import androidx.test.ext.junit.runners.AndroidJUnit4
import app.hocket.core.Commands
import app.hocket.core.CoreHandle
import app.hocket.core.CoreKind
import app.hocket.core.DevLog
import app.hocket.core.api.Command
import app.hocket.core.api.CommandStashAddInner
import app.hocket.core.api.Event
import app.hocket.core.api.Query
import app.hocket.core.api.QueryResult
import app.hocket.core.api.EventMediaSessionInner
import app.hocket.core.api.EventSettingChangedInner
import app.hocket.core.api.EventStartedInner
import app.hocket.core.api.MediaSessionAction
import app.hocket.core.api.MediaSessionMetadata
import app.hocket.core.api.MediaSessionState
import app.hocket.core.api.PositionStamp
import app.hocket.core.api.RepeatMode
import app.hocket.core.api.Setting
import app.hocket.core.api.SettingScope
import app.hocket.core.api.Snapshot
import app.hocket.core.fake.FakeCore
import app.hocket.playback.pebble.PebbleBridge.Delivery
import app.hocket.playback.pebble.PebbleProtocol.Key
import app.hocket.playback.pebble.PebbleProtocol.Type
import io.rebble.pebblekit2.common.model.PebbleDictionary
import io.rebble.pebblekit2.common.model.PebbleDictionaryItem
import java.time.Duration
import kotlinx.coroutines.CoroutineStart
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.async
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.filterIsInstance
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.test.TestScope
import kotlinx.coroutines.test.advanceTimeBy
import kotlinx.coroutines.test.runCurrent
import kotlinx.coroutines.test.runTest
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.annotation.Config
import org.robolectric.shadows.ShadowSystemClock

/** What the bridge sends the watch, against a fake link: retries, the Hello path and message counts. */
@OptIn(ExperimentalCoroutinesApi::class)
@RunWith(AndroidJUnit4::class)
@Config(sdk = [34], application = Application::class)
class PebbleBridgeTest {
    private val sent = mutableListOf<PebbleDictionary>()
    /** Answers for the next transmissions, in order; Sent once they run out. */
    private val answers = ArrayDeque<Delivery>()

    private fun TestScope.bridge() = PebbleBridge(
        ApplicationProvider.getApplicationContext(),
        core = { error("no core") as CoreHandle },
        serverId = { null },
        scope = backgroundScope,
        transmit = { sent += it; answers.removeFirstOrNull() ?: Delivery.Sent },
    )

    private fun types() = sent.map { (it[Key.TYPE] as PebbleDictionaryItem.UInt8).value.toInt() }
    private fun count(type: Int) = types().count { it == type }

    private fun state(atMs: Long, playing: Boolean = true, title: String = "Song", positionMs: Long = atMs) = MediaSessionState(
        MediaSessionMetadata(title, "Artist", "Album", 600_000u, null, "t1", null, false, 0u),
        playing,
        PositionStamp(positionMs.toUInt(), atMs.toDouble(), 1.0, playing),
        false,
        RepeatMode.Off,
        1.0,
        false,
        listOf(MediaSessionAction.Play, MediaSessionAction.Pause, MediaSessionAction.Seek),
        true,
    )

    private fun session(s: MediaSessionState) = Event.MediaSession(EventMediaSessionInner(s))

    private fun hello() = mapOf(Key.TYPE to PebbleDictionaryItem.UInt8(Type.HELLO.toUByte()), Key.PROTO to PebbleDictionaryItem.UInt32(PebbleProtocol.PROTO.toUInt()))

    private fun snapshot(): Snapshot = runBlocking {
        val fake = FakeCore(startWithServer = false, startPlaying = false, timers = false, dispatcher = Dispatchers.Unconfined)
        val first = async(start = CoroutineStart.UNDISPATCHED) { fake.events.filterIsInstance<Event.Started>().first() }
        fake.dispatch(Command.Start)
        first.await().data.snapshot.also { fake.close() }
    }

    /** One second of playback: the session ticks, and both clocks move on. */
    private fun TestScope.tick(bridge: PebbleBridge, second: Long) {
        ShadowSystemClock.advanceBy(Duration.ofSeconds(1))
        bridge.onEvent(session(state(second * 1000)))
        advanceTimeBy(1000)
        runCurrent()
    }

    @Test
    fun helloIsAckedBeforeTheRepliesGoOut() = runTest {
        val b = bridge()
        b.onEvent(Event.Started(EventStartedInner(snapshot())))
        b.onEvent(session(state(0)))
        b.onMessage(hello())
        // onMessage returned (the watch's Hello is acked) with nothing sent yet.
        assertTrue(sent.isEmpty())
        runCurrent()
        assertEquals(listOf(Type.HELLO_REPLY, Type.SETTINGS, Type.NOW), types())
    }

    @Test
    fun aNackedMessageIsSentAgain() = runTest {
        val b = bridge()
        b.onEvent(session(state(0)))
        answers += Delivery.Failed
        b.onMessage(hello())
        advanceTimeBy(5_000)
        runCurrent()
        // HelloReply nacked once, then taken; Now follows.
        assertEquals(listOf(Type.HELLO_REPLY, Type.HELLO_REPLY, Type.NOW), types())
    }

    @Test
    fun theLinkIsLoggedForDeveloperDiagnostics() = runTest {
        // DevLog's ring is process-wide: only what this test logs counts.
        val marker = "marker-${java.util.UUID.randomUUID()}"
        DevLog.append("info", "kotlin/Test", marker)
        val b = bridge()
        b.onEvent(session(state(0)))
        repeat(PebbleBridge.SEND_RETRIES + 1) { answers += Delivery.Failed }
        b.onMessage(hello())
        advanceTimeBy(5_000)
        runCurrent()
        b.appClosed()
        // What the diagnostics page shows as the core log.
        val log = DevLog.recent().substringAfter(marker).lines().filter { "kotlin/Pebble:" in it }
        assertTrue(log.toString(), log.any { "<- hello: watchapp" in it && "proto ${PebbleProtocol.PROTO}" in it })
        assertTrue(log.toString(), log.any { "-> hello-reply not delivered after ${PebbleBridge.SEND_RETRIES + 1} attempts" in it })
        assertTrue(log.toString(), log.any { "DEBUG" in it && "-> now" in it })
        assertTrue(log.toString(), log.any { "watchapp closed" in it })
        b.onMessage(mapOf(Key.TYPE to PebbleDictionaryItem.UInt8(Type.LIST_REQ.toUByte()), Key.PATH to PebbleDictionaryItem.Text("search:secret words")))
        runCurrent()
        assertTrue(DevLog.recent().substringAfter(marker).contains("search:…"))
        assertTrue(!DevLog.recent().contains("secret words"))
    }

    @Test
    fun aNowThatNeverArrivesIsPushedAgainOnTheNextTick() = runTest {
        val b = bridge()
        b.onEvent(session(state(0)))
        b.onMessage(hello())
        runCurrent()
        sent.clear()
        // A pause that the watch never takes (three nacks), then a position tick.
        answers += listOf(Delivery.Failed, Delivery.Failed, Delivery.Failed)
        b.onEvent(session(state(1000, playing = false)))
        advanceTimeBy(5_000)
        runCurrent()
        assertEquals(3, count(Type.NOW))
        b.onEvent(session(state(6000, playing = false, positionMs = 1000)))
        advanceTimeBy(1_000)
        runCurrent()
        assertEquals(4, count(Type.NOW))
        // Delivered now: the next tick is quiet.
        b.onEvent(session(state(7000, playing = false, positionMs = 1000)))
        advanceTimeBy(1_000)
        runCurrent()
        assertEquals(4, count(Type.NOW))
    }

    @Test
    fun aClosedWatchappStopsThePushes() = runTest {
        val b = bridge()
        b.onEvent(session(state(0)))
        b.onMessage(hello())
        runCurrent()
        sent.clear()
        answers += Delivery.AppGone
        b.onEvent(session(state(1000, title = "Next")))
        advanceTimeBy(1_000)
        b.onEvent(session(state(2000, title = "After")))
        advanceTimeBy(1_000)
        runCurrent()
        assertEquals(1, sent.size)
    }

    @Test
    fun unchangedSettingsAreNotSentAgain() = runTest {
        val b = bridge()
        val snap = snapshot()
        b.onEvent(Event.Started(EventStartedInner(snap)))
        b.onEvent(session(state(0)))
        b.onMessage(hello())
        runCurrent()
        sent.clear()
        // The same values again (a snapshot, a setting written with its current value): nothing.
        b.onEvent(Event.Snapshot(app.hocket.core.api.EventSnapshotInner(snap)))
        b.onEvent(Event.SettingChanged(EventSettingChangedInner(Setting("pebble.showProgressBar", "true", SettingScope.DeviceLocal, 0.0))))
        runCurrent()
        assertEquals(0, count(Type.SETTINGS))
        b.onEvent(Event.SettingChanged(EventSettingChangedInner(Setting("pebble.showProgressBar", "false", SettingScope.DeviceLocal, 0.0))))
        runCurrent()
        assertEquals(1, count(Type.SETTINGS))
    }

    @Test
    fun tenMinutesOfPlaybackSendTenNowMessages() = runTest {
        val b = bridge()
        b.onEvent(session(state(0)))
        b.onMessage(hello())
        runCurrent()
        sent.clear()
        for (second in 1L..600L) tick(b, second)
        // Ticks are position-only; only the 60 s resync goes out (it was every 15 s: 40).
        println("Now messages in 10 min of playback: ${count(Type.NOW)} (elapsed ${SystemClock.elapsedRealtime()})")
        assertEquals(PebbleBridge.NOW_RESYNC_MS, 60_000L)
        assertEquals(10, count(Type.NOW))
    }

    @Test
    fun stashAddsThePlayingTrackWithoutToggling() = runTest {
        val dispatched = mutableListOf<Command>()
        val b = PebbleBridge(
            ApplicationProvider.getApplicationContext(),
            core = {
                object : CoreHandle {
                    override val kind = CoreKind.Fake
                    override fun dispatch(command: Command) { dispatched += command }
                    override suspend fun query(query: Query): QueryResult = error("unused")
                    override val events = MutableSharedFlow<Event>()
                    override fun close() = Unit
                }
            },
            serverId = { null },
            scope = backgroundScope,
            transmit = { sent += it; Delivery.Sent },
        )
        b.onEvent(session(state(0)))
        b.onMessage(mapOf(
            Key.TYPE to PebbleDictionaryItem.UInt8(Type.CMD.toUByte()),
            Key.CMD to PebbleDictionaryItem.UInt8(PebbleProtocol.Cmd.STASH.toUByte()),
        ))
        runCurrent()
        assertEquals(listOf<Command>(Command.StashAdd(CommandStashAddInner(listOf(TrackRef("", "t1"))))), dispatched)
    }

    // -- queue, accounts and offline ------------------------------------------------------------

    /** A core that answers queries from [answer] and records what is dispatched. */
    private class ScriptedCore(val answer: (Query) -> QueryResult?) : CoreHandle {
        val dispatched = mutableListOf<Command>()
        override val kind = CoreKind.Fake
        override fun dispatch(command: Command) { dispatched += command }
        override suspend fun query(query: Query): QueryResult = answer(query) ?: error("unexpected $query")
        override val events = MutableSharedFlow<Event>()
        override fun close() = Unit
    }

    private fun TestScope.bridge(core: ScriptedCore) = PebbleBridge(
        ApplicationProvider.getApplicationContext(),
        core = { core },
        serverId = { "acct-1" },
        scope = backgroundScope,
        transmit = { sent += it; Delivery.Sent },
    )

    private fun summary(id: String, source: String = "src-1", offline: OfflineState = OfflineState.None) =
        TrackSummary(id = id, serverId = source, title = "Song $id", artist = "Artist", durationMs = 1000u, rating = 0u, loved = false, offline = offline)

    private fun entry(i: Int, group: Boolean = false) =
        QueueEntry(QueueItem("k$i", TrackRef("src-1", "t$i"), QueueSource.Inserted), summary("t$i"), inGroup = group)

    /** A queue of [total] entries, [history] behind the current one, [group] in the group; the core's window holds [window] rows from 0. */
    private fun queue(total: Int, history: Int, group: Int, window: Int) = QueueView(
        contextLabel = "Night Drives",
        entries = (0 until window).map { entry(it, group = it in history + 1..history + group) },
        windowStart = 0u,
        total = total.toUInt(),
        historyCount = history.toUInt(),
        currentIndex = history.toUInt(),
        groupCount = group.toUInt(),
        totalUpcoming = (total - history - 1).toUInt(),
        current = entry(history),
        shuffle = false,
        repeat = RepeatMode.Off,
        autoplay = false,
    )

    private fun setting(key: String, value: String) =
        Event.SettingChanged(EventSettingChangedInner(Setting(key, value, SettingScope.DeviceLocal, 0.0)))

    @Test
    fun holdingAQueueRowMovesItsEntryInsteadOfAddingACopy() = runTest {
        val core = ScriptedCore { null }
        val b = bridge(core)
        b.onEvent(Event.QueueChanged(EventQueueChangedInner(queue(total = 6, history = 1, group = 1, window = 6))))
        val row = PebbleProtocol.queueRowId("k4")
        assertEquals(Command.AddToQueue(CommandAddToQueueInner(emptyList(), listOf("k4"))), b.queueCommand(next = false, "queue", row))
        assertEquals(Command.PlayNext(CommandPlayNextInner(emptyList(), listOf("k4"))), b.queueCommand(next = true, "queue", row))
        // With the Queue section off there is no group: Add to queue goes to the end, as the phone's does.
        b.onEvent(setting(SettingKeys.QUEUE_SECTION, "false"))
        assertEquals(Command.PlayLater(CommandPlayLaterInner(emptyList(), listOf("k4"))), b.queueCommand(next = false, "queue", row))
    }

    @Test
    fun theWholeQueueIsListedWithSectionsNotJustTheCoresWindow() = runTest {
        val ranges = mutableListOf<Query.QueueRange>()
        val long = queue(total = 40, history = 2, group = 2, window = 10)
        val core = ScriptedCore { q ->
            if (q is Query.QueueRange) {
                ranges += q
                val start = q.data.start.toInt()
                QueryResult.Queue(long.copy(entries = (start until start + q.data.count.toInt()).map { entry(it) }, windowStart = start.toUInt()))
            } else null
        }
        val b = bridge(core)
        b.onEvent(Event.QueueChanged(EventQueueChangedInner(long)))
        val (total, first, _) = b.list("queue", 0)
        // History(2) + Now playing(1) + group(2) + rest(35), each section under a heading.
        assertEquals(2 + 1 + 2 + 35 + 4, total)
        assertEquals(listOf("History", "Song t0", "Song t1", "Now playing", "Song t2", "Next in queue", "Song t3", "Song t4", "Next from: Night Drives", "Song t5", "Song t6", "Song t7"), first.map { it.title })
        assertEquals(PebbleProtocol.RowFlag.HEADER, first[0].flags)
        assertEquals(true, first[4].flags and PebbleProtocol.RowFlag.CURRENT != 0)
        assertEquals(emptyList<Query.QueueRange>(), ranges)
        // Rows past the window are paged in, and act like any other.
        val (_, last, _) = b.list("queue", 36)
        assertEquals((32..39).map { "Song t$it" }, last.map { it.title })
        assertEquals(1, ranges.size)
        b.onMessage(cmd(PebbleProtocol.Cmd.ACTIVATE, "queue", last.last().id))
        runCurrent()
        assertEquals(listOf<Command>(Commands.jumpToQueueItem("k39")), core.dispatched)
    }

    @Test
    fun anAlbumPlaysFromTheAccountItIsIn() = runTest {
        val album = Album(id = "al1", serverId = "acct-2", name = "Second Account Album", songCount = 1u, durationMs = 1000u, rating = 0u, loved = false, playCount = 0u, isCompilation = false, offline = OfflineState.None)
        val core = ScriptedCore { q -> if (q is Query.Album) QueryResult.AlbumDetail(album) else null }
        val b = bridge(core)
        b.onMessage(cmd(PebbleProtocol.Cmd.ACTIVATE, "album:al1", "play"))
        runCurrent()
        assertEquals(listOf<Command>(Commands.playContext(Commands.albumContext("acct-2", "al1", "Second Account Album"))), core.dispatched)
    }

    @Test
    fun aTrackRowKeepsItsAccountAndIsMarkedWhenOfflineCantPlayIt() = runTest {
        val tracks = listOf(track("a", "src-2", OfflineState.Downloaded), track("b", "src-2", OfflineState.None))
        val core = ScriptedCore { q -> if (q is Query.Tracks) QueryResult.Tracks(TrackPage(tracks, 0u, 2u)) else null }
        val b = bridge(core)
        var (_, rows, _) = b.list("offline", 0)
        assertEquals(listOf(0, 0), rows.map { it.flags and PebbleProtocol.RowFlag.UNAVAILABLE })
        assertEquals(Command.AddToQueue(CommandAddToQueueInner(listOf(TrackRef("src-2", "b")))), b.queueCommand(next = false, "offline", rows[1].id))
        // Offline, and this phone would play it: the song that needs the network is marked.
        b.onEvent(Event.NetworkChanged(EventNetworkChangedInner(NetworkState(NetworkKind.Offline, metered = false))))
        rows = b.list("offline", 0).second
        assertEquals(listOf(0, PebbleProtocol.RowFlag.UNAVAILABLE), rows.map { it.flags and PebbleProtocol.RowFlag.UNAVAILABLE })
    }

    @Test
    fun aQueueEntryThatFailedToPlayOnceIsNotRefused() = runTest {
        val q = queue(total = 3, history = 0, group = 0, window = 3)
        val failed = q.copy(entries = q.entries.mapIndexed { i, e -> if (i == 1) e.copy(item = e.item.copy(unavailable = true)) else e })
        val b = bridge(ScriptedCore { null })
        b.onEvent(Event.QueueChanged(EventQueueChangedInner(failed)))
        val rows = b.list("queue", 0).second.filter { it.flags and PebbleProtocol.RowFlag.HEADER == 0 }
        assertEquals(listOf(0, 0, 0), rows.map { it.flags and PebbleProtocol.RowFlag.UNAVAILABLE })
    }

    private fun track(id: String, source: String, offline: OfflineState) = Track(
        id = id, serverId = source, title = "Song $id", durationMs = 1000u, rating = 0u, loved = false, playCount = 0u, offline = offline, explicit = false,
    )

    private fun cmd(cmd: Int, path: String, item: String) = mapOf(
        Key.TYPE to PebbleDictionaryItem.UInt8(Type.CMD.toUByte()),
        Key.CMD to PebbleDictionaryItem.UInt8(cmd.toUByte()),
        Key.PATH to PebbleDictionaryItem.Text(path),
        Key.ITEM to PebbleDictionaryItem.Text(item),
    )
}

