package app.hocket.ui.components

import androidx.activity.compose.BackHandler
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.platform.LocalContext
import androidx.compose.material3.ExperimentalMaterial3ExpressiveApi
import androidx.compose.material3.LoadingIndicator
import androidx.activity.compose.PredictiveBackHandler
import androidx.compose.animation.AnimatedContent
import androidx.compose.animation.core.CubicBezierEasing
import androidx.compose.animation.core.SeekableTransitionState
import androidx.compose.animation.core.rememberTransition
import androidx.compose.animation.SizeTransform
import androidx.compose.animation.core.Animatable
import androidx.compose.animation.core.tween
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.slideInHorizontally
import androidx.compose.animation.slideOutHorizontally
import androidx.compose.animation.togetherWith
import androidx.compose.animation.core.Spring
import androidx.compose.animation.core.spring
import androidx.compose.foundation.background
import androidx.compose.foundation.focusGroup
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.IntrinsicSize
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.RowScope
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.imePadding
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
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.Immutable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.derivedStateOf
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.produceState
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import app.hocket.core.api.SnoozeFor
import java.text.DateFormat
import java.util.Date
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.clipToBounds
import androidx.compose.ui.focus.focusProperties
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
import app.hocket.core.MenuItemIds
import app.hocket.core.api.MenuScreen
import app.hocket.core.api.MenuSection
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.wrapContentHeight
import app.hocket.core.Commands
import app.hocket.core.api.Command
import app.hocket.core.Queries
import app.hocket.core.api.ActionDescriptor
import app.hocket.core.api.ActionTarget
import app.hocket.core.api.QueryResult
import app.hocket.core.api.SleepTimer
import app.hocket.share.ShareActions
import app.hocket.share.ShareChoice
import app.hocket.share.ShareRequest
import app.hocket.share.shareChoices
import app.hocket.ui.LocalCoreClient
import app.hocket.ui.icons.HocketIcons
import kotlinx.coroutines.launch
import kotlin.math.roundToInt

/** The core's item menu (`Query.Menu`): its rows in display order. */
typealias CoreMenu = app.hocket.core.api.MenuLayout

/** What a menu is for: its header and which design it follows. [Songs] is a multi-selection. */
enum class MenuKind { Song, Album, Artist, Playlist, Songs, Other }

/**
 * The item a menu acts on, as its header and toggles show it: title and lines under it, artwork,
 * loved (several songs can be partly loved), rating (null: several songs rated differently,
 * "Mixed"), downloaded and the download row's note ("4.2 MB", "12 songs"; a song's size is fetched
 * by [trackId] when not given), the album and artist the go-to rows name (a song's [artists] when
 * it has several: Go to artist then asks which), how many songs a selection holds, and the playlist
 * the songs were opened from ("Remove from Late night").
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
    /** The account [trackId] is valid in (its reference's source). */
    val trackSource: String? = null,
    val albumId: String? = null,
    val album: String? = null,
    val artistId: String? = null,
    val artist: String? = null,
    /** A song's artists, in the server's order (only those in the library can be gone to). */
    val artists: List<app.hocket.ui.player.TrackArtist> = emptyList(),
    val count: Int = 1,
    val context: String? = null,
) {
    /** The song the menu is for, as a reference. */
    val trackRef: app.hocket.core.api.TrackRef? get() = trackId?.let { app.hocket.core.api.TrackRef(trackSource.orEmpty(), it) }

    /** The artists Go to artist can open: the song's linked artists, or the one [artistId]. */
    val artistChoices: List<Pair<String, String>> get() =
        artists.mapNotNull { a -> a.id?.let { it to a.name } }.ifEmpty { listOfNotNull(artistId?.let { it to (artist ?: "") }) }
}

/** One row of a menu's lists. [key] names it for tests (the menu item's id for the core's rows). */
sealed interface MenuEntry {
    val key: String

    /** A registry action shown as itself (its label, or the context's "Remove from Late night"). */
    data class Action(val action: ActionDescriptor) : MenuEntry { override val key get() = action.id }

    /** Download / Downloaded: runs `download`, or `unpin` once downloaded. */
    data class Download(val download: ActionDescriptor?, val remove: ActionDescriptor?) : MenuEntry { override val key get() = "download" }

    /** Pin to Library / Pinned: runs `pinToLibrary`, or `unpinFromLibrary` once pinned. */
    data class LibraryPin(val pin: ActionDescriptor?, val unpin: ActionDescriptor?) : MenuEntry { override val key get() = "pin" }

    /** "Go to album · Halfway Home" / "Go to artist · Low Lanterns" (several artists: a sub-page of them). */
    data class GoTo(val action: ActionDescriptor) : MenuEntry { override val key get() = action.id }

    /** Song info / Album info. */
    data object Info : MenuEntry { override val key get() = "info" }

    /** A row the context adds (the player's sleep timer, a playlist's remove). */
    data class Extra(val extra: SongMenuExtra) : MenuEntry { override val key get() = extra.testTag ?: extra.label }

    /** Share: drills into Navidrome, Squigly and (one song) the file. */
    data object Share : MenuEntry { override val key get() = "share" }

    /** Open in Genius: a search for this one song, on the menu itself. */
    data object Genius : MenuEntry { override val key get() = "genius" }
}

/** Love and Unlove, for the header's toggle (it runs the one matching the item's state). */
data class LoveToggle(val love: ActionDescriptor?, val unlove: ActionDescriptor?)

/**
 * Where everything goes on the phone: the header's love toggle, the play group, whether the star
 * row shows, the short list under it ([primary], then "More"), what More reveals ([more]) and,
 * always shown after a gap, the rows that remove or delete ([danger]).
 */
data class SheetLayout(
    val love: LoveToggle?,
    val play: List<ActionDescriptor>,
    val rating: Boolean,
    val primary: List<MenuEntry>,
    val more: List<MenuEntry>,
    val danger: List<MenuEntry>,
) {
    /** What "More" reveals: its count ("4 more"). */
    val hidden: Int get() = more.size
}

/**
 * The app-handled rows a menu can offer (the query's `app_items`): Share where [shareChoices] has
 * any, the info row with [hasInfo], Open in Genius for one song, and the extras that are app items
 * ([SongMenuExtra.item], the player's Lyrics timing).
 */
fun menuAppItems(kind: MenuKind, count: Int, hasInfo: Boolean, extras: List<SongMenuExtra> = emptyList()): List<String> = buildList {
    if (shareChoices(kind, count).any) add(MenuItemIds.SHARE)
    if (hasInfo) add(MenuItemIds.INFO)
    if (kind == MenuKind.Song && count <= 1) add(MenuItemIds.GENIUS)
    extras.mapNotNullTo(this) { it.item }
}

/** How many of [extras] top the short list (the query's `extras`): not the removing ones, those under More, nor app items the core places. */
fun menuExtraCount(extras: List<SongMenuExtra>): Int = extras.count { it.item == null && !it.destructive && !it.underMore }

/** The core's menu ([CoreClient.menu]) for [target], asked with what this app can offer there. */
suspend fun app.hocket.core.client.CoreClient.itemMenu(
    target: ActionTarget,
    kind: MenuKind,
    count: Int = 1,
    screen: MenuScreen = MenuScreen.List,
    hasInfo: Boolean = false,
    extras: List<SongMenuExtra> = emptyList(),
): CoreMenu = menu(target, screen, menuAppItems(kind, count, hasInfo, extras), menuExtraCount(extras))

