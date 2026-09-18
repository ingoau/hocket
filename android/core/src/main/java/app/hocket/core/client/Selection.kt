package app.hocket.core.client

/**
 * Multi-selection keyed by id, with select-all as a predicate over the current filter rather than a
 * materialised id list (design.md, Lists). `count` is what the toolbar shows: for an inverted
 * selection it needs the list's total, which comes from `TrackCount`/`AlbumCount`.
 *
 * Immutable; every operation returns a new value so Compose state sees the change.
 */
data class Selection(
    /** When false: [ids] are selected. When true: everything matching the filter except [ids]. */
    val inverted: Boolean = false,
    val ids: Set<String> = emptySet(),
    /** Total rows in the list the selection is over; needed to count an inverted selection. */
    val total: Int = 0,
) {
    val active: Boolean get() = inverted || ids.isNotEmpty()
    val count: Int get() = if (inverted) (total - ids.size).coerceAtLeast(0) else ids.size
    val isAll: Boolean get() = inverted && ids.isEmpty()

    fun contains(id: String): Boolean = if (inverted) id !in ids else id in ids

    fun toggle(id: String): Selection = copy(ids = if (id in ids) ids - id else ids + id)
    fun add(ids: Collection<String>): Selection = if (inverted) copy(ids = this.ids - ids.toSet()) else copy(ids = this.ids + ids)
    fun selectAll(total: Int): Selection = Selection(inverted = true, ids = emptySet(), total = total)
    fun clear(): Selection = Selection(total = total)
    fun withTotal(total: Int): Selection = copy(total = total)

    /** Materialises the selection against a known id list (for ad-hoc contexts and small lists). */
    fun resolve(all: List<String>): List<String> = if (inverted) all.filter { it !in ids } else all.filter { it in ids }
}
