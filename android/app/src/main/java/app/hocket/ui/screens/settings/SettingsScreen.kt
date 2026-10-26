package app.hocket.ui.screens.settings

import androidx.compose.foundation.clickable
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.IntrinsicSize
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.MaterialShapes
import androidx.compose.material3.SegmentedListItem
import androidx.compose.material3.toShape
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Shape
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import app.hocket.core.Commands
import app.hocket.ui.brand.brandTitleStyle
import app.hocket.ui.components.GroupLabel
import app.hocket.ui.components.segmentColor
import app.hocket.ui.components.segmentedListColors
import androidx.compose.ui.Alignment
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
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.navigation.NavHostController
import app.hocket.R
import app.hocket.ui.LocalCoreClient
import app.hocket.ui.components.formatAgo
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
        topBar = {
            LargeFlexibleTopAppBar(
                title = { Text(stringResource(R.string.settings_title), style = brandTitleStyle(), modifier = Modifier.semantics { heading() }) },
                navigationIcon = { if (nav.previousBackStackEntry != null) androidx.compose.material3.IconButton(onClick = { nav.popBackStack() }) { Icon(HocketIcons.AutoMirrored.Filled.ArrowBack, stringResource(R.string.action_back)) } },
                scrollBehavior = scroll,
            )
        },
    ) { padding ->
        Column(
            Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(top = padding.calculateTopPadding(), bottom = BottomContentInset).padding(horizontal = 16.dp),
            verticalArrangement = Arrangement.spacedBy(16.dp),
        ) {
            AccountCard(onOpen = { nav.navigate(SettingsCategory.Account.route) }, onStats = { nav.navigate(Route.Stats) }, onDevices = { nav.navigate(Route.ConnectSettings) })
            SettingsCategory.GROUPS.forEach { (titleRes, group) ->
                Column(verticalArrangement = Arrangement.spacedBy(ListItemDefaults.SegmentedGap)) {
                    GroupLabel(stringResource(titleRes), color = MaterialTheme.colorScheme.primary)
                    group.forEachIndexed { i, category ->
                        CategoryRow(category, i, group.size, onClick = { nav.navigate(category.route) })
                    }
                }
            }
        }
    }
}

/**
 * Who is signed in and to which server, centred on a tonal card: the user's initial on a cookie,
 * their name in the word mark's face, the server, then the last sync as a button that syncs now,
 * and the shortcuts to listening stats and devices as a pair of tiles.
 */
@Composable
private fun AccountCard(onOpen: () -> Unit, onStats: () -> Unit, onDevices: () -> Unit) {
    val client = LocalCoreClient.current
    val server by client.server.collectAsStateWithLifecycle()
    val sync by client.syncProgress.collectAsStateWithLifecycle()
    val s = server
    val scheme = MaterialTheme.colorScheme
    Surface(shape = RoundedCornerShape(32.dp), color = scheme.secondaryContainer, contentColor = scheme.onSecondaryContainer, modifier = Modifier.fillMaxWidth()) {
        Column(Modifier.padding(16.dp), horizontalAlignment = Alignment.CenterHorizontally) {
            Column(
                Modifier.fillMaxWidth().clip(RoundedCornerShape(24.dp)).clickable(onClick = onOpen).testTag("settings.category.account").padding(vertical = 8.dp)
                    .semantics(mergeDescendants = true) { },
                horizontalAlignment = Alignment.CenterHorizontally,
            ) {
                Box(
                    Modifier.size(88.dp).background(Brush.linearGradient(listOf(scheme.primary.copy(alpha = 0.78f), scheme.primary)), MaterialShapes.Cookie9Sided.toShape()),
                    contentAlignment = Alignment.Center,
                ) { Text((s?.username?.firstOrNull() ?: '?').uppercase(), style = MaterialTheme.typography.displaySmall, fontWeight = FontWeight.SemiBold, color = scheme.onPrimary, modifier = Modifier.clearAndSetSemantics { }) }
                Spacer(Modifier.height(12.dp))
                // The category's name for a screen reader: the card opens Account & server.
                val accountLabel = str(R.string.settings_category_account)
                Text(s?.username ?: stringResource(R.string.settings_category_account), style = MaterialTheme.typography.headlineMedium.merge(brandTitleStyle()), maxLines = 1, overflow = TextOverflow.Ellipsis,
                    modifier = Modifier.semantics { contentDescription = "${s?.username ?: ""}, $accountLabel" })
                if (s != null) {
                    val host = s.url.removePrefix("https://").removePrefix("http://").trimEnd('/')
                    Text(if (s.name.isNotBlank() && s.name != host) stringResource(R.string.settings_account_on, s.name, host) else host, style = MaterialTheme.typography.bodyLarge, maxLines = 1, overflow = TextOverflow.Ellipsis)
                } else Text(stringResource(R.string.settings_summary_no_server), style = MaterialTheme.typography.bodyLarge)
            }
            if (s != null) {
                Spacer(Modifier.height(8.dp))
                val syncing = sync != null
                Button(onClick = { client.dispatch(Commands.syncLibrary(s.id, false)) }, enabled = !syncing, shapes = ButtonDefaults.shapes(), modifier = Modifier.heightIn(min = ButtonDefaults.MediumContainerHeight).testTag("settings.account.sync"),
                    contentPadding = ButtonDefaults.contentPaddingFor(ButtonDefaults.MediumContainerHeight)) {
                    Icon(HocketIcons.Filled.Sync, null, Modifier.size(ButtonDefaults.MediumIconSize))
                    Spacer(Modifier.width(ButtonDefaults.MediumIconSpacing))
                    val last = s.lastSync?.let { stringResource(R.string.settings_last_sync, formatAgo(it)) } ?: stringResource(R.string.settings_never_synced)
                    Text(if (syncing) stringResource(R.string.settings_syncing) else stringResource(R.string.settings_last_sync_now, last), style = ButtonDefaults.textStyleFor(ButtonDefaults.MediumContainerHeight), maxLines = 1, overflow = TextOverflow.Ellipsis)
                }
            }
            Spacer(Modifier.height(16.dp))
            Row(Modifier.fillMaxWidth().height(IntrinsicSize.Min), horizontalArrangement = Arrangement.spacedBy(ListItemDefaults.SegmentedGap)) {
                AccountTile(stringResource(R.string.settings_account_stats), HocketIcons.Outlined.BarChart, RoundedCornerShape(topStart = 24.dp, bottomStart = 24.dp, topEnd = 6.dp, bottomEnd = 6.dp), onStats, Modifier.weight(1f).fillMaxHeight().testTag("settings.account.stats"))
                AccountTile(stringResource(R.string.settings_account_devices), HocketIcons.Outlined.Devices, RoundedCornerShape(topEnd = 24.dp, bottomEnd = 24.dp, topStart = 6.dp, bottomStart = 6.dp), onDevices, Modifier.weight(1f).fillMaxHeight().testTag("settings.account.devices"))
            }
        }
    }
}