/**
 * Lays the core's [menu] (rows in order, each folded or not: `Query.Menu`) out on the sheet. The
 * play strip, then the stars (with a `rating` row) and the header's heart (with `love`); the short
 * list is the context's extras, then the unfolded rows; More holds the extras that are
 * [SongMenuExtra.underMore], then the folded rows; the rows that
 * remove or delete (the core's, then the context's destructive extras) come last, never folded. An
 * extra that [SongMenuExtra.replaces] a row stands in for it; one that is an app [SongMenuExtra.item]
 * sits where the core put that item. Go-to rows need somewhere to go ([canGoToAlbum],
 * [canGoToArtist]). Snooze shows Snooze or Unsnooze by [snoozed] (null, a selection: whichever the
 * core offers). A short list left empty takes More's rows.
 */
fun sheetLayout(
    menu: CoreMenu,
    extras: List<SongMenuExtra> = emptyList(),
    canGoToAlbum: Boolean = false,
    canGoToArtist: Boolean = false,
    snoozed: Boolean? = null,
    /** Whether the saved queue the menu is for is pinned (null: not a saved queue, or not known). */
    queuePinned: Boolean? = null,
): SheetLayout {
    val replaced = extras.mapNotNull { it.replaces }.toSet()
    val play = mutableListOf<ActionDescriptor>()
    var love: LoveToggle? = null
    var rating = false
    val primary = mutableListOf<MenuEntry>()
    val more = mutableListOf<MenuEntry>()
    val danger = mutableListOf<MenuEntry>()
    for (item in menu.items) {
        if (item.id in replaced || item.actions.any { it.id in replaced }) continue
        fun action(id: String) = item.actions.firstOrNull { it.id == id }
        when (item.section) {
            MenuSection.Play -> { play += item.actions; continue }
            MenuSection.Rate -> {
                when (item.id) {
                    MenuItemIds.RATING -> rating = true
                    else -> love = LoveToggle(action(ActionIds.LOVE), action(ActionIds.UNLOVE)).takeIf { it.love != null || it.unlove != null }
                }
                continue
            }
            else -> Unit
        }
        val entries: List<MenuEntry> = when (item.id) {
            ActionIds.PIN_TO_LIBRARY -> listOf(MenuEntry.LibraryPin(action(ActionIds.PIN_TO_LIBRARY), action(ActionIds.UNPIN_FROM_LIBRARY)))
            ActionIds.DOWNLOAD -> listOf(MenuEntry.Download(action(ActionIds.DOWNLOAD), action(ActionIds.UNPIN)))
            ActionIds.GO_TO_ALBUM -> listOfNotNull(action(ActionIds.GO_TO_ALBUM)?.takeIf { canGoToAlbum }?.let { MenuEntry.GoTo(it) })
            ActionIds.GO_TO_ARTIST -> listOfNotNull(action(ActionIds.GO_TO_ARTIST)?.takeIf { canGoToArtist }?.let { MenuEntry.GoTo(it) })
            ActionIds.SNOOZE -> when (snoozed) {
                true -> listOfNotNull(action(ActionIds.UNSNOOZE))
                false -> listOfNotNull(action(ActionIds.SNOOZE))
                null -> item.actions.filter { it.enabled }.ifEmpty { item.actions.take(1) }
            }.map { MenuEntry.Action(it) }
            MenuItemIds.SHARE -> listOf(MenuEntry.Share)
            MenuItemIds.INFO -> listOf(MenuEntry.Info)
            MenuItemIds.GENIUS -> listOf(MenuEntry.Genius)
            // Add / Remove from library: the app doesn't know what the library holds, so both
            // halves the core offers are rows (as before), not a guess that hides one.
            ActionIds.ADD_TO_LIBRARY -> item.actions.filter { it.enabled }.map { MenuEntry.Action(it) }
            // A pair the core enables both halves of is one row: the saved queue's pin by its state.
            ActionIds.PIN_SAVED_QUEUE -> listOfNotNull(
                when (queuePinned) {
                    true -> action(ActionIds.UNPIN_SAVED_QUEUE)
                    false -> action(ActionIds.PIN_SAVED_QUEUE)
                    null -> null
                } ?: item.actions.firstOrNull { it.enabled } ?: item.actions.firstOrNull(),
            ).map { MenuEntry.Action(it) }
            // Another app item is one of the context's extras (the player's Lyrics timing); any
            // other row shows its first enabled action.
            else -> if (item.actions.isEmpty()) extras.filter { it.item == item.id }.map { MenuEntry.Extra(it) }
                else listOfNotNull(item.actions.firstOrNull { it.enabled } ?: item.actions.firstOrNull()).map { MenuEntry.Action(it) }
        }
        when {
            item.section == MenuSection.Danger -> danger += entries
            item.folded -> more += entries
            else -> primary += entries
        }
    }
    val (removing, others) = extras.filter { it.item == null }.partition { it.destructive }
    val (extraMore, extraShort) = others.partition { it.underMore }
    var short = extraShort.map { MenuEntry.Extra(it) } + primary
    var folded: List<MenuEntry> = extraMore.map { MenuEntry.Extra(it) } + more
    if (short.isEmpty()) { short = folded; folded = emptyList() }
    return SheetLayout(love, play, rating, short, folded, danger + removing.map { MenuEntry.Extra(it) })
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
 * The compact menu for a song, an album, an artist, a playlist or several songs, laid out from the
 * core's [menu] ([sheetLayout]): a sheet that opens short (header with the love toggle, the
 * connected Play / Shuffle / queue actions group, the star row, a short connected list ending in
 * "More", then the rows that remove or delete, always in view), and grows to the rest when More,
 * the handle or an upward drag asks; "Show less" shrinks it again. Toggles (love, rating, download,
 * pin) apply in place; every other row closes the sheet with its hide animation, its action or
 * navigation starting at once behind it. Add to playlist, Share, Snooze, Go to artist for a song of
 * several artists and (a context's extras, [SongMenuExtra.page]) the player's sleep timer open as
 * sub-pages of the sheet instead, back or Done bringing the menu back or closing it. The header's
 * artist and album names go there too, where the menu has the row. [onGoToArtist] opens an artist
 * by id. [onRan] follows any action that ran (a selection
 * clears). [extraTop] sits under the header (a Library pin's controls). Tags: `[tag]`, `[tag].header`,
 * `[tag].header.artist`, `[tag].header.album`, `[tag].action.<id>`, `[tag].download`, `[tag].pin`, `[tag].info`, `[tag].more`,
 * `[tag].danger`, `[tag].artists.<i>`, `rating`.
 *
 * [embedded]: the menu without its sheet, laid into a host's own surface (the selection drawer),
 * which scrolls and drags it: every row shows at once (no More, no Show less), and a row that would
 * hide the sheet runs straight away and reports [onDismiss]. [showPlay] false leaves the play group
 * to the host (the drawer keeps it in its peek). [showHeader] false leaves out the header (and its
 * love toggle): the stash lists its songs above the menu instead. [onPlay] runs a play group action
 * in the host's own way (the stash plays as a queue of its own name) instead of the registry's.
 */
@Composable
fun MenuSheet(
    subject: MenuSubject,
    target: ActionTarget,
    menu: CoreMenu,
    onDismiss: () -> Unit,
    extras: List<SongMenuExtra> = emptyList(),
    onGoToAlbum: (() -> Unit)? = null,
    onGoToArtist: ((String) -> Unit)? = null,
    onInfo: (() -> Unit)? = null,
    onRan: () -> Unit = {},
    extraTop: (@Composable () -> Unit)? = null,
    tag: String = "songMenu",
    pull: SheetPull? = null,
    embedded: Boolean = false,
    showPlay: Boolean = true,
    showHeader: Boolean = true,
    onPlay: ((ActionDescriptor) -> Unit)? = null,
    /** On the play group (the stash drawer measures where it ends, to rest with it at its foot). */
    playGroupModifier: Modifier = Modifier,
    /** Embedded: a change sends the menu back to its first page (the host has closed it to a peek). */
    resetKey: Any? = null,
    /** One song's cover previews it ([PreviewArtwork]); false where that song is the one playing (the player's menu). */
    canPreview: Boolean = true,
) {
    val client = LocalCoreClient.current
    val context = LocalContext.current
    // Every menu that hosts this sheet (the song menu, a one-song selection, an item menu)
    // stops the preview when the sheet leaves, however it was closed.
    DisposableEffect(Unit) {
        onDispose { client.dispatch(Command.PreviewStop) }
    }
    val snoozeItems by client.snoozes.collectAsStateWithLifecycle()
    val snoozed = subject.trackId?.let { id -> snoozeItems.any { it.trackRef.id == id } }
    val snoozeUntil = subject.trackId?.let { id -> snoozeItems.firstOrNull { it.trackRef.id == id }?.until }
    // The sleep timer (the player's extra row and its sub-page; nothing else on the menu sees it).
    val sleepTimer by client.sleepTimer.collectAsStateWithLifecycle()
    val sheetState = rememberPulledSheetState(pull)
    val scope = rememberCoroutineScope()
    /** Hides the sheet, then runs [after] (which may open what follows) and reports it gone. */
    /** Embedded: how many times the menu has closed itself (each sends it back to its first page). */
    var closes by remember { androidx.compose.runtime.mutableIntStateOf(0) }
    fun hideThen(after: () -> Unit = {}) {
        if (embedded) { after(); closes++; onDismiss(); return }
        scope.launch { sheetState.hide() }.invokeOnCompletion { after(); onDismiss() }
    }
    val artistChoices = subject.artistChoices
    // A saved queue's menu pins or unpins it by whether it is pinned.
    val savedQueues by client.savedQueues.collectAsStateWithLifecycle()
    val queuePinned = (target as? ActionTarget.SavedQueue)?.let { t -> savedQueues.firstOrNull { it.id == t.data.id }?.pinned }
    val layout = remember(menu, extras, onGoToAlbum != null, onGoToArtist != null && artistChoices.isNotEmpty(), snoozed, queuePinned) {
        sheetLayout(menu, extras, canGoToAlbum = onGoToAlbum != null, canGoToArtist = onGoToArtist != null && artistChoices.isNotEmpty(), snoozed = snoozed, queuePinned = queuePinned)
    }
    val entries = layout.primary + layout.more + layout.danger
    var expanded by rememberSaveable { mutableStateOf(embedded) }
    // The sub-page in view (Share, Snooze), or the menu itself. Back (the header's arrow, the
    // system's back or its gesture) returns to the menu rather than closing the sheet.
    // Seekable, so a predictive back gesture can scrub the way back to the menu.
    val pages = remember { SeekableTransitionState(MenuPage.Main) }
    val pageTransition = rememberTransition(pages, label = "menuPage")
    /**
     * The page the menu is on as far as back is concerned: set as soon as it is asked for, while the
     * transition may still be playing (whose own current state only moves when it finishes).
     */
    var page by remember { mutableStateOf(MenuPage.Main) }
    fun goTo(to: MenuPage) { page = to; scope.launch { pages.animateTo(to) } }
    /** Go to artist: straight there for one, the sub-page of them for several. */
    fun goToArtist() {
        val open = onGoToArtist ?: return
        if (artistChoices.size > 1) goTo(MenuPage.Artists) else artistChoices.firstOrNull()?.let { (id, _) -> open(id); hideThen() }
    }
    /** How far a predictive back gesture has pulled the sheet in, 0 to 1 (the sheet scales with it). */
    val backPull = remember { Animatable(0f) }
    val shareOptions = remember(subject.kind, subject.count) { shareChoices(subject.kind, subject.count) }
    /** The share being fetched (a link, the file): the sheet shows it loading, then hides as the share sheet opens. */
    var sharing by remember { mutableStateOf<ShareChoice?>(null) }
    if (embedded) {
        // Closed (by the host, [resetKey], or by itself): back to the first page at once, nothing left loading.
        val first = remember { booleanArrayOf(true) }
        LaunchedEffect(resetKey, closes) {
            if (first[0]) { first[0] = false; return@LaunchedEffect }
            sharing = null; page = MenuPage.Main; pages.snapTo(MenuPage.Main)
        }
    }
    fun share(choice: ShareChoice) {
        // Once: the Share page stays tappable while it slides away.
        if (sharing != null) return
        sharing = choice
        goTo(MenuPage.Sharing)
        // On the sheet's scope: closing the sheet (a drag, the scrim) gives up on the share.
        scope.launch {
            ShareActions.perform(context, client, ShareRequest(choice, subject, target))
            hideThen()
        }
    }
    var loved by remember { mutableStateOf(subject.loved) }
    var rating by remember { mutableStateOf(subject.rating) }
    var downloaded by remember { mutableStateOf(subject.downloaded) }
    val pinEntry = (layout.more + layout.primary).filterIsInstance<MenuEntry.LibraryPin>().firstOrNull()
    var pinned by remember { mutableStateOf(pinEntry?.unpin?.enabled == true) }
    var snoozeCustom by remember { mutableStateOf(false) }
    val size by produceState<Double?>(initialValue = null, subject.trackId) {
        value = subject.trackRef?.let { (client.query(Queries.track(it)) as? QueryResult.TrackDetail)?.data?.sizeBytes }
    }
    fun run(a: ActionDescriptor) = client.dispatch(Commands.runAction(a.id, target))
    fun runAndClose(a: ActionDescriptor) { if (a.id !in ActionIds.UI_HANDLED) run(a); hideThen(onRan) }
    val canExpand = layout.hidden > 0
    val canExpandNow by rememberUpdatedState(canExpand)
    // How far the rest of the menu is open, 0 to 1: [openness] animates it, and while a finger holds
    // it [held] says where (written at once, so a quick release never races a pending snap).
    val openness = remember { Animatable(if (expanded) 1f else 0f) }
    val held = remember { mutableFloatStateOf(-1f) }
    fun shown(): Float = held.floatValue.takeIf { it >= 0f } ?: openness.value
    /** The rest of the menu's full height, measured each layout (it is always laid out, clipped). */
    val restHeight = remember { intArrayOf(0, 0) }
    val settleSpec = remember { spring<Float>(dampingRatio = Spring.DampingRatioNoBouncy, stiffness = Spring.StiffnessMediumLow) }
    // More, Show less and the handle animate it to where [expanded] says; a released drag has
    // already set off there, with the finger's speed.
    LaunchedEffect(expanded) {
        val target = if (expanded) 1f else 0f
        if (held.floatValue < 0f && openness.targetValue != target) openness.animateTo(target, settleSpec)
    }
    val showingRest by remember { derivedStateOf { shown() > 0f } }
    val showingMore by remember { derivedStateOf { shown() < 1f } }
    // A drag the list can't take follows the finger: up opens the rest of the menu, down (with the
    // list at its top) closes it, and only then does the sheet itself move. Letting go settles it
    // open or closed: a flick goes its way, otherwise whichever is nearer.
    val followDrag = remember {
        object : NestedScrollConnection {
            fun release(open: Boolean, velocity: Float) {
                val from = held.floatValue
                expanded = open
                scope.launch {
                    openness.snapTo(from)
                    held.floatValue = -1f
                    openness.animateTo(if (open) 1f else 0f, settleSpec, initialVelocity = velocity)
                }
            }

            override fun onPostScroll(consumed: Offset, available: Offset, source: NestedScrollSource): Offset {
                val height = restHeight.sum()
                // A sub-page has no rest to open and the menu is not on view: drags there are the page's own.
                if (source != NestedScrollSource.UserInput || page != MenuPage.Main || !canExpandNow || height <= 0) return Offset.Zero
                val at = shown()
                val dy = available.y
                if (!((dy < 0f && at < 1f) || (dy > 0f && at > 0f))) return Offset.Zero
                val next = (at - dy / height).coerceIn(0f, 1f)
                held.floatValue = next
                return Offset(0f, (at - next) * height)
            }

            override suspend fun onPreFling(available: Velocity): Velocity {
                val at = held.floatValue
                if (at < 0f) return Velocity.Zero
                val v = available.y
                // Closed and still going down (the sheet has moved on with it), or open and going up:
                // the fling is the sheet's or the list's.
                if ((at <= 0f && v > 0f) || (at >= 1f && v < 0f)) {
                    release(at >= 1f, 0f)
                    return Velocity.Zero
                }
                val open = when {
                    v < -FLICK_VELOCITY -> true
                    v > FLICK_VELOCITY -> false
                    else -> at > 0.5f
                }
                release(open, -v / restHeight.sum().coerceAtLeast(1))
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
                    ActionIds.STASH -> stringResource(R.string.action_stash)
                    ActionIds.SNOOZE -> stringResource(R.string.action_snooze)
                    ActionIds.UNSNOOZE -> stringResource(R.string.action_unsnooze)
                    else -> a.label
                }
                val untilText = if (a.id == ActionIds.UNSNOOZE && subject.count <= 1) snoozeEnds(snoozeUntil) else null
                val icon = if (a.id == ActionIds.ADD_TO_PLAYLIST) HocketIcons.AutoMirrored.Filled.PlaylistAdd else actionIcon(a.icon)
                MenuRow(AnnotatedString(label), icon, shape, supporting = untilText, enabled = a.enabled, destructive = a.destructive, tag = "$tag.action.${a.id}",
                    trailing = if (a.id == ActionIds.ADD_TO_PLAYLIST || a.id == ActionIds.SNOOZE) { { Icon(HocketIcons.Filled.ChevronRight, null, Modifier.size(20.dp), tint = MaterialTheme.colorScheme.onSurfaceVariant) } } else null,
                ) {
                    when (a.id) {
                        ActionIds.ADD_TO_PLAYLIST -> goTo(MenuPage.Playlist)
                        ActionIds.SNOOZE -> goTo(MenuPage.Snooze)
                        else -> runAndClose(a)
                    }
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
                val several = !album && artistChoices.size > 1
                val name = if (album) subject.album else if (several) artistChoices.joinToString(", ") { it.second } else subject.artist
                val label = buildAnnotatedString {
                    append(stringResource(if (album) R.string.action_go_to_album else R.string.action_go_to_artist))
                    name?.takeIf { it.isNotBlank() }?.let { withStyle(SpanStyle(color = MaterialTheme.colorScheme.onSurfaceVariant)) { append(stringResource(R.string.dot_separator)); append(it) } }
                }
                MenuRow(label, if (album) HocketIcons.Filled.Album else HocketIcons.Filled.Person, shape, enabled = entry.action.enabled, tag = "$tag.action.${entry.action.id}",
                    trailing = if (several) { { Icon(HocketIcons.Filled.ChevronRight, null, Modifier.size(20.dp), tint = MaterialTheme.colorScheme.onSurfaceVariant) } } else null,
                ) {
                    if (album) { onGoToAlbum?.invoke(); hideThen() } else goToArtist()
                }
            }
            MenuEntry.Info -> MenuRow(AnnotatedString(stringResource(if (subject.kind == MenuKind.Album) R.string.menu_album_info else R.string.menu_song_info)), HocketIcons.Filled.Info, shape, tag = "$tag.info") { hideThen { onInfo?.invoke() } }
            MenuEntry.Share -> MenuRow(AnnotatedString(stringResource(R.string.action_share)), HocketIcons.Filled.Share, shape, tag = "$tag.share",
                trailing = { Icon(HocketIcons.Filled.ChevronRight, null, Modifier.size(20.dp), tint = MaterialTheme.colorScheme.onSurfaceVariant) },
            ) { goTo(MenuPage.Share) }
            MenuEntry.Genius -> MenuRow(AnnotatedString(stringResource(R.string.menu_open_genius)), HocketIcons.Filled.Lyrics, shape, tag = "$tag.genius") { hideThen { ShareActions.openGenius(context, subject) } }
            is MenuEntry.Extra -> {
                val e = entry.extra
                MenuRow(AnnotatedString(e.label), e.icon, shape, supporting = e.supporting, destructive = e.destructive, highlighted = e.highlighted, tag = e.testTag,
                    trailing = if (e.page != null) { { Icon(HocketIcons.Filled.ChevronRight, null, Modifier.size(20.dp), tint = MaterialTheme.colorScheme.onSurfaceVariant) } } else null,
                ) {
                    if (e.page != null) goTo(e.page) else hideThen { e.onClick?.invoke() }
                }
            }
        }
    }

    val body = @Composable {
        // Inside the sheet: its window has its own back dispatcher, and this callback, added after
        // the sheet's own, takes back first while a sub-page shows.
        // The gesture scrubs the first part of the slide back to the menu (the sub-page fading as the
        // menu comes in, the height heading its way) and draws the sheet in a little; letting go plays
        // the rest, cancelling slides the sub-page back. A back key (no gesture) just goes.
        // Back during a slide in already goes back; once back is let go the page is the menu, so a
        // second back (even mid-slide) is the sheet's own and closes it.
        PredictiveBackHandler(enabled = page != MenuPage.Main && page != MenuPage.Sharing) { events ->
            val from = page
            try {
                events.collect { e ->
                    val p = PredictiveBackEasing.transform(e.progress)
                    backPull.snapTo(p)
                    pages.seekTo(p * BACK_PEEK, MenuPage.Main)
                }
                scope.launch { backPull.animateTo(0f, settleSpec) }
                goTo(MenuPage.Main)
            } catch (e: kotlinx.coroutines.CancellationException) {
                scope.launch { backPull.animateTo(0f, settleSpec) }
                // Only if nothing else (a tap) has moved the page meanwhile.
                if (page == from) scope.launch { pages.animateTo(from) }
                throw e
            }
        }
        // While a share loads, back does nothing: neither the menu nor closing the sheet.
        BackHandler(enabled = page == MenuPage.Sharing) {}
        Column(if (embedded) Modifier.padding(bottom = 16.dp) else Modifier.nestedScroll(followDrag).verticalScroll(rememberScrollState()).navigationBarsPadding().padding(bottom = 24.dp)) {
            // The header's artist and album go where the menu's rows would (where it has them).
            val headerArtist = if (entries.any { it is MenuEntry.GoTo && it.action.id == ActionIds.GO_TO_ARTIST }) { { goToArtist() } } else null
            val headerAlbum = if (entries.any { it is MenuEntry.GoTo && it.action.id == ActionIds.GO_TO_ALBUM }) { { onGoToAlbum?.invoke(); hideThen() } } else null
            val previewTrack = subject.trackRef?.takeIf { canPreview && subject.kind == MenuKind.Song && subject.count <= 1 }
            if (showHeader) MenuHeader(subject, layout.love, loved, tag, previewTrack, onArtist = headerArtist, onAlbum = headerAlbum) { love, unlove ->
                if (loved == Tick.On) { unlove?.let(::run); loved = Tick.Off } else { love?.let(::run); loved = Tick.On }
            }
            // A sub-page slides in from the end, the menu slides back from the start, and the height
            // animates alongside the slide: the sheet follows its content's size, and snaps to it if
            // that jumps, so the glide has to come from here. Going back cross-fades over longer, so a
            // back gesture scrubbing its first part shows the sub-page giving way to the menu.
            pageTransition.AnimatedContent(
                transitionSpec = {
                    val forward = targetState != MenuPage.Main
                    val spec = tween<androidx.compose.ui.unit.IntOffset>(PAGE_MS, easing = androidx.compose.animation.core.FastOutSlowInEasing)
                    val fadeOutMs = if (forward) PAGE_MS / 3 else PAGE_MS / 2
                    (slideInHorizontally(spec) { w -> if (forward) w / 4 else -w / 4 } + fadeIn(tween(PAGE_MS / 2, delayMillis = PAGE_MS / 6)))
                        .togetherWith(slideOutHorizontally(spec) { w -> if (forward) -w / 4 else w / 4 } + fadeOut(tween(fadeOutMs)))
                        .using(SizeTransform(clip = false) { _, _ -> tween(PAGE_MS, easing = androidx.compose.animation.core.FastOutSlowInEasing) })
                },
                contentAlignment = Alignment.TopCenter,
            ) { shownPage ->
            when (shownPage) {
            MenuPage.Share -> SharePage(shareOptions, tag) { choice -> if (sharing != null) Unit else if (choice == null) goTo(MenuPage.Main) else share(choice) }
            MenuPage.Sharing -> SharingPage(sharing, tag)
            MenuPage.Snooze -> SnoozePage(tag, onCustom = { snoozeCustom = true }) { duration ->
                if (duration == null) goTo(MenuPage.Main)
                else hideThen {
                    val ids = snoozeTrackRefs(target, subject.trackRef, client.queue.value)
                    if (ids.isNotEmpty()) client.dispatch(Commands.snooze(ids, duration))
                }
            }
            MenuPage.Playlist -> PlaylistMenuPage(tag, target, onBack = { goTo(MenuPage.Main) }, onClose = { hideThen(onRan) })
            MenuPage.Artists -> ArtistsMenuPage(tag, artistChoices, onBack = { goTo(MenuPage.Main) }) { id -> onGoToArtist?.invoke(id); hideThen() }
            MenuPage.Sleep -> SleepMenuPage(tag, sleepTimer, onBack = { goTo(MenuPage.Main) }) { timer ->
                hideThen { client.dispatch(Commands.setSleepTimer(timer)) }
            }
            MenuPage.Main -> Column {
            extraTop?.let { it(); androidx.compose.foundation.layout.Spacer(Modifier.height(8.dp)) }
            if (showPlay && layout.play.isNotEmpty()) PlayGroup(layout.play, tag, playGroupModifier) { a -> if (onPlay != null) { onPlay(a); hideThen(onRan) } else runAndClose(a) }
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
                    Box(Modifier.revealed { 1f - shown() }) {
                        MoreRow(layout.hidden, connectedShape(primaryRows - 1, primaryRows), tag) { expanded = true }
                    }
                }
            }
            if (canExpand) {
                // Always laid out (so a drag knows its full height), shown as far as it is open.
                Column(
                    Modifier
                        .fillMaxWidth()
                        .revealed(onMeasured = { restHeight[0] = it }) { shown() }
                        // Closed, it is out of reach: no semantics, and focus (a keyboard's Tab) passes it by.
                        .then(if (showingRest) Modifier else Modifier.clearAndSetSemantics {}.focusProperties { onEnter = { cancelFocusChange() } })
                        .focusGroup(),
                ) {
                    if (layout.more.isNotEmpty()) Column(Modifier.fillMaxWidth().padding(start = 16.dp, end = 16.dp, top = 12.dp), verticalArrangement = Arrangement.spacedBy(2.dp)) {
                        layout.more.forEachIndexed { i, entry -> EntryRow(entry, connectedShape(i, layout.more.size)) }
                    }
                }
            }
            // Removing and deleting: always in view, last, after a gap.
            if (layout.danger.isNotEmpty()) Column(Modifier.fillMaxWidth().padding(start = 16.dp, end = 16.dp, top = 20.dp).testTag("$tag.danger"), verticalArrangement = Arrangement.spacedBy(2.dp)) {
                layout.danger.forEachIndexed { i, entry -> EntryRow(entry, connectedShape(i, layout.danger.size)) }
            }
            if (canExpand && !embedded) {
                Box(
                    Modifier
                        .fillMaxWidth()
                        .revealed(onMeasured = { restHeight[1] = it }) { shown() }
                        .then(if (showingRest) Modifier else Modifier.clearAndSetSemantics {}.focusProperties { onEnter = { cancelFocusChange() } }),
                    contentAlignment = Alignment.Center,
                ) {
                    TextButton(onClick = { expanded = false }, modifier = Modifier.padding(top = 18.dp).heightIn(min = 48.dp).testTag("$tag.showLess")) {
                        Icon(HocketIcons.Filled.KeyboardArrowDown, null, Modifier.size(20.dp).rotate(180f))
                        Text(stringResource(R.string.menu_show_less), style = MaterialTheme.typography.titleSmall, modifier = Modifier.padding(start = 8.dp))
                    }
                }
            }
            }
            }
            }
        }
    }
    if (embedded) {
        Box(Modifier.testTag(tag)) { body() }
    } else {
        ModalBottomSheet(
            onDismissRequest = onDismiss,
            sheetState = sheetState,
            scrimColor = pulledScrim(pull),
            dragHandle = { MenuHandle(expanded, canExpand) { expanded = !expanded } },
            modifier = Modifier.pulled(pull).backPulled { backPull.value }.testTag(tag),
        ) { body() }
    }
    if (snoozeCustom) {
        SnoozeUntilPicker(tag, onDismiss = { snoozeCustom = false }) { until ->
            snoozeCustom = false
            hideThen {
                val ids = snoozeTrackRefs(target, subject.trackRef, client.queue.value)
                if (ids.isNotEmpty()) client.dispatch(Commands.snoozeUntil(ids, until.toDouble()))
            }
        }
    }
}

