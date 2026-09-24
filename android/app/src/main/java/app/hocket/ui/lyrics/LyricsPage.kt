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
import androidx.compose.material3.ButtonGroup
import androidx.compose.material3.ButtonGroupDefaults
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
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
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
 * cue timings, driven by the extrapolated position (`PositionStamp` + clock, [CoreClient.positionNow])
 * at frame rate, never by event arrival; line tier: per-line highlight (syllables are never
 * fabricated); unsynced: scrollable text. Lines that overlap (a background vocal or duet voice over
 * the main line) are lit and swept together; the focus (scale, scroll) stays on the main voice.
 * Depth-of-field blur on inactive lines, duet sides, background sub-voices smaller and dimmer, tap
 * to seek, offset control, fluid background. Throttled to 30 fps in battery saver and stopped when
 * not visible.
 */
@Composable
fun LyricsPage(visible: Boolean, modifier: Modifier = Modifier) {
    val client = LocalCoreClient.current
    val entry by client.nowPlaying.collectAsStateWithLifecycle()
    val lyricsMap by client.lyrics.collectAsStateWithLifecycle()
    val batterySaver by client.batterySaver.collectAsStateWithLifecycle()
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

    // Position at frame rate; 30 fps in battery saver; nothing when not visible.
    var positionMs by remember { mutableStateOf(0L) }
    LaunchedEffect(visible, batterySaver, trackId) {
        if (!visible) return@LaunchedEffect
        var last = 0L
        while (true) withFrameNanos { now ->
            if (!batterySaver || now - last >= 33_000_000L) { positionMs = client.positionNow(); last = now }
        }
    }

    Box(modifier.fillMaxSize()) {
        LyricsBackground(entry?.track?.coverArt, animated = animatedSetting && !batterySaver, visible = visible, modifier = Modifier.fillMaxSize())
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
            LyricsList(doc, positionMs, onSeek = { client.dispatch(Commands.seekTo(it)) }, modifier = Modifier.weight(1f))
            OffsetControl(doc, onOffset = { client.dispatch(Commands.setLyricsOffset(doc.trackId, it)) })
        }
    }
}

