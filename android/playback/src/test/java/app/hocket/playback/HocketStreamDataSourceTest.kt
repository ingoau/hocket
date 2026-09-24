package app.hocket.playback

import android.app.Application
import android.net.Uri
import android.os.Looper
import androidx.media3.common.C
import androidx.media3.common.PlaybackException
import androidx.media3.datasource.DataSource
import androidx.media3.datasource.DataSourceException
import androidx.media3.datasource.DataSpec
import androidx.media3.datasource.TransferListener
import androidx.test.core.app.ApplicationProvider
import androidx.test.ext.junit.runners.AndroidJUnit4
import app.hocket.core.CoreHandle
import app.hocket.core.CoreKind
import app.hocket.core.CoreStreamException
import app.hocket.core.CoreStreamInfo
import app.hocket.core.CoreStreams
import app.hocket.core.api.BackendCommand
import app.hocket.core.api.BackendCommandLoadInner
import app.hocket.core.api.BackendReport
import app.hocket.core.api.Command
import app.hocket.core.api.Event
import app.hocket.core.api.MediaSource
import app.hocket.core.api.OfflineState
import app.hocket.core.api.Query
import app.hocket.core.api.QueryResult
import app.hocket.core.api.TrackSummary
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.SharedFlow
import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertSame
import org.junit.Assert.assertTrue
import org.junit.Assert.fail
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.Shadows.shadowOf
import org.robolectric.annotation.Config
import java.io.ByteArrayOutputStream
import java.util.concurrent.CopyOnWriteArrayList

/** An in-memory core stream reader: tokens map to byte arrays; records every call. */
internal class FakeStreams(private val files: Map<String, ByteArray>, private val maxRead: Int = CoreStreams.MAX_READ) : CoreStreams {
    val calls = CopyOnWriteArrayList<String>()
    val open = HashMap<Long, Triple<ByteArray, Int, Int>>() // handle -> (bytes, position, end)
    private var next = 1L
    var failOpenWith: CoreStreamException? = null

    @Synchronized
    override fun streamOpen(url: String, offset: Long, length: Long?): CoreStreamInfo {
        calls += "open $url @$offset${length?.let { "+$it" } ?: ""}"
        failOpenWith?.let { throw it }
        val bytes = files[url.removePrefix("hocket-stream://")] ?: throw CoreStreamException(CoreStreamException.Kind.UnknownToken)
        if (offset > bytes.size) throw CoreStreamException(CoreStreamException.Kind.RangeNotSatisfiable)
        val end = if (length != null) minOf(bytes.size.toLong(), offset + length).toInt() else bytes.size
        val h = next++
        open[h] = Triple(bytes, offset.toInt(), end)
        return CoreStreamInfo(h, offset, bytes.size.toLong(), (end - offset), "audio/wav")
    }

    @Synchronized
    override fun streamRead(handle: Long, maxBytes: Int): ByteArray {
        calls += "read $handle $maxBytes"
        val (bytes, pos, end) = open[handle] ?: throw CoreStreamException(CoreStreamException.Kind.UnknownHandle)
        val n = minOf(maxBytes, maxRead, end - pos)
        open[handle] = Triple(bytes, pos + n, end)
        return bytes.copyOfRange(pos, pos + n)
    }

    @Synchronized
    override fun streamClose(handle: Long) {
        calls += "close $handle"
        open.remove(handle)
    }
}

@RunWith(AndroidJUnit4::class)
@Config(sdk = [34], application = Application::class)
class HocketStreamDataSourceTest {
    private val audio = ByteArray(700_000) { (it * 31 % 251).toByte() }

    private fun readAll(source: DataSource, bufferSize: Int = 4096): ByteArray {
        val out = ByteArrayOutputStream()
        val buf = ByteArray(bufferSize)
        while (true) {
            val n = source.read(buf, 0, buf.size)
            if (n == C.RESULT_END_OF_INPUT) break
            out.write(buf, 0, n)
        }
        return out.toByteArray()
    }

