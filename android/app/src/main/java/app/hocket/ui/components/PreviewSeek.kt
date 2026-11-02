package app.hocket.ui.components

/**
 * Where a horizontal drag on a preview row seeks, as a fraction of the song: the progress the drag
 * started from, moved by the distance dragged (the row's width is the whole song). The finger's
 * position doesn't matter, only how far it went, as on an iOS slider.
 */
fun previewDragFraction(anchor: Float, distance: Float, width: Float): Float =
    (anchor + distance / width.coerceAtLeast(1f)).coerceIn(0f, 1f)

/** Whether the core's progress has caught up with a released seek (within a second, or 2% of a short song). */
fun previewSeekLanded(fraction: Float, target: Float, durationMs: Long): Boolean {
    if (durationMs <= 0) return true
    val tolerance = maxOf(1_000f / durationMs, 0.02f)
    return kotlin.math.abs(fraction - target) <= tolerance
}

/** Position extrapolated from the core's sample, capped at the song. */
fun previewPositionMs(positionMs: Long, takenAt: Double, playing: Boolean, durationMs: Long, now: Double): Long {
    val extra = if (playing) (now - takenAt).coerceAtLeast(0.0) else 0.0
    val at = positionMs + extra.toLong()
    return if (durationMs > 0) at.coerceIn(0, durationMs) else at.coerceAtLeast(0)
}
