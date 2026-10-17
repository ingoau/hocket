package app.hocket.ui.components

import androidx.compose.animation.core.Animatable
import androidx.compose.animation.core.Spring
import androidx.compose.animation.core.spring
import androidx.compose.foundation.background
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.RowScope
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.selection.triStateToggleable
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.Immutable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.derivedStateOf
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.produceState
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.clipToBounds
import androidx.compose.ui.draw.drawWithContent
import androidx.compose.ui.draw.rotate
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Shape
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.graphics.drawscope.clipRect
import androidx.compose.ui.graphics.luminance
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.input.nestedscroll.NestedScrollConnection
import androidx.compose.ui.input.nestedscroll.NestedScrollSource
import androidx.compose.ui.input.nestedscroll.nestedScroll
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.layout
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.collapse
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.expand
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.isTraversalGroup
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.state.ToggleableState
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.text.withStyle
import androidx.compose.ui.unit.Constraints
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.Velocity
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import app.hocket.R
import app.hocket.core.ActionIds
import app.hocket.core.ArtworkSizes
import app.hocket.core.Commands
import app.hocket.core.Queries
import app.hocket.core.api.ActionDescriptor
import app.hocket.core.api.ActionTarget
import app.hocket.core.api.QueryResult
import app.hocket.ui.LocalCoreClient
import app.hocket.ui.icons.HocketIcons
import kotlinx.coroutines.launch
import kotlin.math.roundToInt

/** What a menu is for: its header and which design it follows. [Songs] is a multi-selection. */
enum class MenuKind { Song, Album, Artist, Playlist, Songs, Other }

/**
 * The item a menu acts on, as its header and toggles show it: title and lines under it, artwork,
 * loved (several songs can be partly loved), rating (null: several songs rated differently,
 * "Mixed"), downloaded and the download row's note ("4.2 MB", "12 songs"; a song's size is fetched
 * by [trackId] when not given), the album and artist the go-to rows name, how many songs a
 * selection holds, and the playlist the songs were opened from ("Remove from Late night").
 */
@Immutable
data class MenuSubject(
    val kind: MenuKind,
    val title: String,
    val subtitle: String? = null,
    val meta: String? = null,
    val coverArt: String? = null,
    val loved: Tick = Tick.Off,
    val lovedCount: Int = 0,
    val rating: Int? = 0,
    val downloaded: Boolean = false,
    val downloadNote: String? = null,
    val trackId: String? = null,
    val albumId: String? = null,
    val album: String? = null,
    val artistId: String? = null,
    val artist: String? = null,
    val count: Int = 1,
    val context: String? = null,
)

/** One row of a menu's lists. [key] names it for tests (the action id for registry rows). */
sealed interface MenuEntry {
    val key: String

    /** A registry action shown as itself (its label, or the context's "Remove from Late night"). */
    data class Action(val action: ActionDescriptor) : MenuEntry { override val key get() = action.id }

    /** Download / Downloaded: runs `download`, or `unpin` once downloaded. */
    data class Download(val download: ActionDescriptor?, val remove: ActionDescriptor?) : MenuEntry { override val key get() = "download" }

    /** Pin to Library / Pinned: runs `pinToLibrary`, or `unpinFromLibrary` once pinned. */
    data class LibraryPin(val pin: ActionDescriptor?, val unpin: ActionDescriptor?) : MenuEntry { override val key get() = "pin" }

    /** "Go to album · Halfway Home" / "Go to artist · Low Lanterns". */
    data class GoTo(val action: ActionDescriptor) : MenuEntry { override val key get() = action.id }

    /** Song info / Album info. */
    data object Info : MenuEntry { override val key get() = "info" }

    /** The rating dialog, for items rated without the star row (an album's rating). */
    data object Rate : MenuEntry { override val key get() = "rate" }

    /** A row the context adds (the player's sleep timer, a playlist's remove). */
    data class Extra(val extra: SongMenuExtra) : MenuEntry { override val key get() = extra.testTag ?: extra.label }
}

/** Love and Unlove, for the header's toggle (it runs the one matching the item's state). */
data class LoveToggle(val love: ActionDescriptor?, val unlove: ActionDescriptor?)

/**
 * Where everything goes: the header's love toggle, the play group, whether the star row shows,
 * the short list under it ([primary], then "More"), what More reveals ([more]) and, last, the
 * destructive rows.
 */
data class MenuLayout(
    val love: LoveToggle?,
    val play: List<ActionDescriptor>,
    val rating: Boolean,
    val primary: List<MenuEntry>,
    val more: List<MenuEntry>,
    val destructive: List<MenuEntry>,
) {
    /** What "More" reveals: its count ("4 more"). */
    val hidden: Int get() = more.size + destructive.size
}

