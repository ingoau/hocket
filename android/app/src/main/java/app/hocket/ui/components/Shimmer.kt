package app.hocket.ui.components

import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.animation.core.withInfiniteAnimationFrameNanos
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.MaterialTheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.drawBehind
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Outline
import androidx.compose.ui.graphics.Shape
import androidx.compose.ui.graphics.drawOutline
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import app.hocket.ui.a11y.LocalReducedMotion

/**
 * Loading skeletons (design: loading). One [ShimmerHost] drives a single sweep that every
 * [skeleton] below it reads in the draw phase only, so a screen of placeholders costs one frame
 * callback and no recomposition per frame. The sweep runs only while at least one skeleton is on
 * screen, and not at all under reduced motion (or outside a host), where the blocks are static.
 */
private class Shimmer {
    val progress = mutableFloatStateOf(0f)
    var attached by mutableIntStateOf(0)
}

private val LocalShimmer = staticCompositionLocalOf<Shimmer?> { null }

/** Provides the shimmer sweep to the [skeleton]s in [content]; static under reduced motion. */
@Composable
fun ShimmerHost(content: @Composable () -> Unit) {
    val shimmer = remember { Shimmer() }
    val reduced = LocalReducedMotion.current
    val active = !reduced && shimmer.attached > 0
    LaunchedEffect(active) {
        if (!active) return@LaunchedEffect
        var start = -1L
        // An infinite-animation frame loop: tests (and hosts that disallow endless animation) cancel it and the blocks stay static.
        while (true) {
            withInfiniteAnimationFrameNanos { now ->
                if (start < 0) start = now
                shimmer.progress.floatValue = ((now - start) % SWEEP_NANOS).toFloat() / SWEEP_NANOS
            }
        }
    }
    CompositionLocalProvider(LocalShimmer provides if (reduced) null else shimmer, content = content)
}

private const val SWEEP_NANOS = 1_100_000_000L

/**
 * Paints a placeholder block in [shape]: a low-alpha onSurface fill with a lighter band sweeping
 * across it when inside a [ShimmerHost]. The band is laid out in window-independent px from -1 to 2
 * widths of the element, so neighbouring blocks sweep together.
 */
@Composable
fun Modifier.skeleton(shape: Shape = RoundedCornerShape(8.dp)): Modifier {
    val base = MaterialTheme.colorScheme.onSurface
    val shimmer = LocalShimmer.current
    if (shimmer != null) DisposableEffect(shimmer) { shimmer.attached++; onDispose { shimmer.attached-- } }
    val progress = shimmer?.progress
    return this.drawBehind {
        val outline = shape.createOutline(size, layoutDirection, this)
        val p = progress?.floatValue
        val brush: Brush = if (p == null) {
            androidx.compose.ui.graphics.SolidColor(base.copy(alpha = 0.08f))
        } else {
            val w = size.width.coerceAtLeast(1f)
            val x = -w + p * 3f * w
            Brush.linearGradient(
                listOf(base.copy(alpha = 0.07f), base.copy(alpha = 0.15f), base.copy(alpha = 0.07f)),
                start = Offset(x - w * 0.6f, 0f), end = Offset(x + w * 0.6f, size.height),
            )
        }
        when (outline) {
            is Outline.Rectangle -> drawRect(brush)
            is Outline.Rounded -> {
                val r = outline.roundRect
                if (r.topLeftCornerRadius == r.bottomRightCornerRadius && r.topLeftCornerRadius == r.topRightCornerRadius) {
                    drawRoundRect(brush, cornerRadius = CornerRadius(r.topLeftCornerRadius.x, r.topLeftCornerRadius.y))
                } else drawOutline(outline, brush)
            }
            else -> drawOutline(outline, brush)
        }
    }
}

/** A pill-shaped text-line placeholder [height] tall (Navic: 16dp title, 14dp subtitle). */
@Composable
fun SkeletonLine(modifier: Modifier = Modifier, height: Dp = 14.dp) {
    Box(modifier.height(height).skeleton(CircleShape))
}

