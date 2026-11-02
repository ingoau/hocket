package app.hocket

import android.content.Intent
import android.os.Bundle
import android.os.SystemClock
import android.view.KeyEvent
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
import app.hocket.ui.nav.ShellEntrance
import app.hocket.ui.theme.HocketTheme

class MainActivity : ComponentActivity() {
    /** Set once AppRoot shows something other than its loading state: the splash may go. */
    @Volatile private var firstScreenReady = false
    /** The system splash is on its way out ([exitSplash]): it starts the shell's entrance itself. */
    private var splashExiting = false
    /**
     * Starts the shell's entrance when no splash exit will (see [onFirstScreen]): once both
     * [NO_SPLASH_REVEAL_MS] and [NO_SPLASH_REVEAL_FRAMES] frames have passed since the first frame,
     * so a slow device, whose exit hand-over lags with its frames, still gets its exit first.
     */
    private val revealWithoutSplash = object : Runnable, android.view.Choreographer.FrameCallback {
        var frames = 0
        var since = 0L
        var stopped = false
        override fun run() {
            if (stopped) return
            frames = 0
            since = SystemClock.uptimeMillis()
            android.view.Choreographer.getInstance().postFrameCallback(this)
        }
        override fun doFrame(frameTimeNanos: Long) {
            if (stopped || splashExiting) return
            if (++frames < NO_SPLASH_REVEAL_FRAMES || SystemClock.uptimeMillis() - since < NO_SPLASH_REVEAL_MS) {
                android.view.Choreographer.getInstance().postFrameCallback(this)
                return
            }
            ShellEntrance.reveal()
        }
        fun stop() {
            stopped = true
            android.view.Choreographer.getInstance().removeFrameCallback(this)
        }
    }

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
            // The shell comes in as the splash goes ([ShellEntrance]).
            ShellEntrance.arm()
            splash.setKeepOnScreenCondition { !firstScreenReady && SystemClock.uptimeMillis() - splashStart < SPLASH_MAX_MS }
        } else {
            // A recreation (even one during the splash) has no splash to wait for.
            ShellEntrance.reveal()
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
        // Normally the splash's exit starts the shell's entrance; should none begin (a launch the
        // system showed no splash for), the shell must not wait for it. The exit is handed over
        // with the first frame drawn, so shortly after that frame it is known whether one is coming.
        val decor = window.decorView
        val firstDraw = object : android.view.ViewTreeObserver.OnDrawListener {
            var seen = false
            override fun onDraw() {
                if (seen) return
                seen = true
                decor.post { decor.viewTreeObserver.removeOnDrawListener(this) }
                decor.post(revealWithoutSplash)
            }
        }
        decor.viewTreeObserver.addOnDrawListener(firstDraw)
    }

    /**
     * Lets the mark finish drawing itself in (if the app was ready first), then lifts the splash
     * away, swelling as it fades, as the shell's entrance plays underneath ([ShellEntrance]).
     */
    private fun exitSplash(provider: SplashScreenViewProvider) {
        splashExiting = true
        val view = provider.view
        // The icon animation's start is wall-clock time (API 31+; no animation below that).
        val remaining = if (provider.iconAnimationDurationMillis > 0) {
            (provider.iconAnimationStartMillis + provider.iconAnimationDurationMillis - System.currentTimeMillis()).coerceIn(0, SPLASH_MAX_WAIT_FOR_ICON_MS)
        } else 0L
        // M3's emphasized accelerate: things leaving the screen.
        val leave = PathInterpolator(0.3f, 0f, 0.8f, 0.15f)
        // The shell starts coming in first, under the still-opaque splash, so it is mostly there by
        // the time the splash clears (never a moment with neither on screen).
        view.postDelayed(ShellEntrance::reveal, remaining)
        // From API 31 the mark is a surface the system hands over, which can drop out on the way
        // (and does not animate as a view). Once it has finished drawing in, a copy of the finished
        // mark (ic_launcher_foreground at the splash icon's size and place) stands in for it.
        val markSize = (SPLASH_MARK_DP * resources.displayMetrics.density).toInt()
        val mark = android.widget.ImageView(this).apply {
            setImageResource(R.drawable.ic_launcher_foreground)
            alpha = 0f
        }
        (view as? android.view.ViewGroup)?.addView(mark, android.widget.FrameLayout.LayoutParams(markSize, markSize, android.view.Gravity.CENTER))
        view.postDelayed({
            mark.alpha = 1f
            runCatching { provider.iconView }.getOrNull()?.visibility = android.view.View.INVISIBLE
        }, remaining)
        // Then the splash lifts away: the mark swells on as the tile and it fade.
        mark.animate().scaleX(SPLASH_MARK_EXIT_SCALE).scaleY(SPLASH_MARK_EXIT_SCALE)
            .setStartDelay(remaining).setDuration(SPLASH_EXIT_DELAY_MS + SPLASH_EXIT_MS).setInterpolator(leave)
            .start()
        view.animate().alpha(0f)
            .setStartDelay(remaining + SPLASH_EXIT_DELAY_MS).setDuration(SPLASH_EXIT_MS).setInterpolator(leave)
            .withEndAction { provider.remove() }
            .start()
    }

    /** Ctrl+Shift+D on a hardware keyboard opens developer diagnostics (while developer mode is on). */
    override fun dispatchKeyEvent(event: KeyEvent): Boolean = DeveloperTriggers.onKeyDown(event) || super.dispatchKeyEvent(event)

    override fun onDestroy() {
        window.decorView.removeCallbacks(revealWithoutSplash)
        revealWithoutSplash.stop()
        super.onDestroy()
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        handleShortcut(intent)
    }

    private companion object {
        /** The longest the splash waits for the first screen. */
        const val SPLASH_MAX_MS = 8_000L
        /** The longest the exit waits for the mark to finish drawing in. */
        const val SPLASH_MAX_WAIT_FOR_ICON_MS = 1_000L
        /** After the first frame, how long the shell waits for a splash exit to begin before coming in anyway. */
        const val NO_SPLASH_REVEAL_MS = 250L
        const val NO_SPLASH_REVEAL_FRAMES = 6
        /** The shell's head start under the splash before it starts to go. */
        const val SPLASH_EXIT_DELAY_MS = 160L
        const val SPLASH_EXIT_MS = 320L
        /** The splash icon's size without an icon background: the 108-unit tile the mark is drawn on. */
        const val SPLASH_MARK_DP = 288
        const val SPLASH_MARK_EXIT_SCALE = 1.4f
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
