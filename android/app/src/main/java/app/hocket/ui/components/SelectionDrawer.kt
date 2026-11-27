package app.hocket.ui.components

import androidx.activity.compose.PredictiveBackHandler
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.gestures.AnchoredDraggableDefaults
import androidx.compose.foundation.gestures.AnchoredDraggableState
import androidx.compose.foundation.gestures.DraggableAnchors
import androidx.compose.foundation.gestures.Orientation
import androidx.compose.foundation.gestures.anchoredDraggable
import androidx.compose.foundation.gestures.animateTo
import androidx.compose.animation.core.animate
import androidx.compose.foundation.MutatePriority
import androidx.compose.foundation.focusGroup
import androidx.compose.ui.focus.focusProperties
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.IntrinsicSize
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.navigationBars
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBars
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.layout.windowInsetsBottomHeight
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.Immutable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.derivedStateOf
import androidx.compose.runtime.getValue
import androidx.compose.runtime.key
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.produceState
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.setValue
import androidx.compose.runtime.snapshotFlow
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.draw.clip
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Shape
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.input.nestedscroll.NestedScrollConnection
import androidx.compose.ui.input.nestedscroll.NestedScrollSource
import androidx.compose.ui.input.nestedscroll.nestedScroll
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.onSizeChanged
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.collapse
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.expand
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.IntOffset
import androidx.compose.ui.unit.Velocity
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import app.hocket.R
import app.hocket.core.ActionIds
import app.hocket.core.Commands
import app.hocket.core.Queries
import app.hocket.core.api.ActionDescriptor
import app.hocket.core.api.ActionTarget
import app.hocket.core.api.Album
import app.hocket.core.api.OfflineState
import app.hocket.core.api.Pin
import app.hocket.core.api.QueryResult
import app.hocket.core.api.TrackSummary
import app.hocket.core.client.CoreClient
import app.hocket.core.client.Selection
import app.hocket.core.client.SelectionKind
import app.hocket.core.toSummary
import app.hocket.ui.DetailNavigator
import app.hocket.ui.LocalCoreClient
import app.hocket.ui.LocalDetailNavigator
import app.hocket.ui.icons.HocketIcons
import app.hocket.ui.theme.Motion
import kotlinx.coroutines.flow.collectLatest
import kotlinx.coroutines.launch
import kotlin.math.abs
import kotlin.math.roundToInt

/** Where the selection drawer rests: below the screen's edge, peeking, or open. */
enum class DrawerValue { Hidden, Peek, Open }

/**
 * A selection's actions in a drawer over the bottom of the page. While anything is selected it
 * peeks: its handle, "3 selected" between Clear and Select all, and the quick actions in one
 * connected row (the play group: Play, Shuffle, Play next, Add to queue, Play later; failing those
 * the first four actions). Swiping it up (or tapping the handle) opens the rest over a scrim: for
 * one item (named by id: not an inverted select-all, whose items aren't listed) the very menu its three dots open, [MenuSheet] embedded (its
 * header, go-to rows and info); for several songs (or queue entries) their menu ("3 songs · 11 min",
 * love, the rating starting "Mixed" while they differ, "Download 3 songs", Share, Snooze, "Remove 3
 * from Late night"); for several albums, artists or playlists the rest of their actions, the rateN
 * entries folded into one "Set rating…" whose dialog rates every selected item. Swiping down, the scrim or back brings it
 * back to the peek; swiping the peek away, Clear or back from the peek ends the selection. An action
 * that runs ends it too. [context] names the playlist the songs are listed in;
 * [onRemoveFromContext] (an editable playlist) removes the selected entries from it. [kinds]: two
 * panes side by side each have a drawer, each showing only what its own pane selects; left out, a
 * page's drawer shows every selection but queue entries' (the queue has its own).
 *
 * Lay it over the whole page ([Modifier.matchParentSize]): it rises from the page's bottom edge, over
 * the system navigation bar (the shell moves its own bar and the mini player aside meanwhile).
 * [onPeekHeight] hears how tall the peek is (px, from the page's bottom edge), so a host can keep its
 * content clear of it.
 */
