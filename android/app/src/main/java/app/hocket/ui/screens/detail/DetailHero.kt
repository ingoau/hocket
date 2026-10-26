package app.hocket.ui.screens.detail

import android.graphics.BitmapFactory
import androidx.compose.animation.animateColorAsState
import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.RowScope
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.asPaddingValues
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBars
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.lazy.LazyRow
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.FilledIconToggleButton
import androidx.compose.material3.FilledTonalButton
import androidx.compose.material3.FilledTonalIconButton
import androidx.compose.material3.FilledTonalIconToggleButton
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButtonDefaults
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.rotate
import androidx.compose.ui.draw.shadow
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.RectangleShape
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.graphics.lerp
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import app.hocket.R
import app.hocket.core.ArtworkSizes
import app.hocket.core.api.Album
import app.hocket.ui.LocalCoreClient
import app.hocket.ui.components.Artwork
import app.hocket.ui.components.smallerSize
import app.hocket.ui.icons.HocketIcons
import app.hocket.ui.nav.ArtworkShare
import app.hocket.ui.nav.rememberArtworkShareSource
import app.hocket.ui.nav.sharedArtworkDestination
import app.hocket.ui.player.artworkSeed
import app.hocket.ui.theme.ArtworkColors
import app.hocket.ui.theme.LocalDarkTheme
import java.io.File

/**
 * How a detail page's header shows its artwork, never upscaled: [Large] full-bleed when the image
 * is at least the header's size in pixels, [Small] as a tile on a tinted banner when it is smaller
 * (or still being fetched, or the core has no file and the id's gradient stands in), [None] when
 * the item has no artwork at all.
 */
enum class HeroMode { Large, Small, None }

/** The full-bleed header's height for a page [width] wide (the boards' 390 × 470). */
internal fun largeHeroHeight(width: Dp): Dp = width * (470f / 390f)

/** Natural sizes of hero images (cover@size to its file and size), so a page visited again lays out in its mode at once. */
private val naturalSizes = android.util.LruCache<String, Pair<String, Pair<Int, Int>>>(64)

@Composable
internal fun rememberHeroMode(coverArt: String?, widthPx: Int, heightPx: Int): HeroMode {
    val client = LocalCoreClient.current
    val batterySaver by client.batterySaver.collectAsStateWithLifecycle()
    if (coverArt == null) return HeroMode.None
    val size = if (batterySaver) smallerSize(ArtworkSizes.HERO) else ArtworkSizes.HERO
    // The server never sends more than was asked for: below the header's width it can't be Large,
    // so don't fetch the big file just to measure it.
    if (size < widthPx) return HeroMode.Small
    val key = "$coverArt@$size"
    val natural = remember(key) { mutableStateOf(naturalSizes.get(key)?.second) }
    LaunchedEffect(key) {
        val path = client.artworkPath(coverArt, size) ?: return@LaunchedEffect
        // Known for this file: nothing to read. A new file (the cache refetched it) is measured again.
        naturalSizes.get(key)?.takeIf { it.first == path }?.let { natural.value = it.second; return@LaunchedEffect }
        val bounds = kotlinx.coroutines.withContext(kotlinx.coroutines.Dispatchers.IO) {
            val opts = BitmapFactory.Options().apply { inJustDecodeBounds = true }
            BitmapFactory.decodeFile(File(path.removePrefix("file://")).absolutePath, opts)
            if (opts.outWidth > 0 && opts.outHeight > 0) opts.outWidth to opts.outHeight else null
        } ?: return@LaunchedEffect
        naturalSizes.put(key, path to bounds)
        natural.value = bounds
    }
    val n = natural.value ?: return HeroMode.Small
    if (widthPx <= 0 || heightPx <= 0) return HeroMode.Small
    // Cropped to cover: the tighter side sets the scale.
    val scale = maxOf(widthPx.toFloat() / n.first, heightPx.toFloat() / n.second)
    return if (scale <= 1f) HeroMode.Large else HeroMode.Small
}

