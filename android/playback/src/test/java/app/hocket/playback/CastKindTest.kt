package app.hocket.playback

import app.hocket.playback.CastDevices.Kind
import org.junit.Assert.assertEquals
import org.junit.Test

/** What a Cast receiver shows as: Google's devices by model name, others by capability. */
class CastKindTest {
    private fun kind(model: String?, group: Boolean = false, video: Boolean = false, audio: Boolean = true) =
        Kind.of(model, group, video, audio)

    @Test
    fun googleDevicesByModel() {
        assertEquals(Kind.NestMini, kind("Google Nest Mini"))
        assertEquals(Kind.HomeMini, kind("Google Home Mini"))
        assertEquals(Kind.NestAudio, kind("Google Nest Audio"))
        assertEquals(Kind.HomeMax, kind("Google Home Max"))
        assertEquals(Kind.NestHub, kind("Google Nest Hub", video = true))
        assertEquals(Kind.NestHub, kind("Google Home Hub", video = true))
        assertEquals(Kind.NestHubMax, kind("Google Nest Hub Max", video = true))
        assertEquals(Kind.Chromecast, kind("Chromecast", video = true))
        assertEquals(Kind.Chromecast, kind("Chromecast Audio"))
        assertEquals(Kind.Chromecast, kind("Chromecast Ultra", video = true))
        assertEquals(Kind.ChromecastGoogleTv, kind("Chromecast HD", video = true))
        assertEquals(Kind.ChromecastGoogleTv, kind("Google TV Streamer", video = true))
        assertEquals(Kind.WifiPoint, kind("Google Nest Wifi point"))
    }

    @Test
    fun groupsWhateverTheModel() {
        assertEquals(Kind.Group, kind("Google Cast Group"))
        assertEquals(Kind.Group, kind(null, group = true))
    }

    @Test
    fun othersByCapability() {
        assertEquals(Kind.Tv, kind("BRAVIA 4K VH2", video = true))
        assertEquals(Kind.Speaker, kind("JBL Link 20"))
        assertEquals(Kind.Speaker, kind("Google Home"))
        assertEquals(Kind.Unknown, kind(null, audio = false))
    }
}