@Composable
fun SelectionDrawer(modifier: Modifier = Modifier, onSelectAll: (() -> Unit)? = null, context: String? = null, onRemoveFromContext: (() -> Unit)? = null, kinds: Set<SelectionKind>? = null, onPeekHeight: ((Int) -> Unit)? = null) {
    val client = LocalCoreClient.current
    val selectionAll by client.selection.collectAsStateWithLifecycleCompat()
    val kind by client.selectionKind.collectAsStateWithLifecycleCompat()
    val selection = if (if (kinds == null) kind != SelectionKind.QueueItems else kind in kinds) selectionAll else Selection()
    // Composed from the selection's start until the drawer has slid away after its end, showing the
    // last selection meanwhile.
    var leaving by remember { mutableStateOf(false) }
    val held = remember { arrayOf(selection) }
    if (selection.active) held[0] = selection
    LaunchedEffect(selection.active) { if (selection.active) leaving = true }
    if (selection.active || leaving) {
        Drawer(selection.active, held[0], kind, onSelectAll, context, onRemoveFromContext, modifier, onPeekHeight) { leaving = false }
    }
}

@Composable
private fun Drawer(
    active: Boolean,
    selection: Selection,
    kind: SelectionKind,
    onSelectAll: (() -> Unit)?,
    context: String?,
    onRemoveFromContext: (() -> Unit)?,
    modifier: Modifier,
    onPeekHeight: ((Int) -> Unit)?,
    onHidden: () -> Unit,
) {
    val client = LocalCoreClient.current
    val scope = rememberCoroutineScope()
    val density = LocalDensity.current
    val resources = androidx.compose.ui.platform.LocalResources.current
    val dot = stringResource(R.string.dot_separator)
    val songs = kind == SelectionKind.Tracks || kind == SelectionKind.QueueItems
    var actions by remember { mutableStateOf<List<ActionDescriptor>>(emptyList()) }
    var playlistPicker by remember { mutableStateOf(false) }
    var rating by remember { mutableStateOf(false) }
    LaunchedEffect(selection, kind) {
        if (active) actions = client.actions("contextMenu", client.selectionTarget())
    }
    LaunchedEffect(Unit) { client.actionsChanged.collectLatest { if (client.selection.value.active) actions = client.actions("contextMenu", client.selectionTarget()) } }
    val navigator = LocalDetailNavigator.current
    val pins by client.pins.collectAsStateWithLifecycleCompat()
    val unknownArtist = stringResource(R.string.unknown_artist)
    val selectedCount = stringResource(R.string.selected_count, selection.count)
    // The open drawer's menu: one item's own, or several songs'; worked out again as the selection changes.
    val menu by produceState<DrawerMenu?>(null, selection, kind, pins) {
        if (!active) return@produceState
        value = singleMenu(client, selection, kind, context, pins, resources, dot, unknownArtist, selectedCount)
            ?: if (songs) DrawerMenu(selectionSubject(client, selection, kind, context, resources, dot)) else null
    }
    // What one item's Song info / Album info opened (over the drawer, which keeps the selection).
    var songInfo by remember { mutableStateOf<TrackSummary?>(null) }
    var albumInfo by remember { mutableStateOf<Album?>(null) }
    /** Navigates away from the selection, which ends it. */
    fun leaveTo(open: (DetailNavigator) -> Unit): (() -> Unit)? = navigator?.let { n -> { client.clearSelection(); open(n) } }

    // Bulk actions: navigation and per-item ui-handled entries make no sense for a selection.
    val toolbarActions = actions.filter { it.id !in ActionIds.UI_HANDLED || it.id == ActionIds.ADD_TO_PLAYLIST }.filter { !it.id.matches(Regex("rate[0-4]")) }
    val playIds = PlayActionIds
    val quick = actions.filter { it.id in playIds }.sortedBy { playIds.indexOf(it.id) }.ifEmpty { toolbarActions.take(4) }
    val rest = toolbarActions.filter { a -> quick.none { it.id == a.id } }
    val setRating = stringResource(R.string.action_set_rating)
    fun label(a: ActionDescriptor) = if (a.id == ActionIds.rate(5)) setRating else a.label
    fun icon(a: ActionDescriptor) = actionIcon(if (a.id == ActionIds.rate(5)) "star_rate" else a.icon)
    fun run(a: ActionDescriptor) {
        when (a.id) {
            ActionIds.ADD_TO_PLAYLIST -> playlistPicker = true
            ActionIds.rate(5) -> rating = true
            else -> { client.dispatch(Commands.runAction(a.id, client.selectionTarget())); client.clearSelection() }
        }
    }

    val state = remember { AnchoredDraggableState(DrawerValue.Hidden) }
    val spec = Motion.sheet
    fun settleTo(value: DrawerValue) { scope.launch { state.animateTo(value, spec) } }
    /** Where the open drawer's top sits, and the peek's (px from the page's top; NaN until laid out). */
    fun openAt() = if (state.anchors.hasPositionFor(DrawerValue.Open)) state.anchors.positionOf(DrawerValue.Open) else Float.NaN
    fun peekAt() = if (state.anchors.hasPositionFor(DrawerValue.Peek)) state.anchors.positionOf(DrawerValue.Peek) else Float.NaN
    /** How far open the drawer is, 0 at the peek (or below it) to 1 open. */
    fun openness(): Float {
        val open = openAt(); val peek = peekAt(); val at = state.offset
        if (open.isNaN() || peek.isNaN() || at.isNaN() || peek - open < 1f) return 0f
        return ((peek - at) / (peek - open)).coerceIn(0f, 1f)
    }
    val opened by remember { derivedStateOf { openness() > 0f } }
    // Each time the drawer comes back down to its peek (or below), the songs' menu goes back to its
    // first page: a sub-page left open (Share, Snooze) would otherwise hold on to back.
    var menuResets by remember { mutableIntStateOf(0) }
    LaunchedEffect(state) { snapshotFlow { state.settledValue != DrawerValue.Open }.collect { if (it) menuResets++ } }
    val canOpen by remember { derivedStateOf { state.anchors.hasPositionFor(DrawerValue.Open) } }
    val isOpen by remember { derivedStateOf { state.targetValue == DrawerValue.Open } }

    // Swiped off the bottom edge while the selection lasts: that ends it.
    val activeNow by rememberUpdatedState(active)
    LaunchedEffect(state) {
        var risen = false
        snapshotFlow { state.settledValue }.collect { v ->
            if (v != DrawerValue.Hidden) risen = true
            else if (risen && activeNow) client.clearSelection()
        }
    }
    // Back closes the open drawer to its peek, and from the peek ends the selection. (A sub-page of
    // the songs' menu takes back first: its handler is added after this one.)
    PredictiveBackHandler(enabled = active) { progress ->
        try {
            progress.collect { }
            if (state.targetValue == DrawerValue.Open) settleTo(DrawerValue.Peek) else client.clearSelection()
        } catch (e: kotlinx.coroutines.CancellationException) { }
    }

    val velocityThreshold = with(density) { 125.dp.toPx() }
    /** Settles a release of [velocity] (px/s, down positive): a flick goes on to the next anchor its way, else the nearest. */
    suspend fun settle(velocity: Float) {
        val at = state.offset
        if (at.isNaN() || state.isAnimationRunning) return
        val anchors = DrawerValue.entries.filter { state.anchors.hasPositionFor(it) }.map { it to state.anchors.positionOf(it) }
        if (anchors.any { abs(it.second - at) < 0.5f }) return
        val target = when {
            velocity > velocityThreshold -> anchors.filter { it.second > at }.minByOrNull { it.second }
            velocity < -velocityThreshold -> anchors.filter { it.second < at }.maxByOrNull { it.second }
            else -> null
        } ?: anchors.minByOrNull { abs(it.second - at) } ?: return
        state.animateTo(target.first, spec)
    }
    // The open drawer's list: dragging up opens the drawer before the list scrolls; dragging down
    // past the list's top brings the drawer down with it.
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

    BoxWithConstraints(modifier) {
        val pageH = constraints.maxHeight
        val topRoom = WindowInsets.statusBars.getTop(density) + with(density) { 12.dp.roundToPx() }
        val navBottom = WindowInsets.navigationBars.getBottom(density)
        var sheetH by remember { mutableIntStateOf(0) }
        var peekPartH by remember { mutableIntStateOf(0) }
        val ready = sheetH > 0 && peekPartH > 0
        LaunchedEffect(peekPartH, navBottom) { if (peekPartH > 0) onPeekHeight?.invoke(peekPartH + navBottom) }
        LaunchedEffect(pageH, sheetH, peekPartH, navBottom) {
            if (sheetH <= 0 || peekPartH <= 0) return@LaunchedEffect
            val peek = (pageH - peekPartH - navBottom).toFloat()
            val open = (pageH - sheetH).toFloat()
            state.updateAnchors(DraggableAnchors {
                DrawerValue.Hidden at pageH.toFloat()
                DrawerValue.Peek at peek
                if (peek - open > 1f) DrawerValue.Open at open
            }, state.targetValue.takeIf { it != DrawerValue.Open || peek - open > 1f } ?: DrawerValue.Peek)
        }
        // In as the selection starts (or starts again while it was leaving), out once it ends (then
        // gone). Leaving overrides a finger still on the drawer, so it always gets there.
        LaunchedEffect(active, ready) {
            if (!ready) { if (!active) onHidden(); return@LaunchedEffect }
            if (active) { if (state.targetValue != DrawerValue.Open) state.animateTo(DrawerValue.Peek, spec) }
            else {
                state.anchoredDrag(DrawerValue.Hidden, MutatePriority.PreventUserInput) { anchors, target ->
                    val from = state.offset.takeUnless { it.isNaN() } ?: anchors.positionOf(target)
                    animate(from, anchors.positionOf(target), animationSpec = spec) { v, velocity -> dragTo(v, velocity) }
                }
                onHidden()
            }
        }

        // The scrim behind the open drawer: tapping it brings the drawer back to its peek.
        if (opened) {
            Box(
                Modifier
                    .fillMaxSize()
                    .graphicsLayer { alpha = openness() }
                    .background(MaterialTheme.colorScheme.scrim.copy(alpha = 0.32f))
                    .pointerInput(Unit) { detectTapGestures { settleTo(DrawerValue.Peek) } }
                    .testTag("selection.scrim"),
            )
        }
        val count = stringResource(R.string.selected_count, selection.count)
        Surface(
            modifier = Modifier
                .align(Alignment.TopCenter)
                .widthIn(max = 640.dp)
                .fillMaxWidth()
                .heightIn(max = with(density) { (pageH - topRoom).coerceAtLeast(0).toDp() })
                // Until it is laid out and anchored, below the page.
                .offset { IntOffset(0, state.offset.takeUnless { it.isNaN() }?.roundToInt() ?: pageH) }
                .onSizeChanged { sheetH = it.height }
                .nestedScroll(followDrag)
                .anchoredDraggable(state, Orientation.Vertical, enabled = active, flingBehavior = fling)
                .testTag("selection.drawer"),
            shape = RoundedCornerShape(topStart = 28.dp, topEnd = 28.dp),
            color = MaterialTheme.colorScheme.surfaceContainerLow,
            shadowElevation = 8.dp,
        ) {
            Column {
                // The peek: what shows while the drawer rests at the bottom.
                Column(Modifier.fillMaxWidth().onSizeChanged { peekPartH = it.height }.testTag("selection.peek")) {
                    DrawerHandle(isOpen, canOpen) { settleTo(if (isOpen) DrawerValue.Peek else DrawerValue.Open) }
                    Row(
                        Modifier.fillMaxWidth().padding(start = 8.dp, end = 8.dp).semantics { contentDescription = count }.testTag("selection.toolbar"),
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        IconButton(onClick = { client.clearSelection() }) { Icon(HocketIcons.Filled.Close, stringResource(R.string.action_clear_selection)) }
                        Text(count, style = MaterialTheme.typography.titleMedium, fontWeight = FontWeight.SemiBold, modifier = Modifier.weight(1f).padding(start = 4.dp).testTag("selection.count"))
                        if (onSelectAll != null) IconButton(onClick = onSelectAll) { Icon(HocketIcons.Filled.SelectAll, stringResource(R.string.action_select_all)) }
                    }
                    // Always as tall, so the peek doesn't jump as the actions arrive.
                    Row(
                        Modifier.fillMaxWidth().padding(start = 16.dp, end = 16.dp, top = 4.dp, bottom = 16.dp).heightIn(min = QuickHeight).height(IntrinsicSize.Min),
                        horizontalArrangement = Arrangement.spacedBy(3.dp),
                    ) {
                        quick.forEachIndexed { i, a ->
                            QuickButton(a, quickShape(i, quick.size), playFace(a.id) ?: (label(a) to icon(a)), Modifier.weight(1f), enabled = active && a.enabled) {
                                if (songs) { client.dispatch(Commands.runAction(a.id, client.selectionTarget())); client.clearSelection() } else run(a)
                            }
                        }
                    }
                }
                // The rest: drawn as it opens; out of reach (no semantics, no focus) at the peek.
                Column(
                    Modifier
                        .weight(1f, fill = false)
                        .graphicsLayer { alpha = (openness() * 2f).coerceIn(0f, 1f) }
                        .then(if (opened) Modifier else Modifier.clearAndSetSemantics { }.focusProperties { onEnter = { cancelFocusChange() } })
                        .focusGroup()
                        .verticalScroll(rememberScrollState()),
                ) {
                    val m = menu
                    if (m != null) {
                        val s = m.subject
                        // A fresh sheet per selection, not per change in what it shows: the item's own
                        // Download toggle changes the pins, which mustn't reset the sheet (or stop a preview).
                        key(kind, selection.inverted, selection.ids, s.kind) {
                            MenuSheet(
                                s, client.selectionTarget(), actions,
                                extras = listOfNotNull(onRemoveFromContext?.takeIf { context != null && songs }?.let { remove ->
                                    val label = if (s.count > 1) resources.getString(R.string.menu_remove_n_from, s.count, context) else resources.getString(R.string.menu_remove_from, context)
                                    SongMenuExtra(label, HocketIcons.Filled.PlaylistRemove,
                                        onClick = { remove(); client.clearSelection() }, destructive = true, testTag = "selectionMenu.removeFromPlaylist", replaces = ActionIds.REMOVE_FROM_PLAYLIST)
                                }),
                                onGoToAlbum = s.albumId?.takeIf { s.kind == MenuKind.Song }?.let { id -> leaveTo { it.openAlbum(id) } },
                                onGoToArtist = s.artistId?.takeIf { s.kind != MenuKind.Songs }?.let { id -> leaveTo { it.openArtist(id) } },
                                onInfo = m.track?.let { t -> { songInfo = t } } ?: m.album?.let { a -> { albumInfo = a } },
                                onDismiss = { if (client.selection.value.active) settleTo(DrawerValue.Peek) },
                                onRan = { client.clearSelection() },
                                tag = "selectionMenu",
                                embedded = true,
                                showPlay = false,
                                resetKey = menuResets,
                            )
                        }
                    } else if (rest.isNotEmpty()) {
                        Column(Modifier.fillMaxWidth().padding(start = 16.dp, end = 16.dp, bottom = 16.dp), verticalArrangement = Arrangement.spacedBy(2.dp)) {
                            rest.forEachIndexed { i, a ->
                                MenuRow(AnnotatedString(label(a)), icon(a), connectedShape(i, rest.size), enabled = a.enabled, destructive = a.destructive, tag = "selection.action.${a.id}") { run(a) }
                            }
                        }
                    }
                    Spacer(Modifier.windowInsetsBottomHeight(WindowInsets.navigationBars))
                }
            }
        }
    }
    songInfo?.let { t ->
        ModalBottomSheet(onDismissRequest = { songInfo = null }, sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = true), modifier = Modifier.testTag("selectionMenu.infoSheet")) {
            app.hocket.ui.player.PlayerAbout(
                Modifier.fillMaxWidth().navigationBarsPadding(), track = t, title = stringResource(R.string.menu_song_info), onClose = { songInfo = null },
                onOpenAlbum = navigator?.let { n -> { id -> songInfo = null; client.clearSelection(); n.openAlbum(id) } },
                onOpenArtist = navigator?.let { n -> { id -> songInfo = null; client.clearSelection(); n.openArtist(id) } },
            )
        }
    }
    albumInfo?.let { a ->
        ModalBottomSheet(onDismissRequest = { albumInfo = null }, sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = true), modifier = Modifier.testTag("selectionMenu.infoSheet")) {
            app.hocket.ui.player.AlbumAbout(a, stringResource(R.string.menu_album_info), Modifier.fillMaxWidth().navigationBarsPadding(),
                onOpenArtist = navigator?.let { n -> { id -> albumInfo = null; client.clearSelection(); n.openArtist(id) } })
        }
    }
    if (playlistPicker) PlaylistPicker(target = client.selectionTarget(), onDismiss = { playlistPicker = false; client.clearSelection() })
    if (rating) {
        // Applies to the whole selection, so 0 (clear) is offered alongside the stars.
        RatingDialog(current = 0, onRate = { stars -> client.dispatch(Commands.runAction(ActionIds.rate(stars), client.selectionTarget())); rating = false; client.clearSelection() },
            onDismiss = { rating = false }, title = setRating, offerClear = true)
    }
}

