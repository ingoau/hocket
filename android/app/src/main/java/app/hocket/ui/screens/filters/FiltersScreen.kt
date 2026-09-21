package app.hocket.ui.screens.filters

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.filled.Add
import androidx.compose.material.icons.filled.Delete
import androidx.compose.material.icons.filled.FilterAlt
import androidx.compose.material.icons.filled.PlayArrow
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.FloatingActionButton
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
    val scroll = TopAppBarDefaults.exitUntilCollapsedScrollBehavior()
    Scaffold(
        modifier = Modifier.nestedScroll(scroll.nestedScrollConnection),
        topBar = { MediumFlexibleTopAppBar(title = { Text(stringResource(R.string.filters_title)) }, navigationIcon = { IconButton(onClick = { nav.popBackStack() }) { Icon(Icons.AutoMirrored.Filled.ArrowBack, stringResource(R.string.action_back)) } }, scrollBehavior = scroll) },
        floatingActionButton = { FloatingActionButton(onClick = { nav.navigate(Route.FilterBuilder()) }, modifier = Modifier.padding(bottom = 72.dp)) { Icon(Icons.Filled.Add, stringResource(R.string.filters_new)) } },
    ) { padding ->
        if (filters.isEmpty()) {
            EmptyState(stringResource(R.string.empty_filters_title), stringResource(R.string.empty_filters_body), Modifier.padding(padding), stringResource(R.string.filters_new)) { nav.navigate(Route.FilterBuilder()) }
            return@Scaffold
        }
        LazyColumn(Modifier.fillMaxSize(), contentPadding = PaddingValues(top = padding.calculateTopPadding(), bottom = 160.dp)) {
            items(filters, key = { it.id }) { f ->
                ListItem(
                    headlineContent = { Text(f.name) },
                    supportingContent = { Text(describe(f)) },
                    leadingContent = { Icon(Icons.Filled.FilterAlt, null) },
                    trailingContent = {
                        androidx.compose.foundation.layout.Row {
                            IconButton(onClick = { server?.let { client.dispatch(Commands.playContext(Commands.filterContext(it.id, f))) } }) { Icon(Icons.Filled.PlayArrow, stringResource(R.string.filter_play)) }
                            IconButton(onClick = { deleting = f }) { Icon(Icons.Filled.Delete, stringResource(R.string.action_delete)) }
                        }
                    },
                    modifier = Modifier.clickable { nav.navigate(Route.FilterBuilder(f.id)) },
                )
            }
        }
    }
    deleting?.let { f -> ConfirmDialog(stringResource(R.string.filter_delete_confirm, f.name), stringResource(R.string.action_delete), onConfirm = { client.dispatch(Commands.deleteFilter(f.id)) }, onDismiss = { deleting = null }) }
}

@Composable
fun describe(f: Filter): String = FilterText.describe(f.root)
