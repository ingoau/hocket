package app.hocket.ui.screens.detail

import android.os.SystemClock
import androidx.activity.compose.BackHandler
import androidx.compose.animation.core.Spring
import androidx.compose.animation.core.animate
import androidx.compose.animation.core.spring
import androidx.compose.foundation.background
import androidx.compose.foundation.basicMarquee
import androidx.compose.foundation.layout.width
import androidx.compose.material3.LocalContentColor
import androidx.compose.material3.darkColorScheme
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.TransformOrigin
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.stateDescription
import app.hocket.ui.lyrics.LyricsBackground
import app.hocket.ui.player.TransportRow
import androidx.compose.foundation.clickable
import androidx.compose.foundation.gestures.Orientation
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.gestures.draggable
import androidx.compose.foundation.gestures.rememberDraggableState
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxScope
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.asPaddingValues
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBars
import androidx.compose.foundation.lazy.LazyListState
import androidx.compose.foundation.pager.HorizontalPager
import androidx.compose.foundation.pager.PageSize
import androidx.compose.foundation.pager.PagerState
import androidx.compose.foundation.pager.rememberPagerState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.FilledTonalButton
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.Stable
import androidx.compose.runtime.derivedStateOf
import androidx.compose.runtime.getValue
import androidx.compose.runtime.key
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.produceState
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.setValue
import androidx.compose.runtime.snapshotFlow
import androidx.compose.runtime.withFrameNanos
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.geometry.lerp
import androidx.compose.ui.graphics.RectangleShape
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.hapticfeedback.HapticFeedback
import androidx.compose.ui.hapticfeedback.HapticFeedbackType
import androidx.compose.ui.input.nestedscroll.NestedScrollConnection
import androidx.compose.ui.input.nestedscroll.NestedScrollSource
import androidx.compose.ui.input.nestedscroll.nestedScroll
import androidx.compose.ui.input.pointer.PointerEventPass
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.onGloballyPositioned
import androidx.compose.ui.layout.positionInRoot
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.LocalHapticFeedback
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.CustomAccessibilityAction
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.customActions
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.IntOffset
import androidx.compose.ui.unit.Velocity
import androidx.compose.ui.unit.dp
import androidx.compose.ui.util.lerp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import app.hocket.R
import app.hocket.core.Commands
import app.hocket.core.api.Command
import app.hocket.core.has
import app.hocket.core.ref
import app.hocket.core.api.Track
import app.hocket.core.client.CoreClient
import app.hocket.ui.LocalCoreClient
import app.hocket.ui.a11y.LocalReducedMotion
import app.hocket.ui.components.Artwork
import app.hocket.ui.components.previewPositionMs
import app.hocket.ui.icons.HocketIcons
import app.hocket.ui.nav.BottomContentInset
import app.hocket.ui.player.WavySeekBar
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.drop
import kotlinx.coroutines.launch
import kotlin.math.abs
import kotlin.math.absoluteValue
import kotlin.math.exp
import kotlin.math.roundToInt

/*
 * Album preview: pull an album page down while it is at its top and the cover follows the finger
 * from the header to the middle of the screen. Let go past the threshold and it docks there as the
 * first of a row of cards, one per song: swipe between them and each plays a song preview (the
 * menu's Preview: from 30% in, on this device only, whatever was playing here paused and given back
 * when the preview closes). Drag the card back up, tap outside it or go back to put it away.
 */

/** How far the finger travels to bring the cover from the header to the middle. */
private val PULL_DISTANCE = 128.dp

/** Under this much of the way back (dragging a docked card up) a release puts the cover away. */
private const val CLOSE_AT = 0.72f

/** A release this fast (in pull distances per second) opens or closes whatever the distance. */
private const val FLICK = 4f

/** How far past docked an over-pull can carry the cover (as a fraction of the way). */
private const val OVERPULL = 0.18f

