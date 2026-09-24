package app.hocket.core

import app.hocket.core.api.ErrorKind
import app.hocket.core.api.Event
import app.hocket.core.api.EventErrorInner
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.CoroutineStart
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.async
import kotlinx.coroutines.cancel
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.take
import kotlinx.coroutines.flow.toList
import kotlinx.coroutines.launch
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.withTimeout
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

/** The native seam's event hand-off: never blocks the core's thread, never drops, keeps order. */
class EventPumpTest {
    private fun event(i: Int) = Event.Error(EventErrorInner(ErrorKind.Network, "e$i", null))

    @Test
    fun aBurstIsDeliveredCompleteAndInOrderToASlowSubscriber() = runBlocking {
        val scope = CoroutineScope(SupervisorJob() + Dispatchers.Default)
        val pump = EventPump(scope)
        val n = 3_000
        val collected = scope.async(start = CoroutineStart.UNDISPATCHED) { pump.events.take(n).toList() }
        // A slow subscriber on top (the main thread mid-composition) must not cost anything either.
        val slow = scope.launch(start = CoroutineStart.UNDISPATCHED) { pump.events.collect { delay(1) } }
        // The producer is the core's actor thread: offering never blocks, even far past any buffer.
        val started = System.nanoTime()
        val producer = Thread { repeat(n) { assertTrue(pump.offer(event(it))) } }
        producer.start(); producer.join()
        assertTrue("offer never blocks the core", System.nanoTime() - started < 2_000_000_000L)
        val events = withTimeout(60_000) { collected.await() }
        assertEquals(n, events.size)
        assertEquals((0 until n).map { "e$it" }, events.map { (it as Event.Error).data.message })
        slow.cancel()
        pump.close()
        scope.cancel()
    }
}
