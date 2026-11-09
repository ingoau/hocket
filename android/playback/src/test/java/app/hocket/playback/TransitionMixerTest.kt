package app.hocket.playback

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import java.nio.ByteBuffer
import kotlin.math.pow

/** [TransitionMixer]'s boundaries, gain and mixing on a fake, Rust-free [DspEngine] (plain JVM). */
class TransitionMixerTest {
    /**
     * Gain in dB as a multiplier; the shaper halves B over its 400-frame overlap (like
     * `IncomingShaper`, it leaves B alone after it); the limiter clamps to ±0.9 (so it shows).
     */
    private class FakeDsp : DspEngine {
        var gainDb = 0.0
        var resets = 0
        val settings = mutableListOf<String>()
        var shapersLive = 0
        private var shaped = 0

        override fun chainNew(rate: Int, channels: Int) = 1L
        override fun chainFree(handle: Long) = Unit
        override fun chainSetFormat(handle: Long, rate: Int, channels: Int) = Unit
        override fun chainSetAudioSettings(handle: Long, json: String): Boolean {
            settings += json
            return true
        }
        override fun chainSetTrackGain(handle: Long, gainDb: Double) {
            this.gainDb = gainDb
        }
        override fun chainReset(handle: Long) {
            resets++
        }
        var processed = 0
        override fun chainIsBypassed(handle: Long) = gainDb == 0.0
        override fun chainProcess(handle: Long, buffer: ByteBuffer, samples: Int) {
            processed += samples
            val g = 10.0.pow(gainDb / 20).toFloat()
            for (i in 0 until samples) buffer.putFloat(i * 4, buffer.getFloat(i * 4) * g)
        }
        override fun shaperNew(planJson: String, channels: Int, rate: Int): Long {
            shapersLive++
            shaped = 0
            return if (planJson == "bad") 0L else 7L
        }
        override fun shaperFree(handle: Long) {
            shapersLive--
        }
        override fun shaperProcess(handle: Long, buffer: ByteBuffer, samples: Int) {
            for (i in 0 until samples) if (shaped++ < 400) buffer.putFloat(i * 4, buffer.getFloat(i * 4) * 0.5f)
        }
        override fun limit(buffer: ByteBuffer, samples: Int) {
            for (i in 0 until samples) buffer.putFloat(i * 4, buffer.getFloat(i * 4).coerceIn(-0.9f, 0.9f))
        }
    }

    private val dsp = FakeDsp()
    private val control = MixControl()
    private val started = mutableListOf<String>()
    private val skipped = mutableListOf<String>()
    private val mixer = TransitionMixer(dsp, control, object : MixListener {
        override fun onMixStarted(armed: MixControl.Armed) {
            started += "${armed.from}->${armed.to}"
        }
        override fun onMixSkipped(armed: MixControl.Armed, reason: String) {
            skipped += reason
        }
    })

    /** Mono at 1 kHz: a frame is a millisecond. */
    init {
        mixer.configure(1_000, 1)
    }

    private fun feed(vararg samples: Float): FloatArray {
        val work = mixer.ensureWork(samples.size)
        samples.forEachIndexed { i, s -> work.putFloat(i * 4, s) }
        val n = mixer.process(samples.size)
        return FloatArray(n) { mixer.output.getFloat(it * 4) }
    }

    private fun ones(n: Int) = FloatArray(n) { 1f }

    private fun arm(tail: MixControl.Tail?, planJson: String = "{}"): MixControl.Armed {
        val armed = MixControl.Armed("a", "b", planJson, incomingAtMs = 100, incomingFromMs = 50, tailLenMs = 500, handoverMs = 300, tailStartMs = 1_000, tailSourceMs = 500)
        armed.tail = tail
        control.armed = armed
        return armed
    }

    private fun tail(frames: Int = 500, value: Float = 0.25f) = MixControl.Tail(FloatArray(frames) { value }, 1_000, 1)

    @Test
    fun gainSwitchesAtTheFirstFrameOfTheNextItem() {
        control.items["a"] = MixControl.Item(-6.0, null)
        control.items["b"] = MixControl.Item(6.0, null)
        mixer.startItem("a", 0, contiguous = false)
        assertEquals(0.501f, feed(*ones(10)).last(), 0.01f)
        mixer.startItem("b", 0, contiguous = true)
        val out = feed(*ones(10))
        assertEquals("positive gain is applied, not clamped to unity", 1.995f, out.first(), 0.01f)
        assertEquals("a gapless join keeps the filter state", 1, dsp.resets)
    }

    @Test
    fun framesPastTheClipEndAreDropped() {
        control.items["a"] = MixControl.Item(0.0, 100)
        mixer.startItem("a", 40_000, contiguous = false) // from 40 ms: 60 frames left
        assertEquals(50, feed(*ones(50)).size)
        assertEquals(10, feed(*ones(50)).size)
        assertEquals(0, feed(*ones(5)).size)
    }

    @Test
    fun theTailLeadsThenSumsWithShapedB() {
        control.items["a"] = MixControl.Item(0.0, 1_000)
        control.items["b"] = MixControl.Item(0.0, null)
        arm(tail())
        mixer.startItem("a", 0, contiguous = false)
        feed(*ones(1_000))
        mixer.startItem("b", 0, contiguous = true)
        assertEquals(listOf("a->b"), started)
        val out = feed(*ones(200))
        // 100 ms of tail alone, then B from 50 ms in (150 frames), shaped and summed.
        assertEquals(250, out.size)
        assertEquals(0.25f, out[0], 1e-6f)
        assertEquals(0.25f, out[99], 1e-6f)
        assertEquals(0.75f, out[100], 1e-6f)
        // The rest of the overlap: 500 − 100 − 150 = 250 frames of tail left.
        val more = feed(*ones(300))
        assertEquals(300, more.size)
        assertEquals(0.75f, more[249], 1e-6f)
        assertEquals("after the tail B passes through unshaped", 1f, more[250], 1e-6f)
        assertEquals(0, dsp.shapersLive)
        assertTrue(!mixer.mixing)
    }

