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
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.pager.HorizontalPager
import androidx.compose.foundation.pager.rememberPagerState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.KeyboardArrowDown
import androidx.compose.material3.ColorScheme
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.LocalContentColor
import androidx.compose.material3.MaterialExpressiveTheme
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.PrimaryScrollableTabRow
import androidx.compose.material3.PrimaryTabRow
import androidx.compose.material3.Tab
import androidx.compose.material3.Text
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
import app.hocket.ui.lyrics.LyricsPage
import app.hocket.ui.queue.QueuePanel
import app.hocket.ui.theme.ArtworkColors
import app.hocket.ui.theme.LocalArtworkSeedState
import app.hocket.ui.theme.LocalDarkTheme
import app.hocket.ui.theme.Motion
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.launch
import kotlin.math.roundToInt
import kotlin.math.sqrt

enum class SheetValue { Collapsed, Expanded }

/**
 * The now-playing sheet's drag state: one [AnchoredDraggableState] whose offset is the sheet's top
 * edge. `progress` is 0 collapsed (mini bar) to 1 expanded (full player).
 */
class NowPlayingSheetState(initial: SheetValue = SheetValue.Collapsed) {
    val draggable = AnchoredDraggableState(initial)
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
        /** Saves where the sheet is headed, so a rotation keeps an open player open. */
        val Saver: Saver<NowPlayingSheetState, String> = Saver(
            save = { it.draggable.targetValue.name },
            restore = { NowPlayingSheetState(SheetValue.valueOf(it)) },
        )
        val MINI_HEIGHT: Dp = 64.dp
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
 * Where the artwork is in both players, for the one artwork that flies between them while the sheet
 * moves: the mini player's round thumbnail and the page's artwork slot report their bounds relative
 * to the sheet ([root]); the sheet interpolates between the two in a graphics layer.
 */
@Stable
internal class HeroGeometry {
    var root: LayoutCoordinates? = null
    var thumb by mutableStateOf(Rect.Zero)
    var slot by mutableStateOf(Rect.Zero)

    fun measure(coordinates: LayoutCoordinates): Rect {
        val r = root ?: return Rect.Zero
        if (!r.isAttached || !coordinates.isAttached) return Rect.Zero
        return r.localBoundingBoxOf(coordinates, clipBounds = false)
    }