/** A shortcut on the account card: its icon over its label, on the card's lifted surface. */
@Composable
private fun AccountTile(label: String, icon: ImageVector, shape: Shape, onClick: () -> Unit, modifier: Modifier) {
    Surface(onClick = onClick, shape = shape, color = segmentColor(), contentColor = MaterialTheme.colorScheme.onSurface, modifier = modifier) {
        Column(Modifier.padding(horizontal = 12.dp, vertical = 16.dp), horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.spacedBy(6.dp)) {
            Icon(icon, null, tint = MaterialTheme.colorScheme.primary)
            Text(label, style = MaterialTheme.typography.titleSmall, fontWeight = FontWeight.SemiBold, textAlign = TextAlign.Center)
        }
    }
}

@Composable
private fun CategoryRow(category: SettingsCategory, index: Int, count: Int, onClick: () -> Unit) {
    SegmentedListItem(
        onClick = onClick,
        shapes = ListItemDefaults.segmentedShapes(index, count),
        colors = segmentedListColors(),
        leadingContent = {
            Box(Modifier.size(40.dp).background(MaterialTheme.colorScheme.secondaryContainer, RoundedCornerShape(12.dp)), contentAlignment = Alignment.Center) {
                Icon(category.icon(), null, tint = MaterialTheme.colorScheme.onSecondaryContainer)
            }
        },
        trailingContent = { Icon(HocketIcons.Filled.ChevronRight, null) },
        modifier = Modifier.testTag("settings.category.${category.id}"),
    ) { Text(category.title()) }
}

private fun SettingsCategory.icon(): ImageVector = when (this) {
    SettingsCategory.Account -> HocketIcons.Outlined.Person
    SettingsCategory.Appearance -> HocketIcons.Outlined.Palette
    SettingsCategory.Playback -> HocketIcons.AutoMirrored.Filled.QueueMusic
    SettingsCategory.Audio -> HocketIcons.Filled.GraphicEq
    SettingsCategory.Streaming -> HocketIcons.Filled.NetworkCheck
    SettingsCategory.Downloads -> HocketIcons.Outlined.Download
    SettingsCategory.Lyrics -> HocketIcons.Outlined.Lyrics
    SettingsCategory.Library -> HocketIcons.Outlined.LibraryMusic
    SettingsCategory.Battery -> HocketIcons.Outlined.BatterySaver
    SettingsCategory.Connect -> HocketIcons.Outlined.Devices
    SettingsCategory.Pebble -> HocketIcons.Filled.Watch
    SettingsCategory.Customise -> HocketIcons.Outlined.Tune
    SettingsCategory.Backup -> HocketIcons.Filled.SettingsBackupRestore
    SettingsCategory.Advanced -> HocketIcons.Filled.Code
    SettingsCategory.About -> HocketIcons.Outlined.Info
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
