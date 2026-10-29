package app.hocket.ui.components

import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Shape
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import app.hocket.core.ArtworkSizes
import app.hocket.core.client.CoreClient
import app.hocket.ui.LocalCoreClient
import app.hocket.ui.theme.ArtworkColors
import app.hocket.ui.theme.LocalDarkTheme
import coil3.compose.AsyncImage
import coil3.compose.LocalPlatformContext
import coil3.request.ImageRequest
import coil3.request.crossfade
import androidx.compose.runtime.LaunchedEffect
import kotlinx.coroutines.delay
import java.io.File

/**
 * Resolves `coverArt` through `Query.Artwork` at one of the fixed cache sizes and shows it with coil.
 *
 * Resolved paths are remembered in [ArtworkPathCache], so a row composed again (scrolling back, a
 * pager page, the player hero) starts from the known path on its first frame and coil serves the
 * bitmap from its memory cache without a placeholder flash. A first load crossfades in over the
 * plain `surfaceContainerHigh` backdrop. When the core has no file (fake core, offline), a
 * deterministic two-tone gradient derived from the id stands in, so the fake core still looks like
 * a library. Battery saver drops one size step (design: battery saver).
 *
 * A size the core could not fetch (a network drop: it answers "no file" for a failed fetch as for a
 * missing image; it remembers a 404, so asking again costs nothing then) or a file the loader cannot
 * read is asked for again, a few times with a growing wait, afresh on every network change, and meanwhile a smaller copy already fetched for
 * another place (the mini player's) stands in for the gradient.
 */
@Composable
fun Artwork(
    coverArt: String?,
    size: Int,
    contentDescription: String?,
    modifier: Modifier = Modifier,
    shape: Shape = RoundedCornerShape(12.dp),
    client: CoreClient = LocalCoreClient.current,
) {
    val batterySaver by client.batterySaver.collectAsStateWithLifecycleCompat()
    val requested = if (batterySaver) smallerSize(size) else size
    val cacheKey = coverArt?.let { ArtworkPathCache.key(it, requested) }
    // Unresolved (null: still asking the core) and resolved-to-nothing both show the id-seeded
    // gradient, so a shared-element remount never flashes an empty white card.
    val startedWithCachedPath = remember(cacheKey) { cacheKey?.let { ArtworkPathCache.get(it) } != null }
    val state = remember(cacheKey) { mutableStateOf(cacheKey?.let { ArtworkPathCache.get(it) }?.let { Resolved(it) }) }
    val network by client.network.collectAsStateWithLifecycleCompat()
    // A fresh run of retries on each network change (the link may take a moment to carry traffic).
    var attempt by remember(cacheKey, network) { mutableIntStateOf(0) }
    LaunchedEffect(cacheKey, attempt, network) {
        if (state.value?.path != null) return@LaunchedEffect
        val path = client.artworkPath(coverArt, requested)
        if (cacheKey != null && path != null) ArtworkPathCache.put(cacheKey, path)
        state.value = Resolved(path)
        if (path == null && coverArt != null && attempt < ARTWORK_RETRIES) {
            delay(ARTWORK_RETRY_MS shl attempt)
            attempt++
        }
    }
    val resolved = state.value
    val platformContext = LocalPlatformContext.current
    val r = resolved
    val path = r?.path
    // Until this size shows, a smaller file already known for the same cover, scaled up (looked up
    // again after each answer, so one fetched meanwhile for another place is picked up).
    val stand = remember(coverArt, requested, r) {
        if (coverArt != null && (path == null || !startedWithCachedPath)) ArtworkPathCache.smaller(coverArt, requested) else null
    }
    // A remount that already knows the file (shared-element overlay, a second card of the same
    // album) must not paint the empty surfaceContainerHigh card or crossfade in over it — that is
    // the white flash on the Albums tile fan and the albums page Recent row. Unresolved still
    // paints the id-seeded gradient so a dest overlay's first frame is never a blank card.
    Box(
        modifier.clip(shape).then(
            if (path == null || !startedWithCachedPath) Modifier.background(MaterialTheme.colorScheme.surfaceContainerHigh)
            else Modifier
        ),
    ) {
        var standFailed by remember(stand) { mutableStateOf(false) }
        var loaded by remember(path) { mutableStateOf(false) }
        // The gradient stays under the stand-in while that decodes, never an empty card.
        if (path == null) ArtworkPlaceholder(coverArt, Modifier.fillMaxSize())
        if (!loaded && stand != null && !standFailed) {
            AsyncImage(
                model = remember(stand, requested) { ImageRequest.Builder(platformContext).data(File(stand.path.removePrefix("file://"))).size(requested).build() },
                contentDescription = null,
                contentScale = ContentScale.Crop,
                modifier = Modifier.fillMaxSize(),
                onError = { ArtworkPathCache.remove(stand.key); standFailed = true },
            )
        }
        if (path != null) {
            val request = remember(path, requested, startedWithCachedPath) {
                // Decoded at the requested artwork size, not the view's: a view that resizes (the
                // player's artwork going edge to edge) keeps its image instead of reloading it.
                ImageRequest.Builder(platformContext).data(File(path.removePrefix("file://"))).size(requested)
                    .crossfade(if (startedWithCachedPath) 0 else ARTWORK_CROSSFADE_MS).build()
            }
            AsyncImage(
                model = request,
                contentDescription = contentDescription,
                contentScale = ContentScale.Crop,
                modifier = Modifier.fillMaxSize(),
                onSuccess = { loaded = true },
                // Asked again (the core fetches afresh a file that has gone), a bounded number of times.
                onError = {
                    cacheKey?.let { ArtworkPathCache.remove(it) }
                    state.value = Resolved(null)
                    if (attempt < ARTWORK_RETRIES) attempt++
                },
            )
        }
    }
}

