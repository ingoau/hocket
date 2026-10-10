package app.hocket.ui.player

import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.graphics.PointF
import android.media.FaceDetector
import android.util.LruCache
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.runtime.Composable
import androidx.compose.runtime.State
import androidx.compose.runtime.produceState
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.BlurredEdgeTreatment
import androidx.compose.ui.draw.blur
import androidx.compose.ui.draw.clipToBounds
import androidx.compose.ui.draw.drawWithContent
import androidx.compose.ui.graphics.BlendMode
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.CompositingStrategy
import androidx.compose.ui.graphics.RectangleShape
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.layout.layout
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.unit.Constraints
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import app.hocket.core.ArtworkLayouts
import app.hocket.core.ArtworkSizes
import app.hocket.core.api.ArtworkEdge
import app.hocket.core.api.ArtworkLayout
import app.hocket.core.api.ArtworkLayoutRequest
import app.hocket.core.api.ArtworkStyle
import app.hocket.core.api.FaceRect
import app.hocket.ui.LocalCoreClient
import app.hocket.ui.components.Artwork
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import java.io.File
import kotlin.math.max
import kotlin.math.roundToInt
import app.hocket.core.api.ImmersiveArtwork as Preference

/**
 * Immersive artwork (docs/design.md, "Immersive artwork"): the full player's artwork edge to edge
 * from the top of the screen, carried on below it by a reflection ([ArtworkStyle.Mirror]) or by its
 * own colours ([ArtworkStyle.Extend]), or the usual card. The core decides which
 * (`hocket_core::artwork`, through [ArtworkLayouts]); this side decodes the cover, finds faces in it
 * (the framework's [FaceDetector]: frontal faces, no extra dependency) and draws the result.
 */
object ImmersiveArtwork {
    /** The side the cover is decoded to for the classifier (it resamples to 128 itself). */
    private const val SIDE = 256

    /** Immersive only where the square artwork leaves room for the controls: width ≤ this share of the height. */
    const val MAX_WIDTH_SHARE = 0.62f

    private val cache = LruCache<String, ArtworkLayout>(64)

    /** The `display.immersiveArtwork` setting from its stored JSON value. */
    fun preference(json: String?): Preference {
        val v = json?.trim()?.trim('"') ?: return Preference.Automatic
        return Preference.entries.firstOrNull { it.string == v } ?: Preference.Automatic
    }

    /** The layout for the cover at [path], or null when it cannot be decoded or classified. */
    suspend fun analyse(path: String, preference: Preference): ArtworkLayout? = withContext(Dispatchers.Default) {
        val key = "$path|${preference.string}"
        cache.get(key)?.let { return@withContext it }
        val file = File(path.removePrefix("file://"))
        if (!file.exists()) return@withContext null
        val bounds = BitmapFactory.Options().apply { inJustDecodeBounds = true }
        BitmapFactory.decodeFile(file.absolutePath, bounds)
        val longest = max(bounds.outWidth, bounds.outHeight)
        if (longest <= 0) return@withContext null
        var sample = 1
        while (longest / (sample * 2) >= SIDE) sample *= 2
        val decoded = BitmapFactory.decodeFile(file.absolutePath, BitmapFactory.Options().apply { inSampleSize = sample; inPreferredConfig = Bitmap.Config.ARGB_8888 })
            ?: return@withContext null
        val scale = minOf(1f, SIDE.toFloat() / max(decoded.width, decoded.height))
        val bitmap = if (scale < 1f) Bitmap.createScaledBitmap(decoded, (decoded.width * scale).roundToInt().coerceAtLeast(1), (decoded.height * scale).roundToInt().coerceAtLeast(1), true) else decoded
        try {
            val w = bitmap.width
            val h = bitmap.height
            val px = IntArray(w * h)
            bitmap.getPixels(px, 0, w, 0, 0, w, h)
            val rgba = ByteArray(w * h * 4)
            for (i in px.indices) {
                val c = px[i]
                rgba[i * 4] = (c shr 16).toByte()
                rgba[i * 4 + 1] = (c shr 8).toByte()
                rgba[i * 4 + 2] = c.toByte()
                rgba[i * 4 + 3] = (c ushr 24).toByte()
            }
            ArtworkLayouts.layout(rgba, w, h, ArtworkLayoutRequest(faces = faces(bitmap), preference = preference))?.also { cache.put(key, it) }
        } finally {
            if (bitmap !== decoded) bitmap.recycle()
            decoded.recycle()
        }
    }

