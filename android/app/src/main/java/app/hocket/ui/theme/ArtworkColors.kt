package app.hocket.ui.theme

import android.graphics.BitmapFactory
import androidx.compose.material3.ColorScheme
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.toArgb
import androidx.core.graphics.ColorUtils
import androidx.palette.graphics.Palette
import app.hocket.ui.a11y.Contrast
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import kotlin.math.abs

/**
 * Seed-colour extraction and a compact tonal scheme builder, kept in the app (no Material Kolor):
 * the seed's hue drives primary, hue+40 secondary, hue+90 tertiary; tones follow the Material 3 role
 * table (40/80 primary, 90/30 containers, near-neutral surfaces tinted by the seed). HSL lightness is
 * not perceptual (a yellow at L 0.40 is far brighter than a blue), so every text/on-colour pair is
 * then nudged until it meets WCAG AA ([accessible]): 4.5:1 for text roles, 3:1 for the outline.
 */
object ArtworkColors {
    /** Reads a downsampled bitmap from a local path and returns a vibrant seed, or null. */
    suspend fun seedFrom(path: String?): Color? = withContext(Dispatchers.IO) {
        if (path.isNullOrEmpty()) return@withContext null
        val file = java.io.File(path.removePrefix("file://"))
        if (!file.exists()) return@withContext null
        val opts = BitmapFactory.Options().apply { inSampleSize = 4 }
        val bitmap = BitmapFactory.decodeFile(file.absolutePath, opts) ?: return@withContext null
        try {
            val palette = Palette.from(bitmap).maximumColorCount(16).generate()
            val swatch = palette.vibrantSwatch ?: palette.dominantSwatch ?: palette.mutedSwatch ?: return@withContext null
            Color(swatch.rgb)
        } finally {
            bitmap.recycle()
        }
    }

    /** Deterministic placeholder seed for an id, so items without artwork still get a stable hue. */
    fun seedFor(id: String?): Color {
        val h = abs((id ?: "").hashCode())
        val hue = (h % 360).toFloat()
        return Color(ColorUtils.HSLToColor(floatArrayOf(hue, 0.55f, 0.55f)))
    }

    fun scheme(seed: Color, dark: Boolean): ColorScheme = accessible(tonalScheme(seed, dark), dark)

    /**
     * Moves foreground roles (never surfaces) until each meets its contrast target against every
     * surface it is drawn on: primary/secondary/tertiary (text and icons on surfaces, and under their
     * on-colours), on-surface roles, on-container roles, the inverse pair and the outline.
     */
    fun accessible(s: ColorScheme, dark: Boolean): ColorScheme {
        val surfaces = listOf(s.background, s.surface, s.surfaceVariant, s.surfaceContainerLowest, s.surfaceContainerLow, s.surfaceContainer, s.surfaceContainerHigh, s.surfaceContainerHighest)
        // Light: foregrounds get darker; dark: lighter. On-colours of accents go the other way.
        val fgDarken = !dark
        val primary = Contrast.ensure(s.primary, surfaces + s.onPrimary, darken = fgDarken)
        val secondary = Contrast.ensure(s.secondary, surfaces + s.onSecondary, darken = fgDarken)
        val tertiary = Contrast.ensure(s.tertiary, surfaces + s.onTertiary, darken = fgDarken)
        // A container can be too bright (dark theme, yellow hues) for even white text: then the
        // container itself moves (darker in dark theme, lighter in light theme).
        fun pair(on: Color, container: Color): Pair<Color, Color> {
            val fg = Contrast.ensure(on, listOf(container), darken = fgDarken)
            if (Contrast.ratio(fg, container) >= Contrast.TEXT) return fg to container
            return fg to Contrast.ensure(container, listOf(fg), darken = !fgDarken)
        }
        val containers = listOf(pair(s.onPrimaryContainer, s.primaryContainer), pair(s.onSecondaryContainer, s.secondaryContainer), pair(s.onTertiaryContainer, s.tertiaryContainer))
        return s.copy(
            primary = primary,
            onPrimary = Contrast.ensure(s.onPrimary, listOf(primary), darken = !fgDarken),
            secondary = secondary,
            onSecondary = Contrast.ensure(s.onSecondary, listOf(secondary), darken = !fgDarken),
            tertiary = tertiary,
            onTertiary = Contrast.ensure(s.onTertiary, listOf(tertiary), darken = !fgDarken),
            primaryContainer = containers[0].second, onPrimaryContainer = containers[0].first,
            secondaryContainer = containers[1].second, onSecondaryContainer = containers[1].first,
            tertiaryContainer = containers[2].second, onTertiaryContainer = containers[2].first,
            onBackground = Contrast.ensure(s.onBackground, surfaces, darken = fgDarken),
            onSurface = Contrast.ensure(s.onSurface, surfaces, darken = fgDarken),
            onSurfaceVariant = Contrast.ensure(s.onSurfaceVariant, surfaces, darken = fgDarken),
            inverseOnSurface = Contrast.ensure(s.inverseOnSurface, listOf(s.inverseSurface), darken = !fgDarken),
            outline = Contrast.ensure(s.outline, surfaces, min = Contrast.LARGE_TEXT, darken = fgDarken),
        )
    }

