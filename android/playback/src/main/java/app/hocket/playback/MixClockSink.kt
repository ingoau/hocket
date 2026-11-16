package app.hocket.playback

import androidx.media3.exoplayer.audio.AudioSink
import androidx.media3.exoplayer.audio.ForwardingAudioSink
import java.util.concurrent.atomic.AtomicLong
import kotlin.math.abs

/**
 * Keeps ExoPlayer's clock on the item's real position after a mix (`docs/automix.md` → Android).
 *
 * The sink's clock counts output frames since the item's first frame. A mix outputs the tail's
 * first `incoming_at_ms`, then B from `incoming_from_ms`, so B's clock runs
 * `incoming_at_ms − incoming_from_ms` ahead of B. [ExoBackend] maps that back while the tail sounds,
 * then asks for a [correct] by the same amount. Left alone, the lead would carry into the next item:
 * ExoPlayer would move on early (or late) by the lead, and the renderer, whose position never goes
 * back on its own, would hold the next item's position at the lead until real time caught up.
 *
 * A correction moves the clock back (or forward) once, as a position discontinuity so the renderer
 * accepts it. It lasts until the sink re-bases its clock itself: at the next item's first frame (a
 * checkpoint, or a new `AudioTrack` at a format change) the raw position jumps by the lead it
 * carried, and the correction drops by that jump. A flush (seek, stop) starts over.
 */
class MixClockSink(
    sink: AudioSink,
    private val nanoTime: () -> Long = System::nanoTime,
    /** The renderer plays out the end of the whole playlist (the next end of stream is the last). */
    private val onPlayToEnd: () -> Unit = {},
) : ForwardingAudioSink(sink) {
    private val requested = AtomicLong()
    private var correctionUs = 0L
    private var lastRawUs = NONE
    private var lastAtNs = 0L
    private var listener: AudioSink.Listener? = null

    /** Corrections applied so far (each lands at a position reading). Any thread. */
    @Volatile var applied = 0
        private set

    /** Move the clock back by [us] (forward when negative), from its next reading. Any thread. */
    fun correct(us: Long) {
        requested.addAndGet(us)
    }

    override fun setListener(listener: AudioSink.Listener) {
        this.listener = listener
        super.setListener(listener)
    }

    override fun getCurrentPositionUs(sourceEnded: Boolean): Long {
        val raw = super.getCurrentPositionUs(sourceEnded)
        if (raw == AudioSink.CURRENT_POSITION_NOT_SET) return raw
        val now = nanoTime()
        if (correctionUs != 0L && lastRawUs != NONE) {
            // How far the raw clock moved beyond the time that passed: a re-base, not playback.
            val elapsedUs = (now - lastAtNs) / 1_000
            val moved = raw - lastRawUs
            val jump = when {
                moved < -REBASE_US -> moved
                moved - elapsedUs > REBASE_US -> moved - elapsedUs
                else -> 0L
            }
            // The re-base took out (some of) the lead this correction stood for.
            if (jump != 0L && (jump < 0) == (correctionUs > 0)) {
                val left = correctionUs + jump
                correctionUs = if (abs(left) < REBASE_US || (left > 0) != (correctionUs > 0)) 0L else left
            }
        }
        lastRawUs = raw
        lastAtNs = now
        val add = requested.getAndSet(0)
        if (add != 0L) {
            correctionUs += add
            applied++
            // Lets the renderer's position go back (its position is otherwise monotonic).
            listener?.onPositionDiscontinuity()
        }
        return raw - correctionUs
    }

    override fun playToEndOfStream() {
        onPlayToEnd()
        super.playToEndOfStream()
    }

    override fun flush() {
        clear()
        super.flush()
    }

    override fun reset() {
        clear()
        super.reset()
    }

    private fun clear() {
        requested.set(0)
        correctionUs = 0
        lastRawUs = NONE
    }

    internal companion object {
        private const val NONE = Long.MIN_VALUE
        /** A raw clock step this far from real time is the sink re-basing, not jitter. */
        const val REBASE_US = 50_000L
    }
}
