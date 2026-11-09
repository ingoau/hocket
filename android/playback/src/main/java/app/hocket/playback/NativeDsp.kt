package app.hocket.playback

import java.nio.ByteBuffer

/**
 * The per-buffer DSP calls of [MixProcessor] (`crates/hocket-android/src/dsp.rs`). Buffers are
 * direct `ByteBuffer`s of native-order floats, processed in place from index 0; handles are opaque
 * (0 = none, ignored). The seam lets the mixing logic run in JVM tests without the native library.
 */
interface DspEngine {
    fun chainNew(rate: Int, channels: Int): Long
    fun chainFree(handle: Long)
    fun chainSetFormat(handle: Long, rate: Int, channels: Int)
    /** An `AudioSettings` JSON: EQ and normalisation (ReplayGain is the explicit track gain). */
    fun chainSetAudioSettings(handle: Long, json: String): Boolean
    /** A new track starts at the next sample: its gain in dB (boost included), no ramp. */
    fun chainSetTrackGain(handle: Long, gainDb: Double)
    fun chainReset(handle: Long)
    /** Whether [chainProcess] would leave samples untouched (0 dB, EQ and normalisation off). */
    fun chainIsBypassed(handle: Long): Boolean
    fun chainProcess(handle: Long, buffer: ByteBuffer, samples: Int)
    /** `IncomingShaper` for a `TransitionPlan` JSON; 0 when the plan doesn't parse. */
    fun shaperNew(planJson: String, channels: Int, rate: Int): Long
    fun shaperFree(handle: Long)
    fun shaperProcess(handle: Long, buffer: ByteBuffer, samples: Int)
    /** The chain's soft limiter (−1 dB knee), for the sum of a tail and B. */
    fun limit(buffer: ByteBuffer, samples: Int)
}

/**
 * Direct JNI into `libhocket_android.so` (the UniFFI library; JNA loads the same file). JNI rather
 * than UniFFI because these run every few milliseconds on the playback thread: UniFFI would copy
 * every buffer through a `RustBuffer` both ways.
 */
object NativeDsp : DspEngine {
    /** The engine when the native library loads (never in JVM unit tests), else null. */
    val orNull: DspEngine? by lazy {
        try {
            System.loadLibrary("hocket_android")
            NativeDsp
        } catch (e: Throwable) {
            PlaybackLog.w("native DSP unavailable: ${e.message}")
            null
        }
    }

    external override fun chainNew(rate: Int, channels: Int): Long
    external override fun chainFree(handle: Long)
    external override fun chainSetFormat(handle: Long, rate: Int, channels: Int)
    external override fun chainSetAudioSettings(handle: Long, json: String): Boolean
    external override fun chainSetTrackGain(handle: Long, gainDb: Double)
    external override fun chainReset(handle: Long)
    external override fun chainIsBypassed(handle: Long): Boolean
    external override fun chainProcess(handle: Long, buffer: ByteBuffer, samples: Int)
    external override fun shaperNew(planJson: String, channels: Int, rate: Int): Long
    external override fun shaperFree(handle: Long)
    external override fun shaperProcess(handle: Long, buffer: ByteBuffer, samples: Int)
    external override fun limit(buffer: ByteBuffer, samples: Int)
}
