package app.hocket.core.fake

import app.hocket.core.api.LyricLine
import app.hocket.core.api.LyricSyllable
import app.hocket.core.api.Lyrics
import app.hocket.core.api.LyricsAgent
import app.hocket.core.api.LyricsSource
import app.hocket.core.api.LyricsTier
import app.hocket.core.api.TrackId
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.booleanOrNull
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import kotlinx.serialization.json.longOrNull

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

data class RawCue(val start: Long?, val end: Long?, val value: String, val byteStart: Int? = null, val byteEnd: Int? = null, val agentId: String? = null)

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
    /** `main`, `translation`, `pronunciation`; absent means `main`. */
    val kind: String? = null,
    /** LRC-style server offset: positive means the lyrics are early, so timestamps move earlier. */
    val offset: Long? = null,
) {
    val effectiveKind: String get() = kind?.takeIf { it.isNotBlank() }?.lowercase() ?: "main"
}

object EnhancedLyrics {
    /**
     * Parses an OpenSubsonic `getLyricsBySongId` answer (the whole `subsonic-response` document, or
     * just its `lyricsList`) into its structured entries. Unknown fields are ignored.
     */
    fun parseOpenSubsonic(json: String): List<RawStructuredLyrics> {
        val root = Json.parseToJsonElement(json).jsonObject
        val list = (root["subsonic-response"]?.jsonObject ?: root)["lyricsList"]?.jsonObject ?: root
        return list.arr("structuredLyrics").map { e ->
            val o = e.jsonObject
            RawStructuredLyrics(
                displayArtist = o.str("displayArtist"),
                displayTitle = o.str("displayTitle"),
                lang = o.str("lang") ?: "",
                synced = o["synced"]?.jsonPrimitive?.booleanOrNull ?: false,
                agents = o.arr("agents").map { a -> a.jsonObject.let { RawAgent(it.str("id") ?: "", it.str("role") ?: "", it.str("name")) } },
                line = o.arr("line").map { l -> l.jsonObject.let { RawLine(it.long("start"), it.str("value") ?: "") } },
                cueLine = o.arr("cueLine").map { c ->
                    c.jsonObject.let { cl ->
                        RawCueLine(
                            cl.long("index")?.toInt(), cl.long("start"), cl.long("end"), cl.str("value") ?: "", cl.str("agentId"),
                            cl.arr("cue").map { q -> q.jsonObject.let { RawCue(it.long("start"), it.long("end"), it.str("value") ?: "", it.long("byteStart")?.toInt(), it.long("byteEnd")?.toInt(), it.str("agentId")) } },
                        )
                    }
                },
                kind = o.str("kind"),
                offset = o.long("offset"),
            )
        }
    }

    private fun JsonObject.str(k: String): String? = (this[k] as? JsonPrimitive)?.takeIf { it.isString }?.content
    private fun JsonObject.long(k: String): Long? = (this[k] as? JsonPrimitive)?.longOrNull
    private fun JsonObject.arr(k: String): List<JsonElement> = (this[k] as? JsonArray) ?: emptyList()

    /** The core's `adapt_response`: the entry to render ([pickMain]) plus a translation layer when one lines up. */
    fun adaptList(trackId: TrackId, entries: List<RawStructuredLyrics>, source: LyricsSource = LyricsSource.Server): Lyrics? {
        val main = pickMain(entries) ?: return null
        val (lyrics, origin) = adaptMapped(trackId, main, source)
        val translation = entries.firstOrNull { it.effectiveKind == "translation" && it !== main } ?: return lyrics
        return attachTranslation(lyrics, origin, main, translation)
    }

    /** The first synced `main` entry, then any `main`, then the first non-empty one. */
    fun pickMain(entries: List<RawStructuredLyrics>): RawStructuredLyrics? =
        entries.firstOrNull { it.effectiveKind == "main" && it.synced && it.line.isNotEmpty() }
            ?: entries.firstOrNull { it.effectiveKind == "main" && it.line.isNotEmpty() }
            ?: entries.firstOrNull { it.line.isNotEmpty() }

