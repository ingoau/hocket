package app.hocket.ui.player

import androidx.compose.ui.graphics.Color
import org.junit.Assert.assertEquals
import org.junit.Test

/**
 * Issue #147: after a skip the full player showed the last song's cover until the new one's layout
 * and colours were worked out. The held scene keeps only its style; the cover is always the
 * current track's.
 */
class SceneShownTest {
    private val last = Scene("cover-a", layout = null, seed = Color.Red)
    private val current = Scene("cover-b", layout = null, seed = null)

    @Test
    fun whileHoldingTheLastStyleWrapsTheCurrentCover() {
        val shown = sceneShown(current, ready = false, held = last, holding = true)
        assertEquals("cover-b", shown.coverArt)
        assertEquals(Color.Red, shown.seed)
    }

    @Test
    fun readyShowsTheCurrentScene() {
        val ready = current.copy(seed = Color.Blue)
        assertEquals(ready, sceneShown(ready, ready = true, held = last, holding = true))
    }

    @Test
    fun noHoldShowsTheCurrentCoverAsItIs() {
        assertEquals(current, sceneShown(current, ready = false, held = last, holding = false))
        assertEquals(current, sceneShown(current, ready = false, held = null, holding = true))
    }

    @Test
    fun nothingPlayingShowsNoCover() {
        val none = Scene(null, layout = null, seed = null)
        assertEquals(null, sceneShown(none, ready = false, held = last, holding = true).coverArt)
    }
}
