package app.hocket.ui.screens.filters

import android.content.Intent
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.filled.ArrowDropDown
import androidx.compose.material.icons.filled.Close
import androidx.compose.material.icons.filled.PlayArrow
import androidx.compose.material.icons.filled.Save
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.Checkbox
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.FilledTonalButton
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Scaffold
import androidx.compose.material3.SplitButtonDefaults
import androidx.compose.material3.SplitButtonLayout
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.runtime.snapshotFlow
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.navigation.NavHostController
import app.hocket.R
import app.hocket.ui.nav.BottomContentInset
import app.hocket.core.Commands
import app.hocket.core.Queries
import app.hocket.core.api.Event
import app.hocket.core.api.Filter
import app.hocket.core.api.FilterField
import app.hocket.core.api.FilterNode
import app.hocket.core.api.FilterOp
import app.hocket.core.api.FilterPreview
import app.hocket.core.api.FilterRule
import app.hocket.core.api.FilterValue
import app.hocket.core.api.FilterValueRangeInner
import app.hocket.core.api.QueryResult
import app.hocket.core.api.SortOrder
import app.hocket.ui.LocalCoreClient
import app.hocket.ui.components.TrackRow
import app.hocket.ui.screens.library.sortLabel
import kotlinx.coroutines.FlowPreview
import kotlinx.coroutines.flow.debounce
import java.util.UUID

/** Human text for a rule tree, and the field/op vocabulary (a superset of NSP, design: filters). */
object FilterText {
    fun describe(node: FilterNode): String = when (node) {
        is FilterNode.All -> node.data.joinToString(" and ", transform = ::describe)
        is FilterNode.Any -> "(" + node.data.joinToString(" or ", transform = ::describe) + ")"
        is FilterNode.Rule -> "${node.data.field.string} ${node.data.op.string} ${value(node.data.value)}"
    }

    private fun value(v: FilterValue): String = when (v) {
        is FilterValue.Text -> "“${v.data}”"; is FilterValue.Number -> v.data.toString().removeSuffix(".0"); is FilterValue.Range -> "${v.data.low}–${v.data.high}"
        is FilterValue.Date -> v.data; is FilterValue.Bool -> ""; is FilterValue.Days -> "${v.data} days"; is FilterValue.List -> v.data.joinToString(", ")
    }

    val textFields = setOf(FilterField.Title, FilterField.Album, FilterField.Artist, FilterField.AlbumArtist, FilterField.Genre, FilterField.FilePath, FilterField.FileType, FilterField.Comment, FilterField.Key, FilterField.Mood, FilterField.Lyrics, FilterField.InPlaylist)
    val numberFields = setOf(FilterField.Year, FilterField.PlayCount, FilterField.Rating, FilterField.Duration, FilterField.BitRate, FilterField.DiscNumber, FilterField.TrackNumber, FilterField.Bpm, FilterField.Energy, FilterField.LocalPlayCount)
    val dateFields = setOf(FilterField.DateAdded, FilterField.DateModified, FilterField.LastPlayed, FilterField.LocalLastPlayed)
    val boolFields = setOf(FilterField.Loved, FilterField.HasCoverArt, FilterField.Compilation, FilterField.Downloaded, FilterField.Cached)

    fun opsFor(field: FilterField): List<FilterOp> = when (field) {
        in textFields -> listOf(FilterOp.Is, FilterOp.IsNot, FilterOp.Contains, FilterOp.NotContains, FilterOp.StartsWith, FilterOp.EndsWith)
        in numberFields -> listOf(FilterOp.Is, FilterOp.IsNot, FilterOp.Gt, FilterOp.Lt, FilterOp.InTheRange)
        in dateFields -> listOf(FilterOp.Before, FilterOp.After, FilterOp.InTheLast, FilterOp.NotInTheLast)
        else -> listOf(FilterOp.IsTrue, FilterOp.IsFalse)
    }

