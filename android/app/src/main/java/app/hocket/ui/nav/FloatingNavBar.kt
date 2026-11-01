package app.hocket.ui.nav

import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.animateColorAsState
import androidx.compose.animation.core.spring
import androidx.compose.animation.expandHorizontally
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.shrinkHorizontally
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
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
import app.hocket.ui.a11y.LocalReducedMotion

/**
 * The phone's navigation bar as a floating pill (detached from the screen's edges, with a shadow),
 * in line with the floating mini player above it. The selected place shows as a filled capsule
 * holding its icon and label; the others are icons alone. Switching places slides the capsule's
 * label in on the new place as it folds away on the old one, so the bar's items shift sideways
 * to make room.
 *
 * Each item is a selectable tab whose accessible name is its label (shown or not).
 */
@Composable
internal fun FloatingNavBar(items: List<NavItem>, selected: NavItem?, onSelect: (NavItem) -> Unit, modifier: Modifier = Modifier) {
    val scheme = MaterialTheme.colorScheme
    Box(
        modifier
            .fillMaxWidth()
            .navigationBarsPadding()
            .padding(start = FloatingNavBarDefaults.SideMargin, end = FloatingNavBarDefaults.SideMargin, bottom = FloatingNavBarDefaults.BottomMargin),
        contentAlignment = Alignment.Center,
    ) {
        Row(
            Modifier
                .widthIn(max = HeroGeometry.MINI_MAX_WIDTH - FloatingNavBarDefaults.SideMargin * 2)
                .fillMaxWidth()
                .height(FloatingNavBarDefaults.PillHeight)
                .shadow(FloatingNavBarDefaults.Elevation, CircleShape, ambientColor = Color.Black, spotColor = Color.Black)
                .background(scheme.surfaceContainerHigh, CircleShape)
                .border(0.5.dp, scheme.outlineVariant.copy(alpha = 0.35f), CircleShape)
                .padding(horizontal = 8.dp)
                .selectableGroup(),
            horizontalArrangement = Arrangement.SpaceEvenly,
            verticalAlignment = Alignment.CenterVertically,
        ) {
            items.forEach { item ->
                FloatingNavItem(item, item == selected, onClick = { onSelect(item) })
            }
        }
    }
}

@Composable
private fun FloatingNavItem(item: NavItem, selected: Boolean, onClick: () -> Unit) {
    val scheme = MaterialTheme.colorScheme
    val reduced = LocalReducedMotion.current
    val label = item.barLabel()
    val container by animateColorAsState(if (selected) scheme.secondaryContainer else scheme.secondaryContainer.copy(alpha = 0f), label = "navItemContainer")
    val content by animateColorAsState(if (selected) scheme.onSecondaryContainer else scheme.onSurfaceVariant, label = "navItemContent")
    Row(
        Modifier
            .heightIn(min = 48.dp)
            .height(FloatingNavBarDefaults.PillHeight - 16.dp)
            .clip(CircleShape)
            .background(container)
            .selectable(selected = selected, onClick = onClick, role = Role.Tab, interactionSource = remember { MutableInteractionSource() }, indication = ripple())
            .semantics { contentDescription = label }
            .testTag("navBar." + item.id)
            .padding(horizontal = 14.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Icon(item.icon(selected), null, tint = content, modifier = Modifier.size(24.dp))
        AnimatedVisibility(
            visible = selected,
            enter = if (reduced) fadeIn() else fadeIn() + expandHorizontally(spring(dampingRatio = 0.85f, stiffness = 500f), expandFrom = Alignment.Start),
            exit = if (reduced) fadeOut() else fadeOut() + shrinkHorizontally(spring(dampingRatio = 1f, stiffness = 600f), shrinkTowards = Alignment.Start),
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
                    modifier = Modifier.widthIn(max = 104.dp).clearAndSetSemantics { },
                )
            }
        }
    }
}

internal object FloatingNavBarDefaults {
    val PillHeight: Dp = 64.dp
    val SideMargin: Dp = HeroGeometry.MINI_MARGIN
    val BottomMargin: Dp = 8.dp
    val Elevation: Dp = 10.dp
    /** The bar's height above the system navigation bar: the pill and its bottom margin. */
    val Height: Dp = PillHeight + BottomMargin
}
