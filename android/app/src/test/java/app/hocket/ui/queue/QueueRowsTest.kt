package app.hocket.ui.queue

import app.hocket.core.api.Command
import app.hocket.core.api.OfflineState
import app.hocket.core.api.QueueEntry
import app.hocket.core.api.QueueItem
import app.hocket.core.api.QueueSource
import app.hocket.core.api.QueueSourceAutoplayInner
import app.hocket.core.api.QueueSourceContextInner
import app.hocket.core.api.AutoplayProvider
import app.hocket.core.api.TrackSummary
import app.hocket.core.queueViewOf
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

/** The queue screen's sections, row markers and drag membership, as plain data. */
class QueueRowsTest {
    private fun entry(key: String, source: QueueSource = QueueSource.Context(QueueSourceContextInner(0u)), added: Boolean = false, autoplay: Boolean = false) = QueueEntry(
        QueueItem(key, "t-$key", source, added = added),
        TrackSummary(id = "t-$key", serverId = "s", title = key, durationMs = 1u, rating = 0u, loved = false, offline = OfflineState.None),
        autoplay = autoplay,
    )
    private val auto = QueueSource.Autoplay(QueueSourceAutoplayInner(AutoplayProvider.SonicSimilarity, "Similar", null))

    private val view = queueViewOf(
        history = listOf(entry("h0"), entry("h1", QueueSource.Inserted, added = true)),
        current = entry("c"),
        group = listOf(entry("g0", QueueSource.Inserted, added = true), entry("g1", QueueSource.Inserted, added = true)),
        rest = listOf(entry("r0"), entry("r1", QueueSource.Inserted, added = true), entry("a0", auto, autoplay = true)),
        contextLabel = "Album",
    )

    private fun shape(lines: List<QueueLine>) = lines.map { l ->
        when (l) {
            is QueueLine.Header -> "#" + l.id.name + (if (l.id == QueueLine.Header.Id.Group) ":" + l.count else "") + (l.label?.let { ":$it" } ?: "")
            is QueueLine.Item -> l.entry.item.key + "/" + l.section.name
            is QueueLine.More -> if (l.top) "more-top" else "more-bottom"
            QueueLine.Footer -> "footer"
        }
    }

    @Test
    fun sectionsAreHistoryNowPlayingTheGroupAndNextFrom() {
        assertEquals(
            listOf("#History", "h0/History", "h1/History", "#Now", "c/Current", "#Group:2", "g0/Group", "g1/Group",
                "#Later:Album", "r0/Later", "r1/Later", "a0/Later", "footer"),
            shape(queueLines(view)),
        )
    }

    @Test
    fun noGroupNoHeadingAndNoContextSaysLater() {
        val v = queueViewOf(current = entry("c"), rest = listOf(entry("x", QueueSource.Inserted, added = true)), contextLabel = "Album")
        assertEquals(listOf("#Now", "c/Current", "#Later", "x/Later", "footer"), shape(queueLines(v)))
    }

    @Test
    fun aWindowShowsWhereMoreRowsArePagedIn() {
        val full = view
        val window = full.copy(entries = full.entries.subList(1, 7), windowStart = 1u)
        assertEquals(listOf("more-top", "h1/History", "#Now", "c/Current", "#Group:2", "g0/Group", "g1/Group", "#Later:Album", "r0/Later", "r1/Later", "more-bottom"), shape(queueLines(window)))
    }

    @Test
    fun markersShowWhereSongsCameFrom() {
        val lines = queueLines(view).filterIsInstance<QueueLine.Item>().associate { it.entry.item.key to rowMarker(it.entry, it.section) }
        assertEquals(RowMarker.None, lines["h0"])
        assertEquals("added, in history", RowMarker.Added, lines["h1"])
        assertEquals(RowMarker.None, lines["c"])
        assertEquals("the heading says it", RowMarker.None, lines["g0"])
        assertEquals(RowMarker.None, lines["r0"])
        assertEquals("added, later on", RowMarker.Added, lines["r1"])
        assertEquals(RowMarker.Autoplay, lines["a0"])
    }

    @Test
    fun droppingAboveTheNextFromHeadingJoinsTheGroup() {
        val base = dragOrder(view, sectionOn = true)
        assertEquals(listOf("g0", "g1", LATER_HEADER_KEY, "r0", "r1", "a0"), base)
        // r1 dragged above the heading: after g1, in the group.
        val up = listOf("g0", "g1", "r1", LATER_HEADER_KEY, "r0", "a0")
        val join = dropMove(up, "r1", "c", base) as Command.MoveQueueItem
        assertEquals("g1", join.data.after)
        assertEquals(true, join.data.in_group)
        // g0 dragged below the heading: after it, the first of the rest, out of the group.
        val down = listOf("g1", LATER_HEADER_KEY, "g0", "r0", "r1", "a0")
        val leave = dropMove(down, "g0", "c", base) as Command.MoveQueueItem
        assertEquals("g1", leave.data.after)
        assertEquals(false, leave.data.in_group)
        // To the very top: after the current song.
        val top = listOf("r0", "g0", "g1", LATER_HEADER_KEY, "r1", "a0")
        val first = dropMove(top, "r0", "c", base) as Command.MoveQueueItem
        assertEquals("c", first.data.after)
        assertEquals(true, first.data.in_group)
        // Back where it was: nothing to send.
        assertNull(dropMove(base, "r0", "c", base))
    }

    @Test
    fun withTheSectionOffThereIsNoGroupToJoin() {
        val v = queueViewOf(current = entry("c"), rest = listOf(entry("r0"), entry("r1")))
        val base = dragOrder(v, sectionOn = false)
        assertEquals(listOf("r0", "r1"), base)
        val moved = dropMove(listOf("r1", "r0"), "r1", "c", base) as Command.MoveQueueItem
        assertEquals("c", moved.data.after)
        assertEquals(false, moved.data.in_group)
    }

    @Test
    fun movingOneStepCrossesTheHeadingToo() {
        val base = dragOrder(view, sectionOn = true)
        assertEquals(listOf("g0", "g1", "r0", LATER_HEADER_KEY, "r1", "a0"), stepOrder(base, "r0", up = true))
        assertNull(stepOrder(base, "g0", up = true))
        assertNull(stepOrder(base, "a0", up = false))
    }
}
