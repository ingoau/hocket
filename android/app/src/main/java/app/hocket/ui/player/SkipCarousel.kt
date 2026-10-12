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
import androidx.compose.runtime.derivedStateOf
import androidx.compose.runtime.mutableFloatStateOf
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.compose.runtime.setValue
import androidx.compose.runtime.withFrameNanos
import androidx.compose.ui.Modifier
import androidx.compose.ui.hapticfeedback.HapticFeedback
import androidx.compose.ui.hapticfeedback.HapticFeedbackType
import androidx.compose.ui.unit.dp
import app.hocket.core.api.Command
import app.hocket.core.api.QueueEntry
import app.hocket.core.api.QueueSource
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
 * autoplay's included) and the previous one (the last of the history). At the end of the queue, as
 * the core (`session::reducer`'s `candidate`): with Repeat All it wraps to the context's first item
 * in play order, which unshuffled is the context's item 0 (found in the history or current); with
 * shuffle that is the shuffled order's first, which the queue view does not tell, so no peek (the
 * swipe rubber-bands, and still skips). Nothing for Repeat One or Off.
 */
@Immutable
internal data class Neighbours(val previous: QueueEntry?, val next: QueueEntry?) {
    companion object {
        val None = Neighbours(null, null)

        fun of(queue: QueueView): Neighbours {
            val next = queue.playingNext.firstOrNull() ?: queue.upcoming.firstOrNull() ?: wrap(queue)
            return Neighbours(previous = queue.history.lastOrNull(), next = next)
        }

        private fun wrap(queue: QueueView): QueueEntry? {
            if (queue.repeat != RepeatMode.All || queue.shuffle) return null
            fun first(e: QueueEntry) = (e.item.source as? QueueSource.Context)?.data?.index == 0u
            return queue.current?.takeIf(::first) ?: queue.history.lastOrNull(::first)
        }
    }
}

/** The current queue's [Neighbours], recomputed only when the queue changes. */
@Composable
internal fun rememberNeighbours(): State<Neighbours> {
    val queue = LocalCoreClient.current.queue.collectAsStateWithLifecycle()
    return remember(queue) { derivedStateOf { Neighbours.of(queue.value) } }
}

/**
 * The core's rule (`hocket_core::session::reducer::previous_should_restart`): Previous restarts the
 * track from this far in, and when there is nothing before it.
 */
internal const val PREVIOUS_RESTART_MS = 3_000L

/**
 * Whether Previous restarts the current track rather than going back (the swipe then peeks at it
 * again), as the core (`handlers::session::previous`): [PREVIOUS_RESTART_MS] into a track loaded
 * here (this device holds the transport: not while another device plays, nor before anything has
 * loaded, as on a cold start or after Stop), or with nothing before it.
 */
internal fun previousRestarts(ownsTransport: Boolean, positionMs: Long, historyEmpty: Boolean): Boolean =
    (ownsTransport && positionMs >= PREVIOUS_RESTART_MS) || historyEmpty

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
    var span by mutableFloatStateOf(1f)
    var restartPeek by mutableStateOf(false)
        private set
    /** Settling on a neighbour and handing over to it (the new track's scene takes over at once). */
    var committing by mutableStateOf(false)
        private set
    /** Under the finger, or settling a skip: the scene follows the swipe rather than animating. */
    val busy: Boolean get() = committing || offset.value != 0f

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
                restartPeek = previousRestarts(client.ownsTransport.value, client.positionNow(), client.queue.value.history.isEmpty())
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
                            committing = true
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
                            committing = false
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
