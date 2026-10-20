package app.hocket

import android.content.Intent
import android.os.Bundle
import android.view.KeyEvent
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.compose.runtime.getValue
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.LifecycleEventObserver
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import app.hocket.core.Commands
import app.hocket.diagnostics.DeveloperTriggers
import app.hocket.ui.nav.AppRoot
import app.hocket.ui.theme.HocketTheme

class MainActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        enableEdgeToEdge()
        super.onCreate(savedInstanceState)
        val app = application as HocketApp
        // Visibility for renderer throttling and prefetch pausing (design: performance budget).
        lifecycle.addObserver(LifecycleEventObserver { _, event ->
            val client = app.client.value ?: return@LifecycleEventObserver
            when (event) {
                Lifecycle.Event.ON_RESUME -> client.dispatch(Commands.setVisibility(visible = true, focused = true))
                Lifecycle.Event.ON_PAUSE -> client.dispatch(Commands.setVisibility(visible = true, focused = false))
                Lifecycle.Event.ON_STOP -> client.dispatch(Commands.setVisibility(visible = false, focused = false))
                else -> Unit
            }
        })
        // A launcher shortcut (only a fresh start: a recreation must not play it again).
        if (savedInstanceState == null) handleShortcut(intent)
        setContent {
            val client by app.client.collectAsStateWithLifecycle()
            HocketTheme(client = client) {
                AppRoot(client = client)
            }
        }
    }

    /** Ctrl+Shift+D on a hardware keyboard opens developer diagnostics (while developer mode is on). */
    override fun dispatchKeyEvent(event: KeyEvent): Boolean = DeveloperTriggers.onKeyDown(event) || super.dispatchKeyEvent(event)

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        handleShortcut(intent)
    }

    /**
     * Runs the launcher shortcut [intent] stands for, once: a task started by a shortcut keeps it as
     * its base intent, which Android hands back when the task is relaunched from Recents (after a
     * reboot, or once Back finished the activity), so a relaunch from history never runs it, and a
     * handled one is cleared.
     */
    private fun handleShortcut(intent: Intent?) {
        val shortcut = AppShortcuts.parse(this, intent) ?: return
        setIntent(Intent(intent).setAction(Intent.ACTION_MAIN).replaceExtras(null as android.os.Bundle?))
        (application as HocketApp).runShortcut(shortcut)
    }
}
