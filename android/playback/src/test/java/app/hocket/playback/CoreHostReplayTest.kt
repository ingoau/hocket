package app.hocket.playback

import app.hocket.core.CoreHandle
import app.hocket.core.CoreKind
import app.hocket.core.api.Command
import app.hocket.core.api.ErrorKind
import app.hocket.core.api.Event
import app.hocket.core.api.EventErrorInner
import app.hocket.core.api.EventServersChangedInner
import app.hocket.core.api.EventSnapshotInner
import app.hocket.core.api.EventStartedInner
import app.hocket.core.api.Query
import app.hocket.core.api.QueryResult
import app.hocket.core.api.ServerCapabilities
import app.hocket.core.api.ServerInfo
import app.hocket.core.api.Snapshot
import app.hocket.core.fake.FakeCore
import kotlinx.coroutines.CoroutineStart
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.async
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.SharedFlow
import kotlinx.coroutines.flow.filterIsInstance
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.launch
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.yield
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test

/**
 * [CoreHost.replayCredentials] against a scripted core: the event sequences the real core produces
 * (`Started` once on Start, `Snapshot` on every RequestSnapshot, and `Snapshot` then
 * `ServersChanged` on RemoveServer) must replay a stored login exactly once, and sign-out must never
 * resurrect the server. A core that repeated `Started` would not trigger a second replay either.
 */
class CoreHostReplayTest {
    private class ScriptedCore : CoreHandle {
        override val kind = CoreKind.Native
        private val flow = MutableSharedFlow<Event>()
        override val events: SharedFlow<Event> = flow
        val dispatched = mutableListOf<Command>()
        var onDispatch: (Command) -> Unit = {}
        override fun dispatch(command: Command) { dispatched += command; onDispatch(command) }
        override suspend fun query(query: Query): QueryResult = error("unused")
        override fun close() = Unit
        suspend fun emit(event: Event) { flow.emit(event); yield(); yield() }
        val addServers get() = dispatched.filterIsInstance<Command.AddServer>()
    }

    private val url = "https://music.example.net"
    private val caps = ServerCapabilities("0.63.1", true, emptyList(), true, true, true, true, false, true, false, true)
    private fun server(reachable: Boolean = true, user: String = "alice") = ServerInfo("srv", url, user, "Music", caps, null, reachable)
    private val credential = ServerCredential(url, "alice", "secret", null)
    private lateinit var snapshot: Snapshot
    private lateinit var core: ScriptedCore
    private lateinit var store: InMemoryCredentialStore
    private var replay: Job? = null

    private fun started(servers: List<ServerInfo>) = Event.Started(EventStartedInner(snapshot.copy(servers = servers)))
    private fun snapshotOf(servers: List<ServerInfo>) = Event.Snapshot(EventSnapshotInner(snapshot.copy(servers = servers)))
    private fun serversChanged(servers: List<ServerInfo>) = Event.ServersChanged(EventServersChangedInner(servers))
    private fun authError() = Event.Error(EventErrorInner(ErrorKind.Auth, "Wrong username or password", null))

    @Before
    fun setUp() = runBlocking {
        // A complete snapshot from the fake core; only `servers` matters here.
        val fake = FakeCore(startWithServer = false, startPlaying = false, timers = false, dispatcher = Dispatchers.Unconfined)
        val first = async(start = CoroutineStart.UNDISPATCHED) { fake.events.filterIsInstance<Event.Started>().first() }
        fake.dispatch(Command.Start)
        snapshot = first.await().data.snapshot
        fake.close()
        CoreHost.resetForTests()
        core = ScriptedCore()
        store = InMemoryCredentialStore()
        CoreHost.credentials = store
    }

    @After
    fun tearDown() {
        replay?.cancel()
        CoreHost.credentials = null
        CoreHost.resetForTests()
    }

    /**
     * The collector runs on the test's event loop but is not a child of it: it never completes on
     * its own, and `runBlocking` would otherwise wait for it forever. [tearDown] cancels it.
     */
    private fun kotlinx.coroutines.CoroutineScope.startReplay() {
        replay = kotlinx.coroutines.CoroutineScope(coroutineContext + Job()).launch(start = CoroutineStart.UNDISPATCHED) { CoreHost.replayCredentials(core, store) }
    }

    @Test
    fun storedLoginIsReplayedOnceAndSnapshotsNeverReplay() = runBlocking {
        store.save(credential)
        startReplay()
        core.emit(started(emptyList()))
        assertEquals(1, core.addServers.size)
        assertTrue(CoreHost.credentialsReplayed.value)
        assertTrue("login state is published after the replay", CoreHost.hasLogin(server(), CoreHost.logins.value))
        // RequestSnapshot (every UI attach, every service reconnect) emits Snapshot: no second AddServer, no second probe.
        core.emit(snapshotOf(listOf(server(reachable = true))))
        core.emit(snapshotOf(listOf(server(reachable = true))))
        assertEquals(1, core.addServers.size)
    }