/** The banner's tint: the artwork's seed as a soft container (the boards' peach for a warm cover). */
@Composable
internal fun bannerTint(coverArt: String?): Color {
    val seed by artworkSeed(coverArt)
    val dark = LocalDarkTheme.current
    val base = MaterialTheme.colorScheme.surfaceContainer
    val target = seed?.let { s -> val scheme = ArtworkColors.scheme(s, dark); lerp(scheme.primaryContainer, base, if (dark) 0.45f else 0.55f) } ?: base
    return animateColorAsState(target, label = "bannerTint").value
}

/**
 * A detail page's header. [Large]: the artwork edge to edge, fading into the page, the title over
 * its foot. [Small] and [None]: a tinted banner with rounded lower corners, the artwork (if any) as
 * a tilted tile (a circle for an artist) bleeding off its top right, the title over it. [eyebrow]
 * goes above the title, [below] under it, [actions] last (inside the banner, under the image).
 */
@Composable
internal fun DetailHero(
    mode: HeroMode,
    /** Null while the page loads: a skeleton line in its place. */
    title: String?,
    coverArt: String?,
    shareKey: String?,
    round: Boolean,
    eyebrow: (@Composable () -> Unit)? = null,
    below: @Composable ColumnScope.() -> Unit = {},
    actions: @Composable RowScope.(onBanner: Boolean) -> Unit,
) {
    val statusTop = WindowInsets.statusBars.asPaddingValues().calculateTopPadding()
    BoxWithConstraints(Modifier.fillMaxWidth().testTag("detail.header").semantics { }) {
        val width = maxWidth
        when (mode) {
            HeroMode.Large -> Column {
                val heroHeight = largeHeroHeight(width)
                Box(Modifier.fillMaxWidth()) {
                    Box(Modifier.fillMaxWidth().height(heroHeight)) {
                        // The image is square, the header a little taller: the page fades in over its foot.
                        Artwork(coverArt, ArtworkSizes.HERO, title, Modifier.fillMaxWidth().aspectRatio(1f).sharedArtworkDestination(shareKey).testTag("detail.artwork"), RectangleShape)
                        // A shade under the status bar and the floating buttons, then the fade into the page.
                        Box(Modifier.fillMaxWidth().height(120.dp + statusTop).background(Brush.verticalGradient(listOf(Color.Black.copy(alpha = 0.30f), Color.Transparent))))
                        val bg = MaterialTheme.colorScheme.background
                        Box(Modifier.fillMaxSize().background(Brush.verticalGradient(0f to Color.Transparent, 0.5f to Color.Transparent, 0.74f to bg.copy(alpha = 0.8f), 0.84f to bg)))
                    }
                    Column(Modifier.padding(top = heroHeight - 100.dp, start = 20.dp, end = 20.dp)) {
                        eyebrow?.invoke()
                        HeroTitle(title)
                        below()
                    }
                }
                androidx.compose.foundation.layout.FlowRow(Modifier.padding(start = 20.dp, end = 20.dp, top = 16.dp), horizontalArrangement = Arrangement.spacedBy(8.dp), verticalArrangement = Arrangement.spacedBy(8.dp), itemVerticalAlignment = Alignment.CenterVertically) { actions(false) }
            }
            else -> Surface(color = bannerTint(coverArt), shape = RoundedCornerShape(bottomStart = 36.dp, bottomEnd = 36.dp), modifier = Modifier.fillMaxWidth()) {
                Box {
                    if (mode == HeroMode.Small) {
                        if (round) {
                            Artwork(coverArt, ArtworkSizes.GRID, title, Modifier.align(Alignment.TopEnd).offset(x = 56.dp, y = statusTop + 16.dp).size(minOf(220.dp, width * 0.58f))
                                .shadow(16.dp, CircleShape).sharedArtworkDestination(shareKey).testTag("detail.artwork"), CircleShape)
                        } else {
                            Artwork(coverArt, ArtworkSizes.GRID, title, Modifier.align(Alignment.TopEnd).offset(x = 30.dp, y = statusTop + 40.dp).size(minOf(200.dp, width * 0.52f)).rotate(-6f)
                                .shadow(14.dp, RoundedCornerShape(32.dp)).sharedArtworkDestination(shareKey).testTag("detail.artwork"), RoundedCornerShape(32.dp))
                        }
                    }
                    Column(Modifier.fillMaxWidth().padding(start = 20.dp, end = 20.dp, bottom = 20.dp, top = statusTop + if (mode == HeroMode.Small) 64.dp + 128.dp else 88.dp)) {
                        eyebrow?.invoke()
                        HeroTitle(title)
                        below()
                        // Wraps at large text sizes rather than squeezing the buttons.
                        androidx.compose.foundation.layout.FlowRow(Modifier.padding(top = 16.dp), horizontalArrangement = Arrangement.spacedBy(8.dp), verticalArrangement = Arrangement.spacedBy(8.dp), itemVerticalAlignment = Alignment.CenterVertically) { actions(true) }
                    }
                }
            }
        }
    }
}