/** The quick actions' row height: kept while the actions load, so the peek doesn't jump. */
private val QuickHeight = 72.dp

/** A quick action's shape in the connected row: round at the row's ends, tight between. */
private fun quickShape(index: Int, count: Int): Shape {
    val start = if (index == 0) 24.dp else 8.dp
    val end = if (index == count - 1) 24.dp else 8.dp
    return RoundedCornerShape(topStart = start, bottomStart = start, topEnd = end, bottomEnd = end)
}

/** One quick action: its icon over its short label; Play stands out in the primary colour. */
@Composable
private fun QuickButton(a: ActionDescriptor, shape: Shape, face: Pair<String, ImageVector>, modifier: Modifier, enabled: Boolean, onClick: () -> Unit) {
    val main = a.id == ActionIds.PLAY
    Surface(
        onClick = onClick,
        enabled = enabled,
        shape = shape,
        color = if (main) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.secondaryContainer,
        contentColor = if (main) MaterialTheme.colorScheme.onPrimary else MaterialTheme.colorScheme.onSecondaryContainer,
        modifier = modifier.fillMaxHeight().testTag("selection.action.${a.id}"),
    ) {
        Column(
            Modifier.alpha(if (a.enabled) 1f else 0.38f).padding(horizontal = 4.dp, vertical = 8.dp),
            horizontalAlignment = Alignment.CenterHorizontally,
            verticalArrangement = Arrangement.spacedBy(4.dp, Alignment.CenterVertically),
        ) {
            Icon(face.second, null, Modifier.size(22.dp))
            // One line at normal font sizes; a large font size wraps rather than cutting the label off.
            Text(face.first, style = MaterialTheme.typography.labelMedium.copy(fontSize = 12.sp), fontWeight = if (main) FontWeight.SemiBold else FontWeight.Medium, textAlign = TextAlign.Center, maxLines = 2, overflow = TextOverflow.Ellipsis)
        }
    }
}

