package app.hocket.ui.components

import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.MaterialTheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Shape
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import app.hocket.core.ArtworkSizes
import app.hocket.core.client.CoreClient
import app.hocket.ui.LocalCoreClient
import app.hocket.ui.theme.ArtworkColors
import coil3.compose.AsyncImage
import coil3.compose.LocalPlatformContext
import coil3.request.ImageRequest
import coil3.request.crossfade
import androidx.compose.runtime.LaunchedEffect
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
    // Unresolved (null: still asking the core) is distinct from resolved-to-nothing: only the latter shows the gradient.
    val state = remember(cacheKey) { mutableStateOf(cacheKey?.let { ArtworkPathCache.get(it) }?.let { Resolved(it) }) }
    LaunchedEffect(cacheKey) {
        if (state.value?.path != null) return@LaunchedEffect
        val path = client.artworkPath(coverArt, requested)
        if (cacheKey != null && path != null) ArtworkPathCache.put(cacheKey, path)
        state.value = Resolved(path)
    }
    val resolved = state.value
    val platformContext = LocalPlatformContext.current
    Box(modifier.clip(shape).background(MaterialTheme.colorScheme.surfaceContainerHigh)) {
        val r = resolved
        if (coverArt == null || (r != null && r.path == null)) ArtworkPlaceholder(coverArt, Modifier.fillMaxSize())
        val path = r?.path
        if (path != null) {
            val request = remember(path, requested) {
                // Decoded at the requested artwork size, not the view's: a view that resizes (the
                // player's artwork going edge to edge) keeps its image instead of reloading it.
                ImageRequest.Builder(platformContext).data(File(path.removePrefix("file://"))).size(requested).crossfade(ARTWORK_CROSSFADE_MS).build()
            }
            AsyncImage(
                model = request,
                contentDescription = contentDescription,
                contentScale = ContentScale.Crop,
                modifier = Modifier.fillMaxSize(),
                onError = { cacheKey?.let { ArtworkPathCache.remove(it) } },
            )
        }
    }
}

private class Resolved(val path: String?)

private const val ARTWORK_CROSSFADE_MS = 300

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

fun smallerSize(size: Int): Int = when (size) {
    ArtworkSizes.FULL -> ArtworkSizes.GRID
    ArtworkSizes.GRID -> ArtworkSizes.LIST
    ArtworkSizes.LIST -> ArtworkSizes.THUMB
    else -> size
}

@Composable
fun <T> kotlinx.coroutines.flow.StateFlow<T>.collectAsStateWithLifecycleCompat(): androidx.compose.runtime.State<T> = collectAsStateWithLifecycle()
