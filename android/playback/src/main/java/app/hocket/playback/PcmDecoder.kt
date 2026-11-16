package app.hocket.playback

import android.media.AudioFormat
import android.media.MediaCodec
import android.media.MediaDataSource
import android.media.MediaExtractor
import android.media.MediaFormat
import android.net.Uri
import app.hocket.core.CoreStreams
import java.io.IOException
import java.nio.ByteOrder

/**
 * Decodes a stretch of a track to interleaved float PCM with `MediaExtractor` + `MediaCodec`, for
 * AutoMix: A's tail before a transition and a track's head and tail for a loudness record. Blocks:
 * run it on a background thread. `hocket-stream://` URLs are read through the core
 * ([CoreStreamMediaDataSource]), `file:` from disk, anything else over HTTP with its headers.
 */
class PcmDecoder(private val streams: () -> CoreStreams?) {
    class Pcm(val samples: FloatArray, val rate: Int, val channels: Int) {
        val frames: Int get() = samples.size / channels.coerceAtLeast(1)
        val durationMs: Long get() = if (rate > 0) frames * 1000L / rate else 0

        /** Little-endian float bytes, what the UniFFI calls take. */
        fun leBytes(): ByteArray {
            val b = java.nio.ByteBuffer.allocate(samples.size * 4).order(ByteOrder.LITTLE_ENDIAN)
            b.asFloatBuffer().put(samples)
            return b.array()
        }
    }

    /** The decoded format of [url]'s audio track as the container states it: (rate, channels). */
    fun probe(url: String, headers: Map<String, String>): Pair<Int, Int> {
        val (extractor, source) = open(url, headers)
        try {
            val format = extractor.getTrackFormat(audioTrack(extractor))
            return format.getInteger(MediaFormat.KEY_SAMPLE_RATE) to format.getInteger(MediaFormat.KEY_CHANNEL_COUNT)
        } finally {
            extractor.release()
            source?.close()
        }
    }

    /**
     * [lengthMs] of [url] from [fromMs] (to the end when shorter), sample-exact by the decoder's
     * timestamps. [cancelled] is checked between buffers; a cancel throws [InterruptedException].
     */
    fun decode(url: String, headers: Map<String, String>, fromMs: Long, lengthMs: Long, cancelled: () -> Boolean = { false }): Pcm {
        val (extractor, source) = open(url, headers)
        var codec: MediaCodec? = null
        try {
            val track = audioTrack(extractor)
            extractor.selectTrack(track)
            val format = extractor.getTrackFormat(track)
            var rate = format.getInteger(MediaFormat.KEY_SAMPLE_RATE)
            var channels = format.getInteger(MediaFormat.KEY_CHANNEL_COUNT)
            var float = false
            val fromUs = fromMs.coerceAtLeast(0) * 1000
            // A little preroll: audio frames are all sync samples, but a seek can land just after.
            extractor.seekTo((fromUs - PREROLL_US).coerceAtLeast(0), MediaExtractor.SEEK_TO_PREVIOUS_SYNC)
            val c = MediaCodec.createDecoderByType(format.getString(MediaFormat.KEY_MIME)!!)
            codec = c
            c.configure(format, null, null, 0)
            c.start()
            val info = MediaCodec.BufferInfo()
            var out = FloatArray(0)
            var size = 0
            var wantFrames = Long.MAX_VALUE
            var inputDone = false
            while (true) {
                if (cancelled()) throw InterruptedException("decode cancelled")
                if (!inputDone) {
                    val i = c.dequeueInputBuffer(TIMEOUT_US)
                    if (i >= 0) {
                        val buf = c.getInputBuffer(i)!!
                        val n = extractor.readSampleData(buf, 0)
                        if (n < 0) {
                            c.queueInputBuffer(i, 0, 0, 0, MediaCodec.BUFFER_FLAG_END_OF_STREAM)
                            inputDone = true
                        } else {
                            c.queueInputBuffer(i, 0, n, extractor.sampleTime, 0)
                            extractor.advance()
                        }
                    }
                }
                val o = c.dequeueOutputBuffer(info, TIMEOUT_US)
                when {
                    o == MediaCodec.INFO_OUTPUT_FORMAT_CHANGED -> {
                        val f = c.outputFormat
                        rate = f.getInteger(MediaFormat.KEY_SAMPLE_RATE)
                        channels = f.getInteger(MediaFormat.KEY_CHANNEL_COUNT)
                        float = f.containsKey(MediaFormat.KEY_PCM_ENCODING) && f.getInteger(MediaFormat.KEY_PCM_ENCODING) == AudioFormat.ENCODING_PCM_FLOAT
                    }
                    o >= 0 -> {
                        if (wantFrames == Long.MAX_VALUE) wantFrames = lengthMs * rate / 1000
                        val buf = c.getOutputBuffer(o)!!.order(ByteOrder.nativeOrder())
                        buf.position(info.offset)
                        buf.limit(info.offset + info.size)
                        val bytesPerSample = if (float) 4 else 2
                        val frames = info.size / (bytesPerSample * channels)
                        // Frames before `fromMs` are preroll.
                        val skip = if (info.presentationTimeUs >= fromUs) 0 else ((fromUs - info.presentationTimeUs) * rate / 1_000_000).coerceAtMost(frames.toLong()).toInt()
                        val have = size / channels
                        val take = minOf((frames - skip).toLong(), wantFrames - have).toInt().coerceAtLeast(0)
                        if (take > 0) {
                            if (out.size < size + take * channels) out = out.copyOf(maxOf(out.size * 2, size + take * channels))
                            val base = info.offset + skip * channels * bytesPerSample
                            for (k in 0 until take * channels) {
                                out[size + k] = if (float) buf.getFloat(base + k * 4) else buf.getShort(base + k * 2) / 32768f
                            }
                            size += take * channels
                        }
                        val eos = info.flags and MediaCodec.BUFFER_FLAG_END_OF_STREAM != 0
                        c.releaseOutputBuffer(o, false)
                        if (eos || size / channels >= wantFrames) break
                    }
                }
            }
            return Pcm(out.copyOf(size), rate, channels)
        } finally {
            runCatching { codec?.stop() }
            runCatching { codec?.release() }
            extractor.release()
            source?.close()
        }
    }