/**
 * The drawer's handle: a wide target that opens or closes it, drawn as the usual pill. Labelled
 * More / Show less, with expand and collapse actions for accessibility.
 */
@Composable
private fun DrawerHandle(open: Boolean, canOpen: Boolean, onToggle: () -> Unit) {
    val label = stringResource(if (!canOpen) R.string.menu_handle else if (open) R.string.menu_show_less else R.string.menu_more)
    val toggle by rememberUpdatedState(onToggle)
    Box(Modifier.fillMaxWidth(), contentAlignment = Alignment.Center) {
        Box(
            Modifier
                .width(96.dp)
                .height(28.dp)
                .clip(RoundedCornerShape(14.dp))
                .then(if (canOpen) Modifier.clickable(interactionSource = remember { MutableInteractionSource() }, indication = null) { toggle() } else Modifier)
                .semantics {
                    contentDescription = label
                    if (canOpen) { if (open) collapse { toggle(); true } else expand { toggle(); true } }
                }
                .testTag("selection.handle"),
            contentAlignment = Alignment.Center,
        ) {
            Box(Modifier.size(width = 32.dp, height = 4.dp).clip(RoundedCornerShape(2.dp)).background(MaterialTheme.colorScheme.onSurfaceVariant.copy(alpha = 0.6f)))
        }
    }
}

