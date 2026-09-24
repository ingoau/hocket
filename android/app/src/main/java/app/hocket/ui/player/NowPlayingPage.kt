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
import app.hocket.ui.components.ActionSheet
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

/**
 * The full player (the owner's mockup, made Material 3 Expressive). Top to bottom:
 *
 * - "Playing from" and the queue's source (album, playlist, search...), with the collapse chevron;
 * - the mode area: the big artwork in [PlayerMode.Artwork], or Lyrics / Queue / About in its place;
 * - notices (resume offer, a problem, autoplay's reason, remote playback, the sleep timer);
 * - the title row: a small thumbnail slot (non-artwork modes), title, "artist • album" links, and
 *   favourite, add to playlist and the More sheet;
 * - the wavy seek bar with elapsed / total, the transport, and the mode pills flanked by the sleep
 *   timer and Connect.
 *
 * There is ONE artwork ([PlayerArtwork]), drawn over the page and moved in a graphics layer between
 * the big slot and the thumbnail slot as [modeFraction] goes 0 (artwork) to 1 (another mode); both
 * slots are measured into [hero], which the sheet's flying artwork also lands on.
 *
 * At least a screen tall: the area takes what the header and the controls leave (never less than
 * [MIN_AREA]); on short screens or at large font sizes the whole page scrolls. [position] is read
 * in the draw phase (the seek bar) and once a second (its labels), never here.
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
) {
    val client = LocalCoreClient.current
    val entry by client.nowPlaying.collectAsStateWithLifecycle()
    val track = entry?.track
    val density = LocalDensity.current
    var handoff by remember { mutableStateOf(false) }
    var sleepSheet by remember { mutableStateOf(false) }
    var more by remember { mutableStateOf(false) }
    var addTo by remember { mutableStateOf(false) }
    hero.artInset = with(density) { PAGE_PADDING.toPx() }
    hero.artMax = with(density) { MAX_ART.toPx() }
    hero.artPad = with(density) { 8.dp.toPx() }
    BoxWithConstraints(Modifier.fillMaxSize()) {
        val viewport = maxHeight
        val pageWidth = maxWidth
        Box(Modifier.fillMaxSize().verticalScroll(rememberScrollState()).testTag("player.page")) {
            PlayerLayout(
                minHeight = viewport,
                header = { PlayerHeader(onCollapse) },
                area = { ModeArea(mode, hero, lyricsVisible) },
                controls = {
                    PlayerControls(
                        mode = mode,
                        onMode = onMode,
                        onOpenAlbum = onOpenAlbum,
                        onOpenArtist = onOpenArtist,
                        position = position,
                        hero = hero,
                        pageWidth = pageWidth,
                        onMore = { more = true },
                        onAddTo = { addTo = true },
                        onSleep = { sleepSheet = true },
                        onConnect = { handoff = true },
                    )
                },
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
        )
    }
    if (handoff) HandoffSheet(onDismiss = { handoff = false })
    if (sleepSheet) SleepTimerSheet(onDismiss = { sleepSheet = false })
    if (more && track != null) ActionSheet(Commands.tracks(listOf(track.id)), track.title, track.artist, onDismiss = { more = false },
        onGoToAlbum = track.albumId?.let { id -> { onOpenAlbum(id) } }, onGoToArtist = track.artistId?.let { id -> { onOpenArtist(id) } })
    if (addTo && track != null) PlaylistPicker(Commands.tracks(listOf(track.id)), onDismiss = { addTo = false })
}

/** The page's side padding, the artwork's largest size, and the least room the mode area gets. */
private val PAGE_PADDING = 24.dp
private val MAX_ART = 520.dp
private val MIN_AREA = 220.dp

/**
 * Header, mode area and controls stacked: the header and the controls take what they need, the
 * area what is left of [minHeight] (at least [MIN_AREA]). Laid out inside the page's vertical
 * scroll, so it can be taller than the screen.
 */
@Composable
private fun PlayerLayout(minHeight: Dp, header: @Composable () -> Unit, area: @Composable () -> Unit, controls: @Composable () -> Unit) {
    Layout(contents = listOf(header, area, controls)) { (h, a, c), constraints ->
        val loose = Constraints(maxWidth = constraints.maxWidth)
        val hp = h.map { it.measure(loose) }
        val cp = c.map { it.measure(loose) }
        val hh = hp.maxOfOrNull { it.height } ?: 0
        val ch = cp.maxOfOrNull { it.height } ?: 0
        val areaH = maxOf(MIN_AREA.roundToPx(), minHeight.roundToPx() - hh - ch)
        val ap = a.map { it.measure(Constraints.fixed(constraints.maxWidth, areaH)) }
        layout(constraints.maxWidth, hh + areaH + ch) {
            hp.forEach { it.place(0, 0) }
            ap.forEach { it.place(0, hh) }
            cp.forEach { it.place(0, hh + areaH) }
        }
    }
}

