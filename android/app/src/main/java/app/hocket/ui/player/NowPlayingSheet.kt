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
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Outline
import androidx.compose.ui.graphics.RectangleShape
import androidx.compose.ui.graphics.Shape
import androidx.compose.ui.graphics.TransformOrigin
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
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.launch
import kotlin.math.roundToInt
import kotlin.math.sqrt

enum class SheetValue { Collapsed, Expanded }

/**
 * What fills the full player's big area: the artwork, or lyrics, the queue or the song's details in
 * its place (the artwork then sits small beside the title). Kept across collapsing and expanding
 * the player and across rotation (Apple Music).
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
                    parts.getOrNull(1)?.let { m -> PlayerMode.entries.firstOrNull { it.name == m } } ?: PlayerMode.Artwork,
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
    /** The header's height (px) as the page laid it out: the immersive artwork starts this far above the area. */
    var headerHeight = 0f

    fun measure(coordinates: LayoutCoordinates): Rect {
        val r = root ?: return Rect.Zero
        if (!r.isAttached || !coordinates.isAttached) return Rect.Zero
        return r.localBoundingBoxOf(coordinates, clipBounds = false)
    }

    /**
     * The big artwork's square, centred across the top of its area: [artSide] (the same in every
     * mode, so the artwork does not resize while the area grows for lyrics or the queue), or before
     * that is known as large as the area allows (up to [artMax]).
     */
    fun bigSlot(): Rect {
        val a = area
        if (a.width <= 0f || a.height <= 0f) return Rect.Zero
        if (immersive) return Rect(Offset(a.left, a.top - headerHeight), Size(a.width, a.width))
        val side = if (artSide > 0f) minOf(artSide, a.width - 2 * artInset) else minOf(a.width - 2 * artInset, a.height - 2 * artPad, artMax)
        if (side <= 0f) return Rect.Zero
        return Rect(Offset(a.center.x - side / 2, a.top + artPad), Size(side, side))
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

    /** The artwork's shadow (px) at mode fraction [f]: none while immersive and big. */
    fun elevationFor(f: Float, density: Density): Float = with(density) {
        lerpF(if (immersive) 0f else HERO_ELEVATION.toPx(), 2.dp.toPx(), f.coerceIn(0f, 1f))
    }

    companion object {
        /** The mini player's artwork and its corners (square-ish, Navic). */
        val THUMB_SIZE: Dp = 48.dp
        val THUMB_CORNER: Dp = 8.dp
        /** The thumbnail beside the title in a non-artwork mode, and its corners. */
        val SMALL_SIZE: Dp = 56.dp
        val SMALL_CORNER: Dp = 12.dp
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
                override fun onPreScroll(available: Offset, source: NestedScrollSource): Offset {
                    val d = available.y
                    return if (d < 0 && source == NestedScrollSource.UserInput && state.progress < 1f) Offset(0f, state.draggable.dispatchRawDelta(d)) else Offset.Zero
                }
                override fun onPostScroll(consumed: Offset, available: Offset, source: NestedScrollSource): Offset =
                    if (source == NestedScrollSource.UserInput) Offset(0f, state.draggable.dispatchRawDelta(available.y)) else Offset.Zero
                override suspend fun onPreFling(available: Velocity): Velocity {
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
        val coverArt = entry?.track?.coverArt
        val artworkSeed by artworkSeed(coverArt)
        val seed = artworkSeed ?: coverArt?.let { ArtworkColors.seedFor(it) }
        // Immersive artwork on a portrait screen (the square artwork leaves room for the controls).
        val settings by client.settings.collectAsStateWithLifecycle()
        val preference = ImmersiveArtwork.preference(settings[app.hocket.core.SettingKeys.DISPLAY_IMMERSIVE_ARTWORK]?.value)
        val artworkLayout by rememberArtworkLayout(coverArt, preference, dark = appDark)
        val portrait = constraints.maxWidth <= constraints.maxHeight * ImmersiveArtwork.MAX_WIDTH_SHARE
        val immersiveEdge = artworkLayout?.bottom?.takeIf { portrait && it.style != ArtworkStyle.Card }
        val hero = remember { HeroGeometry() }
        hero.immersive = immersiveEdge != null
        // The full player is dark (it sits on darkened artwork), themed from the artwork, except in
        // artwork mode over a light immersive continuation (a white cover carried on in white): dark
        // controls there. The scheme crossfades on a track or mode change while the player is
        // showing, and snaps while hidden.
        val lightPlayer = immersiveEdge?.light == true && state.mode == PlayerMode.Artwork
        val targetScheme = remember(seed, lightPlayer) {
            val scheme = seed?.let { ArtworkColors.scheme(it, dark = !lightPlayer) } ?: darkColorScheme()
            // Over a light continuation the text sits on the cover's colour, not on the scheme's
            // near-white surfaces: the one dark the core checked 4.5:1 for, and translucent dark
            // tonal buttons that take on the colour beneath instead of a pale container.
            if (!lightPlayer) scheme else scheme.copy(
                onSurface = IMMERSIVE_DARK, onSurfaceVariant = IMMERSIVE_DARK, onBackground = IMMERSIVE_DARK,
                secondaryContainer = IMMERSIVE_DARK.copy(alpha = 0.12f), onSecondaryContainer = IMMERSIVE_DARK,
                surfaceContainerHighest = IMMERSIVE_DARK.copy(alpha = 0.12f),
            )
        }
        val playerScheme = animateColorScheme(targetScheme, animate = fullPlayerAccessible && !reducedMotion)
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
        val sheetTitle = stringResource(R.string.player_sheet_title)
        val collapseLabel = stringResource(R.string.player_collapse)
        val expandLabel = stringResource(R.string.player_expand)
        val expanded = state.isExpanded
        val margin = with(density) { HeroGeometry.MINI_MARGIN.toPx() }
        val maxCard = with(density) { HeroGeometry.MINI_MAX_WIDTH.toPx() }
        val corner = with(density) { HeroGeometry.MINI_CORNER.toPx() }
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
                        corner = corner * (1f - p),
                        bottom = lerpF(miniHeightPx, screenBottom, (p / 0.5f).coerceIn(0f, 1f)),
                    )
                    clip = true
                    shadowElevation = cardElevation * (1f - p * 3f).coerceIn(0f, 1f)
                }
                .nestedScroll(nested)
                .anchoredDraggable(state.draggable, Orientation.Vertical, flingBehavior = fling)
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
                // An immersive artwork's continuation (reflection or colours) below it, fading out as
                // the artwork shrinks to a thumbnail for lyrics, the queue or the details.
                if (immersiveEdge != null) {
                    ImmersiveContinuation(coverArt, immersiveEdge, hero, modifier = Modifier.fillMaxSize().graphicsLayer { alpha = backgroundAlpha(state.progress) * (1f - modeAnim.value) })
                }
                // One theme call, always: the subtree (modes, queue, lyrics, scroll positions) is
                // never rebuilt when the colours change.
                MaterialExpressiveTheme(colorScheme = playerScheme) {
                    CompositionLocalProvider(LocalContentColor provides playerScheme.onSurface, LocalDarkTheme provides !lightPlayer) {
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
                                immersive = artworkLayout?.takeIf { immersiveEdge != null },
                            )
                        }
                    }
                }
                // The mini player, fading out as the sheet opens (gone from the tree once the full
                // player is what accessibility sees).
                if (!fullPlayerAccessible) {
                    Box(Modifier.fillMaxWidth().height(miniHeight).graphicsLayer { alpha = 1f - (state.progress * 4f).coerceAtMost(1f) }) {
                        MiniPlayerBar(onExpand = { scope.launch { state.expand() } }, position = readPosition, hero = hero, thumbHidden = { flying })
                    }
                }
                // The one artwork that flies from the mini player's thumbnail to the full player's
                // big artwork while the sheet moves in artwork mode (both of those hide meanwhile;
                // see `flying`). Decorative.
                FlyingArtwork(coverArt, hero, progress = { state.progress }, flying = { flying }, modeFraction = { modeAnim.value }, scale = { artScale.value }, immersiveEdge = immersiveEdge)
            }
        }
    }
}

