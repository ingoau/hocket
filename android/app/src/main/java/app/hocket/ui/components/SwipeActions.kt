package app.hocket.ui.components

import androidx.compose.animation.core.animate
import androidx.compose.animation.core.spring
import androidx.compose.animation.core.tween
import androidx.compose.foundation.background
import androidx.compose.foundation.gestures.Orientation
import androidx.compose.foundation.gestures.draggable
import androidx.compose.foundation.gestures.rememberDraggableState
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.Immutable
import androidx.compose.runtime.derivedStateOf
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.drawBehind
import androidx.compose.ui.draw.scale
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.graphics.isSpecified
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.hapticfeedback.HapticFeedbackType
import androidx.compose.ui.layout.onSizeChanged
import androidx.compose.ui.platform.LocalHapticFeedback
import androidx.compose.ui.platform.LocalLayoutDirection
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.LayoutDirection
import androidx.compose.ui.unit.dp
import app.hocket.core.ActionIds
import app.hocket.core.Commands
import app.hocket.core.SettingKeys
import app.hocket.core.SwipeOptions
import app.hocket.core.api.ActionTarget
import app.hocket.core.api.OfflineState
import app.hocket.core.api.TrackSummary
import app.hocket.ui.LocalCoreClient
import app.hocket.ui.icons.HocketIcons
import app.hocket.ui.screens.settings.actionLabel
import app.hocket.ui.screens.settings.setting
import kotlinx.coroutines.launch
import kotlin.math.abs
import kotlin.math.sign

/** How far (of the row's width) a swipe has to travel before letting go runs its action. */
private const val SWIPE_THRESHOLD = 0.3f

/** How far (of the row's width) the row follows the finger. */
private const val SWIPE_MAX = 0.7f

/** One side of a row's swipe: the action it runs and how its revealed background looks. */
@Immutable
class SwipeAction(
    /** The registry id actually run (love or unlove for "love"): a test and debugging handle. */
    val id: String,
    val label: String,
    val icon: ImageVector,
    val container: Color,
    val content: Color,
    /** The row goes away (remove from queue): it slides out instead of springing back. */
    val dismisses: Boolean,
    val onSwipe: () -> Unit,
)

/** Which pair of swipe settings a row follows. */
enum class SwipeSurface(val startToEndKey: String, val endToStartKey: String) {
    Queue(SettingKeys.SWIPE_QUEUE_START_TO_END, SettingKeys.SWIPE_QUEUE_END_TO_START),
    List(SettingKeys.SWIPE_LIST_START_TO_END, SettingKeys.SWIPE_LIST_END_TO_START),
}

/** The configured action ids (start-to-end, end-to-start) for [surface]; the registry defaults until the core has sent them. */
@Composable
fun swipeActionIds(surface: SwipeSurface): Pair<String, String> {
    fun id(key: String, stored: String?) = stored ?: SwipeOptions.DEFAULTS.getValue(key)
    return id(surface.startToEndKey, setting(surface.startToEndKey).string) to id(surface.endToStartKey, setting(surface.endToStartKey).string)
}

/**
 * The [SwipeAction] a swipe setting's [id] means for one song row, or null (none, or nothing to do:
 * "download" on a row that is already downloaded). Everything runs through the core's action
 * registry on [target]; "love" runs love or unlove by the row's state, "addToPlaylist" (handled by
 * the platform) calls [onAddToPlaylist] to open the picker.
 */
@Composable
fun trackSwipeAction(id: String, track: TrackSummary, target: ActionTarget, onAddToPlaylist: () -> Unit): SwipeAction? {
    val client = LocalCoreClient.current
    val scheme = MaterialTheme.colorScheme
    val run = { action: String -> { client.dispatch(Commands.runAction(action, target)) } }
    return when (id) {
        ActionIds.REMOVE_FROM_QUEUE -> SwipeAction(id, actionLabel(id), HocketIcons.Filled.Delete, scheme.errorContainer, scheme.onErrorContainer, dismisses = true, run(id))
        ActionIds.PLAY_NEXT -> SwipeAction(id, actionLabel(id), HocketIcons.AutoMirrored.Filled.PlaylistPlay, scheme.primaryContainer, scheme.onPrimaryContainer, dismisses = false, run(id))
        ActionIds.PLAY_LATER -> SwipeAction(id, actionLabel(id), HocketIcons.AutoMirrored.Filled.QueueMusic, scheme.secondaryContainer, scheme.onSecondaryContainer, dismisses = false, run(id))
        ActionIds.LOVE -> {
            val action = if (track.loved) ActionIds.UNLOVE else ActionIds.LOVE
            SwipeAction(action, actionLabel(action), if (track.loved) HocketIcons.Filled.FavoriteBorder else HocketIcons.Filled.Favorite, scheme.tertiaryContainer, scheme.onTertiaryContainer, dismisses = false, run(action))
        }
        ActionIds.ADD_TO_PLAYLIST -> SwipeAction(id, actionLabel(id), HocketIcons.AutoMirrored.Filled.PlaylistAdd, scheme.secondaryContainer, scheme.onSecondaryContainer, dismisses = false, onAddToPlaylist)
        ActionIds.DOWNLOAD -> if (track.offline == OfflineState.Downloaded) null
            else SwipeAction(id, actionLabel(id), HocketIcons.Filled.Download, scheme.primaryContainer, scheme.onPrimaryContainer, dismisses = false, run(id))
        else -> null
    }
}

