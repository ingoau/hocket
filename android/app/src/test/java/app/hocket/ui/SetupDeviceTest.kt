package app.hocket.ui

import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithTag
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onFirst
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.performScrollTo
import androidx.compose.ui.test.performTextInput
import androidx.compose.ui.test.performSemanticsAction
import androidx.test.ext.junit.runners.AndroidJUnit4
import app.hocket.AppLinks
import app.hocket.core.Commands
import app.hocket.core.Queries
import app.hocket.core.SettingKeys
import app.hocket.core.api.QueryResult
import app.hocket.playback.CoreHost
import app.hocket.playback.InMemoryCredentialStore
import app.hocket.playback.ServerCredential
import app.hocket.ui.nav.AppRoot
import app.hocket.ui.nav.qrMatrix
import app.hocket.ui.screens.setup.SetupLink
import kotlinx.coroutines.runBlocking
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode

/** Set up another device: the account sheet's QR code, and a setup link opened on a new device. */
@RunWith(AndroidJUnit4::class)
@Config(sdk = [35], application = android.app.Application::class, qualifiers = "w411dp-h891dp")
@GraphicsMode(GraphicsMode.Mode.NATIVE)
class SetupDeviceTest {
    @get:Rule
    val compose = createComposeRule()

    @After
    fun tearDown() {
        CoreHost.credentials = null
        AppLinks.setup.value?.let(AppLinks::consume)
    }

    private fun exists(tag: String) = compose.onAllNodesWithTag(tag).fetchSemanticsNodes().isNotEmpty()

    private fun await(tag: String) {
        compose.waitUntil(5_000) { exists(tag) }
        compose.waitForIdle()
    }

    private fun click(tag: String) {
        await(tag)
        compose.onAllNodesWithTag(tag).onFirst().performSemanticsAction(SemanticsActions.OnClick)
        compose.waitForIdle()
    }

    @Test
    fun theAccountSheetShowsTheSetupCodeAndThePasswordGoesInOnlyWhenAsked() {
        val core = TestCore(startPlaying = false)
        compose.setThemedContent(core) { AppRoot(core.client) }
        core.start()
        await("navBar")
        val server = core.client.server.value!!
        core.client.dispatch(Commands.setSetting(SettingKeys.QUEUE_SAVED_CAP, "5"))
        CoreHost.credentials = InMemoryCredentialStore().apply { save(ServerCredential(server.url, server.username, "hunter2", null)) }
        click("account.button")
        click("account.setUpDevice")
        await("setupDevice.code")
        // No coordinator here, so the changed setting rides in the code.
        compose.onNodeWithTag("setupDevice.settings").performScrollTo()
        compose.waitUntil(5_000) { compose.onAllNodesWithText("1 changed setting comes along").fetchSemanticsNodes().isNotEmpty() }
        compose.onNodeWithTag("setupDevice.coordinator").performScrollTo()
        compose.onNodeWithTag("setupDevice.includePassword").performScrollTo()
        click("setupDevice.includePassword")
        // The sheet still shows a code (now a longer link): drawn without error.
        await("setupDevice.code")
    }

    @Test
    fun theCodeEncodesTheLink() {
        val text = SetupLink("https://music.example.net", "ada", "hunter2", "wss://c.example.net").format()
        val m = qrMatrix(text)
        assertTrue(m.width == m.height && m.width >= 21)
    }

    @Test
    fun anOpenedSetupLinkFillsTheFormWaitsForConnectAndDropsItsCoordinatorAndSettings() {
        val core = TestCore(startWithServer = false, startPlaying = false)
        compose.setThemedContent(core) { AppRoot(core.client) }
        core.start()
        await("setup.start")
        AppLinks.openSetup(SetupLink("https://music.example.net", "ada", "secret", "wss://coord.example.net", """{"${SettingKeys.QUEUE_SAVED_CAP}":7}"""))
        // A link from outside waits for Connect (any web page can open one), and what the form
        // doesn't show (a coordinator, which would get the credential, and settings) stays out.
        await("setup.connect")
        click("setup.connect")
        await("setup.continue")
        assertEquals(null, core.client.connection.value.coordinatorUrl)
        val cap = runBlocking { core.client.query(Queries.setting(SettingKeys.QUEUE_SAVED_CAP)) as QueryResult.SettingDetail }
        assertEquals("10", cap.data?.value)
        assertEquals(null, AppLinks.setup.value)
    }

    @Test
    fun aPastedSetupLinkWithThePasswordSignsInStraightAway() {
        val core = TestCore(startWithServer = false, startPlaying = false)
        compose.setThemedContent(core) { AppRoot(core.client) }
        core.start()
        click("setup.start")
        await("setup.url")
        compose.onNodeWithTag("setup.url").performTextInput(SetupLink("https://music.example.net", "ada", "secret", "wss://coord.example.net").format())
        await("setup.continue")
        assertEquals("wss://coord.example.net", core.client.connection.value.coordinatorUrl)
    }

    @Test
    fun aSetupLinksSettingsAreAppliedAsItConnects() {
        val core = TestCore(startWithServer = false, startPlaying = false)
        compose.setThemedContent(core) { AppRoot(core.client) }
        core.start()
        click("setup.start")
        await("setup.url")
        val link = SetupLink("https://music.example.net", "ada", "secret", settings = """{"${SettingKeys.QUEUE_SAVED_CAP}":7}""")
        compose.onNodeWithTag("setup.url").performTextInput(link.format())
        await("setup.continue")
        val cap = runBlocking { core.client.query(Queries.setting(SettingKeys.QUEUE_SAVED_CAP)) as QueryResult.SettingDetail }
        assertEquals("7", cap.data?.value)
    }

    @Test
    fun aSetupLinkWithoutThePasswordFillsTheForm() {
        val core = TestCore(startWithServer = false, startPlaying = false)
        compose.setThemedContent(core) { AppRoot(core.client) }
        core.start()
        await("setup.start")
        AppLinks.openSetup(SetupLink("https://music.example.net", "ada"))
        await("setup.password")
        compose.waitUntil(5_000) { compose.onAllNodesWithText("https://music.example.net").fetchSemanticsNodes().isNotEmpty() }
        assertTrue(compose.onAllNodesWithText("ada").fetchSemanticsNodes().isNotEmpty())
    }

    @Test
    fun aSetupLinkOnASignedInDeviceIsTurnedAway() {
        val core = TestCore(startPlaying = false)
        compose.setThemedContent(core) { AppRoot(core.client) }
        core.start()
        await("navBar")
        AppLinks.openSetup(SetupLink("https://other.example.net", "bob", "pw"))
        compose.waitUntil(5_000) { AppLinks.setup.value == null }
        compose.waitUntil(5_000) { compose.onAllNodesWithText("This device is already set up", substring = true).fetchSemanticsNodes().isNotEmpty() }
    }
}
