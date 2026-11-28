package app.hocket.ui.stash

import androidx.activity.compose.PredictiveBackHandler
import androidx.compose.animation.core.animate
import androidx.compose.foundation.MutatePriority
import androidx.compose.foundation.gestures.AnchoredDraggableDefaults
import androidx.compose.foundation.gestures.AnchoredDraggableState
import androidx.compose.foundation.gestures.DraggableAnchors
import androidx.compose.foundation.gestures.Orientation
import androidx.compose.foundation.gestures.anchoredDraggable
import androidx.compose.foundation.gestures.animateTo
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.navigationBars
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.statusBars
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.lazy.LazyListState
import androidx.compose.runtime.derivedStateOf
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.snapshotFlow
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.input.nestedscroll.NestedScrollConnection
import androidx.compose.ui.input.nestedscroll.NestedScrollSource
import androidx.compose.ui.input.nestedscroll.nestedScroll
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.LayoutCoordinates
import androidx.compose.ui.layout.onGloballyPositioned
import androidx.compose.ui.layout.onSizeChanged
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.semantics.onClick
import androidx.compose.ui.semantics.paneTitle
import androidx.compose.ui.unit.IntOffset
import androidx.compose.ui.unit.Velocity
import app.hocket.ui.components.DrawerHandle
import app.hocket.ui.theme.Motion
import kotlin.math.abs
import kotlin.math.roundToInt
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.first
import androidx.compose.animation.AnimatedContent
import androidx.compose.animation.core.Spring
import androidx.compose.animation.core.animateDpAsState
import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.animation.core.spring
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.slideInVertically
import androidx.compose.animation.slideOutVertically
import androidx.compose.animation.togetherWith
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.material3.SnackbarHost
import androidx.compose.material3.SnackbarHostState
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.role
import app.hocket.ui.components.AppSnackbar
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.key
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.shadow
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.hapticfeedback.HapticFeedbackType
import androidx.compose.ui.platform.LocalHapticFeedback
import androidx.compose.ui.platform.LocalResources
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.CustomAccessibilityAction
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import app.hocket.R
import app.hocket.core.ActionIds
import app.hocket.core.ArtworkSizes
import app.hocket.core.Commands
import app.hocket.core.ref
import app.hocket.core.api.Command
import app.hocket.core.api.CommandStashMoveInner
import app.hocket.core.api.CommandStashRemoveInner
import app.hocket.core.api.StashItem
import app.hocket.ui.LocalCoreClient
import app.hocket.ui.a11y.LocalReducedMotion
import app.hocket.ui.components.Artwork
import app.hocket.ui.components.LocalAppSnackbar
import app.hocket.ui.icons.HocketIcons
import androidx.compose.material3.ListItemDefaults
import app.hocket.core.api.ActionDescriptor
import app.hocket.core.toSummary
import app.hocket.ui.DetailNavigator
import app.hocket.ui.LocalDetailNavigator
import app.hocket.ui.components.MenuSheet
import app.hocket.ui.components.SongMenuExtra
import app.hocket.ui.components.songSubject
import app.hocket.ui.components.TrackRow
import app.hocket.ui.components.formatClock
import app.hocket.ui.components.segmentedShape
import app.hocket.ui.components.songsSubject
import kotlinx.coroutines.flow.collectLatest
import kotlinx.coroutines.launch
import sh.calvin.reorderable.ReorderableItem
import sh.calvin.reorderable.rememberReorderableLazyListState

/**
 * The song stash (#150) on the phone: a chip with a fan of the first two stashed covers and the
 * count. With the floating navigation bar it sits beside the pill, as tall as it; otherwise it
 * floats above the mini player at the end edge ([StashChipDefaults.StandaloneHeight]). Tapping it
 * opens [StashSheet]. A new song landing in the stash ticks (haptic) and pops the count.
 */
