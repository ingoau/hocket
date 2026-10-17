package app.hocket.ui.screens.library

import android.content.res.Resources
import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.grid.GridCells
import androidx.compose.foundation.lazy.grid.LazyVerticalGrid
import androidx.compose.foundation.lazy.grid.itemsIndexed
import androidx.compose.foundation.lazy.grid.rememberLazyGridState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Snackbar
import androidx.compose.material3.SnackbarDuration
import androidx.compose.material3.SnackbarHost
import androidx.compose.material3.SnackbarHostState
import androidx.compose.material3.SnackbarResult
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.TopAppBarDefaults
import androidx.compose.runtime.Composable
import androidx.compose.runtime.SideEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.produceState
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.graphics.luminance
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.graphics.vector.addPathNodes
import androidx.compose.ui.hapticfeedback.HapticFeedbackType
import androidx.compose.ui.platform.LocalHapticFeedback
import androidx.compose.ui.platform.LocalResources
import androidx.compose.ui.platform.LocalView
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.CustomAccessibilityAction
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.customActions
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.window.Dialog
import androidx.compose.ui.window.DialogProperties
import androidx.compose.ui.window.DialogWindowProvider
import androidx.core.view.WindowCompat
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import app.hocket.R
import app.hocket.core.ArtworkSizes
import app.hocket.core.Queries
import app.hocket.core.api.QueryResult
import app.hocket.core.client.CoreClient
import app.hocket.ui.LocalCoreClient
import app.hocket.ui.icons.HocketIcons
import kotlinx.coroutines.launch
import sh.calvin.reorderable.ReorderableCollectionItemScope
import sh.calvin.reorderable.ReorderableItem
import sh.calvin.reorderable.rememberReorderableLazyGridState