/** "Playing from" and the queue's source, and the collapse chevron. */
@Composable
private fun PlayerHeader(onCollapse: () -> Unit) {
    val client = LocalCoreClient.current
    val queue by client.queue.collectAsStateWithLifecycle()
    val entry by client.nowPlaying.collectAsStateWithLifecycle()
    val source = when {
        entry?.item?.source is QueueSource.Autoplay -> stringResource(R.string.player_playing_from_autoplay)
        else -> queue.contextLabel?.takeIf { it.isNotBlank() }
    } ?: stringResource(R.string.player_sheet_title)
    val sourceDesc = stringResource(R.string.player_playing_from_a11y, source)
    Row(Modifier.fillMaxWidth().statusBarsPadding().padding(start = PAGE_PADDING, end = 12.dp, top = 12.dp, bottom = 4.dp), verticalAlignment = Alignment.CenterVertically) {
        Column(Modifier.weight(1f).semantics(mergeDescendants = true) { contentDescription = sourceDesc }.testTag("player.source")) {
            Text(stringResource(R.string.player_playing_from), style = MaterialTheme.typography.labelLarge, color = MaterialTheme.colorScheme.onSurfaceVariant, maxLines = 1)
            Text(source, style = MaterialTheme.typography.titleLargeEmphasized, fontWeight = FontWeight.Bold, maxLines = 1, overflow = TextOverflow.Ellipsis)
        }
        IconButton(onClick = onCollapse, modifier = Modifier.testTag("player.collapse")) {
            Icon(Icons.Filled.KeyboardArrowDown, stringResource(R.string.player_collapse), Modifier.size(28.dp))
        }
    }
}

