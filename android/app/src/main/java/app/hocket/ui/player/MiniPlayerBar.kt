package app.hocket.ui.player

import androidx.compose.animation.core.Animatable
import androidx.compose.foundation.clickable
import androidx.compose.foundation.gestures.detectHorizontalDragGestures
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Pause
import androidx.compose.material.icons.filled.PlayArrow
import androidx.compose.material.icons.filled.SkipNext
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.LinearWavyProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.WavyProgressIndicatorDefaults
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.hapticfeedback.HapticFeedbackType
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.LocalHapticFeedback
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.stringResource
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.ui.semantics.CustomAccessibilityAction
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.customActions
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.onClick
import app.hocket.ui.a11y.LocalReducedMotion
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.IntOffset
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import app.hocket.R
import app.hocket.core.api.Command
import app.hocket.ui.LocalCoreClient
import app.hocket.ui.theme.Motion
import kotlinx.coroutines.launch
import kotlin.math.abs
import kotlin.math.roundToInt

/**
 * The collapsed player: tap to expand, swipe left/right to skip (springs back), thin wavy progress.
 * The artwork itself is drawn by the sheet so it can scale into the hero.
 *
 * Accessibility: one item "Now playing: <title> by <artist>" (plus a notice when there is one) whose
 * click expands the player and whose actions replace the skip swipe (next / previous track); the
 * play/pause and next buttons stay separate targets. The item is a polite live region: its text
 * changes only when the track (or the notice) does, so a track change is announced once and
 * position never is (the progress line is hidden from accessibility; the full player's seek bar
 * carries the position).
 */
@Composable
fun MiniPlayerBar(onExpand: () -> Unit) {
    val client = LocalCoreClient.current
    val entry by client.nowPlaying.collectAsStateWithLifecycle()
    val playing by client.isPlaying.collectAsStateWithLifecycle()
    val position by client.position.collectAsStateWithLifecycle()
    val notice by client.playerNotice.collectAsStateWithLifecycle()
    val resume by client.resumeOffer.collectAsStateWithLifecycle()
    val track = entry?.track
    val scope = rememberCoroutineScope()
    val haptics = LocalHapticFeedback.current
    val density = LocalDensity.current
    val swipe = remember { Animatable(0f) }
    val threshold = with(density) { 96.dp.toPx() }
    val title = track?.title ?: ""
    val artist = track?.artist ?: stringResource(R.string.unknown_artist)
    val nowPlayingDesc = stringResource(R.string.player_mini_a11y, title, artist)
    val sub = when {
        notice != null -> notice!!
        resume != null -> stringResource(R.string.player_resume_offer, resume!!.deviceName, resume!!.track.title)
        else -> artist
    }
    val desc = if (sub != artist) "$nowPlayingDesc. $sub" else nowPlayingDesc
    val expandLabel = stringResource(R.string.player_expand)
    val nextLabel = stringResource(R.string.action_next)
    val previousLabel = stringResource(R.string.action_previous)
    val reducedMotion = LocalReducedMotion.current
    Column(
        Modifier
            .fillMaxSize()
            .offset { IntOffset(swipe.value.roundToInt(), 0) }
            .pointerInput(Unit) {
                detectHorizontalDragGestures(
                    onDragStart = { haptics.performHapticFeedback(HapticFeedbackType.GestureThresholdActivate) },
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
                    // Resistance past the threshold.
                    val next = swipe.value + delta
                    val resisted = if (abs(next) > threshold) threshold * next.sign() + (abs(next) - threshold) * 0.35f * next.sign() else next
                    scope.launch { swipe.snapTo(resisted) }
                }
            }
            // Taps outside the info (the thumbnail, the progress line) expand too; pointer only, the
            // info item below carries the accessible click.
            .pointerInput(onExpand) { detectTapGestures(onTap = { onExpand() }) }
            .testTag("miniPlayer"),
    ) {
        Row(Modifier.weight(1f).fillMaxWidth().padding(start = 64.dp, end = 4.dp), verticalAlignment = Alignment.CenterVertically) {
            Column(
                Modifier
                    .weight(1f)
                    .fillMaxHeight()
                    .clickable(onClickLabel = expandLabel, onClick = onExpand)
                    .clearAndSetSemantics {
                        contentDescription = desc
                        liveRegion = LiveRegionMode.Polite
                        customActions = listOf(
                            CustomAccessibilityAction(nextLabel) { client.dispatch(Command.Next); true },
                            CustomAccessibilityAction(previousLabel) { client.dispatch(Command.Previous); true },
                        )
                        onClick(expandLabel) { onExpand(); true }
                    }
                    .testTag("miniPlayer.info"),
                verticalArrangement = Arrangement.Center,
            ) {
                Text(title, style = MaterialTheme.typography.bodyLarge, maxLines = 1, overflow = TextOverflow.Ellipsis)
                Text(sub, style = MaterialTheme.typography.bodySmall, color = if (notice != null) MaterialTheme.colorScheme.error else MaterialTheme.colorScheme.onSurfaceVariant, maxLines = 1, overflow = TextOverflow.Ellipsis)
            }
            IconButton(onClick = { client.dispatch(Command.TogglePlay) }, modifier = Modifier.testTag("miniPlayer.playPause")) {
                Icon(if (playing) Icons.Filled.Pause else Icons.Filled.PlayArrow, stringResource(if (playing) R.string.action_pause else R.string.action_play))
            }
            IconButton(onClick = { client.dispatch(Command.Next) }) { Icon(Icons.Filled.SkipNext, stringResource(R.string.action_next)) }
        }
        val duration = track?.durationMs?.toFloat()?.takeIf { it > 0f } ?: 1f
        LinearWavyProgressIndicator(
            progress = { (position / duration).coerceIn(0f, 1f) },
            // Position ticks are never exposed here (they would be re-read at 60 Hz); the full
            // player's seek bar is the accessible position control.
            modifier = Modifier.fillMaxWidth().height(6.dp).padding(horizontal = 8.dp).clearAndSetSemantics { },
            stroke = Stroke(width = with(density) { 2.dp.toPx() }),
            trackStroke = Stroke(width = with(density) { 2.dp.toPx() }),
            amplitude = { if (playing && !reducedMotion) 1f else 0f },
            wavelength = 24.dp,
        )
        Spacer(Modifier.height(2.dp))
    }
}

private fun Float.sign(): Float = if (this < 0f) -1f else 1f