    /** Translation `line[k]` goes on the line that came from main `line[k]`, only when the layers line up. */
    private fun attachTranslation(lyrics: Lyrics, origin: List<Int?>, main: RawStructuredLyrics, tr: RawStructuredLyrics): Lyrics {
        if (tr.line.size != main.line.size) return lyrics
        if (main.synced && tr.synced) {
            val aligned = main.line.zip(tr.line).all { (m, t) ->
                val a = m.start; val b = t.start
                if (a != null && b != null) kotlin.math.abs(a - b) <= 500 else a == null && b == null
            }
            if (!aligned) return lyrics
        }
        val lines = lyrics.lines.mapIndexed { i, line ->
            val k = origin[i] ?: return@mapIndexed line
            val t = tr.line.getOrNull(k)?.value
            if (t != null && t.isNotBlank()) line.copy(translation = t) else line
        }
        return lyrics.copy(lines = lines)
    }

    /** The core's `adapt_entry`, for the fake. */
    fun adapt(trackId: TrackId, entry: RawStructuredLyrics, source: LyricsSource = LyricsSource.Server): Lyrics = adaptMapped(trackId, entry, source).first

    private fun adaptMapped(trackId: TrackId, entry: RawStructuredLyrics, source: LyricsSource): Pair<Lyrics, List<Int?>> {
        val shift = -(entry.offset ?: 0L)
        val agents = buildAgents(entry.agents)
        val bgIds = entry.agents.filter { it.role.equals("bg", ignoreCase = true) }.map { it.id }.toSet()
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
            val startMs = if (synced) (raw.start ?: cueLine?.start)?.let { toMs(it + shift) } else null
            var endMs = cueLine?.end?.let { toMs(it + shift) }
            val (text, syllables) = if (cueLine != null) (cueLine.value.ifEmpty { raw.value }) to syllablesFrom(cueLine, shift) else raw.value to emptyList()
            if (endMs == null) syllables.lastOrNull()?.let { endMs = it.endMs }
            val agent = cueLine?.let(::agentOf)
            lines += LyricLine(startMs, endMs, text, syllables, agent, agent != null && agent in bgIds, null)
            origin += i
            for (cl in cueLines) subVoiceLine(cl, shift, bgIds)?.let { lines += it; origin += null }
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
        ) to origin
    }

    /** A cue line's voice: its own `agentId`, else the first cue that names one. */
    private fun agentOf(cl: RawCueLine): String? = cl.agentId ?: cl.cue.firstNotNullOfOrNull { it.agentId }

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
    private fun subVoiceLine(cl: RawCueLine, shift: Long, bgIds: Set<String>): LyricLine? {
        if (cl.value.isBlank()) return null
        val syllables = syllablesFrom(cl, shift)
        val start = (cl.cue.firstNotNullOfOrNull { it.start } ?: cl.start)?.let { toMs(it + shift) }
        val end = cl.end?.let { toMs(it + shift) } ?: syllables.lastOrNull()?.endMs
        val agent = agentOf(cl)
        return LyricLine(start, end, cl.value, syllables, agent, agent != null && agent in bgIds, null)
    }

    /**
     * Syllables for a cue line: whitespace-only cues are word boundaries; two cues are joined when
     * nothing but non-space bytes sits between them in the line (from the inclusive byte offsets;
     * cues without offsets fall back to their own whitespace). Empty when any cue lacks a start.
     */
    fun syllablesFrom(cl: RawCueLine, shift: Long = 0L): List<LyricSyllable> {
        if (cl.cue.isEmpty() || cl.cue.any { it.start == null }) return emptyList()
        val out = ArrayList<LyricSyllable>()
        val cues = cl.cue
        for ((i, cue) in cues.withIndex()) {
            if (cue.value.isBlank()) {
                if (out.isNotEmpty()) out[out.lastIndex] = out.last().copy(joined = false)
                continue
            }
            val start = cue.start!! + shift
            val end = (cue.end ?: cues.drop(i + 1).firstNotNullOfOrNull { it.start } ?: cl.end)?.let { it + shift } ?: start
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
