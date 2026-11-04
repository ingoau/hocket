package app.hocket.ui.theme

import androidx.compose.animation.core.Animatable
import androidx.compose.animation.core.AnimationSpec
import androidx.compose.animation.core.FastOutSlowInEasing
import androidx.compose.animation.core.spring
import androidx.compose.animation.core.tween
import androidx.compose.foundation.interaction.InteractionSource
import androidx.compose.foundation.interaction.PressInteraction
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.State
import androidx.compose.runtime.remember
import androidx.compose.ui.Modifier
import androidx.compose.ui.layout.Measurable
import androidx.compose.ui.layout.MeasureResult
import androidx.compose.ui.layout.MeasureScope
import androidx.compose.ui.node.CompositionLocalConsumerModifierNode
import androidx.compose.ui.node.LayoutModifierNode
import androidx.compose.ui.node.ModifierNodeElement
import androidx.compose.ui.node.currentValueOf
import androidx.compose.ui.platform.InspectorInfo
import androidx.compose.ui.unit.Constraints
import app.hocket.ui.a11y.LocalReducedMotion
import kotlinx.coroutines.Job
import kotlinx.coroutines.coroutineScope
import kotlinx.coroutines.launch

/**
 * The app's one press bounce (the floating bar's places, grid covers, Library tiles): the control
 * eases down to a smaller scale as the finger lands and springs back a little past full size when
 * it lifts. A quick tap still shows the whole dip, because the release waits for the press-down to
 * finish. Bounce answers a touch, so nothing else in the app uses it.
 */
object PressScale {
    /** Small controls (the floating bar's places). */
    const val CONTROL = 0.9f
    /** Cards and tiles: a gentler dip, so a big surface doesn't lurch. */
    const val CARD = 0.95f

    val Down: AnimationSpec<Float> = tween(durationMillis = 90, easing = FastOutSlowInEasing)
    val Release: AnimationSpec<Float> = spring(dampingRatio = 0.45f, stiffness = 420f)
}

/** Drives [scale] from [interactions]' presses until cancelled. */
private suspend fun animatePresses(scale: Animatable<Float, *>, interactions: InteractionSource, pressed: Float) = coroutineScope {
    var down: Job? = null
    var up: Job? = null
    interactions.interactions.collect { interaction ->
        when (interaction) {
            is PressInteraction.Press -> {
                // A new press overrides a pending spring-back (a fast double tap).
                up?.cancel()
                down = launch { scale.animateTo(pressed, PressScale.Down) }
            }
            is PressInteraction.Release, is PressInteraction.Cancel -> {
                val pressing = down
                up = launch {
                    pressing?.join()
                    scale.animateTo(1f, PressScale.Release)
                }
            }
        }
    }
}

/**
 * The press scale as state, for a control that combines it with other motion. Stays at 1 with
 * reduced motion. Read it in a graphics layer, not in composition.
 */
@Composable
fun rememberPressScale(interactions: InteractionSource, pressed: Float = PressScale.CONTROL): State<Float> {
    val reduced = LocalReducedMotion.current
    val scale = remember { Animatable(1f) }
    LaunchedEffect(interactions, pressed, reduced) {
        if (reduced) { scale.snapTo(1f); return@LaunchedEffect }
        animatePresses(scale, interactions, pressed)
    }
    return scale.asState()
}

/**
 * Scales this element with [interactions]' presses (see [PressScale]). It's drawn in a graphics
 * layer, so there's no recomposition or relayout. Off with reduced motion.
 */
fun Modifier.pressScale(interactions: InteractionSource, pressed: Float = PressScale.CARD): Modifier =
    this then PressScaleElement(interactions, pressed)

private data class PressScaleElement(val interactions: InteractionSource, val pressed: Float) : ModifierNodeElement<PressScaleNode>() {
    override fun create() = PressScaleNode(interactions, pressed)
    override fun update(node: PressScaleNode) = node.update(interactions, pressed)
    override fun InspectorInfo.inspectableProperties() {
        name = "pressScale"
        properties["pressed"] = pressed
    }
}

private class PressScaleNode(
    private var interactions: InteractionSource,
    private var pressed: Float,
) : Modifier.Node(), LayoutModifierNode, CompositionLocalConsumerModifierNode {
    private val scale = Animatable(1f)
    private var job: Job? = null

    override val shouldAutoInvalidate get() = false

    override fun onAttach() = start()

    override fun onDetach() {
        job = null
    }

    fun update(interactions: InteractionSource, pressed: Float) {
        if (interactions == this.interactions && pressed == this.pressed) return
        this.interactions = interactions
        this.pressed = pressed
        if (isAttached) start()
    }

    private fun start() {
        job?.cancel()
        val reduced = currentValueOf(LocalReducedMotion)
        job = coroutineScope.launch {
            scale.snapTo(1f)
            if (!reduced) animatePresses(scale, interactions, pressed)
        }
    }

    override fun MeasureScope.measure(measurable: Measurable, constraints: Constraints): MeasureResult {
        val placeable = measurable.measure(constraints)
        return layout(placeable.width, placeable.height) {
            placeable.placeWithLayer(0, 0) {
                val s = scale.value
                scaleX = s
                scaleY = s
            }
        }
    }
}