/**
 * The pin editor (the Library's Edit), a full-screen dialog: the pins as a two-column connected
 * group of tall tiles, each with a drag handle, an unpin button and a chip that steps through what a
 * tap does (songs always play, so theirs has none). Every change goes to a draft; Done saves it to
 * `library.pins`, and × or back throws it away. [pins] are the Library's resolved pins, for titles
 * and artwork; a pin that isn't shown (its album or filter is gone) keeps its place in the draft.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun PinEditor(pins: List<ResolvedPin>, onClose: () -> Unit) {
    val client = LocalCoreClient.current
    val saved = remember { LibraryPins.current(client) }
    // The draft, as `library.pins` JSON so it survives rotation.
    var draftJson by rememberSaveable { mutableStateOf(LibraryPins.encode(saved)) }
    val draft = remember(draftJson) { LibraryPins.parse(draftJson) }
    // Event handlers read the draft as it is now, not as it was when they were made.
    fun draftNow() = LibraryPins.parse(draftJson)
    fun edit(next: List<PinEntry>) { draftJson = LibraryPins.encode(next) }
    val byKey = remember(pins) { pins.associateBy { it.entry.kind to it.entry.id } }
    // What's drawn: the draft's pins this build can show, with the draft's tap.
    val shown = remember(draft, byKey) { draft.mapNotNull { e -> byKey[e.kind to e.id]?.copy(entry = e) } }
    val shownEntries = remember(shown) { shown.map { it.entry } }
    val details by rememberPinDetails(pins)
    val snackbar = remember { SnackbarHostState() }
    val scope = rememberCoroutineScope()
    val haptics = LocalHapticFeedback.current
    val resources = LocalResources.current
    val undoLabel = stringResource(R.string.action_undo)
    fun done() {
        val final = draftNow()
        if (LibraryPins.changed(saved, final)) LibraryPins.save(client, final)
        onClose()
    }
    fun unpin(pin: ResolvedPin) {
        val (next, at) = LibraryPins.unpinned(draftNow(), pin.entry)
        if (at < 0) return
        edit(next)
        scope.launch {
            // One Undo at a time: the last unpin's snackbar replaces the one before.
            snackbar.currentSnackbarData?.dismiss()
            if (snackbar.showSnackbar(resources.getString(R.string.library_unpinned, pin.title), undoLabel, duration = SnackbarDuration.Short) == SnackbarResult.ActionPerformed) {
                edit(LibraryPins.restored(draftNow(), pin.entry, at))
            }
        }
    }
    Dialog(onDismissRequest = onClose, properties = DialogProperties(usePlatformDefaultWidth = false, decorFitsSystemWindows = false)) {
        val background = MaterialTheme.colorScheme.surfaceContainer
        // The dialog's own window draws the system bars: their icons follow the theme, as the app's do.
        val view = LocalView.current
        SideEffect {
            (view.parent as? DialogWindowProvider)?.window?.let { window ->
                window.setDimAmount(0f)
                WindowCompat.getInsetsController(window, view).apply {
                    isAppearanceLightStatusBars = background.luminance() > 0.5f
                    isAppearanceLightNavigationBars = background.luminance() > 0.5f
                }
            }
        }
        Scaffold(
            containerColor = background,
            modifier = Modifier.testTag("library.pins.editor"),
            topBar = {
                TopAppBar(
                    navigationIcon = {
                        IconButton(onClick = onClose, modifier = Modifier.testTag("library.pins.editor.cancel")) {
                            Icon(HocketIcons.Filled.Close, stringResource(R.string.library_pins_cancel))
                        }
                    },
                    title = { Text(stringResource(R.string.library_pins_edit), fontWeight = FontWeight.SemiBold, modifier = Modifier.semantics { heading() }) },
                    actions = {
                        Button(onClick = ::done, contentPadding = ButtonDefaults.ButtonWithIconContentPadding, modifier = Modifier.padding(end = 8.dp).testTag("library.pins.editor.done")) {
                            Icon(HocketIcons.Filled.Check, null, Modifier.size(ButtonDefaults.IconSize))
                            Spacer(Modifier.width(ButtonDefaults.IconSpacing))
                            Text(stringResource(R.string.library_pins_done))
                        }
                    },
                    colors = TopAppBarDefaults.topAppBarColors(containerColor = background, scrolledContainerColor = background),
                )
            },
            snackbarHost = { SnackbarHost(snackbar) { data -> Snackbar(data, shape = RoundedCornerShape(16.dp)) } },
        ) { padding ->
            Box(Modifier.fillMaxSize().padding(padding)) {
                Column(Modifier.fillMaxSize()) {
                    Row(
                        Modifier.padding(horizontal = 16.dp, vertical = 4.dp).fillMaxWidth()
                            .background(MaterialTheme.colorScheme.secondaryContainer, RoundedCornerShape(16.dp))
                            .padding(horizontal = 14.dp, vertical = 10.dp),
                        horizontalArrangement = Arrangement.spacedBy(10.dp),
                    ) {
                        Icon(HocketIcons.Outlined.Info, null, Modifier.padding(top = 1.dp).size(18.dp), tint = MaterialTheme.colorScheme.onSecondaryContainer)
                        Text(stringResource(R.string.library_pins_hint), style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSecondaryContainer)
                    }
                    val grid = rememberLazyGridState()
                    val reorderable = rememberReorderableLazyGridState(grid) { from, to ->
                        val now = draftNow()
                        val moving = now.firstOrNull { pinKey(it) == from.key } ?: return@rememberReorderableLazyGridState
                        val other = now.firstOrNull { pinKey(it) == to.key } ?: return@rememberReorderableLazyGridState
                        edit(LibraryPins.movedPast(now, moving, other, later = to.index > from.index))
                        haptics.performHapticFeedback(HapticFeedbackType.SegmentFrequentTick)
                    }
                    LazyVerticalGrid(
                        columns = GridCells.Fixed(2),
                        state = grid,
                        // Room at the end for the count (or the snackbar) over the grid's bottom.
                        contentPadding = PaddingValues(start = 16.dp, end = 16.dp, top = 12.dp, bottom = 72.dp),
                        horizontalArrangement = Arrangement.spacedBy(GROUP_GAP),
                        verticalArrangement = Arrangement.spacedBy(GROUP_GAP),
                        modifier = Modifier.weight(1f).fillMaxWidth().testTag("library.pins.editor.grid"),
                    ) {
                        itemsIndexed(shown, key = { _, pin -> pinKey(pin.entry) }, contentType = { _, _ -> "pin" }) { i, pin ->
                            ReorderableItem(reorderable, key = pinKey(pin.entry)) { dragging ->
                                val moveEarlier = stringResource(R.string.library_pin_move_earlier)
                                val moveLater = stringResource(R.string.library_pin_move_later)
                                val actions = buildList {
                                    if (i > 0) add(CustomAccessibilityAction(moveEarlier) { edit(LibraryPins.moved(draftNow(), pin.entry, -1, shownEntries)); true })
                                    if (i < shown.lastIndex) add(CustomAccessibilityAction(moveLater) { edit(LibraryPins.moved(draftNow(), pin.entry, 1, shownEntries)); true })
                                }
                                PinEditTile(
                                    pin, details[pin.entry.kind to pin.entry.id], pinGroupEdges(i, shown.size), dragging, actions,
                                    onCycle = { edit(LibraryPins.withTap(draftNow(), pin.entry, pin.entry.pinTap.next)) },
                                    onUnpin = { unpin(pin) },
                                )
                            }
                        }
                    }
                }
                if (snackbar.currentSnackbarData == null) {
                    Text(
                        pluralStringResource(R.plurals.library_pins_count, shown.size, shown.size),
                        style = MaterialTheme.typography.bodyMedium,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                        textAlign = TextAlign.Center,
                        modifier = Modifier.align(Alignment.BottomCenter).fillMaxWidth().padding(horizontal = 16.dp, vertical = 20.dp).testTag("library.pins.editor.count"),
                    )
                }
            }
        }
    }
}

private fun pinKey(pin: PinEntry) = "${pin.kind}:${pin.id}"

/** The chip's place in a tile: its 48dp touch target and the 2dp above it. */
private val TapChipHeight = 50.dp

