package app.hocket.ui.player

import androidx.compose.animation.core.Animatable
import androidx.compose.foundation.gestures.detectHorizontalDragGestures
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Pause
import androidx.compose.material.icons.filled.PlayArrow
import androidx.compose.material3.FilledIconButton
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButtonDefaults
import androidx.compose.material3.LinearWavyProgressIndicator
import androidx.compose.material3.MaterialShapes
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.toPath
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Matrix
import androidx.compose.ui.graphics.Outline
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.Shape
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.hapticfeedback.HapticFeedbackType
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.LocalHapticFeedback
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.semantics.ProgressBarRangeInfo
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.progressBarRangeInfo
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.setProgress
import androidx.compose.ui.semantics.stateDescription
import app.hocket.ui.a11y.Spoken
import androidx.compose.ui.unit.Density
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.IntOffset
import androidx.compose.ui.unit.LayoutDirection
import androidx.compose.ui.unit.dp
import androidx.graphics.shapes.Morph
import app.hocket.R
import app.hocket.ui.components.formatClock
import app.hocket.ui.theme.Motion
import kotlinx.coroutines.launch
import kotlin.math.roundToInt

/** A [Shape] that renders a [Morph] at a given progress, scaled to the layout size. */
class MorphShape(private val morph: Morph, private val progress: Float) : Shape {
    override fun createOutline(size: Size, layoutDirection: LayoutDirection, density: Density): Outline {
        val path: Path = morph.toPath(progress = progress)
        val matrix = Matrix()
        matrix.scale(size.width, size.height)
        path.transform(matrix)
        return Outline.Generic(path)
    }
}

/** Play/pause with shape morphing: a soft cookie when paused, a rounded square while playing. */
@Composable
fun PlayPauseButton(playing: Boolean, onToggle: () -> Unit, modifier: Modifier = Modifier, size: Dp = 88.dp) {
    val morph = remember { Morph(MaterialShapes.Cookie9Sided, MaterialShapes.Square) }
    val progress = remember { Animatable(if (playing) 1f else 0f) }
    LaunchedEffect(playing) { progress.animateTo(if (playing) 1f else 0f, Motion.snap) }
    val haptics = LocalHapticFeedback.current
    val label = stringResource(if (playing) R.string.action_pause else R.string.action_play)
    FilledIconButton(
        onClick = { haptics.performHapticFeedback(HapticFeedbackType.Confirm); onToggle() },
        shape = MorphShape(morph, progress.value),
        colors = IconButtonDefaults.filledIconButtonColors(),
        modifier = modifier.size(size).semantics { contentDescription = label }.testTag("player.playPause"),
    ) {
        Icon(if (playing) Icons.Filled.Pause else Icons.Filled.PlayArrow, null, Modifier.size(size * 0.45f))
    }
}

/**
 * Wavy progress with drag-to-seek: while dragging, the wave flattens, the thumb follows the finger
 * and a time bubble floats above it. The seek is dispatched on release.
 *
 * Accessibility: one adjustable node, "Playback position, 1 minute 32 seconds of 3 minutes 32
 * seconds", with a range in whole seconds (so it changes at most once a second, never per frame)
 * and `setProgress` for the screen reader's adjust gestures (steps of 5% of the track).
 */
