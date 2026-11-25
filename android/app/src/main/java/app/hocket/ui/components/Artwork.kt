package app.hocket.ui.components

import app.hocket.core.has
import app.hocket.core.engaged
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
import androidx.compose.runtime.key
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
 * read is asked for again, a few times with a growing wait, on one schedule per cover and size
 * however many places show it ([ArtworkMisses], [askForArtwork]); not while offline, and afresh
 * when the network comes back or a server does. Meanwhile a smaller copy already fetched for
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
    val requested = if (batterySaver.has(app.hocket.core.api.SaverFeature.SmallArtwork)) smallerSize(size) else size
    val cacheKey = coverArt?.let { ArtworkPathCache.key(it, requested) }
    // Unresolved (null: still asking the core) and resolved-to-nothing both show the id-seeded
    // gradient, so a shared-element remount never flashes an empty white card.
    val startedWithCachedPath = remember(cacheKey) { cacheKey?.let { ArtworkPathCache.get(it) } != null }
    val state = remember(cacheKey) { mutableStateOf(cacheKey?.let { ArtworkPathCache.get(it) }?.let { Resolved(it) }) }
    val reach = rememberArtworkReach(client)
    // A file that failed to load asks again (bounded by the shared schedule).
    var reload by remember(cacheKey) { mutableIntStateOf(0) }
    LaunchedEffect(cacheKey, reach, reload) {
        // A stand-in size (the core answers with what it has when the server can't be reached) is
        // asked for again on a network or server change, and never remembered as this size's file.
        if (state.value?.let { it.path != null && isExactArtwork(it.path, requested) } == true) return@LaunchedEffect
        if (coverArt == null || cacheKey == null) { state.value = Resolved(null); return@LaunchedEffect }
        val asked = askForArtwork(client, coverArt, requested, reach) { path ->
            if (path != null && isExactArtwork(path, requested)) ArtworkPathCache.put(cacheKey, path)
            state.value = Resolved(path)
        }
        // Nothing asked (offline with a miss known, or out of retries): unresolved is a miss now.
        if (!asked && state.value == null) state.value = Resolved(null)
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
        // Each file in its own painter (key): a painter handed a new file keeps drawing the one it
        // had until the new one decodes, which on a track change is another song's cover (#147).
        if (!loaded && stand != null && !standFailed) key(stand.path) {
            AsyncImage(
                model = remember(stand, requested) { ImageRequest.Builder(platformContext).data(File(stand.path.removePrefix("file://"))).size(requested).build() },
                contentDescription = null,
                contentScale = ContentScale.Crop,
                modifier = Modifier.fillMaxSize(),
                onError = { ArtworkPathCache.remove(stand.key); standFailed = true },
            )
        }
        if (path != null) key(path) {
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
                    cacheKey?.let { ArtworkPathCache.remove(it); ArtworkMisses.missed(it, ArtworkMisses.round(it), System.currentTimeMillis()) }
                    state.value = Resolved(null)
                    reload++
                },
            )
        }
    }
}

private class Resolved(val path: String?)

/** The core's cache sizes ("<id>_<size>.img"); a request snaps up to the nearest. */
private val CORE_ARTWORK_SIZES = intArrayOf(64, 160, 320, 640, 1280)

/**
 * Whether [path] is the core's file for [size] itself rather than another size standing in for it
 * (offline, the core answers with whichever size it kept). Paths of another shape count as exact.
 */
internal fun isExactArtwork(path: String, size: Int): Boolean {
    val m = Regex("_(\\d+)\\.img$").find(path) ?: return true
    val snapped = CORE_ARTWORK_SIZES.firstOrNull { it >= size } ?: CORE_ARTWORK_SIZES.last()
    return m.groupValues[1].toIntOrNull() == snapped
}

private const val ARTWORK_CROSSFADE_MS = 120
/** Asks after a failed one, 2 s, 4 s, 8 s then 16 s apart (and afresh once back online). */
internal const val ARTWORK_RETRIES = 4
internal const val ARTWORK_RETRY_MS = 2_000L

/**
 * What decides whether covers are worth asking for: [online] (the core's network isn't offline) and
 * how many servers are [reachable]. Metered or network-id changes don't change it, so they don't
 * restart lookups.
 */
