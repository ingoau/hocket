package app.hocket.ui.screens.settings

import androidx.compose.foundation.clickable
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.material3.ExperimentalMaterial3ExpressiveApi
import androidx.compose.material3.LargeFlexibleTopAppBar
import androidx.compose.material3.TopAppBarDefaults
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.input.nestedscroll.nestedScroll
import androidx.compose.ui.platform.testTag
import androidx.compose.foundation.selection.toggleable
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.text.InlineTextContent
import androidx.compose.foundation.text.appendInlineContent
import androidx.compose.runtime.remember
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.text.Placeholder
import androidx.compose.ui.text.PlaceholderVerticalAlign
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.buildAnnotatedString
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.navigation.NavHostController
import app.hocket.R
import app.hocket.ui.icons.HocketIcons
import app.hocket.ui.nav.BottomContentInset
import app.hocket.ui.components.contentMaxWidth
import app.hocket.core.Commands
import app.hocket.core.api.SettingScope
import app.hocket.ui.LocalCoreClient
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.size
import androidx.compose.runtime.mutableStateOf
import androidx.compose.ui.unit.em

/** Reads a setting's raw JSON value and offers a setter through the core. */
class SettingHandle(val key: String, val raw: String?, val scope: SettingScope?, private val set: (String) -> Unit) {
    val string: String? get() = raw?.trim()?.takeIf { it != "null" }?.trim('"')
    val bool: Boolean? get() = raw?.trim()?.let { if (it == "true") true else if (it == "false") false else null }
    val int: Int? get() = raw?.trim()?.toIntOrNull()
    val double: Double? get() = raw?.trim()?.toDoubleOrNull()
    fun setString(v: String) = set("\"" + v.replace("\"", "\\\"") + "\"")
    fun setBool(v: Boolean) = set(v.toString())
    fun setInt(v: Int) = set(v.toString())
    fun setDouble(v: Double) = set(v.toString())
    fun setNull() = set("null")
    fun setRaw(v: String) = set(v)
}

@Composable
fun setting(key: String): SettingHandle {
    val client = LocalCoreClient.current
    val settings by client.settings.collectAsStateWithLifecycle()
    val s = settings[key]
    return SettingHandle(key, s?.value, s?.scope) { v -> client.dispatch(Commands.setSetting(key, v)) }
}

@Composable
private fun scopeLabel(scope: SettingScope?): String =
    stringResource(if (scope == SettingScope.AccountSynced) R.string.settings_scope_synced else R.string.settings_scope_local)

/** Row titles: bodyLarge with a tighter line height, so a wrapped title reads as one block. */
@Composable
private fun titleStyle(): TextStyle = MaterialTheme.typography.bodyLarge.copy(lineHeight = 20.sp)

/**
 * Set by a settings screen ([SubScreen]): flipped on when one of its rows is a synced setting, so
 * the page ends with the one line saying what the sync mark means.
 */
internal val LocalSyncedMark = androidx.compose.runtime.staticCompositionLocalOf<androidx.compose.runtime.MutableState<Boolean>?> { null }

/**
 * A setting's title. A synced setting (one that follows the account to the other devices) carries
 * a small sync mark INLINE after the last word, so it wraps with the title instead of taking a
 * column of its own; a device-local one, the common case, carries nothing. The mark is drawn only;
 * its meaning is part of the title's text (", Synced"), so a screen reader reads it once.
 */
@Composable
fun SettingTitle(title: String, scope: SettingScope?, color: Color = Color.Unspecified, modifier: Modifier = Modifier) {
    val style = titleStyle()
    if (scope != SettingScope.AccountSynced) { Text(title, style = style, color = color, modifier = modifier); return }
    val mark = LocalSyncedMark.current
    androidx.compose.runtime.SideEffect { mark?.value = true }
    val label = scopeLabel(scope)
    val text = buildAnnotatedString {
        append(title)
        append(' ')
        appendInlineContent(SYNC_MARK, ", $label")
    }
    val inline = mapOf(
        SYNC_MARK to InlineTextContent(Placeholder(1.1.em, 1.em, PlaceholderVerticalAlign.TextCenter)) {
            Icon(HocketIcons.Filled.Sync, null, tint = MaterialTheme.colorScheme.primary, modifier = Modifier.fillMaxSize().clearAndSetSemantics { }.testTag("setting.syncedMark"))
        },
    )
    Text(text, style = style, color = color, inlineContent = inline, modifier = modifier)
}

private const val SYNC_MARK = "syncMark"

/** The line at the foot of a page with synced settings: the mark and what it means. */
@Composable
private fun SyncedLegend() {
    Row(Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 20.dp).testTag("settings.syncedLegend"), verticalAlignment = Alignment.CenterVertically) {
        Icon(HocketIcons.Filled.Sync, null, tint = MaterialTheme.colorScheme.primary, modifier = Modifier.size(18.dp))
        Spacer(Modifier.width(8.dp))
        Text(stringResource(R.string.settings_synced_legend), style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
    }
}

