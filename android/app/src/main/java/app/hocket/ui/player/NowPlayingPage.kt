package app.hocket.ui.player

import androidx.compose.animation.AnimatedContent
import androidx.compose.animation.animateContentSize
import androidx.compose.animation.animateColorAsState
import androidx.compose.animation.core.Animatable
import androidx.compose.animation.core.tween
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.scaleIn
import androidx.compose.animation.togetherWith
import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.gestures.detectHorizontalDragGestures
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.indication
import androidx.compose.foundation.interaction.MutableInteractionSource
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
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.wrapContentHeight
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.selection.selectable
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.PlaylistAdd
import androidx.compose.material.icons.automirrored.filled.QueueMusic
import androidx.compose.material.icons.filled.ArrowDropDown
import androidx.compose.material.icons.filled.Bedtime
import androidx.compose.material.icons.filled.Cast
import androidx.compose.material.icons.filled.Close
import androidx.compose.material.icons.filled.KeyboardArrowDown
import androidx.compose.material.icons.filled.Lyrics
import androidx.compose.material.icons.filled.MoreVert
import androidx.compose.material.icons.filled.Star
import androidx.compose.material.icons.filled.StarBorder
import androidx.compose.material.icons.outlined.Info
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.IconToggleButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.ripple
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
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
import androidx.compose.ui.graphics.BlendMode
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.CompositingStrategy
import androidx.compose.ui.graphics.RectangleShape
import androidx.compose.ui.graphics.TransformOrigin
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.hapticfeedback.HapticFeedbackType
import androidx.compose.ui.input.pointer.pointerInput
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
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.text.rememberTextMeasurer
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Constraints
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import app.hocket.R
import app.hocket.core.ArtworkSizes
import app.hocket.core.Commands
import app.hocket.core.api.Command
import app.hocket.core.api.QueueSource
import app.hocket.ui.LocalCoreClient
import app.hocket.ui.a11y.LocalReducedMotion
import app.hocket.ui.components.Artwork
import app.hocket.ui.components.PlaylistPicker
import app.hocket.ui.components.formatClock
import app.hocket.ui.lyrics.LyricsPage
import app.hocket.ui.queue.QueueList
import app.hocket.ui.theme.ArtworkColors
import app.hocket.ui.theme.Motion
import kotlinx.coroutines.launch
import kotlin.math.abs
import kotlin.math.roundToInt
import androidx.compose.ui.text.style.LineHeightStyle
import androidx.compose.foundation.combinedClickable

