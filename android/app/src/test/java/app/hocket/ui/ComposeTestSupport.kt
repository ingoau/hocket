package app.hocket.ui

import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.ui.test.junit4.ComposeContentTestRule
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
