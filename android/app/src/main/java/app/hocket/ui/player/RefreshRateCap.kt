package app.hocket.ui.player

import android.app.Activity
import android.content.Context
import android.content.ContextWrapper
import android.os.Build
import android.view.Display
import android.view.Window
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.ui.platform.LocalView
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import app.hocket.core.api.SaverFeature
import app.hocket.core.has
import app.hocket.ui.LocalCoreClient
import kotlin.math.abs

/**
 * Battery saver's 60 Hz cap ([SaverFeature.RefreshCap]): while [active] (the full player or lyrics
 * open and settled) and the feature is on, the activity's window asks for a ~60 Hz display mode of
 * the current resolution; the window's own preference comes back once nothing asks any more. A
 * screen already at 60 Hz or below is left alone. Several callers (the player, the lyrics panel)
 * may hold it at once: it is counted per window.
 */
@Composable
fun RefreshRateCap(active: Boolean) {
    val saver = LocalCoreClient.current.batterySaver.collectAsStateWithLifecycle().value
    val on = active && saver.has(SaverFeature.RefreshCap)
    val view = LocalView.current
    DisposableEffect(on, view) {
        val window = if (on) view.context.findActivity()?.window else null
        if (window == null) return@DisposableEffect onDispose { }
        @Suppress("DEPRECATION")
        val display = (if (Build.VERSION.SDK_INT >= 30) view.display else window.windowManager.defaultDisplay)
        RefreshCaps.acquire(window, display)
        onDispose { RefreshCaps.release(window) }
    }
}

/** The windows capped now, each with how many callers hold the cap and what it asked for before. */
internal object RefreshCaps {
    private class Held(var count: Int, val modeId: Int, val rate: Float)

    private val held = HashMap<Window, Held>()

    fun acquire(window: Window, display: Display?) {
        held[window]?.let { it.count++; return }
        val attrs = window.attributes
        held[window] = Held(1, attrs.preferredDisplayModeId, attrs.preferredRefreshRate)
        val mode = display?.let { sixtyHz(it) } ?: return
        window.attributes = attrs.also {
            it.preferredDisplayModeId = mode.modeId
            it.preferredRefreshRate = mode.refreshRate
        }
    }

    fun release(window: Window) {
        val h = held[window] ?: return
        if (--h.count > 0) return
        held.remove(window)
        window.attributes = window.attributes.also {
            it.preferredDisplayModeId = h.modeId
            it.preferredRefreshRate = h.rate
        }
    }

    /** The current resolution's mode nearest 60 Hz (at least ~48), or null when the screen already runs at 60 or below. */
    private fun sixtyHz(display: Display): Display.Mode? {
        val current = display.mode
        if (current.refreshRate <= CAP_HZ + 1f) return null
        return display.supportedModes
            .filter { it.physicalWidth == current.physicalWidth && it.physicalHeight == current.physicalHeight && it.refreshRate in 47f..(CAP_HZ + 1f) }
            .minByOrNull { abs(it.refreshRate - CAP_HZ) }
    }

    private const val CAP_HZ = 60f
}

private tailrec fun Context.findActivity(): Activity? = when (this) {
    is Activity -> this
    is ContextWrapper -> baseContext.findActivity()
    else -> null
}
