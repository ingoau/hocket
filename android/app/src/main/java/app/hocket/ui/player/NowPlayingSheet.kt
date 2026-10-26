package app.hocket.ui.player

import android.os.Build
import androidx.activity.compose.PredictiveBackHandler
import androidx.compose.animation.Crossfade
import androidx.compose.animation.animateColorAsState
import androidx.compose.animation.core.Animatable
import androidx.compose.animation.core.AnimationSpec
import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.animation.core.snap
import androidx.compose.animation.core.tween
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.gestures.AnchoredDraggableDefaults
import androidx.compose.foundation.gestures.AnchoredDraggableState
import androidx.compose.foundation.gestures.DraggableAnchors
import androidx.compose.foundation.gestures.Orientation
import androidx.compose.foundation.gestures.anchoredDraggable
import androidx.compose.foundation.gestures.animateTo
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.ColorScheme
import androidx.compose.material3.LocalContentColor
import androidx.compose.material3.MaterialExpressiveTheme
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.darkColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.snapshotFlow
import androidx.compose.runtime.SideEffect
import androidx.compose.runtime.Stable
import androidx.compose.runtime.derivedStateOf
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.saveable.Saver
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.BlurredEdgeTreatment
import androidx.compose.ui.draw.blur
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.geometry.RoundRect
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.BlendMode
import androidx.compose.ui.graphics.CompositingStrategy
import androidx.compose.ui.draw.drawWithContent
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Outline
import androidx.compose.ui.graphics.RectangleShape
import androidx.compose.ui.graphics.Shape
import androidx.compose.ui.graphics.TransformOrigin
import androidx.compose.ui.platform.LocalHapticFeedback
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.input.nestedscroll.NestedScrollConnection
import androidx.compose.ui.input.nestedscroll.NestedScrollSource
import androidx.compose.ui.input.nestedscroll.nestedScroll
import androidx.compose.ui.layout.LayoutCoordinates
import androidx.compose.ui.layout.layout
import androidx.compose.ui.layout.onPlaced
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.collapse
import androidx.compose.ui.semantics.dismiss
import androidx.compose.ui.semantics.expand
import androidx.compose.ui.semantics.isTraversalGroup
import androidx.compose.ui.semantics.paneTitle
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.Constraints
import androidx.compose.ui.unit.Density
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.IntOffset
import androidx.compose.ui.unit.LayoutDirection
import androidx.compose.ui.unit.Velocity
import androidx.compose.ui.unit.dp
import androidx.compose.ui.zIndex
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import app.hocket.R
import app.hocket.core.ArtworkSizes
import app.hocket.ui.LocalCoreClient
import app.hocket.ui.a11y.LocalReducedMotion
import app.hocket.ui.components.Artwork
import app.hocket.ui.theme.ArtworkColors
import app.hocket.ui.theme.LocalArtworkSeedState
import app.hocket.ui.theme.LocalDarkTheme
import app.hocket.ui.theme.Motion
import app.hocket.core.api.ArtworkStyle
import app.hocket.core.api.ArtworkLayout
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.launch
import kotlin.math.roundToInt
import kotlin.math.sqrt
import kotlin.math.abs

enum class SheetValue { Collapsed, Expanded }

/**
 * What fills the full player's big area: the artwork, or lyrics or the queue in its place (the
 * artwork then sits small beside the title). Kept across collapsing and expanding the player and
 * across rotation (Apple Music). [About] is no longer shown in the area (the song's details are a
 * sheet from the title); it stays so a saved state that names it still restores (to [Artwork]).
 */
enum class PlayerMode { Artwork, Lyrics, Queue, About }

/**
 * The now-playing sheet's drag state: one [AnchoredDraggableState] whose offset is the sheet's top
 * edge. `progress` is 0 collapsed (mini bar) to 1 expanded (full player).
 */
class NowPlayingSheetState(initial: SheetValue = SheetValue.Collapsed, initialMode: PlayerMode = PlayerMode.Artwork) {
    val draggable = AnchoredDraggableState(initial)
    /** The full player's mode; survives collapse/expand and (through [Saver]) rotation. */
    var mode by mutableStateOf(initialMode)
    var collapsedOffset by mutableStateOf(0f)
        internal set
    val progress: Float
        get() {
            val offset = draggable.offset
            if (offset.isNaN() || collapsedOffset <= 0f) return if (draggable.currentValue == SheetValue.Expanded) 1f else 0f
            return (1f - offset / collapsedOffset).coerceIn(0f, 1f)
        }
    val isExpanded: Boolean get() = draggable.currentValue == SheetValue.Expanded || draggable.targetValue == SheetValue.Expanded

    /** Fling speed (px/s) past which a release settles in the fling's direction; set from the density. */
    internal var velocityThreshold by mutableStateOf(Float.POSITIVE_INFINITY)

    suspend fun expand() = draggable.animateTo(SheetValue.Expanded, Motion.sheet)
    suspend fun collapse() = draggable.animateTo(SheetValue.Collapsed, Motion.sheet)

    /**
     * Where a release with [velocity] (px/s, positive = downwards) settles: a fling past
     * [velocityThreshold] goes its way; a slow release switches only once the sheet has travelled
     * [POSITIONAL_THRESHOLD] of the way from the anchor it last rested on (the drag's own fling
     * behaviour uses the same rules).
     */
    internal fun targetFor(velocity: Float): SheetValue {
        val p = progress
        return when {
            velocity >= velocityThreshold -> SheetValue.Collapsed
            velocity <= -velocityThreshold -> SheetValue.Expanded
            draggable.settledValue == SheetValue.Expanded -> if (1f - p > POSITIONAL_THRESHOLD) SheetValue.Collapsed else SheetValue.Expanded
            else -> if (p > POSITIONAL_THRESHOLD) SheetValue.Expanded else SheetValue.Collapsed
        }
    }

    /**
     * Settles the sheet on an anchor after a fling that a scrolling page inside it did not consume
     * (the nested-scroll path). Deliberately not `AnchoredDraggableState.settle(velocity)`: that
     * overload is deprecated and throws for a state built without thresholds (this one), which
     * crashed the app whenever the player was swiped away from its queue, lyrics or scrolled page.
     */
    suspend fun settle(velocity: Float) {
        if (collapsedOffset <= 0f || draggable.offset.isNaN()) return
        // A page's fling can end long after the sheet has settled (a list flinging back to its
        // top): with the sheet resting on an anchor, or already animating (a tap on the mini
        // player expanding it), there is nothing to settle, and animating would interrupt that.
        if (draggable.isAnimationRunning) return
        if (kotlin.math.abs(draggable.offset - draggable.anchors.positionOf(draggable.settledValue)) < 0.5f) return
        draggable.animateTo(targetFor(velocity), Motion.sheet)
    }

    companion object {
        /** Saves where the sheet is headed and the player's mode, so a rotation keeps both. */
        val Saver: Saver<NowPlayingSheetState, String> = Saver(
            save = { it.draggable.targetValue.name + "|" + it.mode.name },
            restore = { saved ->
                val parts = saved.split("|")
                NowPlayingSheetState(
                    SheetValue.valueOf(parts[0]),
                    // About is a sheet now (from the title): a saved About mode opens on the artwork.
                    parts.getOrNull(1)?.let { m -> PlayerMode.entries.firstOrNull { it.name == m } }?.takeIf { it != PlayerMode.About } ?: PlayerMode.Artwork,
                )
            },
        )
        val MINI_HEIGHT: Dp = 68.dp
        /** The gap the floating mini player keeps above the navigation bar (or the screen's bottom inset). */
        val MINI_GAP: Dp = 8.dp
        /** A release moves the sheet to the other anchor once it has travelled this share of the distance. */
        const val POSITIONAL_THRESHOLD = 0.3f
        /** Release speed that settles in the fling's direction regardless of position (Material's default). */
        val VELOCITY_THRESHOLD: Dp = 125.dp
    }
}

