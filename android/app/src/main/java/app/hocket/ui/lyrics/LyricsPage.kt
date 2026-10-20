package app.hocket.ui.lyrics

import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.animation.core.snap
import androidx.compose.animation.core.spring
import androidx.compose.animation.core.tween
import androidx.compose.foundation.interaction.collectIsDraggedAsState
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.text.BasicText
import androidx.compose.ui.draw.drawBehind
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.drawscope.withTransform
import androidx.compose.ui.layout.layout
import androidx.compose.ui.text.style.LineBreak
import androidx.compose.ui.unit.Dp
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.Stable
import kotlinx.coroutines.flow.first
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.setValue
import androidx.compose.runtime.withFrameNanos
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.hapticfeedback.HapticFeedbackType
import androidx.compose.ui.layout.LayoutCoordinates
import androidx.compose.ui.layout.onGloballyPositioned
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.platform.LocalHapticFeedback
import androidx.compose.ui.platform.LocalView
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.SemanticsPropertyKey
import androidx.compose.ui.semantics.SemanticsPropertyReceiver
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.isTraversalGroup
import androidx.compose.ui.semantics.onClick
import androidx.compose.ui.semantics.onLongClick
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.stateDescription
import app.hocket.ui.a11y.LocalReducedMotion
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import app.hocket.R
import app.hocket.core.Commands
import app.hocket.core.Queries
import app.hocket.core.SettingKeys
import app.hocket.core.api.LyricLine
import app.hocket.core.api.Lyrics
import app.hocket.core.api.LyricsSource
import app.hocket.core.api.LyricsTier
import app.hocket.core.api.QueryResult
import app.hocket.core.lyrics.LyricsCursor
import app.hocket.ui.LocalCoreClient
import app.hocket.ui.components.EmptyState
import app.hocket.ui.icons.HocketIcons

/**
 * Native lyrics renderer, after Apple Music (by way of AMLL and huddlefm's native renderer, see
 * [LyricsMotion]). Syllable tier: each syllable lights from its own cue timings with a soft leading
 * edge, sung words rise a little and short held words swell and glow letter by letter
 * ([SyllableLineText]); all driven by the extrapolated position (`PositionStamp` + clock,
 * `CoreClient.positionNow`) at frame rate, never by event arrival. Line tier: the whole line lights
 * at once (syllables are never fabricated); unsynced: scrollable text. Lines that overlap (a
 * background vocal or duet voice over the main line) are lit and swept together; the focus (scale,
 * scroll) stays on the main voice, and background vocals slide out under their line while it is
 * sung and fold away after. The current line grows and lights on springs; the list scrolls to it a
 * moment early with the lines rippling into place one after another ([StaggeredScroll]); lines
 * further away soften (depth of field); long gaps breathe three dots. Duet voices sit on their own
 * sides. Tap to seek, offset control, fluid background. Position sampled every display frame (30 fps on
 * battery saver, slower still with reduced motion) and stopped when not visible; the scroll and the springs run only while
 * something moves; battery saver drops the depth-of-field blur and the fluid background.
 *
 * Accessibility: the page is one labelled container ("Lyrics"); each line is one item with its text,
 * a "current line" state on the line being sung and a "background vocal" state on sub-voice lines,
 * "Play from this line" as its click and "Share lyrics" as its long-click. Nothing is a live region:
 * lines and syllables are never announced as they pass; the instrumental dots are one labelled
 * item. With a screen reader on, background vocals stay in place (it reads every line). With reduced
 * motion (animations removed) the sweep becomes a static highlight of the lit line, blur, line
 * scaling, rising and swelling words and the folding background vocals are off, the list jumps
 * instead of scrolling, the fluid background stands still and the position is sampled 4 times a
 * second.
 *
 * Holding a line opens the share screen ([LyricsShareDialog]) with that line on a card.
 *
 * [listModifier] applies to the lines only (the full player fades them out at its slot's edges).
 * [embedded] (the full player) also leaves out the tier, source and timing row under the lines: the
 * player offers it from the song menu ([LyricsTimingSheet]).
 */
