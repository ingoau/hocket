package app.hocket.ui.components

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.gestures.Orientation
import androidx.compose.foundation.gestures.draggable
import androidx.compose.foundation.gestures.rememberDraggableState
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.produceState
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Shape
import androidx.compose.ui.layout.LayoutCoordinates
import androidx.compose.ui.layout.onGloballyPositioned
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.LocalView
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.ProgressBarRangeInfo
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.progressBarRangeInfo
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import app.hocket.core.Commands
import app.hocket.core.api.ActionDescriptor
import app.hocket.core.api.ActionTarget
import app.hocket.core.api.Command
import app.hocket.core.api.PreviewState
import app.hocket.ui.LocalCoreClient
import app.hocket.ui.icons.HocketIcons
import kotlinx.coroutines.delay

/** Clear of the system back gesture, which starts at the screen edge. */
private val EDGE = 24.dp

/**
 * The Preview row. Tapping it does not close the menu: while the preview plays the row is a small
 * player, the progress drawn behind the label. Tap pauses or resumes; a horizontal drag moves the
 * progress by as far as the finger travels (like an iOS slider, it doesn't jump to the finger),
 * staying clear of the screen edges so the back gesture still owns them. A vertical drag is left
 * to the menu's scroll.
 */
@Composable
fun PreviewMenuRow(action: ActionDescriptor, trackId: String?, target: ActionTarget, shape: Shape, tag: String) {
    val client = LocalCoreClient.current
    val preview by client.preview.collectAsStateWithLifecycle()
    val sample = preview?.takeIf { trackId == null || it.trackId == trackId }
    val now by produceState(System.currentTimeMillis().toDouble(), sample?.playing, sample?.takenAt, sample?.positionMs) {
        while (true) {
            value = System.currentTimeMillis().toDouble()
            if (sample?.playing != true) break
            delay(80)
        }
    }
    val duration = sample?.durationMs?.toLong() ?: 0L
    val position = if (sample == null) 0L else previewPositionMs(sample.positionMs.toLong(), sample.takenAt, sample.playing, duration, now)
    val fraction = if (duration > 0) (position.toFloat() / duration).coerceIn(0f, 1f) else 0f
    val spoken = when {
        sample == null -> action.label
        sample.playing -> "${action.label}, playing"
        else -> "${action.label}, paused"
    }
    var width by remember { mutableFloatStateOf(1f) }
    var coords by remember { mutableStateOf<LayoutCoordinates?>(null) }
    var dragFraction by remember { mutableFloatStateOf(Float.NaN) }
    var ignoreDrag by remember { mutableStateOf(false) }
    // Where the drag began and how far the finger has gone: the bar moves by the drag, not to the finger.
    var dragAnchor by remember { mutableFloatStateOf(0f) }
    var dragDistance by remember { mutableFloatStateOf(0f) }
    // A released seek, shown until the core's preview catches up so the bar doesn't snap back.
    var pending by remember { mutableFloatStateOf(Float.NaN) }
    var pendingFrom by remember { mutableStateOf<PreviewState?>(null) }
    LaunchedEffect(sample, pending) {
        if (pending.isNaN()) return@LaunchedEffect
        if (sample == null || (sample !== pendingFrom && previewSeekLanded(fraction, pending, duration))) {
            pending = Float.NaN
            return@LaunchedEffect
        }
        delay(2_000)
        pending = Float.NaN
    }
    val shown = when {
        !dragFraction.isNaN() -> dragFraction
        !pending.isNaN() -> pending
        else -> fraction
    }
    val density = LocalDensity.current
    val view = LocalView.current
    val edgePx = with(density) { EDGE.toPx() }
    val scheme = MaterialTheme.colorScheme
    val drag = rememberDraggableState { delta ->
        if (ignoreDrag || duration <= 0) return@rememberDraggableState
        dragDistance += delta
        dragFraction = previewDragFraction(dragAnchor, dragDistance, width)
    }

    Box(
        Modifier
            .fillMaxWidth()
            .heightIn(min = 52.dp)
            .clip(shape)
            .background(menuRowColor())
            .onGloballyPositioned {
                coords = it
                width = it.size.width.toFloat().coerceAtLeast(1f)
            }
            .clickable(enabled = action.enabled) {
                if (sample == null) {
                    if (trackId != null) client.dispatch(Commands.previewStart(trackId))
                    else client.dispatch(Commands.runAction(action.id, target))
                } else {
                    client.dispatch(Command.PreviewToggle)
                }
            }
            .draggable(
                state = drag,
                orientation = Orientation.Horizontal,
                enabled = sample != null && action.enabled,
                onDragStarted = { start ->
                    val windowX = coords?.localToRoot(start)?.x ?: start.x
                    ignoreDrag = windowX < edgePx || windowX > view.width - edgePx
                    dragAnchor = shown
                    dragDistance = 0f
                    if (!ignoreDrag && duration > 0) dragFraction = dragAnchor
                },
                onDragStopped = {
                    if (!ignoreDrag && !dragFraction.isNaN() && duration > 0) {
                        client.dispatch(Commands.previewSeek((dragFraction * duration).toLong()))
                        pending = dragFraction
                        pendingFrom = sample
                    }
                    dragFraction = Float.NaN
                    ignoreDrag = false
                },
            )
            .testTag(tag)
            .semantics {
                contentDescription = spoken
                if (sample != null && duration > 0) progressBarRangeInfo = ProgressBarRangeInfo(shown, 0f..1f)
            },
    ) {
        if (sample != null) {
            // matchParentSize: the menu scrolls, so a plain fillMaxHeight has no bounded height and draws nothing.
            Box(Modifier.matchParentSize()) {
                Box(Modifier.fillMaxHeight().fillMaxWidth(shown).background(scheme.primary.copy(alpha = 0.38f)))
            }
        }
        Row(
            Modifier.padding(horizontal = 16.dp, vertical = 4.dp).heightIn(min = 44.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Icon(
                if (sample?.playing == true) HocketIcons.Filled.Pause else HocketIcons.Filled.Headphones,
                null,
                tint = scheme.onSurfaceVariant,
            )
            Text(
                action.label,
                style = MaterialTheme.typography.bodyLarge,
                color = scheme.onSurface,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
                modifier = Modifier.padding(start = 16.dp),
            )
        }
    }
}
