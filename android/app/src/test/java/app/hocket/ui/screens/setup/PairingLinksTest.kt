package app.hocket.ui.screens.setup

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class PairingLinksTest {
    @Test
    fun findsAPairingLinkInWhateverHoldsIt() {
        val link = "hocket://pair?d=eyJpZGVudGl0eSI6ImFiYyJ9_-x"
        assertEquals(link, PairingLinks.pair(link))
        assertEquals(link, PairingLinks.pair("Join me: $link"))
        assertNull(PairingLinks.pair("hocket://pair?d=short"))
        assertNull(PairingLinks.pair("hocket://setup?server=https%3A%2F%2Fx&user=ada"))
        assertNull(PairingLinks.pair(null))
    }

    @Test
    fun readsACoordinatorClaim() {
        assertEquals("wss://c.example/ws" to "ABCD-EFGH", PairingLinks.claim("hocket://claim?url=wss%3A%2F%2Fc.example%2Fws&code=ABCD-EFGH"))
        assertEquals("" to "ABCD-EFGH", PairingLinks.claim("hocket://claim?code=ABCD-EFGH"))
        assertNull(PairingLinks.claim("hocket://claim?url=https%3A%2F%2Fx&code=ABCD"))
        assertNull(PairingLinks.claim("hocket://claim?url=wss%3A%2F%2Fx"))
    }
}
