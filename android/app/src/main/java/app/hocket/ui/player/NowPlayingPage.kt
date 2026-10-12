package app.hocket.ui.player

import androidx.compose.animation.AnimatedContent
import androidx.compose.animation.animateColorAsState
import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.animation.core.tween
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.scaleIn
import androidx.compose.animation.togetherWith
import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.interaction.collectIsPressedAsState
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.FilledIconToggleButton
import androidx.compose.material3.FilledTonalIconButton
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.IconButtonDefaults
import androidx.compose.material3.IconToggleButtonColors
import androidx.compose.material3.LocalContentColor
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.SplitButtonDefaults
import androidx.compose.material3.SplitButtonLayout
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.material3.ripple
import androidx.compose.runtime.Composable
import androidx.compose.runtime.derivedStateOf
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.clipToBounds
import androidx.compose.ui.draw.drawWithContent
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.graphics.BlendMode
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.CompositingStrategy
import androidx.compose.ui.graphics.RectangleShape
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.hapticfeedback.HapticFeedbackType
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.AlignmentLine
import androidx.compose.ui.layout.FirstBaseline
import androidx.compose.ui.layout.LastBaseline
import androidx.compose.ui.layout.Layout
import androidx.compose.ui.layout.layout
import androidx.compose.ui.layout.onGloballyPositioned
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.LocalHapticFeedback
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.CustomAccessibilityAction
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.customActions
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.onClick
import androidx.compose.ui.semantics.role
import androidx.compose.ui.semantics.selected
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Constraints
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.IntOffset
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import app.hocket.R
import app.hocket.core.ArtworkSizes
import app.hocket.core.Commands
import app.hocket.core.SettingKeys
import app.hocket.core.api.ArtworkEdge
import app.hocket.core.api.ArtworkLayout
import app.hocket.core.api.Command
import app.hocket.core.api.DeviceInfo
import app.hocket.core.api.Platform
import app.hocket.core.api.QueueSource
import app.hocket.core.api.TrackSummary
import app.hocket.ui.LocalCoreClient
import app.hocket.ui.a11y.LocalReducedMotion
import app.hocket.ui.components.Artwork
import app.hocket.ui.components.PlaylistPicker
import app.hocket.ui.components.RatingStars
import app.hocket.ui.components.formatClock
import app.hocket.ui.icons.HocketIcons
import app.hocket.ui.lyrics.LyricsPage
import app.hocket.ui.queue.QueueList
import app.hocket.ui.theme.ArtworkColors
import app.hocket.ui.theme.Motion
import kotlin.math.abs
import kotlin.math.roundToInt

/**
 * The full player (#40's prototypes, made Material 3 Expressive). Top to bottom:
 *
 * - the drag handle (a tap collapses the player);
 * - the mode area: the big artwork in [PlayerMode.Artwork], or the lyrics or the queue in its place,
 *   straight on the player's backdrop (the artwork then sits small beside the title);
 * - the title row: a thumbnail slot (lyrics / queue), title and "artist · album" (a tap opens the
 *   song's details in a sheet), the star rating (draggable; hidden by `display.playerRating`) and a
 *   tonal add-to-playlist button;
 * - spread over what is left: the wavy seek bar with elapsed and
 *   remaining time and, between them, "Playing on <device>" (only while another device plays) or
 *   the current notice; the transport button group (play/pause shows a loading indicator while
 *   buffering); and the action row: the queue's name as a split button (the name opens the queue,
 *   the chevron the queue switcher), the device switcher, lyrics and the song menu ([PlayerLayout]).
 *
 * There is ONE artwork ([PlayerArtwork]), drawn over the page and moved in a graphics layer between
 * the big slot and the thumbnail slot as [modeFraction] goes 0 (artwork) to 1 (another mode); both
 * slots are measured into [hero], which the sheet's flying artwork also lands on. Swiping it skips,
 * with the neighbours' covers ([neighbours]) sliding in ([SkipCarousel]).
 *
 * At least a screen tall; on short screens or at large font sizes the whole page scrolls, without
 * an overscroll effect (a swipe past either end, or on a page that fits, must not stretch the
 * player). [position] is read in the draw phase (the seek bar) and once a second (its labels),
 * never here.
 */
