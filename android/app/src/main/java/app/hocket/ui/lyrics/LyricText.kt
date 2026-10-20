package app.hocket.ui.lyrics

import androidx.compose.animation.core.CubicBezierEasing
import androidx.compose.foundation.layout.Spacer
import androidx.compose.runtime.Composable
import androidx.compose.runtime.State
import androidx.compose.runtime.derivedStateOf
import androidx.compose.runtime.rememberUpdatedState
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
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.layout.Layout
import androidx.compose.ui.graphics.drawscope.clipRect
import androidx.compose.ui.graphics.drawscope.translate
import androidx.compose.ui.graphics.lerp
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.TextLayoutResult
import androidx.compose.ui.text.TextMeasurer
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.drawText
import androidx.compose.ui.text.rememberTextMeasurer
import androidx.compose.ui.text.style.ResolvedTextDirection
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.Constraints
import androidx.compose.ui.unit.IntOffset
import app.hocket.core.api.LyricLine
import java.text.BreakIterator
import kotlin.math.ceil
import kotlin.math.floor
import kotlin.math.max
import kotlin.math.min
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
internal fun SyllableLineText(line: LyricLine, style: TextStyle, tone: LyricTone, sweep: List<Float>, timeMs: Long, bright: () -> Float, motion: Boolean, background: Boolean, near: Boolean = false, modifier: Modifier = Modifier) {
    val measurer = rememberTextMeasurer(cacheSize = 0)
    val model = remember(line) { SyllableModel(line) }
    val holder = remember(model, style, measurer) { MeasuredHolder(model, style, measurer) }
    // This frame's sweep and time, read while drawing only (the pieces redraw, never recompose).
    val frame = rememberUpdatedState(LineFrame(sweep, timeMs))
    // Moving words: while the line is current or next, every word (and each letter of a word that
    // swells) is its own piece in its own layer, moved by the layer. Text is drawn snapped to whole
    // pixels vertically, so a word rising a pixel or two would step instead of glide; a layer
    // moved by a fraction of a pixel is filtered smoothly (huddlefm's sprites, as GPU layers).
    val pieces = motion && near
    val paint = remember(model, tone, background, motion) { LinePaint(model, tone, background, motion) }
    Layout(
        content = {
            Spacer(Modifier.drawBehind { holder.current?.let { m -> if (!pieces || bright() <= 0.001f) paint.drawStill(this, m, frame.value, bright()) } })
            if (pieces) {
                model.words.forEachIndexed { wi, w ->
                    val ranges = model.letters[wi]
                    if (ranges == null) WordPiece(holder, paint, wi, frame, bright)
                    else ranges.indices.forEach { li -> LetterPiece(holder, paint, wi, li, frame, bright) }
                }
            }
        },
        modifier = modifier,
    ) { measurables, constraints ->
        val m = holder.at(constraints.maxWidth)
        val base = measurables[0].measure(Constraints.fixed(m.layout.size.width, m.layout.size.height))
        val placed = ArrayList<Pair<androidx.compose.ui.layout.Placeable, IntOffset>>()
        if (pieces) {
            var k = 1
            model.words.forEachIndexed { wi, _ ->
                val rects = m.letters[wi]?.map { m.letterBox(it) } ?: listOf(m.wordBox(wi))
                for (r in rects) {
                    val left = floor(r.left).toInt(); val top = floor(r.top).toInt()
                    val p = measurables[k++].measure(Constraints.fixed(ceil(r.right - left).toInt().coerceAtLeast(1), ceil(r.bottom - top).toInt().coerceAtLeast(1)))
                    placed += p to IntOffset(left, top)
                }
            }
        }
        layout(base.width, base.height) {
            base.place(0, 0)
            for ((p, at) in placed) p.place(at)
        }
    }
}

/** What changes every frame on a lit line. */
@androidx.compose.runtime.Immutable
internal class LineFrame(val sweep: List<Float>, val timeMs: Long)