@Composable
private fun OffsetControl(doc: Lyrics, onOffset: (Int) -> Unit) {
    Row(Modifier.fillMaxWidth().navigationBarsPadding().padding(horizontal = 16.dp, vertical = 6.dp), verticalAlignment = Alignment.CenterVertically) {
        val tier = stringResource(when (doc.tier) { LyricsTier.Syllable -> R.string.lyrics_tier_syllable; LyricsTier.Line -> R.string.lyrics_tier_line; LyricsTier.Unsynced -> R.string.lyrics_tier_unsynced })
        val source = stringResource(when (doc.source) { LyricsSource.Server -> R.string.lyrics_source_server; LyricsSource.External -> R.string.lyrics_source_external; LyricsSource.Embedded -> R.string.lyrics_source_embedded })
        Text("$tier${stringResource(R.string.dot_separator)}$source", style = MaterialTheme.typography.labelSmall, color = Color.White.copy(alpha = 0.7f), modifier = Modifier.weight(1f))
        if (doc.tier != LyricsTier.Unsynced) {
            Text(stringResource(R.string.lyrics_offset_value, doc.offsetMs), style = MaterialTheme.typography.labelMedium, color = Color.White, modifier = Modifier.padding(end = 8.dp))
            ButtonGroup(overflowIndicator = {}, horizontalArrangement = Arrangement.spacedBy(ButtonGroupDefaults.ConnectedSpaceBetween)) {
                clickableItem(onClick = { onOffset(doc.offsetMs - 100) }, label = "−")
                clickableItem(onClick = { onOffset(0) }, label = "0")
                clickableItem(onClick = { onOffset(doc.offsetMs + 100) }, label = "+")
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
    var SemanticsPropertyReceiver.lyricSweep by Sweep
    var SemanticsPropertyReceiver.lyricVoice by Voice
    var SemanticsPropertyReceiver.lyricLit by Lit
    var SemanticsPropertyReceiver.lyricFocused by Focused
}

/**
 * The line the list follows: the cursor's primary line, unless that is a background sub-voice line
 * singing over a main line that is still running, in which case the main line keeps the focus.
 */
internal fun focusLine(doc: Lyrics, cursor: LyricsCursor): Int {
    val primary = cursor.lineIndex
    if (primary < 0 || !doc.lines[primary].background) return primary
    return cursor.activeLines.firstOrNull { !doc.lines[it].background } ?: primary
}

@Composable
private fun LyricsList(doc: Lyrics, positionMs: Long, onSeek: (Long) -> Unit, modifier: Modifier) {
    val cursor = remember(doc, positionMs) { LyricsCursor.at(doc, positionMs) }
    val focus = remember(doc, cursor) { focusLine(doc, cursor) }
    val listState = rememberLazyListState()
    var userScrolling by remember { mutableStateOf(false) }
    LaunchedEffect(listState.isScrollInProgress) { if (listState.isScrollInProgress) userScrolling = true }
    LaunchedEffect(focus) {
        if (focus >= 0 && !listState.isScrollInProgress) {
            listState.animateScrollToItem(focus, scrollOffset = -220)
            userScrolling = false
        }
    }
    val sides = remember(doc) { doc.agents.associate { it.id to it.side.toInt() } }
    LazyColumn(state = listState, modifier = modifier.fillMaxSize().testTag("lyrics.list"), contentPadding = PaddingValues(top = 200.dp, bottom = 320.dp, start = 24.dp, end = 24.dp)) {
        itemsIndexed(doc.lines) { i, line ->
            // Lit: the primary line (until the gap state) and every line singing over it right now
            // (duet, background). Each lit line sweeps its own syllables from the same position.
            val lit = (i == cursor.lineIndex && !cursor.inGap) || (i != cursor.lineIndex && i in cursor.activeLines)
            val distance = if (focus < 0) 2 else kotlin.math.abs(i - focus)
            val side = line.agent?.let { sides[it] } ?: 0
            // The core compares `position - offset` against the line start, so seeking to a line's
            // start means `start + offset` in audio time.
            LyricLineView(i, line, doc.tier, lit, i == focus, distance, side, cursor, onClick = { line.startMs?.let { onSeek((it.toLong() + doc.offsetMs).coerceAtLeast(0)) } })
        }
        if (cursor.inGap) item { Text(stringResource(R.string.lyrics_instrumental), color = Color.White, modifier = Modifier.padding(8.dp)) }
    }
}

@Composable
private fun LyricLineView(index: Int, line: LyricLine, tier: LyricsTier, lit: Boolean, focused: Boolean, distance: Int, side: Int, cursor: LyricsCursor, onClick: () -> Unit) {
    val scale by animateFloatAsState(if (lit) 1f else 0.92f, label = "lineScale")
    val blur = if (lit || tier == LyricsTier.Unsynced) 0.dp else (distance.coerceAtMost(4) * 1.2f).dp
    val alpha = if (tier == LyricsTier.Unsynced) 1f else if (lit) 1f else (0.55f - distance * 0.06f).coerceAtLeast(0.25f)
    // Background sub-voices: smaller, lighter weight and dimmer than the main voice.
    val baseSize = if (line.background) 18.sp else 28.sp
    val weight = if (line.background) FontWeight.Medium else FontWeight.Bold
    val desc = stringResource(R.string.lyrics_line_a11y, line.text)
    // Syllable sweep from this line's own timing (a background line lit beside its main line sweeps
    // independently); held at 1 once the line has ended, 0 before it starts.
    val sweep = remember(line, tier, cursor.effectiveMs) { if (tier == LyricsTier.Syllable) LyricsCursor.sweep(line, cursor.effectiveMs) else emptyList() }
    Column(
        Modifier.fillMaxWidth().padding(vertical = 8.dp)
            .graphicsLayer { scaleX = scale; scaleY = scale; transformOrigin = androidx.compose.ui.graphics.TransformOrigin(if (side == 1) 1f else 0f, 0.5f) }
            .blur(blur).alpha(alpha)
            .clickable(enabled = line.startMs != null, onClick = onClick)
            .semantics {
                contentDescription = desc
                this[LyricsSemantics.Sweep] = sweep
                this[LyricsSemantics.Voice] = if (line.background) "bg" else "main"
                this[LyricsSemantics.Lit] = lit
                this[LyricsSemantics.Focused] = focused
            }
            .testTag("lyrics.line.$index"),
        horizontalAlignment = if (side == 1) Alignment.End else Alignment.Start,
    ) {
        val activeColor = Color.White
        val inactiveColor = Color.White.copy(alpha = if (line.background) 0.45f else 0.6f)
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
            tier == LyricsTier.Line && lit -> {
                // Per-line highlight: a soft sweep across the line by lineProgress; no syllables invented.
                val p = if (index == cursor.lineIndex) cursor.lineProgress else 1f
                Text(line.text, style = style.copy(brush = Brush.horizontalGradient(0f to activeColor, (p - 0.15f).coerceAtLeast(0f) to activeColor, (p + 0.15f).coerceAtMost(1f) to inactiveColor, 1f to inactiveColor)))
            }
            else -> Text(line.text, style = style, color = if (lit) activeColor else inactiveColor)
        }
        line.translation?.let { Text(it, style = MaterialTheme.typography.bodyMedium, color = Color.White.copy(alpha = 0.6f)) }
    }
}
