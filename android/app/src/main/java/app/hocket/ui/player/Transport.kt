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
import androidx.compose.ui.layout.layout
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
import androidx.compose.material3.ButtonGroup
import app.hocket.R
import app.hocket.ui.a11y.LocalReducedMotion
import app.hocket.ui.a11y.Spoken
import app.hocket.ui.components.formatClock
import app.hocket.ui.icons.HocketIcons
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
 * A filled transport button drawn in one graphics layer: its shape (from [shapeFor], given the
 * layer's size) and the clip change without recomposition or relayout.
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
    Box(
        modifier
            .graphicsLayer {
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
 * How pressed a button looks, 0 (rest) to 1 (held), on the expressive morph spring: the button group
 * squishes its corners while a finger is down and springs back on release.
 */
@Composable
private fun rememberPressFraction(source: InteractionSource): State<Float> {
    val reduced = LocalReducedMotion.current
    val pressed by source.collectIsPressedAsState()
    val f = remember { Animatable(0f) }
    LaunchedEffect(pressed, reduced) {
        val target = if (pressed && !reduced) 1f else 0f
        if (reduced) f.snapTo(target) else f.animateTo(target, Motion.morph)
    }
    return f.asState()
}

/**
 * Play/pause, the wide middle of the transport's button group (Material 3 Expressive): a filled
 * pill while paused that turns into a rounded rectangle while playing (the toggle button's
 * unchecked and checked shapes), its corners tightening further while pressed, on springy specs.
 * The icon crossfades with a little scale; haptics are toggle on/off. While [buffering] a loading
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
    playingCorner: Dp = 28.dp,
) {
    val reduced = LocalReducedMotion.current
    // 0 = paused (a pill), 1 = playing (rounded rectangle).
    val morphProgress = remember { Animatable(if (playing) 1f else 0f) }
    LaunchedEffect(playing, reduced) {
        val target = if (playing) 1f else 0f
        if (reduced) morphProgress.snapTo(target) else morphProgress.animateTo(target, Motion.morph)
    }
    val press by rememberPressFraction(interactionSource)
    val haptics = LocalHapticFeedback.current
    val label = stringResource(if (playing) R.string.action_pause else R.string.action_play)
    val bufferingLabel = stringResource(R.string.player_buffering)
    val density = LocalDensity.current
    val playingPx = with(density) { playingCorner.toPx() }
    val pressedPx = with(density) { PRESSED_CORNER.toPx() }
    TransportButton(
        onClick = { haptics.performHapticFeedback(if (playing) HapticFeedbackType.ToggleOff else HapticFeedbackType.ToggleOn); onToggle() },
        label = label,
        container = MaterialTheme.colorScheme.primary,
        content = MaterialTheme.colorScheme.onPrimary,
        interactionSource = interactionSource,
        shapeFor = {
            val rest = lerp(size.height / 2f, playingPx.coerceAtMost(size.height / 2f), morphProgress.value.coerceIn(0f, 1f))
            RoundedCornerShape(lerp(rest, pressedPx, press.coerceIn(0f, 1f)))
        },
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
            else Icon(if (p) HocketIcons.Filled.Pause else HocketIcons.Filled.PlayArrow, null, Modifier.size(iconSize))
        }
    }
}

/** The corner every transport button tightens to while pressed. */
private val PRESSED_CORNER = 12.dp

/**
 * Previous, play/pause and next as one Material 3 Expressive button group: tonal previous and next
 * squares with "leaf" corners (round on the outside, tighter towards play/pause) either side of a
 * filled play/pause that takes the rest of the row. A pressed button widens while its neighbours
 * give way ([ButtonGroup]'s `animateWidth`) and its corners squish, on springy specs. [height] is
 * the row's height (and the skip buttons' width). While [buffering] play/pause shows a loading
 * indicator.
 */
@Composable
fun TransportRow(playing: Boolean, onPrevious: () -> Unit, onToggle: () -> Unit, onNext: () -> Unit, height: Dp, modifier: Modifier = Modifier, buffering: Boolean = false) {
    val prevSource = remember { MutableInteractionSource() }
    val playSource = remember { MutableInteractionSource() }
    val nextSource = remember { MutableInteractionSource() }
    val tonal = MaterialTheme.colorScheme.secondaryContainer
    val onTonal = MaterialTheme.colorScheme.onSecondaryContainer
    val previousLabel = stringResource(R.string.action_previous)
    val nextLabel = stringResource(R.string.action_next)
    ButtonGroup(
        overflowIndicator = { },
        modifier = modifier.fillMaxWidth().height(height),
        horizontalArrangement = Arrangement.spacedBy(TRANSPORT_GAP),
    ) {
        customItem(
            buttonGroupContent = {
                SkipButton(onPrevious, previousLabel, HocketIcons.Filled.SkipPrevious, tonal, onTonal, prevSource, leading = true,
                    Modifier.animateWidth(prevSource).size(height).testTag("player.previous"))
            },
            menuContent = { },
        )
        customItem(
            buttonGroupContent = {
                PlayPauseButton(playing, onToggle, Modifier.animateWidth(playSource).weight(1f).height(height), playSource, iconSize = height * 0.4f, buffering = buffering)
            },
            menuContent = { },
        )
        customItem(
            buttonGroupContent = {
                SkipButton(onNext, nextLabel, HocketIcons.Filled.SkipNext, tonal, onTonal, nextSource, leading = false,
                    Modifier.animateWidth(nextSource).size(height).testTag("player.next"))
            },
            menuContent = { },
        )
    }
}

/** The gap between the transport's buttons. */
private val TRANSPORT_GAP = 10.dp

/**
 * A skip button: a tonal square with leaf corners (about 3/8 of its height on the outer side, 1/6
 * towards play/pause), squishing towards [PRESSED_CORNER] while pressed.
 */
@Composable
private fun SkipButton(onClick: () -> Unit, label: String, icon: ImageVector, container: Color, content: Color, source: MutableInteractionSource, leading: Boolean, modifier: Modifier) {
    val press by rememberPressFraction(source)
    val density = LocalDensity.current
    val pressedPx = with(density) { PRESSED_CORNER.toPx() }
    TransportButton(onClick, label, container, content, source, modifier, shapeFor = {
        val p = press.coerceIn(0f, 1f)
        val outer = lerp(size.height * 0.36f, pressedPx, p)
        val inner = lerp(size.height * 0.17f, pressedPx, p)
        // Outer is the side away from play/pause: the start for previous, the end for next (the
        // shape follows the layout direction).
        val startSide = if (leading) outer else inner
        val endSide = if (leading) inner else outer
        RoundedCornerShape(topStart = startSide, bottomStart = startSide, topEnd = endSide, bottomEnd = endSide)
    }) {
        Icon(icon, null, Modifier.size(32.dp))
    }
}

/**
 * Wavy progress with tap- and drag-to-seek (Metrolist/Navic): the wave's amplitude eases to flat
 * while paused or dragging, the thumb follows the finger, and after a release (or a tap) the bar
 * holds the new position until the core reports it (or [SEEK_HOLD_MS] passes) instead of snapping
 * back to the old one. Elapsed and remaining time sit underneath, with [centre] (a status line)
 * between them.
 *
 * [position] is read in the draw phase only; the rest of the bar changes once a second at most.
 *
 * Accessibility: one adjustable node, "Playback position, 1 minute 32 seconds of 3 minutes 32
 * seconds", with a range in whole seconds (so it changes at most once a second, never per frame)
 * and `setProgress` for the screen reader's adjust gestures.
 */
@Composable
fun WavySeekBar(position: () -> Long, durationMs: Long, playing: Boolean, onSeek: (Long) -> Unit, modifier: Modifier = Modifier, centre: (@Composable () -> Unit)? = null) {
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
    Column(modifier.fillMaxWidth()) {
    Box(
        Modifier.fillMaxWidth().testTag("player.seekBar").clearAndSetSemantics {
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
                // A 48 dp touch target; the times tuck up into its slack below the wave.
                .height(48.dp)
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
    }
    SeekTimes(positionS * 1000, durationMs, centre)
    }
}

/**
 * Elapsed at the start, remaining ("−0:47") at the end, [centre] between them, kept clear of both.
 * The times are read by the seek bar's own node, so they stay out of the accessibility tree.
 */
@Composable
private fun SeekTimes(positionMs: Long, durationMs: Long, centre: (@Composable () -> Unit)?) {
    val style = MaterialTheme.typography.labelMedium
    val color = MaterialTheme.colorScheme.onSurfaceVariant
    val remaining = stringResource(R.string.player_remaining, formatClock((durationMs - positionMs).coerceAtLeast(0)))
    Row(
        Modifier.fillMaxWidth().layout { m, c ->
            val p = m.measure(c)
            val pull = 8.dp.roundToPx()
            layout(p.width, (p.height - pull).coerceAtLeast(0)) { p.place(0, -pull) }
        },
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Text(formatClock(positionMs), style = style, color = color, modifier = Modifier.clearAndSetSemantics { }, maxLines = 1)
        Box(Modifier.weight(1f).padding(horizontal = 12.dp), contentAlignment = Alignment.Center) { centre?.invoke() }
        Text(remaining, style = style, color = color, modifier = Modifier.clearAndSetSemantics { }, maxLines = 1)
    }
}

/** How long a seek's target is held on screen at most while waiting for the core to report it. */
private const val SEEK_HOLD_MS = 2_500L

/** Linear interpolation for layer maths. */
internal fun lerpF(a: Float, b: Float, t: Float): Float = lerp(a, b, t)