    fun defaultValue(field: FilterField, op: FilterOp): FilterValue = when {
        op == FilterOp.InTheRange -> FilterValue.Range(FilterValueRangeInner(0.0, 100.0))
        op == FilterOp.InTheLast || op == FilterOp.NotInTheLast -> FilterValue.Days(30u)
        op == FilterOp.Before || op == FilterOp.After -> FilterValue.Date("2024-01-01")
        op == FilterOp.IsTrue -> FilterValue.Bool(true)
        op == FilterOp.IsFalse -> FilterValue.Bool(false)
        field in numberFields -> FilterValue.Number(0.0)
        else -> FilterValue.Text("")
    }
}

/**
 * The rule builder: a tree of all/any groups with field/op/value pickers, a live count from
 * `Query.FilterPreview`, the server-expressibility indicator, and the four outputs (play, save,
 * smart playlist when the capability exists, static playlist, .nsp export through the share sheet).
 */
@OptIn(ExperimentalMaterial3Api::class, FlowPreview::class)
@Composable
fun FilterBuilderScreen(nav: NavHostController, id: String?) {
    val client = LocalCoreClient.current
    val context = LocalContext.current
    val server by client.server.collectAsStateWithLifecycle()
    val saved by client.filters.collectAsStateWithLifecycle()
    val untitled = stringResource(R.string.filter_untitled)
    var filter by remember(id) {
        mutableStateOf(saved.firstOrNull { it.id == id } ?: Filter(UUID.randomUUID().toString().replace("-", ""), untitled,
            FilterNode.All(listOf(FilterNode.Rule(FilterRule(FilterField.Genre, FilterOp.Is, FilterValue.Text(""))))), SortOrder.Title, false, null))
    }
    var preview by remember { mutableStateOf<FilterPreview?>(null) }
    var newName by remember { mutableStateOf("") }
    var namingFor by remember { mutableStateOf<String?>(null) }
    LaunchedEffect(Unit) { snapshotFlow { filter }.debounce(250).collect { f -> preview = (client.query(Queries.filterPreview(f)) as? QueryResult.Preview)?.data } }
    LaunchedEffect(Unit) {
        client.exports.collect { e ->
            if (e is Event.NspExported && e.data.filter_id == filter.id) {
                val send = Intent(Intent.ACTION_SEND).apply { type = "application/json"; putExtra(Intent.EXTRA_SUBJECT, filter.name + ".nsp"); putExtra(Intent.EXTRA_TEXT, e.data.document) }
                context.startActivity(Intent.createChooser(send, filter.name + ".nsp"))
            }
        }
    }
    Scaffold(topBar = {
        TopAppBar(title = { Text(filter.name) }, navigationIcon = { IconButton(onClick = { nav.popBackStack() }) { Icon(Icons.AutoMirrored.Filled.ArrowBack, stringResource(R.string.action_back)) } },
            actions = { IconButton(onClick = { client.dispatch(Commands.saveFilter(filter)) }) { Icon(Icons.Filled.Save, stringResource(R.string.filter_save)) } })
    }) { padding ->
        LazyColumn(Modifier.fillMaxSize(), contentPadding = PaddingValues(top = padding.calculateTopPadding(), bottom = BottomContentInset, start = 16.dp, end = 16.dp)) {
            item { OutlinedTextField(value = filter.name, onValueChange = { filter = filter.copy(name = it) }, label = { Text(stringResource(R.string.filter_name)) }, singleLine = true, modifier = Modifier.fillMaxWidth()) }
            item { Spacer(Modifier.height(12.dp)); GroupEditor(filter.root, depth = 0, onChange = { filter = filter.copy(root = it) }, onRemove = null) }
            item {
                Spacer(Modifier.height(12.dp))
                Row(verticalAlignment = Alignment.CenterVertically) {
                    EnumPicker(stringResource(R.string.filter_sort), sortLabel(filter.sort), SortOrder.entries.map { it to sortLabel(it) }) { filter = filter.copy(sort = it) }
                    Spacer(Modifier.width(8.dp))
                    Checkbox(checked = filter.descending, onCheckedChange = { filter = filter.copy(descending = it) }); Text(stringResource(R.string.sort_descending))
                    Spacer(Modifier.width(8.dp))
                    OutlinedTextField(value = filter.limit?.toString() ?: "", onValueChange = { filter = filter.copy(limit = it.toUIntOrNull()) }, label = { Text(stringResource(R.string.filter_limit)) }, placeholder = { Text(stringResource(R.string.filter_no_limit)) }, singleLine = true, modifier = Modifier.width(120.dp))
                }
            }
            item {
                Spacer(Modifier.height(12.dp))
                val p = preview
                Surface(color = MaterialTheme.colorScheme.surfaceContainer, shape = MaterialTheme.shapes.medium, modifier = Modifier.fillMaxWidth()) {
                    Column(Modifier.padding(12.dp)) {
                        Text(stringResource(R.string.filter_count, p?.count?.toInt() ?: 0), style = MaterialTheme.typography.titleMedium)
                        val cap = p?.capability
                        if (cap != null) {
                            if (cap.serverExpressible) Text(stringResource(R.string.filter_server_ok), color = MaterialTheme.colorScheme.primary, style = MaterialTheme.typography.bodySmall)
                            else Text(stringResource(R.string.filter_local_only, cap.localOnlyFields.joinToString { it.string }), color = MaterialTheme.colorScheme.tertiary, style = MaterialTheme.typography.bodySmall)
                        }
                    }
                }
            }
            item {
                Spacer(Modifier.height(12.dp))
                val sid = server?.id
                val smartOk = server?.capabilities?.nativeApi == true && preview?.capability?.serverExpressible == true
                Row(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalAlignment = Alignment.CenterVertically) {
                    var menu by remember { mutableStateOf(false) }
                    SplitButtonLayout(
                        leadingButton = { SplitButtonDefaults.LeadingButton(onClick = { sid?.let { client.dispatch(Commands.playContext(Commands.filterContext(it, filter))) } }) { Icon(Icons.Filled.PlayArrow, null); Spacer(Modifier.width(6.dp)); Text(stringResource(R.string.filter_play)) } },
                        trailingButton = {
                            SplitButtonDefaults.TrailingButton(onClick = { menu = true }) { Icon(Icons.Filled.ArrowDropDown, stringResource(R.string.action_more)) }
                            DropdownMenu(expanded = menu, onDismissRequest = { menu = false }) {
                                DropdownMenuItem(text = { Text(stringResource(R.string.filter_save)) }, onClick = { client.dispatch(Commands.saveFilter(filter)); menu = false })
                                DropdownMenuItem(text = { Text(stringResource(R.string.filter_create_smart)) }, enabled = smartOk, onClick = { namingFor = "smart"; menu = false })
                                DropdownMenuItem(text = { Text(stringResource(R.string.filter_create_static)) }, onClick = { namingFor = "static"; menu = false })
                                DropdownMenuItem(text = { Text(stringResource(R.string.filter_export)) }, onClick = { client.dispatch(Commands.exportNsp(filter)); menu = false })
                            }
                        },
                    )
                    FilledTonalButton(onClick = { client.dispatch(Commands.exportNsp(filter)) }, shapes = ButtonDefaults.shapes()) { Text(stringResource(R.string.filter_export_share)) }
                }
                if (server?.capabilities?.nativeApi == false) Text(stringResource(R.string.filter_smart_unavailable), style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant, modifier = Modifier.padding(top = 6.dp))
            }
            preview?.sample?.takeIf { it.isNotEmpty() }?.let { sample ->
                item { Spacer(Modifier.height(12.dp)); Text(stringResource(R.string.filter_sample), style = MaterialTheme.typography.titleSmall) }
                items(sample, key = { it.id }) { t -> TrackRow(t, onClick = {}) }
            }
        }
    }
    namingFor?.let { kind ->
        androidx.compose.material3.AlertDialog(onDismissRequest = { namingFor = null }, title = { Text(stringResource(if (kind == "smart") R.string.filter_create_smart else R.string.filter_create_static)) },
            text = { OutlinedTextField(value = newName, onValueChange = { newName = it }, label = { Text(stringResource(R.string.playlist_name)) }, singleLine = true) },
            confirmButton = {
                TextButton(enabled = newName.isNotBlank(), onClick = {
                    server?.id?.let { sid -> client.dispatch(if (kind == "smart") Commands.createSmartPlaylist(sid, filter, newName.trim()) else Commands.createStaticPlaylistFromFilter(sid, filter, newName.trim())) }
                    namingFor = null
                }) { Text(stringResource(R.string.action_save)) }
            },
            dismissButton = { TextButton(onClick = { namingFor = null }) { Text(stringResource(R.string.action_cancel)) } })
    }
}

