package app.hocket

import android.content.Intent
import android.os.Bundle
import android.os.SystemClock
import android.view.KeyEvent
import android.view.View
import android.view.animation.PathInterpolator
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.core.splashscreen.SplashScreen.Companion.installSplashScreen
import androidx.core.splashscreen.SplashScreenViewProvider
import androidx.compose.runtime.getValue
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.LifecycleEventObserver
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import app.hocket.core.Commands
import app.hocket.diagnostics.DeveloperTriggers
import app.hocket.ui.nav.AppRoot
import app.hocket.ui.theme.HocketTheme

class MainActivity : ComponentActivity() {
    /** Set once AppRoot shows something other than its loading state: the splash may go. */
    @Volatile private var firstScreenReady = false

    override fun onCreate(savedInstanceState: Bundle?) {
        // The splash (Theme.Hocket.Starting) stands in for the loading screen: it stays until the
        // core is up and the first real screen is composed, or for SPLASH_MAX_MS at most (AppRoot's
        // LoadingScreen, which looks the same, takes over after that). The core is built meanwhile.
        (application as HocketApp).prewarmCore()
        val splash = installSplashScreen()
        val splashStart = SystemClock.uptimeMillis()
        // Only on a fresh start: a recreation has no system splash to hold, and holding the first
        // draw would leave the window blank instead.
        if (savedInstanceState == null) {
            splash.setKeepOnScreenCondition { !firstScreenReady && SystemClock.uptimeMillis() - splashStart < SPLASH_MAX_MS }
        }
        splash.setOnExitAnimationListener(::exitSplash)
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
                AppRoot(client = client, onFirstScreen = ::onFirstScreen)
            }
        }
    }

    /** The first real screen is composed: lift the splash and tell the system startup is done. */
    private fun onFirstScreen() {
        if (firstScreenReady) return
        firstScreenReady = true
        reportFullyDrawn()
    }

    /**
     * Lets the mark finish drawing itself in (if the app was ready first), then lifts the splash
     * away: the mark swells a little further as the tile fades, so the first screen shows through.
     */
    private fun exitSplash(provider: SplashScreenViewProvider) {
        val view = provider.view
        // The icon animation's start is wall-clock time (API 31+; no animation below that).
        val remaining = if (provider.iconAnimationDurationMillis > 0) {
            (provider.iconAnimationStartMillis + provider.iconAnimationDurationMillis - System.currentTimeMillis()).coerceIn(0, SPLASH_MAX_WAIT_FOR_ICON_MS)
        } else 0L
        val icon: View? = runCatching { provider.iconView }.getOrNull()
        val ease = PathInterpolator(0.3f, 0f, 0.8f, 0.15f)
        icon?.animate()?.scaleX(1.25f)?.scaleY(1.25f)?.setStartDelay(remaining)?.setDuration(SPLASH_EXIT_MS)?.setInterpolator(ease)?.start()
        view.animate().alpha(0f).setStartDelay(remaining).setDuration(SPLASH_EXIT_MS).setInterpolator(ease)
            .withEndAction { provider.remove() }
            .start()
    }

    /** Ctrl+Shift+D on a hardware keyboard opens developer diagnostics (while developer mode is on). */
    override fun dispatchKeyEvent(event: KeyEvent): Boolean = DeveloperTriggers.onKeyDown(event) || super.dispatchKeyEvent(event)

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        handleShortcut(intent)
    }

    private companion object {
        /** The longest the splash waits for the first screen. */
        const val SPLASH_MAX_MS = 8_000L
        /** The longest the exit waits for the mark to finish drawing in. */
        const val SPLASH_MAX_WAIT_FOR_ICON_MS = 1_000L
        const val SPLASH_EXIT_MS = 260L
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
