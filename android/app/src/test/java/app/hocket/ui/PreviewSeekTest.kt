package app.hocket.ui

import app.hocket.core.api.PreviewClip
import app.hocket.core.api.PreviewState
import app.hocket.ui.components.previewClipFraction
import app.hocket.ui.components.previewPositionMs
import org.junit.Assert.assertEquals
import org.junit.Test

class PreviewSeekTest {
    @Test
    fun positionExtrapolatesWhilePlayingAndHoldsWhilePaused() {
        assertEquals(3_500L, previewPositionMs(3_000, 1_000.0, true, 10_000, 1_500.0))
        assertEquals(3_000L, previewPositionMs(3_000, 1_000.0, false, 10_000, 9_000.0))
        assertEquals(10_000L, previewPositionMs(9_500, 0.0, true, 10_000, 2_000.0))
    }

    @Test
    fun aClipsRingFillsOverTheClipNotTheSong() {
        fun state(positionMs: Int, clip: PreviewClip?) = PreviewState(
            trackId = "t", title = "T", durationMs = 200_000u, positionMs = positionMs.toUInt(),
            playing = false, takenAt = 0.0, clip = clip,
        )
        val clip = PreviewClip(startMs = 60_000u, endMs = 90_000u)
        assertEquals(0f, previewClipFraction(state(60_000, clip), 0.0), 0.001f)
        assertEquals(0.5f, previewClipFraction(state(75_000, clip), 0.0), 0.001f)
        assertEquals(1f, previewClipFraction(state(95_000, clip), 0.0), 0.001f)
        // Without a clip, the whole song.
        assertEquals(0.5f, previewClipFraction(state(100_000, null), 0.0), 0.001f)
    }
}
