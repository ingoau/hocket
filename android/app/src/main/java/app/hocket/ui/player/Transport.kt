package app.hocket.ui.player

import androidx.compose.animation.AnimatedContent
import androidx.compose.animation.core.Animatable
import androidx.compose.animation.core.FastOutSlowInEasing
import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.animation.core.animateDpAsState
import androidx.compose.material3.MaterialShapes
import androidx.compose.material3.toPath
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.GraphicsLayerScope
import androidx.compose.ui.graphics.Matrix
import androidx.compose.ui.graphics.Outline
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.Shape
import androidx.compose.ui.unit.Density
import androidx.compose.ui.unit.LayoutDirection
import androidx.graphics.shapes.Morph
import androidx.compose.animation.core.tween
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.scaleIn
import androidx.compose.animation.scaleOut
import androidx.compose.animation.togetherWith
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.gestures.detectHorizontalDragGestures
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.interaction.InteractionSource
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.interaction.PressInteraction
import androidx.compose.foundation.interaction.collectIsPressedAsState
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.RowScope
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Pause
import androidx.compose.material.icons.filled.PlayArrow
import androidx.compose.material.icons.filled.SkipNext
import androidx.compose.material.icons.filled.SkipPrevious
import androidx.compose.material3.Icon
import androidx.compose.material3.LinearWavyProgressIndicator
import androidx.compose.material3.LoadingIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.ripple
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.State
import androidx.compose.runtime.derivedStateOf
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.setValue
import androidx.compose.runtime.snapshotFlow
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.drawWithContent
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.hapticfeedback.HapticFeedbackType
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.LocalHapticFeedback
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.ProgressBarRangeInfo
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.progressBarRangeInfo
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.setProgress
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.util.lerp
import app.hocket.R
import app.hocket.ui.a11y.LocalReducedMotion
import app.hocket.ui.a11y.Spoken
import app.hocket.ui.components.formatClock
import app.hocket.ui.theme.Motion
import kotlinx.coroutines.flow.collectLatest
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.withTimeoutOrNull
import kotlin.math.abs

/**
 * A Navic-style press bounce: the control dips while held, then pops slightly past rest on release
 * and springs back. Drawn with a graphics layer (no relayout); off with reduced motion.
 */
@Composable
fun rememberPressScale(interactionSource: InteractionSource): State<Float> {
    val reduced = LocalReducedMotion.current
    val scale = remember { Animatable(1f) }
    LaunchedEffect(interactionSource, reduced) {
        if (reduced) { scale.snapTo(1f); return@LaunchedEffect }
        interactionSource.interactions.collectLatest { i ->
            when (i) {
                is PressInteraction.Press -> scale.animateTo(0.94f, Motion.quick)
                is PressInteraction.Release, is PressInteraction.Cancel -> {
                    scale.animateTo(1.04f, tween(100, easing = FastOutSlowInEasing))
                    scale.animateTo(1f, Motion.pressRelease)
                }
            }
        }
    }
    return scale.asState()
}

/**
 * A filled transport button drawn in one graphics layer: the press bounce, its shape (from
 * [shapeFor], given the layer's size) and the clip all change without recomposition or relayout.
 */
@Composable
private fun TransportButton(
    onClick: () -> Unit,
    label: String,
    container: Color,
    content: Color,
    interactionSource: MutableInteractionSource,
    modifier: Modifier = Modifier,
    shapeFor: GraphicsLayerScope.() -> Shape,
    icon: @Composable () -> Unit,
) {
    val scale by rememberPressScale(interactionSource)
    Box(
        modifier
            .graphicsLayer {
                scaleX = scale
                scaleY = scale
                shape = shapeFor()
                clip = true
            }
            .background(container)
            .clickable(interactionSource = interactionSource, indication = ripple(color = content), role = Role.Button, onClick = onClick)
            .semantics { contentDescription = label },
        contentAlignment = Alignment.Center,
    ) {
        androidx.compose.runtime.CompositionLocalProvider(androidx.compose.material3.LocalContentColor provides content) { icon() }
    }
}