@Composable
internal fun FullPlayer(
    mode: PlayerMode,
    onMode: (PlayerMode) -> Unit,
    onCollapse: () -> Unit,
    onOpenAlbum: (String) -> Unit,
    onOpenArtist: (String) -> Unit,
    position: () -> Long,
    hero: HeroGeometry,
    modeFraction: () -> Float,
    artworkHidden: () -> Boolean,
    artworkScale: () -> Float,
    lyricsVisible: Boolean,
    onPreviewToggle: (Color) -> Unit,
    immersive: ArtworkLayout? = null,
    carousel: SkipCarousel = remember { SkipCarousel() },
    neighbours: Neighbours = Neighbours.None,
    neighbourEdges: Pair<ArtworkEdge?, ArtworkEdge?> = null to null,
    headerAlpha: () -> Float = { 1f },
) {
    val client = LocalCoreClient.current
    val entry by client.nowPlaying.collectAsStateWithLifecycle()
    val settings by client.settings.collectAsStateWithLifecycle()
    val showRating = settings[SettingKeys.DISPLAY_PLAYER_RATING]?.value?.trim() != "false"
    val track = entry?.track
    val density = LocalDensity.current
    var handoff by remember { mutableStateOf(false) }
    var queues by remember { mutableStateOf(false) }
    var sleepSheet by remember { mutableStateOf(false) }
    var info by remember { mutableStateOf(false) }
    var lyricsTiming by remember { mutableStateOf(false) }
    // While the lyrics show, the song menu offers their timing (the row that sat under them).
    val lyricsDoc = if (mode == PlayerMode.Lyrics) app.hocket.ui.lyrics.rememberCurrentLyrics().lyrics else null
    val timingLabel = stringResource(R.string.player_lyrics_timing)
    val songMenu = app.hocket.ui.components.rememberSongMenu()
    val sleepLabel = stringResource(R.string.player_sleep_timer)
    val resources = androidx.compose.ui.platform.LocalResources.current
    var addTo by remember { mutableStateOf(false) }
    hero.artInset = with(density) { PAGE_PADDING.toPx() }
    hero.artMax = with(density) { MAX_ART.toPx() }
    hero.artPad = with(density) { ART_PAD.toPx() }
    val openMenu = {
        // The song menu is hosted at the app level, not in this (moving) sheet; the player adds
        // the sleep timer as its extra (it has no other place in the player).
        val sleep = client.sleepTimer.value
        val sleepState = sleep?.let { t -> t.endsAt?.let { resources.getString(R.string.sleep_active, formatClock((it - System.currentTimeMillis()).toLong().coerceAtLeast(0))) } ?: resources.getString(R.string.sleep_active_end_of_track) }
        track?.let {
            val timing = lyricsDoc?.let { doc ->
                app.hocket.ui.components.SongMenuExtra(timingLabel, HocketIcons.Filled.Tune, onClick = { lyricsTiming = true },
                    supporting = resources.getString(R.string.lyrics_offset_value, doc.offsetMs), testTag = "player.lyricsTiming")
            }
            songMenu.open(it, extras = listOfNotNull(timing, app.hocket.ui.components.SongMenuExtra(sleepLabel, HocketIcons.Filled.Bedtime,
                onClick = { sleepSheet = true }, supporting = sleepState, highlighted = sleep != null, testTag = "player.sleep")))
        }
        Unit
    }
    BoxWithConstraints(Modifier.fillMaxSize()) {
        val viewport = maxHeight
        val pageWidth = maxWidth
        val scroll = rememberScrollState()
        val header = @Composable { PlayerHandle(onCollapse) }
        Box(Modifier.fillMaxSize().verticalScroll(scroll, overscrollEffect = null).testTag("player.page")) {
            PlayerLayout(
                minHeight = viewport,
                hero = hero,
                // An immersive artwork runs under the handle, and the artwork is drawn over the
                // page: the page keeps the handle's room (invisible) and the real one goes on top.
                header = { if (immersive != null) Box(Modifier.graphicsLayer { alpha = 0f }.clearAndSetSemantics { }) { header() } else Box(Modifier.graphicsLayer { alpha = headerAlpha() }) { header() } },
                area = { ModeArea(mode, hero, lyricsVisible) },
                title = { PlayerTitle(mode, hero, showRating, carousel, neighbours, onInfo = { info = true }, onAddTo = { addTo = true }) },
                items = listOf(
                    { PlayerSeek(position) },
                    {
                        val playing by client.isPlaying.collectAsStateWithLifecycle()
                        val transport by client.transport.collectAsStateWithLifecycle()
                        // Narrow screens (display size "largest" leaves ~320 dp): a lower transport.
                        TransportRow(
                            playing = playing,
                            buffering = transport.buffering,
                            onPrevious = { client.dispatch(Command.Previous) },
                            onToggle = { client.dispatch(Command.TogglePlay) },
                            onNext = { client.dispatch(Command.Next) },
                            height = if (pageWidth < 360.dp) 68.dp else TRANSPORT_HEIGHT,
                            modifier = Modifier.padding(horizontal = PAGE_PADDING),
                        )
                    },
                    {
                        PlayerActions(
                            mode = mode,
                            onMode = onMode,
                            switcherOpen = queues,
                            onSwitchQueue = { queues = true },
                            onConnect = { handoff = true },
                            onMore = openMenu,
                            modifier = Modifier.navigationBarsPadding().padding(start = PAGE_PADDING, end = PAGE_PADDING, bottom = 16.dp),
                        )
                    },
                ),
            )
        }
        PlayerArtwork(
            coverArt = track?.coverArt,
            title = track?.title,
            hero = hero,
            modeFraction = modeFraction,
            hidden = artworkHidden,
            scale = artworkScale,
            thumbMode = mode != PlayerMode.Artwork,
            onShowArtwork = { onMode(PlayerMode.Artwork) },
            onPreviewToggle = onPreviewToggle,
            immersive = immersive,
            carousel = carousel,
            neighbours = neighbours,
            neighbourEdges = neighbourEdges,
        )
        if (immersive != null) {
            // Its colours follow the artwork's top (dark over a light top), not the controls below.
            val fg = immersiveHeaderColor(immersive)
            val scheme = MaterialTheme.colorScheme.copy(onSurface = fg, onSurfaceVariant = fg.copy(alpha = 0.8f))
            MaterialTheme(colorScheme = scheme) {
                androidx.compose.runtime.CompositionLocalProvider(LocalContentColor provides fg) {
                    Box(Modifier.offset { IntOffset(0, -scroll.value) }.graphicsLayer { alpha = headerAlpha() }) { header() }
                }
            }
        }
    }
    if (handoff) HandoffSheet(onDismiss = { handoff = false })
    if (queues) QueueSwitcherSheet(onDismiss = { queues = false })
    if (sleepSheet) SleepTimerSheet(onDismiss = { sleepSheet = false })
    if (lyricsTiming) app.hocket.ui.lyrics.LyricsTimingSheet(onDismiss = { lyricsTiming = false })
    if (info) SongInfoSheet(onDismiss = { info = false }, onOpenAlbum = { info = false; onOpenAlbum(it) }, onOpenArtist = { info = false; onOpenArtist(it) })
    if (addTo && track != null) PlaylistPicker(Commands.tracks(listOf(track.id)), onDismiss = { addTo = false })
}

/** The page's side margins (everything aligns to them), the artwork's largest size and its vertical margin. */
internal val PAGE_PADDING = 20.dp
private val MAX_ART = 520.dp
private val ART_PAD = 8.dp
/** The least room the mode area gets. */
private val MIN_AREA = 220.dp
/** The transport's height (the skip buttons are this square). */
private val TRANSPORT_HEIGHT = 84.dp

/** The least gap above each of the [PlayerLayout] items (seek bar, transport, actions). */
private val ITEM_GAPS = listOf(4.dp, 12.dp, 16.dp)

/**
 * The page's layout: the header, the mode area at the artwork's size (as large as the width
 * allows, up to [MAX_ART], shrinking on short screens but never below what [MIN_AREA] leaves; an
 * immersive artwork the full width from the top of the page, under the header), the title right
 * below it, and the [items] spread evenly over what is left of [minHeight] (the same share above
 * each and below the last, on top of [ITEM_GAPS]). The same in every mode: lyrics and the queue
 * take the artwork's place, so nothing below moves when the mode changes. Inside the page's
 * vertical scroll, so it can be taller than the screen. The artwork's side goes to
 * [HeroGeometry.artSide].
 */