/** The play group, in its fixed order. */
private val PLAY_IDS = listOf(ActionIds.PLAY, ActionIds.PLAY_SHUFFLED, ActionIds.PLAY_NEXT, ActionIds.PLAY_LATER)

/** The registry's rate entries: the star row (or the Rate row) replaces them. */
private val RATE_IDS = (0..5).map { ActionIds.rate(it) }.toSet() + ActionIds.RATE

/** Registry actions that only apply in one context (the queue, a playlist): shown only when the core enables them. */
private val CONTEXT_IDS = setOf(ActionIds.REMOVE_FROM_QUEUE, ActionIds.REMOVE_FROM_PLAYLIST)

/** Ui-handled ids a menu cannot perform (navigation, player-only sheets): never listed. */
private val UNPERFORMABLE = ActionIds.UI_HANDLED - setOf(ActionIds.ADD_TO_PLAYLIST, ActionIds.RATE, ActionIds.GO_TO_ALBUM, ActionIds.GO_TO_ARTIST)

/**
 * Lays [actions] (the registry's, for the target) out per the menu designs. Song: Play, Shuffle,
 * Play next, Play later; the star row; Add to playlist, Download, Go to album, the context's extras;
 * More: Pin to Library, Go to artist, Song info; then the destructive rows (Remove from Late night).
 * Album: the same without the stars, Go to artist in the short list and Album info under More.
 * Artist and playlist: no go-to rows. Several songs: no go-to, pin or info. Availability and
 * enabled state stay the core's; any registry action the designs don't place goes under More (or,
 * destructive, last), so nothing is lost. A short list left empty takes More's rows.
 */
fun menuLayout(
    actions: List<ActionDescriptor>,
    kind: MenuKind,
    extras: List<SongMenuExtra> = emptyList(),
    canGoToAlbum: Boolean = false,
    canGoToArtist: Boolean = false,
    hasInfo: Boolean = false,
): MenuLayout {
    val byId = actions.associateBy { it.id }
    val replaced = extras.mapNotNull { it.replaces }.toSet()
    val used = mutableSetOf<String>()
    fun take(id: String): ActionDescriptor? { used += id; return byId[id] }
    val play = PLAY_IDS.mapNotNull(::take)
    val love = LoveToggle(take(ActionIds.LOVE), take(ActionIds.UNLOVE)).takeIf { it.love != null || it.unlove != null }
    val rateable = actions.any { it.id in RATE_IDS }
    used += RATE_IDS
    // One song always has its stars (as the player does); several, where the registry rates them.
    val stars = kind == MenuKind.Song || (rateable && kind == MenuKind.Songs)
    val addTo = take(ActionIds.ADD_TO_PLAYLIST)?.let { MenuEntry.Action(it) }
    val download = MenuEntry.Download(take(ActionIds.DOWNLOAD), take(ActionIds.UNPIN)).takeIf { it.download != null || it.remove != null }
    val pin = MenuEntry.LibraryPin(take(ActionIds.PIN_TO_LIBRARY), take(ActionIds.UNPIN_FROM_LIBRARY)).takeIf { it.pin != null || it.unpin != null }
    val goAlbum = take(ActionIds.GO_TO_ALBUM)?.takeIf { canGoToAlbum }?.let { MenuEntry.GoTo(it) }
    val goArtist = take(ActionIds.GO_TO_ARTIST)?.takeIf { canGoToArtist }?.let { MenuEntry.GoTo(it) }
    val info = MenuEntry.Info.takeIf { hasInfo }
    val context = CONTEXT_IDS.mapNotNull { id -> take(id)?.takeIf { it.enabled && id !in replaced } }
    val (destructiveExtras, otherExtras) = extras.partition { it.destructive }
    val rest = actions.filter { it.id !in used && it.id !in UNPERFORMABLE }
    val (restDestructive, restOther) = rest.partition { it.destructive }
    var primary = when (kind) {
        MenuKind.Song -> listOfNotNull(addTo, download, goAlbum)
        MenuKind.Album -> listOfNotNull(addTo, download, goArtist)
        else -> listOfNotNull(addTo, download)
    } + otherExtras.map { MenuEntry.Extra(it) }
    var more = when (kind) {
        MenuKind.Song -> listOfNotNull(pin, goArtist, info)
        MenuKind.Album -> listOfNotNull(pin, info)
        MenuKind.Songs -> emptyList()
        else -> listOfNotNull(pin, goArtist, info)
    } + listOfNotNull(MenuEntry.Rate.takeIf { rateable && !stars }) + restOther.map { MenuEntry.Action(it) }
    if (primary.isEmpty()) { primary = more; more = emptyList() }
    val destructive = context.map { MenuEntry.Action(it) } + restDestructive.map { MenuEntry.Action(it) } + destructiveExtras.map { MenuEntry.Extra(it) }
    return MenuLayout(love, play, stars, primary, more, destructive)
}

