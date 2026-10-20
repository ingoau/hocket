package app.hocket.ui.nav

import androidx.compose.animation.AnimatedVisibilityScope
import androidx.compose.animation.EnterExitState
import androidx.compose.animation.SharedTransitionScope
import androidx.compose.animation.core.animateFloat
import androidx.compose.animation.core.tween
import androidx.compose.runtime.Composable
import androidx.compose.runtime.compositionLocalOf
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateMapOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.graphics.graphicsLayer
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

/**
 * One artwork taking part in a flight: the matching key, the source instance, the cover to show
 * while the other side loads, and the source's pose (its tilt, and its place in the overlay) that
 * the destination eases out of on the way in and back into on the way back.
 */
internal data class SharedArtworkEntry(
    val key: String,
    val sourceId: Any,
    val coverArt: String?,
    val rotation: Float = 0f,
    val zIndex: Float = 1f,
)

/**
 * Which artwork flies, per page. [prepare] notes the source about to be opened; the page pushed
 * next takes it ([attach], from the NavController's destination listener) and keeps it for as long
 * as it is on the back stack, so popping that page flies the art back into the same instance even
 * after other pages were pushed and popped above it. A page matches its own entries as the
 * destination; a source matches by its instance id, so the album's other cards stay put.
 */
internal class SharedArtworkSession {
    /** Prepared by a tap, not yet taken by a page. Not state: read only by [attach]. */
    private var pending: Map<String, SharedArtworkEntry> = emptyMap()
    private val pages = mutableStateMapOf<String, Map<String, SharedArtworkEntry>>()

    fun prepare(key: String, sourceId: Any, coverArt: String? = null) =
        prepareAll(listOf(SharedArtworkEntry(key, sourceId, coverArt)))

    fun prepareAll(items: List<SharedArtworkEntry>) {
        pending = items.associateBy { it.key }
    }

    /** Open a detail with no matching source (the player, a menu): the art appears in place. */
    fun prepareDestination(key: String, coverArt: String? = null) = prepare(key, DESTINATION, coverArt)

    /**
     * [pageId] became the current page. A page new to the session takes what was prepared for it
     * (nothing, for a tab switch or a link that prepared nothing); one already known (a pop back to
     * it) keeps its own. Either way the pending flight is spent. True when the page is new.
     */
    fun attach(pageId: String): Boolean {
        val claimed = pending
        pending = emptyMap()
        if (pageId in pages) return false
        pages[pageId] = claimed
        return true
    }

    /** [pageId] left the back stack for good (after its pop animation). */
    fun release(pageId: String) {
        pages.remove(pageId)
    }

    fun entryFor(pageId: String?, key: String): SharedArtworkEntry? = pageId?.let { pages[it]?.get(key) }

    fun matchesDestination(pageId: String?, key: String): Boolean = entryFor(pageId, key) != null

    fun matchesSource(key: String, sourceId: Any): Boolean = pages.values.any { it[key]?.sourceId == sourceId }

    fun coverArtFor(pageId: String?, key: String): String? = entryFor(pageId, key)?.coverArt

    companion object {
        internal val DESTINATION = Any()
    }
}

internal val LocalSharedArtwork = compositionLocalOf<SharedArtworkSession?> { null }
internal val LocalSharedTransitionScope = compositionLocalOf<SharedTransitionScope?> { null }
internal val LocalNavAnimatedVisibilityScope = compositionLocalOf<AnimatedVisibilityScope?> { null }
/** The back-stack entry id of the NavHost page being composed, the session's page key. */
internal val LocalNavPageId = compositionLocalOf<String?> { null }

/** What a list/grid artwork needs: its shared-element [modifier] and a [prepare] to call on open. */
internal class ArtworkShareSource(
    val modifier: Modifier,
    val prepare: () -> Unit,
    internal val entry: SharedArtworkEntry?,
)

