package app.hocket.ui.nav

import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.animateColorAsState
import androidx.compose.animation.core.Spring
import androidx.compose.animation.core.snap
import androidx.compose.animation.core.spring
import androidx.compose.animation.expandHorizontally
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.shrinkHorizontally
import androidx.compose.foundation.background
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.selection.selectable
import androidx.compose.foundation.selection.selectableGroup
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.ripple
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.shadow
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import app.hocket.ui.player.HeroGeometry
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.material3.ColorScheme
import androidx.compose.ui.graphics.lerp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import app.hocket.ui.player.nowPlayingScheme
import app.hocket.ui.a11y.LocalReducedMotion

/**
 * The phone's navigation bar as a compact floating pill, centred under the mini player and only as
 * wide as its places (it never spans the screen). While something plays, the bar takes the playing
 * artwork's colours, as the mini player does ([nowPlayingScheme]); otherwise the page's.
 *
 * The selected place is a capsule holding its icon and label; the others are icons alone. On a
 * switch the new place's capsule widens first and its label fades in once there is room, while the
 * old one's label fades at once and its capsule folds away; the bar re-centres as its width changes.
 *
 * Each item is a selectable tab whose accessible name is its label (shown or not).
 */
@Composable
internal fun FloatingNavBar(
    items: List<NavItem>,
    selected: NavItem?,
    onSelect: (NavItem) -> Unit,
    modifier: Modifier = Modifier,
    stash: List<app.hocket.core.api.StashItem> = emptyList(),
    onOpenStash: () -> Unit = {},
    alignment: Alignment = Alignment.Center,
) {
    val client = app.hocket.ui.LocalCoreClient.current
    val nowPlaying by client.nowPlaying.collectAsStateWithLifecycle()
    val colours = navBarColours(nowPlayingScheme(nowPlaying?.track?.coverArt) ?: MaterialTheme.colorScheme)
    BoxWithConstraints(
        modifier
            .fillMaxWidth()
            .navigationBarsPadding()
            .padding(start = FloatingNavBarDefaults.SideMargin, end = FloatingNavBarDefaults.SideMargin, bottom = FloatingNavBarDefaults.BottomMargin),
        // (At the start, beside the mini player, on wide screens.)
        contentAlignment = alignment,
    ) {
        // The selected label gets whatever the icon-only places leave (ellipsized past that), so
        // five places fit on a narrow phone at a large font size.
        val others = (items.size - 1).coerceAtLeast(0)
        // The stash chip beside the pill (#150) takes its share first.
        val hasStash = stash.isNotEmpty()
        val chipRoom = if (hasStash) app.hocket.ui.stash.StashChipDefaults.BesideBarWidth + StashGap else 0.dp
        val labelMax = (maxWidth - FloatingNavBarDefaults.Padding * 2 - FloatingNavBarDefaults.ItemWidth * others - SelectedChrome - chipRoom).coerceIn(0.dp, 120.dp)
        val reduced = LocalReducedMotion.current
        // The chip scales from its side by the pill (the right one right to left).
        val pillSideX = if (androidx.compose.ui.platform.LocalLayoutDirection.current == androidx.compose.ui.unit.LayoutDirection.Rtl) 1f else 0f
        // The last non-empty stash, so the chip keeps its covers while it folds away.
        val held = remember { arrayOf(stash) }
        if (hasStash) held[0] = stash
        val shownStash = held[0]
        Row(verticalAlignment = Alignment.CenterVertically) {
        Row(
            Modifier
                .height(FloatingNavBarDefaults.PillHeight)
                .shadow(FloatingNavBarDefaults.Elevation, CircleShape, ambientColor = Color.Black, spotColor = Color.Black)
                .background(colours.container, CircleShape)
                .padding(FloatingNavBarDefaults.Padding)
                .selectableGroup(),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            items.forEach { item ->
                FloatingNavItem(item, item == selected, colours, labelMax, onClick = { onSelect(item) })
            }
        }
        // The chip grows out of the pill's end: the pair re-centres as it widens, the chip
        // scaling up from its start with a little overshoot. Unclipped, so its shadow shows
        // throughout; the room opens from the chip's starting scale, so it never overhangs much.
        AnimatedVisibility(
            visible = hasStash,
            enter = if (reduced) fadeIn() else expandHorizontally(spring(dampingRatio = 0.8f, stiffness = 380f), expandFrom = Alignment.Start, clip = false) { (it * 0.4f).toInt() } +
                androidx.compose.animation.scaleIn(spring(dampingRatio = 0.55f, stiffness = 380f), initialScale = 0.4f, transformOrigin = androidx.compose.ui.graphics.TransformOrigin(pillSideX, 0.5f)) +
                fadeIn(androidx.compose.animation.core.tween(120)),
            exit = if (reduced) fadeOut() else fadeOut(androidx.compose.animation.core.tween(90)) +
                androidx.compose.animation.scaleOut(spring(stiffness = 600f), targetScale = 0.4f, transformOrigin = androidx.compose.ui.graphics.TransformOrigin(pillSideX, 0.5f)) +
                shrinkHorizontally(spring(dampingRatio = 1f, stiffness = 600f), shrinkTowards = Alignment.Start, clip = false),
        ) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Spacer(Modifier.width(StashGap))
                app.hocket.ui.stash.StashChip(
                    shownStash,
                    onClick = onOpenStash,
                    height = FloatingNavBarDefaults.PillHeight,
                    container = colours.container,
                    content = colours.selected,
                    elevation = FloatingNavBarDefaults.Elevation,
                )
            }
        }
        }
    }
}

/**
 * The widest the pill of [places] gets: every place but the selected one an icon, the selected one's
 * label at its longest (120 dp). What the shell leaves for it beside the mini player on wide screens.
 */
