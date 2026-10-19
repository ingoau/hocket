package app.hocket.ui.player

import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.graphics.PointF
import android.media.FaceDetector
import android.util.LruCache
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.runtime.Composable
import androidx.compose.runtime.State
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import app.hocket.ui.components.ArtworkPathCache
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.BlurredEdgeTreatment
import androidx.compose.ui.draw.blur
import androidx.compose.ui.draw.drawBehind
import androidx.compose.ui.draw.clipToBounds
import androidx.compose.ui.graphics.luminance
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
import app.hocket.core.api.ArtworkTheme
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
    /** Covers worked out to no layout (undecodable, or no native core): known without asking again. */
    private val none = LruCache<String, Boolean>(64)

    /** Tests only: a layout per cover id, in place of decoding and classifying (null: a card). */
    @androidx.annotation.VisibleForTesting
    internal var layoutsForTest: ((String) -> ArtworkLayout?)? = null

    private fun key(path: String, preference: Preference, dark: Boolean) = "$path|${preference.string}|$dark"

    /**
     * The layout already worked out for [coverArt] (its path known to [ArtworkPathCache]), at once:
     * null when it has not been (yet), else [Resolved] (whose layout is null for a card).
     */
    fun cached(coverArt: String?, preference: Preference, dark: Boolean): Resolved? {
        if (coverArt == null) return Resolved(null)
        layoutsForTest?.let { return Resolved(it(coverArt)) }
        val path = ArtworkPathCache.get(ArtworkPathCache.key(coverArt, ArtworkSizes.GRID)) ?: return null
        val k = key(path, preference, dark)
        cache.get(k)?.let { return Resolved(it) }
        return if (none.get(k) == true) Resolved(null) else null
    }

    /** A worked-out layout: [layout] null means a card. */
    class Resolved(val layout: ArtworkLayout?)

    /** The `display.immersiveArtwork` setting from its stored JSON value. */
    fun preference(json: String?): Preference {
        val v = json?.trim()?.trim('"') ?: return Preference.Automatic
        return Preference.entries.firstOrNull { it.string == v } ?: Preference.Automatic
    }

    /** The layout for the cover at [path], or null when it cannot be decoded or classified. */
    suspend fun analyse(path: String, preference: Preference, dark: Boolean): ArtworkLayout? = withContext(Dispatchers.Default) {
        val key = key(path, preference, dark)
        cache.get(key)?.let { return@withContext it }
        if (none.get(key) == true) return@withContext null
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
            ArtworkLayouts.layout(rgba, w, h, ArtworkLayoutRequest(faces = faces(bitmap), preference = preference, theme = if (dark) ArtworkTheme.Dark else ArtworkTheme.Light))
                .also { if (it != null) cache.put(key, it) else none.put(key, true) }
        } finally {
            if (bitmap !== decoded) bitmap.recycle()
            decoded.recycle()
        }
    }

    /**
     * Faces as fractions of the image. [FaceDetector] wants RGB_565 and an even width, and reports
     * the eyes' midpoint and spacing; a face is about two eye-spacings wide and 2.4 tall, ending
     * between the mouth and the chin: the core only rules a mirror out for a face whose box reaches
     * the reflection zone, and a chin tip alone reflected at the seam doesn't read as a face.
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

/**
 * The layout for [coverArt]: null while it is worked out, else [ImmersiveArtwork.Resolved] (its
 * layout null for a card, without a cover, or without the native core). One already worked out (a
 * neighbour's, say) is there from the first frame. [dark] is the app's theme: the controls follow it
 * unless the cover is almost all dark or light.
 */
