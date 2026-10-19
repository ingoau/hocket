package app.hocket.ui.lyrics

import androidx.compose.animation.core.CubicBezierEasing
import androidx.compose.foundation.layout.Spacer
import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.drawBehind
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Shadow
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.graphics.drawscope.DrawScope
import androidx.compose.ui.graphics.drawscope.clipRect
import androidx.compose.ui.graphics.drawscope.translate
import androidx.compose.ui.graphics.drawscope.withTransform
import androidx.compose.ui.graphics.lerp
import androidx.compose.ui.layout.layout
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.TextLayoutResult
import androidx.compose.ui.text.TextMeasurer
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.drawText
import androidx.compose.ui.text.rememberTextMeasurer
import androidx.compose.ui.text.style.ResolvedTextDirection
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.Constraints
import app.hocket.core.api.LyricLine
import java.text.BreakIterator
import kotlin.math.PI
import kotlin.math.max
import kotlin.math.min
import kotlin.math.sin
import kotlin.math.sqrt

/**
 * How bright a line's text is (white at these alphas): [idle] on a line not being sung; on the lit
 * line, [unsung] before a syllable is sung and [sung] after. The idle and unsung alphas keep
 * [LyricsContrast]'s floors.
 */
internal class LyricTone(val idle: Color, val unsung: Color, val sung: Color) {
    fun sung(bright: Float) = lerp(idle, sung, bright)
    fun unsung(bright: Float) = lerp(idle, unsung, bright)

    companion object {
        val Main = LyricTone(LyricsContrast.inactive, LyricsContrast.unsung, LyricsContrast.active)
        /** Background vocals are smaller (not "large text"), so they keep the 4.5:1 secondary colour. */
        val Background = LyricTone(LyricsContrast.secondary, LyricsContrast.secondary, LyricsContrast.active)
    }
}

/**
 * A syllable-timed line, drawn the Apple Music way (AMLL's, as huddlefm's native renderer draws it):
 * laid out once as one paragraph (shaping and wrapping as the text's own), then each word drawn
 * clipped to its own box so it can move on its own. The syllable being sung carries a soft leading
 * edge [LyricsMotion.FADE_WIDTH_EM] wide; each word rises [LyricsMotion.LIFT_EM] as it is sung; a
 * short word held a second or more swells, spreads and glows letter by letter.
 *
 * [sweep] is the per-syllable progress (as published in [LyricsSemantics.Sweep]); [timeMs] the
 * effective position, for the rise and the swell; [bright] how lit the line is (0..1), read while
 * drawing only. Without [motion] nothing rises or swells: the sweep alone (static with reduced
 * motion) shows what is being sung.
 */
@Composable
internal fun SyllableLineText(line: LyricLine, style: TextStyle, tone: LyricTone, sweep: List<Float>, timeMs: Long, bright: () -> Float, motion: Boolean, background: Boolean, modifier: Modifier = Modifier) {
    val measurer = rememberTextMeasurer(cacheSize = 0)
    val model = remember(line) { SyllableModel(line) }
    val holder = remember(model, style, measurer) { MeasuredHolder(model, style, measurer) }
    Spacer(
        modifier
            .layout { measurable, constraints ->
                val m = holder.at(constraints.maxWidth)
                val placeable = measurable.measure(Constraints.fixed(m.layout.size.width, m.layout.size.height))
                layout(placeable.width, placeable.height) { placeable.place(0, 0) }
            }
            .drawBehind { holder.current?.let { drawLine(it, model, tone, sweep, timeMs, bright(), motion, background) } },
    )
}

/** A word: syllables [first]..[last], characters [start] until [end] (no trailing space). */
internal class LyricWord(val first: Int, val last: Int, val start: Int, val end: Int, val startMs: Long, val endMs: Long, val emphasis: Emphasis?)

/** How much a held word swells and glows, and over how long (AMLL's numbers). */
internal class Emphasis(val amount: Float, val glow: Float, val durationMs: Float)

