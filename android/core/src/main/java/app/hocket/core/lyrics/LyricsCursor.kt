package app.hocket.core.lyrics

import app.hocket.core.api.LyricLine
import app.hocket.core.api.Lyrics
import app.hocket.core.api.LyricsTier

/**
 * Where the playhead is inside a [Lyrics] document at a given position.
 *
 * A Kotlin reimplementation of the core's cursor rules so the renderer can run it per frame from the
 * extrapolated position without an FFI round trip:
 *
 * - The active line is the last line whose start is at or before the position (after the user
 *   offset); lines without timing never become active. A line stays active until the next timed
 *   line starts, or until its own end when there is a gap ("instrumental" state).
 * - Syllable progress exists only on the syllable tier and only inside a syllable's own timing.
 *   Line-tier lyrics get a per-line highlight; nothing is ever interpolated to fake syllables.
 * - [lineProgress] is a 0..1 sweep across the active line for line-tier rendering.
 */
data class LyricsCursor(
    val lineIndex: Int,
    val syllableIndex: Int,
    /** 0..1 within the active syllable (syllable tier only). */
    val syllableProgress: Float,
    /** 0..1 within the active line (line + syllable tiers). */
    val lineProgress: Float,
    /** True when between timed lines: a gap longer than [GAP_MS], entered [GAP_GRACE_MS] after the line ended. */
    val inGap: Boolean,
) {
    companion object {
        const val NONE = -1
        /** Gaps shorter than this keep the previous line lit rather than flashing an empty state. */
        const val GAP_MS = 4_000L
        /** The previous line stays lit this long after its end before the gap state shows. */
        const val GAP_GRACE_MS = 1_000L

        val EMPTY = LyricsCursor(NONE, NONE, 0f, 0f, inGap = false)

        fun at(lyrics: Lyrics, rawPositionMs: Long): LyricsCursor {
            if (lyrics.tier == LyricsTier.Unsynced || lyrics.lines.isEmpty()) return EMPTY
            val position = rawPositionMs + lyrics.offsetMs
            val lines = lyrics.lines
            var active = NONE
            for (i in lines.indices) {
                val start = lines[i].startMs?.toLong() ?: continue
                if (start <= position) active = i else break
            }
            if (active == NONE) return EMPTY
            val line = lines[active]
            val start = line.startMs!!.toLong()
            val end = lineEnd(lines, active)
            val nextStart = nextTimedStart(lines, active)
            val inGap = nextStart != null && nextStart - end > GAP_MS && position >= end + GAP_GRACE_MS && position < nextStart
            val lineProgress = if (end > start) ((position - start).toFloat() / (end - start)).coerceIn(0f, 1f) else 1f

            if (lyrics.tier != LyricsTier.Syllable || line.syllables.isEmpty()) {
                return LyricsCursor(active, NONE, 0f, lineProgress, inGap)
            }
            var syl = NONE
            var progress = 0f
            for (i in line.syllables.indices) {
                val s = line.syllables[i]
                val sStart = s.startMs.toLong()
                val sEnd = s.endMs.toLong()
                if (position < sStart) break
                syl = i
                progress = if (position >= sEnd) 1f else if (sEnd > sStart) ((position - sStart).toFloat() / (sEnd - sStart)) else 1f
            }
            return LyricsCursor(active, syl, progress, lineProgress, inGap)
        }

        /** A line ends at its own end, else at the next timed line's start, else 5 s after its start. */
        fun lineEnd(lines: List<LyricLine>, index: Int): Long {
            val line = lines[index]
            line.endMs?.let { return it.toLong() }
            line.syllables.lastOrNull()?.let { return it.endMs.toLong() }
            nextTimedStart(lines, index)?.let { return it }
            return (line.startMs?.toLong() ?: 0L) + 5_000L
        }

        private fun nextTimedStart(lines: List<LyricLine>, index: Int): Long? {
            for (i in index + 1 until lines.size) lines[i].startMs?.let { return it.toLong() }
            return null
        }
    }
}
