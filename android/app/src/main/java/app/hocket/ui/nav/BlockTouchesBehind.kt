package app.hocket.ui.nav

import androidx.compose.ui.Modifier
import androidx.compose.ui.input.pointer.pointerInput

/**
 * Makes this element a touch target of its own, so a tap or scroll on it never reaches what is drawn
 * behind it (hit testing stops at the topmost sibling that takes pointer input). It consumes
 * nothing: its children's taps, which are cancelled by any consumption between down and up, still
 * work however much the finger wobbles.
 */
internal fun Modifier.blockTouchesBehind(): Modifier = pointerInput(Unit) {
    awaitPointerEventScope {
        while (true) awaitPointerEvent()
    }
}