/**
 * The full player (the owner's mockup, made Material 3 Expressive). Top to bottom:
 *
 * - "Playing from" and the queue's name (a tap switches queues), with Connect (the only way to the
 *   device picker; highlighted while another device plays) and the collapse chevron;
 * - the mode area: the big artwork (at its top) in [PlayerMode.Artwork], or Lyrics / Queue / About;
 * - the title row, right under the artwork: a small thumbnail slot (non-artwork modes), title,
 *   "artist • album" links, and add to playlist and the More sheet (the rating and the sleep timer);
 * - spread over what is left: notices (resume offer, a problem, autoplay's reason, the sleep timer), the wavy seek bar with elapsed / total, the transport (play/pause shows a
 *   loading indicator while buffering), and the Lyrics / Queue / About pills ([PlayerLayout]).
 *
 * There is ONE artwork ([PlayerArtwork]), drawn over the page and moved in a graphics layer between
 * the big slot and the thumbnail slot as [modeFraction] goes 0 (artwork) to 1 (another mode); both
 * slots are measured into [hero], which the sheet's flying artwork also lands on.
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
    immersive: app.hocket.core.api.ArtworkLayout? = null,
) {
    val client = LocalCoreClient.current
    val entry by client.nowPlaying.collectAsStateWithLifecycle()
    val track = entry?.track
    val density = LocalDensity.current
    var handoff by remember { mutableStateOf(false) }
    var queues by remember { mutableStateOf(false) }
    var sleepSheet by remember { mutableStateOf(false) }
    val songMenu = app.hocket.ui.components.rememberSongMenu()
    val sleepLabel = stringResource(R.string.player_sleep_timer)
    val resources = androidx.compose.ui.platform.LocalResources.current
    var addTo by remember { mutableStateOf(false) }
    hero.artInset = with(density) { PAGE_PADDING.toPx() }
    hero.artMax = with(density) { MAX_ART.toPx() }
    hero.artPad = with(density) { 8.dp.toPx() }
    BoxWithConstraints(Modifier.fillMaxSize()) {
        val viewport = maxHeight
        val pageWidth = maxWidth
        Box(Modifier.fillMaxSize().verticalScroll(rememberScrollState(), overscrollEffect = null).testTag("player.page")) {
            PlayerLayout(
                minHeight = viewport,
                hero = hero,
                header = { PlayerHeader(onCollapse, onConnect = { handoff = true }, onSwitchQueue = { queues = true }) },
                area = { ModeArea(mode, hero, lyricsVisible) },
                title = { PlayerTitle(mode, onOpenAlbum, onOpenArtist, hero, onMore = {
                        // The song menu is hosted at the app level, not in this (moving) sheet;
                        // the player adds the sleep timer as its extra.
                        val sleep = client.sleepTimer.value
                        val sleepState = sleep?.let { t -> t.endsAt?.let { resources.getString(R.string.sleep_active, formatClock((it - System.currentTimeMillis()).toLong().coerceAtLeast(0))) } ?: resources.getString(R.string.sleep_active_end_of_track) }
                        track?.let { songMenu.open(it, extras = listOf(app.hocket.ui.components.SongMenuExtra(sleepLabel, Icons.Filled.Bedtime,
                            onClick = { sleepSheet = true }, supporting = sleepState, highlighted = sleep != null, testTag = "player.sleep"))) }
                    }, onAddTo = { addTo = true }) },
                items = listOf(
                    { PlayerNoticeLines() },
                    { PlayerSeek(position) },
                    {
                        val client = LocalCoreClient.current
                        val playing by client.isPlaying.collectAsStateWithLifecycle()
                        val transport by client.transport.collectAsStateWithLifecycle()
                        // Narrow screens (display size "largest" leaves ~320 dp): a smaller transport.
                        TransportRow(
                            playing = playing,
                            buffering = transport.buffering,
                            onPrevious = { client.dispatch(Command.Previous) },
                            onToggle = { client.dispatch(Command.TogglePlay) },
                            onNext = { client.dispatch(Command.Next) },
                            height = if (pageWidth < 360.dp) 68.dp else 80.dp,
                            modifier = Modifier.padding(horizontal = PAGE_PADDING),
                        )
                    },
                    { Column(Modifier.fillMaxWidth().navigationBarsPadding().padding(bottom = 8.dp)) { ModeBar(mode, onMode, pageWidth = pageWidth) } },
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
        )
    }
    if (handoff) HandoffSheet(onDismiss = { handoff = false })
    if (queues) QueueSwitcherSheet(onDismiss = { queues = false })
    if (sleepSheet) SleepTimerSheet(onDismiss = { sleepSheet = false })
    if (addTo && track != null) PlaylistPicker(Commands.tracks(listOf(track.id)), onDismiss = { addTo = false })
}

/** The page's side padding, the artwork's largest size, and the least room the mode area gets. */
private val PAGE_PADDING = 24.dp
private val MAX_ART = 520.dp
private val MIN_AREA = 220.dp

/** The least gap above each of the [PlayerLayout] items (notices, seek bar, transport, mode pills). */
private val ITEM_GAPS = listOf(0.dp, 4.dp, 8.dp, 12.dp)