/** The dark text and controls over a light immersive continuation (the core's contrast target). */
private val IMMERSIVE_DARK = Color(0xFF1C1B1F)

/** Sheet progress from which the full player (not the mini bar) is what accessibility sees. */
private const val FULL_PLAYER_A11Y_PROGRESS = 0.6f

/** Below this distance from either anchor the sheet counts as resting (the flying artwork is not drawn). */
private const val FLY_EPSILON = 0.002f

/** The backdrop's opacity for a sheet progress (Metrolist). */
private fun backgroundAlpha(p: Float): Float = (1.4f * sqrt((p.coerceAtLeast(0.1f) - 0.1f))).coerceIn(0f, 1f)

/**
 * The sheet's clip: the floating card (side margins, all four corners, the mini player's height)
 * growing into a full-screen rectangle.
 */
private class SheetShape(private val inset: Float, private val corner: Float, private val bottom: Float) : Shape {
    override fun createOutline(size: Size, layoutDirection: LayoutDirection, density: Density): Outline =
        Outline.Rounded(RoundRect(left = inset, top = 0f, right = size.width - inset, bottom = bottom.coerceIn(0f, size.height), cornerRadius = CornerRadius(corner)))
    override fun equals(other: Any?): Boolean = other is SheetShape && other.inset == inset && other.corner == corner && other.bottom == bottom
    override fun hashCode(): Int = (31 * inset.hashCode() + corner.hashCode()) * 31 + bottom.hashCode()
}

