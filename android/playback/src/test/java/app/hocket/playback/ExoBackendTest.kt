package app.hocket.playback

import android.app.Application
import androidx.test.core.app.ApplicationProvider
import androidx.test.ext.junit.runners.AndroidJUnit4
import app.hocket.core.api.BackendCommand
import app.hocket.core.api.BackendCommandLoadInner
import app.hocket.core.api.BackendCommandSetNextInner
import app.hocket.core.api.BackendReport
import app.hocket.core.api.Command
import app.hocket.core.api.MediaSource
import app.hocket.core.api.OfflineState
import app.hocket.core.api.TrackSummary
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import org.junit.After
import androidx.media3.common.PlaybackException
import androidx.media3.datasource.DataSourceException
import androidx.media3.datasource.DataSpec
import androidx.media3.datasource.HttpDataSource
import androidx.media3.exoplayer.ExoPlaybackException
import app.hocket.core.CoreStreamException
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.annotation.Config

/** ExoPlayer under Robolectric: the playlist-shape rules that keep reports attributable. */
@RunWith(AndroidJUnit4::class)
@Config(sdk = [34], application = Application::class)
class ExoBackendTest {
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Main)
    private val dispatched = mutableListOf<Command>()
    private lateinit var backend: ExoBackend

    private fun source(key: String) = MediaSource(
        key, TrackSummary("t-$key", "srv", "Title", null, null, null, null, 1000u, null, 0u, false, OfflineState.Cached),
        "file:///nonexistent/$key.wav", HashMap(), "audio/wav", 0.0, false,
    )

    /** A core stream the core does not have whole yet. */
    private fun stream(key: String, local: Boolean? = null) = source(key).copy(url = "hocket-stream://tok-$key", local = local)

    /** The core's answers to [app.hocket.core.CoreStreams.streamLocal], by URL. */
    private class LocalStreams : app.hocket.core.CoreStreams {
        val local = java.util.concurrent.ConcurrentHashMap<String, Boolean>()
        override fun streamOpen(url: String, offset: Long, length: Long?): app.hocket.core.CoreStreamInfo = throw UnsupportedOperationException()
        override fun streamRead(handle: Long, maxBytes: Int): ByteArray = throw UnsupportedOperationException()
        override fun streamClose(handle: Long) {}
        override fun streamLocal(url: String): Boolean = local[url] ?: false
    }

    /** Runs the main looper until the backend's wake mode is [mode] (the core is asked off the main thread). */
    private fun awaitWakeMode(b: ExoBackend, mode: Int) {
        val deadline = System.currentTimeMillis() + 5_000
        while (b.wakeMode != mode && System.currentTimeMillis() < deadline) {
            org.robolectric.Shadows.shadowOf(android.os.Looper.getMainLooper()).idle()
            Thread.sleep(5)
        }
        assertEquals(mode, b.wakeMode)
    }

    @Test
    fun theWifiLockIsHeldOnlyWhileTheCurrentOrNextItemIsStreamed() {
        val core = LocalStreams()
        val b = ExoBackend(ApplicationProvider.getApplicationContext(), scope, { dispatched += it }, { core })
        try {
            val network = androidx.media3.common.C.WAKE_MODE_NETWORK
            val local = androidx.media3.common.C.WAKE_MODE_LOCAL
            assertEquals(network, b.wakeMode)
            // Both on the device (`file:`): the wake lock alone.
            b.handle(BackendCommand.Load(BackendCommandLoadInner(source("a"), source("b"), 0u, false)))
            awaitWakeMode(b, local)
            // A streamed follow-up needs the network, whatever the queue shows: a copy at another
            // profile makes the track Cached while this stream still comes from the server.
            b.handle(BackendCommand.SetNext(BackendCommandSetNextInner(stream("c"))))
            awaitWakeMode(b, network)
            b.onQueueChanged()
            awaitWakeMode(b, network)
            // The core fetched it whole (and says so): the queue changing asks again.
            core.local["hocket-stream://tok-c"] = true
            b.onQueueChanged()
            awaitWakeMode(b, local)
            // Local when resolved, but evicted since: the core's "no" wins over the flag.
            b.handle(BackendCommand.Load(BackendCommandLoadInner(stream("d", local = true), null, 0u, false)))
            awaitWakeMode(b, network)
            core.local["hocket-stream://tok-d"] = true
            b.handle(BackendCommand.Load(BackendCommandLoadInner(stream("d", local = true), null, 0u, false)))
            awaitWakeMode(b, local)
            core.local.remove("hocket-stream://tok-d")
            b.onQueueChanged()
            awaitWakeMode(b, network)
        } finally {
            b.release()
        }
    }

    @Test
    fun wakeModeForNeedsEveryItemOnTheDevice() {
        assertEquals(androidx.media3.common.C.WAKE_MODE_LOCAL, ExoBackend.wakeModeFor(true, null))
        assertEquals(androidx.media3.common.C.WAKE_MODE_LOCAL, ExoBackend.wakeModeFor(true, true))
        assertEquals(androidx.media3.common.C.WAKE_MODE_NETWORK, ExoBackend.wakeModeFor(true, false))
        assertEquals(androidx.media3.common.C.WAKE_MODE_NETWORK, ExoBackend.wakeModeFor(false, true))
    }

    @Before
    fun setUp() {
        backend = ExoBackend(ApplicationProvider.getApplicationContext(), scope, { dispatched += it })
    }

    @After
    fun tearDown() {
        backend.release()
        scope.cancel()
    }

    @Test
    fun setNextWithoutACurrentItemIsIgnored() {
        backend.handle(BackendCommand.SetNext(BackendCommandSetNextInner(source("b"))))
        assertEquals("a follow-up must not become the current item", 0, backend.player.mediaItemCount)
    }

    @Test
    fun errorsAreAttributedToTheLastLoadUntilStop() {
        assertNull(backend.errorKey())
        backend.handle(BackendCommand.Load(BackendCommandLoadInner(source("a"), source("b"), 0u, false)))
        assertEquals("a", backend.errorKey())
        backend.handle(BackendCommand.Stop)
        assertNull("after Stop there is nothing to report an error against", backend.errorKey())
        backend.handle(BackendCommand.SetNext(BackendCommandSetNextInner(source("c"))))
        assertEquals(0, backend.player.mediaItemCount)
    }

    private fun reports() = dispatched.filterIsInstance<Command.BackendReport>().map { it.data.report }

    @Test
    fun theMusicVolumeIsReportedAndSetAsTheSystemVolume() {
        // ExoPlayer reads the stream's volume off the main thread: wait for its first report.
        val deadline = System.currentTimeMillis() + 5_000
        while (reports().none { it is BackendReport.SystemVolume } && System.currentTimeMillis() < deadline) {
            org.robolectric.Shadows.shadowOf(android.os.Looper.getMainLooper()).idle()
            Thread.sleep(10)
        }
        assertTrue("reported at start", reports().any { it is BackendReport.SystemVolume })
        val max = backend.player.deviceInfo.maxVolume
        backend.handle(BackendCommand.SetSystemVolume(app.hocket.core.api.BackendCommandSetSystemVolumeInner(1.0, false)))
        org.robolectric.Shadows.shadowOf(android.os.Looper.getMainLooper()).idle()
        assertEquals(max, backend.player.deviceVolume)
        val last = reports().filterIsInstance<BackendReport.SystemVolume>().last()
        assertEquals(1.0, last.data.volume, 1e-9)
        backend.handle(BackendCommand.SetSystemVolume(app.hocket.core.api.BackendCommandSetSystemVolumeInner(1.0, true)))
        org.robolectric.Shadows.shadowOf(android.os.Looper.getMainLooper()).idle()
        assertTrue(backend.player.isDeviceMuted)
    }

    @Test
    fun autoTransitionEndsThePlayedItemBeforeTheTransition() {
        backend.handle(BackendCommand.Load(BackendCommandLoadInner(source("a"), source("b"), 0u, false)))
        // Stand-in for ExoPlayer's own advance: the follow-up becomes current, then the AUTO callback.
        backend.player.seekToNextMediaItem()
        dispatched.clear()
        backend.onAutoTransition(backend.player.currentMediaItem!!)
        val reports = reports()
        assertEquals(BackendReport.Ended(app.hocket.core.api.BackendReportEndedInner("a")), reports[0])
        assertEquals(BackendReport.TransitionedToNext(app.hocket.core.api.BackendReportTransitionedToNextInner("b")), reports[1])
        assertEquals("the played item is dropped: current(+next)", 1, backend.player.mediaItemCount)
        assertEquals("b", backend.player.currentMediaItem?.mediaId)
    }

    @Test
    fun audioHeardReportsOnlyWhilePlaying() {
        backend.handle(BackendCommand.Load(BackendCommandLoadInner(source("a"), source("b"), 0u, false)))
        dispatched.clear()
        backend.onAudioHeard()
        assertTrue("a paused player has nothing to re-stamp", reports().none { it is BackendReport.Position })
    }

    private fun sourceError(cause: Throwable, code: Int): PlaybackException =
        ExoPlaybackException.createForSource(java.io.IOException(cause), code)

    @Test
    fun errorsALaterPrepareCanFixAreRecoverableTheRestAreNot() {
        fun core(kind: CoreStreamException.Kind, status: Int? = null, code: Int = PlaybackException.ERROR_CODE_IO_UNSPECIFIED) =
            sourceError(DataSourceException(CoreStreamException(kind, status), code), code)
        // The connection, a stalled server, a handle the core closed: retried.
        assertTrue(ExoBackend.isRecoverable(core(CoreStreamException.Kind.Network, code = PlaybackException.ERROR_CODE_IO_NETWORK_CONNECTION_FAILED)))
        assertTrue(ExoBackend.isRecoverable(core(CoreStreamException.Kind.Status, 503, PlaybackException.ERROR_CODE_IO_BAD_HTTP_STATUS)))
        assertTrue(ExoBackend.isRecoverable(core(CoreStreamException.Kind.Closed)))
        assertTrue(ExoBackend.isRecoverable(core(CoreStreamException.Kind.UnknownHandle)))
        assertTrue(ExoBackend.isRecoverable(core(CoreStreamException.Kind.TooManyHandles)))
        assertTrue(ExoBackend.isRecoverable(core(CoreStreamException.Kind.NoServer)))
        assertTrue("a stuck player is re-prepared", ExoBackend.isRecoverable(PlaybackException.ERROR_CODE_TIMEOUT))
        val http5xx = HttpDataSource.InvalidResponseCodeException(502, "Bad Gateway", null, emptyMap(), DataSpec(android.net.Uri.parse("https://music.example/s")), ByteArray(0))
        assertTrue(ExoBackend.isRecoverable(sourceError(http5xx, PlaybackException.ERROR_CODE_IO_BAD_HTTP_STATUS)))
        // The media, the token, a 4xx, a core that is gone: fatal, the core decides.
        assertFalse(ExoBackend.isRecoverable(core(CoreStreamException.Kind.Status, 404, PlaybackException.ERROR_CODE_IO_BAD_HTTP_STATUS)))
        assertFalse(ExoBackend.isRecoverable(core(CoreStreamException.Kind.UnknownToken, code = PlaybackException.ERROR_CODE_IO_FILE_NOT_FOUND)))
        assertFalse(ExoBackend.isRecoverable(core(CoreStreamException.Kind.ShutDown)))
        assertFalse(ExoBackend.isRecoverable(core(CoreStreamException.Kind.ErrorEnvelope, code = PlaybackException.ERROR_CODE_IO_BAD_HTTP_STATUS)))
        assertFalse(ExoBackend.isRecoverable(ExoPlaybackException.createForUnexpected(RuntimeException("decoder"), PlaybackException.ERROR_CODE_DECODING_FAILED)))
    }

    @Test
    fun aStallIsPausedOnlyWhenThePlayerNoLongerMeansToPlay() {
        // Rebuffering, a load being retried, an error being recovered from: still buffering.
        assertEquals(ExoBackend.Stall.Buffering, ExoBackend.stall(true, androidx.media3.common.Player.STATE_BUFFERING))
        assertEquals(ExoBackend.Stall.Nothing, ExoBackend.stall(true, androidx.media3.common.Player.STATE_IDLE))
        assertEquals(ExoBackend.Stall.Nothing, ExoBackend.stall(true, androidx.media3.common.Player.STATE_ENDED))
        assertEquals(ExoBackend.Stall.Nothing, ExoBackend.stall(true, androidx.media3.common.Player.STATE_READY))
        // A pause, becoming noisy, a lasting focus loss.
        assertEquals(ExoBackend.Stall.Paused, ExoBackend.stall(false, androidx.media3.common.Player.STATE_READY))
        assertEquals(ExoBackend.Stall.Paused, ExoBackend.stall(false, androidx.media3.common.Player.STATE_BUFFERING))
        assertEquals(ExoBackend.Stall.Nothing, ExoBackend.stall(false, androidx.media3.common.Player.STATE_ENDED))
        assertEquals(ExoBackend.Stall.Nothing, ExoBackend.stall(false, androidx.media3.common.Player.STATE_IDLE))
    }

    @Test
    fun thePlayerIsBusyWhileItStillMeansToPlayAndIdleAfterStop() {
        assertFalse(backend.isBusy())
        backend.handle(BackendCommand.Load(BackendCommandLoadInner(source("a"), null, 0u, true)))
        assertTrue("loading with play: the service must not stop it", backend.isBusy())
        backend.handle(BackendCommand.Pause)
        assertFalse("paused: idle as far as the service is concerned", backend.isBusy())
        backend.handle(BackendCommand.Play)
        assertTrue(backend.isBusy())
        backend.handle(BackendCommand.Stop)
        assertFalse(backend.isBusy())
        backend.onConnectivityChanged(false)
        backend.onConnectivityChanged(true)
        assertFalse("connectivity with no pending recovery changes nothing", backend.isBusy())
    }

    @Test
    fun networkErrorsAreRetriedWithBackoffThenGiveUp() {
        assertTrue(ExoBackend.isRecoverable(PlaybackException.ERROR_CODE_IO_NETWORK_CONNECTION_FAILED))
        assertTrue(ExoBackend.isRecoverable(PlaybackException.ERROR_CODE_IO_NETWORK_CONNECTION_TIMEOUT))
        assertFalse(ExoBackend.isRecoverable(PlaybackException.ERROR_CODE_IO_FILE_NOT_FOUND))
        assertFalse(ExoBackend.isRecoverable(PlaybackException.ERROR_CODE_DECODING_FAILED))
        val delays = generateSequence(0) { it + 1 }.map { ExoBackend.recoveryDelayMs(it) }.takeWhile { it != null }.toList()
        assertTrue("backs off", delays.zipWithNext().all { (a, b) -> a!! < b!! })
        assertTrue("keeps trying for about a minute", delays.sumOf { it!! } in 45_000L..120_000L)
        assertNull(ExoBackend.recoveryDelayMs(delays.size))
    }
}
