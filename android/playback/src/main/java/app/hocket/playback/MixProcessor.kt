package app.hocket.playback

import androidx.media3.common.C
import androidx.media3.common.Timeline
import androidx.media3.common.audio.AudioProcessor
import androidx.media3.common.audio.BaseAudioProcessor
import java.nio.ByteBuffer
import java.nio.ByteOrder
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.atomic.AtomicInteger

/**
 * What [ExoBackend] (main thread) tells the playback thread's [MixProcessor]: per-item gain and
 * clip end, the audio settings, and the armed transition with its rendered tail. Every field is
 * safe to write from any thread; the processor picks changes up on its next buffer.
 */
class MixControl {
    /** A playlist item by media id (the queue key). */
    data class Item(val gainDb: Double, val clipEndMs: Long?)

    /**
     * A rendered tail: interleaved floats at B's rate and channel count. [lead] is A just before the
     * tail (B's format) and [after] A from the tail's start (A's format, [afterRate] ×
     * [afterChannels]): the [OutputRate] seam when A and B are converted differently.
     */
    class Tail(
        val samples: FloatArray,
        val rate: Int,
        val channels: Int,
        val lead: FloatArray = FloatArray(0),
        val after: FloatArray = FloatArray(0),
        val afterRate: Int = 0,
        val afterChannels: Int = 0,
    ) {
        val frames: Int get() = samples.size / channels.coerceAtLeast(1)
    }

    /** The transition into [to] from [from], from its plan; [tail] is set once rendered. */
    class Armed(
        val from: String,
        val to: String,
        val planJson: String,
        val incomingAtMs: Long,
        val incomingFromMs: Long,
        val tailLenMs: Long,
        val handoverMs: Long,
        val tailStartMs: Long,
        val tailSourceMs: Long,
    ) {
        @Volatile var tail: Tail? = null
    }

    val items = ConcurrentHashMap<String, Item>()
    @Volatile var armed: Armed? = null
    /** `AudioSettings` JSON for the chain (EQ, normalisation); null leaves it flat. */
    @Volatile var audioSettingsJson: String? = null
    private val cancels = AtomicInteger()

    /** Drop the running mix's remaining tail at the next buffer (`complete_transition`). */
    fun cancelMix() {
        cancels.incrementAndGet()
    }

    internal val cancelCount: Int get() = cancels.get()
}

/** What the playback thread reports; [ExoBackend] hops to the main thread. */
interface MixListener {
    /** B's first frame after A's clip point met a ready tail: the mix runs. */
    fun onMixStarted(armed: MixControl.Armed)

    /** B's first frame came but the tail wasn't usable: B plays unmixed. */
    fun onMixSkipped(armed: MixControl.Armed, reason: String)
}

/**
 * The mixing logic of [MixProcessor], free of Media3 so JVM tests can drive it. Works on interleaved
 * floats in direct buffers ([work] in, [output] out).
 *
 * - Each item's frames run through the DSP chain at the item's gain ([MixControl.Item.gainDb]); a
 *   new item ([startItem]) snaps the gain at its first frame, which is how ReplayGain switches at the
 *   exact sample of a gapless join.
 * - Frames past an item's clip end are dropped (ExoPlayer clips by whole decoder buffers).
 * - At a contiguous join from A into B with a matching armed transition and a tail at B's format:
 *   the tail's first `incoming_at_ms` are output at once, then B (its first `incoming_from_ms`
 *   dropped, through the chain and the `IncomingShaper`) is summed with the rest of the tail and
 *   limited. So the output runs `incoming_at_ms − incoming_from_ms` ahead of B's input, which
 *   [ExoBackend] maps back when it reports B's position.
 */