/** The sheet state, surviving configuration changes (an open player stays open across a rotation). */
@Composable
fun rememberNowPlayingSheetState(): NowPlayingSheetState = rememberSaveable(saver = NowPlayingSheetState.Saver) { NowPlayingSheetState() }

/**
 * The mini player's height: [NowPlayingSheetState.MINI_HEIGHT] at the default font size, taller when
 * the user's font scale makes its two lines of text (title, artist) need more, so they never clip.
 */
@Composable
fun miniPlayerHeight(): Dp {
    val typography = MaterialTheme.typography
    val text = with(LocalDensity.current) { (typography.bodyLarge.lineHeight.toDp() + typography.bodySmall.lineHeight.toDp()) }
    return maxOf(NowPlayingSheetState.MINI_HEIGHT, text + 24.dp)
}

/**
 * Where the artwork is, for the one artwork of the full player and the one that flies between the
 * players while the sheet moves. Measured relative to the sheet ([root]): the mini player's
 * thumbnail ([thumb]), the full player's mode area ([area], the big artwork sits at its top) and
 * the thumbnail slot beside the title ([small]). [playerArt] is where the full player's artwork is
 * for a mode fraction (0 = big, 1 = thumbnail); the sheet interpolates from [thumb] to it.
 */
@Stable
internal class HeroGeometry {
    var root: LayoutCoordinates? = null
    var thumb by mutableStateOf(Rect.Zero)
    var area by mutableStateOf(Rect.Zero)
    var small by mutableStateOf(Rect.Zero)
    /** The big artwork's side margin, largest side and least vertical margin in its area (px). */
    var artInset = 0f
    var artMax = Float.MAX_VALUE
    var artPad = 0f
    /** The big artwork's side (px) as the full player's layout sized it for artwork mode; 0 until then. */
    var artSide by mutableStateOf(0f)
    /** The corners at both ends (px), set from the density. */
    var bigCorner = 0f
    var smallCorner = 0f
    /** The mode animation (0 = artwork mode, 1 = another mode), read in layout and draw lambdas. */
    var modeFraction: () -> Float = { 0f }
    /**
     * Immersive artwork ([ImmersiveArtwork]): the big artwork runs edge to edge from the top of the
     * page, under the header, with square corners and no shadow, and does not shrink while paused.
     */
    var immersive by mutableStateOf(false)
    /**
     * The big artwork's top in its area (px): [artPad], or lower on a tall screen, where the area
     * has more height than the artwork needs and the artwork is centred in it.
     */
    var artTop = 0f
    /** The header's height (px) as the page laid it out. */
    var headerHeight = 0f
    /**
     * Where an immersive artwork's top is from the area's top (px, set by the page's layout): at the
     * top of the page (under the header), or higher where the area is shorter than the artwork, so
     * that its bottom never runs past the area into the controls.
     */
    var immersiveTop = 0f
    /** How much the title row's stars have folded away (px; they show in artwork mode only): the mode area takes it. */
    var titleFold = 0f
    /** The full player's skip swipe (px, read in layer lambdas): the immersive continuation slides with it. */
    var swipe: () -> Float = { 0f }
    /** The swipe up to the queue or the lyrics ([SwipeUpPull]): the artwork's lift (px, upwards) and its share of the most (read in layer lambdas). */
    var lift: () -> Float = { 0f }
    var liftFraction: () -> Float = { 0f }

    fun measure(coordinates: LayoutCoordinates): Rect {
        val r = root ?: return Rect.Zero
        if (!r.isAttached || !coordinates.isAttached) return Rect.Zero
        return r.localBoundingBoxOf(coordinates, clipBounds = false)
    }

    /** The big artwork's square in the current track's style: [immersiveSlot] or [cardSlot]. */
    fun bigSlot(): Rect = if (immersive) immersiveSlot() else cardSlot()

    /**
     * A card's square, centred in its area ([artTop] down): [artSide] (the same in every mode, so the
     * artwork does not resize while the area grows for lyrics or the queue), or before that is known
     * as large as the area allows (up to [artMax]).
     */
    fun cardSlot(): Rect {
        val a = area
        if (a.width <= 0f || a.height <= 0f) return Rect.Zero
        val side = if (artSide > 0f) minOf(artSide, a.width - 2 * artInset) else minOf(a.width - 2 * artInset, a.height - 2 * artPad, artMax)
        if (side <= 0f) return Rect.Zero
        return Rect(Offset(a.center.x - side / 2, a.top + maxOf(artTop, artPad)), Size(side, side))
    }

    /**
     * An immersive artwork's square: the area's full width, from the top of the page (under the
     * header; [immersiveTop]), its continuation running down behind the controls. The same area as
     * a card's, so the controls never move between the two styles.
     */
    fun immersiveSlot(): Rect {
        val a = area
        if (a.width <= 0f || a.height <= 0f) return Rect.Zero
        return Rect(Offset(a.left, a.top + immersiveTop), Size(a.width, a.width))
    }

    /** Where the full player's artwork is at mode fraction [f], shrunk to [pausedScale] while big (not when immersive). */
    fun playerArt(f: Float, pausedScale: Float): Rect {
        val big = bigSlot()
        if (big.width <= 0f) return Rect.Zero
        val s = if (immersive) 1f else lerpF(pausedScale, 1f, f.coerceIn(0f, 1f))
        val from = Rect(big.center - Offset(big.width * s / 2, big.height * s / 2), Size(big.width * s, big.height * s))
        val to = small.takeIf { it.width > 0f } ?: return if (f < 0.5f) from else Rect.Zero
        return Rect(lerpF(from.left, to.left, f), lerpF(from.top, to.top, f), lerpF(from.right, to.right, f), lerpF(from.bottom, to.bottom, f))
    }

    /** The visible corner radius at mode fraction [f]. */
    fun cornerFor(f: Float, @Suppress("UNUSED_PARAMETER") width: Float): Float = lerpF(if (immersive) 0f else bigCorner, smallCorner, f.coerceIn(0f, 1f))

    /**
     * Where the artwork in flight is at sheet progress [p] (0 = the mini player's thumbnail, 1 =
     * landed on [playerArt]). A card follows the progress; an immersive artwork stays at its full
     * size, sliding down with the sheet, until [SHRINK_FROM], and only then shrinks into the
     * thumbnail (Apple Music), so the scene never shrinks beside a continuation laid out for it.
     */
    fun flightRect(p: Float, f: Float, pausedScale: Float, fallbackThumb: Rect): Rect {
        val from = thumb.takeIf { it.width > 0f } ?: fallbackThumb
        val target = playerArt(f, pausedScale)
        val to = if (target.width > 0f) target else from
        val t = flightFraction(p)
        return Rect(lerpF(from.left, to.left, t), lerpF(from.top, to.top, t), lerpF(from.right, to.right, t), lerpF(from.bottom, to.bottom, t))
    }

    /**
     * What puts content laid out for the landed artwork ([bigSlot]: the immersive continuation) onto
     * the artwork in flight at sheet progress [p]: a scale about the top-left corner, then a
     * translation, so the two move and shrink as one unit.
     */
    fun flightTransform(p: Float, f: Float, pausedScale: Float, fallbackThumb: Rect): FlightTransform {
        val big = bigSlot()
        if (big.width <= 0f) return FlightTransform(1f, 0f, 0f)
        val r = flightRect(p, f, pausedScale, fallbackThumb)
        val k = r.width / big.width
        return FlightTransform(k, r.left - big.left * k, r.top - big.top * k)
    }

