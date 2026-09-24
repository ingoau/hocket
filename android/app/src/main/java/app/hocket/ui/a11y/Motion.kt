package app.hocket.ui.a11y

import android.content.Context
import android.database.ContentObserver
import android.os.Handler
import android.os.Looper
import android.provider.Settings
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.compositionLocalOf
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.platform.LocalContext

/**
 * True when the user asked for less motion: the system animator duration scale is 0 (Developer
 * options, or Accessibility > "Remove animations", which sets it). Compose's own animations already
 * honour the scale; this local is for the motion the app drives itself frame by frame: the lyric
 * syllable sweep (becomes a static highlight), the depth-of-field blur and line scaling, the lyric
 * auto-scroll animation, the AGSL fluid background and the wavy progress line (all stop).
 * Provided by `HocketTheme`; tests override it.
 */
val LocalReducedMotion = compositionLocalOf { false }

/** Reads the animator duration scale (0 = animations removed). */
fun animatorDurationScale(context: Context): Float =
    try {
        Settings.Global.getFloat(context.contentResolver, Settings.Global.ANIMATOR_DURATION_SCALE, 1f)
    } catch (e: Exception) {
        1f
    }

/** The reduced-motion preference, following changes to the system setting while composed. */
@Composable
fun rememberReducedMotion(): Boolean {
    val context = LocalContext.current
    var reduced by remember { mutableStateOf(animatorDurationScale(context) == 0f) }
    DisposableEffect(context) {
        val observer = object : ContentObserver(Handler(Looper.getMainLooper())) {
            override fun onChange(selfChange: Boolean) {
                reduced = animatorDurationScale(context) == 0f
            }
        }
        val registered = runCatching {
            context.contentResolver.registerContentObserver(Settings.Global.getUriFor(Settings.Global.ANIMATOR_DURATION_SCALE), false, observer)
        }.isSuccess
        onDispose { if (registered) context.contentResolver.unregisterContentObserver(observer) }
    }
    return reduced
}