class TransitionMixer(
    private val dsp: DspEngine,
    private val control: MixControl,
    private val listener: MixListener,
) {
    private class Active(val armed: MixControl.Armed, val tail: MixControl.Tail, val shaper: Long, val prefixFrames: Int, var dropFrames: Long) {
        var tailPos = 0
        var prefixDone = false
    }

    private var chain = 0L
    var rate = 0
        private set
    var channels = 0
        private set
    private var settingsApplied: String? = null
    var item: String? = null
        private set
    private var itemStartUs = 0L
    private var itemFrames = 0L
    /** The last flush joined from the previous item (or continued a join) and no frame came since. */
    private var joining = false
    private var mix: Active? = null
    private var seenCancels = control.cancelCount

    /** Input floats for [process], from index 0, native order. */
    var work: ByteBuffer = floatBuffer(4096)
        private set

    /** Output floats of the last [process], from index 0. */
    var output: ByteBuffer = floatBuffer(4096)
        private set

    val mixing: Boolean get() = mix != null

    /** The running mix's tail. */
    val tail: MixControl.Tail? get() = mix?.tail

    /** The last [startItem] continued the stream before it (a gapless join, or the same item going on). */
    val joined: Boolean get() = joining

    fun configure(rate: Int, channels: Int) {
        if (chain == 0L) {
            chain = dsp.chainNew(rate, channels)
            settingsApplied = null
        } else if (rate != this.rate || channels != this.channels) {
            dsp.chainSetFormat(chain, rate, channels)
        }
        this.rate = rate
        this.channels = channels
    }

    /**
     * The stream (re)starts with [mediaId] at [positionUs]: [contiguous] when it follows the previous
     * item's end without a seek (a gapless join), else a seek or a fresh start.
     */
    fun startItem(mediaId: String?, positionUs: Long, contiguous: Boolean) {
        // Until a frame comes, a flush at the item's start still belongs to the join. When the sink
        // can't keep its `AudioTrack` for B (another channel count; the rate is always the same)
        // it drains A, flushes once still naming A (the new configuration takes over after that
        // flush), then flushes for B without another end of stream. Taken as a seek, that second
        // flush would leave B unmixed. A seek while the sink waits for A to play out lands
        // elsewhere and stays a seek.
        val joined = contiguous || (joining && itemFrames == 0L && positionUs < JOIN_START_US)
        joining = joined
        if (joined && mediaId != null && mediaId == item) {
            // The same stream going on: the sink drains and flushes again to resync its timing,
            // right after every stream change (the renderer's `handleDiscontinuity`) and before B's
            // first frame. A new item here would end the mix before a frame of the tail is heard.
            itemStartUs = positionUs.coerceAtLeast(0)
            itemFrames = 0
            applySettings()
            return
        }
        val previous = item
        endMix()
        item = mediaId
        itemStartUs = positionUs.coerceAtLeast(0)
        itemFrames = 0
        seenCancels = control.cancelCount
        applySettings()
        if (chain != 0L) {
            if (!joined) dsp.chainReset(chain)
            dsp.chainSetTrackGain(chain, mediaId?.let { control.items[it]?.gainDb } ?: 0.0)
        }
        if (!joined || previous == null || mediaId == null) return
        val armed = control.armed ?: return
        if (armed.from != previous || armed.to != mediaId) return
        val tail = armed.tail
        val reason = when {
            tail == null -> "tail not ready"
            tail.rate != rate || tail.channels != channels -> "tail at ${tail.rate} Hz × ${tail.channels}, B at $rate Hz × $channels"
            chain == 0L -> "no DSP chain"
            else -> null
        }
        if (reason != null || tail == null) {
            listener.onMixSkipped(armed, reason ?: "tail not ready")
            return
        }
        val shaper = dsp.shaperNew(armed.planJson, channels, rate)
        if (shaper == 0L) {
            listener.onMixSkipped(armed, "plan unreadable")
            return
        }
        val prefix = framesFor(armed.incomingAtMs).coerceAtMost(tail.frames.toLong()).toInt()
        mix = Active(armed, tail, shaper, prefix, framesFor(armed.incomingFromMs))
        listener.onMixStarted(armed)
    }

    /** Make [work] hold at least [samples] floats. */
    fun ensureWork(samples: Int): ByteBuffer {
        if (work.capacity() < samples * 4) work = floatBuffer(samples)
        return work
    }

    /**
     * Whether the next [frames] input frames would come out unchanged: no mix running, none past the
     * clip end, and a chain that is a passthrough (0 dB, EQ off). Then they are counted as played
     * and the caller passes its input through as is (bit-exact, no conversion, no JNI); else
     * nothing is consumed and the caller goes through [process].
     */
    fun passThrough(frames: Int): Boolean {
        applySettings()
        checkCancel()
        if (mix != null || framesBeforeClip(frames) < frames) return false
        if (chain != 0L && !dsp.chainIsBypassed(chain)) return false
        itemFrames += frames
        return true
    }

    /** Process [frames] input frames from [work]; returns the frames written to [output]. */
    fun process(frames: Int): Int {
        val ch = channels.coerceAtLeast(1)
        applySettings()
        checkCancel()
        val n = framesBeforeClip(frames)
        itemFrames += frames
        val m = mix
        if (m == null) {
            if (chain != 0L && n > 0) dsp.chainProcess(chain, work, n * ch)
            ensureOutput(n * ch)
            copy(work, 0, output, 0, n * ch)
            return n
        }
        var out = 0
        if (!m.prefixDone) {
            m.prefixDone = true
            ensureOutput((m.prefixFrames + n) * ch)
            for (i in 0 until m.prefixFrames * ch) output.putFloat(i * 4, m.tail.samples[i])
            m.tailPos = m.prefixFrames
            out = m.prefixFrames
        } else {
            ensureOutput(n * ch)
        }
        val drop = minOf(m.dropFrames, n.toLong()).toInt()
        m.dropFrames -= drop
        val b = n - drop
        if (b > 0) {
            if (drop > 0) copy(work, drop * ch, work, 0, b * ch)
            dsp.chainProcess(chain, work, b * ch)
            dsp.shaperProcess(m.shaper, work, b * ch)
            val overlap = minOf(b, m.tail.frames - m.tailPos)
            if (overlap > 0) {
                val base = m.tailPos * ch
                for (i in 0 until overlap * ch) work.putFloat(i * 4, work.getFloat(i * 4) + m.tail.samples[base + i])
                dsp.limit(work, overlap * ch)
                m.tailPos += overlap
            }
            copy(work, 0, output, out * ch, b * ch)
            out += b
        }
        if (m.tailPos >= m.tail.frames) endMix()
        return out
    }

    fun release() {
        endMix()
        if (chain != 0L) dsp.chainFree(chain)
        chain = 0L
        item = null
        joining = false
    }

    private fun endMix() {
        val m = mix ?: return
        mix = null
        dsp.shaperFree(m.shaper)
    }

    private fun checkCancel() {
        if (control.cancelCount == seenCancels) return
        seenCancels = control.cancelCount
        endMix()
    }

    private fun applySettings() {
        val json = control.audioSettingsJson
        if (chain == 0L || json == settingsApplied) return
        settingsApplied = json
        if (json != null) dsp.chainSetAudioSettings(chain, json)
    }

    /** How many of the next [frames] input frames lie before the current item's clip end. */
    private fun framesBeforeClip(frames: Int): Int {
        val first = itemFrames
        val clipMs = item?.let { control.items[it]?.clipEndMs } ?: return frames
        if (rate <= 0 || itemStartUs >= clipMs * 1000) return frames
        val end = (clipMs * 1000 - itemStartUs) * rate / 1_000_000
        return (end - first).coerceIn(0, frames.toLong()).toInt()
    }

    private fun framesFor(ms: Long): Long = ms * rate / 1000

    private fun ensureOutput(samples: Int) {
        if (output.capacity() < samples * 4) output = floatBuffer(samples)
    }

    private companion object {
        /** A flush continuing a join starts the item within this (its first sample, after encoder delay). */
        const val JOIN_START_US = 500_000L

        fun floatBuffer(samples: Int): ByteBuffer = ByteBuffer.allocateDirect(samples.coerceAtLeast(1) * 4).order(ByteOrder.nativeOrder())

        fun copy(from: ByteBuffer, fromSample: Int, to: ByteBuffer, toSample: Int, samples: Int) {
            for (i in 0 until samples) to.putFloat((toSample + i) * 4, from.getFloat((fromSample + i) * 4))
        }
    }
}

