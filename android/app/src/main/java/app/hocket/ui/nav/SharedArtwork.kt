package app.hocket.ui.nav

import androidx.compose.animation.AnimatedVisibilityScope
import androidx.compose.animation.SharedTransitionScope
import androidx.compose.animation.core.tween
import androidx.compose.runtime.Composable
import androidx.compose.runtime.compositionLocalOf
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Rect
import app.hocket.ui.a11y.LocalReducedMotion
import java.util.UUID

/**
 * Shared-element keys for artwork that travels between a list/grid and its detail page (Navic),
 * and for the Library's Albums tile fan into that list's Recent row.
 *
 * Keys are per destination so two different albums never collide; a [SharedArtworkSession] then
 * picks the source instance that was actually opened, so a pin and a grid cell of the same album
 * do not both try to fly.
 */
internal object ArtworkShare {
    fun album(id: String) = "album:$id"
    fun artist(id: String) = "artist:$id"
    fun playlist(id: String) = "playlist:$id"
    /** The Albums browse-tile fan cover that lands on the albums page's matching Recent card. */
    fun albumsTile(id: String) = "albums-tile:$id"
}

/** One artwork taking part in a flight: the matching key, the source instance, and the cover to show while the other side loads. */
internal data class SharedArtworkEntry(val key: String, val sourceId: Any, val coverArt: String?)

/**
 * Which artwork is mid-flight. [sourceId] is the remembered identity of the list/grid instance that
 * called [prepare]; the other page matches on the key alone. Several entries at once let the Albums
 * tile's three fan covers fly together.
 */
internal class SharedArtworkSession {
    var entries by mutableStateOf<Map<String, SharedArtworkEntry>>(emptyMap())
        private set

    fun prepare(key: String, sourceId: Any, coverArt: String? = null) =
        prepareAll(listOf(SharedArtworkEntry(key, sourceId, coverArt)))

    fun prepareAll(items: List<SharedArtworkEntry>) {
        entries = items.associateBy { it.key }
    }

    /** Open a detail with no matching source (the player, a menu): the art appears in place. */
    fun prepareDestination(key: String, coverArt: String? = null) = prepare(key, DESTINATION, coverArt)

    fun matches(key: String, source: Boolean, sourceId: Any?): Boolean {
        val entry = entries[key] ?: return false
        return if (source) entry.sourceId == sourceId else true
    }

    fun coverArtFor(key: String): String? = entries[key]?.coverArt

    companion object {
        internal val DESTINATION = Any()
    }
}

internal val LocalSharedArtwork = compositionLocalOf<SharedArtworkSession?> { null }
internal val LocalSharedTransitionScope = compositionLocalOf<SharedTransitionScope?> { null }
internal val LocalNavAnimatedVisibilityScope = compositionLocalOf<AnimatedVisibilityScope?> { null }

/** What a list/grid artwork needs: its shared-element [modifier] and a [prepare] to call on open. */
internal class ArtworkShareSource(
    val modifier: Modifier,
    val prepare: () -> Unit,
    internal val entry: SharedArtworkEntry?,
)

@Composable
internal fun rememberArtworkShareSource(
    key: String?,
    coverArt: String? = null,
    zIndex: Float = 1f,
): ArtworkShareSource {
    val session = LocalSharedArtwork.current
    // Survives the source page leaving composition while the destination is on screen, so a pop
    // can pair the same instance again and fly the art back (NavHost disposes covered entries).
    val sourceId = rememberSaveable(key) { UUID.randomUUID().toString() }
    val modifier = sharedArtworkModifier(key, source = true, sourceId = sourceId, zIndex = zIndex)
    val entry = key?.let { SharedArtworkEntry(it, sourceId, coverArt) }
    return ArtworkShareSource(
        modifier = modifier,
        prepare = { if (entry != null) session?.prepareAll(listOf(entry)) },
        entry = entry,
    )
}

@Composable
internal fun Modifier.sharedArtworkDestination(key: String?, zIndex: Float = 1f): Modifier =
    then(sharedArtworkModifier(key, source = false, sourceId = null, zIndex = zIndex))

@Composable
private fun sharedArtworkModifier(
    key: String?,
    source: Boolean,
    sourceId: Any?,
    zIndex: Float,
): Modifier {
    val session = LocalSharedArtwork.current
    val visibility = LocalNavAnimatedVisibilityScope.current
    val scope = LocalSharedTransitionScope.current
    val reduced = LocalReducedMotion.current
    // Keep remember order stable when [key] is still null (the Albums tile's fan fills in after load).
    if (session == null || visibility == null || scope == null) return Modifier
    val state = with(scope) { rememberSharedContentState(key = key ?: "artwork.idle") }
    val enabled = key != null && !reduced && session.matches(key, source, sourceId)
    if (!enabled) return Modifier
    val bounds = { _: Rect, _: Rect -> tween<Rect>(PUSH_MS, easing = MiuixEasing) }
    return with(scope) {
        Modifier.sharedElement(
            state,
            animatedVisibilityScope = visibility,
            boundsTransform = bounds,
            zIndexInOverlay = zIndex,
        )
    }
}
