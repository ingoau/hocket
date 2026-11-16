package app.hocket.playback

import android.app.Application
import android.os.Looper
import androidx.media3.common.C
import androidx.test.core.app.ApplicationProvider
import androidx.test.ext.junit.runners.AndroidJUnit4
import app.hocket.core.api.AnalysisRecord
import app.hocket.core.api.BackendCommand
import app.hocket.core.api.BackendCommandLoadInner
import app.hocket.core.api.BackendCommandSeekInner
import app.hocket.core.api.BackendCommandSetGaplessInner
import app.hocket.core.api.BackendCommandSetNextInner
import app.hocket.core.api.FadeCurve
import app.hocket.core.api.MediaSource
import app.hocket.core.api.OfflineState
import app.hocket.core.api.TrackSummary
import app.hocket.core.api.TransitionKind
import app.hocket.core.api.TransitionPlan
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.Shadows.shadowOf
import org.robolectric.annotation.Config
import java.nio.ByteBuffer

/**
 * AutoMix's clip on the current item (with a fake DSP, so the processor is installed): it stays
 * only while a transition out of the item can still run, else the item plays to its end.
 */
@RunWith(AndroidJUnit4::class)
@Config(sdk = [34], application = Application::class)
class ExoBackendClipTest {
    private object PassDsp : DspEngine {
        override fun chainNew(rate: Int, channels: Int) = 1L
        override fun chainFree(handle: Long) = Unit
        override fun chainSetFormat(handle: Long, rate: Int, channels: Int) = Unit
        override fun chainSetAudioSettings(handle: Long, json: String) = true
        override fun chainSetTrackGain(handle: Long, gainDb: Double) = Unit
        override fun chainReset(handle: Long) = Unit
        override fun chainIsBypassed(handle: Long) = true
        override fun chainProcess(handle: Long, buffer: ByteBuffer, samples: Int) = Unit
        override fun shaperNew(planJson: String, channels: Int, rate: Int) = 1L
        override fun shaperFree(handle: Long) = Unit
        override fun shaperProcess(handle: Long, buffer: ByteBuffer, samples: Int) = Unit
        override fun limit(buffer: ByteBuffer, samples: Int) = Unit
        override fun resamplerNew(inRate: Int, outRate: Int, channels: Int) = 0L
        override fun resamplerFree(handle: Long) = Unit
        override fun resamplerPrime(handle: Long, buffer: ByteBuffer, samples: Int) = Unit
        override fun resamplerFinish(handle: Long, buffer: ByteBuffer, samples: Int) = Unit
        override fun resamplerProcess(handle: Long, input: ByteBuffer, inSamples: Int, output: ByteBuffer, outOffset: Int, outCapacity: Int) = 0
    }

    /**
     * Every render fails, like a tail that never gets ready; a late fade is the plan from the new
     * tail start, unless [lateFades] is off (no room for one).
     */
    private class FailingFfi : AutomixFfi {
        var lateFades = true

        override fun usable(record: AnalysisRecord, durationMs: Long) = true
        override fun loudnessRecord(head: PcmDecoder.Pcm, tail: PcmDecoder.Pcm, tailStartMs: Long, durationMs: Long, transcoded: Boolean): AnalysisRecord? = null
        override fun prepareTail(planJson: String, pcm: PcmDecoder.Pcm, contextMs: Int, outChannels: Int, outRate: Int, audioSettingsJson: String?, gainDb: Double): RenderedTail =
            throw IllegalStateException("no tail")

        override fun lateFade(plan: TransitionPlan, tailStartMs: Long, aDurationMs: Long): TransitionPlan? =
            if (lateFades) plan.copy(tailStartMs = tailStartMs.toUInt(), tailLenMs = 4_000u, tailSourceMs = 4_000u, incomingAtMs = 0u, handoverMs = 2_000u) else null
    }

