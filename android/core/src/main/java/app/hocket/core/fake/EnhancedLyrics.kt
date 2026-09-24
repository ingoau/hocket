package app.hocket.core.fake

import app.hocket.core.api.LyricLine
import app.hocket.core.api.LyricSyllable
import app.hocket.core.api.Lyrics
import app.hocket.core.api.LyricsAgent
import app.hocket.core.api.LyricsSource
import app.hocket.core.api.LyricsTier
import app.hocket.core.api.TrackId

/*
 * OpenSubsonic songLyrics v2 shapes as Navidrome sends them for `getLyricsBySongId?enhanced=true`,
 * and the core's adapter rules (`crates/hocket-core/src/lyrics/adapt.rs`) re-stated in Kotlin, so the
 * fake core serves exactly what the real core would for a server document: 0-based *inclusive*
 * `byteStart`/`byteEnd` offsets into the line's UTF-8 value ("I" is 0..0, the next word starts at 2),
 * true syllable splits ("ti" 19..20 + "tle" 21..23 are joined because nothing sits between them),
 * background-vocal agents (`role: "bg"`, Navidrome's `__nd_bg__|<agent>`) whose extra cue lines
 * become sub-voice lines, and cue lines without cues, which stay at line tier.
 */

data class RawAgent(val id: String, val role: String, val name: String? = null)

data class RawLine(val start: Long?, val value: String)

data class RawCue(val start: Long?, val end: Long?, val value: String, val byteStart: Int? = null, val byteEnd: Int? = null)

/** Word/syllable timing for one line; [index] refers to the `line[]` entry. */
data class RawCueLine(val index: Int?, val start: Long?, val end: Long?, val value: String, val agentId: String?, val cue: List<RawCue>)

data class RawStructuredLyrics(
    val displayArtist: String? = null,
    val displayTitle: String? = null,
    val lang: String = "",
    val synced: Boolean = true,
    val agents: List<RawAgent> = emptyList(),
    val line: List<RawLine>,
    val cueLine: List<RawCueLine> = emptyList(),
)

object EnhancedLyrics {
    /** The core's `adapt_entry`, for the fake. */
    fun adapt(trackId: TrackId, entry: RawStructuredLyrics, source: LyricsSource = LyricsSource.Server): Lyrics {
        val agents = buildAgents(entry.agents)
        val bgIds = entry.agents.filter { it.role.equals("bg", ignoreCase = true) }.map { it.id }.toSet()
        fun agentOf(cl: RawCueLine) = cl.agentId
        fun isBg(cl: RawCueLine) = agentOf(cl)?.let { it in bgIds } == true
        val synced = entry.synced && entry.line.any { it.start != null }
        val lines = ArrayList<LyricLine>()
        val origin = ArrayList<Int?>()
        entry.line.forEachIndexed { i, raw ->
            // Several cue lines can share one `line[]` index: the main voice's words plus one per
            // extra voice singing over it. The first non-background one is the line itself.
            val cueLines = if (synced) cueLinesFor(entry, i).toMutableList() else ArrayList()
            val mainPos = cueLines.indexOfFirst { !isBg(it) }.takeIf { it >= 0 } ?: 0
            val cueLine = if (cueLines.isNotEmpty()) cueLines.removeAt(mainPos) else null
            val startMs = if (synced) (raw.start ?: cueLine?.start)?.let(::toMs) else null
            var endMs = cueLine?.end?.let(::toMs)
            val (text, syllables) = if (cueLine != null) (cueLine.value.ifEmpty { raw.value }) to syllablesFrom(cueLine) else raw.value to emptyList()
            if (endMs == null) syllables.lastOrNull()?.let { endMs = it.endMs }
            val agent = cueLine?.let(::agentOf)
            lines += LyricLine(startMs, endMs, text, syllables, agent, agent != null && agent in bgIds, null)
            origin += i
            for (cl in cueLines) subVoiceLine(cl, bgIds)?.let { lines += it; origin += null }
        }
        // A line's end defaults to the next line's start when the server gave none (sub-voice lines
        // are not "next": they overlap their line).
        if (synced) {
            for (i in lines.indices) {
                if (lines[i].endMs == null && origin[i] != null) {
                    val nextStart = (i + 1 until lines.size).filter { origin[it] != null }.firstNotNullOfOrNull { lines[it].startMs }
                    val s = lines[i].startMs
                    if (s != null && nextStart != null && nextStart >= s) lines[i] = lines[i].copy(endMs = nextStart)
                }
            }
        }
        val tier = when {
            !synced -> LyricsTier.Unsynced
            lines.any { it.syllables.size >= 2 } -> LyricsTier.Syllable
            else -> LyricsTier.Line
        }
        return Lyrics(
            trackId, tier, entry.lang.takeIf { it.isNotEmpty() && it != "xxx" && it != "und" },
            entry.displayArtist, entry.displayTitle, agents, lines, source, 0,
        )
    }

