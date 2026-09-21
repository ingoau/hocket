package app.hocket

import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.compose.runtime.getValue
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.LifecycleEventObserver
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import app.hocket.core.Commands
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
        setContent {
            val client by app.client.collectAsStateWithLifecycle()
            HocketTheme(client = client) {
                AppRoot(client = client)
            }
        }
    }
}