@Composable
private fun PlayerLayout(
    minHeight: Dp,
    hero: HeroGeometry,
    header: @Composable () -> Unit,
    area: @Composable () -> Unit,
    title: @Composable () -> Unit,
    items: List<@Composable () -> Unit>,
) {
    Layout(contents = listOf(header, area, title) + items) { slots, constraints ->
        val w = constraints.maxWidth
        val loose = Constraints(maxWidth = w)
        val hp = slots[0].map { it.measure(loose) }
        val tp = slots[2].map { it.measure(loose) }
        val ip = slots.drop(3).map { s -> s.map { it.measure(loose) } }
        val hh = hp.maxOfOrNull { it.height } ?: 0
        // The title row less what its folding stars have taken off: the area gets that back.
        val fold = hero.titleFold.roundToInt()
        val th = (tp.maxOfOrNull { it.height } ?: 0) + fold
        val ih = ip.map { p -> p.maxOfOrNull { it.height } ?: 0 }
        // An empty item (no resume offer) takes no gap either.
        val gaps = ih.mapIndexed { i, h -> if (h > 0) ITEM_GAPS.getOrElse(i) { 0.dp }.roundToPx() else 0 }
        val rest = ih.sum() + gaps.sum()
        val view = minHeight.roundToPx()
        val pad = ART_PAD.roundToPx()
        val minArea = MIN_AREA.roundToPx()
        val maxSide = minOf(w - 2 * PAGE_PADDING.roundToPx(), MAX_ART.roundToPx())
        val side = if (hero.immersive) w else (view - hh - th - rest - 2 * pad).coerceIn(minOf(minArea - 2 * pad, maxSide), maxSide).coerceAtLeast(0)
        hero.artSide = side.toFloat()
        hero.headerHeight = hh.toFloat()
        val areaH = if (hero.immersive) (side - hh).coerceAtLeast(0) else side + 2 * pad
        val spare = (view - hh - areaH - th - rest).coerceAtLeast(0)
        val shares = ih.count { it > 0 } + 1
        val share = spare.toFloat() / shares
        val ap = slots[1].map { it.measure(Constraints.fixed(w, areaH + fold)) }
        var y = hh + areaH + th
        val tops = ih.mapIndexed { i, h -> if (h > 0) { y += gaps[i] + share.roundToInt(); val top = y; y += h; top } else y }
        layout(w, maxOf(y + share.roundToInt(), view)) {
            hp.forEach { it.place(0, 0) }
            ap.forEach { it.place(0, hh) }
            tp.forEach { it.place(0, hh + areaH + fold) }
            ip.forEachIndexed { i, p -> p.forEach { it.place(0, tops[i]) } }
        }
    }
}

/**
 * The sheet's drag handle (Material's bottom sheet handle) under the status bar. A tap closes the
 * player, as the handle of a modal bottom sheet does; dragging anywhere on the page moves it.
 */
@Composable
private fun PlayerHandle(onCollapse: () -> Unit) {
    val label = stringResource(R.string.player_collapse)
    Box(Modifier.fillMaxWidth().statusBarsPadding().height(HANDLE_AREA), contentAlignment = Alignment.Center) {
        Box(
            Modifier
                .size(width = 64.dp, height = HANDLE_AREA)
                .clip(RoundedCornerShape(16.dp))
                .clickable(onClickLabel = label, role = Role.Button, onClick = onCollapse)
                .semantics { contentDescription = label }
                .testTag("player.collapse"),
            contentAlignment = Alignment.Center,
        ) {
            Box(Modifier.size(width = 32.dp, height = 4.dp).clip(CircleShape).background(LocalContentColor.current.copy(alpha = 0.4f)))
        }
    }
}

private val HANDLE_AREA = 48.dp

/**
 * The mode area: empty in artwork mode (the one artwork is drawn over it), else Lyrics or the Queue
 * in the artwork's slot ([ModeSlot]), crossfading with a slight grow as the artwork shrinks away.
 * Its bounds go to [hero] (the big artwork sits at their top). Lists fade out at the slot's top and
 * bottom edges.
 */
@Composable
private fun ModeArea(mode: PlayerMode, hero: HeroGeometry, lyricsVisible: Boolean) {
    val reduced = LocalReducedMotion.current
    Box(Modifier.fillMaxSize().onGloballyPositioned { hero.area = hero.measure(it) }) {
        AnimatedContent(
            targetState = mode,
            transitionSpec = {
                if (reduced) fadeIn(tween(0)) togetherWith fadeOut(tween(0))
                else (fadeIn(tween(220, delayMillis = 60)) + scaleIn(tween(260, delayMillis = 60), initialScale = 0.96f)) togetherWith fadeOut(tween(120))
            },
            label = "playerMode",
            modifier = Modifier.fillMaxSize(),
        ) { m ->
            when (m) {
                PlayerMode.Artwork, PlayerMode.About -> Spacer(Modifier.fillMaxSize())
                // The rows' own 16 dp start padding plus 4 dp: aligned with the page's 20 dp margins.
                PlayerMode.Queue -> ModeSlot {
                    QueueList(Modifier.fillMaxSize(), contentPadding = PaddingValues(horizontal = PAGE_PADDING - 16.dp), controlsAtBottom = true, listModifier = Modifier.fadingEdges(top = 32.dp, bottom = 0.dp))
                }
                PlayerMode.Lyrics -> ModeSlot { LyricsPage(visible = lyricsVisible, modifier = Modifier.fillMaxSize(), embedded = true, listModifier = Modifier.fadingEdges(top = LYRICS_FADE, bottom = LYRICS_FADE)) }
            }
        }
    }
}

/**
 * Where lyrics and the queue sit: the artwork's slot (its top, down to the title row, which gives it
 * the room its folded stars took; with an immersive artwork, the area under the handle less the
 * usual margin), across the page's full width, straight
 * on the player's backdrop with no container. Content is clipped to the slot; the lists fade at its
 * edges.
 */
@Composable
private fun ModeSlot(content: @Composable () -> Unit) {
    Box(
        Modifier
            .fillMaxSize()
            .layout { measurable, constraints ->
                val w = constraints.maxWidth
                val h = constraints.maxHeight
                val pad = ART_PAD.roundToPx()
                // The artwork's height plus whatever the title row's folded stars gave the area.
                val sh = h - 2 * pad
                val p = measurable.measure(Constraints.fixed(w, sh.coerceAtLeast(0)))
                layout(w, h) { p.place(0, pad) }
            }
            .clipToBounds()
            .testTag("player.panel"),
    ) { content() }
}

/** How far the lyric lines fade in and out at the slot's top and bottom. */
private val LYRICS_FADE = 72.dp

/**
 * Content scrolling out of the mode slot fades at its top and bottom edges instead of being cut:
 * over [top] and [bottom] (the lyrics' big lines need the longer fade to dissolve rather than be
 * sliced by the slot's edge).
 */