@Composable
fun rememberArtworkLayout(coverArt: String?, preference: Preference, dark: Boolean): State<ImmersiveArtwork.Resolved?> {
    val client = LocalCoreClient.current
    val state = remember(coverArt, preference, dark) { mutableStateOf(ImmersiveArtwork.cached(coverArt, preference, dark)) }
    LaunchedEffect(state) {
        if (state.value != null || coverArt == null) return@LaunchedEffect
        val path = client.artworkPath(coverArt, ArtworkSizes.GRID)
        if (path != null) ArtworkPathCache.put(ArtworkPathCache.key(coverArt, ArtworkSizes.GRID), path)
        state.value = ImmersiveArtwork.Resolved(path?.let { ImmersiveArtwork.analyse(it, preference, dark) })
    }
    return state
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

/**
 * One colour carried on: the artwork already ends in it, so no fade and no blur at the seam. The
 * core says so ("extend:flat"); a plain but textured edge ("extend:plain") may smooth into one
 * colour too, yet fades in like any other extension.
 */
internal fun ArtworkEdge.isFlat(): Boolean = style == ArtworkStyle.Extend && reason == "extend:flat"

/** The share of the artwork that fades into the continuation. */
internal const val FEATHER = 0.18f
/** Blur at the seam and far from it, as shares of the artwork's side. */
private const val BLUR_NEAR = 0.025f
private const val BLUR_FAR = 0.1f

/**
 * What carries the immersive artwork on below it, between the player's backdrop and the page, laid
 * out from [FEATHER] above the immersive artwork's bottom edge ([HeroGeometry.immersiveSlot]) down. The seam
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
 * stays white). Under the controls, the scrim the core worked out for 4.5:1, and from the title
 * down a partial fade to black under light controls or white under dark ones. The controls follow
 * the app's theme unless the cover is almost all dark or light ([rememberArtworkLayout]).
 */
@Composable
internal fun ImmersiveContinuation(coverArt: String?, edge: ArtworkEdge, hero: HeroGeometry, modifier: Modifier = Modifier) {
    val base = argb(edge.baseColor)
    val colors = edge.edgeColors.map(::argb)
    val flat = edge.isFlat()
    val side = { hero.immersiveSlot().width }
    val density = LocalDensity.current
    val sidePx = hero.immersiveSlot().width
    val near = with(density) { (sidePx * BLUR_NEAR).toDp() }
    val far = with(density) { (sidePx * BLUR_FAR).toDp() }
    /** Where the seam (the artwork's bottom edge) is in this box: [FEATHER] of the side below its top. */
    val seam = { if (flat) 0f else side() * FEATHER }

    /**
     * The artwork with its reflection below it, blurred by [blur] as one image, its seam on ours:
     * both sides of the seam come from one blur, so it stays continuous (two separately blurred
     * copies each fade at their own edge, which shows as a dark line there). Masked to [stops]
     * (fractions of the side from this box's top, with alphas).
     */
    @Composable
    fun Reflected(blur: Dp, vararg stops: Pair<Float, Float>) = Box(
        Modifier
            .fillMaxSize()
            .verticalMask(side, *stops)
            .clipToBounds()
            .layout { m, c ->
                val s = side().roundToInt().coerceAtLeast(1)
                val p = m.measure(Constraints.fixed(c.maxWidth, 2 * s))
                layout(c.maxWidth, c.maxHeight) { p.place(0, (seam() - s).roundToInt()) }
            },
    ) {
        Column(Modifier.fillMaxSize().let { if (blur > 0.dp) it.blur(blur, BlurredEdgeTreatment.Rectangle) else it }) {
            Artwork(coverArt, ArtworkSizes.GRID, null, Modifier.fillMaxWidth().weight(1f), RectangleShape)
            Artwork(coverArt, ArtworkSizes.GRID, null, Modifier.fillMaxWidth().weight(1f).graphicsLayer { scaleY = -1f }, RectangleShape)
        }
    }
    Box(
        modifier
            .clearAndSetSemantics { }
            .layout { measurable, constraints ->
                // Laid out for an immersive artwork's square, whatever the current one's style.
                val slot = hero.immersiveSlot()
                val top = (slot.bottom - if (flat) 0f else slot.width * FEATHER).roundToInt().coerceIn(0, constraints.maxHeight)
                val h = (constraints.maxHeight - top).coerceAtLeast(0)
                val placeable = measurable.measure(Constraints.fixed(constraints.maxWidth, h))
                layout(constraints.maxWidth, constraints.maxHeight) { placeable.place(0, top) }
            },
    ) {
        if (flat || edge.light) Box(Modifier.fillMaxSize().background(base))
        val f = FEATHER
        when {
            edge.style == ArtworkStyle.Mirror -> {
                Reflected(far, 0f to 1f, f + 0.5f to 1f, f + 0.9f to 0f)
                // Only a thin band at the seam stays this sharp: the title sits right under the artwork.
                Reflected(near, 0f to 1f, f + 0.03f to 1f, f + 0.1f to 0f)
            }
            !flat -> {
                // The edge's colours from the seam down, softening sideways with distance.
                val below = Modifier.layout { m, c ->
                    val top = seam().roundToInt().coerceIn(0, c.maxHeight)
                    val p = m.measure(Constraints.fixed(c.maxWidth, (c.maxHeight - top).coerceAtLeast(0)))
                    layout(c.maxWidth, c.maxHeight) { p.place(0, top) }
                }
                Box(below) {
                    Box(Modifier.fillMaxSize().verticalMask(side, 0f to 1f, 0.5f to 1f, 0.9f to 0f).background(Brush.horizontalGradient(soften(colors, 6))))
                    Box(Modifier.fillMaxSize().verticalMask(side, 0f to 1f, 0.15f to 1f, 0.45f to 0f).background(Brush.horizontalGradient(soften(colors, 2))))
                }
                // Under the artwork's fading edge and just past the seam: the artwork blurred, then
                // its short reflection, bridging into the colours.
                Reflected(near, 0f to 1f, f to 1f, f + 0.1f to 0f)
            }
        }
        // The scrim the controls need, already at full strength where they start (right at the
        // seam): it comes in under the artwork's fading edge (or, under a crisp flat edge, just
        // past it).
        if (edge.scrim > 0.0) {
            val scrim = (if (edge.light) Color.White else Color.Black).copy(alpha = edge.scrim.toFloat())
            Box(Modifier.fillMaxSize().drawBehind {
                val s = side()
                val (from, to) = if (flat) 0f to s * 0.12f else 0f to seam()
                drawRect(Brush.verticalGradient(0f to scrim.copy(alpha = 0f), 1f to scrim, startY = from, endY = to.coerceAtLeast(from + 1f)))
            })
        }
        // From the title down, the continuation fades part of the way to the controls' opposite:
        // black under light controls (dark mode, or a dark cover), white under dark ones.
        val end = (if (edge.light) Color.White else Color.Black).copy(alpha = BOTTOM_FADE)
        Box(Modifier.fillMaxSize().drawBehind {
            drawRect(Brush.verticalGradient(0f to Color.Transparent, 1f to end, startY = seam(), endY = size.height))
        })
    }
}

/** How far the bottom of the screen fades to black or white. */
private const val BOTTOM_FADE = 0.6f

/**
 * The immersive artwork's own last [FEATHER] fading out onto the continuation's blurred copy, as
 * much as [fraction] (1 in artwork mode, 0 as a thumbnail), and, where its [top] strip is not a
 * flat colour, its first [FEATHER] fading in from that strip's blurred copy as much as [topFraction]. Not for a flat extension, which stays
 * crisp to its edge, unless [evenFlat] (the artwork in flight, whose flat continuation fades out
 * under it: its edge fades with it rather than standing as a hard line).
 */
internal fun Modifier.immersiveFade(edge: ArtworkEdge?, fraction: () -> Float, evenFlat: Boolean = false, top: ArtworkEdge? = null, topFraction: () -> Float = fraction): Modifier {
    val bottom = edge != null && (!edge.isFlat() || evenFlat)
    // The top fades onto [ImmersiveTopStrip]'s blurred copy, unless that is a crisp flat colour.
    val fadeTop = edge != null && top != null && !top.isFlat()
    if (!bottom && !fadeTop) return this
    return this
        .graphicsLayer { compositingStrategy = CompositingStrategy.Offscreen }
        .drawWithContent {
            drawContent()
            val f = fraction().coerceIn(0f, 1f)
            if (bottom && f > 0f) {
                val from = size.height * (1f - FEATHER)
                drawRect(
                    Brush.verticalGradient(0f to Color.Black, 1f to Color.Black.copy(alpha = 1f - f), startY = from, endY = size.height),
                    topLeft = androidx.compose.ui.geometry.Offset(0f, from),
                    size = size.copy(height = size.height - from),
                    blendMode = BlendMode.DstIn,
                )
            }
            val t = topFraction().coerceIn(0f, 1f)
            if (fadeTop && t > 0f) {
                val to = size.height * FEATHER
                drawRect(
                    Brush.verticalGradient(0f to Color.Black.copy(alpha = 1f - t), 1f to Color.Black, startY = 0f, endY = to),
                    size = size.copy(height = to),
                    blendMode = BlendMode.DstIn,
                )
            }
        }
}

/** The colour of the strip above an immersive artwork ([ImmersiveTopStrip]): its top edge's colours, averaged. */
internal fun stripColor(layout: ArtworkLayout): Color {
    val colors = layout.top.edgeColors.map(::argb).ifEmpty { listOf(argb(layout.top.baseColor)) }
    return Color(colors.map { it.red }.average().toFloat(), colors.map { it.green }.average().toFloat(), colors.map { it.blue }.average().toFloat())
}

/** Whether the strip above an immersive artwork is light: dark status bar icons and handle over it. */
internal fun stripLight(layout: ArtworkLayout): Boolean = stripColor(layout).luminance() > 0.4f

/** The handle over an immersive artwork's strip: from the strip's colour, not from the controls' below. */
internal fun immersiveHeaderColor(layout: ArtworkLayout): Color = if (stripLight(layout)) Color(0xFF1C1B1F) else Color.White

/**
 * The room above an immersive artwork (under the status bar and the player's handle,
 * [HeroGeometry.immersiveSlot]'s top down to the page's), filled from the artwork's top edge
 * ([ArtworkLayout.top]): a flat edge (a white cover, say) carries on upwards in its colour with a
 * crisp edge, so the cover just looks taller; any other fades the artwork's top ([immersiveFade])
 * onto a blurred copy of itself that carries on upwards as calm, blurred colour (never a sharp
 * mirror). Drawn in the continuation's layer, so it moves with the artwork in flight and in a swipe.
 */
@Composable
internal fun ImmersiveTopStrip(coverArt: String?, layout: ArtworkLayout, hero: HeroGeometry, modifier: Modifier = Modifier) {
    val edge = layout.top
    val flat = edge.isFlat()
    val colors = edge.edgeColors.map(::argb)
    val density = LocalDensity.current
    val far = with(density) { (hero.immersiveSlot().width * BLUR_FAR).toDp() }
    /** The artwork's top in this box, and how far under it its fading top runs. */
    val artTop = { hero.immersiveSlot().top }
    val under = { if (flat) 0f else hero.immersiveSlot().width * FEATHER }
    Box(
        modifier
            .clearAndSetSemantics { }
            .layout { m, c ->
                val h = (artTop() + under()).roundToInt().coerceIn(0, c.maxHeight)
                val p = m.measure(Constraints.fixed(c.maxWidth, h))
                layout(c.maxWidth, c.maxHeight) { p.place(0, 0) }
            },
    ) {
        if (flat) {
            // The edge's colour, then the cover's own first rows stretched upwards over it: they
            // meet the artwork's top exactly (a flat edge is rarely one colour to the last level).
            Box(Modifier.fillMaxSize().background(colors.firstOrNull() ?: argb(edge.baseColor)))
            Box(Modifier.fillMaxSize().clipToBounds()) {
                Artwork(
                    coverArt, ArtworkSizes.FULL, null,
                    Modifier
                        .layout { m, c ->
                            val side = hero.immersiveSlot().width.roundToInt().coerceAtLeast(1)
                            val p = m.measure(Constraints.fixed(c.maxWidth, side))
                            layout(c.maxWidth, c.maxHeight) { p.place(0, 0) }
                        }
                        .graphicsLayer {
                            // The first two of the cover's ArtworkSizes.FULL rows, over the whole strip.
                            val rows = 2f * hero.immersiveSlot().width / ArtworkSizes.FULL
                            transformOrigin = androidx.compose.ui.graphics.TransformOrigin(0.5f, 0f)
                            scaleY = (artTop() / rows.coerceAtLeast(1f)).coerceAtLeast(1f)
                        },
                    RectangleShape,
                )
            }
            return@Box
        }
        // Calm colour: the edge's colours, softened sideways.
        Box(Modifier.fillMaxSize().background(Brush.horizontalGradient(soften(colors.ifEmpty { listOf(argb(edge.baseColor)) }, 6))))
        // The artwork and its reflection above it, blurred as one image (its top edge on the
        // artwork's), fading out upwards into the colour.
        Box(
            Modifier
                .fillMaxSize()
                .graphicsLayer { compositingStrategy = CompositingStrategy.Offscreen }
                .drawWithContent {
                    drawContent()
                    val seam = artTop()
                    drawRect(Brush.verticalGradient(0f to Color.Transparent, 1f to Color.Black, startY = 0f, endY = seam.coerceAtLeast(1f)), blendMode = BlendMode.DstIn)
                }
                .clipToBounds()
                .layout { m, c ->
                    val side = hero.immersiveSlot().width.roundToInt().coerceAtLeast(1)
                    val p = m.measure(Constraints.fixed(c.maxWidth, 2 * side))
                    layout(c.maxWidth, c.maxHeight) { p.place(0, (artTop() - side).roundToInt()) }
                },
        ) {
            Column(Modifier.fillMaxSize().let { if (far > 0.dp) it.blur(far, BlurredEdgeTreatment.Rectangle) else it }) {
                Artwork(coverArt, ArtworkSizes.FULL, null, Modifier.fillMaxWidth().weight(1f).graphicsLayer { scaleY = -1f }, RectangleShape)
                Artwork(coverArt, ArtworkSizes.FULL, null, Modifier.fillMaxWidth().weight(1f), RectangleShape)
            }
        }
    }
}
