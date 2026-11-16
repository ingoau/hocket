package app.hocket.playback

import java.nio.ByteBuffer
import java.nio.ByteOrder

/**
 * The last stage of [MixProcessor]: everything leaves at one sample rate (`docs/automix.md` →
 * Android → Output rate), so the sink never rebuilds its `AudioTrack` between items. A rebuild means
 * the old track plays out to silence before the new one starts: a gap at every join between
 * tracks at different rates, and in the middle of A's audio when AutoMix mixes across rates.
 *
 * - **The rate** is the first stream's after a reset when that is 44.1 or 48 kHz, else [fallbackRate]
 *   (the device's). A library at one rate is never resampled; the rest is converted to it.
 * - **Streams**: [start] at every flush. A stream joined gaplessly at the same format goes on through
 *   the same resampler (no seam inside continuous audio); anything else starts a new one.
 * - **Seams**: when the next stream is at another format, the old one [finish]es at its end of
 *   stream, with A's real continuation as lookahead when a mix follows; the new one is primed with
 *   the mix tail's lead. So a mix across rates is as seamless as one at a single rate.
 *
 * Free of Media3 so JVM tests can drive it; floats in direct buffers, native order.
 */
class OutputRate(private val dsp: DspEngine, private val fallbackRate: Int) {
    /** The rate everything leaves at; 0 until the first [latch]. */
    var outRate = 0
        private set
    var inRate = 0
        private set
    var channels = 0
        private set
    private var handle = 0L
    /** The resampler hasn't had input yet (a lead can still prime it). */
    private var fresh = false

    /** Whether the current stream is converted (else it passes through as is). */
    val resampling: Boolean get() = handle != 0L

    /** Output floats of the last [process] or [finish], from index 0. */
    var output: ByteBuffer = floatBuffer(4096)
        private set
    private var scratch: ByteBuffer = floatBuffer(1024)

    /** The output rate for a stream at [rate], fixed by the first stream after a [release]. */
    fun latch(rate: Int): Int {
        if (outRate == 0) outRate = if (rate == 44_100 || rate == 48_000 || fallbackRate <= 0) rate else fallbackRate
        return outRate
    }

    /**
     * A stream (re)starts at [rate] × [channels]: [joined] when it follows the previous one's end
     * without a seek. [lead] (at this stream's format) is the audio just before it, when a mix tail
     * opens it: history for the resampler, not output.
     */
    fun start(rate: Int, channels: Int, joined: Boolean, lead: FloatArray?) {
        if (outRate == 0) latch(rate)
        if (joined && rate == inRate && channels == this.channels) {
            // A stream that hasn't started yet can still take a lead: at a change of channel count
            // the sink flushes twice before the first frame, and only the second starts the mix.
            if (handle != 0L && fresh && lead != null && lead.size >= channels) prime(lead)
            return
        }
        free()
        inRate = rate
        this.channels = channels
        if (rate == outRate || rate <= 0) return
        handle = dsp.resamplerNew(rate, outRate, channels)
        if (handle == 0L) {
            PlaybackLog.w("no resampler for $rate Hz -> $outRate Hz: plays at the wrong speed")
            return
        }
        fresh = true
        if (joined && lead != null && lead.size >= channels) prime(lead)
    }

    private fun prime(lead: FloatArray) {
        val n = lead.size / channels * channels
        dsp.resamplerPrime(handle, fill(lead, n), n)
        fresh = false
    }

    /** Whether the stream must end before a next one at [rate] × [channels]: it is converted, and differently. */
    fun endsBefore(rate: Int, channels: Int): Boolean = handle != 0L && (rate != inRate || channels != this.channels)

    /**
     * End the stream: its last frames into [output], with [lookahead] (at its format: the audio that
     * would follow) informing them. Returns the frames written. The next [start] begins anew.
     */
    fun finish(lookahead: FloatArray?): Int {
        if (handle == 0L) return 0
        val ch = channels.coerceAtLeast(1)
        val n = if (lookahead != null) lookahead.size / ch * ch else 0
        dsp.resamplerFinish(handle, fill(lookahead ?: FloatArray(0), n), n)
        val frames = drain(scratch, 0, 64)
        free()
        inRate = 0
        return frames
    }

    /** Convert [frames] input frames from [input]; returns the frames written to [output]. */
    fun process(input: ByteBuffer, frames: Int): Int {
        fresh = false
        return drain(input, frames * channels, frames)
    }

    fun release() {
        free()
        outRate = 0
        inRate = 0
        channels = 0
    }

    private fun drain(input: ByteBuffer, inSamples: Int, inFrames: Int): Int {
        val ch = channels.coerceAtLeast(1)
        // What `inFrames` make plus what the kernel held back; [resamplerProcess] is called again
        // while it fills the buffer, so this is only the usual case.
        val want = ((inFrames.toLong() * outRate / inRate.coerceAtLeast(1)) + SLACK_FRAMES).toInt() * ch
        ensureOutput(want, 0)
        var written = dsp.resamplerProcess(handle, input, inSamples, output, 0, output.capacity() / 4)
        while (written >= output.capacity() / 4 - ch) {
            ensureOutput(output.capacity() / 4 * 2, written)
            val more = dsp.resamplerProcess(handle, input, 0, output, written, output.capacity() / 4)
            if (more == 0) break
            written += more
        }
        return written / ch
    }

    private fun free() {
        if (handle != 0L) dsp.resamplerFree(handle)
        handle = 0L
    }

    private fun fill(samples: FloatArray, n: Int): ByteBuffer {
        if (scratch.capacity() < n * 4) scratch = floatBuffer(n)
        for (i in 0 until n) scratch.putFloat(i * 4, samples[i])
        return scratch
    }

    /** Make [output] hold [samples] floats, keeping its first [keep]. */
    private fun ensureOutput(samples: Int, keep: Int) {
        if (output.capacity() >= samples * 4) return
        val grown = floatBuffer(samples)
        for (i in 0 until keep) grown.putFloat(i * 4, output.getFloat(i * 4))
        output = grown
    }

    private companion object {
        /** More than the widest kernel holds back (256 input frames). */
        const val SLACK_FRAMES = 600L

        fun floatBuffer(samples: Int): ByteBuffer = ByteBuffer.allocateDirect(samples.coerceAtLeast(1) * 4).order(ByteOrder.nativeOrder())
    }
}