/** A song this close to its end moves on to the next card before the preview would close. */
private const val ADVANCE_EARLY_MS = 450L

/** A preview asked for but not reported yet is not asked for again within this long (asking twice toggles it). */
private const val REQUEST_HOLD_MS = 1_500L

private val CARD_RADIUS = 28.dp

/**
 * Where the album preview is: [progress] 0 with the cover in the header, 1 docked in the middle,
 * between while the finger (or a settle) moves it. [open] once it has docked and the songs can be
 * swiped; it stays open until put away.
 */
@Stable
internal class AlbumPreviewState(private val scope: CoroutineScope) {
    /** The finger's way from the header to the middle, unclamped: past 1 is an over-pull. */
    private var travel by mutableFloatStateOf(0f)

    /** 0: the cover in the header. 1: docked. A little past 1 while over-pulled (rubber-banded). */
    val progress: Float get() = rubber(travel)

    var open by mutableStateOf(false)
        private set

    /** A finger is moving the cover. */
    var dragging by mutableStateOf(false)
        private set

    /** A release is easing the cover somewhere. */
    var settling by mutableStateOf(false)
        private set

    /** Docked and still: the card row takes over from the flying cover. */
    val docked: Boolean get() = open && !dragging && !settling

    /** Anything of the preview is on screen (the header's cover is hidden meanwhile). */
    val active: Boolean get() = open || travel > 0f

    /** The header's cover in the window, and its corner radius and artwork size, to fly from and back to. */
    internal var heroBounds by mutableStateOf<Rect?>(null)
    internal var heroRadius by mutableStateOf(0.dp)
    internal var heroArtSize by mutableStateOf(app.hocket.core.ArtworkSizes.FULL)

    /** Set by the overlay: the pull distance in pixels, haptics, reduced motion. */
    internal var pullPx = 1f
    internal var haptics: HapticFeedback? = null
    internal var reducedMotion = false

    /** The gesture began with the list at its top: only then does a pull bring the cover out. */
    internal var armed = false

    /** Set by the overlay: stops the preview (closing, from wherever, stops it at once). */
    internal var stopPreview: () -> Unit = {}

    private var settle: Job? = null

    /**
     * Move by [deltaPx] of finger travel (down is towards the middle). Returns the pixels used:
     * the cover doesn't go back past the header.
     */
    fun dragBy(deltaPx: Float): Float {
        settle?.cancel()
        settling = false
        dragging = true
        val before = travel
        travel = (travel + deltaPx / pullPx).coerceAtLeast(0f)
        // A detent at the threshold: crossing it out (or, docked, back past where a release closes).
        val mark = if (open) CLOSE_AT else 1f
        if (before < mark && travel >= mark) haptics?.performHapticFeedback(if (open) HapticFeedbackType.SegmentTick else HapticFeedbackType.GestureThresholdActivate)
        else if (before >= mark && travel < mark) haptics?.performHapticFeedback(if (open) HapticFeedbackType.GestureThresholdActivate else HapticFeedbackType.SegmentFrequentTick)
        // Pushed all the way back: no longer a pull, so the list's fling is its own again.
        if (travel == 0f && !open) dragging = false
        return (travel - before) * pullPx
    }

    /** The finger let go, moving at [velocityPx] (px/s, down positive). */
    fun release(velocityPx: Float) {
        if (!dragging) return
        dragging = false
        val v = velocityPx / pullPx
        if (!open) {
            if (travel >= 1f || (travel > 0.4f && v > FLICK)) openNow() else settleTo(0f)
        } else {
            if (travel < CLOSE_AT || v < -FLICK) close() else settleTo(1f)
        }
    }

    /** Dock the cover in the middle and open the songs (a release past the threshold, or the accessibility action). */
    fun openNow() {
        if (!open) haptics?.performHapticFeedback(HapticFeedbackType.Confirm)
        open = true
        settleTo(1f)
    }