/**
 * One tile of the editor: the drag handle and the unpin button, the artwork in its kind's shape,
 * the title and kind line, and what a tap does. Picked up by its handle ([dragging]) it lifts and
 * tilts a little.
 */
@Composable
private fun ReorderableCollectionItemScope.PinEditTile(
    pin: ResolvedPin,
    detail: String?,
    edges: GroupEdges,
    dragging: Boolean,
    actions: List<CustomAccessibilityAction>,
    onCycle: () -> Unit,
    onUnpin: () -> Unit,
) {
    val haptics = LocalHapticFeedback.current
    val lift by animateFloatAsState(if (dragging) 1f else 0f, label = "pinLift")
    val shape = edges.shape()
    val kindLabel = stringResource(pin.kind.label)
    Surface(
        shape = shape,
        color = MaterialTheme.colorScheme.surfaceBright,
        modifier = Modifier
            .graphicsLayer {
                rotationZ = -1.5f * lift
                scaleX = 1f + 0.02f * lift
                scaleY = 1f + 0.02f * lift
                shadowElevation = 12.dp.toPx() * lift
                this.shape = shape
            }
            .heightIn(min = 200.dp)
            .semantics(mergeDescendants = true) { customActions = actions }
            .testTag("library.pins.editor.pin"),
    ) {
        Column(Modifier.padding(start = 2.dp, end = 2.dp, bottom = 6.dp), horizontalAlignment = Alignment.CenterHorizontally) {
            Row(Modifier.fillMaxWidth().height(48.dp), horizontalArrangement = Arrangement.SpaceBetween, verticalAlignment = Alignment.CenterVertically) {
                val reorderLabel = stringResource(R.string.library_pin_reorder, pin.title)
                // Its own stop for TalkBack, with the moves (a drag needs a finger on it).
                Box(
                    Modifier
                        .draggableHandle(
                            onDragStarted = { haptics.performHapticFeedback(HapticFeedbackType.GestureThresholdActivate) },
                            onDragStopped = { haptics.performHapticFeedback(HapticFeedbackType.GestureEnd) },
                        )
                        .size(48.dp).semantics(mergeDescendants = true) { contentDescription = reorderLabel; customActions = actions }.testTag("library.pins.editor.handle"),
                    contentAlignment = Alignment.Center,
                ) {
                    Icon(DragIndicator, null, Modifier.size(20.dp), tint = MaterialTheme.colorScheme.onSurfaceVariant)
                }
                IconButton(onClick = onUnpin, modifier = Modifier.size(48.dp).testTag("library.pins.editor.unpin")) {
                    Box(Modifier.size(28.dp).background(MaterialTheme.colorScheme.surfaceContainerHighest, CircleShape), contentAlignment = Alignment.Center) {
                        Icon(HocketIcons.Filled.Close, stringResource(R.string.library_pin_unpin_named, pin.title), Modifier.size(16.dp), tint = MaterialTheme.colorScheme.onSurface)
                    }
                }
            }
            PinArt(pin, Modifier.offset(y = (-8).dp).size(64.dp), artSize = ArtworkSizes.LIST, iconSize = 30.dp)
            Text(
                pin.title, style = MaterialTheme.typography.titleSmall, fontWeight = FontWeight.SemiBold, maxLines = 1, overflow = TextOverflow.Ellipsis,
                textAlign = TextAlign.Center, modifier = Modifier.fillMaxWidth().padding(start = 4.dp, end = 4.dp, top = 2.dp),
            )
            Text(
                if (detail != null) "$kindLabel · $detail" else kindLabel,
                style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant, maxLines = 1, overflow = TextOverflow.Ellipsis,
                textAlign = TextAlign.Center, modifier = Modifier.fillMaxWidth().padding(horizontal = 4.dp),
            )
            // A song has no chip (it always plays): the same room stays empty, so a row's tiles match.
            if (pin.kind.choosesTap) TapChip(pin, onCycle) else Spacer(Modifier.height(TapChipHeight))
        }
    }
}

