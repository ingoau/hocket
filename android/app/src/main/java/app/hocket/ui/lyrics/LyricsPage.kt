package app.hocket.ui.lyrics

import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
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
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.runtime.withFrameNanos
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.draw.blur
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.SemanticsPropertyKey
import androidx.compose.ui.semantics.SemanticsPropertyReceiver
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Add
import androidx.compose.material.icons.filled.Remove
import androidx.compose.material.icons.filled.RestartAlt
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.isTraversalGroup
import androidx.compose.ui.semantics.onClick
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

/**
 * Native lyrics renderer. Syllable tier: continuous per-syllable gradient sweep from each line's own
 * cue timings, driven by the extrapolated position (`PositionStamp` + clock, `CoreClient.positionNow`)
 * at frame rate, never by event arrival; line tier: per-line highlight (syllables are never
 * fabricated); unsynced: scrollable text. Lines that overlap (a background vocal or duet voice over
 * the main line) are lit and swept together; the focus (scale, scroll) stays on the main voice.
 * Depth-of-field blur on inactive lines, duet sides, background sub-voices smaller and dimmer, tap
 * to seek, offset control, fluid background. Throttled to 30 fps in battery saver and stopped when
 * not visible.
 *
 * Accessibility: the page is one labelled container ("Lyrics"); each line is one item with its text,
 * a "current line" state on the line being sung and a "background vocal" state on sub-voice lines,
 * and "Play from this line" as its click. Nothing is a live region: lines and syllables are never
 * announced as they pass. With reduced motion (animations removed) the sweep becomes a static
 * highlight of the lit line, blur and line scaling are off, the list jumps instead of scrolling
 * smoothly, the fluid background stands still and the position is sampled 4 times a second.
 */
