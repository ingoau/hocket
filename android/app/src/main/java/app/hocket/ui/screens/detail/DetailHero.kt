package app.hocket.ui.screens.detail

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
import androidx.compose.material3.ButtonShapes
import androidx.compose.material3.Surface
import androidx.compose.material3.FilledTonalButton
import androidx.compose.material3.FilledTonalIconButton
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButtonDefaults
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.shadow
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.RectangleShape
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.graphics.lerp
import androidx.compose.ui.graphics.luminance
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
import app.hocket.ui.icons.HocketIcons
import app.hocket.ui.nav.ArtworkShare
import app.hocket.ui.nav.rememberArtworkShareSource
import app.hocket.ui.nav.sharedArtworkDestination
import app.hocket.ui.player.artworkSeed
import app.hocket.ui.theme.ArtworkColors
import app.hocket.ui.theme.LocalDarkTheme
import androidx.compose.animation.core.tween
import androidx.compose.material3.ColorScheme
import androidx.compose.ui.draw.BlurredEdgeTreatment
import androidx.compose.ui.draw.blur
import androidx.compose.ui.draw.clipToBounds
import androidx.compose.ui.draw.drawBehind
import androidx.compose.ui.draw.drawWithContent
import androidx.compose.ui.graphics.BlendMode
import androidx.compose.ui.graphics.CompositingStrategy
import androidx.compose.ui.graphics.compositeOver
import androidx.compose.ui.layout.layout
import androidx.compose.ui.layout.onGloballyPositioned
import androidx.compose.ui.layout.positionInRoot
import androidx.compose.foundation.layout.wrapContentHeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.Constraints
import app.hocket.core.SettingKeys
import app.hocket.core.api.ArtworkEdge
import app.hocket.core.api.ArtworkStyle
import app.hocket.core.api.ImmersiveArtwork as ImmersiveArtworkPreference
import app.hocket.ui.player.CRISP_FEATHER
import app.hocket.ui.player.FEATHER
import app.hocket.core.api.ArtworkLayout
import app.hocket.ui.player.ImmersiveArtwork
import app.hocket.ui.player.argb
import app.hocket.ui.player.grainBrush
import app.hocket.ui.player.immersiveFade
import app.hocket.ui.player.isFlat
import app.hocket.ui.player.rememberArtworkLayout
import app.hocket.ui.player.soften
import kotlin.math.roundToInt

/**
 * How a detail page looks, worked out from its artwork: [immersive] (the cover edge to edge at the
 * top, carried on below by [edge], as in the full player) or a card (the cover as a tile over the
 * title); either way the whole page in the artwork's colour ([scheme], [dark]). [layout]: the
 * cover's worked-out layout once known.
 */
@androidx.compose.runtime.Immutable
internal class DetailLook(val immersive: Boolean, val layout: ArtworkLayout?, val scheme: ColorScheme, val dark: Boolean) {
    val edge: ArtworkEdge? get() = layout?.bottom
}

/** Immersive headers only up to this width: wider (a tablet's pane), the square would fill the screen. */
private val IMMERSIVE_MAX_WIDTH = 600.dp

/**
 * The look for a page with [coverArt]. A cover goes edge to edge when the full player would show
 * it that way ([ImmersiveArtwork], asked with the same setting); an artist's photo ([round]), a cover
 * the classifier keeps as a card, or the setting at Never show a card. Until the cover is worked out the page assumes
 * immersive in the app's colours, then eases into the cover's.
 */
