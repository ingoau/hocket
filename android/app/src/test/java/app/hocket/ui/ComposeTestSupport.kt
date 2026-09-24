package app.hocket.ui

import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.test.junit4.ComposeContentTestRule
import androidx.compose.ui.test.onAllNodesWithTag
import androidx.compose.ui.test.onFirst
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.performSemanticsAction
import app.hocket.core.client.CoreClient
import app.hocket.core.fake.FakeCore
import app.hocket.ui.theme.HocketTheme
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob

/** A fake core + client on the main dispatcher, the way the app wires them. */
class TestCore(startWithServer: Boolean = true, startPlaying: Boolean = true, seed: Long = 7, now: () -> Double = { System.currentTimeMillis().toDouble() }) {
    val fake = FakeCore(seed = seed, startWithServer = startWithServer, startPlaying = startPlaying, timers = false, now = now, dispatcher = Dispatchers.Unconfined)
    val scope = CoroutineScope(SupervisorJob() + Dispatchers.Main.immediate)
    /** The same clock stamps positions in the fake and extrapolates them in the client. */
    val client = CoreClient(fake, scope, now)

    fun start() = client.requestSnapshot()
}

fun ComposeContentTestRule.setThemedContent(core: TestCore, content: @Composable () -> Unit) {
    setContent {
        HocketTheme(client = core.client) {
            CompositionLocalProvider(LocalCoreClient provides core.client) { content() }
        }
    }
}

/**
 * Opens Settings the way a user does since Settings left the bottom bar: the account button in the
 * current page's top app bar, then Settings in the account sheet.
 */
fun ComposeContentTestRule.openSettingsFromAccount() {
    waitUntil(5_000) { onAllNodesWithTag("account.button").fetchSemanticsNodes().isNotEmpty() }
    onAllNodesWithTag("account.button").onFirst().performSemanticsAction(SemanticsActions.OnClick)
    waitUntil(5_000) { onAllNodesWithTag("account.settings").fetchSemanticsNodes().isNotEmpty() }
    onNodeWithTag("account.settings").performSemanticsAction(SemanticsActions.OnClick)
    waitUntil(5_000) { onAllNodesWithTag("settings.categories").fetchSemanticsNodes().isNotEmpty() }
    waitForIdle()
}
