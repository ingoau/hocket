package app.hocket.ui.lyrics

import androidx.compose.animation.AnimatedContent
import androidx.compose.animation.ContentTransform
import androidx.compose.animation.SizeTransform
import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.animation.core.spring
import androidx.compose.animation.core.tween
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.slideInVertically
import androidx.compose.animation.slideOutVertically
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.text.BasicText
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.layout.Layout
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.testTag
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.rememberTextMeasurer
import androidx.compose.ui.text.style.LineBreak
import androidx.compose.ui.unit.Constraints
import androidx.compose.ui.unit.TextUnit
import androidx.compose.ui.unit.sp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import app.hocket.core.api.Lyrics
import app.hocket.core.api.LyricsTier
import app.hocket.core.has
import app.hocket.core.lyrics.LyricsCursor
import app.hocket.ui.LocalCoreClient
import app.hocket.ui.a11y.LocalReducedMotion

/**
 * The line being sung in the full player (`lyrics.playerLine`; off by default), in [textStyle] and
 * [color]: word by word where the lyrics are syllable-timed, else the whole line lit at once;
 * nothing for unsynced lyrics. It fades away between lines (an instrumental gap, the intro, the
 * end) and ticks only while [visible]. Up to two lines (shrinking a little to fit them), ending
 * where it is placed and growing upwards: it takes no height of its own, so nothing moves as lines
 * come and go. Not touchable, and silent for accessibility: the lyrics page reads the lyrics.
 */
@Composable
internal fun PlayerLyricLine(
    visible: Boolean,
    textStyle: TextStyle,
    color: Color,
    modifier: Modifier = Modifier,
    alpha: () -> Float = { 1f },
) {
    val client = LocalCoreClient.current
    val saver by client.batterySaver.collectAsStateWithLifecycle()
    val lighter = saver.has(app.hocket.core.api.SaverFeature.LighterLyrics)
    val simple = saver.has(app.hocket.core.api.SaverFeature.SimpleLyrics)
    val reducedMotion = LocalReducedMotion.current || simple
    val current = rememberCurrentLyrics()
    val doc = current.lyrics?.takeIf { d -> d.tier != LyricsTier.Unsynced && d.lines.any { it.startMs != null } }
    val positionMs by rememberLyricsPlayhead(visible && doc != null, reducedMotion, lighter, simple, current.trackId)
    val cursor = if (doc != null) LyricsCursor.at(doc, positionMs) else LyricsCursor.EMPTY
    val focus = if (doc != null) focusLine(doc, cursor) else -1
    val present = visible && doc != null && focus >= 0 && !cursor.inGap && doc.lines[focus].text.isNotBlank()
    val fade = animateFloatAsState(if (present) 1f else 0f, if (reducedMotion) tween(150) else tween(if (present) 320 else 500), label = "playerLyricLine")
    // The line on show: held while it fades out (the cursor has moved past it).
    val held = remember(doc?.trackId) { intArrayOf(-1) }
    if (present) held[0] = focus
    val line = doc?.let { d -> held[0].takeIf { it in d.lines.indices } }
    val tone = remember(color) { LyricTone(idle = color.copy(alpha = color.alpha * 0.4f), unsung = color.copy(alpha = color.alpha * 0.4f), sung = color) }
    Layout(
        content = {
            if (doc != null && line != null) {
                LineSwap(line, reducedMotion) { i -> FitLine(doc, i, cursor, textStyle, tone, motion = !reducedMotion) }
            }
        },
        modifier = modifier
            .graphicsLayer { this.alpha = (fade.value * alpha()).coerceIn(0f, 1f) }
            .clearAndSetSemantics { testTag = "player.lyricLine" },
    ) { measurables, c ->
        val p = measurables.map { it.measure(Constraints(maxWidth = c.maxWidth)) }
        // No room of its own: its bottom where it is placed, zero high.
        layout(c.maxWidth, 0) { p.forEach { it.place(0, -it.height) } }
    }
}

/** How far a sung word rises, in ems: the room kept above and below a lit line for it. */
private const val RISE = 0.3f

/** The most lines a lyric line takes before it stops shrinking (it wraps on past them). */
private const val MAX_LINES = 2

/** The smallest a long line shrinks to (from its style's size) to fit in [MAX_LINES]. */
private val MIN_SIZE: TextUnit = 17.sp

/** One line to the next: the new one rising in as the old one rises away, or a crossfade with reduced motion. */
@Composable
private fun LineSwap(line: Int, reducedMotion: Boolean, content: @Composable (Int) -> Unit) {
    AnimatedContent(
        targetState = line,
        transitionSpec = {
            if (reducedMotion) ContentTransform(fadeIn(tween(120)), fadeOut(tween(120)), sizeTransform = SizeTransform(clip = false))
            else ContentTransform(
                fadeIn(tween(260, delayMillis = 60)) + slideInVertically(spring(dampingRatio = 0.9f, stiffness = 380f)) { it / 3 },
                fadeOut(tween(180)) + slideOutVertically(spring(dampingRatio = 0.9f, stiffness = 380f)) { -it / 3 },
                sizeTransform = SizeTransform(clip = false),
            )
        },
        contentAlignment = Alignment.BottomCenter,
        label = "playerLyricLineSwap",
    ) { i -> content(i) }
}

/**
 * Line [index] of [doc] in [style], shrinking (to [MIN_SIZE]) where it doesn't fit in [MAX_LINES],
 * its words lighting up as they are sung where they are timed ([motion]: sweeping through each).
 */
@Composable
private fun FitLine(doc: Lyrics, index: Int, cursor: LyricsCursor, style: TextStyle, tone: LyricTone, motion: Boolean) {
    val line = doc.lines[index]
    val measurer = rememberTextMeasurer()
    BoxWithConstraints(Modifier.fillMaxWidth()) {
        val w = constraints.maxWidth
        val fit = remember(line.text, w, style) {
            var size = style.fontSize.value
            fun at(s: Float) = style.copy(fontSize = s.sp, lineHeight = (s * 1.18f).sp, lineBreak = LineBreak.Heading)
            while (size > MIN_SIZE.value && measurer.measure(line.text, at(size), constraints = Constraints(maxWidth = w)).lineCount > MAX_LINES) size -= 1f
            at(size)
        }
        val syllables = doc.tier == LyricsTier.Syllable && line.syllables.isNotEmpty()
        val sweep = when {
            !syllables -> emptyList()
            !motion -> List(line.syllables.size) { 1f }
            else -> LyricsCursor.sweep(line, cursor.effectiveMs)
        }
        val rise = with(LocalDensity.current) { (fit.fontSize * RISE).toDp() }
        val m = Modifier.fillMaxWidth().padding(vertical = rise)
        if (syllables) SyllableLineText(line, fit, tone, sweep, cursor.effectiveMs, bright = { 1f }, motion = motion, background = false, near = motion, modifier = m)
        else BasicText(line.text, style = fit, color = { tone.sung }, modifier = m)
    }
}
