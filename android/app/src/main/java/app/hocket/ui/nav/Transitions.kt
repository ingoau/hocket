package app.hocket.ui.nav

import androidx.compose.animation.AnimatedContentTransitionScope
import androidx.compose.animation.AnimatedVisibilityScope
import androidx.compose.animation.EnterExitState
import androidx.compose.animation.EnterTransition
import androidx.compose.animation.ExitTransition
import androidx.compose.animation.core.Easing
import androidx.compose.animation.core.LinearEasing
import androidx.compose.animation.core.animateFloat
import androidx.compose.animation.core.snap
import androidx.compose.animation.core.tween
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.slideInHorizontally
import androidx.compose.animation.slideOutHorizontally
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.drawWithContent
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.unit.LayoutDirection
import androidx.compose.ui.unit.dp
import androidx.navigation.NavBackStackEntry
import kotlin.math.PI
import kotlin.math.cos
import kotlin.math.exp
import kotlin.math.roundToInt
import kotlin.math.sin
import kotlin.math.sqrt

/*
 * Screen transitions for the shell's NavHost, matching MiuixDefault from InstallerX Revived
 * (full-width slide + quarter-width covered parallax + light alpha falloff + a dim scrim).
 *
 * - Pushes: the new page slides in the full width from the trailing edge (right in LTR); the page
 *   underneath parallaxes a quarter width toward the leading edge, loses 10% opacity, and a dark
 *   scrim settles over it. The incoming page's leading corners round while it travels.
 * - Pops reverse the same motion. Predictive back scrubs that pop 1:1 with the finger, always in
 *   that trailing-edge direction — never away from whichever edge the swipe started at.
 * - Switching places from the bottom bar or rail is a quick fade-through with a short slide in the
 *   direction of the tab, so it reads as a lateral move, not a push.
 * - With reduced motion everything is a plain short crossfade (predictive back scrubs a crossfade).
 */

/**
 * Miuix's programmatic settle curve: the step response of an underdamped spring (`response = 0.8`,
 * `damping = 0.95`) baked into a 500 ms tween. Brisk middle (~48% at a fifth of the duration) and
 * a long, gentle tail; monotone in 0..1, never a bounce. See `NavProgrammaticEasing` in miuix-nav.
 */
internal val MiuixEasing: Easing = MiuixSettleEasing(response = 0.8f, damping = 0.95f)

internal class MiuixSettleEasing(response: Float, damping: Float) : Easing {
    private val r: Float
    private val w: Float
    private val c2: Float

    init {
        val omega = 2.0 * PI / response
        val k = omega * omega
        val c = damping * 4.0 * PI / response
        w = (sqrt(4.0 * k - c * c) / 2.0).toFloat()
        r = (-c / 2.0).toFloat()
        c2 = r / w
    }

    override fun transform(fraction: Float): Float {
        val t = fraction.toDouble()
        val decay = exp(r * t)
        return (decay * (-cos(w * t) + c2 * sin(w * t)) + 1.0).toFloat()
    }
}

/** Programmatic push/pop: Miuix's 500 ms settle. */
internal const val PUSH_MS = 500
/** Gesture-driven pop: linear so the finger maps 1:1 onto the same geometry. */
internal const val PREDICTIVE_MS = 400
private const val TAB_MS = 280
private const val TAB_EXIT_MS = 100
private const val REDUCED_MS = 150
/** Covered page travels a quarter of the width toward the leading edge. */
private const val COVER_PARALLAX = 0.25f
/** Covered page keeps 90% opacity (Miuix's light alpha falloff). */
private const val COVER_ALPHA = 0.9f
/** Cap of the dark scrim drawn over the covered page. */
private const val DIM_AMOUNT = 0.5f
/** Leading-edge corners of the travelling top page (InstallerX / system-ish). */
private val LeadingCorner = 28.dp

