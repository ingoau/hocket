package app.hocket.ui.player

import androidx.compose.animation.core.animate
import androidx.compose.animation.core.spring
import androidx.compose.runtime.Stable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.input.nestedscroll.NestedScrollConnection
import androidx.compose.ui.input.nestedscroll.NestedScrollSource
import androidx.compose.ui.unit.Velocity
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.launch

/**
 * How far the full player's controls have folded away under the queue: 0 is the whole player, 1 is
 * the compact one (the seek bar without its times, a lower transport, no action row, tight gaps; the
 * queue takes the room). Scrolling the queue towards its end folds them, scrolling back unfolds them
 * (at once, wherever the list is, as a collapsing toolbar comes back); the list scrolls as ever, the
 * fold consumes nothing. When a gesture (and its fling) ends between the two, it settles on the
 * nearer one.
 */
@Stable
internal class ControlsCollapse(private val scope: CoroutineScope) {
    var fraction by mutableFloatStateOf(0f)
        private set

    /** The scroll, in pixels, that takes the controls from one end to the other. */
    var range = 1f

    private var settling: Job? = null

    val connection = object : NestedScrollConnection {
        override fun onPreScroll(available: Offset, source: NestedScrollSource): Offset {
            if (available.y != 0f) {
                settling?.cancel()
                fraction = (fraction - available.y / range).coerceIn(0f, 1f)
            }
            return Offset.Zero
        }

        override suspend fun onPostFling(consumed: Velocity, available: Velocity): Velocity {
            settle()
            return Velocity.Zero
        }
    }

    private fun settle() {
        if (fraction == 0f || fraction == 1f) return
        animateTo(if (fraction > 0.5f) 1f else 0f)
    }

    /** Eases to [target] (straight there with [instant]). */
    fun animateTo(target: Float, instant: Boolean = false) {
        settling?.cancel()
        if (instant || fraction == target) { fraction = target; return }
        settling = scope.launch {
            animate(fraction, target, animationSpec = spring(stiffness = 500f)) { v, _ -> fraction = v.coerceIn(0f, 1f) }
        }
    }
}
