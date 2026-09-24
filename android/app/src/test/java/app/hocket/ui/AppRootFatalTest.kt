package app.hocket.ui

import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.performClick
import androidx.test.ext.junit.runners.AndroidJUnit4
import app.hocket.ui.nav.AppRoot
import app.hocket.ui.nav.FatalErrorScreen
import kotlinx.coroutines.flow.MutableStateFlow
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode

/** A release build whose native core could not start shows the fatal screen, never a (fake) library. */
@RunWith(AndroidJUnit4::class)
@Config(sdk = [35], application = android.app.Application::class, qualifiers = "w411dp-h891dp")
@GraphicsMode(GraphicsMode.Mode.NATIVE)
class AppRootFatalTest {
    @get:Rule
    val compose = createComposeRule()

    @Test
    fun aFailedNativeCoreShowsTheFatalScreenInsteadOfTheShell() {
        val core = TestCore()
        val fatal = MutableStateFlow<String?>("libhocket_android.so: cannot open shared object")
        compose.setThemedContent(core) { AppRoot(core.client, fatalError = fatal) }
        core.start()
        compose.waitUntil(5_000) { core.client.started.value }
        compose.onNodeWithTag("fatal").assertIsDisplayed()
        compose.onNodeWithTag("miniPlayer").assertDoesNotExist()
        compose.onNodeWithTag("setup.url").assertDoesNotExist()
    }

    @Test
    fun resetAsksForConfirmationThenResets() {
        var reset = false
        compose.setContent { FatalErrorScreen("boom", onReset = { reset = true }) }
        compose.onNodeWithTag("fatal.reset").performClick()
        compose.waitForIdle()
        compose.onNodeWithTag("confirm.ok").performClick()
        compose.waitForIdle()
        assertTrue(reset)
    }
}