/** The page's name, large and tight; long names step down a size so they still fit in three lines. */
@Composable
private fun HeroTitle(title: String?) {
    if (title == null) { app.hocket.ui.components.SkeletonLine(Modifier.padding(vertical = 6.dp).fillMaxWidth(0.6f), 44.dp); return }
    val style = if (title.length > 28) MaterialTheme.typography.displaySmall else MaterialTheme.typography.displayMedium
    Text(title, style = style.merge(TextStyle(fontWeight = FontWeight.Bold, letterSpacing = (-1.2).sp)), maxLines = 3, overflow = TextOverflow.Ellipsis,
        modifier = Modifier.widthIn(max = 360.dp).semantics { heading() }.testTag("detail.title"))
}

/** The hero's Play: a medium filled button with its label. */
@Composable
internal fun HeroPlayButton(onClick: () -> Unit, modifier: Modifier = Modifier) {
    Button(onClick = onClick, shapes = ButtonDefaults.shapes(), modifier = modifier.heightIn(min = ButtonDefaults.MediumContainerHeight).testTag("detail.play"),
        contentPadding = ButtonDefaults.contentPaddingFor(ButtonDefaults.MediumContainerHeight)) {
        Icon(HocketIcons.Filled.PlayArrow, null, Modifier.size(ButtonDefaults.MediumIconSize))
        Spacer(Modifier.size(ButtonDefaults.MediumIconSpacing))
        Text(stringResource(R.string.action_play), style = ButtonDefaults.textStyleFor(ButtonDefaults.MediumContainerHeight))
    }
}

/** A medium square icon button beside Play: on the banner it sits on the lifted surface, under the image on the tonal one. */
@Composable
internal fun HeroIconButton(icon: androidx.compose.ui.graphics.vector.ImageVector, label: String, onBanner: Boolean, onClick: () -> Unit, modifier: Modifier = Modifier) {
    FilledTonalIconButton(
        onClick = onClick, shapes = IconButtonDefaults.shapes(),
        colors = if (onBanner) IconButtonDefaults.filledTonalIconButtonColors(containerColor = app.hocket.ui.components.segmentColor(), contentColor = MaterialTheme.colorScheme.onSurface) else IconButtonDefaults.filledTonalIconButtonColors(),
        modifier = modifier.size(IconButtonDefaults.mediumContainerSize()),
    ) { Icon(icon, label, Modifier.size(IconButtonDefaults.mediumIconSize)) }
}

/**
 * Love as a medium toggle. Loved: a light (bright surface) square with the heart filled in the
 * primary colour, rather than a filled primary square, which read as a dark block among the light
 * buttons beside it. Not loved: the outlined heart on the surface, round.
 */
