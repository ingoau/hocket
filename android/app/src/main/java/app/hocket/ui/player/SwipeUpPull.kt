package app.hocket.ui.player

import androidx.compose.animation.core.animate
import androidx.compose.foundation.OverscrollEffect
import androidx.compose.runtime.Stable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.hapticfeedback.HapticFeedback
import androidx.compose.ui.hapticfeedback.HapticFeedbackType
import androidx.compose.ui.input.nestedscroll.NestedScrollSource
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.Velocity
import androidx.compose.ui.unit.dp
import app.hocket.ui.components.SheetPull
import app.hocket.ui.theme.Motion
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.launch
import kotlin.math.exp

/**
 * Swiping up on the open player (`swipe.playerUp`) opens the queue (Apple Music), the lyrics, the
 * device picker or the song's menu. The finger's travel past the open sheet (what the sheet's drag,
 * or the page's scroll, left over) goes one of two ways:
 *
 * - into another mode ([onOpen]): it lifts the artwork ([raw]), rubber-banded to at most [maxLift]
 *   and a little smaller as it goes ([lift], [fraction]); released past [commit] or flung up, the
 *   player switches mode and the lift eases out as the artwork flies to the thumbnail beside the
 *   title, else it springs back;
 * - with [openSheet] set, into a sheet: the first upward pull shows it ([SheetPull]) and from there
 *   the sheet's top follows the finger; released past [sheetCommit] or flung up it opens, else it
 *   drops back and closes.
 *
 * Only while [enabled] (the sheet open, in artwork mode, nothing being swiped); dragging down first
 * gives back the lift (or brings the sheet down).
 *
 * The sheet's own drag (one starting on the artwork, which sits outside the page's scroll) reaches it
 * as the `anchoredDraggable`'s overscroll; the page's scroll through the sheet's nested-scroll
 * connection ([pull], [release]).
 */
@Stable
internal class SwipeUpPull(private val scope: CoroutineScope) : OverscrollEffect {
    var enabled: () -> Boolean = { false }
    var onOpen: () -> Unit = {}
    /** Set when the swipe brings up a sheet: shows it, following the given pull. */
    var openSheet: ((SheetPull) -> Unit)? = null
    var haptics: HapticFeedback? = null
    var reduced = false
    /** The lift's limit, the travel that opens the mode and the fling (px/s) that does (set from the density). */
    var maxLift = 1f
    var commit = Float.POSITIVE_INFINITY
    var flingVelocity = Float.POSITIVE_INFINITY

    /** The finger's travel up past the open sheet (px), into another mode. */
    var raw by mutableFloatStateOf(0f)
        private set
    /** The sheet under the finger, if the swipe brings one up. */
    private var sheet: SheetPull? = null
    private var crossed = false
    private var settling: Job? = null

    /**
     * A pull under the finger: lifted (or a sheet brought up) and not springing back or easing out.
     * Only a held pull takes a downward drag or a fling, so the queue's own list (just opened) or the
     * page is never blocked while the lift settles.
     */
    val held: Boolean get() = sheet?.held == true || (raw > 0f && settling?.isActive != true)

    /** How far the artwork is lifted (px, upwards): [raw] rubber-banded towards [maxLift]; none with reduced motion. */
    fun lift(): Float = if (reduced || maxLift <= 0f) 0f else maxLift * (1f - exp(-raw / maxLift))

    /** [lift] as a share of [maxLift] (0..1). */
    fun fraction(): Float = if (maxLift <= 0f) 0f else lift() / maxLift

    /** The travel that opens a sheet of [height] (px; 0 before it is laid out): half of it, at most twice [commit]. */
    fun sheetCommit(height: Float): Float = if (height > 0f) minOf(height / 2f, commit * 2f) else commit

    /**
     * Takes [dy] (px, negative = up) that the sheet left over: up only while [enabled], down only to
     * give back a lift (or bring a sheet down). Returns what it consumed (in [dy]'s sign).
     */
    fun pull(dy: Float): Float {
        if (dy == 0f) return 0f
        sheet?.let { s ->
            if (!s.held) return 0f
            val taken = s.drag(dy)
            threshold(s.travel > sheetCommit(s.height))
            return taken
        }
        if ((dy < 0f && !enabled()) || (dy > 0f && !held)) return 0f
        openSheet?.let { open ->
            if (dy > 0f || raw > 0f) return@let
            val s = SheetPull(scope)
            sheet = s
            open(s)
            return s.drag(dy)
        }
        settling?.cancel()
        val before = raw
        raw = (raw - dy).coerceAtLeast(0f)
        threshold(raw > commit)
        return before - raw
    }

    /** A tick as the pull crosses the point where letting go opens. */
    private fun threshold(over: Boolean) {
        if (over != crossed) {
            crossed = over
            haptics?.performHapticFeedback(if (over) HapticFeedbackType.GestureThresholdActivate else HapticFeedbackType.SegmentTick)
        }
    }

    /** The finger left with [velocity] (px/s, negative = up): open the mode or the sheet, or spring back. */
    fun release(velocity: Float) {
        if (!held) return
        crossed = false
        sheet?.let { s ->
            sheet = null
            val open = velocity < -flingVelocity || (velocity <= flingVelocity && s.travel > sheetCommit(s.height))
            if (open) haptics?.performHapticFeedback(HapticFeedbackType.Confirm)
            s.release(open, velocity)
            return
        }
        val open = enabled() && (raw > commit || velocity < -flingVelocity)
        if (open) {
            haptics?.performHapticFeedback(HapticFeedbackType.Confirm)
            onOpen()
        }
        val from = raw
        settling?.cancel()
        // Opening, the lift eases out with the mode change (the same spring as the artwork's flight).
        settling = scope.launch { animate(from, 0f, animationSpec = if (open) Motion.mode else Motion.snap) { v, _ -> raw = v } }
    }

    /**
     * Drops any lift at once (and lets a sheet under the finger drop back): the sheet started moving
     * or left the open player (back, the collapse button), or a drag ended without a release
     * (cancelled), so the artwork never stays lifted.
     */
    fun reset() {
        settling?.cancel()
        settling = null
        crossed = false
        raw = 0f
        sheet?.release(open = false)
        sheet = null
    }

    override fun applyToScroll(delta: Offset, source: NestedScrollSource, performScroll: (Offset) -> Offset): Offset {
        // Dragging down gives back the lift before the sheet moves.
        val back = if (delta.y > 0f) pull(delta.y) else 0f
        val consumed = performScroll(Offset(delta.x, delta.y - back))
        val left = delta.y - back - consumed.y
        val up = if (left < 0f) pull(left) else 0f
        return Offset(consumed.x, consumed.y + back + up)
    }

    override suspend fun applyToFling(velocity: Velocity, performFling: suspend (Velocity) -> Velocity) {
        if (!held) {
            performFling(velocity)
            return
        }
        // The sheet is resting open under a pull: it stays there.
        performFling(Velocity.Zero)
        release(velocity.y)
    }

    override val isInProgress: Boolean get() = raw > 0f || sheet != null

    companion object {
        val MAX_LIFT: Dp = 40.dp
        val COMMIT: Dp = 64.dp
        val FLING: Dp = 600.dp
        /** How much smaller a card artwork gets at the full lift. */
        const val SHRINK = 0.06f
    }
}
