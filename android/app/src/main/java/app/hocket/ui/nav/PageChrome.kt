package app.hocket.ui.nav

import androidx.compose.material3.ColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.SideEffect
import androidx.compose.runtime.Stable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.runtime.staticCompositionLocalOf

/**
 * The colours of the page on screen when it paints itself in its own (a detail page in its
 * artwork's), so the chrome floating over it (the bottom bar and the scrim it sits on) follows
 * them rather than fading into the app's surface over, say, a cream page. Null: the app's colours.
 */
@Stable
class PageChrome {
    var scheme by mutableStateOf<ColorScheme?>(null)
        internal set
    internal var owner: Any? = null
}

val LocalPageChrome = staticCompositionLocalOf { PageChrome() }

/**
 * Hands [scheme] to the chrome while this page is composed. The page composed last wins (pushing a
 * page over another, or popping back to one, composes it afresh); leaving gives the app's colours
 * back unless another page has claimed the chrome meanwhile.
 */
@Composable
fun PageColors(scheme: ColorScheme) {
    val chrome = LocalPageChrome.current
    val key = remember { Any() }
    SideEffect {
        chrome.owner = key
        chrome.scheme = scheme
    }
    DisposableEffect(chrome, key) {
        onDispose { if (chrome.owner === key) { chrome.owner = null; chrome.scheme = null } }
    }
}