/**
 * The header and toggles of the selected songs' menu: "3 songs · 11 min", their titles, the first
 * one's artwork, how many are loved, their rating (null when they differ: "Mixed"), whether all are
 * downloaded. A select-all selection names only its count (its songs are not listed here).
 */
internal suspend fun selectionSubject(client: CoreClient, selection: Selection, kind: SelectionKind, context: String?, resources: android.content.res.Resources, dot: String): MenuSubject {
    val count = selection.count
    val tracks: List<TrackSummary> = if (selection.inverted) emptyList() else when (kind) {
        SelectionKind.QueueItems -> {
            val q = client.queue.value
            val all = q.entries + listOfNotNull(q.current)
            selection.ids.mapNotNull { k -> all.firstOrNull { it.item.key == k }?.track }
        }
        else -> (client.query(Queries.tracksByIds((client.selectionTarget() as? app.hocket.core.api.ActionTarget.Tracks)?.data?.tracks.orEmpty())) as? QueryResult.TrackList)?.data?.map { it.toSummary() }.orEmpty()
    }
    return songsSubject(count, tracks, context, resources, dot, removeFromQueue = kind == SelectionKind.QueueItems)
}

/**
 * The open drawer's menu: its header, and what one item's info row opens (the song, or the album).
 */
@Immutable
internal data class DrawerMenu(val subject: MenuSubject, val track: TrackSummary? = null, val album: Album? = null)

