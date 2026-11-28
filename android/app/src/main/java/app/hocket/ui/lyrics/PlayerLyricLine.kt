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
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.runtime.withFrameNanos
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.layout.Layout
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.semantics.SemanticsPropertyKey
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
    // The lyrics only while it shows (not for every song while the player is collapsed).
    val current = if (visible) rememberCurrentLyrics() else NO_LYRICS
    val doc = current.lyrics?.takeIf { d -> d.tier != LyricsTier.Unsynced && d.lines.any { it.startMs != null } }
    // Every frame for words being swept; line-timed lyrics only change at a line, so less often.
    val syllables = doc?.tier == LyricsTier.Syllable
    val playhead = rememberLyricsPlayhead(visible && doc != null, LocalReducedMotion.current, lighter, simple || !syllables, current.trackId)
    val positionMs by playhead
    // The playhead for this opening and this song: until it has ticked once it still holds the last
    // one's position, whose line must not flash up.
    var fresh by remember(visible, current.trackId) { mutableStateOf(false) }
    LaunchedEffect(visible, current.trackId) { if (visible) { withFrameNanos { }; fresh = true } }
    val cursor = if (doc != null) LyricsCursor.at(doc, positionMs) else LyricsCursor.EMPTY
    val focus = if (doc != null) focusLine(doc, cursor) else -1
    val present = visible && fresh && doc != null && focus >= 0 && !cursor.inGap && doc.lines[focus].text.isNotBlank()
    val fade = animateFloatAsState(if (present) 1f else 0f, if (reducedMotion) tween(150) else tween(if (present) 320 else 500), label = "playerLyricLine")
    // The line on show (and its song's lyrics): held while it fades out, past it, into another song
    // or out of sight.
    val held = remember { arrayOfNulls<HeldLine>(1) }
    if (present && (held[0]?.doc !== doc || held[0]?.index != focus)) held[0] = HeldLine(doc!!, focus)
    val line = held[0]
    val tone = remember(color) { LyricTone(idle = color.copy(alpha = color.alpha * 0.4f), unsung = color.copy(alpha = color.alpha * 0.4f), sung = color) }
    Layout(
        content = {
            if (line != null) {
                LineSwap(line, reducedMotion) { l ->
                    // Another song's (or a replaced copy's) line fading out: lit, as it was sung.
                    val live = l.doc === doc
                    FitLine(l.doc, l.index, if (live) cursor else LyricsCursor.EMPTY, textStyle, tone, motion = live && !reducedMotion)
                }
            }
        },
        modifier = modifier
            .graphicsLayer { this.alpha = (fade.value * alpha()).coerceIn(0f, 1f) }
            .clearAndSetSemantics { testTag = "player.lyricLine"; this[PlayerLyricLineShown] = present },
    ) { measurables, c ->
        val p = measurables.map { it.measure(Constraints(maxWidth = c.maxWidth)) }
        // No room of its own: its bottom where it is placed, zero high.
        layout(c.maxWidth, 0) { p.forEach { it.place(0, -it.height) } }
    }
}

/** A line of a song's lyrics, by identity (a fresh copy of the lyrics is another line). */
private class HeldLine(val doc: Lyrics, val index: Int) {
    override fun equals(other: Any?) = other is HeldLine && other.doc === doc && other.index == index
    override fun hashCode() = System.identityHashCode(doc) * 31 + index
}

private val NO_LYRICS = CurrentLyrics(null, null, loaded = false)

/** For tests: whether a line is on show (it is silent for accessibility services). */
internal val PlayerLyricLineShown = SemanticsPropertyKey<Boolean>("PlayerLyricLineShown")

/** How far a sung word rises, in ems: the room kept above and below a lit line for it. */
private const val RISE = 0.3f

/** The most lines a lyric line takes before it stops shrinking (it wraps on past them). */
private const val MAX_LINES = 2

/** The smallest a long line shrinks to (from its style's size) to fit in [MAX_LINES]. */
private val MIN_SIZE: TextUnit = 17.sp

/** One line to the next: the new one rising in as the old one rises away, or a crossfade with reduced motion. */
@Composable
private fun LineSwap(line: HeldLine, reducedMotion: Boolean, content: @Composable (HeldLine) -> Unit) {
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
    val line = doc.lines.getOrNull(index) ?: return
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