/**
 * The page's layout. In artwork mode: the header, the artwork at the top of the mode area (as large
 * as the width allows, up to [MAX_ART], shrinking on short screens but never below what [MIN_AREA]
 * leaves), the title right below it, and the [items] spread evenly over what is left of
 * [minHeight] (the same share above each and below the last, on top of [ITEM_GAPS]). In the other
 * modes the area takes all that room (at least [MIN_AREA]) and the title and items stack at the
 * bottom with just [ITEM_GAPS]. Between the two it follows [HeroGeometry.modeFraction], read here in
 * the layout phase (the mode's animation relays out, never recomposes). The area's content is
 * always laid out at its full (other-mode) height and clipped to the visible part, so lyrics and the
 * queue do not relayout as it opens. Inside the page's vertical scroll, so it can be taller than
 * the screen. The artwork's side goes to [HeroGeometry.artSide].
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
        val th = tp.maxOfOrNull { it.height } ?: 0
        val ih = ip.map { p -> p.maxOfOrNull { it.height } ?: 0 }
        // An empty item (no notices) takes no gap either.
        val gaps = ih.mapIndexed { i, h -> if (h > 0) ITEM_GAPS.getOrElse(i) { 0.dp }.roundToPx() else 0 }
        val rest = ih.sum() + gaps.sum()
        val view = minHeight.roundToPx()
        val pad = 8.dp.roundToPx()
        val minArea = MIN_AREA.roundToPx()
        // Artwork mode: the largest artwork that leaves room for the rest; immersive, the full width
        // from the top of the page (under the header), the title right below it.
        val maxSide = minOf(w - 2 * PAGE_PADDING.roundToPx(), MAX_ART.roundToPx())
        val side = if (hero.immersive) w else (view - hh - th - rest - 2 * pad).coerceIn(minOf(minArea - 2 * pad, maxSide), maxSide).coerceAtLeast(0)
        hero.artSide = side.toFloat()
        hero.headerHeight = hh.toFloat()
        val artArea = if (hero.immersive) (side - hh).coerceAtLeast(0) else side + 2 * pad
        val spare = (view - hh - artArea - th - rest).coerceAtLeast(0)
        val shares = ih.count { it > 0 } + 1
        // Other modes: the area takes it all.
        val fullArea = maxOf(minArea, view - hh - th - rest)
        val f = hero.modeFraction().coerceIn(0f, 1f)
        val areaH = lerpF(artArea.toFloat(), fullArea.toFloat(), f).roundToInt()
        val share = spare * (1f - f) / shares
        val ap = slots[1].map { it.measure(Constraints(minWidth = w, maxWidth = w, minHeight = areaH, maxHeight = maxOf(areaH, fullArea))) }
        var y = hh + areaH + th
        val tops = ih.mapIndexed { i, h -> if (h > 0) { y += gaps[i] + share.roundToInt(); val top = y; y += h; top } else y }
        layout(w, maxOf(y + share.roundToInt(), view)) {
            hp.forEach { it.place(0, 0) }
            ap.forEach { it.place(0, hh) }
            tp.forEach { it.place(0, hh + areaH) }
            ip.forEachIndexed { i, p -> p.forEach { it.place(0, tops[i]) } }
        }
    }
}

/**
 * "Playing from" and the queue's name (its context: the album, playlist, search...; "Autoplay" once
 * autoplay has taken over a queue without one), Connect and the collapse chevron. Tapping the name
 * opens the queue switcher (the recent and pinned queues).
 */