/** What the sheet shows under its header: the menu, or one of its sub-pages. Public for [SongMenuExtra.page]. */
enum class MenuPage { Main, Share, Snooze, Sharing, Playlist, Sleep, Artists }

/** Material's predictive back easing: the gesture's progress as the sheet follows it. */
private val PredictiveBackEasing = CubicBezierEasing(0.1f, 0.1f, 0f, 1f)

/**
 * Draws the sheet in from its bottom edge as a predictive back gesture pulls ([pull], 0 to 1), as
 * Material's sheets do: 48dp narrower and 24dp shorter at the full pull.
 */
private fun Modifier.backPulled(pull: () -> Float): Modifier = graphicsLayer {
    val p = pull()
    if (p > 0f && size.width > 0f && size.height > 0f) {
        scaleX = 1f - p * 48.dp.toPx() / size.width
        scaleY = 1f - p * 24.dp.toPx() / size.height
        transformOrigin = androidx.compose.ui.graphics.TransformOrigin(0.5f, 1f)
    }
}

/** How much of the way back to the menu a full predictive back gesture scrubs before it is let go. */
private const val BACK_PEEK = 0.5f

/** How long a sub-page takes to slide in or out. */
private const val PAGE_MS = 300

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

/**
 * Artwork, the title (one line) with the lines under it, and the love toggle. The subtitle's artist
 * (at its start) and album (at its end) are links where [onArtist] / [onAlbum] go there: the menu's
 * Go to rows, a tap nearer. With [previewTrack] the artwork plays a clip of it ([PreviewArtwork]).
 */
