package app.hocket.ui.player

import androidx.compose.animation.core.Animatable
import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.gestures.detectHorizontalDragGestures
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Bedtime
import androidx.compose.material.icons.filled.Cast
import androidx.compose.material.icons.filled.Favorite
import androidx.compose.material.icons.filled.FavoriteBorder
import androidx.compose.material.icons.filled.MoreVert
import androidx.compose.material.icons.filled.Repeat
import androidx.compose.material.icons.filled.RepeatOne
import androidx.compose.material.icons.filled.Shuffle
import androidx.compose.material.icons.filled.SkipNext
import androidx.compose.material.icons.filled.SkipPrevious
import androidx.compose.material.icons.filled.AutoAwesome
import androidx.compose.material3.ButtonGroupDefaults
import androidx.compose.material3.FilledTonalIconButton
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.IconToggleButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.Role
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.hapticfeedback.HapticFeedbackType
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.LocalHapticFeedback
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.stringResource
import androidx.compose.foundation.clickable
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.layout.wrapContentHeight
import androidx.compose.material.icons.filled.Close
import androidx.compose.foundation.layout.heightIn
import androidx.compose.ui.semantics.CustomAccessibilityAction
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.customActions
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.semantics
import app.hocket.ui.a11y.LocalReducedMotion
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.IntOffset
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import app.hocket.R
import app.hocket.core.ArtworkSizes
import app.hocket.core.Commands
import app.hocket.core.api.Command
import app.hocket.core.api.QueueSource
import app.hocket.core.api.RepeatMode
import app.hocket.ui.LocalCoreClient
import app.hocket.ui.components.ActionSheet
import app.hocket.ui.components.Artwork
import app.hocket.ui.components.RatingStars
import app.hocket.ui.components.formatClock
import app.hocket.ui.theme.ArtworkColors
import app.hocket.ui.theme.Motion
import kotlinx.coroutines.launch
import kotlin.math.abs
import kotlin.math.roundToInt

/**
 * The hero artwork: swipe horizontally to skip (springs back), long-press toggles a whole-app
 * dynamic colour preview from this artwork. Drawn by the sheet so it can scale from the mini bar.
 */
@OptIn(ExperimentalFoundationApi::class)
@Composable
fun HeroArtwork(coverArt: String?, title: String?, size: Dp, interactive: Boolean, describe: Boolean = true, onPreviewToggle: (Color) -> Unit) {
    val client = LocalCoreClient.current
    val scope = rememberCoroutineScope()
    val haptics = LocalHapticFeedback.current
    val density = LocalDensity.current
    val swipe = remember { Animatable(0f) }
    val threshold = with(density) { 120.dp.toPx() }
    val seed by artworkSeed(coverArt)
    val desc = stringResource(R.string.player_artwork_a11y, title ?: "")
    val previewDesc = stringResource(R.string.player_dynamic_preview)
    val nextLabel = stringResource(R.string.action_next)
    val previousLabel = stringResource(R.string.action_previous)
    val togglePreview = { haptics.performHapticFeedback(HapticFeedbackType.LongPress); onPreviewToggle(seed ?: ArtworkColors.seedFor(coverArt)) }
    Box(
        Modifier
            .offset { IntOffset(swipe.value.roundToInt(), 0) }
            .size(size)
            .then(if (interactive) Modifier
                .pointerInput(Unit) {
                    detectHorizontalDragGestures(
                        onDragStart = { haptics.performHapticFeedback(HapticFeedbackType.GestureThresholdActivate) },
                        onDragEnd = {
                            scope.launch {
                                val v = swipe.value
                                if (v < -threshold) { client.dispatch(Command.Next); haptics.performHapticFeedback(HapticFeedbackType.Confirm) }
                                if (v > threshold) { client.dispatch(Command.Previous); haptics.performHapticFeedback(HapticFeedbackType.Confirm) }
                                swipe.animateTo(0f, Motion.snap)
                            }
                        },
                        onDragCancel = { scope.launch { swipe.animateTo(0f, Motion.snap) } },
                    ) { change, delta ->
                        change.consume()
                        val next = swipe.value + delta
                        val resisted = if (abs(next) > threshold) (if (next < 0) -1f else 1f) * (threshold + (abs(next) - threshold) * 0.3f) else next
                        scope.launch { swipe.snapTo(resisted) }
                    }
                }
                // Long-press only (no click to announce): a pointer gesture, mirrored by the actions below.
                .pointerInput(Unit) { detectTapGestures(onLongPress = { togglePreview() }) }
            else Modifier)
            // The skip swipe and the colour-preview long-press as actions; in the collapsed sheet the
            // thumbnail is decorative (the mini player's item names the track).
            .testTag("player.artwork")
            .then(if (describe) Modifier.semantics {
                contentDescription = desc
                if (interactive) customActions = listOf(
                    CustomAccessibilityAction(nextLabel) { client.dispatch(Command.Next); true },
                    CustomAccessibilityAction(previousLabel) { client.dispatch(Command.Previous); true },
                    CustomAccessibilityAction(previewDesc) { togglePreview(); true },
                )
            } else Modifier.clearAndSetSemantics { }),
    ) {
        Artwork(coverArt, ArtworkSizes.FULL, null, Modifier.fillMaxSize(), RoundedCornerShape(androidx.compose.ui.unit.lerp(12.dp, 28.dp, ((size - 48.dp) / 300.dp).coerceIn(0f, 1f))))
    }
}