@Composable
private fun PlayerHeader(onCollapse: () -> Unit, onConnect: () -> Unit, onSwitchQueue: () -> Unit) {
    val client = LocalCoreClient.current
    val owns by client.ownsTransport.collectAsStateWithLifecycle()
    val queue by client.queue.collectAsStateWithLifecycle()
    val entry by client.nowPlaying.collectAsStateWithLifecycle()
    val source = queue.contextLabel?.takeIf { it.isNotBlank() }
        ?: (if (entry?.item?.source is QueueSource.Autoplay) stringResource(R.string.player_playing_from_autoplay) else null)
        ?: stringResource(R.string.player_sheet_title)
    val sourceDesc = stringResource(R.string.player_playing_from_a11y, source)
    val switchLabel = stringResource(R.string.player_switch_queue)
    Row(Modifier.fillMaxWidth().statusBarsPadding().padding(start = PAGE_PADDING - 8.dp, end = 12.dp, top = 12.dp, bottom = 4.dp), verticalAlignment = Alignment.CenterVertically) {
        Row(
            Modifier.weight(1f).clip(RoundedCornerShape(12.dp))
                .clickable(onClickLabel = switchLabel, role = Role.Button, onClick = onSwitchQueue)
                .semantics(mergeDescendants = true) { contentDescription = sourceDesc }
                .padding(horizontal = 8.dp, vertical = 2.dp)
                .testTag("player.source"),
            verticalAlignment = Alignment.Bottom,
        ) {
            Column(Modifier.weight(1f, fill = false)) {
                Text(stringResource(R.string.player_playing_from), style = MaterialTheme.typography.labelLarge, color = MaterialTheme.colorScheme.onSurfaceVariant, maxLines = 1)
                Text(source, style = MaterialTheme.typography.titleLargeEmphasized, fontWeight = FontWeight.Bold, maxLines = 1, overflow = TextOverflow.Ellipsis)
            }
            Icon(Icons.Filled.ArrowDropDown, null, Modifier.padding(bottom = 2.dp), tint = MaterialTheme.colorScheme.onSurfaceVariant)
        }
        // Connect, highlighted while another device plays. Tap opens the picker; long-press pulls
        // playback straight to this device.
        val devices by client.devices.collectAsStateWithLifecycle()
        val haptics = LocalHapticFeedback.current
        val takeOverLabel = stringResource(R.string.player_take_over)
        Box(
            Modifier.size(48.dp).clip(CircleShape)
                .combinedClickable(
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
                .testTag("player.connect"),
            contentAlignment = Alignment.Center,
        ) {
            Icon(Icons.Filled.Cast, stringResource(R.string.player_connect), tint = if (!owns) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.onSurfaceVariant)
        }
        IconButton(onClick = onCollapse, modifier = Modifier.testTag("player.collapse")) {
            Icon(Icons.Filled.KeyboardArrowDown, stringResource(R.string.player_collapse), Modifier.size(28.dp))
        }
    }
}

/**
 * The mode area: empty in artwork mode (the one artwork is drawn over it), else Lyrics, Queue or
 * About, crossfading with a slight grow as the artwork shrinks away. Its bounds go to [hero] (the
 * big artwork sits at their top). Lists fade out at the area's top and bottom edges.
 *
 * [PlayerLayout] gives it its visible height as the least height and its full (other-mode) height
 * as the most: the content is laid out at the full height and clipped to the visible part.
 */
@Composable
private fun ModeArea(mode: PlayerMode, hero: HeroGeometry, lyricsVisible: Boolean) {
    val reduced = LocalReducedMotion.current
    Box(
        Modifier
            .onGloballyPositioned { hero.area = hero.measure(it) }
            .clipToBounds()
            .layout { measurable, constraints ->
                val placeable = measurable.measure(Constraints.fixed(constraints.maxWidth, constraints.maxHeight))
                layout(constraints.maxWidth, constraints.minHeight) { placeable.place(0, 0) }
            },
    ) {
        AnimatedContent(
            targetState = mode,
            transitionSpec = {
                if (reduced) fadeIn(tween(0)) togetherWith fadeOut(tween(0))
                else (fadeIn(tween(220, delayMillis = 60)) + scaleIn(tween(260, delayMillis = 60), initialScale = 0.96f)) togetherWith fadeOut(tween(120))
            },
            label = "playerMode",
            modifier = Modifier.fillMaxSize(),
        ) { m ->
            val content = Modifier.fillMaxSize().fadingEdges()
            when (m) {
                PlayerMode.Artwork -> Spacer(Modifier.fillMaxSize())
                PlayerMode.Queue -> QueueList(Modifier.fillMaxSize(), listModifier = Modifier.fadingEdges()) // header stays crisp; only the list fades
                PlayerMode.Lyrics -> LyricsPage(visible = lyricsVisible, modifier = content, embedded = true)
                PlayerMode.About -> PlayerAbout(content)
            }
        }
    }
}

/** Content scrolling out of the mode area fades at its top and bottom edges instead of being cut. */
private fun Modifier.fadingEdges(): Modifier = this
    .graphicsLayer { compositingStrategy = CompositingStrategy.Offscreen }
    .drawWithContent {
        drawContent()
        val top = 20.dp.toPx()
        val bottom = 28.dp.toPx()
        drawRect(Brush.verticalGradient(0f to Color.Transparent, 1f to Color.Black, startY = 0f, endY = top), blendMode = BlendMode.DstIn, size = size.copy(height = top))
        drawRect(Brush.verticalGradient(0f to Color.Black, 1f to Color.Transparent, startY = size.height - bottom, endY = size.height), blendMode = BlendMode.DstIn, topLeft = Offset(0f, size.height - bottom), size = size.copy(height = bottom))
    }

/** The title row ([TitleRow]), or what to do when nothing is queued. */
@Composable
private fun PlayerTitle(mode: PlayerMode, onOpenAlbum: (String) -> Unit, onOpenArtist: (String) -> Unit, hero: HeroGeometry, onMore: () -> Unit, onAddTo: () -> Unit) {
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
            TitleRow(track, compact = mode != PlayerMode.Artwork, onOpenAlbum, onOpenArtist, hero, onMore, onAddTo)
        }
    }
}

