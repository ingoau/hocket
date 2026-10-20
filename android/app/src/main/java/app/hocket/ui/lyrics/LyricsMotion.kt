package app.hocket.ui.lyrics

import androidx.compose.foundation.lazy.LazyListState
import androidx.compose.runtime.Immutable
import androidx.compose.runtime.Stable
import androidx.compose.runtime.mutableLongStateOf
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.runtime.withFrameNanos
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.launch
import kotlin.math.abs
import kotlin.math.cos
import kotlin.math.exp
import kotlin.math.sin
import kotlin.math.sqrt

/**
 * How the lyrics move: Apple Music's feel, as AMLL (applemusic-like-lyrics) and huddlefm's native
 * renderer tune it. Times in ms unless named otherwise; sizes in ems of the line's font. The
 * timings live in [LyricsFeel].
 */
internal object LyricsMotion {
    /** How big lines other than the current one are. */
    const val INACTIVE_SCALE = 0.94f
    /** Where the focused line's centre sits, as a share of the list's height. */
    const val FOCUS_POSITION = 0.35f
    /** Width of a syllable's soft leading edge. */
    const val FADE_WIDTH_EM = 0.5f
    /** How far a sung word rises (twice that on a background vocal). */
    const val LIFT_EM = 0.05f
    /** A first line starting this late gets the breathing dots before it. */
    const val INTRO_GAP_MS = 3_000L
}

/**
 * The lyrics' timings. [Default] is AMLL's, and what the app uses; [LocalLyricsFeel] can hand the
 * page others, to try a different feel (stiffer springs, a wider ripple, a little overshoot).
 */
@Immutable
internal data class LyricsFeel(
    /** Scrolling between lines: a spring of this stiffness over mass 0.9. */
    val scrollStiffness: Float,
    /** Growing into and out of the current line, and background vocals sliding out. */
    val scaleStiffness: Float,
    /** Between one line starting to move and the next, seconds; it shrinks by 5% a line below the focus. */
    val staggerS: Float,
    /** How early the view moves to a line before it is sung. */
    val leadMs: Long,
    /** A line lighting up or dimming. */
    val brightenMs: Int,
    /** A line's depth-of-field blur easing to its new radius as the focus moves on. */
    val blurMs: Int,
    /** Both springs' damping ratio: 1 arrives without overshooting, less overshoots and settles back. */
    val dampingRatio: Float = 1f,
) {
    private val omega = sqrt(scrollStiffness / 0.9f)

    /** The scroll spring's step response: 0 at rest, 1 arrived. */
    fun scrollResponse(seconds: Float): Float {
        if (seconds <= 0f) return 0f
        if (dampingRatio >= 1f) {
            val x = omega * seconds
            return 1f - (1f + x) * exp(-x)
        }
        val z = dampingRatio
        val wd = omega * sqrt(1f - z * z)
        return 1f - exp(-z * omega * seconds) * (cos(wd * seconds) + z / sqrt(1f - z * z) * sin(wd * seconds))
    }

    /** How long the scroll spring takes to come within 0.1% and stay there, seconds. */
    val settleS: Float = 9.2f / (dampingRatio.coerceAtMost(1f) * omega)

    /** Each line's delay after line [first] when the focus is [focus], seconds. */
    fun staggerDelays(first: Int, focus: Int, count: Int): FloatArray {
        val delays = FloatArray(count.coerceAtLeast(1))
        var delay = 0f
        var step = staggerS
        for (rank in delays.indices) {
            delays[rank] = delay
            delay += step
            if (first + rank >= focus) step /= 1.05f
        }
        return delays
    }

    companion object {
        val Default = LyricsFeel(scrollStiffness = 300f, scaleStiffness = 260f, staggerS = 0.035f, leadMs = 120L, brightenMs = 180, blurMs = 400)
    }
}

/** The lyrics' timings ([LyricsFeel.Default] unless provided). */
internal val LocalLyricsFeel = staticCompositionLocalOf { LyricsFeel.Default }