/** What a tap does, as a chip that steps on to the next choice: Opens, Plays (filled), Shuffles (tonal). */
@Composable
private fun TapChip(pin: ResolvedPin, onCycle: () -> Unit) {
    val tap = pin.entry.pinTap
    val (text, label) = when (tap) {
        PinTap.Open -> R.string.library_pin_chip_opens to R.string.library_pin_chip_opens_named
        PinTap.Play -> R.string.library_pin_chip_plays to R.string.library_pin_chip_plays_named
        PinTap.Shuffle -> R.string.library_pin_chip_shuffles to R.string.library_pin_chip_shuffles_named
    }
    val colors = MaterialTheme.colorScheme
    val (container, content) = when (tap) {
        PinTap.Open -> colors.surfaceContainerHighest to colors.onSurface
        PinTap.Play -> colors.primary to colors.onPrimary
        PinTap.Shuffle -> colors.secondaryContainer to colors.onSecondaryContainer
    }
    val description = stringResource(label, pin.title)
    // The chip draws 30dp tall; the Surface keeps a full-size touch target round it.
    Box(Modifier.padding(top = 2.dp).height(TapChipHeight - 2.dp), contentAlignment = Alignment.Center) { Surface(
        onClick = onCycle,
        shape = CircleShape,
        color = container,
        contentColor = content,
        modifier = Modifier.semantics { contentDescription = description }.testTag("library.pins.editor.chip"),
    ) {
        Row(Modifier.height(30.dp).padding(start = 10.dp, end = 12.dp), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(6.dp)) {
            val icon = when (tap) {
                PinTap.Open -> ArrowOutward
                PinTap.Play -> HocketIcons.Filled.PlayArrow
                PinTap.Shuffle -> HocketIcons.Filled.Shuffle
            }
            Icon(icon, null, Modifier.size(16.dp))
            Text(stringResource(text), style = MaterialTheme.typography.labelLarge, fontWeight = FontWeight.SemiBold, modifier = Modifier.clearAndSetSemantics { })
            Icon(HocketIcons.Filled.KeyboardArrowDown, null, Modifier.size(14.dp))
        }
    } }
}

/**
 * Each pin's kind line after its kind (keyed by kind and id): an album's or song's artist, and how
 * many songs a playlist, genre or filter holds. Artists have none.
 */
@Composable
private fun rememberPinDetails(pins: List<ResolvedPin>) = run {
    val client = LocalCoreClient.current
    val resources = LocalResources.current
    val libraryGen by client.libraryGeneration.collectAsStateWithLifecycle()
    produceState(emptyMap<Pair<String, String>, String>(), pins, libraryGen) { value = pinDetails(client, resources, pins) }
}

