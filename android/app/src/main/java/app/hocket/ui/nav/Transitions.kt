package app.hocket.ui.nav

import androidx.compose.animation.AnimatedContentTransitionScope
import androidx.compose.animation.EnterTransition
import androidx.compose.animation.ExitTransition
import androidx.compose.animation.core.CubicBezierEasing
import androidx.compose.animation.core.LinearEasing
import androidx.compose.animation.core.tween
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.slideInHorizontally
import androidx.compose.animation.slideOutHorizontally
import androidx.compose.ui.unit.Density
import androidx.compose.ui.unit.LayoutDirection
import androidx.compose.ui.unit.dp
import androidx.navigation.NavBackStackEntry
import kotlin.math.roundToInt

/*
 * Screen transitions for the shell's NavHost.
 *
 * - Pushes (a detail or settings screen) use Material's shared X axis: the new screen slides in
 *   from ~30% of the width on an emphasized-decelerate curve while it fades in; the old one drifts
 *   30 dp the other way while fading out quickly. Pops (and predictive back, which scrubs the pop
 *   transition) reverse it.
 * - Switching places from the bottom bar or rail is a quick fade-through with a short slide in the
 *   direction of the tab (right for a tab to the right), so it reads as a lateral move, not a push.
 * - With reduced motion everything is a plain short crossfade.
 */

/** Material's emphasized decelerate curve (entering elements). */
internal val EmphasizedDecelerate = CubicBezierEasing(0.05f, 0.7f, 0.1f, 1f)

/** Material's emphasized accelerate curve (exiting elements). */
internal val EmphasizedAccelerate = CubicBezierEasing(0.3f, 0f, 0.8f, 0.15f)

private const val PUSH_MS = 300
private const val PUSH_EXIT_MS = 200
private const val TAB_MS = 220
private const val TAB_EXIT_MS = 90
private const val REDUCED_MS = 150
private val ExitDrift = 30.dp

/**
 * What the shell tells the transitions about the navigation it just made. Not snapshot state: it is
 * written right before `navigate` and read by the transition lambdas when the change is animated.
 */
internal class ShellTransitionInfo {
    /**
     * The entries a bottom-bar/rail switch left and landed on, and whether the new tab lies after
     * the previous one. Matching both ends means a later pop back to that tab's root is not
     * mistaken for the switch.
     */
    var tabFromId: String? = null
    var tabTargetId: String? = null
    var tabForward: Boolean = true
    var reducedMotion: Boolean = false
    var layoutDirection: LayoutDirection = LayoutDirection.Ltr
    var density: Density = Density(1f)

    private fun isTabSwitch(scope: AnimatedContentTransitionScope<NavBackStackEntry>) = scope.targetState.id == tabTargetId && scope.initialState.id == tabFromId

    /** +1 when "forward" moves content to the start (right to left in LTR). */
    private val sign: Int get() = if (layoutDirection == LayoutDirection.Ltr) 1 else -1
    private val drift: Int get() = with(density) { ExitDrift.toPx().roundToInt() }

    fun enter(scope: AnimatedContentTransitionScope<NavBackStackEntry>): EnterTransition = when {
        reducedMotion -> fadeIn(tween(REDUCED_MS))
        isTabSwitch(scope) -> tabEnter()
        else -> fadeIn(tween(PUSH_MS, delayMillis = 50, easing = LinearEasing)) +
            slideInHorizontally(tween(PUSH_MS, easing = EmphasizedDecelerate)) { w -> sign * (w * 0.3f).roundToInt() }
    }

    fun exit(scope: AnimatedContentTransitionScope<NavBackStackEntry>): ExitTransition = when {
        reducedMotion -> fadeOut(tween(REDUCED_MS))
        isTabSwitch(scope) -> tabExit()
        else -> fadeOut(tween(PUSH_EXIT_MS / 2 + 50, easing = LinearEasing)) +
            slideOutHorizontally(tween(PUSH_EXIT_MS, easing = EmphasizedAccelerate)) { -sign * drift }
    }

    fun popEnter(scope: AnimatedContentTransitionScope<NavBackStackEntry>): EnterTransition = when {
        reducedMotion -> fadeIn(tween(REDUCED_MS))
        isTabSwitch(scope) -> tabEnter()
        else -> fadeIn(tween(PUSH_MS, delayMillis = 50, easing = LinearEasing)) +
            slideInHorizontally(tween(PUSH_MS, easing = EmphasizedDecelerate)) { -sign * drift }
    }

    fun popExit(scope: AnimatedContentTransitionScope<NavBackStackEntry>): ExitTransition = when {
        reducedMotion -> fadeOut(tween(REDUCED_MS))
        isTabSwitch(scope) -> tabExit()
        else -> fadeOut(tween(PUSH_EXIT_MS, easing = LinearEasing)) +
            slideOutHorizontally(tween(PUSH_MS, easing = EmphasizedAccelerate)) { w -> sign * (w * 0.3f).roundToInt() }
    }

    private fun tabEnter(): EnterTransition {
        val dir = if (tabForward) sign else -sign
        return fadeIn(tween(TAB_MS, delayMillis = TAB_EXIT_MS / 2, easing = LinearEasing)) +
            slideInHorizontally(tween(TAB_MS + 60, easing = EmphasizedDecelerate)) { w -> dir * w / 10 }
    }

    // The outgoing tab just fades out fast: no ghosting of two pages over each other.
    private fun tabExit(): ExitTransition = fadeOut(tween(TAB_EXIT_MS, easing = LinearEasing))
}
