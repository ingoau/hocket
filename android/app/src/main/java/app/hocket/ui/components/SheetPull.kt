package app.hocket.ui.components

import androidx.compose.animation.core.animate
import androidx.compose.material3.BottomSheetDefaults
import androidx.compose.material3.SheetState
import androidx.compose.material3.SheetValue
import androidx.compose.material3.rememberBottomSheetState
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.Stable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.key
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.runtime.snapshotFlow
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.layout.onSizeChanged
import app.hocket.ui.theme.Motion
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.launch

/**
 * A sheet brought up by a drag that started somewhere else (the full player's swipe up): while the
 * finger holds it ([held]) the sheet's top follows the finger, [travel] up from the bottom of the
 * screen; let go, it opens the rest of the way ([open]: from then on a sheet like any other) or
 * drops back and closes ([onClosed], which takes it out of composition).
 *
 * The sheet is the usual `ModalBottomSheet`, shown at once and already expanded
 * ([rememberPulledSheetState]), slid down by what the finger has not yet brought up ([pulled]),
 * with its scrim following ([pulledScrim]). The finger stays on the window it went down on, so the
 * drag goes on reaching whoever drives this ([drag], [release]) while the sheet's window is up.
 */
@Stable
class SheetPull(private val scope: CoroutineScope) {
    /** How far up the finger has brought the sheet (px). */
    var travel by mutableFloatStateOf(0f)
        private set
    /** The sheet's height (px), once it has been laid out (0 until then). */
    var height by mutableFloatStateOf(0f)
        internal set
    /** Let go and opened: the sheet is no longer slid down. */
    var open by mutableStateOf(false)
        private set
    /** The sheet has dropped back: take it away. Set by whoever shows it. */
    var onClosed: () -> Unit = {}
    private var settling: Job? = null

    /** Under the finger: not yet let go. */
    val held: Boolean get() = !open && settling == null

    /** How much of the sheet shows (0..1). */
    fun shown(): Float = when {
        open -> 1f
        height <= 0f -> 0f
        else -> (travel / height).coerceIn(0f, 1f)
    }

    /** Moves the sheet by [dy] (px, negative = up) while [held]. Returns what it took (in [dy]'s sign). */
    fun drag(dy: Float): Float {
        if (!held || dy == 0f) return 0f
        val before = travel
        travel = (travel - dy).coerceIn(0f, if (height > 0f) height else Float.POSITIVE_INFINITY)
        return before - travel
    }

    /**
     * The finger left with [velocity] (px/s, negative = up): the sheet opens (once it has been laid
     * out, if it has not yet) or drops back and closes.
     */
    fun release(open: Boolean, velocity: Float = 0f) {
        if (!held) return
        settling = scope.launch {
            if (open) {
                val to = snapshotFlow { height }.first { it > 0f }
                animate(travel.coerceAtMost(to), to, initialVelocity = -velocity, animationSpec = Motion.sheet) { v, _ -> travel = v }
                this@SheetPull.open = true
            } else {
                animate(travel, 0f, initialVelocity = -velocity, animationSpec = Motion.sheet) { v, _ -> travel = v }
                onClosed()
            }
        }
    }
}

/** The sheet state for a sheet that may be [pull]ed up: one that starts expanded (and is slid down instead) if so. */
@Composable
fun rememberPulledSheetState(pull: SheetPull?): SheetState = key(pull) {
    if (pull == null) rememberModalBottomSheetState(skipPartiallyExpanded = true)
    else rememberBottomSheetState(initialValue = SheetValue.Expanded, enabledValues = setOf(SheetValue.Hidden, SheetValue.Expanded))
}

/** Slides a [pull]ed sheet down by what the finger has not yet brought up (for the sheet's `modifier`). */
fun Modifier.pulled(pull: SheetPull?): Modifier = if (pull == null) this else this
    .onSizeChanged { pull.height = it.height.toFloat() }
    .graphicsLayer { translationY = if (pull.open) 0f else size.height - pull.travel.coerceIn(0f, size.height) }

/** The sheet's scrim, fading in as a [pull]ed sheet comes up. */
@Composable
fun pulledScrim(pull: SheetPull?): Color = BottomSheetDefaults.ScrimColor.let { if (pull == null) it else it.copy(alpha = it.alpha * pull.shown()) }
