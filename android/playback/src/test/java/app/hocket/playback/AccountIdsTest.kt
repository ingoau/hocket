package app.hocket.playback

import org.junit.Assert.assertEquals
import org.junit.Test

class AccountIdsTest {
    @Test
    fun matchesTheCoresDerivation() {
        // Same vector as hocket-core's `scope_key_normalises`.
        assertEquals("cb1d7475e3b86104", AccountIds.navidrome("HTTPS://Music.Example/", "bob"))
        assertEquals("cb1d7475e3b86104", AccountIds.navidrome(" https://music.example ", "bob"))
        assertEquals("https://music.example:4533/Navi/Drome", AccountIds.serverKey("HTTPS://Music.Example:4533/Navi/Drome/"))
    }
}