/**
 * Play/pause as a big filled button whose outline morphs (Material 3 Expressive): a soft
 * nine-sided cookie while paused, a rounded square while playing, on a springy spec. The icon
 * crossfades with a little scale; haptics are toggle on/off. While [buffering] a loading
 * indicator takes the icon's place (the button still toggles, and says it is buffering).
 */
@Composable
fun PlayPauseButton(
    playing: Boolean,
    onToggle: () -> Unit,
    modifier: Modifier = Modifier,
    interactionSource: MutableInteractionSource = remember { MutableInteractionSource() },
    iconSize: Dp = 40.dp,
    buffering: Boolean = false,
) {
    val reduced = LocalReducedMotion.current
    // 0 = paused (cookie), 1 = playing (rounded square).
    val morphProgress = remember { Animatable(if (playing) 1f else 0f) }
    LaunchedEffect(playing, reduced) {
        val target = if (playing) 1f else 0f
        if (reduced) morphProgress.snapTo(target) else morphProgress.animateTo(target, Motion.morph)
    }
    val morph = remember { Morph(MaterialShapes.Cookie9Sided, MaterialShapes.Square) }
    val haptics = LocalHapticFeedback.current
    val label = stringResource(if (playing) R.string.action_pause else R.string.action_play)
    val bufferingLabel = stringResource(R.string.player_buffering)
    TransportButton(
        onClick = { haptics.performHapticFeedback(if (playing) HapticFeedbackType.ToggleOff else HapticFeedbackType.ToggleOn); onToggle() },
        label = label,
        container = MaterialTheme.colorScheme.primary,
        content = MaterialTheme.colorScheme.onPrimary,
        interactionSource = interactionSource,
        shapeFor = { MorphShape(morph, morphProgress.value) },
        modifier = modifier.testTag("player.playPause").semantics { if (buffering) stateDescription = bufferingLabel },
    ) {
        AnimatedContent(
            targetState = if (buffering) null else playing,
            transitionSpec = {
                if (reduced) fadeIn(tween(0)) togetherWith fadeOut(tween(0))
                else (fadeIn(tween(150)) + scaleIn(initialScale = 0.6f)) togetherWith (fadeOut(tween(100)) + scaleOut(targetScale = 0.6f))
            },
            label = "playPauseIcon",
        ) { p ->
            if (p == null) LoadingIndicator(Modifier.size(iconSize * 1.2f), color = androidx.compose.material3.LocalContentColor.current)
            else Icon(if (p) Icons.Filled.Pause else Icons.Filled.PlayArrow, null, Modifier.size(iconSize))
        }
    }
}

/**
 * A [Morph] between two of [MaterialShapes] (normalised to a unit square) at [progress], scaled to
 * the outline's size. Cheap to create: a graphics layer builds one per frame while it animates.
 */
private class MorphShape(private val morph: Morph, private val progress: Float) : Shape {
    override fun createOutline(size: Size, layoutDirection: LayoutDirection, density: Density): Outline {
        val path = morph.toPath(progress, Path())
        path.transform(Matrix().apply { scale(size.width, size.height) })
        return Outline.Generic(path)
    }
}

/**
 * Previous, play/pause and next (Material 3 Expressive): tonal round skip buttons either side of a
 * larger shape-morphing play/pause. A pressed skip button widens and squishes towards a rounded
 * square while its neighbours give way a little, on springy specs; every button bounces on press.
 * [height] is the play button's size; the skip buttons are a little smaller. While [buffering] the
 * play button shows a loading indicator.
 */
