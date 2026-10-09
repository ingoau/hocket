package app.hocket.ui.player

import androidx.compose.animation.core.Animatable
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.widthIn
import androidx.compose.material3.IconButtonDefaults
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.foundation.basicMarquee
import androidx.compose.foundation.clickable
import androidx.compose.foundation.gestures.detectHorizontalDragGestures
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
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Pause
import androidx.compose.material.icons.filled.PlayArrow
import androidx.compose.material.icons.filled.SkipNext
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.LoadingIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
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
import app.hocket.ui.theme.Motion
import kotlinx.coroutines.launch
import kotlin.math.abs

/**
 * The collapsed player, a floating card (Navic's detached style): square-ish rounded artwork, title
 * and artist (scrolling once when too long), play/pause (a loading indicator while buffering) and
 * next, and a thin progress line along the card's bottom edge. The card itself (margins, corners,
 * shadow) is the sheet's clip, so it grows into the full player. Tap or drag up to expand; swipe the content left/right to skip: the
 * artwork and text move with the finger, a haptic marks the threshold, and it settles back without
 * overshoot.
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
internal fun MiniPlayerBar(onExpand: () -> Unit, position: () -> Long, hero: HeroGeometry, thumbHidden: () -> Boolean) {
    val client = LocalCoreClient.current
    val entry by client.nowPlaying.collectAsStateWithLifecycle()
    val playing by client.isPlaying.collectAsStateWithLifecycle()
    val transport by client.transport.collectAsStateWithLifecycle()
    val playerNotice by client.playerNotice.collectAsStateWithLifecycle()
    val notice = playerNoticeText(playerNotice)
    val resume by client.resumeOffer.collectAsStateWithLifecycle()
    val track = entry?.track
    val scope = rememberCoroutineScope()
    val haptics = LocalHapticFeedback.current
    val density = LocalDensity.current
    val swipe = remember { Animatable(0f) }
    val threshold = with(density) { 72.dp.toPx() }
    val title = track?.title ?: ""
    val artist = track?.artist ?: stringResource(R.string.unknown_artist)
    val nowPlayingDesc = stringResource(R.string.player_mini_a11y, title, artist)
    val sub = when {
        notice != null -> notice
        resume != null -> stringResource(R.string.player_resume_offer, resume!!.deviceName, resume!!.track.title)
        else -> artist
    }
    val desc = if (sub != artist) "$nowPlayingDesc. $sub" else nowPlayingDesc
    val expandLabel = stringResource(R.string.player_expand)
    val nextLabel = stringResource(R.string.action_next)
    val previousLabel = stringResource(R.string.action_previous)
    val reducedMotion = LocalReducedMotion.current
    val durationMs = track?.durationMs?.toFloat()?.takeIf { it > 0f } ?: 1f
    val line = MaterialTheme.colorScheme.primary
    val lineTrack = MaterialTheme.colorScheme.onSurface.copy(alpha = 0.08f)
    // The item's description already carries both lines; the scrolling texts themselves stay out of
    // the tree (a marquee is laid out wider than its box on purpose).
    val marquee = if (reducedMotion) Modifier else Modifier.clearAndSetSemantics { }.basicMarquee(iterations = 1, initialDelayMillis = 3_000, velocity = 30.dp)
    Box(Modifier.fillMaxSize(), contentAlignment = Alignment.Center) {
        Box(
            Modifier
                .widthIn(max = HeroGeometry.MINI_MAX_WIDTH)
                .fillMaxSize()
                .padding(horizontal = HeroGeometry.MINI_MARGIN)
                .background(MaterialTheme.colorScheme.surfaceContainer)
                // The progress line along the bottom edge (the sheet's clip rounds its ends).
                .drawWithContent {
                    drawContent()
                    val h = 3.dp.toPx()
                    drawRect(lineTrack, topLeft = Offset(0f, size.height - h), size = Size(size.width, h))
                    val f = (position() / durationMs).coerceIn(0f, 1f)
                    drawRect(line, topLeft = Offset(0f, size.height - h), size = Size(size.width * f, h))
                }
                .pointerInput(Unit) {
                    var crossed = false
                    detectHorizontalDragGestures(
                        onDragStart = { crossed = false },
                        onDragEnd = {
                            scope.launch {
                                val v = swipe.value
                                when {
                                    v < -threshold -> { client.dispatch(Command.Next); haptics.performHapticFeedback(HapticFeedbackType.Confirm) }
                                    v > threshold -> { client.dispatch(Command.Previous); haptics.performHapticFeedback(HapticFeedbackType.Confirm) }
                                }
                                swipe.animateTo(0f, Motion.snap)
                            }
                        },
                        onDragCancel = { scope.launch { swipe.animateTo(0f, Motion.snap) } },
                    ) { change, delta ->
                        change.consume()
                        // Resistance past the threshold; a tick when the finger crosses it either way.
                        val next = swipe.value + delta
                        val resisted = if (abs(next) > threshold) threshold * next.sign() + (abs(next) - threshold) * 0.35f * next.sign() else next
                        val over = abs(resisted) > threshold
                        if (over != crossed) { crossed = over; haptics.performHapticFeedback(if (over) HapticFeedbackType.GestureThresholdActivate else HapticFeedbackType.SegmentTick) }
                        scope.launch { swipe.snapTo(resisted) }
                    }
                }
                // Taps outside the info (the thumbnail, the card's edges) expand too; pointer only,
                // the info item below carries the accessible click.
                .pointerInput(onExpand) { detectTapGestures(onTap = { onExpand() }) }
                .testTag("miniPlayer"),
        ) {
            androidx.compose.runtime.CompositionLocalProvider(androidx.compose.material3.LocalContentColor provides MaterialTheme.colorScheme.onSurface) {
            Row(
                Modifier
                    .fillMaxSize()
                    .graphicsLayer {
                        translationX = swipe.value
                        alpha = 1f - (abs(swipe.value) / (threshold * 3f)).coerceIn(0f, 0.6f)
                    }
                    .padding(start = 10.dp, end = 4.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Artwork(
                    track?.coverArt, ArtworkSizes.LIST, null,
                    Modifier
                        .size(HeroGeometry.THUMB_SIZE)
                        .onGloballyPositioned { hero.thumb = hero.measure(it) }
                        .graphicsLayer { alpha = if (thumbHidden()) 0f else 1f },
                    RoundedCornerShape(HeroGeometry.THUMB_CORNER),
                )
                Spacer(Modifier.width(12.dp))
                Column(
                    Modifier
                        .weight(1f)
                        .fillMaxHeight()
                        .clickable(onClickLabel = expandLabel, onClick = onExpand)
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
                    Text(sub, style = MaterialTheme.typography.bodyMedium, color = if (notice != null) MaterialTheme.colorScheme.error else MaterialTheme.colorScheme.onSurfaceVariant, maxLines = 1, overflow = if (reducedMotion) TextOverflow.Ellipsis else TextOverflow.Clip, modifier = marquee)
                }
                val colors = IconButtonDefaults.iconButtonVibrantColors()
                val playLabel = stringResource(if (playing) R.string.action_pause else R.string.action_play)
                val bufferingLabel = stringResource(R.string.player_buffering)
                IconButton(
                    onClick = { haptics.performHapticFeedback(if (playing) HapticFeedbackType.ToggleOff else HapticFeedbackType.ToggleOn); client.dispatch(Command.TogglePlay) },
                    colors = colors,
                    modifier = Modifier.testTag("miniPlayer.playPause").semantics { contentDescription = playLabel; if (transport.buffering) stateDescription = bufferingLabel },
                ) {
                    if (transport.buffering) LoadingIndicator(Modifier.size(28.dp), color = androidx.compose.material3.LocalContentColor.current)
                    else Icon(if (playing) Icons.Filled.Pause else Icons.Filled.PlayArrow, null)
                }
                IconButton(onClick = { client.dispatch(Command.Next) }, colors = colors) { Icon(Icons.Filled.SkipNext, stringResource(R.string.action_next)) }
            }
            }
        }
    }
}

private fun Float.sign(): Float = if (this < 0f) -1f else 1f
