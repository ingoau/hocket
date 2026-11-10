package app.hocket.playback

import androidx.media3.common.C
import androidx.media3.common.audio.AudioProcessor
import java.nio.ByteBuffer
import java.nio.ByteOrder
import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/** [OutputRate] and its place in [MixProcessor], on [FakeResamplerDsp] (plain JVM). */
class OutputRateTest {
    private val dsp = FakeResamplerDsp()

    private fun floats(values: FloatArray): ByteBuffer {
        val b = ByteBuffer.allocateDirect(values.size * 4).order(ByteOrder.nativeOrder())
        values.forEachIndexed { i, v -> b.putFloat(i * 4, v) }
        return b
    }

    private fun OutputRate.feed(values: FloatArray): FloatArray {
        val n = process(floats(values), values.size / channels)
        return FloatArray(n * channels) { output.getFloat(it * 4) }
    }

    private fun OutputRate.end(lookahead: FloatArray? = null): FloatArray {
        val ch = channels
        val n = finish(lookahead)
        return FloatArray(n * ch) { output.getFloat(it * 4) }
    }

    /** A ramp, so every frame is distinct and a linear kernel reproduces it exactly. */
    private fun ramp(from: Int, n: Int) = FloatArray(n) { (from + it) / 1_000f }

    /** [input] at 3 Hz converted to 4 Hz in one stream, finished with [lookahead]. */
    private fun whole(input: FloatArray, lookahead: FloatArray? = null): FloatArray {
        val r = OutputRate(FakeResamplerDsp(), 4)
        r.latch(4)
        r.start(3, 1, joined = false, lead = null)
        return r.feed(input) + r.end(lookahead)
    }

    @Test
    fun theFirstStreamAt44Or48FixesTheRateElseTheDevicesIs() {
        assertEquals(44_100, OutputRate(dsp, 48_000).latch(44_100))
        val r = OutputRate(dsp, 48_000)
        assertEquals(48_000, r.latch(96_000))
        assertEquals(48_000, r.latch(44_100))
        r.release()
        assertEquals(44_100, r.latch(44_100))
    }

    @Test
    fun aStreamAtTheOutputRatePassesThrough() {
        val r = OutputRate(dsp, 48_000)
        r.latch(44_100)
        r.start(44_100, 2, joined = false, lead = null)
        assertFalse(r.resampling)
        assertEquals(0, dsp.created)
        r.start(48_000, 2, joined = true, lead = null)
        assertTrue(r.resampling)
    }

    @Test
    fun aGaplessJoinAtTheSameFormatGoesOnThroughTheSameResampler() {
        val r = OutputRate(dsp, 4)
        r.latch(4)
        r.start(3, 1, joined = false, lead = null)
        val a = r.feed(ramp(0, 30))
        r.start(3, 1, joined = true, lead = null)
        val b = r.feed(ramp(30, 30))
        assertEquals(1, dsp.created)
        val all = a + b + r.end()
        assertArrayEquals(whole(ramp(0, 60)), all, 0f)
        assertEquals(80, all.size)
    }

    @Test
    fun aSeekStartsANewStream() {
        val r = OutputRate(dsp, 4)
        r.latch(4)
        r.start(3, 1, joined = false, lead = null)
        r.feed(ramp(0, 30))
        r.start(3, 1, joined = false, lead = null)
        assertEquals(2, dsp.created)
        assertEquals(1, dsp.live)
        // Output frame 0 of the new stream is its input frame 0.
        assertEquals(0.5f, r.feed(ramp(500, 3))[0], 0f)
    }

    @Test
    fun aStreamEndsExactlyAndWithTheLookaheadLikeAnUncutOne() {
        val r = OutputRate(dsp, 4)
        r.latch(4)
        r.start(3, 1, joined = false, lead = null)
        assertTrue(r.endsBefore(4, 1))
        assertFalse(r.endsBefore(3, 1))
        val out = r.feed(ramp(0, 30)) + r.end(ramp(30, 5))
        // 30 frames at 3 Hz are 40 at 4 Hz, the last one using the continuation.
        assertArrayEquals(whole(ramp(0, 40)).copyOf(40), out, 1e-6f)
        assertEquals(0, dsp.live)
        assertFalse(r.resampling)
    }

    @Test
    fun aLeadPrimesTheNextStreamSoItStartsWhereTheOldOneStopped() {
        val r = OutputRate(dsp, 4)
        r.latch(4)
        r.start(4, 1, joined = false, lead = null)
        assertFalse(r.resampling)
        // B at 3 Hz opens with a tail whose lead is the 3 frames before it.
        r.start(3, 1, joined = true, lead = ramp(27, 3))
        val out = r.feed(ramp(30, 6)) + r.end()
        // Exactly the uncut stream's frames from frame 30 (output frame 40) on.
        assertArrayEquals(whole(ramp(0, 36)).copyOfRange(40, 48), out, 1e-6f)
        // The fake's kernel only looks ahead, so this is about position: B's first output frame is
        // its first input frame, not a lead frame.
        assertEquals(0.03f, out[0], 1e-6f)
    }

