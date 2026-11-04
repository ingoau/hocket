package app.hocket.ui.components

import androidx.compose.animation.core.Animatable
import androidx.compose.animation.core.FastOutSlowInEasing
import androidx.compose.animation.core.spring
import androidx.compose.animation.core.tween
import androidx.compose.foundation.layout.Box
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.drawBehind
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.unit.dp
import app.hocket.ui.a11y.LocalReducedMotion
import kotlinx.coroutines.launch

/**
 * A heart that answers being loved: when [loved] turns true the [content] pops (dips, then springs
 * past full size) and a thin ring in [color] spreads out and fades behind it. Unloving and the
 * first composition stay still, as does everything with reduced motion.
 */
@Composable
fun LoveBurst(loved: Boolean, color: Color, modifier: Modifier = Modifier, content: @Composable () -> Unit) {
    val reduced = LocalReducedMotion.current
    val pop = remember { Animatable(1f) }
    // 0 = no ring; runs to 1 as the ring spreads and fades.
    val ring = remember { Animatable(1f) }
    val was = remember { booleanArrayOf(loved) }
    LaunchedEffect(loved, reduced) {
        val became = loved && !was[0]
        was[0] = loved
        if (!became || reduced) return@LaunchedEffect
        launch {
            pop.snapTo(LoveBurstMotion.POP_FROM)
            pop.animateTo(1f, LoveBurstMotion.Pop)
        }
        launch {
            ring.snapTo(0f)
            ring.animateTo(1f, LoveBurstMotion.Ring)
        }
    }
    Box(
        modifier.drawBehind {
            val r = ring.value
            if (r >= 1f) return@drawBehind
            val maxRadius = size.minDimension / 2f
            val stroke = LoveBurstMotion.RING_WIDTH.toPx() * (1f - r)
            drawCircle(
                color = color.copy(alpha = color.alpha * (1f - r)),
                radius = maxRadius * (0.45f + 0.55f * r),
                style = Stroke(width = stroke),
            )
        },
        contentAlignment = Alignment.Center,
    ) {
        Box(Modifier.graphicsLayer { val s = pop.value; scaleX = s; scaleY = s }) { content() }
    }
}

private object LoveBurstMotion {
    const val POP_FROM = 0.6f
    val Pop = spring<Float>(dampingRatio = 0.4f, stiffness = 500f)
    val Ring = tween<Float>(durationMillis = 450, easing = FastOutSlowInEasing)
    val RING_WIDTH = 3.dp
}
