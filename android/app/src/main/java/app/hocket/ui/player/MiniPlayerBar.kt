package app.hocket.ui.player

import androidx.compose.foundation.background
import androidx.compose.ui.unit.Dp
import androidx.compose.material3.IconButtonDefaults
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.foundation.basicMarquee
import androidx.compose.foundation.clickable
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.LoadingIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.setValue
import androidx.compose.ui.graphics.Color
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.material3.ColorScheme
import androidx.compose.ui.draw.drawBehind
import androidx.compose.ui.graphics.drawscope.scale
import androidx.compose.foundation.border
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.drawWithContent
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.hapticfeedback.HapticFeedbackType
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.onGloballyPositioned
import androidx.compose.ui.layout.onSizeChanged
import androidx.compose.ui.draw.clipToBounds
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.LocalHapticFeedback
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.CustomAccessibilityAction
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.customActions
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.onClick
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import app.hocket.R
import app.hocket.core.ArtworkSizes
import app.hocket.core.api.Command
import app.hocket.ui.LocalCoreClient
import app.hocket.ui.a11y.LocalReducedMotion
import app.hocket.ui.components.Artwork
import app.hocket.ui.icons.HocketIcons
import app.hocket.ui.theme.Motion

/**
 * The collapsed player, a floating card (Navic's detached style): square-ish rounded artwork, title
 * and artist (scrolling once when too long), play/pause (a loading indicator while buffering) and
 * next, and a thin progress line along the card's bottom edge. The card itself (margins, corners,
 * shadow) is the sheet's clip, so it grows into the full player. Tap or drag up to expand; swipe the
 * content left/right to skip: a carousel in miniature ([SkipCarousel]) where the artwork and text
 * follow the finger and the neighbour's thumbnail, title and artist slide in beside them; past half
 * way (or flung) it settles on the neighbour and skips, otherwise it springs back without overshoot.
 *
 * The progress line is drawn in the draw phase from [position], so ticks never recompose the bar.
 * The thumbnail reports its bounds to [hero] (the artwork that flies into the full player starts
 * there) and is hidden while that artwork is flying ([thumbHidden]).
 *
 * Accessibility: one item "Now playing: <title> by <artist>" (plus a notice when there is one) whose
 * click expands the player and whose actions replace the skip swipe (next / previous track); the
 * play/pause and next buttons stay separate targets. The item is a polite live region: its text
 * changes only when the track (or the notice) does, so a track change is announced once and
 * position never is (the progress line is not exposed; the full player's seek bar carries it).
 */
