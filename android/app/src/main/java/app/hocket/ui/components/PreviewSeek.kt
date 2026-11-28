package app.hocket.ui.components

/** Position extrapolated from the core's sample, capped at the song. */
fun previewPositionMs(positionMs: Long, takenAt: Double, playing: Boolean, durationMs: Long, now: Double): Long {
    val extra = if (playing) (now - takenAt).coerceAtLeast(0.0) else 0.0
    val at = positionMs + extra.toLong()
    return if (durationMs > 0) at.coerceIn(0, durationMs) else at.coerceAtLeast(0)
}
