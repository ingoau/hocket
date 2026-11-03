package app.hocket.ui.queue

import app.hocket.core.Commands
import app.hocket.core.api.Command
import app.hocket.core.api.QueueEntry
import app.hocket.core.api.QueueKey
import app.hocket.core.api.QueueSource
import app.hocket.core.api.QueueView
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
        override val key: String get() = if (top) "more:top" else "more:bottom"
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

/**
 * The drag order: the group's keys, the "Next from" heading (only with the Queue section setting
 * on, when dropping above it joins the group), then the rest's keys.
 */
internal fun dragOrder(view: QueueView, sectionOn: Boolean): List<String> =
    view.groupEntries.map { it.item.key } + (if (sectionOn) listOf(LATER_HEADER_KEY) else emptyList()) + view.restEntries.map { it.item.key }

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
    val header = order.indexOf(LATER_HEADER_KEY)
    val inGroup = header >= 0 && at < header
    val after = order.subList(0, at).lastOrNull { it != LATER_HEADER_KEY } ?: currentKey
    return after to inGroup
}

/** [order] with [key] moved one place up ([up]) or down, past the heading too; null at either end. */
internal fun stepOrder(order: List<String>, key: QueueKey, up: Boolean): List<String>? {
    val at = order.indexOf(key)
    val to = if (up) at - 1 else at + 1
    if (at < 0 || to !in order.indices) return null
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
        val header = drag.indexOf(LATER_HEADER_KEY)
        val split = if (header >= 0) header else group.size.coerceAtMost(drag.size)
        group = drag.subList(0, split).mapNotNull { byKey[it] }
        rest = drag.subList((if (header >= 0) header + 1 else split), drag.size).mapNotNull { byKey[it] }
    }
    // A drag never adds or drops the heading under the finger: the core's answer does that.
    if (view.groupCount.toInt() > 0) {
        add(QueueLine.Header(QueueLine.Header.Id.Group, count = if (drag != null) group.size else view.groupCount.toInt()))
        group.forEach { add(QueueLine.Item(it, Section.Group)) }
    }
    val moreAfter = view.windowEnd < view.total.toInt()
    // The heading is also where a drag leaves the group, so it stays while a drag is under way.
    if (rest.isNotEmpty() || moreAfter || (drag != null && sectionOn && view.current != null)) {
        add(QueueLine.Header(QueueLine.Header.Id.Later, label = laterLabel(view)))
        rest.forEach { add(QueueLine.Item(it, Section.Later)) }
    }
    if (moreAfter) add(QueueLine.More(top = false))
    else if (isNotEmpty()) add(QueueLine.Footer)
}
