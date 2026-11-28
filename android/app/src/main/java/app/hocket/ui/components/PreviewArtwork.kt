package app.hocket.ui.components

import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.size
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.Icon
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.produceState
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Shape
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.ProgressBarRangeInfo
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.progressBarRangeInfo
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import app.hocket.R
import app.hocket.core.ArtworkSizes
import app.hocket.core.Commands
import app.hocket.core.api.Command
import app.hocket.core.api.PreviewState
import app.hocket.core.api.TrackRef
import app.hocket.ui.LocalCoreClient
import app.hocket.ui.icons.HocketIcons
import kotlinx.coroutines.delay

/**
 * A menu's cover that previews its song. Tapping it plays a 30-second clip (the core's clipped
 * preview, from 30% in) without closing the menu; while the clip is open a ring over the cover
 * fills with it around a pause button (play while paused), and a tap pauses or resumes. [track]
 * null (several songs, an album, the song that is playing) leaves a plain cover. Tags: `[tag]`, and
 * `[tag].ring` while the clip is open.
 */
@Composable
fun PreviewArtwork(coverArt: String?, track: TrackRef?, size: Dp, shape: Shape, tag: String) {
    val client = LocalCoreClient.current
    val preview by client.preview.collectAsStateWithLifecycle()
    val sample = preview?.takeIf { track != null && it.trackId == track.id }
    val now by produceState(System.currentTimeMillis().toDouble(), sample?.playing, sample?.takenAt, sample?.positionMs) {
        while (true) {
            value = System.currentTimeMillis().toDouble()
            if (sample?.playing != true) break
            delay(50)
        }
    }
    val fraction = sample?.let { previewClipFraction(it, now) } ?: 0f
    val label = when {
        sample == null -> stringResource(R.string.menu_preview_play)
        sample.playing -> stringResource(R.string.menu_preview_pause)
        else -> stringResource(R.string.menu_preview_resume)
    }
    Box(
        Modifier
            .size(size)
            .clip(shape)
            .then(
                if (track == null) Modifier
                else Modifier
                    .clickable(onClickLabel = label, role = Role.Button) {
                        if (sample == null) client.dispatch(Commands.previewStart(track, clip = true))
                        else client.dispatch(Command.PreviewToggle)
                    }
                    .semantics {
                        contentDescription = label
                        if (sample != null) progressBarRangeInfo = ProgressBarRangeInfo(fraction, 0f..1f)
                    },
            )
            .testTag(tag),
        contentAlignment = Alignment.Center,
    ) {
        Artwork(coverArt, ArtworkSizes.THUMB, null, Modifier.fillMaxSize(), shape)
        AnimatedVisibility(sample != null, Modifier.fillMaxSize(), enter = fadeIn(), exit = fadeOut()) {
            Box(Modifier.fillMaxSize().background(Color.Black.copy(alpha = 0.45f)).testTag("$tag.ring"), contentAlignment = Alignment.Center) {
                CircularProgressIndicator(
                    progress = { fraction },
                    modifier = Modifier.size(size * 0.62f),
                    color = Color.White,
                    trackColor = Color.White.copy(alpha = 0.3f),
                    strokeWidth = 3.dp,
                    strokeCap = StrokeCap.Round,
                    gapSize = 0.dp,
                )
                Icon(
                    if (sample?.playing == false) HocketIcons.Filled.PlayArrow else HocketIcons.Filled.Pause,
                    null,
                    tint = Color.White,
                    modifier = Modifier.size(size * 0.32f),
                )
            }
        }
    }
}

/** How far a preview is through its clip (through the song when it has none), extrapolated to [now]. */
fun previewClipFraction(sample: PreviewState, now: Double): Float {
    val duration = sample.durationMs.toLong()
    val at = previewPositionMs(sample.positionMs.toLong(), sample.takenAt, sample.playing, duration, now)
    val clip = sample.clip
    val (start, end) = if (clip != null) clip.startMs.toLong() to clip.endMs.toLong() else 0L to duration
    if (end <= start) return 0f
    return ((at - start).toFloat() / (end - start)).coerceIn(0f, 1f)
}