@Composable
fun LyricsPage(visible: Boolean, modifier: Modifier = Modifier, embedded: Boolean = false, listModifier: Modifier = Modifier) {
    val client = LocalCoreClient.current
    val entry by client.nowPlaying.collectAsStateWithLifecycle()
    val batterySaver by client.batterySaver.collectAsStateWithLifecycle()
    val reducedMotion = LocalReducedMotion.current
    val settings by client.settings.collectAsStateWithLifecycle()
    val animatedSetting = settings[SettingKeys.DISPLAY_ANIMATED_BACKGROUND]?.value?.trim() != "false"
    val current = rememberCurrentLyrics()
    val trackId = current.trackId
    val lyrics = current.lyrics

    // Position every display frame, so the sweep and the rising words move as smoothly as the
    // springs around them; 30 fps on battery saver; 4 per second with reduced motion (enough to
    // move the static highlight between lines); nothing when not visible, and no frames at all
    // while paused (the position is frozen until a seek, play or another track).
    var positionMs by remember { mutableStateOf(0L) }
    LaunchedEffect(visible, reducedMotion, batterySaver, trackId) {
        if (!visible) return@LaunchedEffect
        val interval = if (reducedMotion) 250_000_000L else if (batterySaver) 33_000_000L else 0L
        var last = 0L
        while (true) {
            val mark = client.positionMark()
            withFrameNanos { now ->
                if (!mark.moving || interval == 0L || now - last >= interval) { positionMs = client.positionNow(); last = now }
            }
            client.awaitPositionMayMove(mark)
        }
    }

    val pageLabel = stringResource(R.string.lyrics_page_a11y)
    Box(modifier.fillMaxSize().semantics { contentDescription = pageLabel; isTraversalGroup = true }.testTag("lyrics.page")) {
        // Inside the full player (embedded) the player's own artwork gradient shows through.
        if (!embedded) LyricsBackground(entry?.track?.coverArt, animated = animatedSetting && !batterySaver, visible = visible, modifier = Modifier.fillMaxSize())
        if (trackId == null) { EmptyState(stringResource(R.string.empty_queue_title), stringResource(R.string.empty_queue_body)); return@Box }
        if (current.loaded && lyrics == null) {
            Column(Modifier.fillMaxSize(), verticalArrangement = Arrangement.Center, horizontalAlignment = Alignment.CenterHorizontally) {
                EmptyState(stringResource(R.string.empty_lyrics_title), stringResource(R.string.empty_lyrics_body))
                val external = settings[SettingKeys.LYRICS_EXTERNAL_ENABLED]?.value?.trim() == "true"
                if (external) TextButton(onClick = { client.dispatch(Commands.fetchLyrics(trackId)) }) { Text(stringResource(R.string.lyrics_fetch)) }
                else Text(stringResource(R.string.empty_lyrics_external_hint), style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
            }
            return@Box
        }
        val doc = lyrics ?: return@Box
        var share by remember(doc.trackId) { mutableStateOf<LyricsShareRequest?>(null) }
        Column(Modifier.fillMaxSize()) {
            LyricsList(doc, positionMs, reducedMotion, lowPower = batterySaver, onSeek = { client.dispatch(Commands.seekTo(it)) }, onShare = { share = it }, sharing = share?.line ?: -1, modifier = Modifier.weight(1f).then(listModifier))
            // Inside the full player the timing lives in the song menu instead ([LyricsTimingSheet]).
            if (!embedded) OffsetControl(doc, onOffset = { client.dispatch(Commands.setLyricsOffset(doc.trackId, it)) })
        }
        val track = entry?.track
        share?.let { request ->
            if (track != null && track.id == doc.trackId && request.line in doc.lines.indices) LyricsShareDialog(doc, track, request, onDismiss = { share = null })
        }
    }
}

/**
 * The current track's lyrics: the core's live copy once it has one, else fetched once per track.
 * [CurrentLyrics.loaded] tells "none" (loaded, null) from "still asking" (not loaded).
 */
@Composable
internal fun rememberCurrentLyrics(): CurrentLyrics {
    val client = LocalCoreClient.current
    val entry by client.nowPlaying.collectAsStateWithLifecycle()
    val lyricsMap by client.lyrics.collectAsStateWithLifecycle()
    val trackId = entry?.track?.id
    var fetched by remember { mutableStateOf<Lyrics?>(null) }
    var fetchedFor by remember { mutableStateOf<String?>(null) }
    LaunchedEffect(trackId) {
        fetched = null; fetchedFor = null
        if (trackId != null) { fetched = (client.query(Queries.lyrics(trackId)) as? QueryResult.LyricsResult)?.data; fetchedFor = trackId }
    }
    val live = trackId != null && lyricsMap.containsKey(trackId)
    val lyrics: Lyrics? = if (live) lyricsMap[trackId] else if (fetchedFor == trackId) fetched else null
    return CurrentLyrics(trackId, lyrics, loaded = live || (trackId != null && fetchedFor == trackId))
}

/** See [rememberCurrentLyrics]. */
@androidx.compose.runtime.Immutable
internal data class CurrentLyrics(val trackId: String?, val lyrics: Lyrics?, val loaded: Boolean)

/**
 * The lyrics' tier and source, and (synced lyrics) the timing offset with earlier / reset / later as
 * labelled 48 dp buttons. Under the standalone page, over its dark backdrop ([LyricsContrast]); in
 * the [LyricsTimingSheet] in the sheet's own colours.
 */
@Composable
internal fun OffsetControl(
    doc: Lyrics,
    onOffset: (Int) -> Unit,
    modifier: Modifier = Modifier,
    systemBarPadding: Boolean = true,
    secondary: Color = LyricsContrast.secondary,
    active: Color = LyricsContrast.active,
    padding: PaddingValues = PaddingValues(horizontal = 16.dp, vertical = 6.dp),
) {
    Row(modifier.fillMaxWidth().then(if (systemBarPadding) Modifier.navigationBarsPadding() else Modifier).padding(padding), verticalAlignment = Alignment.CenterVertically) {
        val tier = stringResource(when (doc.tier) { LyricsTier.Syllable -> R.string.lyrics_tier_syllable; LyricsTier.Line -> R.string.lyrics_tier_line; LyricsTier.Unsynced -> R.string.lyrics_tier_unsynced })
        val source = stringResource(when (doc.source) { LyricsSource.Server -> R.string.lyrics_source_server; LyricsSource.External -> R.string.lyrics_source_external; LyricsSource.Embedded -> R.string.lyrics_source_embedded })
        Text("$tier${stringResource(R.string.dot_separator)}$source", style = MaterialTheme.typography.labelSmall, color = secondary, modifier = Modifier.weight(1f))
        if (doc.tier != LyricsTier.Unsynced) {
            Text(stringResource(R.string.lyrics_offset_value, doc.offsetMs), style = MaterialTheme.typography.labelMedium, color = active, modifier = Modifier.padding(end = 4.dp))
            // Labelled 48 dp buttons (a "−" glyph alone reads as "minus").
            IconButton(onClick = { onOffset(doc.offsetMs - 100) }, modifier = Modifier.testTag("lyrics.offset.earlier")) { Icon(HocketIcons.Filled.Remove, stringResource(R.string.lyrics_offset_earlier_a11y), tint = active) }
            IconButton(onClick = { onOffset(0) }, modifier = Modifier.testTag("lyrics.offset.reset")) { Icon(HocketIcons.Filled.RestartAlt, stringResource(R.string.lyrics_offset_reset_a11y), tint = active) }
            IconButton(onClick = { onOffset(doc.offsetMs + 100) }, modifier = Modifier.testTag("lyrics.offset.later")) { Icon(HocketIcons.Filled.Add, stringResource(R.string.lyrics_offset_later_a11y), tint = active) }
        }
    }
}

/**
 * "Lyrics timing" from the full player's song menu (while the lyrics show): what the lyrics are
 * (word-, line- or unsynced, and where they came from) and, for synced lyrics, the timing offset,
 * as under the standalone lyrics page.
 */
@OptIn(androidx.compose.material3.ExperimentalMaterial3Api::class)
@Composable
fun LyricsTimingSheet(onDismiss: () -> Unit) {
    val client = LocalCoreClient.current
    val doc = rememberCurrentLyrics().lyrics
    androidx.compose.material3.ModalBottomSheet(onDismissRequest = onDismiss, modifier = Modifier.testTag("lyrics.timing")) {
        Column(Modifier.fillMaxWidth().navigationBarsPadding().padding(bottom = 16.dp)) {
            Text(
                stringResource(R.string.player_lyrics_timing),
                style = MaterialTheme.typography.titleLarge,
                modifier = Modifier.padding(horizontal = 24.dp).semantics { heading() },
            )
            if (doc == null) {
                Text(stringResource(R.string.empty_lyrics_title), style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant, modifier = Modifier.padding(horizontal = 24.dp, vertical = 12.dp))
            } else {
                OffsetControl(
                    doc,
                    onOffset = { client.dispatch(Commands.setLyricsOffset(doc.trackId, it)) },
                    systemBarPadding = false,
                    secondary = MaterialTheme.colorScheme.onSurfaceVariant,
                    active = MaterialTheme.colorScheme.onSurface,
                    padding = PaddingValues(start = 24.dp, end = 12.dp, top = 8.dp),
                )
            }
        }
    }
}

/** Semantics the renderer publishes per lyric line, so tests can see the sweep without reading pixels. */
object LyricsSemantics {
    /** Per-syllable 0..1 progress of a syllable-tier line (empty for other tiers). */
    val Sweep = SemanticsPropertyKey<List<Float>>("LyricSweep")
    /** `main` or `bg` (a background sub-voice line, rendered smaller and dimmer). */
    val Voice = SemanticsPropertyKey<String>("LyricVoice")
    /** The line is lit: primary, or singing over the primary line right now. */
    val Lit = SemanticsPropertyKey<Boolean>("LyricLit")
    /** The line the view scrolls to and scales up (the main voice, even when a background line is primary). */
    val Focused = SemanticsPropertyKey<Boolean>("LyricFocused")
    /** The depth-of-field blur radius the line is drawn with, in dp (0 with reduced motion). */
    val Blur = SemanticsPropertyKey<Float>("LyricBlur")
    var SemanticsPropertyReceiver.lyricSweep by Sweep
    var SemanticsPropertyReceiver.lyricVoice by Voice
    var SemanticsPropertyReceiver.lyricLit by Lit
    var SemanticsPropertyReceiver.lyricFocused by Focused
}

/**
 * The line the list follows: the cursor's primary line, unless that is a background sub-voice line
 * (it starts with or after the line it is sung over, so it is often the later-starting primary). The
 * focus then stays on the main voice: the main line the adapter placed it after, whether or not
 * that line is still running, so a finished background line never outranks it.
 */
internal fun focusLine(doc: Lyrics, cursor: LyricsCursor): Int {
    val primary = cursor.lineIndex
    if (primary < 0 || !doc.lines[primary].background) return primary
    cursor.activeLines.firstOrNull { !doc.lines[it].background }?.let { return it }
    for (i in primary downTo 0) if (!doc.lines[i].background && doc.lines[i].startMs != null) return i
    return primary
}


/** The breathing dots in a long gap: placed before line [before], from [startMs] to [endMs] (offset-adjusted). */
@androidx.compose.runtime.Immutable
internal data class Interlude(val before: Int, val startMs: Long, val endMs: Long)

/**
 * Where the breathing dots go, if anywhere: in the cursor's instrumental gap (from its grace period
 * after the last sung line to the next line), or before a first line that starts
 * [LyricsMotion.INTRO_GAP_MS] or more in.
 */
internal fun interlude(doc: Lyrics, cursor: LyricsCursor): Interlude? {
    if (doc.tier == LyricsTier.Unsynced) return null
    val position = cursor.effectiveMs
    var next = -1
    var nextStart = Long.MAX_VALUE
    var sungUntil = Long.MIN_VALUE
    doc.lines.forEachIndexed { i, line ->
        val start = line.startMs?.toLong() ?: return@forEachIndexed
        if (start <= position) sungUntil = maxOf(sungUntil, LyricsCursor.lineEnd(doc.lines, i))
        else if (start < nextStart) { nextStart = start; next = i }
    }
    if (next < 0) return null
    if (sungUntil == Long.MIN_VALUE) return if (nextStart >= LyricsMotion.INTRO_GAP_MS) Interlude(next, 0L, nextStart) else null
    if (!cursor.inGap) return null
    return Interlude(next, sungUntil + LyricsCursor.GAP_GRACE_MS, nextStart)
}

/** Depth of field: lines two or more from the focus soften with distance, as AMLL's do toward the edges. */
internal fun lineBlur(distance: Int): Float = when {
    distance <= 1 -> 0f
    distance == 2 -> 0.8f
    distance == 3 -> 1.6f
    else -> 2.4f
}

@Composable
private fun LyricsList(doc: Lyrics, positionMs: Long, reducedMotion: Boolean, lowPower: Boolean, onSeek: (Long) -> Unit, onShare: (LyricsShareRequest) -> Unit, sharing: Int, modifier: Modifier) {
    val cursor = remember(doc, positionMs) { LyricsCursor.at(doc, positionMs) }
    val focus = remember(doc, cursor) { focusLine(doc, cursor) }
    // The view moves to a line a moment before it is sung.
    val feel = LocalLyricsFeel.current
    val ahead = remember(doc, positionMs, feel) { focusLine(doc, LyricsCursor.at(doc, positionMs + feel.leadMs)) }
    val gap = remember(doc, cursor) { interlude(doc, cursor) }
    val dotsAt = gap?.before ?: -1
    fun lazyIndex(line: Int) = if (dotsAt in 0..line) line + 1 else line
    val target = when {
        gap != null && cursor.effectiveMs + feel.leadMs < gap.endMs -> dotsAt
        ahead >= 0 -> lazyIndex(ahead)
        else -> lazyIndex(doc.lines.indexOfFirst { it.startMs != null }.coerceAtLeast(0))
    }
    val listState = rememberLazyListState()
    val scope = rememberCoroutineScope()
    val scroller = remember(listState, feel) { StaggeredScroll(listState, scope, feel) }
    val dragged by listState.interactionSource.collectIsDraggedAsState()
    val held by rememberUpdatedState(dragged)
    // The user's scroll wins: whatever was moving stops where it is, and the list comes back with
    // the next line.
    LaunchedEffect(dragged) { if (dragged) scroller.stop() }
    LaunchedEffect(target) {
        if (target < 0 || held) return@LaunchedEffect
        val previous = scroller.target
        val visible = listState.layoutInfo.visibleItemsInfo.any { it.index == target }
        // A seek, the first lines, or coming back from far away: jump. With reduced motion: always.
        if (reducedMotion || previous < 0 || !visible || kotlin.math.abs(target - previous) > 3) scroller.snap(target)
        else scroller.follow(target) { held }
    }
    val sides = remember(doc) { doc.agents.associate { it.id to it.side.toInt() } }
    // Duets leave each side's lines a share of the width toward the other voice, as Apple Music does.
    val duet = remember(doc) { doc.lines.any { l -> l.agent?.let { sides[it] } == 1 } }
    // The main line each background line is sung over (the adapter puts it right after).
    val mains = remember(doc) { IntArray(doc.lines.size) { i -> var j = i; while (j >= 0 && doc.lines[j].background) j--; j } }
    val context = androidx.compose.ui.platform.LocalContext.current
    // Background vocals fold away under their line while it is not sung (Apple Music's), except for
    // a screen reader, which reads every line, or with reduced motion (nothing slides).
    val touchExploration = remember(context) { context.getSystemService(android.view.accessibility.AccessibilityManager::class.java)?.isTouchExplorationEnabled == true }
    val collapse = !reducedMotion && !touchExploration
    val instrumental = stringResource(R.string.lyrics_instrumental)

    @Composable
    fun LineRow(i: Int, line: LyricLine) {
        // Lit: every line being sung right now (main, duet, background), and the focused main
        // line held after it ends until the gap state. Each lit line sweeps its own syllables
        // from the same position; a finished background line goes dim with its main line.
        val lit = i in cursor.activeLines || (i == focus && !cursor.inGap)
        val focused = i == focus
        val distance = if (focus < 0) 2 else kotlin.math.abs(i - focus)
        val side = line.agent?.let { sides[it] } ?: 0
        // Live cursor only on the singing/focused rows. Past rows keep a stable "held at 1"
        // sweep; future rows stay at 0 — neither recomposes when the playhead moves.
        val past = !lit && !focused && (line.startMs?.toLong() ?: Long.MAX_VALUE) <= cursor.effectiveMs
        val lineCursor = if (lit || focused) cursor else LyricsCursor.EMPTY
        val main = mains[i]
        val revealed = !line.background || !collapse || lit || (main >= 0 && (main in cursor.activeLines || (main == focus && !cursor.inGap)))
        val blur = if (reducedMotion || lowPower || lit || doc.tier == LyricsTier.Unsynced) 0f else lineBlur(distance)
        val lazy = lazyIndex(i)
        // The core compares `position - offset` against the line start, so seeking to a line's
        // start means `start + offset` in audio time.
        LyricLineView(i, line, doc.tier, lit, focused, lit || distance <= 1, revealed, side, duet, lineCursor, past, reducedMotion, blur.dp, lag = { scroller.lag(lazy) }, onShift = { scroller.shift(lazy, it) }, shifts = remember(scroller, lazy) { LineShifts(scroller, lazy) }, onClick = { line.startMs?.let { onSeek((it.toLong() + doc.offsetMs).coerceAtLeast(0)) } }, onShare = { onShare(LyricsShareRequest(i, it)) }, hidden = i == sharing)
    }

    BoxWithConstraints(modifier.fillMaxSize()) {
        // Room above and below for the first and last lines to reach the focus line.
        val padding = PaddingValues(start = 24.dp, end = 24.dp, top = maxHeight * LyricsMotion.FOCUS_POSITION, bottom = maxHeight * (1 - LyricsMotion.FOCUS_POSITION))
        LazyColumn(state = listState, modifier = Modifier.fillMaxSize().testTag("lyrics.list"), contentPadding = padding) {
            if (gap == null) {
                itemsIndexed(doc.lines, key = { i, _ -> i }) { i, line -> LineRow(i, line) }
            } else {
                items(dotsAt, key = { it }) { i -> LineRow(i, doc.lines[i]) }
                item(key = "interlude") {
                    val side = doc.lines[dotsAt].agent?.let { sides[it] } ?: 0
                    InterludeDots(gap, cursor.effectiveMs, side, reducedMotion, instrumental, lag = { scroller.lag(dotsAt) }, modifier = Modifier.animateItem())
                }
                items(doc.lines.size - dotsAt, key = { dotsAt + it }) { k -> LineRow(dotsAt + k, doc.lines[dotsAt + k]) }
            }
        }
    }
}

/** A background vocal's space closing after its fade, as [StaggeredScroll.beginShift] runs it. */
@Stable
internal class LineShifts(private val scroller: StaggeredScroll, private val index: Int) {
    fun begin(dy: Float) = scroller.beginShift(index, dy)
    fun apply(shift: StaggeredScroll.Kick?) = scroller.applyShift(shift)
    fun cancel(shift: StaggeredScroll.Kick?) = scroller.cancelShift(shift)
}

private val dotEnter = androidx.compose.animation.core.CubicBezierEasing(0.34f, 1.56f, 0.64f, 1f)
private val dotLight = androidx.compose.animation.core.CubicBezierEasing(0.56f, 0.01f, 0.45f, 1f)

/**
 * AMLL's interlude: three dots that pop in, breathe, light one by one through the gap, and swell
 * then vanish just before the next line. With reduced motion they only light up, in place.
 */
@Composable
private fun InterludeDots(gap: Interlude, timeMs: Long, side: Int, reducedMotion: Boolean, label: String, lag: () -> Float, modifier: Modifier = Modifier) {
    Box(
        modifier.fillMaxWidth().height(44.dp)
            .graphicsLayer { translationY = lag() }
            .semantics { contentDescription = label }
            .testTag("lyrics.interlude")
            .drawBehind {
                val total = (gap.endMs - gap.startMs).toFloat()
                val elapsed = (timeMs - gap.startMs).toFloat()
                if (total <= 0f || elapsed >= total) return@drawBehind
                val e = elapsed.coerceAtLeast(0f)
                val exit = minOf(600f, total * 0.25f)
                val enter = minOf(500f, total * 0.2f)
                val body = maxOf(1f, total - exit)
                var scale = 1f
                var alpha = 1f
                if (!reducedMotion) {
                    scale = dotEnter.transform((e / enter).coerceIn(0f, 1f))
                    alpha = (e / (enter * 0.6f)).coerceIn(0f, 1f)
                    scale *= 1f + 0.07f * kotlin.math.sin(e / 2_500f * kotlin.math.PI.toFloat() * 2f)
                    if (e > body) {
                        val x = (e - body) / exit
                        if (x < 0.65f) scale *= 1f + 0.15f * (1f - (1f - x / 0.65f) * (1f - x / 0.65f))
                        else { val y = (x - 0.65f) / 0.35f; scale *= 1.15f * (1f - y * y); alpha *= 1f - y }
                    }
                }
                if (alpha <= 0f || scale <= 0f) return@drawBehind
                val progress = (e / body).coerceIn(0f, 1f)
                val dot = 10.dp.toPx()
                val space = 7.dp.toPx()
                val width = dot * 3 + space * 2
                val left = if (side == 1) size.width - width else 2.dp.toPx()
                val pivotX = if (side == 1) size.width else left
                val cy = size.height / 2
                withTransform({ scale(scale, scale, Offset(pivotX, cy)) }) {
                    for (k in 0 until 3) {
                        val light = dotLight.transform((progress * 3 - k).coerceIn(0f, 1f))
                        drawCircle(Color.White.copy(alpha = (alpha * (0.25f + 0.7f * light)).coerceIn(0f, 1f)), dot / 2, Offset(left + dot / 2 + k * (dot + space), cy))
                    }
                }
            },
    )
}

@Composable
private fun LyricLineView(index: Int, line: LyricLine, tier: LyricsTier, lit: Boolean, focused: Boolean, near: Boolean, revealed: Boolean, side: Int, duet: Boolean, cursor: LyricsCursor, past: Boolean, reducedMotion: Boolean, blur: Dp, lag: () -> Float, onShift: (Float) -> Unit, shifts: LineShifts, onClick: () -> Unit, onShare: (() -> SourceLine?) -> Unit, hidden: Boolean = false) {
    // Read while drawing only: the line grows, lights and (background vocals) slides out without
    // recomposing.
    val feel = LocalLyricsFeel.current
    val scale = animateFloatAsState(if (lit || reducedMotion) 1f else LyricsMotion.INACTIVE_SCALE, spring(dampingRatio = feel.dampingRatio, stiffness = feel.scaleStiffness), label = "lineScale")
    val bright = animateFloatAsState(if (lit) 1f else 0f, if (reducedMotion) snap() else tween(feel.brightenMs), label = "lineBright")
    val blurRadius = animateFloatAsState(blur.value, if (reducedMotion) snap() else tween(feel.blurMs), label = "lineBlur")
    val shown = animateFloatAsState(if (revealed) 1f else 0f, if (reducedMotion) snap() else spring(dampingRatio = feel.dampingRatio, stiffness = feel.scaleStiffness), label = "lineShown")
    // Background sub-voices: smaller and lighter than the main voice.
    val baseSize = if (line.background) 18.sp else 28.sp
    val weight = if (line.background) FontWeight.SemiBold else FontWeight.Bold
    val tone = if (line.background) LyricTone.Background else LyricTone.Main
    val currentState = stringResource(R.string.lyrics_state_current)
    val backgroundState = stringResource(R.string.lyrics_state_background)
    val seekHere = stringResource(R.string.lyrics_seek_here)
    val shareLabel = stringResource(R.string.lyrics_share)
    val haptics = LocalHapticFeedback.current
    val view = LocalView.current
    // Where the line is laid out and how it is drawn, for its text to fly into the share card and
    // back (not state: read on hold and on close only).
    val coords = remember { arrayOfNulls<LayoutCoordinates>(1) }
    // Balanced rows (no lone word on the last one), as Apple Music wraps.
    val lineStyle = TextStyle(fontSize = baseSize, fontWeight = weight, lineHeight = baseSize * 1.2, textAlign = if (side == 1) TextAlign.End else TextAlign.Start, lineBreak = LineBreak.Heading)
    val lineColor = if (lit) LyricsContrast.active else tone.idle
    val currentLineStyle by rememberUpdatedState(lineStyle)
    val currentLineColor by rememberUpdatedState(lineColor)
    val share: (() -> Unit)? = if (line.text.isBlank()) null else ({
        onShare { coords[0]?.let { c -> screenBounds(view, c)?.let { SourceLine(it, c.size.width, currentLineStyle, currentLineColor) } } }
    })
    val currentShare by rememberUpdatedState(share)
    val a11yText = listOfNotNull(line.text, line.translation).joinToString(". ")
    val a11yState = listOfNotNull(if (focused) currentState else null, if (line.background) backgroundState else null).joinToString(", ")
    // Syllable sweep from this line's own timing (a background line lit beside its main line sweeps
    // independently); held at 1 once the line has ended, 0 before it starts. With reduced motion
    // the rendered sweep is static: the whole lit line highlighted, nothing moving across it.
    val sweep = remember(line, tier, reducedMotion, lit, focused, past, if (lit || focused) cursor.effectiveMs else 0L) {
        when {
            tier != LyricsTier.Syllable -> emptyList()
            reducedMotion -> List(line.syllables.size) { if (lit) 1f else 0f }
            lit || focused -> LyricsCursor.sweep(line, cursor.effectiveMs)
            past -> List(line.syllables.size) { 1f }
            else -> List(line.syllables.size) { 0f }
        }
    }
    // The time the words rise and swell by, held once the line goes by so they settle as it dims.
    val heldTime = remember { LongArray(1) }
    if (lit || focused) heldTime[0] = cursor.effectiveMs
    val origin = androidx.compose.ui.graphics.TransformOrigin(if (side == 1) 1f else 0f, 0.5f)
    // A background vocal's space: open while it shows. It opens at once (the lines it moves are
    // held where they were and glide to their places, see StaggeredScroll.shift); it closes once
    // the vocal has faded, the lines gliding over its space from the moment the fade begins. Both
    // in a frame callback with the matching shift, so a jump and its hold land in the same frame.
    val open = remember { mutableStateOf(revealed) }
    val height = remember { IntArray(1) }
    if (line.background) LaunchedEffect(revealed) {
        if (open.value == revealed) return@LaunchedEffect
        if (reducedMotion) { open.value = revealed; return@LaunchedEffect }
        if (revealed) {
            withFrameNanos { onShift(height[0].toFloat()); open.value = true }
            return@LaunchedEffect
        }
        val closing = withFrameNanos { shifts.begin(-height[0].toFloat()) }
        try {
            androidx.compose.runtime.snapshotFlow { shown.value <= 0.01f }.first { it }
            withFrameNanos { shifts.apply(closing); open.value = false }
        } finally {
            if (open.value) shifts.cancel(closing)
        }
    }
    Box(
        Modifier.fillMaxWidth().then(
            if (!line.background) Modifier
            else Modifier.layout { measurable, constraints ->
                // The space opens at once and closes once the vocal has faded (see the reveal
                // effect above); the vocal itself slides and fades in the line's own layer.
                val placeable = measurable.measure(constraints)
                height[0] = placeable.height
                layout(placeable.width, if (open.value) placeable.height else 0) { placeable.place(0, 0) }
            },
        ),
        contentAlignment = if (side == 1) Alignment.TopEnd else Alignment.TopStart,
    ) {
        Column(
            Modifier.fillMaxWidth(if (duet) 0.85f else 1f)
                // Outside the layer and its padding: the whole row is the touch target.
                // Tap seeks, hold shares; unsynced lines have nowhere to seek to, but can still be held.
                .then(
                    when {
                        line.startMs != null -> Modifier.combinedClickable(onClickLabel = seekHere, onClick = onClick, onLongClickLabel = shareLabel, onLongClick = share)
                        // combinedClickable gives the long-press haptic itself; here it is ours to give.
                        share != null -> Modifier.pointerInput(Unit) { detectTapGestures(onLongPress = { haptics.performHapticFeedback(HapticFeedbackType.LongPress); currentShare?.invoke() }) }
                        else -> Modifier
                    },
                )
                .testTag("lyrics.line.$index")
                // One item per line: its text (and translation), its state; no live region.
                .clearAndSetSemantics {
                    contentDescription = a11yText
                    if (a11yState.isNotEmpty()) stateDescription = a11yState
                    if (line.startMs != null) onClick(seekHere) { onClick(); true }
                    if (share != null) onLongClick(shareLabel) { share(); true }
                    this[LyricsSemantics.Sweep] = sweep
                    this[LyricsSemantics.Voice] = if (line.background) "bg" else "main"
                    this[LyricsSemantics.Lit] = lit
                    this[LyricsSemantics.Focused] = focused
                    this[LyricsSemantics.Blur] = blur.value
                }
                // Offscreen: the line is drawn once at its own size and the layer scaled and moved, so
                // it grows and glides smoothly (text drawn scaled steps from pixel to pixel).
                .graphicsLayer {
                    // A background vocal also slides out from behind its line and grows from 80%
                    // (AMLL's reveal), or shrinks back and fades: one layer with the line's own
                    // motion (a placement layer around this one lost the line while it faded).
                    val reveal = if (line.background) shown.value.coerceIn(0f, 1f) else 1f
                    scaleX = scale.value * (0.8f + 0.2f * reveal); scaleY = scaleX
                    translationY = lag() - (1f - reveal) * 0.8f * height[0]
                    alpha = reveal
                    transformOrigin = if (line.background) androidx.compose.ui.graphics.TransformOrigin(if (side == 1) 1f else 0f, 0f) else origin
                    // A blurred line (or one still easing out of its blur) is a layer already, the
                    // blur's: no second one around it.
                    compositingStrategy = if (reducedMotion || blur > 0.dp || blurRadius.value > 0f) androidx.compose.ui.graphics.CompositingStrategy.Auto else androidx.compose.ui.graphics.CompositingStrategy.Offscreen
                }
                // Inside the moving layer (a blur outside it would cut off a line moved past its box).
                // Eased from one radius to the next as the focus moves on, not stepped, and set on
                // the layer, so it changes without recomposing. Decal edges (unbounded, no clip):
                // the default edge treatment clips the blurred glyphs to the line's box, so the halo
                // of text near the sides was cut off (#68); for the same reason a blurred line has
                // no offscreen layer of its own around the blur.
                .graphicsLayer {
                    val r = blurRadius.value.dp.toPx()
                    renderEffect = if (r > 0.1f) androidx.compose.ui.graphics.BlurEffect(r, r, androidx.compose.ui.graphics.TileMode.Decal) else null
                    clip = false
                }
                // Inside the layer: room for the words to rise and glow.
                .padding(vertical = if (line.background) 4.dp else 10.dp)
                // Inside the scale: the bounds as drawn, the size as laid out.
                .onGloballyPositioned { coords[0] = it }
                // Hidden while it is on the share card (its text flies there and back).
                .alpha(if (hidden) 0f else 1f),
            horizontalAlignment = if (side == 1) Alignment.End else Alignment.Start,
        ) {
            if (tier == LyricsTier.Syllable && line.syllables.isNotEmpty()) {
                SyllableLineText(line, lineStyle, tone, sweep, heldTime[0], bright = { bright.value }, motion = !reducedMotion, background = line.background, near = near, modifier = Modifier.fillMaxWidth())
            } else {
                // Line-timed (and unsynced): the whole line lights at once; no syllables invented.
                BasicText(line.text, style = lineStyle, color = { androidx.compose.ui.graphics.lerp(tone.idle, tone.sung, bright.value) }, modifier = Modifier.fillMaxWidth())
            }
            line.translation?.let { Text(it, style = MaterialTheme.typography.bodyMedium, color = LyricsContrast.secondary, modifier = Modifier.padding(top = 4.dp)) }
        }
    }
}