/** A word's piece: the paragraph drawn clipped to the word, in a layer that rises with it. */
@Composable
private fun WordPiece(holder: MeasuredHolder, paint: LinePaint, word: Int, frame: State<LineFrame>, bright: () -> Float) {
    val look = remember(paint, word) { derivedStateOf { paint.look(word, frame.value, bright()) } }
    Spacer(
        Modifier
            .graphicsLayer {
                val b = bright()
                translationY = -paint.rise(holder.current, word, frame.value.timeMs, b)
                // Offscreen from the start (the piece exists from the line before), never switched
                // on as the line lights: a layer's first offscreen frame can come out empty.
                compositingStrategy = androidx.compose.ui.graphics.CompositingStrategy.Offscreen
            }
            .drawBehind {
                val m = holder.current ?: return@drawBehind
                val l = look.value
                if (l.bright <= 0.001f) return@drawBehind
                val box = m.wordBox(word)
                translate(-floor(box.left), -floor(box.top)) { paint.drawWord(this, m, word, l) }
            },
    )
}

/** A letter of a swelling word: its own layer, which swells, spreads, lifts and glows. */
@Composable
private fun LetterPiece(holder: MeasuredHolder, paint: LinePaint, word: Int, letter: Int, frame: State<LineFrame>, bright: () -> Float) {
    Spacer(
        Modifier
            .graphicsLayer {
                val m = holder.current ?: return@graphicsLayer
                val b = bright()
                val e = paint.emphasis(m, word, letter, frame.value.timeMs, b)
                val box = m.letterBox(m.letters[word]!![letter])
                val l = m.letters[word]!![letter]
                translationX = e.dx
                translationY = e.dy
                scaleX = e.scale; scaleY = e.scale
                // The pivot: the letter's centre, a little below its middle.
                transformOrigin = androidx.compose.ui.graphics.TransformOrigin(
                    ((l.x + l.layout.size.width / 2f) - floor(box.left)) / size.width.coerceAtLeast(1f),
                    ((l.y + l.layout.size.height * 0.6f) - floor(box.top)) / size.height.coerceAtLeast(1f),
                )
                compositingStrategy = androidx.compose.ui.graphics.CompositingStrategy.Offscreen
            }
            .drawBehind {
                val m = holder.current ?: return@drawBehind
                val b = bright()
                if (b <= 0.001f) return@drawBehind
                val box = m.letterBox(m.letters[word]!![letter])
                translate(-floor(box.left), -floor(box.top)) { paint.drawLetter(this, m, word, letter, frame.value, b) }
            },
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
    /** Per word: its letters' character ranges, for the words that swell. */
    val letters: List<List<IntRange>?>

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
        letters = words.map { w ->
            if (w.emphasis == null) return@map null
            val it = BreakIterator.getCharacterInstance()
            it.setText(text.substring(w.start, w.end))
            buildList<IntRange> {
                var a = it.first()
                var b = it.next()
                while (b != BreakIterator.DONE) {
                    if (text.substring(w.start + a, w.start + b).isNotBlank()) add(w.start + a until w.start + b)
                    a = b; b = it.next()
                }
            }
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
) {
    /** The word's boxes together. */
    fun wordBox(word: Int): Rect = boxes[word].reduceOrNull { a, b -> Rect(min(a.left, b.left), min(a.top, b.top), max(a.right, b.right), max(a.bottom, b.bottom)) } ?: Rect.Zero

    /** Where a swelling letter's layer goes: the letter with room to grow and glow. */
    fun letterBox(letter: Letter): Rect {
        val reach = fontPx * 0.7f
        // Not past the line's sides, where the line's own layer ends and would clip it anyway.
        return Rect((letter.x - reach).coerceAtLeast(0f), letter.y - reach, (letter.x + letter.layout.size.width + reach).coerceAtMost(layout.size.width.toFloat()), letter.y + letter.layout.size.height + reach)
    }
}

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
            // No glyphs on its row (an empty syllable): no extent, left out of its word's.
            if (l > r) { l = Float.NaN; r = Float.NaN }
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
        // Room above and below for the rise, within the line's own padding (which grows with the
        // font to keep it); none to the sides, where the line's offscreen layer ends and clips.
        val margin = fontPx * 0.25f
        val boxes = List(model.words.size) { ArrayList<Rect>(1) }
        for ((row, inRow) in segs.groupBy { it.row }) {
            val sorted = inRow.sortedBy { it.left }
            val top = layout.getLineTop(row) - if (row == 0) margin else 0f
            val bottom = layout.getLineBottom(row) + if (row == layout.lineCount - 1) margin else 0f
            sorted.forEachIndexed { k, s ->
                val left = if (k == 0) 0f else (sorted[k - 1].right + s.left) / 2
                val right = if (k == sorted.lastIndex) layout.size.width.toFloat() else (s.right + sorted[k + 1].left) / 2
                boxes[s.word] += Rect(left, top, right, bottom)
            }
        }
        val letters = model.letters.map { ranges ->
            ranges?.map { r ->
                val single = measurer.measure(AnnotatedString(text.substring(r.first, r.last + 1)), style.copy(textAlign = TextAlign.Start), softWrap = false)
                val box = layout.getBoundingBox(r.first)
                Letter(single, box.left, layout.getLineBaseline(layout.getLineForOffset(r.first)) - single.firstBaseline)
            }
        }
        return MeasuredLine(layout, boxes, sylLeft, sylRight, rtl, letters, fontPx)
    }
}