    /** How far the artwork in flight is from the thumbnail (0) to landed (1) at sheet progress [p]. */
    fun flightFraction(p: Float): Float = if (immersive) (p / SHRINK_FROM).coerceIn(0f, 1f) else p.coerceIn(0f, 1f)

    /** The artwork's shadow (px) at mode fraction [f]: none while immersive and big. */
    fun elevationFor(f: Float, density: Density): Float = with(density) {
        lerpF(if (immersive) 0f else HERO_ELEVATION.toPx(), 2.dp.toPx(), f.coerceIn(0f, 1f))
    }

    companion object {
        /** The mini player's artwork and its corners (square-ish, Navic). */
        val THUMB_SIZE: Dp = 48.dp
        val THUMB_CORNER: Dp = 8.dp
        /**
         * The thumbnail beside the title in a non-artwork mode (before the title block has been
         * measured: it spans that block, about this at the default font size) and its corners.
         */
        val SMALL_SIZE: Dp = 72.dp
        val SMALL_CORNER: Dp = 16.dp
        /** The big artwork's corners and shadow. */
        val HERO_CORNER: Dp = 28.dp
        val HERO_ELEVATION: Dp = 16.dp
        /** The floating mini player: side margin, gap above the navigation bar, corners, shadow and widest size. */
        val MINI_MARGIN: Dp = 12.dp
        val MINI_GAP: Dp = NowPlayingSheetState.MINI_GAP
        val MINI_CORNER: Dp = 16.dp
        val MINI_ELEVATION: Dp = 8.dp
        val MINI_MAX_WIDTH: Dp = 600.dp
        /** How much the artwork shrinks while paused (Navic). */
        const val PAUSED_SCALE = 0.86f
        /** Sheet progress above which an immersive artwork in flight keeps its full size ([flightRect]). */
        const val SHRINK_FROM = 0.6f

        /** The immersive continuation while the sheet moves: gone before the artwork starts shrinking. */
        fun continuationAlpha(p: Float): Float = ((p - 0.45f) / 0.45f).coerceIn(0f, 1f).let { it * it * (3f - 2f * it) }

        /** The immersive artwork's seam fade in flight: the landed one until the continuation has gone. */
        fun seamFraction(p: Float): Float = ((p - 0.3f) / 0.15f).coerceIn(0f, 1f)

        /** The handle (and the immersive header scrim) fade out first, so nothing hangs over the art. */
        fun headerAlpha(p: Float): Float = ((p - 0.85f) / 0.1f).coerceIn(0f, 1f)
    }
}

/**
 * The player as one morphing sheet (Metrolist): a floating mini-player pill above the navigation bar
 * that grows into the full player. Everything that follows the finger (the sheet's position, its
 * inset and corners, the fading layers, the artwork flying from thumbnail to hero) is read in layout
 * or draw lambdas from `state.progress`, so a drag never recomposes the player. The full player sits
 * on a blurred copy of the artwork and is themed (dark) from it, with colours crossfading when the
 * track changes. The pill slides in when something starts playing and out when nothing is left.
 */
