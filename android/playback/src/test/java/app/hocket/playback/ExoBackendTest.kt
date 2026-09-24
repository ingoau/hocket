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