/**
 * The menu of a selection of one item named by id (an inverted select-all lists none): the same as its three dots open, a
 * song's ([songSubject]) or an album's, artist's or playlist's ([menuSubject]). Null for several
 * items, or a song that can't be found (the songs' menu stands in).
 */
internal suspend fun singleMenu(
    client: CoreClient, selection: Selection, kind: SelectionKind, context: String?, pins: List<Pin>,
    resources: android.content.res.Resources, dot: String, unknownArtist: String, fallbackTitle: String,
): DrawerMenu? {
    if (selection.inverted || selection.ids.size != 1) return null
    val target = client.selectionTarget()
    return when (kind) {
        SelectionKind.Tracks, SelectionKind.QueueItems -> {
            val track = if (kind == SelectionKind.QueueItems) {
                val q = client.queue.value
                (q.entries + listOfNotNull(q.current)).firstOrNull { it.item.key == selection.ids.first() }?.track
            } else (client.query(Queries.tracksByIds((target as? ActionTarget.Tracks)?.data?.tracks.orEmpty())) as? QueryResult.TrackList)?.data?.firstOrNull()?.toSummary()
            // The queue's own menu names no playlist, as its three dots don't.
            track?.let { DrawerMenu(songSubject(it, context.takeIf { kind == SelectionKind.Tracks }, pins, dot, unknownArtist), track = it) }
        }
        else -> menuSubject(client, target, fallbackTitle, null, resources, dot, pins).let { (subject, album) -> DrawerMenu(subject, album = album) }
    }
}