    @Test
    fun theSinksResyncFlushAtBsStartKeepsTheMix() {
        control.items["a"] = MixControl.Item(0.0, 1_000)
        control.items["b"] = MixControl.Item(0.0, null)
        arm(tail())
        mixer.startItem("a", 0, contiguous = false)
        feed(*ones(1_000))
        // DefaultAudioSink at a gapless join: drain + flush for B's format, then drain + flush again
        // for the timing resync the renderer's `handleDiscontinuity` asked for.
        mixer.startItem("b", 0, contiguous = true)
        mixer.startItem("b", 0, contiguous = true)
        assertEquals(listOf("a->b"), started)
        assertTrue(mixer.mixing)
        val out = feed(*ones(200))
        assertEquals(250, out.size)
        assertEquals(0.25f, out[0], 1e-6f)
        assertEquals(0.75f, out[100], 1e-6f)
    }

    @Test
    fun theLimiterRunsAfterTheSum() {
        control.items["b"] = MixControl.Item(0.0, null)
        arm(tail(value = 0.8f))
        mixer.startItem("a", 0, contiguous = false)
        mixer.startItem("b", 0, contiguous = true)
        val out = feed(*ones(200))
        assertEquals(0.8f, out[0], 1e-6f)
        assertEquals("0.8 + 0.5 limited", 0.9f, out[100], 1e-6f)
    }

    @Test
    fun aTailThatIsNotReadyLeavesBUnmixed() {
        arm(null)
        mixer.startItem("a", 0, contiguous = false)
        mixer.startItem("b", 0, contiguous = true)
        assertEquals(listOf("tail not ready"), skipped)
        assertEquals(200, feed(*ones(200)).size)
    }

    @Test
    fun aTailAtAnotherFormatOrAnUnreadablePlanIsSkipped() {
        arm(MixControl.Tail(FloatArray(1_000), 44_100, 1))
        mixer.startItem("a", 0, contiguous = false)
        mixer.startItem("b", 0, contiguous = true)
        arm(tail(), planJson = "bad")
        mixer.startItem("a", 0, contiguous = false)
        mixer.startItem("b", 0, contiguous = true)
        assertEquals(2, skipped.size)
        assertTrue(started.isEmpty())
    }

    @Test
    fun aSeekIntoBNeverMixes() {
        arm(tail())
        mixer.startItem("a", 0, contiguous = false)
        mixer.startItem("b", 0, contiguous = false)
        assertTrue(started.isEmpty())
        assertEquals(2, dsp.resets)
    }

    @Test
    fun anotherPairDoesNotMix() {
        arm(tail())
        mixer.startItem("x", 0, contiguous = false)
        mixer.startItem("b", 0, contiguous = true)
        assertTrue(started.isEmpty() && skipped.isEmpty())
    }

    @Test
    fun cancelDropsTheRestOfTheTail() {
        arm(tail())
        mixer.startItem("a", 0, contiguous = false)
        mixer.startItem("b", 0, contiguous = true)
        assertEquals(250, feed(*ones(200)).size)
        control.cancelMix()
        val out = feed(*ones(10))
        assertEquals(10, out.size)
        assertEquals(1f, out[0], 1e-6f)
        assertEquals(0, dsp.shapersLive)
    }

    @Test
    fun audioSettingsApplyOncePerChange() {
        control.audioSettingsJson = "{eq}"
        mixer.startItem("a", 0, contiguous = false)
        feed(*ones(4))
        feed(*ones(4))
        control.audioSettingsJson = "{eq2}"
        feed(*ones(4))
        assertEquals(listOf("{eq}", "{eq2}"), dsp.settings)
    }

    @Test
    fun aPassthroughChainSkipsTheBufferUntilAClipOrAMix() {
        control.items["a"] = MixControl.Item(0.0, 100)
        control.items["b"] = MixControl.Item(0.0, null)
        mixer.startItem("a", 0, contiguous = false)
        assertTrue("0 dB, nothing clipped yet: passed through", mixer.passThrough(60))
        assertFalse("the clip end falls in this buffer", mixer.passThrough(60))
        assertEquals("the passed-through frames still count towards the clip", 40, feed(*ones(60)).size)
        arm(tail())
        mixer.startItem("b", 0, contiguous = true)
        assertFalse("a mix runs", mixer.passThrough(10))
        control.items["c"] = MixControl.Item(-6.0, null)
        mixer.startItem("c", 0, contiguous = false)
        assertFalse("a gain to apply", mixer.passThrough(10))
        assertEquals("only the frames before the clip went through the chain", 40, dsp.processed)
    }

    @Test
    fun pcm16ConversionClamps() {
        assertEquals(32767.toShort(), MixProcessor.toPcm16(1.5f))
        assertEquals((-32768).toShort(), MixProcessor.toPcm16(-2f))
        assertEquals(16384.toShort(), MixProcessor.toPcm16(0.5f))
        // Rounded, not truncated: a 16-bit sample survives the float round trip exactly, and a
        // value just under a step lands on it.
        assertEquals(1.toShort(), MixProcessor.toPcm16(0.9f / 32768f))
        assertEquals((-1).toShort(), MixProcessor.toPcm16(-0.9f / 32768f))
        for (v in listOf(-32768, -12345, -1, 0, 1, 12345, 32767)) assertEquals(v.toShort(), MixProcessor.toPcm16(v / 32768f))
        assertEquals(0.toShort(), MixProcessor.toPcm16(Float.NaN))
    }
}