@Composable
fun TransportRow(playing: Boolean, onPrevious: () -> Unit, onToggle: () -> Unit, onNext: () -> Unit, height: Dp, modifier: Modifier = Modifier, buffering: Boolean = false) {
    val reduced = LocalReducedMotion.current
    val prevSource = remember { MutableInteractionSource() }
    val playSource = remember { MutableInteractionSource() }
    val nextSource = remember { MutableInteractionSource() }
    val prevPressed by prevSource.collectIsPressedAsState()
    val playPressed by playSource.collectIsPressedAsState()
    val nextPressed by nextSource.collectIsPressedAsState()
    val skip = height * 0.8f
    fun width(pressed: Boolean, otherPressed: Boolean) = if (reduced) skip else if (pressed) skip * 1.35f else if (otherPressed) skip * 0.92f else skip
    val prevWidth by animateDpAsState(width(prevPressed, playPressed || nextPressed), Motion.pressDp, label = "prevWidth")
    val nextWidth by animateDpAsState(width(nextPressed, playPressed || prevPressed), Motion.pressDp, label = "nextWidth")
    val tonal = MaterialTheme.colorScheme.secondaryContainer
    val onTonal = MaterialTheme.colorScheme.onSecondaryContainer
    Row(modifier.fillMaxWidth().height(height), horizontalArrangement = Arrangement.spacedBy(20.dp, Alignment.CenterHorizontally), verticalAlignment = Alignment.CenterVertically) {
        SkipButton(onPrevious, stringResource(R.string.action_previous), Icons.Filled.SkipPrevious, tonal, onTonal, prevSource, prevPressed, Modifier.size(prevWidth, skip).testTag("player.previous"))
        PlayPauseButton(playing, onToggle, Modifier.size(height), playSource, iconSize = height * 0.45f, buffering = buffering)
        SkipButton(onNext, stringResource(R.string.action_next), Icons.Filled.SkipNext, tonal, onTonal, nextSource, nextPressed, Modifier.size(nextWidth, skip).testTag("player.next"))
    }
}

@Composable
private fun SkipButton(onClick: () -> Unit, label: String, icon: ImageVector, container: Color, content: Color, source: MutableInteractionSource, pressed: Boolean, modifier: Modifier) {
    val reduced = LocalReducedMotion.current
    // Round at rest, squishing to a rounded square while pressed.
    val corner = remember { Animatable(0.5f) }
    LaunchedEffect(pressed, reduced) {
        val target = if (pressed && !reduced) 0.28f else 0.5f
        if (reduced) corner.snapTo(target) else corner.animateTo(target, Motion.morph)
    }
    TransportButton(onClick, label, container, content, source, modifier, shapeFor = { RoundedCornerShape(size.height * corner.value.coerceIn(0f, 0.5f)) }) {
        Icon(icon, null, Modifier.size(32.dp))
    }
}

/**
 * Wavy progress with tap- and drag-to-seek (Metrolist/Navic): the wave's amplitude eases to flat
 * while paused or dragging, the thumb follows the finger, and after a release (or a tap) the bar
 * holds the new position until the core reports it (or [SEEK_HOLD_MS] passes) instead of snapping
 * back to the old one. Elapsed and total time sit underneath.
 *
 * [position] is read in the draw phase only; the rest of the bar changes once a second at most.
 *
 * Accessibility: one adjustable node, "Playback position, 1 minute 32 seconds of 3 minutes 32
 * seconds", with a range in whole seconds (so it changes at most once a second, never per frame)
 * and `setProgress` for the screen reader's adjust gestures.
 */
