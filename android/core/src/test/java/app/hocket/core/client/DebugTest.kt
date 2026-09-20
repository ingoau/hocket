package app.hocket.core.client

import app.hocket.core.api.*
import app.hocket.core.fake.FakeCore
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.launchIn
import kotlinx.coroutines.flow.onEach
import kotlinx.coroutines.test.advanceUntilIdle
import kotlinx.coroutines.test.runCurrent
import kotlinx.coroutines.test.runTest
import org.junit.Test

class DebugTest {
    @Test
    fun debug() = runTest {
        val core = FakeCore(seed = 3, timers = false, now = { 1.0 }, dispatcher = Dispatchers.Unconfined)
        core.events.onEach { println("EVENT ${it::class.simpleName}") }.launchIn(backgroundScope)
        runCurrent()
        println("subscribers=${core.events.subscriptionCount.value}")
        core.dispatch(Command.RequestSnapshot)
        println("dispatched")
        advanceUntilIdle()
        val client = CoreClient(core, backgroundScope, now = { 1.0 })
        runCurrent()
        println("subscribers2=${core.events.subscriptionCount.value}")
        try { client.onEvent(Event.Started(EventStartedInner((core.query(Query.Snapshot) as QueryResult.SnapshotResult).data))) } catch (e: Throwable) { println("ONEVENT THREW $e"); e.printStackTrace() }
        println("started=${client.started.value}")
    }
}
