package app.hocket.ui

import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performScrollTo
import androidx.compose.ui.test.performTextClearance
import androidx.compose.ui.test.performTextInput
import androidx.test.ext.junit.runners.AndroidJUnit4
import app.hocket.ui.nav.AppRoot
import org.junit.Assert.assertEquals
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode

@RunWith(AndroidJUnit4::class)
@Config(sdk = [35], application = android.app.Application::class, qualifiers = "w411dp-h891dp")
@GraphicsMode(GraphicsMode.Mode.NATIVE)
class ServerSetupFlowTest {
    @get:Rule
    val compose = createComposeRule()

    @Test
    fun setupIsTheWholeFirstScreenAndConnectsToAGoodServer() {
        val core = TestCore(startWithServer = false, startPlaying = false)
        compose.setThemedContent(core) { AppRoot(core.client) }
        core.start()
        compose.waitUntil(5_000) { compose.onAllNodesWithTagCount("setup.url") == 1 }
        compose.onNodeWithTag("setup.url").assertIsDisplayed()
        compose.onNodeWithTag("setup.connect").performScrollTo().assertIsDisplayed()
        compose.onNodeWithTag("setup.url").performTextInput("https://music.example.net")
        compose.onNodeWithTag("setup.username").performTextInput("ada")
        compose.onNodeWithTag("setup.password").performTextInput("secret")
        compose.onNodeWithTag("setup.connect").performScrollTo().performClick()
        // A server that meets the floor takes us into the shell: the Home title appears.
        compose.waitUntil(5_000) { compose.onAllNodesWithTagCount("setup.url") == 0 }
        compose.onNodeWithText("Your music").assertIsDisplayed()
    }

    @Test
    fun serverBelowTheFloorShowsAClearError() {
        val core = TestCore(startWithServer = false, startPlaying = false)
        compose.setThemedContent(core) { AppRoot(core.client) }
        core.start()
        compose.waitUntil(5_000) { compose.onAllNodesWithTagCount("setup.url") == 1 }
        compose.onNodeWithTag("setup.url").performTextInput("https://old.example.net")
        compose.onNodeWithTag("setup.username").performTextInput("ada")
        compose.onNodeWithTag("setup.password").performTextInput("secret")
        compose.onNodeWithTag("setup.connect").performScrollTo().performClick()
        compose.waitUntil(5_000) { compose.onAllNodesWithTagCount("setup.tooOld") == 1 }
        compose.onNodeWithTag("setup.tooOld").performScrollTo().assertIsDisplayed()
        // Still on setup: the URL field is there, not the shell.
        compose.onNodeWithTag("setup.url").performScrollTo().assertIsDisplayed()
    }

    @Test
    fun wrongPasswordStaysOnSetupWithTheError() {
        val core = TestCore(startWithServer = false, startPlaying = false)
        compose.setThemedContent(core) { AppRoot(core.client) }
        core.start()
        compose.waitUntil(5_000) { compose.onAllNodesWithTagCount("setup.url") == 1 }
        compose.onNodeWithTag("setup.url").performTextInput("https://music.example.net")
        compose.onNodeWithTag("setup.username").performTextInput("ada")
        compose.onNodeWithTag("setup.password").performTextInput("wrong")
        compose.onNodeWithTag("setup.connect").performScrollTo().performClick()
        compose.waitUntil(5_000) { compose.onAllNodesWithTagCount("setup.error") == 1 }
        compose.onNodeWithTag("setup.url").performScrollTo().assertIsDisplayed()
    }

    private fun textOf(tag: String): String =
        compose.onNodeWithTag(tag).fetchSemanticsNode().config.getOrElseNullable(androidx.compose.ui.semantics.SemanticsProperties.EditableText) { null }?.text ?: ""

    @Test
    fun thePasswordNeverGoesIntoTheSavedInstanceState() {
        val core = TestCore(startWithServer = false, startPlaying = false)
        val restore = androidx.compose.ui.test.junit4.StateRestorationTester(compose)
        restore.setContent {
            app.hocket.ui.theme.HocketTheme(client = core.client) {
                androidx.compose.runtime.CompositionLocalProvider(LocalCoreClient provides core.client) {
                    app.hocket.ui.screens.setup.ServerSetupScreen(existing = null, needsRelogin = false)
                }
            }
        }
        compose.onNodeWithTag("setup.url").performTextInput("https://music.example.net")
        compose.onNodeWithTag("setup.username").performTextInput("ada")
        compose.onNodeWithTag("setup.password").performTextInput("secret")
        assertEquals(6, textOf("setup.password").length)
        // Activity recreation / process death: the Bundle keeps the address and user, never the password.
        restore.emulateSavedInstanceStateRestore()
        assertEquals("https://music.example.net", textOf("setup.url"))
        assertEquals("ada", textOf("setup.username"))
        assertEquals("", textOf("setup.password"))
    }

    @Test
    fun onlyAPlainHttpAddressShowsTheCleartextWarning() {
        val core = TestCore(startWithServer = false, startPlaying = false)
        compose.setThemedContent(core) { AppRoot(core.client) }
        core.start()
        compose.waitUntil(5_000) { compose.onAllNodesWithTagCount("setup.url") == 1 }
        compose.onNodeWithTag("setup.url").performTextInput("https://music.example.net")
        assertEquals(0, compose.onAllNodesWithTagCount("setup.cleartextWarning"))
        compose.onNodeWithTag("setup.url").performTextClearance()
        compose.onNodeWithTag("setup.url").performTextInput("http://192.168.1.20:4533")
        compose.onNodeWithTag("setup.cleartextWarning").performScrollTo().assertIsDisplayed()
    }
}

fun androidx.compose.ui.test.junit4.ComposeContentTestRule.onAllNodesWithTagCount(tag: String): Int =
    onAllNodes(androidx.compose.ui.test.hasTestTag(tag)).fetchSemanticsNodes().size
