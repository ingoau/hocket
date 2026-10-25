package app.hocket.ui.screens.settings

import androidx.compose.foundation.clickable
import androidx.compose.ui.Alignment
import androidx.compose.material3.AssistChip
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.LargeFlexibleTopAppBar
import androidx.compose.material3.ListItem
import androidx.compose.material3.ListItemDefaults
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TopAppBarDefaults
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.input.nestedscroll.nestedScroll
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.navigation.NavHostController
import app.hocket.R
import app.hocket.core.SettingKeys
import app.hocket.core.api.ConnectionTier
import app.hocket.core.api.ReplayGainMode
import app.hocket.ui.LocalCoreClient
import app.hocket.ui.components.formatAgo
import app.hocket.ui.components.formatBytes
import app.hocket.ui.icons.HocketIcons
import app.hocket.ui.nav.BottomContentInset
import app.hocket.ui.nav.Route

/** The settings categories, in the order they are listed. [id] is the stable test id. */
enum class SettingsCategory(val id: String, val route: Route) {
    Account("account", Route.SettingsAccount),
    Appearance("appearance", Route.SettingsAppearance),
    Playback("playback", Route.SettingsPlayback),
    Audio("audio", Route.AudioSettings),
    Streaming("streaming", Route.TranscodingSettings),
    Downloads("downloads", Route.SettingsDownloads),
    Lyrics("lyrics", Route.SettingsLyrics),
    Library("library", Route.SettingsLibrary),
    Battery("battery", Route.SettingsBattery),
    Connect("connect", Route.ConnectSettings),
    Pebble("pebble", Route.SettingsPebble),
    Customise("customise", Route.CustomiseSettings),
    Backup("backup", Route.SettingsBackup),
    Advanced("advanced", Route.SettingsAdvanced),
    About("about", Route.About);

    companion object {
        /** How the list is grouped under its headings (Android system-settings style: related categories share a card). Account is the card on top. */
        val GROUPS: List<Pair<Int, List<SettingsCategory>>> = listOf(
            R.string.settings_group_look_and_sound to listOf(Appearance, Playback, Audio, Streaming),
            R.string.settings_group_library_and_storage to listOf(Downloads, Lyrics, Library, Battery),
            R.string.settings_group_devices_and_controls to listOf(Connect, Pebble, Customise),
            R.string.settings_group_app to listOf(Backup, Advanced, About),
        )
    }
}

/**
 * Top-level settings: the account card, then one row per category (leading icon, title, chevron),
 * grouped in cards under headings (no summaries: they cluttered the list); each opens its own sub-screen. Every setting lives in exactly
 * one category.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun SettingsScreen(nav: NavHostController) {
    val scroll = TopAppBarDefaults.exitUntilCollapsedScrollBehavior()
    Scaffold(
        modifier = Modifier.nestedScroll(scroll.nestedScrollConnection).testTag("settings.categories"),
        topBar = { LargeFlexibleTopAppBar(title = { Text(stringResource(R.string.settings_title), modifier = Modifier.semantics { heading() }) }, scrollBehavior = scroll) },
    ) { padding ->
        Column(
            Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(top = padding.calculateTopPadding(), bottom = BottomContentInset).padding(horizontal = 16.dp),
            verticalArrangement = Arrangement.spacedBy(16.dp),
        ) {
            AccountCard(onOpen = { nav.navigate(SettingsCategory.Account.route) }, onStats = { nav.navigate(Route.Stats) }, onDevices = { nav.navigate(Route.ConnectSettings) })
            SettingsCategory.GROUPS.forEach { (heading, group) ->
                Column(verticalArrangement = Arrangement.spacedBy(2.dp)) {
                    Text(stringResource(heading), style = MaterialTheme.typography.titleSmall, color = MaterialTheme.colorScheme.primary, modifier = Modifier.padding(start = 16.dp, top = 4.dp, bottom = 6.dp).semantics { heading() })
                    group.forEachIndexed { i, category ->
                        CategoryRow(category, first = i == 0, last = i == group.lastIndex, onClick = { nav.navigate(category.route) })
                    }
                }
            }
        }
    }
}

/** Who is signed in, to which server, and when the library last synced; with shortcuts to listening stats and devices. */
@Composable
private fun AccountCard(onOpen: () -> Unit, onStats: () -> Unit, onDevices: () -> Unit) {
    val client = LocalCoreClient.current
    val server by client.server.collectAsStateWithLifecycle()
    val s = server
    Surface(shape = RoundedCornerShape(28.dp), color = MaterialTheme.colorScheme.secondaryContainer, modifier = Modifier.fillMaxWidth()) {
        Column(Modifier.clickable(onClick = onOpen).testTag("settings.category.account").padding(20.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Surface(shape = CircleShape, color = MaterialTheme.colorScheme.primary, modifier = Modifier.size(48.dp)) {
                    Box(contentAlignment = Alignment.Center) { Text((s?.username?.firstOrNull() ?: '?').uppercase(), style = MaterialTheme.typography.titleLarge, color = MaterialTheme.colorScheme.onPrimary) }
                }
                Spacer(Modifier.width(16.dp))
                Column(Modifier.weight(1f)) {
                    Text(s?.username ?: stringResource(R.string.settings_category_account), style = MaterialTheme.typography.titleMedium, maxLines = 1, overflow = TextOverflow.Ellipsis)
                    if (s != null) {
                        val host = s.url.removePrefix("https://").removePrefix("http://").trimEnd('/')
                        Text(if (s.name.isNotBlank() && s.name != host) stringResource(R.string.settings_account_on, s.name, host) else host, style = MaterialTheme.typography.bodyMedium, maxLines = 1, overflow = TextOverflow.Ellipsis)
                        Text(s.lastSync?.let { stringResource(R.string.settings_last_sync, formatAgo(it)) } ?: stringResource(R.string.settings_never_synced), style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSecondaryContainer)
                    } else Text(stringResource(R.string.settings_summary_no_server), style = MaterialTheme.typography.bodyMedium)
                }
                Icon(HocketIcons.Filled.ChevronRight, null)
            }
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                AssistChip(onClick = onStats, label = { Text(stringResource(R.string.settings_account_stats)) }, leadingIcon = { Icon(HocketIcons.Filled.BarChart, null) }, modifier = Modifier.testTag("settings.account.stats"))
                AssistChip(onClick = onDevices, label = { Text(stringResource(R.string.settings_account_devices)) }, leadingIcon = { Icon(HocketIcons.Filled.Devices, null) }, modifier = Modifier.testTag("settings.account.devices"))
            }
        }
    }
}