private fun Modifier.fadingEdges(top: Dp = 20.dp, bottom: Dp = 28.dp): Modifier = this
    .graphicsLayer { compositingStrategy = CompositingStrategy.Offscreen }
    .drawWithContent {
        drawContent()
        val top = top.toPx().coerceAtLeast(1f)
        val bottom = bottom.toPx().coerceAtLeast(1f)
        // Eased: mostly gone over the outer half, so a line there dissolves rather than reads cut.
        drawRect(Brush.verticalGradient(0f to Color.Transparent, 0.5f to Color.Black.copy(alpha = 0.3f), 1f to Color.Black, startY = 0f, endY = top), blendMode = BlendMode.DstIn, size = size.copy(height = top))
        drawRect(Brush.verticalGradient(0f to Color.Black, 0.5f to Color.Black.copy(alpha = 0.3f), 1f to Color.Transparent, startY = size.height - bottom, endY = size.height), blendMode = BlendMode.DstIn, topLeft = Offset(0f, size.height - bottom), size = size.copy(height = bottom))
    }

/** The title row ([TitleRow]), or what to do when nothing is queued. */
@Composable
private fun PlayerTitle(mode: PlayerMode, hero: HeroGeometry, showRating: Boolean, carousel: SkipCarousel, neighbours: Neighbours, onInfo: () -> Unit, onAddTo: () -> Unit) {
    val client = LocalCoreClient.current
    val entry by client.nowPlaying.collectAsStateWithLifecycle()
    val track = entry?.track
    Column(Modifier.fillMaxWidth().padding(horizontal = PAGE_PADDING)) {
        if (track == null) {
            Column(Modifier.fillMaxWidth().padding(vertical = 16.dp), horizontalAlignment = Alignment.CenterHorizontally) {
                Text(stringResource(R.string.empty_queue_title), style = MaterialTheme.typography.headlineSmall)
                Text(stringResource(R.string.empty_queue_body), style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant, textAlign = TextAlign.Center)
            }
        } else {
            TitleRow(track, compact = mode != PlayerMode.Artwork, hero, showRating, carousel, neighbours, onInfo, onAddTo)
        }
    }
}

/** The device holding the transport lease (playing), when it is not this one. */
@Composable
private fun remoteDevice(): DeviceInfo? {
    val client = LocalCoreClient.current
    val owns by client.ownsTransport.collectAsStateWithLifecycle()
    val devices by client.devices.collectAsStateWithLifecycle()
    val transport by client.transport.collectAsStateWithLifecycle()
    if (owns) return null
    val owner = transport.lease.owner
    return devices.firstOrNull { it.id == owner && !it.isSelf } ?: devices.firstOrNull { it.playing && !it.isSelf }
}

/** The wavy seek bar with elapsed and remaining time, and the status line between them ([SeekStatus]). */
@Composable
private fun PlayerSeek(position: () -> Long) {
    val client = LocalCoreClient.current
    val entry by client.nowPlaying.collectAsStateWithLifecycle()
    val playing by client.isPlaying.collectAsStateWithLifecycle()
    val reducedMotion = LocalReducedMotion.current
    val track = entry?.track ?: return
    Column(Modifier.fillMaxWidth().padding(horizontal = PAGE_PADDING)) {
        WavySeekBar(position = position, durationMs = track.durationMs.toLong(), playing = playing && !reducedMotion, onSeek = { client.dispatch(Commands.seekTo(it)) }, centre = { SeekStatus() })
    }
}

/**
 * One line between the times: a problem (the error colour) first; else "Playing on <device>" in the
 * accent colour, only while another device holds the lease; else the sleep timer, or autoplay's
 * reason for this song. Nothing otherwise.
 */
@Composable
private fun SeekStatus() {
    val client = LocalCoreClient.current
    val entry by client.nowPlaying.collectAsStateWithLifecycle()
    val playerNotice by client.playerNotice.collectAsStateWithLifecycle()
    val sleep by client.sleepTimer.collectAsStateWithLifecycle()
    val notice = playerNoticeText(playerNotice)
    val remote = remoteDevice()
    val owns by client.ownsTransport.collectAsStateWithLifecycle()
    val style = MaterialTheme.typography.labelLarge
    @Composable fun line(text: String, color: Color, tag: String, weight: FontWeight? = null) =
        Text(text, style = style, color = color, fontWeight = weight, maxLines = 1, overflow = TextOverflow.Ellipsis, textAlign = TextAlign.Center, modifier = Modifier.testTag(tag))
    when {
        notice != null -> line(notice, MaterialTheme.colorScheme.error, "player.notice")
        !owns -> line(stringResource(R.string.player_playing_on, remote?.name ?: stringResource(R.string.player_another_device)), MaterialTheme.colorScheme.primary, "player.playingOn", FontWeight.Bold)
        sleep != null -> {
            val t = sleep!!
            line(t.endsAt?.let { stringResource(R.string.sleep_active, formatClock((it - System.currentTimeMillis()).toLong().coerceAtLeast(0))) } ?: stringResource(R.string.sleep_active_end_of_track), MaterialTheme.colorScheme.onSurfaceVariant, "player.sleepLine")
        }
        else -> (entry?.item?.source as? QueueSource.Autoplay)?.let { line(stringResource(R.string.player_autoplay_reason, it.data.reason), MaterialTheme.colorScheme.tertiary, "player.autoplayReason") }
    }
}

/**
 * Title and "artist · album" as one target that opens the song's details ([SongInfoSheet]), the
 * star rating under them (when [showRating]), and add to playlist (a tonal round button). In a
 * non-artwork mode a thumbnail slot opens at the start of the row (its width follows
 * [HeroGeometry.modeFraction] in the layout phase); the one artwork lands in it. The thumbnail is
 * 48 dp with the title and artist centred on it (or, where they span more, from the title's cap
 * height to the artist's baseline). The stars show in artwork mode only, folding away with the
 * mode change ([HeroGeometry.titleFold]: the mode area takes the room, so nothing below moves). While the artwork
 * is swiped, the neighbour's title and artist crossfade in with the swipe.
 */