/** [selectionSubject] once the songs are known (pure but for the words). */
internal fun songsSubject(count: Int, tracks: List<TrackSummary>, context: String?, resources: android.content.res.Resources, dot: String, removeFromQueue: Boolean = false): MenuSubject {
    val minutes = (tracks.sumOf { it.durationMs.toLong() } / 60_000).toInt()
    val title = resources.getQuantityString(R.plurals.menu_songs, count, count) + if (tracks.isNotEmpty()) dot + if (minutes >= 60) resources.getString(R.string.duration_hours_minutes, minutes / 60, minutes % 60) else resources.getString(R.string.duration_minutes, minutes) else ""
    val loved = tracks.count { it.loved }
    val ratings = tracks.map { it.rating.toInt() }.distinct()
    return MenuSubject(
        kind = MenuKind.Songs,
        title = title,
        subtitle = tracks.joinToString(", ") { it.title }.ifEmpty { null },
        coverArt = tracks.firstOrNull()?.coverArt,
        loved = when { tracks.isEmpty() || loved == 0 -> Tick.Off; loved == tracks.size -> Tick.On; else -> Tick.Mixed },
        lovedCount = loved,
        rating = ratings.singleOrNull() ?: if (ratings.isEmpty()) 0 else null,
        downloaded = tracks.isNotEmpty() && tracks.all { it.offline == OfflineState.Downloaded },
        count = count,
        context = if (removeFromQueue) null else context,
    )
}
