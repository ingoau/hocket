package app.hocket.ui.theme

import androidx.compose.animation.core.Animatable
import androidx.compose.foundation.interaction.InteractionSource
import androidx.compose.material3.MaterialShapes
import androidx.compose.material3.toPath
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.remember
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Matrix
import androidx.compose.ui.graphics.Outline
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.Shape
import androidx.compose.ui.unit.Density
import androidx.compose.ui.unit.LayoutDirection
import androidx.graphics.shapes.Morph
import androidx.graphics.shapes.RoundedPolygon
import app.hocket.ui.a11y.LocalReducedMotion

/**
 * A shape between two [MaterialShapes] polygons: [progress] 0 is [Morph]'s start, 1 its end. The
 * polygons are in the unit square (as [MaterialShapes]' are), scaled to the size drawn. [progress]
 * is read when the outline is made, so a shape read in a graphics layer or a draw block follows it
 * without recomposition.
 */
class MorphShape(private val morph: Morph, private val progress: () -> Float) : Shape {
    private val matrix = Matrix()
    // Reused frame to frame: an outline is drawn straight away (as in a drawBehind) and not kept.
    private val path = Path()

    override fun createOutline(size: Size, layoutDirection: LayoutDirection, density: Density): Outline {
        path.rewind()
        morph.toPath(progress(), path)
        matrix.reset()
        matrix.scale(size.width, size.height)
        path.transform(matrix)
        return Outline.Generic(path)
    }
}

/**
 * A badge shape that softens towards a circle while [interactions] is pressed and springs back
 * (with [Motion.morph]'s little wobble) when it lifts: touch feedback on a decorative
 * [MaterialShapes] mark, such as the Library tiles' icons. A round mark passes a different [to].
 * Holds still with reduced motion. Draw it fresh each frame (a drawBehind), since the outline's
 * path is reused.
 */
@Composable
fun rememberPressMorphShape(shape: RoundedPolygon, interactions: InteractionSource, to: RoundedPolygon = MaterialShapes.Circle): Shape {
    val reduced = LocalReducedMotion.current
    val progress = remember { Animatable(0f) }
    LaunchedEffect(interactions, reduced) {
        progress.snapTo(0f)
        if (!reduced) animatePresses(progress, interactions, pressed = 1f, rest = 0f, release = Motion.morph)
    }
    return remember(shape, to) { MorphShape(Morph(shape, to)) { progress.value } }
}
