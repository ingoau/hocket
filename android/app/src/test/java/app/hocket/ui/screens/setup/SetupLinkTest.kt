package app.hocket.ui.screens.setup

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class SetupLinkTest {
    @Test
    fun roundTripsEveryField() {
        val link = SetupLink("https://music.example.net/navidrome", "ada lovelace", "p&ss=w0rd+ü", "wss://coord.example.net")
        val text = link.format()
        assertEquals("hocket://setup?server=https%3A%2F%2Fmusic.example.net%2Fnavidrome&user=ada%20lovelace&password=p%26ss%3Dw0rd%2B%C3%BC&coordinator=wss%3A%2F%2Fcoord.example.net", text)
        assertEquals(link, SetupLink.parse(text))
    }

    @Test
    fun passwordAndCoordinatorAreOptional() {
        val link = SetupLink("https://music.example.net", "ada")
        assertEquals("hocket://setup?server=https%3A%2F%2Fmusic.example.net&user=ada", link.format())
        assertEquals(link, SetupLink.parse(link.format()))
    }

    @Test
    fun readsTheDesktopsForm() {
        // encodeURIComponent leaves some characters bare; the server is normalised as typed URLs are.
        val parsed = SetupLink.parse("  HOCKET://setup?user=ada&server=Https%3A%2F%2Fmusic.example.net%2F&extra=1&password=a'b!  ")
        assertEquals(SetupLink("https://music.example.net", "ada", "a'b!", null), parsed)
    }

    @Test
    fun rejectsWhatIsNotASetupLink() {
        assertNull(SetupLink.parse(null))
        assertNull(SetupLink.parse("https://music.example.net"))
        assertNull(SetupLink.parse("hocket://album/1"))
        assertNull(SetupLink.parse("hocket://setup?user=ada"))
        assertNull(SetupLink.parse("hocket://setup?server=ftp%3A%2F%2Fx&user=ada"))
        assertNull(SetupLink.parse("hocket://setup?server=https%3A%2F%2Fx&user=%20"))
        assertNull(SetupLink.parse("hocket://setup?server=https%3A%2F%2Fx&user=ada%ZZ"))
    }

    @Test
    fun carriesSettingsPacked() {
        val settings = """{"queue.savedCap":7,"nav.mobileBar":["home","search"]}"""
        val link = SetupLink("https://music.example.net", "ada", settings = settings)
        val text = link.format()
        val packed = text.substringAfter("&settings=")
        assertTrue("link-safe as it stands: $packed", packed.matches(Regex("[A-Za-z0-9_-]+")))
        assertEquals(link, SetupLink.parse(text))
    }

    @Test
    fun readsTheDesktopsPackedSettings() {
        // CompressionStream("deflate") and base64url, as desktop/src/shared/setup-settings.ts writes them.
        val parsed = SetupLink.parse("hocket://setup?server=https%3A%2F%2Fmusic.example.net&user=ada&settings=eJyrViosTS1N1StOLEtNcU4sULIy11HKSyzTy81PysxJdUosUrKKVsrIz01V0lEqTk0sSs5Qiq0FAABqEl0")
        assertEquals("""{"queue.savedCap":7,"nav.mobileBar":["home","search"]}""", parsed?.settings)
    }

    @Test
    fun settingsThatDoNotUnpackAreDroppedNotTheLink() {
        val notJson = SetupLink.packSettings("[1]")
        for (bad in listOf("%%%", "AAAA", notJson)) {
            assertEquals(bad, SetupLink("https://x", "ada"), SetupLink.parse("hocket://setup?server=https%3A%2F%2Fx&user=ada&settings=$bad"))
        }
    }

    @Test
    fun toStringHidesThePassword() {
        assertFalse(SetupLink("https://x", "ada", "hunter2").toString().contains("hunter2"))
    }
}