    /** Agents in order of appearance with `main` first; sides alternate, `bg` shares the side of the agent before it. */
    private fun buildAgents(raw: List<RawAgent>): List<LyricsAgent> {
        val ordered = raw.sortedBy { if (it.role.equals("main", ignoreCase = true)) 0 else 1 }
        var voiceIndex = 0
        var lastSide = 0
        return ordered.map { a ->
            val side = if (a.role.equals("bg", ignoreCase = true)) lastSide else (voiceIndex % 2).also { voiceIndex++; lastSide = it }
            LyricsAgent(a.id, a.name, side.toUInt())
        }
    }

    private fun cueLinesFor(entry: RawStructuredLyrics, lineIndex: Int): List<RawCueLine> =
        if (entry.cueLine.all { it.index == null }) listOfNotNull(entry.cueLine.getOrNull(lineIndex))
        else entry.cueLine.filter { it.index == lineIndex }

    /** An extra cue line at an already-taken index: a voice singing over the line; starts at its first timed word. */
    private fun subVoiceLine(cl: RawCueLine, bgIds: Set<String>): LyricLine? {
        if (cl.value.isBlank()) return null
        val syllables = syllablesFrom(cl)
        val start = (cl.cue.firstNotNullOfOrNull { it.start } ?: cl.start)?.let(::toMs)
        val end = cl.end?.let(::toMs) ?: syllables.lastOrNull()?.endMs
        return LyricLine(start, end, cl.value, syllables, cl.agentId, cl.agentId != null && cl.agentId in bgIds, null)
    }

    /**
     * Syllables for a cue line: whitespace-only cues are word boundaries; two cues are joined when
     * nothing but non-space bytes sits between them in the line (from the inclusive byte offsets;
     * cues without offsets fall back to their own whitespace). Empty when any cue lacks a start.
     */
    fun syllablesFrom(cl: RawCueLine): List<LyricSyllable> {
        if (cl.cue.isEmpty() || cl.cue.any { it.start == null }) return emptyList()
        val out = ArrayList<LyricSyllable>()
        val cues = cl.cue
        for ((i, cue) in cues.withIndex()) {
            if (cue.value.isBlank()) {
                if (out.isNotEmpty()) out[out.lastIndex] = out.last().copy(joined = false)
                continue
            }
            val start = cue.start!!
            val end = cue.end ?: cues.drop(i + 1).firstNotNullOfOrNull { it.start } ?: cl.end ?: start
            var text = cue.value.trimEnd()
            val trailingSpace = cue.value.lastOrNull()?.isWhitespace() == true
            val next = cues.drop(i + 1).firstOrNull { it.value.isNotEmpty() }
            val nextIsText = next != null && next.value.isNotBlank() && !next.value.first().isWhitespace()
            var joined = !trailingSpace && nextIsText
            if (joined && next != null) {
                byteGap(cl.value, cue, next)?.let { gap ->
                    if (gap.any { it.isWhitespace() }) joined = false else text += gap // "well-known": punctuation stays visible
                }
            }
            out += LyricSyllable(text, toMs(start), toMs(maxOf(end, start)), joined)
        }
        if (out.isNotEmpty()) out[out.lastIndex] = out.last().copy(joined = false)
        return out
    }

    /** The bytes of [line] between the end of [cue] and the start of [next] (inclusive offsets), or null when the offsets do not describe [line]. */
    internal fun byteGap(line: String, cue: RawCue, next: RawCue): String? {
        val bs = cue.byteStart ?: return null
        val be = cue.byteEnd ?: return null
        val nbs = next.byteStart ?: return null
        if (bs < 0 || be < 0 || nbs < 0) return null
        val bytes = line.toByteArray(Charsets.UTF_8)
        val value = cue.value.trimEnd().toByteArray(Charsets.UTF_8)
        val endExcl = if (bs + value.size <= bytes.size && bytes.copyOfRange(bs, bs + value.size).contentEquals(value)) bs + value.size else be + 1
        if (nbs < endExcl || nbs > bytes.size) return null
        if (!isCharBoundary(bytes, endExcl) || !isCharBoundary(bytes, nbs)) return null
        return String(bytes.copyOfRange(endExcl, nbs), Charsets.UTF_8)
    }

    private fun isCharBoundary(bytes: ByteArray, i: Int): Boolean = i == 0 || i == bytes.size || (i in 1 until bytes.size && (bytes[i].toInt() and 0xC0) != 0x80)

    private fun toMs(v: Long): UInt = v.coerceIn(0L, UInt.MAX_VALUE.toLong()).toUInt()
}