    @Test
    fun opensReadsInChunksSeeksByReopeningAndCloses() {
        val streams = FakeStreams(mapOf("tok" to audio))
        val source = HocketStreamDataSource { streams }
        val uri = Uri.parse("hocket-stream://tok")
        assertEquals(audio.size.toLong(), source.open(DataSpec(uri)))
        assertEquals(uri, source.uri)
        assertArrayEquals(audio, readAll(source))
        // Small extractor reads are served from one chunk: far fewer FFI reads than 4 KiB reads.
        val reads = streams.calls.count { it.startsWith("read") }
        assertTrue("reads=$reads", reads <= audio.size / HocketStreamDataSource.MIN_CHUNK_BYTES + 2)
        assertTrue("no read asks for more than the core's cap", streams.calls.filter { it.startsWith("read") }.all { it.substringAfterLast(' ').toInt() <= CoreStreams.MAX_READ })
        source.close()
        assertTrue(streams.open.isEmpty())

        // A seek is a new open at the position; a bounded length stops there.
        assertEquals(1000L, source.open(DataSpec.Builder().setUri(uri).setPosition(500_000).setLength(1000).build()))
        assertArrayEquals(audio.copyOfRange(500_000, 501_000), readAll(source, bufferSize = 300))
        source.close()
        source.close() // idempotent
        assertEquals(listOf("open hocket-stream://tok @0", "open hocket-stream://tok @500000+1000"), streams.calls.filter { it.startsWith("open") })
        assertEquals(2, streams.calls.count { it.startsWith("close") })
    }

    @Test
    fun unknownLengthReadsUntilAnEmptyChunk() {
        // A transcode: the core does not know the length up front.
        val inner = FakeStreams(mapOf("t" to audio))
        val streams = object : CoreStreams by inner {
            override fun streamOpen(url: String, offset: Long, length: Long?) = inner.streamOpen(url, offset, length).copy(totalLength = null, length = null)
        }
        val source = HocketStreamDataSource { streams }
        assertEquals(C.LENGTH_UNSET.toLong(), source.open(DataSpec(Uri.parse("hocket-stream://t"))))
        assertArrayEquals(audio, readAll(source))
        source.close()
    }

    @Test
    fun anUnknownTokenIsFileNotFoundSoItIsNotRetriedAndTheCoreReResolves() {
        val streams = FakeStreams(emptyMap())
        val source = HocketStreamDataSource { streams }
        try {
            source.open(DataSpec(Uri.parse("hocket-stream://expired")))
            fail("expected a DataSourceException")
        } catch (e: DataSourceException) {
            assertEquals(PlaybackException.ERROR_CODE_IO_FILE_NOT_FOUND, e.reason)
            assertTrue(e.cause is CoreStreamException)
        }
        // The backend's load policy never retries it (Media3's default would, three times): the error
        // reaches the player at once. Transient failures keep the default retries.
        val policy = CoreStreamLoadErrorPolicy()
        val info = androidx.media3.exoplayer.upstream.LoadErrorHandlingPolicy.LoadErrorInfo(
            androidx.media3.exoplayer.source.LoadEventInfo(0, DataSpec(Uri.parse("hocket-stream://expired")), 0),
            androidx.media3.exoplayer.source.MediaLoadData(C.DATA_TYPE_MEDIA),
            HocketStreamDataSource.toDataSourceException(CoreStreamException(CoreStreamException.Kind.UnknownToken)), 1,
        )
        assertEquals(C.TIME_UNSET, policy.getRetryDelayMsFor(info))
        val network = androidx.media3.exoplayer.upstream.LoadErrorHandlingPolicy.LoadErrorInfo(
            info.loadEventInfo, info.mediaLoadData,
            HocketStreamDataSource.toDataSourceException(CoreStreamException(CoreStreamException.Kind.Network, detail = "reset")), 1,
        )
        assertTrue("a network hiccup is retried", policy.getRetryDelayMsFor(network) != C.TIME_UNSET)
        source.close() // nothing opened: no close call, no transfer end
        assertFalse(streams.calls.any { it.startsWith("close") })
    }

    @Test
    fun errorsMapToMedia3Reasons() {
        fun reason(kind: CoreStreamException.Kind) = HocketStreamDataSource.toDataSourceException(CoreStreamException(kind)).reason
        assertEquals(PlaybackException.ERROR_CODE_IO_FILE_NOT_FOUND, reason(CoreStreamException.Kind.UnknownToken))
        assertEquals(PlaybackException.ERROR_CODE_IO_READ_POSITION_OUT_OF_RANGE, reason(CoreStreamException.Kind.RangeNotSatisfiable))
        assertEquals(PlaybackException.ERROR_CODE_IO_BAD_HTTP_STATUS, reason(CoreStreamException.Kind.Status))
        assertEquals(PlaybackException.ERROR_CODE_IO_NETWORK_CONNECTION_FAILED, reason(CoreStreamException.Kind.Network))
        assertEquals(PlaybackException.ERROR_CODE_IO_NETWORK_CONNECTION_FAILED, reason(CoreStreamException.Kind.NoServer))
        assertEquals(PlaybackException.ERROR_CODE_IO_UNSPECIFIED, reason(CoreStreamException.Kind.ShutDown))
    }