    private fun tonalScheme(seed: Color, dark: Boolean): ColorScheme {
        val hsl = FloatArray(3).also { ColorUtils.colorToHSL(seed.toArgb(), it) }
        val hue = hsl[0]
        val sat = hsl[1].coerceIn(0.35f, 0.75f)
        fun tone(h: Float, s: Float, l: Float) = Color(ColorUtils.HSLToColor(floatArrayOf((h + 360f) % 360f, s, l)))
        val h2 = hue + 40f
        val h3 = hue + 90f
        return if (dark) darkColorScheme(
            primary = tone(hue, sat, 0.78f), onPrimary = tone(hue, sat, 0.18f),
            primaryContainer = tone(hue, sat, 0.30f), onPrimaryContainer = tone(hue, sat, 0.92f),
            secondary = tone(h2, sat * 0.6f, 0.75f), onSecondary = tone(h2, sat * 0.6f, 0.18f),
            secondaryContainer = tone(h2, sat * 0.6f, 0.28f), onSecondaryContainer = tone(h2, sat * 0.6f, 0.92f),
            tertiary = tone(h3, sat * 0.7f, 0.78f), onTertiary = tone(h3, sat * 0.7f, 0.18f),
            tertiaryContainer = tone(h3, sat * 0.7f, 0.30f), onTertiaryContainer = tone(h3, sat * 0.7f, 0.92f),
            background = tone(hue, 0.12f, 0.08f), onBackground = tone(hue, 0.08f, 0.92f),
            surface = tone(hue, 0.12f, 0.08f), onSurface = tone(hue, 0.08f, 0.92f),
            surfaceVariant = tone(hue, 0.12f, 0.20f), onSurfaceVariant = tone(hue, 0.08f, 0.78f),
            surfaceContainerLowest = tone(hue, 0.12f, 0.05f), surfaceContainerLow = tone(hue, 0.12f, 0.11f),
            surfaceContainer = tone(hue, 0.12f, 0.13f), surfaceContainerHigh = tone(hue, 0.12f, 0.16f), surfaceContainerHighest = tone(hue, 0.12f, 0.20f),
            outline = tone(hue, 0.08f, 0.55f), outlineVariant = tone(hue, 0.08f, 0.28f),
            inverseSurface = tone(hue, 0.08f, 0.90f), inverseOnSurface = tone(hue, 0.08f, 0.15f), inversePrimary = tone(hue, sat, 0.40f),
        ) else lightColorScheme(
            primary = tone(hue, sat, 0.40f), onPrimary = Color.White,
            primaryContainer = tone(hue, sat, 0.90f), onPrimaryContainer = tone(hue, sat, 0.12f),
            secondary = tone(h2, sat * 0.6f, 0.40f), onSecondary = Color.White,
            secondaryContainer = tone(h2, sat * 0.6f, 0.90f), onSecondaryContainer = tone(h2, sat * 0.6f, 0.12f),
            tertiary = tone(h3, sat * 0.7f, 0.38f), onTertiary = Color.White,
            tertiaryContainer = tone(h3, sat * 0.7f, 0.90f), onTertiaryContainer = tone(h3, sat * 0.7f, 0.12f),
            background = tone(hue, 0.25f, 0.985f), onBackground = tone(hue, 0.1f, 0.12f),
            surface = tone(hue, 0.25f, 0.985f), onSurface = tone(hue, 0.1f, 0.12f),
            surfaceVariant = tone(hue, 0.2f, 0.90f), onSurfaceVariant = tone(hue, 0.1f, 0.30f),
            surfaceContainerLowest = Color.White, surfaceContainerLow = tone(hue, 0.25f, 0.96f),
            surfaceContainer = tone(hue, 0.25f, 0.94f), surfaceContainerHigh = tone(hue, 0.25f, 0.92f), surfaceContainerHighest = tone(hue, 0.25f, 0.90f),
            outline = tone(hue, 0.1f, 0.50f), outlineVariant = tone(hue, 0.1f, 0.80f),
            inverseSurface = tone(hue, 0.1f, 0.18f), inverseOnSurface = tone(hue, 0.1f, 0.95f), inversePrimary = tone(hue, sat, 0.80f),
        )
    }
}