    /** Put the cover back in the header. */
    fun close() {
        if (!active) return
        if (open) {
            haptics?.performHapticFeedback(HapticFeedbackType.GestureEnd)
            stopPreview()
        }
        open = false
        dragging = false
        settleTo(0f)
    }

    private fun settleTo(target: Float) {
        settle?.cancel()
        if (reducedMotion) {
            travel = target
            settling = false
            return
        }
        settling = true
        settle = scope.launch {
            try {
                // A little bounce docking; none going home, where the header's cover takes over.
                val spec = if (target > 0f) spring(0.72f, 420f) else spring(Spring.DampingRatioNoBouncy, 520f, visibilityThreshold = 0.001f)
                animate(travel, target, animationSpec = spec) { value, velocity ->
                    travel = if (target == 0f) value.coerceAtLeast(0f) else value
                    // Docked once the bounce has all but died away: the card row takes over a cover that has stopped moving.
                    if (target == 1f && settling && abs(value - 1f) < 0.012f && abs(velocity) < 0.25f) settling = false
                }
                travel = target
            } finally {
                // A settle cancelled by the next one leaves the flag to it.
                if (settle === coroutineContext[Job]) settling = false
            }
        }
    }

    private fun rubber(t: Float): Float = if (t <= 1f) t else 1f + OVERPULL * (1f - exp(-(t - 1f) * 1.6f))
}

@Composable
internal fun rememberAlbumPreviewState(): AlbumPreviewState {
    val scope = rememberCoroutineScope()
    return remember(scope) { AlbumPreviewState(scope) }
}

/**
 * The pull on an album page's list: once the list can't scroll up any further, a drag that began at
 * its top moves the cover out instead of stretching the list; dragging back up puts it back first.
 */
internal class AlbumPreviewPull(private val state: AlbumPreviewState) : NestedScrollConnection {
    override fun onPreScroll(available: Offset, source: NestedScrollSource): Offset {
        if (source != NestedScrollSource.UserInput || !state.dragging || state.open || available.y >= 0f) return Offset.Zero
        return Offset(0f, state.dragBy(available.y))
    }

    override fun onPostScroll(consumed: Offset, available: Offset, source: NestedScrollSource): Offset {
        if (source != NestedScrollSource.UserInput || !state.armed || state.open || available.y <= 0f) return Offset.Zero
        return Offset(0f, state.dragBy(available.y))
    }

    override suspend fun onPreFling(available: Velocity): Velocity {
        if (!state.dragging || state.open || state.progress <= 0f) return Velocity.Zero
        state.release(available.y)
        return available
    }
}

/** The album list's side of the pull: arms it for a gesture that starts at the top, and lets go of a pull that ends without a fling. */
@Composable
internal fun Modifier.albumPreviewPull(state: AlbumPreviewState, list: LazyListState): Modifier {
    val connection = remember(state) { AlbumPreviewPull(state) }
    val scope = rememberCoroutineScope()
    return this
        .pointerInput(state, list) {
            awaitEachGesture {
                awaitFirstDown(requireUnconsumed = false, pass = PointerEventPass.Initial)
                state.armed = !list.canScrollBackward && !state.open
                do {
                    val event = awaitPointerEvent(PointerEventPass.Initial)
                } while (event.changes.any { it.pressed })
                state.armed = false
                // The list's fling (and so onPreFling) normally comes first; a cancelled drag never flings.
                scope.launch {
                    withFrameNanos { }
                    if (state.dragging && !state.open) state.release(0f)
                }
            }
        }
        .nestedScroll(connection)
}

/** On the header's cover: where it is (to fly from), hidden while the preview has it, and the accessibility way in. */
internal fun Modifier.albumPreviewSource(state: AlbumPreviewState, radius: Dp, artSize: Int, label: String): Modifier = this
    .onGloballyPositioned {
        val at = it.positionInRoot()
        state.heroBounds = Rect(at, Size(it.size.width.toFloat(), it.size.height.toFloat()))
        state.heroRadius = radius
        state.heroArtSize = artSize
    }
    .graphicsLayer { alpha = if (state.active) 0f else 1f }
    .semantics { customActions = listOf(CustomAccessibilityAction(label) { state.openNow(); true }) }

