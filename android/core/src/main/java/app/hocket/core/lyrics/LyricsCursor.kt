package app.hocket.core.lyrics

import app.hocket.core.api.LyricLine
import app.hocket.core.api.Lyrics
import app.hocket.core.api.LyricsTier

/**
 * Where the playhead is inside a [Lyrics] document at a given position.
 *
 * A Kotlin reimplementation of the core's cursor rules (`lyrics/cursor.rs`) so the renderer can run
 * it per frame from the extrapolated position without an FFI round trip. The rules must agree with
 * the core's:
 *
 * - The per-track user offset is applied the core's way: a positive offset makes the lyrics appear
 *   later, so the audio position is compared as if it were earlier (`effective = position - offset`).
 * - The primary line is the latest-starting line whose start is at or before the position (ties go
 *   to the later line); lines without timing never become primary. Nothing assumes the lines are
 *   sorted: a background or duet sub-voice line follows the line it sings over and may start later
 *   than the next main line.
 * - [activeLines] holds every line whose `[start, end)` contains the position, several at once for
 *   overlapping duet and background lines, so each of them sweeps at the same time. A line without
 *   an end runs until the next timed line starts.
 * - Syllable progress exists only on the syllable tier and only inside a syllable's own timing;
 *   after a line's last syllable ends the sweep holds at 1 (a gap between lines never sweeps
 *   anything). Line-tier lyrics get a per-line highlight; nothing is ever interpolated to fake
 *   syllables. [sweep] gives the per-syllable progress for any line from its own timings, so a
 *   background line lit at the same time as its main line sweeps independently of which is primary.
 * - [lineProgress] is a 0..1 sweep across the primary line for line-tier rendering.
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
    /** Every line containing the position (duets, background vocals); holds [lineIndex] while that line runs. */
    val activeLines: List<Int> = if (lineIndex >= 0) listOf(lineIndex) else emptyList(),
    /** The position after the user offset, ms: what every rule above compared against. */
    val effectiveMs: Long = 0L,
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
            val position = rawPositionMs - lyrics.offsetMs
            val lines = lyrics.lines
            var active = NONE
            var activeStart = Long.MIN_VALUE
            for (i in lines.indices) {
                val start = lines[i].startMs?.toLong() ?: continue
                if (start <= position && start >= activeStart) { active = i; activeStart = start }
            }
            if (active == NONE) return EMPTY.copy(effectiveMs = position)
            val activeLines = ArrayList<Int>()
            for (i in lines.indices) {
                val start = lines[i].startMs?.toLong() ?: continue
                if (start > position) continue
                val end = lines[i].endMs?.toLong() ?: nextTimedStart(lines, i)
                val inside = if (end != null) position < end || (i == active && end <= start) else i == active
                if (inside) activeLines += i
            }
            val line = lines[active]
            val start = line.startMs!!.toLong()
            val end = lineEnd(lines, active)
            val nextStart = nextTimedStart(lines, active)
            val inGap = nextStart != null && nextStart - end > GAP_MS && position >= end + GAP_GRACE_MS && position < nextStart
            val lineProgress = if (end > start) ((position - start).toFloat() / (end - start)).coerceIn(0f, 1f) else 1f

            if (lyrics.tier != LyricsTier.Syllable || line.syllables.isEmpty()) {
                return LyricsCursor(active, NONE, 0f, lineProgress, inGap, activeLines, position)
            }
            var syl = NONE
            var progress = 0f
            for (i in line.syllables.indices) {
                val s = line.syllables[i]
                if (position < s.startMs.toLong()) break
                syl = i
                progress = syllableProgress(s.startMs.toLong(), s.endMs.toLong(), position)
            }
            return LyricsCursor(active, syl, progress, lineProgress, inGap, activeLines, position)
        }

        /**
         * Per-syllable progress for one line at [effectiveMs] (already offset-adjusted, see
         * [effectiveMs]): 0 before a syllable starts, its own `start..end` sweep while it lasts, 1
         * once it has ended. A line that has not started is all zeros, one that has finished is all
         * ones (the last syllable is held, never swept further). Empty for lines without syllables.
         */
        fun sweep(line: LyricLine, effectiveMs: Long): List<Float> = line.syllables.map { s ->
            if (effectiveMs < s.startMs.toLong()) 0f else syllableProgress(s.startMs.toLong(), s.endMs.toLong(), effectiveMs)
        }

        private fun syllableProgress(start: Long, end: Long, position: Long): Float =
            if (position >= end) 1f else if (end > start) ((position - start).toFloat() / (end - start)).coerceIn(0f, 1f) else 1f

        /** A line ends at its own end, else at its last syllable's end, else at the next timed line's start, else 5 s after its start. */
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