/** Track info, transport, seek, toggles, sleep timer, Connect, resume offer and notices. */
@OptIn(ExperimentalFoundationApi::class)
@Composable
fun NowPlayingPage(sheetProgress: Float, onOpenAlbum: (String) -> Unit, onOpenArtist: (String) -> Unit, artworkSlot: Boolean) {
    val client = LocalCoreClient.current
    val entry by client.nowPlaying.collectAsStateWithLifecycle()
    val queue by client.queue.collectAsStateWithLifecycle()
    val playing by client.isPlaying.collectAsStateWithLifecycle()
    val transport by client.transport.collectAsStateWithLifecycle()
    val position by client.position.collectAsStateWithLifecycle()
    val playerNotice by client.playerNotice.collectAsStateWithLifecycle()
    val notice = playerNoticeText(playerNotice)
    val resume by client.resumeOffer.collectAsStateWithLifecycle()
    val sleep by client.sleepTimer.collectAsStateWithLifecycle()
    val devices by client.devices.collectAsStateWithLifecycle()
    val owns by client.ownsTransport.collectAsStateWithLifecycle()
    var handoff by remember { mutableStateOf(false) }
    var sleepSheet by remember { mutableStateOf(false) }
    var more by remember { mutableStateOf(false) }
    val track = entry?.track
    val density = LocalDensity.current
    androidx.compose.foundation.layout.BoxWithConstraints(Modifier.fillMaxSize()) {
        val hero = with(density) { (constraints.maxWidth.toDp() - 48.dp).coerceAtMost(420.dp) }
        val pageWidth = maxWidth
        Column(Modifier.fillMaxSize().verticalScroll(rememberScrollState()).testTag("player.page").padding(horizontal = 24.dp).navigationBarsPadding(), horizontalAlignment = Alignment.CenterHorizontally) {
            // Artwork slot (the sheet draws the artwork over this space).
            Spacer(Modifier.height(hero + 24.dp))
            if (track == null) {
                Text(stringResource(R.string.empty_queue_title), style = MaterialTheme.typography.headlineSmall)
                Text(stringResource(R.string.empty_queue_body), style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant, textAlign = TextAlign.Center)
                return@Column
            }
            // Notices: skipped-unavailable, resume offer, autoplay "why", remote playback.
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

            Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
                Column(Modifier.weight(1f)) {
                    // The page's heading, and a polite live region: it changes once per track, so a
                    // track change is announced once (never the position).
                    val trackDesc = stringResource(R.string.player_track_a11y, track.title, track.artist ?: stringResource(R.string.unknown_artist))
                    Text(track.title, style = MaterialTheme.typography.headlineSmall, maxLines = 2, overflow = TextOverflow.Ellipsis,
                        modifier = Modifier.semantics { heading(); liveRegion = LiveRegionMode.Polite; contentDescription = trackDesc }.testTag("player.title"))
                    val goArtist = stringResource(R.string.action_go_to_artist)
                    val goAlbum = stringResource(R.string.action_go_to_album)
                    Text(track.artist ?: stringResource(R.string.unknown_artist), style = MaterialTheme.typography.bodyLarge, color = MaterialTheme.colorScheme.onSurfaceVariant, maxLines = 1, overflow = TextOverflow.Ellipsis,
                        modifier = Modifier.then(track.artistId?.let { id -> Modifier.textLink(goArtist) { onOpenArtist(id) } } ?: Modifier))
                    track.album?.let { Text(it, style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant, maxLines = 1, overflow = TextOverflow.Ellipsis,
                        modifier = Modifier.then(track.albumId?.let { id -> Modifier.textLink(goAlbum) { onOpenAlbum(id) } } ?: Modifier)) }
                }
                IconToggleButton(checked = track.loved, onCheckedChange = { client.dispatch(Commands.loveTrack(track.id, it)) }, modifier = Modifier.testTag("player.love")) {
                    Icon(if (track.loved) Icons.Filled.Favorite else Icons.Filled.FavoriteBorder, stringResource(if (track.loved) R.string.player_loved else R.string.player_not_loved), tint = if (track.loved) MaterialTheme.colorScheme.tertiary else MaterialTheme.colorScheme.onSurfaceVariant)
                }
                IconButton(onClick = { more = true }, modifier = Modifier.testTag("player.more")) { Icon(Icons.Filled.MoreVert, stringResource(R.string.action_more)) }
            }
            RatingStars(track.rating.toInt(), onRate = { client.dispatch(Commands.rateTrack(track.id, it)) }, starSize = 22.dp, modifier = Modifier.padding(top = 4.dp))
            Spacer(Modifier.height(8.dp))
            WavySeekBar(positionMs = position, durationMs = track.durationMs.toLong(), playing = playing && !LocalReducedMotion.current, onSeek = { client.dispatch(Commands.seekTo(it)) })
            if (transport.buffering) Text(stringResource(R.string.player_buffering), style = MaterialTheme.typography.labelSmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
            Spacer(Modifier.height(8.dp))
            // Narrow screens (display size "largest" leaves ~320 dp): smaller skip and play buttons so
            // the five controls never overlap or clip.
            val narrow = pageWidth < 360.dp
            val skipSize = if (narrow) 48.dp else 56.dp
            val playSize = if (narrow) 72.dp else 88.dp
            Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.SpaceEvenly, verticalAlignment = Alignment.CenterVertically) {
                IconToggleButton(checked = queue.shuffle, onCheckedChange = { client.dispatch(Commands.setShuffle(it)) }) {
                    Icon(Icons.Filled.Shuffle, stringResource(if (queue.shuffle) R.string.player_shuffle_on else R.string.player_shuffle_off), tint = if (queue.shuffle) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.onSurfaceVariant)
                }
                FilledTonalIconButton(onClick = { client.dispatch(Command.Previous) }, modifier = Modifier.size(skipSize).testTag("player.previous")) { Icon(Icons.Filled.SkipPrevious, stringResource(R.string.action_previous), Modifier.size(32.dp)) }
                PlayPauseButton(playing = playing, onToggle = { client.dispatch(Command.TogglePlay) }, size = playSize)
                FilledTonalIconButton(onClick = { client.dispatch(Command.Next) }, modifier = Modifier.size(skipSize).testTag("player.next")) { Icon(Icons.Filled.SkipNext, stringResource(R.string.action_next), Modifier.size(32.dp)) }
                val repeatLabel = stringResource(when (queue.repeat) { RepeatMode.Off -> R.string.player_repeat_off; RepeatMode.All -> R.string.player_repeat_all; RepeatMode.One -> R.string.player_repeat_one })
                IconButton(onClick = { client.dispatch(Commands.setRepeat(when (queue.repeat) { RepeatMode.Off -> RepeatMode.All; RepeatMode.All -> RepeatMode.One; RepeatMode.One -> RepeatMode.Off })) }) {
                    Icon(if (queue.repeat == RepeatMode.One) Icons.Filled.RepeatOne else Icons.Filled.Repeat, repeatLabel, tint = if (queue.repeat != RepeatMode.Off) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.onSurfaceVariant)
                }
            }
            Spacer(Modifier.height(8.dp))
            Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.SpaceEvenly, verticalAlignment = Alignment.CenterVertically) {
                IconToggleButton(checked = queue.autoplay, onCheckedChange = { client.dispatch(Commands.setAutoplay(it)) }) {
                    Icon(Icons.Filled.AutoAwesome, stringResource(if (queue.autoplay) R.string.player_autoplay_on else R.string.player_autoplay_off), tint = if (queue.autoplay) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.onSurfaceVariant)
                }
                val sleepActive = sleep != null
                IconToggleButton(checked = sleepActive, onCheckedChange = { sleepSheet = true }, modifier = Modifier.testTag("player.sleep")) {
                    Icon(Icons.Filled.Bedtime, stringResource(R.string.player_sleep_timer), tint = if (sleepActive) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.onSurfaceVariant)
                }
                // Tap opens the picker; long-press pulls playback straight to this device.
                val haptics = LocalHapticFeedback.current
                val takeOverLabel = stringResource(R.string.player_take_over)
                Box(
                    Modifier.size(48.dp).clip(CircleShape)
                        .combinedClickable(
                            role = Role.Button,
                            onClick = { handoff = true },
                            onLongClickLabel = takeOverLabel,
                            onLongClick = {
                                val self = devices.firstOrNull { it.isSelf }
                                if (!owns && self != null) {
                                    haptics.performHapticFeedback(HapticFeedbackType.LongPress)
                                    client.dispatch(Commands.handoffTo(self.id))
                                } else handoff = true
                            },
                        )
                        .testTag("player.connect"),
                    contentAlignment = Alignment.Center,
                ) {
                    Icon(Icons.Filled.Cast, stringResource(R.string.player_connect), tint = if (!owns) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.onSurfaceVariant)
                }
            }
            sleep?.let { t ->
                val label = t.endsAt?.let { stringResource(R.string.sleep_active, formatClock((it - System.currentTimeMillis()).toLong().coerceAtLeast(0))) } ?: stringResource(R.string.sleep_active_end_of_track)
                Text(label, style = MaterialTheme.typography.labelMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
            }
            Spacer(Modifier.height(24.dp))
        }
    }
    if (handoff) HandoffSheet(onDismiss = { handoff = false })
    if (sleepSheet) SleepTimerSheet(onDismiss = { sleepSheet = false })
    if (more && track != null) ActionSheet(Commands.tracks(listOf(track.id)), track.title, track.artist, onDismiss = { more = false },
        onGoToAlbum = track.albumId?.let { id -> { onOpenAlbum(id) } }, onGoToArtist = track.artistId?.let { id -> { onOpenArtist(id) } })
}

@Composable
private fun NoticeLine(text: String, color: Color) {
    Text(text, style = MaterialTheme.typography.labelMedium, color = color, modifier = Modifier.fillMaxWidth().padding(bottom = 4.dp), maxLines = 1, overflow = TextOverflow.Ellipsis)
}

/** A text that navigates: labelled click, at least a 48 dp touch target, content vertically centred. */
private fun Modifier.textLink(label: String, onClick: () -> Unit): Modifier =
    this.clickable(onClickLabel = label, onClick = onClick).heightIn(min = 48.dp).wrapContentHeight(Alignment.CenterVertically)
