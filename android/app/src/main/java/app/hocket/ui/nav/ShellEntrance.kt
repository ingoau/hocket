package app.hocket.ui.nav

import androidx.compose.animation.core.Animatable
import androidx.compose.animation.core.CubicBezierEasing
import androidx.compose.animation.core.LinearEasing
import androidx.compose.animation.core.tween
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow

/**
 * The shell's entrance as the splash screen leaves: the page rises into place, then the navigation
 * bar and the mini player slide up after it. Armed by a fresh MainActivity start (only then is there
 * a splash to come out of) and started when the splash begins to leave, so the shell builds itself
 * under the dissolving tile. Never armed otherwise (a recreation, tests): the shell is just there.
 */
object ShellEntrance {
    private val armed = MutableStateFlow(false)
    private val _revealed = MutableStateFlow(false)
    internal val revealed: StateFlow<Boolean> = _revealed.asStateFlow()

    /** A fresh start: the next shell to appear plays the entrance, once the splash leaves. */
    fun arm() {
        armed.value = true
        _revealed.value = false
    }

    /** The splash is leaving (or there is none left to wait for). */
    fun reveal() {
        _revealed.value = true
    }

    internal val isArmed: Boolean get() = armed.value

    internal fun played() {
        armed.value = false
    }

    /** The whole entrance; each part takes its own stretch of it ([entrancePart]). */
    internal const val DURATION_MS = 720
    internal const val CONTENT_START_MS = 0
    internal const val CONTENT_END_MS = 520
    internal const val BAR_START_MS = 120
    internal const val BAR_END_MS = 640
    internal const val MINI_START_MS = 200
    internal const val MINI_END_MS = 720
}

/** M3's emphasized decelerate: things arriving on screen. */
private val EmphasizedDecelerate = CubicBezierEasing(0.05f, 0.7f, 0.1f, 1f)

/**
 * The entrance's linear progress (0 to 1), read in the draw or placement phase so it never
 * recomposes the shell. Already 1 when no entrance is due.
 */
@Composable
internal fun rememberShellEntrance(): () -> Float {
    val progress = remember { Animatable(if (ShellEntrance.isArmed) 0f else 1f) }
    val revealed by ShellEntrance.revealed.collectAsState()
    LaunchedEffect(revealed) {
        if (revealed && progress.value < 1f) {
            ShellEntrance.played()
            progress.animateTo(1f, tween(ShellEntrance.DURATION_MS, easing = LinearEasing))
        }
    }
    return { progress.value }
}

/** One part of the entrance at overall progress [t]: 0 before [startMs], eased to 1 by [endMs]. */
internal fun entrancePart(t: Float, startMs: Int, endMs: Int): Float {
    if (t >= 1f) return 1f
    val x = ((t * ShellEntrance.DURATION_MS - startMs) / (endMs - startMs)).coerceIn(0f, 1f)
    return EmphasizedDecelerate.transform(x)
}
