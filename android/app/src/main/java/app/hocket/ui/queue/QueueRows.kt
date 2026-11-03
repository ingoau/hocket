package app.hocket.ui.queue

import app.hocket.core.Commands
import app.hocket.core.api.Command
import app.hocket.core.api.QueueEntry
import app.hocket.core.api.QueueKey
import app.hocket.core.api.QueueSource
import app.hocket.core.api.QueueView
import app.hocket.core.groupEnd
import app.hocket.core.groupEntries
import app.hocket.core.historyEntries
import app.hocket.core.restEntries
import app.hocket.core.windowEnd

/*
 * The queue screen's rows, as plain data (see "Queue model" in docs/design.md): History, Now
 * playing, the "Next in queue" group under its own heading, then "Next from: <context>" (or
 * "Later"). Pure, so the sections, markers and drag membership are tested without Compose.
 */

/** LazyColumn key of the "Next from" heading, which is also the drop target dividing the group from the rest. */
internal const val LATER_HEADER_KEY = "hdr:u"

/**
 * LazyColumn key of the "more rows below" row. When the group runs past the loaded rows it also
 * ends the drag order in place of the heading: every loaded row there is in the group, and
 * nothing can be moved past it.
 */
internal const val MORE_BOTTOM_KEY = "more:bottom"

/** LazyColumn key of the "more rows above" row. */
internal const val MORE_TOP_KEY = "more:top"

internal sealed interface QueueLine {
    /** The list key (entries are keyed by queue key, headings by `hdr:<id>`). */
    val key: String

    data class Header(val id: Id, val count: Int = 0, val label: String? = null) : QueueLine {
        enum class Id(val tag: String) { History("h"), Now("c"), Group("g"), Later("u") }
        override val key: String get() = "hdr:" + id.tag
    }

    data class Item(val entry: QueueEntry, val section: Section) : QueueLine {
        override val key: String get() = entry.item.key
    }

    /** More rows exist above or below what is loaded; showing it pages them in. */
    data class More(val top: Boolean) : QueueLine {
        override val key: String get() = if (top) MORE_TOP_KEY else MORE_BOTTOM_KEY
    }

    data object Footer : QueueLine {
        override val key: String get() = "footer"
    }
}

internal enum class Section { History, Current, Group, Later }

/** What sits at the end of a row: nothing, the queue icon for an added song outside the group, or autoplay's icon. */
internal enum class RowMarker { None, Added, Autoplay }

internal fun rowMarker(entry: QueueEntry, section: Section): RowMarker = when {
    section == Section.Current || section == Section.Group -> RowMarker.None
    entry.autoplay == true || entry.item.source is QueueSource.Autoplay -> RowMarker.Autoplay
    entry.item.added == true && entry.inGroup != true -> RowMarker.Added
    else -> RowMarker.None
}

/**
 * The "Next from" heading's context name: the queue's context while any loaded row after the
 * group came from it; null ("Later") when nothing there has a single source.
 */
internal fun laterLabel(view: QueueView): String? =
    view.contextLabel?.takeIf { it.isNotBlank() && view.restEntries.any { e -> e.item.source is QueueSource.Context } }

/** The "Next in queue" group runs on past the loaded rows (its end, and the "Next from" heading, are not loaded yet). */
internal val QueueView.groupCut: Boolean get() = groupCount.toInt() > 0 && groupEnd > windowEnd

/**
 * Dropping above the "Next from" heading joins the group: with the Queue section setting on, or
 * when a group is already there (a Connect peer's, or from before the setting went off), which the
 * core keeps and shows whatever the setting.
 */
internal fun groupDrops(view: QueueView, sectionOn: Boolean): Boolean = sectionOn || view.groupCount.toInt() > 0

/**
 * The drag order: the group's keys, the "Next from" heading (when [groupDrops]: dropping above
 * it joins the group), then the rest's keys. While the group runs past the loaded rows
 * ([groupCut]) it is the group's keys and [MORE_BOTTOM_KEY]: everything loaded stays in the group.
 */
internal fun dragOrder(view: QueueView, sectionOn: Boolean): List<String> {
    val group = view.groupEntries.map { it.item.key }
    if (view.groupCut) return group + MORE_BOTTOM_KEY
    return group + (if (groupDrops(view, sectionOn)) listOf(LATER_HEADER_KEY) else emptyList()) + view.restEntries.map { it.item.key }
}

