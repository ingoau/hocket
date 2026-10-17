package app.hocket.ui.components

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.BasicAlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.FilledTonalButton
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.LoadingIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.OutlinedTextFieldDefaults
import androidx.compose.material3.SnackbarDuration
import androidx.compose.material3.SnackbarHostState
import androidx.compose.material3.SnackbarResult
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TriStateCheckbox
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.Immutable
import androidx.compose.runtime.Stable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.produceState
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.compositeOver
import androidx.compose.ui.graphics.Shape
import androidx.compose.ui.hapticfeedback.HapticFeedbackType
import androidx.compose.ui.platform.LocalHapticFeedback
import androidx.compose.ui.platform.LocalResources
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.CustomAccessibilityAction
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.customActions
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.toggleableState
import androidx.compose.ui.state.ToggleableState
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.text.withStyle
import androidx.compose.ui.unit.dp
import app.hocket.R
import app.hocket.core.ArtworkSizes
import app.hocket.core.Commands
import app.hocket.core.Queries
import app.hocket.core.SettingKeys
import app.hocket.core.api.ActionTarget
import app.hocket.core.api.Playlist
import app.hocket.core.api.QueryResult
import app.hocket.core.client.CoreClient
import app.hocket.ui.LocalCoreClient
import app.hocket.ui.LocalDetailNavigator
import app.hocket.ui.icons.HocketIcons
import app.hocket.ui.screens.library.LibraryPins
import app.hocket.ui.screens.library.PinKind
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.launch
import kotlinx.coroutines.withTimeoutOrNull

/**
 * The app's snackbar, for what outlives the sheet that started it (the add to playlist sheet's
 * "Added to Run club · Undo" shows after the sheet has gone). Provided by the app root; null in a
 * screen shown on its own, where such messages are simply not shown.
 */
@Stable
class AppSnackbar(val host: SnackbarHostState, private val scope: CoroutineScope) {
    fun launch(block: suspend CoroutineScope.() -> Unit) { scope.launch(block = block) }
}

val LocalAppSnackbar = staticCompositionLocalOf<AppSnackbar?> { null }

/** The line under the sheet's title: what is being added (a song, an album, or "3 songs"). */
@Immutable
private data class PickerSubject(val title: String, val detail: String?, val coverArt: String?)

/** Everything the sheet needs, loaded once when it opens. */
@Immutable
private data class PickerData(val trackIds: List<String>, val subject: PickerSubject, val playlists: List<Playlist>, val edits: PickerEdits)

/**
 * Add to playlist: the playlists the songs can go in (pinned, recent, then all; one "Matches" group
 * while searching), each ticked where it already holds them, with a ×2 badge where it holds a song
 * more than once. Ticks and unticks are pending until Done, which applies them all with
 * `PlaylistAdd` / `PlaylistRemove` and says what it did in a snackbar whose Undo takes back every
 * change at once (the core's undo, down to the first entry Done made). Unticking a playlist holding
 * the song twice asks which copies go; press and hold a row (or its accessibility actions) adds
 * another copy or opens the playlist. "New playlist" creates one with the songs (named after the
 * search while there is one).
 */
