package app.hocket.ui

import app.hocket.ui.components.previewPositionMs
import app.hocket.ui.components.previewDragFraction
import app.hocket.ui.components.previewSeekLanded
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class PreviewSeekTest {
    @Test
    fun dragMovesTheProgressByTheDistanceNotToTheFinger() {
        assertEquals(0.3f, previewDragFraction(0.3f, 0f, 200f), 0.001f)
        assertEquals(0.4f, previewDragFraction(0.3f, 20f, 200f), 0.001f)
        assertEquals(0.2f, previewDragFraction(0.3f, -20f, 200f), 0.001f)
        assertEquals(1f, previewDragFraction(0.9f, 100f, 200f), 0.001f)
        assertEquals(0f, previewDragFraction(0.1f, -100f, 200f), 0.001f)
    }

    @Test
    fun aSeekHasLandedOnceTheCoreIsCloseToIt() {
        assertTrue(previewSeekLanded(0.5f, 0.505f, 30_000))
        assertFalse(previewSeekLanded(0.2f, 0.5f, 30_000))
        assertTrue(previewSeekLanded(0.2f, 0.5f, 0))
    }

    @Test
    fun positionExtrapolatesWhilePlayingAndHoldsWhilePaused() {
        assertEquals(3_500L, previewPositionMs(3_000, 1_000.0, true, 10_000, 1_500.0))
        assertEquals(3_000L, previewPositionMs(3_000, 1_000.0, false, 10_000, 9_000.0))
        assertEquals(10_000L, previewPositionMs(9_500, 0.0, true, 10_000, 2_000.0))
    }
}
