package app.hocket.realcore

import app.hocket.core.Commands
import app.hocket.core.NativeCore
import app.hocket.core.api.AudioMode
import app.hocket.core.api.Command
import app.hocket.core.api.CoreConfig
import app.hocket.core.api.Event
import app.hocket.core.api.LyricsTier
import app.hocket.core.api.NetworkKind
import app.hocket.core.api.NetworkState
import app.hocket.core.api.Platform
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.launchIn
import kotlinx.coroutines.flow.onEach
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.withTimeoutOrNull
import org.junit.After
import org.junit.Assert.assertEquals
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
 * The real core against a real Navidrome, the way the app drives it: login, a full library sync,
 * and lyrics for a track known to carry syllable timing with a background-vocal agent ("Tally",
 * twenty one pilots) arriving at the syllable tier. Runs only when `HOCKET_TEST_URL`,
 * `HOCKET_TEST_USER` and `HOCKET_TEST_PASS` are set in the environment (never stored anywhere);
 * skipped otherwise, and when the host `libhocket_android.so` is absent. Nothing from the
 * environment is ever printed: failure messages carry event kinds and messages with the server
 * address masked.
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [32], application = android.app.Application::class)
class RealServerTest {
    private lateinit var url: String
    private lateinit var user: String
    private lateinit var pass: String
    private lateinit var dataDir: File
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Default)
    private val events = CopyOnWriteArrayList<Event>()
    private var core: NativeCore? = null

    private companion object {
        /** "Tally": syllable-timed lyrics with Navidrome's `__nd_bg__|v1` background agent. */
        const val TALLY = "6kMqUUFnK9Y66sW9asXWjB"
    }

    @Before
    fun setUp() {
        val lib = RealCoreEndToEndTest.hostLib()
        assumeTrue("host libhocket_android.so not built; skipping", lib != null)
        val u = System.getenv("HOCKET_TEST_URL"); val n = System.getenv("HOCKET_TEST_USER"); val p = System.getenv("HOCKET_TEST_PASS")
        assumeTrue("HOCKET_TEST_URL/USER/PASS not set; skipping the real-server test", !u.isNullOrBlank() && !n.isNullOrBlank() && !p.isNullOrBlank())
        url = u!!.trim().trimEnd('/'); user = n!!.trim(); pass = p!!
        System.setProperty("uniffi.component.hocket_android.libraryOverride", lib!!.absolutePath)
        assumeTrue("host library not loadable on this JVM", NativeCore.isAvailable())
        dataDir = File(System.getProperty("java.io.tmpdir"), "hocket-real-" + System.nanoTime()).apply { mkdirs() }
    }

    @After
    fun tearDown() {
        core?.close()
        scope.cancel()
        if (::dataDir.isInitialized) dataDir.deleteRecursively()
    }

    private fun mask(s: String?): String? = s?.replace(url, "<server>")?.replace(user, "<user>")?.replace(pass, "<password>")

    private fun diagnostics(): String = "errors=" + events.filterIsInstance<Event.Error>().map { "${it.data.kind}: ${mask(it.data.message)} / ${mask(it.data.detail)}" } +
        " servers=" + events.filterIsInstance<Event.ServersChanged>().lastOrNull()?.data?.servers?.map { "${it.capabilities.serverVersion} reachable=${it.reachable} floor=${it.capabilities.meetsFloor} lyrics=${it.capabilities.songLyrics}" } +
        " sync=" + events.filterIsInstance<Event.SyncProgress>().lastOrNull()?.data?.progress?.let { "${it.phase} ${it.done}/${it.total} finished=${it.finished}" } +
        " jobs=" + events.filterIsInstance<Event.JobsChanged>().lastOrNull()?.data?.jobs?.map { "${it.kind}/${it.state}" }

    private suspend fun <T : Event> waitFor(timeoutMs: Long, what: String, predicate: (Event) -> T?): T =
        withTimeoutOrNull(timeoutMs) {
            while (true) {
                events.firstNotNullOfOrNull(predicate)?.let { return@withTimeoutOrNull it }
                delay(100)
            }
            @Suppress("UNREACHABLE_CODE") error("unreachable")
        } ?: throw AssertionError("timed out waiting for $what; ${diagnostics()}")

    @Test
    fun loginSyncAndSyllableLyricsFromTheRealServer() = runBlocking {
        val config = CoreConfig(
            dataDir = File(dataDir, "data").apply { mkdirs() }.absolutePath,
            cacheDir = File(dataDir, "cache").apply { mkdirs() }.absolutePath,
            deviceId = "jvm-real-server-test", deviceName = "JVM", platform = Platform.Android, appVersion = "0.1.0-test", audio = AudioMode.External, coordinatorListen = null,
        )
        val c = NativeCore.create(config, logLevel = "warn")
        c.events.onEach { events += it }.launchIn(scope)
        core = c
        c.dispatch(Command.Start)
        waitFor(20_000, "Started") { it as? Event.Started }
        c.dispatch(Commands.setNetworkState(NetworkState(NetworkKind.Wifi, false, "test")))

        // Login: the probe must accept the credentials and the server must meet the floor.
        c.dispatch(Commands.addServer(url, user, pass, null))
        val servers = waitFor(60_000, "a reachable server") { e -> (e as? Event.ServersChanged)?.takeIf { it.data.servers.any { s -> s.reachable && s.capabilities.serverVersion != null } } }
        val info = servers.data.servers.first()
        assertTrue("server below the floor: ${info.capabilities.serverVersion}", info.capabilities.meetsFloor)
        assertTrue("songLyrics extension expected", info.capabilities.songLyrics)
        assertTrue("no auth error expected; ${diagnostics()}", events.none { it is Event.Error && it.data.kind == app.hocket.core.api.ErrorKind.Auth })

        // A full library sync completes.
        waitFor(600_000, "the library sync to finish") { e -> (e as? Event.SyncProgress)?.takeIf { it.data.progress.finished } }

        // Tally's lyrics: fetched from the server with `enhanced=true`, adapted to the syllable tier
        // with the background-vocal agent as sub-voice lines.
        c.dispatch(Commands.fetchLyrics(TALLY))
        val changed = waitFor(60_000, "lyrics for Tally") { e -> (e as? Event.LyricsChanged)?.takeIf { it.data.track_id == TALLY && it.data.lyrics != null } }
        val lyrics = changed.data.lyrics!!
        assertEquals("syllable tier expected (did the core send enhanced=true?): lines=${lyrics.lines.size} syllables=${lyrics.lines.sumOf { it.syllables.size }}", LyricsTier.Syllable, lyrics.tier)
        assertTrue("a background-vocal agent expected: ${lyrics.agents.map { it.id }}", lyrics.agents.any { it.id.startsWith("__nd_bg__") })
        assertTrue("background lines expected as sub-voice lines", lyrics.lines.any { it.background && it.syllables.isNotEmpty() })
        assertTrue("main-voice lines with several syllables expected", lyrics.lines.count { !it.background && it.syllables.size >= 2 } > 20)
        // "I lost my rank and title": the syllable split "ti" + "tle" is one word (inclusive byte offsets).
        val title = lyrics.lines.first { it.text.startsWith("I lost my rank and title") }
        val ti = title.syllables.indexOfFirst { it.text == "ti" }
        assertTrue("expected a ti|tle split in ${title.syllables.map { it.text }}", ti >= 0 && title.syllables.getOrNull(ti + 1)?.text == "tle")
        assertTrue("'ti' is joined to 'tle'", title.syllables[ti].joined)
        assertEquals("the syllables are contiguous in time", title.syllables[ti].endMs, title.syllables[ti + 1].startMs)
        // Server timing as given: syllables run forward inside each line, and some lines end before
        // the next one starts (a gap the renderer holds, never sweeps; a few end a few ms after it).
        assertTrue("syllables are ordered", lyrics.lines.all { l -> l.syllables.all { it.startMs <= it.endMs } && l.syllables.zipWithNext().all { (a, b) -> a.startMs <= b.startMs } })
        assertTrue("gaps between lines are kept", lyrics.lines.filter { !it.background }.zipWithNext().any { (a, b) -> (a.endMs ?: 0u) < (b.startMs ?: 0u) })
        assertTrue(lyrics.lines.none { it.background && it.text.isBlank() })
    }

    /**
     * Tally through the app's streaming path against the real server: with the core-stream
     * capability the player gets a `hocket-stream://` source (no server URL, no credentials), and
     * [app.hocket.playback.HocketStreamDataSource] reads it through the core: the start of the file
     * is audio, and a seek (a new open at an offset, bounded) returns the same bytes as the
     * sequential read did.
     */
    @Test
    fun tallyStreamsThroughTheCoreDataSource() = runBlocking {
        val config = CoreConfig(
            dataDir = File(dataDir, "data").apply { mkdirs() }.absolutePath,
            cacheDir = File(dataDir, "cache").apply { mkdirs() }.absolutePath,
            deviceId = "jvm-real-server-stream", deviceName = "JVM", platform = Platform.Android, appVersion = "0.1.0-test", audio = AudioMode.External, coordinatorListen = null,
        )
        val c = NativeCore.create(config, logLevel = "warn")
        c.events.onEach { events += it }.launchIn(scope)
        core = c
        app.hocket.playback.CoreHost.start(c)
        waitFor(20_000, "Started") { it as? Event.Started }
        // Metered: no background prefetch competing with the reads below.
        c.dispatch(Commands.setNetworkState(NetworkState(NetworkKind.Wifi, true, "test")))
        c.dispatch(Commands.addServer(url, user, pass, null))
        val info = waitFor(60_000, "a reachable server") { e -> (e as? Event.ServersChanged)?.takeIf { it.data.servers.any { s -> s.reachable && s.capabilities.serverVersion != null } } }.data.servers.first()
        waitFor(600_000, "the library sync to finish") { e -> (e as? Event.SyncProgress)?.takeIf { it.data.progress.finished } }

        c.dispatch(Commands.playTracks(info.id, listOf(TALLY), 0, "Stream test"))
        val backend = waitFor(60_000, "a Load for Tally") { e -> (e as? Event.Backend)?.takeIf { (it.data.command as? app.hocket.core.api.BackendCommand.Load)?.data?.source?.track?.id == TALLY } }
        val load = (backend.data.command as app.hocket.core.api.BackendCommand.Load).data.source
        assertTrue("a core stream: ${mask(load.url.take(20))}", load.url.startsWith("hocket-stream://"))
        assertTrue("no server address or credentials in the player's url", !load.url.contains(url) && !load.url.contains("t=") && !load.url.contains(user))

        val source = app.hocket.playback.HocketStreamDataSource { c }
        val uri = android.net.Uri.parse(load.url)
        val length = source.open(androidx.media3.datasource.DataSpec(uri))
        val head = ByteArray(256 * 1024)
        var got = 0
        while (got < head.size) {
            val n = source.read(head, got, head.size - got)
            if (n == androidx.media3.common.C.RESULT_END_OF_INPUT) break
            got += n
        }
        source.close()
        assertTrue("read ${got} bytes of ${length}", got == head.size)
        val magic = String(head, 0, 4, Charsets.ISO_8859_1)
        assertTrue("audio expected, got ${head.take(8).map { it.toInt() and 0xff }}", magic == "fLaC" || magic.startsWith("ID3") || magic == "OggS" || String(head, 4, 4, Charsets.ISO_8859_1) == "ftyp" || (head[0].toInt() and 0xff) == 0xff)

        val offset = 100_000L
        assertEquals(1000L, source.open(androidx.media3.datasource.DataSpec.Builder().setUri(uri).setPosition(offset).setLength(1000).build()))
        val part = ByteArray(1000)
        var p = 0
        while (p < part.size) {
            val n = source.read(part, p, part.size - p)
            if (n == androidx.media3.common.C.RESULT_END_OF_INPUT) break
            p += n
        }
        source.close()
        assertTrue("the ranged read matches the sequential one", part.contentEquals(head.copyOfRange(offset.toInt(), offset.toInt() + 1000)))
    }
}
