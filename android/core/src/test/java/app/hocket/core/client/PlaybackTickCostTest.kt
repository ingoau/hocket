package app.hocket.core.client

import app.hocket.core.Commands
import app.hocket.core.CoreHandle
import app.hocket.core.CoreKind
import app.hocket.core.api.*
import app.hocket.core.fake.FakeCore
import kotlin.coroutines.CoroutineContext
import kotlinx.coroutines.CoroutineDispatcher
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.SharedFlow
import kotlinx.coroutines.launch
import kotlinx.coroutines.test.StandardTestDispatcher
import kotlinx.coroutines.test.advanceTimeBy
import kotlinx.coroutines.test.runCurrent
import kotlinx.coroutines.test.runTest
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * What a playing (or paused) player costs the UI process: main-thread hops for the core's
 * position ticks, and the position ticker's polling while nothing moves.
 */
@OptIn(ExperimentalCoroutinesApi::class)
class PlaybackTickCostTest {
    private var clock = 5_000_000.0

    /** Counts every hop onto the client's ("main") dispatcher. */
    private class CountingDispatcher(private val inner: CoroutineDispatcher) : CoroutineDispatcher() {
        var dispatches = 0
        override fun isDispatchNeeded(context: CoroutineContext) = inner.isDispatchNeeded(context)
        override fun dispatch(context: CoroutineContext, block: Runnable) {
            dispatches++
            inner.dispatch(context, block)
        }
    }

    /** A core whose events the test emits by hand. */
    private class ScriptedCore : CoreHandle {
        override val kind = CoreKind.Fake
        val flow = MutableSharedFlow<Event>(extraBufferCapacity = 1024)
        override val events: SharedFlow<Event> = flow
        override fun dispatch(command: Command) = Unit
        override suspend fun query(query: Query): QueryResult = throw UnsupportedOperationException()
        override fun close() = Unit
    }

    private fun stamp(positionMs: Long, takenAt: Double, playing: Boolean = true) = TransportState(
        TransportLease("me", 1u, 1.0), PositionStamp(positionMs.toUInt(), takenAt, 1.0, playing), false, positionMs.toUInt(), 0.5,
    )

    @Test
    fun positionTicksDoNotHopToTheMainThread() = runTest {
        val main = CountingDispatcher(StandardTestDispatcher(testScheduler))
        val core = ScriptedCore()
        val client = CoreClient(core, CoroutineScope(backgroundScope.coroutineContext + main), now = { clock })
        runCurrent()
        core.flow.emit(Event.TransportChanged(EventTransportChangedInner(stamp(0, clock))))
        runCurrent()
        val published = client.transport.value
        val before = main.dispatches
        // A minute of ExoBackend position reports (one each 750 ms).
        val ticks = 80
        for (i in 1..ticks) {
            core.flow.emit(Event.TransportChanged(EventTransportChangedInner(stamp(i * 750L, clock + i * 750.0))))
            runCurrent()
        }
        val hops = main.dispatches - before
        println("MEASURE main-thread hops for $ticks position ticks: $hops")
        assertTrue("ticks stay off the main thread ($hops hops)", hops <= 2)
        assertTrue(client.transport.value === published)
        clock += ticks * 750.0
        assertEquals("the clock still follows the ticks", ticks * 750L, client.positionNow())
        // A pause is not a tick: it reaches the main thread and the flows.
        core.flow.emit(Event.TransportChanged(EventTransportChangedInner(stamp(ticks * 750L, clock, playing = false))))
        runCurrent()
        assertFalse(client.transport.value.position.isPlaying)
    }

    @Test
    fun aPausedPositionTickerDoesNotPoll() = runTest {
        var reads = 0
        val core = FakeCore(seed = 3, timers = false, now = { clock }, dispatcher = Dispatchers.Unconfined)
        val client = CoreClient(core, backgroundScope, now = { reads++; clock })
        runCurrent()
        client.requestSnapshot()
        runCurrent()
        client.dispatch(Command.Pause)
        runCurrent()
        assertFalse(client.isPlaying.value)
        backgroundScope.launch { client.position.collect { } }
        runCurrent()
        reads = 0
        advanceTimeBy(10_000)
        runCurrent()
        println("MEASURE paused 10 s: position reads=$reads")
        assertTrue("paused: no polling ($reads reads)", reads <= 2)
        // A seek while paused still shows, a small one included (it is not republished as transport).
        client.dispatch(Commands.seekTo(42_000))
        runCurrent()
        advanceTimeBy(100)
        assertEquals(42_000L, client.position.value)
        client.dispatch(Commands.seekTo(42_500))
        runCurrent()
        advanceTimeBy(100)
        assertEquals(42_500L, client.position.value)
        // Playing ticks again at ~30 Hz.
        client.dispatch(Command.Play)
        runCurrent()
        reads = 0
        advanceTimeBy(1_000)
        runCurrent()
        println("MEASURE playing 1 s: position reads=$reads")
        assertTrue("playing: ~30 Hz ($reads reads)", reads >= 25)
    }

    @Test
    fun ownsTransportFollowsMediaSessionEventsNotOnlySnapshots() = runTest {
        val core = FakeCore(seed = 3, timers = false, now = { clock }, dispatcher = Dispatchers.Unconfined)
        val client = CoreClient(core, backgroundScope, now = { clock })
        runCurrent()
        client.requestSnapshot()
        runCurrent()
        assertTrue(client.ownsTransport.value)
        client.dispatch(Commands.handoffTo("laptop"))
        runCurrent()
        assertFalse("a handoff is seen without a new snapshot", client.ownsTransport.value)
        assertFalse(client.mediaSession.value.ownsTransport)
    }
}