@Composable
fun PlaylistPicker(target: ActionTarget, onDismiss: () -> Unit) {
    val client = LocalCoreClient.current
    val server by client.server.collectAsStateWithLifecycleCompat()
    val serverId = server?.id ?: return
    val settings by client.settings.collectAsStateWithLifecycleCompat()
    val pinnedIds = remember(settings[SettingKeys.LIBRARY_PINS]?.value) {
        LibraryPins.parse(settings[SettingKeys.LIBRARY_PINS]?.value).filter { it.kind == PinKind.Playlist.id }.map { it.id }
    }
    val resources = LocalResources.current
    val data by produceState<PickerData?>(initialValue = null, target, serverId) { value = loadPicker(client, serverId, target, resources) }
    var edits by remember(data) { mutableStateOf(data?.edits) }
    var query by remember { mutableStateOf("") }
    var asking by remember { mutableStateOf<Playlist?>(null) }
    var naming by remember { mutableStateOf(false) }
    val snackbar = LocalAppSnackbar.current
    val navigator = LocalDetailNavigator.current
    val undoLabel = stringResource(R.string.action_undo)

    fun apply() {
        val e = edits ?: return
        val commands = e.commands()
        if (commands.isEmpty()) return
        val text = e.outcome(data!!.playlists.associate { it.id to it.name })?.let { outcomeText(resources, it) }
        val before = client.undo.value.history.map { it.id }.toSet()
        commands.forEach(client::dispatch)
        if (snackbar != null && text != null) snackbar.launch {
            // Undo takes back everything Done did: every history entry it added, oldest last.
            val after = withTimeoutOrNull(2_000) { client.undo.first { u -> u.history.any { it.id !in before } } }
            val first = after?.history?.lastOrNull { it.id !in before }?.id
            val result = snackbar.host.showSnackbar(text, actionLabel = first?.let { undoLabel }, duration = SnackbarDuration.Short)
            if (result == SnackbarResult.ActionPerformed && first != null) client.dispatch(Commands.undoEntry(first))
        }
    }

    fun create(name: String) {
        val ids = data?.trackIds ?: return
        apply()
        client.dispatch(Commands.createPlaylist(serverId, name.trim(), ids))
        onDismiss()
    }

    ModalBottomSheet(onDismissRequest = onDismiss, sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = true), modifier = Modifier.testTag("picker")) {
        Column(Modifier.fillMaxHeight().imePadding()) {
            Column(Modifier.padding(start = 20.dp, end = 16.dp, bottom = 14.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
                Text(stringResource(R.string.action_add_to_playlist), style = MaterialTheme.typography.headlineSmall, fontWeight = FontWeight.SemiBold, modifier = Modifier.semantics { heading() })
                data?.subject?.let { s ->
                    Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                        Artwork(s.coverArt, ArtworkSizes.THUMB, null, Modifier.size(24.dp), RoundedCornerShape(7.dp))
                        Text(
                            buildAnnotatedString {
                                withStyle(SpanStyle(color = MaterialTheme.colorScheme.onSurface, fontWeight = FontWeight.Medium)) { append(s.title) }
                                s.detail?.let { append(stringResource(R.string.dot_separator)); append(it) }
                            },
                            style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant, maxLines = 1, overflow = TextOverflow.Ellipsis,
                            modifier = Modifier.testTag("picker.subject"),
                        )
                    }
                }
            }
            OutlinedTextField(
                value = query,
                onValueChange = { query = it },
                placeholder = { Text(stringResource(R.string.picker_search)) },
                leadingIcon = { Icon(HocketIcons.Filled.Search, null) },
                trailingIcon = if (query.isNotEmpty()) { { IconButton(onClick = { query = "" }) { Icon(HocketIcons.Filled.Close, stringResource(R.string.search_clear)) } } } else null,
                singleLine = true,
                shape = RoundedCornerShape(26.dp),
                keyboardOptions = KeyboardOptions(imeAction = ImeAction.Search),
                colors = OutlinedTextFieldDefaults.colors(focusedContainerColor = menuRowColor(), unfocusedContainerColor = menuRowColor(), unfocusedBorderColor = MaterialTheme.colorScheme.outlineVariant),
                modifier = Modifier.fillMaxWidth().padding(start = 16.dp, end = 16.dp, bottom = 10.dp).testTag("picker.search"),
            )
            Box(Modifier.weight(1f).fillMaxWidth()) {
                val d = data
                val e = edits
                if (d == null || e == null) {
                    LoadingIndicator(Modifier.align(Alignment.Center))
                } else {
                    val sections = pickerSections(d.playlists, pinnedIds, query)
                    LazyColumn(Modifier.fillMaxWidth().testTag("picker.list"), contentPadding = PaddingValues(start = 16.dp, end = 16.dp, bottom = 16.dp)) {
                        item(key = "new") {
                            NewPlaylistButton(query.trim()) { if (query.isNotBlank()) create(query) else naming = true }
                        }
                        if (query.isNotBlank() && sections.isEmpty()) item(key = "empty") {
                            Surface(shape = RoundedCornerShape(20.dp), color = menuRowColor(), modifier = Modifier.fillMaxWidth().padding(top = 14.dp)) {
                                Text(stringResource(R.string.picker_no_match, query.trim()), style = MaterialTheme.typography.bodyLarge, color = MaterialTheme.colorScheme.onSurfaceVariant,
                                    maxLines = 1, overflow = TextOverflow.Ellipsis, textAlign = androidx.compose.ui.text.style.TextAlign.Center, modifier = Modifier.padding(20.dp).testTag("picker.empty"))
                            }
                        }
                        sections.forEach { section ->
                            item(key = "h.${section.group}") { GroupLabel(section.group) }
                            itemsIndexed(section.playlists, key = { _, p -> "${section.group}.${p.id}" }) { i, p ->
                                PickerRowItem(
                                    playlist = p,
                                    row = e.row(p),
                                    shape = connectedShape(i, section.playlists.size),
                                    onTap = {
                                        when (val r = e.tap(p.id)) {
                                            is PickerTap.Changed -> edits = r.edits
                                            PickerTap.AskWhichCopies -> asking = p
                                        }
                                    },
                                    onAddCopy = { edits = e.addCopy(p.id) },
                                    onOpen = navigator?.openPlaylist?.let { open -> { onDismiss(); open(p.id) } },
                                    modifier = Modifier.padding(bottom = 2.dp),
                                )
                            }
                        }
                    }
                }
            }
            HorizontalDivider(color = MaterialTheme.colorScheme.outlineVariant)
            Row(Modifier.fillMaxWidth().navigationBarsPadding().padding(start = 24.dp, end = 16.dp, top = 14.dp, bottom = 14.dp), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                val e = edits
                val ids = data?.playlists?.map { it.id }.orEmpty()
                val summary = if (e == null) "" else {
                    val (adding, removing) = e.pending(ids)
                    when {
                        adding > 0 || removing > 0 -> listOfNotNull(
                            adding.takeIf { it > 0 }?.let { stringResource(R.string.picker_adding_count, it) },
                            removing.takeIf { it > 0 }?.let { stringResource(R.string.picker_removing_count, it) },
                        ).joinToString(stringResource(R.string.dot_separator))
                        e.inCount(ids) > 0 -> pluralStringResource(R.plurals.picker_in_playlists, e.inCount(ids), e.inCount(ids))
                        else -> stringResource(R.string.picker_not_in_any)
                    }
                }
                Text(summary, style = MaterialTheme.typography.titleMedium, fontWeight = FontWeight.SemiBold, maxLines = 1, overflow = TextOverflow.Ellipsis,
                    modifier = Modifier.weight(1f).semantics { liveRegion = LiveRegionMode.Polite }.testTag("picker.summary"))
                Button(onClick = { apply(); onDismiss() }, enabled = e != null, modifier = Modifier.heightIn(min = 52.dp).widthIn(min = 116.dp).testTag("picker.done")) {
                    Text(stringResource(R.string.picker_done), style = MaterialTheme.typography.titleSmall)
                }
            }
        }
    }
    asking?.let { p ->
        val e = edits
        if (e != null) RemoveCopiesDialog(p.name, e.copies(p.id), onRemoveAll = { edits = e.removeAll(p.id); asking = null }, onRemoveOne = { edits = e.removeOne(p.id); asking = null }, onDismiss = { asking = null })
    }
    if (naming) NewPlaylistDialog(onCreate = { naming = false; create(it) }, onDismiss = { naming = false })
}

