package app.hocket.ui.components

/**
 * Where a horizontal drag on a preview row seeks, as a fraction of the song.
 * [edge] is kept clear at both ends (the Android back gesture owns the screen edge).
 */
fun previewSeekFraction(x: Float, width: Float, edge: Float): Float {
    val span = (width - edge * 2f).coerceAtLeast(1f)
    return ((x - edge) / span).coerceIn(0f, 1f)
}

/** Position extrapolated from the core's sample, capped at the song. */
fun previewPositionMs(positionMs: Long, takenAt: Double, playing: Boolean, durationMs: Long, now: Double): Long {
    val extra = if (playing) (now - takenAt).coerceAtLeast(0.0) else 0.0
    val at = positionMs + extra.toLong()
    return if (durationMs > 0) at.coerceIn(0, durationMs) else at.coerceAtLeast(0)
}