private class Resolved(val path: String?)

private const val ARTWORK_CROSSFADE_MS = 120
/** Asks after a failed one, 2 s, 4 s, 8 s then 16 s apart (and on each network change after). */
internal const val ARTWORK_RETRIES = 4
internal const val ARTWORK_RETRY_MS = 2_000L

/**
 * Process-wide memory of `Query.Artwork` answers ("id@size" to a file path), bounded LRU. Only
 * found files are kept, so art that arrives later (a download, going online) is still picked up.
 */
object ArtworkPathCache {
    private const val MAX = 1024
    private val map = object : LinkedHashMap<String, String>(256, 0.75f, true) {
        override fun removeEldestEntry(eldest: MutableMap.MutableEntry<String, String>?): Boolean = size > MAX
    }
    fun key(id: String, size: Int): String = "$id@$size"
    fun get(key: String): String? = synchronized(map) { map[key] }
    fun put(key: String, path: String) { synchronized(map) { map[key] = path } }
    fun remove(key: String) { synchronized(map) { map.remove(key) } }

    /** The largest file known for [id] below [size], or null. */
    fun smaller(id: String, size: Int): Known? = synchronized(map) {
        SIZES.filter { it < size }.sortedDescending().firstNotNullOfOrNull { s -> key(id, s).let { k -> map[k]?.let { Known(k, it) } } }
    }

    class Known(val key: String, val path: String)

    private val SIZES = listOf(ArtworkSizes.THUMB, ArtworkSizes.LIST, ArtworkSizes.GRID, ArtworkSizes.FULL, ArtworkSizes.HERO)
}

@Composable
fun ArtworkPlaceholder(id: String?, modifier: Modifier = Modifier) {
    val seed = remember(id) { ArtworkColors.seedFor(id) }
    val second = remember(id) { ArtworkColors.seedFor((id ?: "") + "~") }
    Canvas(modifier.background(MaterialTheme.colorScheme.surfaceContainerHigh)) {
        drawRect(Brush.linearGradient(listOf(seed.copy(alpha = 0.85f), second.copy(alpha = 0.7f))))
        drawCircle(Color.White.copy(alpha = 0.10f), radius = size.minDimension * 0.42f)
        drawCircle(Color.Black.copy(alpha = 0.18f), radius = size.minDimension * 0.08f)
    }
}

/**
 * Artwork for something with no cover (a filter, a genre, a coverless queue): its icon in [accent]
 * on a soft, see-through tint of it (the desktop's `--accent-soft`), so it reads as tonal on any
 * surface, a card's or the screen's, in either theme.
 */
@Composable
fun IconArt(icon: ImageVector, shape: Shape, modifier: Modifier = Modifier, iconSize: Dp = 24.dp, accent: Color = MaterialTheme.colorScheme.primary) {
    // A touch stronger in the dark theme, where the light accent over a dark card reads fainter.
    Box(modifier.background(accent.copy(alpha = if (LocalDarkTheme.current) 0.28f else 0.2f), shape), contentAlignment = Alignment.Center) {
        Icon(icon, null, Modifier.size(iconSize), tint = accent)
    }
}

fun smallerSize(size: Int): Int = when (size) {
    ArtworkSizes.HERO -> ArtworkSizes.FULL
    ArtworkSizes.FULL -> ArtworkSizes.GRID
    ArtworkSizes.GRID -> ArtworkSizes.LIST
    ArtworkSizes.LIST -> ArtworkSizes.THUMB
    else -> size
}

@Composable
fun <T> kotlinx.coroutines.flow.StateFlow<T>.collectAsStateWithLifecycleCompat(): androidx.compose.runtime.State<T> = collectAsStateWithLifecycle()