private val easeOut = CubicBezierEasing(0f, 0f, 0.58f, 1f)
private val empIn = CubicBezierEasing(0.2f, 0.4f, 0.58f, 1f)
private val empOut = CubicBezierEasing(0.3f, 0f, 0.58f, 1f)
private fun emphasisCurve(x: Float) = if (x < 0.5f) empIn.transform(x / 0.5f) else 1f - empOut.transform((x - 0.5f) / 0.5f)

/** How one line's words and letters are painted, for the still line and for its moving pieces. */
internal class LinePaint(private val model: SyllableModel, private val tone: LyricTone, private val background: Boolean, private val motion: Boolean) {
    /** A swelling letter's place: moved by [dx], [dy], grown by [scale]. */
    class Swell(val dx: Float, val dy: Float, val scale: Float, val glow: Float)

    private fun lift(m: MeasuredLine) = (if (background) 2 else 1) * LyricsMotion.LIFT_EM * m.fontPx

    /** How far the word has risen, px: over its own time (a second at least), easing out, with the line's brightness. */
    fun rise(m: MeasuredLine?, word: Int, timeMs: Long, bright: Float): Float {
        // Without motion nothing rises (reduced motion: the static highlight alone).
        if (m == null || bright <= 0f || !motion) return 0f
        val w = model.words[word]
        return lift(m) * bright * easeOut.transform(((timeMs - w.startMs).toFloat() / max(1_000L, w.endMs - w.startMs)).coerceIn(0f, 1f))
    }

    /** AMLL's emphasis for one letter, on top of the rise every sung word has (so a held word settles level with its neighbours). */
    fun emphasis(m: MeasuredLine, word: Int, letter: Int, timeMs: Long, bright: Float): Swell {
        val w = model.words[word]
        val e = w.emphasis ?: return Swell(0f, 0f, 1f, 0f)
        val count = m.letters[word]?.size?.coerceAtLeast(1) ?: 1
        val size = m.fontPx
        val delay = w.startMs + e.durationMs / 2.5f / count * letter
        val swell = emphasisCurve(((timeMs - delay) / e.durationMs).coerceIn(0f, 1f)) * bright
        return Swell(
            dx = -swell * 0.03f * e.amount * (count / 2f - letter) * size,
            dy = -swell * 0.025f * e.amount * size - rise(m, word, timeMs, bright),
            scale = 1f + swell * 0.1f * e.amount,
            glow = swell * e.glow,
        )
    }

    /** How word [word] is painted on [frame] at [bright]. */
    fun look(word: Int, frame: LineFrame, bright: Float) = WordLook.of(model.words[word], frame.sweep, bright)

    /** The whole line, unmoved: one draw when it is not lit; word by word (risen, snapped) without pieces. */
    fun drawStill(scope: DrawScope, m: MeasuredLine, frame: LineFrame, bright: Float) = with(scope) {
        if (bright <= 0.001f) { drawText(m.layout, color = tone.idle); return@with }
        for (wi in model.words.indices) {
            val rise = rise(m, wi, frame.timeMs, bright)
            translate(top = -rise) { drawWord(this, m, wi, WordLook.of(model.words[wi], frame.sweep, bright)) }
        }
    }

    /** One word in the line's coordinates, clipped to its boxes. */
    fun drawWord(scope: DrawScope, m: MeasuredLine, word: Int, look: WordLook) = with(scope) {
        val brush = wordBrush(m, model.words[word], look, tone.sung(look.bright), tone.unsung(look.bright), m.fontPx * LyricsMotion.FADE_WIDTH_EM)
        // alpha = 1 explicitly: left out, a gradient brush's text takes the paint's last alpha (the
        // idle tone's), so a word about to be sung drew darker than the words around it.
        for (box in m.boxes[word]) clipRect(box.left, box.top, box.right, box.bottom) { drawText(m.layout, brush, alpha = 1f) }
    }