@Composable
internal fun MiniPlayerBar(
    onExpand: () -> Unit,
    position: () -> Long,
    hero: HeroGeometry,
    thumbHidden: () -> Boolean,
    liveNeighbours: Neighbours = Neighbours.None,
    start: Dp = HeroGeometry.MINI_MARGIN,
    end: Dp = HeroGeometry.MINI_MARGIN,
) {
    val client = LocalCoreClient.current
    val entry by client.nowPlaying.collectAsStateWithLifecycle()
    val playing by client.isPlaying.collectAsStateWithLifecycle()
    val buffering by client.buffering.collectAsStateWithLifecycle()
    val playerNotice by client.playerNotice.collectAsStateWithLifecycle()
    val notice = playerNoticeText(playerNotice)
    val track = entry?.track
    val scope = rememberCoroutineScope()
    val haptics = LocalHapticFeedback.current
    val density = LocalDensity.current
    val carousel = remember { SkipCarousel() }
    val neighbours = carousel.shown(liveNeighbours)
    val threshold = with(density) { 72.dp.toPx() }
    val title = track?.title ?: ""
    val artist = track?.artist ?: stringResource(R.string.unknown_artist)
    val nowPlayingDesc = stringResource(R.string.player_mini_a11y, title, artist)
    val sub = notice ?: artist
    val desc = if (sub != artist) "$nowPlayingDesc. $sub" else nowPlayingDesc
    val expandLabel = stringResource(R.string.player_expand)
    val nextLabel = stringResource(R.string.action_next)
    val previousLabel = stringResource(R.string.action_previous)
    val reducedMotion = LocalReducedMotion.current
    val durationMs = track?.durationMs?.toFloat()?.takeIf { it > 0f } ?: 1f
    // Waiting for the room's position (ConnectSync): the line shows, dimmed.
    val syncing = rememberConnectSyncing()
    // The card in the artwork's colours (a tinted container that stands out from the page),
    // crossfading as the track changes.
    val colours = miniPlayerColours(track?.coverArt)
    val line = colours.accent.let { if (syncing) it.copy(alpha = CONNECT_SYNC_ALPHA) else it }
    val lineTrack = colours.accent.copy(alpha = 0.28f)
    // The item's description already carries both lines; the scrolling texts themselves stay out of
    // the tree (a marquee is laid out wider than its box on purpose).
    val marquee = if (reducedMotion) Modifier else Modifier.clearAndSetSemantics { }.basicMarquee(iterations = 1, initialDelayMillis = 3_000, velocity = 30.dp)
    // The card's edges ([start], [end]: the sheet's card insets, centred or at the end edge).
    Box(Modifier.fillMaxSize()) {
        Box(
            Modifier
                .fillMaxSize()
                .padding(start = start, end = end)
                .background(colours.container)
                // The skip swipe: the artwork and the text slide, the neighbour's sliding in.
                .then(with(carousel) { Modifier.gesture(client, scope, haptics, enabled = true, reduced = reducedMotion, rubberThreshold = threshold) })
                // Taps outside the info (the thumbnail, the card's edges) expand too; pointer only,
                // the info item below carries the accessible click.
                .pointerInput(onExpand) { detectTapGestures(onTap = { onExpand() }) }
                .testTag("miniPlayer"),
        ) {
            MaterialTheme(colorScheme = MaterialTheme.colorScheme.copy(onSurface = colours.content, onSurfaceVariant = colours.contentVariant)) {
            androidx.compose.runtime.CompositionLocalProvider(androidx.compose.material3.LocalContentColor provides colours.content) {
            Row(Modifier.fillMaxSize().padding(end = 10.dp), verticalAlignment = Alignment.CenterVertically) {
                // The strip: the current track's artwork and text, its neighbours a strip's width
                // either side (off view, clipped, until a swipe brings one in).
                Box(
                    Modifier
                        .weight(1f)
                        .fillMaxHeight()
                        .clipToBounds()
                        .onSizeChanged { carousel.span = it.width.toFloat().coerceAtLeast(1f) },
                ) {
                    Row(
                        Modifier
                            .fillMaxSize()
                            .graphicsLayer { translationX = carousel.offset.value }
                            .padding(start = MINI_START_PADDING),
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        Artwork(
                            track?.coverArt, ArtworkSizes.LIST, null,
                            Modifier
                                .size(HeroGeometry.THUMB_SIZE)
                                .onGloballyPositioned { hero.thumb = hero.measure(it) }
                                // The ring hides with the thumbnail while the artwork flies.
                                .graphicsLayer { alpha = if (thumbHidden()) 0f else 1f }
                                .border(1.dp, colours.content.copy(alpha = 0.14f), RoundedCornerShape(HeroGeometry.THUMB_CORNER)),
                            RoundedCornerShape(HeroGeometry.THUMB_CORNER),
                        )
                        Spacer(Modifier.width(12.dp))
                        Column(
                            Modifier
                                .weight(1f)
                                .fillMaxHeight()
                                .clickable(onClickLabel = expandLabel, onClick = onExpand)
                                // Centred above the progress bar.
                                .padding(bottom = MINI_PROGRESS_ROOM)
                                // Semantics modifiers inside a clearAndSetSemantics on the same node are dropped: tag first.
                                .testTag("miniPlayer.info")
                                .clearAndSetSemantics {
                                    contentDescription = desc
                                    liveRegion = LiveRegionMode.Polite
                                    customActions = listOf(
                                        CustomAccessibilityAction(nextLabel) { client.dispatch(Command.Next); true },
                                        CustomAccessibilityAction(previousLabel) { client.dispatch(Command.Previous); true },
                                    )
                                    onClick(expandLabel) { onExpand(); true }
                                },
                            verticalArrangement = Arrangement.Center,
                        ) {
                            Text(title, style = MaterialTheme.typography.bodyLarge, fontWeight = FontWeight.Medium, maxLines = 1, overflow = if (reducedMotion) TextOverflow.Ellipsis else TextOverflow.Clip, modifier = marquee)
                            Text(sub, style = MaterialTheme.typography.bodySmall, color = if (notice != null) MaterialTheme.colorScheme.error else MaterialTheme.colorScheme.onSurfaceVariant, maxLines = 1, overflow = if (reducedMotion) TextOverflow.Ellipsis else TextOverflow.Clip, modifier = marquee)
                        }
                    }
                    if (!reducedMotion) {
                        neighbours.previous?.track?.let { MiniNeighbour(it, carousel, side = -1f, tag = "miniPlayer.previous") }
                        neighbours.next?.track?.let { MiniNeighbour(it, carousel, side = 1f, tag = "miniPlayer.next") }
                    }
                    // Under the title, still while the track's artwork and text slide (it belongs
                    // to the player, not to the track in the strip).
                    MiniProgress(
                        position, durationMs, line, lineTrack,
                        Modifier
                            .align(Alignment.BottomStart)
                            .padding(start = MINI_START_PADDING + HeroGeometry.THUMB_SIZE + 12.dp, end = 12.dp, bottom = MINI_PROGRESS_ROOM - 2.dp)
                            .fillMaxWidth(),
                    )
                }
                val playLabel = stringResource(if (playing) R.string.action_pause else R.string.action_play)
                val bufferingLabel = stringResource(R.string.player_buffering)
                // Play/pause in its own filled circle, in the artwork's accent.
                androidx.compose.material3.FilledIconButton(
                    onClick = { haptics.performHapticFeedback(if (playing) HapticFeedbackType.ToggleOff else HapticFeedbackType.ToggleOn); client.dispatch(Command.TogglePlay) },
                    shape = androidx.compose.foundation.shape.CircleShape,
                    colors = IconButtonDefaults.filledIconButtonColors(containerColor = colours.button, contentColor = colours.onButton),
                    modifier = Modifier.size(44.dp).testTag("miniPlayer.playPause").semantics { contentDescription = playLabel; if (buffering) stateDescription = bufferingLabel },
                ) {
                    if (buffering) LoadingIndicator(Modifier.size(28.dp), color = androidx.compose.material3.LocalContentColor.current)
                    else Icon(if (playing) HocketIcons.Filled.Pause else HocketIcons.Filled.PlayArrow, null)
                }
                IconButton(onClick = { client.dispatch(Command.Next) }) { Icon(HocketIcons.Filled.SkipNext, stringResource(R.string.action_next)) }
            }
            }
            }
        }
    }
}