/** The snackbar's words for what Done did. */
internal fun outcomeText(resources: android.content.res.Resources, outcome: PickerOutcome): String = when (outcome) {
    is PickerOutcome.AddedTo -> resources.getString(R.string.picker_added_to, outcome.name)
    is PickerOutcome.AddedAgain -> resources.getString(R.string.picker_added_again, outcome.name)
    is PickerOutcome.RemovedFrom -> resources.getString(R.string.picker_removed_from, outcome.name)
    is PickerOutcome.RemovedSome -> if (outcome.count == 1) resources.getString(R.string.picker_removed_one, outcome.name) else resources.getString(R.string.picker_removed_some, outcome.count, outcome.name)
    is PickerOutcome.AddedToMany -> resources.getQuantityString(R.plurals.picker_added_to_many, outcome.playlists, outcome.playlists)
    is PickerOutcome.RemovedFromMany -> resources.getQuantityString(R.plurals.picker_removed_from_many, outcome.playlists, outcome.playlists)
    is PickerOutcome.AddedAndRemoved -> resources.getString(R.string.picker_added_and_removed, outcome.added, outcome.removed)
}

/** The songs, the line describing them, the playlists they can go in and where they already are. */
private suspend fun loadPicker(client: CoreClient, serverId: String, target: ActionTarget, resources: android.content.res.Resources): PickerData {
    val ids = resolveTrackIds(client, target)
    val album = (target as? ActionTarget.Albums)?.data?.ids?.singleOrNull()?.let { (client.query(Queries.album(it)) as? QueryResult.AlbumDetail)?.data }
    val first = ids.firstOrNull()?.let { (client.query(Queries.tracksByIds(listOf(it))) as? QueryResult.TrackList)?.data?.firstOrNull() }
    val subject = when {
        album != null -> PickerSubject(album.name, album.artist, album.coverArt)
        ids.size == 1 && first != null -> PickerSubject(first.title, first.artist, first.coverArt)
        else -> PickerSubject(resources.getQuantityString(R.plurals.menu_songs, ids.size, ids.size), null, first?.coverArt)
    }
    val playlists = addablePlaylists((client.query(Queries.playlists(serverId)) as? QueryResult.Playlists)?.data.orEmpty())
    val membership = (client.query(Queries.playlistMembership(serverId, ids)) as? QueryResult.Membership)?.data.orEmpty()
    return PickerData(ids, subject, playlists, PickerEdits.of(ids, membership))
}