/** The line's text and where each syllable and word sits in it. */
internal class SyllableModel(line: LyricLine) {
    val text: String
    val sylStart = IntArray(line.syllables.size)
    val sylEnd = IntArray(line.syllables.size)
    val words: List<LyricWord>

    init {
        val sb = StringBuilder()
        line.syllables.forEachIndexed { i, s ->
            sylStart[i] = sb.length
            sb.append(s.text)
            sylEnd[i] = sb.length
            if (!s.joined && i < line.syllables.lastIndex) sb.append(' ')
        }
        text = sb.toString()
        val groups = buildList<IntRange> {
            var first = 0
            line.syllables.forEachIndexed { i, s -> if (!s.joined || i == line.syllables.lastIndex) { add(first..i); first = i + 1 } }
        }
        words = groups.mapIndexed { wi, g ->
            val startMs = line.syllables[g.first].startMs.toLong()
            val endMs = g.maxOf { line.syllables[it].endMs.toLong() }
            val wordText = text.substring(sylStart[g.first], sylEnd[g.last])
            val emphasis = if (shouldEmphasize(wordText, endMs - startMs)) emphasisFor(endMs - startMs, wi == groups.lastIndex) else null
            LyricWord(g.first, g.last, sylStart[g.first], sylEnd[g.last], startMs, endMs, emphasis)
        }
    }

    companion object {
        private val cjk = Regex("[぀-ヿ㐀-鿿가-힯豈-﫿]")

        /**
         * AMLL's rule for words held long enough to glow: a second or more, and CJK or 2 to 7
         * letters. Only scripts whose letters stand alone (the swell draws them one by one).
         */
        fun shouldEmphasize(text: String, durationMs: Long): Boolean {
            val trimmed = text.trim()
            if (durationMs < 1_000 || trimmed.isEmpty() || !standalone(trimmed)) return false
            if (cjk.containsMatchIn(trimmed)) return true
            val length = trimmed.codePointCount(0, trimmed.length)
            return length in 2..7
        }

        private fun standalone(text: String): Boolean = text.codePoints().allMatch { cp ->
            when (Character.UnicodeScript.of(cp)) {
                Character.UnicodeScript.LATIN, Character.UnicodeScript.CYRILLIC, Character.UnicodeScript.GREEK, Character.UnicodeScript.HAN,
                Character.UnicodeScript.HIRAGANA, Character.UnicodeScript.KATAKANA, Character.UnicodeScript.HANGUL, Character.UnicodeScript.COMMON -> true
                else -> false
            }
        }

        fun emphasisFor(durationMs: Long, lastWord: Boolean): Emphasis {
            var span = max(1_000f, durationMs.toFloat())
            var amount = span / 2_000f
            amount = if (amount > 1) sqrt(amount) else amount * amount * amount
            var glow = span / 3_000f
            glow = if (glow > 1) sqrt(glow) else glow * glow * glow
            amount *= 0.6f
            glow *= 0.5f
            if (lastWord) { amount *= 1.6f; glow *= 1.5f; span *= 1.2f }
            return Emphasis(min(1.2f, amount), min(0.8f, glow), span)
        }
    }
}

/** A letter of a swelling word, laid out alone, with where it sits in the line. */
internal class Letter(val layout: TextLayoutResult, val x: Float, val y: Float)

/** The line laid out at one width: the paragraph, each word's boxes and each syllable's span. */
internal class MeasuredLine(
    val layout: TextLayoutResult,
    /** Per word: the boxes it is drawn clipped to (one per row it is on); they tile each row. */
    val boxes: List<List<Rect>>,
    val sylLeft: FloatArray,
    val sylRight: FloatArray,
    val rtl: BooleanArray,
    /** Per word: its letters, for the words that swell. */
    val letters: List<List<Letter>?>,
    val fontPx: Float,
)

/** Measures the line once per width (the same width every frame) and keeps it. */
internal class MeasuredHolder(private val model: SyllableModel, private val style: TextStyle, private val measurer: TextMeasurer) {
    var current: MeasuredLine? = null
        private set
    private var width = -1