/**
 * Notices: skipped-unavailable, resume offer, autoplay "why", sleep timer. Nothing at all when
 * there are none (the layout then gives it no gap).
 */
@Composable
private fun PlayerNoticeLines() {
    val client = LocalCoreClient.current
    val entry by client.nowPlaying.collectAsStateWithLifecycle()
    val playerNotice by client.playerNotice.collectAsStateWithLifecycle()
    val notice = playerNoticeText(playerNotice)
    val resume by client.resumeOffer.collectAsStateWithLifecycle()
    val sleep by client.sleepTimer.collectAsStateWithLifecycle()
    Column(Modifier.fillMaxWidth().padding(horizontal = PAGE_PADDING)) {
        notice?.let { NoticeLine(it, MaterialTheme.colorScheme.error) }
        resume?.let { offer ->
            Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
                Text(stringResource(R.string.player_resume_offer, offer.deviceName, offer.track.title), style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant, modifier = Modifier.weight(1f), maxLines = 1, overflow = TextOverflow.Ellipsis)
                TextButton(onClick = { client.dispatch(Command.ResumeHere) }, modifier = Modifier.testTag("player.resumeHere")) { Text(stringResource(R.string.player_resume_here)) }
                IconButton(onClick = { client.dispatch(Command.DismissResumeOffer) }) { Icon(Icons.Filled.Close, stringResource(R.string.action_dismiss)) }
            }
        }
        (entry?.item?.source as? QueueSource.Autoplay)?.let { NoticeLine(stringResource(R.string.player_autoplay_reason, it.data.reason), MaterialTheme.colorScheme.tertiary) }
        sleep?.let { t ->
            val label = t.endsAt?.let { stringResource(R.string.sleep_active, formatClock((it - System.currentTimeMillis()).toLong().coerceAtLeast(0))) } ?: stringResource(R.string.sleep_active_end_of_track)
            NoticeLine(label, MaterialTheme.colorScheme.onSurfaceVariant)
        }
    }
}

/** The wavy seek bar with elapsed / total. */
@Composable
private fun PlayerSeek(position: () -> Long) {
    val client = LocalCoreClient.current
    val entry by client.nowPlaying.collectAsStateWithLifecycle()
    val playing by client.isPlaying.collectAsStateWithLifecycle()
    val reducedMotion = LocalReducedMotion.current
    val track = entry?.track ?: return
    Column(Modifier.fillMaxWidth().padding(horizontal = PAGE_PADDING)) {
        WavySeekBar(position = position, durationMs = track.durationMs.toLong(), playing = playing && !reducedMotion, onSeek = { client.dispatch(Commands.seekTo(it)) })
    }
}

/**
 * Title, "artist • album" (each a link that collapses the player and opens the page), favourite,
 * add to playlist and More. In a non-artwork mode a thumbnail slot opens at the start of the row
 * (its width follows [HeroGeometry.modeFraction] in the layout phase); the one artwork lands in it.
 */