/** A list row's fill: a step lighter than the sheet in both themes, so the connected groups read as tiles. */
@Composable
fun menuRowColor(): Color = MaterialTheme.colorScheme.let { if (it.surface.luminance() > 0.5f) it.surfaceContainerLowest else it.surfaceContainerHighest }

/** A row's shape in an M3 Expressive connected list: [outer] corners at the group's ends, [inner] between rows. */
fun connectedShape(index: Int, count: Int, outer: Dp = 20.dp, inner: Dp = 6.dp): Shape {
    val top = if (index == 0) outer else inner
    val bottom = if (index == count - 1) outer else inner
    return RoundedCornerShape(topStart = top, topEnd = top, bottomStart = bottom, bottomEnd = bottom)
}

/**
 * The compact menu for a song, an album, an artist, a playlist or several songs: a sheet that opens
 * short (header with the love toggle, the connected Play / Shuffle / Play next / Play later group,
 * the star row for songs, a short connected list ending in "More"), and grows to the rest when More,
 * the handle or an upward drag asks; "Show less" shrinks it again. Toggles (love, rating, download,
 * pin) apply in place; every other row closes the sheet with its hide animation, its action or
 * navigation starting at once behind it. [onRan] follows any action that ran (a selection clears).
 * [extraTop] sits under the header (a Library pin's controls). Tags: `[tag]`, `[tag].header`,
 * `[tag].action.<id>`, `[tag].download`, `[tag].pin`, `[tag].info`, `[tag].more`, `rating`.
 */
