package app.hocket.playback

import android.app.Application
import android.os.CancellationSignal
import androidx.test.ext.junit.runners.AndroidJUnit4
import java.util.concurrent.CountDownLatch
import java.util.concurrent.Executors
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicInteger
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.annotation.Config

/** [ArtworkProvider]'s waits: shared per cover, few at a time, bounded, and cancellable. */
@RunWith(AndroidJUnit4::class)
@Config(sdk = [34], application = Application::class)
class ArtworkWaitsTest {
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.IO)
    private val pool = Executors.newFixedThreadPool(8)

    @After
    fun tearDown() {
        scope.cancel()
        pool.shutdownNow()
    }

    @Test
    fun aCachedAnswerComesBackAtOnce() {
        val waits = ArtworkWaits(scope, quickMs = 1_000, timeoutMs = 2_000, maxWaiting = 0)
        // No waiting slots at all: the quick wait still answers.
        assertEquals("/c/a.jpg", waits.await("320/a", null) { "/c/a.jpg" })
    }

    @Test
    fun oneLookupServesEveryoneAskingForTheSameCover() {
        val waits = ArtworkWaits(scope, quickMs = 50, timeoutMs = 5_000, maxWaiting = 4)
        val calls = AtomicInteger()
        val answer = CompletableDeferred<String?>()
        val started = CountDownLatch(3)
        val results = (1..3).map {
            pool.submit<String?> { started.countDown(); waits.await("320/a", null) { calls.incrementAndGet(); answer.await() } }
        }
        assertTrue(started.await(2, TimeUnit.SECONDS))
        Thread.sleep(100)
        answer.complete("/c/a.jpg")
        assertEquals(listOf("/c/a.jpg", "/c/a.jpg", "/c/a.jpg"), results.map { it.get(2, TimeUnit.SECONDS) })
        assertEquals(1, calls.get())
    }

    @Test
    fun pastTheWaitingCapAnOpenFailsAtOnce() {
        val waits = ArtworkWaits(scope, quickMs = 20, timeoutMs = 10_000, maxWaiting = 1)
        val never = CompletableDeferred<String?>()
        val holding = pool.submit<String?> { waits.await("320/a", null) { never.await() } }
        Thread.sleep(200)
        val t0 = System.nanoTime()
        assertNull(waits.await("320/b", null) { never.await() })
        assertTrue("answered without waiting out the timeout", TimeUnit.NANOSECONDS.toMillis(System.nanoTime() - t0) < 2_000)
        never.complete(null)
        assertNull(holding.get(2, TimeUnit.SECONDS))
    }

    @Test
    fun aWaitIsBoundedAndTheFetchCarriesOnForTheNextAsk() {
        val waits = ArtworkWaits(scope, quickMs = 20, timeoutMs = 200, maxWaiting = 4)
        val answer = CompletableDeferred<String?>()
        val calls = AtomicInteger()
        assertNull(waits.await("320/a", null) { calls.incrementAndGet(); answer.await() })
        // Still in flight: the next ask joins it rather than starting another.
        val next = pool.submit<String?> { waits.await("320/a", null) { calls.incrementAndGet(); "/c/other.jpg" } }
        Thread.sleep(50)
        answer.complete("/c/a.jpg")
        assertEquals("/c/a.jpg", next.get(2, TimeUnit.SECONDS))
        assertEquals(1, calls.get())
    }

    @Test
    fun onlyAFewLookupsRunAtOnceAndTheRestWaitTheirTurn() {
        val waits = ArtworkWaits(scope, quickMs = 20, timeoutMs = 5_000, maxWaiting = 0, maxLookups = 2)
        val running = AtomicInteger()
        val most = AtomicInteger()
        val calls = AtomicInteger()
        val release = CompletableDeferred<String?>()
        // Five covers asked for and given up on (no waiting slots): their lookups go on, two at a time.
        (1..5).forEach { i ->
            assertNull(waits.await("320/$i", null) {
                calls.incrementAndGet()
                most.accumulateAndGet(running.incrementAndGet(), ::maxOf)
                try { release.await() } finally { running.decrementAndGet() }
            })
        }
        Thread.sleep(200)
        assertEquals(2, calls.get())
        release.complete(null)
        val deadline = System.currentTimeMillis() + 2_000
        while (calls.get() < 5 && System.currentTimeMillis() < deadline) Thread.sleep(10)
        assertEquals(5, calls.get())
        assertEquals(2, most.get())
    }

    @Test
    fun aCancelledOpenIsLetGo() {
        val waits = ArtworkWaits(scope, quickMs = 20, timeoutMs = 30_000, maxWaiting = 4)
        val signal = CancellationSignal()
        val never = CompletableDeferred<String?>()
        val open = pool.submit<String?> { waits.await("320/a", signal) { never.await() } }
        Thread.sleep(200)
        signal.cancel()
        assertNull(open.get(2, TimeUnit.SECONDS))
        // Already cancelled: nothing is looked up.
        val calls = AtomicInteger()
        assertNull(waits.await("320/b", signal) { calls.incrementAndGet(); "/c/b.jpg" })
        assertEquals(0, calls.get())
    }
}
