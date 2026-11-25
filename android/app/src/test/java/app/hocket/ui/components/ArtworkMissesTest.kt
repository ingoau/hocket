package app.hocket.ui.components

import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Before
import org.junit.Test

/** One retry schedule per cover, shared by every place showing it, and none while offline. */
class ArtworkMissesTest {
    private val online = ArtworkReach(online = true, reachable = 1)
    private val offline = ArtworkReach(online = false, reachable = 0)

    @Before fun setUp() = ArtworkMisses.clear()
    @After fun tearDown() = ArtworkMisses.clear()

    @Test
    fun anUnknownCoverIsAskedAtOnce() {
        assertEquals(0L, ArtworkMisses.waitFor("a@320", online = true, now = 0))
        assertEquals(0L, ArtworkMisses.waitFor("a@320", online = false, now = 0))
    }

    @Test
    fun placesSharingOneAnswerCountOneMissAndBackOffTogether() {
        ArtworkMisses.observe(online)
        // Three places got the same null answer at about the same time.
        repeat(3) { ArtworkMisses.missed("a@320", round = 0, now = 1_000) }
        assertEquals(1, ArtworkMisses.round("a@320"))
        assertEquals(ARTWORK_RETRY_MS, ArtworkMisses.waitFor("a@320", true, now = 1_000))
        // Asked again when due, and missed again: the wait doubles.
        ArtworkMisses.missed("a@320", round = 1, now = 1_000 + ARTWORK_RETRY_MS)
        assertEquals(ARTWORK_RETRY_MS * 2, ArtworkMisses.waitFor("a@320", true, now = 1_000 + ARTWORK_RETRY_MS))
    }

    @Test
    fun retriesRunOut() {
        var now = 0L
        repeat(ARTWORK_RETRIES + 1) { round ->
            now += 60_000
            ArtworkMisses.missed("a@320", round, now)
        }
        assertNull(ArtworkMisses.waitFor("a@320", true, now))
    }

    @Test
    fun offlineAKnownMissIsNotAskedAndComingBackOnlineAsksAfresh() {
        ArtworkMisses.observe(online)
        ArtworkMisses.missed("a@320", round = 0, now = 0)
        ArtworkMisses.observe(offline)
        assertNull(ArtworkMisses.waitFor("a@320", online = false, now = 60_000))
        // Metered or network-id changes aren't a reach change; only coming back is.
        ArtworkMisses.observe(online)
        assertEquals(0L, ArtworkMisses.waitFor("a@320", online = true, now = 60_000))
    }

    @Test
    fun aCoverThatShowedIsAskedForAgainOfflineAndStartsAfresh() {
        ArtworkMisses.observe(online)
        ArtworkMisses.missed("a@320", round = 0, now = 0)
        // Asked again, its file came and loaded.
        ArtworkMisses.forget("a@320")
        assertEquals(0L, ArtworkMisses.waitFor("a@320", online = false, now = 0))
        assertEquals(0, ArtworkMisses.round("a@320"))
        // Places still holding the old round don't count a later failure twice.
        ArtworkMisses.missed("a@320", round = 0, now = 0)
        ArtworkMisses.missed("a@320", round = 0, now = 0)
        assertEquals(1, ArtworkMisses.round("a@320"))
    }

    @Test
    fun aServerComingBackAsksAfresh() {
        ArtworkMisses.observe(ArtworkReach(true, 0))
        ArtworkMisses.missed("a@320", round = 0, now = 0)
        ArtworkMisses.observe(ArtworkReach(true, 1))
        assertEquals(0L, ArtworkMisses.waitFor("a@320", true, now = 0))
    }
}