@Composable
private fun TitleRow(track: TrackSummary, compact: Boolean, hero: HeroGeometry, showRating: Boolean, carousel: SkipCarousel, neighbours: Neighbours, onInfo: () -> Unit, onAddTo: () -> Unit) {
    val client = LocalCoreClient.current
    val infoLabel = stringResource(R.string.player_about_title)
    val trackDesc = stringResource(R.string.player_track_a11y, track.title, track.artist ?: stringResource(R.string.unknown_artist))
    val previous = if (carousel.restartPeek) track else neighbours.previous?.track
    val capHeight = rememberCapHeightPx(MaterialTheme.typography.headlineSmallEmphasized.fontSize)
    // The thumbnail's side as the last layout worked it out (it follows the title and artist lines'
    // metrics, which change only with the font scale).
    var thumbSide by remember { mutableIntStateOf(0) }
    // The stars show in artwork mode only: they fade and fold away as the artwork flies to the
    // thumbnail (and stay composed until that has finished).
    val starsLeft by remember(hero) { derivedStateOf { hero.modeFraction() < 1f } }
    Layout(
        contents = listOf(
            {
                // The thumbnail slot (the one artwork lands in it); empty itself.
                Box(Modifier.onGloballyPositioned { hero.small = hero.measure(it) })
            },
            {
                Column {
                    Box(Modifier.fillMaxWidth()) {
                        // The page's heading and a polite live region: it changes once per track, so a
                        // track change is announced once (never the position). A tap opens the details.
                        Column(
                            Modifier
                                .fillMaxWidth()
                                .graphicsLayer { alpha = outgoingAlpha(carousel.fraction()) }
                                .clip(RoundedCornerShape(12.dp))
                                .clickable(onClickLabel = infoLabel, role = Role.Button, onClick = onInfo)
                                .semantics(mergeDescendants = true) { heading(); liveRegion = LiveRegionMode.Polite; contentDescription = trackDesc }
                                .testTag("player.title"),
                        ) { TrackLines(track, compact) }
                        // The neighbours' lines, faded in as the artwork is swiped towards them.
                        for ((neighbour, sign) in listOf(previous to 1f, neighbours.next?.track to -1f)) {
                            if (neighbour == null || neighbour === track) continue
                            Column(Modifier.matchParentSize().graphicsLayer { alpha = incomingAlpha(carousel.fraction() * sign) }.clearAndSetSemantics { }) { TrackLines(neighbour, compact = true, tagged = false) }
                        }
                    }
                    if (showRating && (!compact || starsLeft)) {
                        RatingStars(
                            track.rating.toInt(),
                            onRate = { client.dispatch(Commands.rateTrack(track.id, it)) },
                            starSize = STAR_SIZE,
                            spacing = 8.dp,
                            tint = MaterialTheme.colorScheme.primary,
                            emptyTint = MaterialTheme.colorScheme.onSurfaceVariant.copy(alpha = 0.7f),
                            // Right under the artist line: the stars at the top of their 48 dp row, whose
                            // touch area hangs below them past the room it takes in the layout.
                            verticalAlignment = Alignment.Top,
                            modifier = Modifier
                                .then(if (compact) Modifier.clearAndSetSemantics { } else Modifier)
                                .layout { m, c ->
                                    val p = m.measure(c)
                                    val gap = STAR_GAP.roundToPx()
                                    val full = p.height - STAR_TOUCH_OVERHANG.roundToPx() + gap
                                    layout(p.width, (full * (1f - hero.modeFraction())).roundToInt().coerceAtLeast(0)) { p.place(0, gap) }
                                }
                                .graphicsLayer { alpha = outgoingAlpha(carousel.fraction()) * (1f - 2f * hero.modeFraction()).coerceIn(0f, 1f) },
                        )
                    }
                }
            },
            {
                FilledTonalIconButton(onClick = onAddTo, shapes = IconButtonDefaults.shapes(), modifier = Modifier.size(48.dp).testTag("player.addToPlaylist")) {
                    Icon(HocketIcons.AutoMirrored.Filled.PlaylistAdd, stringResource(R.string.player_add_to_playlist))
                }
            },
        ),
        modifier = Modifier.fillMaxWidth().padding(top = 16.dp, bottom = 4.dp),
    ) { (thumbSlot, text, add), constraints ->
        val w = constraints.maxWidth
        val gap = THUMB_GAP.roundToPx()
        val addGap = 12.dp.roundToPx()
        val a = add.first().measure(Constraints())
        // The thumbnail's width opens with the mode (it lands as the mode animates).
        val side0 = thumbSide.takeIf { it > 0 } ?: HeroGeometry.SMALL_SIZE.roundToPx()
        val reserve = ((side0 + gap) * hero.modeFraction()).roundToInt()
        val textW = (w - reserve - addGap - a.width).coerceAtLeast(0)
        val t = text.first().measure(Constraints(minWidth = textW, maxWidth = textW))
        // The thumbnail and the title + artist pair: their span (the title's cap height to the
        // artist's baseline) centred on it, at least [THUMB_MIN]; large fonts on narrow screens cap
        // it (a share of the row), centred the same way.
        val first = t[FirstBaseline]
        val last = t[LastBaseline]
        val top = if (first == AlignmentLine.Unspecified) 0 else (first - capHeight).roundToInt().coerceAtLeast(0)
        val bottom = if (last == AlignmentLine.Unspecified) t.height else last
        val span = bottom - top
        val side = span.coerceIn(THUMB_MIN.roundToPx(), minOf(THUMB_MAX.roundToPx(), (w * THUMB_MAX_SHARE).roundToInt()))
        if (side != thumbSide) thumbSide = side
        val thumb = thumbSlot.first().measure(Constraints.fixed(side, side))
        val h = maxOf(t.height, a.height)
        val textY = (h - t.height) / 2
        // How much the folding stars have taken off the row: the mode area takes it, so nothing
        // below moves.
        val starsFull = if (showRating) (48.dp - STAR_TOUCH_OVERHANG + STAR_GAP).roundToPx() else 0
        val folded = (starsFull * hero.modeFraction()).roundToInt()
        hero.titleFold = (maxOf(t.height + folded, a.height) - h).toFloat()
        layout(w, h) {
            thumb.place(0, textY + top + (span - side) / 2)
            t.place(reserve, textY)
            a.place(w - a.width, (h - a.height) / 2)
        }
    }
}

/** The stars' size in the title block. */
private val STAR_SIZE = 20.dp
/**
 * How much of the stars' 48 dp touch row hangs below the room it takes in the layout: the title
 * row's bottom padding and the seek bar's least gap, so it never reaches the seek bar.
 */
