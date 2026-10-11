package app.hocket.ui.player

import androidx.compose.animation.core.Animatable
import androidx.compose.animation.core.Spring
import androidx.compose.animation.core.spring
import androidx.compose.foundation.gestures.Orientation
import androidx.compose.foundation.gestures.draggable
import androidx.compose.foundation.gestures.rememberDraggableState
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.runtime.Composable
import androidx.compose.runtime.Immutable
import androidx.compose.runtime.Stable
import androidx.compose.runtime.State
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.produceState
import androidx.compose.runtime.setValue
import androidx.compose.runtime.withFrameNanos
import androidx.compose.ui.Modifier
import androidx.compose.ui.hapticfeedback.HapticFeedback
import androidx.compose.ui.hapticfeedback.HapticFeedbackType
import androidx.compose.ui.unit.dp
import app.hocket.core.api.Command
import app.hocket.core.api.QueueEntry
import app.hocket.core.api.QueueView
import app.hocket.core.api.RepeatMode
import app.hocket.core.client.CoreClient
import app.hocket.ui.LocalCoreClient
import app.hocket.ui.theme.Motion
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.launch
import kotlinx.coroutines.withTimeoutOrNull
import kotlin.math.abs
import kotlin.math.sign

/**
 * What a skip swipe peeks at: the next item (the first of "playing next", else the first upcoming,
 * autoplay's included; with repeat on, the queue's start again) and the previous one (the last of
 * the history). No next at the end of a queue that does not repeat: the swipe then rubber-bands.
 */
@Immutable
internal data class Neighbours(val previous: QueueEntry?, val next: QueueEntry?) {
    companion object {
        val None = Neighbours(null, null)

        fun of(queue: QueueView): Neighbours {
            val next = queue.playingNext.firstOrNull() ?: queue.upcoming.firstOrNull()
                ?: if (queue.repeat != RepeatMode.Off) queue.history.firstOrNull() ?: queue.current else null
            return Neighbours(previous = queue.history.lastOrNull(), next = next)
        }
    }
}

/** The current queue's [Neighbours], recomputed only when the queue changes. */
@Composable
internal fun rememberNeighbours(): State<Neighbours> {
    val client = LocalCoreClient.current
    return produceState(Neighbours.of(client.queue.value), client) {
        client.queue.collect { value = Neighbours.of(it) }
    }
}

/**
 * The core's rule (`hocket_core::session::reducer::previous_should_restart`): Previous restarts the
 * track from this far in, and when there is nothing before it.
 */
internal const val PREVIOUS_RESTART_MS = 3_000L

/**
 * A three-slot carousel (previous, current, next) under the finger: [offset] follows it 1:1 (px,
 * negative towards next). Released past half of [span] (the slot's width plus the gap to its
 * neighbour) or flung, it settles on the neighbour, sends Next / Previous, and once the new track
 * is current snaps back to 0, the neighbour now being the current one; otherwise it springs back.
 * Where there is no neighbour it rubber-bands as the old skip swipe did (and still skips past
 * [rubberThreshold], so a swipe past the end of the queue ends it). [restartPeek] says the previous
 * slot shows the current track again (Previous will restart it).
 */
@Stable
internal class SkipCarousel {
    val offset = Animatable(0f)
    var span by mutableStateOf(1f)
    var restartPeek by mutableStateOf(false)
        private set

    /** -1..1: how far towards the previous (positive) or next (negative) neighbour. */
    fun fraction(): Float = if (span <= 1f) 0f else (offset.value / span).coerceIn(-1f, 1f)

    /**
     * The swipe on whatever this modifies: Material's `draggable`, so a release's velocity counts the
     * finger's last moves only (a finger that stopped before lifting does not fling).
     */
    @Composable
    fun Modifier.gesture(
        client: CoreClient,
        scope: CoroutineScope,
        haptics: HapticFeedback,
        enabled: Boolean,
        reduced: Boolean,
        hasNext: () -> Boolean,
        rubberThreshold: Float,
    ): Modifier {
        val density = LocalDensity.current
        val fling = with(density) { 500.dp.toPx() }
        val minFling = with(density) { 24.dp.toPx() }
        val drag = remember { DragTrack() }
        val currentHasNext by rememberUpdatedState(hasNext)
        fun slotFor(d: Float) = d > 0f || (d < 0f && currentHasNext())
        val state = rememberDraggableState { delta ->
            drag.raw += delta
            val raw = drag.raw
            val slot = slotFor(raw)
            val shown = when {
                reduced -> 0f
                slot -> raw.coerceIn(-span, span)
                abs(raw) > rubberThreshold -> sign(raw) * (rubberThreshold + (abs(raw) - rubberThreshold) * 0.3f)
                else -> raw
            }
            val over = if (slot && !reduced) abs(shown) > span / 2f else abs(if (reduced) raw else shown) > rubberThreshold
            if (over != drag.crossed) {
                drag.crossed = over
                haptics.performHapticFeedback(if (over) HapticFeedbackType.GestureThresholdActivate else HapticFeedbackType.SegmentTick)
            }
            scope.launch { offset.snapTo(shown) }
        }
        return draggable(
            state = state,
            orientation = Orientation.Horizontal,
            enabled = enabled,
            onDragStarted = {
                offset.stop()
                drag.raw = offset.value
                drag.crossed = false
                restartPeek = client.positionNow() >= PREVIOUS_RESTART_MS || client.queue.value.history.isEmpty()
            },
            onDragStopped = { v ->
                val shown = offset.value
                val raw = drag.raw
                val d = if (reduced) raw else shown
                val slot = slotFor(d)
                val flung = abs(v) > fling && sign(v) == sign(d) && abs(d) > minFling
                scope.launch {
                    when {
                        reduced -> if (abs(raw) > rubberThreshold || flung) dispatch(client, forward = raw < 0f)
                        slot && (abs(shown) > span / 2f || flung) -> {
                            val forward = shown < 0f
                            offset.animateTo(if (forward) -span else span, SETTLE, initialVelocity = v)
                            haptics.performHapticFeedback(HapticFeedbackType.Confirm)
                            val restart = !forward && restartPeek
                            val before = client.nowPlaying.value?.item?.key
                            dispatch(client, forward)
                            if (!restart) withTimeoutOrNull(1_500) { client.nowPlaying.first { it?.item?.key != before } }
                            // Let the new track's cover compose before the strip jumps back under it.
                            withFrameNanos { }
                            withFrameNanos { }
                            offset.snapTo(0f)
                        }
                        !slot && abs(shown) > rubberThreshold -> {
                            haptics.performHapticFeedback(HapticFeedbackType.Confirm)
                            dispatch(client, forward = shown < 0f)
                            offset.animateTo(0f, Motion.snap, initialVelocity = v)
                        }
                        else -> offset.animateTo(0f, Motion.snap, initialVelocity = v)
                    }
                }
            },
        )
    }

    /** The finger's own travel in this drag (px), and whether it is past the commit point. */
    private class DragTrack { var raw = 0f; var crossed = false }

    private fun dispatch(client: CoreClient, forward: Boolean) = client.dispatch(if (forward) Command.Next else Command.Previous)

    companion object {
        /** Settling on a neighbour: quick and never past it. */
        private val SETTLE = spring<Float>(dampingRatio = Spring.DampingRatioNoBouncy, stiffness = Spring.StiffnessMedium)
    }
}
