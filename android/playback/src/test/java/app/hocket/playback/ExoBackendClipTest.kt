package app.hocket.playback

import android.app.Application
import android.os.Looper
import androidx.media3.common.C
import androidx.test.core.app.ApplicationProvider
import androidx.test.ext.junit.runners.AndroidJUnit4
import app.hocket.core.api.AnalysisRecord
import app.hocket.core.api.BackendCommand
import app.hocket.core.api.BackendCommandLoadInner
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

    /** Every render fails, like a tail that never gets ready. */
    private object FailingFfi : AutomixFfi {
        override fun usable(record: AnalysisRecord, durationMs: Long) = true
        override fun loudnessRecord(head: PcmDecoder.Pcm, tail: PcmDecoder.Pcm, tailStartMs: Long, durationMs: Long, transcoded: Boolean): AnalysisRecord? = null
        override fun prepareTail(planJson: String, pcm: PcmDecoder.Pcm, contextMs: Int, outChannels: Int, outRate: Int, audioSettingsJson: String?, gainDb: Double): RenderedTail =
            throw IllegalStateException("no tail")
    }

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
        backend.handle(BackendCommand.Load(BackendCommandLoadInner(source("a", mixIn = false), source("b"), positionMs, false)))

    @Before
    fun setUp() {
        backend = ExoBackend(ApplicationProvider.getApplicationContext(), scope, {}, query = { null }, dsp = PassDsp, ffi = FailingFfi)
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
        backend.handle(BackendCommand.Load(BackendCommandLoadInner(source("a", mixIn = false), source("b", mixIn = false), 0u, false)))
        assertEquals(C.TIME_END_OF_SOURCE, clipEnd())
    }

    @Test
    fun noTailNearTheClipPointUnclips() {
        // Within LATE_TAIL_MS of the clip point and the render failed: A plays to its end.
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