    private val ffi = FailingFfi()

    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Main)
    private lateinit var backend: ExoBackend

    private val plan = TransitionPlan(
        TransitionKind.Fade, tailStartMs = 100_000u, tailSourceMs = 8_000u, tailLenMs = 8_000u, incomingAtMs = 2_000u,
        incomingFromMs = 0u, handoverMs = 4_000u, outCurve = FadeCurve.EqualPower, inCurve = FadeCurve.EqualPower, incomingGainDb = 0f,
    )

    private fun source(key: String, mixOut: Boolean = true, mixIn: Boolean = true) = MediaSource(
        key, TrackSummary("t-$key", "srv", "Title", null, null, null, null, 200_000u, null, 0u, false, OfflineState.Cached),
        "file:///nonexistent/$key.wav", HashMap(), "audio/wav", 0.0, false,
    ).copy(mixOutMs = if (mixOut) 100_000u else null, mixIn = if (mixIn) plan else null)

    private fun clipEnd(): Long = backend.player.currentMediaItem!!.clippingConfiguration.endPositionMs

    private fun idle() = shadowOf(Looper.getMainLooper()).idle()

    private fun load(positionMs: UInt) =
        backend.handle(BackendCommand.Load(BackendCommandLoadInner(source("a", mixIn = false), source("b"), positionMs, false, 1u)))

    @Before
    fun setUp() {
        backend = ExoBackend(ApplicationProvider.getApplicationContext(), scope, {}, query = { null }, dsp = PassDsp, ffi = ffi)
    }

    @After
    fun tearDown() {
        backend.release()
        scope.cancel()
    }

    @Test
    fun aPlannedTransitionKeepsTheClip() {
        load(30_000u)
        idle()
        assertEquals(100_000L, clipEnd())
        backend.handle(BackendCommand.SetNext(BackendCommandSetNextInner(source("c"))))
        assertEquals("another next with a plan", 100_000L, clipEnd())
        assertEquals(listOf("a", "c"), (0 until backend.player.mediaItemCount).map { backend.player.getMediaItemAt(it).mediaId })
    }

    @Test
    fun aNextWithoutAPlanUnclipsAtTheSamePosition() {
        load(30_000u)
        backend.handle(BackendCommand.SetNext(BackendCommandSetNextInner(source("c", mixIn = false))))
        assertEquals(C.TIME_END_OF_SOURCE, clipEnd())
        assertEquals("a", backend.player.currentMediaItem!!.mediaId)
        assertEquals(30_000L, backend.player.currentPosition)
        assertEquals(listOf("a", "c"), (0 until backend.player.mediaItemCount).map { backend.player.getMediaItemAt(it).mediaId })
        // Unclipped, a plan coming back can't mix (A's end would play twice): gapless.
        backend.handle(BackendCommand.SetNext(BackendCommandSetNextInner(source("d"))))
        assertEquals(C.TIME_END_OF_SOURCE, clipEnd())
    }

    @Test
    fun noNextUnclips() {
        load(30_000u)
        backend.handle(BackendCommand.SetNext(BackendCommandSetNextInner(null)))
        assertEquals(C.TIME_END_OF_SOURCE, clipEnd())
        assertEquals(1, backend.player.mediaItemCount)
        assertEquals(30_000L, backend.player.currentPosition)
    }

    @Test
    fun gaplessOffUnclips() {
        load(30_000u)
        backend.handle(BackendCommand.SetGapless(BackendCommandSetGaplessInner(false)))
        assertEquals(C.TIME_END_OF_SOURCE, clipEnd())
        assertEquals(1, backend.player.mediaItemCount)
    }

    @Test
    fun aLoadWithoutAPlanIsNotClipped() {
        backend.handle(BackendCommand.Load(BackendCommandLoadInner(source("a", mixIn = false), source("b", mixIn = false), 0u, false, 1u)))
        assertEquals(C.TIME_END_OF_SOURCE, clipEnd())
    }

    private fun seek(positionMs: Long) = backend.handle(BackendCommand.Seek(BackendCommandSeekInner(positionMs.toUInt())))

    private fun playlist() = (0 until backend.player.mediaItemCount).map { backend.player.getMediaItemAt(it).mediaId }

    /** Where a late fade starts after a seek to [positionMs]. */
    private fun lateAt(positionMs: Long) = positionMs + ExoBackend.LATE_TAIL_MS + ExoBackend.LATE_FADE_LEAD_MS

    @Test
    fun aSeekJustBeforeTheMixPointMixesLate() {
        // The diagnostics case: 855 ms before the tail start, too late to render the planned tail.
        load(30_000u)
        idle()
        seek(99_145)
        assertEquals(lateAt(99_145), clipEnd())
        assertEquals("a", backend.player.currentMediaItem!!.mediaId)
        assertEquals(99_145L, backend.player.currentPosition)
        assertEquals(listOf("a", "b"), playlist())
        // Another seek in the late zone (before the late render is done) moves the late fade along.
        seek(99_400)
        assertEquals(lateAt(99_400), clipEnd())
        // Far enough back, the planned mix again.
        seek(60_000)
        assertEquals(100_000L, clipEnd())
        assertEquals(60_000L, backend.player.currentPosition)
        assertEquals(listOf("a", "b"), playlist())
    }

    @Test
    fun aSeekAfterAFailedRenderPlaysAOut() {
        load(30_000u)
        idle()
        seek(99_145)
        // The guard leaves the late clip for its tail to render (here it fails, off the main thread).
        val deadline = System.currentTimeMillis() + 5_000
        while (!backend.armedRenderFailed && System.currentTimeMillis() < deadline) {
            idle()
            Thread.sleep(5)
        }
        assertTrue(backend.armedRenderFailed)
        assertEquals(lateAt(99_145), clipEnd())
        // A render that failed would fail again: no second late fade, A plays out.
        seek(99_400)
        assertEquals(C.TIME_END_OF_SOURCE, clipEnd())
        assertEquals(99_400L, backend.player.currentPosition)
    }

    @Test
    fun aSeekInsideTheRenderLeadMixesLateAtOnce() {
        // Outside the guard's 2 s margin but too close to render the planned tail: mix late now,
        // not after a second re-buffer from the guard.
        load(30_000u)
        seek(100_000 - ExoBackend.LATE_TAIL_MS - 500)
        assertEquals(lateAt(100_000 - ExoBackend.LATE_TAIL_MS - 500), clipEnd())
    }

    @Test
    fun aSeekPastTheMixPointMixesLate() {
        load(30_000u)
        seek(104_000)
        assertEquals(lateAt(104_000), clipEnd())
        assertEquals(104_000L, backend.player.currentPosition)
    }

    @Test
    fun aSeekWellBeforeTheMixPointKeepsTheClip() {
        load(30_000u)
        seek(100_000 - ExoBackend.LATE_TAIL_MS - ExoBackend.LATE_FADE_LEAD_MS - 1)
        assertEquals(100_000L, clipEnd())
    }

    @Test
    fun aLoadJustBeforeTheMixPointMixesLate() {
        load(99_000u)
        val deadline = System.currentTimeMillis() + 5_000
        while (clipEnd() == 100_000L && System.currentTimeMillis() < deadline) {
            idle()
            Thread.sleep(5)
        }
        assertEquals(lateAt(99_000), clipEnd())
        assertEquals(99_000L, backend.player.currentPosition)
    }

    @Test
    fun aSeekWithNoRoomForALateFadePlaysAToItsEnd() {
        ffi.lateFades = false
        load(30_000u)
        seek(99_145)
        assertEquals(C.TIME_END_OF_SOURCE, clipEnd())
        assertEquals(99_145L, backend.player.currentPosition)
        // Unclipped, it stays so: A's end would play twice.
        seek(50_000)
        assertEquals(C.TIME_END_OF_SOURCE, clipEnd())
    }

    @Test
    fun noTailNearTheClipPointUnclips() {
        // Within LATE_TAIL_MS of the clip point, the render failed and no room for a late fade: A plays to its end.
        ffi.lateFades = false
        load((100_000 - ExoBackend.LATE_TAIL_MS + 500).toUInt())
        val deadline = System.currentTimeMillis() + 5_000
        while (clipEnd() != C.TIME_END_OF_SOURCE && System.currentTimeMillis() < deadline) {
            idle()
            Thread.sleep(5)
        }
        assertEquals(C.TIME_END_OF_SOURCE, clipEnd())
        assertEquals(100_000 - ExoBackend.LATE_TAIL_MS + 500, backend.player.currentPosition)
        assertEquals(listOf("a", "b"), (0 until backend.player.mediaItemCount).map { backend.player.getMediaItemAt(it).mediaId })
    }
}
