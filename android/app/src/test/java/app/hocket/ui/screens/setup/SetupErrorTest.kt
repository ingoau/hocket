package app.hocket.ui.screens.setup

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/** Which core errors the setup form shows, and how. */
class SetupErrorTest {
    @Test
    fun anotherAccountsBackgroundErrorIsNotTheForms() {
        assertFalse(isSetupError("other", attempting = false, formAccount = "mine"))
        assertFalse(isSetupError("other", attempting = false, formAccount = null))
        assertTrue(isSetupError("mine", attempting = false, formAccount = "mine"))
        assertTrue(isSetupError(null, attempting = false, formAccount = "mine"))
        // Signing in: the new account's id isn't known yet, so any error is the attempt's.
        assertTrue(isSetupError("new", attempting = true, formAccount = null))
    }

    @Test
    fun theCoresDetailStaysUnderTheFriendlyWords() {
        assertEquals("Can't reach the server\ncertificate has expired", setupErrorText("Can't reach the server", "network error", "certificate has expired"))
        assertEquals("Can't reach the server", setupErrorText("Can't reach the server", "network error", null))
        assertEquals("network error\ntimed out", setupErrorText(null, "network error", "timed out"))
        assertEquals("network error", setupErrorText(null, "network error", " "))
    }
}