@Composable
fun NowPlayingSheet(state: NowPlayingSheetState, bottomInset: Dp, onOpenAlbum: (String) -> Unit, onOpenArtist: (String) -> Unit) {
    val client = LocalCoreClient.current
    val nowPlaying by client.nowPlaying.collectAsStateWithLifecycle()
    val playing by client.isPlaying.collectAsStateWithLifecycle()
    val position = client.position.collectAsStateWithLifecycle()
    val readPosition: () -> Long = remember(position) { { position.value } }
    val scope = rememberCoroutineScope()
    val reducedMotion = LocalReducedMotion.current
    val entry = nowPlaying
    val visible = entry != null || state.isExpanded
    // Slide the pill in and out rather than popping it.
    val appear = remember { Animatable(if (visible) 1f else 0f) }
    LaunchedEffect(visible, reducedMotion) {
        val target = if (visible) 1f else 0f
        if (reducedMotion) appear.snapTo(target) else appear.animateTo(target, if (visible) Motion.sheet else tween(200))
    }
    val visibleNow by rememberUpdatedState(visible)
    val present by remember { derivedStateOf { visibleNow || appear.value > 0f } }
    if (!present) return
    BoxWithConstraints(Modifier.fillMaxSize().zIndex(10f)) {
        val density = LocalDensity.current
        val navPx = with(density) { bottomInset.toPx() }
        val maxHeightPx = constraints.maxHeight.toFloat()
        val miniHeight = miniPlayerHeight()
        val miniHeightPx = with(density) { miniHeight.toPx() }
        val collapsedOffset = maxHeightPx - miniHeightPx - with(density) { HeroGeometry.MINI_GAP.toPx() } - navPx
        LaunchedEffect(collapsedOffset, density) {
            state.collapsedOffset = collapsedOffset
            state.velocityThreshold = with(density) { NowPlayingSheetState.VELOCITY_THRESHOLD.toPx() }
            state.draggable.updateAnchors(DraggableAnchors { SheetValue.Collapsed at collapsedOffset; SheetValue.Expanded at 0f }, state.draggable.targetValue)
        }
        val haptics = LocalHapticFeedback.current
        // Swiping up on the open player opens the queue, the lyrics or a sheet (see [SwipeUpPull]).
        val queuePull = remember { SwipeUpPull(scope) }
        queuePull.haptics = haptics
        queuePull.reduced = reducedMotion
        with(density) {
            queuePull.maxLift = SwipeUpPull.MAX_LIFT.toPx()
            queuePull.commit = SwipeUpPull.COMMIT.toPx()
            queuePull.flingVelocity = SwipeUpPull.FLING.toPx()
        }
        val fling = AnchoredDraggableDefaults.flingBehavior(state.draggable, positionalThreshold = { d -> d * NowPlayingSheetState.POSITIONAL_THRESHOLD }, animationSpec = Motion.sheet)

        // Predictive back drives the sheet down with the gesture, then settles either way.
        PredictiveBackHandler(enabled = state.isExpanded) { events ->
            try {
                events.collect { ev ->
                    val target = collapsedOffset * (ev.progress * 0.35f)
                    state.draggable.anchoredDrag { dragTo(target) }
                }
                state.collapse()
            } catch (e: CancellationException) {
                state.expand()
            }
        }

        // Thresholds of the progress, as derived state: composition reacts when one is crossed,
        // never on every frame of a drag.
        val fullPlayerAccessible by remember(state) { derivedStateOf { state.progress >= FULL_PLAYER_A11Y_PROGRESS } }
        val scrimShown by remember(state) { derivedStateOf { state.progress > 0f } }
        val scrimTappable by remember(state) { derivedStateOf { state.progress > 0.5f } }
        val settledOpen by remember(state) { derivedStateOf { state.progress > 0.9f } }
        // Only the big artwork flies to and from the mini player. In lyrics / queue / about the
        // artwork is a thumbnail beside the title near the bottom of the page: flown opaque from
        // there to the mini player's thumbnail it would float over the fading, shrinking sheet and
        // over the screen behind it, so instead it stays on the page (sliding and fading with it)
        // while the mini player's own thumbnail fades in with the bar.
        val flying by remember(state) { derivedStateOf { state.mode == PlayerMode.Artwork && state.progress.let { it > FLY_EPSILON && it < 1f - FLY_EPSILON } } }

        // Scrim behind the sheet (a pointer affordance only: the collapse button and the sheet's
        // collapse/dismiss actions are the accessible way out).
        if (scrimShown) {
            // clearAndSetSemantics first: semantics modifiers after it on the same node (the click)
            // would otherwise survive.
            Box(
                Modifier.fillMaxSize().clearAndSetSemantics { }.graphicsLayer { alpha = state.progress * 0.6f }.background(Color.Black)
                    .let { if (scrimTappable) it.clickable(indication = null, interactionSource = remember { MutableInteractionSource() }) { scope.launch { state.collapse() } } else it },
            )
        }

        // Content inside the sheet scrolls; whatever it does not consume (dragging down at the top,
        // or up while the sheet is still opening) moves the sheet, and flings settle it.
        val nested = remember(state) {
            object : NestedScrollConnection {
                // The page scrolled in this gesture: the rest of it (and its fling) stays the page's,
                // so a flick that runs into the end of a short screen's scroll never opens the queue.
                var pageScrolled = false
                override fun onPreScroll(available: Offset, source: NestedScrollSource): Offset {
                    val d = available.y
                    if (source != NestedScrollSource.UserInput) return Offset.Zero
                    // Dragging down after a swipe up gives back the artwork's lift before the page scrolls.
                    if (d > 0 && queuePull.held) return Offset(0f, queuePull.pull(d))
                    return if (d < 0 && state.progress < 1f) Offset(0f, state.draggable.dispatchRawDelta(d)) else Offset.Zero
                }
                override fun onPostScroll(consumed: Offset, available: Offset, source: NestedScrollSource): Offset {
                    if (source != NestedScrollSource.UserInput) return Offset.Zero
                    if (consumed.y != 0f) pageScrolled = true
                    val moved = state.draggable.dispatchRawDelta(available.y)
                    // Up past the open sheet: the swipe to the queue.
                    val left = available.y - moved
                    return Offset(0f, moved + if (left < 0f && !pageScrolled) queuePull.pull(left) else 0f)
                }
                override suspend fun onPreFling(available: Velocity): Velocity {
                    pageScrolled = false
                    if (queuePull.held) { queuePull.release(available.y); return available }
                    return if (available.y < 0 && state.progress < 1f) { state.settle(available.y); available } else Velocity.Zero
                }
                override suspend fun onPostFling(consumed: Velocity, available: Velocity): Velocity {
                    state.settle(available.y)
                    return available
                }
            }
        }
        val seedState = LocalArtworkSeedState.current
        // The app's own theme (the player overrides it for its subtree, below).
        val appDark = LocalDarkTheme.current
        val trackCover = entry?.track?.coverArt
        // Immersive artwork on a portrait screen (the square artwork leaves room for the controls).
        val settings by client.settings.collectAsStateWithLifecycle()
        val preference = ImmersiveArtwork.preference(settings[app.hocket.core.SettingKeys.DISPLAY_IMMERSIVE_ARTWORK]?.value)
        val portrait = constraints.maxWidth <= constraints.maxHeight * ImmersiveArtwork.MAX_WIDTH_SHARE
        fun ArtworkLayout?.immersiveOrNull() = this?.takeIf { portrait && it.bottom.style != ArtworkStyle.Card }
        // The scene (the cover, its layout and its colours) changes as one: a new cover waits until
        // its layout and seed are known (at once for one already worked out, a neighbour's, say),
        // so it never shows as a card and then turns immersive, or in the wrong colours. A slow
        // one gives up waiting after [SCENE_HOLD_MS] and shows as a card until it is known.
        val trackLayout by rememberArtworkLayout(trackCover, preference, dark = appDark)
        val trackSeed by artworkSeed(trackCover)
        val carousel = remember { SkipCarousel() }
        val ready = trackLayout != null && (trackSeed != null || trackCover == null)
        var held by remember { mutableStateOf<Scene?>(null) }
        var holdOver by remember(trackCover) { mutableStateOf(false) }
        LaunchedEffect(trackCover) { kotlinx.coroutines.delay(SCENE_HOLD_MS); holdOver = true }
        val scene = when {
            ready -> Scene(trackCover, trackLayout?.layout.immersiveOrNull(), trackSeed)
            // A skip swipe has already shown the new song in its own style: no holding the old one.
            !holdOver && held != null && !carousel.committing -> held!!
            else -> Scene(trackCover, trackLayout?.layout.immersiveOrNull(), trackSeed)
        }
        SideEffect { if (ready) held = scene }
        val coverArt = scene.coverArt
        val seed = scene.seed ?: coverArt?.let { ArtworkColors.seedFor(it) }
        val artworkLayout = scene.layout
        val immersiveEdge = artworkLayout?.bottom
        val hero = remember { HeroGeometry() }
        hero.immersive = immersiveEdge != null
        // The skip swipe's neighbours, with their layouts and colours worked out ahead (cached), so
        // a neighbour slides in in its own style and the commit changes nothing on screen.
        // Held while a skip settles (see [SkipCarousel.shown]); the mini player holds its own.
        val liveNeighbours by rememberNeighbours()
        val neighbours = carousel.shown(liveNeighbours)
        val previousCover = neighbours.previous?.track?.coverArt
        val nextCover = neighbours.next?.track?.coverArt
        val previousLayout = rememberArtworkLayout(previousCover, preference, dark = appDark).value?.layout.immersiveOrNull()
        val nextLayout = rememberArtworkLayout(nextCover, preference, dark = appDark).value?.layout.immersiveOrNull()
        val previousSeed by artworkSeed(previousCover)
        val nextSeed by artworkSeed(nextCover)
        val swiping by remember(carousel) { derivedStateOf { carousel.offset.value != 0f } }
        // The swipe's fraction towards a neighbour in steps (1 = previous, -1 = next): the backdrop
        // and the colours follow it, recomposing a few times a swipe, not every frame.
        val towards by remember(carousel) { derivedStateOf { (carousel.fraction() * SCHEME_STEPS).roundToInt() / SCHEME_STEPS.toFloat() } }
        val previousShown = Triple(previousCover, previousLayout, previousSeed)
        // The full player is dark (it sits on darkened artwork), themed from the artwork, except in
        // artwork mode over a light immersive continuation (a white cover carried on in white): dark
        // controls there. The scheme crossfades on a track or mode change while the player is
        // showing, snaps while hidden, and follows a skip swipe towards the neighbour's.
        val artworkMode = state.mode == PlayerMode.Artwork
        val lightPlayer = immersiveEdge?.light == true && artworkMode
        val targetScheme = remember(seed, lightPlayer) { playerSchemeFor(seed, lightPlayer) }
        // Read as derived state: the swipe's offset changes every frame, the flag only twice a swipe.
        val carouselBusy by remember(carousel) { derivedStateOf { carousel.busy } }
        val playerScheme0 = animateColorScheme(targetScheme, animate = fullPlayerAccessible && !reducedMotion && !carouselBusy)
        val neighbourShown = when {
            towards > 0f -> previousShown.let { (c, l, sd) -> Triple(sd ?: c?.let { ArtworkColors.seedFor(it) }, l, c) }
            towards < 0f -> Triple(nextSeed ?: nextCover?.let { ArtworkColors.seedFor(it) }, nextLayout, nextCover)
            else -> null
        }
        val neighbourLight = neighbourShown?.second?.bottom?.light == true && artworkMode
        val neighbourSeed = neighbourShown?.first
        val neighbourScheme = remember(neighbourShown != null, neighbourSeed, neighbourLight) { if (neighbourShown != null) playerSchemeFor(neighbourSeed, neighbourLight) else null }
        val playerScheme = if (neighbourScheme != null) lerpScheme(playerScheme0, neighbourScheme, abs(towards)) else playerScheme0
        val lightShown = if (neighbourShown != null && abs(towards) >= 0.5f) neighbourLight else lightPlayer
        // The open player is dark (its backdrop, the queue, the lyrics): light icons over it, on a
        // light system theme too; over an immersive artwork's top strip they follow the strip.
        StatusBarOver(if (scrimTappable) (if (artworkMode) artworkLayout?.let { stripLight(it) } ?: false else false) else null)
        val artScale = animateFloatAsState(if (playing || reducedMotion) 1f else HeroGeometry.PAUSED_SCALE, Motion.artwork, label = "artworkScale")
        hero.bigCorner = with(density) { HeroGeometry.HERO_CORNER.toPx() }
        hero.smallCorner = with(density) { HeroGeometry.SMALL_CORNER.toPx() }
        // The mode's animation: 0 with the big artwork, 1 with lyrics / queue / about (the artwork
        // then small beside the title). Snaps while the full player is not showing.
        val mode = state.mode
        val modeAnim = remember { Animatable(if (mode == PlayerMode.Artwork) 0f else 1f) }
        LaunchedEffect(mode, reducedMotion) {
            val target = if (mode == PlayerMode.Artwork) 0f else 1f
            if (reducedMotion || !state.isExpanded) modeAnim.snapTo(target) else modeAnim.animateTo(target, Motion.mode)
        }
        hero.modeFraction = { modeAnim.value }
        // The lift eases out as the artwork flies to the thumbnail (and is gone in another mode).
        hero.lift = { queuePull.lift() * (1f - modeAnim.value) }
        hero.liftFraction = { queuePull.fraction() * (1f - modeAnim.value) }
        // Only from a player already resting open: an expand drag that carries on past the top
        // (settledValue stays Collapsed until it ends) never turns into a pull.
        queuePull.enabled = {
            state.mode == PlayerMode.Artwork && state.progress >= 1f && state.draggable.settledValue == SheetValue.Expanded &&
                !carousel.busy && !state.draggable.isAnimationRunning
        }
        // `swipe.playerUp`: the queue or the lyrics take the artwork's place; the device picker and
        // the song menu come up as sheets ([FullPlayer] sets [SwipeUpPull.openSheet] for those).
        val swipeUp = settings[app.hocket.core.SettingKeys.SWIPE_PLAYER_UP]?.value?.trim()?.removeSurrounding("\"")
        queuePull.onOpen = { state.mode = if (swipeUp == app.hocket.core.SwipeOptions.PLAYER_UP_LYRICS) PlayerMode.Lyrics else PlayerMode.Queue }
        // The sheet moving (a collapse by the button, back or the scrim) drops any lift, so the
        // artwork in flight takes over from an unlifted one.
        LaunchedEffect(queuePull, state) {
            snapshotFlow { state.progress >= 1f && state.draggable.settledValue == SheetValue.Expanded }.collect { open -> if (!open) queuePull.reset() }
        }
        val sheetTitle = stringResource(R.string.player_sheet_title)
        val collapseLabel = stringResource(R.string.player_collapse)
        val expandLabel = stringResource(R.string.player_expand)
        val expanded = state.isExpanded
        val margin = with(density) { HeroGeometry.MINI_MARGIN.toPx() }
        val maxCard = with(density) { HeroGeometry.MINI_MAX_WIDTH.toPx() }
        val corner = with(density) { HeroGeometry.MINI_CORNER.toPx() }
        val sheetCorner = with(density) { SHEET_CORNER.toPx() }
        val cardElevation = with(density) { HeroGeometry.MINI_ELEVATION.toPx() }
        val appearShift = miniHeightPx + with(density) { HeroGeometry.MINI_GAP.toPx() }
        val sheetOffset: () -> Float = { state.draggable.offset.takeIf { !it.isNaN() } ?: if (state.draggable.currentValue == SheetValue.Expanded) 0f else collapsedOffset }
        Box(
            Modifier
                .fillMaxWidth()
                .height(with(density) { maxHeightPx.toDp() })
                .offset { IntOffset(0, sheetOffset().roundToInt()) }
                // The floating card's margins, corners, height and shadow morph into the
                // full-screen sheet; the whole sheet slides in/out with `appear`. One layer, no
                // relayout.
                .graphicsLayer {
                    val p = state.progress
                    alpha = appear.value
                    translationY = (1f - appear.value) * appearShift
                    val cardInset = maxOf(margin, (size.width - maxCard) / 2f + margin)
                    val screenBottom = maxHeightPx - sheetOffset()
                    shape = SheetShape(
                        inset = cardInset * (1f - (p / 0.3f).coerceIn(0f, 1f)),
                        topCorner = sheetTopCorner(p, corner, sheetCorner),
                        bottomCorner = corner * (1f - p),
                        bottom = lerpF(miniHeightPx, screenBottom, (p / 0.5f).coerceIn(0f, 1f)),
                    )
                    clip = true
                    shadowElevation = cardElevation * (1f - p * 3f).coerceIn(0f, 1f)
                }
                .nestedScroll(nested)
                .anchoredDraggable(state.draggable, Orientation.Vertical, overscrollEffect = queuePull, flingBehavior = fling)
                // The drag has non-gesture equivalents: expand from the mini player, and collapse or
                // dismiss the open sheet (also the collapse button and back).
                .semantics {
                    isTraversalGroup = true
                    if (expanded) {
                        paneTitle = sheetTitle
                        collapse(collapseLabel) { scope.launch { state.collapse() }; true }
                        dismiss(collapseLabel) { scope.launch { state.collapse() }; true }
                    } else {
                        expand(expandLabel) { scope.launch { state.expand() }; true }
                    }
                }
                .testTag("nowPlaying.sheet"),
        ) {
            Box(Modifier.fillMaxSize().onPlaced { hero.root = it }) {
                // The full player's backdrop: blurred artwork over the artwork's colour fading to
                // near black, fading in as the sheet opens (Metrolist's curve: nothing for the first
                // tenth, then quickly opaque).
                PlayerBackground(coverArt, seed, moving = settledOpen, Modifier.fillMaxSize().graphicsLayer { alpha = backgroundAlpha(state.progress) })
                // While the artwork is swiped, the neighbour's backdrop fades in with the swipe.
                if (swiping && neighbourShown != null) {
                    val sign = if (towards > 0f) 1f else -1f
                    PlayerBackground(neighbourShown.third, neighbourSeed, moving = false, Modifier.fillMaxSize().graphicsLayer { alpha = backgroundAlpha(state.progress) * (carousel.fraction() * sign).coerceIn(0f, 1f) })
                }
                // An immersive artwork's continuation (reflection or colours) below it, fading out as
                // the artwork shrinks to a thumbnail for lyrics or the queue. While the sheet moves
                // it is one unit with the artwork in flight (the same transform), fading out before
                // the artwork shrinks; while the artwork is swiped it slides with it, and an
                // immersive neighbour's continuation slides in beside it (whatever the current
                // artwork's style: each page in its own).
                val span = { carousel.span }
                // Each with its top strip (the room under the status bar and the handle).
                @Composable
                fun Scene(cover: String?, layout: ArtworkLayout, offsetSpans: Float) {
                    fun layer(feather: Boolean) = Modifier.fillMaxSize().continuationLayer(hero, state, { flying }, { modeAnim.value }, { artScale.value }, density, offsetSpans, span, feather)
                    ImmersiveContinuation(cover, layout.bottom, hero, modifier = layer(feather = true))
                    // Part of the artwork (it looks taller): crisp at a swipe's seam, as the artwork is.
                    ImmersiveTopStrip(cover, layout, hero, modifier = layer(feather = false))
                }
                if (artworkLayout != null) Scene(coverArt, artworkLayout, 0f)
                if (swiping) {
                    previousShown.second?.let { Scene(previousShown.first, it, -1f) }
                    nextLayout?.let { Scene(nextCover, it, 1f) }
                }
                // One theme call, always: the subtree (modes, queue, lyrics, scroll positions) is
                // never rebuilt when the colours change.
                MaterialExpressiveTheme(colorScheme = playerScheme) {
                    CompositionLocalProvider(LocalContentColor provides playerScheme.onSurface, LocalDarkTheme provides !lightShown) {
                        Box(
                            Modifier
                                .fillMaxSize()
                                .graphicsLayer { alpha = ((state.progress - 0.15f) * 4f).coerceIn(0f, 1f) }
                                .then(if (fullPlayerAccessible) Modifier else Modifier.clearAndSetSemantics { }),
                        ) {
                            FullPlayer(
                                mode = mode,
                                onMode = { state.mode = it },
                                onCollapse = { scope.launch { state.collapse() } },
                                onOpenAlbum = onOpenAlbum,
                                onOpenArtist = onOpenArtist,
                                position = readPosition,
                                hero = hero,
                                modeFraction = { modeAnim.value },
                                artworkHidden = { flying },
                                artworkScale = { artScale.value },
                                lyricsVisible = expanded && settledOpen && mode == PlayerMode.Lyrics,
                                onPreviewToggle = { seed -> seedState.seed = if (seedState.seed == null) seed else null },
                                coverArt = coverArt,
                                immersive = artworkLayout,
                                carousel = carousel,
                                neighbours = neighbours,
                                neighbourLayouts = previousLayout to nextLayout,
                                headerAlpha = { HeroGeometry.headerAlpha(state.progress) },
                                swipeUp = queuePull,
                                swipeUpAction = swipeUp,
                            )
                        }
                    }
                }
                // The mini player, fading out as the sheet opens (gone from the tree once the full
                // player is what accessibility sees).
                if (!fullPlayerAccessible) {
                    Box(Modifier.fillMaxWidth().height(miniHeight).graphicsLayer { alpha = 1f - (state.progress * 4f).coerceAtMost(1f) }) {
                        MiniPlayerBar(onExpand = { scope.launch { state.expand() } }, position = readPosition, hero = hero, thumbHidden = { flying }, liveNeighbours = liveNeighbours)
                    }
                }
                // The one artwork that flies from the mini player's thumbnail to the full player's
                // big artwork while the sheet moves in artwork mode (both of those hide meanwhile;
                // see `flying`). Decorative.
                FlyingArtwork(coverArt, hero, progress = { state.progress }, flying = { flying }, modeFraction = { modeAnim.value }, scale = { artScale.value }, immersiveEdge = immersiveEdge, immersiveLayout = artworkLayout)
            }
        }
    }
}

