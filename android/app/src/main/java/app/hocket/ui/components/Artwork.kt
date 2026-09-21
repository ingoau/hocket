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
import androidx.compose.runtime.produceState
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
import java.io.File

/**
 * Resolves `coverArt` through `Query.Artwork` at one of the fixed cache sizes and shows it with coil.
 * While resolving, or when the core has no file (fake core, offline), a deterministic two-tone
 * gradient derived from the id stands in, so grids never flash white and the fake core still looks
 * like a library. Battery saver drops one size step (design: battery saver).
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
    val path by produceState<String?>(initialValue = null, coverArt, requested) {
        value = client.artworkPath(coverArt, requested)
    }
    Box(modifier.clip(shape)) {
        ArtworkPlaceholder(coverArt, Modifier.fillMaxSize())
        path?.let {
            AsyncImage(
                model = File(it.removePrefix("file://")),
                contentDescription = contentDescription,
                contentScale = ContentScale.Crop,
                modifier = Modifier.fillMaxSize(),
            )
        }
    }
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