    /**
     * Faces as fractions of the image. [FaceDetector] wants RGB_565 and an even width, and reports
     * the eyes' midpoint and spacing; a face is about two eye-spacings wide and 2.4 tall.
     */
    private fun faces(bitmap: Bitmap): List<FaceRect> {
        val w = bitmap.width and 1.inv()
        val h = bitmap.height
        if (w < 2 || h < 2) return emptyList()
        val rgb565 = Bitmap.createBitmap(w, h, Bitmap.Config.RGB_565)
        android.graphics.Canvas(rgb565).drawBitmap(bitmap, 0f, 0f, null)
        val found = arrayOfNulls<FaceDetector.Face>(8)
        val n = try { FaceDetector(w, h, found.size).findFaces(rgb565, found) } catch (e: Exception) { 0 } finally { rgb565.recycle() }
        val mid = PointF()
        return (0 until n).mapNotNull { found[it] }.map { f ->
            f.getMidPoint(mid)
            val e = f.eyesDistance()
            FaceRect(x = ((mid.x - e) / w).toDouble(), y = ((mid.y - e) / h).toDouble(), w = (2 * e / w).toDouble(), h = (2.4f * e / h).toDouble())
        }
    }
}

/** The layout for [coverArt] (null while it is worked out, without a cover, or without the native core). */
@Composable
fun rememberArtworkLayout(coverArt: String?, preference: Preference): State<ArtworkLayout?> {
    val client = LocalCoreClient.current
    return produceState<ArtworkLayout?>(initialValue = null, coverArt, preference) {
        value = coverArt?.let { client.artworkPath(it, ArtworkSizes.GRID) }?.let { ImmersiveArtwork.analyse(it, preference) }
    }
}

private fun argb(c: UInt): Color = Color(c.toInt() or 0xFF000000.toInt())

/** [colors] averaged over a window of `2 × radius + 1`: the same edge, blurred sideways. */
private fun soften(colors: List<Color>, radius: Int): List<Color> = colors.indices.map { i ->
    val span = colors.subList(maxOf(0, i - radius), minOf(colors.size, i + radius + 1))
    Color(span.map { it.red }.average().toFloat(), span.map { it.green }.average().toFloat(), span.map { it.blue }.average().toFloat())
}

/** Keeps the content only between [stops] (fractions of [unit] px from the top, with alphas). */
private fun Modifier.verticalMask(unit: () -> Float, vararg stops: Pair<Float, Float>): Modifier = this
    .graphicsLayer { compositingStrategy = CompositingStrategy.Offscreen }
    .drawWithContent {
        drawContent()
        val u = unit().coerceAtLeast(1f)
        val brush = Brush.verticalGradient(*stops.map { (y, a) -> (y * u / size.height).coerceIn(0f, 1f) to Color.Black.copy(alpha = a) }.toTypedArray(), startY = 0f, endY = size.height)
        drawRect(brush, blendMode = BlendMode.DstIn)
    }

/** One colour carried on: the artwork already ends in it, so no fade and no blur at the seam. */
internal fun ArtworkEdge.isFlat(): Boolean = style == ArtworkStyle.Extend && edgeColors.distinct().size <= 1

/** The share of the artwork that fades into the continuation. */
internal const val FEATHER = 0.18f
/** Blur at the seam and far from it, as shares of the artwork's side. */
private const val BLUR_NEAR = 0.025f
private const val BLUR_FAR = 0.067f