@Composable
fun WavySeekBar(position: () -> Long, durationMs: Long, playing: Boolean, onSeek: (Long) -> Unit, modifier: Modifier = Modifier) {
    val density = LocalDensity.current
    val haptics = LocalHapticFeedback.current
    val currentPosition by rememberUpdatedState(position)
    val currentDuration by rememberUpdatedState(durationMs)
    // Non-null while a finger drags the thumb.
    var dragFraction by remember { mutableStateOf<Float?>(null) }
    // A seek sent to the core that it has not reported back yet: shown instead of the stale position.
    var pending by remember { mutableStateOf<Long?>(null) }
    val shownMs: () -> Long = remember {
        { dragFraction?.let { (it * currentDuration).toLong() } ?: pending ?: currentPosition() }
    }
    val fraction: () -> Float = remember { { if (currentDuration > 0) (shownMs().toFloat() / currentDuration).coerceIn(0f, 1f) else 0f } }
    LaunchedEffect(pending) {
        val target = pending ?: return@LaunchedEffect
        withTimeoutOrNull(SEEK_HOLD_MS) { snapshotFlow { currentPosition() }.first { abs(it - target) < 1_000 } }
        pending = null
    }
    val seek: (Long) -> Unit = { ms -> pending = ms; onSeek(ms) }
    val amplitude by animateFloatAsState(if (playing && dragFraction == null) 1f else 0f, tween(400), label = "waveAmplitude")
    val label = stringResource(R.string.player_seek_a11y)
    val resources = androidx.compose.ui.platform.LocalResources.current
    // Whole seconds: the semantics and the labels move once a second at most.
    val positionS by remember { derivedStateOf { (shownMs() / 1000).coerceAtLeast(0) } }
    val durationS = (durationMs / 1000).coerceAtLeast(0)
    val state = remember(positionS, durationS) { Spoken.position(resources, positionS * 1000, durationS * 1000) }
    val active = MaterialTheme.colorScheme.primary
    val track = MaterialTheme.colorScheme.onSurface.copy(alpha = 0.2f)
    Column(
        modifier.fillMaxWidth().testTag("player.seekBar").clearAndSetSemantics {
            contentDescription = label
            stateDescription = state
            progressBarRangeInfo = ProgressBarRangeInfo(positionS.toFloat().coerceAtMost(durationS.toFloat()), 0f..durationS.toFloat().coerceAtLeast(1f), steps = 0)
            setProgress(label) { value ->
                if (durationMs <= 0) return@setProgress false
                seek((value.coerceIn(0f, durationS.toFloat()) * 1000).toLong())
                true
            }
        },
    ) {
        Box(
            Modifier
                .fillMaxWidth()
                .height(36.dp)
                .testTag("player.seek")
                .pointerInput(durationMs) {
                    detectTapGestures(onTap = { o ->
                        if (durationMs > 0) {
                            haptics.performHapticFeedback(HapticFeedbackType.SegmentTick)
                            seek(((o.x / size.width).coerceIn(0f, 1f) * durationMs).toLong())
                        }
                    })
                }
                .pointerInput(durationMs) {
                    detectHorizontalDragGestures(
                        onDragStart = { o -> dragFraction = (o.x / size.width).coerceIn(0f, 1f); haptics.performHapticFeedback(HapticFeedbackType.GestureThresholdActivate) },
                        onDragEnd = {
                            dragFraction?.let { if (durationMs > 0) seek((it * durationMs).toLong()) }
                            dragFraction = null
                            haptics.performHapticFeedback(HapticFeedbackType.GestureEnd)
                        },
                        onDragCancel = { dragFraction = null },
                    ) { change, _ ->
                        change.consume()
                        dragFraction = (change.position.x / size.width).coerceIn(0f, 1f)
                    }
                }
                // The thumb: a rounded bar drawn over the wave at the shown position (draw phase only).
                .drawWithContent {
                    drawContent()
                    val dragging = dragFraction != null
                    val w = (if (dragging) 6.dp else 4.dp).toPx()
                    val h = (if (dragging) 28.dp else 20.dp).toPx()
                    val x = fraction() * size.width
                    drawRoundRect(active, topLeft = Offset((x - w / 2).coerceIn(0f, size.width - w), center.y - h / 2), size = Size(w, h), cornerRadius = CornerRadius(w / 2))
                },
            contentAlignment = Alignment.CenterStart,
        ) {
            LinearWavyProgressIndicator(
                progress = fraction,
                modifier = Modifier.fillMaxWidth().height(14.dp),
                color = active,
                trackColor = track,
                stroke = Stroke(width = with(density) { 4.dp.toPx() }, cap = StrokeCap.Round),
                trackStroke = Stroke(width = with(density) { 4.dp.toPx() }, cap = StrokeCap.Round),
                gapSize = 8.dp,
                amplitude = { amplitude },
                wavelength = 32.dp,
            )
        }
        SeekTimes(positionS * 1000, durationMs)
    }
}

@Composable
private fun SeekTimes(positionMs: Long, durationMs: Long) {
    val style = MaterialTheme.typography.labelMedium
    val color = MaterialTheme.colorScheme.onSurfaceVariant
    Row(Modifier.fillMaxWidth().padding(top = 2.dp)) {
        Text(formatClock(positionMs), style = style, color = color, modifier = Modifier.weight(1f))
        Text(formatClock(durationMs.coerceAtLeast(0)), style = style, color = color)
    }
}

/** How long a seek's target is held on screen at most while waiting for the core to report it. */
private const val SEEK_HOLD_MS = 2_500L

/** Linear interpolation for layer maths. */
internal fun lerpF(a: Float, b: Float, t: Float): Float = lerp(a, b, t)