/**
 * The preview over the album page: a scrim in the page's colour, the cover flying between the
 * header and the middle, and once docked a row of song cards with the preview's progress (seekable),
 * play/pause and Play from here under it. [startIndex]: the card it opens on.
 */
@Composable
internal fun BoxScope.AlbumPreviewOverlay(
    state: AlbumPreviewState,
    tracks: List<Track>,
    coverArt: String?,
    albumArtist: String?,
    startIndex: Int,
    onPlayFrom: (Int) -> Unit,
) {
    val client = LocalCoreClient.current
    val density = LocalDensity.current
    state.pullPx = with(density) { PULL_DISTANCE.toPx() }
    state.haptics = LocalHapticFeedback.current
    state.reducedMotion = LocalReducedMotion.current
    val active by remember(state) { derivedStateOf { state.active } }
    BackHandler(enabled = state.open) { state.close() }
    // Put away (or the page left with it open): stop the preview, which gives back what it paused.
    state.stopPreview = { client.dispatch(Command.PreviewStop) }
    DisposableEffect(state) { onDispose { if (state.open) client.dispatch(Command.PreviewStop) } }
    val requests = remember(client) { PreviewRequests(client) }
    if (!active || tracks.isEmpty()) return

    var hostOrigin by remember { mutableStateOf(Offset.Zero) }
    val settings by client.settings.collectAsStateWithLifecycle()
    val saver by client.batterySaver.collectAsStateWithLifecycle()
    // The full player's look: its blurred cover behind, its dark scheme from the cover's colour.
    val animatedBackground = settings[app.hocket.core.SettingKeys.DISPLAY_ANIMATED_BACKGROUND]?.value?.trim() != "false" && !saver.has(app.hocket.core.api.SaverFeature.StillBackground)
    val seed by app.hocket.ui.player.artworkSeed(coverArt)
    val scheme = remember(seed) { seed?.let { app.hocket.ui.theme.ArtworkColors.scheme(it, dark = true) } ?: darkColorScheme() }
    BoxWithConstraints(Modifier.matchParentSize().onGloballyPositioned { hostOrigin = it.positionInRoot() }.testTag("albumPreview")) {
        val statusTop = WindowInsets.statusBars.asPaddingValues().calculateTopPadding()
        val top = statusTop + 56.dp
        val room = (maxHeight - top - BottomContentInset).coerceAtLeast(0.dp)
        // The card as wide as the screen allows with its neighbours peeking, the controls under it
        // to the same edges; the two centred together in the room between the bar and the mini player.
        val side = minOf(maxWidth - CARD_INSET * 2, (room - INFO_HEIGHT).coerceAtLeast(160.dp), 420.dp)
        val infoWidth = maxOf(side, 280.dp).coerceAtMost(maxWidth - 32.dp)
        val cardTop = top + ((room - side - INFO_HEIGHT) / 2).coerceAtLeast(0.dp)
        val sidePx = with(density) { side.toPx() }
        val target = with(density) { Rect(Offset((maxWidth.toPx() - sidePx) / 2f, cardTop.toPx()), Size(sidePx, sidePx)) }
        val pager = rememberPagerState(initialPage = startIndex.coerceIn(0, tracks.lastIndex)) { tracks.size }

        // Behind: the player's blurred cover. Tapping it (or dragging the card up) puts the cover away.
        val dismiss = rememberDraggableState { delta -> state.dragBy(delta) }
        Box(
            Modifier
                .fillMaxSize()
                .graphicsLayer { alpha = (state.progress * 1.25f).coerceIn(0f, 1f) }
                .background(Color.Black)
                .pointerInput(state) { detectTapGestures { if (state.open) state.close() } }
                .draggable(dismiss, Orientation.Vertical, enabled = state.open, onDragStopped = { v -> state.release(v) }),
        ) {
            LyricsBackground(coverArt, animated = animatedBackground, visible = state.docked, modifier = Modifier.fillMaxSize(), tag = "albumPreview.background")
        }

        MaterialTheme(colorScheme = scheme) {
            CompositionLocalProvider(LocalContentColor provides scheme.onSurface, app.hocket.ui.theme.LocalDarkTheme provides true) {
              // Until it's open (on the way out after a short pull, or home after closing) nothing
              // here takes a touch: the controls are invisible, and a tap must not start a preview.
              Box(Modifier.matchParentSize().pointerInput(state) {
                  awaitPointerEventScope {
                      while (true) {
                          val event = awaitPointerEvent(PointerEventPass.Initial)
                          if (!state.open) event.changes.forEach { it.consume() }
                      }
                  }
              }) {
                PreviewCards(state, pager, tracks, coverArt, side, target, dismiss, requests)
                FlyingCover(state, coverArt, target, hostOrigin)
                PreviewInfo(state, pager, tracks, albumArtist, target, infoWidth, stillWave = saver.has(app.hocket.core.api.SaverFeature.StillSeekWave), requests, onPlayFrom)
                // A way out you can see, as well as back and the drag.
                IconButton(
                    onClick = { state.close() },
                    modifier = Modifier.align(Alignment.TopEnd).padding(top = statusTop + 4.dp, end = 4.dp)
                        .graphicsLayer { alpha = ((state.progress - 0.6f) / 0.4f).coerceIn(0f, 1f) }.testTag("albumPreview.close"),
                    enabled = state.open,
                ) { Icon(HocketIcons.Filled.Close, stringResource(R.string.album_preview_close)) }
              }
            }
        }
    }
}