private val STAR_TOUCH_OVERHANG = 8.dp
/** The room between the artist line's box and the stars' icons (about 8 dp from its descenders to the star glyphs). */
private val STAR_GAP = 1.dp
/** How much closer than their line boxes the artist line sits to the title (about 2 dp between the glyphs). */
private val TITLE_ARTIST_PULL = 7.6.dp
/** The gap between the thumbnail and the text, and the thumbnail's bounds (it follows the text block). */
private val THUMB_GAP = 14.dp
private val THUMB_MIN = 48.dp
private val THUMB_MAX = 96.dp
/** The thumbnail's largest share of the row's width (keeps room for the text at large font sizes). */
private const val THUMB_MAX_SHARE = 0.22f

/** The cap height (px) of the system sans-serif, bold, at [fontSize]: where a title's letters start. */
@Composable
private fun rememberCapHeightPx(fontSize: androidx.compose.ui.unit.TextUnit): Float {
    val px = with(LocalDensity.current) { fontSize.toPx() }
    return remember(px) {
        val paint = android.graphics.Paint().apply {
            typeface = android.graphics.Typeface.create(android.graphics.Typeface.SANS_SERIF, android.graphics.Typeface.BOLD)
            textSize = px
        }
        val bounds = android.graphics.Rect()
        paint.getTextBounds("H", 0, 1, bounds)
        -bounds.top.toFloat()
    }
}

/** The current song's lines while the artwork is swiped [f] of the way to a neighbour: gone by half way. */
private fun outgoingAlpha(f: Float): Float = (1f - 2f * abs(f)).coerceIn(0f, 1f)

/** A neighbour's lines at [f] (towards it, 0..1): in over the second half, so the two never overlap. */
private fun incomingAlpha(f: Float): Float = (2f * f - 1f).coerceIn(0f, 1f)

/** Title (emphasized headline) and "artist · album" (muted), each ellipsised. */
@Composable
private fun TrackLines(track: TrackSummary, compact: Boolean, tagged: Boolean = true) {
    // Grouped by proximity: the artist line tucks up under the title ([TITLE_ARTIST_PULL] into the
    // two line boxes' leading), the stars keep their distance below ([STAR_GAP]).
    Text(track.title, style = MaterialTheme.typography.headlineSmallEmphasized, fontWeight = FontWeight.ExtraBold, maxLines = if (compact) 1 else 2, overflow = TextOverflow.Ellipsis)
    Row(
        Modifier.layout { m, c ->
            // Measured at its full height (the pull reports less), so a box that fits the lines
            // exactly (a neighbour's, matching the current one's) never squeezes the text.
            val pull = TITLE_ARTIST_PULL.roundToPx()
            val p = m.measure(c.copy(minHeight = 0, maxHeight = Constraints.Infinity))
            layout(p.width, (p.height - pull).coerceIn(c.minHeight, c.maxHeight)) { p.place(0, -pull) }
        },
        verticalAlignment = Alignment.CenterVertically,
    ) {
        // Untrimmed line boxes (trimming clips the text at large font scales): the pull above and
        // [STAR_GAP] below account for their leading.
        val sub = MaterialTheme.typography.bodyLarge
        val subColor = MaterialTheme.colorScheme.onSurfaceVariant
        Text(track.artist ?: stringResource(R.string.unknown_artist), style = sub, color = subColor, maxLines = 1, overflow = TextOverflow.Ellipsis,
            modifier = Modifier.weight(1f, fill = false).then(if (tagged) Modifier.testTag("player.artist") else Modifier))
        track.album?.let { album ->
            Text(stringResource(R.string.dot_separator), style = sub, color = subColor)
            Text(album, style = sub, color = subColor, maxLines = 1, overflow = TextOverflow.Ellipsis, modifier = Modifier.weight(1f, fill = false).then(if (tagged) Modifier.testTag("player.album") else Modifier))
        }
    }
}

/**
 * The row under the transport (#40): the queue's name as a split button (the name opens the queue,
 * filled with the accent while it is shown; the chevron opens the queue switcher), the device
 * switcher ([DeviceButton]), lyrics (a toggle, filled while shown) and the song menu.
 */
@Composable
private fun PlayerActions(mode: PlayerMode, onMode: (PlayerMode) -> Unit, switcherOpen: Boolean, onSwitchQueue: () -> Unit, onConnect: () -> Unit, onMore: () -> Unit, modifier: Modifier = Modifier) {
    val client = LocalCoreClient.current
    val queue by client.queue.collectAsStateWithLifecycle()
    val entry by client.nowPlaying.collectAsStateWithLifecycle()
    val name = queue.contextLabel?.takeIf { it.isNotBlank() }
        ?: (if (entry?.item?.source is QueueSource.Autoplay) stringResource(R.string.player_playing_from_autoplay) else null)
        ?: stringResource(R.string.player_mode_queue)
    val queueShown = mode == PlayerMode.Queue
    val lyricsShown = mode == PlayerMode.Lyrics
    val queueDesc = stringResource(R.string.player_queue_a11y, name)
    val switchLabel = stringResource(R.string.player_switch_queue)
    val lyricsLabel = stringResource(R.string.player_mode_lyrics)
    Row(modifier.fillMaxWidth().height(48.dp), horizontalArrangement = Arrangement.spacedBy(8.dp), verticalAlignment = Alignment.CenterVertically) {
        val colors = if (queueShown) ButtonDefaults.buttonColors() else ButtonDefaults.filledTonalButtonColors()
        SplitButtonLayout(
            leadingButton = {
                SplitButtonDefaults.LeadingButton(
                    onClick = { onMode(if (queueShown) PlayerMode.Artwork else PlayerMode.Queue) },
                    colors = colors,
                    modifier = Modifier.fillMaxWidth().heightIn(min = 48.dp).testTag("player.mode.queue")
                        .semantics { contentDescription = queueDesc; selected = queueShown },
                ) {
                    Text(name, style = MaterialTheme.typography.titleMedium, fontWeight = FontWeight.Bold, maxLines = 1, overflow = TextOverflow.Ellipsis, modifier = Modifier.weight(1f, fill = false))
                }
            },
            trailingButton = {
                val turn by animateFloatAsState(if (switcherOpen) 180f else 0f, Motion.settle, label = "chevron")
                SplitButtonDefaults.TrailingButton(
                    checked = switcherOpen,
                    onCheckedChange = { onSwitchQueue() },
                    colors = colors,
                    modifier = Modifier.heightIn(min = 48.dp).testTag("player.switchQueue").semantics { contentDescription = switchLabel },
                ) {
                    // Up: the switcher opens upwards, as a sheet.
                    Icon(HocketIcons.Filled.KeyboardArrowDown, null, Modifier.size(SplitButtonDefaults.TrailingIconSize).graphicsLayer { rotationZ = 180f + turn })
                }
            },
            modifier = Modifier.weight(1f),
        )
        DeviceButton(onConnect)
        FilledIconToggleButton(
            checked = lyricsShown,
            onCheckedChange = { onMode(if (lyricsShown) PlayerMode.Artwork else PlayerMode.Lyrics) },
            shapes = IconButtonDefaults.toggleableShapes(),
            colors = IconToggleButtonColors(
                containerColor = Color.Transparent, contentColor = LocalContentColor.current,
                disabledContainerColor = Color.Transparent, disabledContentColor = LocalContentColor.current.copy(alpha = 0.38f),
                checkedContainerColor = MaterialTheme.colorScheme.primary, checkedContentColor = MaterialTheme.colorScheme.onPrimary,
            ),
            modifier = Modifier.size(48.dp).testTag("player.mode.lyrics"),
        ) { Icon(HocketIcons.Filled.Lyrics, lyricsLabel) }
        IconButton(onClick = onMore, shapes = IconButtonDefaults.shapes(), modifier = Modifier.size(48.dp).testTag("player.more")) {
            Icon(HocketIcons.Filled.MoreVert, stringResource(R.string.action_more))
        }
    }
}

