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
import androidx.compose.ui.semantics.contentDescription
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
    suspend fun expand() = draggable.animateTo(SheetValue.Expanded, Motion.sheet)
    suspend fun collapse() = draggable.animateTo(SheetValue.Collapsed, Motion.sheet)

    companion object {
        val MINI_HEIGHT: Dp = 64.dp
    }
}

@Composable
fun rememberNowPlayingSheetState(): NowPlayingSheetState = remember { NowPlayingSheetState() }

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
        val collapsedOffset = maxHeightPx - with(density) { NowPlayingSheetState.MINI_HEIGHT.toPx() } - navPx
        LaunchedEffect(collapsedOffset) {
            state.collapsedOffset = collapsedOffset
            state.draggable.updateAnchors(DraggableAnchors { SheetValue.Collapsed at collapsedOffset; SheetValue.Expanded at 0f }, state.draggable.targetValue)
        }
        val progress = state.progress
        val fling = AnchoredDraggableDefaults.flingBehavior(state.draggable, positionalThreshold = { d -> d * 0.3f }, animationSpec = Motion.sheet)

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

        // Scrim behind the sheet.
        if (progress > 0f) {
            Box(Modifier.fillMaxSize().alpha(progress * 0.6f).background(Color.Black).let { if (progress > 0.5f) it.clickable(indication = null, interactionSource = remember { MutableInteractionSource() }) { scope.launch { state.collapse() } } else it })
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
                    return if (available.y < 0 && state.progress < 1f) { state.draggable.settle(available.y); available } else Velocity.Zero
                }
                override suspend fun onPostFling(consumed: Velocity, available: Velocity): Velocity {
                    state.draggable.settle(available.y)
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
        val expandedDesc = stringResource(R.string.player_expand)
        Box(
            Modifier
                .fillMaxWidth()
                .offset { IntOffset(0, (state.draggable.offset.takeIf { !it.isNaN() } ?: collapsedOffset).roundToInt()) }
                .padding(horizontal = sidePad)
                .height(with(density) { maxHeightPx.toDp() })
                .clip(RoundedCornerShape(topStart = corner, topEnd = corner))
                .nestedScroll(nested)
                .anchoredDraggable(state.draggable, Orientation.Vertical, flingBehavior = fling)
                .semantics { contentDescription = expandedDesc }
                .testTag("nowPlaying.sheet"),
        ) {
            val content: @Composable () -> Unit = {
                Surface(color = MaterialTheme.colorScheme.surfaceContainerHigh, modifier = Modifier.fillMaxSize()) {
                    Box(Modifier.fillMaxSize()) {
                        val pager = rememberPagerState { 3 }
                        // Full player, fading in as the sheet opens.
                        Column(Modifier.fillMaxSize().alpha(((progress - 0.35f) / 0.65f).coerceIn(0f, 1f)).statusBarsPadding()) {
                            Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
                                IconButton(onClick = { scope.launch { state.collapse() } }) { Icon(Icons.Filled.KeyboardArrowDown, stringResource(R.string.player_collapse)) }
                                PrimaryTabRow(selectedTabIndex = pager.currentPage, modifier = Modifier.weight(1f), containerColor = Color.Transparent) {
                                    listOf(R.string.player_tab_now_playing, R.string.player_tab_queue, R.string.player_tab_lyrics).forEachIndexed { i, res ->
                                        Tab(selected = pager.currentPage == i, onClick = { scope.launch { pager.animateScrollToPage(i) } }, text = { Text(stringResource(res)) })
                                    }
                                }
                                Box(Modifier.size(48.dp))
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
                        if (progress < 0.6f) {
                            Box(Modifier.fillMaxWidth().height(NowPlayingSheetState.MINI_HEIGHT).alpha(1f - (progress / 0.5f).coerceIn(0f, 1f))) {
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
                                HeroArtwork(entry?.track?.coverArt, entry?.track?.title, artSize, interactive = progress > 0.95f, onPreviewToggle = { seed ->
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

/** Resolves the artwork's seed colour: palette from the cached file, else the id-derived placeholder hue. */
@Composable
fun artworkSeed(coverArt: String?): androidx.compose.runtime.State<Color?> {
    val client = LocalCoreClient.current
    return androidx.compose.runtime.produceState<Color?>(initialValue = null, coverArt) {
        value = if (coverArt == null) null else (ArtworkColors.seedFrom(client.artworkPath(coverArt, ArtworkSizes.LIST)) ?: ArtworkColors.seedFor(coverArt))
    }
}