/** Where the group ends in a drag [order]: the heading's index, or the cut group's end marker's; -1 for neither. */
private fun groupSplit(order: List<String>): Int =
    order.indexOf(LATER_HEADER_KEY).takeIf { it >= 0 } ?: order.indexOf(MORE_BOTTOM_KEY)

private fun isMarker(key: String) = key == LATER_HEADER_KEY || key == MORE_BOTTOM_KEY

/**
 * The move that puts [key] where it sits in [order]: after the entry before it (the current one
 * at the very top) and in the group when above the "Next from" heading. Null when there is
 * nothing to do: no current entry, a key not in [order], or the place it already had in [base].
 */
internal fun dropMove(order: List<String>, key: QueueKey, currentKey: QueueKey?, base: List<String>): Command? {
    currentKey ?: return null
    val target = placement(order, key, currentKey) ?: return null
    if (target == placement(base, key, currentKey)) return null
    return Commands.moveQueueItem(key, target.first, target.second)
}

/** [key]'s (after, inGroup) in [order]. */
private fun placement(order: List<String>, key: QueueKey, currentKey: QueueKey): Pair<QueueKey, Boolean>? {
    val at = order.indexOf(key)
    if (at < 0) return null
    val header = groupSplit(order)
    val inGroup = header >= 0 && at < header
    val after = order.subList(0, at).lastOrNull { !isMarker(it) } ?: currentKey
    return after to inGroup
}

/** [order] with [key] moved one place up ([up]) or down, past the heading too; null at either end. */
internal fun stepOrder(order: List<String>, key: QueueKey, up: Boolean): List<String>? {
    val at = order.indexOf(key)
    val to = if (up) at - 1 else at + 1
    if (at < 0 || to !in order.indices || order[to] == MORE_BOTTOM_KEY) return null
    return order.toMutableList().apply { add(to, removeAt(at)) }
}

/**
 * The rows for [view]; with a [drag] order, the group and the rest as the finger has put them
 * (the heading shows where the group ends).
 */
internal fun queueLines(view: QueueView, drag: List<String>? = null, sectionOn: Boolean = true): List<QueueLine> = buildList {
    val history = view.historyEntries
    if (view.windowStart.toInt() > 0) add(QueueLine.More(top = true))
    else if (view.historyCount.toInt() > 0) add(QueueLine.Header(QueueLine.Header.Id.History))
    history.forEach { add(QueueLine.Item(it, Section.History)) }
    view.current?.let { add(QueueLine.Header(QueueLine.Header.Id.Now)); add(QueueLine.Item(it, Section.Current)) }
    var group = view.groupEntries
    var rest = view.restEntries
    if (drag != null) {
        val byKey = (group + rest).associateBy { it.item.key }
        val header = groupSplit(drag)
        val split = if (header >= 0) header else group.size.coerceAtMost(drag.size)
        group = drag.subList(0, split).mapNotNull { byKey[it] }
        rest = drag.subList((if (header >= 0) header + 1 else split), drag.size).mapNotNull { byKey[it] }
    }
    // A row dragged above the "Next from" heading starts a group: it shows (with its heading)
    // under the finger, so the dragged row keeps its place in the list. The count is the whole
    // group's, give or take the dragged row.
    if (view.groupCount.toInt() > 0 || (drag != null && group.isNotEmpty())) {
        val count = view.groupCount.toInt() + if (drag != null) group.size - view.groupEntries.size else 0
        add(QueueLine.Header(QueueLine.Header.Id.Group, count = count))
        group.forEach { add(QueueLine.Item(it, Section.Group)) }
    }
    val moreAfter = view.windowEnd < view.total.toInt()
    // The heading is also where a drag leaves the group, so it stays while a drag is under way;
    // while the group runs past the loaded rows it is not here yet (it would sit mid-group).
    if (!view.groupCut && (rest.isNotEmpty() || moreAfter || (drag != null && groupDrops(view, sectionOn) && view.current != null))) {
        add(QueueLine.Header(QueueLine.Header.Id.Later, label = laterLabel(view)))
        rest.forEach { add(QueueLine.Item(it, Section.Later)) }
    }
    if (moreAfter) add(QueueLine.More(top = false))
    else if (isNotEmpty()) add(QueueLine.Footer)
}