/** Which part this destination plays in the in-flight page transition. */
internal enum class PageMotion { Push, Pop, Predictive, Tab, Reduced }

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

    /**
     * The in-flight motion and which entries are the sliding top vs the covered page, so
     * [PageTransitionLayer] can round the top's leading corners and dim the page underneath.
     */
    var motion: PageMotion = PageMotion.Push
        private set
    var topEntryId: String? = null
        private set
    var coveredEntryId: String? = null
        private set

    private fun isTabSwitch(scope: AnimatedContentTransitionScope<NavBackStackEntry>) =
        scope.targetState.id == tabTargetId && scope.initialState.id == tabFromId

    /** +1 when "forward" (trailing-edge enter) moves content toward the start (right to left in LTR). */
    private val sign: Int get() = if (layoutDirection == LayoutDirection.Ltr) 1 else -1

    private fun markPush(scope: AnimatedContentTransitionScope<NavBackStackEntry>) {
        if (reducedMotion) {
            motion = PageMotion.Reduced
            topEntryId = null
            coveredEntryId = null
            return
        }
        if (isTabSwitch(scope)) {
            motion = PageMotion.Tab
            topEntryId = null
            coveredEntryId = null
            return
        }
        motion = PageMotion.Push
        topEntryId = scope.targetState.id
        coveredEntryId = scope.initialState.id
    }

    private fun markPop(scope: AnimatedContentTransitionScope<NavBackStackEntry>, predictive: Boolean) {
        if (reducedMotion) {
            motion = PageMotion.Reduced
            topEntryId = null
            coveredEntryId = null
            return
        }
        if (isTabSwitch(scope)) {
            motion = PageMotion.Tab
            topEntryId = null
            coveredEntryId = null
            return
        }
        motion = if (predictive) PageMotion.Predictive else PageMotion.Pop
        // The page being left is the top; the one revealed underneath is covered.
        topEntryId = scope.initialState.id
        coveredEntryId = scope.targetState.id
    }

    fun enter(scope: AnimatedContentTransitionScope<NavBackStackEntry>): EnterTransition {
        markPush(scope)
        return when (motion) {
            PageMotion.Reduced -> fadeIn(tween(REDUCED_MS))
            PageMotion.Tab -> tabEnter()
            else -> slideInHorizontally(tween(PUSH_MS, easing = MiuixEasing)) { w -> sign * w }
        }
    }

    fun exit(scope: AnimatedContentTransitionScope<NavBackStackEntry>): ExitTransition {
        markPush(scope)
        return when (motion) {
            PageMotion.Reduced -> fadeOut(tween(REDUCED_MS))
            PageMotion.Tab -> tabExit()
            else -> fadeOut(tween(PUSH_MS, easing = MiuixEasing), targetAlpha = COVER_ALPHA) +
                slideOutHorizontally(tween(PUSH_MS, easing = MiuixEasing)) { w -> -sign * (w * COVER_PARALLAX).roundToInt() }
        }
    }

    fun popEnter(scope: AnimatedContentTransitionScope<NavBackStackEntry>): EnterTransition {
        markPop(scope, predictive = false)
        return when (motion) {
            PageMotion.Reduced -> fadeIn(tween(REDUCED_MS))
            PageMotion.Tab -> tabEnter()
            else -> fadeIn(tween(PUSH_MS, easing = MiuixEasing), initialAlpha = COVER_ALPHA) +
                slideInHorizontally(tween(PUSH_MS, easing = MiuixEasing)) { w -> -sign * (w * COVER_PARALLAX).roundToInt() }
        }
    }

    fun popExit(scope: AnimatedContentTransitionScope<NavBackStackEntry>): ExitTransition {
        markPop(scope, predictive = false)
        return when (motion) {
            PageMotion.Reduced -> fadeOut(tween(REDUCED_MS))
            PageMotion.Tab -> tabExit()
            else -> slideOutHorizontally(tween(PUSH_MS, easing = MiuixEasing)) { w -> sign * w }
        }
    }

    /**
     * Predictive back uses the same pop geometry regardless of the swipe's edge: the leaving page
     * always travels toward the trailing edge, the page behind parallaxes in from the leading edge.
     */
    fun predictivePopEnter(scope: AnimatedContentTransitionScope<NavBackStackEntry>, @Suppress("UNUSED_PARAMETER") swipeEdge: Int): EnterTransition {
        markPop(scope, predictive = true)
        return when {
            reducedMotion -> fadeIn(tween(PREDICTIVE_MS, easing = LinearEasing))
            else -> fadeIn(tween(PREDICTIVE_MS, easing = LinearEasing), initialAlpha = COVER_ALPHA) +
                slideInHorizontally(tween(PREDICTIVE_MS, easing = LinearEasing)) { w -> -sign * (w * COVER_PARALLAX).roundToInt() }
        }
    }

    fun predictivePopExit(scope: AnimatedContentTransitionScope<NavBackStackEntry>, @Suppress("UNUSED_PARAMETER") swipeEdge: Int): ExitTransition {
        markPop(scope, predictive = true)
        return when {
            reducedMotion -> fadeOut(tween(PREDICTIVE_MS, easing = LinearEasing))
            else -> slideOutHorizontally(tween(PREDICTIVE_MS, easing = LinearEasing)) { w -> sign * w }
        }
    }

    private fun tabEnter(): EnterTransition {
        val dir = if (tabForward) sign else -sign
        return fadeIn(tween(TAB_MS, delayMillis = TAB_EXIT_MS / 2, easing = LinearEasing)) +
            slideInHorizontally(tween(TAB_MS + 40, easing = MiuixEasing)) { w -> dir * w / 12 }
    }

    private fun tabExit(): ExitTransition = fadeOut(tween(TAB_EXIT_MS, easing = LinearEasing))
}

/**
 * Wraps every screen in the NavHost. During a Miuix push/pop the travelling top page clips to
 * rounded leading-edge corners, and the covered page draws a dark scrim. Both follow the entry's
 * own enter/exit transition (so a predictive back gesture scrubs them); at rest this adds nothing.
 */
@Composable
internal fun AnimatedVisibilityScope.PageTransitionLayer(transitions: ShellTransitionInfo, entryId: String, content: @Composable () -> Unit) {
    val progress by transition.animateFloat(
        transitionSpec = {
            when (transitions.motion) {
                PageMotion.Predictive -> tween(PREDICTIVE_MS, easing = LinearEasing)
                PageMotion.Push, PageMotion.Pop -> tween(PUSH_MS, easing = MiuixEasing)
                else -> snap()
            }
        },
        label = "pageTransition",
    ) { state ->
        val involved = entryId == transitions.topEntryId || entryId == transitions.coveredEntryId
        if (!involved || transitions.motion == PageMotion.Tab || transitions.motion == PageMotion.Reduced) 0f
        else if (state == EnterExitState.Visible) 0f else 1f
    }
    val ltr = transitions.layoutDirection == LayoutDirection.Ltr
    Box(
        Modifier
            .fillMaxSize()
            .graphicsLayer {
                val f = progress
                if (f > 0f && entryId == transitions.topEntryId) {
                    val r = LeadingCorner * f
                    shape = if (ltr) RoundedCornerShape(topStart = r, bottomStart = r) else RoundedCornerShape(topEnd = r, bottomEnd = r)
                    clip = true
                }
            }
            .drawWithContent {
                drawContent()
                val f = progress
                if (f > 0f && entryId == transitions.coveredEntryId) {
                    drawRect(Color.Black.copy(alpha = DIM_AMOUNT * f))
                }
            },
    ) { content() }
}
