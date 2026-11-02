package app.hocket.ui

import app.hocket.ui.components.previewPositionMs
import app.hocket.ui.components.previewSeekFraction
import org.junit.Assert.assertEquals
import org.junit.Test

class PreviewSeekTest {
    @Test
    fun seekStaysInsideTheRowAndClearOfTheEdges() {
        assertEquals(0f, previewSeekFraction(0f, 200f, 24f), 0.001f)
        assertEquals(0f, previewSeekFraction(24f, 200f, 24f), 0.001f)
        assertEquals(1f, previewSeekFraction(200f, 200f, 24f), 0.001f)
        assertEquals(0.5f, previewSeekFraction(100f, 200f, 24f), 0.001f)
    }

    @Test
    fun positionExtrapolatesWhilePlayingAndHoldsWhilePaused() {
        assertEquals(3_500L, previewPositionMs(3_000, 1_000.0, true, 10_000, 1_500.0))
        assertEquals(3_000L, previewPositionMs(3_000, 1_000.0, false, 10_000, 9_000.0))
        assertEquals(10_000L, previewPositionMs(9_500, 0.0, true, 10_000, 2_000.0))
    }
}
