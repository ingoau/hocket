package app.hocket.ui.theme

import android.os.Build
import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.material3.ColorScheme
import androidx.compose.material3.MaterialExpressiveTheme
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.MotionScheme
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.dynamicDarkColorScheme
import androidx.compose.material3.dynamicLightColorScheme
import androidx.compose.material3.expressiveLightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.compositionLocalOf
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import app.hocket.core.client.CoreClient

/** The accent colour taken from the playing artwork, provided by the now-playing sheet while it is open. */
val LocalArtworkSeed = compositionLocalOf<Color?> { null }

/** Whether the app is in dark mode after applying the theme setting. */
val LocalDarkTheme = compositionLocalOf { false }

/** A setter the now-playing sheet uses to publish the artwork seed to the whole tree. */
class ArtworkSeedState { var seed: Color? by mutableStateOf(null) }
val LocalArtworkSeedState = compositionLocalOf { ArtworkSeedState() }

/**
 * Material 3 Expressive theme: expressive colour scheme (dynamic on Android 12+ unless the accent
 * setting overrides it), expressive motion scheme, and the system font on an expressive type scale.
 * Settings are read straight from the client's settings map (`ui.theme`, `ui.accent`).
 */
@Composable
fun HocketTheme(client: CoreClient?, content: @Composable () -> Unit) {
    val settings = client?.settings?.collectAsStateWithLifecycle()?.value ?: emptyMap()
    val themeSetting = settings["ui.theme"]?.value?.trim('"') ?: "system"
    val accentSetting = settings["ui.accent"]?.value?.trim('"') ?: "dynamic"
    val dark = when (themeSetting) { "light" -> false; "dark" -> true; else -> isSystemInDarkTheme() }
    val context = LocalContext.current
    val seedState = remember { ArtworkSeedState() }
    val artworkSeed = seedState.seed
    val scheme: ColorScheme = remember(dark, accentSetting, artworkSeed) {
        when {
            artworkSeed != null && accentSetting != "static" -> ArtworkColors.scheme(artworkSeed, dark)
            accentSetting.startsWith("#") -> runCatching { Color(android.graphics.Color.parseColor(accentSetting)) }.getOrNull()?.let { ArtworkColors.scheme(it, dark) }
                ?: baseScheme(dark, context)
            else -> baseScheme(dark, context)
        }
    }
    CompositionLocalProvider(LocalDarkTheme provides dark, LocalArtworkSeedState provides seedState, LocalArtworkSeed provides artworkSeed) {
        MaterialExpressiveTheme(
            colorScheme = scheme,
            motionScheme = MotionScheme.expressive(),
            typography = expressiveTypography(MaterialTheme.typography),
            content = content,
        )
    }
}

private fun baseScheme(dark: Boolean, context: android.content.Context): ColorScheme =
    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
        if (dark) dynamicDarkColorScheme(context) else dynamicLightColorScheme(context)
    } else {
        if (dark) darkColorScheme() else expressiveLightColorScheme()
    }