@Composable
private fun NewPlaylistButton(query: String, onClick: () -> Unit) {
    Surface(
        onClick = onClick,
        shape = RoundedCornerShape(20.dp),
        color = MaterialTheme.colorScheme.secondaryContainer,
        contentColor = MaterialTheme.colorScheme.onSecondaryContainer,
        modifier = Modifier.fillMaxWidth().padding(bottom = 4.dp).testTag("picker.new"),
    ) {
        Row(Modifier.height(64.dp).padding(start = 10.dp, end = 16.dp), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(14.dp)) {
            Box(Modifier.size(44.dp).clip(RoundedCornerShape(14.dp)).background(MaterialTheme.colorScheme.primary), contentAlignment = Alignment.Center) {
                Icon(HocketIcons.Filled.Add, null, tint = MaterialTheme.colorScheme.onPrimary)
            }
            Text(if (query.isEmpty()) stringResource(R.string.playlist_new) else stringResource(R.string.picker_new_named, query),
                style = MaterialTheme.typography.titleMedium, fontWeight = FontWeight.SemiBold, maxLines = 1, overflow = TextOverflow.Ellipsis)
        }
    }
}

@Composable
private fun GroupLabel(group: PickerGroup) {
    val label = when (group) {
        PickerGroup.Pinned -> R.string.picker_group_pinned
        PickerGroup.Recent -> R.string.picker_group_recent
        PickerGroup.All -> R.string.picker_group_all
        PickerGroup.Matches -> R.string.picker_group_matches
    }
    Row(Modifier.padding(start = 4.dp, end = 4.dp, top = 14.dp, bottom = 8.dp).semantics(mergeDescendants = true) { heading() }, verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(6.dp)) {
        if (group == PickerGroup.Pinned) Icon(HocketIcons.Filled.PushPin, null, Modifier.size(14.dp), tint = MaterialTheme.colorScheme.onSurfaceVariant)
        Text(stringResource(label), style = MaterialTheme.typography.labelLarge, fontWeight = FontWeight.SemiBold, color = MaterialTheme.colorScheme.onSurfaceVariant)
    }
}

/**
 * One playlist: cover, name (one line) with its ×N badge, "N songs · Adding", and the checkbox. A
 * tap ticks or unticks it; press and hold opens "Add another copy" / "Open playlist", which a screen
 * reader gets as the row's actions (and its long-press).
 */
