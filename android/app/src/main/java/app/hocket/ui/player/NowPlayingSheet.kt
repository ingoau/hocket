package app.hocket.ui.player

import androidx.activity.compose.PredictiveBackHandler
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
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.pager.HorizontalPager
import androidx.compose.foundation.pager.rememberPagerState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.KeyboardArrowDown
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialExpressiveTheme
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.PrimaryTabRow
import androidx.compose.material3.Surface
import androidx.compose.material3.Tab
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.derivedStateOf
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.draw.clip
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.input.nestedscroll.NestedScrollConnection
import androidx.compose.ui.input.nestedscroll.NestedScrollSource
import androidx.compose.ui.input.nestedscroll.nestedScroll
import androidx.compose.ui.unit.Velocity
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.stringResource
import androidx.compose.material3.PrimaryScrollableTabRow
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.collapse
import androidx.compose.ui.semantics.dismiss
import androidx.compose.ui.semantics.expand
import androidx.compose.ui.semantics.isTraversalGroup
import androidx.compose.ui.semantics.paneTitle
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.IntOffset
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.lerp
import androidx.compose.ui.zIndex
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import app.hocket.R
import app.hocket.core.ArtworkSizes
import app.hocket.ui.LocalCoreClient
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
        val MINI_HEIGHT: Dp = 64.dp
        /** A release moves the sheet to the other anchor once it has travelled this share of the distance. */
        const val POSITIONAL_THRESHOLD = 0.3f
        /** Release speed that settles in the fling's direction regardless of position (Material's default). */
        val VELOCITY_THRESHOLD: Dp = 125.dp
    }
}

@Composable
fun rememberNowPlayingSheetState(): NowPlayingSheetState = remember { NowPlayingSheetState() }

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
 * Mini player bar that expands into the full player with a physics-based drag: velocity-aware settle,
 * a scrim, corners morphing from a pill to square, and the artwork scaling from the 48 dp thumbnail
 * to full width as the sheet opens. Inside: a pager for Now playing / Queue / Lyrics.
 */
