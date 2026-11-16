package app.hocket.cast

import app.hocket.cast.CastFailures.Action
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class CastFailuresTest {
    private val f = CastFailures()

    @Test
    fun navidromeFailingFirstIsTheServersReachThenTheItems() {
        assertEquals(Action.Unreachable, f.failed(ytm = false))
        f.playing(ytm = false)
        assertEquals(Action.Report, f.failed(ytm = false))
    }

    @Test
    fun youtubeMusicFailingFirstIsRetriedWithAFreshUrlThenExplained() {
        // The core fetches a fresh URL and retries.
        assertEquals(Action.Report, f.failed(ytm = true))
        // Still nothing: the receiver can't use this phone's URLs.
        assertEquals(Action.YtmUnreachable, f.failed(ytm = true))
    }

    @Test
    fun aMixedQueueKeepsPlayingNavidromeAndExplainsOnce() {
        f.playing(ytm = false)
        assertEquals(Action.Report, f.failed(ytm = true))
        assertEquals(Action.ReportWithYtmHint, f.failed(ytm = true))
        // Skipped by the core; later ones are just skipped.
        assertEquals(Action.Report, f.failed(ytm = true))
        assertEquals(Action.Report, f.failed(ytm = false))
    }

    @Test
    fun onceYoutubeMusicHasPlayedAFailureIsTheItems() {
        f.playing(ytm = true)
        assertEquals(Action.Report, f.failed(ytm = true))
        assertEquals(Action.Report, f.failed(ytm = true))
        f.reset()
        assertEquals(Action.Report, f.failed(ytm = true))
        assertEquals(Action.YtmUnreachable, f.failed(ytm = true))
    }

    @Test
    fun recognisesGooglevideoUrlsAndTheCause() {
        assertTrue(CastFailures.isYtm("https://rr1---sn-u1ntq5-5g.googlevideo.com/videoplayback?expire=1&ip=203.0.113.7"))
        assertFalse(CastFailures.isYtm("https://music.example/rest/stream?id=t1"))
        assertFalse(CastFailures.isYtm("https://googlevideo.com.evil.example/x"))
        assertFalse(CastFailures.isYtm("not a url"))
        assertEquals(CastFailures.Cause.Vpn, CastFailures.cause(cellular = true, vpn = true))
        assertEquals(CastFailures.Cause.MobileData, CastFailures.cause(cellular = true, vpn = false))
        assertEquals(CastFailures.Cause.Other, CastFailures.cause(cellular = false, vpn = false))
    }
}
