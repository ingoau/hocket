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
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
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
fun ScopeBadge(scope: SettingScope?) {
    when (scope) {
        SettingScope.AccountSynced -> Badge(stringResource(R.string.settings_scope_synced), container = MaterialTheme.colorScheme.tertiaryContainer)
        SettingScope.DeviceLocal, null -> Badge(stringResource(R.string.settings_scope_local))
    }
}

/** Test tag of a settings row with the stable id [id] (see the settings screens' `tag` arguments). */
fun settingTag(id: String) = "setting.$id"

@Composable
fun SettingRow(title: String, subtitle: String? = null, scope: SettingScope? = null, onClick: (() -> Unit)? = null, tag: String? = null, trailing: (@Composable () -> Unit)? = null) {
    Row(Modifier.fillMaxWidth().let { if (tag != null) it.testTag(settingTag(tag)) else it }.let { if (onClick != null) it.clickable(onClick = onClick) else it }.padding(horizontal = 16.dp, vertical = 12.dp), verticalAlignment = Alignment.CenterVertically) {
        Column(Modifier.weight(1f)) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Text(title, style = MaterialTheme.typography.bodyLarge)
                if (scope != null) { Spacer(Modifier.width(8.dp)); ScopeBadge(scope) }
            }
            subtitle?.let { Text(it, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant) }
        }
        trailing?.invoke()
    }
}

@Composable
fun SwitchRow(title: String, checked: Boolean, onChange: (Boolean) -> Unit, subtitle: String? = null, scope: SettingScope? = null, tag: String? = null) {
    SettingRow(title, subtitle, scope, onClick = { onChange(!checked) }, tag = tag) { Switch(checked = checked, onCheckedChange = onChange) }
}

@Composable
fun SettingsSection(title: String) {
    Text(title, style = MaterialTheme.typography.labelLarge, color = MaterialTheme.colorScheme.primary, modifier = Modifier.padding(start = 16.dp, top = 20.dp, bottom = 4.dp))
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
                title = { Text(title) },
                navigationIcon = { IconButton(onClick = { nav.popBackStack() }, modifier = Modifier.testTag("settings.back")) { Icon(Icons.AutoMirrored.Filled.ArrowBack, stringResource(R.string.action_back)) } },
                scrollBehavior = scroll,
            )
        },
    ) { padding ->
        Column(Modifier.padding(padding).verticalScroll(rememberScrollState()).padding(bottom = BottomContentInset)) { content() }
    }
}
