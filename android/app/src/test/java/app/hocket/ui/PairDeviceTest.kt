package app.hocket.ui

import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithTag
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onFirst
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.performTextInput
import androidx.compose.ui.test.performSemanticsAction
import androidx.test.ext.junit.runners.AndroidJUnit4
import app.hocket.AppLinks
import app.hocket.playback.CoreHost
import app.hocket.ui.nav.AppRoot
import app.hocket.ui.nav.qrMatrix
import org.junit.After
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode

/** Pairing: the account sheet's Add a device code, a pairing link opened on a new device or a set-up one, and the devices settings. */
@RunWith(AndroidJUnit4::class)
@Config(sdk = [35], application = android.app.Application::class, qualifiers = "w411dp-h891dp")
@GraphicsMode(GraphicsMode.Mode.NATIVE)
class PairDeviceTest {
    @get:Rule
    val compose = createComposeRule()

    @After
    fun tearDown() {
        CoreHost.credentials = null
        AppLinks.pair.value?.let(AppLinks::consumePair)
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

    private val PAIR_LINK = "hocket://pair?d=ZmFrZS1wYWlyaW5nLWludml0ZQ"

    @Test
    fun theAccountSheetShowsAPairingCode() {
        val core = TestCore(startPlaying = false)
        compose.setThemedContent(core) { AppRoot(core.client) }
        core.start()
        await("navBar")
        click("account.button")
        click("account.setUpDevice")
        await("pairDevice.code")
        await("pairDevice.status")
        assertTrue(core.client.pairingInvite.value?.link?.startsWith("hocket://pair?d=") == true)
    }

    @Test
    fun theCodeEncodesTheLink() {
        val m = qrMatrix(PAIR_LINK)
        assertTrue(m.width == m.height && m.width >= 21)
    }

    @Test
    fun aPairingLinkPastedOnANewDevicePairsThenAsksForTheServer() {
        val core = TestCore(startWithServer = false, startPlaying = false)
        compose.setThemedContent(core) { AppRoot(core.client) }
        core.start()
        click("setup.start")
        await("setup.url")
        compose.onNodeWithTag("setup.url").performTextInput(PAIR_LINK)
        // The fake core answers PairingFinished with no accounts: the form waits for a server.
        compose.waitUntil(5_000) { compose.onAllNodesWithText("Pairing with your other device", substring = true).fetchSemanticsNodes().isEmpty() }
        await("setup.url")
    }

    @Test
    fun aPairingLinkOnASetUpDeviceAsksFirst() {
        val core = TestCore(startPlaying = false)
        compose.setThemedContent(core) { AppRoot(core.client) }
        core.start()
        await("navBar")
        AppLinks.openPair(PAIR_LINK)
        compose.waitUntil(5_000) { compose.onAllNodesWithText("Pair with another device?").fetchSemanticsNodes().isNotEmpty() }
        compose.onAllNodesWithText("Pair").onFirst().performSemanticsAction(SemanticsActions.OnClick)
        compose.waitUntil(5_000) { AppLinks.pair.value == null }
        compose.waitUntil(5_000) { compose.onAllNodesWithText("Paired with your other device").fetchSemanticsNodes().isNotEmpty() }
    }
}