/**
 * The previews this overlay asks for. Asking for the song that's already previewing toggles it, so
 * a request still on its way is not repeated; one for another song still on its way doesn't count
 * the song the core reports as already playing.
 */
private class PreviewRequests(private val client: CoreClient) {
    private var requested: Pair<String, Long>? = null

    fun ensure(t: Track) {
        val r = requested
        val fresh = r != null && SystemClock.uptimeMillis() - r.second < REQUEST_HOLD_MS
        if (fresh) {
            if (r!!.first == t.id) return
        } else if (client.preview.value?.trackId == t.id) return
        requested = t.id to SystemClock.uptimeMillis()
        client.dispatch(Commands.previewStart(t.ref))
    }

    fun reset() {
        requested = null
    }
}

/** How far in from the screen's edges a docked card sits: room for its neighbours to show. */
private val CARD_INSET = 36.dp

/** The height of what sits under a docked card: title, artist, seek bar, transport and Play from here. */
private val INFO_HEIGHT = 284.dp

/** The transport's height under the card (the full player's compact one). */
private val TRANSPORT_HEIGHT = 72.dp

/** The cover between the header and the middle, following the progress (draw phase only). */
@Composable
private fun FlyingCover(state: AlbumPreviewState, coverArt: String?, target: Rect, hostOrigin: Offset) {
    val density = LocalDensity.current
    val cardRadius = with(density) { CARD_RADIUS.toPx() }
    val shadow = with(density) { 24.dp.toPx() }
    Box(
        Modifier
            .offset { IntOffset(target.left.roundToInt(), target.top.roundToInt()) }
            .size(with(density) { target.width.toDp() })
            .graphicsLayer {
                val p = state.progress
                val hidden = state.docked
                alpha = if (hidden) 0f else 1f
                val hero = state.heroBounds?.translate(-hostOrigin)
                val from = hero ?: Rect(target.center - Offset(target.width / 4f, target.height / 4f), target.size / 2f)
                val fit = p.coerceIn(0f, 1f)
                val w = lerp(from.width, target.width, fit)
                val centre = lerp(from.center, target.center, p)
                val scale = w / target.width
                scaleX = scale
                scaleY = scale
                translationX = centre.x - target.center.x
                translationY = centre.y - target.center.y
                val r = lerp(if (hero != null) with(density) { state.heroRadius.toPx() } else cardRadius, cardRadius, fit)
                shape = RoundedCornerShape((r / scale.coerceAtLeast(0.01f)).coerceAtLeast(0f))
                clip = true
                shadowElevation = shadow * fit
                if (hero == null) alpha *= fit
            },
    ) {
        Artwork(coverArt, state.heroArtSize, null, Modifier.fillMaxSize(), RectangleShape)
    }
}

