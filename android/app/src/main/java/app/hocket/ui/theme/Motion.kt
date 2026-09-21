package app.hocket.ui.theme

import androidx.compose.animation.core.Spring
import androidx.compose.animation.core.SpringSpec
import androidx.compose.animation.core.spring
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.IntOffset

/**
 * Motion conventions (see android/README.md): anything the finger drives settles with a spring, never
 * a tween. Bouncy for hero elements, low-bounce for everything that moves content.
 */
object Motion {
    /** Finger-driven surfaces: sheet, mini player swipe, artwork swipe. */
    val sheet: SpringSpec<Float> = spring(dampingRatio = Spring.DampingRatioLowBouncy, stiffness = Spring.StiffnessMediumLow)
    /** Snapping small things back (swipe-to-remove resistance, reorder drop). */
    val snap: SpringSpec<Float> = spring(dampingRatio = Spring.DampingRatioMediumBouncy, stiffness = Spring.StiffnessMedium)
    /** Values that must not overshoot (progress, alpha). */
    val settle: SpringSpec<Float> = spring(dampingRatio = Spring.DampingRatioNoBouncy, stiffness = Spring.StiffnessMedium)
    val quick: SpringSpec<Float> = spring(dampingRatio = Spring.DampingRatioNoBouncy, stiffness = Spring.StiffnessHigh)
    val dp: SpringSpec<Dp> = spring(dampingRatio = Spring.DampingRatioLowBouncy, stiffness = Spring.StiffnessMediumLow)
    val offset: SpringSpec<IntOffset> = spring(dampingRatio = Spring.DampingRatioLowBouncy, stiffness = Spring.StiffnessMedium)
}
