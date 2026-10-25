package app.hocket.core

import app.hocket.core.api.Album
import app.hocket.core.api.OfflineState
import app.hocket.core.api.ReleaseKind
import org.junit.Assert.assertEquals
import org.junit.Test

class ReleaseKindsTest {
    private fun album(id: String, types: List<String>?, compilation: Boolean = false) = Album(
        id = id, serverId = "s", name = id, songCount = 1u, durationMs = 0u, rating = 0u, loved = false, playCount = 0u,
        isCompilation = compilation, offline = OfflineState.None, releaseTypes = types,
    )

    @Test
    fun followsTheCore() {
        assertEquals(ReleaseKind.Compilation, album("a", listOf("ep"), compilation = true).releaseKind())
        assertEquals(ReleaseKind.Compilation, album("a", listOf("compilation")).releaseKind())
        assertEquals(ReleaseKind.Ep, album("a", listOf("single", "ep")).releaseKind())
        assertEquals(ReleaseKind.Single, album("a", listOf("single")).releaseKind())
        assertEquals(ReleaseKind.Album, album("a", listOf("live")).releaseKind())
        assertEquals(ReleaseKind.Album, album("a", null).releaseKind())
    }

    @Test
    fun groupsInShelfOrder() {
        val groups = listOf(album("1", listOf("single")), album("2", null), album("3", listOf("single"))).byReleaseKind()
        assertEquals(listOf(ReleaseKind.Album to listOf("2"), ReleaseKind.Single to listOf("1", "3")), groups.map { (k, a) -> k to a.map { it.id } })
    }
}
