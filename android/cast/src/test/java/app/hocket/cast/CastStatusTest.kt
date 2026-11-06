package app.hocket.cast

import com.google.android.gms.cast.MediaStatus
import org.junit.Assert.assertEquals
import org.junit.Test

class CastStatusTest {
    private val s = CastStatus()

    @Test
    fun aLoadReportsReadyOnceThenEachChangeOnce() {
        s.loading("k1")
        // The load already said it is buffering.
        assertEquals(emptyList<CastStatus.Event>(), s.update("k1", MediaStatus.PLAYER_STATE_LOADING, 0))
        assertEquals(listOf(CastStatus.Event.Ready, CastStatus.Event.Playing), s.update("k1", MediaStatus.PLAYER_STATE_PLAYING, 0))
        assertEquals(emptyList<CastStatus.Event>(), s.update("k1", MediaStatus.PLAYER_STATE_PLAYING, 0))
        assertEquals(listOf(CastStatus.Event.Paused), s.update("k1", MediaStatus.PLAYER_STATE_PAUSED, 0))
        assertEquals(listOf(CastStatus.Event.Buffering), s.update("k1", MediaStatus.PLAYER_STATE_BUFFERING, 0))
        assertEquals(listOf(CastStatus.Event.Playing), s.update("k1", MediaStatus.PLAYER_STATE_PLAYING, 0))
    }

    @Test
    fun finishingEndsOnceAndAnErrorFails() {
        s.loading("k1")
        s.update("k1", MediaStatus.PLAYER_STATE_PLAYING, 0)
        assertEquals(listOf(CastStatus.Event.Ended), s.update("k1", MediaStatus.PLAYER_STATE_IDLE, MediaStatus.IDLE_REASON_FINISHED))
        assertEquals(emptyList<CastStatus.Event>(), s.update("k1", MediaStatus.PLAYER_STATE_IDLE, MediaStatus.IDLE_REASON_FINISHED))
        s.loading("k2")
        assertEquals(listOf(CastStatus.Event.Failed), s.update("k2", MediaStatus.PLAYER_STATE_IDLE, MediaStatus.IDLE_REASON_ERROR))
        // Replaced by a load: nothing about the item.
        s.loading("k3")
        assertEquals(emptyList<CastStatus.Event>(), s.update("k3", MediaStatus.PLAYER_STATE_IDLE, MediaStatus.IDLE_REASON_INTERRUPTED))
    }

    @Test
    fun aPreloadedFollowUpNeedsNoReady() {
        s.loading("k1")
        s.update("k1", MediaStatus.PLAYER_STATE_PLAYING, 0)
        s.transitioned("k2")
        assertEquals(listOf(CastStatus.Event.Playing), s.update("k2", MediaStatus.PLAYER_STATE_PLAYING, 0))
    }

    @Test
    fun aTranscodeStartsAtItsOffsetInWholeSeconds() {
        val url = "https://srv/rest/stream?id=t0&format=mp3"
        assertEquals(url, CastRenderer.streamUrl(url, 0))
        assertEquals("$url&timeOffset=42", CastRenderer.streamUrl(url, 42_000))
        assertEquals("https://srv/s?timeOffset=1", CastRenderer.streamUrl("https://srv/s", 1_999))
    }
}
