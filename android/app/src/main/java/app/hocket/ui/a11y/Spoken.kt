package app.hocket.ui.a11y

import android.content.res.Resources
import androidx.compose.runtime.Composable
import app.hocket.R

/** Durations and positions the way a screen reader should say them: "1 minute 32 seconds". */
object Spoken {
    fun duration(resources: Resources, ms: Long): String {
        val total = (ms / 1000).coerceAtLeast(0)
        val h = (total / 3600).toInt()
        val m = ((total % 3600) / 60).toInt()
        val s = (total % 60).toInt()
        val parts = buildList {
            if (h > 0) add(resources.getQuantityString(R.plurals.a11y_hours, h, h))
            if (m > 0) add(resources.getQuantityString(R.plurals.a11y_minutes, m, m))
            if (s > 0 || isEmpty()) add(resources.getQuantityString(R.plurals.a11y_seconds, s, s))
        }
        return parts.joinToString(" ")
    }

    /** "1 minute 32 seconds of 3 minutes 32 seconds". */
    fun position(resources: Resources, positionMs: Long, durationMs: Long): String =
        resources.getString(R.string.player_seek_state, duration(resources, positionMs), duration(resources, durationMs))

    /** "3 of 5 stars" / "Not rated". */
    fun rating(resources: Resources, stars: Int): String =
        if (stars <= 0) resources.getString(R.string.rating_none) else resources.getQuantityString(R.plurals.a11y_rating_state, stars, stars)
}

@Composable
fun spokenDuration(ms: Long): String = Spoken.duration(androidx.compose.ui.platform.LocalResources.current, ms)
