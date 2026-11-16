package app.hocket.ui

import app.hocket.ui.components.SheetPull
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class SheetPullTest {
    /** Brought up before the sheet is laid out, the travel is cut to its height, and a drag never takes more than it was given. */
    @Test
    fun theTravelStaysWithinTheSheetAndADragTakesNoMoreThanItsDelta() {
        val pull = SheetPull(CoroutineScope(Dispatchers.Unconfined))
        assertEquals(-600f, pull.drag(-600f), 0f)
        pull.height = 400f
        assertEquals(400f, pull.travel, 0f)
        assertEquals("already all the way up", 0f, pull.drag(-5f), 0f)
        assertEquals(5f, pull.drag(5f), 0f)
        assertEquals(0.9875f, pull.shown(), 1e-4f)
    }

    /** A pull its host took away stops following, and a drop-back cut short by its scope still closes the sheet. */
    @Test
    fun aDismissedPullStopsAndACancelledDropBackStillCloses() {
        val dismissed = SheetPull(CoroutineScope(Dispatchers.Unconfined))
        dismissed.drag(-50f)
        dismissed.dismiss()
        assertFalse(dismissed.held)
        assertEquals(0f, dismissed.drag(-10f), 0f)

        val scope = CoroutineScope(SupervisorJob() + Dispatchers.Unconfined)
        val pull = SheetPull(scope)
        var closed = false
        pull.onClosed = { closed = true }
        pull.drag(-50f)
        pull.release(open = false)
        scope.cancel()
        assertTrue("closed when the scope went", closed)
    }
}
