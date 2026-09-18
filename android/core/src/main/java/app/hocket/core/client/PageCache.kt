package app.hocket.core.client

import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Deferred
import kotlinx.coroutines.async
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock

/**
 * A small keyed page cache for virtualised lists.
 *
 * Lists are never paginated in the UI sense: `LazyColumn` asks for the row at index `i`, the cache
 * maps that to page `i / pageSize`, serves it from memory when present, and otherwise fetches it once
 * (in-flight pages are de-duplicated). [invalidate] drops everything for a query key prefix, which is
 * what `Event.LibraryChanged` triggers; rows visible on screen refetch on their next composition.
 *
 * `K` identifies the list (server, sort, filter...). Total counts come from a separate query and live
 * in [PagedList.total] so select-all can be a predicate with a count rather than a materialised list.
 */
class PageCache<K, T>(
    private val scope: CoroutineScope,
    val pageSize: Int = 60,
    private val fetch: suspend (key: K, offset: Int, limit: Int) -> Page<T>,
) {
    data class Page<T>(val items: List<T>, val offset: Int, val total: Int)

    /** Snapshot of everything loaded for one key. Exposed as state so lists recompose as pages land. */
    data class PagedList<T>(val pages: Map<Int, List<T>> = emptyMap(), val total: Int = -1, val generation: Int = 0) {
        fun item(index: Int, pageSize: Int): T? = pages[index / pageSize]?.getOrNull(index % pageSize)
        val known: Boolean get() = total >= 0
    }

    private val mutex = Mutex()
    private val inFlight = HashMap<Pair<K, Int>, Deferred<Unit>>()
    private val states = HashMap<K, MutableStateFlow<PagedList<T>>>()

    fun state(key: K): StateFlow<PagedList<T>> = flowFor(key).asStateFlow()

    private fun flowFor(key: K): MutableStateFlow<PagedList<T>> =
        synchronized(states) { states.getOrPut(key) { MutableStateFlow(PagedList()) } }

    /** Ensures the page containing [index] is loaded (or loading). Cheap to call from composition. */
    fun ensure(key: K, index: Int) {
        if (index < 0) return
        val page = index / pageSize
        val flow = flowFor(key)
        if (flow.value.pages.containsKey(page)) return
        val id = key to page
        scope.async {
            val job = mutex.withLock {
                inFlight[id] ?: async {
                    try {
                        val result = fetch(key, page * pageSize, pageSize)
                        flow.update { it.copy(pages = it.pages + (page to result.items), total = result.total) }
                    } catch (e: Exception) {
                        // Leave the page absent; the next ensure() retries. Nothing to surface: the core owns errors.
                    } finally {
                        mutex.withLock { inFlight.remove(id) }
                    }
                }.also { inFlight[id] = it }
            }
            job.await()
        }
    }

    /** Fetches the first page and returns the total. Used for counts and small lists. */
    suspend fun prime(key: K): PagedList<T> {
        ensure(key, 0)
        return state(key).value
    }

    /** Drops cached pages for every key matching [predicate] (all when null), bumping generations. */
    fun invalidate(predicate: ((K) -> Boolean)? = null) {
        synchronized(states) {
            for ((k, flow) in states) {
                if (predicate == null || predicate(k)) flow.update { PagedList(generation = it.generation + 1) }
            }
        }
    }
}
