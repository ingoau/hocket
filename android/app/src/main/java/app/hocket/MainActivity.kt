package app.hocket

import android.content.Intent
import android.os.Build
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
import app.hocket.ui.nav.SplashHold
import app.hocket.ui.theme.HocketTheme

class MainActivity : ComponentActivity() {
    /** Set once AppRoot shows something other than its loading state: the splash may go. */
    @Volatile private var firstScreenReady = false
    /**
     * About when this activity's splash went up and its mark started drawing in (uptime): the
     * process's start for the first activity of a cold start (the system puts the splash up as it
     * launches the process), else this activity's creation.
     */
    private var splashStart = 0L
    /** This instance is a recreation: there is no splash, only (below API 31) the compat one to clear. */
    private var recreated = false
    override fun onCreate(savedInstanceState: Bundle?) {
        // The splash (Theme.Hocket.Starting) stands in for the loading screen: it stays until the
        // core is up and the first real screen is composed, or for SPLASH_MAX_MS at most (AppRoot's
        // LoadingScreen, which looks the same, takes over after that). The core is built meanwhile.
        (application as HocketApp).prewarmCore()
        val splash = installSplashScreen()
        val now = SystemClock.uptimeMillis()
        val processStart = android.os.Process.getStartUptimeMillis()
        // (A process the media service started long before any activity is not a cold start.)
        splashStart = if (firstActivityOfProcess && now - processStart < SPLASH_ICON_MS) processStart else now
        firstActivityOfProcess = false
        recreated = savedInstanceState != null
        // Only on a fresh start: a recreation has no system splash to hold, and holding the first
        // draw would leave the window blank instead.
        if (savedInstanceState == null) {
            // It also stays until the mark has drawn itself in (API 31+): handed over any sooner,
            // the system may drop the splash with its own plain fade instead of this exit, and the
            // mark's animation would be cut short.
            val minMs = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) SPLASH_ICON_MS else 0L
            // And while the first screen's content loads under it ([SplashHold]), up to
            // SPLASH_CONTENT_MAX_MS: past that the screen shows its own loading state.
            SplashHold.splashShown()
            splash.setKeepOnScreenCondition {
                val shown = SystemClock.uptimeMillis() - splashStart
                val keep = shown < SPLASH_MAX_MS && (!firstScreenReady || shown < minMs || (SplashHold.holding && shown < SPLASH_CONTENT_MAX_MS))
                // Polled every frame even when the system showed no splash (and so never calls
                // exitSplash): the first "let go" is when nothing is covering any more.
                if (!keep) SplashHold.splashGone()
                keep
            }
        } else {
            SplashHold.splashGone()
        }
        // An entrance a previous instance hid the shell for but never got to play plays now, on a
        // fresh start; a recreation (even mid-entrance) just shows the shell.
        // A page staged for an entrance that never played (no splash after all) doesn't carry over.
        if (recreated) ShellEntrance.finish() else ShellEntrance.restart()
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
     * away, swelling as it fades, as the shell's entrance plays underneath ([ShellEntrance]).
     */
    private fun exitSplash(provider: SplashScreenViewProvider) {
        // Below API 31 the compat splash hands over an exit on every creation, a recreation too:
        // there it just goes, and the shell stays as it is.
        SplashHold.splashGone()
        if (recreated) {
            provider.remove()
            return
        }
        // The shell is drawn whole until now (so a launch with no splash just shows it); under the
        // opaque splash it steps back, unseen, to wait for its entrance.
        ShellEntrance.hide()
        val view = provider.view
        // How long the mark has left to draw itself in (API 31+; no animation below that). The
        // platform's start is wall-clock time, and missing (0) when it went unrecorded: then the
        // splash's own start stands in, rather than taking the mark as finished and swapping the
        // still-drawing one for the finished copy below.
        val duration = provider.iconAnimationDurationMillis
        val remaining = if (duration > 0) {
            val elapsed = provider.iconAnimationStartMillis.takeIf { it > 0 }?.let { System.currentTimeMillis() - it }
                ?: (SystemClock.uptimeMillis() - splashStart)
            (duration - elapsed).coerceIn(0, SPLASH_MAX_WAIT_FOR_ICON_MS)
        } else 0L
        // M3's emphasized accelerate: things leaving the screen.
        val leave = PathInterpolator(0.3f, 0f, 0.8f, 0.15f)
        // The shell comes in once the tile has begun to clear: the fade accelerates, so the tile is
        // still mostly opaque then, and the page fades in fast enough that the two cross over
        // (never a moment with neither on screen) while the rest of the entrance plays in view.
        window.decorView.postDelayed(ShellEntrance::play, remaining + SPLASH_EXIT_DELAY_MS + SPLASH_ENTRANCE_INTO_FADE_MS)
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

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        handleShortcut(intent)
    }

    private companion object {
        /** No activity has been created in this process yet (see [splashStart]). */
        var firstActivityOfProcess = true
        /** The longest the splash waits for the first screen. */
        const val SPLASH_MAX_MS = 8_000L
        /** The longest the splash waits, from launch, for the first screen's content ([SplashHold]). */
        const val SPLASH_CONTENT_MAX_MS = 2_000L
        /** The longest the exit waits for the mark to finish drawing in. */
        const val SPLASH_MAX_WAIT_FOR_ICON_MS = 1_000L
        /** The mark's draw-in: windowSplashScreenAnimationDuration in values-v31/splash.xml. */
        const val SPLASH_ICON_MS = 940L
        /** The mark's head start in swelling before the tile starts to fade. */
        const val SPLASH_EXIT_DELAY_MS = 160L
        const val SPLASH_EXIT_MS = 320L
        /** How far into the tile's fade the shell's entrance starts (the tile is still ~85% opaque). */
        const val SPLASH_ENTRANCE_INTO_FADE_MS = 160L
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
        if (intent?.action == Intent.ACTION_VIEW) {
            // Another device's setup code (hocket://setup), once, as for shortcuts: never again from Recents.
            if (intent.flags and Intent.FLAG_ACTIVITY_LAUNCHED_FROM_HISTORY == 0) {
                app.hocket.ui.screens.setup.SetupLink.parse(intent.dataString)?.let(AppLinks::openSetup)
            }
            setIntent(Intent(intent).setAction(Intent.ACTION_MAIN).setData(null))
            return
        }
        val shortcut = AppShortcuts.parse(this, intent) ?: return
        setIntent(Intent(intent).setAction(Intent.ACTION_MAIN).replaceExtras(null as android.os.Bundle?))
        (application as HocketApp).runShortcut(shortcut)
    }
}