/**
 * A neighbour in the mini player's strip, a strip's width to the [side] (-1 before, 1 after):
 * its thumbnail, title and artist, decorative (the current item carries the semantics).
 */
@Composable
private fun MiniNeighbour(track: app.hocket.core.api.TrackSummary, carousel: SkipCarousel, side: Float, tag: String) {
    Row(
        Modifier
            .fillMaxSize()
            .graphicsLayer { translationX = carousel.offset.value + side * carousel.span }
            .padding(start = MINI_START_PADDING)
            .testTag(tag)
            .clearAndSetSemantics { },
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Artwork(track.coverArt, ArtworkSizes.LIST, null, Modifier.size(HeroGeometry.THUMB_SIZE), RoundedCornerShape(HeroGeometry.THUMB_CORNER))
        Spacer(Modifier.width(12.dp))
        Column(Modifier.weight(1f).padding(bottom = MINI_PROGRESS_ROOM), verticalArrangement = Arrangement.Center) {
            Text(track.title, style = MaterialTheme.typography.bodyLarge, fontWeight = FontWeight.Medium, maxLines = 1, overflow = TextOverflow.Ellipsis)
            Text(track.artist ?: stringResource(R.string.unknown_artist), style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant, maxLines = 1, overflow = TextOverflow.Ellipsis)
        }
    }
}


/** The mini player's content inset from the card's leading edge (before the thumbnail). */
internal val MINI_START_PADDING = 10.dp

/** Room under the title and artist for the progress bar (the text centres above it). */
internal val MINI_PROGRESS_ROOM = 10.dp