@Composable
fun StashChip(
    items: List<StashItem>,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
    height: Dp = StashChipDefaults.StandaloneHeight,
    container: Color = MaterialTheme.colorScheme.secondaryContainer,
    content: Color = MaterialTheme.colorScheme.onSecondaryContainer,
    elevation: Dp = 6.dp,
) {
    val reduced = LocalReducedMotion.current
    val count = items.size
    val description = pluralStringResource(R.plurals.stash_chip_description, count, count)
    val art = (height - 16.dp).coerceAtMost(32.dp)
    Row(
        modifier
            .height(height)
            .shadow(elevation, CircleShape, ambientColor = Color.Black, spotColor = Color.Black)
            .clip(CircleShape)
            .background(container)
            .testTag("stash.chip")
            .clickable(role = Role.Button, onClick = onClick)
            .clearAndSetSemantics { contentDescription = description; role = Role.Button }
            .padding(start = (height - art) / 2, end = 14.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Fan(items, art, reduced)
        Spacer(Modifier.width(8.dp))
        AnimatedContent(
            targetState = count,
            transitionSpec = {
                if (reduced) fadeIn() togetherWith fadeOut()
                else {
                    val up = targetState > initialState
                    (slideInVertically(spring(dampingRatio = 0.6f, stiffness = 500f)) { if (up) it else -it } + fadeIn()) togetherWith
                        (slideOutVertically(spring(stiffness = Spring.StiffnessMedium)) { if (up) -it else it } + fadeOut())
                }
            },
            label = "stashCount",
        ) { n ->
            Text(n.toString(), color = content, style = MaterialTheme.typography.titleMedium, fontWeight = FontWeight.SemiBold, maxLines = 1, softWrap = false, modifier = Modifier.testTag("stash.count"))
        }
    }
}

/**
 * The two newest covers fanned out, the newest in front tilted towards the count over the one
 * before it (mirrored right to left). Each song stashed drops in as the new front card, pushing
 * the old front card back and the old back card out behind it.
 */
@Composable
private fun Fan(items: List<StashItem>, size: Dp, reduced: Boolean) {
    val two = items.size > 1
    val dir = if (androidx.compose.ui.platform.LocalLayoutDirection.current == androidx.compose.ui.unit.LayoutDirection.Rtl) -1f else 1f
    val spec = if (reduced) spring<Float>(stiffness = Spring.StiffnessHigh) else spring(dampingRatio = 0.55f, stiffness = 400f)
    val spread by animateFloatAsState(if (two) 1f else 0f, spec, label = "stashFan")
    val width by animateDpAsState(if (two) size + 10.dp else size, if (reduced) spring(stiffness = Spring.StiffnessHigh) else spring(dampingRatio = 0.8f, stiffness = 500f), label = "stashFanWidth")
    Box(Modifier.width(width).height(size)) {
        // The newest three, oldest first so the front draws on top, all from one keyed loop: a card
        // keeps its place in the composition (and its loaded art) as newer songs push it back, and
        // moves from its old pose to its new one. The third only shows while it fades out behind.
        val shown = items.takeLast(3)
        shown.forEachIndexed { i, item ->
            key(item.entryId) {
                FanCard(item, size, depth = shown.size - 1 - i, spread = { spread }, dir, spec, reduced)
            }
        }
    }
}

/** One card of the [Fan] at [depth] (0 the front, 1 the back, 2 leaving behind it). */
@Composable
private fun FanCard(item: StashItem, size: Dp, depth: Int, spread: () -> Float, dir: Float, spec: androidx.compose.animation.core.AnimationSpec<Float>, reduced: Boolean) {
    // Starts at its first depth, so a card arriving behind doesn't slide in from the front.
    val d by animateFloatAsState(depth.toFloat(), if (reduced) spec else spring(dampingRatio = 0.75f, stiffness = 400f), label = "stashCardDepth")
    // A card that arrives in front drops in.
    var dropped by remember { mutableStateOf(reduced || depth != 0) }
    LaunchedEffect(Unit) { dropped = true }
    val drop by animateFloatAsState(if (dropped) 1f else 0f, spec, label = "stashCardDrop")
    Card(
        item, size,
        Modifier.graphicsLayer {
            val s = spread()
            val tx: Float
            val rot: Float
            val scale: Float
            val fade: Float
            if (d <= 1f) {
                // Front (nudged towards the count, tilted to it) to back (tilted away).
                tx = lerp(10.dp.toPx() * s, 0f, d)
                rot = lerp(8f * s, -10f * s, d)
                scale = 1f
                fade = 1f
            } else {
                // Back to gone: tipping further away as it shrinks and fades.
                val t = d - 1f
                tx = lerp(0f, -6.dp.toPx(), t)
                rot = lerp(-10f * s, -18f, t)
                scale = lerp(1f, 0.85f, t)
                fade = (1f - t).coerceIn(0f, 1f)
            }
            translationX = tx * dir
            rotationZ = (rot - 20f * (1f - drop)) * dir
            translationY = -size.toPx() * 0.4f * (1f - drop)
            scaleX = scale * (0.6f + 0.4f * drop)
            scaleY = scale * (0.6f + 0.4f * drop)
            alpha = fade * drop.coerceIn(0f, 1f)
        },
    )
}

private fun lerp(a: Float, b: Float, t: Float) = a + (b - a) * t

@Composable
private fun Card(item: StashItem, size: Dp, modifier: Modifier) {
    Surface(modifier.size(size), shape = RoundedCornerShape(8.dp), shadowElevation = 2.dp, color = Color.Transparent) {
        Artwork(item.track.coverArt, ArtworkSizes.THUMB, null, Modifier.size(size), RoundedCornerShape(8.dp))
    }
}

object StashChipDefaults {
    /** The chip's height when it floats on its own (above the mini player). */
    val StandaloneHeight: Dp = 44.dp
    /** The room the standalone chip takes above the mini player: its height and the gap under it. */
    val StandaloneRoom: Dp = StandaloneHeight + 8.dp
    /** The width the chip takes beside the floating bar (for the bar's label budget). */
    val BesideBarWidth: Dp = 96.dp
}

/**
 * The stash as a sheet: its songs listed as a playlist's are (drag the handle to reorder, tap to
 * pick some), then the menu the selection drawer would open for them, without its header: for
 * several songs the songs' menu (the play group, the stars, Add to playlist, Download, Share,
 * Snooze); for one song (picked, or all the stash holds) that song's own menu (Preview, Go to
 * album, Song info, ...). With nothing picked the menu acts on the whole stash, otherwise on the
 * songs picked ("Remove 2 from stash" then too); either way the stash is kept. Play and Shuffle
 * play it as a queue named after the stash and close the sheet; going to an album or artist closes
 * it too. Clear empties it (the core's toast has an Undo).
 */
@Composable
fun StashSheet(onDismiss: () -> Unit, modifier: Modifier = Modifier, navigator: DetailNavigator? = LocalDetailNavigator.current) {
    val client = LocalCoreClient.current
    val items by client.stash.collectAsStateWithLifecycle()
    val haptics = LocalHapticFeedback.current
    val resources = LocalResources.current
    val scope = rememberCoroutineScope()
    val density = LocalDensity.current
    val state = remember { AnchoredDraggableState(StashAt.Hidden) }
    val spec = Motion.sheet
    // The drawer is over the shell's snackbars: they (and their Undo) show over it instead.
    val sheetSnackbars = remember { SnackbarHostState() }
    val snackbar = remember(scope) { AppSnackbar(sheetSnackbars, scope) }
    DisposableEffect(Unit) {
        app.hocket.ui.components.SnackbarRedirect.host = sheetSnackbars
        onDispose { if (app.hocket.ui.components.SnackbarRedirect.host === sheetSnackbars) app.hocket.ui.components.SnackbarRedirect.host = null }
    }
    // Closing: slides off the bottom edge (overriding a finger still on it), then gone.
    var closing by remember { mutableStateOf(false) }
    fun close() {
        if (closing) return
        closing = true
        scope.launch {
            if (state.anchors.hasPositionFor(StashAt.Hidden)) state.anchoredDrag(StashAt.Hidden, MutatePriority.PreventUserInput) { anchors, target ->
                val from = state.offset.takeUnless { it.isNaN() } ?: anchors.positionOf(target)
                animate(from, anchors.positionOf(target), animationSpec = spec) { v, velocity -> dragTo(v, velocity) }
            }
        }.invokeOnCompletion { onDismiss() }
    }
    val listState = rememberLazyListState()
    // To its rest at the buttons with the list back at its top, where they are.
    fun settleTo(value: StashAt) {
        scope.launch { state.animateTo(value, spec) }
        if (value == StashAt.Peek) scope.launch { listState.animateScrollToItem(0) }
    }
    // Each time it comes down to its rest (or away), the menu goes back to its first page: a
    // sub-page (Share, Snooze) has no play buttons to rest on, and would hold on to back.
    var menuResets by remember { mutableIntStateOf(0) }
    LaunchedEffect(state) { snapshotFlow { state.settledValue != StashAt.Open }.collect { if (it) menuResets++ } }
    // Swiped off the bottom edge: closed.
    LaunchedEffect(state) {
        var risen = false
        snapshotFlow { state.settledValue }.collect { v ->
            if (v != StashAt.Hidden) risen = true
            else if (risen && !closing) { closing = true; onDismiss() }
        }
    }
    val hasPeek by remember { derivedStateOf { state.anchors.hasPositionFor(StashAt.Peek) } }
    // Back: from open to the rest at the buttons (when it has one), else closed. (A sub-page of the
    // menu takes back first: its handler is added after this one.)
    PredictiveBackHandler { progress ->
        try {
            progress.collect { }
            if (state.targetValue == StashAt.Open && hasPeek) settleTo(StashAt.Peek) else close()
        } catch (e: kotlinx.coroutines.CancellationException) { }
    }
    var picked by remember { mutableStateOf(setOf<String>()) }
    // Emptied (here or on another device) after having songs: it slides away. (Restored before the
    // stash has arrived, it waits for it rather than closing at once.)
    var hadSongs by remember { mutableStateOf(false) }
    LaunchedEffect(items.isEmpty()) {
        if (items.isNotEmpty()) hadSongs = true
        else if (hadSongs) close()
    }
    val selection = items.filter { it.entryId in picked }
    val acting = selection.ifEmpty { items }
    val refs = acting.map { it.track.ref }
    val target = remember(refs) { Commands.tracks(refs) }
    val title = stringResource(R.string.stash_title)
    val dot = stringResource(R.string.dot_separator)
    // The registry's actions for what the menu acts on, again as that (or the registry) changes.
    var actions by remember { mutableStateOf<List<ActionDescriptor>>(emptyList()) }
    val currentTarget by rememberUpdatedState(target)
    LaunchedEffect(target) { if (refs.isNotEmpty()) actions = client.actions("contextMenu", target) }
    LaunchedEffect(Unit) {
        client.actionsChanged.collectLatest {
            val t = currentTarget
            if (t !is app.hocket.core.api.ActionTarget.Tracks || t.data.tracks.isEmpty()) return@collectLatest
            val fresh = client.actions("contextMenu", t)
            // Picked songs changed meanwhile: that set's own fetch has (or will have) the answer.
            if (t == currentTarget) actions = fresh
        }
    }
    // One song (picked, or all the stash holds): its own menu, as its three dots would open it (and a
    // one-item selection's drawer does). Several: the songs' menu.
    val pins by client.pins.collectAsStateWithLifecycle()
    val unknownArtist = stringResource(R.string.unknown_artist)
    val single = acting.singleOrNull()?.track?.toSummary()
    val subject = remember(acting, pins, resources, dot) {
        single?.let { songSubject(it, null, pins, dot, unknownArtist) } ?: songsSubject(acting.size, acting.map { it.track.toSummary() }, null, resources, dot)
    }
    // What its Song info opens, over the sheet.
    var songInfo by remember { mutableStateOf<app.hocket.core.api.TrackSummary?>(null) }
    fun playAction(a: ActionDescriptor) {
        when (a.id) {
            ActionIds.PLAY, ActionIds.PLAY_SHUFFLED -> { client.dispatch(Commands.playTracks(refs, 0, title, a.id == ActionIds.PLAY_SHUFFLED)); close() }
            else -> {
                client.dispatch(Commands.runAction(a.id, target))
                val plural = when (a.id) { ActionIds.PLAY_NEXT -> R.plurals.stash_queued_next; ActionIds.ADD_TO_QUEUE -> R.plurals.stash_queued; else -> R.plurals.stash_queued_later }
                val text = resources.getQuantityString(plural, refs.size, refs.size)
                snackbar.launch { sheetSnackbars.showSnackbar(text) }
            }
        }
    }
    // The menu's go-to rows (and Song info's links) close the stash first, so what they open is in view.
    val dismiss by rememberUpdatedState(onDismiss)
    val songNavigator = remember(navigator) {
        navigator?.let { n ->
            DetailNavigator(
                openAlbum = { id -> dismiss(); n.openAlbum(id) },
                openArtist = { id -> dismiss(); n.openArtist(id) },
                openPlaylist = n.openPlaylist?.let { open -> { id -> dismiss(); open(id) } },
                openSetting = n.openSetting?.let { open -> { route, tag -> dismiss(); open(route, tag) } },
            )
        }
    }
    val velocityThreshold = with(density) { 125.dp.toPx() }
    /** Settles a release of [velocity] (px/s, down positive): a flick goes on to the next anchor its way, else the nearest. */
    suspend fun settle(velocity: Float) {
        val at = state.offset
        if (at.isNaN() || state.isAnimationRunning || closing) return
        val anchors = StashAt.entries.filter { state.anchors.hasPositionFor(it) }.map { it to state.anchors.positionOf(it) }
        if (anchors.any { abs(it.second - at) < 0.5f }) return
        val target = when {
            velocity > velocityThreshold -> anchors.filter { it.second > at }.minByOrNull { it.second }
            velocity < -velocityThreshold -> anchors.filter { it.second < at }.maxByOrNull { it.second }
            else -> null
        } ?: anchors.minByOrNull { abs(it.second - at) } ?: return
        state.animateTo(target.first, spec)
    }
    fun openAt() = if (state.anchors.hasPositionFor(StashAt.Open)) state.anchors.positionOf(StashAt.Open) else Float.NaN
    // The list: dragging up opens the drawer before the list scrolls; dragging down past the list's
    // top brings the drawer down with it (to its rest at the buttons, or away).
    val followDrag = remember(state) {
        object : NestedScrollConnection {
            override fun onPreScroll(available: Offset, source: NestedScrollSource): Offset {
                val open = openAt()
                return if (source == NestedScrollSource.UserInput && available.y < 0f && !open.isNaN() && state.offset > open) Offset(0f, state.dispatchRawDelta(available.y)) else Offset.Zero
            }
            override fun onPostScroll(consumed: Offset, available: Offset, source: NestedScrollSource): Offset =
                if (source == NestedScrollSource.UserInput && available.y > 0f) Offset(0f, state.dispatchRawDelta(available.y)) else Offset.Zero
            override suspend fun onPreFling(available: Velocity): Velocity {
                val open = openAt()
                return if (available.y < 0f && !open.isNaN() && state.offset > open + 0.5f) { settle(available.y); available } else Velocity.Zero
            }
            override suspend fun onPostFling(consumed: Velocity, available: Velocity): Velocity { settle(available.y); return available }
        }
    }
    val fling = AnchoredDraggableDefaults.flingBehavior(state, positionalThreshold = { d -> d * 0.5f }, animationSpec = spec)
    val closeLabel = stringResource(R.string.action_close)
    // A screen reader gets no rest at the buttons: the menu below the screen's edge would take its
    // focus there, out of sight and out of the list's reach.
    val context = androidx.compose.ui.platform.LocalContext.current
    val a11y = remember(context) { context.getSystemService(android.view.accessibility.AccessibilityManager::class.java) }
    var touchExploration by remember(a11y) { mutableStateOf(a11y?.isTouchExplorationEnabled == true) }
    DisposableEffect(a11y) {
        val listener = android.view.accessibility.AccessibilityManager.TouchExplorationStateChangeListener { touchExploration = it }
        a11y?.addTouchExplorationStateChangeListener(listener)
        onDispose { a11y?.removeTouchExplorationStateChangeListener(listener) }
    }
    CompositionLocalProvider(LocalAppSnackbar provides snackbar, LocalDetailNavigator provides songNavigator) {
    BoxWithConstraints(modifier.fillMaxSize()) {
        val pageH = constraints.maxHeight
        val topRoom = WindowInsets.statusBars.getTop(density) + with(density) { 12.dp.roundToPx() }
        val navBottom = WindowInsets.navigationBars.getBottom(density)
        val margin = with(density) { 16.dp.roundToPx() }
        var sheetH by remember { mutableIntStateOf(0) }
        // How far down the drawer the play group ends, with the list at its top (px; -1 not yet
        // known, 0 none: the menu's actions never came).
        var playFoot by remember { mutableIntStateOf(-1) }
        val sheetAt = remember { arrayOfNulls<LayoutCoordinates>(1) }
        LaunchedEffect(Unit) { delay(1_000); if (playFoot < 0) playFoot = 0 }
        // Where it rests: open (as tall as it all is, up to under the status bar), and, when the songs
        // and buttons fit with more menu below, at the buttons: they sit at the screen's foot and the
        // rest of the menu is a drag (or the handle) away. A long stash just opens, at its top.
        LaunchedEffect(pageH, sheetH, playFoot, navBottom, touchExploration) {
            if (sheetH <= 0) return@LaunchedEffect
            val full = sheetH >= pageH - topRoom - 1
            if (playFoot < 0 && !full) return@LaunchedEffect
            val open = (pageH - sheetH).toFloat()
            val peek = (pageH - playFoot - margin - navBottom).toFloat()
            val withPeek = playFoot > 0 && peek - open > 1f && !touchExploration
            state.updateAnchors(DraggableAnchors {
                StashAt.Hidden at pageH.toFloat()
                if (withPeek) StashAt.Peek at peek
                StashAt.Open at open
            }, state.targetValue.takeIf { it != StashAt.Peek || withPeek } ?: StashAt.Open)
        }
        // In, once it knows where it rests.
        LaunchedEffect(state) {
            snapshotFlow { state.anchors.hasPositionFor(StashAt.Open) }.first { it }
            if (!closing) state.animateTo(if (state.anchors.hasPositionFor(StashAt.Peek)) StashAt.Peek else StashAt.Open, spec)
        }
        /** How far in the drawer is, 0 hidden to 1 at its first rest. */
        fun shown(): Float {
            val at = state.offset
            if (at.isNaN() || pageH <= 0) return 0f
            val rest = StashAt.entries.filter { it != StashAt.Hidden && state.anchors.hasPositionFor(it) }.maxOfOrNull { state.anchors.positionOf(it) } ?: return 0f
            if (pageH - rest < 1f) return 1f
            return ((pageH - at) / (pageH - rest)).coerceIn(0f, 1f)
        }
        // The scrim: tapping it closes the stash.
        Box(
            Modifier
                .fillMaxSize()
                .graphicsLayer { alpha = shown() }
                .background(MaterialTheme.colorScheme.scrim.copy(alpha = 0.32f))
                .pointerInput(Unit) { detectTapGestures { close() } }
                .semantics { contentDescription = closeLabel; onClick { close(); true } }
                .testTag("stash.scrim"),
        )
        Surface(
            modifier = Modifier
                .align(Alignment.TopCenter)
                .widthIn(max = 640.dp)
                .fillMaxWidth()
                .heightIn(max = with(density) { (pageH - topRoom).coerceAtLeast(0).toDp() })
                // Until it is laid out and anchored, below the page.
                .offset { IntOffset(0, state.offset.takeUnless { it.isNaN() }?.roundToInt() ?: pageH) }
                .onSizeChanged { sheetH = it.height }
                .onGloballyPositioned { sheetAt[0] = it }
                .nestedScroll(followDrag)
                .anchoredDraggable(state, Orientation.Vertical, enabled = !closing, flingBehavior = fling)
                .semantics { paneTitle = title }
                .testTag("stash.sheet"),
            shape = RoundedCornerShape(topStart = 28.dp, topEnd = 28.dp),
            color = MaterialTheme.colorScheme.surfaceContainerLow,
            shadowElevation = 8.dp,
        ) {
        Column {
            val isOpen = state.targetValue == StashAt.Open
            DrawerHandle(isOpen, hasPeek, tag = "stash.sheetHandle") { settleTo(if (isOpen) StashAt.Peek else StashAt.Open) }
            StashList(
                state = listState,
                bottomPadding = 16.dp + with(density) { navBottom.toDp() },
                header = {
                Row(Modifier.fillMaxWidth().padding(start = 24.dp, end = 12.dp, bottom = 8.dp), verticalAlignment = Alignment.CenterVertically) {
                    Column(Modifier.weight(1f)) {
                        Text(title, style = MaterialTheme.typography.headlineSmall, fontWeight = FontWeight.SemiBold, modifier = Modifier.semantics { heading() })
                        Text(
                            if (selection.isNotEmpty()) pluralStringResource(R.plurals.stash_selected_of, items.size, selection.size, items.size)
                            else pluralStringResource(R.plurals.stash_count, items.size, items.size),
                            style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant,
                            modifier = Modifier.testTag("stash.summary"),
                        )
                    }
                    if (selection.isNotEmpty()) TextButton(onClick = { picked = emptySet() }, modifier = Modifier.testTag("stash.deselect")) { Text(stringResource(R.string.stash_deselect)) }
                    else TextButton(onClick = { client.dispatch(Command.StashClear) }, modifier = Modifier.testTag("stash.clear")) {
                        Icon(HocketIcons.Filled.ClearAll, null, Modifier.size(18.dp))
                        Spacer(Modifier.width(6.dp))
                        Text(stringResource(R.string.stash_clear))
                    }
                }
                },
                items = items,
                picked = picked,
                onToggle = { id -> picked = if (id in picked) picked - id else picked + id; haptics.performHapticFeedback(HapticFeedbackType.SegmentTick) },
                onMove = { id, to -> client.dispatch(Command.StashMove(CommandStashMoveInner(id, to.toUInt()))) },
            ) {
                // What the menu acts on, so it's never a surprise: everything, or the songs picked.
                Text(
                    if (selection.isNotEmpty()) pluralStringResource(R.plurals.stash_acting_selected, selection.size, selection.size)
                    else pluralStringResource(R.plurals.stash_acting_all, items.size, items.size),
                    style = MaterialTheme.typography.titleSmall, color = MaterialTheme.colorScheme.onSurfaceVariant,
                    modifier = Modifier.fillMaxWidth().padding(start = 24.dp, end = 24.dp, top = 20.dp, bottom = 10.dp).semantics { heading() }.testTag("stash.acting"),
                )
                val removePicked = if (selection.isNotEmpty()) SongMenuExtra(
                    if (selection.size == 1) stringResource(R.string.stash_remove_one) else pluralStringResource(R.plurals.stash_remove_n, selection.size, selection.size), HocketIcons.Filled.RemoveCircleOutline,
                    onClick = { client.dispatch(Command.StashRemove(CommandStashRemoveInner(selection.map { it.entryId }))) },
                    destructive = true, testTag = "stash.menu.remove", replaces = ActionIds.STASH,
                ) else null
                // A fresh menu for each set of songs it acts on (not per change in what it shows:
                // one song's own Download toggle changes the pins, which mustn't reset the menu or
                // stop a preview). Add to stash makes no sense here: an extra stands in for it (or,
                // for the whole stash, Clear does).
                // Scrolled out of the list (it grew, say, on another device), the buttons are no
                // longer where it rests: no rest at them until they're measured again.
                DisposableEffect(Unit) { onDispose { playFoot = 0 } }
                key(acting.map { it.entryId }) {
                    MenuSheet(
                        subject, target, actions.filter { it.id != ActionIds.STASH },
                        onDismiss = {},
                        extras = listOfNotNull(removePicked),
                        onGoToAlbum = single?.albumId?.let { id -> songNavigator?.let { n -> { n.openAlbum(id) } } },
                        onGoToArtist = single?.artistId?.let { id -> songNavigator?.let { n -> { n.openArtist(id) } } },
                        onInfo = single?.let { t -> { songInfo = t } },
                        onRan = { picked = emptySet() },
                        tag = "stash.menu",
                        embedded = true,
                        showHeader = false,
                        onPlay = ::playAction,
                        resetKey = menuResets,
                        playGroupModifier = Modifier.onGloballyPositioned { c ->
                            val sheet = sheetAt[0]
                            if (sheet != null && sheet.isAttached && c.isAttached && listState.firstVisibleItemIndex == 0 && listState.firstVisibleItemScrollOffset == 0) {
                                playFoot = (sheet.localPositionOf(c, Offset.Zero).y + c.size.height).roundToInt()
                            }
                        },
                    )
                }
            }
        }
        }
        // Its snackbars ride just above the drawer, so they never cover the buttons at its foot
        // (under the status bar when it is open all the way).
        var snackH by remember { mutableIntStateOf(0) }
        SnackbarHost(
            sheetSnackbars,
            Modifier
                .align(Alignment.TopCenter)
                .widthIn(max = 640.dp)
                .offset { IntOffset(0, ((state.offset.takeUnless { it.isNaN() } ?: pageH.toFloat()).roundToInt() - snackH).coerceAtLeast(topRoom - margin)) }
                .onSizeChanged { snackH = it.height }
                .padding(8.dp),
        )
    }
    }
        songInfo?.let { t ->
            ModalBottomSheet(onDismissRequest = { songInfo = null }, sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = true), modifier = Modifier.testTag("stash.menu.infoSheet")) {
                app.hocket.ui.player.PlayerAbout(
                    Modifier.fillMaxWidth().navigationBarsPadding(), track = t, title = stringResource(R.string.menu_song_info), onClose = { songInfo = null },
                    onOpenAlbum = songNavigator?.let { n -> { id -> songInfo = null; n.openAlbum(id) } },
                    onOpenArtist = songNavigator?.let { n -> { id -> songInfo = null; n.openArtist(id) } },
                )
            }
        }
}