/**
 * The device switcher: the playing device's icon (this phone, or the laptop / speaker another
 * device is), tonal while this device plays and filled with the accent while it controls another
 * one. A tap opens the device picker; a long press pulls playback straight to this device. Its
 * corners tighten while pressed (the expressive icon button's press shape).
 */
@OptIn(ExperimentalFoundationApi::class)
@Composable
private fun DeviceButton(onConnect: () -> Unit) {
    val client = LocalCoreClient.current
    val owns by client.ownsTransport.collectAsStateWithLifecycle()
    val devices by client.devices.collectAsStateWithLifecycle()
    val remote = remoteDevice()
    val haptics = LocalHapticFeedback.current
    val takeOverLabel = stringResource(R.string.player_take_over)
    val interaction = remember { MutableInteractionSource() }
    val pressed by interaction.collectIsPressedAsState()
    val corner by animateFloatAsState(if (pressed) 0.25f else 0.5f, Motion.morph, label = "deviceCorner")
    val container by animateColorAsState(if (!owns) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.secondaryContainer, label = "deviceContainer")
    val content by animateColorAsState(if (!owns) MaterialTheme.colorScheme.onPrimary else MaterialTheme.colorScheme.onSecondaryContainer, label = "deviceContent")
    val playingOn = remote?.let { stringResource(R.string.player_playing_on, it.name) }
    val icon = remote?.let { deviceIcon(it.platform) } ?: if (!owns) HocketIcons.Filled.Devices else HocketIcons.Filled.PhoneAndroid
    Box(
        Modifier
            .size(48.dp)
            .graphicsLayer { shape = RoundedCornerShape(percent = (corner * 100).roundToInt()); clip = true }
            .background(container)
            .combinedClickable(
                interactionSource = interaction,
                indication = ripple(color = content),
                role = Role.Button,
                onClick = onConnect,
                onLongClickLabel = takeOverLabel,
                onLongClick = {
                    val self = devices.firstOrNull { it.isSelf }
                    if (!owns && self != null) {
                        haptics.performHapticFeedback(HapticFeedbackType.LongPress)
                        client.dispatch(Commands.handoffTo(self.id))
                    } else onConnect()
                },
            )
            .semantics { if (playingOn != null) stateDescription = playingOn }
            .testTag("player.connect"),
        contentAlignment = Alignment.Center,
    ) {
        Icon(icon, stringResource(R.string.player_connect), tint = content)
    }
}

/** A device's icon by platform: a phone, a computer, or a speaker (the coordinator). */
internal fun deviceIcon(platform: Platform): ImageVector = when (platform) {
    Platform.Android -> HocketIcons.Filled.PhoneAndroid
    Platform.Coordinator -> HocketIcons.Filled.Speaker
    else -> HocketIcons.Filled.Computer
}

/**
 * The song's details (the old About mode) in a sheet, from a tap on the title: the rating, then
 * what the library knows of the track. Album and artist rows go to their pages.
 */
@Composable
private fun SongInfoSheet(onDismiss: () -> Unit, onOpenAlbum: (String) -> Unit, onOpenArtist: (String) -> Unit) {
    ModalBottomSheet(onDismissRequest = onDismiss, sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = true), modifier = Modifier.testTag("player.info")) {
        PlayerAbout(Modifier.fillMaxWidth().navigationBarsPadding(), onOpenAlbum = onOpenAlbum, onOpenArtist = onOpenArtist)
    }
}

/**
 * The player's one artwork: laid out at the big slot's size and moved and scaled in a graphics
 * layer to wherever [HeroGeometry.playerArt] says (the big slot, the thumbnail slot, or between as
 * the mode changes), its corners going from the big radius to the thumbnail's. In artwork mode it
 * shrinks a little while paused (Navic), a horizontal swipe skips with the neighbours' covers
 * sliding in beside it ([SkipCarousel]; an immersive neighbour edge to edge with its own seam, a
 * card one as a card), and a long press toggles a whole-app dynamic colour preview from it; as a
 * thumbnail, a tap brings the artwork back. While the sheet's flying artwork is in the air
 * ([hidden]) it is not drawn.
 */