    @Test
    fun aSnapshotBeforeStartedDoesNotReplayAndARepeatedStartedDoesNotEither() = runBlocking {
        store.save(credential)
        startReplay()
        // A snapshot alone is not the start of a core: nothing to replay yet, the gate stays closed.
        core.emit(snapshotOf(listOf(server(reachable = false))))
        assertEquals(0, core.addServers.size)
        assertFalse(CoreHost.credentialsReplayed.value)
        core.emit(started(listOf(server(reachable = false))))
        assertEquals(1, core.addServers.size)
        // Defensive: a core that repeated Started still replays exactly once per instance.
        core.emit(started(listOf(server(reachable = true))))
        assertEquals(1, core.addServers.size)
    }

    @Test
    fun replayIsLimitedToLoginsMatchingAPersistedServer() = runBlocking {
        store.save(credential)
        store.save(ServerCredential("https://other.example.net", "bob", "pw", null))
        startReplay()
        core.emit(started(listOf(server(reachable = false))))
        assertEquals(listOf("alice"), core.addServers.map { it.data.username })
    }

    @Test
    fun signOutRemovesTheLoginBeforeRemoveServerAndTheServerNeverComesBack() = runBlocking {
        store.save(credential)
        startReplay()
        core.emit(started(listOf(server(reachable = false))))
        core.emit(serversChanged(listOf(server(reachable = true))))
        assertEquals(1, core.addServers.size)
        var storeEmptyAtRemove: Boolean? = null
        core.onDispatch = { c -> if (c is Command.RemoveServer) storeEmptyAtRemove = store.all().isEmpty() }
        CoreHost.removeServer(core::dispatch, server())
        assertEquals("the credential is gone before RemoveServer reaches the core", true, storeEmptyAtRemove)
        assertFalse(CoreHost.hasLogin(server(), CoreHost.logins.value))
        // The core clears the server and re-emits everything: Snapshot{servers: []} then ServersChanged{[]}.
        core.emit(snapshotOf(emptyList()))
        core.emit(serversChanged(emptyList()))
        assertEquals("nothing was replayed by the post-removal Snapshot", 1, core.addServers.size)
        assertTrue(store.all().isEmpty())
    }

    @Test
    fun setupLoginIsStoredOnlyOnceTheServerIsReachableWithIt() = runBlocking {
        startReplay()
        core.emit(started(emptyList()))
        assertEquals(0, core.addServers.size)
        CoreHost.login(core::dispatch, credential)
        assertEquals(1, core.addServers.size)
        assertNull("not stored before the core verified it", store.get(url, "alice"))
        // Install-time list for a persisted server: reachable=false, still unverified.
        core.emit(serversChanged(listOf(server(reachable = false))))
        assertNull(store.get(url, "alice"))
        core.emit(serversChanged(listOf(server(reachable = true))))
        assertEquals("secret", store.get(url, "alice")?.password)
        assertTrue(CoreHost.hasLogin(server(), CoreHost.logins.value))
    }

    @Test
    fun refusedSetupLoginIsNeverStored() = runBlocking {
        startReplay()
        core.emit(started(emptyList()))
        CoreHost.login(core::dispatch, credential)
        core.emit(authError())
        core.emit(serversChanged(listOf(server(reachable = true))))
        assertNull("a login refused by the server is not saved, even when a server later shows up", store.get(url, "alice"))
        assertTrue(store.all().isEmpty())
    }

    @Test
    fun storedLoginRefusedByTheServerIsRemoved() = runBlocking {
        store.save(credential)
        startReplay()
        core.emit(started(listOf(server(reachable = false))))
        assertEquals(1, core.addServers.size)
        core.emit(authError())
        assertTrue("the stale password is not replayed forever", store.all().isEmpty())
        assertFalse(CoreHost.hasLogin(server(), CoreHost.logins.value))
    }

    @Test
    fun serverListPrunesLoginsOnlyAfterTheReplay() = runBlocking {
        store.save(credential)
        startReplay()
        // Before the replay a ServersChanged must not prune what the replay is about to use.
        core.emit(serversChanged(emptyList()))
        assertEquals(1, store.all().size)
        core.emit(started(emptyList()))
        assertEquals(1, core.addServers.size)
        core.emit(serversChanged(listOf(server(user = "someone-else"))))
        assertTrue("logins for servers the core no longer lists are dropped", store.all().isEmpty())
    }
}