@Composable
private fun StashList(
    state: LazyListState,
    bottomPadding: Dp,
    items: List<StashItem>,
    picked: Set<String>,
    onToggle: (String) -> Unit,
    onMove: (String, Int) -> Unit,
    /** Above the songs; it scrolls with them (the sheet keeps no fixed header). */
    header: @Composable () -> Unit,
    /** The menu after the songs. */
    footer: @Composable () -> Unit,
) {
    val haptics = LocalHapticFeedback.current
    val dot = stringResource(R.string.dot_separator)
    // The list as shown: moves land here at once and reach the core when the drag ends.
    var shown by remember { mutableStateOf(items) }
    var dragging by remember { mutableStateOf<String?>(null) }
    LaunchedEffect(items, dragging) { if (dragging == null) shown = items }
    val current by rememberUpdatedState(items)
    val listState = state
    val reorder = rememberReorderableLazyListState(listState) { from, to ->
        // Only song rows move (the menu after them is no row of the list).
        val a = shown.indexOfFirst { it.entryId == from.key }
        val b = shown.indexOfFirst { it.entryId == to.key }
        if (a < 0 || b < 0) return@rememberReorderableLazyListState
        shown = shown.toMutableList().apply { add(b, removeAt(a)) }
        haptics.performHapticFeedback(HapticFeedbackType.SegmentFrequentTick)
    }
    val moveUp = stringResource(R.string.stash_move_up)
    val moveDown = stringResource(R.string.stash_move_down)
    LazyColumn(
        state = listState,
        modifier = Modifier.fillMaxWidth().heightIn(min = 120.dp).testTag("stash.list"),
        contentPadding = PaddingValues(bottom = bottomPadding),
    ) {
        item(key = "stash.header") { header() }
        itemsIndexed(shown, key = { _, it -> it.entryId }) { index, item ->
            ReorderableItem(reorder, key = item.entryId) { isDragging ->
                val track = remember(item.track) { item.track.toSummary() }
                val lift by animateDpAsState(if (isDragging) 8.dp else 0.dp, label = "stashRowLift")
                val moves = buildList {
                    if (index > 0) add(CustomAccessibilityAction(moveUp) { onMove(item.entryId, index - 1); true })
                    if (index < shown.lastIndex) add(CustomAccessibilityAction(moveDown) { onMove(item.entryId, index + 1); true })
                }
                TrackRow(
                    track,
                    onClick = { onToggle(item.entryId) },
                    // Tapping picks a song: the stash is always choosing.
                    selected = item.entryId in picked,
                    selectionActive = true,
                    onToggleSelect = { onToggle(item.entryId) },
                    extraActions = moves,
                    segment = index to shown.size,
                    subtitle = listOfNotNull(track.artist, formatClock(track.durationMs.toLong())).joinToString(dot),
                    showDuration = false,
                    offlineGate = false,
                    modifier = Modifier
                        .padding(start = 16.dp, end = 16.dp, top = if (index > 0) ListItemDefaults.SegmentedGap else 0.dp)
                        .shadow(lift, segmentedShape(index, shown.size))
                        .testTag("stash.row"),
                    trailing = {
                        Box(
                            Modifier
                                .size(40.dp)
                                .draggableHandle(
                                    onDragStarted = { dragging = item.entryId; haptics.performHapticFeedback(HapticFeedbackType.GestureThresholdActivate) },
                                    onDragStopped = {
                                        val id = dragging
                                        dragging = null
                                        // Where it landed, as an index among the others in the stash as it is now
                                        // (another device may have changed it meanwhile): after the row it was
                                        // dropped under.
                                        val at = shown.indexOfFirst { it.entryId == id }
                                        if (id != null && at >= 0) {
                                            val others = current.filter { it.entryId != id }
                                            val prev = shown.getOrNull(at - 1)?.entryId
                                            val to = if (prev == null) 0 else others.indexOfFirst { it.entryId == prev } + 1
                                            if (to >= 0 && current.indexOfFirst { it.entryId == id } != to) onMove(id, to)
                                        }
                                        haptics.performHapticFeedback(HapticFeedbackType.GestureEnd)
                                    },
                                )
                                .testTag("stash.handle"),
                            contentAlignment = Alignment.Center,
                        ) {
                            Icon(HocketIcons.Filled.DragHandle, null, tint = MaterialTheme.colorScheme.onSurfaceVariant)
                        }
                    },
                )
            }
        }
        item(key = "stash.menu") { footer() }
    }
}

/** Where the stash drawer rests: below the screen's edge, with the play buttons at its foot, or open. */
private enum class StashAt { Hidden, Peek, Open }
