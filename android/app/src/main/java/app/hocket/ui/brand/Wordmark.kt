package app.hocket.ui.brand

import androidx.compose.foundation.Canvas
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.graphics.drawscope.scale
import androidx.compose.ui.graphics.drawscope.translate
import androidx.compose.ui.graphics.vector.PathParser
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.Font
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontVariation
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.LineHeightStyle
import androidx.compose.material3.LocalTextStyle
import androidx.compose.material3.MaterialTheme
import androidx.compose.ui.graphics.luminance
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.em
import app.hocket.R

/**
 * The brand (docs/brand.md): the Crescendo mark and the word mark, in one colour.
 *
 * The word mark is Unbounded at weight 500, always lowercase. `res/font/unbounded.ttf` is the whole
 * variable font from google/fonts (SIL OFL 1.1, the licence in `assets/licenses/unbounded-OFL.txt`);
 * [unbounded] maps each weight to its point on the font's weight axis.
 */
object Brand {
    /** The word mark's ink on dark grounds. */
    val inkOnDark = Color(0xFFECE9FF)

    /** The word mark's ink on light grounds (the tile gradient's bottom stop). */
    val inkOnLight = Color(0xFF1B1730)

    /** The two voices: notes 1 and 3, then 2 and 4. */
    internal val voice1 = Color(0xFFE4DFFF)
    internal val voice2 = Color(0xFF8F80F5)

    /** Unbounded at every weight its axis has (200–900). */
    val unbounded = FontFamily(
        listOf(FontWeight.ExtraLight, FontWeight.Light, FontWeight.Normal, FontWeight.Medium, FontWeight.SemiBold, FontWeight.Bold, FontWeight.ExtraBold, FontWeight.Black).map { w ->
            Font(R.font.unbounded, w, variationSettings = FontVariation.Settings(FontVariation.weight(w.weight)))
        },
    )

    /** The four notes on the mark's 100-unit tile (docs/brand.md, the Crescendo mark). */
    internal val notes = listOf(
        "M16 48.5 C23 37.81 26 37.81 33 48.5",
        "M33 48.5 C40 63.2 43 63.2 50 48.5",
        "M50 48.5 C57 29.8 60 29.8 67 48.5",
        "M67 48.5 C74 71.21 77 71.21 84 48.5",
    )

    /** The wave's extent on the tile, round caps included: x 10–90, y 28.4–71.6. */
    internal const val WAVE_LEFT = 10f
    internal const val WAVE_TOP = 28.4f
    internal const val WAVE_WIDTH = 80f
    internal const val WAVE_HEIGHT = 43.2f
    internal const val STROKE = 12f
}

/**
 * The bare Crescendo wave in one [color] (no tile), [height] tall from cap to cap. Only on dark
 * grounds: the brand keeps the bare wave off light ones.
 */
@Composable
fun CrescendoMark(height: Dp, color: Color, modifier: Modifier = Modifier) {
    val paths = remember { Brand.notes.map { PathParser().parsePathString(it).toPath() } }
    Canvas(modifier.size(width = height * (Brand.WAVE_WIDTH / Brand.WAVE_HEIGHT), height = height)) {
        val k = size.height / Brand.WAVE_HEIGHT
        scale(k, k, pivot = Offset.Zero) {
            translate(-Brand.WAVE_LEFT, -Brand.WAVE_TOP) {
                for (p in paths) drawPath(p, color, style = Stroke(width = Brand.STROKE, cap = StrokeCap.Round))
            }
        }
    }
}

/**
 * The lockup in one [color]: the bare wave, then "hocket" in the word mark's type at [fontSize]
 * (as a Dp, so it ignores the font scale, like the share card around it). The wave is as tall as
 * the word's x-height and a little more, the gap 0.3 of the type size; tracking −0.03em (display
 * sizes: the exported card sets it well above 24 px).
 */
@Composable
fun Lockup(fontSize: Dp, color: Color = Brand.inkOnDark, modifier: Modifier = Modifier) {
    val sp = with(LocalDensity.current) { fontSize.toSp() }
    Row(modifier, verticalAlignment = Alignment.CenterVertically) {
        CrescendoMark(height = fontSize * 0.8f, color = color)
        Spacer(Modifier.width(fontSize * 0.3f))
        Text(
            "hocket",
            color = color,
            style = TextStyle(
                fontFamily = Brand.unbounded,
                fontWeight = FontWeight.Medium,
                fontSize = sp,
                letterSpacing = (-0.03).em,
                lineHeight = sp,
                lineHeightStyle = LineHeightStyle(LineHeightStyle.Alignment.Center, LineHeightStyle.Trim.Both),
            ),
        )
    }
}

/** The word mark's ink for the current theme's ground. */
@Composable
fun brandInk(): Color = if (MaterialTheme.colorScheme.surface.luminance() < 0.5f) Brand.inkOnDark else Brand.inkOnLight

/**
 * A page header set in the word mark's type: Unbounded 500 at the size of the surrounding title
 * style (so a collapsing top bar resizes it), tracked −0.015em.
 */
@Composable
fun brandTitleStyle(): TextStyle = LocalTextStyle.current.merge(
    TextStyle(fontFamily = Brand.unbounded, fontWeight = FontWeight.Medium, letterSpacing = (-0.015).em),
)

/**
 * The app's name as a page header (Home): the bare Crescendo wave, then "hocket" in the word mark,
 * at the size of the surrounding title style (the wave as tall as the type's x-height and a little
 * more, the gap 0.3 of the type size). On dark grounds the wave keeps its two voices; on light ones,
 * where the pale voice would vanish, it's one colour, the word mark's ink. It reads as one heading.
 */
@Composable
fun HeaderLockup(modifier: Modifier = Modifier) {
    val style = brandTitleStyle()
    val name = stringResource(R.string.app_name)
    val type = with(LocalDensity.current) { style.fontSize.toDp() }
    val ink = brandInk()
    val dark = ink == Brand.inkOnDark
    Row(modifier.semantics(mergeDescendants = true) { contentDescription = name; heading() }, verticalAlignment = Alignment.CenterVertically) {
        VoicedMark(height = type * 0.8f, voices = if (dark) listOf(Brand.voice1, Brand.voice2) else listOf(ink, ink))
        Spacer(Modifier.width(type * 0.3f))
        Text(
            "hocket",
            color = ink,
            maxLines = 1,
            style = style.copy(lineHeightStyle = LineHeightStyle(LineHeightStyle.Alignment.Center, LineHeightStyle.Trim.Both)),
            modifier = Modifier.clearAndSetSemantics {},
        )
    }
}

/** The bare wave [height] tall, notes 1 and 3 in the first of [voices], 2 and 4 in the second. */
@Composable
private fun VoicedMark(height: Dp, voices: List<Color>, modifier: Modifier = Modifier) {
    val paths = remember { Brand.notes.map { PathParser().parsePathString(it).toPath() } }
    Canvas(modifier.size(width = height * (Brand.WAVE_WIDTH / Brand.WAVE_HEIGHT), height = height)) {
        val k = size.height / Brand.WAVE_HEIGHT
        scale(k, k, pivot = Offset.Zero) {
            translate(-Brand.WAVE_LEFT, -Brand.WAVE_TOP) {
                paths.forEachIndexed { i, p -> drawPath(p, voices[i % 2], style = Stroke(width = Brand.STROKE, cap = StrokeCap.Round)) }
            }
        }
    }
}