@Composable
fun MenuSheet(
    subject: MenuSubject,
    target: ActionTarget,
    actions: List<ActionDescriptor>,
    onDismiss: () -> Unit,
    onAddToPlaylist: () -> Unit,
    extras: List<SongMenuExtra> = emptyList(),
    onGoToAlbum: (() -> Unit)? = null,
    onGoToArtist: (() -> Unit)? = null,
    onInfo: (() -> Unit)? = null,
    onRan: () -> Unit = {},
    extraTop: (@Composable () -> Unit)? = null,
    tag: String = "songMenu",
) {
    val client = LocalCoreClient.current
    val sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = true)
    val scope = rememberCoroutineScope()
    /** Hides the sheet, then runs [after] (which may open what follows) and reports it gone. */
    fun hideThen(after: () -> Unit = {}) {
        scope.launch { sheetState.hide() }.invokeOnCompletion { after(); onDismiss() }
    }
    val layout = remember(actions, subject.kind, extras, onGoToAlbum != null, onGoToArtist != null, onInfo != null) {
        menuLayout(actions, subject.kind, extras, canGoToAlbum = onGoToAlbum != null, canGoToArtist = onGoToArtist != null, hasInfo = onInfo != null)
    }
    var expanded by rememberSaveable { mutableStateOf(false) }
    var loved by remember { mutableStateOf(subject.loved) }
    var rating by remember { mutableStateOf(subject.rating) }
    var downloaded by remember { mutableStateOf(subject.downloaded) }
    val pinEntry = (layout.more + layout.primary).filterIsInstance<MenuEntry.LibraryPin>().firstOrNull()
    var pinned by remember { mutableStateOf(pinEntry?.unpin?.enabled == true) }
    var rateDialog by remember { mutableStateOf(false) }
    val size by produceState<Double?>(initialValue = null, subject.trackId) {
        value = subject.trackId?.let { (client.query(Queries.track(it)) as? QueryResult.TrackDetail)?.data?.sizeBytes }
    }
    fun run(a: ActionDescriptor) = client.dispatch(Commands.runAction(a.id, target))
    fun runAndClose(a: ActionDescriptor) { if (a.id !in ActionIds.UI_HANDLED) run(a); hideThen(onRan) }
    val canExpand = layout.hidden > 0
    val canExpandNow by rememberUpdatedState(canExpand)
    // How far the rest of the menu is open, 0 to 1. A drag moves it with the finger; More, Show less
    // and the handle animate it to where [expanded] says.
    val openness = remember { Animatable(if (expanded) 1f else 0f) }
    /** The rest of the menu's full height, measured each layout (it is always laid out, clipped). */
    val restHeight = remember { intArrayOf(0) }
    val settleSpec = remember { spring<Float>(dampingRatio = Spring.DampingRatioNoBouncy, stiffness = Spring.StiffnessMediumLow) }
    LaunchedEffect(expanded) {
        val target = if (expanded) 1f else 0f
        // A settling drag has already set off towards it, with the finger's speed.
        if (openness.targetValue != target) openness.animateTo(target, settleSpec)
    }
    val showingRest by remember { derivedStateOf { openness.value > 0f } }
    val showingMore by remember { derivedStateOf { openness.value < 1f } }
    // A drag the list can't take follows the finger: up opens the rest of the menu, down (with the
    // list at its top) closes it, and only then does the sheet itself move. Letting go settles it
    // open or closed: a flick goes its way, otherwise whichever is nearer.
    val followDrag = remember {
        object : NestedScrollConnection {
            var dragging = false
            var at = 0f

            override fun onPostScroll(consumed: Offset, available: Offset, source: NestedScrollSource): Offset {
                val height = restHeight[0]
                if (source != NestedScrollSource.UserInput || !canExpandNow || height <= 0) return Offset.Zero
                if (!dragging) at = openness.value
                val dy = available.y
                if (!((dy < 0f && at < 1f) || (dy > 0f && at > 0f))) return Offset.Zero
                val next = (at - dy / height).coerceIn(0f, 1f)
                val used = (at - next) * height
                at = next
                dragging = true
                scope.launch { openness.snapTo(next) }
                return Offset(0f, used)
            }

            override suspend fun onPreFling(available: Velocity): Velocity {
                if (!dragging) return Velocity.Zero
                dragging = false
                val v = available.y
                val open = when {
                    v < -FLICK_VELOCITY -> true
                    v > FLICK_VELOCITY -> false
                    else -> at > 0.5f
                }
                expanded = open
                val height = restHeight[0].coerceAtLeast(1)
                openness.animateTo(if (open) 1f else 0f, settleSpec, initialVelocity = -v / height)
                // Taken whole: neither the list nor the sheet flings on from it.
                return available
            }
        }
    }

    @Composable
    fun EntryRow(entry: MenuEntry, shape: Shape) {
        when (entry) {
            is MenuEntry.Action -> {
                val a = entry.action
                val label = when (a.id) {
                    ActionIds.REMOVE_FROM_PLAYLIST -> subject.context?.let { ctx -> if (subject.count > 1) stringResource(R.string.menu_remove_n_from, subject.count, ctx) else stringResource(R.string.menu_remove_from, ctx) } ?: a.label
                    ActionIds.REMOVE_FROM_QUEUE -> if (subject.count > 1) stringResource(R.string.menu_remove_n_from, subject.count, stringResource(R.string.menu_queue)) else stringResource(R.string.action_remove_from_queue)
                    ActionIds.ADD_TO_PLAYLIST -> stringResource(R.string.action_add_to_playlist)
                    else -> a.label
                }
                val icon = if (a.id == ActionIds.ADD_TO_PLAYLIST) HocketIcons.AutoMirrored.Filled.PlaylistAdd else actionIcon(a.icon)
                MenuRow(AnnotatedString(label), icon, shape, enabled = a.enabled, destructive = a.destructive, tag = "$tag.action.${a.id}",
                    trailing = if (a.id == ActionIds.ADD_TO_PLAYLIST) { { Icon(HocketIcons.Filled.ChevronRight, null, Modifier.size(20.dp), tint = MaterialTheme.colorScheme.onSurfaceVariant) } } else null,
                ) {
                    if (a.id == ActionIds.ADD_TO_PLAYLIST) hideThen(onAddToPlaylist) else runAndClose(a)
                }
            }
            is MenuEntry.Download -> {
                val label = when {
                    subject.kind == MenuKind.Songs -> pluralStringResource(if (downloaded) R.plurals.menu_downloaded_songs else R.plurals.menu_download_songs, subject.count, subject.count)
                    downloaded -> stringResource(R.string.menu_downloaded)
                    else -> stringResource(R.string.action_download)
                }
                val action = if (downloaded) entry.remove else entry.download
                val note = subject.downloadNote ?: size?.takeIf { it > 0 }?.let { formatBytes(it) }
                MenuRow(AnnotatedString(label), if (downloaded) HocketIcons.Filled.DownloadDone else HocketIcons.Filled.Download, shape, enabled = action?.enabled == true, tag = "$tag.download",
                    trailing = {
                        // Reserved either way, so the label never shifts as the state flips.
                        Box(Modifier.width(64.dp), contentAlignment = Alignment.CenterEnd) {
                            if (downloaded) Icon(HocketIcons.Filled.Check, null, Modifier.size(20.dp), tint = MaterialTheme.colorScheme.primary)
                            else note?.let { Text(it, style = MaterialTheme.typography.labelLarge, color = MaterialTheme.colorScheme.onSurfaceVariant, maxLines = 1) }
                        }
                    },
                ) { action?.let { run(it); downloaded = !downloaded } }
            }
            is MenuEntry.LibraryPin -> {
                val action = if (pinned) entry.unpin else entry.pin
                MenuRow(AnnotatedString(stringResource(if (pinned) R.string.menu_pinned else R.string.library_pin_add)), HocketIcons.Filled.PushPin, shape, enabled = action?.enabled == true, tag = "$tag.pin",
                    trailing = { Box(Modifier.size(20.dp)) { if (pinned) Icon(HocketIcons.Filled.Check, null, Modifier.size(20.dp), tint = MaterialTheme.colorScheme.primary) } },
                ) { action?.let { run(it); pinned = !pinned } }
            }
            is MenuEntry.GoTo -> {
                val album = entry.action.id == ActionIds.GO_TO_ALBUM
                val name = if (album) subject.album else subject.artist
                val label = buildAnnotatedString {
                    append(stringResource(if (album) R.string.action_go_to_album else R.string.action_go_to_artist))
                    name?.let { withStyle(SpanStyle(color = MaterialTheme.colorScheme.onSurfaceVariant)) { append(stringResource(R.string.dot_separator)); append(it) } }
                }
                MenuRow(label, if (album) HocketIcons.Filled.Album else HocketIcons.Filled.Person, shape, enabled = entry.action.enabled, tag = "$tag.action.${entry.action.id}") {
                    (if (album) onGoToAlbum else onGoToArtist)?.invoke()
                    hideThen()
                }
            }
            MenuEntry.Info -> MenuRow(AnnotatedString(stringResource(if (subject.kind == MenuKind.Album) R.string.menu_album_info else R.string.menu_song_info)), HocketIcons.Filled.Info, shape, tag = "$tag.info") { hideThen { onInfo?.invoke() } }
            MenuEntry.Rate -> MenuRow(AnnotatedString(stringResource(R.string.action_rate)), HocketIcons.Filled.Star, shape, tag = "$tag.rate") { rateDialog = true }
            is MenuEntry.Extra -> {
                val e = entry.extra
                MenuRow(AnnotatedString(e.label), e.icon, shape, supporting = e.supporting, destructive = e.destructive, highlighted = e.highlighted, tag = e.testTag) { hideThen(e.onClick) }
            }
        }
    }

    ModalBottomSheet(
        onDismissRequest = onDismiss,
        sheetState = sheetState,
        dragHandle = { MenuHandle(expanded, canExpand) { expanded = !expanded } },
        modifier = Modifier.testTag(tag),
    ) {
        Column(Modifier.nestedScroll(followDrag).verticalScroll(rememberScrollState()).navigationBarsPadding().padding(bottom = 24.dp)) {
            MenuHeader(subject, layout.love, loved, tag) { love, unlove ->
                if (loved == Tick.On) { unlove?.let(::run); loved = Tick.Off } else { love?.let(::run); loved = Tick.On }
            }
            extraTop?.let { it(); androidx.compose.foundation.layout.Spacer(Modifier.height(8.dp)) }
            if (layout.play.isNotEmpty()) PlayGroup(layout.play, tag) { runAndClose(it) }
            if (layout.rating) {
                RatingRow(rating) { stars ->
                    rating = stars
                    client.dispatch(Commands.runAction(ActionIds.rate(stars), target))
                }
            }
            val withMore = canExpand && showingMore
            val primaryRows = layout.primary.size + if (withMore) 1 else 0
            Column(Modifier.fillMaxWidth().padding(start = 16.dp, end = 16.dp, top = if (layout.rating) 8.dp else 16.dp), verticalArrangement = Arrangement.spacedBy(2.dp)) {
                layout.primary.forEachIndexed { i, entry -> EntryRow(entry, connectedShape(i, primaryRows)) }
                if (withMore) {
                    // Folds away as the rest opens.
                    Box(Modifier.revealed { 1f - openness.value }) {
                        MoreRow(layout.hidden, connectedShape(primaryRows - 1, primaryRows), tag) { expanded = true }
                    }
                }
            }
            if (canExpand) {
                // Always laid out (so a drag knows its full height), shown as far as it is open; closed,
                // it is out of the semantics tree too.
                Column(
                    Modifier
                        .fillMaxWidth()
                        .revealed(onMeasured = { restHeight[0] = it }) { openness.value }
                        .then(if (showingRest) Modifier else Modifier.clearAndSetSemantics {}),
                ) {
                    if (layout.more.isNotEmpty()) Column(Modifier.fillMaxWidth().padding(start = 16.dp, end = 16.dp, top = 12.dp), verticalArrangement = Arrangement.spacedBy(2.dp)) {
                        layout.more.forEachIndexed { i, entry -> EntryRow(entry, connectedShape(i, layout.more.size)) }
                    }
                    if (layout.destructive.isNotEmpty()) Column(Modifier.fillMaxWidth().padding(start = 16.dp, end = 16.dp, top = 12.dp), verticalArrangement = Arrangement.spacedBy(2.dp)) {
                        layout.destructive.forEachIndexed { i, entry -> EntryRow(entry, connectedShape(i, layout.destructive.size)) }
                    }
                    Box(Modifier.fillMaxWidth().padding(top = 18.dp), contentAlignment = Alignment.Center) {
                        TextButton(onClick = { expanded = false }, modifier = Modifier.heightIn(min = 48.dp).testTag("$tag.showLess")) {
                            Icon(HocketIcons.Filled.KeyboardArrowDown, null, Modifier.size(20.dp).rotate(180f))
                            Text(stringResource(R.string.menu_show_less), style = MaterialTheme.typography.titleSmall, modifier = Modifier.padding(start = 8.dp))
                        }
                    }
                }
            }
        }
    }
    if (rateDialog) {
        RatingDialog(current = 0, onRate = { stars -> client.dispatch(Commands.runAction(ActionIds.rate(stars), target)); rateDialog = false; hideThen(onRan) }, onDismiss = { rateDialog = false }, offerClear = true)
    }
}