    @Test
    fun noRunningCoreFailsTheOpenInsteadOfCrashing() {
        val source = HocketStreamDataSource { null }
        try {
            source.open(DataSpec(Uri.parse("hocket-stream://x")))
            fail("expected a DataSourceException")
        } catch (e: DataSourceException) {
            assertEquals(PlaybackException.ERROR_CODE_IO_UNSPECIFIED, e.reason)
        }
    }

    /** Records what it was asked to open. */
    private class RecordingSource(val name: String) : DataSource {
        val opened = mutableListOf<Uri>()
        var closed = 0
        val listeners = mutableListOf<TransferListener>()
        private var current: Uri? = null
        override fun addTransferListener(transferListener: TransferListener) { listeners += transferListener }
        override fun open(dataSpec: DataSpec): Long { opened += dataSpec.uri; current = dataSpec.uri; return 3 }
        override fun read(buffer: ByteArray, offset: Int, length: Int): Int = C.RESULT_END_OF_INPUT
        override fun getUri(): Uri? = current
        override fun close() { closed++; current = null }
    }

    @Test
    fun theFactoryRoutesByScheme() {
        val core = RecordingSource("core")
        val other = RecordingSource("default")
        val routing = CoreStreamRoutingDataSource(core, other)
        val listener = object : TransferListener {
            override fun onTransferInitializing(source: DataSource, dataSpec: DataSpec, isNetwork: Boolean) = Unit
            override fun onTransferStart(source: DataSource, dataSpec: DataSpec, isNetwork: Boolean) = Unit
            override fun onBytesTransferred(source: DataSource, dataSpec: DataSpec, isNetwork: Boolean, bytesTransferred: Int) = Unit
            override fun onTransferEnd(source: DataSource, dataSpec: DataSpec, isNetwork: Boolean) = Unit
        }
        routing.addTransferListener(listener)
        assertSame(listener, core.listeners.single())
        assertSame(listener, other.listeners.single())
        for ((url, expect) in listOf(
            "hocket-stream://abc" to core, "HOCKET-STREAM://abc" to core,
            "file:///data/x.flac" to other, "https://music.example.net/rest/stream?id=1" to other, "content://media/1" to other,
        )) {
            routing.open(DataSpec(Uri.parse(url)))
            assertEquals(url, Uri.parse(url), routing.uri)
            routing.close()
            assertEquals(url, Uri.parse(url), expect.opened.last())
        }
        assertEquals(2, core.opened.size)
        assertEquals(3, other.opened.size)
        routing.close() // closing a closed source is a no-op
        assertEquals(2, core.closed)
        assertEquals(3, other.closed)
        // The real factory builds the router over HocketStreamDataSource.
        val real = CoreStreamDataSourceFactory(ApplicationProvider.getApplicationContext(), { FakeStreams(mapOf("t" to audio)) }).createDataSource()
        assertEquals(audio.size.toLong(), real.open(DataSpec(Uri.parse("hocket-stream://t"))))
        assertArrayEquals(audio, readAll(real))
        real.close()
    }

    /** A core handle that is also a stream reader, recording dispatches (for [CoreHost.start]). */
    private class StreamingCore(streams: CoreStreams) : CoreHandle, CoreStreams by streams {
        override val kind = CoreKind.Native
        override val events: SharedFlow<Event> = MutableSharedFlow()
        val dispatched = mutableListOf<Command>()
        override fun dispatch(command: Command) { dispatched += command }
        override suspend fun query(query: Query): QueryResult = error("unused")
        override fun close() = Unit
    }

