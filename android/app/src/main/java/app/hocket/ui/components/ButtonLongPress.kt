package app.hocket.ui.components

import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.interaction.PressInteraction
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.Stable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.ui.hapticfeedback.HapticFeedbackType
import androidx.compose.ui.platform.LocalHapticFeedback
import androidx.compose.ui.platform.LocalViewConfiguration
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch

/**
 * A long press for a Material button that has no `onLongClick`: pass [interactionSource] to the
 * button and wrap its click in [click]. Held past the long-press timeout, the action runs (with a
 * haptic) and the click its release makes is swallowed. Give TalkBack the same action with a
 * `semantics { onLongClick(...) }`.
 */
@Stable
class ButtonLongPress internal constructor(val interactionSource: MutableInteractionSource) {
    internal var fired = false

    /** Runs [action] unless this press already ran the long press. */
    fun click(action: () -> Unit) {
        if (fired) fired = false else action()
    }
}

@Composable
fun rememberButtonLongPress(onLongPress: () -> Unit): ButtonLongPress {
    val press = remember { ButtonLongPress(MutableInteractionSource()) }
    val haptics = LocalHapticFeedback.current
    val timeout = LocalViewConfiguration.current.longPressTimeoutMillis
    val latest by rememberUpdatedState(onLongPress)
    LaunchedEffect(press, timeout) {
        var pending: Job? = null
        press.interactionSource.interactions.collect { i ->
            when (i) {
                is PressInteraction.Press -> {
                    press.fired = false
                    pending?.cancel()
                    pending = launch {
                        delay(timeout)
                        press.fired = true
                        haptics.performHapticFeedback(HapticFeedbackType.LongPress)
                        latest()
                    }
                }
                is PressInteraction.Release, is PressInteraction.Cancel -> pending?.cancel()
            }
        }
    }
    return press
}
