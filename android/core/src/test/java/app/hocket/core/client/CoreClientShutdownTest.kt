package app.hocket.core.client

import app.hocket.core.CoreHandle
import app.hocket.core.CoreKind
import app.hocket.core.Queries
import app.hocket.core.api.Command
import app.hocket.core.api.Event
import app.hocket.core.api.Page
import app.hocket.core.api.Query
import app.hocket.core.api.QueryResult
import app.hocket.core.api.SortOrder
import app.hocket.core.fake.FakeCore
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.async
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.SharedFlow
import kotlinx.coroutines.test.runCurrent
import kotlinx.coroutines.test.runTest
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * `CoreClient.query` after the core has gone: composition coroutines (`LaunchedEffect`,
 * `produceState`) must see "no result", never an exception, whatever the core throws.
 */
@OptIn(ExperimentalCoroutinesApi::class)
class CoreClientShutdownTest {
    /** A core that answers until [shutDown], then throws like a freed UniFFI object does. */
    private class ClosingCore(private val delegate: CoreHandle) : CoreHandle {
        override val kind = CoreKind.Native
        override val events: SharedFlow<Event> = delegate.events
        @Volatile var shutDown = false
        var failure: () -> Exception = { IllegalStateException("HocketCore object has already been destroyed") }
        override fun dispatch(command: Command) { if (!shutDown) delegate.dispatch(command) }
        override suspend fun query(query: Query): QueryResult {
            if (shutDown) throw failure()
            return delegate.query(query)
        }
        override fun close() { shutDown = true }
    }

    @Test
    fun queriesAfterShutdownFailSoft() = runTest {
        val fake = FakeCore(seed = 3, timers = false, dispatcher = Dispatchers.Unconfined)
        val core = ClosingCore(fake)
        val client = CoreClient(core, backgroundScope)
        runCurrent()
        client.requestSnapshot()
        runCurrent()
        val serverId = client.server.value!!.id
        assertTrue("answers while running", client.query(Queries.albums(serverId, Page(0u, 10u), SortOrder.Default)) is QueryResult.Albums)

        core.close()
        for (failure in listOf<() -> Exception>(
            { IllegalStateException("HocketCore object has already been destroyed") }, // freed handle
            { RuntimeException("Failed: core is shut down") },                         // HocketException
            { IllegalStateException("the core is unavailable") },                      // DeadCore
        )) {
            core.failure = failure
            assertNull(client.query(Queries.albums(serverId, Page(0u, 10u), SortOrder.Default)))
            assertNull(client.artworkPath("al-1", 160))
            assertEquals(emptyList<Any>(), client.actions("contextMenu", app.hocket.core.Commands.tracks(listOf("t-1"))))
        }
        assertEquals(9, client.queryFailures.value)
        // The page caches' loaders go through the same path: the page stays absent (a later ensure()
        // retries) instead of caching an empty library, and nothing throws.
        val key = TrackListKey(serverId, SortOrder.Default, false)
        client.trackPages.ensure(key, 0)
        runCurrent()
        assertTrue(client.trackPages.state(key).value.pages.isEmpty())
        assertEquals(-1, client.trackPages.state(key).value.total)
    }

    @Test
    fun cancellationStillPropagates() = runTest {
        val gate = CompletableDeferred<QueryResult>()
        val core = object : CoreHandle {
            override val kind = CoreKind.Native
            override val events: SharedFlow<Event> = MutableSharedFlow()
            override fun dispatch(command: Command) = Unit
            override suspend fun query(query: Query): QueryResult = gate.await()
            override fun close() = Unit
        }
        val client = CoreClient(core, backgroundScope)
        val pending = async { client.query(Query.Servers) }
        runCurrent()
        pending.cancel()
        runCurrent()
        assertTrue(pending.isCancelled)
        var threw = false
        try { pending.await() } catch (e: CancellationException) { threw = true }
        assertTrue("a cancelled query is not turned into a null result", threw)
        assertEquals(0, client.queryFailures.value)
    }
}