/**
 * The mode area: empty in artwork mode (the one artwork is drawn over it), else Lyrics, Queue or
 * About, crossfading with a slight grow as the artwork shrinks away. Its bounds go to [hero] (the
 * big artwork is centred in them). Lists fade out at the area's top and bottom edges.
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
            val content = Modifier.fillMaxSize().fadingEdges()
            when (m) {
                PlayerMode.Artwork -> Spacer(Modifier.fillMaxSize())
                PlayerMode.Queue -> QueueList(content)
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

@Composable
private fun PlayerControls(
    mode: PlayerMode,
    onMode: (PlayerMode) -> Unit,
    onOpenAlbum: (String) -> Unit,
    onOpenArtist: (String) -> Unit,
    position: () -> Long,
    hero: HeroGeometry,
    pageWidth: Dp,
    onMore: () -> Unit,
    onAddTo: () -> Unit,
    onSleep: () -> Unit,
    onConnect: () -> Unit,
) {
    val client = LocalCoreClient.current
    val entry by client.nowPlaying.collectAsStateWithLifecycle()
    val playing by client.isPlaying.collectAsStateWithLifecycle()
    val transport by client.transport.collectAsStateWithLifecycle()
    val playerNotice by client.playerNotice.collectAsStateWithLifecycle()
    val notice = playerNoticeText(playerNotice)
    val resume by client.resumeOffer.collectAsStateWithLifecycle()
    val sleep by client.sleepTimer.collectAsStateWithLifecycle()
    val devices by client.devices.collectAsStateWithLifecycle()
    val owns by client.ownsTransport.collectAsStateWithLifecycle()
    val reducedMotion = LocalReducedMotion.current
    val track = entry?.track
    Column(Modifier.fillMaxWidth().navigationBarsPadding().padding(bottom = 8.dp)) {
        Column(Modifier.fillMaxWidth().padding(horizontal = PAGE_PADDING)) {
            // Notices: skipped-unavailable, resume offer, autoplay "why", remote playback, sleep timer.
            notice?.let { NoticeLine(it, MaterialTheme.colorScheme.error) }
            resume?.let { offer ->
                Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
                    Text(stringResource(R.string.player_resume_offer, offer.deviceName, offer.track.title), style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant, modifier = Modifier.weight(1f), maxLines = 1, overflow = TextOverflow.Ellipsis)
                    TextButton(onClick = { client.dispatch(Command.ResumeHere) }, modifier = Modifier.testTag("player.resumeHere")) { Text(stringResource(R.string.player_resume_here)) }
                    IconButton(onClick = { client.dispatch(Command.DismissResumeOffer) }) { Icon(Icons.Filled.Close, stringResource(R.string.action_dismiss)) }
                }
            }
            (entry?.item?.source as? QueueSource.Autoplay)?.let { NoticeLine(stringResource(R.string.player_autoplay_reason, it.data.reason), MaterialTheme.colorScheme.tertiary) }
            if (!owns) devices.firstOrNull { it.playing }?.let { NoticeLine(stringResource(R.string.player_playing_on, it.name), MaterialTheme.colorScheme.primary) }
            sleep?.let { t ->
                val label = t.endsAt?.let { stringResource(R.string.sleep_active, formatClock((it - System.currentTimeMillis()).toLong().coerceAtLeast(0))) } ?: stringResource(R.string.sleep_active_end_of_track)
                NoticeLine(label, MaterialTheme.colorScheme.onSurfaceVariant)
            }
            if (track == null) {
                Column(Modifier.fillMaxWidth().padding(vertical = 16.dp), horizontalAlignment = Alignment.CenterHorizontally) {
                    Text(stringResource(R.string.empty_queue_title), style = MaterialTheme.typography.headlineSmall)
                    Text(stringResource(R.string.empty_queue_body), style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant, textAlign = TextAlign.Center)
                }
            } else {
                TitleRow(track, compact = mode != PlayerMode.Artwork, onOpenAlbum, onOpenArtist, hero, onMore, onAddTo)
                Spacer(Modifier.height(4.dp))
                WavySeekBar(position = position, durationMs = track.durationMs.toLong(), playing = playing && !reducedMotion, onSeek = { client.dispatch(Commands.seekTo(it)) })
                if (transport.buffering) Text(stringResource(R.string.player_buffering), style = MaterialTheme.typography.labelSmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
            }
            Spacer(Modifier.height(8.dp))
            // Narrow screens (display size "largest" leaves ~320 dp): a smaller transport.
            TransportRow(
                playing = playing,
                onPrevious = { client.dispatch(Command.Previous) },
                onToggle = { client.dispatch(Command.TogglePlay) },
                onNext = { client.dispatch(Command.Next) },
                height = if (pageWidth < 360.dp) 68.dp else 80.dp,
            )
            Spacer(Modifier.height(12.dp))
        }
        ModeBar(mode, onMode, sleepActive = sleep != null, onSleep = onSleep, remote = !owns, onConnect = onConnect, pageWidth = pageWidth)
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
    Row(Modifier.fillMaxWidth().padding(top = 12.dp), verticalAlignment = Alignment.CenterVertically) {
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
            Text(track.title, style = MaterialTheme.typography.headlineSmallEmphasized, fontWeight = FontWeight.Bold, maxLines = if (compact) 1 else 2, overflow = TextOverflow.Ellipsis,
                modifier = Modifier.semantics { heading(); liveRegion = LiveRegionMode.Polite; contentDescription = trackDesc }.testTag("player.title"))
            val goArtist = stringResource(R.string.action_go_to_artist)
            val goAlbum = stringResource(R.string.action_go_to_album)
            Row(verticalAlignment = Alignment.CenterVertically) {
                val sub = MaterialTheme.typography.bodyLarge
                val subColor = MaterialTheme.colorScheme.onSurfaceVariant
                Text(track.artist ?: stringResource(R.string.unknown_artist), style = sub, color = subColor, maxLines = 1, overflow = TextOverflow.Ellipsis,
                    modifier = Modifier.weight(1f, fill = false).then(track.artistId?.let { id -> Modifier.textLink(goArtist) { onOpenArtist(id) } } ?: Modifier).testTag("player.artist"))
                // Beside the thumbnail there is room for the artist only (the album is in About).
                track.album?.takeIf { !compact }?.let { album ->
                    Text(stringResource(R.string.dot_separator), style = sub, color = subColor, modifier = Modifier.clearAndSetSemantics { })
                    Text(album, style = sub, color = subColor, maxLines = 1, overflow = TextOverflow.Ellipsis,
                        modifier = Modifier.weight(1f, fill = false).then(track.albumId?.let { id -> Modifier.textLink(goAlbum) { onOpenAlbum(id) } } ?: Modifier).testTag("player.album"))
                }
            }
        }
        IconToggleButton(checked = track.loved, onCheckedChange = { client.dispatch(Commands.loveTrack(track.id, it)) }, modifier = Modifier.testTag("player.love")) {
            // A springy pop when the star fills (not when a loved track first shows).
            val scale = remember { Animatable(1f) }
            val was = remember { mutableStateOf(track.loved) }
            androidx.compose.runtime.LaunchedEffect(track.loved) {
                if (track.loved && !was.value) { scale.snapTo(0.6f); scale.animateTo(1f, Motion.pressRelease) }
                was.value = track.loved
            }
            Icon(
                if (track.loved) Icons.Filled.Star else Icons.Filled.StarBorder,
                stringResource(if (track.loved) R.string.player_loved else R.string.player_not_loved),
                tint = if (track.loved) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.onSurface,
                modifier = Modifier.graphicsLayer { scaleX = scale.value; scaleY = scale.value },
            )
        }
        IconButton(onClick = onAddTo, modifier = Modifier.testTag("player.addToPlaylist")) { Icon(Icons.AutoMirrored.Filled.PlaylistAdd, stringResource(R.string.player_add_to_playlist)) }
        IconButton(onClick = onMore, modifier = Modifier.testTag("player.more")) { Icon(Icons.Filled.MoreVert, stringResource(R.string.action_more)) }
    }
}

/**
 * Lyrics / Queue / About as three pills (the selected one a filled tonal pill; tapping it again goes
 * back to the artwork), flanked by the sleep timer and Connect. Where the three labels do not fit
 * (narrow screens, large fonts) only the selected pill keeps its label; the others show their icon
 * and say their name to a screen reader. The pills are tabs with a selected state.
 */