/**
 * What carries the immersive artwork on below it, between the player's backdrop and the page, laid
 * out from [FEATHER] above the big artwork's bottom edge ([HeroGeometry.bigSlot]) down. The seam
 * never cuts over: the artwork fades out over its last [FEATHER] ([immersiveFade]) onto a blurred
 * copy of itself drawn here, and the continuation starts at that same blur:
 *
 * - [ArtworkStyle.Mirror]: the reflection, blurring further with distance;
 * - [ArtworkStyle.Extend]: a short blurred reflection bridging into the colours along the edge,
 *   softening sideways with distance;
 * - a flat extension ([isFlat]): none of that, the artwork already ends in that colour, so it stays
 *   crisp to its edge and the colour simply carries on.
 *
 * Blurs clamp at their edges (the same edge row on both sides of the seam) and need Android 12;
 * before it the copies are sharp and only fade. Everything fades into the moving backdrop, except
 * over a light continuation or a flat colour, which carry on in their own colour (a white cover
 * stays white). Under the controls, the scrim the core worked out for 4.5:1, and under light
 * controls a fade to near black at the bottom of the screen.
 */
@Composable
internal fun ImmersiveContinuation(coverArt: String?, edge: ArtworkEdge, hero: HeroGeometry, modifier: Modifier = Modifier) {
    val base = argb(edge.baseColor)
    val colors = edge.edgeColors.map(::argb)
    val flat = edge.isFlat()
    val side = { hero.bigSlot().width }
    val density = LocalDensity.current
    val sidePx = hero.bigSlot().width
    val near = with(density) { (sidePx * BLUR_NEAR).toDp() }
    val far = with(density) { (sidePx * BLUR_FAR).toDp() }
    /** The artwork (or its reflection) at the artwork's size, placed [dy] px down, blurred by [blur]. */
    @Composable
    fun Copy(flipped: Boolean, blur: Dp, dy: () -> Float, modifier: Modifier = Modifier) = Box(
        modifier
            .fillMaxSize()
            .clipToBounds()
            .layout { m, c ->
                val s = side().roundToInt().coerceAtLeast(1)
                val p = m.measure(Constraints.fixed(c.maxWidth, s))
                layout(c.maxWidth, c.maxHeight) { p.place(0, dy().roundToInt()) }
            },
    ) {
        Box(Modifier.fillMaxSize().graphicsLayer { if (flipped) scaleY = -1f }.let { if (blur > 0.dp) it.blur(blur, BlurredEdgeTreatment.Rectangle) else it }) {
            Artwork(coverArt, ArtworkSizes.FULL, null, Modifier.fillMaxSize(), RectangleShape)
        }
    }
    Box(
        modifier
            .clearAndSetSemantics { }
            .layout { measurable, constraints ->
                val slot = hero.bigSlot()
                val top = (slot.bottom - if (flat) 0f else slot.width * FEATHER).roundToInt().coerceIn(0, constraints.maxHeight)
                val h = (constraints.maxHeight - top).coerceAtLeast(0)
                val placeable = measurable.measure(Constraints.fixed(constraints.maxWidth, h))
                layout(constraints.maxWidth, constraints.maxHeight) { placeable.place(0, top) }
            },
    ) {
        if (flat || edge.light) Box(Modifier.fillMaxSize().background(base))
        // Everything from here down is measured from the seam, [FEATHER] of the side below this box's top.
        val seam = { if (flat) 0f else side() * FEATHER }
        val below = Modifier.layout { m, c ->
            val top = seam().roundToInt().coerceIn(0, c.maxHeight)
            val p = m.measure(Constraints.fixed(c.maxWidth, (c.maxHeight - top).coerceAtLeast(0)))
            layout(c.maxWidth, c.maxHeight) { p.place(0, top) }
        }
        if (!flat) {
            // Under the artwork's fading edge: the same artwork, blurred (its bottom at the seam).
            Copy(flipped = false, blur = near, dy = { seam() - side() }, modifier = Modifier.layout { m, c ->
                val h = seam().roundToInt().coerceIn(0, c.maxHeight)
                val p = m.measure(Constraints.fixed(c.maxWidth, h))
                layout(c.maxWidth, c.maxHeight) { p.place(0, 0) }
            })
        }
        Box(below) {
            when {
                edge.style == ArtworkStyle.Mirror -> {
                    Copy(flipped = true, blur = far, dy = { 0f }, modifier = Modifier.verticalMask(side, 0f to 1f, 0.5f to 1f, 0.9f to 0f))
                    Copy(flipped = true, blur = near, dy = { 0f }, modifier = Modifier.verticalMask(side, 0f to 1f, 0.12f to 1f, 0.4f to 0f))
                }
                !flat -> {
                    Box(Modifier.fillMaxSize().verticalMask(side, 0f to 1f, 0.5f to 1f, 0.9f to 0f).background(Brush.horizontalGradient(soften(colors, 6))))
                    Box(Modifier.fillMaxSize().verticalMask(side, 0f to 1f, 0.15f to 1f, 0.45f to 0f).background(Brush.horizontalGradient(soften(colors, 2))))
                    // The bridge: a short blurred reflection, so the colours take over from exactly what the softened edge shows.
                    Copy(flipped = true, blur = near, dy = { 0f }, modifier = Modifier.verticalMask(side, 0f to 1f, 0.1f to 0f))
                }
            }
            if (edge.scrim > 0.0) {
                val scrim = (if (edge.light) Color.White else Color.Black).copy(alpha = edge.scrim.toFloat())
                Box(Modifier.fillMaxSize().background(Brush.verticalGradient(0f to scrim.copy(alpha = 0f), 0.2f to scrim, 1f to scrim)))
            }
        }
        // Under light controls, the continuation darkens to near black towards the bottom of the
        // screen (the mode pills sit on it). Not under dark controls: they need the light colour.
        if (!edge.light) {
            Box(Modifier.fillMaxSize().background(Brush.verticalGradient(0.45f to Color.Transparent, 1f to Color.Black.copy(alpha = BOTTOM_FADE))))
        }
    }
}