@Composable
private fun CategoryRow(category: SettingsCategory, first: Boolean, last: Boolean, onClick: () -> Unit) {
    val outer = 24.dp
    val inner = 6.dp
    val shape = RoundedCornerShape(topStart = if (first) outer else inner, topEnd = if (first) outer else inner, bottomStart = if (last) outer else inner, bottomEnd = if (last) outer else inner)
    Surface(shape = shape, color = MaterialTheme.colorScheme.surfaceContainer, modifier = Modifier.fillMaxWidth()) {
        ListItem(
            headlineContent = { Text(category.title()) },
            leadingContent = { Icon(category.icon(), null, tint = MaterialTheme.colorScheme.primary) },
            trailingContent = { Icon(HocketIcons.Filled.ChevronRight, null) },
            colors = ListItemDefaults.colors(containerColor = MaterialTheme.colorScheme.surfaceContainer),
            modifier = Modifier.clickable(onClick = onClick).testTag("settings.category.${category.id}"),
        )
    }
}

private fun SettingsCategory.icon(): ImageVector = when (this) {
    SettingsCategory.Account -> HocketIcons.Filled.AccountCircle
    SettingsCategory.Appearance -> HocketIcons.Filled.Palette
    SettingsCategory.Playback -> HocketIcons.AutoMirrored.Filled.QueueMusic
    SettingsCategory.Audio -> HocketIcons.Filled.GraphicEq
    SettingsCategory.Streaming -> HocketIcons.Filled.NetworkCheck
    SettingsCategory.Downloads -> HocketIcons.Filled.Download
    SettingsCategory.Lyrics -> HocketIcons.Filled.Lyrics
    SettingsCategory.Library -> HocketIcons.Filled.LibraryMusic
    SettingsCategory.Battery -> HocketIcons.Filled.BatterySaver
    SettingsCategory.Connect -> HocketIcons.Filled.Devices
    SettingsCategory.Pebble -> HocketIcons.Filled.Watch
    SettingsCategory.Customise -> HocketIcons.Filled.Tune
    SettingsCategory.Backup -> HocketIcons.Filled.SettingsBackupRestore
    SettingsCategory.Advanced -> HocketIcons.Filled.Code
    SettingsCategory.About -> HocketIcons.Filled.Info
}

@Composable
fun SettingsCategory.title(): String = stringResource(
    when (this) {
        SettingsCategory.Account -> R.string.settings_category_account
        SettingsCategory.Appearance -> R.string.settings_section_appearance
        SettingsCategory.Playback -> R.string.settings_category_playback
        SettingsCategory.Audio -> R.string.settings_section_audio
        SettingsCategory.Streaming -> R.string.settings_category_streaming
        SettingsCategory.Downloads -> R.string.settings_category_downloads
        SettingsCategory.Lyrics -> R.string.settings_section_lyrics
        SettingsCategory.Library -> R.string.settings_category_library
        SettingsCategory.Battery -> R.string.settings_section_battery
        SettingsCategory.Connect -> R.string.settings_category_connect
        SettingsCategory.Pebble -> R.string.settings_category_pebble
        SettingsCategory.Customise -> R.string.settings_section_customise
        SettingsCategory.Backup -> R.string.settings_category_backup
        SettingsCategory.Advanced -> R.string.settings_category_advanced
        SettingsCategory.About -> R.string.settings_section_about
    },
)

@Composable
internal fun str(id: Int): String = stringResource(id)