@Composable
private fun TitleRow(track: app.hocket.core.api.TrackSummary, compact: Boolean, onOpenAlbum: (String) -> Unit, onOpenArtist: (String) -> Unit, hero: HeroGeometry, onMore: () -> Unit, onAddTo: () -> Unit) {
    val client = LocalCoreClient.current
    // The bottom 12 dp is the room the artist links' trimmed touch targets reach into.
    Column(Modifier.fillMaxWidth().padding(top = 12.dp, bottom = 12.dp)) {
    Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
        // The thumbnail slot: laid out at its full size, reporting a width that opens with the mode.
        Box(
            Modifier
                .layout { measurable, _ ->
                    val side = HeroGeometry.SMALL_SIZE.roundToPx()
                    val gap = 14.dp.roundToPx()
                    val placeable = measurable.measure(Constraints.fixed(side, side))
                    layout(((side + gap) * hero.modeFraction()).roundToInt(), side) { placeable.place(0, 0) }
                }
                .onGloballyPositioned { hero.small = hero.measure(it) },
        )
        Column(Modifier.weight(1f)) {
            // The page's heading, and a polite live region: it changes once per track, so a track
            // change is announced once (never the position).
            val trackDesc = stringResource(R.string.player_track_a11y, track.title, track.artist ?: stringResource(R.string.unknown_artist))
            // Line boxes trimmed where title and artist meet, so the two read as one block.
            Text(track.title, style = MaterialTheme.typography.headlineSmallEmphasized.copy(lineHeightStyle = LineHeightStyle(LineHeightStyle.Alignment.Center, LineHeightStyle.Trim.LastLineBottom)), fontWeight = FontWeight.Bold, maxLines = if (compact) 1 else 2, overflow = TextOverflow.Ellipsis,
                modifier = Modifier.semantics { heading(); liveRegion = LiveRegionMode.Polite; contentDescription = trackDesc }.testTag("player.title"))
            val goArtist = stringResource(R.string.action_go_to_artist)
            val goAlbum = stringResource(R.string.action_go_to_album)
            // The links keep their 48 dp touch height, but the row tucks up under the title so the
            // text sits close to it (the target overlaps the title's line box and the space below).
            Row(
                Modifier.layout { measurable, constraints ->
                    val placeable = measurable.measure(constraints)
                    // Trim the target's slack above and below the text alike, so the block's
                    // height is the text's and it centres against the thumbnail and buttons.
                    val pull = 12.dp.roundToPx().coerceAtMost(placeable.height / 4)
                    layout(placeable.width, placeable.height - 2 * pull) { placeable.place(0, -pull) }
                },
                verticalAlignment = Alignment.CenterVertically,
            ) {
                val sub = MaterialTheme.typography.bodyLarge.copy(lineHeightStyle = LineHeightStyle(LineHeightStyle.Alignment.Center, LineHeightStyle.Trim.FirstLineTop))
                val subColor = MaterialTheme.colorScheme.onSurfaceVariant
                Text(track.artist ?: stringResource(R.string.unknown_artist), style = sub, color = subColor, maxLines = 1, overflow = TextOverflow.Ellipsis,
                    modifier = Modifier.weight(1f, fill = false).then(track.artistId?.let { id -> Modifier.textLink(goArtist) { onOpenArtist(id) } } ?: Modifier).testTag("player.artist"))
                // Beside the thumbnail too: both shrink to share the row, each ellipsised.
                track.album?.let { album ->
                    Text(stringResource(R.string.dot_separator), style = sub, color = subColor, modifier = Modifier.clearAndSetSemantics { })
                    Text(album, style = sub, color = subColor, maxLines = 1, overflow = TextOverflow.Ellipsis,
                        modifier = Modifier.weight(1f, fill = false).then(track.albumId?.let { id -> Modifier.textLink(goAlbum) { onOpenAlbum(id) } } ?: Modifier).testTag("player.album"))
                }
            }
        }
        IconButton(onClick = onAddTo, modifier = Modifier.testTag("player.addToPlaylist")) { Icon(Icons.AutoMirrored.Filled.PlaylistAdd, stringResource(R.string.player_add_to_playlist)) }
        IconButton(onClick = onMore, modifier = Modifier.testTag("player.more")) { Icon(Icons.Filled.MoreVert, stringResource(R.string.action_more)) }
    }
    }
}