/**
 * The artwork in flight: laid out at the big artwork's size and moved/scaled in a graphics layer
 * from the mini player's thumbnail to wherever the full player's artwork is (the big slot, or on
 * its way to the thumbnail beside the title if the mode is changing; measured, so it lands exactly however the
 * page is scrolled or the status bar is sized). Its corners go from the thumbnail's to the target's,
 * and it gains the target's shadow. With nothing measured yet it just fades out from the thumbnail.
 */
@Composable
private fun FlyingArtwork(coverArt: String?, hero: HeroGeometry, progress: () -> Float, flying: () -> Boolean, modeFraction: () -> Float, scale: () -> Float, immersiveEdge: app.hocket.core.api.ArtworkEdge? = null) {
    val density = LocalDensity.current
    val thumbSizePx = with(density) { HeroGeometry.THUMB_SIZE.toPx() }
    val fallbackThumb = with(density) {
        val margin = HeroGeometry.MINI_MARGIN.toPx() + 10.dp.toPx()
        Rect(Offset(margin, (NowPlayingSheetState.MINI_HEIGHT.toPx() - thumbSizePx) / 2), Size(thumbSizePx, thumbSizePx))
    }
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
                val f = modeFraction()
                val from = hero.thumb.takeIf { it.width > 0f } ?: fallbackThumb
                val target = hero.playerArt(f, scale())
                val landed = target.width > 0f
                val to = if (landed) target else from
                val base = hero.bigSlot().width.takeIf { it > 0f } ?: thumbSizePx
                val left = lerpF(from.left, to.left, p)
                val top = lerpF(from.top, to.top, p)
                val sizePx = lerpF(from.width, to.width, p)
                val k = sizePx / base
                transformOrigin = TransformOrigin(0f, 0f)
                translationX = left
                translationY = top
                scaleX = k
                scaleY = k
                val fromRadius = HeroGeometry.THUMB_CORNER.toPx()
                val visibleRadius = lerpF(fromRadius, if (landed) hero.cornerFor(f, to.width) else fromRadius, p)
                shape = RoundedCornerShape(visibleRadius / k.coerceAtLeast(0.01f))
                clip = true
                shadowElevation = if (landed) hero.elevationFor(f, this) * p else 0f
                alpha = if (landed) 1f else (1f - p * 4f).coerceIn(0f, 1f)
            }
            .clearAndSetSemantics { },
    ) {
        // Immersive: its bottom fades into the continuation as the sheet opens (and back as it
        // closes), so the seam is the same in flight as landed; the corners go thumbnail to square.
        Artwork(coverArt, ArtworkSizes.FULL, null, Modifier.fillMaxSize().immersiveFade(immersiveEdge, fraction = progress), RectangleShape)
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
    return androidx.compose.runtime.produceState<Color?>(initialValue = null, coverArt) {
        value = if (coverArt == null) null else (ArtworkColors.seedFrom(client.artworkPath(coverArt, ArtworkSizes.LIST)) ?: ArtworkColors.seedFor(coverArt))
    }
}