private suspend fun pinDetails(client: CoreClient, resources: Resources, pins: List<ResolvedPin>): Map<Pair<String, String>, String> {
    val serverId = client.server.value?.id
    fun songs(n: Int) = resources.getQuantityString(R.plurals.menu_songs, n, n)
    // Every genre's count comes in one query, made only when a genre is pinned.
    val genres = if (pins.any { it.kind == PinKind.Genre } && serverId != null) (client.query(Queries.genres(serverId)) as? QueryResult.Genres)?.data.orEmpty() else emptyList()
    return pins.mapNotNull { pin ->
        val id = pin.entry.id
        val detail = when (pin.kind) {
            PinKind.Album -> (client.query(Queries.album(id)) as? QueryResult.AlbumDetail)?.data?.artist
            PinKind.Track -> (client.query(Queries.track(id)) as? QueryResult.TrackDetail)?.data?.let { it.artist ?: it.albumArtist }
            PinKind.Playlist -> (client.query(Queries.playlist(id)) as? QueryResult.PlaylistDetail)?.data?.let { songs(it.songCount.toInt()) }
            PinKind.Genre -> genres.firstOrNull { it.name == id }?.let { songs(it.songCount.toInt()) }
            PinKind.Filter -> client.filters.value.firstOrNull { it.id == id }?.let { filter ->
                val count = (serverId?.let { client.query(Queries.trackCount(it, filter.root)) } as? QueryResult.Count)?.data?.toInt()
                count?.let { songs(filter.limit?.toInt()?.coerceAtMost(it) ?: it) }
            }
            PinKind.Artist -> null
        }
        detail?.takeIf { it.isNotBlank() }?.let { (pin.entry.kind to id) to it }
    }.toMap()
}

/*
 * Two Material Symbols (rounded) the generated HocketIcons doesn't carry yet, from the same Iconify
 * set: drag_indicator (the handle's six dots) and arrow_outward (the chip's "Opens").
 */
private val DragIndicator: ImageVector by lazy {
    symbol("DragIndicator", "M 9 20 q -0.825 0 -1.412 -0.587 T 7 18 t 0.588 -1.412 T 9 16 t 1.413 0.588 T 11 18 t -0.587 1.413 T 9 20 m 6 0 q -0.825 0 -1.412 -0.587 T 13 18 t 0.588 -1.412 T 15 16 t 1.413 0.588 T 17 18 t -0.587 1.413 T 15 20 m -6 -6 q -0.825 0 -1.412 -0.587 T 7 12 t 0.588 -1.412 T 9 10 t 1.413 0.588 T 11 12 t -0.587 1.413 T 9 14 m 6 0 q -0.825 0 -1.412 -0.587 T 13 12 t 0.588 -1.412 T 15 10 t 1.413 0.588 T 17 12 t -0.587 1.413 T 15 14 M 9 8 q -0.825 0 -1.412 -0.587 T 7 6 t 0.588 -1.412 T 9 4 t 1.413 0.588 T 11 6 t -0.587 1.413 T 9 8 m 6 0 q -0.825 0 -1.412 -0.587 T 13 6 t 0.588 -1.412 T 15 4 t 1.413 0.588 T 17 6 t -0.587 1.413 T 15 8")
}

private val ArrowOutward: ImageVector by lazy {
    symbol("ArrowOutward", "m 16 8.4 l -8.9 8.9 q -0.275 0.275 -0.7 0.275 t -0.7 -0.275 t -0.275 -0.7 t 0.275 -0.7 L 14.6 7 H 7 q -0.425 0 -0.712 -0.288 T 6 6 t 0.288 -0.712 T 7 5 h 10 q 0.425 0 0.713 0.288 T 18 6 v 10 q 0 0.425 -0.288 0.713 T 17 17 t -0.712 -0.288 T 16 16 z")
}

private fun symbol(name: String, d: String): ImageVector =
    ImageVector.Builder(name, 24.dp, 24.dp, 24f, 24f).apply { addPath(addPathNodes(d), fill = SolidColor(Color.Black)) }.build()
