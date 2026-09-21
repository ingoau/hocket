package app.hocket.ui.components

import androidx.compose.runtime.Composable
import androidx.compose.ui.res.stringResource
import app.hocket.R
import java.util.Locale

/** `m:ss` or `h:mm:ss` for positions and track durations. */
fun formatClock(ms: Long): String {
    val total = (ms / 1000).coerceAtLeast(0)
    val h = total / 3600
    val m = (total % 3600) / 60
    val s = total % 60
    return if (h > 0) String.format(Locale.US, "%d:%02d:%02d", h, m, s) else String.format(Locale.US, "%d:%02d", m, s)
}

fun formatClock(ms: UInt): String = formatClock(ms.toLong())

/** "1 h 12 min" / "42 min" for album and playlist lengths. */
@Composable
fun formatDurationWords(ms: Long): String {
    val minutes = (ms / 60_000).toInt()
    return if (minutes >= 60) stringResource(R.string.duration_hours_minutes, minutes / 60, minutes % 60) else stringResource(R.string.duration_minutes, minutes)
}

@Composable
fun formatBytes(bytes: Double): String = when {
    bytes >= 1e9 -> stringResource(R.string.size_gb, bytes / 1e9)
    bytes >= 1e6 -> stringResource(R.string.size_mb, bytes / 1e6)
    bytes >= 1e3 -> stringResource(R.string.size_kb, bytes / 1e3)
    else -> stringResource(R.string.size_bytes, bytes.toLong())
}

/** Relative age: "now", "5m", "3h", "2d". */
@Composable
fun formatAgo(epochMs: Double, nowMs: Double = System.currentTimeMillis().toDouble()): String {
    val delta = ((nowMs - epochMs) / 1000).toLong().coerceAtLeast(0)
    return when {
        delta < 60 -> stringResource(R.string.time_now)
        delta < 3600 -> stringResource(R.string.time_minutes, (delta / 60).toInt())
        delta < 86_400 -> stringResource(R.string.time_hours, (delta / 3600).toInt())
        else -> stringResource(R.string.time_days, (delta / 86_400).toInt())
    }
}