    private fun open(url: String, headers: Map<String, String>): Pair<MediaExtractor, MediaDataSource?> {
        val extractor = MediaExtractor()
        try {
            return when {
                url.startsWith(CoreStreams.SCHEME + ":") -> {
                    val core = streams() ?: throw IOException("the core is not running")
                    val source = CoreStreamMediaDataSource(core, url)
                    extractor.setDataSource(source)
                    extractor to source
                }
                url.startsWith("file:") -> {
                    extractor.setDataSource(Uri.parse(url).path ?: throw IOException("bad file URL"))
                    extractor to null
                }
                else -> {
                    extractor.setDataSource(url, headers)
                    extractor to null
                }
            }
        } catch (e: Exception) {
            extractor.release()
            throw e
        }
    }

    private fun audioTrack(extractor: MediaExtractor): Int =
        (0 until extractor.trackCount).firstOrNull { extractor.getTrackFormat(it).getString(MediaFormat.KEY_MIME)?.startsWith("audio/") == true }
            ?: throw IOException("no audio track")

    private companion object {
        const val PREROLL_US = 200_000L
        const val TIMEOUT_US = 10_000L
    }
}

/**
 * A core stream as a random-access [MediaDataSource]: sequential reads go through one open handle,
 * a jump reopens it at the new offset (the core serves ranges from its cache).
 */
class CoreStreamMediaDataSource(private val core: CoreStreams, private val url: String) : MediaDataSource() {
    private var handle: Long? = null
    private var next = -1L
    private var total: Long? = null
    private var pending = ByteArray(0)
    private var pendingAt = 0

    @Synchronized
    override fun readAt(position: Long, buffer: ByteArray, offset: Int, size: Int): Int {
        if (size == 0) return 0
        total?.let { if (position >= it) return -1 }
        if (handle == null || position != next) reopen(position)
        if (pendingAt >= pending.size) {
            pending = core.streamRead(handle!!, maxOf(size, 64 * 1024).coerceAtMost(CoreStreams.MAX_READ))
            pendingAt = 0
            if (pending.isEmpty()) return -1
        }
        val n = minOf(size, pending.size - pendingAt)
        System.arraycopy(pending, pendingAt, buffer, offset, n)
        pendingAt += n
        next = position + n
        return n
    }

    @Synchronized
    override fun getSize(): Long {
        if (handle == null) reopen(0)
        return total ?: -1
    }

    @Synchronized
    override fun close() {
        handle?.let(core::streamClose)
        handle = null
    }

    private fun reopen(position: Long) {
        handle?.let(core::streamClose)
        handle = null
        val info = core.streamOpen(url, position, null)
        handle = info.handle
        total = info.totalLength ?: total
        next = position
        pending = ByteArray(0)
        pendingAt = 0
    }
}
