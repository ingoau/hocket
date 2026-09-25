package app.hocket.ui.theme

import android.os.Build
import androidx.compose.animation.animateColorAsState
import androidx.compose.animation.core.Spring
import androidx.compose.animation.core.spring
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
import app.hocket.core.SettingKeys
import app.hocket.core.client.CoreClient
import app.hocket.ui.a11y.LocalReducedMotion
import app.hocket.ui.a11y.rememberReducedMotion

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
    // Registry keys: display.theme (system|light|dark), display.accent (null or #RRGGBB), display.dynamicColour (bool).
    val themeSetting = settings[SettingKeys.DISPLAY_THEME]?.value?.trim()?.trim('"') ?: "system"
    val accentSetting = settings[SettingKeys.DISPLAY_ACCENT]?.value?.trim()?.trim('"')?.takeIf { it.isNotEmpty() && it != "null" }
    val dynamicColour = settings[SettingKeys.DISPLAY_DYNAMIC_COLOUR]?.value?.trim() != "false"
    val dark = when (themeSetting) { "light" -> false; "dark" -> true; else -> isSystemInDarkTheme() }
    val context = LocalContext.current
    val seedState = remember { ArtworkSeedState() }
    val artworkSeed = seedState.seed
    val scheme: ColorScheme = remember(dark, accentSetting, dynamicColour, artworkSeed) {
        when {
            artworkSeed != null && dynamicColour -> ArtworkColors.scheme(artworkSeed, dark)
            accentSetting != null -> runCatching { Color(android.graphics.Color.parseColor(if (accentSetting.startsWith("#")) accentSetting else "#$accentSetting")) }.getOrNull()?.let { ArtworkColors.scheme(it, dark) }
                ?: baseScheme(dark, context, dynamicColour)
            else -> baseScheme(dark, context, dynamicColour)
        }
    }
    val reducedMotion = rememberReducedMotion()
    CompositionLocalProvider(LocalDarkTheme provides dark, LocalArtworkSeedState provides seedState, LocalArtworkSeed provides artworkSeed, LocalReducedMotion provides reducedMotion) {
        MaterialExpressiveTheme(
            colorScheme = animatedScheme(scheme, reducedMotion),
            motionScheme = MotionScheme.expressive(),
            typography = expressiveTypography(MaterialTheme.typography),
            content = content,
        )
    }
}

/**
 * [target] with every role eased toward it, so an accent change, a light/dark switch or the
 * artwork seed arriving crossfades over ~400 ms instead of snapping the whole app to new colours.
 * A critically damped spring (no bounce) so colours never overshoot. With reduced motion the
 * scheme switches at once.
 */
@Composable
private fun animatedScheme(target: ColorScheme, reducedMotion: Boolean): ColorScheme {
    if (reducedMotion) return target
    @Composable fun Color.animated(): Color = animateColorAsState(this, ColorSpring, label = "scheme").value
    return target.copy(
        primary = target.primary.animated(),
        onPrimary = target.onPrimary.animated(),
        primaryContainer = target.primaryContainer.animated(),
        onPrimaryContainer = target.onPrimaryContainer.animated(),
        inversePrimary = target.inversePrimary.animated(),
        secondary = target.secondary.animated(),
        onSecondary = target.onSecondary.animated(),
        secondaryContainer = target.secondaryContainer.animated(),
        onSecondaryContainer = target.onSecondaryContainer.animated(),
        tertiary = target.tertiary.animated(),
        onTertiary = target.onTertiary.animated(),
        tertiaryContainer = target.tertiaryContainer.animated(),
        onTertiaryContainer = target.onTertiaryContainer.animated(),
        background = target.background.animated(),
        onBackground = target.onBackground.animated(),
        surface = target.surface.animated(),
        onSurface = target.onSurface.animated(),
        surfaceVariant = target.surfaceVariant.animated(),
        onSurfaceVariant = target.onSurfaceVariant.animated(),
        surfaceTint = target.surfaceTint.animated(),
        inverseSurface = target.inverseSurface.animated(),
        inverseOnSurface = target.inverseOnSurface.animated(),
        error = target.error.animated(),
        onError = target.onError.animated(),
        errorContainer = target.errorContainer.animated(),
        onErrorContainer = target.onErrorContainer.animated(),
        outline = target.outline.animated(),
        outlineVariant = target.outlineVariant.animated(),
        scrim = target.scrim.animated(),
        surfaceBright = target.surfaceBright.animated(),
        surfaceDim = target.surfaceDim.animated(),
        surfaceContainer = target.surfaceContainer.animated(),
        surfaceContainerHigh = target.surfaceContainerHigh.animated(),
        surfaceContainerHighest = target.surfaceContainerHighest.animated(),
        surfaceContainerLow = target.surfaceContainerLow.animated(),
        surfaceContainerLowest = target.surfaceContainerLowest.animated(),
        primaryFixed = target.primaryFixed.animated(),
        primaryFixedDim = target.primaryFixedDim.animated(),
        onPrimaryFixed = target.onPrimaryFixed.animated(),
        onPrimaryFixedVariant = target.onPrimaryFixedVariant.animated(),
        secondaryFixed = target.secondaryFixed.animated(),
        secondaryFixedDim = target.secondaryFixedDim.animated(),
        onSecondaryFixed = target.onSecondaryFixed.animated(),
        onSecondaryFixedVariant = target.onSecondaryFixedVariant.animated(),
        tertiaryFixed = target.tertiaryFixed.animated(),
        tertiaryFixedDim = target.tertiaryFixedDim.animated(),
        onTertiaryFixed = target.onTertiaryFixed.animated(),
        onTertiaryFixedVariant = target.onTertiaryFixedVariant.animated(),
    )
}

/** No bounce; settles in roughly 400 ms. */
private val ColorSpring = spring<Color>(dampingRatio = Spring.DampingRatioNoBouncy, stiffness = 200f)

private fun baseScheme(dark: Boolean, context: android.content.Context, dynamic: Boolean): ColorScheme =
    if (dynamic && Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
        if (dark) dynamicDarkColorScheme(context) else dynamicLightColorScheme(context)
    } else {
        if (dark) darkColorScheme() else expressiveLightColorScheme()
    }