@Composable
internal fun rememberDetailLook(coverArt: String?, round: Boolean, width: Dp): DetailLook {
    val client = LocalCoreClient.current
    val settings by client.settings.collectAsStateWithLifecycle()
    val appDark = LocalDarkTheme.current
    val app = MaterialTheme.colorScheme
    val preference = ImmersiveArtwork.preference(settings[SettingKeys.DISPLAY_IMMERSIVE_ARTWORK]?.value)
    val candidate = coverArt != null && !round && width <= IMMERSIVE_MAX_WIDTH && preference != ImmersiveArtworkPreference.Never
    val resolved = if (candidate) rememberArtworkLayout(coverArt, preference, appDark).value else null
    val layout = resolved?.layout?.takeIf { it.bottom.style != ArtworkStyle.Card }
    val immersive = candidate && (resolved == null || layout != null)
    val seed by artworkSeed(coverArt)
    val edge = layout?.bottom
    // Immersive: the colour the cover carries on in, under the scrim its text needs. A card: the
    // seed's container, softened towards the page.
    val (target, light) = when {
        edge != null -> {
            val scrim = (if (edge.light) Color.White else Color.Black).copy(alpha = edge.scrim.toFloat())
            scrim.compositeOver(argb(edge.baseColor)) to edge.light
        }
        seed != null && !immersive -> {
            val s = ArtworkColors.scheme(seed!!, appDark)
            lerp(s.primaryContainer, s.surface, if (appDark) 0.5f else 0.45f) to !appDark
        }
        else -> app.background to !appDark
    }
    val page by animateColorAsState(target, tween(450), label = "detailPage")
    // While the page eases from one colour to the next, its text follows the colour on screen
    // (dark text on a page still near black would not read), then the target's own call.
    val shownLight = if (page == target) light else page.luminance() > 0.4f
    // Nothing of the cover's yet (or ever: no artwork): the app's own colours, accents included.
    val scheme = if (seed == null && edge == null && page == app.background) app else remember(page, seed, shownLight) { ArtworkColors.pageScheme(seed, page, shownLight) }
    return DetailLook(immersive, layout, scheme, dark = !shownLight)
}

/**
 * A detail page's header, centred. Immersive: the cover edge to edge from the top of the screen,
 * fading at its foot into its own colours ([DetailContinuation]), the title just under it. A card:
 * the cover as a tile (a circle for an artist) under the bar, then the title; without a cover just
 * the title. [eyebrow] goes above the title, [below] under it, [actions] (Play and Shuffle) last.
 */
@Composable
internal fun DetailHero(
    look: DetailLook,
    /** Null while the page loads: a skeleton line in its place. */
    title: String?,
    coverArt: String?,
    shareKey: String?,
    round: Boolean,
    eyebrow: (@Composable () -> Unit)? = null,
    below: @Composable ColumnScope.() -> Unit = {},
    actions: @Composable RowScope.() -> Unit,
    /** The title's bottom edge in the window (px), as it scrolls: the bar fills in once it passes under it. */
    onTitleBottom: (Float) -> Unit = {},
    /** A card joined under the actions (an album's notes), or null. */
    footer: (@Composable () -> Unit)? = null,
) {
    val statusTop = WindowInsets.statusBars.asPaddingValues().calculateTopPadding()
    BoxWithConstraints(Modifier.fillMaxWidth().testTag("detail.header").semantics { }) {
        val width = maxWidth
        if (look.immersive) {
            val edge = look.edge
            // The cover from the very top of the screen, under the status bar and the bar's icons,
            // a shade over its top so they read whatever the cover is.
            if (edge != null) BoxWithConstraints(Modifier.matchParentSize().clipToBounds()) { DetailContinuation(coverArt, edge, width, 0.dp, maxHeight) }
            // Its foot fades as the player's does (a mirror's onto its reflection, a flat edge's last
            // few rows onto its colour); any other extension carries straight on with no fade.
            val footFades = edge != null && (edge.style == ArtworkStyle.Mirror || edge.isFlat())
            Artwork(coverArt, ArtworkSizes.HERO, title, Modifier.fillMaxWidth().aspectRatio(1f).sharedArtworkDestination(shareKey)
                .immersiveFade(edge, fraction = { if (footFades) 1f else 0f }).testTag("detail.artwork"), RectangleShape)
            Box(Modifier.fillMaxWidth().height(statusTop + 120.dp).background(Brush.verticalGradient(
                *Array(9) { i -> val t = i / 8f; t to Color.Black.copy(alpha = 0.55f * (1f - t) * (1f - t)) },
            )))
            HeroText(title, eyebrow, below, actions, footer, onTitleBottom, Modifier.padding(top = width + 16.dp).padding(bottom = 4.dp))
        } else {
            // A glow of the cover's colour behind the tile, fading into the page.
            Box(Modifier.matchParentSize().background(Brush.verticalGradient(0f to lerp(MaterialTheme.colorScheme.background, MaterialTheme.colorScheme.primary, 0.14f), 1f to Color.Transparent)))
            Column(Modifier.fillMaxWidth().padding(top = statusTop + 72.dp), horizontalAlignment = Alignment.CenterHorizontally) {
                if (coverArt != null) {
                    val side = minOf(width * 0.74f, 320.dp)
                    val shape = if (round) CircleShape else RoundedCornerShape(28.dp)
                    Artwork(coverArt, if (round) ArtworkSizes.FULL else ArtworkSizes.FULL, title, Modifier.size(side).shadow(24.dp, shape, ambientColor = MaterialTheme.colorScheme.primary, spotColor = Color.Black)
                        .sharedArtworkDestination(shareKey).testTag("detail.artwork"), shape)
                    Spacer(Modifier.height(24.dp))
                } else Spacer(Modifier.height(24.dp))
                HeroText(title, eyebrow, below, actions, footer, onTitleBottom, Modifier.padding(bottom = 8.dp))
            }
        }
    }
}