    // -- In MixProcessor ---------------------------------------------------------------------

    private val processor = MixProcessor(dsp, MixControl(), object : MixListener {
        override fun onMixStarted(armed: MixControl.Armed) = Unit
        override fun onMixSkipped(armed: MixControl.Armed, reason: String) = Unit
    }, fallbackRate = 48_000)

    private fun pcm16(rate: Int) = AudioProcessor.AudioFormat(rate, 1, C.ENCODING_PCM_16BIT)

    private fun queue(frames: Int): Int {
        val b = ByteBuffer.allocateDirect(frames * 2).order(ByteOrder.nativeOrder())
        for (i in 0 until frames) b.putShort(i * 2, 1_000)
        processor.queueInput(b)
        return processor.output.remaining() / 2
    }

    @Test
    fun theOutputRateStaysFixedAcrossItemsAtOtherRates() {
        assertEquals(48_000, processor.configure(pcm16(48_000)).sampleRate)
        processor.flush(AudioProcessor.StreamMetadata.DEFAULT)
        assertEquals(480, queue(480))
        assertEquals(pcm16(48_000), processor.configure(pcm16(44_100)))
        assertEquals(pcm16(48_000), processor.configure(pcm16(96_000)))
    }

    @Test
    fun aResampledStreamComesOutWholeBeforeAFormatChange() {
        processor.configure(pcm16(48_000))
        processor.flush(AudioProcessor.StreamMetadata.DEFAULT)
        processor.configure(pcm16(44_100))
        processor.queueEndOfStream()
        processor.flush(AudioProcessor.StreamMetadata.DEFAULT)
        // A at 44.1 kHz into 48 kHz: one frame held back for lookahead.
        assertEquals(479, queue(441))
        // B at 48 kHz is configured while A plays; A's end of stream lets out its last frame.
        processor.configure(pcm16(48_000))
        processor.queueEndOfStream()
        assertFalse("the held-back frame is still to come", processor.isEnded)
        processor.queueInput(ByteBuffer.allocateDirect(0))
        assertEquals(1, processor.output.remaining() / 2)
        assertTrue(processor.isEnded)
        processor.flush(AudioProcessor.StreamMetadata.DEFAULT)
        assertEquals(0, dsp.live)
        assertEquals(100, queue(100))
    }

    @Test
    fun anEndOfStreamAtTheSameFormatHoldsTheResamplerOpen() {
        processor.configure(pcm16(48_000))
        processor.flush(AudioProcessor.StreamMetadata.DEFAULT)
        processor.configure(pcm16(44_100))
        processor.queueEndOfStream()
        processor.flush(AudioProcessor.StreamMetadata.DEFAULT)
        queue(441)
        processor.queueEndOfStream()
        assertTrue(processor.isEnded)
        processor.flush(AudioProcessor.StreamMetadata.DEFAULT)
        assertEquals(1, dsp.live)
        // The held-back frame comes out with the next item's first: 147 frames in, 160 out (one
        // held back again).
        assertEquals(160, queue(147))
    }

    @Test
    fun theLastStreamOfThePlaylistComesOutWhole() {
        processor.configure(pcm16(48_000))
        processor.flush(AudioProcessor.StreamMetadata.DEFAULT)
        processor.configure(pcm16(44_100))
        processor.queueEndOfStream()
        processor.flush(AudioProcessor.StreamMetadata.DEFAULT)
        assertEquals(479, queue(441))
        processor.playToEnd()
        processor.queueEndOfStream()
        assertFalse(processor.isEnded)
        processor.queueInput(ByteBuffer.allocateDirect(0))
        assertEquals(1, processor.output.remaining() / 2)
        assertTrue(processor.isEnded)
    }

    @Test
    fun aLeadStillPrimesAStreamThatHasNotStarted() {
        val r = OutputRate(dsp, 4)
        r.latch(4)
        r.start(4, 1, joined = false, lead = null)
        // A first flush for the new format without the mix, then the one that starts it.
        r.start(3, 1, joined = true, lead = null)
        r.start(3, 1, joined = true, lead = ramp(27, 3))
        val out = r.feed(ramp(30, 6)) + r.end()
        assertArrayEquals(whole(ramp(0, 36)).copyOfRange(40, 48), out, 1e-6f)
        // Once it has had input, a lead is too late.
        r.start(3, 1, joined = false, lead = null)
        r.feed(ramp(0, 3))
        r.start(3, 1, joined = true, lead = ramp(27, 3))
        assertEquals(1, dsp.live)
    }
}