/** Test tag of a settings row with the stable id [id] (see the settings screens' `tag` arguments). */
fun settingTag(id: String) = "setting.$id"

/**
 * A settings row: title (with its [scope] badge inline), optional subtitle, optional trailing
 * content. [destructive] draws the title in the error colour (sign out).
 */
@Composable
fun SettingRow(title: String, subtitle: String? = null, scope: SettingScope? = null, onClick: (() -> Unit)? = null, tag: String? = null, destructive: Boolean = false, trailing: (@Composable () -> Unit)? = null) {
    Row(Modifier.fillMaxWidth().heightIn(min = 48.dp).settingAnchor(tag).let { if (tag != null) it.testTag(settingTag(tag)) else it }.let { if (onClick != null) it.clickable(onClick = onClick) else it }.padding(horizontal = 16.dp, vertical = 12.dp), verticalAlignment = Alignment.CenterVertically) {
        Column(Modifier.weight(1f)) {
            SettingTitle(title, scope, color = if (destructive) MaterialTheme.colorScheme.error else Color.Unspecified)
            subtitle?.let { Text(it, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant) }
        }
        if (trailing != null) { Spacer(Modifier.width(12.dp)); trailing() }
    }
}

/**
 * A setting with a switch: the whole row is ONE toggle (role switch) that reads "title, subtitle,
 * switch, on"; the switch itself is drawn only (no second focus stop, no second click target).
 */
@Composable
fun SwitchRow(title: String, checked: Boolean, onChange: (Boolean) -> Unit, subtitle: String? = null, scope: SettingScope? = null, tag: String? = null) {
    Row(
        Modifier.fillMaxWidth().settingAnchor(tag).let { if (tag != null) it.testTag(settingTag(tag)) else it }
            .toggleable(value = checked, role = Role.Switch, onValueChange = onChange)
            .padding(horizontal = 16.dp, vertical = 12.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Column(Modifier.weight(1f)) {
            SettingTitle(title, scope)
            subtitle?.let { Text(it, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant) }
        }
        Spacer(Modifier.width(12.dp))
        Switch(checked = checked, onCheckedChange = null)
    }
}

/** A heading over a run of rows; with a [tag] it is the target settings search jumps to for the run (a field or an editor, not a row). */
@Composable
fun SettingsSection(title: String, tag: String? = null) {
    Text(title, style = MaterialTheme.typography.labelLarge, color = MaterialTheme.colorScheme.primary, modifier = Modifier.fillMaxWidth().settingAnchor(tag).let { if (tag != null) it.testTag(settingTag(tag)) else it }.padding(start = 16.dp, top = 20.dp, bottom = 4.dp).semantics { heading() })
}

/** True for a category's screen in the pane beside the settings list: there is nothing to go back to. */
val LocalSettingsPane = androidx.compose.runtime.staticCompositionLocalOf { false }

/** A settings sub-screen: large flexible top app bar that collapses as the page scrolls, back arrow, scrolling rows. */
@OptIn(ExperimentalMaterial3Api::class, ExperimentalMaterial3ExpressiveApi::class)
@Composable
fun SubScreen(nav: NavHostController, title: String, content: @Composable androidx.compose.foundation.layout.ColumnScope.() -> Unit) {
    val scroll = TopAppBarDefaults.exitUntilCollapsedScrollBehavior()
    // The row a settings search picked, if it opened this screen: scrolled to, focused and highlighted.
    val jump = remember { SettingsJump.take()?.let(::JumpTarget) }
    val scrollState = rememberScrollState()
    JumpEffect(jump, scrollState)
    Scaffold(
        modifier = Modifier.nestedScroll(scroll.nestedScrollConnection).testTag("settings.screen"),
        topBar = {
            app.hocket.ui.components.ContentWidthTopBar { insets -> LargeFlexibleTopAppBar(windowInsets = insets,
                title = { Text(title, modifier = Modifier.semantics { heading() }.testTag("settings.title")) },
                navigationIcon = { if (!LocalSettingsPane.current) IconButton(onClick = { nav.popBackStack() }, modifier = Modifier.testTag("settings.back")) { Icon(HocketIcons.AutoMirrored.Filled.ArrowBack, stringResource(R.string.action_back)) } },
                scrollBehavior = scroll,
            ) }
        },
    ) { padding ->
        // Top only: the shell already keeps content clear of the navigation bar.
        val synced = remember { mutableStateOf(false) }
        androidx.compose.runtime.CompositionLocalProvider(LocalJumpTarget provides jump, LocalSyncedMark provides synced) {
            // The scroll spans the screen; the rows stop at ContentMaxWidth, centred.
            Column(Modifier.padding(top = padding.calculateTopPadding()).verticalScroll(scrollState).padding(bottom = BottomContentInset)) {
                Column(Modifier.contentMaxWidth()) {
                    content()
                    if (synced.value) SyncedLegend()
                }
            }
        }
    }
}