internal fun floatingPillMaxWidth(places: Int): Dp =
    FloatingNavBarDefaults.Padding * 2 + FloatingNavBarDefaults.ItemWidth * (places - 1).coerceAtLeast(0) + SelectedChrome + 120.dp

/** The gap between the pill and the stash chip. */
private val StashGap = 8.dp

/**
 * Whether the stash chip fits beside a pill of [places] in [width] (the screen's) with room for a
 * short label, at [fontScale] (its count and the label grow with it). When it doesn't, the shell
 * floats the chip above the mini player instead.
 */
internal fun stashFitsBesideBar(width: Dp, places: Int, fontScale: Float): Boolean {
    val pill = FloatingNavBarDefaults.Padding * 2 + FloatingNavBarDefaults.ItemWidth * (places - 1).coerceAtLeast(0) + SelectedChrome + 40.dp * fontScale
    val chip = app.hocket.ui.stash.StashChipDefaults.BesideBarWidth + 14.dp * (fontScale - 1f).coerceAtLeast(0f) * 3f
    return pill + StashGap + chip <= width - FloatingNavBarDefaults.SideMargin * 2
}

/** The selected capsule's room besides its label: side padding, the icon and the gap. */
private val SelectedChrome = 16.dp + 24.dp + 8.dp + 16.dp

@Composable
private fun FloatingNavItem(item: NavItem, selected: Boolean, colours: NavBarColours, labelMax: Dp, onClick: () -> Unit) {
    val reduced = LocalReducedMotion.current
    val label = item.barLabel()
    val colourSpec = if (reduced) snap() else spring<Color>(stiffness = Spring.StiffnessMediumLow)
    val container by animateColorAsState(if (selected) colours.indicator else colours.indicator.copy(alpha = 0f), colourSpec, label = "navItemContainer")
    val content by animateColorAsState(if (selected) colours.selected else colours.unselected, colourSpec, label = "navItemContent")
    // An unselected place is a fixed-width cell; the selected one pads out around its label.
    val sidePadding by androidx.compose.animation.core.animateDpAsState(if (selected) 16.dp else (FloatingNavBarDefaults.ItemWidth - 24.dp) / 2, if (reduced) snap() else spring(dampingRatio = 0.9f, stiffness = 500f), label = "navItemPadding")
    Row(
        Modifier
            .fillMaxHeight()
            .clip(CircleShape)
            .background(container)
            .selectable(selected = selected, onClick = onClick, role = Role.Tab, interactionSource = remember { MutableInteractionSource() }, indication = ripple())
            .semantics { contentDescription = label }
            .testTag("navBar." + item.id)
            .padding(horizontal = sidePadding),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Icon(item.icon(selected), null, tint = content, modifier = Modifier.size(24.dp))
        AnimatedVisibility(
            visible = selected,
            enter = if (reduced) fadeIn() else expandHorizontally(spring(dampingRatio = 0.9f, stiffness = 500f), expandFrom = Alignment.Start) + fadeIn(androidx.compose.animation.core.tween(160, delayMillis = 110)),
            exit = if (reduced) fadeOut() else fadeOut(androidx.compose.animation.core.tween(70)) + shrinkHorizontally(spring(dampingRatio = 1f, stiffness = 600f), shrinkTowards = Alignment.Start),
        ) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Spacer(Modifier.width(8.dp))
                Text(
                    label,
                    style = MaterialTheme.typography.labelLarge,
                    fontWeight = FontWeight.SemiBold,
                    color = content,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                    modifier = Modifier.widthIn(max = labelMax).clearAndSetSemantics { },
                )
            }
        }
    }
}

/** The bar's colours: its pill, the selected capsule, and the selected and other places' content. */
internal class NavBarColours(val container: Color, val indicator: Color, val selected: Color, val unselected: Color)

/**
 * The bar in [scheme]'s colours: a pill a little tinted towards the primary container (darker than
 * the mini player in a dark theme), the selected capsule in the mini player's container colour (the
 * full primary container in a light theme), its content in the primary colour (dark) or on it (light). Crossfades ([Motion.COLOUR_MS]) when the scheme changes.
 */
@Composable
private fun navBarColours(scheme: ColorScheme): NavBarColours {
    val dark = app.hocket.ui.theme.LocalDarkTheme.current
    val spec = androidx.compose.animation.core.tween<Color>(app.hocket.ui.theme.Motion.COLOUR_MS)
    @Composable fun a(c: Color) = animateColorAsState(c, spec, label = "navBarColour").value
    return NavBarColours(
        container = a(lerp(if (dark) scheme.surfaceContainer else scheme.surfaceContainerHigh, scheme.primaryContainer, if (dark) 0.3f else 0.2f)),
        // In light themes the full primary container, so the capsule stands clear of the pale pill.
        indicator = a(if (dark) app.hocket.ui.player.miniContainer(scheme, dark) else scheme.primaryContainer),
        selected = a(if (dark) scheme.primary else scheme.onPrimaryContainer),
        unselected = a(scheme.onSurfaceVariant),
    )
}

internal object FloatingNavBarDefaults {
    val PillHeight: Dp = 56.dp
    /** Room around the places inside the pill. */
    val Padding: Dp = 4.dp
    /** An unselected place's width (the pill's inner height, 48 dp, is every place's height). */
    val ItemWidth: Dp = 48.dp
    val SideMargin: Dp = HeroGeometry.MINI_MARGIN
    val BottomMargin: Dp = 8.dp
    val Elevation: Dp = 8.dp
    /** The bar's height above the system navigation bar: the pill and its bottom margin. */
    val Height: Dp = PillHeight + BottomMargin
}

