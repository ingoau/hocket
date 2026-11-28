package app.hocket.ui.stash

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
import androidx.compose.foundation.gestures.scrollBy
import androidx.compose.runtime.snapshotFlow
import androidx.compose.runtime.withFrameNanos
import androidx.compose.ui.layout.LayoutCoordinates
import androidx.compose.ui.layout.onGloballyPositioned
import androidx.compose.ui.layout.positionInWindow
import androidx.compose.ui.platform.LocalDensity
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.withTimeoutOrNull
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
fun StashSheet(onDismiss: () -> Unit, navigator: DetailNavigator? = LocalDetailNavigator.current) {
    val client = LocalCoreClient.current
    val items by client.stash.collectAsStateWithLifecycle()
    val haptics = LocalHapticFeedback.current
    val resources = LocalResources.current
    val scope = rememberCoroutineScope()
    val sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = true)
    // The sheet is a window over the shell's snackbars: they (and their Undo) show in it instead.
    val sheetSnackbars = remember { SnackbarHostState() }
    val snackbar = remember(scope) { AppSnackbar(sheetSnackbars, scope) }
    DisposableEffect(Unit) {
        app.hocket.ui.components.SnackbarRedirect.host = sheetSnackbars
        onDispose { if (app.hocket.ui.components.SnackbarRedirect.host === sheetSnackbars) app.hocket.ui.components.SnackbarRedirect.host = null }
    }
    fun close() { scope.launch { sheetState.hide() }.invokeOnCompletion { onDismiss() } }
    var picked by remember { mutableStateOf(setOf<String>()) }
    // Emptied (here or on another device) after having songs: it slides away. (Restored before the
    // stash has arrived, it waits for it rather than closing at once.)
    var hadSongs by remember { mutableStateOf(false) }
    LaunchedEffect(items.isEmpty()) {
        if (items.isNotEmpty()) hadSongs = true
        else if (hadSongs) { sheetState.hide(); onDismiss() }
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
    ModalBottomSheet(onDismissRequest = onDismiss, sheetState = sheetState, modifier = Modifier.testTag("stash.sheet")) {
        CompositionLocalProvider(LocalAppSnackbar provides snackbar, LocalDetailNavigator provides songNavigator) {
        Box {
        Column(Modifier.navigationBarsPadding()) {
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
            StashList(
                items = items,
                picked = picked,
                onToggle = { id -> picked = if (id in picked) picked - id else picked + id; haptics.performHapticFeedback(HapticFeedbackType.SegmentTick) },
                onMove = { id, to -> client.dispatch(Command.StashMove(CommandStashMoveInner(id, to.toUInt()))) },
            ) { playGroupModifier ->
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
                        playGroupModifier = playGroupModifier,
                    )
                }
            }
        }
        SnackbarHost(sheetSnackbars, Modifier.align(Alignment.BottomCenter).navigationBarsPadding().padding(8.dp))
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
}

@Composable
private fun StashList(
    items: List<StashItem>,
    picked: Set<String>,
    onToggle: (String) -> Unit,
    onMove: (String, Int) -> Unit,
    /** The menu after the songs; its play group carries the modifier it is handed. */
    footer: @Composable (playGroupModifier: Modifier) -> Unit,
) {
    val haptics = LocalHapticFeedback.current
    val dot = stringResource(R.string.dot_separator)
    // The list as shown: moves land here at once and reach the core when the drag ends.
    var shown by remember { mutableStateOf(items) }
    var dragging by remember { mutableStateOf<String?>(null) }
    LaunchedEffect(items, dragging) { if (dragging == null) shown = items }
    val current by rememberUpdatedState(items)
    val listState = rememberLazyListState()
    val reorder = rememberReorderableLazyListState(listState) { from, to ->
        // Only song rows move (the menu after them is no row of the list).
        val a = shown.indexOfFirst { it.entryId == from.key }
        val b = shown.indexOfFirst { it.entryId == to.key }
        if (a < 0 || b < 0) return@rememberReorderableLazyListState
        shown = shown.toMutableList().apply { add(b, removeAt(a)) }
        haptics.performHapticFeedback(HapticFeedbackType.SegmentFrequentTick)
    }
    // Opened, the list lands with the play group at its foot: as many songs as fit above it, the
    // buttons in reach. (Once: after that it scrolls only as the user scrolls it.)
    val listAt = remember { arrayOfNulls<LayoutCoordinates>(1) }
    val playAt = remember { arrayOfNulls<LayoutCoordinates>(1) }
    val margin = with(LocalDensity.current) { 12.dp.toPx() }
    LaunchedEffect(Unit) {
        snapshotFlow { shown.size }.first { it > 0 }
        val footer = shown.size
        if (listState.layoutInfo.visibleItemsInfo.none { it.index == footer }) listState.scrollToItem(footer)
        // The play group shows once the menu's actions have loaded.
        val play = withTimeoutOrNull(2_000) {
            var found: LayoutCoordinates? = null
            while (found == null) { withFrameNanos { }; found = playAt[0]?.takeIf { it.isAttached } }
            found
        } ?: return@LaunchedEffect
        val list = listAt[0]?.takeIf { it.isAttached } ?: return@LaunchedEffect
        val playBottom = play.positionInWindow().y + play.size.height
        val listBottom = list.positionInWindow().y + list.size.height
        listState.scrollBy(playBottom - (listBottom - margin))
    }
    val moveUp = stringResource(R.string.stash_move_up)
    val moveDown = stringResource(R.string.stash_move_down)
    LazyColumn(
        state = listState,
        modifier = Modifier.fillMaxWidth().heightIn(min = 120.dp).onGloballyPositioned { listAt[0] = it }.testTag("stash.list"),
        contentPadding = PaddingValues(bottom = 16.dp),
    ) {
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
        item(key = "stash.menu") { footer(Modifier.onGloballyPositioned { playAt[0] = it }) }
    }
}
