package app.hocket.ui.player

import app.hocket.core.nextEntry
import androidx.compose.animation.core.Animatable
import androidx.compose.animation.core.Spring
import androidx.compose.animation.core.spring
import androidx.compose.foundation.gestures.Orientation
import androidx.compose.foundation.gestures.draggable
import androidx.compose.foundation.gestures.rememberDraggableState
import androidx.compose.runtime.remember
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
import app.hocket.core.api.QueueView
import app.hocket.core.client.CoreClient
import app.hocket.ui.LocalCoreClient
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.flow.combine
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.launch
import kotlinx.coroutines.withTimeoutOrNull
import kotlin.math.abs
import kotlin.math.sign

/**
 * What a skip swipe peeks at: the next item (the first of "playing next", else the first upcoming,
 * autoplay's included) and the previous one (the last of the history). At the end of the queue it is
 * the core's [QueueView.wrapNext]: with Repeat All, the context's first item in play order (the
 * shuffled order's first with shuffle on). Nothing for Repeat One or Off. The previous one is the
 * core's [QueueView.previous], what going back plays: the last history item still playable, or
 * the context's item before (from an album started part way in, say); nothing where there is none.
 */
@Immutable
internal data class Neighbours(val previous: QueueEntry?, val next: QueueEntry?) {
    companion object {
        val None = Neighbours(null, null)

        fun of(queue: QueueView): Neighbours {
            val next = queue.nextEntry
            return Neighbours(previous = queue.previous, next = next)
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
 * A three-slot carousel (previous, current, next) under the finger: [offset] follows it 1:1 (px,
 * negative towards next). Released past half of [span] (the slot's width plus the gap to its
 * neighbour) or flung, it settles on the neighbour, sends Next / Previous (unless the song changed
 * meanwhile), and once the new track is current snaps back to 0, the neighbour now being the
 * current one; otherwise it springs back. A new drag while it settles sends the skip at once and
 * carries on from its page (so N quick swipes skip N songs).
 * A swipe back always goes back a song ([Command.PreviousTrack], never Previous's restart of this
 * one). Where there is no neighbour it rubber-bands as the old skip swipe did: past
 * [rubberThreshold] a swipe past the end of the queue still ends it, and with nothing before the
 * current song a swipe back does nothing. While a skip settles the neighbours shown are held
 * ([shown]) until the strip is back at 0, so the new song's own neighbours never flash in the page
 * it settled on.
 */
@Stable
internal class SkipCarousel {
    /** Settling (springing back, or onto a neighbour): animated. Under the finger it is [follow]. */
    private val settle = Animatable(0f)
    /** Under the finger: written straight from each drag delta, so the strip never trails it by a frame. */
    private var follow by mutableFloatStateOf(0f)
    private var following by mutableStateOf(false)
    /** Where the strip is (px, negative towards next). */
    val offset: Float get() = if (following) follow else settle.value
    var span by mutableFloatStateOf(1f)
    /** The neighbours as they were when a settling skip started (see [shown]). */
    private var held by mutableStateOf<Neighbours?>(null)
    /** Settling on a neighbour and handing over to it (the new track's scene takes over at once). */
    var committing by mutableStateOf(false)
        private set
    /** Under the finger, or settling a skip: the scene follows the swipe rather than animating. */
    val busy: Boolean get() = committing || offset != 0f

    /** -1..1: how far towards the previous (positive) or next (negative) neighbour. */
    fun fraction(): Float = if (span <= 1f) 0f else (offset / span).coerceIn(-1f, 1f)

    /**
     * The neighbours to draw: [live], except while a skip settles. The new song is current a frame
     * or two before the strip jumps back under it, and in those frames the page showing is the old
     * neighbour's slot, which must not turn into the new song's neighbour.
     */
    fun shown(live: Neighbours): Neighbours = held ?: live

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
        rubberThreshold: Float,
    ): Modifier {
        val density = LocalDensity.current
        val fling = with(density) { 400.dp.toPx() }
        val minFling = with(density) { 24.dp.toPx() }
        val drag = remember { DragTrack() }
        // The queue's own neighbours, not the ones drawn (held while a skip settles).
        fun live() = Neighbours.of(client.queue.value)
        fun slotFor(d: Float) = live().let { (d > 0f && it.previous != null) || (d < 0f && it.next != null) }
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
            // Without a slot only a swipe on (ending the queue) does anything.
            val over = if (slot && !reduced) abs(shown) > span / 2f else (raw < 0f || (reduced && slot)) && abs(if (reduced) raw else shown) > rubberThreshold
            // A light tick each way across the commit point; the firm one is the release's.
            if (over != drag.crossed) {
                drag.crossed = over
                haptics.performHapticFeedback(HapticFeedbackType.SegmentTick)
            }
            follow = shown
        }
        return draggable(
            state = state,
            orientation = Orientation.Horizontal,
            enabled = enabled,
            onDragStarted = {
                drag.generation++
                drag.crossed = false
                if (following) {
                    // The last drag's release is still waiting on a rebase (and now never acts):
                    // this one carries on from where the finger is.
                    drag.raw = follow
                } else {
                    // A skip still settling: it is done now (sent once), and this drag carries on
                    // from its page.
                    pending?.let { finishNow(client, scope, drag, it) }
                    settle.stop()
                    drag.raw = settle.value
                    follow = settle.value
                    following = true
                }
            },
            onDragStopped = { v ->
                val generation = drag.generation
                scope.launch {
                    // A finished skip's page becomes the current one first (see [finishNow]).
                    rebase?.join()
                    // A newer drag started meanwhile: its own release decides.
                    if (drag.generation != generation) return@launch
                    // The finger's position hands over to the spring.
                    settle.snapTo(follow)
                    following = false
                    val shown = settle.value
                    val raw = drag.raw
                    val d = if (reduced) raw else shown
                    val slot = slotFor(d)
                    val flung = abs(v) > fling && sign(v) == sign(d) && abs(d) > minFling
                    when {
                        reduced -> if ((abs(raw) > rubberThreshold || flung) && (raw < 0f || slotFor(raw))) {
                            haptics.performHapticFeedback(HapticFeedbackType.Confirm)
                            skip(client, forward = raw < 0f)
                        }
                        slot && (abs(shown) > span / 2f || flung) -> {
                            val forward = shown < 0f
                            // The song the skip is from: if another takes its place before the
                            // skip is sent (it ended, or another device skipped), that was the skip.
                            val neighbours = live()
                            val commit = Pending(forward, client.nowPlaying.value?.item?.key)
                            commit.job = coroutineContext[kotlinx.coroutines.Job]
                            pending = commit
                            held = neighbours
                            committing = true
                            // Felt as the finger lifts, not once the strip has come to rest.
                            haptics.performHapticFeedback(HapticFeedbackType.Confirm)
                            try {
                                settle.animateTo(if (forward) -span else span, SETTLE, initialVelocity = v)
                                send(client, commit)
                                awaitChange(client, commit)
                                // Let the new track's cover compose before the strip jumps back under it.
                                withFrameNanos { }
                                withFrameNanos { }
                                settle.snapTo(0f)
                            } finally {
                                // Done, or finished early by a new drag ([finishNow]).
                                if (pending === commit) {
                                    pending = null
                                    held = null
                                    committing = false
                                }
                            }
                        }
                        !slot && shown < 0f && abs(shown) > rubberThreshold -> {
                            haptics.performHapticFeedback(HapticFeedbackType.Confirm)
                            skip(client, forward = true)
                            settle.animateTo(0f, BACK, initialVelocity = v)
                        }
                        else -> settle.animateTo(0f, BACK, initialVelocity = v)
                    }
                }
            },
        )
    }

    /** A skip settling: its direction, the song it is from, and whether it was sent. */
    private class Pending(val forward: Boolean, val before: String?) {
        var sent = false
        var job: kotlinx.coroutines.Job? = null
    }

    private var pending: Pending? = null

    /** While a finished skip's page waits to become the current one (the strip shifting by a page). */
    private var rebase: kotlinx.coroutines.Job? = null

    /** Sends [commit]'s Next / Previous, once, unless its song has already been replaced. */
    private fun send(client: CoreClient, commit: Pending) {
        if (commit.sent) return
        commit.sent = true
        if (client.nowPlaying.value?.item?.key == commit.before) skip(client, commit.forward)
    }

    /**
     * A new drag while [commit] settles: the skip is sent now, the strip snaps to its page, and once
     * that song is current the strip (and the drag under way) shift back by a page, so the finger
     * carries on from the new song. N swipes, N skips.
     */
    private suspend fun finishNow(client: CoreClient, scope: CoroutineScope, drag: DragTrack, commit: Pending) {
        pending = null
        committing = false
        commit.job?.cancel()
        val page = if (commit.forward) -span else span
        settle.snapTo(page)
        send(client, commit)
        rebase?.cancel()
        rebase = scope.launch {
            try {
                awaitChange(client, commit)
                withFrameNanos { }
                drag.raw -= page
                held = null
                if (following) follow -= page else settle.snapTo(settle.value - page)
            } finally {
                // Cancelled (the artwork left): nothing settling any more.
                held = null
            }
        }
    }

    /** The finger's own travel in this drag (px), whether it is past the commit point, and which drag it is. */
    private class DragTrack { var raw = 0f; var crossed = false; var generation = 0 }

    /** Next, or back to the song before this one (never Previous's restart of this one). */
    private fun skip(client: CoreClient, forward: Boolean) = client.dispatch(if (forward) Command.Next else Command.PreviousTrack)

    /**
     * Until [commit]'s song is replaced in both the queue and now playing (they arrive as separate
     * events), so the neighbours read next are the new song's. At most 1.5 s.
     */
    private suspend fun awaitChange(client: CoreClient, commit: Pending) {
        withTimeoutOrNull(1_500) {
            combine(client.queue, client.nowPlaying) { q, e -> q.current?.item?.key != commit.before && e?.item?.key != commit.before }.first { it }
        }
    }

    companion object {
        /**
         * Settling on a neighbour: quick and never past it. Done within a pixel (the default 0.01 px
         * tail would hold the skip back ~150 ms after the strip has visibly stopped).
         */
        private val SETTLE = spring(dampingRatio = Spring.DampingRatioNoBouncy, stiffness = 2_000f, visibilityThreshold = 1f)
        /** Springing back to the current song, likewise done within a pixel. */
        private val BACK = spring(dampingRatio = Spring.DampingRatioNoBouncy, stiffness = Spring.StiffnessMedium, visibilityThreshold = 1f)
    }
}