/** The title, what goes around it and the actions, centred. */
@Composable
private fun HeroText(title: String?, eyebrow: (@Composable () -> Unit)?, below: @Composable ColumnScope.() -> Unit, actions: @Composable RowScope.() -> Unit, footer: (@Composable () -> Unit)?, onTitleBottom: (Float) -> Unit, modifier: Modifier = Modifier) {
    Column(modifier.fillMaxWidth(), horizontalAlignment = Alignment.CenterHorizontally) {
        Column(Modifier.fillMaxWidth().padding(horizontal = 24.dp), horizontalAlignment = Alignment.CenterHorizontally) {
            eyebrow?.invoke()
            HeroTitle(title, onTitleBottom)
            below()
        }
        // The actions and the card under them are one connected group: round outside, a small
        // radius wherever two of them meet; as wide as the songs under them.
        Column(Modifier.padding(top = 16.dp, start = 16.dp, end = 16.dp).fillMaxWidth(), verticalArrangement = Arrangement.spacedBy(GROUP_GAP)) {
            androidx.compose.runtime.CompositionLocalProvider(LocalJoinedBelow provides (footer != null)) {
                Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(GROUP_GAP), verticalAlignment = Alignment.CenterVertically) { actions() }
            }
            footer?.let { content ->
                Surface(color = MaterialTheme.colorScheme.surfaceContainerHigh, shape = RoundedCornerShape(topStart = GROUP_INNER, topEnd = GROUP_INNER, bottomStart = GROUP_OUTER, bottomEnd = GROUP_OUTER), modifier = Modifier.fillMaxWidth()) { content() }
            }
        }
    }
}

/** The actions' group: its outer corners, where two pieces meet, and the gap between them. */
private val GROUP_OUTER = 28.dp
private val GROUP_INNER = 8.dp
private val GROUP_GAP = 4.dp

/** Whether a card is joined under the actions (their lower corners are then inner ones). */
private val LocalJoinedBelow = androidx.compose.runtime.staticCompositionLocalOf { false }

/** Where an action sits in its row: the first and last take the group's outer corners on their side. */
internal enum class GroupPos { Start, Middle, End }

@Composable
private fun groupShape(pos: GroupPos): RoundedCornerShape {
    val below = LocalJoinedBelow.current
    val o = GROUP_OUTER
    val i = GROUP_INNER
    return when (pos) {
        GroupPos.Start -> RoundedCornerShape(topStart = o, topEnd = i, bottomStart = if (below) i else o, bottomEnd = i)
        GroupPos.Middle -> RoundedCornerShape(i)
        GroupPos.End -> RoundedCornerShape(topStart = i, topEnd = o, bottomStart = i, bottomEnd = if (below) i else o)
    }
}