/**
 * Light status bar appearance (dark icons) when [light] is true, light icons when false, while the
 * value is not null; the window's own appearance again once it is.
 */
@Composable
private fun StatusBarOver(light: Boolean?) {
    val view = androidx.compose.ui.platform.LocalView.current
    androidx.compose.runtime.DisposableEffect(light, view) {
        var context = view.context
        while (context !is android.app.Activity && context is android.content.ContextWrapper) context = context.baseContext
        val window = (context as? android.app.Activity)?.window
        if (light == null || window == null) return@DisposableEffect onDispose { }
        val controller = androidx.core.view.WindowCompat.getInsetsController(window, view)
        val before = controller.isAppearanceLightStatusBars
        controller.isAppearanceLightStatusBars = light
        onDispose { controller.isAppearanceLightStatusBars = before }
    }
}

/** The dark text and controls over a light immersive continuation (the core's contrast target). */
private val IMMERSIVE_DARK = Color(0xFF1C1B1F)

/** What the player shows for a song: its cover, its immersive layout (null: a card) and its seed colour. */
private data class Scene(val coverArt: String?, val layout: ArtworkLayout?, val seed: Color?)

/** How long a new cover's scene waits for its layout and colours before showing as a card. */
private const val SCENE_HOLD_MS = 400L

