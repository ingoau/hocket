package app.hocket.core.client

import app.hocket.core.api.*
import app.hocket.core.fake.FakeCore
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.launch
import kotlinx.coroutines.test.runCurrent
import kotlinx.coroutines.test.runTest
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertSame
import org.junit.Assert.assertTrue
import org.junit.Test

@OptIn(ExperimentalCoroutinesApi::class)
class LibraryPatchesTest {
    private var clock = 5_000_000.0

    private fun summary(id: String, rating: UInt = 0u, loved: Boolean = false) =
        TrackSummary(id, "s", "T $id", null, null, null, null, 1000u, null, rating, loved, OfflineState.None)

    private fun state(kind: LibraryItemKind, id: String, rating: UInt, loved: Boolean) = LibraryItemState(kind, id, rating, loved)

    @Test
    fun patchesOnlyTheNamedItemsOfTheirKind() {
        val items = listOf(state(LibraryItemKind.Track, "t1", 4u, true), state(LibraryItemKind.Album, "t2", 5u, true))
        val t1 = LibraryPatches.track(summary("t1"), items)
        assertEquals(4u, t1.rating)
        assertTrue(t1.loved)
        // Same id, other kind: untouched (and the same instance, so Compose skips it).
        val t2 = summary("t2")
        assertSame(t2, LibraryPatches.track(t2, items))
        val results = SearchResults("r", "q", listOf(summary("t0"), summary("t1")), emptyList(), emptyList(), emptyList(), false)
        val patched = LibraryPatches.search(results, items)
        assertEquals(listOf(0u, 4u), patched.tracks.map { it.rating })
    }

    @Test
    fun itemChangesPatchNowPlayingAndEveryLibraryChangeBumpsTheGeneration() = runTest {
        val core = FakeCore(seed = 3, timers = false, now = { clock }, dispatcher = Dispatchers.Unconfined)
        val client = CoreClient(core, backgroundScope, now = { clock })
        runCurrent()
        client.requestSnapshot()
        runCurrent()
        val np = client.nowPlaying.value
        assertNotNull(np)
        val id = np!!.track.id
        val gen = client.libraryGeneration.value
        val received = mutableListOf<EventLibraryItemsChangedInner>()
        backgroundScope.launch(Dispatchers.Unconfined) { client.libraryItemsChanged.collect { received += it } }
        runCurrent()

        client.onEvent(Event.LibraryItemsChanged(EventLibraryItemsChangedInner(np.track.serverId, listOf(state(LibraryItemKind.Track, id, 3u, !np.track.loved)), "other-device")))
        assertEquals(3u, client.nowPlaying.value!!.track.rating)
        assertEquals(!np.track.loved, client.nowPlaying.value!!.track.loved)
        client.queue.value.current?.let { assertEquals(3u, it.track.rating) }
        assertEquals("other-device", received.single().from_device)
        assertEquals("patched in place; the LibraryChanged that follows refetches", gen, client.libraryGeneration.value)

        // Two identical LibraryChanged payloads still count as two changes.
        val same = Event.LibraryChanged(EventLibraryChangedInner(np.track.serverId, listOf("tracks"), listOf(id)))
        client.onEvent(same)
        client.onEvent(same)
        assertEquals(gen + 2, client.libraryGeneration.value)
    }
}