/** A group piece's shapes: its own at rest, squarer while pressed (the Expressive press). */
@Composable
private fun groupButtonShapes(pos: GroupPos) = ButtonShapes(shape = groupShape(pos), pressedShape = RoundedCornerShape(GROUP_INNER))

/** The page's name, large and centred; long names step down a size so they still fit in three lines. */
@Composable
private fun HeroTitle(title: String?, onBottom: (Float) -> Unit) {
    if (title == null) { app.hocket.ui.components.SkeletonLine(Modifier.padding(vertical = 6.dp).fillMaxWidth(0.6f), 40.dp); return }
    val style = when {
        title.length > 40 -> MaterialTheme.typography.headlineMediumEmphasized
        title.length > 20 -> MaterialTheme.typography.headlineLargeEmphasized
        else -> MaterialTheme.typography.displaySmallEmphasized
    }
    Text(title, style = style.merge(TextStyle(fontWeight = FontWeight.Bold, letterSpacing = (-0.5).sp, textAlign = TextAlign.Center)), maxLines = 3, overflow = TextOverflow.Ellipsis,
        color = MaterialTheme.colorScheme.onBackground, modifier = Modifier.widthIn(max = 480.dp).onGloballyPositioned { onBottom(it.positionInRoot().y + it.size.height) }.semantics { heading() }.testTag("detail.title"))
}

/** A line of the header under the title (the artist, the facts), centred. */
@Composable
internal fun HeroLine(text: String, modifier: Modifier = Modifier, style: TextStyle = MaterialTheme.typography.bodyMedium, color: Color = MaterialTheme.colorScheme.onSurfaceVariant, fontWeight: FontWeight? = null, maxLines: Int = 2) {
    Text(text, style = style, color = color, fontWeight = fontWeight, textAlign = TextAlign.Center, maxLines = maxLines, overflow = TextOverflow.Ellipsis, modifier = modifier)
}

/** The hero's Play: a medium filled button with its label, sharing the row with Shuffle. */
@Composable
internal fun RowScope.HeroPlayButton(onClick: () -> Unit, modifier: Modifier = Modifier, pos: GroupPos = GroupPos.Start) {
    Button(onClick = onClick, shapes = groupButtonShapes(pos), modifier = modifier.weight(1f).heightIn(min = ButtonDefaults.MediumContainerHeight).testTag("detail.play"),
        contentPadding = ButtonDefaults.contentPaddingFor(ButtonDefaults.MediumContainerHeight)) {
        Icon(HocketIcons.Filled.PlayArrow, null, Modifier.size(ButtonDefaults.MediumIconSize))
        Spacer(Modifier.size(ButtonDefaults.MediumIconSpacing))
        Text(stringResource(R.string.action_play), style = ButtonDefaults.textStyleFor(ButtonDefaults.MediumContainerHeight), maxLines = 1)
    }
}

/** Shuffle beside Play: the same size, tonal. */
@Composable
internal fun RowScope.HeroShuffleButton(onClick: () -> Unit, modifier: Modifier = Modifier, pos: GroupPos = GroupPos.End) {
    FilledTonalButton(onClick = onClick, shapes = groupButtonShapes(pos), modifier = modifier.weight(1f).heightIn(min = ButtonDefaults.MediumContainerHeight).testTag("detail.shuffle"),
        colors = ButtonDefaults.filledTonalButtonColors(containerColor = MaterialTheme.colorScheme.surfaceContainerHighest, contentColor = MaterialTheme.colorScheme.onSurface),
        contentPadding = ButtonDefaults.contentPaddingFor(ButtonDefaults.MediumContainerHeight)) {
        Icon(HocketIcons.Filled.Shuffle, null, Modifier.size(ButtonDefaults.MediumIconSize))
        Spacer(Modifier.size(ButtonDefaults.MediumIconSpacing))
        Text(stringResource(R.string.action_shuffle), style = ButtonDefaults.textStyleFor(ButtonDefaults.MediumContainerHeight), maxLines = 1)
    }
}

