package app.hocket.ui.nav

import androidx.compose.material3.ColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.SideEffect
import androidx.compose.runtime.Stable
import androidx.compose.runtime.mutableStateMapOf
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.lifecycle.viewmodel.compose.LocalViewModelStoreOwner
import androidx.navigation.NavBackStackEntry

/**
 * The colours of pages that paint themselves in their own (a detail page in its artwork's), by
 * back stack entry, so the chrome floating over the page on top (the bottom bar and the scrim it
 * sits on) follows it rather than fading into the app's surface over, say, a cream page. Keyed by
 * entry, not "last composed wins": the page beneath a predictive back gesture, or one still leaving
 * after a push or pop, never takes the bar from the page on top.
 */
@Stable
class PageChrome {
    private val schemes = mutableStateMapOf<String, ColorScheme>()
    private val statusLight = mutableStateMapOf<String, Boolean>()

    /** The colours for the entry [entryId] (the top of the back stack), or null for the app's. */
    fun schemeFor(entryId: String?): ColorScheme? = entryId?.let { schemes[it] }

    /** Whether the status bar's icons should be dark over the entry [entryId], or null for the app's own. */
    fun statusLightFor(entryId: String?): Boolean? = entryId?.let { statusLight[it] }

    internal fun put(entryId: String, scheme: ColorScheme, light: Boolean) {
        if (schemes[entryId] != scheme) schemes[entryId] = scheme
        if (statusLight[entryId] != light) statusLight[entryId] = light
    }
    internal fun remove(entryId: String) { schemes.remove(entryId); statusLight.remove(entryId) }

    /** Each composed page's push/pop progress ([PageTransitionLayer]: 0 at rest, 1 gone or covered). */
    private val progress = mutableStateMapOf<String, Float>()
    /** The moving top page and the page it covers, as of the last progress reported. */
    private var movingTop: String? = null
    private var movingCovered: String? = null

    internal fun reportProgress(entryId: String, value: Float, top: String?, covered: String?) {
        movingTop = top
        movingCovered = covered
        progress[entryId] = value
    }

    internal fun clearProgress(entryId: String) { progress.remove(entryId) }

    /**
     * The chrome's colours with [entryId] on top: while a push or a pop (a predictive back gesture
     * included) is moving, blended from the moving top page's colours ([fallback] for a page with
     * none) to the covered page's by its progress, so the bar crossfades with the pages.
     */
    fun chromeScheme(entryId: String?, fallback: ColorScheme): ColorScheme {
        val top = movingTop
        // The last push or pop's top page while it is still composed: on a pop it is the page
        // leaving (the back stack already names the one under it, from the first frame), at rest
        // after a push the page showing.
        val f = top?.let { progress[it] } ?: return schemeFor(entryId) ?: fallback
        val from = schemeFor(top) ?: fallback
        val covered = movingCovered
        if (f <= 0f || covered == null) return from
        return lerpScheme(from, schemeFor(covered) ?: fallback, f.coerceIn(0f, 1f))
    }
}

/** [a] to [b] by [t], role by role (the roles the bar, its indicator and its scrim read, and the rest of the surfaces). */
internal fun lerpScheme(a: ColorScheme, b: ColorScheme, t: Float): ColorScheme {
    fun l(x: androidx.compose.ui.graphics.Color, y: androidx.compose.ui.graphics.Color) = androidx.compose.ui.graphics.lerp(x, y, t)
    return b.copy(
        primary = l(a.primary, b.primary), onPrimary = l(a.onPrimary, b.onPrimary),
        primaryContainer = l(a.primaryContainer, b.primaryContainer), onPrimaryContainer = l(a.onPrimaryContainer, b.onPrimaryContainer),
        secondary = l(a.secondary, b.secondary), onSecondary = l(a.onSecondary, b.onSecondary),
        secondaryContainer = l(a.secondaryContainer, b.secondaryContainer), onSecondaryContainer = l(a.onSecondaryContainer, b.onSecondaryContainer),
        tertiary = l(a.tertiary, b.tertiary), onTertiary = l(a.onTertiary, b.onTertiary),
        background = l(a.background, b.background), onBackground = l(a.onBackground, b.onBackground),
        surface = l(a.surface, b.surface), onSurface = l(a.onSurface, b.onSurface),
        surfaceVariant = l(a.surfaceVariant, b.surfaceVariant), onSurfaceVariant = l(a.onSurfaceVariant, b.onSurfaceVariant),
        surfaceContainerLowest = l(a.surfaceContainerLowest, b.surfaceContainerLowest), surfaceContainerLow = l(a.surfaceContainerLow, b.surfaceContainerLow),
        surfaceContainer = l(a.surfaceContainer, b.surfaceContainer), surfaceContainerHigh = l(a.surfaceContainerHigh, b.surfaceContainerHigh),
        surfaceContainerHighest = l(a.surfaceContainerHighest, b.surfaceContainerHighest),
        outline = l(a.outline, b.outline), outlineVariant = l(a.outlineVariant, b.outlineVariant),
    )
}

val LocalPageChrome = staticCompositionLocalOf { PageChrome() }

/**
 * Hands [scheme] and the status bar's appearance ([statusLight]: dark icons) to the chrome for
 * this page's back stack entry while it is composed. Outside a NavHost destination it does nothing.
 */
@Composable
fun PageColors(scheme: ColorScheme, statusLight: Boolean) {
    val chrome = LocalPageChrome.current
    val entryId = (LocalViewModelStoreOwner.current as? NavBackStackEntry)?.id ?: return
    SideEffect { chrome.put(entryId, scheme, statusLight) }
    DisposableEffect(chrome, entryId) { onDispose { chrome.remove(entryId) } }
}

/**
 * The status bar's icons for the page on top ([entryId]): its own appearance while it has one,
 * the app theme's ([appDark]) once it has none, and left alone while [suspended] (the full player
 * sets its own). Decided by whichever page is on top, never saved and restored page by page, so
 * pages leaving in any order can't undo the one showing.
 */
@Composable
fun PageStatusBar(entryId: String?, appDark: Boolean, suspended: Boolean) {
    val light = LocalPageChrome.current.statusLightFor(entryId)
    val view = androidx.compose.ui.platform.LocalView.current
    // Only once a page has set it does going back to none need the theme's again.
    val touched = androidx.compose.runtime.remember { booleanArrayOf(false) }
    DisposableEffect(light, appDark, suspended, view) {
        if (!suspended && (light != null || touched[0])) {
            var context = view.context
            while (context !is android.app.Activity && context is android.content.ContextWrapper) context = context.baseContext
            (context as? android.app.Activity)?.window?.let { window ->
                androidx.core.view.WindowCompat.getInsetsController(window, view).isAppearanceLightStatusBars = light ?: !appDark
                touched[0] = light != null
            }
        }
        onDispose { }
    }
}
