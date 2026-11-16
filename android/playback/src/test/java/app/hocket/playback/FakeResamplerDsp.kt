package app.hocket.playback

import java.nio.ByteBuffer

/**
 * A Rust-free [DspEngine] for JVM tests: a passthrough chain, and resamplers with the native one's
 * contract (`crates/hocket-android/src/resampler.rs`) on a linear kernel: output frame `j` is the
 * input at `j × in / out` exactly, one frame of lookahead is held back until [resamplerFinish], and
 * [resamplerPrime] / [resamplerFinish] take history / lookahead that is never output itself.
 */
class FakeResamplerDsp : DspEngine {
    private class Stream(val inRate: Long, val outRate: Long, val channels: Int) {
        val input = ArrayList<Float>()
        /** Input frames of the stream (primed history included). */
        var end = 0L
        var finished = false
        /** Next output frame's position, in units of `1 / outRate` input frames. */
        var pos = 0L
    }

    private val streams = HashMap<Long, Stream>()
    private var next = 1L
    var created = 0
        private set
    val live: Int get() = streams.size

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

    override fun resamplerNew(inRate: Int, outRate: Int, channels: Int): Long {
        created++
        return next++.also { streams[it] = Stream(inRate.toLong(), outRate.toLong(), channels) }
    }

    override fun resamplerFree(handle: Long) {
        check(streams.remove(handle) != null) { "freed twice" }
    }

    override fun resamplerPrime(handle: Long, buffer: ByteBuffer, samples: Int) {
        val s = streams.getValue(handle)
        check(s.end == 0L) { "primed after input" }
        append(s, buffer, samples)
        s.end = (s.input.size / s.channels).toLong()
        s.pos = s.end * s.outRate
    }

    override fun resamplerFinish(handle: Long, buffer: ByteBuffer, samples: Int) {
        val s = streams.getValue(handle)
        append(s, buffer, samples)
        s.finished = true
    }

    override fun resamplerProcess(handle: Long, input: ByteBuffer, inSamples: Int, output: ByteBuffer, outOffset: Int, outCapacity: Int): Int {
        val s = streams.getValue(handle)
        if (!s.finished) {
            append(s, input, inSamples)
            s.end = (s.input.size / s.channels).toLong()
        }
        val have = (s.input.size / s.channels).toLong()
        var at = outOffset
        while (at + s.channels <= outCapacity) {
            val i = s.pos / s.outRate
            val ready = if (s.finished) i < s.end else i + 1 < s.end
            if (!ready) break
            val frac = (s.pos % s.outRate).toFloat() / s.outRate
            for (c in 0 until s.channels) {
                fun x(k: Long) = if (k < have) s.input[(k * s.channels + c).toInt()] else 0f
                output.putFloat(at * 4, x(i) + frac * (x(i + 1) - x(i)))
                at++
            }
            s.pos += s.inRate
        }
        return at - outOffset
    }

    private fun append(s: Stream, buffer: ByteBuffer, samples: Int) {
        for (i in 0 until samples / s.channels * s.channels) s.input += buffer.getFloat(i * 4)
    }
}