/** The page's menu beside Play and Shuffle: a medium tonal icon button, last in the group. */
@Composable
internal fun HeroMoreButton(onClick: () -> Unit, modifier: Modifier = Modifier) {
    FilledTonalIconButton(onClick = onClick, shapes = androidx.compose.material3.IconButtonShapes(shape = groupShape(GroupPos.End), pressedShape = RoundedCornerShape(GROUP_INNER)),
        colors = IconButtonDefaults.filledTonalIconButtonColors(containerColor = MaterialTheme.colorScheme.surfaceContainerHighest, contentColor = MaterialTheme.colorScheme.onSurface),
        modifier = modifier.size(ButtonDefaults.MediumContainerHeight).testTag("detail.more")) {
        Icon(HocketIcons.Filled.MoreVert, stringResource(R.string.action_more), Modifier.size(ButtonDefaults.MediumIconSize))
    }
}

/** Love in the bar: the heart filled in the primary colour once loved. */
@Composable
internal fun DetailLoveButton(loved: Boolean, onToggle: (Boolean) -> Unit) {
    DetailBarButton(if (loved) HocketIcons.Filled.Favorite else HocketIcons.Filled.FavoriteBorder, stringResource(if (loved) R.string.action_unlove else R.string.action_love),
        onClick = { onToggle(!loved) }, modifier = Modifier.testTag("detail.love"), tint = if (loved && !LocalHeroBarOverArt.current) MaterialTheme.colorScheme.primary else null)
}

/** Download in the bar; a check once the item is kept on the device. */
@Composable
internal fun DetailDownloadButton(downloaded: Boolean, onToggle: (Boolean) -> Unit) {
    DetailBarButton(if (downloaded) HocketIcons.Filled.DownloadDone else HocketIcons.Filled.Download, stringResource(if (downloaded) R.string.detail_remove_download else R.string.detail_download),
        onClick = { onToggle(!downloaded) }, modifier = Modifier.testTag("detail.download"))
}

/** How far the continuation runs under the title, as a share of the cover's side. */
private const val CONTINUATION = 0.62f

/**
 * What carries an immersive cover on under the title, ending in the page's colour.
 *
 * - A flat edge, as in the full player: the cover's last [CRISP_FEATHER] fades onto its one
 *   colour, which carries on with the cover's grain over it.
 * - Any other extension: no fade at all. The cover's own last rows are stretched down from its
 *   edge, so the join is seamless, and settle within [STRETCH] of the side into the colours along
 *   the edge, softened sideways.
 * - A mirror, as in the full player: the cover's last [FEATHER] fades ([immersiveFade]) onto a
 *   blurred reflection of itself, which blurs further with distance.
 *
 * Either way under the scrim the text needs (eased in just past the edge) and fading into the page
 * by the header's foot.
 */
