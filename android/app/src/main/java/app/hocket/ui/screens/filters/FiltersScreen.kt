package app.hocket.ui.screens.filters

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.ListItemDefaults
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.SegmentedListItem
import androidx.compose.ui.Alignment
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import app.hocket.ui.components.Badge
import app.hocket.ui.components.segmentedListColors
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.ListItem
import androidx.compose.material3.MediumFlexibleTopAppBar
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TopAppBarDefaults
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.input.nestedscroll.nestedScroll
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.navigation.NavHostController
import app.hocket.R
import app.hocket.ui.icons.HocketIcons
import app.hocket.ui.nav.AccountButton
import app.hocket.ui.nav.BottomContentInset
import app.hocket.core.Commands
import app.hocket.core.api.Filter
import app.hocket.ui.LocalCoreClient
import app.hocket.ui.components.ConfirmDialog
import app.hocket.ui.components.EmptyState
import app.hocket.ui.nav.Route

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun FiltersScreen(nav: NavHostController) {
    val client = LocalCoreClient.current
    val filters by client.filters.collectAsStateWithLifecycle()
    val server by client.server.collectAsStateWithLifecycle()
    var deleting by remember { mutableStateOf<Filter?>(null) }
    var menu by remember { mutableStateOf<Filter?>(null) }
    val scroll = TopAppBarDefaults.exitUntilCollapsedScrollBehavior()
    Scaffold(
        modifier = Modifier.nestedScroll(scroll.nestedScrollConnection),
        topBar = { MediumFlexibleTopAppBar(title = { Text(stringResource(R.string.filters_title), style = app.hocket.ui.brand.brandTitleStyle(), maxLines = 1, modifier = Modifier.semantics { heading() }) }, actions = { AccountButton() }, navigationIcon = { IconButton(onClick = { nav.popBackStack() }) { Icon(HocketIcons.AutoMirrored.Filled.ArrowBack, stringResource(R.string.action_back)) } }, scrollBehavior = scroll) },
        floatingActionButton = {
            val newLabel = stringResource(R.string.filters_new)
            // M3 Expressive's medium extended FAB, on the primary colour as the boards show it.
            androidx.compose.material3.MediumExtendedFloatingActionButton(
                onClick = { nav.navigate(Route.FilterBuilder()) },
                containerColor = androidx.compose.material3.MaterialTheme.colorScheme.primary,
                icon = { Icon(HocketIcons.Filled.Add, null) },
                text = { Text(newLabel, Modifier.clearAndSetSemantics {}) },
                // The text is the label; said once, as the button's name.
                modifier = Modifier.padding(bottom = (app.hocket.ui.nav.BottomOverlayInset - 16.dp).coerceAtLeast(0.dp)).testTag("filters.new").semantics(mergeDescendants = true) { contentDescription = newLabel },
            )
        },
    ) { padding ->
        if (filters.isEmpty()) {
            EmptyState(stringResource(R.string.empty_filters_title), stringResource(R.string.empty_filters_body), Modifier.padding(top = padding.calculateTopPadding()), stringResource(R.string.filters_new)) { nav.navigate(Route.FilterBuilder()) }
            return@Scaffold
        }
        LazyColumn(Modifier.fillMaxSize(), contentPadding = PaddingValues(top = padding.calculateTopPadding(), bottom = BottomContentInset)) {
            itemsIndexed(filters, key = { _, f -> f.id }) { i, f ->
                val local = preview(f)
                SegmentedListItem(
                    onClick = { nav.navigate(Route.FilterBuilder(f.id)) },
                    shapes = ListItemDefaults.segmentedShapes(i, filters.size), colors = segmentedListColors(),
                    modifier = Modifier.padding(start = 16.dp, end = 16.dp, top = if (i > 0) ListItemDefaults.SegmentedGap else 0.dp).testTag("filters.row"),
                    leadingContent = {
                        Box(Modifier.size(44.dp).background(MaterialTheme.colorScheme.secondaryContainer, RoundedCornerShape(14.dp)), contentAlignment = Alignment.Center) {
                            Icon(HocketIcons.Outlined.FilterAlt, null, tint = MaterialTheme.colorScheme.onSecondaryContainer)
                        }
                    },
                    supportingContent = { Text(describe(f), maxLines = 2, overflow = TextOverflow.Ellipsis) },
                    trailingContent = {
                        Row {
                            IconButton(onClick = { server?.let { client.dispatch(Commands.playContext(Commands.filterContext(it.id, f))) } }) { Icon(HocketIcons.Filled.PlayArrow, stringResource(R.string.filter_play_named, f.name), tint = MaterialTheme.colorScheme.primary) }
                            IconButton(onClick = { menu = f }, modifier = Modifier.testTag("filters.more")) { Icon(HocketIcons.Filled.MoreVert, stringResource(R.string.action_more_for, f.name)) }
                        }
                    },
                ) {
                    Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                        Text(f.name, fontWeight = FontWeight.SemiBold, maxLines = 1, overflow = TextOverflow.Ellipsis, modifier = Modifier.weight(1f, fill = false))
                        if (local) Badge(stringResource(R.string.filter_this_device), container = MaterialTheme.colorScheme.surfaceContainerHigh)
                    }
                }
            }
        }
    }
    menu?.let { f -> server?.id?.let { sid -> app.hocket.ui.screens.library.FilterMenuSheet(f, nav, sid, onDismiss = { menu = null }, onDelete = { deleting = f }) } }
    deleting?.let { f -> ConfirmDialog(stringResource(R.string.filter_delete_confirm, f.name), stringResource(R.string.action_delete), onConfirm = { client.dispatch(Commands.deleteFilter(f.id)) }, onDismiss = { deleting = null }) }
}

@Composable
fun describe(f: Filter): String = FilterText.describe(f.root)

/** Whether a filter only works here (it reads what is on this device): the boards' "This device". */
private fun preview(f: Filter): Boolean = uses(f.root) { FilterText.isLocal(it) }
private fun uses(node: app.hocket.core.api.FilterNode, test: (app.hocket.core.api.FilterField) -> Boolean): Boolean = when (node) {
    is app.hocket.core.api.FilterNode.Rule -> test(node.data.field)
    is app.hocket.core.api.FilterNode.All -> node.data.any { uses(it, test) }
    is app.hocket.core.api.FilterNode.Any -> node.data.any { uses(it, test) }
}
