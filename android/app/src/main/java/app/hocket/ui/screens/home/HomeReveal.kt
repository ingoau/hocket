package app.hocket.ui.screens.home

import android.os.SystemClock
import androidx.compose.animation.core.Animatable
import androidx.compose.animation.core.EaseOutCubic
import androidx.compose.animation.core.tween
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.Stable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.setValue
import androidx.compose.runtime.snapshotFlow
import androidx.compose.ui.Modifier
import androidx.compose.ui.composed
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.unit.dp
import app.hocket.ui.a11y.LocalReducedMotion
import app.hocket.ui.nav.ShellEntrance
import app.hocket.ui.nav.SplashHold
import app.hocket.ui.nav.stagedPart
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.combine
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.withTimeoutOrNull

/** Where a cold Home load is: nothing yet, skeletons, or the feed. */
enum class RevealPhase { Waiting, Skeleton, Shown }

/** How long a cold load shows nothing before skeletons: most land first, and a skeleton that flashes reads as slower. */
internal const val SKELETON_DELAY_MS = 250L

/** Skeletons, once up, stay at least this long, so content landing just after them doesn't flicker. */
internal const val SKELETON_MIN_MS = 350L

/** From the start of a cold load, how long Home waits for every section before showing the ones it has. */
internal const val REVEAL_CAP_MS = 900L

/** The most Home waits under the splash itself (the splash's own cap, MainActivity's SPLASH_CONTENT_MAX_MS, normally ends it first). */
private const val SPLASH_HOLD_MAX_MS = 2_000L

/** Sections composed within this long of the reveal play the entrance; later ones (scrolled to) just appear. */
private const val ENTRANCE_WINDOW_MS = 600L
private const val ENTRANCE_STAGGER_MS = 45L
private const val ENTRANCE_MS = 320
private val STAGED_RISE = 40.dp
private const val STAGED_HERO_SCALE = 0.06f

@Stable
class HomeReveal internal constructor(phase: RevealPhase) {
    var phase by mutableStateOf(phase)
        internal set

    /** True just after a cold reveal: sections composed now fade and rise in, in order. */
    var entrance by mutableStateOf(false)
        internal set

    /**
     * The feed was in under the splash: sections come in on the shell's entrance ([stagedPart])
     * as the splash clears, rather than on their own clock.
     */
    var staged by mutableStateOf(false)
        internal set

    internal var holdingSplash = false
        private set

    internal fun holdSplash() {
        if (holdingSplash) return
        holdingSplash = true
        SplashHold.hold()
    }

    internal fun releaseSplash() {
        if (!holdingSplash) return
        holdingSplash = false
        SplashHold.release()
    }
}

/**
 * Paces Home's first appearance. With a cached feed it's [RevealPhase.Shown] at once, with no
 * entrance. Under the launch splash it holds the splash ([SplashHold]) until the feed is [ready],
 * then its sections come in on the shell's own entrance ([HomeReveal.staged]); if the splash gives up first, skeletons show
 * straight away. Otherwise a cold load shows nothing for [SKELETON_DELAY_MS]; if the feed is ready
 * by then it comes straight in, otherwise skeletons show (for at least [SKELETON_MIN_MS]) until the
 * feed is ready or [REVEAL_CAP_MS] passes, and whatever has loaded comes in together.
 */
@Composable
internal fun rememberHomeReveal(key: Any, ready: () -> Boolean): HomeReveal {
    // The hold is taken while composing, so the splash never sees a frame without it.
    val reveal = remember(key) {
        if (ready()) HomeReveal(RevealPhase.Shown)
        else HomeReveal(RevealPhase.Waiting).also { if (SplashHold.covering.value) it.holdSplash() }
    }
    DisposableEffect(reveal) { onDispose { reveal.releaseSplash() } }
    val isReady by rememberUpdatedState(ready)
    val reducedMotion = LocalReducedMotion.current
    LaunchedEffect(reveal) {
        if (reveal.phase == RevealPhase.Shown) return@LaunchedEffect
        val readyFlow = snapshotFlow { isReady() }
        if (reveal.holdingSplash) {
            // The splash lifts once the feed is in: the page is simply there under it.
            // Capped here too, in case nothing ever reports the splash gone.
            withTimeoutOrNull(SPLASH_HOLD_MAX_MS) {
                readyFlow.combine(SplashHold.covering) { done, covered -> done || !covered }.first { it }
            }
            reveal.releaseSplash()
            if (isReady()) {
                // The shell's entrance brings the sections in, top-down, as the splash clears.
                if (!reducedMotion) {
                    reveal.staged = true
                    ShellEntrance.pageStaged = true
                }
                reveal.phase = RevealPhase.Shown
                return@LaunchedEffect
            }
            // The splash waited as long as it would: no blank beat after it, skeletons now.
            reveal.phase = RevealPhase.Skeleton
            val shownAt = SystemClock.uptimeMillis()
            withTimeoutOrNull(REVEAL_CAP_MS) { readyFlow.first { it } }
            val held = SystemClock.uptimeMillis() - shownAt
            if (held < SKELETON_MIN_MS) delay(SKELETON_MIN_MS - held)
        } else if (withTimeoutOrNull(SKELETON_DELAY_MS) { readyFlow.first { it } } == null) {
            reveal.phase = RevealPhase.Skeleton
            val shownAt = SystemClock.uptimeMillis()
            withTimeoutOrNull(REVEAL_CAP_MS - SKELETON_DELAY_MS) { readyFlow.first { it } }
            val held = SystemClock.uptimeMillis() - shownAt
            if (held < SKELETON_MIN_MS) delay(SKELETON_MIN_MS - held)
        }
        reveal.entrance = !reducedMotion
        reveal.phase = RevealPhase.Shown
        delay(ENTRANCE_WINDOW_MS)
        reveal.entrance = false
    }
    return reveal
}

/**
 * The cold-reveal entrance for the [step]th section: a fade and a short rise, staggered top-down.
 * A [hero] (Spotlight) settles in from a little larger instead, when the entrance is staged.
 */
internal fun Modifier.homeEntrance(reveal: HomeReveal, step: Int, hero: Boolean = false): Modifier = composed {
    // On the splash's exit: in step with the shell's entrance. Spotlight (the hero, always first)
    // settles down from a little larger, carrying on the mark's swell; the sections rise in.
    if (remember { reveal.staged }) {
        val density = LocalDensity.current
        val rise = with(density) { STAGED_RISE.toPx() }
        return@composed graphicsLayer {
            val e = stagedPart(ShellEntrance.progress.value, step)
            if (e >= 1f) return@graphicsLayer
            alpha = (e * 1.6f).coerceAtMost(1f)
            if (hero) {
                val s = 1f + STAGED_HERO_SCALE * (1f - e)
                scaleX = s
                scaleY = s
            } else {
                translationY = (1f - e) * rise
            }
        }
    }
    val play = remember { reveal.entrance }
    if (!play) return@composed this
    val progress = remember { Animatable(0f) }
    LaunchedEffect(Unit) {
        delay(step.coerceAtMost(5) * ENTRANCE_STAGGER_MS)
        progress.animateTo(1f, tween(ENTRANCE_MS, easing = EaseOutCubic))
    }
    val rise = with(LocalDensity.current) { 24.dp.toPx() }
    graphicsLayer {
        alpha = progress.value
        translationY = (1f - progress.value) * rise
    }
}