/**
 * A list/grid artwork that can fly to the page it opens. [rotation] is its resting tilt (the
 * Albums tile fan): the modifier applies it, and eases it to and from the destination's upright
 * pose during a flight instead of snapping.
 */
@Composable
internal fun rememberArtworkShareSource(
    key: String?,
    coverArt: String? = null,
    zIndex: Float = 1f,
    rotation: Float = 0f,
): ArtworkShareSource {
    val session = LocalSharedArtwork.current
    // Survives the source page leaving composition while the destination is on screen, so a pop
    // can pair the same instance again and fly the art back (NavHost disposes covered entries).
    // Not keyed on [key]: a key that fills in after load (the Albums tile's fan) must keep the id
    // the pending flight was prepared with, or the flight back finds no source.
    val sourceId = rememberSaveable { UUID.randomUUID().toString() }
    val modifier = sharedArtworkModifier(key, sourceId = sourceId, zIndex = zIndex, rotation = rotation)
    val entry = key?.let { SharedArtworkEntry(it, sourceId, coverArt, rotation, zIndex) }
    return ArtworkShareSource(
        modifier = modifier,
        prepare = { if (entry != null) session?.prepareAll(listOf(entry)) },
        entry = entry,
    )
}

@Composable
internal fun Modifier.sharedArtworkDestination(key: String?, zIndex: Float = 1f): Modifier =
    then(sharedArtworkModifier(key, sourceId = null, zIndex = zIndex, rotation = 0f))

/** The flight's spec: the page push's own duration and curve, so the art lands as the page settles. */
private fun <T> flightSpec() = tween<T>(PUSH_MS, easing = MiuixEasing)

/** A source when [sourceId] is set, else a destination. */
@Composable
private fun sharedArtworkModifier(
    key: String?,
    sourceId: Any?,
    zIndex: Float,
    rotation: Float,
): Modifier {
    val tilt = if (rotation != 0f) Modifier.graphicsLayer { rotationZ = rotation } else Modifier
    val session = LocalSharedArtwork.current
    val visibility = LocalNavAnimatedVisibilityScope.current
    val scope = LocalSharedTransitionScope.current
    val pageId = LocalNavPageId.current
    val reduced = LocalReducedMotion.current
    if (session == null || visibility == null || scope == null) return tilt
    // Keep remember order stable when [key] is still null (the Albums tile's fan fills in after load).
    val state = with(scope) { rememberSharedContentState(key = key ?: "artwork.idle") }
    val source = sourceId != null
    val entry = if (key == null || source) null else session.entryFor(pageId, key)
    val enabled = key != null && !reduced && if (source) session.matchesSource(key, sourceId!!) else entry != null
    if (!enabled) return tilt
    // Only the target side draws during a flight, so each side eases the tilt itself: a tilted
    // source leans in from upright as its page returns; a destination straightens out of the
    // source's tilt as it arrives. Upright art (nearly all of it) skips the extra animation.
    val lean = if (source) rotation else entry?.rotation ?: 0f
    val pose = if (lean == 0f) Modifier else {
        // 1 while this page is shown, 0 while it is off screen: how far into its own pose the art is.
        val shown by visibility.transition.animateFloat(transitionSpec = { flightSpec() }, label = "artworkPose") {
            if (it == EnterExitState.Visible) 1f else 0f
        }
        Modifier.graphicsLayer { rotationZ = lean * if (source) shown else 1f - shown }
    }
    val bounds = { _: Rect, _: Rect -> flightSpec<Rect>() }
    return with(scope) {
        Modifier.sharedElement(
            state,
            animatedVisibilityScope = visibility,
            boundsTransform = bounds,
            // A destination stacks like the source it pairs with, so the fan's covers keep their order in flight.
            zIndexInOverlay = entry?.zIndex ?: zIndex,
        )
    }.then(pose)
}

/** The cover the source showed, for a destination page to draw while its own data loads. */
@Composable
internal fun flyingCoverArt(key: String): String? =
    LocalSharedArtwork.current?.coverArtFor(LocalNavPageId.current, key)