/** Past this speed (px/s) a released drag finishes the way it was going. */
private const val FLICK_VELOCITY = 1200f

/**
 * Shows [fraction] (0 to 1, read at layout) of the content's full height from the top, clipped and
 * faded with it; [onMeasured] gets the full height. Reading the fraction only at layout and draw
 * keeps a drag from recomposing the menu every frame.
 */
private fun Modifier.revealed(onMeasured: (Int) -> Unit = {}, fraction: () -> Float): Modifier = this
    .clipToBounds()
    .layout { measurable, constraints ->
        val placeable = measurable.measure(constraints.copy(minHeight = 0, maxHeight = Constraints.Infinity))
        onMeasured(placeable.height)
        layout(placeable.width, (placeable.height * fraction().coerceIn(0f, 1f)).roundToInt()) { placeable.placeRelative(0, 0) }
    }
    .graphicsLayer { alpha = fraction().coerceIn(0f, 1f) }

/**
 * The sheet's handle: a 96 × 36 dp target that grows or shrinks the menu, drawn as the usual pill.
 * Its label and expand / collapse actions merge into the sheet's own handle node (a separate
 * clickable here would leave that node unlabelled); the tap itself is a pointer gesture.
 */
@Composable
private fun MenuHandle(expanded: Boolean, canExpand: Boolean, onToggle: () -> Unit) {
    val label = stringResource(if (!canExpand) R.string.menu_handle else if (expanded) R.string.menu_show_less else R.string.menu_more)
    val toggle by rememberUpdatedState(onToggle)
    Box(
        Modifier
            .padding(top = 4.dp)
            .width(96.dp)
            .height(36.dp)
            .clip(RoundedCornerShape(18.dp))
            .then(if (canExpand) Modifier.pointerInput(Unit) { detectTapGestures { toggle() } } else Modifier)
            .semantics {
                contentDescription = label
                if (canExpand) { if (expanded) collapse { toggle(); true } else expand { toggle(); true } }
            },
        contentAlignment = Alignment.Center,
    ) {
        Box(Modifier.size(width = 32.dp, height = 4.dp).clip(RoundedCornerShape(2.dp)).background(MaterialTheme.colorScheme.onSurfaceVariant.copy(alpha = 0.6f)))
    }
}