@OptIn(ExperimentalFoundationApi::class)
@Composable
internal fun PlayerArtwork(
    coverArt: String?,
    title: String?,
    hero: HeroGeometry,
    modeFraction: () -> Float,
    hidden: () -> Boolean,
    scale: () -> Float,
    thumbMode: Boolean,
    onShowArtwork: () -> Unit,
    onPreviewToggle: (Color) -> Unit,
    immersive: ArtworkLayout? = null,
    carousel: SkipCarousel = remember { SkipCarousel() },
    neighbours: Neighbours = Neighbours.None,
    neighbourEdges: Pair<ArtworkEdge?, ArtworkEdge?> = null to null,
) {
    val client = LocalCoreClient.current
    val scope = rememberCoroutineScope()
    val haptics = LocalHapticFeedback.current
    val density = LocalDensity.current
    val reduced = LocalReducedMotion.current
    val threshold = with(density) { 96.dp.toPx() }
    val seed by artworkSeed(coverArt)
    val desc = stringResource(R.string.player_artwork_a11y, title ?: "")
    val previewDesc = stringResource(R.string.player_dynamic_preview)
    val nextLabel = stringResource(R.string.action_next)
    val previousLabel = stringResource(R.string.action_previous)
    val showLabel = stringResource(R.string.player_show_artwork)
    val togglePreview = { haptics.performHapticFeedback(HapticFeedbackType.LongPress); onPreviewToggle(seed ?: ArtworkColors.seedFor(coverArt)) }
    val currentThumbMode = androidx.compose.runtime.rememberUpdatedState(thumbMode)
    val currentNeighbours = androidx.compose.runtime.rememberUpdatedState(neighbours)
    val gapPx = if (hero.immersive) 0f else hero.artInset
    hero.swipe = { carousel.offset.value * (1f - modeFraction()) }
    Box(
        Modifier
            // Laid out at its current size and place (so its bounds are what shows; it relays out
            // only while the mode or the paused size animates, never during a sheet drag).
            .layout { measurable, constraints ->
                val r = hero.playerArt(modeFraction(), scale())
                val side = r.width.roundToInt().coerceAtLeast(1)
                carousel.span = neighbourSpan(hero, gapPx)
                val placeable = measurable.measure(Constraints.fixed(side, side))
                layout(constraints.maxWidth, constraints.maxHeight) { placeable.place(r.left.roundToInt(), r.top.roundToInt()) }
            }
            .graphicsLayer {
                if (hidden() || hero.bigSlot().width <= 0f) { alpha = 0f; return@graphicsLayer }
                translationX = hero.swipe()
            }
            .then(with(carousel) {
                Modifier.gesture(client, scope, haptics, enabled = !thumbMode, reduced = reduced, hasNext = { currentNeighbours.value.next != null }, rubberThreshold = threshold)
            })
            // Long-press everywhere; a tap only as the thumbnail (back to the artwork). Pointer
            // gestures, mirrored by the actions below.
            .pointerInput(Unit) { detectTapGestures(onLongPress = { togglePreview() }, onTap = { if (currentThumbMode.value) onShowArtwork() }) }
            .testTag("player.artwork")
            .semantics {
                contentDescription = desc
                customActions = listOf(
                    CustomAccessibilityAction(nextLabel) { client.dispatch(Command.Next); true },
                    CustomAccessibilityAction(previousLabel) { client.dispatch(Command.Previous); true },
                    CustomAccessibilityAction(previewDesc) { togglePreview(); true },
                )
                if (thumbMode) onClick(showLabel) { onShowArtwork(); true }
            },
    ) {
        Box(
            Modifier.fillMaxSize().graphicsLayer {
                val f = modeFraction()
                shape = RoundedCornerShape(hero.cornerFor(f, size.width))
                clip = true
                shadowElevation = hero.elevationFor(f, this)
            },
        ) {
            Artwork(coverArt, ArtworkSizes.FULL, null, Modifier.fillMaxSize().immersiveFade(immersive?.bottom, fraction = { 1f - modeFraction() }), RectangleShape)
            if (immersive != null) ImmersiveArtworkOverlay(immersive, fraction = { 1f - modeFraction() })
        }
        // The neighbours, off screen at rest (so their covers are loaded before a swipe shows them).
        if (!thumbMode && !reduced) {
            val previous = if (carousel.restartPeek) PeekCover(coverArt, immersive?.bottom) else neighbours.previous?.let { PeekCover(it.track.coverArt, neighbourEdges.first) }
            val next = neighbours.next?.let { PeekCover(it.track.coverArt, neighbourEdges.second) }
            previous?.let { NeighbourCover(it, hero, scale, side = 1f, gapPx, tag = "player.artwork.previous") }
            next?.let { NeighbourCover(it, hero, scale, side = -1f, gapPx, tag = "player.artwork.next") }
        }
    }
}

/**
 * How far a neighbour sits from the current artwork (px): a slot's width plus [gapPx] (the side
 * margin), or, where the artwork is narrower than the page allows (short screens, large fonts),
 * far enough to wait off screen.
 */
private fun neighbourSpan(hero: HeroGeometry, gapPx: Float): Float {
    val big = hero.bigSlot()
    return maxOf(big.width + gapPx, hero.area.width - big.left, big.right).coerceAtLeast(1f)
}

/** A neighbour's cover and, when it is shown immersive, its bottom edge. */
private data class PeekCover(val coverArt: String?, val edge: ArtworkEdge?)

/**
 * A neighbour beside the player's artwork, a slot's span away on [side] (1 = before it, the
 * previous; -1 = after it, the next), in its own style: in an immersive scene an immersive
 * neighbour fills the same edge-to-edge square (its own continuation is drawn under the page by the
 * sheet) and a card neighbour is a card in the area under the handle; in a card scene, a card in
 * the same slot.
 */
@Composable
private fun NeighbourCover(cover: PeekCover, hero: HeroGeometry, scale: () -> Float, side: Float, gapPx: Float, tag: String) {
    val density = LocalDensity.current
    val cardCorner = with(density) { HeroGeometry.HERO_CORNER.toPx() }
    val immersiveNeighbour = hero.immersive && cover.edge != null
    Box(
        Modifier
            .layout { measurable, constraints ->
                val big = hero.bigSlot()
                val r = hero.playerArt(0f, scale())
                val target: Rect = when {
                    !hero.immersive || immersiveNeighbour -> big
                    else -> {
                        // A card in the immersive scene: the area under the handle, with margins.
                        val a = hero.area
                        val s = minOf(a.width - 2 * hero.artInset, a.height - 2 * hero.artPad).coerceAtLeast(1f)
                        Rect(Offset(a.center.x - s / 2, a.top + hero.artPad), androidx.compose.ui.geometry.Size(s, s))
                    }
                }
                val w = target.width.roundToInt().coerceAtLeast(1)
                val p = measurable.measure(Constraints.fixed(w, target.height.roundToInt().coerceAtLeast(1)))
                val dx = (target.left - side * neighbourSpan(hero, gapPx) - r.left).roundToInt()
                val dy = (target.top - r.top).roundToInt()
                layout(constraints.maxWidth, constraints.maxHeight) { p.place(dx, dy) }
            }
            .graphicsLayer {
                shape = RoundedCornerShape(if (immersiveNeighbour) 0f else cardCorner)
                clip = true
            }
            .testTag(tag)
            .clearAndSetSemantics { },
    ) {
        Artwork(cover.coverArt, ArtworkSizes.FULL, null, Modifier.fillMaxSize().then(if (immersiveNeighbour) Modifier.immersiveFade(cover.edge, fraction = { 1f }) else Modifier), RectangleShape)
    }
}
