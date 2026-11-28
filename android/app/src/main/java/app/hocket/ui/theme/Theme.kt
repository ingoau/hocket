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
import androidx.compose.runtime.LaunchedEffect
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
import app.hocket.ui.player.resolveNowPlayingSeed
import kotlinx.coroutines.flow.distinctUntilChanged
import kotlinx.coroutines.flow.map

/** The accent colour the app's scheme is built from when it comes from artwork (what's playing, or a preview), else null. */
val LocalArtworkSeed = compositionLocalOf<Color?> { null }

/** Whether the app is in dark mode after applying the theme setting. */
val LocalDarkTheme = compositionLocalOf { false }

/** The playing track's cover and its seed colour, as the theme works them out; null above the theme or with no client. */
class NowPlayingSeed(val coverArt: String?, val seed: Color?)
val LocalNowPlayingSeed = compositionLocalOf<NowPlayingSeed?> { null }

/** A setter the now-playing sheet uses to preview a cover's colours across the whole tree (with colour from artwork off). */
class ArtworkSeedState { var seed: Color? by mutableStateOf(null) }
val LocalArtworkSeedState = compositionLocalOf { ArtworkSeedState() }

/**
 * Material 3 Expressive theme: expressive colour scheme, expressive motion scheme, and the system
 * font on an expressive type scale. The colours come from, in order: the playing artwork (with
 * `display.dynamicColour` on; while a new cover's seed is worked out the last one holds), a cover
 * the now-playing sheet previews (with it off), a picked accent (`display.accent`), then the
 * wallpaper on Android 12+. `display.dynamicColour` only gates the artwork: turning it off falls
 * back to the accent or the wallpaper, not the stock scheme.
 */
@Composable
fun HocketTheme(client: CoreClient?, content: @Composable () -> Unit) {
    val settings = client?.settings?.collectAsStateWithLifecycle()?.value ?: emptyMap()
    // Registry keys: display.theme (system|light|dark), display.accent (null or #RRGGBB), display.dynamicColour (bool: colour from artwork).
    val themeSetting = settings[SettingKeys.DISPLAY_THEME]?.value?.trim()?.trim('"') ?: "system"
    val accentSetting = settings[SettingKeys.DISPLAY_ACCENT]?.value?.trim()?.trim('"')?.takeIf { it.isNotEmpty() && it != "null" }
    val dynamicColour = settings[SettingKeys.DISPLAY_DYNAMIC_COLOUR]?.value?.trim() != "false"
    val dark = when (themeSetting) { "light" -> false; "dark" -> true; else -> isSystemInDarkTheme() }
    val context = LocalContext.current
    val seedState = remember { ArtworkSeedState() }
    // Only the cover: the entry changes far more often (a star, a rating, the queue), and each
    // recomposition here hands the whole tree a new scheme.
    val playing = client?.let { c ->
        val cover by remember(c) { c.nowPlaying.map { it?.track?.coverArt }.distinctUntilChanged() }.collectAsStateWithLifecycle(c.nowPlaying.value?.track?.coverArt)
        val seed = resolveNowPlayingSeed(cover, c)
        remember(cover, seed) { NowPlayingSeed(cover, seed) }
    }
    // A preview only means something with colour from artwork off; turning it on drops one.
    LaunchedEffect(dynamicColour) { if (dynamicColour) seedState.seed = null }
    val artworkSeed = if (dynamicColour) playing?.seed else seedState.seed
    val scheme: ColorScheme = remember(dark, accentSetting, artworkSeed) {
        when {
            artworkSeed != null -> ArtworkColors.scheme(artworkSeed, dark)
            accentSetting != null -> runCatching { Color(android.graphics.Color.parseColor(if (accentSetting.startsWith("#")) accentSetting else "#$accentSetting")) }.getOrNull()?.let { ArtworkColors.scheme(it, dark) }
                ?: baseScheme(dark, context)
            else -> baseScheme(dark, context)
        }
    }
    val reducedMotion = rememberReducedMotion()
    CompositionLocalProvider(LocalDarkTheme provides dark, LocalArtworkSeedState provides seedState, LocalArtworkSeed provides artworkSeed, LocalNowPlayingSeed provides playing, LocalReducedMotion provides reducedMotion) {
        MaterialExpressiveTheme(
            colorScheme = animatedScheme(scheme, reducedMotion),
            motionScheme = MotionScheme.expressive(),
            typography = expressiveTypography(MaterialTheme.typography),
        ) {
            // Text outside any Surface (most pages sit straight on the window) in the scheme's
            // on-background, not the platform's default black.
            CompositionLocalProvider(androidx.compose.material3.LocalContentColor provides MaterialTheme.colorScheme.onBackground, content = content)
        }
    }
}

/**
 * [target] with every role eased toward it, so an accent change, a light/dark switch or the
 * artwork seed arriving crossfades over ~400 ms instead of snapping the whole app to new colours.
 * A critically damped spring (no bounce) so colours never overshoot. With reduced motion the
 * scheme switches at once.
 */
@Composable
internal fun animatedScheme(target: ColorScheme, reducedMotion: Boolean): ColorScheme {
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

/** The wallpaper's (Material You) scheme on Android 12+, otherwise the stock expressive scheme. */
private fun baseScheme(dark: Boolean, context: android.content.Context): ColorScheme =
    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
        if (dark) dynamicDarkColorScheme(context) else dynamicLightColorScheme(context)
    } else {
        if (dark) darkColorScheme() else expressiveLightColorScheme()
    }
