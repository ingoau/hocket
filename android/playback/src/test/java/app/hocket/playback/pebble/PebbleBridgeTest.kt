package app.hocket.playback.pebble

import android.os.SystemClock
import android.app.Application
import androidx.test.core.app.ApplicationProvider
import androidx.test.ext.junit.runners.AndroidJUnit4
import app.hocket.core.CoreHandle
import app.hocket.core.DevLog
import app.hocket.core.api.Command
import app.hocket.core.api.Event
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
        MediaSessionMetadata(title, "Artist", "Album", 600_000u, null, "t1", false, 0u),
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
}
