package app.hocket.ui.theme

import androidx.compose.animation.core.Spring
import androidx.compose.animation.core.SpringSpec
import androidx.compose.animation.core.spring
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.IntOffset

/**
 * Motion conventions (see android/README.md): anything the finger drives settles with a spring, never
 * a tween. Surfaces that move content (the player sheet, swipe-backs) never overshoot; only small
 * press feedback is allowed to bounce.
 */
object Motion {
    /** The player sheet: critically damped, so a settle never overshoots its anchor (Metrolist-like). */
    val sheet: SpringSpec<Float> = spring(dampingRatio = Spring.DampingRatioNoBouncy, stiffness = Spring.StiffnessMediumLow)
    /** Snapping small things back (swipe-to-skip, swipe-to-remove resistance, reorder drop): no bounce. */
    val snap: SpringSpec<Float> = spring(dampingRatio = Spring.DampingRatioNoBouncy, stiffness = Spring.StiffnessMediumLow)
    /** Values that must not overshoot (progress, alpha). */
    val settle: SpringSpec<Float> = spring(dampingRatio = Spring.DampingRatioNoBouncy, stiffness = Spring.StiffnessMedium)
    val quick: SpringSpec<Float> = spring(dampingRatio = Spring.DampingRatioNoBouncy, stiffness = Spring.StiffnessHigh)
    /** Press feedback on transport buttons (weights, the release bounce): a little springy. */
    val press: SpringSpec<Float> = spring(dampingRatio = 0.6f, stiffness = 500f)
    /** The release half of a press bounce: back to rest with a visible wobble. */
    val pressRelease: SpringSpec<Float> = spring(dampingRatio = Spring.DampingRatioMediumBouncy, stiffness = Spring.StiffnessLow)
    /** Slow, calm changes of the artwork (shrinks while paused). */
    val artwork: SpringSpec<Float> = spring(dampingRatio = Spring.DampingRatioNoBouncy, stiffness = Spring.StiffnessLow)
    val dp: SpringSpec<Dp> = spring(dampingRatio = Spring.DampingRatioNoBouncy, stiffness = Spring.StiffnessMediumLow)
    val offset: SpringSpec<IntOffset> = spring(dampingRatio = Spring.DampingRatioNoBouncy, stiffness = Spring.StiffnessMedium)
    /**
     * The full player switching mode (artwork <-> lyrics / queue / about): the one artwork travels
     * between its big slot and the thumbnail beside the title. Expressive "default spatial": quick,
     * with the faintest settle, never a visible bounce.
     */
    val mode: SpringSpec<Float> = spring(dampingRatio = 0.9f, stiffness = 420f)
    /** Expressive shape morphs on small controls (play/pause, a pressed button squishing): springy. */
    val morph: SpringSpec<Float> = spring(dampingRatio = 0.6f, stiffness = 380f)
    /** A pressed control growing and its neighbours giving way (width in dp): a little springy. */
    val pressDp: SpringSpec<Dp> = spring(dampingRatio = 0.6f, stiffness = 500f)

    /** Colour crossfades when the artwork (and so the player's palette) changes. */
    const val COLOUR_MS = 500
    /** The blurred-artwork background crossfade on a track change. */
    const val BACKGROUND_MS = 800
}