/** How dark the bottom of the screen gets under light controls. */
private const val BOTTOM_FADE = 0.85f

/**
 * The immersive artwork's own last [FEATHER] fading out onto the continuation's blurred copy, as
 * much as [fraction] (1 in artwork mode, 0 as a thumbnail). Not for a flat extension, which stays
 * crisp to its edge.
 */
internal fun Modifier.immersiveFade(edge: ArtworkEdge?, fraction: () -> Float): Modifier =
    if (edge == null || edge.isFlat()) this else this
        .graphicsLayer { compositingStrategy = CompositingStrategy.Offscreen }
        .drawWithContent {
            drawContent()
            val f = fraction().coerceIn(0f, 1f)
            if (f <= 0f) return@drawWithContent
            val from = size.height * (1f - FEATHER)
            drawRect(
                Brush.verticalGradient(0f to Color.Black, 1f to Color.Black.copy(alpha = 1f - f), startY = from, endY = size.height),
                topLeft = androidx.compose.ui.geometry.Offset(0f, from),
                size = size.copy(height = size.height - from),
                blendMode = BlendMode.DstIn,
            )
        }

/**
 * Drawn over the immersive artwork itself, fading with [fraction] (1 in artwork mode): where the top
 * of the artwork and the controls disagree (dark controls over a dark top, or light over a light
 * one), a scrim under the header.
 */
@Composable
internal fun ImmersiveArtworkOverlay(layout: ArtworkLayout, fraction: () -> Float) {
    val edge = layout.bottom
    if (layout.topLight == edge.light) return
    val scrim = if (edge.light) Color.White else Color.Black
    Box(Modifier.fillMaxSize().graphicsLayer { alpha = fraction() }.clearAndSetSemantics { }) {
        Box(Modifier.fillMaxWidth().fillMaxHeight(0.22f).background(Brush.verticalGradient(0f to scrim.copy(alpha = 0.45f), 1f to scrim.copy(alpha = 0f))))
    }
}
