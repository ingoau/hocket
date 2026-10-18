package app.hocket.ui.nav

import androidx.compose.animation.AnimatedVisibilityScope
import androidx.compose.animation.SharedTransitionScope
import androidx.compose.animation.core.tween
import androidx.compose.runtime.Composable
import androidx.compose.runtime.compositionLocalOf
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import app.hocket.ui.a11y.LocalReducedMotion

/**
 * Shared-element keys for artwork that travels between a list/grid and its detail page (Navic).
 * Keys are per destination so two different albums never collide; a [SharedArtworkSession] then
 * picks the one source instance that was actually opened, so a pin and a grid cell of the same
 * album do not both try to fly.
 */
internal object ArtworkShare {
    fun album(id: String) = "album:$id"
    fun artist(id: String) = "artist:$id"
    fun playlist(id: String) = "playlist:$id"
}

/**
 * Which artwork on the current destination is mid-flight. [sourceId] is the remembered identity of
 * the list/grid instance that called [prepare]; the detail page matches on [key] alone.
 */
internal class SharedArtworkSession {
    var key by mutableStateOf<String?>(null)
        private set
    var sourceId by mutableStateOf<Any?>(null)
        private set
    var coverArt by mutableStateOf<String?>(null)
        private set

    fun prepare(key: String, sourceId: Any, coverArt: String? = null) {
        this.key = key
        this.sourceId = sourceId
        this.coverArt = coverArt
    }

    /** Open a detail with no matching source (the player, a menu): the art appears in place. */
    fun prepareDestination(key: String, coverArt: String? = null) = prepare(key, DESTINATION, coverArt)

    companion object {
        internal val DESTINATION = Any()
    }
}

internal val LocalSharedArtwork = compositionLocalOf<SharedArtworkSession?> { null }
internal val LocalSharedTransitionScope = compositionLocalOf<SharedTransitionScope?> { null }
internal val LocalNavAnimatedVisibilityScope = compositionLocalOf<AnimatedVisibilityScope?> { null }

/** What a list/grid artwork needs: its shared-element [modifier] and a [prepare] to call on open. */
internal class ArtworkShareSource(val modifier: Modifier, val prepare: () -> Unit)

@Composable
internal fun rememberArtworkShareSource(key: String?, coverArt: String? = null): ArtworkShareSource {
    val session = LocalSharedArtwork.current
    val sourceId = remember { Any() }
    val modifier = sharedArtworkModifier(key, source = true, sourceId = sourceId)
    return ArtworkShareSource(
        modifier = modifier,
        prepare = { if (key != null) session?.prepare(key, sourceId, coverArt) },
    )
}

@Composable
internal fun Modifier.sharedArtworkDestination(key: String?): Modifier =
    then(sharedArtworkModifier(key, source = false, sourceId = null))

@Composable
private fun sharedArtworkModifier(key: String?, source: Boolean, sourceId: Any?): Modifier {
    val session = LocalSharedArtwork.current
    val visibility = LocalNavAnimatedVisibilityScope.current
    val scope = LocalSharedTransitionScope.current
    val reduced = LocalReducedMotion.current
    if (key == null || reduced || session == null || visibility == null || scope == null) return Modifier
    val state = with(scope) { rememberSharedContentState(key = key) }
    val enabled = session.key == key && if (source) session.sourceId == sourceId else true
    if (!enabled) return Modifier
    return with(scope) {
        Modifier.sharedElement(
            state,
            animatedVisibilityScope = visibility,
            boundsTransform = { _, _ -> tween(PUSH_MS, easing = MiuixEasing) },
        )
    }
}
