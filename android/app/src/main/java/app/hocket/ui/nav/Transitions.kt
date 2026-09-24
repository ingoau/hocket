package app.hocket.ui.nav

import androidx.compose.animation.AnimatedContentTransitionScope
import androidx.compose.animation.AnimatedVisibilityScope
import androidx.compose.animation.EnterExitState
import androidx.compose.animation.EnterTransition
import androidx.compose.animation.ExitTransition
import androidx.compose.animation.core.animateFloat
import androidx.compose.animation.core.CubicBezierEasing
import androidx.compose.animation.core.Easing
import androidx.compose.animation.core.LinearEasing
import androidx.compose.animation.core.snap
import androidx.compose.animation.core.tween
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.scaleOut
import androidx.compose.animation.slideInHorizontally
import androidx.compose.animation.slideOutHorizontally
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.MaterialTheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.drawBehind
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.unit.Density
import androidx.compose.ui.unit.LayoutDirection
import androidx.compose.ui.unit.dp
import androidx.navigation.NavBackStackEntry
import androidx.navigationevent.NavigationEvent
import kotlin.math.roundToInt

/*
 * Screen transitions for the shell's NavHost.
 *
 * - Pushes (a detail or settings screen) use Material's shared X axis: the new screen slides in
 *   from ~30% of the width on an emphasized-decelerate curve while it fades in; the old one drifts
 *   30 dp the other way while fading out quickly. Pops reverse it.
 * - Predictive back (after Navic) has its own pair, scrubbed by the gesture: the screen being left
 *   becomes a card that shrinks, rounds its corners ([PredictiveBackCard]) and moves with the finger
 *   (toward the far side from the edge the swipe started at), while the screen behind slides in
 *   from a short parallax offset on that side. Letting go finishes the same motion (the card leaves
 *   the screen); cancelling runs it back.
 * - Switching places from the bottom bar or rail is a quick fade-through with a short slide in the
 *   direction of the tab (right for a tab to the right), so it reads as a lateral move, not a push.
 * - With reduced motion everything is a plain short crossfade (predictive back scrubs a crossfade).
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

/** Predictive back: the length the gesture scrubs through (and so how quickly a release finishes it). */
internal const val PREDICTIVE_MS = 350
/** How small the card being left gets, and how round its corners. */
private const val PREDICTIVE_SCALE = 0.9f
private val PredictiveCorner = 28.dp
/** The card's shrink (and corners) lead: most of it happens in the first third of the gesture. */
private val PredictiveShrink = Easing { t -> 1f - (1f - (t * 2.5f).coerceAtMost(1f)).let { it * it } }
/** The card lags the finger at first, then leaves quickly (a release finishes the slide). */
private val PredictiveSlide = CubicBezierEasing(0.4f, 0f, 1f, 1f)

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

    /**
     * The entry a predictive back gesture is currently dragging away (set when its transition is
     * built, cleared by any other transition), so [PredictiveBackCard] only rounds that one.
     */
    var predictiveExitId: String? = null
        private set
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

    fun exit(scope: AnimatedContentTransitionScope<NavBackStackEntry>): ExitTransition {
        predictiveExitId = null
        return when {
            reducedMotion -> fadeOut(tween(REDUCED_MS))
            isTabSwitch(scope) -> tabExit()
            else -> fadeOut(tween(PUSH_EXIT_MS / 2 + 50, easing = LinearEasing)) +
                slideOutHorizontally(tween(PUSH_EXIT_MS, easing = EmphasizedAccelerate)) { -sign * drift }
        }
    }

    fun popEnter(scope: AnimatedContentTransitionScope<NavBackStackEntry>): EnterTransition = when {
        reducedMotion -> fadeIn(tween(REDUCED_MS))
        isTabSwitch(scope) -> tabEnter()
        else -> fadeIn(tween(PUSH_MS, delayMillis = 50, easing = LinearEasing)) +
            slideInHorizontally(tween(PUSH_MS, easing = EmphasizedDecelerate)) { -sign * drift }
    }

    fun popExit(scope: AnimatedContentTransitionScope<NavBackStackEntry>): ExitTransition {
        predictiveExitId = null
        return when {
            reducedMotion -> fadeOut(tween(REDUCED_MS))
            isTabSwitch(scope) -> tabExit()
            else -> fadeOut(tween(PUSH_EXIT_MS, easing = LinearEasing)) +
                slideOutHorizontally(tween(PUSH_MS, easing = EmphasizedAccelerate)) { w -> sign * (w * 0.3f).roundToInt() }
        }
    }

    /**
     * Horizontal direction the predictive card moves in: away from the edge the swipe started at
     * (right from the left edge, left from the right edge; physical, whatever the layout direction).
     */
    private fun predictiveDirection(swipeEdge: Int): Int = when (swipeEdge) {
        NavigationEvent.EDGE_LEFT -> 1
        NavigationEvent.EDGE_RIGHT -> -1
        else -> sign
    }

    /** The screen behind, revealed by a back gesture: slides in from a short offset on the swipe's side. */
    fun predictivePopEnter(swipeEdge: Int): EnterTransition = when {
        reducedMotion -> fadeIn(tween(PREDICTIVE_MS, easing = LinearEasing))
        else -> {
            val dir = predictiveDirection(swipeEdge)
            slideInHorizontally(tween(PREDICTIVE_MS, easing = EmphasizedDecelerate)) { w -> -dir * w / 6 }
        }
    }

    /** The screen a back gesture drags away: a shrinking card following the finger ([PredictiveBackCard] rounds it). */
    fun predictivePopExit(scope: AnimatedContentTransitionScope<NavBackStackEntry>, swipeEdge: Int): ExitTransition {
        if (reducedMotion) {
            predictiveExitId = null
            return fadeOut(tween(PREDICTIVE_MS, easing = LinearEasing))
        }
        predictiveExitId = scope.initialState.id
        val dir = predictiveDirection(swipeEdge)
        return scaleOut(tween(PREDICTIVE_MS, easing = PredictiveShrink), targetScale = PREDICTIVE_SCALE) +
            slideOutHorizontally(tween(PREDICTIVE_MS, easing = PredictiveSlide)) { w -> dir * w }
    }

    private fun tabEnter(): EnterTransition {
        val dir = if (tabForward) sign else -sign
        return fadeIn(tween(TAB_MS, delayMillis = TAB_EXIT_MS / 2, easing = LinearEasing)) +
            slideInHorizontally(tween(TAB_MS + 60, easing = EmphasizedDecelerate)) { w -> dir * w / 10 }
    }

    // The outgoing tab just fades out fast: no ghosting of two pages over each other.
    private fun tabExit(): ExitTransition = fadeOut(tween(TAB_EXIT_MS, easing = LinearEasing))
}