/** Artwork, the title (one line) with the lines under it, and the love toggle. */
@Composable
private fun MenuHeader(subject: MenuSubject, love: LoveToggle?, loved: Tick, tag: String, onLove: (ActionDescriptor?, ActionDescriptor?) -> Unit) {
    Row(Modifier.fillMaxWidth().padding(start = 20.dp, end = 12.dp, bottom = 14.dp).testTag("$tag.header"), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(14.dp)) {
        Artwork(subject.coverArt, ArtworkSizes.THUMB, null, Modifier.size(64.dp), RoundedCornerShape(16.dp))
        Column(Modifier.weight(1f)) {
            Text(subject.title, style = MaterialTheme.typography.titleLarge.copy(fontSize = 19.sp, lineHeight = 24.sp), fontWeight = FontWeight.SemiBold, maxLines = 1, overflow = TextOverflow.Ellipsis, modifier = Modifier.semantics { heading() })
            subject.subtitle?.let { Text(it, style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant, maxLines = 1, overflow = TextOverflow.Ellipsis) }
            subject.meta?.let { Text(it, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant, maxLines = 1, overflow = TextOverflow.Ellipsis) }
        }
        if (love != null) {
            val action = if (loved == Tick.On) love.unlove else love.love
            LoveButton(loved, subject.lovedCount, subject.count, enabled = action?.enabled == true, tag = "$tag.love") { onLove(love.love, love.unlove) }
        }
    }
}