@Composable
private fun PickerRowItem(playlist: Playlist, row: PickerRow, shape: Shape, onTap: () -> Unit, onAddCopy: () -> Unit, onOpen: (() -> Unit)?, modifier: Modifier = Modifier) {
    var menu by remember { mutableStateOf(false) }
    val haptics = LocalHapticFeedback.current
    val on = row.tick != Tick.Off
    val base = menuRowColor()
    val bg = if (on) MaterialTheme.colorScheme.primary.copy(alpha = 0.10f).compositeOver(base) else base
    val addLabel = stringResource(if (on) R.string.picker_add_copy else R.string.picker_add)
    val openLabel = stringResource(R.string.picker_open)
    val holdLabel = stringResource(R.string.picker_hold_hint)
    val state = when (row.tick) { Tick.On -> ToggleableState.On; Tick.Off -> ToggleableState.Off; Tick.Mixed -> ToggleableState.Indeterminate }
    Box(modifier) {
        Row(
            Modifier
                .fillMaxWidth()
                .height(68.dp)
                .clip(shape)
                .background(bg)
                .combinedClickable(
                    role = Role.Checkbox,
                    onLongClickLabel = holdLabel,
                    onLongClick = { haptics.performHapticFeedback(HapticFeedbackType.LongPress); menu = true },
                    onClick = onTap,
                )
                .semantics {
                    toggleableState = state
                    customActions = listOfNotNull(
                        CustomAccessibilityAction(addLabel) { onAddCopy(); true },
                        onOpen?.let { open -> CustomAccessibilityAction(openLabel) { open(); true } },
                    )
                }
                .padding(start = 10.dp, end = 16.dp)
                .testTag("picker.row.${playlist.id}"),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(14.dp),
        ) {
            Artwork(playlist.coverArt, ArtworkSizes.THUMB, null, Modifier.size(48.dp), RoundedCornerShape(12.dp))
            Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(2.dp)) {
                Row(Modifier.height(22.dp), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                    Text(playlist.name, style = MaterialTheme.typography.bodyLarge, fontWeight = FontWeight.Medium, maxLines = 1, overflow = TextOverflow.Ellipsis, modifier = Modifier.weight(1f, fill = false))
                    row.copies?.let { n ->
                        val copies = pluralStringResource(R.plurals.picker_copies, n, n)
                        Text(
                            stringResource(R.string.picker_badge, n),
                            style = MaterialTheme.typography.labelMedium, fontWeight = FontWeight.Bold, color = MaterialTheme.colorScheme.primary, maxLines = 1,
                            modifier = Modifier
                                .semantics { contentDescription = copies }
                                .border(1.5.dp, MaterialTheme.colorScheme.primary, RoundedCornerShape(6.dp))
                                .padding(horizontal = 6.dp)
                                .testTag("picker.badge.${playlist.id}"),
                        )
                    }
                }
                val noteColor = when (row.note) { PickerNote.Removing -> MaterialTheme.colorScheme.error; else -> MaterialTheme.colorScheme.primary }
                val note = when (row.note) { PickerNote.Adding -> stringResource(R.string.picker_adding); PickerNote.Removing -> stringResource(R.string.picker_removing); PickerNote.None -> null }
                Text(
                    buildAnnotatedString {
                        append(pluralStringResource(R.plurals.menu_songs, row.songCount, row.songCount))
                        note?.let { withStyle(SpanStyle(color = noteColor, fontWeight = FontWeight.Medium)) { append(stringResource(R.string.dot_separator)); append(it) } }
                    },
                    style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant, maxLines = 1, overflow = TextOverflow.Ellipsis,
                )
            }
            TriStateCheckbox(state = state, onClick = null)
        }
        DropdownMenu(expanded = menu, onDismissRequest = { menu = false }, shape = RoundedCornerShape(16.dp), offset = androidx.compose.ui.unit.DpOffset(x = 140.dp, y = (-8).dp),
            modifier = Modifier.testTag("picker.menu")) {
            DropdownMenuItem(text = { Text(addLabel) }, leadingIcon = { Icon(HocketIcons.Filled.Add, null) }, onClick = { menu = false; onAddCopy() }, modifier = Modifier.testTag("picker.menu.addCopy"))
            if (onOpen != null) DropdownMenuItem(text = { Text(openLabel) }, leadingIcon = { Icon(HocketIcons.Filled.QueueMusic, null) }, onClick = { menu = false; onOpen() }, modifier = Modifier.testTag("picker.menu.open"))
        }
    }
}

/**
 * Unticking a playlist that holds the song more than once: "In Sunday slow twice", then Remove both
 * (or all N), Remove one, Cancel. The name gives way (one line, ellipsis) so "twice" always shows.
 */
