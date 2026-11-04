package app.hocket.ui.nav

import androidx.compose.animation.core.Animatable
import androidx.compose.animation.core.CubicBezierEasing
import androidx.compose.animation.core.LinearEasing
import androidx.compose.animation.core.tween
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update

/**
 * The shell's entrance as the splash screen leaves: the page rises into place, then the navigation
 * bar and the mini player slide up after it. Only a splash exit plays it: it [hide]s the shell
 * under the still-opaque splash, then [play]s the entrance once the tile has begun to clear, so
 * the entrance is seen rather than spent under the tile. Without a splash exit (a launch the
 * system showed no splash for, a recreation, tests) nothing hides the shell: it is just there.
 * A page that was ready under the splash can stage its own parts on the same clock ([pageStaged],
 * [stagedPart]): the shell then just fades it in, and its parts land one after another.
 */
object ShellEntrance {
    internal enum class Phase { Shown, Hidden, Entering }

    private val _phase = MutableStateFlow(Phase.Shown)
    internal val phase: StateFlow<Phase> = _phase.asStateFlow()

    /** The splash is about to leave: the shell waits, unseen, under it for [play]. */
    fun hide() {
        _phase.value = Phase.Hidden
    }

    /** The splash is clearing (or the wait was cut short): a hidden shell comes in. */
    fun play() {
        _phase.compareAndSet(Phase.Hidden, Phase.Entering)
    }

    /**
     * A fresh start: whatever a previous instance left (a hidden shell, a staged page) is reset,
     * and a shell it hid but never got to bring in [play]s now.
     */
    fun restart() {
        pageStaged = false
        play()
    }

    /** No entrance after all (a recreation has no splash): the shell is just there. */
    fun finish() {
        _phase.value = Phase.Shown
        pageStaged = false
    }

    internal fun played() {
        if (_phase.compareAndSet(Phase.Entering, Phase.Shown)) pageStaged = false
    }

    /**
     * The entrance's linear progress (0 to 1): 1 whenever no entrance is due. Snapshot state, so
     * whatever reads it in the draw phase follows it without recomposing.
     */
    internal val progress = Animatable(1f)

    /**
     * The page under the splash brings its own parts in on this entrance ([stagedPart]), so the
     * shell only fades it in, rather than moving it as one piece. Cleared once the entrance plays.
     */
    internal var pageStaged by mutableStateOf(false)

    /** The whole entrance; each part takes its own stretch of it ([entrancePart]). */
    internal const val DURATION_MS = 840
    internal const val CONTENT_START_MS = 0
    internal const val CONTENT_END_MS = 520
    /** A staged page's own fade: quick, so its parts are seen arriving rather than its backdrop. */
    internal const val STAGED_FADE_END_MS = 200
    /** A staged page's parts come in top-down, one [STAGE_STEP_MS] after another (from [STAGE_START_MS]). */
    internal const val STAGE_START_MS = 40
    internal const val STAGE_STEP_MS = 60
    internal const val STAGE_MS = 440
    internal const val STAGE_MAX_STEP = 6
    internal const val BAR_START_MS = 80
    internal const val BAR_END_MS = 380
    internal const val MINI_START_MS = 140
    internal const val MINI_END_MS = 440
}

/** M3's emphasized decelerate: things arriving on screen. */
private val EmphasizedDecelerate = CubicBezierEasing(0.05f, 0.7f, 0.1f, 1f)

/**
 * The entrance's linear progress (0 to 1), read in the draw or placement phase so it never
 * recomposes the shell. Already 1 when no entrance is due.
 */
@Composable
internal fun rememberShellEntrance(): () -> Float {
    val progress = ShellEntrance.progress
    val phase by ShellEntrance.phase.collectAsState()
    LaunchedEffect(phase) {
        when (phase) {
            ShellEntrance.Phase.Shown -> progress.snapTo(1f)
            ShellEntrance.Phase.Hidden -> progress.snapTo(0f)
            ShellEntrance.Phase.Entering -> {
                progress.animateTo(1f, tween(ShellEntrance.DURATION_MS, easing = LinearEasing))
                ShellEntrance.played()
            }
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

/** A staged page's [step]th part at overall progress [t] (0 to 1, eased): parts land top-down. */
internal fun stagedPart(t: Float, step: Int): Float {
    val start = ShellEntrance.STAGE_START_MS + step.coerceIn(0, ShellEntrance.STAGE_MAX_STEP) * ShellEntrance.STAGE_STEP_MS
    return entrancePart(t, start, start + ShellEntrance.STAGE_MS)
}

/**
 * Lets the first screen keep the splash up while its content loads, so a cold launch goes from the
 * splash straight to a full page instead of to skeletons. The splash still leaves at its own cap
 * (MainActivity's SPLASH_CONTENT_MAX_MS) if the content is slow; [covering] then turns false and
 * the screen carries on with its own loading state.
 */
object SplashHold {
    private val _covering = MutableStateFlow(false)
    /** The splash is on screen (from a fresh start until its exit begins). */
    val covering: StateFlow<Boolean> = _covering.asStateFlow()

    private val _holds = MutableStateFlow(0)
    /** Some screen is still loading under the splash. */
    val holding: Boolean get() = _holds.value > 0

    /** A fresh start: the splash is up. */
    fun splashShown() { _covering.value = true }

    /** The splash is leaving (or there is none): holds no longer matter. */
    fun splashGone() { _covering.value = false }

    fun hold() { _holds.update { it + 1 } }
    fun release() { _holds.update { (it - 1).coerceAtLeast(0) } }
}
