package app.hocket.ui

import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithTag
import androidx.compose.ui.test.onFirst
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.performScrollTo
import androidx.compose.ui.test.performSemanticsAction
import androidx.compose.ui.test.performTextInput
import androidx.test.core.app.ApplicationProvider
import androidx.test.ext.junit.runners.AndroidJUnit4
import app.hocket.R
import app.hocket.ui.nav.AppRoot
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode

/**
 * Settings → Connect has one Coordinator section: with none pinned it offers the address and code
 * fields with Claim and Join with invite; once pinned it shows the coordinator, the connection and
 * Disconnect / Reconnect instead, with no address field. The settings search lands on it.
 */
@RunWith(AndroidJUnit4::class)
@Config(sdk = [35], application = android.app.Application::class, qualifiers = "w411dp-h891dp")
@GraphicsMode(GraphicsMode.Mode.NATIVE)
class ConnectCoordinatorSettingsTest {
    @get:Rule
    val compose = createComposeRule()

    private val context = ApplicationProvider.getApplicationContext<android.content.Context>()

    private fun exists(tag: String) = compose.onAllNodesWithTag(tag, useUnmergedTree = true).fetchSemanticsNodes().isNotEmpty()

    private fun await(tag: String) {
        compose.waitUntil(5_000) { exists(tag) }
        compose.waitForIdle()
    }

    private fun click(tag: String) {
        await(tag)
        compose.onAllNodesWithTag(tag).onFirst().performScrollTo().performSemanticsAction(SemanticsActions.OnClick)
        compose.waitForIdle()
    }

    private fun openConnect(): TestCore {
        val core = TestCore(startPlaying = false)
        compose.setThemedContent(core) { AppRoot(core.client) }
        core.start()
        compose.openSettingsFromAccount()
        click("settings.category.connect")
        await("settings.screen")
        return core
    }

    @Test
    fun withNoCoordinatorItOffersClaimAndRedeem() {
        openConnect()
        await("setting.connect.coordinator")
        for (tag in listOf("setting.identity.coordinatorNone", "identity.coordinatorUrl", "identity.coordinatorCode", "identity.claim", "identity.redeem")) {
            assertTrue("$tag shown", exists(tag))
        }
        assertFalse(exists("identity.coordinatorDisconnect"))
        assertFalse(exists("setting.identity.coordinatorStatus"))
    }

    @Test
    fun claimedItShowsTheConnectionAndDisconnectInsteadOfTheFields() {
        val core = openConnect()
        await("identity.coordinatorUrl")
        compose.onNodeWithTag("identity.coordinatorUrl").performScrollTo().performTextInput("wss://coordinator.example/ws")
        compose.onNodeWithTag("identity.coordinatorCode").performScrollTo().performTextInput("ABCD-EFGH")
        click("identity.claim")
        await("setting.identity.coordinatorPin")
        assertEquals("wss://coordinator.example/ws", core.client.identity.value.coordinator?.url)
        assertEquals("wss://coordinator.example/ws", core.client.connection.value.coordinatorUrl)
        await("identity.coordinatorDisconnect")
        assertTrue(exists("setting.identity.coordinatorStatus"))
        for (tag in listOf("identity.coordinatorUrl", "identity.coordinatorCode", "identity.claim", "identity.redeem", "setting.identity.coordinatorNone")) {
            assertFalse("$tag gone", exists(tag))
        }
        // Disconnect, then Reconnect.
        click("identity.coordinatorDisconnect")
        await("identity.coordinatorReconnect")
        assertFalse(core.client.connection.value.connected)
        click("identity.coordinatorReconnect")
        await("identity.coordinatorDisconnect")
        assertTrue(core.client.connection.value.connected)
    }

    @Test
    fun theSearchLandsOnTheCoordinatorSection() {
        val core = TestCore(startPlaying = false)
        compose.setThemedContent(core) { AppRoot(core.client) }
        core.start()
        compose.openSettingsFromAccount()
        compose.onNodeWithTag("settings.search").performTextInput("claim")
        compose.waitForIdle()
        click("settings.hit.connect.coordinator")
        await("settings.screen")
        compose.mainClock.advanceTimeBy(1_000)
        compose.waitForIdle()
        val title = compose.onNodeWithTag("settings.title").fetchSemanticsNode().config[SemanticsProperties.Text].joinToString("") { it.text }
        assertEquals(context.getString(R.string.settings_category_connect), title)
        compose.onNodeWithTag("setting.connect.coordinator").assertIsDisplayed()
    }
}