/** Love as a toggle: filled when loved, outlined when not, half filled when only some of several songs are. */
@Composable
private fun LoveButton(state: Tick, lovedCount: Int, total: Int, enabled: Boolean, tag: String, onToggle: () -> Unit) {
    val label = stringResource(R.string.action_love)
    val spoken = when (state) {
        Tick.On -> stringResource(R.string.menu_loved)
        Tick.Mixed -> stringResource(R.string.menu_loved_some, lovedCount, total)
        Tick.Off -> null
    }
    val primary = MaterialTheme.colorScheme.primary
    Box(
        Modifier
            .size(48.dp)
            .clip(CircleShape)
            .background(if (state == Tick.On) MaterialTheme.colorScheme.secondaryContainer else Color.Transparent)
            .triStateToggleable(
                state = when (state) { Tick.On -> ToggleableState.On; Tick.Off -> ToggleableState.Off; Tick.Mixed -> ToggleableState.Indeterminate },
                enabled = enabled,
                role = Role.Checkbox,
                onClick = onToggle,
            )
            .semantics { contentDescription = label; spoken?.let { stateDescription = it } }
            .testTag(tag),
        contentAlignment = Alignment.Center,
    ) {
        when (state) {
            Tick.On -> Icon(HocketIcons.Filled.Favorite, null, tint = primary)
            Tick.Off -> Icon(HocketIcons.Filled.FavoriteBorder, null, tint = MaterialTheme.colorScheme.onSurfaceVariant)
            Tick.Mixed -> Box {
                Icon(HocketIcons.Filled.FavoriteBorder, null, tint = primary)
                // The left half filled: some, not all, of the songs are loved.
                Icon(HocketIcons.Filled.Favorite, null, tint = primary, modifier = Modifier.drawWithContent { clipRect(right = size.width / 2f) { this@drawWithContent.drawContent() } })
            }
        }
    }
}

/** Play / Shuffle / Play next / Play later as one connected group, Play filled and wider. */
@Composable
private fun PlayGroup(play: List<ActionDescriptor>, tag: String, onRun: (ActionDescriptor) -> Unit) {
    val groupLabel = stringResource(R.string.menu_play_group)
    Row(Modifier.fillMaxWidth().padding(horizontal = 16.dp).semantics { isTraversalGroup = true; contentDescription = groupLabel }, horizontalArrangement = Arrangement.spacedBy(3.dp)) {
        play.forEachIndexed { i, a ->
            val lead = if (i == 0) 22.dp else 8.dp
            val trail = if (i == play.lastIndex) 22.dp else 8.dp
            val main = a.id == ActionIds.PLAY
            val (label, icon) = when (a.id) {
                ActionIds.PLAY -> stringResource(R.string.action_play) to HocketIcons.Filled.PlayArrow
                ActionIds.PLAY_SHUFFLED -> stringResource(R.string.action_shuffle) to HocketIcons.Filled.Shuffle
                ActionIds.PLAY_NEXT -> stringResource(R.string.action_play_next) to HocketIcons.Filled.PlaylistPlay
                else -> stringResource(R.string.action_play_later) to HocketIcons.Filled.QueueMusic
            }
            Surface(
                onClick = { onRun(a) },
                enabled = a.enabled,
                shape = RoundedCornerShape(topStart = lead, bottomStart = lead, topEnd = trail, bottomEnd = trail),
                color = if (main) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.secondaryContainer,
                contentColor = if (main) MaterialTheme.colorScheme.onPrimary else MaterialTheme.colorScheme.onSecondaryContainer,
                modifier = Modifier.weight(if (main) 1.35f else 1f).heightIn(min = 76.dp).testTag("$tag.action.${a.id}"),
            ) {
                Column(Modifier.padding(horizontal = 4.dp, vertical = 10.dp).alpha(if (a.enabled) 1f else 0.38f), horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.spacedBy(6.dp, Alignment.CenterVertically)) {
                    Icon(icon, null, Modifier.size(if (main) 26.dp else 22.dp))
                    Text(label, style = if (main) MaterialTheme.typography.labelLarge else MaterialTheme.typography.labelMedium.copy(fontSize = 13.sp),
                        fontWeight = if (main) FontWeight.SemiBold else FontWeight.Medium, textAlign = TextAlign.Center, maxLines = 2, overflow = TextOverflow.Ellipsis)
                }
            }
        }
    }
}