@Composable
private fun GroupEditor(node: FilterNode, depth: Int, onChange: (FilterNode) -> Unit, onRemove: (() -> Unit)?) {
    val children = when (node) { is FilterNode.All -> node.data; is FilterNode.Any -> node.data; is FilterNode.Rule -> listOf(node) }
    val isAny = node is FilterNode.Any
    fun rebuild(list: List<FilterNode>): FilterNode = if (isAny) FilterNode.Any(list) else FilterNode.All(list)
    if (node is FilterNode.Rule) { RuleEditor(node.data, onChange = { onChange(FilterNode.Rule(it)) }, onRemove = onRemove ?: {}); return }
    Surface(color = if (depth % 2 == 0) MaterialTheme.colorScheme.surfaceContainerLow else MaterialTheme.colorScheme.surfaceContainer, shape = MaterialTheme.shapes.medium, modifier = Modifier.fillMaxWidth()) {
        Column(Modifier.padding(8.dp)) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                EnumPicker(null, stringResource(if (isAny) R.string.filter_match_any else R.string.filter_match_all), listOf(false to stringResource(R.string.filter_match_all), true to stringResource(R.string.filter_match_any))) { any -> onChange(if (any) FilterNode.Any(children) else FilterNode.All(children)) }
                Spacer(Modifier.weight(1f))
                TextButton(onClick = { onChange(rebuild(children + FilterNode.Rule(FilterRule(FilterField.Title, FilterOp.Contains, FilterValue.Text(""))))) }) { Text(stringResource(R.string.filter_add_rule)) }
                if (depth < 3) TextButton(onClick = {
                    val group: FilterNode = FilterNode.Any(listOf(FilterNode.Rule(FilterRule(FilterField.Artist, FilterOp.Is, FilterValue.Text("")))))
                    onChange(rebuild(children + group))
                }) { Text(stringResource(R.string.filter_add_group)) }
                if (onRemove != null) IconButton(onClick = onRemove) { Icon(Icons.Filled.Close, stringResource(R.string.filter_remove_rule)) }
            }
            children.forEachIndexed { i, child ->
                Spacer(Modifier.height(6.dp))
                GroupEditor(child, depth + 1, onChange = { new -> onChange(rebuild(children.toMutableList().also { it[i] = new })) }, onRemove = { onChange(rebuild(children.toMutableList().also { it.removeAt(i) })) })
            }
        }
    }
}

