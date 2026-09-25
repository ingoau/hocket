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
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
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
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.text.Placeholder
import androidx.compose.ui.text.PlaceholderVerticalAlign
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.rememberTextMeasurer
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.navigation.NavHostController
import app.hocket.R
import app.hocket.ui.nav.BottomContentInset
import app.hocket.core.Commands
import app.hocket.core.api.SettingScope
import app.hocket.ui.LocalCoreClient
import app.hocket.ui.components.Badge

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
fun ScopeBadge(scope: SettingScope?, modifier: Modifier = Modifier) {
    when (scope) {
        SettingScope.AccountSynced -> Badge(scopeLabel(scope), modifier, container = MaterialTheme.colorScheme.tertiaryContainer)
        SettingScope.DeviceLocal, null -> Badge(scopeLabel(scope), modifier)
    }
}

@Composable
private fun scopeLabel(scope: SettingScope?): String =
    stringResource(if (scope == SettingScope.AccountSynced) R.string.settings_scope_synced else R.string.settings_scope_local)

/** Row titles: bodyLarge with a tighter line height, so a wrapped title reads as one block. */
@Composable
private fun titleStyle(): TextStyle = MaterialTheme.typography.bodyLarge.copy(lineHeight = 20.sp)

/**
 * A setting's title with its scope badge INLINE after the last word: when the title wraps, the
 * badge follows it on the last line instead of sitting in a column of its own between the text
 * and the trailing switch. The badge is drawn only; its text is part of the title's text (", This
 * device"), so a screen reader reads it once, after the title.
 */
@Composable
fun SettingTitle(title: String, scope: SettingScope?, color: Color = Color.Unspecified, modifier: Modifier = Modifier) {
    val style = titleStyle()
    if (scope == null) { Text(title, style = style, color = color, modifier = modifier); return }
    val badgeText = scopeLabel(scope)
    val badgeStyle = MaterialTheme.typography.labelSmall
    val measurer = rememberTextMeasurer()
    val density = LocalDensity.current
    val size = remember(badgeText, badgeStyle, density) { measurer.measure(badgeText, badgeStyle).size }
    // The space and a small gap before the badge + the badge's own padding (6 dp each side, 2 dp top and bottom).
    val (width, height) = with(density) { (BadgeGap + size.width.toDp() + 12.dp).toSp() to (size.height.toDp() + 4.dp).toSp() }
    // A breaking space before the badge: when it does not fit it starts the next line flush left.
    val text = buildAnnotatedString {
        append(title)
        append(' ')
        appendInlineContent(SCOPE_BADGE, ", $badgeText")
    }
    val inline = mapOf(
        SCOPE_BADGE to InlineTextContent(Placeholder(width, height, PlaceholderVerticalAlign.TextCenter)) {
            Row(Modifier.clearAndSetSemantics { }, verticalAlignment = Alignment.CenterVertically) { Spacer(Modifier.width(BadgeGap)); ScopeBadge(scope) }
        },
    )
    Text(text, style = style, color = color, inlineContent = inline, modifier = modifier)
}

private const val SCOPE_BADGE = "scopeBadge"
private val BadgeGap = 2.dp

/** Test tag of a settings row with the stable id [id] (see the settings screens' `tag` arguments). */
fun settingTag(id: String) = "setting.$id"

/**
 * A settings row: title (with its [scope] badge inline), optional subtitle, optional trailing
 * content. [destructive] draws the title in the error colour (sign out).
 */
@Composable
fun SettingRow(title: String, subtitle: String? = null, scope: SettingScope? = null, onClick: (() -> Unit)? = null, tag: String? = null, destructive: Boolean = false, trailing: (@Composable () -> Unit)? = null) {
    Row(Modifier.fillMaxWidth().heightIn(min = 48.dp).let { if (tag != null) it.testTag(settingTag(tag)) else it }.let { if (onClick != null) it.clickable(onClick = onClick) else it }.padding(horizontal = 16.dp, vertical = 12.dp), verticalAlignment = Alignment.CenterVertically) {
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
        Modifier.fillMaxWidth().let { if (tag != null) it.testTag(settingTag(tag)) else it }
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

@Composable
fun SettingsSection(title: String) {
    Text(title, style = MaterialTheme.typography.labelLarge, color = MaterialTheme.colorScheme.primary, modifier = Modifier.padding(start = 16.dp, top = 20.dp, bottom = 4.dp).semantics { heading() })
}

/** A settings sub-screen: large flexible top app bar that collapses as the page scrolls, back arrow, scrolling rows. */
@OptIn(ExperimentalMaterial3Api::class, ExperimentalMaterial3ExpressiveApi::class)
@Composable
fun SubScreen(nav: NavHostController, title: String, content: @Composable androidx.compose.foundation.layout.ColumnScope.() -> Unit) {
    val scroll = TopAppBarDefaults.exitUntilCollapsedScrollBehavior()
    Scaffold(
        modifier = Modifier.nestedScroll(scroll.nestedScrollConnection).testTag("settings.screen"),
        topBar = {
            LargeFlexibleTopAppBar(
                title = { Text(title, modifier = Modifier.semantics { heading() }.testTag("settings.title")) },
                navigationIcon = { IconButton(onClick = { nav.popBackStack() }, modifier = Modifier.testTag("settings.back")) { Icon(Icons.AutoMirrored.Filled.ArrowBack, stringResource(R.string.action_back)) } },
                scrollBehavior = scroll,
            )
        },
    ) { padding ->
        // Top only: the shell already keeps content clear of the navigation bar.
        Column(Modifier.padding(top = padding.calculateTopPadding()).verticalScroll(rememberScrollState()).padding(bottom = BottomContentInset)) { content() }
    }
}