/**
 * A row that can be swiped sideways. Past [SWIPE_THRESHOLD] of its width (a haptic tick says so),
 * letting go runs the side's action: one that [SwipeAction.dismisses] slides the row out, anything
 * else springs it back. A side without an action does not move; with neither, the box takes no
 * gestures at all (so a pager around it keeps its swipes).
 *
 * Horizontal only: vertical drags stay with the list, and a drag handle inside wins the drags that
 * start on it. The revealed background (and [swipeSurface] under the content, for rows that are
 * transparent at rest, like the queue over the player's artwork) is drawn only while the row is
 * off its rest position. The swipe is a shortcut: every action is also in the row's menu and its
 * accessibility actions, so the background is decorative.
 */
@Composable
fun SwipeActionBox(
    startToEnd: SwipeAction?,
    endToStart: SwipeAction?,
    modifier: Modifier = Modifier,
    enabled: Boolean = true,
    swipeSurface: Color = Color.Unspecified,
    content: @Composable () -> Unit,
) {
    val scope = rememberCoroutineScope()
    val haptics = LocalHapticFeedback.current
    val rtl = LocalLayoutDirection.current == LayoutDirection.Rtl
    // In reading-direction terms: positive is towards the end of the line.
    var offset by remember { mutableFloatStateOf(0f) }
    var width by remember { mutableIntStateOf(0) }
    var armed by remember { mutableStateOf(false) }
    val start by rememberUpdatedState(startToEnd)
    val end by rememberUpdatedState(endToStart)
    val side by remember { derivedStateOf { sign(offset) } }
    val active = enabled && (startToEnd != null || endToStart != null)
    val drag = rememberDraggableState { delta ->
        val max = if (start != null) width * SWIPE_MAX else 0f
        val min = if (end != null) -width * SWIPE_MAX else 0f
        offset = (offset + delta).coerceIn(min, max)
        val nowArmed = width > 0 && abs(offset) >= width * SWIPE_THRESHOLD
        if (nowArmed != armed) {
            armed = nowArmed
            haptics.performHapticFeedback(if (nowArmed) HapticFeedbackType.GestureThresholdActivate else HapticFeedbackType.SegmentTick)
        }
    }
    Box(
        modifier
            .onSizeChanged { width = it.width }
            .draggable(
                state = drag,
                orientation = Orientation.Horizontal,
                enabled = active,
                reverseDirection = rtl,
                onDragStopped = {
                    val from = offset
                    // Only past the threshold; short of it the row just springs back.
                    val action = (if (from > 0f) start else if (from < 0f) end else null)?.takeIf { armed }
                    armed = false
                    scope.launch {
                        if (action != null) {
                            haptics.performHapticFeedback(HapticFeedbackType.Confirm)
                            action.onSwipe()
                            if (action.dismisses) {
                                // Out of sight; the row leaves the list with the core's update. Back to
                                // rest afterwards, in case it stays (the core refused).
                                animate(from, sign(from) * width, animationSpec = tween(160)) { v, _ -> offset = v }
                                offset = 0f
                                return@launch
                            }
                        }
                        animate(from, 0f, animationSpec = spring()) { v, _ -> offset = v }
                    }
                },
            ),
    ) {
        if (side != 0f) {
            val action = if (side > 0f) startToEnd else endToStart
            if (action != null) SwipeBackground(action, fromStart = side > 0f, armed = armed, modifier = Modifier.matchParentSize())
        }
        Box(
            Modifier
                .graphicsLayer { translationX = if (rtl) -offset else offset }
                .drawBehind { if (offset != 0f && swipeSurface.isSpecified) drawRect(swipeSurface) },
        ) { content() }
    }
}

/** The revealed side: the action's colour, icon and label at the edge the row moved away from. */
@Composable
private fun SwipeBackground(action: SwipeAction, fromStart: Boolean, armed: Boolean, modifier: Modifier) {
    Box(
        modifier.clearAndSetSemantics { }.background(action.container).padding(horizontal = 24.dp).testTag("swipe.background"),
        contentAlignment = if (fromStart) Alignment.CenterStart else Alignment.CenterEnd,
    ) {
        Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.Start) {
            if (!fromStart) Text(action.label, style = MaterialTheme.typography.labelLarge, color = action.content, maxLines = 1, overflow = TextOverflow.Ellipsis)
            if (!fromStart) Spacer(Modifier.width(12.dp))
            Icon(action.icon, null, tint = action.content, modifier = Modifier.scale(if (armed) 1.2f else 1f))
            if (fromStart) Spacer(Modifier.width(12.dp))
            if (fromStart) Text(action.label, style = MaterialTheme.typography.labelLarge, color = action.content, maxLines = 1, overflow = TextOverflow.Ellipsis)
        }
    }
}
