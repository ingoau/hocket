package app.hocket.ui

import androidx.compose.runtime.Composable
import androidx.compose.runtime.MutableState
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.staticCompositionLocalOf

/**
 * The last answer each screen query gave, so a page composed again (a pop back to it, a tab
 * revisited) starts from what it last showed instead of skeletons. Its artwork is then on screen
 * on the first frame, where a shared-element flight back to it needs it. The page still re-queries
 * as before and replaces the value; only the starting point changes. Bounded LRU, keyed by the
 * caller (always including the server or the item, so an account switch never shows another
 * account's rows).
 */
internal class ScreenCache(private val max: Int = 256) {
    private val map = object : LinkedHashMap<Any, Any?>(64, 0.75f, true) {
        override fun removeEldestEntry(eldest: MutableMap.MutableEntry<Any, Any?>?): Boolean = size > max
    }

    fun contains(key: Any): Boolean = map.containsKey(key)

    @Suppress("UNCHECKED_CAST")
    fun <T> get(key: Any): T? = map[key] as T?

    fun put(key: Any, value: Any?) { map[key] = value }

    /** A state that starts from the cached value (else [initial]) and writes every change back. */
    fun <T> state(key: Any, initial: () -> T): MutableState<T> {
        @Suppress("UNCHECKED_CAST")
        val start = if (contains(key)) map[key] as T else initial()
        return CachedState(mutableStateOf(start)) { put(key, it) }
    }

    private class CachedState<T>(private val inner: MutableState<T>, private val write: (T) -> Unit) : MutableState<T> {
        override var value: T
            get() = inner.value
            set(v) { inner.value = v; write(v) }
        override fun component1(): T = value
        override fun component2(): (T) -> Unit = { value = it }
    }
}

/** The shell's [ScreenCache]; null outside it (previews, screens composed alone in tests). */
internal val LocalScreenCache = staticCompositionLocalOf<ScreenCache?> { null }

/** [androidx.compose.runtime.mutableStateOf] remembered under [key], starting from the [ScreenCache]'s last value for it. */
@Composable
internal fun <T> rememberCachedState(key: Any, initial: () -> T): MutableState<T> {
    val cache = LocalScreenCache.current
    return remember(cache, key) { cache?.state(key, initial) ?: mutableStateOf(initial()) }
}