/** The steps the colours take across a skip swipe (one recomposition each). */
private const val SCHEME_STEPS = 12

/**
 * The full player's scheme for a seed: dark, themed from the artwork, or over a light immersive
 * continuation ([light]) dark text on the cover's colour: the one dark the core checked 4.5:1 for,
 * and translucent dark tonal buttons that take on the colour beneath instead of a pale container.
 */
private fun playerSchemeFor(seed: Color?, light: Boolean): ColorScheme {
    val scheme = seed?.let { ArtworkColors.scheme(it, dark = !light) } ?: darkColorScheme()
    return if (!light) scheme else scheme.copy(
        onSurface = IMMERSIVE_DARK, onSurfaceVariant = IMMERSIVE_DARK, onBackground = IMMERSIVE_DARK,
        secondaryContainer = IMMERSIVE_DARK.copy(alpha = 0.12f), onSecondaryContainer = IMMERSIVE_DARK,
        surfaceContainerHighest = IMMERSIVE_DARK.copy(alpha = 0.12f),
    )
}

/** [a] to [b] at [t] for the roles the player animates ([animateColorScheme]). */
private fun lerpScheme(a: ColorScheme, b: ColorScheme, t: Float): ColorScheme {
    fun l(x: Color, y: Color) = androidx.compose.ui.graphics.lerp(x, y, t.coerceIn(0f, 1f))
    return a.copy(
        primary = l(a.primary, b.primary), onPrimary = l(a.onPrimary, b.onPrimary),
        primaryContainer = l(a.primaryContainer, b.primaryContainer), onPrimaryContainer = l(a.onPrimaryContainer, b.onPrimaryContainer),
        secondary = l(a.secondary, b.secondary), onSecondary = l(a.onSecondary, b.onSecondary),
        secondaryContainer = l(a.secondaryContainer, b.secondaryContainer), onSecondaryContainer = l(a.onSecondaryContainer, b.onSecondaryContainer),
        tertiary = l(a.tertiary, b.tertiary), onTertiary = l(a.onTertiary, b.onTertiary),
        surface = l(a.surface, b.surface), onSurface = l(a.onSurface, b.onSurface), onSurfaceVariant = l(a.onSurfaceVariant, b.onSurfaceVariant),
        surfaceContainer = l(a.surfaceContainer, b.surfaceContainer), surfaceContainerHigh = l(a.surfaceContainerHigh, b.surfaceContainerHigh),
        surfaceContainerHighest = l(a.surfaceContainerHighest, b.surfaceContainerHighest),
        outline = l(a.outline, b.outline), outlineVariant = l(a.outlineVariant, b.outlineVariant),
    )
}

/** Sheet progress from which the full player (not the mini bar) is what accessibility sees. */
private const val FULL_PLAYER_A11Y_PROGRESS = 0.6f

/** Below this distance from either anchor the sheet counts as resting (the flying artwork is not drawn). */
private const val FLY_EPSILON = 0.002f

/** The backdrop's opacity for a sheet progress (Metrolist). */
private fun backgroundAlpha(p: Float): Float = (1.4f * sqrt((p.coerceAtLeast(0.1f) - 0.1f))).coerceIn(0f, 1f)

/** A moving sheet's top corners (Material's extra-large shape, as a bottom sheet being dragged keeps). */
private val SHEET_CORNER = 28.dp

/** How much of the travel from fully open the top corners take to round (and to square off as it lands open). */
private const val TOP_CORNER_RAMP = 0.05f

/**
 * The sheet's top corners (px) at progress [p]: the mini card's [miniCorner] when collapsed, growing
 * to [sheetCorner] on the way up, and square only while it rests fully open: off that rest they
 * round within the first [TOP_CORNER_RAMP] of travel, so a dragged sheet's top edge reads as a sheet.
 */