/**
 * The ExoPlayer side of [TransitionMixer] and [OutputRate] (`docs/automix.md` → Android): installed
 * in the main player's `DefaultAudioSink`, on the PCM path only (no offload, 16-bit or float in, the
 * same out).
 *
 * Item boundaries come from the sink itself: at a gapless join it drains the processors
 * (`queueEndOfStream`) and flushes them with the new stream's [AudioProcessor.StreamMetadata],
 * whose timeline and period name the playlist item; a seek flushes without an end of stream first.
 * So the first frame after such a flush is the item's first frame, sample-exact, without counting
 * frames against durations. The sink then drains and flushes once more for the same item (a timing
 * resync after every stream change); [TransitionMixer.startItem] lets that through as the same stream.
 *
 * Every item leaves at one rate ([OutputRate]), so a join at another sample rate is a gapless join
 * like any other: the sink keeps its `AudioTrack`. (A change of channel count still rebuilds it:
 * the sink drains A and flushes twice before B's first frame, the first time still naming A;
 * [TransitionMixer.startItem] takes every flush before the next frame as part of the join.)
 */
class MixProcessor(
    dsp: DspEngine,
    private val control: MixControl,
    listener: MixListener,
    /** The output rate when the first item isn't at 44.1 or 48 kHz: the device's. */
    fallbackRate: Int,
) : BaseAudioProcessor() {
    val mixer = TransitionMixer(dsp, control, listener)
    val rate = OutputRate(dsp, fallbackRate)
    private var endedBeforeFlush = false
    /** The input format the sink configured last: the next stream's, once it differs. */
    private var configured: AudioProcessor.AudioFormat? = null
    /** The stream ends at this end of stream: its resampler's last frames are still to come out. */
    private var finishPending = false
    /** The sink plays out the end of the playlist: the next end of stream is the last. */
    private var lastEnd = false
    private val period = Timeline.Period()
    private val window = Timeline.Window()

    override fun onConfigure(inputAudioFormat: AudioProcessor.AudioFormat): AudioProcessor.AudioFormat {
        if (inputAudioFormat.encoding != C.ENCODING_PCM_16BIT && inputAudioFormat.encoding != C.ENCODING_PCM_FLOAT) {
            PlaybackLog.w("DSP bypassed for PCM encoding ${inputAudioFormat.encoding}")
            return AudioProcessor.AudioFormat.NOT_SET
        }
        configured = inputAudioFormat
        val out = rate.latch(inputAudioFormat.sampleRate)
        return AudioProcessor.AudioFormat(out, inputAudioFormat.channelCount, inputAudioFormat.encoding)
    }

    /** The sink is asked to play out the end of the playlist (on the playback thread, before its drain). */
    fun playToEnd() {
        lastEnd = true
    }

    override fun onQueueEndOfStream() {
        endedBeforeFlush = true
        // The next stream is at another format (it was configured while this one played), or there
        // is none: this resampler's held-back frames are this stream's last, and must come out now.
        val next = configured
        if (rate.resampling && (lastEnd || (next != null && rate.endsBefore(next.sampleRate, next.channelCount)))) finishPending = true
    }

    override fun isEnded(): Boolean = super.isEnded() && !finishPending

    override fun onFlush(streamMetadata: AudioProcessor.StreamMetadata) {
        finishPending = false
        lastEnd = false
        mixer.configure(inputAudioFormat.sampleRate, inputAudioFormat.channelCount)
        mixer.startItem(mediaIdOf(streamMetadata), streamMetadata.positionOffsetUs, endedBeforeFlush)
        val lead = mixer.tail?.takeIf { mixer.joined }?.lead
        rate.start(inputAudioFormat.sampleRate, inputAudioFormat.channelCount, mixer.joined, lead)
        endedBeforeFlush = false
    }

    override fun onReset() {
        mixer.release()
        rate.release()
        configured = null
        endedBeforeFlush = false
        finishPending = false
        lastEnd = false
    }

    override fun queueInput(inputBuffer: ByteBuffer) {
        val format = inputAudioFormat
        val float = format.encoding == C.ENCODING_PCM_FLOAT
        if (finishPending) {
            // After the end of stream the pipeline asks for more with an empty buffer: the last frames.
            finishPending = false
            val n = finish()
            emit(rate.output, n, format.channelCount, float)
            return
        }
        val frames = inputBuffer.remaining() / format.bytesPerFrame
        if (frames == 0) return
        val pass = mixer.passThrough(frames)
        if (pass && !rate.resampling) {
            // Nothing to do (no mix, nothing clipped, a passthrough chain, the output rate): the input as is.
            replaceOutputBuffer(inputBuffer.remaining()).put(inputBuffer).flip()
            return
        }
        val samples = frames * format.channelCount
        val work = mixer.ensureWork(samples)
        val start = inputBuffer.position()
        if (float) {
            for (i in 0 until samples) work.putFloat(i * 4, inputBuffer.getFloat(start + i * 4))
        } else {
            for (i in 0 until samples) work.putFloat(i * 4, inputBuffer.getShort(start + i * 2) / 32768f)
        }
        inputBuffer.position(inputBuffer.limit())
        var src = work
        var n = frames
        if (!pass) {
            n = mixer.process(frames)
            src = mixer.output
        }
        if (rate.resampling) {
            n = rate.process(src, n)
            src = rate.output
        }
        emit(src, n, format.channelCount, float)
    }

    /** The stream's last frames, with A's continuation as lookahead when the armed mix goes on from here. */
    private fun finish(): Int {
        val armed = control.armed
        val tail = armed?.tail
        val after = tail?.takeIf {
            armed.from == mixer.item && it.afterRate == rate.inRate && it.afterChannels == rate.channels
        }?.after
        return rate.finish(after)
    }

    private fun emit(src: ByteBuffer, frames: Int, channels: Int, float: Boolean) {
        val samples = frames * channels
        val out = replaceOutputBuffer(samples * if (float) 4 else 2)
        if (float) {
            for (i in 0 until samples) out.putFloat(src.getFloat(i * 4))
        } else {
            for (i in 0 until samples) out.putShort(toPcm16(src.getFloat(i * 4)))
        }
        out.flip()
    }

    private fun mediaIdOf(meta: AudioProcessor.StreamMetadata): String? = try {
        val uid = meta.periodUid
        if (uid == null || meta.timeline.isEmpty) null
        else meta.timeline.getWindow(meta.timeline.getPeriodByUid(uid, period).windowIndex, window).mediaItem.mediaId
    } catch (e: RuntimeException) {
        null
    }

    internal companion object {
        /** Rounded to the nearest step (a truncation would bias every sample towards zero); NaN is silence. */
        fun toPcm16(x: Float): Short = Math.round(x * 32768f).coerceIn(-32768, 32767).toShort()
    }
}
