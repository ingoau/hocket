package app.hocket.realcore

import app.hocket.core.Commands
import app.hocket.core.NativeCore
import app.hocket.core.Queries
import app.hocket.core.api.AudioMode
import app.hocket.core.api.BackendCommand
import app.hocket.core.api.BackendReport
import app.hocket.core.api.BackendReportEndedInner
import app.hocket.core.api.BackendReportPlayingInner
import app.hocket.core.api.BackendReportPositionInner
import app.hocket.core.api.BackendReportReadyInner
import app.hocket.core.api.Command
import app.hocket.core.api.CoreConfig
import app.hocket.core.api.Event
import app.hocket.core.api.Page
import app.hocket.core.api.Platform
import app.hocket.core.api.QueryResult
import app.hocket.core.api.SortOrder
import app.hocket.core.client.CoreClient
import app.hocket.playback.CoreHost
import app.hocket.playback.InMemoryCredentialStore
import app.hocket.playback.ServerCredential
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.flow.launchIn
import kotlinx.coroutines.flow.onEach
import kotlinx.coroutines.launch
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.withTimeout
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertTrue
import org.junit.Assume.assumeTrue
import org.junit.Before
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import java.io.File
import java.util.concurrent.CopyOnWriteArrayList

/**
 * The real Rust core on the JVM: `libhocket_android.so` built for the host (x86_64-unknown-linux-gnu)
 * is loaded through JNA (UniFFI honours `uniffi.component.hocket_android.libraryOverride`), driven
 * through [NativeCore] against [FakeNavidrome], exactly the way the Android app drives it.
 *
 * Skipped (Assume) when the host library is absent: build it with
 * `CARGO_TARGET_DIR=target-connect cargo build -p hocket-android` or point `HOCKET_HOST_LIB` at it.
 */
