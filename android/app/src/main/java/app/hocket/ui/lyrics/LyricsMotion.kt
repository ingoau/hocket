package app.hocket.ui.lyrics

import androidx.compose.foundation.lazy.LazyListState
import androidx.compose.runtime.Stable
import androidx.compose.runtime.mutableLongStateOf
import androidx.compose.runtime.withFrameNanos
import kotlin.math.abs
import kotlin.math.exp
import kotlin.math.sqrt

/**
 * How the lyrics move: Apple Music's feel, as AMLL (applemusic-like-lyrics) and huddlefm's native
 * renderer tune it. Times in ms unless named otherwise; sizes in ems of the line's font.
 */
internal object LyricsMotion {
    /** Scrolling between lines: a critically damped spring, AMLL's stiffness 300 over mass 0.9 (rad/s). */
    val SCROLL_OMEGA = sqrt(300f / 0.9f)
    /** Growing into and out of the current line (a critically damped spring). */
    const val SCALE_STIFFNESS = 260f
    /** How big lines other than the current one are. */
    const val INACTIVE_SCALE = 0.94f
    /** Between one line starting to scroll and the next, seconds; it shrinks by 5% a line below the focus. */
    const val STAGGER_S = 0.035f
    /** How early the view moves to a line before it is sung. */
    const val LEAD_MS = 120L
    /** Where the focused line's centre sits, as a share of the list's height. */
    const val FOCUS_POSITION = 0.35f
    /** A line lighting up or dimming. */
    const val BRIGHTEN_MS = 180
    /** Width of a syllable's soft leading edge. */
    const val FADE_WIDTH_EM = 0.5f
    /** How far a sung word rises (twice that on a background vocal). */
    const val LIFT_EM = 0.05f
    /** A first line starting this late gets the breathing dots before it. */
    const val INTRO_GAP_MS = 3_000L
    /** A scroll has settled this long after its last line starts moving (the spring is within 0.1%). */
    const val SETTLE_S = 0.5f

    /** The scroll spring's step response: 0 at rest, 1 arrived, never overshooting. */
    fun scrollResponse(seconds: Float): Float {
        if (seconds <= 0f) return 0f
        val x = SCROLL_OMEGA * seconds
        return 1f - (1f + x) * exp(-x)
    }

    /** Line [index]'s scroll delay after the first visible line [first] when the focus is [focus], seconds. */
    fun staggerDelays(first: Int, focus: Int, count: Int): FloatArray {
        val delays = FloatArray(count.coerceAtLeast(1))
        var delay = 0f
        var step = STAGGER_S
        for (rank in delays.indices) {
            delays[rank] = delay
            delay += step
            if (first + rank >= focus) step /= 1.05f
        }
        return delays
    }
}

/**
 * The lyrics' staggered scroll: the list scrolls on [LyricsMotion]'s spring, and each line trails it
 * by its stagger delay, so the lines ripple into place top to bottom instead of moving as one sheet.
 *
 * Every move is a "kick" (a step of the spring's target); kicks superpose, so a new line arriving
 * mid-move just adds its own. The list itself scrolls by the undelayed sum ([LazyListState.dispatchRawDelta],
 * no scroll lock taken from the user); [lag] is what each line still owes on top of that, read by
 * the line's graphics layer, so following a kick redraws the lines without recomposing them. The
 * frame loop runs only while a kick is moving.
 */
@Stable
internal class StaggeredScroll(private val state: LazyListState) {
    private class Kick(val delta: Float, val first: Int, val delays: FloatArray) {
        var startNanos = -1L
        fun elapsed(now: Long, rank: Int = 0): Float =
            if (startNanos < 0) 0f else (now - startNanos) / 1e9f - delays[rank.coerceIn(0, delays.lastIndex)]
    }

    private val kicks = ArrayList<Kick>()
    /** How far the list has been scrolled for the live kicks. */
    private var applied = 0f
    private val frame = mutableLongStateOf(0L)

    /** The lazy index followed last; -1 before the first. */
    var target = -1

    val moving: Boolean get() = kicks.isNotEmpty()

    /** How far below its laid-out place item [index] is drawn right now, px. */
    fun lag(index: Int): Float {
        val now = frame.longValue
        if (kicks.isEmpty()) return 0f
        var lag = 0f
        for (k in kicks) lag += k.delta * (LyricsMotion.scrollResponse(k.elapsed(now)) - LyricsMotion.scrollResponse(k.elapsed(now, index - k.first)))
        return lag
    }

    /** Drops every kick where it is (the user took the list, or it jumped). */
    fun stop() {
        kicks.clear()
        applied = 0f
        frame.longValue = 0L
    }

    /** Jumps to lazy index [index] with its centre at [LyricsMotion.FOCUS_POSITION], no animation. */
    suspend fun snap(index: Int) {
        stop()
        target = index
        val viewport = state.layoutInfo.viewportSize.height
        state.scrollToItem(index, -(viewport * LyricsMotion.FOCUS_POSITION + state.layoutInfo.viewportStartOffset).toInt())
        // Now it is laid out: put its centre (not its top) on the focus line.
        withFrameNanos { }
        error(index)?.let { state.dispatchRawDelta(it) }
    }

    /**
     * Brings lazy index [index] to the focus line and keeps it there while lines above it grow or
     * shrink (background vocals folding away), kicking again whenever the place it is headed for is
     * off by more than a pixel. Returns once everything has settled.
     */
    suspend fun follow(index: Int, held: () -> Boolean) {
        target = index
        do {
            withFrameNanos { now ->
                if (held()) { stop(); return@withFrameNanos }
                for (k in kicks) if (k.startNanos < 0) k.startNanos = now
                var total = 0f
                for (k in kicks) total += k.delta * LyricsMotion.scrollResponse(k.elapsed(now))
                // Where the target lands once the live kicks finish, against where it should.
                error(index)?.let { err ->
                    val remaining = kicks.sumOf { it.delta.toDouble() }.toFloat() - applied
                    val off = err - remaining
                    val reachable = if (off > 0) state.canScrollForward else state.canScrollBackward
                    if (abs(off) > 1f && reachable) {
                        val info = state.layoutInfo
                        val first = info.visibleItemsInfo.firstOrNull()?.index ?: index
                        kicks += Kick(off, first, LyricsMotion.staggerDelays(first, index, info.visibleItemsInfo.size + 2)).also { it.startNanos = now }
                    }
                }
                state.dispatchRawDelta(total - applied)
                applied = total
                val done = kicks.filter { it.elapsed(now, it.delays.lastIndex) > LyricsMotion.SETTLE_S }
                for (k in done) { applied -= k.delta * LyricsMotion.scrollResponse(k.elapsed(now)); kicks.remove(k) }
                frame.longValue = now
            }
        } while (kicks.isNotEmpty())
        frame.longValue = 0L
    }

    /** How far lazy index [index]'s centre is below the focus line, px; null when it is not laid out. */
    private fun error(index: Int): Float? {
        val info = state.layoutInfo
        val item = info.visibleItemsInfo.firstOrNull { it.index == index } ?: return null
        val centre = item.offset - info.viewportStartOffset + item.size / 2f
        return centre - info.viewportSize.height * LyricsMotion.FOCUS_POSITION
    }
}
