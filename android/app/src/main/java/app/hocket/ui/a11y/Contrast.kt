package app.hocket.ui.a11y

import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.compositeOver
import androidx.compose.ui.graphics.luminance
import androidx.compose.ui.graphics.toArgb
import androidx.core.graphics.ColorUtils

/** WCAG 2.x contrast helpers for colours derived at run time (artwork schemes, lyric scrims). */
object Contrast {
    /** Body text. */
    const val TEXT = 4.5
    /** Large text (18 pt, or 14 pt bold) and non-text UI parts. */
    const val LARGE_TEXT = 3.0

    /** The WCAG contrast ratio of [fg] over [bg]; a translucent [fg] is composited over [bg] first. */
    fun ratio(fg: Color, bg: Color): Double {
        val opaqueBg = if (bg.alpha < 1f) bg.compositeOver(Color.Black) else bg
        val opaqueFg = if (fg.alpha < 1f) fg.compositeOver(opaqueBg) else fg
        val l1 = opaqueFg.luminance().toDouble()
        val l2 = opaqueBg.luminance().toDouble()
        return (maxOf(l1, l2) + 0.05) / (minOf(l1, l2) + 0.05)
    }

    /**
     * [fg] with its HSL lightness moved (darker when [darken], else lighter) just far enough to reach
     * [min] against every colour in [bgs]; unchanged when it already does. Falls back to black/white.
     */
    fun ensure(fg: Color, bgs: List<Color>, min: Double = TEXT, darken: Boolean): Color {
        if (bgs.all { ratio(fg, it) >= min }) return fg
        val hsl = FloatArray(3).also { ColorUtils.colorToHSL(fg.toArgb(), it) }
        var l = hsl[2]
        while (if (darken) l > 0f else l < 1f) {
            l = if (darken) (l - 0.01f).coerceAtLeast(0f) else (l + 0.01f).coerceAtMost(1f)
            val c = Color(ColorUtils.HSLToColor(floatArrayOf(hsl[0], hsl[1], l))).copy(alpha = fg.alpha)
            if (bgs.all { ratio(c, it) >= min }) return c
        }
        return if (darken) Color.Black else Color.White
    }
}