@Composable
private fun MenuHeader(subject: MenuSubject, love: LoveToggle?, loved: Tick, tag: String, previewTrack: app.hocket.core.api.TrackRef?, onArtist: (() -> Unit)? = null, onAlbum: (() -> Unit)? = null, onLove: (ActionDescriptor?, ActionDescriptor?) -> Unit) {
    Row(Modifier.fillMaxWidth().padding(start = 16.dp, end = 12.dp, bottom = 14.dp).testTag("$tag.header"), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(14.dp)) {
        PreviewArtwork(subject.coverArt, previewTrack, 64.dp, RoundedCornerShape(16.dp), "$tag.artwork")
        Column(Modifier.weight(1f)) {
            Text(subject.title, style = MaterialTheme.typography.titleLarge.copy(fontSize = 19.sp, lineHeight = 24.sp), fontWeight = FontWeight.SemiBold, maxLines = 1, overflow = TextOverflow.Ellipsis, modifier = Modifier.semantics { heading() })
            subject.subtitle?.let { HeaderSubtitle(it, subject, onArtist, onAlbum, tag) }
            subject.meta?.let { Text(it, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant, maxLines = 1, overflow = TextOverflow.Ellipsis) }
        }
        if (love != null) {
            val action = if (loved == Tick.On) love.unlove else love.love
            LoveButton(loved, subject.lovedCount, subject.count, enabled = action?.enabled == true, tag = "$tag.love") { onLove(love.love, love.unlove) }
        }
    }
}