@RunWith(RobolectricTestRunner::class)
// SDK 32: on 33+ the UniFFI glue uses android.system.SystemCleaner, whose Robolectric shadow needs
// jdk.internal.ref (not exported on JDK 17+); below 33 it uses JNA's own cleaner thread.
@Config(sdk = [32], application = android.app.Application::class)
class RealCoreEndToEndTest {
    private lateinit var server: FakeNavidrome
    private lateinit var dataDir: File
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Default)
    private val events = CopyOnWriteArrayList<Event>()
    private var core: NativeCore? = null

    companion object {
        fun hostLib(): File? {
            System.getenv("HOCKET_HOST_LIB")?.let { File(it) }?.takeIf { it.exists() }?.let { return it }
            var dir: File? = File(System.getProperty("user.dir")).absoluteFile
            repeat(5) {
                val candidate = File(dir, "target-connect/debug/libhocket_android.so")
                if (candidate.exists()) return candidate
                dir = dir?.parentFile
            }
            return null
        }
    }

    @Before
    fun setUp() {
        val lib = hostLib()
        assumeTrue("host libhocket_android.so not built; skipping the real-core test", lib != null)
        System.setProperty("uniffi.component.hocket_android.libraryOverride", lib!!.absolutePath)
        assumeTrue("host library not loadable on this JVM", NativeCore.isAvailable())
        server = FakeNavidrome().also { it.start() }
        dataDir = File(System.getProperty("java.io.tmpdir"), "hocket-e2e-" + System.nanoTime()).apply { mkdirs() }
    }

    @After
    fun tearDown() {
        core?.close()
        scope.cancel()
        if (::server.isInitialized) server.stop()
        if (::dataDir.isInitialized) dataDir.deleteRecursively()
    }

    private fun startCore(): NativeCore {
        val config = CoreConfig(
            dataDir = File(dataDir, "data").apply { mkdirs() }.absolutePath,
            cacheDir = File(dataDir, "cache").apply { mkdirs() }.absolutePath,
            deviceId = "jvm-test-device", deviceName = "JVM", platform = Platform.Android, appVersion = "0.1.0-test", audio = AudioMode.External, coordinatorListen = null,
        )
        val c = NativeCore.create(config, logLevel = "warn")
        c.events.onEach { events += it }.launchIn(scope)
        core = c
        return c
    }

    private suspend fun <T : Event> waitFor(timeoutMs: Long = 20_000, predicate: (Event) -> T?): T = withTimeout(timeoutMs) {
        while (true) {
            events.firstNotNullOfOrNull(predicate)?.let { return@withTimeout it }
            kotlinx.coroutines.delay(25)
        }
        @Suppress("UNREACHABLE_CODE") error("unreachable")
    }

    @Test
    fun addServerSyncPlayAndReportRoundTrip() = runBlocking {
        val core = startCore()
        core.dispatch(Command.Start)
        waitFor { it as? Event.Started }
        // The platform reports connectivity (the service does this on Android); the outbox and prefetch gate on it.
        core.dispatch(Commands.setNetworkState(app.hocket.core.api.NetworkState(app.hocket.core.api.NetworkKind.Wifi, false, "test")))
        assertTrue("fresh core has no servers", (events.first { it is Event.Started } as Event.Started).data.snapshot.servers.isEmpty())

        // AddServer → probe (ping, extensions, /auth/login 404) → ServersChanged with capabilities.
        core.dispatch(Commands.addServer(server.baseUrl, "alice", "secret", "Fake"))
        val servers = waitFor { e -> (e as? Event.ServersChanged)?.takeIf { it.data.servers.any { s -> s.capabilities.serverVersion != null } } }
        val info = servers.data.servers.first()
        assertTrue("0.63.1 meets the floor", info.capabilities.meetsFloor)
        assertTrue(info.capabilities.songLyrics)
        assertFalse("no native API: /auth/login is a 404", info.capabilities.nativeApi)
        assertTrue(server.calls.contains("ping") && server.calls.contains("getOpenSubsonicExtensions"))

        // Launch sync pages the library into the mirror.
        val sync = waitFor(40_000) { e -> (e as? Event.SyncProgress)?.takeIf { it.data.progress.finished } }
        assertEquals(info.id, sync.data.progress.serverId)
        assertTrue(server.calls.contains("search3") && server.calls.contains("getAlbumList2") && server.calls.contains("getArtists"))
        val albums = core.query(Queries.albums(info.id, Page(0u, 50u), SortOrder.Default)) as QueryResult.Albums
        assertTrue("albums synced: ${albums.data.items.map { it.id }}", albums.data.items.any { it.id == "al1" })
        val tracks = core.query(Queries.albumTracks("al1")) as QueryResult.TrackList
        assertEquals(listOf("s1", "s2"), tracks.data.map { it.id }.sorted())

        // Artwork resolves to a local file (possibly on the second query, after the background fetch).
        var path: String? = null
        withTimeout(20_000) {
            while (path == null) { path = (core.query(Queries.artwork("al1", 160)) as? QueryResult.Path)?.data; if (path == null) kotlinx.coroutines.delay(100) }
        }
        assertTrue("artwork cached at $path", File(path!!.removePrefix("file://")).exists())
        assertTrue(server.calls.contains("getCoverArt"))

        // PlayContext → QueueChanged/NowPlayingChanged and a Backend Load for the external player.
        events.clear()
        core.dispatch(Commands.playContext(Commands.albumContext(info.id, "al1", "Music Has the Right")))
        val now = waitFor { e -> (e as? Event.NowPlayingChanged)?.takeIf { it.data.entry != null } }
        // The core resolves the album's order (disc/track number); the first resolved track plays first.
        val firstInAlbum = tracks.data.sortedWith(compareBy({ it.discNumber?.toInt() ?: 1 }, { it.trackNumber?.toInt() ?: 0 })).first().id
        val secondInAlbum = tracks.data.map { it.id }.first { it != firstInAlbum }
        assertEquals(firstInAlbum, now.data.entry!!.track.id)
        val load = waitFor { e -> (e as? Event.Backend)?.takeIf { it.data.command is BackendCommand.Load } }
        val loadCmd = load.data.command as BackendCommand.Load
        assertEquals(now.data.entry!!.item.key, loadCmd.data.source.key)
        assertTrue("stream url from the fake: ${loadCmd.data.source.url}", loadCmd.data.source.url.contains("/rest/stream"))
        assertTrue(loadCmd.data.play)
        assertEquals("the gapless follow-up is preloaded", secondInAlbum, loadCmd.data.next?.track?.id)

        // The backend reports back like ExoPlayer would; the core's transport follows.
        val key = loadCmd.data.source.key
        core.dispatch(Commands.backendReport(BackendReport.Ready(BackendReportReadyInner(key, 4000u))))
        core.dispatch(Commands.backendReport(BackendReport.Playing(BackendReportPlayingInner(key, 0u))))
        waitFor { e -> (e as? Event.TransportChanged)?.takeIf { it.data.transport.position.isPlaying } }
        core.dispatch(Commands.backendReport(BackendReport.Position(BackendReportPositionInner(key, 3500u))))
        waitFor { e -> (e as? Event.TransportChanged)?.takeIf { it.data.transport.position.positionMs >= 3500u } }
        // Natural end without a preloaded transition: the queue advances to the second track.
        core.dispatch(Commands.backendReport(BackendReport.Ended(BackendReportEndedInner(key))))
        val next = waitFor { e -> (e as? Event.NowPlayingChanged)?.takeIf { it.data.entry?.track?.id == secondInAlbum } }
        assertNotNull(next)

        // A rating goes through the outbox to the server and is undoable.
        events.clear()
        core.dispatch(Commands.rateTrack("s1", 4))
        try {
            withTimeout(30_000) { while (server.ratings["s1"] != 4) kotlinx.coroutines.delay(50) }
        } catch (e: kotlinx.coroutines.TimeoutCancellationException) {
            val problems = (core.query(app.hocket.core.api.Query.Problems) as? QueryResult.Problems)?.data
            val jobs = (core.query(app.hocket.core.api.Query.Jobs) as? QueryResult.Jobs)?.data
            throw AssertionError("setRating never reached the server. calls=${server.calls.filter { it !in setOf("stream", "getCoverArt") }.distinct()} problems=${problems?.map { it.summary + ": " + it.detail }} jobs=${jobs?.map { it.label + "/" + it.state + "/" + it.failed }} events=${events.filterIsInstance<Event.Error>().map { it.data.message + ": " + it.data.detail }}")
        }
        val undo = waitFor { e -> (e as? Event.UndoChanged)?.takeIf { it.data.state.canUndo } }
        assertTrue(undo.data.state.undoLabel != null)
        core.dispatch(Command.Undo)
        try {
            withTimeout(30_000) { while (server.ratings["s1"] != 0) kotlinx.coroutines.delay(50) }
        } catch (e: kotlinx.coroutines.TimeoutCancellationException) {
            val problems = (core.query(app.hocket.core.api.Query.Problems) as? QueryResult.Problems)?.data
            val jobs = (core.query(app.hocket.core.api.Query.Jobs) as? QueryResult.Jobs)?.data
            val undoNow = (core.query(app.hocket.core.api.Query.UndoState) as? QueryResult.Undo)?.data
            val track = (core.query(Queries.track("s1")) as QueryResult.TrackDetail).data
            throw AssertionError("undo of the rating never reached the server: server rating=${server.ratings["s1"]} local rating=${track?.rating} setRating calls=${server.calls.count { it == "setRating" }} getSong calls=${server.calls.count { it == "getSong" }} recent calls=${server.calls.takeLast(12)} undo=${undoNow?.undoLabel}/${undoNow?.redoLabel}/${undoNow?.history?.map { it.label + ":" + it.note }} problems=${problems?.map { it.summary + ": " + it.detail }} jobs=${jobs?.map { it.label + "/" + it.state + "/" + it.failed }} toasts=${events.filterIsInstance<Event.Toast>().map { it.data.toast.message }} errors=${events.filterIsInstance<Event.Error>().map { it.data.message + ": " + it.data.detail }}")
        }
        val track = (core.query(Queries.track("s1")) as QueryResult.TrackDetail).data
        assertEquals(0u, track!!.rating)
    }

    @Test
    fun credentialsAreReplayedOnRestartAndSetupIsSkipped() = runBlocking {
        // First run: the app stores the login and the core persists server metadata only.
        val store = InMemoryCredentialStore()
        store.save(ServerCredential(server.baseUrl, "alice", "secret", "Fake"))
        val first = startCore()
        first.dispatch(Command.Start)
        waitFor { it as? Event.Started }
        first.dispatch(Commands.addServer(server.baseUrl, "alice", "secret", "Fake"))
        waitFor { e -> (e as? Event.ServersChanged)?.takeIf { it.data.servers.any { s -> s.capabilities.serverVersion != null } } }
        first.close()
        core = null
        events.clear()
        server.calls.clear()

        // Second run: Started lists the server without credentials; CoreHost replays AddServer from the store.
        val second = startCore()
        val replay = scope.launch { CoreHost.replayCredentials(second, store) }
        second.dispatch(Command.Start)
        val started = waitFor { it as? Event.Started }
        assertEquals(1, started.data.snapshot.servers.size)
        assertFalse("metadata only: not reachable until credentials are replayed", started.data.snapshot.servers[0].reachable)
        val probed = waitFor { e -> (e as? Event.ServersChanged)?.takeIf { it.data.servers.any { s -> s.reachable && s.capabilities.meetsFloor } } }
        assertEquals("alice", probed.data.servers[0].username)
        // The install-time ServersChanged carries the persisted capabilities; the fresh probe follows.
        withTimeout(20_000) { while (!server.calls.contains("ping")) kotlinx.coroutines.delay(50) }
        assertTrue("the replayed AddServer probed the server again", server.calls.contains("ping"))
        replay.cancel()
        // A pruned server list drops the stored login.
        store.retainOnly(emptyList())
        assertTrue(store.all().isEmpty())
    }

    @Test
    fun wrongPasswordIsAnAuthErrorAndTheServerStaysUnreachable() = runBlocking {
        val core = startCore()
        core.dispatch(Command.Start)
        waitFor { it as? Event.Started }
        events.clear() // drop the start-up ServersChanged (empty list)
        core.dispatch(Commands.addServer(server.baseUrl, "alice", "wrong", null))
        val err = waitFor { it as? Event.Error }
        assertTrue(err.data.message.contains("probe"))
        // The install-time ServersChanged reports reachable=true optimistically; the probe result flips it.
        val servers = waitFor { e -> (e as? Event.ServersChanged)?.takeIf { it.data.servers.isNotEmpty() && !it.data.servers.first().reachable } }
        assertFalse(servers.data.servers.first().reachable)
        // Subsonic error 40 ("Wrong username or password") maps to ErrorKind.Auth, so the setup screen can
        // tell a bad password from an outage.
        assertEquals("probe error kind (message=${err.data.message} detail=${err.data.detail})", app.hocket.core.api.ErrorKind.Auth, err.data.kind)
        assertTrue("detail names the credentials: kind=${err.data.kind} message=${err.data.message} detail=${err.data.detail}", err.data.detail?.contains("password", ignoreCase = true) == true || err.data.detail?.contains("auth", ignoreCase = true) == true || err.data.detail?.contains("40", ignoreCase = true) == true)
        val client = CoreClient(core, scope)
        client.close()
    }
}