@Composable
fun LyricsPage(visible: Boolean, modifier: Modifier = Modifier, embedded: Boolean = false) {
    val client = LocalCoreClient.current
    val entry by client.nowPlaying.collectAsStateWithLifecycle()
    val lyricsMap by client.lyrics.collectAsStateWithLifecycle()
    val batterySaver by client.batterySaver.collectAsStateWithLifecycle()
    val reducedMotion = LocalReducedMotion.current
    val settings by client.settings.collectAsStateWithLifecycle()
    val animatedSetting = settings[SettingKeys.DISPLAY_ANIMATED_BACKGROUND]?.value?.trim() != "false"
    val trackId = entry?.track?.id
    var fetched by remember { mutableStateOf<Lyrics?>(null) }
    var fetchedFor by remember { mutableStateOf<String?>(null) }
    LaunchedEffect(trackId) {
        fetched = null; fetchedFor = null
        if (trackId != null) { fetched = (client.query(Queries.lyrics(trackId)) as? QueryResult.LyricsResult)?.data; fetchedFor = trackId }
    }
    val lyrics: Lyrics? = if (trackId != null && lyricsMap.containsKey(trackId)) lyricsMap[trackId] else if (fetchedFor == trackId) fetched else null

    // Position at frame rate; 30 fps in battery saver; 4 per second with reduced motion (enough to
    // move the static highlight between lines); nothing when not visible.
    var positionMs by remember { mutableStateOf(0L) }
    LaunchedEffect(visible, batterySaver, reducedMotion, trackId) {
        if (!visible) return@LaunchedEffect
        val interval = when {
            reducedMotion -> 250_000_000L
            batterySaver -> 33_000_000L
            else -> 0L
        }
        var last = 0L
        while (true) withFrameNanos { now ->
            if (interval == 0L || now - last >= interval) { positionMs = client.positionNow(); last = now }
        }
    }

    val pageLabel = stringResource(R.string.lyrics_page_a11y)
    Box(modifier.fillMaxSize().semantics { contentDescription = pageLabel; isTraversalGroup = true }.testTag("lyrics.page")) {
        // Inside the full player (embedded) the player's own artwork gradient shows through.
        if (!embedded) LyricsBackground(entry?.track?.coverArt, animated = animatedSetting && !batterySaver, visible = visible, modifier = Modifier.fillMaxSize())
        if (trackId == null) { EmptyState(stringResource(R.string.empty_queue_title), stringResource(R.string.empty_queue_body)); return@Box }
        if (fetchedFor == trackId && lyrics == null) {
            Column(Modifier.fillMaxSize(), verticalArrangement = Arrangement.Center, horizontalAlignment = Alignment.CenterHorizontally) {
                EmptyState(stringResource(R.string.empty_lyrics_title), stringResource(R.string.empty_lyrics_body))
                val external = settings[SettingKeys.LYRICS_EXTERNAL_ENABLED]?.value?.trim() == "true"
                if (external) TextButton(onClick = { client.dispatch(Commands.fetchLyrics(trackId)) }) { Text(stringResource(R.string.lyrics_fetch)) }
                else Text(stringResource(R.string.empty_lyrics_external_hint), style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
            }
            return@Box
        }
        val doc = lyrics ?: return@Box
        Column(Modifier.fillMaxSize()) {
            LyricsList(doc, positionMs, reducedMotion, onSeek = { client.dispatch(Commands.seekTo(it)) }, modifier = Modifier.weight(1f),
                padding = if (embedded) PaddingValues(top = 48.dp, bottom = 160.dp, start = 24.dp, end = 24.dp) else PaddingValues(top = 200.dp, bottom = 320.dp, start = 24.dp, end = 24.dp))
            OffsetControl(doc, onOffset = { client.dispatch(Commands.setLyricsOffset(doc.trackId, it)) }, systemBarPadding = !embedded)
        }
    }
}

@Composable
private fun OffsetControl(doc: Lyrics, onOffset: (Int) -> Unit, systemBarPadding: Boolean = true) {
    Row(Modifier.fillMaxWidth().then(if (systemBarPadding) Modifier.navigationBarsPadding() else Modifier).padding(horizontal = 16.dp, vertical = 6.dp), verticalAlignment = Alignment.CenterVertically) {
        val tier = stringResource(when (doc.tier) { LyricsTier.Syllable -> R.string.lyrics_tier_syllable; LyricsTier.Line -> R.string.lyrics_tier_line; LyricsTier.Unsynced -> R.string.lyrics_tier_unsynced })
        val source = stringResource(when (doc.source) { LyricsSource.Server -> R.string.lyrics_source_server; LyricsSource.External -> R.string.lyrics_source_external; LyricsSource.Embedded -> R.string.lyrics_source_embedded })
        Text("$tier${stringResource(R.string.dot_separator)}$source", style = MaterialTheme.typography.labelSmall, color = LyricsContrast.secondary, modifier = Modifier.weight(1f))
        if (doc.tier != LyricsTier.Unsynced) {
            Text(stringResource(R.string.lyrics_offset_value, doc.offsetMs), style = MaterialTheme.typography.labelMedium, color = LyricsContrast.active, modifier = Modifier.padding(end = 4.dp))
            // Labelled 48 dp buttons (a "−" glyph alone reads as "minus").
            IconButton(onClick = { onOffset(doc.offsetMs - 100) }, modifier = Modifier.testTag("lyrics.offset.earlier")) { Icon(Icons.Filled.Remove, stringResource(R.string.lyrics_offset_earlier_a11y), tint = LyricsContrast.active) }
            IconButton(onClick = { onOffset(0) }, modifier = Modifier.testTag("lyrics.offset.reset")) { Icon(Icons.Filled.RestartAlt, stringResource(R.string.lyrics_offset_reset_a11y), tint = LyricsContrast.active) }
            IconButton(onClick = { onOffset(doc.offsetMs + 100) }, modifier = Modifier.testTag("lyrics.offset.later")) { Icon(Icons.Filled.Add, stringResource(R.string.lyrics_offset_later_a11y), tint = LyricsContrast.active) }
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

@Composable
private fun LyricsList(doc: Lyrics, positionMs: Long, reducedMotion: Boolean, onSeek: (Long) -> Unit, modifier: Modifier, padding: PaddingValues = PaddingValues(top = 200.dp, bottom = 320.dp, start = 24.dp, end = 24.dp)) {
    val cursor = remember(doc, positionMs) { LyricsCursor.at(doc, positionMs) }
    val focus = remember(doc, cursor) { focusLine(doc, cursor) }
    val listState = rememberLazyListState()
    var userScrolling by remember { mutableStateOf(false) }
    LaunchedEffect(listState.isScrollInProgress) { if (listState.isScrollInProgress) userScrolling = true }
    LaunchedEffect(focus) {
        if (focus >= 0 && !listState.isScrollInProgress) {
            if (reducedMotion) listState.scrollToItem(focus, scrollOffset = -220) else listState.animateScrollToItem(focus, scrollOffset = -220)
            userScrolling = false
        }
    }
    val sides = remember(doc) { doc.agents.associate { it.id to it.side.toInt() } }
    LazyColumn(state = listState, modifier = modifier.fillMaxSize().testTag("lyrics.list"), contentPadding = padding) {
        itemsIndexed(doc.lines) { i, line ->
            // Lit: every line being sung right now (main, duet, background), and the focused main
            // line held after it ends until the gap state. Each lit line sweeps its own syllables
            // from the same position; a finished background line goes dim with its main line.
            val lit = i in cursor.activeLines || (i == focus && !cursor.inGap)
            val distance = if (focus < 0) 2 else kotlin.math.abs(i - focus)
            val side = line.agent?.let { sides[it] } ?: 0
            // The core compares `position - offset` against the line start, so seeking to a line's
            // start means `start + offset` in audio time.
            LyricLineView(i, line, doc.tier, lit, i == focus, distance, side, cursor, reducedMotion, onClick = { line.startMs?.let { onSeek((it.toLong() + doc.offsetMs).coerceAtLeast(0)) } })
        }
        if (cursor.inGap) item { Text(stringResource(R.string.lyrics_instrumental), color = LyricsContrast.active, modifier = Modifier.padding(8.dp)) }
    }
}

@Composable
private fun LyricLineView(index: Int, line: LyricLine, tier: LyricsTier, lit: Boolean, focused: Boolean, distance: Int, side: Int, cursor: LyricsCursor, reducedMotion: Boolean, onClick: () -> Unit) {
    val animatedScale by animateFloatAsState(if (lit) 1f else 0.92f, label = "lineScale")
    val scale = if (reducedMotion) 1f else animatedScale
    val blur = if (reducedMotion || lit || tier == LyricsTier.Unsynced) 0.dp else (distance.coerceAtMost(4) * 1.2f).dp
    // Inactive lines keep a contrast floor (LyricsContrast); depth comes from the blur, not a fade.
    val alpha = 1f
    // Background sub-voices: smaller, lighter weight and dimmer than the main voice.
    val baseSize = if (line.background) 18.sp else 28.sp
    val weight = if (line.background) FontWeight.Medium else FontWeight.Bold
    val currentState = stringResource(R.string.lyrics_state_current)
    val backgroundState = stringResource(R.string.lyrics_state_background)
    val seekHere = stringResource(R.string.lyrics_seek_here)
    val a11yText = listOfNotNull(line.text, line.translation).joinToString(". ")
    val a11yState = listOfNotNull(if (focused) currentState else null, if (line.background) backgroundState else null).joinToString(", ")
    // Syllable sweep from this line's own timing (a background line lit beside its main line sweeps
    // independently); held at 1 once the line has ended, 0 before it starts. With reduced motion
    // the rendered sweep is static: the whole lit line highlighted, nothing moving across it.
    val sweep = remember(line, tier, cursor.effectiveMs, reducedMotion, lit) {
        when {
            tier != LyricsTier.Syllable -> emptyList()
            reducedMotion -> List(line.syllables.size) { if (lit) 1f else 0f }
            else -> LyricsCursor.sweep(line, cursor.effectiveMs)
        }
    }
    Column(
        Modifier.fillMaxWidth().padding(vertical = 8.dp)
            .graphicsLayer { scaleX = scale; scaleY = scale; transformOrigin = androidx.compose.ui.graphics.TransformOrigin(if (side == 1) 1f else 0f, 0.5f) }
            .blur(blur).alpha(alpha)
            .clickable(enabled = line.startMs != null, onClickLabel = seekHere, onClick = onClick)
            .testTag("lyrics.line.$index")
            // One item per line: its text (and translation), its state; no live region.
            .clearAndSetSemantics {
                contentDescription = a11yText
                if (a11yState.isNotEmpty()) stateDescription = a11yState
                if (line.startMs != null) onClick(seekHere) { onClick(); true }
                this[LyricsSemantics.Sweep] = sweep
                this[LyricsSemantics.Voice] = if (line.background) "bg" else "main"
                this[LyricsSemantics.Lit] = lit
                this[LyricsSemantics.Focused] = focused
                this[LyricsSemantics.Blur] = blur.value
            },
        horizontalAlignment = if (side == 1) Alignment.End else Alignment.Start,
    ) {
        val activeColor = LyricsContrast.active
        // Background vocals are smaller (not "large text"), so they keep the 4.5:1 secondary colour.
        val inactiveColor = if (line.background) LyricsContrast.secondary else LyricsContrast.inactive
        val style = TextStyle(fontSize = baseSize, fontWeight = weight, lineHeight = baseSize * 1.25, textAlign = if (side == 1) TextAlign.End else TextAlign.Start)
        when {
            tier == LyricsTier.Syllable && line.syllables.isNotEmpty() -> {
                FlowRow(horizontalArrangement = if (side == 1) Arrangement.End else Arrangement.Start) {
                    line.syllables.forEachIndexed { si, s ->
                        val progress = sweep.getOrElse(si) { 0f }
                        val brush = when (progress) {
                            0f -> Brush.horizontalGradient(listOf(inactiveColor, inactiveColor))
                            1f -> Brush.horizontalGradient(listOf(activeColor, activeColor))
                            else -> Brush.horizontalGradient(0f to activeColor, (progress - 0.12f).coerceAtLeast(0f) to activeColor, (progress + 0.12f).coerceAtMost(1f) to inactiveColor, 1f to inactiveColor)
                        }
                        Text(s.text + if (s.joined) "" else " ", style = style.copy(brush = brush))
                    }
                }
            }
            tier == LyricsTier.Line && lit && reducedMotion -> Text(line.text, style = style, color = activeColor)
            tier == LyricsTier.Line && lit -> {
                // Per-line highlight: a soft sweep across the line by lineProgress; no syllables invented.
                val p = if (index == cursor.lineIndex) cursor.lineProgress else 1f
                Text(line.text, style = style.copy(brush = Brush.horizontalGradient(0f to activeColor, (p - 0.15f).coerceAtLeast(0f) to activeColor, (p + 0.15f).coerceAtMost(1f) to inactiveColor, 1f to inactiveColor)))
            }
            else -> Text(line.text, style = style, color = if (lit) activeColor else inactiveColor)
        }
        line.translation?.let { Text(it, style = MaterialTheme.typography.bodyMedium, color = LyricsContrast.secondary) }
    }
}
