package app.hocket.ui.screens.filters

import android.content.Intent
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.lazy.LazyRow
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.selection.selectableGroup
import androidx.compose.foundation.selection.toggleable
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.ButtonGroupDefaults
import androidx.compose.material3.FilterChip
import androidx.compose.material3.ListItemDefaults
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.Switch
import androidx.compose.material3.ToggleButton
import androidx.compose.material3.ToggleButtonDefaults
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.role
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import app.hocket.ui.components.Segment
import app.hocket.ui.components.SegmentedGroup
import app.hocket.ui.components.segmentColor
import app.hocket.ui.components.segmentedShape
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
import app.hocket.ui.icons.HocketIcons
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
    /** A filter's rules in one line: "Rating is 5 and Duration less than 4:00", a nested group in brackets. */
    fun describe(node: FilterNode, nested: Boolean = false): String = when (node) {
        is FilterNode.Rule -> "${fieldLabel(node.data.field)} ${condition(node.data)}"
        else -> {
            val parts = (if (node is FilterNode.Any) node.data else (node as FilterNode.All).data).map { describe(it, true) }.filter { it.isNotEmpty() }
            val text = parts.joinToString(if (node is FilterNode.Any) " or " else " and ")
            if (nested && parts.size > 1) "($text)" else text
        }
    }

    private val FIELD_NAMES = mapOf(
        FilterField.Bpm to "BPM", FilterField.BitRate to "Bit rate", FilterField.FilePath to "File path", FilterField.FileType to "File type",
        FilterField.HasCoverArt to "Has cover art", FilterField.LocalPlayCount to "Plays on this device", FilterField.LocalLastPlayed to "Last played on this device",
        FilterField.AvailableOffline to "Available offline",
    )

    /** "dateAdded" → "Date added", unless it has a name of its own (the desktop's labels). */
    fun fieldLabel(f: FilterField): String = FIELD_NAMES[f] ?: f.string.replace(Regex("([A-Z])"), " $1").lowercase().replaceFirstChar { it.uppercase() }

    fun opLabel(op: FilterOp): String = when (op) {
        FilterOp.Is -> "is"; FilterOp.IsNot -> "is not"; FilterOp.Contains -> "contains"; FilterOp.NotContains -> "doesn’t contain"
        FilterOp.StartsWith -> "starts with"; FilterOp.EndsWith -> "ends with"; FilterOp.Gt -> "greater than"; FilterOp.Lt -> "less than"
        FilterOp.InTheRange -> "in the range"; FilterOp.Before -> "before"; FilterOp.After -> "after"; FilterOp.InTheLast -> "in the last"
        FilterOp.NotInTheLast -> "not in the last"; FilterOp.IsTrue -> "is true"; FilterOp.IsFalse -> "is false"
    }

    private fun num(field: FilterField, n: Double): String = if (field == FilterField.Duration) app.hocket.ui.components.formatClock((n * 1000).toLong()) else n.toString().removeSuffix(".0")

    /** A rule's condition and value as it reads in the list ("is “Ambient”", "less than 4:00"). */
    fun condition(rule: FilterRule): String {
        val op = opLabel(rule.op)
        return when (val v = rule.value) {
            is FilterValue.Text -> "$op “${v.data}”"
            is FilterValue.Number -> "$op ${num(rule.field, v.data)}"
            is FilterValue.Range -> "$op ${num(rule.field, v.data.low)}–${num(rule.field, v.data.high)}"
            is FilterValue.Date -> "$op ${v.data}"
            is FilterValue.Days -> "$op ${v.data} days"
            is FilterValue.Bool -> op
            is FilterValue.List -> "$op " + v.data.joinToString(", ") { "“$it”" }
        }
    }

    private val LOCAL_FIELDS = listOf(FilterField.Downloaded, FilterField.Cached, FilterField.AvailableOffline, FilterField.LocalPlayCount, FilterField.LocalLastPlayed, FilterField.InPlaylist)

    /** The rule sheet's field chips, by what they describe. */
    val fieldGroups: List<Pair<String, List<FilterField>>> by lazy {
        val groups = listOf(
            "Song" to listOf(FilterField.Title, FilterField.Genre, FilterField.Year, FilterField.Duration, FilterField.Comment, FilterField.Lyrics, FilterField.DiscNumber, FilterField.TrackNumber),
            "Album and artist" to listOf(FilterField.Album, FilterField.Artist, FilterField.AlbumArtist, FilterField.Compilation),
            "Plays and ratings" to listOf(FilterField.PlayCount, FilterField.Rating, FilterField.Loved, FilterField.LastPlayed, FilterField.DateAdded, FilterField.DateModified),
            "Sound" to listOf(FilterField.Bpm, FilterField.Energy, FilterField.Key, FilterField.Mood, FilterField.BitRate, FilterField.FileType),
            "File" to listOf(FilterField.FilePath, FilterField.HasCoverArt),
            "This device" to LOCAL_FIELDS,
        )
        val rest = FilterField.entries.filter { f -> groups.none { f in it.second } }
        groups.mapIndexed { i, g -> if (i == 0) g.first to g.second + rest else g }
    }

    fun isLocal(f: FilterField): Boolean = f in LOCAL_FIELDS

    val textFields = setOf(FilterField.Title, FilterField.Album, FilterField.Artist, FilterField.AlbumArtist, FilterField.Genre, FilterField.FilePath, FilterField.FileType, FilterField.Comment, FilterField.Key, FilterField.Mood, FilterField.Lyrics, FilterField.InPlaylist)
    val numberFields = setOf(FilterField.Year, FilterField.PlayCount, FilterField.Rating, FilterField.Duration, FilterField.BitRate, FilterField.DiscNumber, FilterField.TrackNumber, FilterField.Bpm, FilterField.Energy, FilterField.LocalPlayCount)
    val dateFields = setOf(FilterField.DateAdded, FilterField.DateModified, FilterField.LastPlayed, FilterField.LocalLastPlayed)
    val boolFields = setOf(FilterField.Loved, FilterField.HasCoverArt, FilterField.Compilation, FilterField.Downloaded, FilterField.Cached, FilterField.AvailableOffline)

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
    // Opened before the saved filters arrived (a cold start, a link): take the saved one when it lands.
    LaunchedEffect(saved, id) { if (id != null && filter.id != id) saved.firstOrNull { it.id == id }?.let { filter = it } }
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
    // The rule being edited in the sheet, by its path in the tree.
    var editing by remember { mutableStateOf<List<Int>?>(null) }
    val sid = server?.id
    val smartOk = server?.capabilities?.nativeApi == true && preview?.capability?.serverExpressible == true
    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text(filter.name.ifBlank { untitled }, style = MaterialTheme.typography.titleLarge, fontWeight = FontWeight.SemiBold, maxLines = 1, overflow = TextOverflow.Ellipsis) },
                navigationIcon = { IconButton(onClick = { nav.popBackStack() }) { Icon(HocketIcons.AutoMirrored.Filled.ArrowBack, stringResource(R.string.action_back)) } },
                actions = { FilledTonalButton(onClick = { client.dispatch(Commands.saveFilter(filter)) }, shapes = ButtonDefaults.shapes(), modifier = Modifier.padding(end = 8.dp).testTag("filter.save")) { Text(stringResource(R.string.filter_save)) } },
            )
        },
        bottomBar = {
            // The live count and what the filter can become, over its outputs: Play (with the rest behind its arrow) and Share.
            Surface(color = MaterialTheme.colorScheme.surfaceContainer, shape = RoundedCornerShape(topStart = 28.dp, topEnd = 28.dp), shadowElevation = 4.dp) {
                Column(Modifier.fillMaxWidth().navigationBarsPadding().padding(start = 16.dp, end = 16.dp, top = 14.dp, bottom = 16.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
                    val p = preview
                    Column(Modifier.padding(horizontal = 4.dp).semantics(mergeDescendants = true) { liveRegion = LiveRegionMode.Polite }) {
                        Text(stringResource(R.string.filter_count, p?.count?.toInt() ?: 0), style = MaterialTheme.typography.titleLarge, fontWeight = FontWeight.SemiBold)
                        val cap = p?.capability
                        if (cap != null) Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                            Icon(if (cap.serverExpressible) HocketIcons.Filled.Check else HocketIcons.Filled.Info, null, Modifier.size(16.dp), tint = if (cap.serverExpressible) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.onSurfaceVariant)
                            Text(if (cap.serverExpressible) stringResource(R.string.filter_server_ok) else stringResource(R.string.filter_local_only, cap.localOnlyFields.joinToString { FilterText.fieldLabel(it) }),
                                style = MaterialTheme.typography.labelLarge, color = if (cap.serverExpressible) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.onSurfaceVariant)
                        }
                        if (server?.capabilities?.nativeApi == false) Text(stringResource(R.string.filter_smart_unavailable), style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
                    }
                    Row(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalAlignment = Alignment.CenterVertically) {
                        var menu by remember { mutableStateOf(false) }
                        Box(Modifier.weight(1f)) {
                            SplitButtonLayout(
                                leadingButton = {
                                    SplitButtonDefaults.LeadingButton(onClick = { sid?.let { client.dispatch(Commands.playContext(Commands.filterContext(it, filter))) } }, modifier = Modifier.heightIn(min = 56.dp).fillMaxWidth(0.78f)) {
                                        Icon(HocketIcons.Filled.PlayArrow, null); Spacer(Modifier.width(8.dp)); Text(stringResource(R.string.filter_play), style = MaterialTheme.typography.titleMedium)
                                    }
                                },
                                trailingButton = {
                                    SplitButtonDefaults.TrailingButton(checked = menu, onCheckedChange = { menu = it }, modifier = Modifier.heightIn(min = 56.dp).semantics { contentDescription = "" }) {
                                        Icon(HocketIcons.Filled.KeyboardArrowDown, stringResource(R.string.filter_more_outputs))
                                    }
                                    DropdownMenu(expanded = menu, onDismissRequest = { menu = false }) {
                                        DropdownMenuItem(text = { Text(stringResource(R.string.filter_save)) }, onClick = { client.dispatch(Commands.saveFilter(filter)); menu = false })
                                        DropdownMenuItem(text = { Text(stringResource(R.string.filter_create_smart)) }, enabled = smartOk, onClick = { namingFor = "smart"; menu = false })
                                        DropdownMenuItem(text = { Text(stringResource(R.string.filter_create_static)) }, onClick = { namingFor = "static"; menu = false })
                                        DropdownMenuItem(text = { Text(stringResource(R.string.filter_export)) }, onClick = { client.dispatch(Commands.exportNsp(filter)); menu = false })
                                    }
                                },
                            )
                        }
                        FilledTonalButton(onClick = { client.dispatch(Commands.exportNsp(filter)) }, shapes = ButtonDefaults.shapes(), modifier = Modifier.heightIn(min = 56.dp)) { Text(stringResource(R.string.filter_export_share)) }
                    }
                }
            }
        },
    ) { padding ->
        LazyColumn(Modifier.fillMaxSize(), contentPadding = PaddingValues(top = padding.calculateTopPadding() + 4.dp, bottom = padding.calculateBottomPadding() + 24.dp, start = 16.dp, end = 16.dp)) {
            item(key = "name") {
                OutlinedTextField(value = filter.name, onValueChange = { filter = filter.copy(name = it) }, label = { Text(stringResource(R.string.filter_name)) }, singleLine = true,
                    shape = RoundedCornerShape(16.dp), modifier = Modifier.fillMaxWidth().testTag("filter.name"))
            }
            item(key = "rules") {
                Surface(color = MaterialTheme.colorScheme.surfaceContainer, shape = RoundedCornerShape(24.dp), modifier = Modifier.fillMaxWidth().padding(top = 20.dp)) {
                    Column(Modifier.padding(12.dp)) {
                        RulesGroup(filter.root, path = emptyList(), top = true, onChange = { filter = filter.copy(root = it) }, onEdit = { editing = it })
                    }
                }
            }
            item(key = "order") {
                SegmentedGroup(Modifier.padding(top = 20.dp)) {
                    Segment(0, 3) {
                        Column(Modifier.padding(top = 12.dp, bottom = 12.dp)) {
                            Text(stringResource(R.string.filter_sort), style = MaterialTheme.typography.bodyLarge, modifier = Modifier.padding(start = 16.dp, bottom = 8.dp))
                            // Album and song counts order artists only (for songs they are the default order).
                            LazyRow(contentPadding = PaddingValues(horizontal = 16.dp), horizontalArrangement = Arrangement.spacedBy(8.dp), modifier = Modifier.selectableGroup()) {
                                items(SortOrder.entries.filter { it != SortOrder.AlbumCount && it != SortOrder.SongCount }, key = { it.string }) { o ->
                                    FilterChip(selected = filter.sort == o, onClick = { filter = filter.copy(sort = o) }, label = { Text(sortLabel(o)) }, modifier = Modifier.semantics { role = Role.RadioButton })
                                }
                            }
                        }
                    }
                    Segment(1, 3) {
                        Row(Modifier.heightIn(min = 60.dp).toggleable(filter.descending, role = Role.Switch) { filter = filter.copy(descending = it) }.padding(start = 16.dp, end = 12.dp), verticalAlignment = Alignment.CenterVertically) {
                            Text(stringResource(R.string.sort_descending), style = MaterialTheme.typography.bodyLarge, modifier = Modifier.weight(1f))
                            Switch(checked = filter.descending, onCheckedChange = null)
                        }
                    }
                    Segment(2, 3) {
                        Row(Modifier.heightIn(min = 60.dp).padding(start = 16.dp, end = 12.dp, top = 6.dp, bottom = 6.dp), verticalAlignment = Alignment.CenterVertically) {
                            Text(stringResource(R.string.filter_limit), style = MaterialTheme.typography.bodyLarge, modifier = Modifier.weight(1f))
                            OutlinedTextField(value = filter.limit?.toString() ?: "", onValueChange = { filter = filter.copy(limit = it.toUIntOrNull()) }, placeholder = { Text(stringResource(R.string.filter_no_limit), textAlign = TextAlign.End, modifier = Modifier.fillMaxWidth()) },
                                singleLine = true, shape = RoundedCornerShape(12.dp), textStyle = MaterialTheme.typography.bodyLarge.copy(textAlign = TextAlign.End),
                                keyboardOptions = androidx.compose.foundation.text.KeyboardOptions(keyboardType = androidx.compose.ui.text.input.KeyboardType.Number), modifier = Modifier.width(128.dp).semantics { contentDescription = "" })
                        }
                    }
                }
            }
            preview?.sample?.takeIf { it.isNotEmpty() }?.let { sample ->
                item(key = "h.sample") { Text(stringResource(R.string.filter_sample), style = MaterialTheme.typography.titleMedium, fontWeight = FontWeight.SemiBold, modifier = Modifier.padding(start = 4.dp, top = 20.dp, bottom = 8.dp).semantics { heading() }) }
                itemsIndexed(sample, key = { _, t -> t.id }) { i, t ->
                    TrackRow(t, onClick = {}, segment = i to sample.size, showDuration = false, subtitle = listOfNotNull(t.artist, t.album).joinToString(stringResource(R.string.dot_separator)),
                        modifier = Modifier.padding(top = if (i > 0) ListItemDefaults.SegmentedGap else 0.dp))
                }
            }
        }
    }
    editing?.let { path ->
        val rule = (nodeAt(filter.root, path) as? FilterNode.Rule)?.data
        if (rule == null) editing = null
        else RuleSheet(rule, onChange = { filter = filter.copy(root = replaceAt(filter.root, path, FilterNode.Rule(it))) },
            onRemove = { filter = filter.copy(root = removeAt(filter.root, path)); editing = null }, onDismiss = { editing = null })
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

private fun children(node: FilterNode): List<FilterNode> = when (node) { is FilterNode.All -> node.data; is FilterNode.Any -> node.data; is FilterNode.Rule -> listOf(node) }
private fun withChildren(node: FilterNode, list: List<FilterNode>): FilterNode = if (node is FilterNode.Any) FilterNode.Any(list) else FilterNode.All(list)
private fun nodeAt(root: FilterNode, path: List<Int>): FilterNode? = if (path.isEmpty()) root else children(root).getOrNull(path.first())?.let { nodeAt(it, path.drop(1)) }
private fun replaceAt(root: FilterNode, path: List<Int>, new: FilterNode): FilterNode =
    if (path.isEmpty()) new else withChildren(root, children(root).toMutableList().also { it[path.first()] = replaceAt(it[path.first()], path.drop(1), new) })
private fun removeAt(root: FilterNode, path: List<Int>): FilterNode =
    if (path.size == 1) withChildren(root, children(root).toMutableList().also { it.removeAt(path.first()) })
    else withChildren(root, children(root).toMutableList().also { it[path.first()] = removeAt(it[path.first()], path.drop(1)) })

private fun newRule(): FilterNode = FilterNode.Rule(FilterRule(FilterField.Artist, FilterOp.Contains, FilterValue.Text("")))

/**
 * A group of rules: Match all/any (a connected pair at the top, a toggle on a nested group), its
 * rules as a segmented list (each opens [RuleSheet]; × removes it), nested groups on a darker
 * container, and Add rule / Add group under the top group.
 */
@Composable
private fun RulesGroup(node: FilterNode, path: List<Int>, top: Boolean, onChange: (FilterNode) -> Unit, onEdit: (List<Int>) -> Unit, onRemove: (() -> Unit)? = null) {
    val list = children(node)
    val any = node is FilterNode.Any
    if (top) {
        Row(Modifier.fillMaxWidth().selectableGroup(), horizontalArrangement = Arrangement.spacedBy(ButtonGroupDefaults.ConnectedSpaceBetween)) {
            listOf(false to R.string.filter_match_all, true to R.string.filter_match_any).forEachIndexed { i, (isAny, label) ->
                ToggleButton(checked = any == isAny, onCheckedChange = { onChange(if (isAny) FilterNode.Any(list) else FilterNode.All(list)) },
                    shapes = if (i == 0) ButtonGroupDefaults.connectedLeadingButtonShapes() else ButtonGroupDefaults.connectedTrailingButtonShapes(),
                    colors = ToggleButtonDefaults.colors(containerColor = segmentColor()),
                    modifier = Modifier.weight(1f).heightIn(min = 44.dp).semantics { role = Role.RadioButton }) { Text(stringResource(label)) }
            }
        }
    } else {
        Row(verticalAlignment = Alignment.CenterVertically) {
            FilledTonalButton(onClick = { onChange(if (any) FilterNode.All(list) else FilterNode.Any(list)) }, colors = ButtonDefaults.filledTonalButtonColors(containerColor = segmentColor(), contentColor = MaterialTheme.colorScheme.onSurface), shapes = ButtonDefaults.shapes()) {
                Text(stringResource(if (any) R.string.filter_match_any else R.string.filter_match_all)); Icon(HocketIcons.Filled.ArrowDropDown, null)
            }
            Spacer(Modifier.weight(1f))
            TextButton(onClick = { onChange(withChildren(node, list + newRule())) }) { Text(stringResource(R.string.filter_add_rule)) }
            if (onRemove != null) IconButton(onClick = onRemove) { Icon(HocketIcons.Filled.Close, stringResource(R.string.filter_remove_group)) }
        }
    }
    Column(Modifier.padding(top = if (top) 10.dp else 6.dp), verticalArrangement = Arrangement.spacedBy(ListItemDefaults.SegmentedGap)) {
        list.forEachIndexed { i, child ->
            val shape = segmentedShape(i, list.size)
            when (child) {
                is FilterNode.Rule -> Surface(onClick = { onEdit(path + i) }, shape = shape, color = segmentColor(), modifier = Modifier.fillMaxWidth().testTag("filter.rule")) {
                    Row(Modifier.heightIn(min = if (top) 60.dp else 56.dp).padding(start = 16.dp, end = 4.dp), verticalAlignment = Alignment.CenterVertically) {
                        Column(Modifier.weight(1f).padding(vertical = 8.dp)) {
                            Text(FilterText.fieldLabel(child.data.field), style = MaterialTheme.typography.labelMedium, fontWeight = FontWeight.SemiBold, color = MaterialTheme.colorScheme.onSurfaceVariant)
                            Text(FilterText.condition(child.data), style = MaterialTheme.typography.bodyLarge, maxLines = 1, overflow = TextOverflow.Ellipsis)
                        }
                        if (list.size > 1 || !top) IconButton(onClick = { onChange(withChildren(node, list.toMutableList().also { it.removeAt(i) })) }) {
                            Icon(HocketIcons.Filled.Close, stringResource(R.string.filter_remove_named, FilterText.fieldLabel(child.data.field)))
                        }
                    }
                }
                else -> Surface(shape = shape, color = MaterialTheme.colorScheme.surfaceContainerHighest, modifier = Modifier.fillMaxWidth()) {
                    Column(Modifier.padding(8.dp)) {
                        RulesGroup(child, path + i, top = false, onChange = { new -> onChange(withChildren(node, list.toMutableList().also { it[i] = new })) }, onEdit = onEdit,
                            onRemove = { onChange(withChildren(node, list.toMutableList().also { it.removeAt(i) })) })
                    }
                }
            }
        }
    }
    if (top) Row(Modifier.padding(top = 10.dp), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
        Button(onClick = { onChange(withChildren(node, list + newRule())) }, shapes = ButtonDefaults.shapes(), modifier = Modifier.testTag("filter.addRule")) {
            Icon(HocketIcons.Filled.Add, null, Modifier.size(ButtonDefaults.IconSize)); Spacer(Modifier.width(ButtonDefaults.IconSpacing)); Text(stringResource(R.string.filter_add_rule))
        }
        if (path.size < 3) FilledTonalButton(onClick = { onChange(withChildren(node, list + FilterNode.Any(listOf(FilterNode.Rule(FilterRule(FilterField.Artist, FilterOp.Is, FilterValue.Text(""))))))) },
            colors = ButtonDefaults.filledTonalButtonColors(containerColor = segmentColor(), contentColor = MaterialTheme.colorScheme.primary), shapes = ButtonDefaults.shapes()) {
            Icon(HocketIcons.Outlined.Category, null, Modifier.size(ButtonDefaults.IconSize)); Spacer(Modifier.width(ButtonDefaults.IconSpacing)); Text(stringResource(R.string.filter_add_group))
        }
    }
}

/**
 * One rule, edited in a sheet: its field as chips grouped by what they describe, the conditions the
 * field has, its value; Remove rule, and Done (changes apply as they are made).
 */
@Composable
private fun RuleSheet(rule: FilterRule, onChange: (FilterRule) -> Unit, onRemove: () -> Unit, onDismiss: () -> Unit) {
    ModalBottomSheet(onDismissRequest = onDismiss, sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = true), modifier = Modifier.testTag("filter.ruleSheet")) {
        Column(Modifier.navigationBarsPadding().padding(start = 16.dp, end = 16.dp, bottom = 24.dp), verticalArrangement = Arrangement.spacedBy(14.dp)) {
            Text("${FilterText.fieldLabel(rule.field)} ${FilterText.condition(rule)}", style = MaterialTheme.typography.headlineSmall, fontWeight = FontWeight.SemiBold, maxLines = 2, overflow = TextOverflow.Ellipsis,
                modifier = Modifier.padding(horizontal = 4.dp).semantics { heading() })
            SheetLabel(stringResource(R.string.filter_field))
            Surface(shape = RoundedCornerShape(20.dp), color = segmentColor()) {
                Column(Modifier.fillMaxWidth().heightIn(max = 196.dp).verticalScroll(rememberScrollState()).padding(start = 12.dp, end = 12.dp, bottom = 12.dp)) {
                    FilterText.fieldGroups.forEach { (label, fields) ->
                        Text(label, style = MaterialTheme.typography.labelMedium, fontWeight = FontWeight.SemiBold, color = MaterialTheme.colorScheme.onSurfaceVariant, modifier = Modifier.padding(start = 2.dp, top = 10.dp, bottom = 6.dp))
                        FlowRow(horizontalArrangement = Arrangement.spacedBy(6.dp), modifier = Modifier.selectableGroup()) {
                            fields.forEach { f ->
                                FilterChip(selected = rule.field == f, onClick = { if (rule.field != f) { val op = FilterText.opsFor(f).first(); onChange(FilterRule(f, op, FilterText.defaultValue(f, op))) } },
                                    label = { Text(FilterText.fieldLabel(f)) }, modifier = Modifier.semantics { role = Role.RadioButton })
                            }
                        }
                    }
                }
            }
            SheetLabel(stringResource(R.string.filter_op))
            FlowRow(horizontalArrangement = Arrangement.spacedBy(6.dp), modifier = Modifier.selectableGroup()) {
                FilterText.opsFor(rule.field).forEach { op ->
                    FilterChip(selected = rule.op == op, onClick = { if (rule.op != op) onChange(rule.copy(op = op, value = FilterText.defaultValue(rule.field, op))) }, label = { Text(FilterText.opLabel(op)) },
                        modifier = Modifier.semantics { role = Role.RadioButton })
                }
            }
            ValueEditor(rule.value, onChange = { onChange(rule.copy(value = it)) })
            if (FilterText.isLocal(rule.field)) Text(stringResource(R.string.filter_rule_local), style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant, modifier = Modifier.padding(horizontal = 4.dp))
            Row(Modifier.padding(top = 4.dp), verticalAlignment = Alignment.CenterVertically) {
                TextButton(onClick = onRemove, colors = ButtonDefaults.textButtonColors(contentColor = MaterialTheme.colorScheme.error), modifier = Modifier.heightIn(min = 52.dp)) { Text(stringResource(R.string.filter_remove_rule)) }
                Spacer(Modifier.weight(1f))
                Button(onClick = onDismiss, shapes = ButtonDefaults.shapes(), modifier = Modifier.heightIn(min = 52.dp).testTag("filter.ruleSheet.done")) { Text(stringResource(R.string.action_done)) }
            }
        }
    }
}