    fun at(maxWidth: Int): MeasuredLine {
        current?.takeIf { width == maxWidth }?.let { return it }
        val m = measure(maxWidth)
        current = m
        width = maxWidth
        return m
    }

    private fun measure(maxWidth: Int): MeasuredLine {
        val layout = measurer.measure(AnnotatedString(model.text), style, constraints = Constraints(minWidth = maxWidth, maxWidth = maxWidth))
        val text = model.text
        val n = model.sylStart.size
        val sylLeft = FloatArray(n); val sylRight = FloatArray(n); val rtl = BooleanArray(n)
        for (i in 0 until n) {
            var l = Float.MAX_VALUE; var r = -Float.MAX_VALUE
            val row = if (model.sylStart[i] < text.length) layout.getLineForOffset(model.sylStart[i]) else 0
            for (c in model.sylStart[i] until model.sylEnd[i]) {
                if (layout.getLineForOffset(c) != row) continue
                val box = layout.getBoundingBox(c)
                l = min(l, box.left); r = max(r, box.right)
            }
            if (l > r) { l = 0f; r = 0f }
            sylLeft[i] = l; sylRight[i] = r
            rtl[i] = model.sylStart[i] < text.length && layout.getBidiRunDirection(model.sylStart[i]) == ResolvedTextDirection.Rtl
        }
        // Each word's extent per row, then the boxes widened to meet their neighbours mid-gap.
        class Seg(val word: Int, val row: Int, var left: Float, var right: Float)
        val segs = ArrayList<Seg>()
        model.words.forEachIndexed { wi, w ->
            var seg: Seg? = null
            for (c in w.start until w.end) {
                val row = layout.getLineForOffset(c)
                val box = layout.getBoundingBox(c)
                if (seg == null || seg.row != row) { seg = Seg(wi, row, box.left, box.right).also { segs += it } }
                else { seg.left = min(seg.left, box.left); seg.right = max(seg.right, box.right) }
            }
        }
        val fontPx = layout.layoutInput.density.run { style.fontSize.toPx() }
        val margin = fontPx
        val boxes = List(model.words.size) { ArrayList<Rect>(1) }
        for ((row, inRow) in segs.groupBy { it.row }) {
            val sorted = inRow.sortedBy { it.left }
            val top = layout.getLineTop(row) - if (row == 0) margin else 0f
            val bottom = layout.getLineBottom(row) + if (row == layout.lineCount - 1) margin else 0f
            sorted.forEachIndexed { k, s ->
                val left = if (k == 0) -margin else (sorted[k - 1].right + s.left) / 2
                val right = if (k == sorted.lastIndex) layout.size.width + margin else (s.right + sorted[k + 1].left) / 2
                boxes[s.word] += Rect(left, top, right, bottom)
            }
        }
        val letters = model.words.map { w ->
            if (w.emphasis == null) return@map null
            val it = BreakIterator.getCharacterInstance()
            it.setText(text.substring(w.start, w.end))
            buildList<Letter> {
                var a = it.first()
                var b = it.next()
                while (b != BreakIterator.DONE) {
                    val from = w.start + a
                    val glyph = text.substring(from, w.start + b)
                    if (glyph.isNotBlank()) {
                        val single = measurer.measure(AnnotatedString(glyph), style.copy(textAlign = TextAlign.Start), softWrap = false)
                        val row = layout.getLineForOffset(from)
                        val box = layout.getBoundingBox(from)
                        add(Letter(single, box.left, layout.getLineBaseline(row) - single.firstBaseline))
                    }
                    a = b; b = it.next()
                }
            }
        }
        return MeasuredLine(layout, boxes, sylLeft, sylRight, rtl, letters, fontPx)
    }
}

private val easeOut = CubicBezierEasing(0f, 0f, 0.58f, 1f)
private val empIn = CubicBezierEasing(0.2f, 0.4f, 0.58f, 1f)
private val empOut = CubicBezierEasing(0.3f, 0f, 0.58f, 1f)
private fun emphasisCurve(x: Float) = if (x < 0.5f) empIn.transform(x / 0.5f) else 1f - empOut.transform((x - 0.5f) / 0.5f)