    @Test
    fun everyCoreStartDeclaresTheCoreStreamCapabilityBeforeStart() {
        val native = StreamingCore(FakeStreams(emptyMap()))
        CoreHost.start(native)
        assertEquals(listOf<Command>(app.hocket.core.Commands.setBackendCapabilities(true), Command.Start), native.dispatched)
        // A core without a stream reader (the fake) keeps server URLs.
        val plain = object : CoreHandle {
            override val kind = CoreKind.Fake
            override val events: SharedFlow<Event> = MutableSharedFlow()
            val dispatched = mutableListOf<Command>()
            override fun dispatch(command: Command) { dispatched += command }
            override suspend fun query(query: Query): QueryResult = error("unused")
            override fun close() = Unit
        }
        CoreHost.start(plain)
        assertEquals(listOf<Command>(app.hocket.core.Commands.setBackendCapabilities(false), Command.Start), plain.dispatched)
    }

    private fun source(key: String, url: String) = MediaSource(
        key, TrackSummary("t-$key", "srv", "Title", null, null, null, null, 1000u, null, 0u, false, OfflineState.None),
        url, HashMap(), "audio/wav", 0.0, false,
    )

    /** 0.2 s of 8 kHz mono silence. */
    private fun wav(): ByteArray {
        val data = ByteArray(3200)
        val out = ByteArrayOutputStream()
        fun le32(v: Int) = byteArrayOf(v.toByte(), (v shr 8).toByte(), (v shr 16).toByte(), (v shr 24).toByte())
        fun le16(v: Int) = byteArrayOf(v.toByte(), (v shr 8).toByte())
        out.write("RIFF".toByteArray()); out.write(le32(36 + data.size)); out.write("WAVE".toByteArray())
        out.write("fmt ".toByteArray()); out.write(le32(16)); out.write(le16(1)); out.write(le16(1)); out.write(le32(8000)); out.write(le32(16000)); out.write(le16(2)); out.write(le16(16))
        out.write("data".toByteArray()); out.write(le32(data.size)); out.write(data)
        return out.toByteArray()
    }

    /** Runs the main looper, advancing the fake clock (ExoPlayer schedules its work by uptime). */
    private fun runUntil(timeoutMs: Long = 10_000, describe: () -> String = { "" }, condition: () -> Boolean) {
        val deadline = System.currentTimeMillis() + timeoutMs
        while (!condition()) {
            if (System.currentTimeMillis() > deadline) fail("condition not met in ${timeoutMs}ms: ${describe()}")
            shadowOf(Looper.getMainLooper()).idleFor(java.time.Duration.ofMillis(10))
            Thread.sleep(5)
        }
    }

    /**
     * ExoPlayer end to end: a `hocket-stream://` Load is read through the core's reader and plays;
     * an expired token surfaces as a fatal `Error` report for the item's key, which is what makes
     * the core resolve the item again.
     */
    @Test
    fun exoPlayerReadsCoreStreamsAndAnExpiredTokenIsAFatalErrorForTheKey() {
        val scope = CoroutineScope(SupervisorJob() + Dispatchers.Main)
        val dispatched = CopyOnWriteArrayList<Command>()
        val streams = FakeStreams(mapOf("good" to wav()))
        val backend = ExoBackend(ApplicationProvider.getApplicationContext(), scope, { dispatched += it }, streams = { streams })
        try {
            backend.handle(BackendCommand.Load(BackendCommandLoadInner(source("a", "hocket-stream://good"), null, 0u, false)))
            runUntil(describe = { "calls=${streams.calls} dispatched=$dispatched" }) { dispatched.any { (it as? Command.BackendReport)?.data?.report is BackendReport.Ready } }
            assertTrue(streams.calls.first().startsWith("open hocket-stream://good @0"))

            dispatched.clear()
            backend.handle(BackendCommand.Load(BackendCommandLoadInner(source("b", "hocket-stream://expired"), null, 0u, true)))
            runUntil(describe = { "calls=${streams.calls} dispatched=$dispatched" }) { dispatched.any { (it as? Command.BackendReport)?.data?.report is BackendReport.Error } }
            val error = dispatched.mapNotNull { ((it as? Command.BackendReport)?.data?.report as? BackendReport.Error)?.data }.single()
            assertEquals("b", error.key)
            assertTrue("an expired token must be fatal so the core re-resolves: ${error.message}", error.fatal)
            assertTrue(error.message, error.message.contains("FILE_NOT_FOUND"))
            assertEquals("not retried by the load policy", 1, streams.calls.count { it.startsWith("open hocket-stream://expired") })
        } finally {
            backend.release()
            scope.cancel()
        }
    }
}