@Composable
fun WavySeekBar(positionMs: Long, durationMs: Long, playing: Boolean, onSeek: (Long) -> Unit, modifier: Modifier = Modifier) {
    val density = LocalDensity.current
    val haptics = LocalHapticFeedback.current
    var dragging by remember { mutableStateOf(false) }
    var dragFraction by remember { mutableFloatStateOf(0f) }
    var widthPx by remember { mutableFloatStateOf(1f) }
    val fraction = if (dragging) dragFraction else if (durationMs > 0) (positionMs.toFloat() / durationMs).coerceIn(0f, 1f) else 0f
    val shownMs = if (dragging) (dragFraction * durationMs).toLong() else positionMs
    val label = stringResource(R.string.player_seek_a11y)
    val resources = LocalContext.current.resources
    // Whole seconds: the semantics (and anything announced from them) move once a second at most.
    val positionS = (shownMs / 1000).coerceAtLeast(0)
    val durationS = (durationMs / 1000).coerceAtLeast(0)
    val state = remember(positionS, durationS) { Spoken.position(resources, positionS * 1000, durationS * 1000) }
    Column(
        modifier.fillMaxWidth().testTag("player.seekBar").clearAndSetSemantics {
            contentDescription = label
            stateDescription = state
            progressBarRangeInfo = ProgressBarRangeInfo(positionS.toFloat().coerceAtMost(durationS.toFloat()), 0f..durationS.toFloat().coerceAtLeast(1f), steps = 0)
            setProgress(label) { value ->
                if (durationMs <= 0) return@setProgress false
                onSeek((value.coerceIn(0f, durationS.toFloat()) * 1000).toLong())
                true
            }
        },
    ) {
        Box(Modifier.fillMaxWidth().height(28.dp)) {
            if (dragging) {
                val x = (dragFraction * widthPx).roundToInt()
                Surface(shape = MaterialTheme.shapes.small, color = MaterialTheme.colorScheme.inverseSurface, modifier = Modifier.offset { IntOffset((x - with(density) { 24.dp.roundToPx() }).coerceIn(0, (widthPx - with(density) { 48.dp.toPx() }).roundToInt().coerceAtLeast(0)), 0) }) {
                    Text(formatClock(shownMs), color = MaterialTheme.colorScheme.inverseOnSurface, style = MaterialTheme.typography.labelMedium, modifier = Modifier.padding(horizontal = 8.dp, vertical = 4.dp).width(32.dp))
                }
            }
        }
        Box(
            Modifier
                .fillMaxWidth()
                .height(32.dp)
                .testTag("player.seek")
                .pointerInput(durationMs) {
                    widthPx = size.width.toFloat()
                    detectHorizontalDragGestures(
                        onDragStart = { offset -> dragging = true; dragFraction = (offset.x / size.width).coerceIn(0f, 1f); haptics.performHapticFeedback(HapticFeedbackType.GestureThresholdActivate) },
                        onDragEnd = { dragging = false; onSeek((dragFraction * durationMs).toLong()); haptics.performHapticFeedback(HapticFeedbackType.GestureEnd) },
                        onDragCancel = { dragging = false },
                    ) { change, _ ->
                        change.consume()
                        dragFraction = (change.position.x / size.width).coerceIn(0f, 1f)
                    }
                },
            contentAlignment = Alignment.CenterStart,
        ) {
            LinearWavyProgressIndicator(
                progress = { fraction },
                modifier = Modifier.fillMaxWidth().height(14.dp),
                stroke = Stroke(width = with(density) { 5.dp.toPx() }, cap = androidx.compose.ui.graphics.StrokeCap.Round),
                trackStroke = Stroke(width = with(density) { 5.dp.toPx() }, cap = androidx.compose.ui.graphics.StrokeCap.Round),
                amplitude = { if (playing && !dragging) 1f else 0f },
                wavelength = 32.dp,
            )
            // Thumb
            Box(Modifier.offset { IntOffset((fraction * widthPx - with(density) { 3.dp.toPx() }).roundToInt(), 0) }.width(6.dp).height(22.dp)) {
                Surface(shape = MaterialTheme.shapes.extraLarge, color = MaterialTheme.colorScheme.primary, modifier = Modifier.size(6.dp, 22.dp)) {}
            }
        }
        Row(Modifier.fillMaxWidth().padding(top = 4.dp)) {
            Text(formatClock(shownMs), style = MaterialTheme.typography.labelMedium, color = MaterialTheme.colorScheme.onSurfaceVariant, modifier = Modifier.weight(1f))
            Text(formatClock(durationMs), style = MaterialTheme.typography.labelMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
        }
    }
}