/**
 * Wraps every screen in the NavHost. While a predictive back gesture drags this screen away
 * ([ShellTransitionInfo.predictiveExitId]) it is drawn as a card: opaque, clipped to corners that
 * round in as it shrinks, with a soft shadow over the screen revealed behind it. The amount follows
 * the entry's own enter/exit transition, so the gesture scrubs it with everything else; otherwise
 * this adds nothing (an instant, zero-length animation).
 */
@Composable
internal fun AnimatedVisibilityScope.PredictiveBackCard(transitions: ShellTransitionInfo, entryId: String, content: @Composable () -> Unit) {
    val card by transition.animateFloat(
        transitionSpec = { if (transitions.predictiveExitId == entryId) tween(PREDICTIVE_MS, easing = PredictiveShrink) else snap() },
        label = "predictiveBackCard",
    ) { state -> if (state == EnterExitState.PostExit && transitions.predictiveExitId == entryId) 1f else 0f }
    val background = MaterialTheme.colorScheme.background
    Box(
        Modifier
            .fillMaxSize()
            .graphicsLayer {
                // Read here (not in composition), so scrubbing only redraws the layer.
                val f = card
                if (f > 0f) {
                    shape = RoundedCornerShape(PredictiveCorner * f)
                    clip = true
                    shadowElevation = 6.dp.toPx() * f
                }
            }
            .drawBehind { if (card > 0f) drawRect(background) },
    ) { content() }
}