@Composable
private fun DetailContinuation(coverArt: String?, edge: ArtworkEdge, side: Dp, artTop: Dp, header: Dp) {
    val mirror = edge.style == ArtworkStyle.Mirror
    val flat = edge.isFlat()
    // Under the cover's fading foot: a mirror's [FEATHER], a flat edge's [CRISP_FEATHER].
    val overlap = when { mirror -> side * FEATHER; flat -> side * CRISP_FEATHER; else -> 0.dp }
    // Faded out by the header's foot, where the page's own colour takes over.
    val height = minOf(overlap + side * CONTINUATION, (header - artTop - side + overlap).coerceAtLeast(overlap + 1.dp))
    val density = LocalDensity.current
    val sidePx = with(density) { side.toPx() }
    val seamPx = with(density) { overlap.toPx() }
    val heightPx = with(density) { height.toPx() }

    /** The cover with its reflection under it, blurred as one, the seam on the cover's edge, kept down to [until] (a share of the side below the seam). */
    @Composable
    fun Reflected(blur: Dp, until: Float) = Box(
        Modifier.fillMaxSize().fadeDown(seamPx, seamPx + sidePx * until).clipToBounds().layout { m, c ->
            val s = sidePx.roundToInt().coerceAtLeast(1)
            val p = m.measure(Constraints.fixed(c.maxWidth, 2 * s))
            layout(c.maxWidth, c.maxHeight) { p.place(0, (seamPx - s).roundToInt()) }
        },
    ) {
        Column(Modifier.fillMaxSize().blur(blur, BlurredEdgeTreatment.Rectangle)) {
            Artwork(coverArt, ArtworkSizes.GRID, null, Modifier.fillMaxWidth().weight(1f), RectangleShape)
            Artwork(coverArt, ArtworkSizes.GRID, null, Modifier.fillMaxWidth().weight(1f).graphicsLayer { scaleY = -1f }, RectangleShape)
        }
    }
    Box(Modifier.wrapContentHeight(Alignment.Top, unbounded = true).padding(top = artTop + side - overlap).fillMaxWidth().height(height).clearAndSetSemantics { }.fadeDown(seamPx, heightPx)) {
        val base = argb(edge.baseColor)
        Box(Modifier.fillMaxSize().background(base))
        if (flat) Box(Modifier.fillMaxSize().background(grainBrush(edge.metrics.grain)))
        else Box(Modifier.fillMaxSize().background(Brush.horizontalGradient(soften(edge.edgeColors.map(::argb).ifEmpty { listOf(base) }, 6))))
        if (mirror) {
            Reflected(side * 0.08f, 0.55f)
            Reflected(side * 0.03f, 0.12f)
        } else if (!flat) {
            // The cover's last rows, flipped about its edge and stretched down: the first pixel
            // under the edge is the cover's last, so nothing fades and no seam shows.
            Box(Modifier.fillMaxSize().fadeDown(0f, sidePx * STRETCH).clipToBounds()) {
                Artwork(coverArt, ArtworkSizes.FULL, null, Modifier
                    .layout { m, c ->
                        val s = sidePx.roundToInt().coerceAtLeast(1)
                        val p = m.measure(Constraints.fixed(c.maxWidth, s))
                        layout(c.maxWidth, c.maxHeight) { p.place(0, -s) }
                    }
                    .graphicsLayer {
                        // Two of the cover's ArtworkSizes.FULL rows over the whole stretch.
                        val rows = 2f * sidePx / ArtworkSizes.FULL
                        transformOrigin = androidx.compose.ui.graphics.TransformOrigin(0.5f, 1f)
                        scaleY = -(sidePx * STRETCH / rows.coerceAtLeast(1f)).coerceAtLeast(1f)
                    }, RectangleShape)
            }
        }
        // The scrim the title needs: eased in from the seam (a mirror: over its fading edge).
        if (edge.scrim > 0.0) {
            val scrim = (if (edge.light) Color.White else Color.Black).copy(alpha = edge.scrim.toFloat())
            val ease = if (overlap > 0.dp) seamPx else sidePx * 0.06f
            Box(Modifier.fillMaxSize().drawBehind { drawRect(Brush.verticalGradient(0f to scrim.copy(alpha = 0f), 1f to scrim, startY = 0f, endY = ease.coerceAtLeast(1f)), size = size) })
        }
    }
}

/** How far an extension's stretched rows run before only its edge colours are left, as a share of the side. */
private const val STRETCH = 0.4f