    companion object {
        val THUMB_SIZE: Dp = 40.dp
        val HERO_CORNER: Dp = 16.dp
        val HERO_ELEVATION: Dp = 16.dp
        /** The floating mini player's side margin, gap above the navigation bar, and corners. */
        val MINI_MARGIN: Dp = 12.dp
        val MINI_GAP: Dp = NowPlayingSheetState.MINI_GAP
        val MINI_CORNER: Dp = 32.dp
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
        val flying by remember(state) { derivedStateOf { state.progress.let { it > FLY_EPSILON && it < 1f - FLY_EPSILON } } }

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
        val coverArt = entry?.track?.coverArt
        val artworkSeed by artworkSeed(coverArt)
        val seed = artworkSeed ?: coverArt?.let { ArtworkColors.seedFor(it) }
        // The full player is always dark (it sits on darkened artwork), themed from the artwork; the
        // scheme crossfades on a track change while the player is showing, and snaps while hidden.
        val targetScheme = remember(seed) { seed?.let { ArtworkColors.scheme(it, dark = true) } ?: darkColorScheme() }
        val playerScheme = animateColorScheme(targetScheme, animate = fullPlayerAccessible && !reducedMotion)
        val artScale = animateFloatAsState(if (playing || reducedMotion) 1f else HeroGeometry.PAUSED_SCALE, Motion.artwork, label = "artworkScale")
        val hero = remember { HeroGeometry() }
        val pager = rememberPagerState { 3 }
        val sheetTitle = stringResource(R.string.player_sheet_title)
        val collapseLabel = stringResource(R.string.player_collapse)
        val expandLabel = stringResource(R.string.player_expand)
        val expanded = state.isExpanded
        val margin = with(density) { HeroGeometry.MINI_MARGIN.toPx() }
        val corner = with(density) { HeroGeometry.MINI_CORNER.toPx() }
        val appearShift = miniHeightPx + with(density) { HeroGeometry.MINI_GAP.toPx() }
        Box(
            Modifier
                .fillMaxWidth()
                .height(with(density) { maxHeightPx.toDp() })
                .offset { IntOffset(0, (state.draggable.offset.takeIf { !it.isNaN() } ?: if (state.draggable.currentValue == SheetValue.Expanded) 0f else collapsedOffset).roundToInt()) }
                // The pill's margin and corners morph into the full-screen sheet; the whole sheet
                // slides in/out with `appear`. One layer, no relayout.
                .graphicsLayer {
                    val p = state.progress
                    alpha = appear.value
                    translationY = (1f - appear.value) * appearShift
                    shape = SheetShape(inset = margin * (1f - (p / 0.3f).coerceIn(0f, 1f)), corner = corner * (1f - p))
                    clip = true
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
                // The full player's backdrop: blurred artwork under a dark scrim, fading in as the
                // sheet opens (Metrolist's curve: nothing for the first tenth, then quickly opaque).
                PlayerBackground(coverArt, seed, Modifier.fillMaxSize().graphicsLayer { alpha = backgroundAlpha(state.progress) })
                // One theme call, always: the subtree (pager, queue, lyrics, scroll positions) is
                // never rebuilt when the colours change.
                MaterialExpressiveTheme(colorScheme = playerScheme) {
                    CompositionLocalProvider(LocalContentColor provides playerScheme.onSurface, LocalDarkTheme provides true) {
                        Column(
                            Modifier
                                .fillMaxSize()
                                .graphicsLayer { alpha = ((state.progress - 0.15f) * 4f).coerceIn(0f, 1f) }
                                .then(if (fullPlayerAccessible) Modifier else Modifier.clearAndSetSemantics { })
                                .statusBarsPadding(),
                        ) {
                            Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
                                IconButton(onClick = { scope.launch { state.collapse() } }, modifier = Modifier.testTag("player.collapse")) { Icon(Icons.Filled.KeyboardArrowDown, collapseLabel) }
                                val tabs: @Composable () -> Unit = {
                                    listOf(R.string.player_tab_now_playing, R.string.player_tab_queue, R.string.player_tab_lyrics).forEachIndexed { i, res ->
                                        Tab(selected = pager.currentPage == i, modifier = Modifier.testTag("player.tab.$i"), onClick = { scope.launch { pager.animateScrollToPage(i) } }, text = { Text(stringResource(res), maxLines = 1) })
                                    }
                                }
                                // Large fonts: the tabs keep their text size and scroll rather than
                                // squeezing three labels into a third of the row each.
                                if (LocalDensity.current.fontScale > 1.3f) {
                                    PrimaryScrollableTabRow(selectedTabIndex = pager.currentPage, modifier = Modifier.weight(1f), containerColor = Color.Transparent, edgePadding = 0.dp, divider = {}) { tabs() }
                                } else {
                                    PrimaryTabRow(selectedTabIndex = pager.currentPage, modifier = Modifier.weight(1f), containerColor = Color.Transparent, divider = {}) { tabs() }
                                    Box(Modifier.size(48.dp))
                                }
                            }
                            HorizontalPager(state = pager, modifier = Modifier.fillMaxSize(), beyondViewportPageCount = 0, userScrollEnabled = settledOpen) { page ->
                                when (page) {
                                    0 -> NowPlayingPage(
                                        onOpenAlbum = onOpenAlbum,
                                        onOpenArtist = onOpenArtist,
                                        position = readPosition,
                                        hero = hero,
                                        artworkHidden = { flying },
                                        artworkScale = { artScale.value },
                                        onPreviewToggle = { seed -> seedState.seed = if (seedState.seed == null) seed else null },
                                    )
                                    1 -> QueuePanel(Modifier.fillMaxSize())
                                    2 -> LyricsPage(visible = expanded && settledOpen, modifier = Modifier.fillMaxSize())
                                }
                            }
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
                // The one artwork that flies from the thumbnail to the page's artwork while the
                // sheet moves (both of those hide meanwhile). Decorative.
                FlyingArtwork(coverArt, hero, progress = { state.progress }, flying = { flying }, onArtworkPage = { pager.currentPage == 0 && pager.currentPageOffsetFraction == 0f }, scale = { artScale.value })
            }
        }
    }
}

/** Sheet progress from which the full player (not the mini bar) is what accessibility sees. */
private const val FULL_PLAYER_A11Y_PROGRESS = 0.6f

/** Below this distance from either anchor the sheet counts as resting (the flying artwork is not drawn). */
private const val FLY_EPSILON = 0.002f

/** The backdrop's opacity for a sheet progress (Metrolist). */
private fun backgroundAlpha(p: Float): Float = (1.4f * sqrt((p.coerceAtLeast(0.1f) - 0.1f))).coerceIn(0f, 1f)

/** The sheet's clip: the pill's side margins and top corners, shrinking to a full-screen rectangle. */
private class SheetShape(private val inset: Float, private val corner: Float) : Shape {
    override fun createOutline(size: Size, layoutDirection: LayoutDirection, density: Density): Outline =
        Outline.Rounded(RoundRect(left = inset, top = 0f, right = size.width - inset, bottom = size.height, topLeftCornerRadius = CornerRadius(corner), topRightCornerRadius = CornerRadius(corner)))
    override fun equals(other: Any?): Boolean = other is SheetShape && other.inset == inset && other.corner == corner
    override fun hashCode(): Int = 31 * inset.hashCode() + corner.hashCode()
}

/**
 * The artwork in flight: laid out at the hero's size and moved/scaled in a graphics layer from the
 * mini player's round thumbnail to the page's slot (both measured, so it lands exactly, however the
 * page is scrolled or the status bar and tabs are sized). Its corners go from a circle to the hero's
 * radius, and it gains the hero's shadow. When the player is on another tab (no artwork slot), it
 * just fades out from the thumbnail.
 */
@Composable
private fun FlyingArtwork(coverArt: String?, hero: HeroGeometry, progress: () -> Float, flying: () -> Boolean, onArtworkPage: () -> Boolean, scale: () -> Float) {
    val density = LocalDensity.current
    val thumbSizePx = with(density) { HeroGeometry.THUMB_SIZE.toPx() }
    val fallbackThumb = with(density) {
        val margin = HeroGeometry.MINI_MARGIN.toPx() + 8.dp.toPx() + 4.dp.toPx()
        Rect(Offset(margin, (NowPlayingSheetState.MINI_HEIGHT.toPx() - thumbSizePx) / 2), Size(thumbSizePx, thumbSizePx))
    }
    Box(
        Modifier
            // Laid out at the slot's size (re-laid out only when the slot's size changes).
            .layout { measurable, constraints ->
                val w = hero.slot.width.roundToInt().takeIf { it > 0 } ?: thumbSizePx.roundToInt()
                val placeable = measurable.measure(Constraints.fixed(w, w))
                layout(placeable.width, placeable.height) { placeable.place(0, 0) }
            }
            .graphicsLayer {
                if (!flying()) { alpha = 0f; return@graphicsLayer }
                val p = progress()
                val from = hero.thumb.takeIf { it.width > 0f } ?: fallbackThumb
                val slot = hero.slot
                val onPage = onArtworkPage() && slot.width > 0f
                val base = if (slot.width > 0f) slot.width else thumbSizePx
                val to = if (onPage) {
                    val s = scale()
                    Rect(slot.center - Offset(slot.width * s / 2, slot.height * s / 2), Size(slot.width * s, slot.height * s))
                } else from
                val left = lerpF(from.left, to.left, p)
                val top = lerpF(from.top, to.top, p)
                val sizePx = lerpF(from.width, to.width, p)
                val k = sizePx / base
                transformOrigin = TransformOrigin(0f, 0f)
                translationX = left
                translationY = top
                scaleX = k
                scaleY = k
                val visibleRadius = lerpF(from.width / 2f, if (onPage) HeroGeometry.HERO_CORNER.toPx() * scale() else from.width / 2f, p)
                shape = RoundedCornerShape(visibleRadius / k.coerceAtLeast(0.01f))
                clip = true
                shadowElevation = if (onPage) HeroGeometry.HERO_ELEVATION.toPx() * p else 0f
                alpha = if (onPage) 1f else (1f - p * 4f).coerceIn(0f, 1f)
            }
            .clearAndSetSemantics { },
    ) {
        Artwork(coverArt, ArtworkSizes.FULL, null, Modifier.fillMaxSize(), RectangleShape)
    }
}

/**
 * The full player's backdrop (Metrolist): the artwork, heavily blurred (Android 12+), over a vertical
 * gradient from the artwork's colour to near black (the whole backdrop before Android 12, and what
 * shows while the image loads), under a dark scrim that keeps light text legible on any cover.
 * Crossfades on a track change.
 */
@Composable
private fun PlayerBackground(coverArt: String?, seed: Color?, modifier: Modifier) {
    val reduced = LocalReducedMotion.current
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
