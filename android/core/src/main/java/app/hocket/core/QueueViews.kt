package app.hocket.core

import app.hocket.core.api.QueueEntry
import app.hocket.core.api.QueueKey
import app.hocket.core.api.QueueView

/*
 * Reading the windowed queue view (see "Queue model" in docs/design.md). The list is
 * `history ++ [current] ++ upcoming`; `entries` holds the rows `windowStart until windowEnd`.
 * Upcoming starts with the "Next in queue" group (`groupCount` rows), then the rest.
 */

/** List index one past the last row in the window. */
val QueueView.windowEnd: Int get() = windowStart.toInt() + entries.size

/** List index of the first upcoming row (right after the current one, or after history when nothing is current). */
val QueueView.upcomingStart: Int get() = (currentIndex?.toInt()?.plus(1)) ?: historyCount.toInt()

/** List index one past the "Next in queue" group. */
val QueueView.groupEnd: Int get() = upcomingStart + groupCount.toInt()

/** The window's row at list index [index], if loaded. */
fun QueueView.entryAt(index: Int): QueueEntry? = entries.getOrNull(index - windowStart.toInt())

/** The list index of [key] when it is in the window, else -1. */
fun QueueView.indexOfKey(key: QueueKey): Int = entries.indexOfFirst { it.item.key == key }.let { if (it < 0) -1 else it + windowStart.toInt() }

private fun QueueView.slice(from: Int, to: Int): List<QueueEntry> {
    val a = (from - windowStart.toInt()).coerceIn(0, entries.size)
    val b = (to - windowStart.toInt()).coerceIn(a, entries.size)
    return entries.subList(a, b)
}

/** The loaded history rows (oldest first). */
val QueueView.historyEntries: List<QueueEntry> get() = slice(0, historyCount.toInt())

/** The loaded rows after the current one: the group, then the rest. */
val QueueView.upcomingEntries: List<QueueEntry> get() = slice(upcomingStart, Int.MAX_VALUE)

/** The loaded "Next in queue" group rows. */
val QueueView.groupEntries: List<QueueEntry> get() = slice(upcomingStart, groupEnd)

/** The loaded rows after the group. */
val QueueView.restEntries: List<QueueEntry> get() = slice(groupEnd, Int.MAX_VALUE)

/** What the next track is: the first upcoming row, or under Repeat All the wrap target. */
val QueueView.nextEntry: QueueEntry? get() = upcomingEntries.firstOrNull() ?: wrapNext

/** Paging the queue view with `Query.QueueRange`. */
object QueueWindows {
    /** Rows to ask for per page (one `QueueRange`). */
    const val PAGE = 100

    /** [a] and [b] describe the same list shape, so rows of one can stand in the other. */
    fun sameShape(a: QueueView, b: QueueView): Boolean =
        a.total == b.total && a.historyCount == b.historyCount && a.currentIndex == b.currentIndex && a.groupCount == b.groupCount

    /**
     * [base] with [page]'s rows spliced in when the two describe the same list and their windows
     * touch or overlap; [base]'s own rows win where both have one. Anything else returns [base].
     */
    fun merge(base: QueueView, page: QueueView): QueueView {
        if (!sameShape(base, page) || page.entries.isEmpty()) return base
        if (base.entries.isEmpty()) return base.copy(entries = page.entries, windowStart = page.windowStart)
        return splice(base, page)
    }

    /**
     * [base] (a new window from the core) keeping [prev]'s rows beyond it when the list itself is
     * unchanged and only the pointer moved: the same total, and every row both windows hold has
     * the same key. Null otherwise.
     */
    fun realign(base: QueueView, prev: QueueView): QueueView? {
        if (base.total != prev.total || base.entries.isEmpty() || prev.entries.isEmpty()) return null
        val from = maxOf(base.windowStart.toInt(), prev.windowStart.toInt())
        val to = minOf(base.windowEnd, prev.windowEnd)
        if (from >= to) return null
        for (i in from until to) if (base.entryAt(i)?.item?.key != prev.entryAt(i)?.item?.key) return null
        return splice(base, prev)
    }

    /** [base] with [page]'s rows outside it added where the two windows touch or overlap. */
    private fun splice(base: QueueView, page: QueueView): QueueView {
        val bs = base.windowStart.toInt()
        val be = base.windowEnd
        val ps = page.windowStart.toInt()
        val pe = page.windowEnd
        if (ps > be || pe < bs) return base
        if (ps >= bs && pe <= be) return base
        val keys = base.entries.mapTo(HashSet()) { it.item.key }
        val before = if (ps < bs) page.entries.subList(0, bs - ps).filter { it.item.key !in keys } else emptyList()
        val after = if (pe > be) page.entries.subList((be - ps).coerceAtLeast(0), page.entries.size).filter { it.item.key !in keys } else emptyList()
        if (before.isEmpty() && after.isEmpty()) return base
        return base.copy(entries = before + base.entries + after, windowStart = (bs - before.size).toUInt())
    }

    /**
     * The next page to load so the window covers [wantBefore] rows behind the pointer and
     * [wantAfter] ahead of it, as (start, count); null when it already does. Earlier rows first.
     */
    fun missing(view: QueueView, wantBefore: Int, wantAfter: Int): Pair<Int, Int>? {
        val total = view.total.toInt()
        if (total == 0) return null
        val pointer = view.currentIndex?.toInt() ?: view.historyCount.toInt()
        val from = (pointer - wantBefore).coerceIn(0, total)
        val to = (pointer + 1 + wantAfter).coerceIn(0, total)
        val ws = view.windowStart.toInt()
        val we = view.windowEnd
        if (view.entries.isEmpty()) return from to (to - from).coerceIn(1, Commands.QUEUE_RANGE_MAX)
        if (from < ws) { val start = maxOf(from, ws - PAGE); return start to (ws - start) }
        if (to > we) return we to minOf(to - we, PAGE)
        return null
    }
}

/**
 * A whole (unwindowed) queue view from its runs: [history], [current], the "Next in queue"
 * [group] and the [rest]. For previews, tests and anything that builds a queue by hand.
 */
fun queueViewOf(
    history: List<QueueEntry> = emptyList(),
    current: QueueEntry? = null,
    group: List<QueueEntry> = emptyList(),
    rest: List<QueueEntry> = emptyList(),
    contextLabel: String? = null,
    shuffle: Boolean = false,
    repeat: app.hocket.core.api.RepeatMode = app.hocket.core.api.RepeatMode.Off,
    autoplay: Boolean = false,
    wrapNext: QueueEntry? = null,
    previous: QueueEntry? = history.lastOrNull(),
): QueueView {
    val upcoming = group.map { it.copy(inGroup = true) } + rest
    val entries = history + listOfNotNull(current) + upcoming
    return QueueView(
        contextLabel = contextLabel, entries = entries, windowStart = 0u, total = entries.size.toUInt(), historyCount = history.size.toUInt(),
        currentIndex = current?.let { history.size.toUInt() }, groupCount = (if (current != null) group.size else 0).toUInt(),
        totalUpcoming = upcoming.size.toUInt(), current = current, shuffle = shuffle, repeat = repeat, autoplay = autoplay,
        wrapNext = wrapNext, previous = previous,
    )
}