/** Same footprint as a [TrackRow] (64dp, 48dp art, two lines), for lists still loading. */
@Composable
fun TrackRowSkeleton(modifier: Modifier = Modifier, showArtwork: Boolean = true, artShape: Shape = RoundedCornerShape(ListArtCorner)) {
    Row(modifier.fillMaxWidth().heightIn(min = 64.dp).padding(horizontal = 16.dp, vertical = 8.dp).clearAndSetSemantics { }, verticalAlignment = androidx.compose.ui.Alignment.CenterVertically) {
        if (showArtwork) {
            Box(Modifier.size(48.dp).skeleton(artShape))
            Spacer(Modifier.width(14.dp))
        }
        Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(8.dp)) {
            SkeletonLine(Modifier.fillMaxWidth(0.62f), 14.dp)
            SkeletonLine(Modifier.fillMaxWidth(0.38f), 12.dp)
        }
    }
}

/** Same footprint as an [AlbumCard]: square art then a title and a subtitle line. */
@Composable
fun AlbumCardSkeleton(modifier: Modifier = Modifier) {
    Column(modifier.fillMaxWidth().padding(6.dp).clearAndSetSemantics { }) {
        Box(Modifier.fillMaxWidth().aspectRatio(1f).skeleton(RoundedCornerShape(GridArtCorner)))
        Spacer(Modifier.height(8.dp))
        AlbumCardTextSkeleton()
    }
}

/**
 * The text block under a grid cell, as tall as [GridCellText] at the current font scale (a
 * two-line title and a one-line subtitle), so a loaded cell takes exactly the skeleton's place.
 */
@Composable
internal fun AlbumCardTextSkeleton() {
    val density = androidx.compose.ui.platform.LocalDensity.current
    val titleH = with(density) { GridTitleStyle.lineHeight.toDp() }
    val subH = with(density) { MaterialTheme.typography.bodySmall.lineHeight.toDp() }
    Box(Modifier.height(titleH), contentAlignment = androidx.compose.ui.Alignment.CenterStart) { SkeletonLine(Modifier.fillMaxWidth(0.85f), (titleH * 0.7f).coerceAtMost(14.dp)) }
    Box(Modifier.height(titleH), contentAlignment = androidx.compose.ui.Alignment.CenterStart) { SkeletonLine(Modifier.fillMaxWidth(0.55f), (titleH * 0.7f).coerceAtMost(14.dp)) }
    Box(Modifier.height(subH), contentAlignment = androidx.compose.ui.Alignment.CenterStart) { SkeletonLine(Modifier.fillMaxWidth(0.45f), (subH * 0.7f).coerceAtMost(12.dp)) }
}

/** A carousel of album skeletons, the size of [AlbumStrip]'s cells. */
@Composable
fun AlbumStripSkeleton(modifier: Modifier = Modifier, count: Int = 4) {
    Row(modifier.fillMaxWidth().padding(horizontal = 16.dp).clearAndSetSemantics { }, horizontalArrangement = Arrangement.spacedBy(12.dp)) {
        repeat(count) {
            Column(Modifier.width(CarouselItemWidth)) {
                Box(Modifier.size(CarouselItemWidth).skeleton(RoundedCornerShape(GridArtCorner)))
                Spacer(Modifier.height(6.dp))
                AlbumCardTextSkeleton()
            }
        }
    }
}

/** A section-header-sized bar. */
@Composable
fun SectionHeaderSkeleton(modifier: Modifier = Modifier) {
    Box(modifier.fillMaxWidth().heightIn(min = 48.dp).padding(start = 16.dp, top = 16.dp, bottom = 8.dp).clearAndSetSemantics { }, contentAlignment = androidx.compose.ui.Alignment.CenterStart) {
        SkeletonLine(Modifier.width(140.dp), 18.dp)
    }
}

/** Artwork corner radii (Navic "soft"): grids and headers, list rows. */
val GridArtCorner = 10.dp
val ListArtCorner = 8.dp
/** Home and detail carousels: Navic's 150dp cells. */
val CarouselItemWidth = 150.dp