internal fun sheetTopCorner(p: Float, miniCorner: Float, sheetCorner: Float): Float {
    if (p >= 1f) return 0f
    return lerpF(miniCorner, sheetCorner, p.coerceIn(0f, 1f)) * ((1f - p) / TOP_CORNER_RAMP).coerceIn(0f, 1f)
}

/**
 * The sheet's clip: the floating card (side margins, all four corners, the mini player's height)
 * growing into a full-screen rectangle; its top corners per [sheetTopCorner], its bottom ones
 * going from the card's to square (the bottom reaches the screen's).
 */
private class SheetShape(private val inset: Float, private val topCorner: Float, private val bottomCorner: Float, private val bottom: Float) : Shape {
    override fun createOutline(size: Size, layoutDirection: LayoutDirection, density: Density): Outline {
        val top = CornerRadius(topCorner)
        val low = CornerRadius(bottomCorner)
        return Outline.Rounded(RoundRect(left = inset, top = 0f, right = size.width - inset, bottom = bottom.coerceIn(0f, size.height), topLeftCornerRadius = top, topRightCornerRadius = top, bottomRightCornerRadius = low, bottomLeftCornerRadius = low))
    }
    override fun equals(other: Any?): Boolean = other is SheetShape && other.inset == inset && other.topCorner == topCorner && other.bottomCorner == bottomCorner && other.bottom == bottom
    override fun hashCode(): Int = ((31 * inset.hashCode() + topCorner.hashCode()) * 31 + bottomCorner.hashCode()) * 31 + bottom.hashCode()
}

/** A scale about the top-left corner, then a translation (px). */
internal data class FlightTransform(val scale: Float, val x: Float, val y: Float)

/** Where the mini player's thumbnail is before it has been measured. */
private fun fallbackThumb(density: Density): Rect = with(density) {
    val thumbSizePx = HeroGeometry.THUMB_SIZE.toPx()
    val margin = HeroGeometry.MINI_MARGIN.toPx() + 10.dp.toPx()
    Rect(Offset(margin, (NowPlayingSheetState.MINI_HEIGHT.toPx() - thumbSizePx) / 2), Size(thumbSizePx, thumbSizePx))
}

/**
 * The layer an immersive continuation is drawn in (laid out for the landed artwork,
 * [HeroGeometry.immersiveSlot]): at rest it follows the full player's skip swipe, [offsetSpans]
 * pages ([span]) along (a neighbour's continuation: -1 the previous, 1 the next); while the sheet moves it takes
 * the artwork in flight's transform ([HeroGeometry.flightRect]), so the two stay one unit, and
 * fades out ([HeroGeometry.continuationAlpha]) before the artwork shrinks. It fades with the mode
 * too (lyrics / queue).
 */
private fun Modifier.continuationLayer(
    hero: HeroGeometry,
    state: NowPlayingSheetState,
    flying: () -> Boolean,
    modeFraction: () -> Float,
    scale: () -> Float,
    density: Density,
    offsetSpans: Float,
    span: () -> Float,
    feather: Boolean = true,
): Modifier = graphicsLayer {
    val p = state.progress
    val big = hero.bigSlot()
    alpha = HeroGeometry.continuationAlpha(p) * (1f - modeFraction())
    transformOrigin = TransformOrigin(0f, 0f)
    // Offscreen only while a seam is feathered (below).
    compositingStrategy = if (feather && seamFeather(hero, offsetSpans, flying()) != 0f) CompositingStrategy.Offscreen else CompositingStrategy.Auto
    if (flying() && big.width > 0f) {
        val t = hero.flightTransform(p, modeFraction(), scale(), fallbackThumb(density))
        scaleX = t.scale
        scaleY = t.scale
        translationX = t.x + offsetSpans * span() * t.scale
        translationY = t.y
    } else {
        translationX = hero.swipe() + offsetSpans * span()
        // Rising with the artwork as it lifts towards the queue; the continuation (not the top
        // strip, which is part of the artwork) stretches below the artwork's bottom edge, so it
        // stays joined to the artwork there and still reaches the screen's bottom.
        val lift = hero.lift()
        translationY = -lift
        val seam = big.bottom
        if (feather && lift > 0f && seam > 0f && seam < size.height) {
            transformOrigin = TransformOrigin(0f, seam / size.height)
            scaleY = (size.height - seam + lift) / (size.height - seam)
        }
    }
}.drawWithContent {
    drawContent()
    // While the artwork is swiped, the edge facing the other page fades out over [SEAM_FEATHER], so
    // the two pages' continuations and backdrops blend across the seam instead of meeting in a
    // hard line (the artwork above stays crisp). It eases in with the first [SEAM_FEATHER] of the
    // swipe, so nothing shows at the screen's edge as a swipe starts.
    val f = if (feather) seamFeather(hero, offsetSpans, flying()) else 0f
    if (f == 0f) return@drawWithContent
    val w = SEAM_FEATHER.toPx()
    val strength = (abs(hero.swipe()) / w).coerceIn(0f, 1f)
    val faded = Color.Black.copy(alpha = 1f - strength)
    if (f < 0f) {
        drawRect(Brush.horizontalGradient(0f to faded, 1f to Color.Black, startX = 0f, endX = w), size = size.copy(width = w), blendMode = BlendMode.DstIn)
    } else {
        drawRect(Brush.horizontalGradient(0f to Color.Black, 1f to faded, startX = size.width - w, endX = size.width), topLeft = Offset(size.width - w, 0f), size = size.copy(width = w), blendMode = BlendMode.DstIn)
    }
}

/** How wide the soft edge between two swiped pages' continuations is. */
private val SEAM_FEATHER = 56.dp

/**
 * Which edge of a continuation [offsetSpans] pages along faces the other page while the artwork is
 * swiped: -1 its left (the next page's, or the current one moving right), 1 its right, 0 none (at
 * rest, or in flight with the sheet).
 */
private fun seamFeather(hero: HeroGeometry, offsetSpans: Float, flying: Boolean): Float {
    val swipe = hero.swipe()
    if (flying || swipe == 0f) return 0f
    return when {
        offsetSpans > 0f -> -1f
        offsetSpans < 0f -> 1f
        swipe < 0f -> 1f
        else -> -1f
    }
}

/**
 * The artwork in flight: laid out at the big artwork's size and moved/scaled in a graphics layer
 * from the mini player's thumbnail to wherever the full player's artwork is (the big slot, or on
 * its way to the thumbnail beside the title if the mode is changing; measured, so it lands exactly
 * however the page is scrolled or the status bar is sized), along [HeroGeometry.flightRect]. Its
 * corners go from the thumbnail's to the target's, and it gains the target's shadow. With nothing
 * measured yet it just fades out from the thumbnail.
 *
 * Immersive: it stays edge to edge and full size for the first part of a dismiss (sliding down with
 * the sheet, its continuation moving with it), keeps the landed seam until the continuation has
 * faded, then shrinks into the thumbnail, its corners rounding as it leaves the edges.
 */