/**
 * The lyrics' staggered motion: the list scrolls on [LyricsFeel]'s spring, and each line trails it
 * by its stagger delay, so the lines ripple into place top to bottom instead of moving as one sheet.
 *
 * Every move is a "kick" (a step of the spring's target); kicks superpose, so a new line arriving
 * mid-move just adds its own. The list itself scrolls by the undelayed sum ([LazyListState.dispatchRawDelta],
 * no scroll lock taken from the user); [lag] is what each line still owes on top of that, read by
 * the line's graphics layer, so following a kick redraws the lines without recomposing them.
 *
 * A [shift] is a line growing or shrinking at once (background vocals opening or folding away):
 * the lines it moves are held where they were and ripple to their new places on the same spring,
 * as AMLL moves them, instead of the space opening or closing frame by frame under them.
 *
 * The frame loop runs only while something is moving.
 */
@Stable
internal class StaggeredScroll(private val state: LazyListState, private val scope: CoroutineScope, private val feel: LyricsFeel) {
    /** A kick ([shifts]: [above] says which side of [first] it holds, rippling out from it). */
    inner class Kick(val delta: Float, val first: Int, val delays: FloatArray, val above: Boolean = false) {
        var startNanos = -1L
        /** A [beginShift] whose change is not laid out yet. */
        var pending = false
        fun elapsed(now: Long, rank: Int = 0): Float =
            if (startNanos < 0 || now < startNanos) 0f else (now - startNanos) / 1e9f - delays[rank.coerceIn(0, delays.lastIndex)]
        val settleS get() = delays.last() + feel.settleS
    }

    private val kicks = ArrayList<Kick>()
    private val shifts = ArrayList<Kick>()
    /** How far the list has been scrolled for the live kicks. */
    private var applied = 0f
    private val frame = mutableLongStateOf(0L)
    private var following = false
    private var held: () -> Boolean = { false }
    private var job: Job? = null

    /** The lazy index followed last; -1 before the first. */
    var target = -1

    /** How far below its laid-out place item [index] is drawn right now, px. */
    fun lag(index: Int): Float {
        val now = frame.longValue
        if (kicks.isEmpty() && shifts.isEmpty()) return 0f
        var lag = 0f
        for (k in kicks) lag += k.delta * (feel.scrollResponse(k.elapsed(now)) - feel.scrollResponse(k.elapsed(now, index - k.first)))
        for (s in shifts) {
            val rank = if (s.above) s.first - index else index - s.first
            if (rank < 0) continue
            val r = feel.scrollResponse(s.elapsed(now, rank))
            // Before the change is laid out the lines glide toward where it will put them; after,
            // they are held back from where it put them. The same place either way.
            lag += if (s.pending) s.delta * r else -s.delta * (1f - r)
        }
        return lag
    }

    /** Drops the scroll where it is (the user took the list); lines already shifting finish. */
    fun stop() {
        kicks.clear()
        applied = 0f
        following = false
        frame.longValue += 1
    }

    /** Jumps to lazy index [index] with its centre at [LyricsMotion.FOCUS_POSITION], no animation. */
    suspend fun snap(index: Int) {
        stop()
        shifts.clear()
        target = index
        val viewport = state.layoutInfo.viewportSize.height
        state.scrollToItem(index, -(viewport * LyricsMotion.FOCUS_POSITION + state.layoutInfo.viewportStartOffset).toInt())
        // Now it is laid out: put its centre (not its top) on the focus line.
        withFrameNanos { }
        error(index)?.let { state.dispatchRawDelta(it) }
        // Kept there as lines above it grow or shrink.
        following = true
    }

    /**
     * Brings lazy index [index] to the focus line and keeps it there while lines above it grow or
     * shrink, kicking again whenever the place it is headed for is off by more than a pixel; until
     * [held] (the user's finger) says otherwise.
     */
    fun follow(index: Int, held: () -> Boolean) {
        target = index
        following = true
        this.held = held
        ensureRunning()
    }