/** "Rating" and the stars (tap the current star to clear); several songs rated differently read "Mixed". */
@Composable
private fun RatingRow(rating: Int?, onRate: (Int) -> Unit) {
    val mixed = stringResource(R.string.menu_rating_mixed)
    Row(Modifier.fillMaxWidth().padding(start = 20.dp, end = 16.dp, top = 10.dp, bottom = 2.dp), verticalAlignment = Alignment.CenterVertically) {
        // The stars speak for themselves ("Rating, 4 of 5 stars"): the label is for the eye only.
        Row(Modifier.weight(1f).clearAndSetSemantics { }, verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            Text(stringResource(R.string.rating_stars), style = MaterialTheme.typography.labelLarge, color = MaterialTheme.colorScheme.onSurfaceVariant)
            if (rating == null) {
                Surface(shape = RoundedCornerShape(10.dp), color = MaterialTheme.colorScheme.surfaceContainerHighest) {
                    Row(Modifier.padding(horizontal = 8.dp, vertical = 2.dp), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(4.dp)) {
                        Icon(HocketIcons.Filled.Remove, null, Modifier.size(12.dp))
                        Text(mixed, style = MaterialTheme.typography.labelSmall, fontWeight = FontWeight.SemiBold)
                    }
                }
            }
        }
        RatingStars(rating ?: 0, onRate = onRate, starSize = 28.dp, spacing = 14.dp, stateOverride = if (rating == null) mixed else null)
    }
}

/** "More", the short list's last row: grows the sheet to the rest ("4 more"). */
@Composable
private fun MoreRow(hidden: Int, shape: Shape, tag: String, onClick: () -> Unit) {
    val primary = MaterialTheme.colorScheme.primary
    MenuRow(AnnotatedString(stringResource(R.string.menu_more)), HocketIcons.Filled.KeyboardArrowDown, shape, tint = primary, labelColor = primary, labelWeight = FontWeight.Medium, tag = "$tag.more",
        trailing = { Text(stringResource(R.string.menu_more_count, hidden), style = MaterialTheme.typography.labelLarge, color = MaterialTheme.colorScheme.onSurfaceVariant) },
        onClick = onClick)
}

/** One tile of a connected list: icon, label (one line, with an optional second), trailing content. */
@Composable
private fun MenuRow(
    label: AnnotatedString,
    icon: ImageVector,
    shape: Shape,
    supporting: String? = null,
    enabled: Boolean = true,
    destructive: Boolean = false,
    highlighted: Boolean = false,
    tint: Color? = null,
    labelColor: Color? = null,
    labelWeight: FontWeight? = null,
    tag: String? = null,
    trailing: (@Composable RowScope.() -> Unit)? = null,
    onClick: () -> Unit,
) {
    val scheme = MaterialTheme.colorScheme
    val iconTint = tint ?: when { destructive -> scheme.error; highlighted -> scheme.primary; else -> scheme.onSurfaceVariant }
    Surface(onClick = onClick, enabled = enabled, shape = shape, color = menuRowColor(), modifier = Modifier.fillMaxWidth().let { if (tag != null) it.testTag(tag) else it }) {
        Row(Modifier.heightIn(min = 52.dp).padding(horizontal = 16.dp, vertical = 4.dp).alpha(if (enabled) 1f else 0.38f), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(16.dp)) {
            Icon(icon, null, tint = iconTint)
            Column(Modifier.weight(1f)) {
                Text(label, style = MaterialTheme.typography.bodyLarge, color = labelColor ?: if (destructive) scheme.error else scheme.onSurface, fontWeight = labelWeight, maxLines = 1, overflow = TextOverflow.Ellipsis)
                supporting?.let { Text(it, style = MaterialTheme.typography.bodySmall, color = scheme.onSurfaceVariant, maxLines = 1, overflow = TextOverflow.Ellipsis) }
            }
            trailing?.invoke(this)
        }
    }
}