@Composable
fun NowPlayingSheet(state: NowPlayingSheetState, bottomInset: Dp, onOpenAlbum: (String) -> Unit, onOpenArtist: (String) -> Unit) {
    val client = LocalCoreClient.current
    val nowPlaying by client.nowPlaying.collectAsStateWithLifecycle()
    val scope = rememberCoroutineScope()
    val entry = nowPlaying
    val visible = entry != null || state.isExpanded
    if (!visible) return
    BoxWithConstraints(Modifier.fillMaxSize().zIndex(10f)) {
        val density = LocalDensity.current
        val navPx = with(density) { bottomInset.toPx() }
        val maxHeightPx = constraints.maxHeight.toFloat()
        val maxWidthPx = constraints.maxWidth
        val miniHeight = miniPlayerHeight()
        val collapsedOffset = maxHeightPx - with(density) { miniHeight.toPx() } - navPx
        LaunchedEffect(collapsedOffset, density) {
            state.collapsedOffset = collapsedOffset
            state.velocityThreshold = with(density) { NowPlayingSheetState.VELOCITY_THRESHOLD.toPx() }
            state.draggable.updateAnchors(DraggableAnchors { SheetValue.Collapsed at collapsedOffset; SheetValue.Expanded at 0f }, state.draggable.targetValue)
        }
        val progress = state.progress
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

        // Scrim behind the sheet (a pointer affordance only: the collapse button and the sheet's
        // collapse/dismiss actions are the accessible way out).
        if (progress > 0f) {
            // clearAndSetSemantics first: semantics modifiers after it on the same node (the click)
            // would otherwise survive.
            Box(Modifier.fillMaxSize().clearAndSetSemantics { }.alpha(progress * 0.6f).background(Color.Black).let { if (progress > 0.5f) it.clickable(indication = null, interactionSource = remember { MutableInteractionSource() }) { scope.launch { state.collapse() } } else it })
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
        val corner = lerp(22.dp, 0.dp, progress)
        val sidePad = lerp(10.dp, 0.dp, progress)
        val seedState = LocalArtworkSeedState.current
        val dark = LocalDarkTheme.current
        // Sheet content is themed from the artwork while open (dynamic colour on the now-playing screen).
        val artworkSeed by artworkSeed(entry?.track?.coverArt)
        val sheetScheme = remember(artworkSeed, dark) { artworkSeed?.let { ArtworkColors.scheme(it, dark) } }
        val sheetTitle = stringResource(R.string.player_sheet_title)
        val collapseLabel = stringResource(R.string.player_collapse)
        val expandLabel = stringResource(R.string.player_expand)
        val expanded = state.isExpanded
        // The full player is hidden from accessibility until it is the visible part of the sheet,
        // and the mini bar is gone by then: exactly one of them is in the tree at any time.
        val fullPlayerAccessible = progress >= FULL_PLAYER_A11Y_PROGRESS
        Box(
            Modifier
                .fillMaxWidth()
                .offset { IntOffset(0, (state.draggable.offset.takeIf { !it.isNaN() } ?: collapsedOffset).roundToInt()) }
                .padding(horizontal = sidePad)
                .height(with(density) { maxHeightPx.toDp() })
                .clip(RoundedCornerShape(topStart = corner, topEnd = corner))
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
            val content: @Composable () -> Unit = {
                Surface(color = MaterialTheme.colorScheme.surfaceContainerHigh, modifier = Modifier.fillMaxSize()) {
                    Box(Modifier.fillMaxSize()) {
                        val pager = rememberPagerState { 3 }
                        // Full player, fading in as the sheet opens.
                        Column(Modifier.fillMaxSize().alpha(((progress - 0.35f) / 0.65f).coerceIn(0f, 1f)).then(if (fullPlayerAccessible) Modifier else Modifier.clearAndSetSemantics { }).statusBarsPadding()) {
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
                                    PrimaryScrollableTabRow(selectedTabIndex = pager.currentPage, modifier = Modifier.weight(1f), containerColor = Color.Transparent, edgePadding = 0.dp) { tabs() }
                                } else {
                                    PrimaryTabRow(selectedTabIndex = pager.currentPage, modifier = Modifier.weight(1f), containerColor = Color.Transparent) { tabs() }
                                    Box(Modifier.size(48.dp))
                                }
                            }
                            HorizontalPager(state = pager, modifier = Modifier.fillMaxSize(), beyondViewportPageCount = 0, userScrollEnabled = progress > 0.9f) { page ->
                                when (page) {
                                    0 -> NowPlayingPage(sheetProgress = progress, onOpenAlbum = onOpenAlbum, onOpenArtist = onOpenArtist, artworkSlot = true)
                                    1 -> QueuePanel(Modifier.fillMaxSize())
                                    2 -> LyricsPage(visible = state.isExpanded && progress > 0.9f, modifier = Modifier.fillMaxSize())
                                }
                            }
                        }
                        // Mini bar, fading out.
                        if (progress < FULL_PLAYER_A11Y_PROGRESS) {
                            Box(Modifier.fillMaxWidth().height(miniHeight).alpha(1f - (progress / 0.5f).coerceIn(0f, 1f))) {
                                MiniPlayerBar(onExpand = { scope.launch { state.expand() } })
                            }
                        }
                        // The one artwork that scales from thumbnail (mini bar) to hero (now-playing page).
                        val hero = remember(maxWidthPx) { with(density) { (maxWidthPx.toDp() - 48.dp).coerceAtMost(420.dp) } }
                        val heroLeft = with(density) { (maxWidthPx.toDp() - hero) / 2 }
                        val artSize = lerp(48.dp, hero, progress)
                        val artLeft = lerp(8.dp, heroLeft, progress)
                        val artTop = lerp(8.dp, 112.dp, progress)
                        if (pager.currentPage == 0 || progress < 0.95f) {
                            Box(Modifier.offset(x = artLeft, y = artTop).size(artSize)) {
                                HeroArtwork(entry?.track?.coverArt, entry?.track?.title, artSize, interactive = progress > 0.95f, describe = fullPlayerAccessible, onPreviewToggle = { seed ->
                                    seedState.seed = if (seedState.seed == null) seed else null
                                })
                            }
                        }
                    }
                }
            }
            if (sheetScheme != null && progress > 0.05f) MaterialExpressiveTheme(colorScheme = sheetScheme, content = content) else content()
        }
    }
}

/** Sheet progress from which the full player (not the mini bar) is what accessibility sees. */
private const val FULL_PLAYER_A11Y_PROGRESS = 0.6f

/** Resolves the artwork's seed colour: palette from the cached file, else the id-derived placeholder hue. */
@Composable
fun artworkSeed(coverArt: String?): androidx.compose.runtime.State<Color?> {
    val client = LocalCoreClient.current
    return androidx.compose.runtime.produceState<Color?>(initialValue = null, coverArt) {
        value = if (coverArt == null) null else (ArtworkColors.seedFrom(client.artworkPath(coverArt, ArtworkSizes.LIST)) ?: ArtworkColors.seedFor(coverArt))
    }
}