/** The row of song cards, the current one in the middle where the cover docked. */
@Composable
private fun PreviewCards(
    state: AlbumPreviewState, pager: PagerState, tracks: List<Track>, coverArt: String?, side: Dp, target: Rect,
    dismiss: androidx.compose.foundation.gestures.DraggableState,
    requests: PreviewRequests,
) {
    val client = LocalCoreClient.current
    val haptics = LocalHapticFeedback.current
    val scope = rememberCoroutineScope()
    val preview by client.preview.collectAsStateWithLifecycle()
    val density = LocalDensity.current
    val pad = with(density) { ((target.left).toDp()).coerceAtLeast(0.dp) }
    // A tick for each song the row passes under the finger.
    LaunchedEffect(pager) {
        snapshotFlow { pager.currentPage }.drop(1).collect { if (pager.isScrollInProgress && state.docked) haptics.performHapticFeedback(HapticFeedbackType.SegmentTick) }
    }
    HorizontalPager(
        pager,
        Modifier
            .fillMaxWidth()
            .offset { IntOffset(0, target.top.roundToInt()) }
            .height(side)
            .draggable(dismiss, Orientation.Vertical, enabled = state.open, onDragStopped = { v -> state.release(v) })
            .testTag("albumPreview.pager"),
        contentPadding = PaddingValues(horizontal = pad),
        pageSize = PageSize.Fixed(side),
        pageSpacing = 12.dp,
        beyondViewportPageCount = 1,
        userScrollEnabled = state.docked,
        key = { tracks[it].id },
    ) { page ->
        val track = tracks[page]
        val sample = preview?.takeIf { it.trackId == track.id }
        val playing = sample?.playing == true
        val label = stringResource(if (playing) R.string.album_preview_pause else R.string.album_preview_play, track.title)
        val position = stringResource(R.string.album_preview_song_of, page + 1, tracks.size)
        Box(
            Modifier
                .fillMaxSize()
                .graphicsLayer {
                    val off = pager.getOffsetDistanceInPages(page).absoluteValue.coerceIn(0f, 1f)
                    val scale = lerp(1f, 0.9f, off)
                    scaleX = scale
                    scaleY = scale
                    // A neighbour shrinks towards the middle card, so what peeks past the edge stays.
                    transformOrigin = TransformOrigin(if (page > pager.currentPage) 0f else if (page < pager.currentPage) 1f else 0.5f, 0.5f)
                    val shown = lerp(1f, 0.5f, off)
                    // Until docked the flying cover is the middle card; its neighbours fade in on the way.
                    alpha = when {
                        state.docked -> shown
                        off < 0.5f -> 0f
                        else -> shown * ((state.progress - 0.35f) / 0.65f).coerceIn(0f, 1f)
                    }
                    shadowElevation = 24.dp.toPx() * (1f - off)
                    shape = RoundedCornerShape(CARD_RADIUS)
                    clip = true
                }
                .clickable(onClickLabel = label) {
                    if (page != pager.currentPage) scope.launch { pager.animateScrollToPage(page) }
                    else {
                        haptics.performHapticFeedback(if (playing) HapticFeedbackType.ToggleOff else HapticFeedbackType.ToggleOn)
                        if (sample == null) requests.ensure(track) else client.dispatch(Command.PreviewToggle)
                    }
                }
                .semantics { contentDescription = label; stateDescription = position }
                .testTag("albumPreview.card.$page"),
        ) {
            Artwork(coverArt, state.heroArtSize, null, Modifier.fillMaxSize(), RectangleShape)
            // Where the song is in the album, so cards wearing one cover still tell apart (faded in as the
            // row takes over from the flying cover).
            val badge by androidx.compose.animation.core.animateFloatAsState(if (state.docked) 1f else 0f, androidx.compose.animation.core.tween(180), label = "previewBadge")
            Text(
                stringResource(R.string.album_preview_badge, page + 1, tracks.size),
                style = MaterialTheme.typography.labelLarge, fontWeight = FontWeight.SemiBold, color = Color.White,
                modifier = Modifier.align(Alignment.TopStart).padding(12.dp).graphicsLayer { alpha = badge }
                    .background(Color.Black.copy(alpha = 0.5f), CircleShape).padding(horizontal = 10.dp, vertical = 4.dp)
                    .clearAndSetSemantics { },
            )
            // Paused (or not started): a play mark over the card, which is the button.
            val paused = page == pager.currentPage && sample != null && !playing
            if (paused) {
                Box(
                    Modifier.align(Alignment.Center).size(72.dp).background(Color.Black.copy(alpha = 0.45f), CircleShape),
                    contentAlignment = Alignment.Center,
                ) { Icon(HocketIcons.Filled.PlayArrow, null, Modifier.size(40.dp), tint = Color.White) }
            }
        }
    }
}