    /**
     * Item [after] has just changed height by [dy] px (in the frame being made): the items that
     * move for it are drawn where they were and ripple to their new places, nearest first. The list
     * keeps its first visible item where it is, so those are the items after [after] when that
     * item is at or above it, and [after] and everything before it otherwise (the space then
     * closes from above). Call from a frame callback, together with the change itself, so the jump
     * and its hold land in the same frame.
     */
    fun shift(after: Int, dy: Float) {
        plan(after, dy)?.let { shifts += it }
        frame.longValue += 1
        ensureRunning()
    }

    /**
     * Item [after] is about to change height by [dy] px, once it has faded (background vocals
     * folding away): the items that will move start gliding to their new places now. Lay the
     * change out in a frame callback that calls [applyShift] with the result, or [cancelShift].
     */
    fun beginShift(after: Int, dy: Float): Kick? {
        val k = plan(after, dy) ?: return null
        k.pending = true
        shifts += k
        frame.longValue += 1
        ensureRunning()
        return k
    }

    /** The change [beginShift] planned is being laid out in this frame. */
    fun applyShift(shift: Kick?) {
        shift ?: return
        shift.pending = false
        frame.longValue += 1
        ensureRunning()
    }

    /** The change [beginShift] planned will not happen. */
    fun cancelShift(shift: Kick?) {
        if (shift != null && shifts.remove(shift)) frame.longValue += 1
    }

    /** Which items move for the change, and by how much ([shift]). */
    private fun plan(after: Int, dy: Float): Kick? {
        if (abs(dy) < 0.5f) return null
        val count = state.layoutInfo.visibleItemsInfo.size + 2
        return if (state.firstVisibleItemIndex > after) Kick(-dy, after, feel.staggerDelays(0, 0, count), above = true)
        else Kick(dy, after + 1, feel.staggerDelays(0, 0, count))
    }

    private fun ensureRunning() {
        if (job?.isActive == true) return
        job = scope.launch {
            do withFrameNanos { step(it) } while (kicks.isNotEmpty() || shifts.isNotEmpty())
            frame.longValue += 1
        }
    }

    private fun step(now: Long) {
        if (held()) { kicks.clear(); applied = 0f; following = false }
        for (k in kicks) if (k.startNanos < 0) k.startNanos = now
        for (s in shifts) if (s.startNanos < 0) s.startNanos = now
        var total = 0f
        for (k in kicks) total += k.delta * feel.scrollResponse(k.elapsed(now))
        // Where the target lands once the live kicks finish, against where it should.
        if (following && target >= 0) error(target)?.let { laidOut ->
            // A fold not laid out yet moves the target all the same: aim for where it will be.
            var err = laidOut
            for (sh in shifts) if (sh.pending && (if (sh.above) target <= sh.first else target >= sh.first)) err += sh.delta
            val remaining = kicks.sumOf { it.delta.toDouble() }.toFloat() - applied
            val off = err - remaining
            val reachable = if (off > 0) state.canScrollForward else state.canScrollBackward
            if (abs(off) > 1f && reachable) {
                val info = state.layoutInfo
                val first = info.visibleItemsInfo.firstOrNull()?.index ?: target
                kicks += Kick(off, first, feel.staggerDelays(first, target, info.visibleItemsInfo.size + 2)).also { it.startNanos = now }
            }
        }
        state.dispatchRawDelta(total - applied)
        applied = total
        for (k in kicks.filter { it.elapsed(now) > it.settleS }) { applied -= k.delta * feel.scrollResponse(k.elapsed(now)); kicks.remove(k) }
        shifts.removeAll { !it.pending && it.elapsed(now) > it.settleS }
        frame.longValue = now
    }

    /** How far lazy index [index]'s centre is below the focus line, px; null when it is not laid out. */
    private fun error(index: Int): Float? {
        val info = state.layoutInfo
        val item = info.visibleItemsInfo.firstOrNull { it.index == index } ?: return null
        val centre = item.offset - info.viewportStartOffset + item.size / 2f
        return centre - info.viewportSize.height * LyricsMotion.FOCUS_POSITION
    }
}
