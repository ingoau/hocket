package app.hocket.ui

import androidx.compose.ui.unit.Density
import app.hocket.core.fake.FakeLibrary
import app.hocket.ui.screens.detail.playlistEntries
import app.hocket.ui.screens.library.AlbumGridCells
import org.junit.Assert.assertEquals
import org.junit.Test

/** Keys and layout rules the content lists rely on to stay still while they load and change. */
class ContentListsTest {
    private val library = FakeLibrary(seed = 7)

    @Test
    fun playlistRowsAreKeyedByTrackAndOccurrenceNotPosition() {
        val (a, b) = library.tracks.take(2)
        val entries = playlistEntries(listOf(a, b, a))
        assertEquals(listOf("${a.id}#0", "${b.id}#0", "${a.id}#1"), entries.map { it.key })
        // Moving a row keeps every key: the dragged row is the same item before and after.
        val moved = entries.toMutableList().apply { add(0, removeAt(1)) }
        assertEquals(entries.map { it.key }.toSet(), moved.map { it.key }.toSet())
        // The committed order read back from the core gives the unique rows the same keys again.
        assertEquals("${b.id}#0", playlistEntries(moved.map { it.track }).first().key)
    }

    @Test
    fun albumGridsAreTwoColumnsOnPhonesAndAdaptiveWhenWider() {
        fun columns(widthDp: Int) = with(AlbumGridCells) { with(Density(1f)) { calculateCrossAxisCellSizes(widthDp, 0) } }.size
        assertEquals(2, columns(360))
        assertEquals(2, columns(411))
        assertEquals(800 / 150, columns(800))
    }
}
