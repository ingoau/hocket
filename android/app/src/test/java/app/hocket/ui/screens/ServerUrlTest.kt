package app.hocket.ui.screens

import app.hocket.ui.screens.setup.normaliseServerUrl
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class ServerUrlTest {
    @Test
    fun theSchemeIsAcceptedInAnyCaseAndLowerCased() {
        assertEquals("https://music.example.net", normaliseServerUrl("Https://music.example.net"))
        assertEquals("https://music.example.net", normaliseServerUrl("HTTPS://music.example.net"))
        assertEquals("http://10.0.0.2:4533", normaliseServerUrl("hTTp://10.0.0.2:4533"))
    }

    @Test
    fun whitespaceAndTrailingSlashesAreDropped() {
        assertEquals("https://music.example.net/navidrome", normaliseServerUrl("  https://music.example.net/navidrome// "))
    }

    @Test
    fun anAddressWithoutASchemeOrHostIsRejected() {
        assertNull(normaliseServerUrl("music.example.net"))
        assertNull(normaliseServerUrl("ftp://music.example.net"))
        assertNull(normaliseServerUrl("https://"))
        assertNull(normaliseServerUrl(""))
    }
}
