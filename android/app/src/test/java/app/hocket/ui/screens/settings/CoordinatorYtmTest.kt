package app.hocket.ui.screens.settings

import app.hocket.core.api.Command
import app.hocket.core.api.CommandSignCoordinatorInToYoutubeMusicInner
import app.hocket.core.api.EventCoordinatorYtmStatusInner
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class CoordinatorYtmTest {
    private val on = EventCoordinatorYtmStatusInner(enabled = true, signed_in = false)

    @Test
    fun theRowIsTheOwnersAndOnlyWhenTheCoordinatorRunsTheAnalysis() {
        assertNull("not the owner", CoordinatorYtm.row(owner = false, status = on))
        assertNull("not asked yet", CoordinatorYtm.row(owner = true, status = null))
        assertNull("--analysis-ytm off", CoordinatorYtm.row(owner = true, status = on.copy(enabled = false, error = "off")))
        assertEquals(CoordinatorYtmRow(signedIn = false, account = null, error = null), CoordinatorYtm.row(true, on))
        assertEquals(
            CoordinatorYtmRow(signedIn = true, account = "Coordinator", error = null),
            CoordinatorYtm.row(true, on.copy(signed_in = true, account = "Coordinator", error = "")),
        )
        assertEquals("refused", CoordinatorYtm.row(true, on.copy(error = "refused"))?.error)
    }

    @Test
    fun aFinishedSignInSendsTheFreshSessionAndSignOutSendsNone() {
        assertEquals(
            Command.SignCoordinatorInToYoutubeMusic(CommandSignCoordinatorInToYoutubeMusicInner("SID=a; SAPISID=b")),
            CoordinatorYtm.signIn(ok = true, cookies = "SID=a; SAPISID=b"),
        )
        assertNull("cancelled", CoordinatorYtm.signIn(ok = false, cookies = "SID=a; SAPISID=b"))
        assertNull("no session", CoordinatorYtm.signIn(ok = true, cookies = null))
        assertNull("blank", CoordinatorYtm.signIn(ok = true, cookies = " "))
        assertEquals(Command.SignCoordinatorInToYoutubeMusic(CommandSignCoordinatorInToYoutubeMusicInner(null)), CoordinatorYtm.signOut)
    }
}
