package app.hocket.ui.components

import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.foundation.gestures.waitForUpOrCancellation
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.ui.Modifier
import androidx.compose.ui.hapticfeedback.HapticFeedbackType
import androidx.compose.ui.input.pointer.PointerEventPass
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.platform.LocalHapticFeedback

/**
 * A long press for a Material button that has no `onLongClick`: put this on the button's modifier.
 * The hold is timed from the finger going down (a button in a scrolling container only reports its
 * press after the tap timeout), and once [onHold] runs the rest of the gesture is consumed so the
 * button's own click doesn't follow. A press the container takes over (a scroll) or one that leaves
 * the button cancels it. Disabled, it does nothing and the button behaves as usual. Give TalkBack
 * the same action with `semantics { onLongClick(...) }`.
 */
@Composable
fun rememberHoldModifier(enabled: Boolean, onHold: () -> Unit): Modifier {
    val haptics = LocalHapticFeedback.current
    val latest by rememberUpdatedState(onHold)
    if (!enabled) return Modifier
    return Modifier.pointerInput(Unit) {
        awaitEachGesture {
            awaitFirstDown(requireUnconsumed = false, pass = PointerEventPass.Initial)
            var ended = false
            withTimeoutOrNull(viewConfiguration.longPressTimeoutMillis) {
                waitForUpOrCancellation(PointerEventPass.Initial)
                ended = true
            }
            if (ended) return@awaitEachGesture
            haptics.performHapticFeedback(HapticFeedbackType.LongPress)
            latest()
            // Swallow the rest, up included: the button sees its press cancelled, not a click.
            do {
                val event = awaitPointerEvent(PointerEventPass.Initial)
                event.changes.forEach { it.consume() }
            } while (event.changes.any { it.pressed })
        }
    }
}