@androidx.compose.runtime.Immutable
data class ArtworkReach(val online: Boolean, val reachable: Int)

@Composable
fun rememberArtworkReach(client: CoreClient = LocalCoreClient.current): ArtworkReach {
    val network by client.network.collectAsStateWithLifecycleCompat()
    val accounts by client.accounts.collectAsStateWithLifecycleCompat()
    val online = network?.kind != app.hocket.core.api.NetworkKind.Offline
    // Which accounts answer: a server coming back on the same network changes it.
    val reachable = accounts.count { it.reachable }
    return remember(online, reachable) { ArtworkReach(online, reachable) }
}

/**
 * Asks the core for [coverArt] at [size] on the shared schedule ([ArtworkMisses]), handing each
 * answer to [onAnswer]: at once, or when the cover's next retry is due, and again after each miss
 * until the retries run out. Offline, a cover already missed is not asked for at all. Returns once
 * a file came or no more asks are due; false when it asked nothing.
 */
internal suspend fun askForArtwork(client: CoreClient, coverArt: String, size: Int, reach: ArtworkReach, onAnswer: suspend (String?) -> Unit): Boolean {
    val key = ArtworkPathCache.key(coverArt, size)
    ArtworkMisses.observe(reach)
    var asked = false
    while (true) {
        val round = ArtworkMisses.round(key)
        val wait = ArtworkMisses.waitFor(key, reach.online, System.currentTimeMillis()) ?: return asked
        if (wait > 0) delay(wait)
        val path = client.artworkPath(coverArt, size)
        asked = true
        onAnswer(path)
        if (path != null) return true
        ArtworkMisses.missed(key, round, System.currentTimeMillis())
    }
}

/**
 * Process-wide memory of covers the core had no file for ("id@size"), so every place showing one
 * cover (the mini player, a queue row, the now-playing page, mirrored pairs) follows one retry
 * schedule rather than each its own, and a row scrolled back into view doesn't ask again for a
 * cover just missed. Cleared when the network comes back from offline or a server comes back;
 * a cover out of retries is asked again after [EXHAUSTED_TTL_MS].
 */
internal object ArtworkMisses {
    private const val MAX = 1024
    private const val EXHAUSTED_TTL_MS = 5 * 60_000L

    private class Miss(val misses: Int, val retryAt: Long)

    private val map = object : LinkedHashMap<String, Miss>(64, 0.75f, true) {
        override fun removeEldestEntry(eldest: MutableMap.MutableEntry<String, Miss>?): Boolean = size > MAX
    }
    private var last: ArtworkReach? = null

    /** Back online, or a server back: every miss is asked for afresh. */
    fun observe(reach: ArtworkReach) {
        synchronized(map) {
            val was = last
            if (was != null && ((reach.online && !was.online) || reach.reachable > was.reachable)) map.clear()
            last = reach
        }
    }

    /** Milliseconds to wait before asking for [key] (0: now), or null not to ask. */
    fun waitFor(key: String, online: Boolean, now: Long): Long? = synchronized(map) {
        val miss = map[key] ?: return 0L
        if (!online) return null
        if (miss.misses > ARTWORK_RETRIES) {
            if (now - miss.retryAt < EXHAUSTED_TTL_MS) return null
            map.remove(key)
            return 0L
        }
        (miss.retryAt - now).coerceAtLeast(0L)
    }

    /** How many misses [key] has had: what an ask passes to [missed]. */
    fun round(key: String): Int = synchronized(map) { map[key]?.misses ?: 0 }

    /**
     * The core had no file for [key] (or its file didn't load) on an ask made in [round]. Several
     * places sharing one answer count as one miss: only the first to report a round moves the
     * schedule on.
     */
    fun missed(key: String, round: Int, now: Long) {
        synchronized(map) {
            val miss = map[key]
            if ((miss?.misses ?: 0) != round) return
            val misses = round + 1
            map[key] = Miss(misses, now + (ARTWORK_RETRY_MS shl (misses - 1).coerceAtMost(ARTWORK_RETRIES - 1)))
        }
    }

    internal fun clear() { synchronized(map) { map.clear(); last = null } }
}

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
