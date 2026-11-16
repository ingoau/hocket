package app.hocket.playback

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class YtmLoginTest {
    @Test fun knowsASignedInSessionBySapisid() {
        assertFalse(YtmLogin.hasSession(null))
        assertFalse(YtmLogin.hasSession("PREF=f6=8; YSC=x"))
        assertFalse(YtmLogin.hasSession("SAPISID="))
        assertTrue(YtmLogin.hasSession("SID=a; SAPISID=abc/def; HSID=b"))
        assertTrue(YtmLogin.hasSession("__Secure-3PAPISID=z"))
    }

    @Test fun anonymousIsAnEmptyPasswordUnderTheFixedKey() {
        val c = YtmLogin.credential(null)
        assertTrue(YtmLogin.isYtm(c))
        assertEquals("", c.password)
        assertEquals("SAPISID=x", YtmLogin.credential("SAPISID=x").password)
        assertFalse(YtmLogin.isYtm(ServerCredential("https://music.example", "ytm", "p", null)))
    }
}