@Composable
private fun RuleEditor(rule: FilterRule, onChange: (FilterRule) -> Unit, onRemove: () -> Unit) {
    Surface(color = MaterialTheme.colorScheme.surfaceContainerHigh, shape = MaterialTheme.shapes.small, modifier = Modifier.fillMaxWidth()) {
        Column(Modifier.padding(8.dp)) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                EnumPicker(stringResource(R.string.filter_field), rule.field.string, FilterField.entries.map { it to it.string }) { f ->
                    val op = FilterText.opsFor(f).first(); onChange(FilterRule(f, op, FilterText.defaultValue(f, op)))
                }
                Spacer(Modifier.width(6.dp))
                EnumPicker(stringResource(R.string.filter_op), rule.op.string, FilterText.opsFor(rule.field).map { it to it.string }) { op -> onChange(rule.copy(op = op, value = FilterText.defaultValue(rule.field, op))) }
                Spacer(Modifier.weight(1f))
                IconButton(onClick = onRemove) { Icon(Icons.Filled.Close, stringResource(R.string.filter_remove_rule)) }
            }
            ValueEditor(rule.value, onChange = { onChange(rule.copy(value = it)) })
        }
    }
}

@Composable
private fun ValueEditor(value: FilterValue, onChange: (FilterValue) -> Unit) {
    when (value) {
        is FilterValue.Text -> OutlinedTextField(value = value.data, onValueChange = { onChange(FilterValue.Text(it)) }, label = { Text(stringResource(R.string.filter_value)) }, singleLine = true, modifier = Modifier.fillMaxWidth())
        is FilterValue.Number -> OutlinedTextField(value = value.data.toString().removeSuffix(".0"), onValueChange = { onChange(FilterValue.Number(it.toDoubleOrNull() ?: 0.0)) }, label = { Text(stringResource(R.string.filter_value)) }, singleLine = true, modifier = Modifier.fillMaxWidth())
        is FilterValue.Range -> Row {
            OutlinedTextField(value = value.data.low.toString().removeSuffix(".0"), onValueChange = { onChange(FilterValue.Range(value.data.copy(low = it.toDoubleOrNull() ?: 0.0))) }, label = { Text(stringResource(R.string.filter_value_low)) }, singleLine = true, modifier = Modifier.weight(1f))
            Spacer(Modifier.width(8.dp))
            OutlinedTextField(value = value.data.high.toString().removeSuffix(".0"), onValueChange = { onChange(FilterValue.Range(value.data.copy(high = it.toDoubleOrNull() ?: 0.0))) }, label = { Text(stringResource(R.string.filter_value_high)) }, singleLine = true, modifier = Modifier.weight(1f))
        }
        is FilterValue.Date -> OutlinedTextField(value = value.data, onValueChange = { onChange(FilterValue.Date(it)) }, label = { Text("YYYY-MM-DD") }, singleLine = true, modifier = Modifier.fillMaxWidth())
        is FilterValue.Days -> OutlinedTextField(value = value.data.toString(), onValueChange = { onChange(FilterValue.Days(it.toUIntOrNull() ?: 0u)) }, label = { Text(stringResource(R.string.filter_days)) }, singleLine = true, modifier = Modifier.fillMaxWidth())
        is FilterValue.Bool -> Unit
        is FilterValue.List -> OutlinedTextField(value = value.data.joinToString(", "), onValueChange = { onChange(FilterValue.List(it.split(",").map { s -> s.trim() }.filter { s -> s.isNotEmpty() })) }, label = { Text(stringResource(R.string.filter_value)) }, modifier = Modifier.fillMaxWidth())
    }
}

@Composable
fun <T> EnumPicker(label: String?, current: String, options: List<Pair<T, String>>, onPick: (T) -> Unit) {
    var open by remember { mutableStateOf(false) }
    androidx.compose.foundation.layout.Box {
        OutlinedButton(onClick = { open = true }, shapes = ButtonDefaults.shapes()) { Text(if (label != null) "$label: $current" else current); Icon(Icons.Filled.ArrowDropDown, null) }
        DropdownMenu(expanded = open, onDismissRequest = { open = false }) {
            options.forEach { (v, text) -> DropdownMenuItem(text = { Text(text) }, onClick = { onPick(v); open = false }) }
        }
    }
}