    /** One letter of a swelling word, in the line's coordinates, glowing as it swells. */
    fun drawLetter(scope: DrawScope, m: MeasuredLine, word: Int, letter: Int, frame: LineFrame, bright: Float) = with(scope) {
        val l = m.letters[word]!![letter]
        val w = model.words[word]
        val e = w.emphasis!!
        val glow = emphasis(m, word, letter, frame.timeMs, bright).glow
        val shadow = if (glow > 0.01f) Shadow(Color.White.copy(alpha = glow.coerceIn(0f, 1f)), Offset.Zero, min(0.3f, e.glow * 0.3f) * m.fontPx * 2) else null
        val brush = wordBrush(m, w, WordLook.of(w, frame.sweep, bright), tone.sung(bright), tone.unsung(bright), m.fontPx * LyricsMotion.FADE_WIDTH_EM, l.x)
        translate(l.x, l.y) { drawText(l.layout, brush, alpha = 1f, shadow = shadow) }
    }
}

/**
 * Sung to the left of the edge, unsung to the right (mirrored for right-to-left text), for text
 * drawn [dx] across. The edge moves through the word as a whole, AMLL's way: from just before
 * the word (all unsung) to its end (all sung), following the syllables' timing in between, so
 * it never steps back where one syllable hands over to the next.
 */
private fun wordBrush(m: MeasuredLine, w: LyricWord, look: WordLook, sung: Color, unsung: Color, fade: Float, dx: Float = 0f): Brush {
    if (look.k == WordLook.SUNG) return SolidColor(sung)
    if (look.k == WordLook.UNSUNG) return SolidColor(unsung)
    var left = Float.MAX_VALUE; var right = -Float.MAX_VALUE
    for (i in w.first..w.last) if (!m.sylLeft[i].isNaN()) { left = min(left, m.sylLeft[i]); right = max(right, m.sylRight[i]) }
    if (left > right) return SolidColor(unsung)
    left -= dx; right -= dx
    val k = look.k
    // An empty syllable sweeps as a point where the word's sung part ends.
    val rtl = m.rtl[k]
    val sylLeft = if (m.sylLeft[k].isNaN()) (if (rtl) right else left) else m.sylLeft[k] - dx
    val sylRight = if (m.sylRight[k].isNaN()) sylLeft else m.sylRight[k] - dx
    val edge = sweepEdge(left, right, sylLeft, sylRight, look.p, fade, rtl)
    return if (rtl) Brush.horizontalGradient(listOf(sung, unsung), startX = edge, endX = edge - fade)
    else Brush.horizontalGradient(listOf(sung, unsung), startX = edge, endX = edge + fade)
}

/**
 * How a word is painted this frame: all sung, all unsung, or syllable [k] [p] sung, at the line's
 * [bright]ness. Equal looks paint the same, so a word piece redraws only when its look changes
 * (the word being sung, and the line lighting or dimming), not on every frame of the line.
 */
@androidx.compose.runtime.Immutable
internal data class WordLook(val k: Int, val p: Float, val bright: Float) {
    companion object {
        const val SUNG = -1
        const val UNSUNG = -2

        fun of(w: LyricWord, sweep: List<Float>, bright: Float): WordLook {
            for (i in w.first..w.last) {
                val p = sweep.getOrElse(i) { 0f }
                if (p < 1f) return if (i == w.first && p <= 0f) WordLook(UNSUNG, 0f, bright) else WordLook(i, p, bright)
            }
            return WordLook(SUNG, 0f, bright)
        }
    }
}

/**
 * Where the sung side of the soft edge ends in a word spanning [left]..[right], while the syllable
 * at [sylLeft]..[sylRight] is [p] sung; the unsung side starts [fade] further on (to the left for
 * [rtl] text). From [left] - [fade] (nothing sung) to [right] (all of it), moving only forward.
 */
internal fun sweepEdge(left: Float, right: Float, sylLeft: Float, sylRight: Float, p: Float, fade: Float, rtl: Boolean): Float {
    val width = (right - left).coerceAtLeast(1f)
    // How far through the word the singing is, by where it is in this syllable.
    return if (rtl) {
        val u = ((right - (sylRight - p * (sylRight - sylLeft))) / width).coerceIn(0f, 1f)
        right + fade - u * (width + fade)
    } else {
        val u = ((sylLeft + p * (sylRight - sylLeft) - left) / width).coerceIn(0f, 1f)
        left - fade + u * (width + fade)
    }
}