@Composable
private fun SheetLabel(text: String) {
    Text(text, style = MaterialTheme.typography.labelLarge, fontWeight = FontWeight.SemiBold, color = MaterialTheme.colorScheme.onSurfaceVariant, modifier = Modifier.padding(horizontal = 4.dp))
}

@Composable
private fun ValueEditor(value: FilterValue, onChange: (FilterValue) -> Unit) {
    val shape = RoundedCornerShape(16.dp)
    when (value) {
        is FilterValue.Text -> OutlinedTextField(value = value.data, onValueChange = { onChange(FilterValue.Text(it)) }, label = { Text(stringResource(R.string.filter_value)) }, singleLine = true, shape = shape, modifier = Modifier.fillMaxWidth())
        is FilterValue.Number -> OutlinedTextField(value = value.data.toString().removeSuffix(".0"), onValueChange = { onChange(FilterValue.Number(it.toDoubleOrNull() ?: 0.0)) }, label = { Text(stringResource(R.string.filter_value)) }, singleLine = true, shape = shape, modifier = Modifier.fillMaxWidth())
        is FilterValue.Range -> Row {
            OutlinedTextField(value = value.data.low.toString().removeSuffix(".0"), onValueChange = { onChange(FilterValue.Range(value.data.copy(low = it.toDoubleOrNull() ?: 0.0))) }, label = { Text(stringResource(R.string.filter_value_low)) }, singleLine = true, shape = shape, modifier = Modifier.weight(1f))
            Spacer(Modifier.width(8.dp))
            OutlinedTextField(value = value.data.high.toString().removeSuffix(".0"), onValueChange = { onChange(FilterValue.Range(value.data.copy(high = it.toDoubleOrNull() ?: 0.0))) }, label = { Text(stringResource(R.string.filter_value_high)) }, singleLine = true, shape = shape, modifier = Modifier.weight(1f))
        }
        is FilterValue.Date -> OutlinedTextField(value = value.data, onValueChange = { onChange(FilterValue.Date(it)) }, label = { Text("YYYY-MM-DD") }, singleLine = true, shape = shape, modifier = Modifier.fillMaxWidth())
        is FilterValue.Days -> OutlinedTextField(value = value.data.toString(), onValueChange = { onChange(FilterValue.Days(it.toUIntOrNull() ?: 0u)) }, label = { Text(stringResource(R.string.filter_days)) }, singleLine = true, shape = shape, modifier = Modifier.fillMaxWidth())
        is FilterValue.Bool -> Unit
        is FilterValue.List -> OutlinedTextField(value = value.data.joinToString(", "), onValueChange = { onChange(FilterValue.List(it.split(",").map { s -> s.trim() }.filter { s -> s.isNotEmpty() })) }, label = { Text(stringResource(R.string.filter_value)) }, shape = shape, modifier = Modifier.fillMaxWidth())
    }
}

@Composable
fun <T> EnumPicker(label: String?, current: String, options: List<Pair<T, String>>, onPick: (T) -> Unit) {
    var open by remember { mutableStateOf(false) }
    androidx.compose.foundation.layout.Box {
        OutlinedButton(onClick = { open = true }, shapes = ButtonDefaults.shapes()) { Text(if (label != null) "$label: $current" else current); Icon(HocketIcons.Filled.ArrowDropDown, null) }
        DropdownMenu(expanded = open, onDismissRequest = { open = false }) {
            options.forEach { (v, text) -> DropdownMenuItem(text = { Text(text) }, onClick = { onPick(v); open = false }) }
        }
    }
}