/**
 * The header's subtitle, its artist and album names ([headerLinkRanges]) each a tap target of its
 * own (24dp tall, named for what it opens) going where [onArtist] / [onAlbum] go.
 */
@Composable
private fun HeaderSubtitle(subtitle: String, subject: MenuSubject, onArtist: (() -> Unit)?, onAlbum: (() -> Unit)?, tag: String) {
    val style = MaterialTheme.typography.bodyMedium
    val color = MaterialTheme.colorScheme.onSurfaceVariant
    val (artist, album) = headerLinkRanges(subtitle, subject, onArtist != null, onAlbum != null)
    val lineHeight = with(androidx.compose.ui.platform.LocalDensity.current) { style.lineHeight.roundToPx() }
    if (artist == null && album == null) {
        Text(subtitle, style = style, color = color, maxLines = 1, overflow = TextOverflow.Ellipsis)
        return
    }
    val artistLabel = stringResource(R.string.action_go_to_artist)
    val albumLabel = stringResource(R.string.action_go_to_album)
    // The subtitle cut at the links: (text, what a tap opens, its name, its tag).
    val parts = buildList {
        var at = 0
        val links = listOfNotNull(artist?.let { Triple(it, onArtist!!, artistLabel to "$tag.header.artist") }, album?.let { Triple(it, onAlbum!!, albumLabel to "$tag.header.album") })
        for ((range, open, named) in links) {
            if (range.first > at) add(PartOf(subtitle.substring(at, range.first)))
            add(PartOf(subtitle.substring(range.first, range.last + 1), open, named.first, named.second))
            at = range.last + 1
        }
        if (at < subtitle.length) add(PartOf(subtitle.substring(at)))
    }
    Row(verticalAlignment = Alignment.CenterVertically) {
        parts.forEach { p ->
            val open = p.open
            Text(
                p.text, style = style, color = color, maxLines = 1, overflow = TextOverflow.Ellipsis, softWrap = false,
                modifier = if (open == null) Modifier else Modifier
                    .weight(1f, fill = false)
                    .touchTargetOverLine(lineHeight)
                    .clip(RoundedCornerShape(12.dp))
                    .clickable(onClickLabel = p.label, role = Role.Button) { open() }
                    .wrapContentHeight()
                    .testTag(p.tag ?: ""),
            )
        }
    }
}