private fun DrawScope.drawLine(m: MeasuredLine, model: SyllableModel, tone: LyricTone, sweep: List<Float>, timeMs: Long, bright: Float, motion: Boolean, background: Boolean) {
    if (bright <= 0.001f) {
        // Not lit: sung and unsung are the same idle tone, nothing rises. One draw.
        drawText(m.layout, color = tone.idle)
        return
    }
    val sung = tone.sung(bright)
    val unsung = tone.unsung(bright)
    val size = m.fontPx
    val fade = size * LyricsMotion.FADE_WIDTH_EM
    val lift = if (motion) (if (background) 2 else 1) * LyricsMotion.LIFT_EM * size else 0f
    model.words.forEachIndexed { wi, w ->
        val letters = m.letters[wi]
        if (motion && letters != null && w.emphasis != null) {
            drawEmphasis(letters, w, w.emphasis, { dx -> wordBrush(m, w, sweep, sung, unsung, fade, dx) }, timeMs, bright, size)
            return@forEachIndexed
        }
        val brush = wordBrush(m, w, sweep, sung, unsung, fade)
        val rise = lift * bright * easeOut.transform(((timeMs - w.startMs).toFloat() / max(1_000L, w.endMs - w.startMs)).coerceIn(0f, 1f))
        for (box in m.boxes[wi]) {
            translate(top = -rise) { clipRect(box.left, box.top, box.right, box.bottom) { drawText(m.layout, brush) } }
        }
    }
}

/** Sung to the left of the edge, unsung to the right (mirrored for right-to-left text), for text drawn [dx] across. */
private fun wordBrush(m: MeasuredLine, w: LyricWord, sweep: List<Float>, sung: Color, unsung: Color, fade: Float, dx: Float = 0f): Brush {
    var k = -1
    for (i in w.first..w.last) if (sweep.getOrElse(i) { 0f } < 1f) { k = i; break }
    if (k < 0) return SolidColor(sung)
    val p = sweep.getOrElse(k) { 0f }
    if (k == w.first && p <= 0f) return SolidColor(unsung)
    val left = m.sylLeft[k] - dx; val right = m.sylRight[k] - dx
    val width = right - left
    return if (m.rtl[k]) {
        val edge = right + fade - p * (width + fade)
        Brush.horizontalGradient(listOf(sung, unsung), startX = edge, endX = edge - fade)
    } else {
        val edge = left - fade + p * (width + fade)
        Brush.horizontalGradient(listOf(sung, unsung), startX = edge, endX = edge + fade)
    }
}

/** AMLL's emphasis: each letter swells, spreads, lifts and glows in turn. */
private fun DrawScope.drawEmphasis(letters: List<Letter>, w: LyricWord, e: Emphasis, brushAt: (Float) -> Brush, timeMs: Long, bright: Float, size: Float) {
    val count = letters.size.coerceAtLeast(1)
    letters.forEachIndexed { index, letter ->
        val delay = w.startMs + e.durationMs / 2.5f / count * index
        val x = ((timeMs - delay) / e.durationMs).coerceIn(0f, 1f)
        val swell = emphasisCurve(x) * bright
        val scale = 1f + swell * 0.1f * e.amount
        val dx = -swell * 0.03f * e.amount * (count / 2f - index) * size
        val float = ((timeMs - (delay - 400f)) / (e.durationMs * 1.4f)).coerceIn(0f, 1f)
        val dy = (-swell * 0.025f * e.amount - sin(float * PI.toFloat()) * LyricsMotion.LIFT_EM) * size
        val glow = swell * e.glow
        val shadow = if (glow > 0.01f) Shadow(Color.White.copy(alpha = glow.coerceIn(0f, 1f)), Offset.Zero, min(0.3f, e.glow * 0.3f) * size * 2) else null
        val pivot = Offset(letter.layout.size.width / 2f, letter.layout.size.height * 0.6f)
        withTransform({ translate(letter.x + dx, letter.y + dy); scale(scale, scale, pivot) }) {
            drawText(letter.layout, brushAt(letter.x), shadow = shadow)
        }
    }
}