@Composable
internal fun HeroLoveToggle(loved: Boolean, onBanner: Boolean, onToggle: (Boolean) -> Unit, modifier: Modifier = Modifier) {
    FilledIconToggleButton(
        checked = loved, onCheckedChange = onToggle, shapes = IconButtonDefaults.toggleableShapes(),
        colors = IconButtonDefaults.filledIconToggleButtonColors(
            containerColor = if (onBanner) app.hocket.ui.components.segmentColor() else MaterialTheme.colorScheme.surfaceContainerHigh, contentColor = MaterialTheme.colorScheme.onSurfaceVariant,
            checkedContainerColor = MaterialTheme.colorScheme.surfaceBright, checkedContentColor = MaterialTheme.colorScheme.primary,
        ),
        modifier = modifier.size(IconButtonDefaults.mediumContainerSize()).testTag("detail.love"),
    ) { Icon(if (loved) HocketIcons.Filled.Favorite else HocketIcons.Filled.FavoriteBorder, stringResource(if (loved) R.string.action_unlove else R.string.action_love), Modifier.size(IconButtonDefaults.mediumIconSize)) }
}

/** Download as a medium toggle; a check once the item is kept on the device. */
@Composable
internal fun HeroDownloadToggle(downloaded: Boolean, onBanner: Boolean, onToggle: (Boolean) -> Unit, modifier: Modifier = Modifier) {
    FilledTonalIconToggleButton(
        checked = downloaded, onCheckedChange = onToggle, shapes = IconButtonDefaults.toggleableShapes(),
        colors = IconButtonDefaults.filledTonalIconToggleButtonColors(containerColor = if (onBanner) app.hocket.ui.components.segmentColor() else MaterialTheme.colorScheme.surfaceContainerHigh),
        modifier = modifier.size(IconButtonDefaults.mediumContainerSize()).testTag("detail.download"),
    ) { Icon(if (downloaded) HocketIcons.Filled.DownloadDone else HocketIcons.Filled.Download, stringResource(if (downloaded) R.string.detail_remove_download else R.string.detail_download), Modifier.size(IconButtonDefaults.mediumIconSize)) }
}

/** A section's heading, with an optional tonal "View more" on its right. */
@Composable
internal fun DetailSectionHead(title: String, modifier: Modifier = Modifier, trailing: (@Composable RowScope.() -> Unit)? = null) {
    Row(modifier.fillMaxWidth().heightIn(min = 48.dp).padding(start = 16.dp, end = 12.dp), verticalAlignment = Alignment.CenterVertically) {
        Text(title, style = MaterialTheme.typography.titleLarge, fontWeight = FontWeight.Bold, modifier = Modifier.weight(1f).semantics { heading() })
        trailing?.invoke(this)
    }
}

@Composable
internal fun ViewMoreButton(onClick: () -> Unit, modifier: Modifier = Modifier, label: String = stringResource(R.string.release_view_more)) {
    FilledTonalButton(onClick = onClick, shapes = ButtonDefaults.shapes(), modifier = modifier, contentPadding = PaddingValues(start = 16.dp, end = 10.dp)) {
        Text(label)
        Icon(HocketIcons.Filled.ChevronRight, null, Modifier.size(18.dp))
    }
}

/**
 * A shelf of releases: 148dp covers with their name and year, each with a small play button on
 * its corner, scrolling sideways.
 */
@Composable
internal fun ReleaseShelf(albums: List<Album>, onOpen: (Album) -> Unit, onPlay: (Album) -> Unit, modifier: Modifier = Modifier, meta: @Composable (Album) -> String) {
    LazyRow(modifier, contentPadding = PaddingValues(horizontal = 16.dp), horizontalArrangement = Arrangement.spacedBy(12.dp)) {
        items(albums, key = { it.id }) { a -> ReleaseTile(a, meta(a), onOpen = { onOpen(a) }, onPlay = { onPlay(a) }, modifier = Modifier.animateItem()) }
    }
}