/**
 * A 48dp-tall (and at least 48dp-wide) touch target for a link in a line of text that takes only
 * the line's [lineHeight] in the layout: the target spills over the lines above and below (which
 * aren't tappable), so the header keeps its height.
 */
private fun Modifier.touchTargetOverLine(lineHeight: Int): Modifier = layout { measurable, constraints ->
    val target = 48.dp.roundToPx()
    val p = measurable.measure(constraints.copy(minWidth = minOf(target, constraints.maxWidth), minHeight = target, maxHeight = maxOf(target, constraints.maxHeight)))
    layout(p.width, lineHeight) { p.place(0, (lineHeight - p.height) / 2) }
}

/** A piece of the header's subtitle; [open] set for a link. */
private class PartOf(val text: String, val open: (() -> Unit)? = null, val label: String? = null, val tag: String? = null)

/**
 * Where the header's subtitle names the artist (at its start: "Low Lanterns · Halfway Home") and the
 * album (at its end, never overlapping the artist; not on an album's own menu), as character
 * ranges; null where it doesn't, or where there's nowhere to go ([artist], [album]).
 */
internal fun headerLinkRanges(subtitle: String, subject: MenuSubject, artist: Boolean, album: Boolean): Pair<IntRange?, IntRange?> {
    val a = subject.artist?.takeIf { artist && it.isNotBlank() && subtitle.startsWith(it) }?.let { 0 until it.length }
    val b = subject.album?.takeIf { album && subject.kind != MenuKind.Album && it.isNotBlank() && subtitle.endsWith(it) }
        ?.let { (subtitle.length - it.length) until subtitle.length }
        ?.takeIf { r -> a == null || r.first > a.last }
    return a to b
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
    // Taps that love (not unlove) burst the heart.
    var bursts by remember { androidx.compose.runtime.mutableIntStateOf(0) }
    Box(
        Modifier
            .size(48.dp)
            .clip(CircleShape)
            .background(if (state == Tick.On) MaterialTheme.colorScheme.secondaryContainer else Color.Transparent)
            .triStateToggleable(
                state = when (state) { Tick.On -> ToggleableState.On; Tick.Off -> ToggleableState.Off; Tick.Mixed -> ToggleableState.Indeterminate },
                enabled = enabled,
                role = Role.Checkbox,
                onClick = { if (state != Tick.On) bursts++; onToggle() },
            )
            .semantics { contentDescription = label; spoken?.let { stateDescription = it } }
            .testTag(tag),
        contentAlignment = Alignment.Center,
    ) {
      LoveBurst(bursts, primary, Modifier.matchParentSize()) {
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
}

/**
 * Play and Shuffle on top (Shuffle isn't offered for one song), then Play next / Add to queue (when offered) /
 * Play later below, as one connected group: rounded outer corners, tight inner ones, Play filled and wider.
 */
@Composable
private fun PlayGroup(play: List<ActionDescriptor>, tag: String, modifier: Modifier = Modifier, onRun: (ActionDescriptor) -> Unit) {
    val groupLabel = stringResource(R.string.menu_play_group)
    val rows = play.partition { it.id == ActionIds.PLAY || it.id == ActionIds.PLAY_SHUFFLED }.toList().filter { it.isNotEmpty() }
    Column(modifier.fillMaxWidth().padding(horizontal = 16.dp).semantics { isTraversalGroup = true; contentDescription = groupLabel }, verticalArrangement = Arrangement.spacedBy(3.dp)) {
        rows.forEachIndexed { r, row ->
            val top = r == 0
            val bottom = r == rows.lastIndex
            Row(Modifier.fillMaxWidth().height(IntrinsicSize.Min), horizontalArrangement = Arrangement.spacedBy(3.dp)) {
                row.forEachIndexed { i, a ->
                    fun corner(outer: Boolean) = if (outer) 24.dp else 8.dp
                    val shape = RoundedCornerShape(
                        topStart = corner(top && i == 0), topEnd = corner(top && i == row.lastIndex),
                        bottomStart = corner(bottom && i == 0), bottomEnd = corner(bottom && i == row.lastIndex),
                    )
                    PlayGroupButton(a, shape, tag, Modifier.weight(if (a.id == ActionIds.PLAY) 1.6f else 1f), onRun)
                }
            }
        }
    }
}

/** One button of [PlayGroup]: Play and Shuffle put the icon beside the label, the queue actions above it. */
@Composable
private fun PlayGroupButton(a: ActionDescriptor, shape: Shape, tag: String, modifier: Modifier, onRun: (ActionDescriptor) -> Unit) {
    val main = a.id == ActionIds.PLAY
    val start = main || a.id == ActionIds.PLAY_SHUFFLED
    val (label, icon) = playFace(a.id) ?: (stringResource(R.string.action_play_later) to HocketIcons.Filled.PlaylistAdd)
    Surface(
        onClick = { onRun(a) },
        enabled = a.enabled,
        shape = shape,
        color = if (main) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.secondaryContainer,
        contentColor = if (main) MaterialTheme.colorScheme.onPrimary else MaterialTheme.colorScheme.onSecondaryContainer,
        modifier = modifier.fillMaxHeight().heightIn(min = if (start) 60.dp else 64.dp).testTag("$tag.action.${a.id}"),
    ) {
        val content = Modifier.alpha(if (a.enabled) 1f else 0.38f)
        if (start) {
            Row(content.padding(horizontal = 12.dp, vertical = 8.dp), horizontalArrangement = Arrangement.spacedBy(8.dp, Alignment.CenterHorizontally), verticalAlignment = Alignment.CenterVertically) {
                Icon(icon, null, Modifier.size(if (main) 26.dp else 22.dp))
                Text(label, style = MaterialTheme.typography.labelLarge, fontWeight = if (main) FontWeight.SemiBold else FontWeight.Medium, maxLines = 1, overflow = TextOverflow.Ellipsis)
            }
        } else {
            Column(content.padding(horizontal = 6.dp, vertical = 8.dp), horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.spacedBy(4.dp, Alignment.CenterVertically)) {
                Icon(icon, null, Modifier.size(22.dp))
                // One line at normal font sizes; a large font size wraps rather than cutting the label off.
                Text(label, style = MaterialTheme.typography.labelMedium.copy(fontSize = 13.sp), fontWeight = FontWeight.Medium, textAlign = TextAlign.Center, maxLines = 2, overflow = TextOverflow.Ellipsis)
            }
        }
    }
}

/** The play group's short label and icon for a play action (Play, Shuffle, Play next, Add to queue, Play later); null for any other. */
@Composable
internal fun playFace(id: String): Pair<String, ImageVector>? = when (id) {
    ActionIds.PLAY -> stringResource(R.string.action_play) to HocketIcons.Filled.PlayArrow
    ActionIds.PLAY_SHUFFLED -> stringResource(R.string.action_shuffle) to HocketIcons.Filled.Shuffle
    ActionIds.PLAY_NEXT -> stringResource(R.string.action_play_next) to HocketIcons.Filled.PlaylistPlay
    ActionIds.ADD_TO_QUEUE -> stringResource(R.string.action_label_add_to_queue) to HocketIcons.Filled.QueueMusic
    ActionIds.PLAY_LATER -> stringResource(R.string.action_play_later) to HocketIcons.Filled.PlaylistAdd
    else -> null
}

/** The play group's actions, in their fixed order. */
internal val PlayActionIds: List<String> = listOf(ActionIds.PLAY, ActionIds.PLAY_SHUFFLED, ActionIds.PLAY_NEXT, ActionIds.ADD_TO_QUEUE, ActionIds.PLAY_LATER)

/** "Rating" and the stars (tap the current star to clear); several songs rated differently read "Mixed". */
@Composable
private fun RatingRow(rating: Int?, onRate: (Int) -> Unit) {
    val mixed = stringResource(R.string.menu_rating_mixed)
    Row(Modifier.fillMaxWidth().padding(start = 16.dp, end = 16.dp, top = 10.dp, bottom = 2.dp), verticalAlignment = Alignment.CenterVertically) {
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

/**
 * The songs Snooze's page snoozes: a selection's tracks, a queue selection's rows looked up in
 * [queue] (the rows on screen are in it), else the one song the menu is for.
 */
internal fun snoozeTrackRefs(target: app.hocket.core.api.ActionTarget, track: app.hocket.core.api.TrackRef?, queue: app.hocket.core.api.QueueView): List<app.hocket.core.api.TrackRef> = when (target) {
    is app.hocket.core.api.ActionTarget.Tracks -> target.data.tracks
    is app.hocket.core.api.ActionTarget.QueueItems -> {
        val entries = queue.entries + listOfNotNull(queue.current)
        val byKey = entries.associate { it.item.key to it.item.track }
        target.data.keys.mapNotNull { byKey[it] }.distinct().ifEmpty { listOfNotNull(track) }
    }
    else -> listOfNotNull(track)
}

/** When a snooze ends, for the Unsnooze row. */
@Composable
private fun snoozeEnds(until: Double?): String = if (until == null) {
    stringResource(R.string.snooze_until_never)
} else {
    stringResource(R.string.snooze_until, DateFormat.getDateTimeInstance(DateFormat.MEDIUM, DateFormat.SHORT).format(Date(until.toLong())))
}

/** A share on its way: the loading indicator and what it is getting, with no way back. Tag: `[tag].sharing`. */
@OptIn(ExperimentalMaterial3ExpressiveApi::class)
@Composable
private fun SharingPage(choice: ShareChoice?, tag: String) {
    val label = stringResource(if (choice == ShareChoice.File) R.string.share_busy_file else R.string.share_busy_link)
    Column(
        Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 24.dp).semantics(mergeDescendants = true) { liveRegion = LiveRegionMode.Polite }.testTag("$tag.sharing"),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.spacedBy(16.dp),
    ) {
        LoadingIndicator(Modifier.size(56.dp))
        Text(label, style = MaterialTheme.typography.titleMedium, color = MaterialTheme.colorScheme.onSurface)
    }
}

/** Snooze's page: its title with back, then each duration and Custom. Null from [onChoose] is back. */
@Composable
private fun SnoozePage(tag: String, onCustom: () -> Unit, onChoose: (SnoozeFor?) -> Unit) {
    val rows = listOf(
        SnoozeFor.Day to R.string.snooze_day,
        SnoozeFor.ThreeDays to R.string.snooze_three_days,
        SnoozeFor.Week to R.string.snooze_week,
        SnoozeFor.Month to R.string.snooze_month,
        SnoozeFor.Indefinite to R.string.snooze_indefinite,
    )
    SubPage(stringResource(R.string.action_snooze), "$tag.snooze", onBack = { onChoose(null) }) {
        rows.forEachIndexed { i, (duration, label) ->
            MenuRow(AnnotatedString(stringResource(label)), HocketIcons.Filled.Bedtime, connectedShape(i, rows.size + 1), tag = "$tag.snooze.${duration.string}") { onChoose(duration) }
        }
        MenuRow(AnnotatedString(stringResource(R.string.snooze_custom)), HocketIcons.Filled.CalendarToday, connectedShape(rows.size, rows.size + 1), tag = "$tag.snooze.custom", onClick = onCustom)
    }
}

/** Share's page: its title with back, then the links and (one song) the file. Null from [onChoose] is back. */
@Composable
private fun SharePage(choices: app.hocket.share.ShareChoices, tag: String, onChoose: (ShareChoice?) -> Unit) {
    val rows = listOfNotNull(
        ShareChoice.Navidrome.takeIf { choices.navidrome },
        ShareChoice.SongLink.takeIf { choices.songLink },
        ShareChoice.File.takeIf { choices.file },
    )
    SubPage(stringResource(R.string.action_share), "$tag.share", onBack = { onChoose(null) }) {
        rows.forEachIndexed { i, choice ->
            val (label, icon, id) = when (choice) {
                ShareChoice.Navidrome -> Triple(R.string.menu_share_navidrome, HocketIcons.Filled.Share, "navidrome")
                ShareChoice.SongLink -> Triple(R.string.menu_share_song_link, HocketIcons.Filled.MusicNote, "songLink")
                ShareChoice.File -> Triple(R.string.menu_share_file, HocketIcons.Filled.Download, "file")
                ShareChoice.Genius -> error("Open in Genius is a row of the menu")
            }
            MenuRow(AnnotatedString(stringResource(label)), icon, connectedShape(i, rows.size), tag = "$tag.share.$id") { onChoose(choice) }
        }
    }
}

/**
 * The sleep timer as a sub-page: each duration (a check on the one running), end of track and, when
 * one is set, Stop. A choice sets (or, on Stop, clears) the timer and closes the menu — like Snooze,
 * the countdown keeps showing on the row that opened the sub-page. A timer with no end and not
 * stop-at-end-of-track is "stop now". Tags: `[tag].sleep`.
 */
@Composable
private fun SleepMenuPage(tag: String, timer: SleepTimer?, onBack: () -> Unit, onChoose: (SleepTimer) -> Unit) {
    val minutes = listOf(15, 30, 45, 60, 90)
    // The clock is read as the page renders, not remembered from the open: the check keeps up with a
    // page that idles, and each row sets its timer from the moment of the tap.
    val remaining = timer?.endsAt?.let { it - System.currentTimeMillis().toDouble() }
    val activeMinutes = if (timer?.stopAtEndOfTrack == true || remaining == null) null else minutes.firstOrNull { it * 60_000.0 >= remaining }
    val check: @Composable () -> Unit = { Icon(HocketIcons.Filled.Check, null, Modifier.size(20.dp), tint = MaterialTheme.colorScheme.primary) }
    val rows = minutes.size + 1 + if (timer == null) 0 else 1
    SubPage(stringResource(R.string.sleep_title), "$tag.sleep", onBack) {
        minutes.forEachIndexed { i, m ->
            MenuRow(AnnotatedString(stringResource(R.string.sleep_minutes, m)), HocketIcons.Filled.Bedtime, connectedShape(i, rows), tag = "$tag.sleep.$m",
                trailing = if (activeMinutes == m) { { check() } } else null,
            ) { onChoose(SleepTimer(System.currentTimeMillis().toDouble() + m * 60_000.0, false)) }
        }
        MenuRow(AnnotatedString(stringResource(R.string.sleep_end_of_track)), HocketIcons.Filled.MusicNote, connectedShape(minutes.size, rows), tag = "$tag.sleep.endOfTrack",
            trailing = if (timer?.stopAtEndOfTrack == true) { { check() } } else null,
        ) { onChoose(SleepTimer(null, true)) }
        if (timer != null) MenuRow(AnnotatedString(stringResource(R.string.sleep_stop)), HocketIcons.Filled.Stop, connectedShape(minutes.size + 1, rows),
            destructive = true, tag = "$tag.sleep.stop") { onChoose(SleepTimer(null, false)) }
    }
}

/**
 * Add to playlist as a sub-page: the picker sheet's own content ([PlaylistPickerBody], its headline
 * given way for the sub-page header, and its subject line too: the menu's header already names the
 * song; its list one fixed height so the sheet stays menu-sized and still). Back returns
 * to the menu; Done or a created playlist closes it.
 */
@Composable
private fun PlaylistMenuPage(tag: String, target: ActionTarget, onBack: () -> Unit, onClose: () -> Unit) {
    SubPage(stringResource(R.string.action_add_to_playlist), "$tag.picker", onBack, horizontal = 0.dp) {
        PlaylistPickerBody(target, onClose, modifier = Modifier.imePadding(), listHeight = 420.dp, navigationBarPad = false, showSubject = false)
    }
}

/** Go to artist for a song of several artists: one row per artist ([choices]: id and name), each going there. */
@Composable
private fun ArtistsMenuPage(tag: String, choices: List<Pair<String, String>>, onBack: () -> Unit, onPick: (String) -> Unit) {
    SubPage(stringResource(R.string.action_go_to_artist), "$tag.artists", onBack) {
        choices.forEachIndexed { i, (id, name) ->
            MenuRow(AnnotatedString(name), HocketIcons.Filled.Person, connectedShape(i, choices.size), tag = "$tag.artists.$i") { onPick(id) }
        }
    }
}

/**
 * A sub-page of the menu (Share, Snooze): a back arrow and the row's name as its title, then its
 * own connected list. [horizontal] insets the list (the header keeps the usual inset): the add to
 * playlist page passes 0 to hold the picker, which indents itself like it does on its own sheet.
 * Tags: `[tag].back`, `[tag].title`.
 */
@Composable
private fun SubPage(title: String, tag: String, onBack: () -> Unit, horizontal: Dp = 16.dp, rows: @Composable () -> Unit) {
    Column(Modifier.fillMaxWidth().padding(top = 8.dp)) {
        Row(Modifier.fillMaxWidth().heightIn(min = 48.dp).padding(start = 16.dp, end = 16.dp, bottom = 8.dp), verticalAlignment = Alignment.CenterVertically) {
            IconButton(onClick = onBack, modifier = Modifier.padding(start = 4.dp).testTag("$tag.back")) {
                Icon(HocketIcons.AutoMirrored.Filled.ArrowBack, stringResource(R.string.action_back))
            }
            Text(title, style = MaterialTheme.typography.titleMedium, color = MaterialTheme.colorScheme.onSurface, maxLines = 1, overflow = TextOverflow.Ellipsis,
                modifier = Modifier.padding(start = 8.dp).semantics { heading() }.testTag("$tag.title"))
        }
        Column(Modifier.fillMaxWidth().padding(start = horizontal, end = horizontal), verticalArrangement = Arrangement.spacedBy(2.dp)) { rows() }
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
internal fun MenuRow(
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