@Composable
private fun ModeBar(mode: PlayerMode, onMode: (PlayerMode) -> Unit, sleepActive: Boolean, onSleep: () -> Unit, remote: Boolean, onConnect: () -> Unit, pageWidth: Dp) {
    val on = MaterialTheme.colorScheme.primary
    val off = MaterialTheme.colorScheme.onSurfaceVariant
    val labels = listOf(PlayerMode.Lyrics to stringResource(R.string.player_mode_lyrics), PlayerMode.Queue to stringResource(R.string.player_mode_queue), PlayerMode.About to stringResource(R.string.player_mode_about))
    val measurer = rememberTextMeasurer()
    val style = MaterialTheme.typography.labelLarge
    val density = LocalDensity.current
    // Room for the pills: the page less its 12 dp sides and the two 48 dp buttons.
    val room = pageWidth - 24.dp - 96.dp
    val needed = with(density) { labels.sumOf { measurer.measure(it.second, style).size.width }.toDp() } + (PILL_PADDING * 2 + PILL_ICON + PILL_GAP) * 3 + 8.dp
    val compact = needed > room
    Row(Modifier.fillMaxWidth().padding(horizontal = 12.dp), verticalAlignment = Alignment.CenterVertically) {
        IconToggleButton(checked = sleepActive, onCheckedChange = { onSleep() }, modifier = Modifier.testTag("player.sleep")) {
            Icon(Icons.Filled.Bedtime, stringResource(R.string.player_sleep_timer), tint = if (sleepActive) on else off)
        }
        Row(Modifier.weight(1f), horizontalArrangement = Arrangement.spacedBy(4.dp, Alignment.CenterHorizontally), verticalAlignment = Alignment.CenterVertically) {
            labels.forEach { (m, label) ->
                val icon = when (m) { PlayerMode.Lyrics -> Icons.Filled.Lyrics; PlayerMode.Queue -> Icons.AutoMirrored.Filled.QueueMusic; else -> Icons.Outlined.Info }
                ModePill(label, icon, selected = mode == m, showLabel = !compact || mode == m, tag = "player.mode." + m.name.lowercase(), onClick = { onMode(if (mode == m) PlayerMode.Artwork else m) })
            }
        }
        IconToggleButton(checked = remote, onCheckedChange = { onConnect() }, modifier = Modifier.testTag("player.connect")) {
            Icon(Icons.Filled.Cast, stringResource(R.string.player_connect), tint = if (remote) on else off)
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
            .layout { measurable, _ ->
                val side = hero.bigSlot().width.roundToInt().coerceAtLeast(1)
                val placeable = measurable.measure(Constraints.fixed(side, side))
                layout(side, side) { placeable.place(0, 0) }
            }
            .graphicsLayer {
                val f = modeFraction()
                val r = hero.playerArt(f, scale())
                val base = hero.bigSlot().width
                if (hidden() || r.width <= 0f || base <= 0f) { alpha = 0f; return@graphicsLayer }
                val k = r.width / base
                transformOrigin = TransformOrigin(0f, 0f)
                translationX = r.left + swipe.value * (1f - f)
                translationY = r.top
                scaleX = k
                scaleY = k
                shape = RoundedCornerShape(hero.cornerFor(f, r.width) / k.coerceAtLeast(0.01f))
                clip = true
                shadowElevation = lerpF(HeroGeometry.HERO_ELEVATION.toPx(), 2.dp.toPx(), f)
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
        Artwork(coverArt, ArtworkSizes.FULL, null, Modifier.fillMaxSize(), RectangleShape)
    }
}

@Composable
private fun NoticeLine(text: String, color: Color) {
    Text(text, style = MaterialTheme.typography.labelMedium, color = color, modifier = Modifier.fillMaxWidth().padding(bottom = 4.dp), maxLines = 1, overflow = TextOverflow.Ellipsis)
}

/** A text that navigates: labelled click, at least a 48 dp touch target, content vertically centred. */
private fun Modifier.textLink(label: String, onClick: () -> Unit): Modifier =
    this.clickable(onClickLabel = label, onClick = onClick).heightIn(min = 48.dp).wrapContentHeight(Alignment.CenterVertically)
