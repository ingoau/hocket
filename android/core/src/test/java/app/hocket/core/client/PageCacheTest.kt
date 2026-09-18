package app.hocket.core.client

import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.test.advanceUntilIdle
import kotlinx.coroutines.test.runTest
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

@OptIn(ExperimentalCoroutinesApi::class)
class PageCacheTest {
    @Test
    fun fetchesEachPageOnceAndServesByIndex() = runTest {
        var fetches = 0
        val cache = PageCache<String, Int>(this, pageSize = 10) { _, offset, limit ->
            fetches++
            PageCache.Page((offset until (offset + limit).coerceAtMost(25)).toList(), offset, 25)
        }
        cache.ensure("k", 0)
        cache.ensure("k", 5)
        cache.ensure("k", 12)
        advanceUntilIdle()
        val state = cache.state("k").value
        assertEquals(2, fetches)
        assertEquals(25, state.total)
        assertEquals(5, state.item(5, 10))
        assertEquals(12, state.item(12, 10))
        assertNull(state.item(22, 10))
    }

    @Test
    fun invalidateDropsPagesAndBumpsGeneration() = runTest {
        val cache = PageCache<String, Int>(this, pageSize = 10) { _, offset, limit -> PageCache.Page((offset until offset + limit).toList(), offset, 100) }
        cache.ensure("a", 0); cache.ensure("b", 0)
        advanceUntilIdle()
        cache.invalidate { it == "a" }
        assertEquals(0, cache.state("a").value.pages.size)
        assertEquals(1, cache.state("a").value.generation)
        assertEquals(1, cache.state("b").value.pages.size)
        cache.ensure("a", 3)
        advanceUntilIdle()
        assertEquals(3, cache.state("a").value.item(3, 10))
    }

    @Test
    fun failedFetchLeavesPageAbsentForRetry() = runTest {
        var fail = true
        val cache = PageCache<String, Int>(this, pageSize = 10) { _, offset, limit ->
            if (fail) throw IllegalStateException("boom")
            PageCache.Page((offset until offset + limit).toList(), offset, 10)
        }
        cache.ensure("k", 0)
        advanceUntilIdle()
        assertEquals(0, cache.state("k").value.pages.size)
        fail = false
        cache.ensure("k", 0)
        advanceUntilIdle()
        assertEquals(0, cache.state("k").value.item(0, 10))
    }
}