@Composable
internal fun ReleaseTile(a: Album, meta: String, onOpen: () -> Unit, onPlay: () -> Unit, modifier: Modifier = Modifier, size: Dp = 148.dp) {
    val share = rememberArtworkShareSource(ArtworkShare.album(a.id), a.coverArt)
    Box(modifier.widthIn(max = size)) {
        Column(Modifier.clickable { share.prepare(); onOpen() }.testTag("detail.release")) {
            Artwork(a.coverArt, ArtworkSizes.GRID, stringResource(R.string.row_album_a11y, a.name, a.artist ?: ""), Modifier.size(size).then(share.modifier), RoundedCornerShape(20.dp))
            Column(Modifier.padding(top = 8.dp).widthIn(max = size)) {
                Text(a.name, style = MaterialTheme.typography.titleSmall, fontWeight = FontWeight.SemiBold, maxLines = 1, overflow = TextOverflow.Ellipsis)
                Text(meta, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant, maxLines = 1, overflow = TextOverflow.Ellipsis)
            }
        }
        FilledTonalIconButton(
            onClick = onPlay, shapes = IconButtonDefaults.shapes(),
            colors = IconButtonDefaults.filledTonalIconButtonColors(containerColor = app.hocket.ui.components.segmentColor(), contentColor = MaterialTheme.colorScheme.onSurface),
            modifier = Modifier.align(Alignment.TopEnd).padding(top = size - 52.dp, end = 8.dp).shadow(4.dp, RoundedCornerShape(16.dp)).size(44.dp),
        ) { Icon(HocketIcons.Filled.PlayArrow, stringResource(R.string.detail_play_named, a.name)) }
    }
}

/**
 * The floating top bar over a detail page: back and more as tonal circles over the header; once the
 * header has scrolled away ([collapsed]) the bar fills in with the page's name.
 */
@Composable
internal fun DetailTopBar(title: String?, collapsed: Boolean, onBack: (() -> Unit)?, actions: @Composable RowScope.() -> Unit) {
    val bar by animateColorAsState(if (collapsed) MaterialTheme.colorScheme.surfaceContainer else Color.Transparent, label = "detailBar")
    val titleAlpha by animateFloatAsState(if (collapsed) 1f else 0f, label = "detailBarTitle")
    Row(
        Modifier.fillMaxWidth().background(bar).statusBarsPadding().height(64.dp).padding(horizontal = 8.dp),
        verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(4.dp),
    ) {
        val circle = IconButtonDefaults.filledTonalIconButtonColors(containerColor = if (collapsed) Color.Transparent else app.hocket.ui.components.segmentColor().copy(alpha = 0.8f), contentColor = MaterialTheme.colorScheme.onSurface)
        if (onBack != null) FilledTonalIconButton(onClick = onBack, colors = circle, shapes = IconButtonDefaults.shapes(), modifier = Modifier.size(48.dp)) {
            Icon(HocketIcons.AutoMirrored.Filled.ArrowBack, stringResource(R.string.action_back))
        }
        Text(title.orEmpty(), style = MaterialTheme.typography.titleLarge, maxLines = 1, overflow = TextOverflow.Ellipsis,
            // Hidden until the bar fills in, and out of the accessibility tree till then (the header says it).
            modifier = Modifier.weight(1f).padding(start = 8.dp).graphicsLayer { alpha = titleAlpha }.then(if (collapsed) Modifier else Modifier.clearAndSetSemantics { }))
        androidx.compose.runtime.CompositionLocalProvider(LocalHeroBarColors provides circle) { actions() }
    }
}

/** The colours of the bar's circle buttons, for the page's own actions in [DetailTopBar]. */
internal val LocalHeroBarColors = androidx.compose.runtime.staticCompositionLocalOf<androidx.compose.material3.IconButtonColors?> { null }

/** One of the page's actions in [DetailTopBar]: a tonal circle like Back. */
@Composable
internal fun DetailBarButton(icon: androidx.compose.ui.graphics.vector.ImageVector, label: String, onClick: () -> Unit, modifier: Modifier = Modifier, enabled: Boolean = true) {
    FilledTonalIconButton(onClick = onClick, enabled = enabled, colors = LocalHeroBarColors.current ?: IconButtonDefaults.filledTonalIconButtonColors(), shapes = IconButtonDefaults.shapes(), modifier = modifier.size(48.dp)) {
        Icon(icon, label)
    }
}

/** Pixel size helper for [rememberHeroMode]. */
@Composable
internal fun Dp.toPx(): Int = with(LocalDensity.current) { this@toPx.roundToPx() }