@Composable
private fun RemoveCopiesDialog(name: String, copies: Int, onRemoveAll: () -> Unit, onRemoveOne: () -> Unit, onDismiss: () -> Unit) {
    val marker = "\u0000"
    val title = if (copies == 2) stringResource(R.string.picker_in_twice, marker) else stringResource(R.string.picker_in_times, marker, copies)
    val before = title.substringBefore(marker)
    val after = title.substringAfter(marker)
    BasicAlertDialog(onDismissRequest = onDismiss, modifier = Modifier.testTag("picker.removeDialog")) {
        Surface(shape = RoundedCornerShape(28.dp), color = MaterialTheme.colorScheme.surfaceContainerHigh) {
            Column(Modifier.padding(start = 24.dp, end = 24.dp, top = 24.dp, bottom = 18.dp), verticalArrangement = Arrangement.spacedBy(16.dp)) {
                Row(Modifier.fillMaxWidth().semantics(mergeDescendants = true) { heading() }, horizontalArrangement = Arrangement.Center) {
                    val style = MaterialTheme.typography.headlineSmall
                    Text(before, style = style, maxLines = 1, softWrap = false)
                    Text(name, style = style, maxLines = 1, overflow = TextOverflow.Ellipsis, modifier = Modifier.weight(1f, fill = false))
                    Text(after, style = style, maxLines = 1, softWrap = false)
                }
                Column(verticalArrangement = Arrangement.spacedBy(2.dp)) {
                    Button(onClick = onRemoveAll, shape = connectedShape(0, 2), modifier = Modifier.fillMaxWidth().heightIn(min = 52.dp).testTag("picker.removeAll")) {
                        Text(if (copies == 2) stringResource(R.string.picker_remove_both) else stringResource(R.string.picker_remove_all, copies), style = MaterialTheme.typography.titleSmall)
                    }
                    FilledTonalButton(onClick = onRemoveOne, shape = connectedShape(1, 2), modifier = Modifier.fillMaxWidth().heightIn(min = 52.dp).testTag("picker.removeOne")) {
                        Text(stringResource(R.string.picker_remove_one), style = MaterialTheme.typography.titleSmall)
                    }
                }
                TextButton(onClick = onDismiss, modifier = Modifier.fillMaxWidth().heightIn(min = 48.dp)) { Text(stringResource(R.string.action_cancel)) }
            }
        }
    }
}

/** "New playlist" with nothing typed in the search: ask for its name. */
@Composable
private fun NewPlaylistDialog(onCreate: (String) -> Unit, onDismiss: () -> Unit) {
    var name by remember { mutableStateOf("") }
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text(stringResource(R.string.playlist_new)) },
        text = {
            OutlinedTextField(value = name, onValueChange = { name = it }, label = { Text(stringResource(R.string.playlist_name)) }, singleLine = true,
                modifier = Modifier.fillMaxWidth().testTag("picker.newName"))
        },
        confirmButton = { TextButton(enabled = name.isNotBlank(), onClick = { onCreate(name) }, modifier = Modifier.testTag("picker.create")) { Text(stringResource(R.string.picker_create)) } },
        dismissButton = { TextButton(onClick = onDismiss) { Text(stringResource(R.string.action_cancel)) } },
    )
}

/** Expands a target to track ids for playlist adds (albums/artists/playlists via their track queries). */
suspend fun resolveTrackIds(client: CoreClient, target: ActionTarget): List<String> = when (target) {
    is ActionTarget.Tracks -> target.data.ids
    is ActionTarget.Albums -> target.data.ids.flatMap { id -> (client.query(Queries.albumTracks(id)) as? QueryResult.TrackList)?.data?.map { it.id } ?: emptyList() }
    is ActionTarget.Artists -> target.data.ids.flatMap { id -> (client.query(Queries.artistTopSongs(id, 500)) as? QueryResult.TrackList)?.data?.map { it.id } ?: emptyList() }
    is ActionTarget.Playlists -> target.data.ids.flatMap { id -> (client.query(Queries.playlistTracks(id, app.hocket.core.api.Page(0u, 5000u))) as? QueryResult.Tracks)?.data?.items?.map { it.id } ?: emptyList() }
    is ActionTarget.QueueItems -> {
        val q = client.queue.value
        val all = q.history + listOfNotNull(q.current) + q.playingNext + q.upcoming
        target.data.keys.mapNotNull { k -> all.firstOrNull { it.item.key == k }?.track?.id }
    }
    else -> emptyList()
}