/** Keeps the content in full down to [from] px, then fades it out by [to]. */
private fun Modifier.fadeDown(from: Float, to: Float): Modifier = this
    .graphicsLayer { compositingStrategy = CompositingStrategy.Offscreen }
    .drawWithContent {
        drawContent()
        drawRect(Brush.verticalGradient(0f to Color.Black, 0.35f to Color.Black.copy(alpha = 0.75f), 1f to Color.Transparent, startY = from, endY = to.coerceAtLeast(from + 1f)), blendMode = BlendMode.DstIn)
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
 * The floating top bar over a detail page: back and the page's actions as bare icons over the
 * header; once the title has scrolled under it ([collapsed]) the bar fills in with the page's colour
 * and name ([filled]: the colour alone, sooner, once anything scrolls under it). [overArt]: the icons sit over an immersive cover, whose top strip is light (true) or
 * dark (false): dark or white icons on the cover's shade, rather than the page's colours.
 */
@Composable
internal fun DetailTopBar(title: String?, collapsed: Boolean, onBack: (() -> Unit)?, overArt: Boolean? = null, shade: Boolean = false, filled: Boolean = collapsed, actions: @Composable RowScope.() -> Unit) {
    val bar by animateColorAsState(if (filled || collapsed) MaterialTheme.colorScheme.surfaceContainer else Color.Transparent, label = "detailBar")
    val titleAlpha by animateFloatAsState(if (collapsed) 1f else 0f, label = "detailBarTitle")
    // Over the middle of a cover (its top strip scrolled away), a shade for the white icons.
    val shadeAlpha by animateFloatAsState(if (shade) 1f else 0f, label = "detailBarShade")
    Row(
        Modifier.fillMaxWidth()
            .drawBehind {
                if (shadeAlpha > 0f) drawRect(Brush.verticalGradient(*Array(9) { i -> val t = i / 8f; t to Color.Black.copy(alpha = 0.45f * shadeAlpha * (1f - t) * (1f - t)) }), size = size.copy(height = size.height * 1.5f))
            }
            .background(bar).statusBarsPadding().height(64.dp).padding(horizontal = 8.dp),
        verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(4.dp),
    ) {
        val art = overArt.takeIf { !collapsed }
        // Bare icons: over a cover they read on its top shade (white over a dark strip, dark over a
        // light one), elsewhere on the page or the filled-in bar.
        val content by animateColorAsState(when (art) { true -> Color(0xFF1C1B1F); false -> Color.White; null -> MaterialTheme.colorScheme.onSurface }, label = "detailBarIcon")
        val circle = IconButtonDefaults.filledTonalIconButtonColors(containerColor = Color.Transparent, contentColor = content)
        if (onBack != null) FilledTonalIconButton(onClick = onBack, colors = circle, shapes = IconButtonDefaults.shapes(), modifier = Modifier.size(48.dp)) {
            Icon(HocketIcons.AutoMirrored.Filled.ArrowBack, stringResource(R.string.action_back))
        }
        Text(title.orEmpty(), style = MaterialTheme.typography.titleLarge, color = MaterialTheme.colorScheme.onSurface, maxLines = 1, overflow = TextOverflow.Ellipsis,
            // Hidden until the bar fills in, and out of the accessibility tree till then (the header says it).
            modifier = Modifier.weight(1f).padding(start = 8.dp).graphicsLayer { alpha = titleAlpha }.then(if (collapsed) Modifier else Modifier.clearAndSetSemantics { }))
        androidx.compose.runtime.CompositionLocalProvider(LocalHeroBarColors provides circle, LocalHeroBarOverArt provides (art != null)) { actions() }
    }
}

/** Whether the bar's icons are over an immersive cover (its shade's colours, not the page's). */
internal val LocalHeroBarOverArt = androidx.compose.runtime.staticCompositionLocalOf { false }

/** The colours of the bar's icon buttons, for the page's own actions in [DetailTopBar]. */
internal val LocalHeroBarColors = androidx.compose.runtime.staticCompositionLocalOf<androidx.compose.material3.IconButtonColors?> { null }

/** One of the page's actions in [DetailTopBar]: a bare icon like Back. */
@Composable
internal fun DetailBarButton(icon: androidx.compose.ui.graphics.vector.ImageVector, label: String, onClick: () -> Unit, modifier: Modifier = Modifier, enabled: Boolean = true, tint: Color? = null) {
    FilledTonalIconButton(onClick = onClick, enabled = enabled, colors = LocalHeroBarColors.current ?: IconButtonDefaults.filledTonalIconButtonColors(), shapes = IconButtonDefaults.shapes(), modifier = modifier.size(48.dp)) {
        if (tint != null) Icon(icon, label, tint = tint) else Icon(icon, label)
    }
}