/**
 * Lyrics / Queue / About as three pills (the selected one a filled tonal pill; tapping it again goes
 * back to the artwork). Where the three labels do not fit
 * (narrow screens, large fonts) only the selected pill keeps its label; the others show their icon
 * and say their name to a screen reader. The pills are tabs with a selected state.
 */
@Composable
private fun ModeBar(mode: PlayerMode, onMode: (PlayerMode) -> Unit, pageWidth: Dp) {
    val labels = listOf(PlayerMode.Lyrics to stringResource(R.string.player_mode_lyrics), PlayerMode.Queue to stringResource(R.string.player_mode_queue), PlayerMode.About to stringResource(R.string.player_mode_about))
    val measurer = rememberTextMeasurer()
    val style = MaterialTheme.typography.labelLarge
    val density = LocalDensity.current
    // Room for the pills: the page less its 12 dp sides.
    val room = pageWidth - 24.dp
    val needed = with(density) { labels.sumOf { measurer.measure(it.second, style).size.width }.toDp() } + (PILL_PADDING * 2 + PILL_ICON + PILL_GAP) * 3 + 8.dp
    val compact = needed > room
    Row(Modifier.fillMaxWidth().padding(horizontal = 12.dp), verticalAlignment = Alignment.CenterVertically) {
        Row(Modifier.weight(1f), horizontalArrangement = Arrangement.spacedBy(4.dp, Alignment.CenterHorizontally), verticalAlignment = Alignment.CenterVertically) {
            labels.forEach { (m, label) ->
                val icon = when (m) { PlayerMode.Lyrics -> Icons.Filled.Lyrics; PlayerMode.Queue -> Icons.AutoMirrored.Filled.QueueMusic; else -> Icons.Outlined.Info }
                ModePill(label, icon, selected = mode == m, showLabel = !compact || mode == m, tag = "player.mode." + m.name.lowercase(), onClick = { onMode(if (mode == m) PlayerMode.Artwork else m) })
            }
        }
    }
}

private val PILL_PADDING = 12.dp
private val PILL_ICON = 20.dp
private val PILL_GAP = 6.dp

/**
 * One mode pill: a 40 dp tall visual pill inside a 48 dp touch target, filled tonal when selected
 * (the colours crossfade), bouncing on press, its width springing as the label comes and goes.
 */
@Composable
private fun ModePill(label: String, icon: ImageVector, selected: Boolean, showLabel: Boolean, tag: String, onClick: () -> Unit) {
    val interaction = remember { MutableInteractionSource() }
    val scale by rememberPressScale(interaction)
    val reduced = LocalReducedMotion.current
    val bg by animateColorAsState(if (selected) MaterialTheme.colorScheme.secondaryContainer else Color.Transparent, label = "pillBg")
    val fg by animateColorAsState(if (selected) MaterialTheme.colorScheme.onSecondaryContainer else MaterialTheme.colorScheme.onSurface, label = "pillFg")
    Box(
        Modifier
            .heightIn(min = 48.dp)
            .selectable(selected = selected, interactionSource = interaction, indication = null, role = Role.Tab, onClick = onClick)
            .then(if (showLabel) Modifier else Modifier.semantics { contentDescription = label })
            .testTag(tag),
        contentAlignment = Alignment.Center,
    ) {
        Row(
            Modifier
                .graphicsLayer { scaleX = scale; scaleY = scale }
                .height(40.dp)
                .clip(CircleShape)
                .background(bg)
                .indication(interaction, ripple(color = fg))
                .then(if (reduced) Modifier else Modifier.animateContentSize())
                .padding(horizontal = PILL_PADDING),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Icon(icon, null, Modifier.size(PILL_ICON), tint = fg)
            if (showLabel) {
                Spacer(Modifier.width(PILL_GAP))
                Text(label, style = MaterialTheme.typography.labelLarge, color = fg, maxLines = 1)
            }
        }
    }
}