/** Under the docked card, as in the full player: the song, its preview's progress, the transport and Play from here; and what keeps the preview on the card in the middle. */
@Composable
private fun BoxScope.PreviewInfo(
    state: AlbumPreviewState, pager: PagerState, tracks: List<Track>, albumArtist: String?, target: Rect, width: Dp, stillWave: Boolean,
    requests: PreviewRequests, onPlayFrom: (Int) -> Unit,
) {
    val client = LocalCoreClient.current
    val haptics = LocalHapticFeedback.current
    val scope = rememberCoroutineScope()
    val preview by client.preview.collectAsStateWithLifecycle()
    val page = pager.currentPage.coerceIn(0, tracks.lastIndex)
    val track = tracks[page]
    val sample = preview?.takeIf { it.trackId == track.id }

    val currentEnsure: (Track) -> Unit = requests::ensure
    var first by remember { mutableStateOf(true) }
    // The preview follows the card in the middle once the row settles.
    LaunchedEffect(state.open, pager.settledPage) {
        if (!state.open) {
            requests.reset()
            first = true
            return@LaunchedEffect
        }
        // A settle that just passes through a card doesn't start it.
        if (!first) delay(150)
        first = false
        currentEnsure(tracks[pager.settledPage.coerceIn(0, tracks.lastIndex)])
    }
    // Near a song's end, on to the next card (asked for first, so the preview never closes between
    // them). On the last card it stops short and pauses back where it started: a preview that ran out
    // would give playback back under the open preview. The scroll runs on its own, as the next
    // song's report changes this effect's keys.
    LaunchedEffect(sample?.trackId, sample?.playing, sample?.takenAt, state.open) {
        val s = sample ?: return@LaunchedEffect
        if (!s.playing || !state.open) return@LaunchedEffect
        val d = s.durationMs.toLong()
        if (d <= 0) return@LaunchedEffect
        val at = previewPositionMs(s.positionMs.toLong(), s.takenAt, true, d, client.now())
        delay((d - at - ADVANCE_EARLY_MS).coerceAtLeast(0))
        if (page < tracks.lastIndex) {
            currentEnsure(tracks[page + 1])
            scope.launch { pager.animateScrollToPage(page + 1) }
        } else {
            client.dispatch(Command.PreviewToggle)
            client.dispatch(Commands.previewSeek((d * 0.3).toLong()))
        }
    }

    val now by produceState(client.now(), sample?.playing, sample?.takenAt, sample?.positionMs) {
        while (true) {
            value = client.now()
            if (sample?.playing != true) break
            delay(CoreClient.PROGRESS_TICK_MS)
        }
    }
    val duration = sample?.durationMs?.toLong()?.takeIf { it > 0 } ?: track.durationMs.toLong()
    // Not started yet: where it will start (30% in, as the core opens a preview).
    val startAt = (duration * 0.3).toLong()
    // Read in the seek bar's lambda, so the clock's tick redraws the bar and nothing else.
    val position = { if (sample != null) previewPositionMs(sample.positionMs.toLong(), sample.takenAt, sample.playing, duration, now) else startAt }
    val playing = sample?.playing == true

    Column(
        Modifier
            .align(Alignment.TopCenter)
            .offset { IntOffset(0, (target.bottom + 12.dp.toPx()).roundToInt()) }
            .width(width)
            .graphicsLayer { alpha = ((state.progress - 0.55f) / 0.45f).coerceIn(0f, 1f) }
            .testTag("albumPreview.info"),
    ) {
        // Start-aligned to the card's edge, as the player's title is to its artwork's.
        Text(track.title, style = MaterialTheme.typography.headlineSmall, fontWeight = FontWeight.ExtraBold, color = MaterialTheme.colorScheme.onSurface,
            maxLines = 1, modifier = Modifier.fillMaxWidth().basicMarquee(iterations = Int.MAX_VALUE).testTag("albumPreview.title"))
        Text(track.artist ?: albumArtist.orEmpty(), style = MaterialTheme.typography.bodyLarge, color = MaterialTheme.colorScheme.onSurfaceVariant,
            maxLines = 1, overflow = TextOverflow.Ellipsis, modifier = Modifier.fillMaxWidth())
        Spacer(Modifier.height(8.dp))
        key(track.id) {
            WavySeekBar(
                position = position, durationMs = duration, playing = playing, stillWave = stillWave,
                onSeek = { ms ->
                    // Not started yet: start it, then seek (the core keeps a seek made before the song reports in).
                    if (sample == null) currentEnsure(track)
                    client.dispatch(Commands.previewSeek(ms))
                },
            )
        }
        Spacer(Modifier.height(8.dp))
        // The player's transport: previous and next move along the row (previous on the first song
        // starts its preview over), the middle one plays and pauses it.
        TransportRow(
            playing = playing,
            onPrevious = {
                if (page > 0) scope.launch { pager.animateScrollToPage(page - 1) }
                else if (sample == null) currentEnsure(track) else client.dispatch(Commands.previewSeek(startAt))
            },
            onToggle = { if (sample == null) currentEnsure(track) else client.dispatch(Command.PreviewToggle) },
            onNext = { if (page < tracks.lastIndex) scope.launch { pager.animateScrollToPage(page + 1) } },
            height = TRANSPORT_HEIGHT,
            modifier = Modifier.testTag("albumPreview.transport"),
        )
        Spacer(Modifier.height(12.dp))
        FilledTonalButton(
            onClick = {
                haptics.performHapticFeedback(HapticFeedbackType.Confirm)
                onPlayFrom(page)
                state.close()
            },
            modifier = Modifier.fillMaxWidth().height(52.dp).testTag("albumPreview.playFrom"),
        ) {
            Icon(HocketIcons.Filled.PlaylistPlay, null, Modifier.size(22.dp))
            Spacer(Modifier.size(8.dp))
            Text(stringResource(R.string.album_preview_play_from_here), style = MaterialTheme.typography.titleSmall)
        }
    }
}