@Composable
private fun FlyingArtwork(coverArt: String?, hero: HeroGeometry, progress: () -> Float, flying: () -> Boolean, modeFraction: () -> Float, scale: () -> Float, immersiveEdge: app.hocket.core.api.ArtworkEdge? = null, immersiveLayout: app.hocket.core.api.ArtworkLayout? = null) {
    val density = LocalDensity.current
    val thumbSizePx = with(density) { HeroGeometry.THUMB_SIZE.toPx() }
    val fallback = fallbackThumb(density)
    Box(
        Modifier
            // Laid out at the big artwork's size (re-laid out only when that changes).
            .layout { measurable, _ ->
                val w = hero.bigSlot().width.roundToInt().takeIf { it > 0 } ?: thumbSizePx.roundToInt()
                val placeable = measurable.measure(Constraints.fixed(w, w))
                layout(placeable.width, placeable.height) { placeable.place(0, 0) }
            }
            .graphicsLayer {
                if (!flying()) { alpha = 0f; return@graphicsLayer }
                val p = progress()
                val t = hero.flightFraction(p)
                val f = modeFraction()
                val landed = hero.playerArt(f, scale()).width > 0f
                val r = hero.flightRect(p, f, scale(), fallback)
                val base = hero.bigSlot().width.takeIf { it > 0f } ?: thumbSizePx
                val k = r.width / base
                transformOrigin = TransformOrigin(0f, 0f)
                translationX = r.left
                translationY = r.top
                scaleX = k
                scaleY = k
                val fromRadius = HeroGeometry.THUMB_CORNER.toPx()
                val visibleRadius = when {
                    !landed -> fromRadius
                    // Square while edge to edge; rounding quickly to a card's corners once it leaves them.
                    hero.immersive -> lerpF(fromRadius, HeroGeometry.HERO_CORNER.toPx(), t) * ((1f - t) / 0.15f).coerceIn(0f, 1f)
                    else -> lerpF(fromRadius, hero.cornerFor(f, r.width), t)
                }
                shape = RoundedCornerShape(visibleRadius / k.coerceAtLeast(0.01f))
                clip = true
                shadowElevation = if (landed) hero.elevationFor(f, this) * t else 0f
                alpha = if (landed) 1f else (1f - p * 4f).coerceIn(0f, 1f)
            }
            .clearAndSetSemantics { },
    ) {
        // Immersive: the landed seam (its bottom fading into the continuation) while the
        // continuation shows, square once it has gone. A flat extension has no seam (the colour
        // carries on crisply): its edge fades as the continuation does instead, so the opaque
        // artwork never ends in a hard line over a half-faded continuation; and, where the seam
        // shows, its own short fade, as on the page.
        val flat = immersiveEdge?.isFlat() == true
        Artwork(
            coverArt, ArtworkSizes.FULL, null,
            Modifier.fillMaxSize().immersiveFade(immersiveEdge, evenFlat = flat, fraction = {
                val p = progress()
                if (flat) (1f - HeroGeometry.continuationAlpha(p)) * HeroGeometry.seamFraction(p) else HeroGeometry.seamFraction(p)
            }, top = immersiveLayout?.top, topFraction = { HeroGeometry.continuationAlpha(progress()) }, crispFraction = { HeroGeometry.seamFraction(progress()) }),
            RectangleShape,
        )
    }
}

/**
 * The full player's backdrop: the artwork, blurred and slowly moving (the lyrics background's AGSL
 * warp, Android 13+, so artwork, lyrics and the queue share one backdrop), darkened by its adaptive
 * scrim. It moves only while [moving] (the player is open and settled) and the animated background
 * is on and battery saver off; otherwise a still. Before Android 13 (Metrolist): the artwork heavily
 * blurred (Android 12) over a gradient from the artwork's colour to near black (the whole backdrop
 * before Android 12, and what shows while the image loads), under a dark scrim. Crossfades on a
 * track change.
 */
@Composable
private fun PlayerBackground(coverArt: String?, seed: Color?, moving: Boolean, modifier: Modifier) {
    val reduced = LocalReducedMotion.current
    if (Build.VERSION.SDK_INT >= 33) {
        val client = LocalCoreClient.current
        val settings by client.settings.collectAsStateWithLifecycle()
        val batterySaver by client.batterySaver.collectAsStateWithLifecycle()
        val animated = settings[app.hocket.core.SettingKeys.DISPLAY_ANIMATED_BACKGROUND]?.value?.trim() != "false" && !batterySaver
        Box(modifier.background(Color.Black)) {
            Crossfade(targetState = coverArt, animationSpec = tween(if (reduced) 0 else Motion.BACKGROUND_MS), label = "playerBackground") { art ->
                app.hocket.ui.lyrics.LyricsBackground(art, animated = animated, visible = moving, modifier = Modifier.fillMaxSize())
            }
        }
        return
    }
    Box(modifier.background(Color.Black)) {
        Crossfade(targetState = coverArt to seed, animationSpec = tween(if (reduced) 0 else Motion.BACKGROUND_MS), label = "playerBackground") { (art, colour) ->
            Box(Modifier.fillMaxSize()) {
                val c = colour ?: Color.DarkGray
                Box(Modifier.fillMaxSize().background(Brush.verticalGradient(0f to c, 0.5f to c.darken(0.6f), 1f to Color(0xFF050505))))
                if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S && art != null) {
                    Artwork(
                        art, ArtworkSizes.LIST, null,
                        Modifier.fillMaxSize().graphicsLayer { scaleX = 1.25f; scaleY = 1.25f }.blur(90.dp, BlurredEdgeTreatment.Rectangle),
                        RectangleShape,
                    )
                }
            }
        }
        // Darker towards the controls at the bottom.
        Box(Modifier.fillMaxSize().background(Brush.verticalGradient(0f to Color.Black.copy(alpha = 0.40f), 1f to Color.Black.copy(alpha = 0.62f))))
    }
}

private fun Color.darken(factor: Float): Color = Color(red * factor, green * factor, blue * factor, alpha)

/**
 * Animates the key roles of a colour scheme (~[Motion.COLOUR_MS]) so an artwork change crossfades
 * the player's colours instead of snapping; with [animate] false it follows the target at once.
 */
@Composable
private fun animateColorScheme(target: ColorScheme, animate: Boolean): ColorScheme {
    val spec: AnimationSpec<Color> = if (animate) tween(Motion.COLOUR_MS) else snap()
    @Composable fun a(c: Color) = animateColorAsState(c, spec, label = "scheme").value
    return target.copy(
        primary = a(target.primary),
        onPrimary = a(target.onPrimary),
        primaryContainer = a(target.primaryContainer),
        onPrimaryContainer = a(target.onPrimaryContainer),
        secondary = a(target.secondary),
        onSecondary = a(target.onSecondary),
        secondaryContainer = a(target.secondaryContainer),
        onSecondaryContainer = a(target.onSecondaryContainer),
        tertiary = a(target.tertiary),
        onTertiary = a(target.onTertiary),
        surface = a(target.surface),
        onSurface = a(target.onSurface),
        onSurfaceVariant = a(target.onSurfaceVariant),
        surfaceContainer = a(target.surfaceContainer),
        surfaceContainerHigh = a(target.surfaceContainerHigh),
        surfaceContainerHighest = a(target.surfaceContainerHighest),
        outline = a(target.outline),
        outlineVariant = a(target.outlineVariant),
    )
}

/** Resolves the artwork's seed colour: palette from the cached file, else the id-derived placeholder hue. */
@Composable
fun artworkSeed(coverArt: String?): androidx.compose.runtime.State<Color?> {
    val client = LocalCoreClient.current
    // Null while worked out; one worked out before (a neighbour's, say) is there from the first frame.
    val state = remember(coverArt) { mutableStateOf(coverArt?.let { seeds.get(it) }) }
    LaunchedEffect(state) {
        if (state.value != null || coverArt == null) return@LaunchedEffect
        val seed = ArtworkColors.seedFrom(client.artworkPath(coverArt, ArtworkSizes.LIST)) ?: ArtworkColors.seedFor(coverArt)
        seeds.put(coverArt, seed)
        state.value = seed
    }
    return state
}

/** Seed colours by cover id. */
private val seeds = android.util.LruCache<String, Color>(64)