/**
 * The mini player's progress: a rounded bar in the accent up to the position, a gap, then the
 * rest of the track fainter with a stop dot at its end (Material 3 Expressive's linear indicator).
 * Drawn from [position] in the draw phase, so ticks never recompose the bar.
 */
@Composable
private fun MiniProgress(position: () -> Long, durationMs: Float, active: Color, track: Color, modifier: Modifier) {
    Spacer(
        modifier.height(4.dp).drawBehind {
            // Fills from the leading edge: mirrored right to left.
            if (layoutDirection == androidx.compose.ui.unit.LayoutDirection.Rtl) { scale(-1f, 1f) { drawProgress(position, durationMs, active, track) } }
            else drawProgress(position, durationMs, active, track)
        },
    )
}

private fun androidx.compose.ui.graphics.drawscope.DrawScope.drawProgress(position: () -> Long, durationMs: Float, active: Color, track: Color) {
            val h = size.height
            val r = androidx.compose.ui.geometry.CornerRadius(h / 2f)
            val gap = 4.dp.toPx()
            val f = (position() / durationMs).coerceIn(0f, 1f)
            val x = size.width * f
            if (x > 0f) drawRoundRect(active, size = Size(maxOf(x, h), h), cornerRadius = r)
            val start = (if (x > 0f) maxOf(x, h) + gap else 0f)
            if (size.width - start > h) drawRoundRect(track, topLeft = Offset(start, 0f), size = Size(size.width - start, h), cornerRadius = r)
            drawCircle(active, radius = h * 0.4f, center = Offset(size.width - h / 2f, h / 2f))
}

/** The mini player card's colours, from its artwork. */
internal class MiniPlayerColours(val container: Color, val content: Color, val contentVariant: Color, val accent: Color, val button: Color, val onButton: Color)

/**
 * The colour scheme of what is playing: the scheme of [coverArt]'s seed colour, or null with no
 * artwork. While a new cover's colours are worked out the last ones hold (rather than dipping
 * through the app's own scheme and back). The mini player and the navigation bar both wear it.
 */
@Composable
internal fun nowPlayingScheme(coverArt: String?): ColorScheme? {
    val dark = app.hocket.ui.theme.LocalDarkTheme.current
    val worked = artworkSeed(coverArt).value
    var last by remember { androidx.compose.runtime.mutableStateOf<Color?>(null) }
    // Nothing playing forgets them, so the next song never starts in a long-gone cover's colours.
    androidx.compose.runtime.SideEffect { if (coverArt == null) last = null else if (worked != null) last = worked }
    val seed = worked ?: last.takeIf { coverArt != null }
    return remember(seed, dark) { seed?.let { app.hocket.ui.theme.ArtworkColors.scheme(it, dark) } }
}

/** The mini player's container in [scheme]: its primary container, a touch towards the surface. */
internal fun miniContainer(scheme: ColorScheme, dark: Boolean): Color =
    androidx.compose.ui.graphics.lerp(scheme.surfaceContainerHighest, scheme.primaryContainer, if (dark) 0.85f else 0.8f)

/**
 * The mini player's colours for [coverArt] ([nowPlayingScheme], the app's own without artwork): the
 * primary container behind, its text, the artist and progress in the accent, and the play/pause
 * circle in the accent. Crossfades ([Motion.COLOUR_MS]) when the track changes.
 */
@Composable
internal fun miniPlayerColours(coverArt: String?): MiniPlayerColours {
    val dark = app.hocket.ui.theme.LocalDarkTheme.current
    val scheme = nowPlayingScheme(coverArt) ?: MaterialTheme.colorScheme
    val spec = androidx.compose.animation.core.tween<Color>(Motion.COLOUR_MS)
    @Composable fun a(c: Color) = androidx.compose.animation.animateColorAsState(c, spec, label = "miniColour").value
    return MiniPlayerColours(
        container = a(miniContainer(scheme, dark)),
        content = a(scheme.onPrimaryContainer),
        contentVariant = a(if (dark) scheme.primary.copy(alpha = 0.9f) else scheme.onPrimaryContainer.copy(alpha = 0.75f)),
        accent = a(scheme.primary),
        button = a(scheme.primary),
        onButton = a(scheme.onPrimary),
    )
}