/**
 * The player's one artwork: laid out at the big slot's size and moved and scaled in a graphics
 * layer to wherever [HeroGeometry.playerArt] says (the big slot, the thumbnail slot, or between as
 * the mode changes), its corners going from the big radius to the thumbnail's. In artwork mode it
 * shrinks a little while paused (Navic), a horizontal swipe skips (springs back without
 * overshoot), and a long press toggles a whole-app dynamic colour preview from it; as a thumbnail,
 * a tap brings the artwork back. While the sheet's flying artwork is in the air ([hidden]) it is
 * not drawn.
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
    immersive: app.hocket.core.api.ArtworkLayout? = null,
) {
    val client = LocalCoreClient.current
    val scope = rememberCoroutineScope()
    val haptics = LocalHapticFeedback.current
    val density = LocalDensity.current
    val swipe = remember { Animatable(0f) }
    val threshold = with(density) { 96.dp.toPx() }
    val seed by artworkSeed(coverArt)
    val desc = stringResource(R.string.player_artwork_a11y, title ?: "")
    val previewDesc = stringResource(R.string.player_dynamic_preview)
    val nextLabel = stringResource(R.string.action_next)
    val previousLabel = stringResource(R.string.action_previous)
    val showLabel = stringResource(R.string.player_show_artwork)
    val togglePreview = { haptics.performHapticFeedback(HapticFeedbackType.LongPress); onPreviewToggle(seed ?: ArtworkColors.seedFor(coverArt)) }
    val currentThumbMode = androidx.compose.runtime.rememberUpdatedState(thumbMode)
    Box(
        Modifier
            // Laid out at its current size and place (so its bounds are what shows; it relays out
            // only while the mode or the paused size animates, never during a sheet drag).
            .layout { measurable, constraints ->
                val r = hero.playerArt(modeFraction(), scale())
                val side = r.width.roundToInt().coerceAtLeast(1)
                val placeable = measurable.measure(Constraints.fixed(side, side))
                layout(constraints.maxWidth, constraints.maxHeight) { placeable.place(r.left.roundToInt(), r.top.roundToInt()) }
            }
            .graphicsLayer {
                val f = modeFraction()
                if (hidden() || hero.bigSlot().width <= 0f) { alpha = 0f; return@graphicsLayer }
                translationX = swipe.value * (1f - f)
                shape = RoundedCornerShape(hero.cornerFor(f, size.width))
                clip = true
                shadowElevation = hero.elevationFor(f, this)
            }
            .pointerInput(thumbMode) {
                if (thumbMode) return@pointerInput
                var crossed = false
                detectHorizontalDragGestures(
                    onDragStart = { crossed = false },
                    onDragEnd = {
                        scope.launch {
                            val v = swipe.value
                            if (v < -threshold) client.dispatch(Command.Next)
                            if (v > threshold) client.dispatch(Command.Previous)
                            swipe.animateTo(0f, Motion.snap)
                        }
                    },
                    onDragCancel = { scope.launch { swipe.animateTo(0f, Motion.snap) } },
                ) { change, delta ->
                    change.consume()
                    val next = swipe.value + delta
                    val resisted = if (abs(next) > threshold) (if (next < 0) -1f else 1f) * (threshold + (abs(next) - threshold) * 0.3f) else next
                    val over = abs(resisted) > threshold
                    if (over != crossed) { crossed = over; haptics.performHapticFeedback(if (over) HapticFeedbackType.GestureThresholdActivate else HapticFeedbackType.SegmentTick) }
                    scope.launch { swipe.snapTo(resisted) }
                }
            }
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
        Artwork(coverArt, ArtworkSizes.FULL, null, Modifier.fillMaxSize().immersiveFade(immersive?.bottom, fraction = { 1f - modeFraction() }), RectangleShape)
        if (immersive != null) ImmersiveArtworkOverlay(immersive, fraction = { 1f - modeFraction() })
    }
}

@Composable
private fun NoticeLine(text: String, color: Color) {
    Text(text, style = MaterialTheme.typography.labelMedium, color = color, modifier = Modifier.fillMaxWidth().padding(bottom = 4.dp), maxLines = 1, overflow = TextOverflow.Ellipsis)
}

/** A text that navigates: labelled click, at least a 48 dp touch target, content vertically centred. */
private fun Modifier.textLink(label: String, onClick: () -> Unit): Modifier =
    this.clickable(onClickLabel = label, onClick = onClick).heightIn(min = 48.dp).wrapContentHeight(Alignment.CenterVertically)
