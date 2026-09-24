package app.hocket.ui.a11y

import androidx.compose.material3.ColorScheme
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.compositeOver
import androidx.core.graphics.ColorUtils
import androidx.test.core.app.ApplicationProvider
import androidx.test.ext.junit.runners.AndroidJUnit4
import app.hocket.ui.lyrics.LyricsContrast
import app.hocket.ui.theme.ArtworkColors
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.annotation.Config

/**
 * Text over artwork-derived colour: every text/on-colour pair of the artwork schemes (light and
 * dark, any hue and saturation) and every lyric text colour over the fluid background (any
 * artwork, however bright) meets WCAG AA.
 */
@RunWith(AndroidJUnit4::class)
@Config(sdk = [35], application = android.app.Application::class)
class ContrastTest {
    private fun seeds(): List<Color> = buildList {
        for (h in 0 until 360 step 9) for (s in listOf(0.05f, 0.4f, 0.8f, 1f)) for (l in listOf(0.15f, 0.5f, 0.85f)) add(Color(ColorUtils.HSLToColor(floatArrayOf(h.toFloat(), s, l))))
        add(Color.White); add(Color.Black); add(Color(0xFFFFEB3B)); add(Color(0xFF00E5FF))
    }

    /** Text roles over the surfaces they are drawn on. */
    private fun pairs(c: ColorScheme): List<Triple<String, Pair<Color, Color>, Double>> {
        val surfaces = listOf("background" to c.background, "surface" to c.surface, "surfaceVariant" to c.surfaceVariant, "surfaceContainerLowest" to c.surfaceContainerLowest,
            "surfaceContainerLow" to c.surfaceContainerLow, "surfaceContainer" to c.surfaceContainer, "surfaceContainerHigh" to c.surfaceContainerHigh, "surfaceContainerHighest" to c.surfaceContainerHighest)
        return buildList {
            for ((name, bg) in surfaces) {
                for ((fgName, fg) in listOf("onSurface" to c.onSurface, "onSurfaceVariant" to c.onSurfaceVariant, "onBackground" to c.onBackground, "primary" to c.primary, "secondary" to c.secondary, "tertiary" to c.tertiary))
                    add(Triple("$fgName on $name", fg to bg, Contrast.TEXT))
                add(Triple("outline on $name", c.outline to bg, Contrast.LARGE_TEXT))
            }
            add(Triple("onPrimary on primary", c.onPrimary to c.primary, Contrast.TEXT))
            add(Triple("onSecondary on secondary", c.onSecondary to c.secondary, Contrast.TEXT))
            add(Triple("onTertiary on tertiary", c.onTertiary to c.tertiary, Contrast.TEXT))
            add(Triple("onPrimaryContainer", c.onPrimaryContainer to c.primaryContainer, Contrast.TEXT))
            add(Triple("onSecondaryContainer", c.onSecondaryContainer to c.secondaryContainer, Contrast.TEXT))
            add(Triple("onTertiaryContainer", c.onTertiaryContainer to c.tertiaryContainer, Contrast.TEXT))
            add(Triple("inverseOnSurface", c.inverseOnSurface to c.inverseSurface, Contrast.TEXT))
        }
    }

    @Test
    fun artworkSchemesMeetAaInLightAndDark() {
        val failures = mutableListOf<String>()
        for (seed in seeds()) for (dark in listOf(false, true)) {
            for ((name, fgBg, min) in pairs(ArtworkColors.scheme(seed, dark))) {
                val r = Contrast.ratio(fgBg.first, fgBg.second)
                if (r < min) failures += "seed=$seed dark=$dark $name: ${"%.2f".format(r)} < $min"
            }
        }
        assertTrue(failures.take(20).joinToString("\n"), failures.isEmpty())
    }

    @Test
    fun theUnadjustedToneTableFailedForBrightHues() {
        // The reason for the adjustment: HSL L 0.40 yellow under white text is ~2:1.
        val yellow = Color(ColorUtils.HSLToColor(floatArrayOf(60f, 0.75f, 0.40f)))
        assertTrue(Contrast.ratio(Color.White, yellow) < 3.0)
        val fixed = ArtworkColors.scheme(Color(0xFFFFEB3B), dark = false)
        assertTrue(Contrast.ratio(fixed.onPrimary, fixed.primary) >= Contrast.TEXT)
    }

    @Test
    fun lyricTextMeetsAaOverAnyArtwork() {
        for (brightest in seeds()) {
            val scrim = LyricsContrast.scrimAlpha(brightest)
            val bg = Color.Black.copy(alpha = scrim).compositeOverOpaque(brightest)
            assertTrue("lit over $brightest", Contrast.ratio(LyricsContrast.active, bg) >= Contrast.TEXT)
            assertTrue("secondary over $brightest", Contrast.ratio(LyricsContrast.secondary, bg) >= Contrast.TEXT)
            assertTrue("inactive (large) over $brightest", Contrast.ratio(LyricsContrast.inactive, bg) >= Contrast.LARGE_TEXT)
            assertTrue(scrim in LyricsContrast.MIN_SCRIM..1f)
        }
        // Dark artwork keeps the light scrim; white artwork needs a heavy one, never a black wall.
        assertEquals(LyricsContrast.MIN_SCRIM, LyricsContrast.scrimAlpha(Color(0xFF101018)), 0.001f)
        assertTrue(LyricsContrast.scrimAlpha(Color.White) in 0.6f..0.9f)
        // The old fixed 35% scrim failed over bright artwork.
        assertTrue(Contrast.ratio(Color.White, Color.Black.copy(alpha = 0.35f).compositeOverOpaque(Color.White)) < Contrast.TEXT)
    }

    @Test
    fun spokenDurationsAreWords() {
        val r = ApplicationProvider.getApplicationContext<android.content.Context>().resources
        assertEquals("1 minute 32 seconds of 3 minutes 32 seconds", Spoken.position(r, 92_000, 212_000))
        assertEquals("0 seconds", Spoken.duration(r, 0))
        assertEquals("1 hour 1 second", Spoken.duration(r, 3_601_000))
        assertEquals("3 of 5 stars", Spoken.rating(r, 3))
        assertEquals("1 of 5 stars", Spoken.rating(r, 1))
        assertEquals("Not rated", Spoken.rating(r, 0))
    }

    private fun Color.compositeOverOpaque(bg: Color): Color = compositeOver(bg)
}
